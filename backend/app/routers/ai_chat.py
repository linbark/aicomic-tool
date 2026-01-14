import asyncio
import json
import time
from typing import Any, Dict, List, Optional
import uuid

from fastapi import APIRouter, BackgroundTasks, Depends, HTTPException
from pydantic import BaseModel
from sqlalchemy.orm import Session

from .. import models
from ..database import SessionLocal, get_db
from ..services.json_extract import extract_json_any
from ..services.llm_client import LlmChatSettings
from ..services.project_lookup import resolve_project_pk
from .ai_shared import _build_memory_context, _chat_client, _context_store, _mask_settings, _read_settings_raw
from .ai_helpers import safe_parse_json, log_ui
from .ai_workflows import (
    WorkflowScriptOptions,
    WorkflowScriptRequest,
    WorkflowStoryboardOptions,
    WorkflowStoryboardRequest,
    workflow_script,
    workflow_storyboard,
)
from .ai_writing import (
    OutlineGenerateRequest,
    OutlineOptimizeRequest,
    ScriptGenerateRequest,
    ScriptOptimizeRequest,
    generate_script,
    outline_generate,
    outline_optimize,
    script_optimize,
)

import logging
import os

log_dir = os.path.join(os.path.dirname(__file__), "..", "logs")
os.makedirs(log_dir, exist_ok=True)
log_file = os.path.join(log_dir, "ai_chat.log")

logger = logging.getLogger(__name__)
logger.setLevel(logging.DEBUG)
handler = logging.FileHandler(log_file)
handler.setLevel(logging.DEBUG)
formatter = logging.Formatter('%(asctime)s - %(name)s - %(levelname)s - %(message)s')
handler.setFormatter(formatter)
logger.addHandler(handler)

router = APIRouter(tags=["AI (DeepSeek)"])


# ==========================
# Chat Orchestrator（意图识别 + 多步编排）
# ==========================


class ChatActRequest(BaseModel):
    project_id: str
    episode_id: Optional[int] = None
    # 当前 UI Tab（用于偏置意图识别）
    current_action_key: Optional[str] = None  # outline_generate/outline_optimize/generate_script/script_optimize/...
    message: str
    ui_context: Optional[Dict[str, Any]] = None
    debug: bool = False
    run_id: str


class ChatPlanStep(BaseModel):
    action_key: str
    # 如果留空，后端会根据上下文自动填充
    input_text: Optional[str] = None
    why: Optional[str] = None


class ChatPlan(BaseModel):
    intent_summary: str
    steps: List[ChatPlanStep]
    final_action_key: str
    # 可选：意图不明时让 planner 直接要求澄清
    needs_clarification: Optional[bool] = None
    clarifying_question: Optional[str] = None
    clarifying_options: Optional[List[str]] = None


class ChatStepTrace(BaseModel):
    step_index: int
    action_key: str
    input_text: str
    output_text_preview: str
    ms: int


class ChatMemoryTraceItem(BaseModel):
    key: str
    # 仅 debug 模式回传
    text: Optional[str] = None


class ChatActResponse(BaseModel):
    run_id: Optional[str] = None
    assistant_message: str
    created_run: Optional[Dict[str, Any]] = None
    # 非 debug：也可返回轻量卡片（用于前端 inline 审阅/确认）
    cards: Optional[List[Dict[str, Any]]] = None
    # debug only
    plan: Optional[Dict[str, Any]] = None
    steps_trace: Optional[List[Dict[str, Any]]] = None
    planner_prompt: Optional[str] = None
    memory_trace: Optional[List[Dict[str, Any]]] = None
    planner_raw: Optional[str] = None


def _now_ms() -> int:
    return int(time.time() * 1000)


def _trim_preview(text: str, limit: int = 300) -> str:
    t = (text or "").strip()
    return t[:limit] + ("..." if len(t) > limit else "")

def _log_chat_event(project_id: int, run_id: str, stage: str, summary: str, level: str = "INFO", data: Optional[Dict[str, Any]] = None):
    payload: Dict[str, Any] = {
        "stage": str(stage),
        "summary": str(summary),
        "run_id": str(run_id),
        "project_id": int(project_id),
        "data": data or {},
        "at_ms": _now_ms(),
    }
    log_ui(payload, level)


class ChatActAsyncResponse(BaseModel):
    run_id: str
    status: str = "queued"  # queued|running|done|error


async def _chat_act_core(
    *,
    req: "ChatActRequest",
    db: Session,
    emit_stages: bool = False,
    run_id: Optional[str] = None,
) -> "ChatActResponse":
    """
    chat_act 核心逻辑：
    - emit_stages=True 时，把 plan/step start/end/final 写入 runs-files stages，供前端轮询展示“执行步骤”
    """
    project_id_pk = resolve_project_pk(db, req.project_id)
    run_id = (str(run_id or "") or str(req.run_id or "")).strip()
    if not run_id:
        raise HTTPException(status_code=400, detail="run_id 不能为空")
    _log_chat_event(
        project_id_pk,
        run_id,
        "chat.core.enter",
        "进入 chat 核心流程",
        "INFO",
        data={"episode_id": req.episode_id, "current_action_key": req.current_action_key},
    )
    if emit_stages and run_id:
        _context_store.snapshot_stage(
            project_id=project_id_pk, run_id=run_id, stage_name="chat.status", data={"status": "running", "at_ms": _now_ms()}
        )

    raw = _read_settings_raw()
    settings = _mask_settings(raw)
    if not settings.has_api_key:
        logger.error(f"[AI][chat_act] AI API Key 未配置")
        _log_chat_event(project_id_pk, run_id, "chat.settings.api_key", "AI API Key 未配置", "ERROR")
        raise HTTPException(status_code=400, detail="AI API Key 未配置")

    message = (req.message or "").strip()
    if not message:
        logger.error(f"[AI][chat_act] message 不能为空")
        _log_chat_event(project_id_pk, str(run_id or ""), "chat.validate.message", "message 不能为空", "ERROR")
        raise HTTPException(status_code=400, detail="message 不能为空")
    _log_chat_event(project_id_pk, str(run_id or ""), "chat.validate.ok", "输入校验通过", "INFO", data={"message_len": len(message)})

    # -------- Intent clarification (rule-first) --------
    def _looks_ambiguous(msg: str) -> bool:
        m = (msg or "").strip()
        if len(m) <= 6:
            return True
        verbs = ["生成", "提取", "优化", "改", "润色", "分镜", "拆", "入库", "保存", "整理", "总结", "加快", "减少"]
        if not any(v in m for v in verbs):
            return True
        return False

    if _looks_ambiguous(message):
        logger.error(f"[AI][chat_act] message 模糊不清: {message}")
        _log_chat_event(project_id_pk, str(run_id or ""), "chat.intent.ambiguous", "意图不清，需要澄清", "WARN", data={"message_preview": _trim_preview(message, 120)})
        resp = ChatActResponse(
            run_id=run_id,
            assistant_message="我需要你明确一下目标：你希望我对本集做什么？（可多选）",
            cards=[
                {
                    "type": "clarify_intent",
                    "title": "请选择你的意图",
                    "options": [
                        {"label": "提取大纲/节拍表", "value": "outline"},
                        {"label": "生成/续写剧本", "value": "script"},
                        {"label": "优化节奏（减少对白/加快推进）", "value": "pace"},
                        {"label": "拆分分镜（把某段场景拆成镜头）", "value": "storyboard"},
                        {"label": "把设定/角色变化入库并提交我确认", "value": "memory_review"},
                    ],
                    "hint": "你也可以直接说：例如“先提取大纲，再按节奏要求优化，然后生成剧本，最后提交入库变更给我确认”。",
                }
            ],
        )
        if emit_stages and run_id:
            _context_store.snapshot_stage(project_id=project_id_pk, run_id=run_id, stage_name="chat.final", data=resp.model_dump())
            _context_store.snapshot_stage(project_id=project_id_pk, run_id=run_id, stage_name="chat.status", data={"status": "done", "at_ms": _now_ms()})
            _context_store.snapshot_run(
                project_id=project_id_pk, run_id=run_id, request=req.model_dump(), response=resp.model_dump(), meta={"workflow": "chat_act"}
            )
        logger.info(f"[AI][chat_act] Response: {resp.model_dump()}")
        _log_chat_event(project_id_pk, str(run_id or ""), "chat.response.clarify", "返回澄清请求", "INFO")
        return resp
    logger.info(f"[AI][chat_act] Message: {message}")
    _log_chat_event(
        project_id_pk,
        str(run_id or ""),
        "chat.message",
        "收到用户消息",
        "INFO",
        data={"message_len": len(message or ""), "message_preview": _trim_preview(message, 200)},
    )
    # -------- Memory (for planner / debug) --------
    memory_trace: List[Dict[str, Any]] = []
    memory_context_text = ""
    if req.project_id:
        try:
            from ..services.memory_indexer import MemoryIndexer
            from ..services.memory_retriever import get_memory_retriever

            indexer = MemoryIndexer()
            indexer.index_series_bible(project_id=project_id_pk, version="v1")
            retriever = get_memory_retriever()
            retrieval_results = retriever.retrieve_for_task(
                project_id=project_id_pk,
                task_description=f"用户意图: {message[:200]}",
            )
            memory_context_text = _build_memory_context(retrieval_results)
            if req.debug:
                for k in ["L2_static", "L1", "L2_dynamic", "negative_constraints"]:
                    try:
                        memory_trace.append({"key": k, "text": (retriever.format_for_prompt(retrieval_results) or {}).get(k)})
                    except Exception as e:
                        memory_trace.append({"key": k, "text": None})
        except Exception as e:
            logger.error(f"[AI][chat_act] Memory retrieval failed: {e}")
            _log_chat_event(
                project_id_pk,
                str(run_id or ""),
                "chat.memory.retrieval.error",
                "记忆检索失败",
                "ERROR",
                data={"error": f"{type(e).__name__}: {e}"},
            )
    # -------- Planner Prompt --------
    ui_ctx = req.ui_context or {}
    logger.info(f"[AI][chat_act] UI Context: {ui_ctx}")
    _log_chat_event(
        project_id_pk,
        str(run_id or ""),
        "chat.ui_context",
        "读取 UI 上下文",
        "INFO",
        data={
            "keys": sorted([str(k) for k in (ui_ctx or {}).keys()]),
            "master_script_len": len(str((ui_ctx or {}).get("master_script") or "")),
            "current_input_len": len(str((ui_ctx or {}).get("current_input") or "")),
        },
    )
    current_action_key = (req.current_action_key or "").strip()
    master_script_preview = str(ui_ctx.get("master_script") or "")
    selected_input_preview = str(ui_ctx.get("current_input") or "")

    planner_system = f"""你是一个“写作编排器（planner）”，负责把用户的自然语言意图拆解为可执行的动作序列。

你必须输出严格的 JSON（不要输出其它文字）。

当用户意图不明确/信息不足时：你应该输出“澄清请求”，不要输出 steps：
{{
  "needs_clarification": true,
  "clarifying_question": "一句话追问（中文）",
  "clarifying_options": ["选项1","选项2","选项3"]
}}

当你能规划执行时：输出“执行计划”，结构如下：
{{
  "intent_summary": "一句话总结用户想要什么（中文）",
  "steps": [
    {{
      "action_key": "outline_generate|outline_optimize|generate_script|script_optimize|workflow_script|workflow_storyboard|memory_extract_changeset",
      "input_text": "可选；如留空，由系统根据上下文填充",
      "why": "可选；这一步的目的"
    }}
  ],
  "final_action_key": "同上，表示最终要写入版本记录的 action"
}}

可用动作说明：
- outline_generate：从输入材料生成本集大纲（JSON）
- outline_optimize：在已有大纲基础上按意图优化大纲（JSON）
- generate_script：从大纲/材料生成剧本（文本）
- script_optimize：在已有剧本基础上按意图优化剧本（文本）
- workflow_script：运行“workflow script”（series_bible + beat_sheet + script_fountain + qc_report）
- workflow_storyboard：运行“workflow storyboard”（把场景文本拆成镜头列表）
- memory_extract_changeset：从当前产物/文本中抽取 changeset.v0 并创建 ChangeSet（后端返回审阅卡片）

硬约束：
1) 仅允许使用上述 action_key
2) steps 最多 4 步，尽量少步完成
3) 如果用户意图涉及多阶段（例如：先生成大纲再生成剧本），请拆成多步
4) final_action_key 必须等于 steps 最后一步的 action_key
5) 如果上下文不足（例如没有可优化的文本），第一步应选择生成类动作
"""

    planner_user = f"""用户输入：
{message}

当前 UI tab（偏置参考，可为空）：
{current_action_key or "(none)"}

当前 Master Script（预览，可能为空）：
{master_script_preview or "(empty)"}

当前工作台输入（预览，可能为空）：
{selected_input_preview or "(empty)"}
"""
    logger.info(f"[AI][chat_act] Planner User: {planner_user}")
    _log_chat_event(project_id_pk, str(run_id or ""), "chat.planner.prompt", "生成 planner 提示词", "INFO", data={"prompt_len": len(planner_user)})
    if memory_context_text:
        planner_user += f"\n\n记忆上下文（必须遵守，可能为空）：\n{memory_context_text}"

    _log_chat_event(project_id_pk, str(run_id or ""), "chat.planner.call", "调用 planner 模型", "INFO")
    planner_raw = await _chat_client.chat(
        settings=LlmChatSettings(
            base_url=settings.base_url,
            api_key=raw.get("api_key") or "",
            model=settings.model,
            temperature=min(float(settings.temperature or 0.2), 0.3),
            max_tokens=settings.max_tokens,  # None 表示不限制
            timeout_seconds=settings.timeout_seconds,
        ),
        messages=[
            {"role": "system", "content": planner_system},
            {"role": "user", "content": planner_user},
        ],
    )
    try:
        plan_parsed = safe_parse_json(planner_raw or "")
    except Exception as e:
        logger.error(f"Planner JSON parse failed: {e}\nRaw: {planner_raw}")
        _log_chat_event(project_id_pk, str(run_id or ""), "chat.planner.parse_error", "planner 输出解析失败", "ERROR", data={"error": str(e)})
        # 构造一个错误提示返回给用户，而不是让后台崩溃
        raise HTTPException(status_code=502, detail=f"AI 响应格式错误: {str(e)}")
    # [修改结束]
    logger.info(f"[AI][chat_act] Planner Parsed: {plan_parsed}")
    _log_chat_event(project_id_pk, str(run_id or ""), "chat.planner.parsed", "planner 计划解析完成", "INFO", data={"has_steps": bool((plan_parsed or {}).get("steps"))})
    if not isinstance(plan_parsed, dict):
        logger.error(f"[AI][chat_act] Planner 输出无法解析为 JSON: {(planner_raw or '')[:500]}")
        raise HTTPException(status_code=502, detail=f"Planner 输出无法解析为 JSON: {(planner_raw or '')[:500]}")
    logger.info(f"[AI][chat_act] Planner Needs Clarification: {bool(plan_parsed.get('needs_clarification'))}")
    if bool(plan_parsed.get("needs_clarification")):
        _log_chat_event(project_id_pk, str(run_id or ""), "chat.planner.needs_clarification", "planner 要求澄清", "WARN")
    if bool(plan_parsed.get("needs_clarification")):
        logger.info(f"[AI][chat_act] Planner Needs Clarification: True")
        q = str(plan_parsed.get("clarifying_question") or "").strip() or "我需要你补充一下目标/范围：你希望我具体做什么？"
        opts = plan_parsed.get("clarifying_options")
        options = []
        if isinstance(opts, list):
            options = [{"label": str(x), "value": str(x)} for x in opts[:8] if str(x).strip()]
        resp = ChatActResponse(
            run_id=run_id,
            assistant_message=q,
            cards=[
                {
                    "type": "clarify_intent",
                    "title": "需要澄清",
                    "options": options or None,
                    "hint": "你也可以直接用一句话说明：例如“先提取大纲，再优化节奏，然后生成剧本，最后提交入库变更给我确认”。",
                }
            ],
            plan=plan_parsed if req.debug else None,
            planner_prompt=(planner_system + "\n\n---\n\n" + planner_user) if req.debug else None,
            memory_trace=memory_trace if req.debug else None,
            planner_raw=(planner_raw or "") if req.debug else None,
        )
        if emit_stages and run_id:
            _context_store.snapshot_stage(project_id=project_id_pk, run_id=run_id, stage_name="chat.plan", data={"plan": plan_parsed})
            _context_store.snapshot_stage(project_id=project_id_pk, run_id=run_id, stage_name="chat.final", data=resp.model_dump())
            _context_store.snapshot_stage(project_id=project_id_pk, run_id=run_id, stage_name="chat.status", data={"status": "done", "at_ms": _now_ms()})
            _context_store.snapshot_run(
                project_id=project_id_pk, run_id=run_id, request=req.model_dump(), response=resp.model_dump(), meta={"workflow": "chat_act"}
            )
        logger.info(f"[AI][chat_act] Response: {resp.model_dump()}")
        return resp

    logger.info(f"[AI][chat_act] Planner Steps: {plan_parsed.get('steps')}")
    _log_chat_event(
        project_id_pk,
        str(run_id or ""),
        "chat.planner.steps",
        "planner 返回 steps",
        "INFO",
        data={
            "step_count": int(len(plan_parsed.get("steps") or [])) if isinstance(plan_parsed.get("steps"), list) else 0,
            "action_keys": [str(x.get("action_key")) for x in (plan_parsed.get("steps") or []) if isinstance(x, dict)][:8],
        },
    )
    steps = plan_parsed.get("steps") or []
    if not isinstance(steps, list) or len(steps) == 0:
        logger.error(f"[AI][chat_act] Planner steps 为空: {plan_parsed}")
        _log_chat_event(
            project_id_pk,
            str(run_id or ""),
            "chat.planner.steps.empty",
            "planner steps 为空",
            "ERROR",
            data={"plan_preview": _trim_preview(json.dumps(plan_parsed, ensure_ascii=False), 400)},
        )
        raise HTTPException(status_code=502, detail=f"Planner steps 为空: {plan_parsed}")
    if len(steps) > 4:
        steps = steps[:4]
    allowed = {
        "outline_generate",
        "outline_optimize",
        "generate_script",
        "script_optimize",
        "workflow_script",
        "workflow_storyboard",
        "memory_extract_changeset",
    }
    for s in steps:
        if not isinstance(s, dict) or s.get("action_key") not in allowed:
            logger.error(f"[AI][chat_act] Planner steps 包含非法 action_key: {s}")
            raise HTTPException(status_code=502, detail=f"Planner steps 包含非法 action_key: {s}")
    
    final_action_key = steps[-1].get("action_key")
    plan_parsed["final_action_key"] = final_action_key
    logger.info(f"[AI][chat_act] Final Action Key: {final_action_key}")
    _log_chat_event(
        project_id_pk,
        str(run_id or ""),
        "chat.planner.final_action_key",
        "确定最终动作",
        "INFO",
        data={"final_action_key": str(final_action_key or "")},
    )
    if emit_stages and run_id:
        logger.info(f"[AI][chat_act] Snapshot Stage: chat.plan")
        _log_chat_event(
            project_id_pk,
            str(run_id or ""),
            "chat.snapshot.plan",
            "写入 plan stage",
            "INFO",
        )
        _context_store.snapshot_stage(
            project_id=project_id_pk,
            run_id=run_id,
            stage_name="chat.plan",
            data={"plan": {"intent_summary": plan_parsed.get("intent_summary"), "steps": steps, "final_action_key": final_action_key}},
        )

    artifacts: Dict[str, Any] = {}
    steps_trace: List[Dict[str, Any]] = []
    cards: List[Dict[str, Any]] = []

    def _step_timeout_seconds(action_key: str) -> float:
        return 600.0

    for idx, step in enumerate(steps):
        logger.info(f"[AI][chat_act] Step {idx} Start: {step}")
        _log_chat_event(
            project_id_pk,
            str(run_id or ""),
            "chat.step.loop.start",
            "进入 step",
            "INFO",
            data={"step_index": idx, "action_key": str((step or {}).get("action_key") or ""), "why": str((step or {}).get("why") or "")},
        )
        ak = str(step.get("action_key"))
        in_text = (step.get("input_text") or "").strip()
        why = (step.get("why") or "").strip()

        # 对于优化类操作，优先使用前一步的 artifacts 输出（如果存在）
        # 因为优化操作需要的是前一步的输出，而不是用户意图
        logger.info(f"[AI][chat_act] Step {idx} Before input processing: ak={ak}, input_text={_trim_preview(in_text, 100)}, artifacts.keys()={list(artifacts.keys())}")
        _log_chat_event(
            project_id_pk,
            str(run_id or ""),
            "chat.step.input.before",
            "准备 step 输入",
            "INFO",
            data={"step_index": idx, "action_key": ak, "input_len": len(in_text or ""), "artifacts_keys": sorted(list(artifacts.keys()))},
        )
        
        if ak == "outline_optimize" and isinstance(artifacts.get("outline"), str) and artifacts.get("outline").strip():
            logger.info(f"[AI][chat_act] Step {idx} Using artifacts['outline'] (length={len(artifacts['outline'])})")
            _log_chat_event(
                project_id_pk,
                str(run_id or ""),
                "chat.step.input.select",
                "选择输入：使用 outline",
                "INFO",
                data={"step_index": idx, "action_key": ak, "source": "artifacts.outline", "text_len": len(artifacts.get("outline") or "")},
            )
            in_text = artifacts["outline"]
        elif ak == "script_optimize" and isinstance(artifacts.get("script"), str) and artifacts.get("script").strip():
            logger.info(f"[AI][chat_act] Step {idx} Using artifacts['script'] (length={len(artifacts['script'])})")
            _log_chat_event(
                project_id_pk,
                str(run_id or ""),
                "chat.step.input.select",
                "选择输入：使用 script",
                "INFO",
                data={"step_index": idx, "action_key": ak, "source": "artifacts.script", "text_len": len(artifacts.get("script") or "")},
            )
            in_text = artifacts["script"]
        elif ak == "generate_script" and isinstance(artifacts.get("outline"), str) and artifacts.get("outline").strip():
            logger.info(f"[AI][chat_act] Step {idx} Using artifacts['outline'] for generate_script (length={len(artifacts['outline'])})")
            _log_chat_event(
                project_id_pk,
                str(run_id or ""),
                "chat.step.input.select",
                "选择输入：用 outline 生成 script",
                "INFO",
                data={"step_index": idx, "action_key": ak, "source": "artifacts.outline", "text_len": len(artifacts.get("outline") or "")},
            )
            in_text = artifacts["outline"]
        elif not in_text:
            # 如果 input_text 为空且没有可用的 artifacts，使用 UI 上下文或用户消息
            logger.info(f"[AI][chat_act] Step {idx} No artifacts available, using UI context or message")
            _log_chat_event(
                project_id_pk,
                str(run_id or ""),
                "chat.step.input.select",
                "选择输入：回退到 UI/消息",
                "INFO",
                data={"step_index": idx, "action_key": ak, "source": "ui_or_message"},
            )
            in_text = selected_input_preview or master_script_preview or message

        # 对于优化类操作，将用户意图追加到输入文本后面
        if ak in ("outline_optimize", "script_optimize"):
            in_text = f"{in_text}\n\n[用户意图]\n{message}"
        
        logger.info(f"[AI][chat_act] Step {idx} After input processing: in_text length={len(in_text)}, preview={_trim_preview(in_text, 200)}")
        _log_chat_event(
            project_id_pk,
            str(run_id or ""),
            "chat.step.input.ready",
            "step 输入已就绪",
            "INFO",
            data={"step_index": idx, "action_key": ak, "in_text_len": len(in_text or ""), "in_text_preview": _trim_preview(in_text, 200)},
        )

        if emit_stages and run_id:
            _context_store.snapshot_stage(
                project_id=project_id_pk,
                run_id=run_id,
                stage_name=f"chat.step.{idx}.start",
                data={"step_index": idx, "action_key": ak, "why": why or None, "input_preview": _trim_preview(in_text, 400), "at_ms": _now_ms()},
            )
            # 额外写入当前 step 指针（用于前端兜底显示进度）
            _context_store.snapshot_stage(
                project_id=project_id_pk,
                run_id=run_id,
                stage_name="chat.status",
                data={"status": "running", "at_ms": _now_ms(), "current_step_index": idx, "current_action_key": ak},
            )

        _log_chat_event(
            project_id_pk,
            str(run_id or ""),
            "chat.step.start",
            "开始执行 step",
            "INFO",
            data={"step_index": idx, "action_key": ak},
        )
        t0 = _now_ms()
        out_text = ""
        
        logger.info(f"[AI][chat_act] Step {idx} About to call _do_step() for action_key={ak}")
        _log_chat_event(
            project_id_pk,
            str(run_id or ""),
            "chat.step.execute.prepare",
            "准备执行 step",
            "INFO",
            data={"step_index": idx, "action_key": ak},
        )

        async def _do_step() -> str:
            nonlocal out_text
            logger.info(f"[AI][chat_act] _do_step() called for action_key={ak}")
            _log_chat_event(
                project_id_pk,
                str(run_id or ""),
                "chat.step.execute.enter",
                "进入 step 执行",
                "INFO",
                data={"step_index": idx, "action_key": ak},
            )
            
            if ak == "outline_generate":
                logger.info(f"[AI][chat_act] Calling outline_generate with text length={len(in_text)}")
                _log_chat_event(
                    project_id_pk,
                    str(run_id or ""),
                    "chat.call.outline_generate",
                    "调用 outline_generate",
                    "INFO",
                    data={"step_index": idx, "input_len": len(in_text or "")},
                )
                resp = await outline_generate(OutlineGenerateRequest(text=in_text, project_id=project_id_pk, run_id=run_id))
                out_text = (resp.text or "").strip()
                artifacts["outline"] = out_text
                _log_chat_event(
                    project_id_pk,
                    str(run_id or ""),
                    "chat.result.outline_generate",
                    "outline_generate 返回",
                    "INFO",
                    data={"step_index": idx, "outline_len": len(out_text or ""), "outline_preview": _trim_preview(out_text, 400)},
                )
                logger.info(f"[AI][chat_act] Outline Generate Response: {out_text}")
                logger.info(f"[AI][chat_act] Outline Generate Response artifacts: {artifacts}")
                return out_text
            if ak == "outline_optimize":
                logger.info(f"[AI][chat_act] Calling outline_optimize with text length={len(in_text)}, project_id={project_id_pk}")
                _log_chat_event(
                    project_id_pk,
                    str(run_id or ""),
                    "chat.call.outline_optimize",
                    "调用 outline_optimize",
                    "INFO",
                    data={"step_index": idx, "input_len": len(in_text or ""), "project_id": int(project_id_pk)},
                )
                try:
                    resp = await outline_optimize(OutlineOptimizeRequest(text=in_text, project_id=project_id_pk, run_id=run_id))
                    logger.info(f"[AI][chat_act] outline_optimize returned, response length={len(resp.text or '')}")
                    _log_chat_event(
                        project_id_pk,
                        str(run_id or ""),
                        "chat.result.outline_optimize",
                        "outline_optimize 返回",
                        "INFO",
                        data={"step_index": idx, "response_len": len(resp.text or "")},
                    )
                    out_text = (resp.text or "").strip()
                    artifacts["outline"] = out_text
                    _log_chat_event(
                        project_id_pk,
                        str(run_id or ""),
                        "chat.result.outline_optimize",
                        "outline_optimize 输出已写入 artifacts",
                        "INFO",
                        data={"step_index": idx, "outline_len": len(out_text or ""), "outline_preview": _trim_preview(out_text, 400)},
                    )
                    logger.info(f"[AI][chat_act] Outline Optimize Response: {out_text}")
                    logger.info(f"[AI][chat_act] Outline Optimize Response artifacts: {artifacts}")
                    return out_text
                except Exception as e:
                    logger.error(f"[AI][chat_act] outline_optimize raised exception: {type(e).__name__}: {e}")
                    _log_chat_event(
                        project_id_pk,
                        str(run_id or ""),
                        "chat.call.outline_optimize.error",
                        "outline_optimize 执行异常",
                        "ERROR",
                        data={"step_index": idx, "error": f"{type(e).__name__}: {e}"},
                    )
                    raise
            if ak == "generate_script":
                resp = await generate_script(ScriptGenerateRequest(text=in_text, project_id=project_id_pk, run_id=run_id))
                out_text = (resp.text or "").strip()
                artifacts["script"] = out_text
                _log_chat_event(
                    project_id_pk,
                    str(run_id or ""),
                    "chat.result.generate_script",
                    "generate_script 返回",
                    "INFO",
                    data={"step_index": idx, "script_len": len(out_text or ""), "script_preview": _trim_preview(out_text, 400)},
                )
                logger.info(f"[AI][chat_act] Generate Script Response: {out_text}")
                logger.info(f"[AI][chat_act] Generate Script Response artifacts: {artifacts}")
                return out_text
            if ak == "script_optimize":
                resp = await script_optimize(ScriptOptimizeRequest(text=in_text, project_id=project_id_pk, run_id=run_id))
                out_text = (resp.text or "").strip()
                artifacts["script"] = out_text
                _log_chat_event(
                    project_id_pk,
                    str(run_id or ""),
                    "chat.result.script_optimize",
                    "script_optimize 返回",
                    "INFO",
                    data={"step_index": idx, "script_len": len(out_text or ""), "script_preview": _trim_preview(out_text, 400)},
                )
                logger.info(f"[AI][chat_act] Script Optimize Response: {out_text}")
                logger.info(f"[AI][chat_act] Script Optimize Response artifacts: {artifacts}")
                return out_text
            if ak == "workflow_script":
                wf_resp = await workflow_script(
                    WorkflowScriptRequest(
                        project_id=req.project_id,
                        input_text=in_text,
                        options=WorkflowScriptOptions(),
                        run_id=run_id,
                    ),
                    db=db,
                )
                artifacts["series_bible"] = wf_resp.series_bible
                artifacts["beat_sheet"] = wf_resp.beat_sheet
                artifacts["script_fountain"] = wf_resp.script_fountain
                artifacts["qc_report"] = wf_resp.qc_report
                out_text = (wf_resp.script_fountain or "").strip()
                return out_text
            if ak == "workflow_storyboard":
                wf_resp = await workflow_storyboard(
                    WorkflowStoryboardRequest(
                        project_id=req.project_id,
                        scene_text=in_text,
                        options=WorkflowStoryboardOptions(),
                        run_id=run_id,
                    ),
                    db=db,
                )
                artifacts["shots"] = wf_resp.shots
                out_text = json.dumps(wf_resp.shots, ensure_ascii=False, indent=2)
                return out_text
            if ak == "memory_extract_changeset":
                from ..services.changeset_extractor import extract_changeset_v0_with_llm_with_trace
                from ..services.entity_resolver import resolve_changeset_entities_with_trace
                from ..services.evidence_ingestor import chunk_text_to_evidences
                from ..services.memory_store import get_memory_store
                logger.info(f"[AI][chat_act] Memory Extract Changeset Artifacts: {artifacts}")
                _log_chat_event(
                    project_id_pk,
                    str(run_id or ""),
                    "chat.memory_extract.artifacts",
                    "准备抽取变更：读取 artifacts",
                    "INFO",
                    data={"step_index": idx, "artifacts_keys": sorted(list(artifacts.keys()))},
                )
                
                # 记录 get_memory_store() 调用前后的时间，用于诊断性能问题
                import time
                t_start = time.time()
                logger.info(f"[AI][chat_act] About to call get_memory_store() at {t_start}")
                _log_chat_event(
                    project_id_pk,
                    str(run_id or ""),
                    "chat.memory_extract.store.start",
                    "初始化 MemoryStore",
                    "INFO",
                    data={"step_index": idx},
                )
                store = get_memory_store()
                t_end = time.time()
                elapsed = t_end - t_start
                logger.info(f"[AI][chat_act] Memory Extract Changeset Store: {store}, elapsed={elapsed:.2f}s")
                _log_chat_event(
                    project_id_pk,
                    str(run_id or ""),
                    "chat.memory_extract.store.done",
                    "MemoryStore 初始化完成",
                    "INFO",
                    data={"step_index": idx, "elapsed_s": float(f"{elapsed:.4f}")},
                )
                base_text = ""
                
                if artifacts.get("script_fountain"):
                    base_text += f"### script_fountain\n{artifacts.get('script_fountain')}\n\n"
                if artifacts.get("beat_sheet"):
                    try:
                        base_text += "### beat_sheet\n" + json.dumps(artifacts.get("beat_sheet"), ensure_ascii=False, indent=2) + "\n\n"
                    except Exception as e:
                        _log_chat_event(
                            project_id_pk,
                            str(run_id or ""),
                            "chat.memory_extract.base_text.beat_sheet.error",
                            "拼接 beat_sheet 失败",
                            "ERROR",
                            data={"step_index": idx, "error": f"{type(e).__name__}: {e}"},
                        )
                if artifacts.get("series_bible"):
                    try:
                        base_text += "### series_bible\n" + json.dumps(artifacts.get("series_bible"), ensure_ascii=False, indent=2) + "\n\n"
                    except Exception as e:
                        _log_chat_event(
                            project_id_pk,
                            str(run_id or ""),
                            "chat.memory_extract.base_text.series_bible.error",
                            "拼接 series_bible 失败",
                            "ERROR",
                            data={"step_index": idx, "error": f"{type(e).__name__}: {e}"},
                        )
                if not base_text:
                    base_text = master_script_preview or message
                logger.info(f"[AI][chat_act] Memory Extract Changeset Base Text: {base_text}")
                _log_chat_event(
                    project_id_pk,
                    str(run_id or ""),
                    "chat.memory_extract.base_text",
                    "构造抽取 base_text",
                    "INFO",
                    data={"step_index": idx, "base_text_len": len(base_text or ""), "base_text_preview": _trim_preview(base_text, 300)},
                )
                evidences = chunk_text_to_evidences(
                    project_id=project_id_pk,
                    run_id=run_id,
                    episode_id=int(req.episode_id) if req.episode_id else None,
                    text=base_text,
                    max_quote_chars=600,
                    tags=["chat_act"],
                )
                evidence_ids: List[str] = []
                for ev in evidences:
                    try:
                        evidence_ids.append(store.upsert_evidence(ev))
                    except Exception as e:
                        _log_chat_event(
                            project_id_pk,
                            str(run_id or ""),
                            "chat.memory_extract.evidence.upsert.error",
                            "写入 evidence 失败",
                            "ERROR",
                            data={"step_index": idx, "error": f"{type(e).__name__}: {e}"},
                        )
                        continue
                logger.info(f"[AI][chat_act] Memory Extract Changeset Evidence IDs: {evidence_ids}")
                _log_chat_event(
                    project_id_pk,
                    str(run_id or ""),
                    "chat.memory_extract.evidence.done",
                    "evidence 切片完成",
                    "INFO",
                    data={"step_index": idx, "evidence_count": int(len(evidence_ids))},
                )
                payload, extractor_trace = await extract_changeset_v0_with_llm_with_trace(
                    llm_settings=LlmChatSettings(
                        base_url=settings.base_url,
                        api_key=raw.get("api_key") or "",
                        model=settings.model,
                        temperature=min(float(settings.temperature or 0.2), 0.3),
                        max_tokens=settings.max_tokens,  # None 表示不限制
                        timeout_seconds=settings.timeout_seconds,
                    ),
                    project_id=project_id_pk,
                    episode_id=int(req.episode_id) if req.episode_id else None,
                    story_order_base=f"CH{str(req.episode_id or 1).zfill(2)}",
                    evidences=[e.model_dump() for e in evidences],
                )
                payload, resolver_trace = resolve_changeset_entities_with_trace(store=store, project_id=project_id_pk, payload=payload)
                changeset_id = store.create_changeset(
                    project_id=project_id_pk, payload=payload, episode_id=int(req.episode_id) if req.episode_id else None
                )
                try:
                    store.append_changeset_review_entry(changeset_id=changeset_id, entry={"at_ms": _now_ms(), "action": "extractor_trace", "data": extractor_trace})
                except Exception as e:
                    _log_chat_event(
                        project_id_pk,
                        str(run_id or ""),
                        "chat.memory_extract.review_entry.extractor_trace.error",
                        "写入 extractor_trace 失败",
                        "ERROR",
                        data={"step_index": idx, "error": f"{type(e).__name__}: {e}"},
                    )
                    pass
                try:
                    store.append_changeset_review_entry(changeset_id=changeset_id, entry={"at_ms": _now_ms(), "action": "resolver_trace", "data": resolver_trace})
                except Exception as e:
                    _log_chat_event(
                        project_id_pk,
                        str(run_id or ""),
                        "chat.memory_extract.review_entry.resolver_trace.error",
                        "写入 resolver_trace 失败",
                        "ERROR",
                        data={"step_index": idx, "error": f"{type(e).__name__}: {e}"},
                    )
                    pass

                def _count_by_type(items: Any) -> Dict[str, int]:
                    out: Dict[str, int] = {}
                    if not isinstance(items, list):
                        return out
                    for it in items:
                        if not isinstance(it, dict):
                            continue
                        t = str(it.get("entity_type") or it.get("type") or "unknown")
                        out[t] = out.get(t, 0) + 1
                    return out

                ent_counts = _count_by_type(payload.get("entities") or [])
                conflicts_n = len(payload.get("conflicts") or []) if isinstance(payload.get("conflicts"), list) else 0
                summary_lines = [
                    f"- 实体：{sum(ent_counts.values())}（" + ", ".join([f"{k}:{v}" for k, v in ent_counts.items()]) + "）" if ent_counts else "- 实体：0",
                    f"- 角色切片 snapshots：{len(payload.get('snapshots') or [])}",
                    f"- 事件 events：{len(payload.get('events') or [])}",
                    f"- 状态变更 state_changes：{len(payload.get('state_changes') or [])}",
                    f"- 冲突 conflicts：{conflicts_n}",
                ]
                cards.append(
                    {
                        "type": "review_changeset",
                        "changeset_id": changeset_id,
                        "title": "需要确认：更新设定/事件（ChangeSet）",
                        "summary": "\n".join(summary_lines),
                        "actions": [
                            {"action": "approve_changeset", "label": "确认提交", "changeset_id": changeset_id},
                            {"action": "reject_changeset", "label": "驳回", "changeset_id": changeset_id},
                        ],
                    }
                )
                out_text = f"已生成待审阅变更单：{changeset_id}（evidence={len(evidence_ids)}）"
                return out_text
            raise HTTPException(status_code=400, detail=f"Unsupported action_key: {ak}")

        try:
            # 执行步骤（带超时控制）
            timeout_sec = _step_timeout_seconds(ak)
            logger.info(f"[AI][chat_act] Step {idx} Starting execution with timeout={timeout_sec}s")
            _log_chat_event(
                project_id_pk,
                str(run_id or ""),
                "chat.step.execute.start",
                "开始执行 step（带超时）",
                "INFO",
                data={"step_index": idx, "action_key": ak, "timeout_s": float(timeout_sec)},
            )
            out_text = await asyncio.wait_for(_do_step(), timeout=timeout_sec)
            logger.info(f"[AI][chat_act] Step {idx} Execution completed successfully")
            _log_chat_event(
                project_id_pk,
                str(run_id or ""),
                "chat.step.execute.done",
                "step 执行成功",
                "INFO",
                data={"step_index": idx, "action_key": ak},
            )
        
        # [新增] 1. 专门捕获超时异常
        except asyncio.TimeoutError as e:
            err_text = f"Step '{ak}' execution timed out (> {timeout_sec}s)"
            logger.warning(f"[AI] {err_text}")
            _log_chat_event(
                project_id_pk,
                str(run_id or ""),
                "chat.step.execute.timeout",
                "step 执行超时",
                "WARN",
                data={"step_index": idx, "action_key": ak, "timeout_s": float(timeout_sec)},
            )
            
            if emit_stages and run_id:
                ts = _now_ms()
                # (A) 记录该步骤的具体错误
                _context_store.snapshot_stage(
                    project_id=project_id_pk,
                    run_id=run_id,
                    stage_name=f"chat.step.{idx}.error",
                    data={
                        "step_index": idx, 
                        "action_key": ak, 
                        "error": "timeout", # 错误类型标记
                        "message": err_text, 
                        "at_ms": ts
                    },
                )
                # (B) 记录全局错误
                _context_store.snapshot_stage(
                    project_id=project_id_pk,
                    run_id=run_id,
                    stage_name="chat.error",
                    data={
                        "error": "timeout", # 简短错误码
                        "message": err_text, # 详细信息
                        "step_index": idx, 
                        "action_key": ak, 
                        "at_ms": ts
                    },
                )
                # (C) [关键] 将全局状态设为 'timeout'
                # 前端检测到 status == 'timeout' 时，可以显示“重试”按钮并解锁 UI
                _context_store.snapshot_stage(
                    project_id=project_id_pk,
                    run_id=run_id,
                    stage_name="chat.status",
                    data={
                        "status": "timeout", # <--- 这是一个新的终态
                        "at_ms": ts, 
                        "current_step_index": idx, 
                        "current_action_key": ak
                    },
                )
            # 重新抛出，中断后续步骤
            raise asyncio.TimeoutError(err_text)

        # [保持] 2. 捕获其他通用异常
        except Exception as e:
            if emit_stages and run_id:
                err_text = f"{type(e).__name__}: {e}"
                ts = _now_ms()
                _log_chat_event(
                    project_id_pk,
                    str(run_id or ""),
                    "chat.step.execute.error",
                    "step 执行异常",
                    "ERROR",
                    data={"step_index": idx, "action_key": ak, "error": err_text},
                )
                logger.error(f"[AI][chat_act] Step '{ak}' execution error: {err_text}")
                _context_store.snapshot_stage(
                    project_id=project_id_pk,
                    run_id=run_id,
                    stage_name=f"chat.step.{idx}.error",
                    data={
                            "step_index": idx, 
                            "action_key": ak, 
                            "error": err_text, 
                            "at_ms": ts},
                )
                _context_store.snapshot_stage(
                    project_id=project_id_pk,
                    run_id=run_id,
                    stage_name="chat.error",
                    data={"error": err_text, "step_index": idx, "action_key": ak, "at_ms": ts},
                )
                _context_store.snapshot_stage(
                    project_id=project_id_pk,
                    run_id=run_id,
                    stage_name="chat.status",
                    data={"status": "error", "at_ms": ts, "current_step_index": idx, "current_action_key": ak},
                )
            raise

        dt = _now_ms() - t0
        steps_trace.append(
            {
                "step_index": idx,
                "action_key": ak,
                "input_text": in_text if req.debug else _trim_preview(in_text, 300),
                "output_text_preview": _trim_preview(out_text, 500),
                "ms": int(dt),
            }
        )
        logger.info(f"[AI][chat_act] Step {idx} End: {ak}, Output Text: {out_text}")
        _log_chat_event(
            project_id_pk,
            str(run_id or ""),
            "chat.step.output",
            "step 输出已生成",
            "INFO",
            data={"step_index": idx, "action_key": ak, "output_len": len(out_text or ""), "output_preview": _trim_preview(out_text, 400)},
        )
        _log_chat_event(
            project_id_pk,
            str(run_id or ""),
            "chat.step.end",
            "step 执行完成",
            "INFO",
            data={"step_index": idx, "action_key": ak, "ms": int(dt)},
        )

        if emit_stages and run_id:
            _context_store.snapshot_stage(
                project_id=project_id_pk,
                run_id=run_id,
                stage_name=f"chat.step.{idx}.end",
                data={"step_index": idx, "action_key": ak, "ms": int(dt), "output_preview": _trim_preview(out_text, 800), "at_ms": _now_ms()},
            )
            logger.info(f"[AI][chat_act] Snapshot Stage: chat.step.{idx}.end")
            _log_chat_event(
                project_id_pk,
                str(run_id or ""),
                "chat.snapshot.step_end",
                "写入 step end stage",
                "INFO",
                data={"step_index": idx, "action_key": ak},
            )

    created_run = None
    persistable = {"outline_generate", "generate_script", "script_optimize", "workflow_script", "workflow_storyboard"}
    logger.info(f"[AI][chat_act] Persistable: {persistable}")
    _log_chat_event(
        project_id_pk,
        str(run_id or ""),
        "chat.persist.persistable",
        "可持久化动作集合",
        "INFO",
        data={"persistable": sorted(list(persistable))},
    )
    if req.episode_id and final_action_key in persistable:
        _log_chat_event(
            project_id_pk,
            str(run_id or ""),
            "chat.persist.start",
            "准备写入 AiActionRun",
            "INFO",
            data={"final_action_key": final_action_key, "episode_id": req.episode_id},
        )
        logger.info(f"[AI][chat_act] Persist Final Action Key: {final_action_key}")
        _log_chat_event(
            project_id_pk,
            str(run_id or ""),
            "chat.persist.final_action_key",
            "准备持久化最终结果",
            "INFO",
            data={"episode_id": int(req.episode_id), "final_action_key": str(final_action_key or "")},
        )
        if final_action_key in ("generate_script", "script_optimize"):
            logger.info(f"[AI][chat_act] Generate Script: {artifacts.get('script')}")
            _log_chat_event(
                project_id_pk,
                str(run_id or ""),
                "chat.persist.payload",
                "选择持久化内容：script",
                "INFO",
                data={"output_len": len(str(artifacts.get("script") or "")), "output_preview": _trim_preview(str(artifacts.get("script") or ""), 400)},
            )
            final_output = str(artifacts.get("script") or "")
        elif final_action_key == "workflow_script":
            logger.info(f"[AI][chat_act] Workflow Script: {artifacts.get('script_fountain')}")
            _log_chat_event(
                project_id_pk,
                str(run_id or ""),
                "chat.persist.payload",
                "选择持久化内容：script_fountain",
                "INFO",
                data={"output_len": len(str(artifacts.get("script_fountain") or "")), "output_preview": _trim_preview(str(artifacts.get("script_fountain") or ""), 400)},
            )
            final_output = str(artifacts.get("script_fountain") or "")
        elif final_action_key == "workflow_storyboard":
            try:
                logger.info(f"[AI][chat_act] Workflow Storyboard: {artifacts.get('shots')}")
                _log_chat_event(
                    project_id_pk,
                    str(run_id or ""),
                    "chat.persist.payload",
                    "选择持久化内容：shots",
                    "INFO",
                    data={"shots_count": int(len(artifacts.get("shots") or [])) if isinstance(artifacts.get("shots"), list) else None},
                )
                final_output = json.dumps(artifacts.get("shots") or [], ensure_ascii=False, indent=2)
            except Exception as e:
                logger.error(f"[AI][chat_act] Workflow Storyboard Error: {e}")
                _log_chat_event(
                    project_id_pk,
                    str(run_id or ""),
                    "chat.persist.payload.error",
                    "workflow_storyboard 序列化失败",
                    "ERROR",
                    data={"error": f"{type(e).__name__}: {e}"},
                )
                final_output = str(artifacts.get("shots") or "")
        else:
            logger.info(f"[AI][chat_act] Outline: {artifacts.get('outline')}")
            _log_chat_event(
                project_id_pk,
                str(run_id or ""),
                "chat.persist.payload",
                "选择持久化内容：outline",
                "INFO",
                data={"output_len": len(str(artifacts.get("outline") or "")), "output_preview": _trim_preview(str(artifacts.get("outline") or ""), 400)},
            )
            final_output = str(artifacts.get("outline") or "")
        persist_action_key = final_action_key
        logger.info(f"[AI][chat_act] Persist Action Key: {persist_action_key}")
        _log_chat_event(
            project_id_pk,
            str(run_id or ""),
            "chat.persist.action_key",
            "确定写入 action_key",
            "INFO",
            data={"persist_action_key": str(persist_action_key or "")},
        )
        if persist_action_key == "outline_optimize":
            persist_action_key = "outline_generate"
        logger.info(f"[AI][chat_act] Persist Action Key: {persist_action_key}")
        _log_chat_event(
            project_id_pk,
            str(run_id or ""),
            "chat.persist.action_key",
            "归一化写入 action_key",
            "INFO",
            data={"persist_action_key": str(persist_action_key or "")},
        )
        db_obj = models.AiActionRun(
            project_id=project_id_pk,
            target_type="episode",
            target_id=int(req.episode_id),
            action_key=str(persist_action_key),
            input_text=message,
            output_text=final_output,
            meta_data={
                "run_id": run_id,
                "source": "chat",
                "intent": plan_parsed.get("intent_summary"),
                "planner_final_action_key": final_action_key,
                "plan": plan_parsed if req.debug else None,
            },
        )
        db.add(db_obj)
        db.commit()
        db.refresh(db_obj)
        created_run = {"id": db_obj.id, "action_key": db_obj.action_key, "created_at": str(db_obj.created_at)}
        _log_chat_event(
            project_id_pk,
            str(run_id or ""),
            "chat.persist.done",
            "AiActionRun 写入完成",
            "INFO",
            data={"run_db_id": db_obj.id, "action_key": db_obj.action_key},
        )

    assistant_msg = f"已完成：{plan_parsed.get('intent_summary') or '执行完成'}（最终动作：{final_action_key}）"
    logger.info(f"[AI][chat_act] Assistant Message: {assistant_msg}")
    _log_chat_event(
        project_id_pk,
        str(run_id or ""),
        "chat.assistant_message",
        "生成 assistant_message",
        "INFO",
        data={"assistant_message": str(assistant_msg or "")},
    )
    resp = ChatActResponse(
        run_id=run_id,
        assistant_message=assistant_msg,
        created_run=created_run,
        cards=cards or None,
        plan=plan_parsed if req.debug else None,
        steps_trace=steps_trace if req.debug else None,
        planner_prompt=(planner_system + "\n\n---\n\n" + planner_user) if req.debug else None,
        memory_trace=memory_trace if req.debug else None,
        planner_raw=(planner_raw or "") if req.debug else None,
    )
    logger.info(f"[AI][chat_act] Response: {resp.model_dump()}")
    _log_chat_event(project_id_pk, str(run_id or ""), "chat.response", "响应已构造", "INFO")
    _log_chat_event(project_id_pk, str(run_id or ""), "chat.response.ready", "响应已生成", "INFO")

    if emit_stages and run_id:
        _context_store.snapshot_stage(project_id=project_id_pk, run_id=run_id, stage_name="chat.final", data=resp.model_dump())
        _context_store.snapshot_stage(project_id=project_id_pk, run_id=run_id, stage_name="chat.status", data={"status": "done", "at_ms": _now_ms()})
        _context_store.snapshot_run(
            project_id=project_id_pk, run_id=run_id, request=req.model_dump(), response=resp.model_dump(), meta={"workflow": "chat_act"}
        )
    return resp


@router.post("/chat/act", response_model=ChatActResponse)
async def chat_act(req: ChatActRequest, db: Session = Depends(get_db)):
    """
    对话驱动编排：
    1) 通过 LLM 生成 JSON 计划（steps）
    2) 顺序执行 steps（复用现有原子能力：大纲/剧本生成与优化）
    3) 仅将最终结果写入 AiActionRun（source=chat），并返回 debug trace（可选）
    """
    run_id = (req.run_id or "").strip()
    if not run_id:
        raise HTTPException(status_code=400, detail="run_id 不能为空")
    project_id_pk = resolve_project_pk(db, req.project_id)
    logger.info(f"[AI][chat_act] Request: {req.model_dump()}")
    _log_chat_event(
        project_id_pk,
        str(run_id or ""),
        "chat.request",
        "收到 chat/act 请求",
        "INFO",
        data={"episode_id": req.episode_id, "current_action_key": req.current_action_key},
    )
    _log_chat_event(
        project_id_pk,
        run_id,
        "chat.request.received",
        "收到 chat/act 请求",
        "INFO",
        data={"episode_id": req.episode_id, "current_action_key": req.current_action_key},
    )
    resp = await _chat_act_core(req=req, db=db, emit_stages=False, run_id=run_id)
    logger.info(f"[AI][chat_act] Response: {resp.model_dump()}")
    _log_chat_event(project_id_pk, str(run_id or ""), "chat.response.sent", "响应已返回", "INFO")
    return resp


@router.post("/chat/act_async", response_model=ChatActAsyncResponse)
def chat_act_async(req: ChatActRequest, bg: BackgroundTasks):
    """
    异步版：返回 run_id；执行过程写入 runs-files stages，供前端轮询展示执行步骤。
    """
    logger.info(f"[AI][chat_act_async] Request: {req.model_dump()}")
    run_id = (req.run_id or "").strip()
    if not run_id:
        raise HTTPException(status_code=400, detail="run_id 不能为空")
    _db = SessionLocal()
    try:
        project_id_pk = resolve_project_pk(_db, req.project_id)
    finally:
        try:
            _db.close()
        except Exception as e:
            pass
    _log_chat_event(
        project_id_pk,
        str(run_id or ""),
        "chat.async.request",
        "收到 chat/act_async 请求",
        "INFO",
        data={"episode_id": req.episode_id, "current_action_key": req.current_action_key},
    )
    _log_chat_event(
        project_id_pk,
        run_id,
        "chat.request.received",
        "收到 chat/act_async 请求",
        "INFO",
        data={"episode_id": req.episode_id, "current_action_key": req.current_action_key},
    )
    _context_store.snapshot_stage(project_id=project_id_pk, run_id=run_id, stage_name="chat.status", data={"status": "queued", "at_ms": _now_ms()})
    payload = req.model_dump()

    def _runner():
        db = SessionLocal()
        try:
            import anyio

            async def _go():
                try:
                    logger.info(f"[AI][chat_act_async] Running step {payload}")
                    _log_chat_event(
                        project_id_pk,
                        str(run_id or ""),
                        "chat.async.runner.start",
                        "后台线程开始执行",
                        "INFO",
                        data={"payload_len": len(str(payload))},
                    )
                    _log_chat_event(project_id_pk, run_id, "chat.async.start", "后台线程开始执行", "INFO", data={"payload_len": len(str(payload))})
                    await _chat_act_core(req=ChatActRequest(**payload), db=db, emit_stages=True, run_id=run_id)
                except Exception as e:
                    logger.error(f"[AI][chat_act_async] Error: {e}")
                    _log_chat_event(
                        project_id_pk,
                        str(run_id or ""),
                        "chat.async.runner.error",
                        "后台执行异常",
                        "ERROR",
                        data={"error": f"{type(e).__name__}: {e}"},
                    )
                    _log_chat_event(project_id_pk, run_id, "chat.async.error", "后台执行异常", "ERROR", data={"error": str(e)})
                    _context_store.snapshot_stage(project_id=project_id_pk, run_id=run_id, stage_name="chat.error", data={"error": str(e), "at_ms": _now_ms()})
                    _context_store.snapshot_stage(project_id=project_id_pk, run_id=run_id, stage_name="chat.status", data={"status": "error", "at_ms": _now_ms()})

            anyio.run(_go)
        finally:
            try:
                db.close()
            except Exception as e:
                pass

    # IMPORTANT: 不使用 BackgroundTasks（其执行在同一 worker 线程，可能阻塞事件循环，导致轮询接口也卡死）。
    # 这里用 daemon thread 真正后台执行，确保 /ai/runs-files 等轻量查询不会被长耗时 LLM 调用阻塞。
    import threading

    threading.Thread(target=_runner, daemon=True).start()
    return ChatActAsyncResponse(run_id=run_id, status="queued")

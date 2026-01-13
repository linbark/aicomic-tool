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
from ..services.context_store import new_run_id
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
    if emit_stages and run_id:
        _context_store.snapshot_stage(
            project_id=project_id_pk, run_id=run_id, stage_name="chat.status", data={"status": "running", "at_ms": _now_ms()}
        )

    raw = _read_settings_raw()
    settings = _mask_settings(raw)
    if not settings.has_api_key:
        logger.error(f"[AI][chat_act] AI API Key 未配置")
        log_ui(project_id_pk, run_id, "AI API Key 未配置", "ERROR")
        raise HTTPException(status_code=400, detail="AI API Key 未配置")

    message = (req.message or "").strip()
    if not message:
        logger.error(f"[AI][chat_act] message 不能为空")
        log_ui(project_id_pk, run_id, "message 不能为空", "ERROR")
        raise HTTPException(status_code=400, detail="message 不能为空")

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
        log_ui(project_id_pk, run_id, f"message 模糊不清: {message}", "ERROR")
        resp = ChatActResponse(
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
        log_ui(project_id_pk, run_id, f"Response: {resp.model_dump()}", "INFO")
        return resp
    logger.info(f"[AI][chat_act] Message: {message}")
    log_ui(project_id_pk, run_id, f"Message: {message}", "INFO")
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
            log_ui(project_id_pk, run_id, f"Memory retrieval failed: {e}", "ERROR")
    # -------- Planner Prompt --------
    ui_ctx = req.ui_context or {}
    logger.info(f"[AI][chat_act] UI Context: {ui_ctx}")
    log_ui(project_id_pk, run_id, f"UI Context: {ui_ctx}", "INFO")
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
    log_ui(project_id_pk, run_id, f"Planner User: {planner_user}", "INFO")
    if memory_context_text:
        planner_user += f"\n\n记忆上下文（必须遵守，可能为空）：\n{memory_context_text}"

    planner_raw = await _chat_client.chat(
        settings=LlmChatSettings(
            base_url=settings.base_url,
            api_key=raw.get("api_key") or "",
            model=settings.model,
            temperature=min(float(settings.temperature or 0.2), 0.3),
            max_tokens=max(int(settings.max_tokens or 0), 2048),
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
        log_ui(project_id_pk, run_id, f"Planner JSON parse failed: {e}\nRaw: {planner_raw}", "ERROR")
        # 构造一个错误提示返回给用户，而不是让后台崩溃
        raise HTTPException(status_code=502, detail=f"AI 响应格式错误: {str(e)}")
    # [修改结束]
    logger.info(f"[AI][chat_act] Planner Parsed: {plan_parsed}")
    log_ui(project_id_pk, run_id, f"Planner Parsed: {plan_parsed}", "INFO")
    if not isinstance(plan_parsed, dict):
        logger.error(f"[AI][chat_act] Planner 输出无法解析为 JSON: {(planner_raw or '')[:500]}")
        raise HTTPException(status_code=502, detail=f"Planner 输出无法解析为 JSON: {(planner_raw or '')[:500]}")
    logger.info(f"[AI][chat_act] Planner Needs Clarification: {bool(plan_parsed.get('needs_clarification'))}")
    log_ui(project_id_pk, run_id, f"Planner Needs Clarification: {bool(plan_parsed.get('needs_clarification'))}", "INFO")
    if bool(plan_parsed.get("needs_clarification")):
        logger.info(f"[AI][chat_act] Planner Needs Clarification: True")
        q = str(plan_parsed.get("clarifying_question") or "").strip() or "我需要你补充一下目标/范围：你希望我具体做什么？"
        opts = plan_parsed.get("clarifying_options")
        options = []
        if isinstance(opts, list):
            options = [{"label": str(x), "value": str(x)} for x in opts[:8] if str(x).strip()]
        resp = ChatActResponse(
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
    log_ui(project_id_pk, run_id, f"Planner Steps: {plan_parsed.get('steps')}", "INFO")
    steps = plan_parsed.get("steps") or []
    if not isinstance(steps, list) or len(steps) == 0:
        logger.error(f"[AI][chat_act] Planner steps 为空: {plan_parsed}")
        log_ui(project_id_pk, run_id, f"Planner steps 为空: {plan_parsed}", "ERROR")
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
    log_ui(project_id_pk, run_id, f"Final Action Key: {final_action_key}", "INFO")
    if emit_stages and run_id:
        logger.info(f"[AI][chat_act] Snapshot Stage: chat.plan")
        log_ui(project_id_pk, run_id, f"Snapshot Stage: chat.plan", "INFO")
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

    for idx, s in enumerate(steps):
        logger.info(f"[AI][chat_act] Step {idx} Start: {s}")
        log_ui(project_id_pk, run_id, f"Step {idx} Start: {s}", "INFO")
        ak = str(s.get("action_key"))
        in_text = (s.get("input_text") or "").strip()
        why = (s.get("why") or "").strip()

        # 对于优化类操作，优先使用前一步的 artifacts 输出（如果存在）
        # 因为优化操作需要的是前一步的输出，而不是用户意图
        logger.info(f"[AI][chat_act] Step {idx} Before input processing: ak={ak}, input_text={_trim_preview(in_text, 100)}, artifacts.keys()={list(artifacts.keys())}")
        log_ui(project_id_pk, run_id, f"Step {idx} Before input processing: ak={ak}, artifacts.keys()={list(artifacts.keys())}", "INFO")
        
        if ak == "outline_optimize" and isinstance(artifacts.get("outline"), str) and artifacts.get("outline").strip():
            logger.info(f"[AI][chat_act] Step {idx} Using artifacts['outline'] (length={len(artifacts['outline'])})")
            log_ui(project_id_pk, run_id, f"Step {idx} Using artifacts['outline'] (length={len(artifacts['outline'])})", "INFO")
            in_text = artifacts["outline"]
        elif ak == "script_optimize" and isinstance(artifacts.get("script"), str) and artifacts.get("script").strip():
            logger.info(f"[AI][chat_act] Step {idx} Using artifacts['script'] (length={len(artifacts['script'])})")
            log_ui(project_id_pk, run_id, f"Step {idx} Using artifacts['script'] (length={len(artifacts['script'])})", "INFO")
            in_text = artifacts["script"]
        elif ak == "generate_script" and isinstance(artifacts.get("outline"), str) and artifacts.get("outline").strip():
            logger.info(f"[AI][chat_act] Step {idx} Using artifacts['outline'] for generate_script (length={len(artifacts['outline'])})")
            log_ui(project_id_pk, run_id, f"Step {idx} Using artifacts['outline'] for generate_script (length={len(artifacts['outline'])})", "INFO")
            in_text = artifacts["outline"]
        elif not in_text:
            # 如果 input_text 为空且没有可用的 artifacts，使用 UI 上下文或用户消息
            logger.info(f"[AI][chat_act] Step {idx} No artifacts available, using UI context or message")
            log_ui(project_id_pk, run_id, f"Step {idx} No artifacts available, using UI context or message", "INFO")
            in_text = selected_input_preview or master_script_preview or message

        # 对于优化类操作，将用户意图追加到输入文本后面
        if ak in ("outline_optimize", "script_optimize"):
            in_text = f"{in_text}\n\n[用户意图]\n{message}"
        
        logger.info(f"[AI][chat_act] Step {idx} After input processing: in_text length={len(in_text)}, preview={_trim_preview(in_text, 200)}")
        log_ui(project_id_pk, run_id, f"Step {idx} After input processing: in_text length={len(in_text)}", "INFO")

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

        t0 = _now_ms()
        out_text = ""
        
        logger.info(f"[AI][chat_act] Step {idx} About to call _do_step() for action_key={ak}")
        log_ui(project_id_pk, run_id, f"Step {idx} About to call _do_step() for action_key={ak}", "INFO")

        async def _do_step() -> str:
            nonlocal out_text
            logger.info(f"[AI][chat_act] _do_step() called for action_key={ak}")
            log_ui(project_id_pk, run_id, f"_do_step() called for action_key={ak}", "INFO")
            
            if ak == "outline_generate":
                logger.info(f"[AI][chat_act] Calling outline_generate with text length={len(in_text)}")
                log_ui(project_id_pk, run_id, f"Calling outline_generate with text length={len(in_text)}", "INFO")
                resp = await outline_generate(OutlineGenerateRequest(text=in_text, project_id=project_id_pk))
                out_text = (resp.text or "").strip()
                artifacts["outline"] = out_text
                log_ui(project_id_pk, run_id, f"Outline Generate Response: {out_text}", "INFO")
                log_ui(project_id_pk, run_id, f"Outline Generate Response artifacts: {artifacts}", "INFO")
                logger.info(f"[AI][chat_act] Outline Generate Response: {out_text}")
                logger.info(f"[AI][chat_act] Outline Generate Response artifacts: {artifacts}")
                return out_text
            if ak == "outline_optimize":
                logger.info(f"[AI][chat_act] Calling outline_optimize with text length={len(in_text)}, project_id={project_id_pk}")
                log_ui(project_id_pk, run_id, f"Calling outline_optimize with text length={len(in_text)}, project_id={project_id_pk}", "INFO")
                try:
                    resp = await outline_optimize(OutlineOptimizeRequest(text=in_text, project_id=project_id_pk))
                    logger.info(f"[AI][chat_act] outline_optimize returned, response length={len(resp.text or '')}")
                    log_ui(project_id_pk, run_id, f"outline_optimize returned, response length={len(resp.text or '')}", "INFO")
                    out_text = (resp.text or "").strip()
                    artifacts["outline"] = out_text
                    log_ui(project_id_pk, run_id, f"Outline Optimize Response: {out_text}", "INFO")
                    log_ui(project_id_pk, run_id, f"Outline Optimize Response artifacts: {artifacts}", "INFO")
                    logger.info(f"[AI][chat_act] Outline Optimize Response: {out_text}")
                    logger.info(f"[AI][chat_act] Outline Optimize Response artifacts: {artifacts}")
                    return out_text
                except Exception as e:
                    logger.error(f"[AI][chat_act] outline_optimize raised exception: {type(e).__name__}: {e}")
                    log_ui(project_id_pk, run_id, f"outline_optimize raised exception: {type(e).__name__}: {e}", "ERROR")
                    raise
            if ak == "generate_script":
                resp = await generate_script(ScriptGenerateRequest(text=in_text, project_id=project_id_pk))
                out_text = (resp.text or "").strip()
                artifacts["script"] = out_text
                log_ui(project_id_pk, run_id, f"Generate Script Response: {out_text}", "INFO")
                log_ui(project_id_pk, run_id, f"Generate Script Response artifacts: {artifacts}", "INFO")
                logger.info(f"[AI][chat_act] Generate Script Response: {out_text}")
                logger.info(f"[AI][chat_act] Generate Script Response artifacts: {artifacts}")
                return out_text
            if ak == "script_optimize":
                resp = await script_optimize(ScriptOptimizeRequest(text=in_text, project_id=project_id_pk))
                out_text = (resp.text or "").strip()
                artifacts["script"] = out_text
                log_ui(project_id_pk, run_id, f"Script Optimize Response: {out_text}", "INFO")
                log_ui(project_id_pk, run_id, f"Script Optimize Response artifacts: {artifacts}", "INFO")
                logger.info(f"[AI][chat_act] Script Optimize Response: {out_text}")
                logger.info(f"[AI][chat_act] Script Optimize Response artifacts: {artifacts}")
                return out_text
            if ak == "workflow_script":
                wf_resp = await workflow_script(
                    WorkflowScriptRequest(
                        project_id=req.project_id,
                        input_text=in_text,
                        options=WorkflowScriptOptions(),
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

                store = get_memory_store()
                base_text = ""
                log_ui(project_id_pk, run_id, f"Memory Extract Changeset Artifacts: {artifacts}", "INFO")
                if artifacts.get("script_fountain"):
                    base_text += f"### script_fountain\n{artifacts.get('script_fountain')}\n\n"
                if artifacts.get("beat_sheet"):
                    try:
                        base_text += "### beat_sheet\n" + json.dumps(artifacts.get("beat_sheet"), ensure_ascii=False, indent=2) + "\n\n"
                    except Exception as e:
                        log_ui(project_id_pk, run_id, f"Memory Extract Changeset Beat Sheet Error: {e}", "ERROR")
                if artifacts.get("series_bible"):
                    try:
                        base_text += "### series_bible\n" + json.dumps(artifacts.get("series_bible"), ensure_ascii=False, indent=2) + "\n\n"
                    except Exception as e:
                        log_ui(project_id_pk, run_id, f"Memory Extract Changeset Series Bible Error: {e}", "ERROR")
                if not base_text:
                    base_text = master_script_preview or message
                log_ui(project_id_pk, run_id, f"Memory Extract Changeset Base Text: {base_text}", "INFO")
                evidences = chunk_text_to_evidences(
                    project_id=project_id_pk,
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
                        log_ui(project_id_pk, run_id, f"Memory Extract Changeset Upsert Evidence Error: {e}", "ERROR")
                        continue
                logger.info(f"[AI][chat_act] Memory Extract Changeset Evidence IDs: {evidence_ids}")
                log_ui(project_id_pk, run_id, f"Memory Extract Changeset Evidence IDs: {evidence_ids}", "INFO")
                payload, extractor_trace = await extract_changeset_v0_with_llm_with_trace(
                    llm_settings=LlmChatSettings(
                        base_url=settings.base_url,
                        api_key=raw.get("api_key") or "",
                        model=settings.model,
                        temperature=min(float(settings.temperature or 0.2), 0.3),
                        max_tokens=max(int(settings.max_tokens or 0), 4096),
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
                    log_ui(project_id_pk, run_id, f"Memory Extract Changeset Append Changeset Review Entry Extractor Trace Error: {e}", "ERROR")
                    pass
                try:
                    store.append_changeset_review_entry(changeset_id=changeset_id, entry={"at_ms": _now_ms(), "action": "resolver_trace", "data": resolver_trace})
                except Exception as e:
                    log_ui(project_id_pk, run_id, f"Memory Extract Changeset Append Changeset Review Entry Resolver Trace Error: {e}", "ERROR")
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
            log_ui(project_id_pk, run_id, f"Step {idx} Starting execution with timeout={timeout_sec}s", "INFO")
            out_text = await asyncio.wait_for(_do_step(), timeout=timeout_sec)
            logger.info(f"[AI][chat_act] Step {idx} Execution completed successfully")
            log_ui(project_id_pk, run_id, f"Step {idx} Execution completed successfully", "INFO")
        
        # [新增] 1. 专门捕获超时异常
        except asyncio.TimeoutError as e:
            err_text = f"Step '{ak}' execution timed out (> {timeout_sec}s)"
            logger.warning(f"[AI] {err_text}")
            log_ui(project_id_pk, run_id, f"Step '{ak}' execution timed out (> {timeout_sec}s)", "WARNING")
            
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
                log_ui(project_id_pk, run_id, f"Step '{ak}' execution error: {err_text}", "ERROR")
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
        log_ui(project_id_pk, run_id, f"Step {idx} End: {ak}, Output Text: {out_text}", "INFO")

        if emit_stages and run_id:
            _context_store.snapshot_stage(
                project_id=project_id_pk,
                run_id=run_id,
                stage_name=f"chat.step.{idx}.end",
                data={"step_index": idx, "action_key": ak, "ms": int(dt), "output_preview": _trim_preview(out_text, 800), "at_ms": _now_ms()},
            )
            logger.info(f"[AI][chat_act] Snapshot Stage: chat.step.{idx}.end")
            log_ui(project_id_pk, run_id, f"Snapshot Stage: chat.step.{idx}.end", "INFO")

    created_run = None
    persistable = {"outline_generate", "generate_script", "script_optimize", "workflow_script", "workflow_storyboard"}
    logger.info(f"[AI][chat_act] Persistable: {persistable}")
    log_ui(project_id_pk, run_id, f"Persistable: {persistable}", "INFO")
    if req.episode_id and final_action_key in persistable:
        logger.info(f"[AI][chat_act] Persist Final Action Key: {final_action_key}")
        log_ui(project_id_pk, run_id, f"Persist Final Action Key: {final_action_key}", "INFO")
        if final_action_key in ("generate_script", "script_optimize"):
            logger.info(f"[AI][chat_act] Generate Script: {artifacts.get('script')}")
            log_ui(project_id_pk, run_id, f"Generate Script: {artifacts.get('script')}", "INFO")
            final_output = str(artifacts.get("script") or "")
        elif final_action_key == "workflow_script":
            logger.info(f"[AI][chat_act] Workflow Script: {artifacts.get('script_fountain')}")
            log_ui(project_id_pk, run_id, f"Workflow Script: {artifacts.get('script_fountain')}", "INFO")
            final_output = str(artifacts.get("script_fountain") or "")
        elif final_action_key == "workflow_storyboard":
            try:
                logger.info(f"[AI][chat_act] Workflow Storyboard: {artifacts.get('shots')}")
                log_ui(project_id_pk, run_id, f"Workflow Storyboard: {artifacts.get('shots')}", "INFO")
                final_output = json.dumps(artifacts.get("shots") or [], ensure_ascii=False, indent=2)
            except Exception as e:
                logger.error(f"[AI][chat_act] Workflow Storyboard Error: {e}")
                log_ui(project_id_pk, run_id, f"Workflow Storyboard Error: {e}", "ERROR")
                final_output = str(artifacts.get("shots") or "")
        else:
            logger.info(f"[AI][chat_act] Outline: {artifacts.get('outline')}")
            log_ui(project_id_pk, run_id, f"Outline: {artifacts.get('outline')}", "INFO")
            final_output = str(artifacts.get("outline") or "")
        persist_action_key = final_action_key
        logger.info(f"[AI][chat_act] Persist Action Key: {persist_action_key}")
        log_ui(project_id_pk, run_id, f"Persist Action Key: {persist_action_key}", "INFO")
        if persist_action_key == "outline_optimize":
            persist_action_key = "outline_generate"
        logger.info(f"[AI][chat_act] Persist Action Key: {persist_action_key}")
        log_ui(project_id_pk, run_id, f"Persist Action Key: {persist_action_key}", "INFO")
        db_obj = models.AiActionRun(
            project_id=project_id_pk,
            target_type="episode",
            target_id=int(req.episode_id),
            action_key=str(persist_action_key),
            input_text=message,
            output_text=final_output,
            meta_data={
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

    assistant_msg = f"已完成：{plan_parsed.get('intent_summary') or '执行完成'}（最终动作：{final_action_key}）"
    logger.info(f"[AI][chat_act] Assistant Message: {assistant_msg}")
    log_ui(project_id_pk, run_id, f"Assistant Message: {assistant_msg}", "INFO")
    resp = ChatActResponse(
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
    log_ui(project_id_pk, run_id, f"Response: {resp.model_dump()}", "INFO")

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
    logger.info(f"[AI][chat_act] Request: {req.model_dump()}")
    log_ui(req.project_id, run_id, f"Request: {req.model_dump()}", "INFO")
    resp = await _chat_act_core(req=req, db=db, emit_stages=False, run_id=None)
    logger.info(f"[AI][chat_act] Response: {resp.model_dump()}")
    log_ui(req.project_id, run_id, f"Response: {resp.model_dump()}", "INFO")
    return resp


@router.post("/chat/act_async", response_model=ChatActAsyncResponse)
def chat_act_async(req: ChatActRequest, bg: BackgroundTasks):
    """
    异步版：返回 run_id；执行过程写入 runs-files stages，供前端轮询展示执行步骤。
    """
    logger.info(f"[AI][chat_act_async] Request: {req.model_dump()}")
    log_ui(req.project_id, "chat_act_async", f"Request: {req.model_dump()}", "INFO")
    run_id = new_run_id()
    _db = SessionLocal()
    try:
        project_id_pk = resolve_project_pk(_db, req.project_id)
    finally:
        try:
            _db.close()
        except Exception as e:
            pass
    _context_store.snapshot_stage(project_id=project_id_pk, run_id=run_id, stage_name="chat.status", data={"status": "queued", "at_ms": _now_ms()})
    payload = req.model_dump()

    def _runner():
        db = SessionLocal()
        try:
            import anyio

            async def _go():
                try:
                    logger.info(f"[AI][chat_act_async] Running step {payload}")
                    log_ui(project_id_pk, run_id, f"开始执行任务，Payload长度: {len(str(payload))}", "INFO")
                    await _chat_act_core(req=ChatActRequest(**payload), db=db, emit_stages=True, run_id=run_id)
                except Exception as e:
                    logger.error(f"[AI][chat_act_async] Error: {e}")
                    log_ui(project_id_pk, run_id, f"发生异常: {str(e)}", "ERROR")
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

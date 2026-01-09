import asyncio
import json
import time
from typing import Any, Dict, List, Optional

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
        raise HTTPException(status_code=400, detail="AI API Key 未配置")

    message = (req.message or "").strip()
    if not message:
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
        return resp

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
                    except Exception:
                        memory_trace.append({"key": k, "text": None})
        except Exception as e:
            print(f"[AI][chat_act] Memory retrieval failed: {e}")

    # -------- Planner Prompt --------
    ui_ctx = req.ui_context or {}
    current_action_key = (req.current_action_key or "").strip()
    master_script_preview = _trim_preview(str(ui_ctx.get("master_script") or ""), 1200)
    selected_input_preview = _trim_preview(str(ui_ctx.get("current_input") or ""), 800)

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
    plan_parsed = extract_json_any(planner_raw or "")
    if not isinstance(plan_parsed, dict):
        raise HTTPException(status_code=502, detail=f"Planner 输出无法解析为 JSON: {(planner_raw or '')[:500]}")

    if bool(plan_parsed.get("needs_clarification")):
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
        return resp

    steps = plan_parsed.get("steps") or []
    if not isinstance(steps, list) or len(steps) == 0:
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
            raise HTTPException(status_code=502, detail=f"Planner steps 包含非法 action_key: {s}")
    final_action_key = steps[-1].get("action_key")
    plan_parsed["final_action_key"] = final_action_key

    if emit_stages and run_id:
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
        base = float(getattr(settings, "timeout_seconds", 60.0) or 60.0)
        base = max(base, 60.0)
        ak = str(action_key or "")
        if ak in ("workflow_script",):
            return max(base * 4.0, 240.0)
        if ak in ("workflow_storyboard", "memory_extract_changeset"):
            return max(base * 2.0, 180.0)
        # 普通单步（大纲/剧本）给一点 buffer，避免网络抖动造成“永远等不到”
        return max(base + 30.0, 90.0)

    for idx, s in enumerate(steps):
        ak = str(s.get("action_key"))
        in_text = (s.get("input_text") or "").strip()
        why = (s.get("why") or "").strip()

        if not in_text:
            if ak in ("outline_optimize",) and isinstance(artifacts.get("outline"), str) and artifacts.get("outline").strip():
                in_text = artifacts["outline"]
            elif ak in ("script_optimize",) and isinstance(artifacts.get("script"), str) and artifacts.get("script").strip():
                in_text = artifacts["script"]
            elif ak in ("generate_script",) and isinstance(artifacts.get("outline"), str) and artifacts.get("outline").strip():
                in_text = artifacts["outline"]
            else:
                in_text = selected_input_preview or master_script_preview or message

        if ak in ("outline_optimize", "script_optimize"):
            in_text = f"{in_text}\n\n[用户意图]\n{message}"

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

        async def _do_step() -> str:
            nonlocal out_text
            if ak == "outline_generate":
                resp = await outline_generate(OutlineGenerateRequest(text=in_text, project_id=project_id_pk))
                out_text = (resp.text or "").strip()
                artifacts["outline"] = out_text
                return out_text
            if ak == "outline_optimize":
                resp = await outline_optimize(OutlineOptimizeRequest(text=in_text, project_id=project_id_pk))
                out_text = (resp.text or "").strip()
                artifacts["outline"] = out_text
                return out_text
            if ak == "generate_script":
                resp = await generate_script(ScriptGenerateRequest(text=in_text, project_id=project_id_pk))
                out_text = (resp.text or "").strip()
                artifacts["script"] = out_text
                return out_text
            if ak == "script_optimize":
                resp = await script_optimize(ScriptOptimizeRequest(text=in_text, project_id=project_id_pk))
                out_text = (resp.text or "").strip()
                artifacts["script"] = out_text
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
                if artifacts.get("script_fountain"):
                    base_text += f"### script_fountain\n{artifacts.get('script_fountain')}\n\n"
                if artifacts.get("beat_sheet"):
                    try:
                        base_text += "### beat_sheet\n" + json.dumps(artifacts.get("beat_sheet"), ensure_ascii=False, indent=2) + "\n\n"
                    except Exception:
                        pass
                if artifacts.get("series_bible"):
                    try:
                        base_text += "### series_bible\n" + json.dumps(artifacts.get("series_bible"), ensure_ascii=False, indent=2) + "\n\n"
                    except Exception:
                        pass
                if not base_text:
                    base_text = master_script_preview or message

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
                    except Exception:
                        continue

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
                except Exception:
                    pass
                try:
                    store.append_changeset_review_entry(changeset_id=changeset_id, entry={"at_ms": _now_ms(), "action": "resolver_trace", "data": resolver_trace})
                except Exception:
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
            out_text = await asyncio.wait_for(_do_step(), timeout=_step_timeout_seconds(ak))
        except Exception as e:
            if emit_stages and run_id:
                err_text = f"{type(e).__name__}: {e}"
                _context_store.snapshot_stage(
                    project_id=project_id_pk,
                    run_id=run_id,
                    stage_name=f"chat.step.{idx}.error",
                    data={"step_index": idx, "action_key": ak, "error": err_text, "at_ms": _now_ms()},
                )
                _context_store.snapshot_stage(
                    project_id=project_id_pk,
                    run_id=run_id,
                    stage_name="chat.error",
                    data={"error": err_text, "step_index": idx, "action_key": ak, "at_ms": _now_ms()},
                )
                _context_store.snapshot_stage(
                    project_id=project_id_pk,
                    run_id=run_id,
                    stage_name="chat.status",
                    data={"status": "error", "at_ms": _now_ms(), "current_step_index": idx, "current_action_key": ak},
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

        if emit_stages and run_id:
            _context_store.snapshot_stage(
                project_id=project_id_pk,
                run_id=run_id,
                stage_name=f"chat.step.{idx}.end",
                data={"step_index": idx, "action_key": ak, "ms": int(dt), "output_preview": _trim_preview(out_text, 800), "at_ms": _now_ms()},
            )

    created_run = None
    persistable = {"outline_generate", "generate_script", "script_optimize", "workflow_script", "workflow_storyboard"}
    if req.episode_id and final_action_key in persistable:
        if final_action_key in ("generate_script", "script_optimize"):
            final_output = str(artifacts.get("script") or "")
        elif final_action_key == "workflow_script":
            final_output = str(artifacts.get("script_fountain") or "")
        elif final_action_key == "workflow_storyboard":
            try:
                final_output = json.dumps(artifacts.get("shots") or [], ensure_ascii=False, indent=2)
            except Exception:
                final_output = str(artifacts.get("shots") or "")
        else:
            final_output = str(artifacts.get("outline") or "")
        persist_action_key = final_action_key
        if persist_action_key == "outline_optimize":
            persist_action_key = "outline_generate"
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
    return await _chat_act_core(req=req, db=db, emit_stages=False, run_id=None)


@router.post("/chat/act_async", response_model=ChatActAsyncResponse)
def chat_act_async(req: ChatActRequest, bg: BackgroundTasks):
    """
    异步版：返回 run_id；执行过程写入 runs-files stages，供前端轮询展示执行步骤。
    """
    run_id = new_run_id()
    _db = SessionLocal()
    try:
        project_id_pk = resolve_project_pk(_db, req.project_id)
    finally:
        try:
            _db.close()
        except Exception:
            pass
    _context_store.snapshot_stage(project_id=project_id_pk, run_id=run_id, stage_name="chat.status", data={"status": "queued", "at_ms": _now_ms()})
    payload = req.model_dump()

    def _runner():
        db = SessionLocal()
        try:
            import anyio

            async def _go():
                try:
                    await _chat_act_core(req=ChatActRequest(**payload), db=db, emit_stages=True, run_id=run_id)
                except Exception as e:
                    _context_store.snapshot_stage(project_id=project_id_pk, run_id=run_id, stage_name="chat.error", data={"error": str(e), "at_ms": _now_ms()})
                    _context_store.snapshot_stage(project_id=project_id_pk, run_id=run_id, stage_name="chat.status", data={"status": "error", "at_ms": _now_ms()})

            anyio.run(_go)
        finally:
            try:
                db.close()
            except Exception:
                pass

    # IMPORTANT: 不使用 BackgroundTasks（其执行在同一 worker 线程，可能阻塞事件循环，导致轮询接口也卡死）。
    # 这里用 daemon thread 真正后台执行，确保 /ai/runs-files 等轻量查询不会被长耗时 LLM 调用阻塞。
    import threading

    threading.Thread(target=_runner, daemon=True).start()
    return ChatActAsyncResponse(run_id=run_id, status="queued")


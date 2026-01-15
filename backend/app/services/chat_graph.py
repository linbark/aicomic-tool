from __future__ import annotations

import asyncio
import json
import threading
import time
from typing import Any, Dict, List, Optional, TypedDict

from fastapi import HTTPException
from langgraph.graph import END, StateGraph
from sqlalchemy.orm import Session

from .. import models
from ..database import SessionLocal
from ..routers.ai_helpers import safe_parse_json
from ..routers.ai_shared import _build_memory_context, _chat_client, _context_store, _mask_settings, _read_settings_raw
from ..routers.ai_workflows import (
    WorkflowScriptOptions,
    WorkflowScriptRequest,
    WorkflowStoryboardOptions,
    WorkflowStoryboardRequest,
    workflow_script,
    workflow_storyboard,
)
from ..routers.ai_writing import (
    OutlineGenerateRequest,
    OutlineOptimizeRequest,
    ScriptGenerateRequest,
    ScriptOptimizeRequest,
    generate_script,
    outline_generate,
    outline_optimize,
    script_optimize,
)
from ..services.llm_client import LlmChatSettings
from ..services.project_lookup import resolve_project_pk


class ChatGraphInterrupt(TypedDict, total=False):
    kind: str
    changeset_id: str
    resume_state: Dict[str, Any]


class ChatGraphState(TypedDict, total=False):
    project_id_pk: int
    project_id: str
    episode_id: Optional[int]
    run_id: str
    message: str
    current_action_key: str
    ui_context: Dict[str, Any]
    debug: bool

    memory_trace: List[Dict[str, Any]]
    memory_context_text: str

    planner_system: str
    planner_user: str
    planner_raw: str
    plan_parsed: Dict[str, Any]
    steps: List[Dict[str, Any]]
    final_action_key: str
    needs_clarification: bool

    artifacts: Dict[str, Any]
    steps_trace: List[Dict[str, Any]]
    cards: List[Dict[str, Any]]
    step_index: int

    created_run: Optional[Dict[str, Any]]
    assistant_message: str
    response: Dict[str, Any]
    interrupt: Optional[ChatGraphInterrupt]


class _Abort(Exception):
    def __init__(self, error: str):
        self.error = error
        super().__init__(error)


def _now_ms() -> int:
    return int(time.time() * 1000)


def _trim_preview(text: Any, n: int) -> str:
    s = str(text or "")
    if len(s) <= n:
        return s
    return s[:n] + "…"


def _looks_ambiguous(msg: str) -> bool:
    m = (msg or "").strip()
    if len(m) <= 6:
        return True
    verbs = ["生成", "提取", "优化", "改", "润色", "分镜", "拆", "入库", "保存", "整理", "总结", "加快", "减少"]
    if not any(v in m for v in verbs):
        return True
    return False


class StageEmitter:
    def __init__(self, *, project_id: int, run_id: str, emit_stages: bool):
        self.project_id = int(project_id)
        self.run_id = str(run_id)
        self.emit_stages = bool(emit_stages)

    def _write(self, stage_name: str, data: Any) -> None:
        if not self.emit_stages:
            return
        _context_store.snapshot_stage(project_id=self.project_id, run_id=self.run_id, stage_name=stage_name, data=data)

    def status(self, status: str, **extra: Any) -> None:
        payload = {"status": str(status), "at_ms": _now_ms(), **{k: v for k, v in (extra or {}).items() if v is not None}}
        self._write("chat.status", payload)

    def plan(self, plan: Dict[str, Any]) -> None:
        self._write("chat.plan", {"plan": plan})

    def step_start(self, *, step_index: int, action_key: str, why: Optional[str], input_preview: str) -> None:
        self._write(
            f"chat.step.{int(step_index)}.start",
            {
                "step_index": int(step_index),
                "action_key": str(action_key),
                "why": why or None,
                "input_preview": _trim_preview(input_preview, 400),
                "at_ms": _now_ms(),
            },
        )
        self.status("running", current_step_index=int(step_index), current_action_key=str(action_key))

    def step_end(self, *, step_index: int, action_key: str, ms: int, output_preview: str) -> None:
        self._write(
            f"chat.step.{int(step_index)}.end",
            {
                "step_index": int(step_index),
                "action_key": str(action_key),
                "ms": int(ms),
                "output_preview": _trim_preview(output_preview, 800),
                "at_ms": _now_ms(),
            },
        )

    def step_error(self, *, step_index: int, action_key: str, error: str, message: Optional[str] = None) -> None:
        self._write(
            f"chat.step.{int(step_index)}.error",
            {
                "step_index": int(step_index),
                "action_key": str(action_key),
                "error": str(error),
                "message": str(message) if message is not None else None,
                "at_ms": _now_ms(),
            },
        )
        self._write(
            "chat.error",
            {"error": str(error), "message": str(message) if message is not None else None, "step_index": int(step_index), "action_key": str(action_key), "at_ms": _now_ms()},
        )
        self.status("error", current_step_index=int(step_index), current_action_key=str(action_key))

    def interrupt(self, interrupt: ChatGraphInterrupt) -> None:
        self._write("chat.interrupt", interrupt)

    def final(self, *, request: Dict[str, Any], response: Dict[str, Any]) -> None:
        self._write("chat.final", response)
        self.status("done")
        if self.emit_stages:
            _context_store.snapshot_run(project_id=self.project_id, run_id=self.run_id, request=request, response=response, meta={"workflow": "chat_act"})


def _planner_system_prompt() -> str:
    return """你是一个“写作编排器（planner）”，负责把用户的自然语言意图拆解为可执行的动作序列。

你必须输出严格的 JSON（不要输出其它文字）。

当用户意图不明确/信息不足时：你应该输出“澄清请求”，不要输出 steps：
{
  "needs_clarification": true,
  "clarifying_question": "一句话追问（中文）",
  "clarifying_options": ["选项1","选项2","选项3"]
}

当你能规划执行时：输出“执行计划”，结构如下：
{
  "intent_summary": "一句话总结用户想要什么（中文）",
  "steps": [
    {
      "action_key": "outline_generate|outline_optimize|generate_script|script_optimize|workflow_script|workflow_storyboard|memory_extract_changeset",
      "input_text": "可选；如留空，由系统根据上下文填充",
      "why": "可选；这一步的目的"
    }
  ],
  "final_action_key": "同上，表示最终要写入版本记录的 action"
}

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


def _build_planner_user(*, message: str, current_action_key: str, ui_ctx: Dict[str, Any], memory_context_text: str) -> str:
    master_script_preview = str((ui_ctx or {}).get("master_script") or "")
    selected_input_preview = str((ui_ctx or {}).get("current_input") or "")
    planner_user = f"""用户输入：
{message}

当前 UI tab（偏置参考，可为空）：
{(current_action_key or '(none)')}

当前 Master Script（预览，可能为空）：
{(master_script_preview or '(empty)')}

当前工作台输入（预览，可能为空）：
{(selected_input_preview or '(empty)')}
"""
    if memory_context_text:
        planner_user += f"\n\n记忆上下文（必须遵守，可能为空）：\n{memory_context_text}"
    return planner_user


async def _call_planner(*, planner_system: str, planner_user: str) -> str:
    raw = _read_settings_raw()
    settings = _mask_settings(raw)
    if not settings.has_api_key:
        raise HTTPException(status_code=400, detail="AI API Key 未配置")
    return await _chat_client.chat(
        settings=LlmChatSettings(
            base_url=settings.base_url,
            api_key=raw.get("api_key") or "",
            model=settings.model,
            temperature=min(float(settings.temperature or 0.2), 0.3),
            max_tokens=settings.max_tokens,
            timeout_seconds=settings.timeout_seconds,
        ),
        messages=[
            {"role": "system", "content": planner_system},
            {"role": "user", "content": planner_user},
        ],
    )


def _normalize_plan(plan: Dict[str, Any]) -> tuple[list[dict], str]:
    allowed = {
        "outline_generate",
        "outline_optimize",
        "generate_script",
        "script_optimize",
        "workflow_script",
        "workflow_storyboard",
        "memory_extract_changeset",
    }
    steps_raw = plan.get("steps")
    if not isinstance(steps_raw, list) or not steps_raw:
        raise HTTPException(status_code=422, detail="Planner steps 为空或格式不正确")
    steps: List[Dict[str, Any]] = []
    for it in steps_raw:
        if not isinstance(it, dict):
            continue
        ak = str(it.get("action_key") or "").strip()
        if ak not in allowed:
            continue
        steps.append(
            {
                "action_key": ak,
                "input_text": str(it.get("input_text") or "").strip(),
                "why": str(it.get("why") or "").strip(),
            }
        )
        if len(steps) >= 4:
            break
    if not steps:
        raise HTTPException(status_code=422, detail="Planner steps 无可用动作（action_key 不合法）")
    final_action_key = str(steps[-1].get("action_key") or "").strip()
    return steps, final_action_key


def _select_step_input(*, action_key: str, step_input_text: str, artifacts: Dict[str, Any], ui_ctx: Dict[str, Any], message: str) -> str:
    in_text = (step_input_text or "").strip()
    master_script_preview = str((ui_ctx or {}).get("master_script") or "")
    selected_input_preview = str((ui_ctx or {}).get("current_input") or "")

    if action_key == "outline_optimize" and isinstance(artifacts.get("outline"), str) and artifacts.get("outline").strip():
        in_text = artifacts["outline"]
    elif action_key == "script_optimize" and isinstance(artifacts.get("script"), str) and artifacts.get("script").strip():
        in_text = artifacts["script"]
    elif action_key == "generate_script" and isinstance(artifacts.get("outline"), str) and artifacts.get("outline").strip():
        in_text = artifacts["outline"]
    elif not in_text:
        in_text = selected_input_preview or master_script_preview or message

    if action_key in ("outline_optimize", "script_optimize"):
        in_text = f"{in_text}\n\n[用户意图]\n{message}"
    return in_text


async def _execute_action_step(
    *,
    action_key: str,
    in_text: str,
    project_id_pk: int,
    project_uuid: str,
    episode_id: Optional[int],
    run_id: str,
    artifacts: Dict[str, Any],
    cards: List[Dict[str, Any]],
    db: Session,
) -> str:
    if action_key == "outline_generate":
        resp = await outline_generate(OutlineGenerateRequest(text=in_text, project_id=project_id_pk, run_id=run_id))
        out_text = (resp.text or "").strip()
        artifacts["outline"] = out_text
        return out_text
    if action_key == "outline_optimize":
        resp = await outline_optimize(OutlineOptimizeRequest(text=in_text, project_id=project_id_pk, run_id=run_id))
        out_text = (resp.text or "").strip()
        artifacts["outline"] = out_text
        return out_text
    if action_key == "generate_script":
        resp = await generate_script(ScriptGenerateRequest(text=in_text, project_id=project_id_pk, run_id=run_id))
        out_text = (resp.text or "").strip()
        artifacts["script"] = out_text
        return out_text
    if action_key == "script_optimize":
        resp = await script_optimize(ScriptOptimizeRequest(text=in_text, project_id=project_id_pk, run_id=run_id))
        out_text = (resp.text or "").strip()
        artifacts["script"] = out_text
        return out_text
    if action_key == "workflow_script":
        wf_resp = await workflow_script(
            WorkflowScriptRequest(project_id=project_uuid, input_text=in_text, options=WorkflowScriptOptions(), run_id=run_id),
            db=db,
        )
        artifacts["series_bible"] = wf_resp.series_bible
        artifacts["beat_sheet"] = wf_resp.beat_sheet
        artifacts["script_fountain"] = wf_resp.script_fountain
        artifacts["qc_report"] = wf_resp.qc_report
        return str(wf_resp.script_fountain or "").strip()
    if action_key == "workflow_storyboard":
        wf_resp = await workflow_storyboard(
            WorkflowStoryboardRequest(project_id=project_uuid, scene_text=in_text, options=WorkflowStoryboardOptions(), run_id=run_id),
            db=db,
        )
        artifacts["shots"] = wf_resp.shots
        return json.dumps(wf_resp.shots, ensure_ascii=False, indent=2)
    if action_key == "memory_extract_changeset":
        from ..services.changeset_extractor import extract_changeset_v0_with_llm_with_trace
        from ..services.entity_resolver import resolve_changeset_entities_with_trace
        from ..services.evidence_ingestor import chunk_text_to_evidences
        from ..services.memory_store import get_memory_store

        raw = _read_settings_raw()
        settings = _mask_settings(raw)
        if not settings.has_api_key:
            raise HTTPException(status_code=400, detail="AI API Key 未配置")

        store = get_memory_store()
        base_text = ""
        if artifacts.get("script_fountain"):
            base_text += f"### script_fountain\n{artifacts.get('script_fountain')}\n\n"
        if artifacts.get("beat_sheet"):
            base_text += "### beat_sheet\n" + json.dumps(artifacts.get("beat_sheet"), ensure_ascii=False, indent=2) + "\n\n"
        if artifacts.get("series_bible"):
            base_text += "### series_bible\n" + json.dumps(artifacts.get("series_bible"), ensure_ascii=False, indent=2) + "\n\n"
        if not base_text:
            base_text = str((artifacts.get("script") or "") or in_text or "")
        evidences = chunk_text_to_evidences(
            project_id=project_id_pk,
            run_id=run_id,
            episode_id=int(episode_id) if episode_id else None,
            text=base_text,
            max_quote_chars=600,
            tags=["chat_act"],
        )
        evidence_ids: List[str] = []
        for ev in evidences:
            evidence_ids.append(store.upsert_evidence(ev))

        payload, extractor_trace = await extract_changeset_v0_with_llm_with_trace(
            llm_settings=LlmChatSettings(
                base_url=settings.base_url,
                api_key=raw.get("api_key") or "",
                model=settings.model,
                temperature=min(float(settings.temperature or 0.2), 0.3),
                max_tokens=settings.max_tokens,
                timeout_seconds=settings.timeout_seconds,
            ),
            project_id=project_id_pk,
            episode_id=int(episode_id) if episode_id else None,
            story_order_base=f"CH{str(episode_id or 1).zfill(2)}",
            evidences=[e.model_dump() for e in evidences],
        )
        payload, resolver_trace = resolve_changeset_entities_with_trace(store=store, project_id=project_id_pk, payload=payload)
        changeset_id = store.create_changeset(project_id=project_id_pk, payload=payload, episode_id=int(episode_id) if episode_id else None)
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
        return f"已生成待审阅变更单：{changeset_id}（evidence={len(evidence_ids)}）"
    raise HTTPException(status_code=400, detail=f"Unsupported action_key: {action_key}")


def _persist_if_needed(*, state: ChatGraphState, db: Session) -> Optional[Dict[str, Any]]:
    episode_id = state.get("episode_id")
    final_action_key = str(state.get("final_action_key") or "")
    if not episode_id:
        return None
    persistable = {"outline_generate", "generate_script", "script_optimize", "workflow_script", "workflow_storyboard"}
    if final_action_key not in persistable:
        return None

    artifacts = state.get("artifacts") or {}
    message = str(state.get("message") or "")
    plan_parsed = state.get("plan_parsed") or {}
    project_id_pk = int(state["project_id_pk"])
    run_id = str(state["run_id"])

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
        target_id=int(episode_id),
        action_key=str(persist_action_key),
        input_text=message,
        output_text=final_output,
        meta_data={
            "run_id": run_id,
            "source": "chat",
            "intent": plan_parsed.get("intent_summary"),
            "planner_final_action_key": final_action_key,
            "plan": plan_parsed if bool(state.get("debug")) else None,
        },
    )
    db.add(db_obj)
    db.commit()
    db.refresh(db_obj)
    return {"id": db_obj.id, "action_key": db_obj.action_key, "created_at": str(db_obj.created_at)}


def _build_response(*, state: ChatGraphState) -> Dict[str, Any]:
    plan_parsed = state.get("plan_parsed") or {}
    final_action_key = str(state.get("final_action_key") or "")
    intent = str(plan_parsed.get("intent_summary") or "执行完成")
    assistant_msg = str(state.get("assistant_message") or "") or f"已完成：{intent}（最终动作：{final_action_key}）"
    resp: Dict[str, Any] = {
        "run_id": str(state.get("run_id") or ""),
        "assistant_message": assistant_msg,
        "created_run": state.get("created_run"),
        "cards": (state.get("cards") or None) or None,
    }
    if state.get("debug"):
        resp["plan"] = plan_parsed or None
        resp["steps_trace"] = state.get("steps_trace") or None
        planner_system = str(state.get("planner_system") or "")
        planner_user = str(state.get("planner_user") or "")
        resp["planner_prompt"] = (planner_system + "\n\n---\n\n" + planner_user) if (planner_system or planner_user) else None
        resp["memory_trace"] = state.get("memory_trace") or None
        resp["planner_raw"] = state.get("planner_raw") or None
    return resp


def _build_start_graph(*, emitter: StageEmitter, db: Session) -> Any:
    async def precheck(state: ChatGraphState) -> ChatGraphState:
        message = (state.get("message") or "").strip()
        if not message:
            raise HTTPException(status_code=400, detail="message 不能为空")
        if _looks_ambiguous(message):
            state["assistant_message"] = "我需要你明确一下目标：你希望我对本集做什么？（可多选）"
            state["cards"] = [
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
            ]
            state["response"] = _build_response(state=state)
        return state

    def route_after_precheck(state: ChatGraphState) -> str:
        return "finalize" if state.get("response") else "retrieve_memory"

    async def retrieve_memory(state: ChatGraphState) -> ChatGraphState:
        memory_trace: List[Dict[str, Any]] = []
        memory_context_text = ""
        try:
            from ..services.memory_indexer import MemoryIndexer
            from ..services.memory_retriever import get_memory_retriever

            project_id_pk = int(state["project_id_pk"])
            message = str(state.get("message") or "")
            indexer = MemoryIndexer()
            indexer.index_series_bible(project_id=project_id_pk, version="v1")
            retriever = get_memory_retriever()
            retrieval_results = retriever.retrieve_for_task(project_id=project_id_pk, task_description=f"用户意图: {message[:200]}")
            memory_context_text = _build_memory_context(retrieval_results)
            if state.get("debug"):
                for k in ["L2_static", "L1", "L2_dynamic", "negative_constraints"]:
                    try:
                        memory_trace.append({"key": k, "text": (retriever.format_for_prompt(retrieval_results) or {}).get(k)})
                    except Exception:
                        memory_trace.append({"key": k, "text": None})
        except Exception:
            pass
        state["memory_trace"] = memory_trace
        state["memory_context_text"] = memory_context_text
        return state

    async def planner(state: ChatGraphState) -> ChatGraphState:
        planner_system = _planner_system_prompt()
        planner_user = _build_planner_user(
            message=str(state.get("message") or ""),
            current_action_key=str(state.get("current_action_key") or ""),
            ui_ctx=state.get("ui_context") or {},
            memory_context_text=str(state.get("memory_context_text") or ""),
        )
        planner_raw = await _call_planner(planner_system=planner_system, planner_user=planner_user)
        plan_parsed = safe_parse_json(planner_raw or "")
        if not isinstance(plan_parsed, dict):
            raise HTTPException(status_code=502, detail="Planner 输出无法解析为 JSON object")
        state["planner_system"] = planner_system
        state["planner_user"] = planner_user
        state["planner_raw"] = str(planner_raw or "")
        state["plan_parsed"] = plan_parsed

        if bool(plan_parsed.get("needs_clarification")):
            state["needs_clarification"] = True
            q = str(plan_parsed.get("clarifying_question") or "").strip() or "我需要你补充一下目标/范围：你希望我具体做什么？"
            opts = plan_parsed.get("clarifying_options")
            options = [{"label": str(x), "value": str(x)} for x in (opts[:8] if isinstance(opts, list) else []) if str(x).strip()]
            state["assistant_message"] = q
            state["cards"] = [
                {
                    "type": "clarify_intent",
                    "title": "需要澄清",
                    "options": options or None,
                    "hint": "你也可以直接用一句话说明：例如“先提取大纲，再优化节奏，然后生成剧本，最后提交入库变更给我确认”。",
                }
            ]
            state["response"] = _build_response(state=state)
            emitter.plan(plan_parsed)
            return state

        steps, final_action_key = _normalize_plan(plan_parsed)
        state["needs_clarification"] = False
        state["steps"] = steps
        state["final_action_key"] = final_action_key
        emitter.plan({"intent_summary": plan_parsed.get("intent_summary"), "steps": steps, "final_action_key": final_action_key})
        return state

    def route_after_planner(state: ChatGraphState) -> str:
        if state.get("response"):
            return "finalize"
        return "init_exec"

    async def init_exec(state: ChatGraphState) -> ChatGraphState:
        state["artifacts"] = state.get("artifacts") or {}
        state["steps_trace"] = state.get("steps_trace") or []
        state["cards"] = state.get("cards") or []
        state["step_index"] = int(state.get("step_index") or 0)
        return state

    async def run_step(state: ChatGraphState) -> ChatGraphState:
        steps = state.get("steps") or []
        idx = int(state.get("step_index") or 0)
        if idx >= len(steps):
            return state
        step = steps[idx] or {}
        ak = str(step.get("action_key") or "").strip()
        why = str(step.get("why") or "").strip()
        artifacts = state.get("artifacts")
        if not isinstance(artifacts, dict):
            artifacts = {}
            state["artifacts"] = artifacts
        in_text = _select_step_input(
            action_key=ak,
            step_input_text=str(step.get("input_text") or ""),
            artifacts=artifacts,
            ui_ctx=state.get("ui_context") or {},
            message=str(state.get("message") or ""),
        )
        emitter.step_start(step_index=idx, action_key=ak, why=why or None, input_preview=in_text)
        t0 = _now_ms()
        cards = state.get("cards")
        if not isinstance(cards, list):
            cards = []
            state["cards"] = cards
        try:
            out_text = await asyncio.wait_for(
                _execute_action_step(
                    action_key=ak,
                    in_text=in_text,
                    project_id_pk=int(state["project_id_pk"]),
                    project_uuid=str(state["project_id"] or ""),
                    episode_id=state.get("episode_id"),
                    run_id=str(state["run_id"] or ""),
                    artifacts=artifacts,
                    cards=cards,
                    db=db,
                ),
                timeout=600.0,
            )
        except asyncio.TimeoutError:
            emitter.step_error(step_index=idx, action_key=ak, error="timeout", message=f"Step '{ak}' execution timed out")
            raise _Abort("timeout")
        except Exception as e:
            emitter.step_error(step_index=idx, action_key=ak, error=f"{type(e).__name__}: {e}")
            raise _Abort(f"{type(e).__name__}: {e}")
        dt = _now_ms() - t0
        emitter.step_end(step_index=idx, action_key=ak, ms=int(dt), output_preview=out_text)
        if state.get("debug"):
            steps_trace = state.get("steps_trace")
            if not isinstance(steps_trace, list):
                steps_trace = []
                state["steps_trace"] = steps_trace
            steps_trace.append(
                {
                    "step_index": idx,
                    "action_key": ak,
                    "input_text": in_text,
                    "output_text_preview": _trim_preview(out_text, 500),
                    "ms": int(dt),
                }
            )
        state["step_index"] = idx + 1
        if ak == "memory_extract_changeset":
            cards = state.get("cards") or []
            csid = ""
            for c in reversed(cards):
                if isinstance(c, dict) and c.get("type") == "review_changeset" and c.get("changeset_id"):
                    csid = str(c.get("changeset_id") or "")
                    break
            if csid:
                resume_state = {
                    k: v
                    for k, v in state.items()
                    if k
                    in {
                        "project_id_pk",
                        "project_id",
                        "episode_id",
                        "run_id",
                        "message",
                        "current_action_key",
                        "ui_context",
                        "debug",
                        "memory_trace",
                        "memory_context_text",
                        "planner_system",
                        "planner_user",
                        "planner_raw",
                        "plan_parsed",
                        "steps",
                        "final_action_key",
                        "artifacts",
                        "steps_trace",
                        "cards",
                        "step_index",
                    }
                }
                state["interrupt"] = {"kind": "review_changeset", "changeset_id": csid, "resume_state": resume_state}
        return state

    def route_after_step(state: ChatGraphState) -> str:
        if state.get("interrupt"):
            return "interrupt_finalize"
        steps = state.get("steps") or []
        idx = int(state.get("step_index") or 0)
        return "run_step" if idx < len(steps) else "persist"

    async def persist(state: ChatGraphState) -> ChatGraphState:
        state["created_run"] = _persist_if_needed(state=state, db=db)
        return state

    async def interrupt_finalize(state: ChatGraphState) -> ChatGraphState:
        interrupt = state.get("interrupt") or None
        if interrupt:
            emitter.interrupt(interrupt)
        state["assistant_message"] = str(state.get("assistant_message") or "") or "已生成待审阅变更单，请先确认或驳回。"
        state["response"] = _build_response(state=state)
        return state

    async def finalize(state: ChatGraphState) -> ChatGraphState:
        if not state.get("response"):
            state["response"] = _build_response(state=state)
        return state

    g = StateGraph(ChatGraphState)
    g.add_node("precheck", precheck)
    g.add_node("retrieve_memory", retrieve_memory)
    g.add_node("planner", planner)
    g.add_node("init_exec", init_exec)
    g.add_node("run_step", run_step)
    g.add_node("persist", persist)
    g.add_node("interrupt_finalize", interrupt_finalize)
    g.add_node("finalize", finalize)

    g.set_entry_point("precheck")
    g.add_conditional_edges("precheck", route_after_precheck, {"retrieve_memory": "retrieve_memory", "finalize": "finalize"})
    g.add_edge("retrieve_memory", "planner")
    g.add_conditional_edges("planner", route_after_planner, {"init_exec": "init_exec", "finalize": "finalize"})
    g.add_edge("init_exec", "run_step")
    g.add_conditional_edges(
        "run_step",
        route_after_step,
        {"run_step": "run_step", "persist": "persist", "interrupt_finalize": "interrupt_finalize"},
    )
    g.add_edge("persist", "finalize")
    g.add_edge("interrupt_finalize", "finalize")
    g.add_edge("finalize", END)
    return g.compile()


def _build_resume_graph(*, emitter: StageEmitter, db: Session) -> Any:
    async def run_step(state: ChatGraphState) -> ChatGraphState:
        steps = state.get("steps") or []
        idx = int(state.get("step_index") or 0)
        if idx >= len(steps):
            return state
        step = steps[idx] or {}
        ak = str(step.get("action_key") or "").strip()
        why = str(step.get("why") or "").strip()
        artifacts = state.get("artifacts")
        if not isinstance(artifacts, dict):
            artifacts = {}
            state["artifacts"] = artifacts
        in_text = _select_step_input(
            action_key=ak,
            step_input_text=str(step.get("input_text") or ""),
            artifacts=artifacts,
            ui_ctx=state.get("ui_context") or {},
            message=str(state.get("message") or ""),
        )
        emitter.step_start(step_index=idx, action_key=ak, why=why or None, input_preview=in_text)
        t0 = _now_ms()
        cards = state.get("cards")
        if not isinstance(cards, list):
            cards = []
            state["cards"] = cards
        try:
            out_text = await asyncio.wait_for(
                _execute_action_step(
                    action_key=ak,
                    in_text=in_text,
                    project_id_pk=int(state["project_id_pk"]),
                    project_uuid=str(state["project_id"] or ""),
                    episode_id=state.get("episode_id"),
                    run_id=str(state["run_id"] or ""),
                    artifacts=artifacts,
                    cards=cards,
                    db=db,
                ),
                timeout=600.0,
            )
        except asyncio.TimeoutError:
            emitter.step_error(step_index=idx, action_key=ak, error="timeout", message=f"Step '{ak}' execution timed out")
            raise _Abort("timeout")
        except Exception as e:
            emitter.step_error(step_index=idx, action_key=ak, error=f"{type(e).__name__}: {e}")
            raise _Abort(f"{type(e).__name__}: {e}")
        dt = _now_ms() - t0
        emitter.step_end(step_index=idx, action_key=ak, ms=int(dt), output_preview=out_text)
        if state.get("debug"):
            steps_trace = state.get("steps_trace")
            if not isinstance(steps_trace, list):
                steps_trace = []
                state["steps_trace"] = steps_trace
            steps_trace.append(
                {
                    "step_index": idx,
                    "action_key": ak,
                    "input_text": in_text,
                    "output_text_preview": _trim_preview(out_text, 500),
                    "ms": int(dt),
                }
            )
        state["step_index"] = idx + 1
        return state

    def route_after_step(state: ChatGraphState) -> str:
        steps = state.get("steps") or []
        idx = int(state.get("step_index") or 0)
        return "run_step" if idx < len(steps) else "persist"

    async def persist(state: ChatGraphState) -> ChatGraphState:
        state["created_run"] = _persist_if_needed(state=state, db=db)
        return state

    async def finalize(state: ChatGraphState) -> ChatGraphState:
        if not state.get("assistant_message"):
            state["assistant_message"] = "已继续执行并完成。"
        state["response"] = _build_response(state=state)
        return state

    g = StateGraph(ChatGraphState)
    g.add_node("run_step", run_step)
    g.add_node("persist", persist)
    g.add_node("finalize", finalize)
    g.set_entry_point("run_step")
    g.add_conditional_edges("run_step", route_after_step, {"run_step": "run_step", "persist": "persist"})
    g.add_edge("persist", "finalize")
    g.add_edge("finalize", END)
    return g.compile()


async def run_chat_graph(*, req: Any, db: Session, emit_stages: bool, run_id: str) -> Dict[str, Any]:
    run_id = (str(run_id or "") or str(getattr(req, "run_id", "") or "")).strip()
    if not run_id:
        raise HTTPException(status_code=400, detail="run_id 不能为空")

    project_id = str(getattr(req, "project_id", "") or "")
    project_id_pk = resolve_project_pk(db, project_id)
    episode_id = getattr(req, "episode_id", None)
    current_action_key = str(getattr(req, "current_action_key", "") or "").strip()
    message = str(getattr(req, "message", "") or "").strip()
    ui_ctx = getattr(req, "ui_context", None) or {}
    debug = bool(getattr(req, "debug", False))

    emitter = StageEmitter(project_id=project_id_pk, run_id=run_id, emit_stages=emit_stages)
    emitter.status("running")

    init_state: ChatGraphState = {
        "project_id_pk": int(project_id_pk),
        "project_id": project_id,
        "episode_id": int(episode_id) if episode_id is not None else None,
        "run_id": run_id,
        "message": message,
        "current_action_key": current_action_key,
        "ui_context": ui_ctx,
        "debug": debug,
        "cards": [],
        "steps_trace": [],
        "artifacts": {},
        "step_index": 0,
    }

    graph = _build_start_graph(emitter=emitter, db=db)
    try:
        out_state = await graph.ainvoke(init_state)
    except _Abort:
        return {"run_id": run_id, "assistant_message": "执行失败", "cards": None}

    resp = out_state.get("response") or _build_response(state=out_state)
    emitter.final(request=getattr(req, "model_dump", lambda: dict(req))(), response=resp)
    return resp


def _read_interrupt_state(*, project_id_pk: int, run_id: str) -> Optional[ChatGraphInterrupt]:
    path = _context_store.stage_path(project_id=int(project_id_pk), run_id=str(run_id), stage_name="chat.interrupt")
    try:
        with open(path, "r", encoding="utf-8") as f:
            obj = json.load(f)
        if isinstance(obj, dict) and obj.get("resume_state") and obj.get("kind"):
            return obj  # type: ignore[return-value]
    except Exception:
        return None
    return None


async def resume_chat_graph(*, project_id_pk: int, run_id: str, decision: str, db: Session) -> None:
    interrupt = _read_interrupt_state(project_id_pk=int(project_id_pk), run_id=str(run_id))
    if not interrupt:
        return
    resume_state = interrupt.get("resume_state")
    if not isinstance(resume_state, dict):
        return
    state: ChatGraphState = dict(resume_state)  # type: ignore[assignment]
    state["assistant_message"] = f"已收到你的决定：{decision}，继续执行中。"
    state["interrupt"] = None
    artifacts = state.get("artifacts") or {}
    artifacts["changeset_review"] = str(decision)
    state["artifacts"] = artifacts

    emitter = StageEmitter(project_id=int(project_id_pk), run_id=str(run_id), emit_stages=True)
    graph = _build_resume_graph(emitter=emitter, db=db)
    try:
        out_state = await graph.ainvoke(state)
    except _Abort:
        return

    resp = out_state.get("response") or _build_response(state=out_state)
    emitter.final(request=_context_store.read_run(int(project_id_pk), str(run_id))["request"], response=resp)  # type: ignore[index]


def resume_chat_graph_in_thread(*, project_id_pk: int, run_id: str, decision: str) -> None:
    def _runner() -> None:
        db = SessionLocal()
        try:
            import anyio

            async def _go() -> None:
                await resume_chat_graph(project_id_pk=int(project_id_pk), run_id=str(run_id), decision=str(decision), db=db)

            anyio.run(_go)
        finally:
            try:
                db.close()
            except Exception:
                pass

    threading.Thread(target=_runner, daemon=True).start()

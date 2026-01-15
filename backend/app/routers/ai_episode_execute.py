import asyncio
import json
import threading
import time
from typing import Any, Dict, Optional

from fastapi import APIRouter, Depends, HTTPException
from pydantic import BaseModel
from sqlalchemy.orm import Session

from .. import models
from ..database import SessionLocal, get_db
from ..services import prompt_registry
from ..services.json_extract import extract_json_any
from ..services.llm_client import LlmChatSettings
from ..services.project_lookup import resolve_project_pk
from ..services.context_store import new_run_id
from ..services.memory_store import get_memory_store
from .ai_shared import _build_memory_context, _chat_client, _context_store, _mask_settings, _read_settings_raw, _repair_json_with_same_agent
from ..services.chat_graph import StageEmitter


router = APIRouter(tags=["AI (DeepSeek)"])


def _now_ms() -> int:
    return int(time.time() * 1000)


class EpisodeExecuteStartRequest(BaseModel):
    project_id: str
    episode_id: int
    script_text: str
    run_id: Optional[str] = None


class EpisodeExecuteStartResponse(BaseModel):
    run_id: str
    status: str = "queued"


class EpisodeExecuteConfirmRequest(BaseModel):
    decision: str
    artifacts: Optional[Dict[str, Any]] = None
    run_id: str


def _read_interrupt_state(*, project_id_pk: int, run_id: str) -> Optional[Dict[str, Any]]:
    path = _context_store.stage_path(project_id=int(project_id_pk), run_id=str(run_id), stage_name="chat.interrupt")
    if not path:
        return None
    try:
        if not isinstance(path, str):
            return None
        import os

        if not os.path.exists(path):
            return None
        with open(path, "r", encoding="utf-8") as f:
            data = json.load(f)
        return data if isinstance(data, dict) else None
    except Exception:
        return None


def _write_interrupt_state(*, project_id_pk: int, run_id: str, interrupt: Dict[str, Any]) -> None:
    _context_store.snapshot_stage(project_id=int(project_id_pk), run_id=str(run_id), stage_name="chat.interrupt", data=interrupt)


async def _llm_json(
    *,
    system_prompt: str,
    user_prompt: str,
    project_id_pk: int,
    run_id: str,
    raw_stage: str,
) -> Dict[str, Any]:
    raw = _read_settings_raw()
    settings = _mask_settings(raw)
    if not settings.has_api_key:
        raise HTTPException(status_code=400, detail="AI API Key 未配置")

    content = await _chat_client.chat(
        settings=LlmChatSettings(
            base_url=settings.base_url,
            api_key=raw.get("api_key") or "",
            model=settings.model,
            temperature=min(float(settings.temperature or 0.2), 0.3),
            max_tokens=settings.max_tokens,
            timeout_seconds=settings.timeout_seconds,
        ),
        messages=[
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_prompt},
        ],
    )
    _context_store.snapshot_stage(project_id=project_id_pk, run_id=run_id, stage_name=raw_stage, data={"text": content})
    parsed = extract_json_any(content)
    if isinstance(parsed, dict):
        return parsed
    repaired = await _repair_json_with_same_agent(
        llm_settings=LlmChatSettings(
            base_url=settings.base_url,
            api_key=raw.get("api_key") or "",
            model=settings.model,
            temperature=0.0,
            max_tokens=settings.max_tokens,
            timeout_seconds=settings.timeout_seconds,
        ),
        system_prompt=system_prompt,
        bad_output_text=str(content),
        error_hint="Root type is not JSON object",
        expected_hint="JSON object",
    )
    if not isinstance(repaired, dict):
        raise HTTPException(status_code=422, detail="LLM output must be a JSON object")
    return repaired


def _ensure_episode_belongs_to_project(*, db: Session, project_id_pk: int, episode_id: int) -> models.Episode:
    ep = db.query(models.Episode).filter(models.Episode.id == int(episode_id)).first()
    if not ep or int(ep.project_id) != int(project_id_pk):
        raise HTTPException(status_code=404, detail="Episode not found")
    return ep


def _set_episode_exec_state(
    *,
    db: Session,
    project_id_pk: int,
    episode_id: int,
    script_locked: Optional[bool] = None,
    last_exec_run_id: Optional[str] = None,
    exec_status: Optional[str] = None,
    exec_artifacts: Optional[Dict[str, Any]] = None,
) -> None:
    ep = _ensure_episode_belongs_to_project(db=db, project_id_pk=project_id_pk, episode_id=episode_id)
    if script_locked is not None and hasattr(ep, "script_locked"):
        ep.script_locked = bool(script_locked)
        if bool(script_locked) and hasattr(ep, "script_locked_at") and not ep.script_locked_at:
            try:
                from datetime import datetime, timezone

                ep.script_locked_at = datetime.now(timezone.utc)
            except Exception:
                pass
    if last_exec_run_id is not None and hasattr(ep, "last_exec_run_id"):
        ep.last_exec_run_id = str(last_exec_run_id)
    if exec_status is not None and hasattr(ep, "exec_status"):
        ep.exec_status = str(exec_status)
    if exec_artifacts is not None and hasattr(ep, "exec_artifacts"):
        ep.exec_artifacts = exec_artifacts
    db.add(ep)
    db.commit()


async def _run_until_interrupt(
    *,
    project_id_pk: int,
    project_uuid: str,
    episode_id: int,
    run_id: str,
    script_text: str,
    start_step_index: int,
    artifacts: Dict[str, Any],
    emitter: StageEmitter,
    db: Session,
) -> None:
    steps = [
        {"action_key": "episode_outline_generate", "why": "从本集剧本生成大纲"},
        {"action_key": "episode_assets_visual_dna", "why": "抽离资产并生成视觉DNA"},
        {"action_key": "episode_split_episodes", "why": "按长度进行剧集分割并生成每集大纲"},
        {"action_key": "episode_asset_ingest", "why": "执行资产入库（生成并应用变更）"},
    ]
    emitter.plan({"intent_summary": "执行本集剧本流水线", "steps": steps, "final_action_key": "episode_asset_ingest"})

    step_index = int(start_step_index)
    while step_index < len(steps):
        ak = str(steps[step_index]["action_key"])
        why = str(steps[step_index].get("why") or "")

        if ak == "episode_outline_generate":
            emitter.step_start(step_index=step_index, action_key=ak, why=why or None, input_preview=script_text)
            t0 = _now_ms()
            system_prompt = prompt_registry.get_template_prompt("outline_generate_system")
            raw = _read_settings_raw()
            settings = _mask_settings(raw)
            if not settings.has_api_key:
                raise HTTPException(status_code=400, detail="AI API Key 未配置")
            outline_text = await _chat_client.chat(
                settings=LlmChatSettings(
                    base_url=settings.base_url,
                    api_key=raw.get("api_key") or "",
                    model=settings.model,
                    temperature=settings.temperature,
                    max_tokens=settings.max_tokens,
                    timeout_seconds=settings.timeout_seconds,
                ),
                messages=[
                    {"role": "system", "content": system_prompt},
                    {"role": "user", "content": script_text},
                ],
            )
            outline_text = (outline_text or "").strip()
            artifacts["outline"] = outline_text
            emitter.step_end(step_index=step_index, action_key=ak, ms=_now_ms() - t0, output_preview=outline_text)
            _set_episode_exec_state(
                db=db,
                project_id_pk=project_id_pk,
                episode_id=episode_id,
                exec_status="waiting_outline_confirm",
                exec_artifacts=artifacts,
            )
            emitter.status("paused", waiting_for="confirm_outline")
            _write_interrupt_state(
                project_id_pk=project_id_pk,
                run_id=run_id,
                interrupt={
                    "kind": "confirm_outline",
                    "resume_state": {
                        "project_id_pk": project_id_pk,
                        "project_id": project_uuid,
                        "episode_id": episode_id,
                        "run_id": run_id,
                        "script_text": script_text,
                        "step_index": step_index + 1,
                        "artifacts": artifacts,
                    },
                },
            )
            return

        if ak == "episode_assets_visual_dna":
            in_text = "\n\n".join(
                [
                    "### script",
                    script_text,
                    "",
                    "### outline",
                    str(artifacts.get("outline") or ""),
                ]
            ).strip()
            emitter.step_start(step_index=step_index, action_key=ak, why=why or None, input_preview=in_text)
            t0 = _now_ms()
            system_prompt = prompt_registry.get_template_prompt("episode_assets_visual_dna_system")
            user_prompt = in_text
            parsed = await _llm_json(
                system_prompt=system_prompt,
                user_prompt=user_prompt,
                project_id_pk=project_id_pk,
                run_id=run_id,
                raw_stage="episode_assets_visual_dna.raw",
            )
            artifacts["assets_visual_dna"] = parsed
            out_preview = json.dumps(parsed, ensure_ascii=False)[:800]
            emitter.step_end(step_index=step_index, action_key=ak, ms=_now_ms() - t0, output_preview=out_preview)
            _set_episode_exec_state(
                db=db,
                project_id_pk=project_id_pk,
                episode_id=episode_id,
                exec_status="waiting_assets_confirm",
                exec_artifacts=artifacts,
            )
            emitter.status("paused", waiting_for="confirm_assets")
            _write_interrupt_state(
                project_id_pk=project_id_pk,
                run_id=run_id,
                interrupt={
                    "kind": "confirm_assets",
                    "resume_state": {
                        "project_id_pk": project_id_pk,
                        "project_id": project_uuid,
                        "episode_id": episode_id,
                        "run_id": run_id,
                        "script_text": script_text,
                        "step_index": step_index + 1,
                        "artifacts": artifacts,
                    },
                },
            )
            return

        if ak == "episode_split_episodes":
            in_text = "\n\n".join(
                [
                    "### script",
                    script_text,
                    "",
                    "### outline",
                    str(artifacts.get("outline") or ""),
                    "",
                    "### assets_visual_dna",
                    json.dumps(artifacts.get("assets_visual_dna") or {}, ensure_ascii=False, indent=2),
                ]
            ).strip()
            emitter.step_start(step_index=step_index, action_key=ak, why=why or None, input_preview=in_text)
            t0 = _now_ms()
            system_prompt = prompt_registry.get_template_prompt("episode_split_episodes_system")
            parsed = await _llm_json(
                system_prompt=system_prompt,
                user_prompt=in_text,
                project_id_pk=project_id_pk,
                run_id=run_id,
                raw_stage="episode_split_episodes.raw",
            )
            artifacts["split_episodes"] = parsed
            out_preview = json.dumps(parsed, ensure_ascii=False)[:800]
            emitter.step_end(step_index=step_index, action_key=ak, ms=_now_ms() - t0, output_preview=out_preview)
            _set_episode_exec_state(
                db=db,
                project_id_pk=project_id_pk,
                episode_id=episode_id,
                exec_status="waiting_split_confirm",
                exec_artifacts=artifacts,
            )
            emitter.status("paused", waiting_for="confirm_split")
            _write_interrupt_state(
                project_id_pk=project_id_pk,
                run_id=run_id,
                interrupt={
                    "kind": "confirm_split",
                    "resume_state": {
                        "project_id_pk": project_id_pk,
                        "project_id": project_uuid,
                        "episode_id": episode_id,
                        "run_id": run_id,
                        "script_text": script_text,
                        "step_index": step_index + 1,
                        "artifacts": artifacts,
                    },
                },
            )
            return

        if ak == "episode_asset_ingest":
            in_text = "\n\n".join(
                [
                    "### script",
                    script_text,
                    "",
                    "### outline",
                    str(artifacts.get("outline") or ""),
                    "",
                    "### assets_visual_dna",
                    json.dumps(artifacts.get("assets_visual_dna") or {}, ensure_ascii=False, indent=2),
                    "",
                    "### split_episodes",
                    json.dumps(artifacts.get("split_episodes") or {}, ensure_ascii=False, indent=2),
                ]
            ).strip()
            emitter.step_start(step_index=step_index, action_key=ak, why=why or None, input_preview=in_text)
            t0 = _now_ms()
            raw = _read_settings_raw()
            settings = _mask_settings(raw)
            if not settings.has_api_key:
                raise HTTPException(status_code=400, detail="AI API Key 未配置")
            store = get_memory_store()
            from ..services.changeset_extractor import extract_changeset_v0_with_llm_with_trace
            from ..services.entity_resolver import resolve_changeset_entities_with_trace
            from ..services.evidence_ingestor import chunk_text_to_evidences

            evidences = chunk_text_to_evidences(
                project_id=project_id_pk,
                run_id=run_id,
                episode_id=int(episode_id),
                text=in_text,
                max_quote_chars=600,
                tags=["episode_execute"],
            )
            evidence_ids = []
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
                episode_id=int(episode_id),
                story_order_base=f"CH{str(episode_id).zfill(2)}",
                evidences=[e.model_dump() for e in evidences],
            )
            payload, resolver_trace = resolve_changeset_entities_with_trace(store=store, project_id=project_id_pk, payload=payload)
            changeset_id = store.create_changeset(project_id=project_id_pk, payload=payload, episode_id=int(episode_id))
            try:
                store.append_changeset_review_entry(changeset_id=changeset_id, entry={"at_ms": _now_ms(), "action": "extractor_trace", "data": extractor_trace})
            except Exception:
                pass
            try:
                store.append_changeset_review_entry(changeset_id=changeset_id, entry={"at_ms": _now_ms(), "action": "resolver_trace", "data": resolver_trace})
            except Exception:
                pass
            artifacts["changeset_id"] = changeset_id
            artifacts["ingest_preview"] = {
                "changeset_id": changeset_id,
                "evidence_count": len(evidence_ids),
                "counts": {
                    "entities": len(payload.get("entities") or []) if isinstance(payload.get("entities"), list) else 0,
                    "events": len(payload.get("events") or []) if isinstance(payload.get("events"), list) else 0,
                    "state_changes": len(payload.get("state_changes") or []) if isinstance(payload.get("state_changes"), list) else 0,
                    "snapshots": len(payload.get("snapshots") or []) if isinstance(payload.get("snapshots"), list) else 0,
                    "conflicts": len(payload.get("conflicts") or []) if isinstance(payload.get("conflicts"), list) else 0,
                },
            }
            out_preview = json.dumps(artifacts["ingest_preview"], ensure_ascii=False)
            emitter.step_end(step_index=step_index, action_key=ak, ms=_now_ms() - t0, output_preview=out_preview)
            _set_episode_exec_state(
                db=db,
                project_id_pk=project_id_pk,
                episode_id=episode_id,
                exec_status="waiting_ingest_confirm",
                exec_artifacts=artifacts,
            )
            emitter.status("paused", waiting_for="confirm_ingest")
            _write_interrupt_state(
                project_id_pk=project_id_pk,
                run_id=run_id,
                interrupt={
                    "kind": "confirm_ingest",
                    "changeset_id": changeset_id,
                    "resume_state": {
                        "project_id_pk": project_id_pk,
                        "project_id": project_uuid,
                        "episode_id": episode_id,
                        "run_id": run_id,
                        "script_text": script_text,
                        "step_index": step_index + 1,
                        "artifacts": artifacts,
                    },
                },
            )
            return

        raise HTTPException(status_code=400, detail=f"Unsupported step: {ak}")


def _apply_split_to_db(*, db: Session, project_id_pk: int, split_payload: Dict[str, Any]) -> None:
    episodes = split_payload.get("episodes")
    if not isinstance(episodes, list):
        return
    max_order = db.query(models.Episode).filter(models.Episode.project_id == int(project_id_pk)).count()
    order_base = int(max_order) + 1
    for idx, it in enumerate(episodes):
        if not isinstance(it, dict):
            continue
        title = str(it.get("title") or f"新建第{idx+1}集").strip()
        outline = str(it.get("outline") or "").strip()
        script_segment = str(it.get("script") or "").strip()
        ep = models.Episode(project_id=int(project_id_pk), title=title, order=order_base + idx, description=script_segment or None)
        if hasattr(ep, "exec_artifacts"):
            ep.exec_artifacts = {"outline": outline} if outline else {}
        db.add(ep)
    db.commit()


def _apply_changeset(*, project_id_pk: int, changeset_id: str) -> None:
    store = get_memory_store()
    store.apply_changeset(changeset_id=changeset_id, reviewer="human", note="episode_execute")


@router.post("/episode-execute/act_async", response_model=EpisodeExecuteStartResponse)
def episode_execute_act_async(req: EpisodeExecuteStartRequest):
    run_id = (req.run_id or "").strip() or new_run_id()
    script_text = (req.script_text or "").strip()
    if not script_text:
        raise HTTPException(status_code=400, detail="script_text 不能为空")
    project_uuid = (req.project_id or "").strip()
    if not project_uuid:
        raise HTTPException(status_code=400, detail="project_id 不能为空")

    db = SessionLocal()
    try:
        project_id_pk = resolve_project_pk(db, project_uuid)
        _set_episode_exec_state(
            db=db,
            project_id_pk=project_id_pk,
            episode_id=int(req.episode_id),
            script_locked=True,
            last_exec_run_id=run_id,
            exec_status="running",
            exec_artifacts={"script_text": script_text},
        )
    finally:
        try:
            db.close()
        except Exception:
            pass

    _context_store.snapshot_stage(project_id=project_id_pk, run_id=run_id, stage_name="chat.status", data={"status": "queued", "at_ms": _now_ms()})

    payload = {"project_id_pk": project_id_pk, "project_uuid": project_uuid, "episode_id": int(req.episode_id), "run_id": run_id, "script_text": script_text}

    def _runner():
        db2 = SessionLocal()
        try:
            emitter = StageEmitter(project_id=project_id_pk, run_id=run_id, emit_stages=True)

            async def _go():
                emitter.status("running")
                await _run_until_interrupt(
                    project_id_pk=project_id_pk,
                    project_uuid=project_uuid,
                    episode_id=int(req.episode_id),
                    run_id=run_id,
                    script_text=script_text,
                    start_step_index=0,
                    artifacts={"script_text": script_text},
                    emitter=emitter,
                    db=db2,
                )

            import anyio

            anyio.run(_go)
        except Exception as e:
            _context_store.snapshot_stage(project_id=project_id_pk, run_id=run_id, stage_name="chat.error", data={"error": str(e), "at_ms": _now_ms()})
            _context_store.snapshot_stage(project_id=project_id_pk, run_id=run_id, stage_name="chat.status", data={"status": "error", "at_ms": _now_ms()})
            try:
                _set_episode_exec_state(db=db2, project_id_pk=project_id_pk, episode_id=int(req.episode_id), exec_status="error")
            except Exception:
                pass
        finally:
            try:
                db2.close()
            except Exception:
                pass

    threading.Thread(target=_runner, daemon=True).start()
    return EpisodeExecuteStartResponse(run_id=run_id, status="queued")


@router.post("/episode-execute/{episode_id}/confirm")
def episode_execute_confirm(episode_id: int, req: EpisodeExecuteConfirmRequest, db: Session = Depends(get_db)):
    run_id = (req.run_id or "").strip()
    if not run_id:
        raise HTTPException(status_code=400, detail="run_id 不能为空")
    decision = (req.decision or "").strip()
    if decision not in {"confirmed", "regenerate", "rejected"}:
        raise HTTPException(status_code=400, detail="decision must be confirmed|regenerate|rejected")

    project_id_pk = None
    try:
        ep = db.query(models.Episode).filter(models.Episode.id == int(episode_id)).first()
        if not ep:
            raise HTTPException(status_code=404, detail="Episode not found")
        project_id_pk = int(ep.project_id)
    except HTTPException:
        raise
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))

    interrupt = _read_interrupt_state(project_id_pk=project_id_pk, run_id=run_id)
    if not interrupt:
        raise HTTPException(status_code=409, detail="No interrupt state for this run_id")
    resume_state = interrupt.get("resume_state")
    if not isinstance(resume_state, dict):
        raise HTTPException(status_code=409, detail="Invalid resume_state")
    kind = str(interrupt.get("kind") or "")

    artifacts = resume_state.get("artifacts")
    if not isinstance(artifacts, dict):
        artifacts = {}
    if isinstance(req.artifacts, dict):
        artifacts.update(req.artifacts)
    resume_state["artifacts"] = artifacts

    if decision == "rejected":
        _context_store.snapshot_stage(project_id=project_id_pk, run_id=run_id, stage_name="chat.status", data={"status": "done", "at_ms": _now_ms()})
        _context_store.snapshot_stage(
            project_id=project_id_pk,
            run_id=run_id,
            stage_name="chat.final",
            data={"run_id": run_id, "assistant_message": "已结束本次执行（已驳回）。"},
        )
        _set_episode_exec_state(db=db, project_id_pk=project_id_pk, episode_id=int(episode_id), exec_status="done", exec_artifacts=artifacts)
        return {"status": "done"}

    try:
        _set_episode_exec_state(
            db=db,
            project_id_pk=project_id_pk,
            episode_id=int(episode_id),
            exec_status="running",
            exec_artifacts=artifacts,
        )
    except Exception:
        pass
    try:
        _write_interrupt_state(project_id_pk=project_id_pk, run_id=run_id, interrupt={"kind": None, "cleared": True, "at_ms": _now_ms()})
    except Exception:
        pass

    def _runner():
        db2 = SessionLocal()
        try:
            emitter = StageEmitter(project_id=project_id_pk, run_id=run_id, emit_stages=True)

            async def _go():
                emitter.status("running")
                next_step_index = int(resume_state.get("step_index") or 0)
                script_text = str(resume_state.get("script_text") or "")
                project_uuid = str(resume_state.get("project_id") or "")

                if kind == "confirm_outline" and decision == "regenerate":
                    next_step_index = 0
                if kind == "confirm_assets" and decision == "regenerate":
                    next_step_index = 1
                if kind == "confirm_split" and decision == "confirmed":
                    split_payload = artifacts.get("split_episodes")
                    if isinstance(split_payload, dict):
                        _apply_split_to_db(db=db2, project_id_pk=project_id_pk, split_payload=split_payload)
                if kind == "confirm_ingest" and decision == "confirmed":
                    csid = str(artifacts.get("changeset_id") or interrupt.get("changeset_id") or "")
                    if csid:
                        _apply_changeset(project_id_pk=project_id_pk, changeset_id=csid)
                    emitter.status("done")
                    _context_store.snapshot_stage(
                        project_id=project_id_pk,
                        run_id=run_id,
                        stage_name="chat.final",
                        data={"run_id": run_id, "assistant_message": "已完成执行：资产已入库。"},
                    )
                    _set_episode_exec_state(db=db2, project_id_pk=project_id_pk, episode_id=int(episode_id), exec_status="done", exec_artifacts=artifacts)
                    _context_store.snapshot_run(
                        project_id=project_id_pk,
                        run_id=run_id,
                        request={"episode_id": int(episode_id), "run_id": run_id},
                        response={"status": "done", "artifacts": artifacts},
                        meta={"workflow": "episode_execute"},
                    )
                    return

                await _run_until_interrupt(
                    project_id_pk=project_id_pk,
                    project_uuid=project_uuid,
                    episode_id=int(episode_id),
                    run_id=run_id,
                    script_text=script_text,
                    start_step_index=next_step_index,
                    artifacts=artifacts,
                    emitter=emitter,
                    db=db2,
                )

            import anyio

            anyio.run(_go)
        except Exception as e:
            _context_store.snapshot_stage(project_id=project_id_pk, run_id=run_id, stage_name="chat.error", data={"error": str(e), "at_ms": _now_ms()})
            _context_store.snapshot_stage(project_id=project_id_pk, run_id=run_id, stage_name="chat.status", data={"status": "error", "at_ms": _now_ms()})
            try:
                _set_episode_exec_state(db=db2, project_id_pk=project_id_pk, episode_id=int(episode_id), exec_status="error")
            except Exception:
                pass
        finally:
            try:
                db2.close()
            except Exception:
                pass

    threading.Thread(target=_runner, daemon=True).start()
    return {"status": "queued"}

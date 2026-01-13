from __future__ import annotations

from fastapi import APIRouter, Depends, HTTPException
from pydantic import BaseModel
from typing import Any, Dict, List, Optional

from sqlalchemy.orm import Session

from ..database import get_db
from ..services.memory_store import get_memory_store
from ..services.evidence_ingestor import chunk_text_to_evidences
from ..services.changeset_extractor import extract_changeset_v0_with_llm_with_trace
from ..services.entity_resolver import resolve_changeset_entities_with_trace
from ..services.llm_client import LlmChatSettings
from ..services.app_paths import ai_settings_path
from ..services.project_lookup import resolve_project_pk
from ..workflows.memory_schemas import TimeBlock, TimeConstraint

import logging
import os
log_dir = os.path.join(os.path.dirname(__file__), "..", "logs")
os.makedirs(log_dir, exist_ok=True)
log_file = os.path.join(log_dir, "memory.log")

logger = logging.getLogger(__name__)
logger.setLevel(logging.DEBUG)
handler = logging.FileHandler(log_file)
handler.setLevel(logging.DEBUG)
formatter = logging.Formatter('%(asctime)s - %(name)s - %(levelname)s - %(message)s')
handler.setFormatter(formatter)
logger.addHandler(handler)


router = APIRouter(prefix="/memory", tags=["Memory (记忆工程)"])



class UpsertTimeConstraintRequest(BaseModel):
    constraint: TimeConstraint


class UpsertTimeConstraintResponse(BaseModel):
    id: str


class ListTimeConstraintsResponse(BaseModel):
    items: List[Dict[str, Any]]


@router.post("/time-constraint", response_model=UpsertTimeConstraintResponse)
def upsert_time_constraint(payload: UpsertTimeConstraintRequest):
    store = get_memory_store()
    try:
        cid = store.upsert_time_constraint(payload.constraint)
        return UpsertTimeConstraintResponse(id=cid)
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"upsert_time_constraint failed: {e}")


@router.get("/time-constraints", response_model=ListTimeConstraintsResponse)
def list_time_constraints(
    project_id: str,
    status: Optional[str] = None,
    limit: int = 200,
    db: Session = Depends(get_db),
):
    store = get_memory_store()
    try:
        pid = resolve_project_pk(db, project_id)
        items = store.list_time_constraints(project_id=pid, status=status, limit=limit)
        return ListTimeConstraintsResponse(items=items)
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"list_time_constraints failed: {e}")


class UpsertTimeBlockRequest(BaseModel):
    block: TimeBlock


class UpsertTimeBlockResponse(BaseModel):
    id: str


class ListTimeBlocksResponse(BaseModel):
    items: List[Dict[str, Any]]


@router.post("/time-block", response_model=UpsertTimeBlockResponse)
def upsert_time_block(payload: UpsertTimeBlockRequest):
    store = get_memory_store()
    try:
        bid = store.upsert_time_block(payload.block)
        return UpsertTimeBlockResponse(id=bid)
    except Exception as e:
        logger.error(f"[Memory] Upsert Time Block Error: {e}")
        raise HTTPException(status_code=500, detail=f"upsert_time_block failed: {e}")


@router.get("/time-blocks", response_model=ListTimeBlocksResponse)
def list_time_blocks(
    project_id: str,
    status: Optional[str] = None,
    limit: int = 200,
    db: Session = Depends(get_db),
):
    store = get_memory_store()
    try:
        pid = resolve_project_pk(db, project_id)
        items = store.list_time_blocks(project_id=pid, status=status, limit=limit)
        return ListTimeBlocksResponse(items=items)
    except Exception as e:
        logger.error(f"[Memory] List Time Blocks Error: {e}")
        raise HTTPException(status_code=500, detail=f"list_time_blocks failed: {e}")


# ==========================
# ReviewConsole：ChangeSet / Conflict
# ==========================


class CreateChangeSetRequest(BaseModel):
    project_id: str
    episode_id: Optional[int] = None
    payload: Dict[str, Any] = {}


class CreateChangeSetResponse(BaseModel):
    changeset_id: str


@router.post("/changeset", response_model=CreateChangeSetResponse)
def create_changeset(req: CreateChangeSetRequest, db: Session = Depends(get_db)):
    store = get_memory_store()
    try:
        pid = resolve_project_pk(db, req.project_id)
        cid = store.create_changeset(
            project_id=pid,
            episode_id=req.episode_id,
            payload=req.payload,
        )
        return CreateChangeSetResponse(changeset_id=cid)
    except Exception as e:
        logger.error(f"[Memory] Create Change Set Error: {e}")
        raise HTTPException(status_code=500, detail=f"create_changeset failed: {e}")


class ListChangeSetsResponse(BaseModel):
    items: List[Dict[str, Any]]


@router.get("/changesets", response_model=ListChangeSetsResponse)
def list_changesets(
    project_id: str,
    review_status: Optional[str] = None,
    limit: int = 50,
    db: Session = Depends(get_db),
):
    store = get_memory_store()
    try:
        pid = resolve_project_pk(db, project_id)
        items = store.list_changesets(project_id=pid, review_status=review_status, limit=limit)
        return ListChangeSetsResponse(items=items)
    except Exception as e:
        logger.error(f"[Memory] List Change Sets Error: {e}")
        raise HTTPException(status_code=500, detail=f"list_changesets failed: {e}")


class GetChangeSetResponse(BaseModel):
    changeset: Dict[str, Any]


@router.get("/changeset/{changeset_id}", response_model=GetChangeSetResponse)
def get_changeset(changeset_id: str):
    store = get_memory_store()
    cs = store.get_changeset(changeset_id)
    if not cs:
        raise HTTPException(status_code=404, detail="ChangeSet not found")
    return GetChangeSetResponse(changeset=cs)


class ApplyChangeSetRequest(BaseModel):
    reviewer: str = "human"
    note: Optional[str] = None


@router.post("/changeset/{changeset_id}/approve")
def approve_changeset(changeset_id: str, req: ApplyChangeSetRequest):
    store = get_memory_store()
    try:
        store.apply_changeset(changeset_id=changeset_id, reviewer=req.reviewer, note=req.note)
        return {"message": "approved"}
    except ValueError:
        logger.error(f"[Memory] Change Set Not Found: {changeset_id}")
        raise HTTPException(status_code=404, detail="ChangeSet not found")
    except Exception as e:
        logger.error(f"[Memory] Approve Change Set Error: {e}")
        raise HTTPException(status_code=500, detail=f"approve_changeset failed: {e}")


@router.post("/changeset/{changeset_id}/reject")
def reject_changeset(changeset_id: str, req: ApplyChangeSetRequest):
    store = get_memory_store()
    try:
        store.reject_changeset(changeset_id=changeset_id, reviewer=req.reviewer, note=req.note)
        return {"message": "rejected"}
    except Exception as e:
        logger.error(f"[Memory] Reject Change Set Error: {e}")
        raise HTTPException(status_code=500, detail=f"reject_changeset failed: {e}")


class CreateConflictRequest(BaseModel):
    project_id: str
    conflict_type: str
    changeset_id: Optional[str] = None
    entity_id: Optional[str] = None
    old_claim: Optional[Dict[str, Any]] = None
    new_claim: Optional[Dict[str, Any]] = None
    suggested_actions: Optional[List[Dict[str, Any]]] = None


class CreateConflictResponse(BaseModel):
    conflict_id: str


@router.post("/conflict", response_model=CreateConflictResponse)
def create_conflict(req: CreateConflictRequest, db: Session = Depends(get_db)):
    store = get_memory_store()
    try:
        pid = resolve_project_pk(db, req.project_id)
        conflict_id = store.create_conflict(
            project_id=pid,
            conflict_type=req.conflict_type,
            changeset_id=req.changeset_id,
            entity_id=req.entity_id,
            old_claim=req.old_claim,
            new_claim=req.new_claim,
            suggested_actions=req.suggested_actions,
        )
        return CreateConflictResponse(conflict_id=conflict_id)
    except Exception as e:
        logger.error(f"[Memory] Create Conflict Error: {e}")
        raise HTTPException(status_code=500, detail=f"create_conflict failed: {e}")


class ListConflictsResponse(BaseModel):
    items: List[Dict[str, Any]]


@router.get("/conflicts", response_model=ListConflictsResponse)
def list_conflicts(
    project_id: str,
    status: Optional[str] = "open",
    limit: int = 100,
    db: Session = Depends(get_db),
):
    store = get_memory_store()
    try:
        pid = resolve_project_pk(db, project_id)
        items = store.list_conflicts(project_id=pid, status=status, limit=limit)
        return ListConflictsResponse(items=items)
    except Exception as e:
        logger.error(f"[Memory] List Conflicts Error: {e}")
        raise HTTPException(status_code=500, detail=f"list_conflicts failed: {e}")


class ResolveConflictRequest(BaseModel):
    resolved_by: str = "human"
    resolution_note: Optional[str] = None
    status: str = "resolved"


@router.post("/conflict/{conflict_id}/resolve")
def resolve_conflict(conflict_id: str, req: ResolveConflictRequest):
    store = get_memory_store()
    try:
        store.resolve_conflict(
            conflict_id=conflict_id,
            resolved_by=req.resolved_by,
            resolution_note=req.resolution_note,
            status=req.status,
        )
        return {"message": "resolved"}
    except Exception as e:
        logger.error(f"[Memory] Resolve Conflict Error: {e}")
        raise HTTPException(status_code=500, detail=f"resolve_conflict failed: {e}")


# ==========================
# Evidence-first：章节原文切片入库
# ==========================


class EvidenceIngestRequest(BaseModel):
    project_id: str
    episode_id: Optional[int] = None
    scene_id: Optional[int] = None
    text: str
    max_quote_chars: int = 600
    tags: List[str] = []


class EvidenceIngestResponse(BaseModel):
    evidence_ids: List[str]
    count: int


@router.post("/evidence/ingest", response_model=EvidenceIngestResponse)
def ingest_evidence(req: EvidenceIngestRequest, db: Session = Depends(get_db)):
    store = get_memory_store()
    try:
        pid = resolve_project_pk(db, req.project_id)
        evidences = chunk_text_to_evidences(
            project_id=pid,
            text=req.text,
            episode_id=req.episode_id,
            scene_id=req.scene_id,
            max_quote_chars=int(req.max_quote_chars or 600),
            tags=req.tags or [],
        )
        ids: List[str] = []
        for ev in evidences:
            ids.append(store.upsert_evidence(ev))
        return EvidenceIngestResponse(evidence_ids=ids, count=len(ids))
    except Exception as e:
        logger.error(f"[Memory] Ingest Evidence Error: {e}")
        raise HTTPException(status_code=500, detail=f"ingest_evidence failed: {e}")


# ==========================
# ChangeSet：payload v0 统一入口（校验 + 默认值）
# ==========================


class CreateChangeSetFromPayloadRequest(BaseModel):
    project_id: str
    episode_id: Optional[int] = None
    payload: Dict[str, Any] = {}
    default_materialize_static_bible: bool = True


class CreateChangeSetFromPayloadResponse(BaseModel):
    changeset_id: str


@router.post("/changeset/from-payload", response_model=CreateChangeSetFromPayloadResponse)
def create_changeset_from_payload(req: CreateChangeSetFromPayloadRequest, db: Session = Depends(get_db)):
    store = get_memory_store()
    try:
        pid = resolve_project_pk(db, req.project_id)
        payload = dict(req.payload or {})
        # 最小校验：schema_version
        schema_version = str(payload.get("schema_version") or "").strip()
        if schema_version and schema_version != "changeset.v0":
            raise HTTPException(status_code=400, detail=f"Unsupported schema_version: {schema_version}")
        if not schema_version:
            payload["schema_version"] = "changeset.v0"

        # 强制对齐 project_id / episode_id
        payload["project_id"] = int(pid)
        if req.episode_id is not None:
            payload["episode_id"] = int(req.episode_id)

        # 默认 materialize：避免 canonical 不进入生成上下文
        materialize = payload.get("materialize")
        if not isinstance(materialize, dict):
            materialize = {}
        if req.default_materialize_static_bible and "write_static_bible" not in materialize:
            materialize["write_static_bible"] = True
        payload["materialize"] = materialize

        cid = store.create_changeset(project_id=int(pid), payload=payload, episode_id=req.episode_id)
        return CreateChangeSetFromPayloadResponse(changeset_id=cid)
    except HTTPException:
        raise
    except Exception as e:
        logger.error(f"[Memory] Create Change Set From Payload Error: {e}")
        raise HTTPException(status_code=500, detail=f"create_changeset_from_payload failed: {e}")


# ==========================
# A：LLM 抽取 changeset.v0（从 Evidence 生成）
# ==========================


class ExtractChangeSetRequest(BaseModel):
    project_id: str
    episode_id: Optional[int] = None
    # 二选一：
    evidence_ids: List[str] = []
    text: Optional[str] = None
    # 如果传 text，会先自动切片入库
    ingest_max_quote_chars: int = 600
    ingest_tags: List[str] = []
    # 输出策略
    story_order_base: Optional[str] = None
    create_changeset: bool = True
    debug: bool = False


class ExtractChangeSetResponse(BaseModel):
    changeset_id: Optional[str] = None
    payload: Dict[str, Any]
    evidence_ids: List[str]
    trace: Optional[Dict[str, Any]] = None


def _read_ai_settings_raw() -> Dict[str, Any]:
    path = ai_settings_path()
    try:
        import os as _os
        if not _os.path.exists(path):
            return {}
    except Exception:
        return {}
    try:
        import json as _json
        with open(path, "r", encoding="utf-8") as f:
            return _json.load(f) or {}
    except Exception:
        return {}


@router.post("/extract/changeset", response_model=ExtractChangeSetResponse)
async def extract_changeset(req: ExtractChangeSetRequest, db: Session = Depends(get_db)):
    """
    用 LLM 从 Evidence 抽取 changeset.v0。
    覆盖实体类型：character/location/organization/prop，并生成激进 snapshot 切片。

    - 如果传 text：会先自动切片入库，得到 evidence_ids
    - 如果传 evidence_ids：直接读取 canonical_evidences 内容作为输入
    - create_changeset=true 时：会自动写入 canonical_changesets（待审阅），并返回 changeset_id
    """
    store = get_memory_store()
    raw = _read_ai_settings_raw()
    api_key = str(raw.get("api_key") or "").strip()
    if not api_key:
        raise HTTPException(status_code=400, detail="AI API Key 未配置（先在 AI 设置里配置）")

    base_url = str(raw.get("base_url") or "https://api.deepseek.com").strip()
    model = str(raw.get("model") or "deepseek-chat").strip()
    temperature = float(raw.get("temperature") or 0.2)
    max_tokens_val = raw.get("max_tokens")
    # 如果配置中没有 max_tokens 或为 0，则返回 None（不限制）
    max_tokens = None if max_tokens_val is None or max_tokens_val == 0 else int(max_tokens_val)
    timeout_seconds = float(raw.get("timeout_seconds") or 120.0)

    llm_settings = LlmChatSettings(
        base_url=base_url,
        api_key=api_key,
        model=model,
        temperature=min(temperature, 0.3),
        max_tokens=max_tokens,  # None 表示不限制
        timeout_seconds=timeout_seconds,
    )

    project_id = int(resolve_project_pk(db, req.project_id))
    episode_id = int(req.episode_id) if req.episode_id is not None else None

    # 1) 获取/生成 evidence_ids
    evidence_ids: List[str] = []
    if (req.text or "").strip():
        evidences = chunk_text_to_evidences(
            project_id=project_id,
            text=str(req.text or ""),
            episode_id=episode_id,
            scene_id=None,
            max_quote_chars=int(req.ingest_max_quote_chars or 600),
            tags=req.ingest_tags or [],
        )
        for ev in evidences:
            evidence_ids.append(store.upsert_evidence(ev))
    else:
        evidence_ids = [str(x).strip() for x in (req.evidence_ids or []) if str(x).strip()]

    if not evidence_ids:
        logger.error(f"[Memory] No evidence found for project_id: {project_id}, evidence_ids: {evidence_ids}")
        raise HTTPException(status_code=400, detail="必须提供 text 或 evidence_ids")

    # 2) 读取 evidence 内容
    evidence_rows = store.list_evidences_by_ids(project_id=project_id, evidence_ids=evidence_ids)
    if not evidence_rows:
        logger.error(f"[Memory] No evidence found for project_id: {project_id}, evidence_ids: {evidence_ids}")
        raise HTTPException(status_code=404, detail="未找到任何 evidence（请确认 evidence_ids 属于该 project_id）")

    # 3) story_order_base
    story_order_base = (req.story_order_base or "").strip()
    if not story_order_base:
        # 默认按 episode_id 推导；缺失则 CH01
        story_order_base = f"CH{str(episode_id or 1).zfill(2)}"

    # 4) 调用 LLM 抽取 payload
    payload, extractor_trace = await extract_changeset_v0_with_llm_with_trace(
        llm_settings=llm_settings,
        project_id=project_id,
        episode_id=episode_id,
        story_order_base=story_order_base,
        evidences=evidence_rows,
    )

    # 4.5) A1：实体归一（强降噪）
    resolver_trace = None
    try:
        payload, resolver_trace = resolve_changeset_entities_with_trace(store=store, project_id=project_id, payload=payload)
    except Exception:
        logger.error(f"[Memory] Resolve Change Set Entities Error: {e}")
        resolver_trace = {"resolver_version": "entity_resolver.unknown", "error": "resolver_failed"}

    # 5) 可选：写入 changeset（待审阅）
    changeset_id: Optional[str] = None
    if bool(req.create_changeset):
        changeset_id = store.create_changeset(project_id=project_id, payload=payload, episode_id=episode_id)
        # A4：把 trace 写入 changeset.review_log_json（便于回放/审计）
        try:
            store.append_changeset_review_entry(
                changeset_id=changeset_id,
                entry={"at_ms": int(time.time() * 1000), "action": "extractor_trace", "data": extractor_trace},
            )
        except Exception as e:
            pass
        try:
            store.append_changeset_review_entry(
                changeset_id=changeset_id,
                entry={"at_ms": int(time.time() * 1000), "action": "resolver_trace", "data": resolver_trace},
            )
        except Exception:
            pass

    trace = None
    if bool(req.debug):
        trace = {"extractor": extractor_trace, "resolver": resolver_trace}
    return ExtractChangeSetResponse(changeset_id=changeset_id, payload=payload, evidence_ids=evidence_ids, trace=trace)


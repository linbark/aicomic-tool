from __future__ import annotations

from fastapi import APIRouter, HTTPException
from pydantic import BaseModel
from typing import Any, Dict, List, Optional

from ..services.memory_store import get_memory_store
from ..workflows.memory_schemas import TimeBlock, TimeConstraint


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
    project_id: int,
    status: Optional[str] = None,
    limit: int = 200,
):
    store = get_memory_store()
    try:
        items = store.list_time_constraints(project_id=project_id, status=status, limit=limit)
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
        raise HTTPException(status_code=500, detail=f"upsert_time_block failed: {e}")


@router.get("/time-blocks", response_model=ListTimeBlocksResponse)
def list_time_blocks(
    project_id: int,
    status: Optional[str] = None,
    limit: int = 200,
):
    store = get_memory_store()
    try:
        items = store.list_time_blocks(project_id=project_id, status=status, limit=limit)
        return ListTimeBlocksResponse(items=items)
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"list_time_blocks failed: {e}")


# ==========================
# ReviewConsole：ChangeSet / Conflict
# ==========================


class CreateChangeSetRequest(BaseModel):
    project_id: int
    episode_id: Optional[int] = None
    payload: Dict[str, Any] = {}


class CreateChangeSetResponse(BaseModel):
    changeset_id: str


@router.post("/changeset", response_model=CreateChangeSetResponse)
def create_changeset(req: CreateChangeSetRequest):
    store = get_memory_store()
    try:
        cid = store.create_changeset(
            project_id=req.project_id,
            episode_id=req.episode_id,
            payload=req.payload,
        )
        return CreateChangeSetResponse(changeset_id=cid)
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"create_changeset failed: {e}")


class ListChangeSetsResponse(BaseModel):
    items: List[Dict[str, Any]]


@router.get("/changesets", response_model=ListChangeSetsResponse)
def list_changesets(
    project_id: int,
    review_status: Optional[str] = None,
    limit: int = 50,
):
    store = get_memory_store()
    try:
        items = store.list_changesets(project_id=project_id, review_status=review_status, limit=limit)
        return ListChangeSetsResponse(items=items)
    except Exception as e:
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
        raise HTTPException(status_code=404, detail="ChangeSet not found")
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"approve_changeset failed: {e}")


@router.post("/changeset/{changeset_id}/reject")
def reject_changeset(changeset_id: str, req: ApplyChangeSetRequest):
    store = get_memory_store()
    try:
        store.reject_changeset(changeset_id=changeset_id, reviewer=req.reviewer, note=req.note)
        return {"message": "rejected"}
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"reject_changeset failed: {e}")


class CreateConflictRequest(BaseModel):
    project_id: int
    conflict_type: str
    changeset_id: Optional[str] = None
    entity_id: Optional[str] = None
    old_claim: Optional[Dict[str, Any]] = None
    new_claim: Optional[Dict[str, Any]] = None
    suggested_actions: Optional[List[Dict[str, Any]]] = None


class CreateConflictResponse(BaseModel):
    conflict_id: str


@router.post("/conflict", response_model=CreateConflictResponse)
def create_conflict(req: CreateConflictRequest):
    store = get_memory_store()
    try:
        conflict_id = store.create_conflict(
            project_id=req.project_id,
            conflict_type=req.conflict_type,
            changeset_id=req.changeset_id,
            entity_id=req.entity_id,
            old_claim=req.old_claim,
            new_claim=req.new_claim,
            suggested_actions=req.suggested_actions,
        )
        return CreateConflictResponse(conflict_id=conflict_id)
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"create_conflict failed: {e}")


class ListConflictsResponse(BaseModel):
    items: List[Dict[str, Any]]


@router.get("/conflicts", response_model=ListConflictsResponse)
def list_conflicts(
    project_id: int,
    status: Optional[str] = "open",
    limit: int = 100,
):
    store = get_memory_store()
    try:
        items = store.list_conflicts(project_id=project_id, status=status, limit=limit)
        return ListConflictsResponse(items=items)
    except Exception as e:
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
        raise HTTPException(status_code=500, detail=f"resolve_conflict failed: {e}")


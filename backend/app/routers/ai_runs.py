from __future__ import annotations

from datetime import datetime
from typing import Any, Dict, List, Optional

from fastapi import APIRouter, Depends, HTTPException, Query
from pydantic import BaseModel, Field
from sqlalchemy.orm import Session

from ..database import get_db
from .. import models


router = APIRouter(prefix="/ai", tags=["AI Runs (按钮输出历史)"])


class AiActionRunRead(BaseModel):
    id: int
    project_id: int
    target_type: str
    target_id: int
    action_key: str
    input_text: Optional[str] = None
    output_text: str
    meta_data: Optional[Dict[str, Any]] = None
    created_at: datetime

    class Config:
        from_attributes = True


class AiActionRunCreate(BaseModel):
    project_id: int
    target_type: str = Field(default="episode")
    target_id: int
    action_key: str
    input_text: Optional[str] = None
    output_text: str
    meta_data: Optional[Dict[str, Any]] = None


@router.get("/runs", response_model=List[AiActionRunRead])
def list_ai_runs(
    project_id: int = Query(...),
    episode_id: Optional[int] = Query(default=None),
    action_key: Optional[str] = Query(default=None),
    limit: int = Query(default=50, ge=1, le=500),
    db: Session = Depends(get_db),
):
    q = db.query(models.AiActionRun).filter(models.AiActionRun.project_id == int(project_id))
    if episode_id is not None:
        q = q.filter(models.AiActionRun.target_type == "episode", models.AiActionRun.target_id == int(episode_id))
    if action_key:
        q = q.filter(models.AiActionRun.action_key == str(action_key))
    return q.order_by(models.AiActionRun.created_at.desc()).limit(int(limit)).all()


@router.post("/runs", response_model=AiActionRunRead)
def create_ai_run(payload: AiActionRunCreate, db: Session = Depends(get_db)):
    if not (payload.action_key or "").strip():
        raise HTTPException(status_code=400, detail="action_key 不能为空")
    if not (payload.output_text or "").strip():
        raise HTTPException(status_code=400, detail="output_text 不能为空")
    if payload.target_type != "episode":
        # 先只允许 episode（按计划范围）
        raise HTTPException(status_code=400, detail="target_type 暂仅支持 episode")

    db_obj = models.AiActionRun(
        project_id=int(payload.project_id),
        target_type="episode",
        target_id=int(payload.target_id),
        action_key=str(payload.action_key),
        input_text=payload.input_text,
        output_text=payload.output_text,
        meta_data=payload.meta_data or None,
    )
    db.add(db_obj)
    db.commit()
    db.refresh(db_obj)
    return db_obj



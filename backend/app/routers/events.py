from fastapi import APIRouter, Depends, HTTPException
from sqlalchemy.orm import Session
from typing import List
from .. import models, schemas
from ..database import get_db

router = APIRouter(
    prefix="/events",
    tags=["Event System (事件系统)"]
)

# 获取项目的所有事件
@router.get("/project/{project_id}", response_model=List[schemas.EventRead])
def get_project_events(project_id: int, db: Session = Depends(get_db)):
    return db.query(models.Event).filter(models.Event.project_id == project_id).all()

# 创建事件
@router.post("/project/{project_id}", response_model=schemas.EventRead)
def create_event(project_id: int, event: schemas.EventCreate, db: Session = Depends(get_db)):
    db_event = models.Event(**event.dict(), project_id=project_id)
    db.add(db_event)
    db.commit()
    db.refresh(db_event)
    return db_event

# 以后这里会增加 create_event_node 等接口 (Phase 2)
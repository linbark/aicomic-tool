from fastapi import APIRouter, Depends, HTTPException
from sqlalchemy.orm import Session
from typing import List
from .. import models, schemas
from ..database import get_db
from typing import Dict, Any

router = APIRouter(
    prefix="/events",
    tags=["Event System (事件系统)"]
)

# 获取项目的所有事件
@router.get("/project/{project_id}", response_model=List[schemas.EventRead])
def get_project_events(project_id: int, db: Session = Depends(get_db)):
    from sqlalchemy.orm import joinedload
    return db.query(models.Event)\
              .options(joinedload(models.Event.nodes))\
              .filter(models.Event.project_id == project_id).all()

# 创建事件
@router.post("/project/{project_id}", response_model=schemas.EventRead)
def create_event(project_id: int, event: schemas.EventCreate, db: Session = Depends(get_db)):
    db_event = models.Event(**event.dict(), project_id=project_id)
    db.add(db_event)
    db.commit()
    db.refresh(db_event)
    return db_event

@router.post("/nodes/{event_id}", response_model=schemas.EventNodeRead)
def upsert_event_node(
    event_id: int, 
    node_data: schemas.EventNodeUpdate, 
    db: Session = Depends(get_db)
):
    # 1. 查找是否存在已有节点
    existing_node = db.query(models.EventNode).filter(
        models.EventNode.event_id == event_id,
        models.EventNode.target_type == node_data.target_type,
        models.EventNode.target_id == node_data.target_id
    ).first()

    if existing_node:
        # 更新
        existing_node.description = node_data.description
        db.commit()
        db.refresh(existing_node)
        return existing_node
    else:
        # 新建
        new_node = models.EventNode(
            event_id=event_id,
            target_type=node_data.target_type,
            target_id=node_data.target_id,
            description=node_data.description
        )
        db.add(new_node)
        db.commit()
        db.refresh(new_node)
        return new_node


# 更新事件详情
@router.patch("/{event_id}", response_model=schemas.EventRead)
def update_event(event_id: int, event_update: schemas.EventUpdate, db: Session = Depends(get_db)):
    db_event = db.query(models.Event).filter(models.Event.id == event_id).first()
    if not db_event:
        raise HTTPException(status_code=404, detail="Event not found")

    if event_update.name is not None:
        db_event.name = event_update.name
    if event_update.description is not None:
        db_event.description = event_update.description
    if event_update.graph_data is not None:
        db_event.graph_data = event_update.graph_data
    if event_update.color is not None:
        db_event.color = event_update.color

    db.commit()
    db.refresh(db_event)
    return db_event

@router.get("/matrix/{project_id}")
def get_event_matrix(project_id: int, db: Session = Depends(get_db)):
    """
    聚合查询：返回该项目下所有事件、以及所有事件关联的节点
    前端拿到后，自己在内存里组装矩阵
    """
    # 1. 获取所有事件
    events = db.query(models.Event).filter(models.Event.project_id == project_id).all()
    
    # 2. 获取所有节点 (通过 join 优化性能)
    # 逻辑：找出属于这些 events 的所有 nodes
    event_ids = [e.id for e in events]
    nodes = db.query(models.EventNode).filter(models.EventNode.event_id.in_(event_ids)).all()
    
    return {
        "events": events, # 包含 id, name, color
        "nodes": nodes    # 包含 target_type, target_id, description, event_id
    }
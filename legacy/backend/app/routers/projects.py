from fastapi import APIRouter, Depends, HTTPException, Query
from sqlalchemy.orm import Session, joinedload
from typing import List, Optional
from pydantic import BaseModel
from .. import models, schemas
from ..database import get_db
import os
from ..models import Character, Asset, Project
from ..services.app_paths import project_root_dir, data_dir
from ..services.project_lookup import resolve_project, resolve_project_pk, ensure_project_uuid

import logging

log_dir = os.path.join(os.path.dirname(__file__), "..", "logs")
os.makedirs(log_dir, exist_ok=True)
log_file = os.path.join(log_dir, "projects.log")

logger = logging.getLogger(__name__)
logger.setLevel(logging.DEBUG)
handler = logging.FileHandler(log_file)
handler.setLevel(logging.DEBUG)
formatter = logging.Formatter('%(asctime)s - %(name)s - %(levelname)s - %(message)s')
handler.setFormatter(formatter)
logger.addHandler(handler)

router = APIRouter(
    prefix="/projects",
    tags=["Projects (项目与人设)"]
)

# =======================
# Pydantic 模型
# =======================
class ProjectCreate(BaseModel):
    name: str
    description: Optional[str] = None

class ProjectUpdate(BaseModel):
    name: Optional[str] = None
    description: Optional[str] = None

class CharacterCreate(BaseModel):
    name: str
    description: Optional[str] = None
    base_prompt: Optional[str] = None
    category: Optional[str] = "persona_visual"

class CharacterUpdate(BaseModel):
    name: Optional[str] = None
    description: Optional[str] = None
    base_prompt: Optional[str] = None
    avatar_path: Optional[str] = None 
    category: Optional[str] = None

# =======================
# 1. 项目管理接口
# =======================

@router.get("/", response_model=List[schemas.ProjectBase])
def get_projects(db: Session = Depends(get_db)):
    rows = db.query(models.Project).all()
    dirty = False
    out = []
    for p in rows:
        if not (getattr(p, "uuid", None) or "").strip():
            p.uuid = __import__("uuid").uuid4().hex
            dirty = True
        out.append({"id": str(p.uuid), "name": p.name, "description": p.description})
    if dirty:
        db.commit()
    return out

@router.post("/", response_model=schemas.ProjectBase)
def create_project(project: ProjectCreate, db: Session = Depends(get_db)):
    import json, os
    try:
        logger.info(f"[Projects][create_project] create_project called: {project.name}, {project.description}")
    except: 
        logging.error(f"[Projects][create_project] create_project called: {project.name}, {project.description}")
    try:
        db_project = models.Project(name=project.name, description=project.description)
        db.add(db_project)
        db.commit()
        db.refresh(db_project)
        ensure_project_uuid(db, db_project)
        try:
            logger.info(f"[Projects][create_project] create_project success: {db_project.id}, {db_project.name}")
        except: pass
        return {"id": str(db_project.uuid), "name": db_project.name, "description": db_project.description}
    except Exception as e:
        logger.error(f"[Projects][create_project] create_project error: {e}")
        raise

# 【新增】修改项目 (重命名)
@router.patch("/{project_id}", response_model=schemas.ProjectBase)
def update_project(project_id: str, project_update: ProjectUpdate, db: Session = Depends(get_db)):
    db_project = resolve_project(db, project_id)
    
    if project_update.name is not None:
        db_project.name = project_update.name
    if project_update.description is not None:
        db_project.description = project_update.description
    
    db.commit()
    db.refresh(db_project)
    ensure_project_uuid(db, db_project)
    return {"id": str(db_project.uuid), "name": db_project.name, "description": db_project.description}

# 【新增】删除项目
@router.delete("/{project_id}")
def delete_project(project_id: str, db: Session = Depends(get_db)):
    db_project = resolve_project(db, project_id)
    
    project_name = db_project.name
    base_data_dir = data_dir()
    project_dir = os.path.join(base_data_dir, project_name)
    pid = int(db_project.id)
    app_project_dir = project_root_dir(pid)
    
    # 删除项目文件夹（如果存在）
    if os.path.exists(project_dir):
        import shutil
        try:
            shutil.rmtree(project_dir)
            logger.info(f"[Projects][delete_project] Deleted project directory: {project_dir}")
        except Exception as e:
            logger.error(f"[Projects][delete_project] Error deleting project directory {project_dir}: {e}")

    # 删除 app_data_dir/projects/{project_id}（context / runs 等）
    if os.path.exists(app_project_dir):
        import shutil
        try:
            shutil.rmtree(app_project_dir)
            logger.info(f"[Projects][delete_project] Deleted app project directory: {app_project_dir}")
        except Exception as e:
            logger.error(f"[Projects][delete_project] Error deleting app project directory {app_project_dir}: {e}")
    
    # 删除数据库记录（显式清理所有 project_id 关联数据，避免 SQLite 未启用外键级联导致残留）
    try:
        # 子查询：episodes / scenes / shots ids
        ep_ids_q = db.query(models.Episode.id).filter(models.Episode.project_id == pid)
        sc_ids_q = db.query(models.Scene.id).filter(models.Scene.episode_id.in_(ep_ids_q))
        shot_ids_q = db.query(models.Shot.id).filter(models.Shot.scene_id.in_(sc_ids_q))

        # Asset: 通过 character 或 shot 关联到项目
        char_ids_q = db.query(models.Character.id).filter(models.Character.project_id == pid)
        deleted_assets = (
            db.query(models.Asset)
            .filter((models.Asset.character_id.in_(char_ids_q)) | (models.Asset.shot_id.in_(shot_ids_q)))
            .delete(synchronize_session=False)
        )
        deleted_shots = db.query(models.Shot).filter(models.Shot.id.in_(shot_ids_q)).delete(synchronize_session=False)
        deleted_scenes = db.query(models.Scene).filter(models.Scene.id.in_(sc_ids_q)).delete(synchronize_session=False)
        deleted_episodes = db.query(models.Episode).filter(models.Episode.id.in_(ep_ids_q)).delete(synchronize_session=False)

        deleted_characters = db.query(models.Character).filter(models.Character.project_id == pid).delete(synchronize_session=False)

        # Events + nodes
        evt_ids_q = db.query(models.Event.id).filter(models.Event.project_id == pid)
        deleted_event_nodes = db.query(models.EventNode).filter(models.EventNode.event_id.in_(evt_ids_q)).delete(synchronize_session=False)
        deleted_events = db.query(models.Event).filter(models.Event.project_id == pid).delete(synchronize_session=False)

        # AI runs（按钮历史）
        deleted_ai_runs = db.query(models.AiActionRun).filter(models.AiActionRun.project_id == pid).delete(synchronize_session=False)

        # 最后删 project
        db.delete(db_project)
        db.commit()
    except Exception as e:
        db.rollback()
        raise HTTPException(status_code=500, detail=f"Failed to delete project fully: {e}")
    
    return {
        "message": f"Project '{project_name}' and all associated data deleted successfully",
        "deleted_counts": {
            "assets": int(deleted_assets or 0),
            "shots": int(deleted_shots or 0),
            "scenes": int(deleted_scenes or 0),
            "episodes": int(deleted_episodes or 0),
            "characters": int(deleted_characters or 0),
            "event_nodes": int(deleted_event_nodes or 0),
            "events": int(deleted_events or 0),
            "ai_action_runs": int(deleted_ai_runs or 0),
        },
    }

# =======================
# 2. 资产条目管理接口（对外不再暴露“人设/角色”概念）
# =======================

@router.get("/{project_id}/asset-items", response_model=List[schemas.AssetItemRead])
def get_project_asset_items(
    project_id: str,
    category: Optional[str] = Query(default=None),
    db: Session = Depends(get_db),
):
    pid = resolve_project_pk(db, project_id)
    q = db.query(models.Character).options(joinedload(models.Character.assets)).filter(
        models.Character.project_id == pid
    )
    if category:
        q = q.filter(models.Character.category == category)
    return q.all()

@router.post("/{project_id}/asset-items", response_model=schemas.AssetItemRead)
def create_asset_item(project_id: str, item: CharacterCreate, db: Session = Depends(get_db)):
    pid = resolve_project_pk(db, project_id)
    exists = db.query(models.Character).filter(
        models.Character.project_id == pid,
        models.Character.name == item.name
    ).first()
    
    if exists:
        raise HTTPException(status_code=400, detail="该项目下已存在同名角色")

    new_char = models.Character(
        project_id=pid,
        name=item.name,
        description=item.description,
        base_prompt=item.base_prompt,
        category=item.category or "persona_visual",
    )
    db.add(new_char)
    db.commit()
    db.refresh(new_char)
    # 重新加载以包含 assets 关系
    db_char = db.query(models.Character).options(
        joinedload(models.Character.assets)
    ).filter(models.Character.id == new_char.id).first()
    return db_char

@router.patch("/asset-items/{item_id}", response_model=schemas.AssetItemRead)
def update_asset_item(item_id: int, item_update: CharacterUpdate, db: Session = Depends(get_db)):
    db_char = db.query(models.Character).filter(models.Character.id == item_id).first()
    if not db_char:
        raise HTTPException(status_code=404, detail="Asset item not found")
    

    if item_update.name and item_update.name != db_char.name:
        exists = db.query(models.Character).filter(
            models.Character.project_id == db_char.project_id,
            models.Character.name == item_update.name
        ).first()
        if exists:
            raise HTTPException(status_code=400, detail="该项目下已存在同名角色")

    if item_update.name is not None:
        db_char.name = item_update.name
    if item_update.description is not None:
        db_char.description = item_update.description
    if item_update.base_prompt is not None:
        db_char.base_prompt = item_update.base_prompt
    if item_update.category is not None:
        db_char.category = item_update.category
        
    if item_update.avatar_path:
        new_asset = models.Asset(
            file_path=item_update.avatar_path,
            file_type="image",
            is_favorite=True 
        )
        db.add(new_asset)
        db.flush() 
        db_char.avatar_asset_id = new_asset.id

    db.commit()
    # 重新加载以包含 assets 关系
    db_char = db.query(models.Character).options(
        joinedload(models.Character.assets)
    ).filter(models.Character.id == item_id).first()
    return db_char
    
@router.delete("/asset-items/{item_id}")
def delete_asset_item(item_id: int, db: Session = Depends(get_db)):
    # 查询资产条目
    character = db.query(models.Character).options(joinedload(models.Character.assets)).filter(models.Character.id == item_id).first()
    if not character:
        raise HTTPException(status_code=404, detail="Asset item not found")
    
    # --- 新增逻辑：物理删除关联的文件 ---
    # 假设你的 DATA_DIR 是项目根目录下的 data 文件夹
    base_data_dir = os.environ.get("AICOMIC_DATA_DIR") or os.path.join(os.getcwd(), "data")
    
    # 遍历该角色的所有资产进行删除
    # 注意：确保 character.assets 是正确关联的列表
    for asset in character.assets:
        if asset.file_path:
            # 拼接完整路径
            file_full_path = os.path.join(base_data_dir, asset.file_path)
            try:
                if os.path.exists(file_full_path):
                    os.remove(file_full_path)
                    logger.info(f"[Projects][delete_asset_item] Deleted file: {file_full_path}")
            except Exception as e:
                logger.error(f"[Projects][delete_asset_item] Error deleting file {file_full_path}: {e}")
    # -----------------------------------

    # 删除数据库记录
    db.delete(character)
    db.commit()
    return {"message": "Asset item and associated files deleted"}

# -----------------------
# 兼容旧接口（前端已切到 /asset-items；此处仅避免旧客户端断掉）
# -----------------------
@router.get("/{project_id}/characters", response_model=List[schemas.AssetItemRead])
def get_project_characters(project_id: str, db: Session = Depends(get_db)):
    return get_project_asset_items(project_id=project_id, category=None, db=db)

@router.post("/{project_id}/characters", response_model=schemas.AssetItemRead)
def create_character(project_id: str, char: CharacterCreate, db: Session = Depends(get_db)):
    return create_asset_item(project_id=project_id, item=char, db=db)

@router.patch("/characters/{char_id}", response_model=schemas.AssetItemRead)
def update_character(char_id: int, char_update: CharacterUpdate, db: Session = Depends(get_db)):
    return update_asset_item(item_id=char_id, item_update=char_update, db=db)

@router.delete("/characters/{character_id}")
def delete_character(character_id: int, db: Session = Depends(get_db)):
    return delete_asset_item(item_id=character_id, db=db)

@router.delete("/assets/{asset_id}")
def delete_asset(asset_id: int, db: Session = Depends(get_db)):
    # 👇 修正：查询 Asset 表
    asset = db.query(Asset).filter(Asset.id == asset_id).first()
    
    if not asset:
        raise HTTPException(status_code=404, detail="Asset not found")
    
    # 物理删除文件
    base_data_dir = os.environ.get("AICOMIC_DATA_DIR") or os.path.join(os.getcwd(), "data")
    if asset.file_path:
        # 防止路径拼接错误，根据你的实际存储逻辑调整
        # 如果 file_path 已经是相对路径 "characters/1/xxx.jpg"，直接拼
        file_full_path = os.path.join(base_data_dir, asset.file_path)
        
        try:
            if os.path.exists(file_full_path):
                os.remove(file_full_path)
                logger.info(f"[Projects][delete_asset] Deleted asset file: {file_full_path}")
            else:
                logger.error(f"[Projects][delete_asset] File not found on disk: {file_full_path}")
        except Exception as e:
            logger.error(f"[Projects][delete_asset] Error deleting file {file_full_path}: {e}")

    # 删除数据库记录
    db.delete(asset)
    db.commit()
    return {"message": "Asset deleted successfully"}
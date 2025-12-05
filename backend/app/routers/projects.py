from fastapi import APIRouter, Depends, HTTPException
from sqlalchemy.orm import Session, joinedload
from typing import List, Optional
from pydantic import BaseModel
from .. import models, schemas
from ..database import get_db
import os
from ..models import Character, Asset, Project

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

class CharacterUpdate(BaseModel):
    name: Optional[str] = None
    description: Optional[str] = None
    base_prompt: Optional[str] = None
    avatar_path: Optional[str] = None 

# =======================
# 1. 项目管理接口
# =======================

@router.get("/", response_model=List[schemas.ProjectBase])
def get_projects(db: Session = Depends(get_db)):
    return db.query(models.Project).all()

@router.post("/", response_model=schemas.ProjectBase)
def create_project(project: ProjectCreate, db: Session = Depends(get_db)):
    db_project = models.Project(name=project.name, description=project.description)
    db.add(db_project)
    db.commit()
    db.refresh(db_project)
    return db_project

# 【新增】修改项目 (重命名)
@router.patch("/{project_id}", response_model=schemas.ProjectBase)
def update_project(project_id: int, project_update: ProjectUpdate, db: Session = Depends(get_db)):
    db_project = db.query(models.Project).filter(models.Project.id == project_id).first()
    if not db_project:
        raise HTTPException(status_code=404, detail="Project not found")
    
    if project_update.name is not None:
        db_project.name = project_update.name
    if project_update.description is not None:
        db_project.description = project_update.description
        
    db.commit()
    db.refresh(db_project)
    return db_project

# =======================
# 2. 人设管理接口
# =======================

@router.get("/{project_id}/characters", response_model=List[schemas.CharacterRead])
def get_project_characters(project_id: int, db: Session = Depends(get_db)):
    chars = db.query(models.Character).options(
        joinedload(models.Character.assets)
    ).filter(models.Character.project_id == project_id).all()
    return chars

@router.post("/{project_id}/characters", response_model=schemas.CharacterRead)
def create_character(project_id: int, char: CharacterCreate, db: Session = Depends(get_db)):
    exists = db.query(models.Character).filter(
        models.Character.project_id == project_id,
        models.Character.name == char.name
    ).first()
    
    if exists:
        raise HTTPException(status_code=400, detail="该项目下已存在同名角色")

    new_char = models.Character(
        project_id=project_id,
        name=char.name,
        description=char.description,
        base_prompt=char.base_prompt
    )
    db.add(new_char)
    db.commit()
    db.refresh(new_char)
    # 重新加载以包含 assets 关系
    db_char = db.query(models.Character).options(
        joinedload(models.Character.assets)
    ).filter(models.Character.id == new_char.id).first()
    return db_char

@router.patch("/characters/{char_id}", response_model=schemas.CharacterRead)
def update_character(char_id: int, char_update: CharacterUpdate, db: Session = Depends(get_db)):
    db_char = db.query(models.Character).filter(models.Character.id == char_id).first()
    if not db_char:
        raise HTTPException(status_code=404, detail="角色不存在")
    

    if char_update.name and char_update.name != db_char.name:
        exists = db.query(models.Character).filter(
            models.Character.project_id == db_char.project_id,
            models.Character.name == char_update.name
        ).first()
        if exists:
            raise HTTPException(status_code=400, detail="该项目下已存在同名角色")

    if char_update.name is not None:
        db_char.name = char_update.name
    if char_update.description is not None:
        db_char.description = char_update.description
    if char_update.base_prompt is not None:
        db_char.base_prompt = char_update.base_prompt
        
    if char_update.avatar_path:
        new_asset = models.Asset(
            file_path=char_update.avatar_path,
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
    ).filter(models.Character.id == char_id).first()
    return db_char
    
@router.delete("/characters/{character_id}")
def delete_character(character_id: int, db: Session = Depends(get_db)):
    # 查询角色
    character = db.query(models.Character).filter(models.Character.id == character_id).first()
    if not character:
        raise HTTPException(status_code=404, detail="Character not found")
    
    # --- 新增逻辑：物理删除关联的文件 ---
    # 假设你的 DATA_DIR 是项目根目录下的 data 文件夹
    base_data_dir = os.path.join(os.getcwd(), "data")
    
    # 遍历该角色的所有资产进行删除
    # 注意：确保 character.assets 是正确关联的列表
    for asset in character.assets:
        if asset.file_path:
            # 拼接完整路径
            file_full_path = os.path.join(base_data_dir, asset.file_path)
            try:
                if os.path.exists(file_full_path):
                    os.remove(file_full_path)
                    print(f"Deleted file: {file_full_path}")
            except Exception as e:
                print(f"Error deleting file {file_full_path}: {e}")
    # -----------------------------------

    # 删除数据库记录
    db.delete(character)
    db.commit()
    return {"message": "Character and associated files deleted"}

@router.delete("/assets/{asset_id}")
def delete_asset(asset_id: int, db: Session = Depends(get_db)):
    # 👇 修正：查询 Asset 表
    asset = db.query(Asset).filter(Asset.id == asset_id).first()
    
    if not asset:
        raise HTTPException(status_code=404, detail="Asset not found")
    
    # 物理删除文件
    base_data_dir = os.path.join(os.getcwd(), "data")
    if asset.file_path:
        # 防止路径拼接错误，根据你的实际存储逻辑调整
        # 如果 file_path 已经是相对路径 "characters/1/xxx.jpg"，直接拼
        file_full_path = os.path.join(base_data_dir, asset.file_path)
        
        try:
            if os.path.exists(file_full_path):
                os.remove(file_full_path)
                print(f"Deleted asset file: {file_full_path}")
            else:
                print(f"File not found on disk: {file_full_path}")
        except Exception as e:
            print(f"Error deleting file {file_full_path}: {e}")

    # 删除数据库记录
    db.delete(asset)
    db.commit()
    return {"message": "Asset deleted successfully"}
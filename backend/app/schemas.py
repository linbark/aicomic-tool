from pydantic import BaseModel
from typing import List, Optional
from datetime import datetime

# === 基础 Asset & Shot ===
class AssetBase(BaseModel):
    file_path: str
    file_type: str
    meta_data: Optional[dict] = None 
    is_favorite: bool = False
    
class AssetRead(AssetBase):
    id: int
    created_at: datetime
    class Config: from_attributes = True

class CharacterRead(BaseModel):
    id: int
    name: str
    description: Optional[str] = None
    base_prompt: Optional[str] = None
    avatar_asset_id: Optional[int] = None
    
    # 【新增】返回该角色的所有图片/视频
    assets: List[AssetRead] = [] 
    
    class Config:
        from_attributes = True
        
class ShotBase(BaseModel):
    sequence_number: int
    title: Optional[str] = None
    action_text: Optional[str] = None
    dialogue: Optional[str] = None
    prompt: Optional[str] = None
    status: str = "draft"

class ShotRead(ShotBase):
    id: int
    assets: List[AssetRead] = [] 
    selected_asset_id: Optional[int] = None
    video_path: Optional[str] = None

    class Config: from_attributes = True

class ShotCreate(ShotBase): pass
class ShotUpdate(BaseModel):
    title: Optional[str] = None
    action_text: Optional[str] = None
    dialogue: Optional[str] = None
    prompt: Optional[str] = None
    status: Optional[str] = None
    selected_asset_id: Optional[int] = None

# === 剧本骨架 (Episode -> Scene) ===

class SceneRead(BaseModel):
    id: int
    title: Optional[str] = None
    sequence_number: Optional[int] = None
    shots: List[ShotRead] = []
    class Config: from_attributes = True

class SceneCreate(BaseModel):
    title: str
    sequence_number: Optional[int] = None

class SceneUpdate(BaseModel):
    title: Optional[str] = None

class EpisodeRead(BaseModel):
    id: int
    title: str
    order: int
    scenes: List[SceneRead] = []
    class Config: from_attributes = True

class EpisodeCreate(BaseModel):
    title: str
    order: int = 0

# === 事件系统 (Event) ===
# --- Pydantic 模型 (建议加到 schemas.py) ---
class EventNodeUpdate(BaseModel):
    description: str
    target_type: str # "episode", "scene", "shot"
    target_id: int
class EventNodeRead(BaseModel):
    id: int
    target_type: str
    target_id: int
    description: str
    class Config: from_attributes = True

class EventRead(BaseModel):
    id: int
    name: str
    color: str
    description: Optional[str] = None
    graph_data: Optional[dict] = None
    nodes: List[EventNodeRead] = []
    class Config: from_attributes = True

class EventCreate(BaseModel):
    name: str
    color: str = "#3B82F6"
    description: Optional[str] = None


class EventUpdate(BaseModel):
    name: Optional[str] = None
    color: Optional[str] = None
    description: Optional[str] = None
    graph_data: Optional[dict] = None

# === 项目 ===
class ProjectBase(BaseModel):
    id: int
    name: str
    description: Optional[str] = None
    class Config: from_attributes = True

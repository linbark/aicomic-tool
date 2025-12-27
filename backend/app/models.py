from sqlalchemy import Column, Integer, String, Text, ForeignKey, DateTime, Boolean, JSON, Enum
from sqlalchemy.orm import relationship
from sqlalchemy.sql import func
from .database import Base 

# =======================
# 1. 基础项目与人设 (保持不变)
# =======================
class Project(Base):
    __tablename__ = "projects"
    id = Column(Integer, primary_key=True, index=True)
    name = Column(String, index=True)
    description = Column(Text, nullable=True)
    created_at = Column(DateTime(timezone=True), server_default=func.now())
    
    characters = relationship("Character", back_populates="project", cascade="all, delete-orphan")
    episodes = relationship("Episode", back_populates="project", cascade="all, delete-orphan")
    events = relationship("Event", back_populates="project", cascade="all, delete-orphan")

class Character(Base):
    __tablename__ = "characters"
    id = Column(Integer, primary_key=True, index=True)
    project_id = Column(Integer, ForeignKey("projects.id"))
    name = Column(String, index=True)
    description = Column(Text)
    base_prompt = Column(Text)
    negative_prompt = Column(Text, nullable=True)
    # 资产条目分类：persona(人设资产) / background(背景) ...
    # 说明：对外 API 不再暴露“角色”概念，但为兼容旧数据与关系，这里保留表名与模型名。
    # 默认分类：角色（视觉）
    category = Column(String, nullable=False, default="persona_visual")
    avatar_asset_id = Column(Integer, ForeignKey("assets.id"), nullable=True)
    project = relationship("Project", back_populates="characters")
    assets = relationship("Asset", back_populates="character", cascade="all, delete-orphan", foreign_keys="Asset.character_id")

# =======================
# 2. 剧本物理层 (The Backbone)
# Project -> Episode -> Scene -> Shot
# =======================

class Episode(Base):
    __tablename__ = "episodes"
    id = Column(Integer, primary_key=True, index=True)
    project_id = Column(Integer, ForeignKey("projects.id"))
    title = Column(String) # 例如 "第一集：初入青云"
    order = Column(Integer, default=0) # 排序用
    
    project = relationship("Project", back_populates="episodes")
    scenes = relationship("Scene", back_populates="episode", cascade="all, delete-orphan")

class Scene(Base):
    __tablename__ = "scenes"
    id = Column(Integer, primary_key=True, index=True)
    # 【重大变更】Scene 现在属于 Episode，而不是直接属于 Project
    episode_id = Column(Integer, ForeignKey("episodes.id")) 
    sequence_number = Column(Integer) 
    title = Column(String) 
    
    episode = relationship("Episode", back_populates="scenes")
    shots = relationship("Shot", back_populates="scene", cascade="all, delete-orphan")

class Shot(Base):
    __tablename__ = "shots"
    id = Column(Integer, primary_key=True, index=True)
    scene_id = Column(Integer, ForeignKey("scenes.id"))
    sequence_number = Column(Integer) 
    title = Column(String, nullable=True)
    
    action_text = Column(Text) 
    dialogue = Column(Text, nullable=True) 
    prompt = Column(Text) 
    negative_prompt = Column(Text, nullable=True)
    selected_asset_id = Column(Integer, ForeignKey("assets.id"), nullable=True)
    status = Column(String, default="draft") 
    video_path = Column(String, nullable=True)
    
    scene = relationship("Scene", back_populates="shots")
    assets = relationship("Asset", back_populates="shot", foreign_keys="Asset.shot_id")

class Asset(Base):
    __tablename__ = "assets"
    id = Column(Integer, primary_key=True, index=True)
    character_id = Column(Integer, ForeignKey("characters.id"), nullable=True)
    shot_id = Column(Integer, ForeignKey("shots.id"), nullable=True) 
    file_path = Column(String) 
    file_type = Column(String) 
    meta_data = Column(JSON, nullable=True) 
    created_at = Column(DateTime(timezone=True), server_default=func.now())
    is_favorite = Column(Boolean, default=False) 
    
    shot = relationship("Shot", back_populates="assets", foreign_keys=[shot_id])
    character = relationship("Character", back_populates="assets", foreign_keys=[character_id])

# =======================
# 3. 事件逻辑层 (The Overlay)
# =======================

class Event(Base):
    __tablename__ = "events"
    id = Column(Integer, primary_key=True, index=True)
    project_id = Column(Integer, ForeignKey("projects.id"))
    
    name = Column(String, index=True)   # 事件名，如 "张小凡黑化"
    color = Column(String, default="#3B82F6") # 显示颜色，默认蓝色
    start_time_sort_key = Column(Integer, default=0) # 用于 Y 轴排序
    description = Column(Text, nullable=True)  # 事件详细描述
    graph_data = Column(JSON, nullable=True)   # Vue Flow 节点/连线数据
    
    project = relationship("Project", back_populates="events")
    nodes = relationship("EventNode", back_populates="event", cascade="all, delete-orphan")

class EventNode(Base):
    __tablename__ = "event_nodes"
    id = Column(Integer, primary_key=True, index=True)
    event_id = Column(Integer, ForeignKey("events.id"))
    
    # 多态关联：指向 Episode / Scene / Shot
    # 在 SQLite/SQLAlchemy 中，简单做法是存 target_type 和 target_id
    target_type = Column(String) # "episode", "scene", "shot"
    target_id = Column(Integer)
    
    description = Column(Text) # 该粒度下的描述
    
    event = relationship("Event", back_populates="nodes")
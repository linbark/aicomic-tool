"""
记忆系统的数据模型定义（MemoryRecord、namespace、type、entity 等）
"""
from __future__ import annotations

from enum import Enum
from typing import Any, Dict, List, Optional
from uuid import uuid4

from pydantic import BaseModel, ConfigDict, Field


class MemoryNamespace(str, Enum):
    """记忆命名空间，用于隔离不同类型的记忆"""
    STATIC_BIBLE = "static_bible"  # 只读/低频写：世界观设定、角色档案、规则
    DYNAMIC_PLOT = "dynamic_plot"  # 读写/高频：剧情进展、人物关系演化
    EPISODIC = "episodic"  # 情节记忆：状态变更、事件
    WORLD_RULES_NEGATIVE = "world_rules_negative"  # 负向约束：禁忌、不允许的内容
    PRODUCTION = "production"  # 生产记忆：prompt 参数、成功样例


class MemoryType(str, Enum):
    """记忆类型，用于细粒度分类"""
    CHARACTER_DESIGN = "character_design"  # 角色设计（Visual DNA）
    CHARACTER_FACT = "character_fact"  # 角色事实（背景、性格）
    WORLD_RULE = "world_rule"  # 世界规则（物理法则、社会规则）
    EVENT = "event"  # 事件（发生了什么）
    RELATIONSHIP = "relationship"  # 关系（角色间关系）
    PROP = "prop"  # 道具
    LOCATION = "location"  # 地点
    STYLE_GUIDE = "style_guide"  # 风格指南
    NEGATIVE_CONSTRAINT = "negative_constraint"  # 负向约束
    PROMPT_TEMPLATE = "prompt_template"  # Prompt 模板/参数


class MemoryRecord(BaseModel):
    """统一的记忆条目"""
    model_config = ConfigDict(extra="allow")

    id: str = Field(default_factory=lambda: uuid4().hex, description="唯一标识")
    project_id: int = Field(..., description="项目 ID")
    namespace: MemoryNamespace = Field(..., description="命名空间")
    type: MemoryType = Field(..., description="记忆类型")
    entity: Optional[str] = Field(None, description="实体锚点（如角色名、道具名）")
    content: str = Field(..., min_length=1, description="用于 embedding 的自然语言内容（短、原子化）")
    payload_json: Optional[Dict[str, Any]] = Field(None, description="结构化数据（VisualDNA JSON、状态变更 JSON 等）")
    source_ref: Optional[str] = Field(None, description="来源引用（run_id/stage_name/episode_id/scene_id）")
    time_index: Optional[str] = Field(None, description="时序索引（episode_seq/scene_seq/beat_seq 或 created_at_ms）")
    hash: Optional[str] = Field(None, description="内容哈希（用于去重）")
    created_at_ms: Optional[int] = Field(None, description="创建时间戳（毫秒）")


class MemoryQuery(BaseModel):
    """记忆查询请求"""
    model_config = ConfigDict(extra="allow")

    project_id: int = Field(..., description="项目 ID")
    query_text: Optional[str] = Field(None, description="查询文本（用于语义检索）")
    namespace: Optional[MemoryNamespace] = Field(None, description="命名空间过滤")
    type: Optional[MemoryType] = Field(None, description="类型过滤")
    entity: Optional[str] = Field(None, description="实体过滤")
    time_index_from: Optional[str] = Field(None, description="时序起始")
    time_index_to: Optional[str] = Field(None, description="时序结束")
    top_k: int = Field(default=10, ge=1, le=100, description="返回数量")
    min_score: Optional[float] = Field(None, ge=0.0, le=1.0, description="最小相似度分数")


class MemoryRetrievalResult(BaseModel):
    """记忆检索结果"""
    model_config = ConfigDict(extra="allow")

    records: List[MemoryRecord] = Field(default_factory=list, description="检索到的记忆条目")
    scores: List[float] = Field(default_factory=list, description="相似度分数（与 records 一一对应）")
    total: int = Field(default=0, description="总匹配数")


class StateChange(BaseModel):
    """状态变更（用于 Episodic 记忆）"""
    model_config = ConfigDict(extra="allow")

    event: str = Field(..., description="事件描述")
    state_changes: Dict[str, Any] = Field(default_factory=dict, description="状态变更详情")
    entities: List[str] = Field(default_factory=list, description="涉及的实体列表")
    episode_id: Optional[int] = Field(None, description="章节 ID")
    scene_id: Optional[int] = Field(None, description="场景 ID")
    beat_index: Optional[int] = Field(None, description="节拍索引")
    created_at_ms: Optional[int] = Field(None, description="创建时间戳")


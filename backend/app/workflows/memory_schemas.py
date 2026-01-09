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
    CANONICAL = "canonical"  # 结构化真值库（Entity/Snapshot/Event/ChangeSet/Conflict/Evidence 等）


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
    EVIDENCE = "evidence"  # 原文证据切片
    ENTITY = "entity"  # 实体主干（角色/地点/组织/道具/规则概念）
    SNAPSHOT = "snapshot"  # 版本切片（时间轴）
    STATE_CHANGE = "state_change"  # 状态变更（结构化补丁）
    CHANGESET = "changeset"  # 变更集（提案/提交单）
    CONFLICT = "conflict"  # 冲突/歧义/待裁决
    TIME_CONSTRAINT = "time_constraint"  # 时间约束（partial order）
    TIME_BLOCK = "time_block"  # 时间块（回忆/插叙容器）


class TruthStatus(str, Enum):
    """真值状态（适用于实体/规则/切片/事件/推断等）"""
    CONFIRMED = "confirmed"
    HYPOTHESIS = "hypothesis"
    RETCONNED = "retconned"


class SourceKind(str, Enum):
    """来源类型（谁/什么产生了这条记忆）"""
    EXTRACTED = "extracted"  # 抽取器从原文显式抽取
    INFERRED = "inferred"  # 从行为/对话推断
    HUMAN = "human"  # 人工确认/编辑
    SYSTEM = "system"  # 系统生成（索引器/迁移/修复）


class StoryTimeType(str, Enum):
    """故事内时间表达类型"""
    ABSOLUTE = "absolute"  # 绝对时间（若能解析）
    RELATIVE = "relative"  # 相对时间（T+3d / 三天后）
    ORDINAL = "ordinal"  # 可排序但不可解释（ARC1.S12）
    UNKNOWN = "unknown"  # 未定/缺失


class StoryTime(BaseModel):
    """故事内时间（可缺失，可逐章回填）"""
    model_config = ConfigDict(extra="allow")

    type: StoryTimeType = Field(default=StoryTimeType.UNKNOWN)
    key: Optional[str] = Field(
        default=None,
        description="可排序键（如 D03T18:00 / ARC1.S12 / T+3600）；unknown 时可为空",
    )
    label: Optional[str] = Field(default=None, description="人类可读标签（如 '傍晚'/'三天后'）")


class TimeRelation(str, Enum):
    """时间约束关系（partial order）"""
    BEFORE = "before"
    AFTER = "after"
    DURING = "during"
    OVERLAPS = "overlaps"
    FLASHBACK_OF = "flashback_of"
    WITHIN_INTERVAL = "within_interval"
    ANCHORED_TO = "anchored_to"


class TimeConstraint(BaseModel):
    """时间约束（用于倒叙/插叙/缺失时间的排序求解）"""
    model_config = ConfigDict(extra="allow")

    id: str = Field(default_factory=lambda: uuid4().hex)
    project_id: int
    relation: TimeRelation
    from_event_id: Optional[str] = None
    to_event_id: Optional[str] = None
    # 可选：约束指向某个锚点（事件或时间块）
    anchor_id: Optional[str] = None
    # 可选：区间约束
    interval: Optional[Dict[str, Any]] = None
    evidence_ids: List[str] = Field(default_factory=list)
    status: TruthStatus = TruthStatus.HYPOTHESIS
    confidence: float = 0.5
    source_kind: SourceKind = SourceKind.EXTRACTED


class TimeBlock(BaseModel):
    """时间块（回忆段/插叙段），用于把一组事件作为容器挂接到主线"""
    model_config = ConfigDict(extra="allow")

    id: str = Field(default_factory=lambda: uuid4().hex)
    project_id: int
    name: Optional[str] = None
    parent_block_id: Optional[str] = None
    anchor_id: Optional[str] = None
    constraint_ids: List[str] = Field(default_factory=list)
    event_ids: List[str] = Field(default_factory=list)
    status: TruthStatus = TruthStatus.HYPOTHESIS
    confidence: float = 0.5
    source_kind: SourceKind = SourceKind.EXTRACTED


class EvidenceSpan(BaseModel):
    """证据定位（最小合同：不绑定具体切片算法）"""
    model_config = ConfigDict(extra="allow")

    # 允许多种定位方式：段落/句子/字符 offset
    paragraph_index: Optional[int] = None
    sentence_index: Optional[int] = None
    start_offset: Optional[int] = None
    end_offset: Optional[int] = None


class EvidenceRecordPayload(BaseModel):
    """Evidence 的结构化载荷（存入 payload_json）"""
    model_config = ConfigDict(extra="allow")

    evidence_id: str = Field(default_factory=lambda: uuid4().hex)
    episode_id: Optional[int] = None
    scene_id: Optional[int] = None
    span: Optional[EvidenceSpan] = None
    quote: str
    speaker: Optional[str] = None
    tags: List[str] = Field(default_factory=list)


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
    # Canonical 扩展字段（对 Vector/SQLite 过滤友好，均为可选以兼容旧数据）
    status: TruthStatus = Field(default=TruthStatus.CONFIRMED, description="真值状态（confirmed/hypothesis/retconned）")
    confidence: float = Field(default=1.0, ge=0.0, le=1.0, description="置信度（0-1）")
    source_kind: SourceKind = Field(default=SourceKind.SYSTEM, description="来源类型（extracted/inferred/human/system）")
    evidence_ids: List[str] = Field(default_factory=list, description="关联的证据 ID 列表")
    story_order: Optional[str] = Field(default=None, description="稳定序键（如 CH01.E0007），用于最低保真排序")
    story_time: Optional[StoryTime] = Field(default=None, description="故事内时间（可 unknown，可回填）")
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
    status: Optional[TruthStatus] = Field(None, description="真值状态过滤（confirmed/hypothesis/retconned）")
    min_confidence: Optional[float] = Field(None, ge=0.0, le=1.0, description="最低置信度过滤（0-1）")
    source_kind: Optional[SourceKind] = Field(None, description="来源过滤（extracted/inferred/human/system）")
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


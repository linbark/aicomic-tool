from pydantic import BaseModel, Field, field_validator, ConfigDict
from typing import Any, Dict, List, Optional, Literal, Union
from datetime import datetime
import re

# =========================================================
# Visual Profile (Image -> JSON Consistency Layer)
# =========================================================

_HEX_COLOR_RE = re.compile(r"^#[0-9a-fA-F]{6}$")
_JSONPATH_RE = re.compile(r"^\$\.[A-Za-z_][A-Za-z0-9_]*(\.[A-Za-z_][A-Za-z0-9_]*)*$")

# 仅允许引用“稳定字段”（便于 field_whitelist 锁定策略做强校验）
_ALLOWED_VISUAL_PROFILE_JSONPATHS = {
    "$.id",
    "$.name",
    "$.source.type",
    "$.source.ref",
    "$.character_core.visual_dna.face",
    "$.character_core.visual_dna.body_type",
    "$.character_core.visual_dna.hair_style",
    "$.character_core.visual_dna.distinguishing_marks",
    "$.character_core.attire.base_layer",
    "$.character_core.attire.accessories",
    "$.technical_specs.lighting_style",
    "$.technical_specs.camera_angle",
    "$.technical_specs.composition",
    "$.technical_specs.color_palette",
}


class VisualProfileSource(BaseModel):
    type: Literal["image", "text"]
    ref: str


class VisualDNAJson(BaseModel):
    face: str
    body_type: Optional[str] = None
    hair_style: Optional[str] = None
    distinguishing_marks: Optional[str] = None


class AttireJson(BaseModel):
    base_layer: Optional[str] = None
    accessories: Optional[List[str]] = None


class CharacterCoreJson(BaseModel):
    visual_dna: VisualDNAJson
    attire: Optional[AttireJson] = None


class TechnicalSpecsJson(BaseModel):
    lighting_style: Optional[str] = None
    camera_angle: Optional[str] = None
    composition: Optional[str] = None
    color_palette: List[str] = Field(default_factory=list)

    @field_validator("color_palette")
    @classmethod
    def validate_hex_palette(cls, v: List[str]) -> List[str]:
        for color in v:
            if not _HEX_COLOR_RE.match(color):
                raise ValueError(f"Invalid hex color: {color}")
        return v


class VisualProfile(BaseModel):
    # 允许 LLM/工具链返回更多字段（不限制死）
    model_config = ConfigDict(extra="allow")
    id: str
    name: Optional[str] = None
    source: VisualProfileSource
    character_core: CharacterCoreJson
    # 兼容“不可变字符串镜像”（便于 prompt 侧逐字锁定）
    visual_dna_string: Optional[str] = None
    technical_specs: Optional[TechnicalSpecsJson] = None
    stable_diffusion_tags: Optional[str] = None
    notes: Optional[str] = None


class VisualProfileLibrary(BaseModel):
    schema_version: str = "visual_profile@0.1"
    profiles: List[VisualProfile] = Field(default_factory=list)


class VisualAssetRef(BaseModel):
    # 允许用数据库 asset_id 直接引用；也允许直接传 image_ref（绝对路径或 data/ 下的相对路径）
    asset_id: Optional[int] = None
    id: Optional[str] = None  # 业务 id（角色/资产 id，字符串）
    name: Optional[str] = None
    image_ref: Optional[str] = None


class VisualAssetIngestRequest(BaseModel):
    assets: List[VisualAssetRef]
    schema_version: str = "visual_profile@0.1"
    extract_mode: Literal["character_only", "character_plus_technical"] = "character_plus_technical"
    # 默认不允许下游覆盖：只允许“加场景”，不改“人设”
    allow_overrides: bool = False


# =========================================================
# Agent Contract (Envelope + Errors + Constraints) - v0.1
# =========================================================


class AgentError(BaseModel):
    code: str
    message: str
    field_path: Optional[str] = None
    suggestion: Optional[str] = None


class JsonConsistencyConstraint(BaseModel):
    enabled: bool = False
    schema_version: str = "visual_profile@0.1"
    locking_policy: Literal["verbatim_json_block", "field_whitelist"] = "field_whitelist"
    required_fields: List[str] = Field(default_factory=list)
    allow_overrides: bool = False
    # 是否强制 required_fields 必须落在系统已知白名单内（默认 true；需要更开放时可关闭）
    enforce_allowlist: bool = True

    @field_validator("required_fields")
    @classmethod
    def validate_required_fields(cls, v: List[str]) -> List[str]:
        # 基础格式校验：JSONPath（简化版，不支持数组/通配符）
        for p in v:
            if not _JSONPATH_RE.match(p):
                raise ValueError(f"Invalid JSONPath (only '$.a.b.c' supported): {p}")
        return v

    @field_validator("required_fields")
    @classmethod
    def validate_required_fields_allowlist(cls, v: List[str], info):
        # 仅当启用 + field_whitelist 时做强校验
        data = info.data or {}
        enabled = bool(data.get("enabled", False))
        locking_policy = data.get("locking_policy", "field_whitelist")
        enforce_allowlist = bool(data.get("enforce_allowlist", True))
        if not enabled:
            return v
        if locking_policy != "field_whitelist":
            return v
        if not v:
            raise ValueError("json_consistency.required_fields must be non-empty when enabled + field_whitelist")
        if enforce_allowlist:
            unknown = [p for p in v if p not in _ALLOWED_VISUAL_PROFILE_JSONPATHS]
            if unknown:
                raise ValueError(f"json_consistency.required_fields contains unknown paths: {unknown}")
        # 去重但保序
        seen = set()
        uniq: List[str] = []
        for p in v:
            if p not in seen:
                uniq.append(p)
                seen.add(p)
        return uniq


class PageTurnEngineeringConstraint(BaseModel):
    enabled: bool = False
    target: Literal["even_page_last_panel"] = "even_page_last_panel"


class VisualDNALockingConstraint(BaseModel):
    enabled: bool = True
    policy: Literal["verbatim", "ordered_tokens"] = "verbatim"


class RefinementLoopConstraint(BaseModel):
    enabled: bool = False
    max_rounds: int = 2


class Constraints(BaseModel):
    visual_first: bool = True
    anti_psychologizing: bool = True
    bubble_text_limit_zh: int = 30
    action_block_max_lines: int = 4
    page_turn_engineering: PageTurnEngineeringConstraint = Field(default_factory=PageTurnEngineeringConstraint)
    visual_dna_locking: VisualDNALockingConstraint = Field(default_factory=VisualDNALockingConstraint)
    fountain_strict: bool = True
    prompt_dialects: List[str] = Field(default_factory=lambda: ["midjourney_v6", "stable_diffusion", "flux"])
    refinement_loop: RefinementLoopConstraint = Field(default_factory=RefinementLoopConstraint)
    json_consistency: JsonConsistencyConstraint = Field(default_factory=JsonConsistencyConstraint)


class AgentRequest(BaseModel):
    request_id: str
    agent: str
    version: str = "0.1"
    input: Dict[str, Any]
    context: Dict[str, Any] = Field(default_factory=dict)
    constraints: Constraints = Field(default_factory=Constraints)
    options: Dict[str, Any] = Field(default_factory=dict)


class AgentResponse(BaseModel):
    request_id: str
    agent: str
    status: Literal["ok", "error"]
    output: Optional[Any] = None
    warnings: List[str] = Field(default_factory=list)
    errors: List[AgentError] = Field(default_factory=list)
    meta: Dict[str, Any] = Field(default_factory=dict)


# =========================================================
# Artifacts (SeriesBible / BeatSheet / Fountain / Storyboard / PromptPack / QC) - v0.1
# 说明：这些是“契约层”结构，不影响现有 Episode/Scene/Shot 的 DB 读写。
# =========================================================

Dialect = Literal["midjourney_v6", "stable_diffusion", "flux"]


class WorldRules(BaseModel):
    era: Optional[str] = None
    physics: List[str] = Field(default_factory=list)
    technology: List[str] = Field(default_factory=list)
    taboos: List[str] = Field(default_factory=list)


class CharacterProfile(BaseModel):
    id: str
    name: str
    # 不可变字符串：推荐直接复用 VisualProfile.visual_dna_string（确定性 JSON 串）
    visual_dna: str
    do_not_change: List[str] = Field(default_factory=list)


class SeriesBible(BaseModel):
    title: str
    logline: Optional[str] = None
    visual_tone: Optional[str] = None
    characters: List[CharacterProfile] = Field(default_factory=list)
    world_rules: WorldRules = Field(default_factory=WorldRules)


class BeatEmotionCharge(BaseModel):
    start: Literal["-", "+"]
    end: Literal["-", "+"]


class Beat(BaseModel):
    id: str
    type: str
    emotion_charge: BeatEmotionCharge
    visual_focus: str
    estimated_panels: int
    page_turn_candidate: bool = False


class BeatSheet(BaseModel):
    arc_summary: str
    beats: List[Beat] = Field(default_factory=list)


class FountainLintResult(BaseModel):
    is_valid: bool
    errors: List[AgentError] = Field(default_factory=list)


class FountainScript(BaseModel):
    format: Literal["fountain"] = "fountain"
    text: str
    lint: Optional[FountainLintResult] = None
    is_valid: bool = True
    errors: List[AgentError] = Field(default_factory=list)


class Bubble(BaseModel):
    speaker: str
    text: str


class ShotSpec(BaseModel):
    size: str
    angle: str


class PageHint(BaseModel):
    page: int
    position: str  # e.g. "last_panel" / "top_left"；实现层可定义受控词汇表


class PanelLayout(BaseModel):
    # 预留：当需要真正的“翻页工程/排版算法”时再细化为受控枚举
    page: Optional[int] = None
    row: Optional[int] = None
    col: Optional[int] = None


class Panel(BaseModel):
    panel_id: str
    shot: ShotSpec
    lighting: Optional[str] = None
    action: str
    dialogues: List[Bubble] = Field(default_factory=list)
    visual_constraints: List[str] = Field(default_factory=list)
    layout: Optional[PanelLayout] = None
    page_hint: Optional[PageHint] = None


class StoryScene(BaseModel):
    scene_id: str
    panels: List[Panel] = Field(default_factory=list)


class Storyboard(BaseModel):
    scenes: List[StoryScene] = Field(default_factory=list)


class PromptItem(BaseModel):
    panel_id: str
    prompt: str
    negative_prompt: Optional[str] = None
    params: Dict[str, Any] = Field(default_factory=dict)
    locked_visual_dna_included: bool = False
    # 启用 json_consistency 时，编排器/QC 应确保为 True
    locked_visual_profile_included: Optional[bool] = None


class PromptPack(BaseModel):
    dialect: Dialect
    items: List[PromptItem] = Field(default_factory=list)
    # 方便“脱离 Orchestrator”做 schema 级校验（测试/工具链）
    json_consistency_enabled: bool = False
    visual_dna_locking_enabled: bool = True

    @field_validator("items")
    @classmethod
    def validate_locking_flags(cls, items: List[PromptItem], info) -> List[PromptItem]:
        data = info.data or {}
        if bool(data.get("visual_dna_locking_enabled", True)):
            if any(not it.locked_visual_dna_included for it in items):
                raise ValueError("PromptPack requires locked_visual_dna_included=true for all items when visual_dna_locking_enabled")
        if bool(data.get("json_consistency_enabled", False)):
            if any(it.locked_visual_profile_included is not True for it in items):
                raise ValueError("PromptPack requires locked_visual_profile_included=true for all items when json_consistency_enabled")
        return items


class JSONPatchOp(BaseModel):
    op: Literal["add", "remove", "replace", "move", "copy", "test"]
    path: str
    # "from" 是关键字，使用别名
    from_path: Optional[str] = Field(default=None, alias="from")
    value: Optional[Any] = None


class QCFix(BaseModel):
    description: str
    before_ref: str  # JSONPath 风格引用（例如 $.context.fountain_script.text）
    after_patch: List[JSONPatchOp] = Field(default_factory=list)


class QCCheck(BaseModel):
    name: str
    result: Literal["pass", "fail", "warn"]
    details: Optional[str] = None
    evidence: List[str] = Field(default_factory=list)
    fixes: List[QCFix] = Field(default_factory=list)


class QCReportSummary(BaseModel):
    pass_: bool = Field(alias="pass")
    score: Optional[float] = None


class QCReport(BaseModel):
    summary: QCReportSummary
    checks: List[QCCheck] = Field(default_factory=list)
    rounds: int = 0


class ManjuWorkflowTarget(BaseModel):
    pages: Optional[int] = None
    dialects: List[Dialect] = Field(default_factory=lambda: ["midjourney_v6", "stable_diffusion", "flux"])


class ManjuWorkflowRequest(BaseModel):
    request_id: str
    source_text: str
    assets: List[VisualAssetRef] = Field(default_factory=list)
    target: ManjuWorkflowTarget = Field(default_factory=ManjuWorkflowTarget)
    constraints: Constraints = Field(default_factory=Constraints)


class ManjuWorkflowResponse(BaseModel):
    request_id: str
    status: Literal["ok", "error"]
    warnings: List[str] = Field(default_factory=list)
    errors: List[AgentError] = Field(default_factory=list)
    visual_profile_library: Optional[VisualProfileLibrary] = None
    series_bible: Optional[SeriesBible] = None
    beat_sheet: Optional[BeatSheet] = None
    fountain_script: Optional[FountainScript] = None
    qc_report: Optional[QCReport] = None
    storyboard: Optional[Storyboard] = None
    prompt_packs: List[PromptPack] = Field(default_factory=list)
    meta: Dict[str, Any] = Field(default_factory=dict)


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

class AssetItemRead(BaseModel):
    id: int
    name: str
    description: Optional[str] = None
    base_prompt: Optional[str] = None
    category: str
    avatar_asset_id: Optional[int] = None
    
    # 【新增】返回该角色的所有图片/视频
    assets: List[AssetRead] = [] 
    
    class Config:
        from_attributes = True

# 兼容旧命名（后端内部仍可能引用 CharacterRead）
CharacterRead = AssetItemRead
        
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

# =========================================================
# LLM Provider 配置 (P3+)
# =========================================================

class LLMProviderConfigBase(BaseModel):
    provider_name: str
    is_active: bool = False
    config_data: Dict[str, Any]
    notes: Optional[str] = None

class LLMProviderConfigRead(LLMProviderConfigBase):
    id: int
    created_at: datetime
    updated_at: datetime
    class Config: from_attributes = True

class LLMProviderConfigCreate(LLMProviderConfigBase):
    pass

class LLMProviderConfigUpdate(BaseModel):
    is_active: Optional[bool] = None
    config_data: Optional[Dict[str, Any]] = None
    notes: Optional[str] = None

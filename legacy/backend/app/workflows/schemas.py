from __future__ import annotations

from enum import Enum
from typing import Any, Dict, List, Optional

from pydantic import BaseModel, ConfigDict, Field, model_validator


class ShotSize(str, Enum):
    ELS = "ELS"
    LS = "LS"
    MS = "MS"
    CU = "CU"
    ECU = "ECU"
    INSERT = "INSERT"


class CameraAngle(str, Enum):
    EYE = "EYE"
    LOW = "LOW"
    HIGH = "HIGH"
    DUTCH = "DUTCH"


class LightingStyle(str, Enum):
    SOFT = "SOFT"
    HARD = "HARD"
    CHIAROSCURO = "CHIAROSCURO"
    RIM = "RIM"
    VOLUMETRIC = "VOLUMETRIC"


class SeriesBible(BaseModel):
    """
    结构化世界观：字段允许渐进扩展，所以采用 extra=allow。
    """

    model_config = ConfigDict(extra="allow")

    world_rules: Optional[Dict[str, Any]] = None
    characters: Optional[Any] = None
    glossary: Optional[Dict[str, Any]] = None
    constraints: Optional[Any] = None

    @model_validator(mode="before")
    @classmethod
    def _ensure_dict(cls, v):
        if not isinstance(v, dict):
            raise TypeError("series_bible must be an object")
        return v


class BeatSheetItem(BaseModel):
    model_config = ConfigDict(extra="allow")

    beat_type: Optional[str] = None
    emotional_charge: Optional[str] = None
    visual_focus: Optional[str] = None
    estimated_panels: Optional[int] = None
    title: Optional[str] = None
    description: Optional[str] = None

    @model_validator(mode="before")
    @classmethod
    def _ensure_dict(cls, v):
        if not isinstance(v, dict):
            raise TypeError("beat item must be an object")
        return v


class QcIssue(BaseModel):
    model_config = ConfigDict(extra="allow")

    type: str = Field(..., min_length=1)
    message: str = Field(..., min_length=1)
    location: Optional[str] = None


class QcReport(BaseModel):
    model_config = ConfigDict(extra="allow")

    issues: List[QcIssue] = Field(default_factory=list)
    revised_script_fountain: Optional[str] = None


class ShotSpec(BaseModel):
    model_config = ConfigDict(extra="allow")

    title: Optional[str] = None
    action_text: str = Field(..., min_length=1)
    dialogue: Optional[str] = None

    shot_size: ShotSize
    camera_angle: CameraAngle
    lighting_style: LightingStyle

    # 下游翻译产出（可选）
    prompt: Optional[str] = None
    negative_prompt: Optional[str] = None


class PromptPair(BaseModel):
    model_config = ConfigDict(extra="allow")

    prompt: str = Field(..., min_length=1)
    negative_prompt: str = Field(..., min_length=1)



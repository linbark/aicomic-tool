"""
受控词汇表：Shot Size / Angle / Lighting 枚举与映射规则
确保术语稳定，减少漂移
"""
from enum import Enum
from typing import Optional


class ShotSize(str, Enum):
    """景别枚举"""
    EXTREME_WIDE = "extreme_wide"  # 极远景
    WIDE = "wide"  # 远景
    MEDIUM_WIDE = "medium_wide"  # 中远景
    MEDIUM = "medium"  # 中景
    MEDIUM_CLOSE = "medium_close"  # 中近景
    CLOSEUP = "closeup"  # 特写
    EXTREME_CLOSEUP = "extreme_closeup"  # 大特写


class ShotAngle(str, Enum):
    """拍摄角度枚举"""
    EYE_LEVEL = "eye_level"  # 平视
    LOW = "low"  # 低角度（仰视）
    HIGH = "high"  # 高角度（俯视）
    DUTCH = "dutch"  # 荷兰角（倾斜）
    BIRDS_EYE = "birds_eye"  # 鸟瞰
    WORMS_EYE = "worms_eye"  # 虫视（极低角度）


class LightingStyle(str, Enum):
    """光影风格枚举"""
    NATURAL = "natural"  # 自然光
    RIM = "rim"  # 轮廓光
    VOLUMETRIC = "volumetric"  # 体积光
    CHIAROSCURO = "chiaroscuro"  # 明暗对比
    SOFT = "soft"  # 柔光
    HARD = "hard"  # 硬光
    DRAMATIC = "dramatic"  # 戏剧性光影
    AMBIENT = "ambient"  # 环境光


def normalize_shot_size(value: Optional[str]) -> str:
    """标准化景别值，返回枚举值或默认值"""
    if not value:
        return ShotSize.MEDIUM.value

    value_lower = value.lower().strip()
    # 映射常见变体
    mapping = {
        "extreme wide": ShotSize.EXTREME_WIDE.value,
        "ews": ShotSize.EXTREME_WIDE.value,
        "wide": ShotSize.WIDE.value,
        "ws": ShotSize.WIDE.value,
        "medium wide": ShotSize.MEDIUM_WIDE.value,
        "mws": ShotSize.MEDIUM_WIDE.value,
        "medium": ShotSize.MEDIUM.value,
        "ms": ShotSize.MEDIUM.value,
        "medium close": ShotSize.MEDIUM_CLOSE.value,
        "mcu": ShotSize.MEDIUM_CLOSE.value,
        "closeup": ShotSize.CLOSEUP.value,
        "cu": ShotSize.CLOSEUP.value,
        "close-up": ShotSize.CLOSEUP.value,
        "extreme closeup": ShotSize.EXTREME_CLOSEUP.value,
        "ecu": ShotSize.EXTREME_CLOSEUP.value,
        "extreme close-up": ShotSize.EXTREME_CLOSEUP.value,
    }

    if value_lower in mapping:
        return mapping[value_lower]

    # 尝试匹配枚举值
    for enum_val in ShotSize:
        if enum_val.value == value_lower:
            return enum_val.value

    # 默认返回 medium
    return ShotSize.MEDIUM.value


def normalize_shot_angle(value: Optional[str]) -> str:
    """标准化拍摄角度值"""
    if not value:
        return ShotAngle.EYE_LEVEL.value

    value_lower = value.lower().strip()
    mapping = {
        "eye level": ShotAngle.EYE_LEVEL.value,
        "eye-level": ShotAngle.EYE_LEVEL.value,
        "low": ShotAngle.LOW.value,
        "low angle": ShotAngle.LOW.value,
        "high": ShotAngle.HIGH.value,
        "high angle": ShotAngle.HIGH.value,
        "dutch": ShotAngle.DUTCH.value,
        "dutch angle": ShotAngle.DUTCH.value,
        "birds eye": ShotAngle.BIRDS_EYE.value,
        "bird's eye": ShotAngle.BIRDS_EYE.value,
        "worms eye": ShotAngle.WORMS_EYE.value,
        "worm's eye": ShotAngle.WORMS_EYE.value,
    }

    if value_lower in mapping:
        return mapping[value_lower]

    for enum_val in ShotAngle:
        if enum_val.value == value_lower:
            return enum_val.value

    return ShotAngle.EYE_LEVEL.value


def normalize_lighting_style(value: Optional[str]) -> str:
    """标准化光影风格值"""
    if not value:
        return LightingStyle.NATURAL.value

    value_lower = value.lower().strip()
    mapping = {
        "natural": LightingStyle.NATURAL.value,
        "rim": LightingStyle.RIM.value,
        "rim light": LightingStyle.RIM.value,
        "volumetric": LightingStyle.VOLUMETRIC.value,
        "volumetric light": LightingStyle.VOLUMETRIC.value,
        "chiaroscuro": LightingStyle.CHIAROSCURO.value,
        "soft": LightingStyle.SOFT.value,
        "soft light": LightingStyle.SOFT.value,
        "hard": LightingStyle.HARD.value,
        "hard light": LightingStyle.HARD.value,
        "dramatic": LightingStyle.DRAMATIC.value,
        "dramatic lighting": LightingStyle.DRAMATIC.value,
        "ambient": LightingStyle.AMBIENT.value,
        "ambient light": LightingStyle.AMBIENT.value,
    }

    if value_lower in mapping:
        return mapping[value_lower]

    for enum_val in LightingStyle:
        if enum_val.value == value_lower:
            return enum_val.value

    return LightingStyle.NATURAL.value


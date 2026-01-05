"""
Prompt Policy：按 dialect 生成参数和负面提示词
"""
from typing import Dict, Any, Optional
from .. import schemas


def generate_midjourney_params(
    pages: Optional[int] = None,
    aspect_ratio: Optional[str] = None,
    stylize: int = 100,
    version: str = "6",
) -> Dict[str, Any]:
    """
    生成 Midjourney 参数
    :param pages: 目标页数（用于推断 aspect_ratio）
    :param aspect_ratio: 显式指定宽高比（如 "16:9", "1:1"）
    :param stylize: --stylize 值（0-1000，默认 100）
    :param version: --v 版本（默认 "6"）
    :return: params 字典
    """
    params: Dict[str, Any] = {}

    # 推断 aspect_ratio
    if not aspect_ratio and pages:
        # 默认：单页漫画通常是竖版（2:3 或 3:4）
        if pages == 1:
            aspect_ratio = "2:3"
        else:
            # 多页：可能是横版或竖版，默认竖版
            aspect_ratio = "2:3"

    if aspect_ratio:
        params["--ar"] = aspect_ratio

    params["--v"] = version
    params["--stylize"] = stylize

    return params


def generate_negative_prompt(
    anti_psychologizing: bool = True,
    generic_negatives: Optional[list[str]] = None,
    character_consistency_negatives: Optional[list[str]] = None,
) -> str:
    """
    生成负面提示词（用于 StableDiffusion/Flux）
    :param anti_psychologizing: 是否启用反心理化（避免心理描写）
    :param generic_negatives: 通用负面词列表
    :param character_consistency_negatives: 角色一致性负面词（如 "blonde hair" 当角色是黑发时）
    :return: 拼接后的负面提示词
    """
    parts: list[str] = []

    # 通用负面词
    if generic_negatives:
        parts.extend(generic_negatives)
    else:
        # 默认通用负面词
        parts.extend([
            "blurry",
            "low quality",
            "distorted",
            "bad anatomy",
            "extra limbs",
            "watermark",
            "text",
        ])

    # 反心理化
    if anti_psychologizing:
        parts.extend([
            "internal monologue",
            "thought bubble",
            "psychological description",
            "emotion text",
        ])

    # 角色一致性负面词
    if character_consistency_negatives:
        parts.extend(character_consistency_negatives)

    return ", ".join(parts)


def generate_prompt_params_for_dialect(
    dialect: schemas.Dialect,
    pages: Optional[int] = None,
    constraints: Optional[schemas.Constraints] = None,
    character_consistency_negatives: Optional[list[str]] = None,
) -> tuple[Optional[str], Dict[str, Any]]:
    """
    为指定 dialect 生成 negative_prompt 和 params
    :param dialect: 提示词方言
    :param pages: 目标页数
    :param constraints: 约束配置
    :param character_consistency_negatives: 角色一致性负面词
    :return: (negative_prompt, params)
    """
    negative_prompt: Optional[str] = None
    params: Dict[str, Any] = {}

    if dialect == "midjourney_v6":
        params = generate_midjourney_params(
            pages=pages,
            stylize=100,  # 可配置
            version="6",
        )
    elif dialect in ["stable_diffusion", "flux"]:
        negative_prompt = generate_negative_prompt(
            anti_psychologizing=constraints.anti_psychologizing if constraints else True,
            character_consistency_negatives=character_consistency_negatives,
        )

    return negative_prompt, params


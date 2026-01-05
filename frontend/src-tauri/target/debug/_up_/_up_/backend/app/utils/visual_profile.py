import os
import json
from typing import Any, Dict, List, Optional, Tuple

from PIL import Image


def _to_hex(rgb: Tuple[int, int, int]) -> str:
    r, g, b = rgb
    return f"#{r:02X}{g:02X}{b:02X}"


def extract_dominant_palette(image_path: str, k: int = 6) -> List[str]:
    """
    轻量本地实现：
    - 读取图片
    - 缩小 + 量化
    - 输出 k 个主色（hex）

    注意：这是“无 LLM”的保底实现，后续接入 LLM 只需要替换上层生成逻辑，不改接口。
    """
    with Image.open(image_path) as im:
        im = im.convert("RGB")
        im.thumbnail((256, 256))
        # quantize 只能用于 P 模式；先转 RGB -> P
        pim = im.quantize(colors=max(2, min(32, k * 4)), method=2)
        palette = pim.getpalette() or []
        color_counts = pim.getcolors() or []

        # getcolors(): [(count, idx), ...]
        color_counts.sort(reverse=True, key=lambda x: x[0])

        hex_colors: List[str] = []
        for _, idx in color_counts:
            base = idx * 3
            if base + 2 >= len(palette):
                continue
            rgb = (palette[base], palette[base + 1], palette[base + 2])
            hex_colors.append(_to_hex(rgb))
            if len(hex_colors) >= k:
                break

        # 去重但保序
        seen = set()
        uniq = []
        for c in hex_colors:
            if c not in seen:
                uniq.append(c)
                seen.add(c)
        return uniq


def resolve_image_path(data_root: str, image_ref: str) -> str:
    """
    支持两种形态：
    - 绝对路径：直接使用
    - 相对路径：认为是 data_root 下的相对路径（与当前 Asset.file_path 语义一致）
    """
    if os.path.isabs(image_ref):
        return image_ref
    return os.path.join(data_root, image_ref)


def guess_face_stub() -> str:
    # 无 LLM 保底：保证 schema 可用、可扩展
    return "unknown"


def build_visual_dna_string_from_profile_dict(profile_dict: Dict[str, Any]) -> str:
    """
    生成“不可变字符串镜像”（用于 prompt 逐字锁定 / SeriesBible.characters[].visual_dna 复用）。

    设计要点：
    - 只使用稳定字段（不包含 notes / 可变 prompt 文本）
    - 使用确定性序列化（JSON + sorted keys）
    - 避免随意自然语言拼接导致漂移
    """
    stable_obj = {
        "schema": "visual_dna_string@0.1",
        "id": profile_dict.get("id"),
        "name": profile_dict.get("name"),
        "character_core": profile_dict.get("character_core"),
        "technical_specs": profile_dict.get("technical_specs"),
    }
    return json.dumps(stable_obj, ensure_ascii=False, sort_keys=True, separators=(",", ":"))



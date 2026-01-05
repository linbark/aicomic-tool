from __future__ import annotations

import json
from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional


@dataclass
class PromptModules:
    """
    按文档推荐结构组织 system prompt（把 Prompt 视为“软件模块”）。
    """

    role_definition: str
    series_bible: Optional[Dict[str, Any]] = None
    constraints: List[str] = field(default_factory=list)
    instruction: List[str] = field(default_factory=list)
    output_format: str = "json"  # "json" / "fountain" / "text"
    extra_blocks: Dict[str, str] = field(default_factory=dict)


def _xml_escape(s: str) -> str:
    # 只做最小逃逸，避免 XML wrapper 被破坏
    return (
        (s or "")
        .replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
    )


def compose_system_prompt_xml(modules: PromptModules) -> str:
    """
    输出 XML wrapper（纯文本），供 LLM 的 system message 使用。
    注意：这里不追求严格 XML 校验，只追求“结构清晰 + 可读 + 不易漂移”。"""

    parts: List[str] = []
    parts.append("<system_prompt>")

    parts.append("  <role_definition>")
    parts.append(f"    {_xml_escape(modules.role_definition).strip()}")
    parts.append("  </role_definition>")

    if modules.series_bible is not None:
        parts.append("  <series_bible>")
        series_json = json.dumps(modules.series_bible, ensure_ascii=False, indent=2)
        # series_bible 作为 JSON 串嵌入（逃逸 <>&），避免被模型当作自然语言改写
        parts.append(f"    {_xml_escape(series_json)}")
        parts.append("  </series_bible>")

    if modules.constraints:
        parts.append("  <constraints>")
        for c in modules.constraints:
            if not (c or "").strip():
                continue
            parts.append(f"    <constraint>{_xml_escape(c).strip()}</constraint>")
        parts.append("  </constraints>")

    if modules.instruction:
        parts.append("  <instruction>")
        for idx, ins in enumerate(modules.instruction, start=1):
            if not (ins or "").strip():
                continue
            parts.append(f"    <step index=\"{idx}\">{_xml_escape(ins).strip()}</step>")
        parts.append("  </instruction>")

    parts.append("  <output_format>")
    parts.append(f"    {_xml_escape(modules.output_format).strip()}")
    parts.append("  </output_format>")

    if modules.extra_blocks:
        parts.append("  <extras>")
        for k, v in modules.extra_blocks.items():
            if not (v or "").strip():
                continue
            key = _xml_escape(str(k))
            parts.append(f"    <block key=\"{key}\">{_xml_escape(str(v)).strip()}</block>")
        parts.append("  </extras>")

    parts.append("</system_prompt>")
    return "\n".join(parts).strip() + "\n"



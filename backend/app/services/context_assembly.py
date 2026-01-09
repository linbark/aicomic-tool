from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Dict, List, Optional

from ..workflows.memory_schemas import MemoryRecord


@dataclass
class MemoryContextSections:
    """
    生成时上下文拼装产物（可解释/可审计）
    - l3_rules: 世界观/角色设定（静态真值）
    - l2_events: 事件链/状态变更（动态记忆）
    - constraints: 负向约束（必须遵守）
    - audit: 本次拼装使用到的 record_id 列表（便于追溯）
    """

    l3_rules: str = ""
    l2_events: str = ""
    l1_buffer: str = ""
    constraints: str = ""
    audit_record_ids: List[str] = None

    def to_markdown(self) -> str:
        parts: List[str] = []
        if self.l3_rules.strip():
            parts.append(f"## L3 世界观与角色设定（真值优先）\n{self.l3_rules.strip()}")
        if self.l2_events.strip():
            parts.append(f"## L2 事件链与剧情进展\n{self.l2_events.strip()}")
        if self.l1_buffer.strip():
            parts.append(f"## L1 当前工作集（Buffer）\n{self.l1_buffer.strip()}")
        if self.constraints.strip():
            parts.append(f"## 约束条件（必须遵守）\n{self.constraints.strip()}")
        # audit 默认不直接注入 prompt（避免噪声），由上层决定是否显示
        return "\n\n".join(parts).strip()


def _format_records(records: List[MemoryRecord]) -> str:
    # 保持和现有 _build_memory_context 类似的简洁格式
    return "\n".join([f"- {r.content}" for r in records if (r.content or "").strip()]).strip()


def assemble_memory_context(
    retrieval_results: Dict[str, Any],
    *,
    include_layers: Optional[List[str]] = None,
    buffer_messages: Optional[List[Dict[str, str]]] = None,
) -> MemoryContextSections:
    """
    将 MemoryRetriever 的分层检索结果拼装为生成时可注入的上下文模板。
    约定：
    - L3: STATIC_BIBLE
    - L2: EPISODIC + DYNAMIC_PLOT
    - constraints: WORLD_RULES_NEGATIVE
    """
    include_layers = include_layers or ["L2_static", "L1", "L2_dynamic", "negative_constraints"]

    audit_ids: List[str] = []

    def _records_of(key: str) -> List[MemoryRecord]:
        res = retrieval_results.get(key)
        if not res:
            return []
        records = getattr(res, "records", None)
        if not records:
            return []
        for r in records:
            rid = getattr(r, "id", None)
            if rid:
                audit_ids.append(str(rid))
        return records

    static_records = _records_of("L2_static") if "L2_static" in include_layers else []
    episodic_records = _records_of("L1") if "L1" in include_layers else []
    dynamic_records = _records_of("L2_dynamic") if "L2_dynamic" in include_layers else []
    negative_records = _records_of("negative_constraints") if "negative_constraints" in include_layers else []

    l3_rules = _format_records(static_records)
    l2_events = "\n".join(
        [s for s in [_format_records(episodic_records), _format_records(dynamic_records)] if s]
    ).strip()
    l1_buffer = ""
    if buffer_messages:
        # 只保留最近若干条，避免噪声；上层可自行裁剪
        recent = buffer_messages[-10:]
        l1_buffer = "\n".join([f"- [{m.get('role','')}] {m.get('content','')}" for m in recent]).strip()
    constraints = _format_records(negative_records)

    return MemoryContextSections(
        l3_rules=l3_rules,
        l2_events=l2_events,
        l1_buffer=l1_buffer,
        constraints=constraints,
        audit_record_ids=audit_ids,
    )


"""
EntityResolver v0（强降噪）

目标：
- 对 changeset.v0 payload 的 entities/snapshots 做“谨慎归一”
- 只在【唯一命中 + 类型一致】时自动复用既有 entity_id
- 任何歧义（多命中/类型冲突/线索不一致）全部进入 conflicts，不硬合并

匹配优先级：
1) canonical_name 精确匹配（同 entity_type）
2) alias 精确匹配（同 entity_type）

线索校验（v0）：
- 根据名称中出现的关键词做 type hint，若与 entity_type 冲突 → conflict（不改动实体）
"""

from __future__ import annotations

import re
from typing import Any, Dict, List, Optional, Tuple

from .memory_store import MemoryStore
from ..workflows.memory_schemas import MemoryNamespace, MemoryQuery, MemoryType, TruthStatus


EntityType = str  # "character" | "location" | "organization" | "prop"


_TYPE_HINTS: List[Tuple[EntityType, List[str]]] = [
    ("location", ["城", "镇", "村", "乡", "街", "巷", "路", "桥", "河", "湖", "海", "山", "谷", "岭", "林", "洞", "寺", "庙", "宫", "殿", "府", "楼", "阁", "院", "馆", "营", "寨", "港", "站", "区", "岛", "界", "境"]),
    ("organization", ["宗", "门", "派", "帮", "会", "盟", "社", "团", "教", "司", "局", "署", "院", "堂", "阁", "馆", "公司", "集团", "军", "队", "营", "旅", "团"]),
    ("prop", ["剑", "刀", "枪", "弓", "戟", "印", "令", "符", "卷", "书", "图", "镜", "盔", "甲", "丹", "丸", "药", "芯片", "钥匙", "戒", "链", "石", "玉", "匕首", "手枪"]),
]


def _normalize_name(s: str) -> str:
    t = (s or "").strip()
    t = re.sub(r"\s+", "", t)
    return t


def _type_hint(name: str) -> Optional[EntityType]:
    n = _normalize_name(name)
    if not n:
        return None
    best: Optional[Tuple[EntityType, int]] = None
    for et, keys in _TYPE_HINTS:
        score = 0
        for k in keys:
            if k and k in n:
                score += 1
        if score <= 0:
            continue
        if best is None or score > best[1]:
            best = (et, score)
    if not best:
        return None
    # v0：至少命中 2 个关键词才认为“强线索”
    return best[0] if best[1] >= 2 else None


def _unique_by_entity_id(rows: List[Dict[str, Any]]) -> List[Dict[str, Any]]:
    seen = set()
    out: List[Dict[str, Any]] = []
    for r in rows:
        eid = str(r.get("entity_id") or "")
        if not eid or eid in seen:
            continue
        seen.add(eid)
        out.append(r)
    return out


def _vector_entity_candidates(
    *,
    store: MemoryStore,
    project_id: int,
    query_text: str,
    top_k: int = 12,
) -> List[Dict[str, Any]]:
    """
    从向量库召回 Canonical ENTITY 候选。
    返回：[{entity_id, entity_type, canonical_name, aliases, score}]
    """
    q = (query_text or "").strip()
    if not q:
        return []
    res = store.retrieve(
        MemoryQuery(
            project_id=int(project_id),
            query_text=q,
            namespace=MemoryNamespace.CANONICAL,
            type=MemoryType.ENTITY,
            status=TruthStatus.CONFIRMED,
            top_k=int(top_k),
        ),
        use_mmr=True,
    )
    out: List[Dict[str, Any]] = []
    for i, r in enumerate(res.records or []):
        pj = getattr(r, "payload_json", None) or {}
        # canonical_entities 的 MemoryRecord.payload_json 来自 CanonicalEntityPayload.model_dump()
        eid = str(pj.get("entity_id") or r.id or "").strip()
        et = str(pj.get("entity_type") or "").strip()
        name = str(pj.get("canonical_name") or r.entity or "").strip()
        aliases = pj.get("aliases") if isinstance(pj.get("aliases"), list) else []
        score = None
        try:
            score = float((res.scores or [])[i])
        except Exception:
            score = None
        if not eid or not et or not name:
            continue
        out.append(
            {
                "entity_id": eid,
                "entity_type": et,
                "canonical_name": name,
                "aliases": aliases,
                "score": score,
            }
        )
    # 去重
    seen = set()
    uniq: List[Dict[str, Any]] = []
    for c in out:
        eid = str(c.get("entity_id") or "")
        if not eid or eid in seen:
            continue
        seen.add(eid)
        uniq.append(c)
    return uniq


def resolve_changeset_entities(
    *,
    store: MemoryStore,
    project_id: int,
    payload: Dict[str, Any],
) -> Dict[str, Any]:
    resolved, _trace = resolve_changeset_entities_with_trace(store=store, project_id=project_id, payload=payload)
    return resolved


def resolve_changeset_entities_with_trace(
    *,
    store: MemoryStore,
    project_id: int,
    payload: Dict[str, Any],
) -> Tuple[Dict[str, Any], Dict[str, Any]]:
    """
    输入：changeset.v0 payload（字典）
    输出：更新后的 payload + trace（A4）
    - 可能重写 entity_id
    - 追加 conflicts
    - 去重 entities
    """
    out = dict(payload or {})
    entities = out.get("entities")
    snapshots = out.get("snapshots")
    conflicts = out.get("conflicts")
    if not isinstance(entities, list):
        entities = []
    if not isinstance(snapshots, list):
        snapshots = []
    if not isinstance(conflicts, list):
        conflicts = []
    before_conflicts_len = len(conflicts)
    before_entities_len = len(entities)
    before_snapshots_len = len(snapshots)

    # 1) payload 内去重（同 type + canonical_name）
    key_to_entity_id: Dict[str, str] = {}
    deduped_entities: List[Dict[str, Any]] = []
    for e in entities:
        if not isinstance(e, dict):
            continue
        et = str(e.get("entity_type") or "").strip()
        name = str(e.get("canonical_name") or "").strip()
        if not et or not name:
            continue
        k = f"{et}::{_normalize_name(name)}"
        cur_id = str(e.get("entity_id") or "").strip()
        if k in key_to_entity_id and cur_id and key_to_entity_id[k] != cur_id:
            conflicts.append(
                {
                    "conflict_type": "duplicate_in_payload",
                    "entity_id": key_to_entity_id[k],
                    "old_claim": {"entity_id": key_to_entity_id[k], "canonical_name": name, "entity_type": et},
                    "new_claim": {"entity_id": cur_id, "canonical_name": name, "entity_type": et},
                    "suggested_actions": [{"action": "dedupe_entities_in_payload"}],
                }
            )
            # 丢弃重复项，后续把 snapshot 引用改到第一个
            continue
        if cur_id:
            key_to_entity_id.setdefault(k, cur_id)
        deduped_entities.append(e)

    entities = deduped_entities

    # 1.5) payload 内：同名不同 type → conflict（更吵，便于调试）
    name_to_items: Dict[str, List[Dict[str, Any]]] = {}
    for e in entities:
        if not isinstance(e, dict):
            continue
        name = str(e.get("canonical_name") or "").strip()
        if not name:
            continue
        key = _normalize_name(name)
        name_to_items.setdefault(key, []).append(e)
    for key, items in name_to_items.items():
        if len(items) <= 1:
            continue
        types = sorted({str(it.get("entity_type") or "").strip() for it in items if str(it.get("entity_type") or "").strip()})
        if len(types) <= 1:
            continue
        conflicts.append(
            {
                "conflict_type": "name_type_collision",
                "entity_id": None,
                "old_claim": {"canonical_name_norm": key, "types": types},
                "new_claim": {
                    "entities": [
                        {
                            "entity_id": it.get("entity_id"),
                            "entity_type": it.get("entity_type"),
                            "canonical_name": it.get("canonical_name"),
                            "aliases": it.get("aliases") if isinstance(it.get("aliases"), list) else [],
                        }
                        for it in items[:10]
                    ]
                },
                "suggested_actions": [{"action": "split_entities_by_type"}, {"action": "review_entity_type"}],
            }
        )

    # 2) 逐实体尝试匹配 canonical_entities
    remap: Dict[str, str] = {}  # from -> to
    for e in entities:
        et = str(e.get("entity_type") or "").strip()
        name = str(e.get("canonical_name") or "").strip()
        if not et or not name:
            continue

        # 2.1 名称线索校验（不改动，仅记录）
        hint = _type_hint(name)
        if hint and hint != et:
            conflicts.append(
                {
                    "conflict_type": "type_hint_mismatch",
                    "entity_id": str(e.get("entity_id") or None),
                    "old_claim": {"entity_type": et, "canonical_name": name},
                    "new_claim": {"hinted_type": hint},
                    "suggested_actions": [{"action": "review_entity_type"}],
                }
            )

        # 2.2 查 canonical：同名
        cands: List[Dict[str, Any]] = []
        cands.extend(store.find_entities_by_name(project_id=project_id, canonical_name=name, limit=20))
        # 2.3 查 canonical：别名（包含 name 自身与 aliases）
        aliases = e.get("aliases")
        if not isinstance(aliases, list):
            aliases = []
        # name 也可能是别人 alias
        cands.extend(store.find_entities_by_alias(project_id=project_id, alias=name, limit=20))
        for a in aliases[:20]:
            aa = str(a or "").strip()
            if aa:
                cands.extend(store.find_entities_by_alias(project_id=project_id, alias=aa, limit=20))
                # alias 也可能是某实体 canonical_name
                cands.extend(store.find_entities_by_name(project_id=project_id, canonical_name=aa, limit=10))

        cands = _unique_by_entity_id(cands)
        if not cands:
            # A2：补充向量召回（更吵一些，给候选与建议动作）
            qtxt = " ".join([name] + [str(a).strip() for a in aliases if str(a).strip()][:8]).strip()
            vcands = _vector_entity_candidates(store=store, project_id=project_id, query_text=qtxt, top_k=12)
            if vcands:
                # 同名但类型不一致（地点 vs 组织）也会通过向量召回命中，这里直接给 conflict
                same_type = [c for c in vcands if str(c.get("entity_type") or "") == et]
                diff_type = [c for c in vcands if str(c.get("entity_type") or "") != et]
                candidates = (same_type + diff_type)[:10]
                conflicts.append(
                    {
                        "conflict_type": "entity_similar",
                        "entity_id": str(e.get("entity_id") or None),
                        "old_claim": {"entity_type": et, "canonical_name": name, "aliases": aliases},
                        "new_claim": {"candidates": candidates, "query": qtxt},
                        "suggested_actions": [
                            {"action": "choose_existing_entity", "params": {"target_entity_id": c.get("entity_id")}}
                            for c in candidates[:5]
                        ]
                        + [{"action": "create_new_entity"}],
                    }
                )
            continue

        same_type = [c for c in cands if str(c.get("entity_type") or "").strip() == et]
        diff_type = [c for c in cands if str(c.get("entity_type") or "").strip() != et]

        if diff_type and not same_type:
            # 名称命中，但类型完全不一致 → 冲突
            conflicts.append(
                {
                    "conflict_type": "canonical_type_mismatch",
                    "entity_id": str(e.get("entity_id") or None),
                    "old_claim": {"entity_type": et, "canonical_name": name},
                    "new_claim": {"candidates": [{"entity_id": c.get("entity_id"), "entity_type": c.get("entity_type"), "canonical_name": c.get("canonical_name")} for c in diff_type[:5]]},
                    "suggested_actions": [
                        {"action": "create_new_entity"},
                        {"action": "review_and_retype_entity"},
                        {"action": "choose_existing_entity", "params": {"target_entity_id": diff_type[0].get("entity_id") if diff_type else None}},
                    ],
                }
            )
            continue

        if len(same_type) > 1:
            # 同类型多命中 → 歧义
            conflicts.append(
                {
                    "conflict_type": "entity_ambiguous",
                    "entity_id": str(e.get("entity_id") or None),
                    "old_claim": {"entity_type": et, "canonical_name": name},
                    "new_claim": {"candidates": [{"entity_id": c.get("entity_id"), "canonical_name": c.get("canonical_name")} for c in same_type[:8]]},
                    "suggested_actions": [{"action": "choose_existing_entity"}, {"action": "create_new_entity"}],
                }
            )
            continue

        if len(same_type) == 1:
            existing_id = str(same_type[0].get("entity_id") or "").strip()
            if not existing_id:
                continue
            current_id = str(e.get("entity_id") or "").strip()
            if current_id and current_id != existing_id:
                remap[current_id] = existing_id
                e["entity_id"] = existing_id

            # alias 建议合并（不硬写入 canonical，只给 suggested_actions）
            existing_aliases = same_type[0].get("aliases") or []
            if not isinstance(existing_aliases, list):
                existing_aliases = []
            new_aliases = [str(a).strip() for a in aliases if str(a).strip()]
            for a in new_aliases:
                if a and a not in existing_aliases and a != name:
                    conflicts.append(
                        {
                            "conflict_type": "alias_new",
                            "entity_id": existing_id,
                            "old_claim": {"aliases": existing_aliases},
                            "new_claim": {"alias": a},
                            "suggested_actions": [{"action": "merge_alias", "target_entity_id": existing_id, "alias": a}],
                        }
                    )

            # A2：即便已唯一命中，也用向量召回一把，给“高相似其它实体”的提示（更吵，便于调试）
            qtxt = " ".join([name] + new_aliases[:6]).strip()
            vcands = _vector_entity_candidates(store=store, project_id=project_id, query_text=qtxt, top_k=8)
            vcands = [c for c in vcands if str(c.get("entity_id")) != existing_id]
            if vcands:
                conflicts.append(
                    {
                        "conflict_type": "entity_similar",
                        "entity_id": existing_id,
                        "old_claim": {"entity_type": et, "canonical_name": name, "matched_existing": existing_id},
                        "new_claim": {"candidates": vcands[:6], "query": qtxt},
                        "suggested_actions": [{"action": "create_new_entity"}],
                    }
                )

    # 3) remap snapshots entity_id
    if remap:
        for s in snapshots:
            if not isinstance(s, dict):
                continue
            eid = str(s.get("entity_id") or "").strip()
            if eid and eid in remap:
                s["entity_id"] = remap[eid]

    out["entities"] = entities
    out["snapshots"] = snapshots
    out["conflicts"] = conflicts
    # trace（A4）
    conflict_types: Dict[str, int] = {}
    for c in conflicts:
        if not isinstance(c, dict):
            continue
        ct = str(c.get("conflict_type") or "unknown")
        conflict_types[ct] = conflict_types.get(ct, 0) + 1
    trace: Dict[str, Any] = {
        "resolver_version": "entity_resolver.v1_a2",
        "inputs": {
            "project_id": int(project_id),
            "entities_before": before_entities_len,
            "snapshots_before": before_snapshots_len,
            "conflicts_before": before_conflicts_len,
        },
        "outputs": {
            "entities_after": len(entities),
            "snapshots_after": len(snapshots),
            "conflicts_after": len(conflicts),
        },
        "remap_count": len(remap),
        "remap_samples": [{"from": k, "to": v} for k, v in list(remap.items())[:10]],
        "conflict_type_hist": conflict_types,
    }
    return out, trace



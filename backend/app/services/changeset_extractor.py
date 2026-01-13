"""
ChangeSet Extractor (LLM)

从 evidence 列表抽取 changeset.v0 payload：
- entities（character/location/organization/prop）
- snapshots（激进切片：每章可为关键实体生成新的版本切片）

注意：
- 输出必须 Evidence-first：每个 entity/snapshot 必须引用 evidence_ids
- 不做强归一：只给出候选 entity_id；冲突/疑似同名可放 conflicts
"""

from __future__ import annotations

import hashlib
import json
import re
from typing import Any, Dict, List, Optional, Tuple
from uuid import uuid4

from fastapi import HTTPException

from .llm_client import DeepSeekChatClient, LlmChatSettings
from .json_extract import extract_json_any
import logging
import os


log_dir = os.path.join(os.path.dirname(__file__), "..", "logs")
os.makedirs(log_dir, exist_ok=True)
log_file = os.path.join(log_dir, "ai_chat.log")

logger = logging.getLogger(__name__)
logger.setLevel(logging.DEBUG)
handler = logging.FileHandler(log_file)
handler.setLevel(logging.DEBUG)
formatter = logging.Formatter('%(asctime)s - %(name)s - %(levelname)s - %(message)s')
handler.setFormatter(formatter)
logger.addHandler(handler)

def _slugify_name(name: str) -> str:
    n = (name or "").strip()
    n = re.sub(r"\s+", "_", n)
    n = re.sub(r"[^\w\u4e00-\u9fff]+", "_", n)
    n = re.sub(r"_+", "_", n).strip("_")
    return n[:60] or "unnamed"


def _entity_id(entity_type: str, canonical_name: str) -> str:
    base = f"{entity_type}:{canonical_name}".strip()
    h = hashlib.sha1(base.encode("utf-8")).hexdigest()[:10]
    return f"{entity_type}:{_slugify_name(canonical_name)}:{h}"


def _snapshot_id(entity_id: str) -> str:
    # snapshot_id == version_id（引用键）
    return f"snap:{_slugify_name(entity_id)}:{uuid4().hex}"

def _event_id(story_order_base: str) -> str:
    return f"ev:{_slugify_name(story_order_base)}:{uuid4().hex}"


def _state_change_id(story_order_base: str) -> str:
    return f"sc:{_slugify_name(story_order_base)}:{uuid4().hex}"

def _preview(text: str, limit: int = 1200) -> str:
    t = (text or "").strip()
    if len(t) <= limit:
        return t
    return t[:limit] + "…"


def _build_system_prompt() -> str:
    # 这里不用你 workflow 那套 XML system prompt，先走最小可用
    return """你是一位"小说→漫剧记忆抽取器（Memory Extractor）"。

你的任务：基于输入的 evidence 列表，为本章生成一个 changeset.v0 的 JSON payload，用于写入记忆系统。

【关键】输出格式要求：
- 必须输出一个完整的 JSON 对象（JSON object），以 { 开头，以 } 结尾
- 不要输出数组（array），不要只输出 evidence_ids 列表
- 不要使用 Markdown 代码块（不要用 ```json 包裹）
- 不要添加任何解释文字，只输出纯 JSON
- 必须包含 output_skeleton 中定义的所有字段（entities、snapshots、events、state_changes 等）

硬性输出约束（必须遵守）：
1) 只输出一个 JSON object（不要 Markdown，不要解释，不要数组）。
2) schema_version 必须是 "changeset.v0"。
3) 你必须只使用输入 evidence 中明确出现的信息；不允许凭空编造事实。
4) Evidence-first：entities/snapshots/time_constraints/time_blocks/conflicts 里所有"主张"都必须引用 evidence_ids（来自输入 evidence_id）。
5) entity_type 仅允许：character | location | organization | prop
6) snapshots.fields 必须是一个 JSON object，顶层仅使用这些命名空间（可以为空）：visual/personality/goals/relations/items/injuries
7) events/state_changes/time_constraints 也必须 Evidence-first：每条都要引用 evidence_ids（来自输入 evidence_id）

抽取策略（v0）：
- entities：列出本章出现的关键角色/地点/组织/重要道具（道具=prop），并给出 canonical_name、aliases（可选）。
- snapshots（激进切片）：对“关键实体”（尤其是角色/关键道具/关键地点）给出一个本章的 snapshot：
  - snapshot_id 作为版本 ID（version_id），必须唯一。
  - entity_id 必须与 entities 中对应。
  - valid_from_story_order 使用 story_order_base + ".E0001" 这样的形式（E序号可由你从 evidence 列表顺序估计）。
  - why 用一句话说明“为什么本章需要一个新切片”（例如：首次出场/外观细化/关系变化/道具获得/伤势出现）。

- events（本章关键事件，A3）：
  - event_id 必须唯一；story_order 必须提供（用 story_order_base + ".EV0001" 形式）。
  - summary 一句话描述事件；participants 里列出参与实体的 entity_id（如未知可留空）。
  - evidence_ids 必须提供（至少 1 个）。

- state_changes（状态变更，A3）：
  - state_change_id 必须唯一；event_id 必须指向 events 中的某个 event_id。
  - patch_json 是一个 JSON object，表达“谁的什么状态变了”（例如：injuries/items/relations 的变化）。
  - target_entity_id 可选但推荐提供（指向变化的主要对象）。
  - evidence_ids 必须提供（至少 1 个）。

- time_constraints（时间约束，A3）：
  - relation 仅允许：before/after/during/overlaps/flashback_of/within_interval/anchored_to
  - from_event_id/to_event_id/anchor_id 指向 events 或 time_blocks 的 id
  - evidence_ids 必须提供（至少 1 个）

冲突与不确定性：
- 如果同名实体可能与历史实体重复（比如同名地点/同名组织），放入 conflicts，并给 suggested_actions（merge_alias/create_new_entity）。
"""


def _build_user_payload(
    *,
    project_id: int,
    episode_id: Optional[int],
    story_order_base: str,
    evidences: List[Dict[str, Any]],
) -> str:
    # 给模型一个“输出骨架”，降低漂移
    skeleton = {
        "schema_version": "changeset.v0",
        "project_id": project_id,
        "episode_id": episode_id,
        "story_order_base": story_order_base,
        "evidences": [],  # 可留空：evidence 已经入库；payload 里主要是引用 evidence_ids
        "entities": [],
        "snapshots": [],
        "events": [],
        "state_changes": [],
        "time_constraints": [],
        "time_blocks": [],
        "conflicts": [],
        "materialize": {"write_static_bible": True},
    }
    return json.dumps(
        {
            "task": "extract_changeset_v0",
            "output_skeleton": skeleton,
            "evidence_list": evidences,
            "notes": {
                "entity_types_allowed": ["character", "location", "organization", "prop"],
                "snapshot_fields_top_level": ["visual", "personality", "goals", "relations", "items", "injuries"],
                "time_relations_allowed": ["before", "after", "during", "overlaps", "flashback_of", "within_interval", "anchored_to"],
            },
        },
        ensure_ascii=False,
        indent=2,
    )


def _validate_and_normalize_payload(
    *,
    payload: Dict[str, Any],
    project_id: int,
    episode_id: Optional[int],
    story_order_base: str,
    evidence_ids: List[str],
) -> Tuple[Dict[str, Any], List[Dict[str, Any]]]:
    """
    轻量校验+归一：
    - 补齐 project_id/episode_id/schema_version/materialize
    - 为 entities/snapshots 自动生成缺失的 entity_id/snapshot_id
    - 强制 evidence_ids 只能来自输入集合
    - 输出 conflicts（新增）
    """
    conflicts: List[Dict[str, Any]] = []
    out = dict(payload or {})
    out["schema_version"] = "changeset.v0"
    out["project_id"] = int(project_id)
    if episode_id is not None:
        out["episode_id"] = int(episode_id)
    out["story_order_base"] = str(out.get("story_order_base") or story_order_base or "").strip() or story_order_base

    mat = out.get("materialize")
    if not isinstance(mat, dict):
        mat = {}
    if "write_static_bible" not in mat:
        mat["write_static_bible"] = True
    out["materialize"] = mat

    valid_eids = {str(x) for x in (evidence_ids or []) if str(x).strip()}

    entities = out.get("entities")
    if not isinstance(entities, list):
        entities = []
    norm_entities: List[Dict[str, Any]] = []
    for e in entities:
        if not isinstance(e, dict):
            continue
        et = str(e.get("entity_type") or "").strip()
        name = str(e.get("canonical_name") or "").strip()
        if not et or not name:
            continue
        if et not in ("character", "location", "organization", "prop"):
            continue
        eid = str(e.get("entity_id") or "").strip()
        if not eid:
            eid = _entity_id(et, name)
        aliases = e.get("aliases")
        if not isinstance(aliases, list):
            aliases = []
        # evidence 约束（entity 主干可以只有 created_from_evidence_id）
        created_from = str(e.get("created_from_evidence_id") or "").strip()
        if created_from and created_from not in valid_eids:
            conflicts.append(
                {
                    "conflict_type": "invalid_evidence_ref",
                    "entity_id": eid,
                    "old_claim": {"created_from_evidence_id": created_from},
                    "new_claim": {"created_from_evidence_id": None},
                    "suggested_actions": [{"action": "remove_invalid_evidence_ref"}],
                }
            )
            created_from = ""
        norm_entities.append(
            {
                "project_id": project_id,
                "entity_id": eid,
                "entity_type": et,
                "canonical_name": name,
                "aliases": [str(a).strip() for a in aliases if str(a).strip()][:50],
                "status": str(e.get("status") or "confirmed"),
                "confidence": float(e.get("confidence") or 0.8),
                "source_kind": str(e.get("source_kind") or "extracted"),
                "created_from_evidence_id": created_from or None,
            }
        )
    out["entities"] = norm_entities

    snaps = out.get("snapshots")
    if not isinstance(snaps, list):
        snaps = []
    # map by canonical_name for easy reference when LLM forgot entity_id
    name_to_entity_id = {e["canonical_name"]: e["entity_id"] for e in norm_entities}

    norm_snaps: List[Dict[str, Any]] = []
    for s in snaps:
        if not isinstance(s, dict):
            continue
        entity_id = str(s.get("entity_id") or "").strip()
        if not entity_id:
            # allow canonical_name fallback
            cname = str(s.get("canonical_name") or "").strip()
            if cname and cname in name_to_entity_id:
                entity_id = name_to_entity_id[cname]
        if not entity_id:
            continue
        sid = str(s.get("snapshot_id") or "").strip()
        if not sid:
            sid = _snapshot_id(entity_id)
        fields = s.get("fields")
        if not isinstance(fields, dict):
            fields = {}
        # 只保留顶层命名空间
        allow_keys = {"visual", "personality", "goals", "relations", "items", "injuries"}
        fields = {k: fields.get(k) for k in fields.keys() if k in allow_keys}
        for k in allow_keys:
            fields.setdefault(k, {} if k in ("visual", "personality") else [])
        eids = s.get("evidence_ids")
        if not isinstance(eids, list):
            eids = []
        eids = [str(x).strip() for x in eids if str(x).strip()]
        eids = [x for x in eids if x in valid_eids]
        if not eids:
            # snapshot 必须有证据，否则丢弃
            continue
        vfrom = str(s.get("valid_from_story_order") or "").strip()
        if not vfrom:
            vfrom = f"{story_order_base}.E0001"
        norm_snaps.append(
            {
                "project_id": project_id,
                "snapshot_id": sid,
                "entity_id": entity_id,
                "valid_from_story_order": vfrom,
                "valid_to_story_order": s.get("valid_to_story_order"),
                "valid_from_story_time_key": s.get("valid_from_story_time_key"),
                "valid_to_story_time_key": s.get("valid_to_story_time_key"),
                "fields": fields,
                "why": str(s.get("why") or "").strip()[:400] or None,
                "evidence_ids": eids,
                "status": str(s.get("status") or "confirmed"),
                "confidence": float(s.get("confidence") or 0.8),
                "source_kind": str(s.get("source_kind") or "extracted"),
            }
        )
    out["snapshots"] = norm_snaps

    # passthrough arrays
    for k in ["events", "state_changes", "time_constraints", "time_blocks", "conflicts"]:
        if not isinstance(out.get(k), list):
            out[k] = []

    # 3) events（A3）
    raw_events = out.get("events") or []
    norm_events: List[Dict[str, Any]] = []
    event_id_list: List[str] = []
    for idx, ev in enumerate(raw_events):
        if not isinstance(ev, dict):
            continue
        ev_id = str(ev.get("event_id") or "").strip() or _event_id(out["story_order_base"])
        story_order = str(ev.get("story_order") or "").strip() or f"{out['story_order_base']}.EV{str(idx+1).zfill(4)}"
        summary = str(ev.get("summary") or "").strip()
        if not summary:
            continue
        eids = ev.get("evidence_ids")
        if not isinstance(eids, list):
            eids = []
        eids = [str(x).strip() for x in eids if str(x).strip()]
        eids = [x for x in eids if x in valid_eids]
        if not eids:
            continue
        participants = ev.get("participants")
        if not isinstance(participants, list):
            participants = []
        # 支持：participants 里既可以是 entity_id，也可以是 canonical_name（尝试映射）
        mapped: List[str] = []
        for p in participants[:50]:
            ps = str(p or "").strip()
            if not ps:
                continue
            if ps in name_to_entity_id:
                mapped.append(name_to_entity_id[ps])
            else:
                mapped.append(ps)
        norm_events.append(
            {
                "project_id": project_id,
                "event_id": ev_id,
                "story_order": story_order,
                "episode_id": episode_id,
                "scene_id": ev.get("scene_id"),
                "event_type": str(ev.get("event_type") or "").strip() or None,
                "summary": summary,
                "participants": mapped,
                "evidence_ids": eids,
                "status": str(ev.get("status") or "confirmed"),
                "confidence": float(ev.get("confidence") or 0.8),
                "source_kind": str(ev.get("source_kind") or "extracted"),
                "story_time": ev.get("story_time"),
                "story_time_key": ev.get("story_time_key"),
            }
        )
        event_id_list.append(ev_id)
    out["events"] = norm_events

    # 4) state_changes（A3）
    raw_sc = out.get("state_changes") or []
    norm_sc: List[Dict[str, Any]] = []
    valid_event_ids = set(event_id_list)
    for idx, sc in enumerate(raw_sc):
        if not isinstance(sc, dict):
            continue
        sc_id = str(sc.get("state_change_id") or "").strip() or _state_change_id(out["story_order_base"])
        ev_id = str(sc.get("event_id") or "").strip()
        if not ev_id or ev_id not in valid_event_ids:
            continue
        patch = sc.get("patch_json") or sc.get("patch") or {}
        if not isinstance(patch, dict) or not patch:
            continue
        eids = sc.get("evidence_ids")
        if not isinstance(eids, list):
            eids = []
        eids = [str(x).strip() for x in eids if str(x).strip()]
        eids = [x for x in eids if x in valid_eids]
        if not eids:
            continue
        target_entity_id = str(sc.get("target_entity_id") or "").strip()
        if not target_entity_id:
            # 支持 canonical_name fallback
            cname = str(sc.get("target_canonical_name") or "").strip()
            if cname and cname in name_to_entity_id:
                target_entity_id = name_to_entity_id[cname]
        norm_sc.append(
            {
                "project_id": project_id,
                "state_change_id": sc_id,
                "event_id": ev_id,
                "target_entity_id": target_entity_id or None,
                "patch_json": patch,
                "before_snapshot_id": sc.get("before_snapshot_id"),
                "after_snapshot_id": sc.get("after_snapshot_id"),
                "evidence_ids": eids,
                "status": str(sc.get("status") or "confirmed"),
                "confidence": float(sc.get("confidence") or 0.8),
                "source_kind": str(sc.get("source_kind") or "extracted"),
            }
        )
    out["state_changes"] = norm_sc

    # 5) time_constraints（A3）：补齐 project_id + 过滤 evidence_ids
    tcs = out.get("time_constraints") or []
    norm_tcs: List[Dict[str, Any]] = []
    for tc in tcs:
        if not isinstance(tc, dict):
            continue
        rel = str(tc.get("relation") or "").strip()
        if not rel:
            continue
        eids = tc.get("evidence_ids")
        if not isinstance(eids, list):
            eids = []
        eids = [str(x).strip() for x in eids if str(x).strip()]
        eids = [x for x in eids if x in valid_eids]
        if not eids:
            continue
        tc2 = dict(tc)
        tc2["project_id"] = project_id
        tc2["relation"] = rel
        tc2["evidence_ids"] = eids
        tc2.setdefault("status", "hypothesis")
        tc2.setdefault("confidence", 0.6)
        tc2.setdefault("source_kind", "extracted")
        norm_tcs.append(tc2)
    out["time_constraints"] = norm_tcs

    # merge conflicts
    out["conflicts"] = (out.get("conflicts") or []) + conflicts
    return out, conflicts


async def extract_changeset_v0_with_llm(
    *,
    llm_settings: LlmChatSettings,
    project_id: int,
    episode_id: Optional[int],
    story_order_base: str,
    evidences: List[Dict[str, Any]],
) -> Dict[str, Any]:
    payload, _trace = await extract_changeset_v0_with_llm_with_trace(
        llm_settings=llm_settings,
        project_id=project_id,
        episode_id=episode_id,
        story_order_base=story_order_base,
        evidences=evidences,
    )
    return payload


async def extract_changeset_v0_with_llm_with_trace(
    *,
    llm_settings: LlmChatSettings,
    project_id: int,
    episode_id: Optional[int],
    story_order_base: str,
    evidences: List[Dict[str, Any]],
) -> Tuple[Dict[str, Any], Dict[str, Any]]:
    logger.info(f"[extract_changeset] Extracting changeset with LLM: project_id={project_id}, episode_id={episode_id}, story_order_base={story_order_base}, evidence_count={len(evidences)}, llm_settings={llm_settings}")
    if not evidences:
        raise HTTPException(status_code=400, detail="evidences 为空，无法抽取")

    logger.info(f"[extract_changeset] Starting extraction: project_id={project_id}, episode_id={episode_id}, story_order_base={story_order_base}, evidence_count={len(evidences)}")
    
    client = DeepSeekChatClient()
    system_prompt = _build_system_prompt()
    user_payload = _build_user_payload(
        project_id=project_id,
        episode_id=episode_id,
        story_order_base=story_order_base,
        evidences=evidences,
    )
    
    # 记录请求详情
    logger.info(f"[extract_changeset] System prompt length={len(system_prompt)}, preview={system_prompt[:300]}...")
    logger.info(f"[extract_changeset] User payload length={len(user_payload)}")
    logger.info(f"[extract_changeset] User payload preview (first 2000 chars):\n{user_payload[:2000]}")
    
    # 记录 LLM 设置
    logger.info(f"[extract_changeset] LLM settings: model={llm_settings.model}, max_tokens={llm_settings.max_tokens}, temperature={llm_settings.temperature}, timeout={llm_settings.timeout_seconds}")
    
    logger.info(f"[extract_changeset] Calling LLM...")
    content = await client.chat(
        settings=llm_settings,
        messages=[
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_payload},
        ],
    )
    logger.info(f"[extract_changeset] LLM response received, length={len(content) if content else 0}")
    # 记录 LLM 返回的原始内容
    logger.info(f"[extract_changeset] LLM raw response preview (first 3000 chars):\n{content[:3000] if content else 'None'}")
    if content and len(content) > 3000:
        logger.info(f"[extract_changeset] LLM raw response (last 1000 chars):\n...{content[-1000:]}")
    
    try:
        logger.info(f"[extract_changeset] Attempting to extract JSON from response...")
        parsed = extract_json_any(content)
        logger.info(f"[extract_changeset] JSON extraction successful, type={type(parsed)}")
    except Exception as e:
        logger.error(f"[extract_changeset] JSON extraction failed: {type(e).__name__}: {e}")
        logger.error(f"[extract_changeset] Full content length={len(content) if content else 0}")
        logger.error(f"[extract_changeset] Content preview (first 2000 chars): {content[:2000] if content else 'None'}")
        if content and len(content) > 2000:
            logger.error(f"[extract_changeset] Content middle (chars 2000-4000): {content[2000:4000]}")
            logger.error(f"[extract_changeset] Content end (last 1000 chars): {content[-1000:]}")
        raise HTTPException(status_code=502, detail=f"LLM 输出无法解析为 JSON: {type(e).__name__}: {str(e)}")
    
    # 检查解析结果类型
    if isinstance(parsed, list):
        logger.error(f"[extract_changeset] Parsed result is a list (length={len(parsed)}), expected dict")
        logger.error(f"[extract_changeset] List content preview: {parsed[:20] if len(parsed) > 20 else parsed}")
        logger.error(f"[extract_changeset] Full LLM response:\n{content}")
        raise HTTPException(
            status_code=502, 
            detail=f"LLM 返回了数组而不是 JSON 对象。LLM 可能误解了任务，只返回了部分数据（如 evidence_ids 列表）。请检查 prompt 和 LLM 响应。返回的数组长度: {len(parsed)},  配置: {llm_settings}"
        )
    if not isinstance(parsed, dict):
        logger.error(f"[extract_changeset] Parsed result is not a dict: type={type(parsed)}, value={str(parsed)[:500]}")
        logger.error(f"[extract_changeset] Full LLM response:\n{content}")
        raise HTTPException(
            status_code=502, 
            detail=f"LLM 输出无法解析为 JSON object（changeset.v0），实际类型: {type(parsed).__name__}"
        )

    evidence_ids = [str(x.get("evidence_id")) for x in evidences if isinstance(x, dict) and x.get("evidence_id")]
    normalized, _ = _validate_and_normalize_payload(
        payload=parsed,
        project_id=project_id,
        episode_id=episode_id,
        story_order_base=story_order_base,
        evidence_ids=evidence_ids,
    )
    # trace（A4）：尽量短、可审计
    try:
        sys_hash = hashlib.sha1(system_prompt.encode("utf-8")).hexdigest()[:10]
    except Exception:
        sys_hash = "unknown"
    trace: Dict[str, Any] = {
        "prompt_version": "changeset_extractor.v2_a3",
        "system_prompt_sha1_10": sys_hash,
        "inputs": {
            "project_id": project_id,
            "episode_id": episode_id,
            "story_order_base": story_order_base,
            "evidence_count": len(evidences),
        },
        "previews": {
            "user_payload_preview": _preview(user_payload, 900),
            "llm_raw_preview": _preview(content, 1200),
        },
        "counts": {
            "entities": len(normalized.get("entities") or []),
            "snapshots": len(normalized.get("snapshots") or []),
            "events": len(normalized.get("events") or []),
            "state_changes": len(normalized.get("state_changes") or []),
            "time_constraints": len(normalized.get("time_constraints") or []),
            "conflicts": len(normalized.get("conflicts") or []),
        },
    }
    return normalized, trace



"""
记忆存储核心接口（整合向量存储 + SQLite）
提供统一的记忆读写、检索、冲突检测等功能
"""
from __future__ import annotations

import hashlib
import json
import os
import sqlite3
import time
from typing import Any, Dict, List, Optional, Sequence, Tuple

import numpy as np

from .app_paths import app_data_dir
from .embedding_provider import EmbeddingProvider
from .vector_store import VectorStore, get_vector_store
from ..workflows.memory_schemas import (
    CanonicalEntityPayload,
    CanonicalSnapshotPayload,
    MemoryNamespace,
    MemoryQuery,
    MemoryRecord,
    MemoryRetrievalResult,
    MemoryType,
    EvidenceRecordPayload,
    StateChange,
    StoryTime,
    TimeBlock,
    TimeConstraint,
    TruthStatus,
    SourceKind,
)

import logging

log_dir = os.path.join(os.path.dirname(__file__), "..", "logs")
os.makedirs(log_dir, exist_ok=True)
log_file = os.path.join(log_dir, "memory_store.log")
logger = logging.getLogger(__name__)
logger.setLevel(logging.DEBUG)
handler = logging.FileHandler(log_file)
handler.setLevel(logging.DEBUG)
formatter = logging.Formatter('%(asctime)s - %(name)s - %(levelname)s - %(message)s')
handler.setFormatter(formatter)
logger.addHandler(handler)

class MemoryStore:
    """
    记忆存储核心类
    整合向量存储（语义检索）和 SQLite（结构化存储，特别是 episodic 记忆）
    """

    def __init__(
        self,
        vector_store: Optional[VectorStore] = None,
        embedding_provider: Optional[EmbeddingProvider] = None,
        db_path: Optional[str] = None,
    ):
        """
        初始化记忆存储

        Args:
            vector_store: 向量存储实例（None 则使用默认单例）
            embedding_provider: Embedding 提供者（None 则使用默认单例）
            db_path: SQLite 数据库路径（None 则使用默认路径）
        """
        import time
        
        t_start = time.time()
        logger.info("[MemoryStore.__init__] Starting initialization...")
        
        t_vs_start = time.time()
        logger.info("[MemoryStore.__init__] Getting vector_store...")
        self.vector_store = vector_store or get_vector_store()
        t_vs_end = time.time()
        logger.info(f"[MemoryStore.__init__] Got vector_store in {t_vs_end - t_vs_start:.2f}s")
        
        self.embedding_provider = embedding_provider or self.vector_store.embedding_provider

        # SQLite 数据库（用于 episodic 记忆的结构化存储）
        if db_path is None:
            db_path = os.path.join(app_data_dir(), "memory_store.db")
        self.db_path = db_path
        
        t_db_start = time.time()
        logger.info("[MemoryStore.__init__] Initializing database...")
        self._init_db()
        t_db_end = time.time()
        logger.info(f"[MemoryStore.__init__] Database initialized in {t_db_end - t_db_start:.2f}s")
        
        t_end = time.time()
        logger.info(f"[MemoryStore.__init__] Initialization completed in {t_end - t_start:.2f}s")

    def _init_db(self) -> None:
        """初始化 SQLite 数据库表结构"""
        os.makedirs(os.path.dirname(self.db_path), exist_ok=True)

        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()

        # 记忆记录表（主表）
        cursor.execute(
            """
            CREATE TABLE IF NOT EXISTS memory_records (
                id TEXT PRIMARY KEY,
                project_id INTEGER NOT NULL,
                namespace TEXT NOT NULL,
                type TEXT NOT NULL,
                entity TEXT,
                content TEXT NOT NULL,
                payload_json TEXT,
                source_ref TEXT,
                time_index TEXT,
                status TEXT,
                confidence REAL,
                source_kind TEXT,
                evidence_ids_json TEXT,
                story_order TEXT,
                story_time_json TEXT,
                hash TEXT,
                created_at_ms INTEGER,
                updated_at_ms INTEGER
            )
            """
        )

        # 轻量迁移：为老库补列（避免 SELECT * 下标错位）
        self._ensure_columns(
            cursor,
            "memory_records",
            [
                ("status", "TEXT"),
                ("confidence", "REAL"),
                ("source_kind", "TEXT"),
                ("evidence_ids_json", "TEXT"),
                ("story_order", "TEXT"),
                ("story_time_json", "TEXT"),
            ],
        )

        # Episodic 记忆表（状态变更）
        cursor.execute(
            """
            CREATE TABLE IF NOT EXISTS episodic_memories (
                id TEXT PRIMARY KEY,
                project_id INTEGER NOT NULL,
                event TEXT NOT NULL,
                state_changes_json TEXT NOT NULL,
                entities_json TEXT NOT NULL,
                episode_id INTEGER,
                scene_id INTEGER,
                beat_index INTEGER,
                created_at_ms INTEGER,
                FOREIGN KEY (id) REFERENCES memory_records(id)
            )
            """
        )

        # Canonical：证据库（Evidence Store）
        cursor.execute(
            """
            CREATE TABLE IF NOT EXISTS canonical_evidences (
                evidence_id TEXT PRIMARY KEY,
                project_id INTEGER NOT NULL,
                episode_id INTEGER,
                scene_id INTEGER,
                span_json TEXT,
                quote TEXT NOT NULL,
                speaker TEXT,
                tags_json TEXT,
                created_at_ms INTEGER
            )
            """
        )
        cursor.execute(
            "CREATE INDEX IF NOT EXISTS idx_canonical_evidences_project ON canonical_evidences(project_id)"
        )

        # Canonical：实体主干（Entity）
        cursor.execute(
            """
            CREATE TABLE IF NOT EXISTS canonical_entities (
                entity_id TEXT PRIMARY KEY,
                project_id INTEGER NOT NULL,
                entity_type TEXT NOT NULL,
                canonical_name TEXT NOT NULL,
                aliases_json TEXT,
                status TEXT,
                confidence REAL,
                source_kind TEXT,
                created_from_evidence_id TEXT,
                created_at_ms INTEGER,
                updated_at_ms INTEGER
            )
            """
        )
        cursor.execute(
            "CREATE INDEX IF NOT EXISTS idx_canonical_entities_project ON canonical_entities(project_id)"
        )
        cursor.execute(
            "CREATE INDEX IF NOT EXISTS idx_canonical_entities_name ON canonical_entities(project_id, canonical_name)"
        )

        # Canonical：事件主轴（Event）
        cursor.execute(
            """
            CREATE TABLE IF NOT EXISTS canonical_events (
                event_id TEXT PRIMARY KEY,
                project_id INTEGER NOT NULL,
                story_order TEXT NOT NULL,
                story_time_key TEXT,
                story_time_json TEXT,
                episode_id INTEGER,
                scene_id INTEGER,
                event_type TEXT,
                summary TEXT NOT NULL,
                participants_json TEXT,
                evidence_ids_json TEXT,
                status TEXT,
                confidence REAL,
                source_kind TEXT,
                created_at_ms INTEGER,
                updated_at_ms INTEGER
            )
            """
        )
        cursor.execute(
            "CREATE INDEX IF NOT EXISTS idx_canonical_events_project ON canonical_events(project_id)"
        )
        cursor.execute(
            "CREATE INDEX IF NOT EXISTS idx_canonical_events_story_order ON canonical_events(project_id, story_order)"
        )
        cursor.execute(
            "CREATE INDEX IF NOT EXISTS idx_canonical_events_story_time ON canonical_events(project_id, story_time_key)"
        )

        # Canonical：时间约束（Time Constraints）
        cursor.execute(
            """
            CREATE TABLE IF NOT EXISTS canonical_time_constraints (
                constraint_id TEXT PRIMARY KEY,
                project_id INTEGER NOT NULL,
                relation TEXT NOT NULL,
                from_event_id TEXT,
                to_event_id TEXT,
                anchor_id TEXT,
                interval_json TEXT,
                evidence_ids_json TEXT,
                status TEXT,
                confidence REAL,
                source_kind TEXT,
                created_at_ms INTEGER,
                updated_at_ms INTEGER
            )
            """
        )
        cursor.execute(
            "CREATE INDEX IF NOT EXISTS idx_canonical_time_constraints_project ON canonical_time_constraints(project_id)"
        )

        # Canonical：时间块（TimeBlock，用于回忆/插叙段）
        cursor.execute(
            """
            CREATE TABLE IF NOT EXISTS canonical_time_blocks (
                block_id TEXT PRIMARY KEY,
                project_id INTEGER NOT NULL,
                name TEXT,
                parent_block_id TEXT,
                anchor_id TEXT,
                constraints_json TEXT,
                event_ids_json TEXT,
                status TEXT,
                confidence REAL,
                source_kind TEXT,
                created_at_ms INTEGER,
                updated_at_ms INTEGER
            )
            """
        )
        cursor.execute(
            "CREATE INDEX IF NOT EXISTS idx_canonical_time_blocks_project ON canonical_time_blocks(project_id)"
        )

        # Canonical：版本切片（Snapshot）
        cursor.execute(
            """
            CREATE TABLE IF NOT EXISTS canonical_snapshots (
                snapshot_id TEXT PRIMARY KEY,
                project_id INTEGER NOT NULL,
                entity_id TEXT NOT NULL,
                valid_from_story_time_key TEXT,
                valid_to_story_time_key TEXT,
                valid_from_story_order TEXT,
                valid_to_story_order TEXT,
                fields_json TEXT NOT NULL,
                why TEXT,
                evidence_ids_json TEXT,
                status TEXT,
                confidence REAL,
                source_kind TEXT,
                created_at_ms INTEGER,
                updated_at_ms INTEGER
            )
            """
        )
        cursor.execute(
            "CREATE INDEX IF NOT EXISTS idx_canonical_snapshots_project ON canonical_snapshots(project_id)"
        )
        cursor.execute(
            "CREATE INDEX IF NOT EXISTS idx_canonical_snapshots_entity ON canonical_snapshots(project_id, entity_id)"
        )

        # Canonical：状态变更（StateChange）
        cursor.execute(
            """
            CREATE TABLE IF NOT EXISTS canonical_state_changes (
                state_change_id TEXT PRIMARY KEY,
                project_id INTEGER NOT NULL,
                event_id TEXT NOT NULL,
                target_entity_id TEXT,
                patch_json TEXT NOT NULL,
                before_snapshot_id TEXT,
                after_snapshot_id TEXT,
                evidence_ids_json TEXT,
                status TEXT,
                confidence REAL,
                source_kind TEXT,
                created_at_ms INTEGER,
                updated_at_ms INTEGER
            )
            """
        )
        cursor.execute(
            "CREATE INDEX IF NOT EXISTS idx_canonical_state_changes_project ON canonical_state_changes(project_id)"
        )
        cursor.execute(
            "CREATE INDEX IF NOT EXISTS idx_canonical_state_changes_event ON canonical_state_changes(project_id, event_id)"
        )

        # Canonical：变更集（ChangeSet）与冲突（Conflict）
        cursor.execute(
            """
            CREATE TABLE IF NOT EXISTS canonical_changesets (
                changeset_id TEXT PRIMARY KEY,
                project_id INTEGER NOT NULL,
                episode_id INTEGER,
                payload_json TEXT NOT NULL,
                review_status TEXT NOT NULL,
                review_log_json TEXT,
                created_at_ms INTEGER,
                updated_at_ms INTEGER
            )
            """
        )
        cursor.execute(
            "CREATE INDEX IF NOT EXISTS idx_canonical_changesets_project ON canonical_changesets(project_id)"
        )
        cursor.execute(
            "CREATE INDEX IF NOT EXISTS idx_canonical_changesets_status ON canonical_changesets(project_id, review_status)"
        )

        cursor.execute(
            """
            CREATE TABLE IF NOT EXISTS canonical_conflicts (
                conflict_id TEXT PRIMARY KEY,
                project_id INTEGER NOT NULL,
                changeset_id TEXT,
                conflict_type TEXT NOT NULL,
                entity_id TEXT,
                old_claim_json TEXT,
                new_claim_json TEXT,
                suggested_actions_json TEXT,
                status TEXT NOT NULL,
                resolved_by TEXT,
                resolution_note TEXT,
                created_at_ms INTEGER,
                updated_at_ms INTEGER
            )
            """
        )
        cursor.execute(
            "CREATE INDEX IF NOT EXISTS idx_canonical_conflicts_project ON canonical_conflicts(project_id)"
        )
        cursor.execute(
            "CREATE INDEX IF NOT EXISTS idx_canonical_conflicts_status ON canonical_conflicts(project_id, status)"
        )

        # 索引
        cursor.execute(
            "CREATE INDEX IF NOT EXISTS idx_project_namespace ON memory_records(project_id, namespace)"
        )
        cursor.execute("CREATE INDEX IF NOT EXISTS idx_project_entity ON memory_records(project_id, entity)")
        cursor.execute("CREATE INDEX IF NOT EXISTS idx_time_index ON memory_records(time_index)")
        cursor.execute("CREATE INDEX IF NOT EXISTS idx_episodic_project ON episodic_memories(project_id)")

        conn.commit()
        conn.close()

    # =========================
    # Canonical: Evidence / Entity / Snapshot（结构化真值库）
    # =========================
    def upsert_evidence(self, evidence: EvidenceRecordPayload) -> str:
        """
        写入/更新 Evidence（原文证据切片）。
        - 写入 canonical_evidences
        - 同步写入一条 CANONICAL MemoryRecord 便于语义检索与审计
        """
        now_ms = int(time.time() * 1000)
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()
        cursor.execute(
            """
            INSERT OR REPLACE INTO canonical_evidences
            (evidence_id, project_id, episode_id, scene_id, span_json, quote, speaker, tags_json, created_at_ms)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                evidence.evidence_id,
                int(evidence.project_id),
                evidence.episode_id,
                evidence.scene_id,
                json.dumps(evidence.span.model_dump(), ensure_ascii=False) if evidence.span else None,
                evidence.quote,
                evidence.speaker,
                json.dumps(evidence.tags, ensure_ascii=False) if evidence.tags else None,
                now_ms,
            ),
        )
        conn.commit()
        conn.close()

        # 写入向量/主表：以 quote 为 content（短一点，避免噪声）
        quote = (evidence.quote or "").strip()
        preview = quote if len(quote) <= 200 else quote[:200] + "…"
        tag_txt = ",".join([t for t in (evidence.tags or []) if str(t).strip()][:6])
        readable = f"证据: {preview}"
        if tag_txt:
            readable += f" [tags:{tag_txt}]"
        mem = MemoryRecord(
            id=evidence.evidence_id,
            project_id=int(evidence.project_id),
            namespace=MemoryNamespace.CANONICAL,
            type=MemoryType.EVIDENCE,
            entity=evidence.speaker or None,
            content=readable.strip(),
            payload_json=evidence.model_dump(),
            source_ref="canonical_evidences",
            status=TruthStatus.CONFIRMED,  # 证据本身默认可信（是否采纳由上层条目 status 决定）
            confidence=1.0,
            source_kind=SourceKind.EXTRACTED,
            evidence_ids=[evidence.evidence_id],
            story_order=None,
        )
        self.write(mem, skip_duplicate=False)
        return evidence.evidence_id

    def upsert_entity(self, entity: CanonicalEntityPayload) -> str:
        """
        写入/更新 Entity（实体主干）。
        - 写入 canonical_entities
        - 同步写入一条 CANONICAL MemoryRecord 便于语义检索与审计
        """
        now_ms = int(time.time() * 1000)
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()
        cursor.execute(
            """
            INSERT OR REPLACE INTO canonical_entities
            (entity_id, project_id, entity_type, canonical_name, aliases_json, status, confidence, source_kind,
             created_from_evidence_id, created_at_ms, updated_at_ms)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                entity.entity_id,
                entity.project_id,
                entity.entity_type,
                entity.canonical_name,
                json.dumps(entity.aliases, ensure_ascii=False) if entity.aliases else None,
                entity.status.value,
                float(entity.confidence),
                entity.source_kind.value,
                entity.created_from_evidence_id,
                now_ms,
                now_ms,
            ),
        )
        conn.commit()
        conn.close()

        aliases = ", ".join([a for a in (entity.aliases or []) if str(a).strip()][:8])
        readable = f"实体({entity.entity_type}): {entity.canonical_name}"
        if aliases:
            readable += f"；别名: {aliases}"
        mem = MemoryRecord(
            id=entity.entity_id,
            project_id=entity.project_id,
            namespace=MemoryNamespace.CANONICAL,
            type=MemoryType.ENTITY,
            entity=entity.canonical_name,
            content=readable.strip(),
            payload_json=entity.model_dump(),
            source_ref="canonical_entities",
            status=entity.status,
            confidence=entity.confidence,
            source_kind=entity.source_kind,
            evidence_ids=[entity.created_from_evidence_id] if entity.created_from_evidence_id else [],
        )
        self.write(mem, skip_duplicate=False)
        return entity.entity_id

    def _get_entity_row(self, project_id: int, entity_id: str) -> Optional[Dict[str, Any]]:
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()
        cursor.execute(
            """
            SELECT entity_id, entity_type, canonical_name, aliases_json, status, confidence, source_kind
            FROM canonical_entities
            WHERE project_id = ? AND entity_id = ?
            LIMIT 1
            """,
            (int(project_id), str(entity_id)),
        )
        row = cursor.fetchone()
        conn.close()
        if not row:
            return None
        return {
            "entity_id": row[0],
            "entity_type": row[1],
            "canonical_name": row[2],
            "aliases": json.loads(row[3]) if row[3] else [],
            "status": row[4],
            "confidence": row[5],
            "source_kind": row[6],
        }

    def find_entities_by_name(
        self,
        *,
        project_id: int,
        canonical_name: str,
        limit: int = 20,
    ) -> List[Dict[str, Any]]:
        """按 canonical_name 查询实体（用于归一/冲突提示）。"""
        name = (canonical_name or "").strip()
        if not name:
            return []
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()
        cursor.execute(
            """
            SELECT entity_id, entity_type, canonical_name, aliases_json, status, confidence, source_kind, created_from_evidence_id
            FROM canonical_entities
            WHERE project_id = ? AND canonical_name = ?
            ORDER BY updated_at_ms DESC
            LIMIT ?
            """,
            (int(project_id), name, int(limit)),
        )
        rows = cursor.fetchall()
        conn.close()
        out: List[Dict[str, Any]] = []
        for r in rows:
            out.append(
                {
                    "entity_id": r[0],
                    "entity_type": r[1],
                    "canonical_name": r[2],
                    "aliases": json.loads(r[3]) if r[3] else [],
                    "status": r[4],
                    "confidence": r[5],
                    "source_kind": r[6],
                    "created_from_evidence_id": r[7],
                }
            )
        return out

    def find_entities_by_alias(
        self,
        *,
        project_id: int,
        alias: str,
        limit: int = 20,
    ) -> List[Dict[str, Any]]:
        """
        按 alias 查询实体（SQLite JSON 字符串的最小实现：LIKE 匹配）。
        注意：这是 v0 版本，后续可升级为单独 alias 表或 SQLite JSON1 查询。
        """
        a = (alias or "").strip()
        if not a:
            return []
        # 朴素 JSON 子串： "alias"
        needle = f"\"{a}\""
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()
        cursor.execute(
            """
            SELECT entity_id, entity_type, canonical_name, aliases_json, status, confidence, source_kind, created_from_evidence_id
            FROM canonical_entities
            WHERE project_id = ? AND aliases_json IS NOT NULL AND aliases_json LIKE ?
            ORDER BY updated_at_ms DESC
            LIMIT ?
            """,
            (int(project_id), f"%{needle}%", int(limit)),
        )
        rows = cursor.fetchall()
        conn.close()
        out: List[Dict[str, Any]] = []
        for r in rows:
            out.append(
                {
                    "entity_id": r[0],
                    "entity_type": r[1],
                    "canonical_name": r[2],
                    "aliases": json.loads(r[3]) if r[3] else [],
                    "status": r[4],
                    "confidence": r[5],
                    "source_kind": r[6],
                    "created_from_evidence_id": r[7],
                }
            )
        return out

    def list_evidences_by_ids(
        self,
        *,
        project_id: int,
        evidence_ids: List[str],
    ) -> List[Dict[str, Any]]:
        """批量读取 evidence（用于抽取器输入）。"""
        ids = [str(x).strip() for x in (evidence_ids or []) if str(x).strip()]
        if not ids:
            return []
        # SQLite IN 参数数量有限，但这里一般不会太大；先做最小实现
        placeholders = ",".join(["?"] * len(ids))
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()
        cursor.execute(
            f"""
            SELECT evidence_id, episode_id, scene_id, span_json, quote, speaker, tags_json, created_at_ms
            FROM canonical_evidences
            WHERE project_id = ? AND evidence_id IN ({placeholders})
            """,
            (int(project_id), *ids),
        )
        rows = cursor.fetchall()
        conn.close()
        out: List[Dict[str, Any]] = []
        for r in rows:
            out.append(
                {
                    "evidence_id": r[0],
                    "episode_id": r[1],
                    "scene_id": r[2],
                    "span": json.loads(r[3]) if r[3] else None,
                    "quote": r[4],
                    "speaker": r[5],
                    "tags": json.loads(r[6]) if r[6] else [],
                    "created_at_ms": r[7],
                }
            )
        # 保持输入顺序（便于 story_order_base 派生）
        order = {eid: i for i, eid in enumerate(ids)}
        out.sort(key=lambda x: order.get(str(x.get("evidence_id")), 10**9))
        return out

    @staticmethod
    def _summarize_snapshot_fields(fields: Dict[str, Any]) -> str:
        """
        将自由 fields（顶层命名空间固定）压缩成短文本，适合做 embedding content。
        """
        if not isinstance(fields, dict) or not fields:
            return ""

        def _kv(obj: Any, limit_items: int = 6) -> str:
            if isinstance(obj, dict):
                parts = []
                for k in list(obj.keys())[:limit_items]:
                    v = obj.get(k)
                    if v is None:
                        continue
                    s = str(v)
                    if len(s) > 60:
                        s = s[:60] + "…"
                    parts.append(f"{k}={s}")
                return "；".join(parts)
            if isinstance(obj, list):
                parts = []
                for x in obj[:limit_items]:
                    if isinstance(x, dict):
                        # relations/items/injuries 常见结构
                        to_ = x.get("to") or x.get("target") or x.get("name") or ""
                        ty = x.get("type") or x.get("kind") or ""
                        s = ",".join([p for p in [str(ty).strip(), str(to_).strip()] if p])
                        parts.append(s or str(x)[:80])
                    else:
                        parts.append(str(x)[:80])
                return "；".join([p for p in parts if p])
            s = str(obj)
            return s if len(s) <= 120 else s[:120] + "…"

        order = ["visual", "personality", "goals", "relations", "items", "injuries"]
        chunks: List[str] = []
        for k in order:
            if k not in fields:
                continue
            txt = _kv(fields.get(k))
            if txt:
                chunks.append(f"{k}:{txt}")
        # fallback：有其它键也带一点
        if not chunks:
            for k in list(fields.keys())[:6]:
                txt = _kv(fields.get(k))
                if txt:
                    chunks.append(f"{k}:{txt}")
        out = " | ".join(chunks).strip()
        return out if len(out) <= 480 else out[:480] + "…"

    def upsert_snapshot(self, snapshot: CanonicalSnapshotPayload) -> str:
        """
        写入/更新 Snapshot（版本切片）。
        - 写入 canonical_snapshots
        - 同步写入 CANONICAL MemoryRecord（id = snapshot_id）
        注意：snapshot_id 视为 version_id（引用键）。
        """
        now_ms = int(time.time() * 1000)
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()
        cursor.execute(
            """
            INSERT OR REPLACE INTO canonical_snapshots
            (snapshot_id, project_id, entity_id,
             valid_from_story_time_key, valid_to_story_time_key,
             valid_from_story_order, valid_to_story_order,
             fields_json, why, evidence_ids_json, status, confidence, source_kind,
             created_at_ms, updated_at_ms)
            VALUES (?, ?, ?,
                    ?, ?,
                    ?, ?,
                    ?, ?, ?, ?, ?, ?,
                    ?, ?)
            """,
            (
                snapshot.snapshot_id,
                snapshot.project_id,
                snapshot.entity_id,
                snapshot.valid_from_story_time_key,
                snapshot.valid_to_story_time_key,
                snapshot.valid_from_story_order,
                snapshot.valid_to_story_order,
                json.dumps(snapshot.fields or {}, ensure_ascii=False),
                snapshot.why,
                json.dumps(snapshot.evidence_ids, ensure_ascii=False) if snapshot.evidence_ids else None,
                snapshot.status.value,
                float(snapshot.confidence),
                snapshot.source_kind.value,
                now_ms,
                now_ms,
            ),
        )
        conn.commit()
        conn.close()

        entity_row = self._get_entity_row(project_id=snapshot.project_id, entity_id=snapshot.entity_id)
        entity_name = (entity_row or {}).get("canonical_name") or snapshot.entity_id
        summary = self._summarize_snapshot_fields(snapshot.fields or {})
        vf = snapshot.valid_from_story_order or snapshot.valid_from_story_time_key or ""
        vt = snapshot.valid_to_story_order or snapshot.valid_to_story_time_key or ""
        window = f"{vf or '?'}~{vt or 'now'}"
        readable = f"切片({entity_name})[{window}]: {summary}".strip()
        mem = MemoryRecord(
            id=snapshot.snapshot_id,
            project_id=snapshot.project_id,
            namespace=MemoryNamespace.CANONICAL,
            type=MemoryType.SNAPSHOT,
            entity=entity_name,
            content=readable,
            payload_json={
                "snapshot_id": snapshot.snapshot_id,
                "entity_id": snapshot.entity_id,
                "fields": snapshot.fields,
                "why": snapshot.why,
                "valid_from_story_order": snapshot.valid_from_story_order,
                "valid_to_story_order": snapshot.valid_to_story_order,
                "valid_from_story_time_key": snapshot.valid_from_story_time_key,
                "valid_to_story_time_key": snapshot.valid_to_story_time_key,
            },
            source_ref="canonical_snapshots",
            status=snapshot.status,
            confidence=snapshot.confidence,
            source_kind=snapshot.source_kind,
            evidence_ids=snapshot.evidence_ids,
            story_order=snapshot.valid_from_story_order,
        )
        self.write(mem, skip_duplicate=False)
        return snapshot.snapshot_id

    @staticmethod
    def _ensure_columns(
        cursor: sqlite3.Cursor,
        table_name: str,
        columns: Sequence[Tuple[str, str]],
    ) -> None:
        """为老库补齐缺失列（SQLite 轻量迁移）"""
        cursor.execute(f"PRAGMA table_info({table_name})")
        existing = {row[1] for row in cursor.fetchall()}  # (cid, name, type, notnull, dflt_value, pk)
        for col_name, col_type in columns:
            if col_name in existing:
                continue
            cursor.execute(f"ALTER TABLE {table_name} ADD COLUMN {col_name} {col_type}")

    def write(
        self,
        record: MemoryRecord,
        skip_duplicate: bool = True,
    ) -> str:
        """
        写入记忆条目

        Args:
            record: 记忆条目
            skip_duplicate: 是否跳过重复（基于 hash）

        Returns:
            记录 ID
        """
        # 计算 hash（如果未提供）
        if not record.hash:
            record.hash = self.embedding_provider.compute_hash(record.content)

        # 检查重复（如果启用）
        if skip_duplicate:
            existing = self._find_by_hash(record.project_id, record.hash)
            if existing:
                return existing.id

        # 设置时间戳
        now_ms = int(time.time() * 1000)
        if not record.created_at_ms:
            record.created_at_ms = now_ms

        # 写入向量存储
        self.vector_store.upsert(record)

        # 写入 SQLite（用于结构化查询）
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()

        story_time_json = None
        if record.story_time is not None:
            if isinstance(record.story_time, StoryTime):
                story_time_json = json.dumps(record.story_time.model_dump(), ensure_ascii=False)
            else:
                # 兼容：允许 dict
                story_time_json = json.dumps(record.story_time, ensure_ascii=False)

        cursor.execute(
            """
            INSERT OR REPLACE INTO memory_records
            (id, project_id, namespace, type, entity, content, payload_json, source_ref, time_index,
             status, confidence, source_kind, evidence_ids_json, story_order, story_time_json,
             hash, created_at_ms, updated_at_ms)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?,
                    ?, ?, ?, ?, ?, ?,
                    ?, ?, ?)
            """,
            (
                record.id,
                record.project_id,
                record.namespace.value,
                record.type.value,
                record.entity,
                record.content,
                json.dumps(record.payload_json) if record.payload_json else None,
                record.source_ref,
                record.time_index,
                getattr(record.status, "value", record.status),
                float(record.confidence) if record.confidence is not None else None,
                getattr(record.source_kind, "value", record.source_kind),
                json.dumps(record.evidence_ids, ensure_ascii=False) if record.evidence_ids else None,
                record.story_order,
                story_time_json,
                record.hash,
                record.created_at_ms,
                now_ms,
            ),
        )

        conn.commit()
        conn.close()

        return record.id

    def write_episodic(
        self,
        project_id: int,
        state_change: StateChange,
        source_ref: Optional[str] = None,
    ) -> str:
        """
        写入 Episodic 记忆（状态变更）

        Args:
            project_id: 项目 ID
            state_change: 状态变更
            source_ref: 来源引用

        Returns:
            记录 ID
        """
        # 构建记忆记录
        event_text = f"{state_change.event}. 状态变更: {json.dumps(state_change.state_changes, ensure_ascii=False)}"
        time_index = self._build_time_index(
            state_change.episode_id,
            state_change.scene_id,
            state_change.beat_index,
        )

        record = MemoryRecord(
            project_id=project_id,
            namespace=MemoryNamespace.EPISODIC,
            type=MemoryType.EVENT,
            entity=",".join(state_change.entities) if state_change.entities else None,
            content=event_text,
            payload_json=state_change.model_dump(),
            source_ref=source_ref,
            time_index=time_index,
            created_at_ms=state_change.created_at_ms or int(time.time() * 1000),
        )

        # 写入主表
        record_id = self.write(record)

        # 写入 Episodic 专用表
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()

        cursor.execute(
            """
            INSERT OR REPLACE INTO episodic_memories
            (id, project_id, event, state_changes_json, entities_json, episode_id, scene_id, beat_index, created_at_ms)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                record_id,
                project_id,
                state_change.event,
                json.dumps(state_change.state_changes, ensure_ascii=False),
                json.dumps(state_change.entities, ensure_ascii=False),
                state_change.episode_id,
                state_change.scene_id,
                state_change.beat_index,
                state_change.created_at_ms or int(time.time() * 1000),
            ),
        )

        conn.commit()
        conn.close()

        return record_id

    def retrieve(
        self,
        query: MemoryQuery,
        use_mmr: bool = False,
        mmr_diversity: float = 0.5,
    ) -> MemoryRetrievalResult:
        """
        检索记忆条目

        Args:
            query: 查询请求
            use_mmr: 是否使用 MMR（Maximal Marginal Relevance）提高多样性
            mmr_diversity: MMR 多样性参数（0-1，越大越多样）

        Returns:
            检索结果
        """
        # 基础向量检索
        result = self.vector_store.search(query)

        if not use_mmr or len(result.records) <= 1:
            return result

        # 应用 MMR（避免返回重复/相似的内容）
        return self._apply_mmr(result, query.query_text or "", mmr_diversity)

    def retrieve_hierarchical(
        self,
        project_id: int,
        query_text: Optional[str] = None,
        entity: Optional[str] = None,
        episode_id: Optional[int] = None,
        scene_id: Optional[int] = None,
        top_k_per_layer: Dict[str, int] = None,
    ) -> Dict[str, MemoryRetrievalResult]:
        """
        分层检索（L0 Buffer / L1 Episodic / L2 Static/Dynamic）

        Args:
            project_id: 项目 ID
            query_text: 查询文本
            entity: 实体过滤
            episode_id: 章节 ID（用于过滤 episodic）
            scene_id: 场景 ID（用于过滤 episodic）
            top_k_per_layer: 每层返回数量（默认：L0=5, L1=10, L2_static=5, L2_dynamic=10）

        Returns:
            按层分组的结果
        """
        if top_k_per_layer is None:
            top_k_per_layer = {
                "L0": 5,
                "L1": 10,
                "L2_static": 5,
                "L2_dynamic": 10,
            }

        results: Dict[str, MemoryRetrievalResult] = {}

        # L1: Episodic（时序优先）
        if query_text or entity:
            episodic_query = MemoryQuery(
                project_id=project_id,
                query_text=query_text,
                namespace=MemoryNamespace.EPISODIC,
                entity=entity,
                top_k=top_k_per_layer.get("L1", 10),
            )
            results["L1"] = self.retrieve(episodic_query, use_mmr=True)

        # L2: Static Bible
        static_query = MemoryQuery(
            project_id=project_id,
            query_text=query_text,
            namespace=MemoryNamespace.STATIC_BIBLE,
            entity=entity,
            top_k=top_k_per_layer.get("L2_static", 5),
        )
        results["L2_static"] = self.retrieve(static_query, use_mmr=True)

        # L2: Dynamic Plot
        dynamic_query = MemoryQuery(
            project_id=project_id,
            query_text=query_text,
            namespace=MemoryNamespace.DYNAMIC_PLOT,
            entity=entity,
            top_k=top_k_per_layer.get("L2_dynamic", 10),
        )
        results["L2_dynamic"] = self.retrieve(dynamic_query, use_mmr=True)

        # L0: Buffer（当前工作集，不走向量，从 AgentState 获取）
        # 这里不实现，由调用方传入

        return results

    def detect_conflicts(
        self,
        records: List[MemoryRecord],
    ) -> List[Dict[str, Any]]:
        """
        检测记忆冲突（例如：同一实体的不同设定）

        Args:
            records: 记忆条目列表

        Returns:
            冲突列表（每个冲突包含冲突的记录 ID 和描述）
        """
        conflicts: List[Dict[str, Any]] = []

        # 按实体分组
        entity_groups: Dict[str, List[MemoryRecord]] = {}
        for record in records:
            if record.entity:
                if record.entity not in entity_groups:
                    entity_groups[record.entity] = []
                entity_groups[record.entity].append(record)

        # 检查每个实体的冲突
        for entity, group in entity_groups.items():
            if len(group) <= 1:
                continue

            # 按 namespace 优先级排序（static_bible > episodic > dynamic_plot）
            namespace_priority = {
                MemoryNamespace.STATIC_BIBLE: 3,
                MemoryNamespace.EPISODIC: 2,
                MemoryNamespace.DYNAMIC_PLOT: 1,
            }

            group_sorted = sorted(
                group,
                key=lambda r: namespace_priority.get(r.namespace, 0),
                reverse=True,
            )

            # 检查是否有内容冲突（简化：检查相同 type 的不同 content）
            type_groups: Dict[MemoryType, List[MemoryRecord]] = {}
            for record in group_sorted:
                if record.type not in type_groups:
                    type_groups[record.type] = []
                type_groups[record.type].append(record)

            for mem_type, type_records in type_groups.items():
                if len(type_records) > 1:
                    # 发现潜在冲突
                    contents = [r.content for r in type_records]
                    if len(set(contents)) > 1:
                        conflicts.append(
                            {
                                "entity": entity,
                                "type": mem_type.value,
                                "records": [r.id for r in type_records],
                                "contents": contents,
                                "priority": [namespace_priority.get(r.namespace, 0) for r in type_records],
                            }
                        )

        return conflicts

    # =========================
    # Canonical: 时间约束与时间块
    # =========================
    def upsert_time_constraint(self, constraint: TimeConstraint) -> str:
        """
        写入/更新时间约束（partial order）
        - 同时写入一条向量化的 Canonical 记忆，便于检索与审计
        """
        now_ms = int(time.time() * 1000)

        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()
        cursor.execute(
            """
            INSERT OR REPLACE INTO canonical_time_constraints
            (constraint_id, project_id, relation, from_event_id, to_event_id, anchor_id, interval_json,
             evidence_ids_json, status, confidence, source_kind, created_at_ms, updated_at_ms)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                constraint.id,
                constraint.project_id,
                constraint.relation.value,
                constraint.from_event_id,
                constraint.to_event_id,
                constraint.anchor_id,
                json.dumps(constraint.interval, ensure_ascii=False) if constraint.interval else None,
                json.dumps(constraint.evidence_ids, ensure_ascii=False) if constraint.evidence_ids else None,
                constraint.status.value,
                float(constraint.confidence),
                constraint.source_kind.value,
                now_ms,
                now_ms,
            ),
        )
        conn.commit()
        conn.close()

        # 向量化索引（可选，但默认打开以支持语义召回）
        readable = f"时间约束({constraint.relation.value}): {constraint.from_event_id or ''} -> {constraint.to_event_id or ''}"
        mem = MemoryRecord(
            id=constraint.id,
            project_id=constraint.project_id,
            namespace=MemoryNamespace.CANONICAL,
            type=MemoryType.TIME_CONSTRAINT,
            entity=None,
            content=readable.strip(),
            payload_json=constraint.model_dump(),
            source_ref="canonical_time_constraints",
            status=constraint.status,
            confidence=constraint.confidence,
            source_kind=constraint.source_kind,
            evidence_ids=constraint.evidence_ids,
        )
        self.write(mem, skip_duplicate=False)
        return constraint.id

    def list_time_constraints(
        self,
        project_id: int,
        status: Optional[str] = None,
        limit: int = 200,
    ) -> List[Dict[str, Any]]:
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()
        if status:
            cursor.execute(
                """
                SELECT constraint_id, relation, from_event_id, to_event_id, anchor_id, interval_json,
                       evidence_ids_json, status, confidence, source_kind, created_at_ms, updated_at_ms
                FROM canonical_time_constraints
                WHERE project_id = ? AND status = ?
                ORDER BY updated_at_ms DESC
                LIMIT ?
                """,
                (project_id, status, int(limit)),
            )
        else:
            cursor.execute(
                """
                SELECT constraint_id, relation, from_event_id, to_event_id, anchor_id, interval_json,
                       evidence_ids_json, status, confidence, source_kind, created_at_ms, updated_at_ms
                FROM canonical_time_constraints
                WHERE project_id = ?
                ORDER BY updated_at_ms DESC
                LIMIT ?
                """,
                (project_id, int(limit)),
            )
        rows = cursor.fetchall()
        conn.close()

        out: List[Dict[str, Any]] = []
        for r in rows:
            out.append(
                {
                    "id": r[0],
                    "relation": r[1],
                    "from_event_id": r[2],
                    "to_event_id": r[3],
                    "anchor_id": r[4],
                    "interval": json.loads(r[5]) if r[5] else None,
                    "evidence_ids": json.loads(r[6]) if r[6] else [],
                    "status": r[7],
                    "confidence": r[8],
                    "source_kind": r[9],
                    "created_at_ms": r[10],
                    "updated_at_ms": r[11],
                }
            )
        return out

    def upsert_time_block(self, block: TimeBlock) -> str:
        """
        写入/更新时间块（回忆段/插叙段容器）
        - 同时写入一条向量化 Canonical 记忆，便于检索与审计
        """
        now_ms = int(time.time() * 1000)
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()
        cursor.execute(
            """
            INSERT OR REPLACE INTO canonical_time_blocks
            (block_id, project_id, name, parent_block_id, anchor_id, constraints_json, event_ids_json,
             status, confidence, source_kind, created_at_ms, updated_at_ms)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                block.id,
                block.project_id,
                block.name,
                block.parent_block_id,
                block.anchor_id,
                json.dumps(block.constraint_ids, ensure_ascii=False) if block.constraint_ids else None,
                json.dumps(block.event_ids, ensure_ascii=False) if block.event_ids else None,
                block.status.value,
                float(block.confidence),
                block.source_kind.value,
                now_ms,
                now_ms,
            ),
        )
        conn.commit()
        conn.close()

        readable = f"时间块: {block.name or block.id} (events={len(block.event_ids)})"
        mem = MemoryRecord(
            id=block.id,
            project_id=block.project_id,
            namespace=MemoryNamespace.CANONICAL,
            type=MemoryType.TIME_BLOCK,
            entity=None,
            content=readable,
            payload_json=block.model_dump(),
            source_ref="canonical_time_blocks",
            status=block.status,
            confidence=block.confidence,
            source_kind=block.source_kind,
        )
        self.write(mem, skip_duplicate=False)
        return block.id

    def list_time_blocks(
        self,
        project_id: int,
        status: Optional[str] = None,
        limit: int = 200,
    ) -> List[Dict[str, Any]]:
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()
        if status:
            cursor.execute(
                """
                SELECT block_id, name, parent_block_id, anchor_id, constraints_json, event_ids_json,
                       status, confidence, source_kind, created_at_ms, updated_at_ms
                FROM canonical_time_blocks
                WHERE project_id = ? AND status = ?
                ORDER BY updated_at_ms DESC
                LIMIT ?
                """,
                (project_id, status, int(limit)),
            )
        else:
            cursor.execute(
                """
                SELECT block_id, name, parent_block_id, anchor_id, constraints_json, event_ids_json,
                       status, confidence, source_kind, created_at_ms, updated_at_ms
                FROM canonical_time_blocks
                WHERE project_id = ?
                ORDER BY updated_at_ms DESC
                LIMIT ?
                """,
                (project_id, int(limit)),
            )
        rows = cursor.fetchall()
        conn.close()

        out: List[Dict[str, Any]] = []
        for r in rows:
            out.append(
                {
                    "id": r[0],
                    "name": r[1],
                    "parent_block_id": r[2],
                    "anchor_id": r[3],
                    "constraint_ids": json.loads(r[4]) if r[4] else [],
                    "event_ids": json.loads(r[5]) if r[5] else [],
                    "status": r[6],
                    "confidence": r[7],
                    "source_kind": r[8],
                    "created_at_ms": r[9],
                    "updated_at_ms": r[10],
                }
            )
        return out

    # =========================
    # Canonical: 变更集与冲突（审阅台）
    # =========================
    def create_changeset(
        self,
        project_id: int,
        payload: Dict[str, Any],
        episode_id: Optional[int] = None,
        review_status: str = "pending_review",
    ) -> str:
        """
        创建一个 ChangeSet（提案/提交单）。
        payload 建议包含：proposed_* / conflicts / notes 等；由上层流水线生产。
        """
        from uuid import uuid4

        changeset_id = uuid4().hex
        now_ms = int(time.time() * 1000)
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()
        cursor.execute(
            """
            INSERT INTO canonical_changesets
            (changeset_id, project_id, episode_id, payload_json, review_status, review_log_json, created_at_ms, updated_at_ms)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                changeset_id,
                project_id,
                episode_id,
                json.dumps(payload or {}, ensure_ascii=False),
                review_status,
                json.dumps([], ensure_ascii=False),
                now_ms,
                now_ms,
            ),
        )
        conn.commit()
        conn.close()

        # 同步写入向量库：便于回溯审计（不依赖语义检索也行，但统一存档）
        mem = MemoryRecord(
            id=changeset_id,
            project_id=project_id,
            namespace=MemoryNamespace.CANONICAL,
            type=MemoryType.CHANGESET,
            entity=None,
            content=f"ChangeSet({review_status}): episode_id={episode_id or ''}",
            payload_json={"changeset_id": changeset_id, "episode_id": episode_id, "payload": payload},
            source_ref="canonical_changesets",
        )
        self.write(mem, skip_duplicate=False)

        return changeset_id

    def list_changesets(
        self,
        project_id: int,
        review_status: Optional[str] = None,
        limit: int = 50,
    ) -> List[Dict[str, Any]]:
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()
        if review_status:
            cursor.execute(
                """
                SELECT changeset_id, episode_id, review_status, created_at_ms, updated_at_ms
                FROM canonical_changesets
                WHERE project_id = ? AND review_status = ?
                ORDER BY updated_at_ms DESC
                LIMIT ?
                """,
                (project_id, review_status, int(limit)),
            )
        else:
            cursor.execute(
                """
                SELECT changeset_id, episode_id, review_status, created_at_ms, updated_at_ms
                FROM canonical_changesets
                WHERE project_id = ?
                ORDER BY updated_at_ms DESC
                LIMIT ?
                """,
                (project_id, int(limit)),
            )
        rows = cursor.fetchall()
        conn.close()
        return [
            {
                "changeset_id": r[0],
                "episode_id": r[1],
                "review_status": r[2],
                "created_at_ms": r[3],
                "updated_at_ms": r[4],
            }
            for r in rows
        ]

    def get_changeset(self, changeset_id: str) -> Optional[Dict[str, Any]]:
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()
        cursor.execute(
            """
            SELECT changeset_id, project_id, episode_id, payload_json, review_status, review_log_json, created_at_ms, updated_at_ms
            FROM canonical_changesets
            WHERE changeset_id = ?
            LIMIT 1
            """,
            (changeset_id,),
        )
        row = cursor.fetchone()
        conn.close()
        if not row:
            return None
        return {
            "changeset_id": row[0],
            "project_id": row[1],
            "episode_id": row[2],
            "payload": json.loads(row[3]) if row[3] else {},
            "review_status": row[4],
            "review_log": json.loads(row[5]) if row[5] else [],
            "created_at_ms": row[6],
            "updated_at_ms": row[7],
        }

    def append_changeset_review_log(
        self,
        changeset_id: str,
        entry: Dict[str, Any],
    ) -> None:
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()
        cursor.execute(
            "SELECT review_log_json FROM canonical_changesets WHERE changeset_id = ? LIMIT 1",
            (changeset_id,),
        )
        row = cursor.fetchone()
        logs = json.loads(row[0]) if row and row[0] else []
        logs.append(entry)
        now_ms = int(time.time() * 1000)
        cursor.execute(
            """
            UPDATE canonical_changesets
            SET review_log_json = ?, updated_at_ms = ?
            WHERE changeset_id = ?
            """,
            (json.dumps(logs, ensure_ascii=False), now_ms, changeset_id),
        )
        conn.commit()
        conn.close()

    def append_changeset_review_entry(
        self,
        changeset_id: str,
        entry: Dict[str, Any],
    ) -> None:
        """
        追加 changeset 审阅日志条目（与 append_changeset_review_log 功能相同）
        
        Args:
            changeset_id: 变更集ID
            entry: 要追加的日志条目（字典格式，通常包含 at_ms, action, data 等字段）
        """
        self.append_changeset_review_log(changeset_id, entry)

    def apply_changeset(
        self,
        changeset_id: str,
        reviewer: str = "human",
        note: Optional[str] = None,
    ) -> None:
        """
        审阅台“批准并提交”：
        - 将 changeset 标记为 approved
        - 尝试应用 payload 中可识别的提案（目前覆盖 time_constraints/time_blocks）
        - 追加审计日志
        """
        cs = self.get_changeset(changeset_id)
        if not cs:
            raise ValueError("changeset not found")

        payload = cs.get("payload") or {}

        # materialize：把 confirmed 的“当前 profile”写回旧分层（STATIC_BIBLE/DYNAMIC_PLOT/EPISODIC），
        # 以兼容现有生成链路（MemoryRetriever 主要从 STATIC_BIBLE 取 L3）。
        materialize = payload.get("materialize") if isinstance(payload, dict) else None
        if not isinstance(materialize, dict):
            # 默认：启用 static_bible 写回（否则 canonical 不会进入生成上下文）
            materialize = {"write_static_bible": True}

        # 1) Evidence / Entity / Snapshot（结构化真值落库）
        for ev in payload.get("evidences", []) or []:
            try:
                self.upsert_evidence(EvidenceRecordPayload(**(ev or {})))
            except Exception:
                continue

        for ent in payload.get("entities", []) or []:
            try:
                self.upsert_entity(CanonicalEntityPayload(**(ent or {})))
            except Exception:
                continue

        latest_snapshot_by_entity: Dict[str, CanonicalSnapshotPayload] = {}
        for sn in payload.get("snapshots", []) or []:
            try:
                snap = CanonicalSnapshotPayload(**(sn or {}))
                self.upsert_snapshot(snap)
                # 用于 materialize：每个 entity 只保留最新 profile（覆盖写）
                prev = latest_snapshot_by_entity.get(snap.entity_id)
                if prev is None:
                    latest_snapshot_by_entity[snap.entity_id] = snap
                else:
                    # 选择“更新”的：优先 valid_from_story_order 更大，其次 created_at（此处没有，退化为保持后者）
                    a = (snap.valid_from_story_order or "") < (prev.valid_from_story_order or "")
                    if not a:
                        latest_snapshot_by_entity[snap.entity_id] = snap
            except Exception:
                continue

        # 2) 时间约束与时间块（支持倒叙/插叙/未定区块）
        for tc in payload.get("time_constraints", []) or []:
            try:
                self.upsert_time_constraint(TimeConstraint(**tc))
            except Exception:
                # 上层可通过 conflict 追踪失败原因；这里不中断整单
                continue

        for tb in payload.get("time_blocks", []) or []:
            try:
                self.upsert_time_block(TimeBlock(**tb))
            except Exception:
                continue

        # 3) materialize：写回 STATIC_BIBLE（每实体一个 current profile，覆盖写）
        if bool(materialize.get("write_static_bible")) and latest_snapshot_by_entity:
            for entity_id, snap in latest_snapshot_by_entity.items():
                try:
                    if snap.status != TruthStatus.CONFIRMED:
                        continue
                    entity_row = self._get_entity_row(project_id=snap.project_id, entity_id=entity_id) or {}
                    entity_name = entity_row.get("canonical_name") or entity_id
                    summary = self._summarize_snapshot_fields(snap.fields or {})
                    # 覆盖写：每实体一个固定 id，保证激进切片不会把 L3 撑爆
                    profile_id = f"profile::{entity_id}"
                    content = f"【{entity_name}】当前设定: {summary}".strip()
                    rec = MemoryRecord(
                        id=profile_id,
                        project_id=snap.project_id,
                        namespace=MemoryNamespace.STATIC_BIBLE,
                        type=MemoryType.SNAPSHOT,
                        entity=entity_name,
                        content=content,
                        payload_json={
                            "entity_id": entity_id,
                            "entity_name": entity_name,
                            "current_snapshot_id": snap.snapshot_id,
                            "fields": snap.fields,
                            "why": snap.why,
                            "valid_from_story_order": snap.valid_from_story_order,
                            "valid_to_story_order": snap.valid_to_story_order,
                            "valid_from_story_time_key": snap.valid_from_story_time_key,
                            "valid_to_story_time_key": snap.valid_to_story_time_key,
                        },
                        source_ref=f"changeset:{changeset_id}:materialize_static_bible",
                        status=snap.status,
                        confidence=snap.confidence,
                        source_kind=snap.source_kind,
                        evidence_ids=snap.evidence_ids,
                        story_order=snap.valid_from_story_order,
                    )
                    self.write(rec, skip_duplicate=False)
                except Exception:
                    continue

        # 更新状态
        now_ms = int(time.time() * 1000)
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()
        cursor.execute(
            "UPDATE canonical_changesets SET review_status = ?, updated_at_ms = ? WHERE changeset_id = ?",
            ("approved", now_ms, changeset_id),
        )
        conn.commit()
        conn.close()

        self.append_changeset_review_log(
            changeset_id,
            {
                "at_ms": now_ms,
                "action": "approved",
                "reviewer": reviewer,
                "note": note,
            },
        )

    def reject_changeset(
        self,
        changeset_id: str,
        reviewer: str = "human",
        note: Optional[str] = None,
    ) -> None:
        now_ms = int(time.time() * 1000)
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()
        cursor.execute(
            "UPDATE canonical_changesets SET review_status = ?, updated_at_ms = ? WHERE changeset_id = ?",
            ("rejected", now_ms, changeset_id),
        )
        conn.commit()
        conn.close()
        self.append_changeset_review_log(
            changeset_id,
            {
                "at_ms": now_ms,
                "action": "rejected",
                "reviewer": reviewer,
                "note": note,
            },
        )

    def create_conflict(
        self,
        project_id: int,
        conflict_type: str,
        changeset_id: Optional[str] = None,
        entity_id: Optional[str] = None,
        old_claim: Optional[Dict[str, Any]] = None,
        new_claim: Optional[Dict[str, Any]] = None,
        suggested_actions: Optional[List[Dict[str, Any]]] = None,
        status: str = "open",
    ) -> str:
        from uuid import uuid4

        conflict_id = uuid4().hex
        now_ms = int(time.time() * 1000)
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()
        cursor.execute(
            """
            INSERT INTO canonical_conflicts
            (conflict_id, project_id, changeset_id, conflict_type, entity_id,
             old_claim_json, new_claim_json, suggested_actions_json,
             status, resolved_by, resolution_note, created_at_ms, updated_at_ms)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                conflict_id,
                project_id,
                changeset_id,
                conflict_type,
                entity_id,
                json.dumps(old_claim, ensure_ascii=False) if old_claim else None,
                json.dumps(new_claim, ensure_ascii=False) if new_claim else None,
                json.dumps(suggested_actions, ensure_ascii=False) if suggested_actions else None,
                status,
                None,
                None,
                now_ms,
                now_ms,
            ),
        )
        conn.commit()
        conn.close()

        mem = MemoryRecord(
            id=conflict_id,
            project_id=project_id,
            namespace=MemoryNamespace.CANONICAL,
            type=MemoryType.CONFLICT,
            entity=entity_id,
            content=f"Conflict({conflict_type}): {entity_id or ''}",
            payload_json={
                "conflict_id": conflict_id,
                "changeset_id": changeset_id,
                "conflict_type": conflict_type,
                "entity_id": entity_id,
                "old_claim": old_claim,
                "new_claim": new_claim,
                "suggested_actions": suggested_actions,
                "status": status,
            },
            source_ref="canonical_conflicts",
        )
        self.write(mem, skip_duplicate=False)

        return conflict_id

    def list_conflicts(
        self,
        project_id: int,
        status: Optional[str] = "open",
        limit: int = 100,
    ) -> List[Dict[str, Any]]:
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()
        if status:
            cursor.execute(
                """
                SELECT conflict_id, changeset_id, conflict_type, entity_id,
                       old_claim_json, new_claim_json, suggested_actions_json,
                       status, resolved_by, resolution_note, created_at_ms, updated_at_ms
                FROM canonical_conflicts
                WHERE project_id = ? AND status = ?
                ORDER BY updated_at_ms DESC
                LIMIT ?
                """,
                (project_id, status, int(limit)),
            )
        else:
            cursor.execute(
                """
                SELECT conflict_id, changeset_id, conflict_type, entity_id,
                       old_claim_json, new_claim_json, suggested_actions_json,
                       status, resolved_by, resolution_note, created_at_ms, updated_at_ms
                FROM canonical_conflicts
                WHERE project_id = ?
                ORDER BY updated_at_ms DESC
                LIMIT ?
                """,
                (project_id, int(limit)),
            )
        rows = cursor.fetchall()
        conn.close()
        out: List[Dict[str, Any]] = []
        for r in rows:
            out.append(
                {
                    "conflict_id": r[0],
                    "changeset_id": r[1],
                    "conflict_type": r[2],
                    "entity_id": r[3],
                    "old_claim": json.loads(r[4]) if r[4] else None,
                    "new_claim": json.loads(r[5]) if r[5] else None,
                    "suggested_actions": json.loads(r[6]) if r[6] else [],
                    "status": r[7],
                    "resolved_by": r[8],
                    "resolution_note": r[9],
                    "created_at_ms": r[10],
                    "updated_at_ms": r[11],
                }
            )
        return out

    def resolve_conflict(
        self,
        conflict_id: str,
        resolved_by: str = "human",
        resolution_note: Optional[str] = None,
        status: str = "resolved",
    ) -> None:
        now_ms = int(time.time() * 1000)
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()
        cursor.execute(
            """
            UPDATE canonical_conflicts
            SET status = ?, resolved_by = ?, resolution_note = ?, updated_at_ms = ?
            WHERE conflict_id = ?
            """,
            (status, resolved_by, resolution_note, now_ms, conflict_id),
        )
        conn.commit()
        conn.close()

    def _find_by_hash(self, project_id: int, hash_value: str) -> Optional[MemoryRecord]:
        """根据 hash 查找记录（用于去重）"""
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()

        cursor.execute(
            """
            SELECT
              id, project_id, namespace, type, entity, content, payload_json, source_ref, time_index,
              status, confidence, source_kind, evidence_ids_json, story_order, story_time_json,
              hash, created_at_ms
            FROM memory_records
            WHERE project_id = ? AND hash = ?
            LIMIT 1
            """,
            (project_id, hash_value),
        )

        row = cursor.fetchone()
        conn.close()

        if not row:
            return None

        return self._row_to_record(row)

    def _row_to_record(self, row: tuple) -> MemoryRecord:
        """将数据库行转换为 MemoryRecord"""
        # 列顺序必须与 _find_by_hash 中 SELECT 保持一致
        story_time = None
        story_time_json = row[14]
        if story_time_json:
            try:
                story_time = StoryTime(**json.loads(story_time_json))
            except Exception:
                story_time = None

        evidence_ids: List[str] = []
        if row[12]:
            try:
                evidence_ids = json.loads(row[12]) or []
            except Exception:
                evidence_ids = []

        return MemoryRecord(
            id=row[0],
            project_id=row[1],
            namespace=MemoryNamespace(row[2]),
            type=MemoryType(row[3]),
            entity=row[4],
            content=row[5],
            payload_json=json.loads(row[6]) if row[6] else None,
            source_ref=row[7],
            time_index=row[8],
            status=row[9] or "confirmed",
            confidence=row[10] if row[10] is not None else 1.0,
            source_kind=row[11] or "system",
            evidence_ids=evidence_ids,
            story_order=row[13],
            story_time=story_time,
            hash=row[15],
            created_at_ms=row[16],
        )

    def _apply_mmr(
        self,
        result: MemoryRetrievalResult,
        query_text: str,
        diversity: float,
    ) -> MemoryRetrievalResult:
        """
        应用 MMR（Maximal Marginal Relevance）算法
        在相关性和多样性之间平衡
        """
        if len(result.records) <= 1:
            return result

        # 计算查询向量
        query_vector = self.embedding_provider.embed_single(query_text, normalize=True)

        # 计算所有记录的向量
        record_vectors = self.embedding_provider.embed(
            [r.content for r in result.records],
            normalize=True,
        )

        # MMR 选择
        selected_indices: List[int] = []
        remaining_indices = list(range(len(result.records)))

        # 第一个：最相关的
        if remaining_indices:
            similarities = np.dot(record_vectors, query_vector)
            first_idx = int(np.argmax(similarities))
            selected_indices.append(first_idx)
            remaining_indices.remove(first_idx)

        # 后续：最大化边际相关性
        while remaining_indices and len(selected_indices) < len(result.records):
            best_idx = None
            best_score = -float("inf")

            for idx in remaining_indices:
                # 相关性（与查询的相似度）
                relevance = float(np.dot(record_vectors[idx], query_vector))

                # 多样性（与已选记录的最大相似度）
                if selected_indices:
                    max_similarity = max(
                        float(np.dot(record_vectors[idx], record_vectors[sel_idx]))
                        for sel_idx in selected_indices
                    )
                    diversity_score = 1.0 - max_similarity
                else:
                    diversity_score = 1.0

                # MMR 分数
                mmr_score = diversity * relevance + (1 - diversity) * diversity_score

                if mmr_score > best_score:
                    best_score = mmr_score
                    best_idx = idx

            if best_idx is not None:
                selected_indices.append(best_idx)
                remaining_indices.remove(best_idx)
            else:
                break

        # 重新排序结果
        selected_records = [result.records[i] for i in selected_indices]
        selected_scores = [result.scores[i] for i in selected_indices]

        return MemoryRetrievalResult(
            records=selected_records,
            scores=selected_scores,
            total=result.total,
        )

    @staticmethod
    def _build_time_index(
        episode_id: Optional[int],
        scene_id: Optional[int],
        beat_index: Optional[int],
    ) -> str:
        """构建时序索引字符串"""
        parts = []
        if episode_id is not None:
            parts.append(f"ep{episode_id}")
        if scene_id is not None:
            parts.append(f"sc{scene_id}")
        if beat_index is not None:
            parts.append(f"bt{beat_index}")
        return "_".join(parts) if parts else ""


# 全局单例
_global_memory_store: Optional[MemoryStore] = None


def get_memory_store(
    vector_store: Optional[VectorStore] = None,
    embedding_provider: Optional[EmbeddingProvider] = None,
    db_path: Optional[str] = None,
) -> MemoryStore:
    """获取全局记忆存储（单例模式）"""
    
    global _global_memory_store
    if _global_memory_store is None:
        t_start = time.time()
        logger.info("[MemoryStore] Creating new MemoryStore instance...")
        _global_memory_store = MemoryStore(
            vector_store=vector_store,
            embedding_provider=embedding_provider,
            db_path=db_path,
        )
        t_end = time.time()
        logger.info(f"[MemoryStore] MemoryStore instance created in {t_end - t_start:.2f}s")
    else:
        logger.debug("[MemoryStore] Returning existing MemoryStore instance")
    return _global_memory_store


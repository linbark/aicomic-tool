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
from typing import Any, Dict, List, Optional

import numpy as np

from .app_paths import app_data_dir
from .embedding_provider import EmbeddingProvider
from .vector_store import VectorStore, get_vector_store
from ..workflows.memory_schemas import (
    MemoryNamespace,
    MemoryQuery,
    MemoryRecord,
    MemoryRetrievalResult,
    MemoryType,
    StateChange,
)


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
        self.vector_store = vector_store or get_vector_store()
        self.embedding_provider = embedding_provider or self.vector_store.embedding_provider

        # SQLite 数据库（用于 episodic 记忆的结构化存储）
        if db_path is None:
            db_path = os.path.join(app_data_dir(), "memory_store.db")
        self.db_path = db_path
        self._init_db()

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
                hash TEXT,
                created_at_ms INTEGER,
                updated_at_ms INTEGER
            )
            """
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

        # 索引
        cursor.execute(
            "CREATE INDEX IF NOT EXISTS idx_project_namespace ON memory_records(project_id, namespace)"
        )
        cursor.execute("CREATE INDEX IF NOT EXISTS idx_project_entity ON memory_records(project_id, entity)")
        cursor.execute("CREATE INDEX IF NOT EXISTS idx_time_index ON memory_records(time_index)")
        cursor.execute("CREATE INDEX IF NOT EXISTS idx_episodic_project ON episodic_memories(project_id)")

        conn.commit()
        conn.close()

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

        cursor.execute(
            """
            INSERT OR REPLACE INTO memory_records
            (id, project_id, namespace, type, entity, content, payload_json, source_ref, time_index, hash, created_at_ms, updated_at_ms)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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

    def _find_by_hash(self, project_id: int, hash_value: str) -> Optional[MemoryRecord]:
        """根据 hash 查找记录（用于去重）"""
        conn = sqlite3.connect(self.db_path)
        cursor = conn.cursor()

        cursor.execute(
            "SELECT * FROM memory_records WHERE project_id = ? AND hash = ? LIMIT 1",
            (project_id, hash_value),
        )

        row = cursor.fetchone()
        conn.close()

        if not row:
            return None

        return self._row_to_record(row)

    def _row_to_record(self, row: tuple) -> MemoryRecord:
        """将数据库行转换为 MemoryRecord"""
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
            hash=row[9],
            created_at_ms=row[10],
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
        _global_memory_store = MemoryStore(
            vector_store=vector_store,
            embedding_provider=embedding_provider,
            db_path=db_path,
        )
    return _global_memory_store


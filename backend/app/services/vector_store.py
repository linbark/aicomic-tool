"""
向量存储封装（Qdrant 本地部署）
提供 upsert、search、filter 等基础操作
"""
from __future__ import annotations

import os
import time
from typing import Any, Dict, List, Optional

import numpy as np
from qdrant_client import QdrantClient
from qdrant_client.http import models
from qdrant_client.http.models import Distance, VectorParams

from .app_paths import app_data_dir
from .embedding_provider import get_embedding_provider
from ..workflows.memory_schemas import (
    MemoryNamespace,
    MemoryRecord,
    MemoryQuery,
    MemoryRetrievalResult,
    MemoryType,
)


class VectorStore:
    """
    向量存储封装（基于 Qdrant）
    支持本地部署、metadata 过滤、相似度搜索
    """

    def __init__(
        self,
        collection_name: str = "aicomic_memories",
        qdrant_url: Optional[str] = None,
        qdrant_api_key: Optional[str] = None,
        embedding_model: str = "BAAI/bge-m3",
        embedding_device: str = "cpu",
    ):
        """
        初始化向量存储

        Args:
            collection_name: Qdrant collection 名称
            qdrant_url: Qdrant 服务 URL（None 表示本地模式）
            qdrant_api_key: Qdrant API Key（本地模式不需要）
            embedding_model: Embedding 模型名称
            embedding_device: Embedding 设备
        """
        self.collection_name = collection_name
        self.embedding_provider = get_embedding_provider(
            model_name=embedding_model,
            device=embedding_device,
        )

        # 初始化 Qdrant 客户端
        if qdrant_url:
            self.client = QdrantClient(url=qdrant_url, api_key=qdrant_api_key)
        else:
            # 本地模式：使用文件存储
            qdrant_path = os.path.join(app_data_dir(), "qdrant_db")
            os.makedirs(qdrant_path, exist_ok=True)
            self.client = QdrantClient(path=qdrant_path)

        self._ensure_collection()

    def _ensure_collection(self) -> None:
        """确保 collection 存在，不存在则创建"""
        try:
            self.client.get_collection(self.collection_name)
        except Exception:
            # Collection 不存在，创建它
            embedding_dim = self.embedding_provider.get_dimension()
            self.client.create_collection(
                collection_name=self.collection_name,
                vectors_config=VectorParams(
                    size=embedding_dim,
                    distance=Distance.COSINE,
                ),
            )

    def upsert(
        self,
        record: MemoryRecord,
        vector: Optional[np.ndarray] = None,
    ) -> str:
        """
        插入或更新记忆条目

        Args:
            record: 记忆条目
            vector: 预计算的向量（None 则自动计算）

        Returns:
            记录 ID
        """
        # 计算 embedding（如果未提供）
        if vector is None:
            vector = self.embedding_provider.embed_single(record.content, normalize=True)

        # 构建 payload（用于过滤）
        payload: Dict[str, Any] = {
            "project_id": record.project_id,
            "namespace": record.namespace.value,
            "type": record.type.value,
            "content": record.content,
        }
        if record.entity:
            payload["entity"] = record.entity
        if record.time_index:
            payload["time_index"] = record.time_index
        if record.source_ref:
            payload["source_ref"] = record.source_ref
        if record.payload_json:
            payload["payload_json"] = record.payload_json
        if record.hash:
            payload["hash"] = record.hash
        if record.created_at_ms:
            payload["created_at_ms"] = record.created_at_ms

        # 插入/更新到 Qdrant
        self.client.upsert(
            collection_name=self.collection_name,
            points=[
                models.PointStruct(
                    id=record.id,
                    vector=vector.tolist(),
                    payload=payload,
                )
            ],
        )

        return record.id

    def search(
        self,
        query: MemoryQuery,
    ) -> MemoryRetrievalResult:
        """
        搜索记忆条目

        Args:
            query: 查询请求

        Returns:
            检索结果
        """
        # 构建过滤条件
        must_conditions: List[Any] = [
            models.FieldCondition(
                key="project_id",
                match=models.MatchValue(value=query.project_id),
            )
        ]

        if query.namespace:
            must_conditions.append(
                models.FieldCondition(
                    key="namespace",
                    match=models.MatchValue(value=query.namespace.value),
                )
            )

        if query.type:
            must_conditions.append(
                models.FieldCondition(
                    key="type",
                    match=models.MatchValue(value=query.type.value),
                )
            )

        if query.entity:
            must_conditions.append(
                models.FieldCondition(
                    key="entity",
                    match=models.MatchValue(value=query.entity),
                )
            )

        if query.time_index_from or query.time_index_to:
            # 时间范围过滤（如果 time_index 是数字字符串）
            # 注意：这里简化处理，实际可能需要更复杂的范围查询
            pass  # TODO: 实现时间范围过滤

        filter_condition = models.Filter(must=must_conditions) if must_conditions else None

        # 计算查询向量
        if query.query_text:
            query_vector = self.embedding_provider.embed_single(query.query_text, normalize=True).tolist()
        else:
            # 如果没有查询文本，使用空向量（会返回所有匹配过滤条件的记录）
            query_vector = [0.0] * self.embedding_provider.get_dimension()

        # 执行搜索
        search_results = self.client.search(
            collection_name=self.collection_name,
            query_vector=query_vector,
            query_filter=filter_condition,
            limit=query.top_k,
            score_threshold=query.min_score,
        )

        # 转换为结果格式
        records: List[MemoryRecord] = []
        scores: List[float] = []

        for result in search_results:
            payload = result.payload
            record = MemoryRecord(
                id=str(result.id),
                project_id=payload.get("project_id", 0),
                namespace=MemoryNamespace(payload.get("namespace", "dynamic_plot")),
                type=MemoryType(payload.get("type", "event")),
                entity=payload.get("entity"),
                content=payload.get("content", ""),
                payload_json=payload.get("payload_json"),
                source_ref=payload.get("source_ref"),
                time_index=payload.get("time_index"),
                hash=payload.get("hash"),
                created_at_ms=payload.get("created_at_ms"),
            )
            records.append(record)
            scores.append(result.score or 0.0)

        return MemoryRetrievalResult(
            records=records,
            scores=scores,
            total=len(records),
        )

    def delete_by_id(self, record_id: str) -> bool:
        """根据 ID 删除记录"""
        try:
            self.client.delete(
                collection_name=self.collection_name,
                points_selector=models.PointIdsList(
                    points=[record_id],
                ),
            )
            return True
        except Exception:
            return False

    def delete_by_filter(
        self,
        project_id: int,
        namespace: Optional[MemoryNamespace] = None,
        type: Optional[MemoryType] = None,
        entity: Optional[str] = None,
    ) -> int:
        """
        根据过滤条件批量删除

        Returns:
            删除的记录数
        """
        must_conditions: List[Any] = [
            models.FieldCondition(
                key="project_id",
                match=models.MatchValue(value=project_id),
            )
        ]

        if namespace:
            must_conditions.append(
                models.FieldCondition(
                    key="namespace",
                    match=models.MatchValue(value=namespace.value),
                )
            )

        if type:
            must_conditions.append(
                models.FieldCondition(
                    key="type",
                    match=models.MatchValue(value=type.value),
                )
            )

        if entity:
            must_conditions.append(
                models.FieldCondition(
                    key="entity",
                    match=models.MatchValue(value=entity),
                )
            )

        filter_condition = models.Filter(must=must_conditions)

        # 先搜索找到所有匹配的点
        search_results = self.client.scroll(
            collection_name=self.collection_name,
            scroll_filter=filter_condition,
            limit=10000,  # 最大删除数
        )

        point_ids = [point.id for point in search_results[0]]

        if point_ids:
            self.client.delete(
                collection_name=self.collection_name,
                points_selector=models.PointIdsList(points=point_ids),
            )

        return len(point_ids)


# 全局单例
_global_vector_store: Optional[VectorStore] = None


def get_vector_store(
    collection_name: str = "aicomic_memories",
    qdrant_url: Optional[str] = None,
    qdrant_api_key: Optional[str] = None,
    embedding_model: str = "BAAI/bge-m3",
    embedding_device: str = "cpu",
) -> VectorStore:
    """获取全局向量存储（单例模式）"""
    global _global_vector_store
    if _global_vector_store is None:
        _global_vector_store = VectorStore(
            collection_name=collection_name,
            qdrant_url=qdrant_url,
            qdrant_api_key=qdrant_api_key,
            embedding_model=embedding_model,
            embedding_device=embedding_device,
        )
    return _global_vector_store


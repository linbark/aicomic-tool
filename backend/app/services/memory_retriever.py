"""
记忆检索器：实现分层检索、查询分解、冲突检测
"""
from __future__ import annotations

from typing import Any, Dict, List, Optional

from .memory_store import MemoryStore, get_memory_store
from ..workflows.memory_schemas import (
    MemoryNamespace,
    MemoryQuery,
    MemoryRetrievalResult,
    MemoryType,
    TruthStatus,
)


class MemoryRetriever:
    """
    记忆检索器
    实现查询分解、分层检索、MMR、冲突检测等功能
    """

    def __init__(self, memory_store: Optional[MemoryStore] = None):
        """
        初始化检索器

        Args:
            memory_store: 记忆存储实例（None 则使用默认单例）
        """
        self.memory_store = memory_store or get_memory_store()

    def decompose_query(
        self,
        task_description: str,
        context: Optional[Dict[str, Any]] = None,
    ) -> Dict[str, str]:
        """
        查询分解：将任务描述分解为多个子查询

        Args:
            task_description: 任务描述
            context: 上下文（例如：当前场景、涉及的角色等）

        Returns:
            子查询字典（rules_query, entity_query, plot_query, visual_query）
        """
        # 简化实现：基于关键词和上下文推断
        # 实际实现可以使用 LLM 进行查询分解

        queries: Dict[str, str] = {}

        # 规则查询：检测是否涉及世界规则/禁忌
        if any(keyword in task_description for keyword in ["规则", "禁忌", "不允许", "不能"]):
            queries["rules_query"] = task_description

        # 实体查询：检测角色/道具/地点
        if context:
            entities = context.get("entities", [])
            if entities:
                queries["entity_query"] = f"涉及实体: {', '.join(entities)}"

        # 情节查询：检测是否需要历史情节
        if any(keyword in task_description for keyword in ["之前", "之前发生", "历史", "伏笔"]):
            queries["plot_query"] = task_description

        # 视觉查询：检测是否需要视觉设定
        if any(keyword in task_description for keyword in ["外观", "视觉", "画面", "角色设计"]):
            queries["visual_query"] = task_description

        # 如果没有特定查询，使用原始任务描述
        if not queries:
            queries["general_query"] = task_description

        return queries

    def retrieve_for_task(
        self,
        project_id: int,
        task_description: str,
        context: Optional[Dict[str, Any]] = None,
        entity: Optional[str] = None,
        episode_id: Optional[int] = None,
        scene_id: Optional[int] = None,
        top_k_per_layer: Optional[Dict[str, int]] = None,
    ) -> Dict[str, MemoryRetrievalResult]:
        """
        为任务检索记忆（分层检索）

        Args:
            project_id: 项目 ID
            task_description: 任务描述
            context: 上下文
            entity: 实体过滤
            episode_id: 章节 ID
            scene_id: 场景 ID
            top_k_per_layer: 每层返回数量

        Returns:
            按层分组的结果
        """
        # 查询分解
        queries = self.decompose_query(task_description, context)

        # 分层检索
        results: Dict[str, MemoryRetrievalResult] = {}

        # L1: Episodic（时序优先）
        if "plot_query" in queries or entity:
            episodic_query = MemoryQuery(
                project_id=project_id,
                query_text=queries.get("plot_query") or queries.get("general_query"),
                namespace=MemoryNamespace.EPISODIC,
                entity=entity,
                top_k=top_k_per_layer.get("L1", 10) if top_k_per_layer else 10,
            )
            results["L1"] = self.memory_store.retrieve(episodic_query, use_mmr=True)

        # L2: Static Bible（规则、角色设计等）
        static_queries = []
        if "rules_query" in queries:
            static_queries.append(queries["rules_query"])
        if "visual_query" in queries:
            static_queries.append(queries["visual_query"])

        if static_queries:
            static_query_text = " ".join(static_queries)
            static_query = MemoryQuery(
                project_id=project_id,
                query_text=static_query_text,
                namespace=MemoryNamespace.STATIC_BIBLE,
                entity=entity,
                status=TruthStatus.CONFIRMED,
                top_k=top_k_per_layer.get("L2_static", 5) if top_k_per_layer else 5,
            )
            results["L2_static"] = self.memory_store.retrieve(static_query, use_mmr=True)

        # L2: Dynamic Plot
        if "plot_query" in queries or queries.get("general_query"):
            dynamic_query = MemoryQuery(
                project_id=project_id,
                query_text=queries.get("plot_query") or queries.get("general_query"),
                namespace=MemoryNamespace.DYNAMIC_PLOT,
                entity=entity,
                top_k=top_k_per_layer.get("L2_dynamic", 10) if top_k_per_layer else 10,
            )
            results["L2_dynamic"] = self.memory_store.retrieve(dynamic_query, use_mmr=True)

        # 负向约束（必取）
        negative_query = MemoryQuery(
            project_id=project_id,
            namespace=MemoryNamespace.WORLD_RULES_NEGATIVE,
            status=TruthStatus.CONFIRMED,
            top_k=20,  # 负向约束通常不多，取更多以确保覆盖
        )
        results["negative_constraints"] = self.memory_store.retrieve(negative_query, use_mmr=False)

        return results

    def detect_conflicts(
        self,
        results: Dict[str, MemoryRetrievalResult],
    ) -> List[Dict[str, Any]]:
        """
        检测检索结果中的冲突

        Args:
            results: 检索结果（按层分组）

        Returns:
            冲突列表
        """
        # 合并所有记录
        all_records: List[Any] = []
        for result in results.values():
            all_records.extend(result.records)

        # 使用 memory_store 的冲突检测
        return self.memory_store.detect_conflicts(all_records)

    def format_for_prompt(
        self,
        results: Dict[str, MemoryRetrievalResult],
        max_tokens: Optional[int] = None,
    ) -> Dict[str, str]:
        """
        将检索结果格式化为 prompt 片段

        Args:
            results: 检索结果
            max_tokens: 最大 token 数（用于截断）

        Returns:
            格式化的 prompt 片段（按层分组）
        """
        formatted: Dict[str, str] = {}

        for layer, result in results.items():
            if not result.records:
                continue

            parts: List[str] = []
            for record in result.records:
                parts.append(f"- {record.content}")

            formatted[layer] = "\n".join(parts)

            # TODO: 如果启用 max_tokens，需要截断

        return formatted


# 全局单例
_global_retriever: Optional[MemoryRetriever] = None


def get_memory_retriever(memory_store: Optional[MemoryStore] = None) -> MemoryRetriever:
    """获取全局记忆检索器（单例模式）"""
    global _global_retriever
    if _global_retriever is None:
        _global_retriever = MemoryRetriever(memory_store=memory_store)
    return _global_retriever

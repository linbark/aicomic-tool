"""
Agent Planner：生成检索计划、任务分解
"""
from __future__ import annotations

from typing import Any, Dict, List, Optional

from .memory_retriever import MemoryRetriever
from .memory_retriever import get_memory_retriever
from ..workflows.agent_state import AgentState


class AgentPlanner:
    """
    Agent 规划器
    负责生成检索计划、任务分解、决策下一步行动
    """

    def __init__(self, memory_retriever: Optional[MemoryRetriever] = None):
        """
        初始化规划器

        Args:
            memory_retriever: 记忆检索器（None 则使用默认单例）
        """
        self.memory_retriever = memory_retriever or get_memory_retriever()

    def plan_retrieval(
        self,
        state: AgentState,
        task_description: str,
    ) -> Dict[str, Any]:
        """
        规划检索策略

        Args:
            state: Agent 状态
            task_description: 任务描述

        Returns:
            检索计划（包含查询、过滤条件等）
        """
        # 从 working_set 提取上下文
        context: Dict[str, Any] = {}
        if state.working_set:
            # 提取涉及的实体（例如：从 beat_sheet 或 shots 中）
            entities = self._extract_entities_from_working_set(state.working_set)
            if entities:
                context["entities"] = entities

        # 使用 memory_retriever 进行查询分解和检索
        retrieval_results = self.memory_retriever.retrieve_for_task(
            project_id=state.project_id,
            task_description=task_description,
            context=context,
            episode_id=state.episode_id,
            scene_id=state.scene_id,
        )

        # 检测冲突
        conflicts = self.memory_retriever.detect_conflicts(retrieval_results)

        # 格式化用于 prompt
        formatted_memories = self.memory_retriever.format_for_prompt(retrieval_results)

        return {
            "retrieval_results": retrieval_results,
            "conflicts": conflicts,
            "formatted_memories": formatted_memories,
        }

    def plan_task_decomposition(
        self,
        state: AgentState,
        main_task: str,
    ) -> List[Dict[str, Any]]:
        """
        任务分解：将主任务分解为子任务

        Args:
            state: Agent 状态
            main_task: 主任务描述

        Returns:
            子任务列表
        """
        # 简化实现：基于任务类型和当前状态推断子任务
        # 实际实现可以使用 LLM 进行任务分解

        subtasks: List[Dict[str, Any]] = []

        # 根据任务类型分解
        if "生成剧本" in main_task or "script" in main_task.lower():
            subtasks = [
                {"type": "architect", "description": "生成世界观和节拍表"},
                {"type": "writer", "description": "基于节拍表生成剧本"},
                {"type": "qc", "description": "检查并修订剧本"},
            ]
        elif "分镜" in main_task or "storyboard" in main_task.lower():
            subtasks = [
                {"type": "storyboard", "description": "拆分场景为镜头列表"},
                {"type": "prompt_translate", "description": "将镜头转换为绘图 prompt"},
            ]

        return subtasks

    @staticmethod
    def _extract_entities_from_working_set(working_set: Dict[str, Any]) -> List[str]:
        """从 working_set 中提取实体"""
        entities: List[str] = []

        # 从 beat_sheet 提取
        beat_sheet = working_set.get("beat_sheet")
        if isinstance(beat_sheet, list):
            for beat in beat_sheet:
                if isinstance(beat, dict):
                    # 尝试从描述中提取实体（简化实现）
                    description = beat.get("description") or ""
                    # TODO: 使用更复杂的实体提取

        # 从 shots 提取
        shots = working_set.get("shots")
        if isinstance(shots, list):
            for shot in shots:
                if isinstance(shot, dict):
                    action_text = shot.get("action_text") or ""
                    # TODO: 使用更复杂的实体提取

        return entities


def get_planner(memory_retriever: Optional[MemoryRetriever] = None) -> AgentPlanner:
    """获取规划器实例"""
    return AgentPlanner(memory_retriever=memory_retriever)


# 全局单例
_global_planner: Optional[AgentPlanner] = None


def get_agent_planner(memory_retriever: Optional[MemoryRetriever] = None) -> AgentPlanner:
    """获取全局规划器（单例模式）"""
    global _global_planner
    if _global_planner is None:
        _global_planner = AgentPlanner(memory_retriever=memory_retriever)
    return _global_planner


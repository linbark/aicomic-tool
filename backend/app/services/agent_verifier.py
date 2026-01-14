"""
Agent Verifier：基于 rules/episodic 做硬校验
"""
from __future__ import annotations

from typing import Any, Dict, List, Optional

from .memory_retriever import MemoryRetriever
from .memory_retriever import get_memory_retriever
from ..workflows.agent_state import AgentState
from ..workflows.memory_schemas import MemoryNamespace


class AgentVerifier:
    """
    Agent 校验器
    基于世界规则和 Episodic 记忆对生成内容进行硬校验
    """

    def __init__(self, memory_retriever: Optional[MemoryRetriever] = None):
        """
        初始化校验器

        Args:
            memory_retriever: 记忆检索器（None 则使用默认单例）
        """
        self.memory_retriever = memory_retriever or get_memory_retriever()

    def verify(
        self,
        state: AgentState,
        generated_content: Dict[str, Any],
        content_type: str = "script",
    ) -> Dict[str, Any]:
        """
        校验生成内容

        Args:
            state: Agent 状态
            generated_content: 生成的内容（例如：script_fountain、shots 等）
            content_type: 内容类型（script、storyboard 等）

        Returns:
            校验结果（is_valid, issues, suggestions）
        """
        issues: List[Dict[str, Any]] = []
        suggestions: List[str] = []

        # 1. 检索相关规则和约束
        negative_constraints = state.retrieved_memories.get("negative_constraints")
        if negative_constraints:
            # 检查是否违反负向约束
            for record in negative_constraints.records:
                constraint_text = record.content
                if self._check_violation(generated_content, constraint_text):
                    issues.append(
                        {
                            "type": "negative_constraint_violation",
                            "message": f"违反约束: {constraint_text}",
                            "constraint": constraint_text,
                        }
                    )

        # 2. 检查一致性（与 Episodic 记忆）
        episodic_memories = state.retrieved_memories.get("L1")
        if episodic_memories:
            consistency_issues = self._check_consistency(generated_content, episodic_memories.records)
            issues.extend(consistency_issues)

        # 3. 检查静态设定一致性（角色设计、世界规则等）
        static_memories = state.retrieved_memories.get("L2_static")
        if static_memories:
            static_issues = self._check_static_consistency(generated_content, static_memories.records)
            issues.extend(static_issues)

        # 生成建议
        if issues:
            suggestions.append("请根据上述问题修订生成内容")

        return {
            "is_valid": len(issues) == 0,
            "issues": issues,
            "suggestions": suggestions,
        }

    def _check_violation(
        self,
        content: Dict[str, Any],
        constraint_text: str,
    ) -> bool:
        """
        检查是否违反约束（简化实现）

        Args:
            content: 生成内容
            constraint_text: 约束文本

        Returns:
            是否违反
        """
        # 简化实现：文本匹配
        # 实际实现应该更智能（例如：使用 LLM 判断）

        content_str = str(content).lower()
        constraint_lower = constraint_text.lower()

        # 检测关键词冲突
        if "不允许" in constraint_lower or "禁止" in constraint_lower:
            # 提取禁止的内容
            forbidden_keywords = self._extract_forbidden_keywords(constraint_text)
            for keyword in forbidden_keywords:
                if keyword.lower() in content_str:
                    return True

        return False

    def _check_consistency(
        self,
        content: Dict[str, Any],
        episodic_records: List[Any],
    ) -> List[Dict[str, Any]]:
        """
        检查与 Episodic 记忆的一致性

        Args:
            content: 生成内容
            episodic_records: Episodic 记忆记录

        Returns:
            一致性问题列表
        """
        issues: List[Dict[str, Any]] = []

        # 简化实现：检查状态变更是否一致
        # 例如：如果 episodic 记录显示角色受伤，但新内容中角色没有受伤迹象

        content_str = str(content).lower()

        for record in episodic_records:
            payload = record.payload_json
            if isinstance(payload, dict):
                state_changes = payload.get("state_changes", {})
                # 检查状态变更是否在新内容中体现
                # TODO: 实现更复杂的一致性检查

        return issues

    def _check_static_consistency(
        self,
        content: Dict[str, Any],
        static_records: List[Any],
    ) -> List[Dict[str, Any]]:
        """
        检查与静态设定的一致性

        Args:
            content: 生成内容
            static_records: 静态记忆记录

        Returns:
            一致性问题列表
        """
        issues: List[Dict[str, Any]] = []

        # 检查角色设计一致性
        character_designs = [r for r in static_records if r.type.value == "character_design"]
        for design in character_designs:
            # 检查生成内容中是否使用了正确的角色设计
            # TODO: 实现更复杂的检查逻辑
            pass

        return issues

    @staticmethod
    def _extract_forbidden_keywords(constraint_text: str) -> List[str]:
        """从约束文本中提取禁止的关键词"""
        # 简化实现
        keywords: List[str] = []
        # TODO: 使用更智能的提取方法
        return keywords


def get_verifier(memory_retriever: Optional[MemoryRetriever] = None) -> AgentVerifier:
    """获取校验器实例"""
    return AgentVerifier(memory_retriever=memory_retriever)


# 全局单例
_global_verifier: Optional[AgentVerifier] = None


def get_agent_verifier(memory_retriever: Optional[MemoryRetriever] = None) -> AgentVerifier:
    """获取全局校验器（单例模式）"""
    global _global_verifier
    if _global_verifier is None:
        _global_verifier = AgentVerifier(memory_retriever=memory_retriever)
    return _global_verifier

"""
状态变更提取器（State Change Extractor）
从场景/章节结束时的输出中提取状态变更，写入 Episodic 记忆
"""
from __future__ import annotations

import json
from typing import Any, Dict, List, Optional

from .memory_store import MemoryStore, get_memory_store
from ..workflows.memory_schemas import MemoryNamespace, MemoryRecord, MemoryType, StateChange


class StateChangeExtractor:
    """
    状态变更提取器
    从 LLM 输出或结构化数据中提取状态变更，写入 Episodic 记忆
    """

    def __init__(self, memory_store: Optional[MemoryStore] = None):
        """
        初始化提取器

        Args:
            memory_store: 记忆存储实例（None 则使用默认单例）
        """
        self.memory_store = memory_store or get_memory_store()

    def extract_from_script_fountain(
        self,
        project_id: int,
        script_fountain: str,
        episode_id: Optional[int] = None,
        scene_id: Optional[int] = None,
        source_ref: Optional[str] = None,
    ) -> List[str]:
        """
        从 Fountain 剧本中提取状态变更

        Args:
            project_id: 项目 ID
            script_fountain: Fountain 格式的剧本
            episode_id: 章节 ID
            scene_id: 场景 ID
            source_ref: 来源引用

        Returns:
            创建的记录 ID 列表
        """
        # 简化实现：使用 LLM 提取状态变更
        # 实际实现中，可以：
        # 1. 使用专门的 LLM prompt 提取状态变更
        # 2. 或使用规则/模式匹配（例如：检测"获得"、"失去"、"受伤"等关键词）

        # 这里先返回空列表，实际提取逻辑需要集成 LLM 调用
        # 或者由调用方传入已提取的状态变更列表
        return []

    def extract_from_structured_output(
        self,
        project_id: int,
        structured_data: Dict[str, Any],
        episode_id: Optional[int] = None,
        scene_id: Optional[int] = None,
        source_ref: Optional[str] = None,
    ) -> List[str]:
        """
        从结构化输出中提取状态变更

        Args:
            project_id: 项目 ID
            structured_data: 结构化数据（例如：beat_sheet、shots 等）
            episode_id: 章节 ID
            scene_id: 场景 ID
            source_ref: 来源引用

        Returns:
            创建的记录 ID 列表
        """
        record_ids: List[str] = []

        # 从 beat_sheet 提取
        if isinstance(structured_data.get("beat_sheet"), list):
            for idx, beat in enumerate(structured_data["beat_sheet"]):
                if isinstance(beat, dict):
                    event = beat.get("description") or beat.get("title") or ""
                    if event:
                        state_change = StateChange(
                            event=event,
                            state_changes={
                                "beat_type": beat.get("beat_type"),
                                "emotional_charge": beat.get("emotional_charge"),
                                "visual_focus": beat.get("visual_focus"),
                            },
                            entities=self._extract_entities_from_text(event),
                            episode_id=episode_id,
                            scene_id=scene_id,
                            beat_index=idx,
                        )
                        record_ids.append(
                            self.memory_store.write_episodic(
                                project_id=project_id,
                                state_change=state_change,
                                source_ref=source_ref,
                            )
                        )

        # 从 shots 提取（道具、伤势、人物入场/退场）
        if isinstance(structured_data.get("shots"), list):
            for idx, shot in enumerate(structured_data["shots"]):
                if isinstance(shot, dict):
                    action_text = shot.get("action_text") or ""
                    if action_text:
                        # 检测状态变更关键词
                        state_changes = self._detect_state_changes_from_action(action_text)
                        if state_changes:
                            state_change = StateChange(
                                event=f"镜头 {idx + 1}: {action_text}",
                                state_changes=state_changes,
                                entities=self._extract_entities_from_text(action_text),
                                episode_id=episode_id,
                                scene_id=scene_id,
                                beat_index=idx,
                            )
                            record_ids.append(
                                self.memory_store.write_episodic(
                                    project_id=project_id,
                                    state_change=state_change,
                                    source_ref=source_ref,
                                )
                            )

        return record_ids

    def extract_from_qc_report(
        self,
        project_id: int,
        qc_report: Dict[str, Any],
        episode_id: Optional[int] = None,
        scene_id: Optional[int] = None,
        source_ref: Optional[str] = None,
    ) -> List[str]:
        """
        从 QC 报告中提取修订原因和变化摘要

        Args:
            project_id: 项目 ID
            qc_report: QC 报告
            episode_id: 章节 ID
            scene_id: 场景 ID
            source_ref: 来源引用

        Returns:
            创建的记录 ID 列表
        """
        record_ids: List[str] = []

        issues = qc_report.get("issues", [])
        if isinstance(issues, list):
            for issue in issues:
                if isinstance(issue, dict):
                    event = f"QC 问题: {issue.get('type', 'unknown')} - {issue.get('message', '')}"
                    state_change = StateChange(
                        event=event,
                        state_changes={
                            "issue_type": issue.get("type"),
                            "location": issue.get("location"),
                        },
                        entities=[],
                        episode_id=episode_id,
                        scene_id=scene_id,
                        source_ref=source_ref,
                    )
                    record_ids.append(
                        self.memory_store.write_episodic(
                            project_id=project_id,
                            state_change=state_change,
                            source_ref=source_ref,
                        )
                    )

        # 如果有修订版本，记录修订摘要
        revised_script = qc_report.get("revised_script_fountain")
        if revised_script:
            state_change = StateChange(
                event="QC 修订完成",
                state_changes={"has_revision": True},
                entities=[],
                episode_id=episode_id,
                scene_id=scene_id,
                source_ref=source_ref,
            )
            record_ids.append(
                self.memory_store.write_episodic(
                    project_id=project_id,
                    state_change=state_change,
                    source_ref=source_ref,
                )
            )

        return record_ids

    @staticmethod
    def _extract_entities_from_text(text: str) -> List[str]:
        """
        从文本中提取实体（简化实现）
        实际实现可以使用 NER 模型或规则匹配
        """
        # 简化：检测常见模式（角色名、道具名等）
        # 实际应该使用更复杂的实体识别
        entities: List[str] = []
        # TODO: 实现实体提取逻辑
        return entities

    @staticmethod
    def _detect_state_changes_from_action(action_text: str) -> Dict[str, Any]:
        """
        从动作文本中检测状态变更（例如：获得道具、受伤、人物入场/退场）

        Args:
            action_text: 动作文本

        Returns:
            状态变更字典（如果有）
        """
        state_changes: Dict[str, Any] = {}

        # 检测关键词模式
        action_lower = action_text.lower()

        # 获得道具
        if any(keyword in action_lower for keyword in ["获得", "拿到", "捡起", "拾取"]):
            state_changes["prop_acquired"] = True

        # 失去道具
        if any(keyword in action_lower for keyword in ["失去", "丢失", "丢弃", "扔掉"]):
            state_changes["prop_lost"] = True

        # 受伤
        if any(keyword in action_lower for keyword in ["受伤", "中弹", "被击", "流血"]):
            state_changes["injured"] = True

        # 人物入场
        if any(keyword in action_lower for keyword in ["进入", "来到", "出现", "登场"]):
            state_changes["character_entered"] = True

        # 人物退场
        if any(keyword in action_lower for keyword in ["离开", "退出", "消失", "退场"]):
            state_changes["character_exited"] = True

        return state_changes if state_changes else {}


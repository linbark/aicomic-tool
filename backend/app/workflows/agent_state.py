"""
AgentState：统一的 Agent 状态包
显式传递 messages、检索结果、工具结果、假设/待办等
"""
from __future__ import annotations

from typing import Any, Dict, List, Optional

from pydantic import BaseModel, ConfigDict, Field

from ..workflows.memory_schemas import MemoryRetrievalResult


class AgentState(BaseModel):
    """
    统一的 Agent 状态包
    用于在 workflow 的不同阶段之间传递状态
    """

    model_config = ConfigDict(extra="allow")

    # 基础信息
    run_id: str = Field(..., description="运行 ID")
    project_id: int = Field(..., description="项目 ID")
    episode_id: Optional[int] = Field(None, description="章节 ID")
    scene_id: Optional[int] = Field(None, description="场景 ID")

    # Buffer（短期工作记忆）
    messages: List[Dict[str, str]] = Field(
        default_factory=list,
        description="对话消息历史（user/assistant/system）",
    )
    working_set: Dict[str, Any] = Field(
        default_factory=dict,
        description="当前 stage 关键中间产物（beat_sheet、shots、qc_report 等）",
    )

    # 记忆检索结果
    retrieved_memories: Dict[str, MemoryRetrievalResult] = Field(
        default_factory=dict,
        description="按 namespace/type 分组的检索结果",
    )

    # 行动记录
    actions_taken: List[Dict[str, Any]] = Field(
        default_factory=list,
        description="Planner 决策、工具调用记录等",
    )

    # 元数据
    cost: float = Field(default=0.0, description="累计成本（token 数或 API 调用费用）")
    latency_ms: float = Field(default=0.0, description="累计延迟（毫秒）")
    stage_name: Optional[str] = Field(None, description="当前阶段名称")

    def add_message(self, role: str, content: str) -> None:
        """添加消息到 buffer"""
        self.messages.append({"role": role, "content": content})

    def update_working_set(self, key: str, value: Any) -> None:
        """更新工作集"""
        self.working_set[key] = value

    def get_working_set(self, key: str, default: Any = None) -> Any:
        """获取工作集项"""
        return self.working_set.get(key, default)

    def add_retrieved_memories(self, key: str, result: MemoryRetrievalResult) -> None:
        """添加检索结果"""
        self.retrieved_memories[key] = result

    def add_action(self, action_type: str, details: Dict[str, Any]) -> None:
        """添加行动记录"""
        self.actions_taken.append(
            {
                "type": action_type,
                "details": details,
                "stage": self.stage_name,
            }
        )

    def to_dict(self) -> Dict[str, Any]:
        """转换为字典（用于持久化）"""
        return self.model_dump()

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> AgentState:
        """从字典创建（用于回放）"""
        return cls(**data)


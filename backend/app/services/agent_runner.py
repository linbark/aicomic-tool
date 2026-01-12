"""
Agent Runner：状态机编排器（不引入 LangGraph 的版本）
将现有 workflow 升级为使用 AgentState
"""
from __future__ import annotations

import time
from typing import Any, Callable, Dict, List, Optional

from .agent_planner import get_agent_planner
from .agent_verifier import get_agent_verifier
from .context_store import ContextStore
from .memory_indexer import MemoryIndexer
from .state_extractor import StateChangeExtractor
from ..workflows.agent_state import AgentState

import logging
log_dir = os.path.join(os.path.dirname(__file__), "..", "logs")
os.makedirs(log_dir, exist_ok=True)
log_file = os.path.join(log_dir, "agent_runner.log")

logger = logging.getLogger(__name__)
logger.setLevel(logging.DEBUG)
handler = logging.FileHandler(log_file)
handler.setLevel(logging.DEBUG)
formatter = logging.Formatter('%(asctime)s - %(name)s - %(levelname)s - %(message)s')
handler.setFormatter(formatter)
logger.addHandler(handler)


class AgentRunner:
    """
    Agent 运行器（状态机编排器）
    管理 Agent 的执行流程，支持 Planner/Executor/Verifier 循环
    """

    def __init__(
        self,
        context_store: Optional[ContextStore] = None,
        memory_indexer: Optional[MemoryIndexer] = None,
        state_extractor: Optional[StateChangeExtractor] = None,
    ):
        """
        初始化运行器

        Args:
            context_store: 上下文存储
            memory_indexer: 记忆索引器
            state_extractor: 状态提取器
        """
        self.context_store = context_store or ContextStore()
        self.memory_indexer = memory_indexer or MemoryIndexer()
        self.state_extractor = state_extractor or StateChangeExtractor()
        self.planner = get_agent_planner()
        self.verifier = get_agent_verifier()

    def run_agent(
        self,
        initial_state: AgentState,
        agent_func: Callable[[AgentState], AgentState],
        max_iterations: int = 3,
        verify: bool = True,
    ) -> AgentState:
        """
        运行单个 Agent（支持 Planner/Executor/Verifier 循环）

        Args:
            initial_state: 初始状态
            agent_func: Agent 函数（输入 AgentState，输出 AgentState）
            max_iterations: 最大迭代次数（用于 Verifier 循环）
            verify: 是否启用校验

        Returns:
            最终状态
        """
        state = initial_state
        iteration = 0

        while iteration < max_iterations:
            # 1. Planner：规划检索策略
            if iteration == 0:
                task_description = state.working_set.get("task_description", "执行任务")
                retrieval_plan = self.planner.plan_retrieval(state, task_description)

                # 更新状态：添加检索结果
                for key, result in retrieval_plan["retrieval_results"].items():
                    state.add_retrieved_memories(key, result)

                # 记录冲突
                if retrieval_plan["conflicts"]:
                    state.add_action("conflict_detected", {"conflicts": retrieval_plan["conflicts"]})

            # 2. Executor：执行 Agent 函数
            start_time = time.time()
            state = agent_func(state)
            state.latency_ms += (time.time() - start_time) * 1000

            # 3. Verifier：校验生成内容（如果启用）
            if verify and state.working_set:
                generated_content = state.working_set
                verification_result = self.verifier.verify(
                    state,
                    generated_content,
                    content_type=state.stage_name or "unknown",
                )

                if verification_result["is_valid"]:
                    # 校验通过，退出循环
                    break
                else:
                    # 校验失败，记录问题并继续迭代
                    state.add_action(
                        "verification_failed",
                        {
                            "issues": verification_result["issues"],
                            "suggestions": verification_result["suggestions"],
                        },
                    )
                    iteration += 1
            else:
                # 不校验或没有生成内容，直接退出
                break

        return state

    def run_workflow(
        self,
        initial_state: AgentState,
        workflow_steps: List[Callable[[AgentState], AgentState]],
        extract_state_changes: bool = True,
    ) -> AgentState:
        """
        运行完整 workflow（多个 Agent 串联）

        Args:
            initial_state: 初始状态
            workflow_steps: workflow 步骤列表（每个步骤是一个 Agent 函数）
            extract_state_changes: 是否在每个步骤后提取状态变更

        Returns:
            最终状态
        """
        logger.info(f"[AgentRunner] Running workflow with {len(workflow_steps)} steps")
        state = initial_state

        for step_idx, step_func in enumerate(workflow_steps):
            logger.info(f"[AgentRunner] Running step {step_idx + 1} of {len(workflow_steps)}")
            # 更新阶段名称
            state.stage_name = f"step_{step_idx + 1}"

            # 执行步骤
            state = step_func(state)

            # 提取状态变更（如果启用）
            if extract_state_changes and state.working_set:
                self._extract_and_write_state_changes(state)

        return state

    def _extract_and_write_state_changes(self, state: AgentState) -> None:
        """提取并写入状态变更"""
        working_set = state.working_set

        # 从不同来源提取状态变更
        if "script_fountain" in working_set:
            # 从剧本提取
            self.state_extractor.extract_from_script_fountain(
                project_id=state.project_id,
                script_fountain=working_set["script_fountain"],
                episode_id=state.episode_id,
                scene_id=state.scene_id,
                source_ref=f"{state.run_id}.{state.stage_name}",
            )

        if "beat_sheet" in working_set or "shots" in working_set:
            # 从结构化输出提取
            self.state_extractor.extract_from_structured_output(
                project_id=state.project_id,
                structured_data=working_set,
                episode_id=state.episode_id,
                scene_id=state.scene_id,
                source_ref=f"{state.run_id}.{state.stage_name}",
            )

        if "qc_report" in working_set:
            # 从 QC 报告提取
            self.state_extractor.extract_from_qc_report(
                project_id=state.project_id,
                qc_report=working_set["qc_report"],
                episode_id=state.episode_id,
                scene_id=state.scene_id,
                source_ref=f"{state.run_id}.{state.stage_name}",
            )

    def index_project_memories(
        self,
        project_id: int,
        version: str = "v1",
    ) -> Dict[str, int]:
        """
        索引项目的所有记忆（SeriesBible + VisualDNA）

        Args:
            project_id: 项目 ID
            version: 版本

        Returns:
            统计信息
        """
        return self.memory_indexer.reindex_project(project_id=project_id, version=version)


# 全局单例
_global_runner: Optional[AgentRunner] = None


def get_agent_runner(
    context_store: Optional[ContextStore] = None,
    memory_indexer: Optional[MemoryIndexer] = None,
    state_extractor: Optional[StateChangeExtractor] = None,
) -> AgentRunner:
    """获取全局 Agent 运行器（单例模式）"""
    global _global_runner
    if _global_runner is None:
        _global_runner = AgentRunner(
            context_store=context_store,
            memory_indexer=memory_indexer,
            state_extractor=state_extractor,
        )
    return _global_runner


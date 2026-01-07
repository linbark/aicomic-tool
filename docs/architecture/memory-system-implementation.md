# 记忆系统实现文档

本文档说明如何将现有的 workflow 系统升级为使用记忆系统（Memory System）和 Agent 架构。

## 概述

记忆系统提供了以下核心能力：

1. **本地向量记忆**：使用 Qdrant + BGE-M3 embedding 实现语义检索
2. **Episodic 记忆**：结构化存储状态变更（SQLite + 向量索引）
3. **分层检索**：L0 Buffer / L1 Episodic / L2 Static/Dynamic
4. **AgentState**：统一的状态包，支持可回放的 trace
5. **Planner/Executor/Verifier 循环**：规划、执行、校验的闭环

## 核心模块

### 1. MemoryStore (`backend/app/services/memory_store.py`)

记忆存储核心接口，整合向量存储和 SQLite。

**主要功能**：
- `write()`: 写入记忆条目
- `write_episodic()`: 写入 Episodic 记忆（状态变更）
- `retrieve()`: 检索记忆（支持 MMR）
- `retrieve_hierarchical()`: 分层检索
- `detect_conflicts()`: 冲突检测

**使用示例**：
```python
from app.services.memory_store import get_memory_store
from app.workflows.memory_schemas import MemoryRecord, MemoryNamespace, MemoryType

memory_store = get_memory_store()

# 写入记忆
record = MemoryRecord(
    project_id=1,
    namespace=MemoryNamespace.STATIC_BIBLE,
    type=MemoryType.CHARACTER_DESIGN,
    entity="K",
    content="K有着银色短发，左眼是红色的赛博义眼",
    payload_json={"visual_dna": {...}},
)
memory_store.write(record)

# 检索记忆
from app.workflows.memory_schemas import MemoryQuery

query = MemoryQuery(
    project_id=1,
    query_text="K的外貌",
    namespace=MemoryNamespace.STATIC_BIBLE,
    top_k=5,
)
result = memory_store.retrieve(query, use_mmr=True)
```

### 2. MemoryIndexer (`backend/app/services/memory_indexer.py`)

将 SeriesBible/VisualDNA 原子化切片并向量化索引。

**主要功能**：
- `index_series_bible()`: 索引 SeriesBible
- `index_visual_dna()`: 索引 VisualDNA
- `reindex_project()`: 重新索引整个项目

**使用示例**：
```python
from app.services.memory_indexer import MemoryIndexer

indexer = MemoryIndexer()

# 索引 SeriesBible
record_ids = indexer.index_series_bible(project_id=1, version="v1")

# 索引 VisualDNA
record_ids = indexer.index_visual_dna(project_id=1, item_id=10, version="v1")
```

### 3. AgentState (`backend/app/workflows/agent_state.py`)

统一的 Agent 状态包。

**主要字段**：
- `run_id`, `project_id`, `episode_id`, `scene_id`
- `messages`: 对话消息历史
- `working_set`: 当前 stage 关键中间产物
- `retrieved_memories`: 检索结果（按 namespace/type 分组）
- `actions_taken`: 行动记录
- `cost`, `latency_ms`: 元数据

**使用示例**：
```python
from app.workflows.agent_state import AgentState

state = AgentState(
    run_id="abc123",
    project_id=1,
    episode_id=1,
)

# 添加消息
state.add_message("user", "生成剧本")

# 更新工作集
state.update_working_set("beat_sheet", [...])

# 添加检索结果
state.add_retrieved_memories("L2_static", retrieval_result)
```

### 4. MemoryRetriever (`backend/app/services/memory_retriever.py`)

记忆检索器，实现查询分解、分层检索、冲突检测。

**主要功能**：
- `decompose_query()`: 查询分解
- `retrieve_for_task()`: 为任务检索记忆（分层）
- `detect_conflicts()`: 冲突检测
- `format_for_prompt()`: 格式化为 prompt 片段

### 5. AgentPlanner (`backend/app/services/agent_planner.py`)

Agent 规划器，生成检索计划、任务分解。

**主要功能**：
- `plan_retrieval()`: 规划检索策略
- `plan_task_decomposition()`: 任务分解

### 6. AgentVerifier (`backend/app/services/agent_verifier.py`)

Agent 校验器，基于规则和记忆进行硬校验。

**主要功能**：
- `verify()`: 校验生成内容

### 7. StateChangeExtractor (`backend/app/services/state_extractor.py`)

状态变更提取器，从输出中提取状态变更。

**主要功能**：
- `extract_from_script_fountain()`: 从 Fountain 剧本提取
- `extract_from_structured_output()`: 从结构化输出提取
- `extract_from_qc_report()`: 从 QC 报告提取

### 8. AgentRunner (`backend/app/services/agent_runner.py`)

Agent 运行器（状态机编排器）。

**主要功能**：
- `run_agent()`: 运行单个 Agent（支持 Planner/Executor/Verifier 循环）
- `run_workflow()`: 运行完整 workflow

## 集成到现有 Workflow

### 步骤 1：初始化记忆系统

在 workflow 开始时，索引项目的记忆：

```python
from app.services.memory_indexer import MemoryIndexer
from app.services.agent_runner import get_agent_runner

# 索引记忆
indexer = MemoryIndexer()
indexer.reindex_project(project_id=1, version="v1")

# 获取运行器
runner = get_agent_runner()
```

### 步骤 2：创建 AgentState

在 workflow 入口创建初始状态：

```python
from app.workflows.agent_state import AgentState
from app.services.context_store import new_run_id

state = AgentState(
    run_id=new_run_id(),
    project_id=project_id,
    episode_id=episode_id,
)
state.update_working_set("task_description", "生成剧本")
```

### 步骤 3：在 Agent 中使用记忆检索

在每个 Agent 函数中，使用 Planner 规划检索：

```python
from app.services.agent_planner import get_agent_planner

planner = get_agent_planner()

def architect_agent(state: AgentState) -> AgentState:
    # 规划检索
    retrieval_plan = planner.plan_retrieval(state, "生成世界观和节拍表")
    
    # 更新状态
    for key, result in retrieval_plan["retrieval_results"].items():
        state.add_retrieved_memories(key, result)
    
    # 格式化记忆用于 prompt
    formatted = retrieval_plan["formatted_memories"]
    
    # 构建 system prompt（包含检索到的记忆）
    system_prompt = compose_system_prompt_xml(
        PromptModules(
            role_definition=architect_role,
            series_bible=existing_bible,
            constraints=[
                "视觉优先：忽略内心独白，只提取可被镜头呈现的信息。",
                # 注入检索到的负向约束
                *[f"约束: {c}" for c in formatted.get("negative_constraints", "").split("\n")],
            ],
            # ... 其他配置
        )
    )
    
    # 执行 LLM 调用
    # ...
    
    # 更新工作集
    state.update_working_set("series_bible", series_bible)
    state.update_working_set("beat_sheet", beat_sheet)
    
    return state
```

### 步骤 4：使用 Verifier 校验

在生成内容后，使用 Verifier 校验：

```python
from app.services.agent_verifier import get_agent_verifier

verifier = get_agent_verifier()

def qc_agent(state: AgentState) -> AgentState:
    # 执行 QC
    # ...
    
    # 校验
    verification_result = verifier.verify(
        state,
        state.working_set,
        content_type="script",
    )
    
    if not verification_result["is_valid"]:
        # 记录问题
        state.add_action("verification_failed", verification_result)
        # 可能需要重新生成
    
    return state
```

### 步骤 5：提取并写入状态变更

在每个阶段结束后，提取状态变更：

```python
from app.services.state_extractor import StateChangeExtractor

extractor = StateChangeExtractor()

# 在 workflow 完成后
extractor.extract_from_structured_output(
    project_id=state.project_id,
    structured_data=state.working_set,
    episode_id=state.episode_id,
    source_ref=f"{state.run_id}.final",
)
```

### 步骤 6：持久化状态

使用现有的 `context_store.snapshot_stage()` 持久化状态：

```python
from app.services.context_store import ContextStore

context_store = ContextStore()

# 持久化每个阶段的状态
context_store.snapshot_stage(
    project_id=state.project_id,
    run_id=state.run_id,
    stage_name=state.stage_name or "unknown",
    data=state.to_dict(),
)
```

## 迁移路径

### 阶段 1：最小集成（不改变现有 workflow）

1. 在 workflow 开始时索引记忆
2. 在 Agent 函数中添加记忆检索（但不强制使用）
3. 在 workflow 结束时提取状态变更

### 阶段 2：逐步升级

1. 将局部变量替换为 AgentState
2. 在 Agent 函数中使用检索到的记忆
3. 添加 Verifier 校验

### 阶段 3：完整 Agent 化

1. 使用 AgentRunner 管理执行流程
2. 实现 Planner/Executor/Verifier 循环
3. 支持可回放的 trace

## 注意事项

1. **性能**：向量检索和 embedding 计算可能较慢，建议异步处理或缓存
2. **存储**：Qdrant 和 SQLite 会占用磁盘空间，需要定期清理
3. **冲突处理**：检测到冲突时需要人工介入或使用优先级规则
4. **版本管理**：记忆的版本管理需要与 SeriesBible/VisualDNA 的版本同步

## 后续优化

1. **Reranker**：引入 BGE-Reranker 提高检索精度
2. **LLM 查询分解**：使用 LLM 进行更智能的查询分解
3. **实体识别**：使用 NER 模型自动提取实体
4. **LangGraph 迁移**：当复杂度增加时，迁移到 LangGraph


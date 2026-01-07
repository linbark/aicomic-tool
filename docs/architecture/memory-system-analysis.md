# 记忆系统工作方式分析与现有 Agent 集成方案

## 一、记忆系统工作方式

### 1.1 核心流程

记忆系统的工作流程如下：

```
用户请求
  ↓
1. 记忆检索（MemoryRetriever）
   - 查询分解（decompose_query）
   - 分层检索（L0 Buffer / L1 Episodic / L2 Static/Dynamic）
   - MMR 去重
   - 冲突检测
  ↓
2. 格式化记忆（format_for_prompt）
   - 将检索结果格式化为 prompt 片段
  ↓
3. 注入到 System Prompt
   - 通过 PromptComposer 的 constraints/extra_blocks 注入
  ↓
4. LLM 生成
  ↓
5. 状态变更提取（StateChangeExtractor）
   - 从输出中提取状态变更
  ↓
6. 写入记忆（MemoryStore）
   - 写入 Episodic 记忆（SQLite + 向量索引）
```

### 1.2 记忆分层

- **L0 Buffer**：当前工作集（不走向量，从 AgentState.working_set 获取）
- **L1 Episodic**：状态变更记忆（时序优先，SQLite + 向量）
- **L2 Static Bible**：世界观设定、角色设计（只读/低频写）
- **L2 Dynamic Plot**：剧情进展、人物关系（读写/高频）
- **Negative Constraints**：负向约束（必取，硬注入）

### 1.3 关键组件

1. **MemoryStore**：记忆存储核心
   - `write()`: 写入记忆条目
   - `retrieve()`: 检索记忆（支持 MMR）
   - `retrieve_hierarchical()`: 分层检索
   - `detect_conflicts()`: 冲突检测

2. **MemoryRetriever**：记忆检索器
   - `decompose_query()`: 查询分解
   - `retrieve_for_task()`: 为任务检索记忆
   - `format_for_prompt()`: 格式化为 prompt

3. **MemoryIndexer**：记忆索引器
   - `index_series_bible()`: 索引 SeriesBible
   - `index_visual_dna()`: 索引 VisualDNA

## 二、现有 "Agent" 分析

### 2.1 当前实现

现有的四个"agent"都是**单次 LLM 调用**，没有使用记忆系统：

```python
# outline_generate
system_prompt = prompt_registry.get_template_prompt("outline_generate_system")
content = await _chat_client.chat(
    messages=[
        {"role": "system", "content": system_prompt},
        {"role": "user", "content": user_text},
    ],
)

# outline_optimize
system_prompt = prompt_registry.get_template_prompt("outline_optimize_system")
content = await _chat_client.chat(...)

# script_generate
system_prompt = prompt_registry.get_template_prompt("script_generate_system")
content = await _chat_client.chat(...)

# script_optimize
system_prompt = prompt_registry.get_template_prompt("script_optimize_system")
content = await _chat_client.chat(...)
```

**问题**：
- ❌ 没有检索记忆（不知道已有的世界观、角色设定）
- ❌ 没有使用 SeriesBible/VisualDNA
- ❌ 没有状态变更提取和写入
- ❌ 没有冲突检测
- ❌ 每次调用都是"全新开始"，无法保持一致性

### 2.2 能否利用记忆系统？

**答案：完全可以！** 这些"agent"应该利用记忆系统来：

1. **大纲生成（outline_generate）**
   - ✅ 检索已有的世界观设定（SeriesBible）
   - ✅ 检索已有的角色设定
   - ✅ 检索已有的剧情进展（避免重复）
   - ✅ 检索负向约束（避免违反规则）

2. **大纲优化（outline_optimize）**
   - ✅ 检索原始大纲（作为上下文）
   - ✅ 检索世界观设定（确保优化后仍符合设定）
   - ✅ 检索已有的优化历史（避免重复优化）

3. **剧本生成（script_generate）**
   - ✅ 检索大纲（作为输入）
   - ✅ 检索世界观设定（确保剧本符合设定）
   - ✅ 检索角色设定（确保角色一致性）
   - ✅ 检索已有的剧本片段（避免重复）

4. **剧本优化（script_optimize）**
   - ✅ 检索原始剧本（作为输入）
   - ✅ 检索世界观设定（确保优化后仍符合设定）
   - ✅ 检索 QC 报告（了解之前的问题）
   - ✅ 检索负向约束（避免引入新问题）

## 三、改造方案

### 3.1 最小改造（保持 API 兼容）

为每个"agent"添加可选的 `project_id` 参数，如果提供则启用记忆检索：

```python
class OutlineGenerateRequest(BaseModel):
    text: str
    project_id: Optional[int] = None  # 新增：如果提供则启用记忆

@router.post("/outline-generate", response_model=OutlineGenerateResponse)
async def outline_generate(req: OutlineGenerateRequest):
    # ... 现有代码 ...
    
    # 如果提供了 project_id，检索记忆
    retrieved_memories = {}
    if req.project_id:
        from app.services.memory_retriever import get_memory_retriever
        retriever = get_memory_retriever()
        
        retrieval_results = retriever.retrieve_for_task(
            project_id=req.project_id,
            task_description=f"生成大纲: {user_text}",
        )
        
        # 格式化记忆
        formatted = retriever.format_for_prompt(retrieval_results)
        
        # 注入到 system prompt
        # 方式1：通过 extra_blocks
        # 方式2：直接拼接到 system_prompt
        memory_context = "\n\n".join([
            f"## {layer}\n{content}"
            for layer, content in formatted.items()
        ])
        system_prompt = f"{system_prompt}\n\n{memory_context}"
    
    # ... 继续现有流程 ...
```

### 3.2 完整改造（使用 AgentState）

将现有"agent"改造为使用 AgentState 和 AgentRunner：

```python
from app.workflows.agent_state import AgentState
from app.services.agent_runner import get_agent_runner
from app.services.memory_retriever import get_memory_retriever

async def outline_generate_agent(state: AgentState) -> AgentState:
    """大纲生成 Agent（使用记忆系统）"""
    from app.services.prompt_composer import PromptModules, compose_system_prompt_xml
    from app.services import prompt_registry
    
    # 1. 检索记忆
    retriever = get_memory_retriever()
    retrieval_results = retriever.retrieve_for_task(
        project_id=state.project_id,
        task_description="生成大纲",
        context={"input_text": state.working_set.get("input_text")},
    )
    
    # 更新状态
    for key, result in retrieval_results.items():
        state.add_retrieved_memories(key, result)
    
    # 2. 格式化记忆
    formatted = retriever.format_for_prompt(retrieval_results)
    
    # 3. 构建 system prompt（注入记忆）
    outline_role = prompt_registry.get_template_prompt("outline_generate_system")
    
    constraints = []
    if "negative_constraints" in formatted:
        constraints.append(f"负向约束：\n{formatted['negative_constraints']}")
    
    system_prompt = compose_system_prompt_xml(
        PromptModules(
            role_definition=outline_role,
            series_bible=state.retrieved_memories.get("L2_static").records[0].payload_json if state.retrieved_memories.get("L2_static") else None,
            constraints=constraints,
            extra_blocks={
                "retrieved_memories": "\n\n".join([
                    f"## {layer}\n{content}"
                    for layer, content in formatted.items()
                    if layer != "negative_constraints"
                ]),
            },
        )
    )
    
    # 4. 执行 LLM 调用
    user_text = state.working_set.get("input_text", "")
    content = await _chat_client.chat(
        messages=[
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_text},
        ],
    )
    
    # 5. 更新工作集
    state.update_working_set("outline_text", content)
    
    return state

# 在 router 中使用
@router.post("/outline-generate", response_model=OutlineGenerateResponse)
async def outline_generate(req: OutlineGenerateRequest):
    # 创建 AgentState
    state = AgentState(
        run_id=new_run_id(),
        project_id=req.project_id or 0,
    )
    state.update_working_set("input_text", req.text)
    state.update_working_set("task_description", "生成大纲")
    
    # 运行 Agent
    runner = get_agent_runner()
    state = runner.run_agent(
        initial_state=state,
        agent_func=outline_generate_agent,
        verify=False,  # 大纲生成不需要校验
    )
    
    # 返回结果
    return OutlineGenerateResponse(text=state.working_set.get("outline_text", ""))
```

### 3.3 推荐方案：渐进式改造

**阶段 1：最小改造（保持兼容）**
- 添加可选的 `project_id` 参数
- 如果提供 `project_id`，则检索记忆并注入到 prompt
- 不改变现有 API 签名

**阶段 2：增强功能**
- 添加状态变更提取
- 添加冲突检测
- 添加写入记忆

**阶段 3：完整 Agent 化**
- 迁移到 AgentState
- 使用 AgentRunner
- 支持 Planner/Executor/Verifier 循环

## 四、具体改造示例

### 4.1 outline_generate 改造

```python
class OutlineGenerateRequest(BaseModel):
    text: str
    project_id: Optional[int] = None  # 新增

@router.post("/outline-generate", response_model=OutlineGenerateResponse)
async def outline_generate(req: OutlineGenerateRequest):
    # ... 现有代码 ...
    
    system_prompt = prompt_registry.get_template_prompt("outline_generate_system")
    
    # 如果提供了 project_id，检索并注入记忆
    if req.project_id:
        from app.services.memory_retriever import get_memory_retriever
        from app.services.memory_indexer import MemoryIndexer
        
        # 确保记忆已索引
        indexer = MemoryIndexer()
        indexer.reindex_project(project_id=req.project_id, version="v1")
        
        # 检索记忆
        retriever = get_memory_retriever()
        retrieval_results = retriever.retrieve_for_task(
            project_id=req.project_id,
            task_description=f"生成大纲: {user_text[:100]}",
        )
        
        # 格式化
        formatted = retriever.format_for_prompt(retrieval_results)
        
        # 注入到 system prompt
        memory_parts = []
        if "L2_static" in formatted:
            memory_parts.append(f"已有世界观设定：\n{formatted['L2_static']}")
        if "negative_constraints" in formatted:
            memory_parts.append(f"约束条件：\n{formatted['negative_constraints']}")
        
        if memory_parts:
            system_prompt = f"{system_prompt}\n\n## 记忆上下文\n\n" + "\n\n".join(memory_parts)
    
    # ... 继续现有流程 ...
```

### 4.2 script_generate 改造

```python
class ScriptGenerateRequest(BaseModel):
    text: str
    project_id: Optional[int] = None  # 新增
    outline_text: Optional[str] = None  # 新增：大纲文本

@router.post("/generate-script", response_model=ScriptGenerateResponse)
async def generate_script(req: ScriptGenerateRequest):
    # ... 现有代码 ...
    
    system_prompt = prompt_registry.get_template_prompt("script_generate_system")
    
    # 如果提供了 project_id，检索并注入记忆
    if req.project_id:
        from app.services.memory_retriever import get_memory_retriever
        from app.services.memory_indexer import MemoryIndexer
        
        # 确保记忆已索引
        indexer = MemoryIndexer()
        indexer.reindex_project(project_id=req.project_id, version="v1")
        
        # 检索记忆
        retriever = get_memory_retriever()
        retrieval_results = retriever.retrieve_for_task(
            project_id=req.project_id,
            task_description="生成剧本",
            context={
                "outline_text": req.outline_text,
            },
        )
        
        # 格式化
        formatted = retriever.format_for_prompt(retrieval_results)
        
        # 注入到 system prompt
        memory_parts = []
        if "L2_static" in formatted:
            memory_parts.append(f"世界观设定：\n{formatted['L2_static']}")
        if "L1" in formatted:
            memory_parts.append(f"已有剧情：\n{formatted['L1']}")
        if "negative_constraints" in formatted:
            memory_parts.append(f"约束条件：\n{formatted['negative_constraints']}")
        
        if memory_parts:
            system_prompt = f"{system_prompt}\n\n## 记忆上下文\n\n" + "\n\n".join(memory_parts)
    
    # ... 继续现有流程 ...
```

## 五、总结

### 5.1 记忆系统工作方式

1. **索引阶段**：SeriesBible/VisualDNA → 原子化切片 → 向量化 → 写入向量库
2. **检索阶段**：查询分解 → 分层检索 → MMR 去重 → 冲突检测
3. **注入阶段**：格式化记忆 → 注入到 System Prompt
4. **写入阶段**：提取状态变更 → 写入 Episodic 记忆

### 5.2 现有 "Agent" 能否利用记忆系统？

**完全可以！** 所有四个"agent"都应该利用记忆系统：

- ✅ **outline_generate**：检索世界观设定、角色设定、负向约束
- ✅ **outline_optimize**：检索原始大纲、世界观设定、优化历史
- ✅ **script_generate**：检索大纲、世界观设定、角色设定、已有剧情
- ✅ **script_optimize**：检索原始剧本、世界观设定、QC 报告、负向约束

### 5.3 改造建议

1. **最小改造**：添加可选的 `project_id` 参数，启用记忆检索
2. **渐进式增强**：逐步添加状态变更提取、冲突检测、记忆写入
3. **完整 Agent 化**：迁移到 AgentState + AgentRunner

### 5.4 收益

- ✅ **一致性**：生成内容符合已有设定
- ✅ **连续性**：利用已有剧情，避免重复
- ✅ **约束遵守**：自动遵守负向约束
- ✅ **可追溯**：状态变更可追溯
- ✅ **可回放**：支持可回放的 trace


# 决策已锁定（按你的 3 点）
- 只先改后端：前端 Chat UI/轮询逻辑不动。
- timeout 统一按 error 处理（前端只认识 queued/running/done/error）。
- changeset 审阅进入“可暂停/可恢复”的 interrupt 模型，但不要求前端新增 resume API 调用（通过现有 approve/reject 接口触发恢复）。

# 目标
## 对前端零破坏
- 继续沿用现有 runs-files stages 作为“执行进度总线”。
- 现有关键 stage 名与结构保持不变：
  - `chat.status`, `chat.plan`, `chat.step.{i}.start|end|error`, `chat.error`, `chat.final`
- 允许新增 stage（前端会忽略），例如 `chat.interrupt`（用于恢复）。

## 后端编排升级
- 用 LangGraph 替换 `_chat_act_core` 的“planner + for-loop 执行器”，得到可扩展、可中断可恢复的图执行模型。

# 现状对齐点（我们会严格兼容）
- 异步入口： [chat_act_async](file:///Users/linjiacheng/Documents/GitHub/aicomic-tool/backend/app/routers/ai_chat.py#L1294-L1375) 通过后台线程写 stages。
- 前端轮询与归约逻辑主要依赖这些 stage：
  - 计划：`chat.plan`
  - 步骤：`chat.step.N.start/end/error` + `chat.status.current_step_index/current_action_key`
  - 终局：`chat.final`（包含 `assistant_message` 与 `cards`）

# LangGraph 设计
## 1) GraphState（状态结构）
- 新增 `ChatGraphState` 承载：
  - 输入：`project_id_pk`, `project_uuid`, `episode_id`, `run_id`, `message`, `ui_context`, `current_action_key`, `debug`
  - planner 产物：`plan`, `steps`, `final_action_key`, `needs_clarification`
  - 执行上下文：`step_index`, `artifacts`, `cards`, `steps_trace`（debug）
  - interrupt：`interrupt`（None 或 {kind, payload}）
  - 输出：`assistant_message`, `created_run`

## 2) 图拓扑（最小可运行 + 可 interrupt）
- 节点与边（保持与当前逻辑一一对应）：
  1. `precheck_ambiguity`：复用“意图不清直接 clarify 卡”规则。
  2. `retrieve_memory`：复用现有 memory 检索，将结果写入 state。
  3. `planner`：复用现有 planner prompt/JSON 解析/约束（最多 4 步、allowlist），并写 `chat.plan`。
  4. `maybe_clarify`：若需要澄清，直接进入 `finalize`。
  5. `run_step`（循环节点）：根据 `step_index` 取 step，分派到 action 实现。
  6. `maybe_interrupt`：如果本步产出 `review_changeset`（即 memory_extract_changeset），进入 interrupt：
     - 写 `chat.interrupt`（保存可恢复信息）
     - 停止图执行（当前后台线程结束，不写 final）
  7. `persist`：复用“最终结果写 AiActionRun（source=chat）”逻辑。
  8. `finalize`：生成 `ChatActResponse`，写 `chat.final` 与 `chat.status=done`。

# stages 与错误策略（强制兼容前端）
## 1) stage emitter（统一出口）
- 提供统一的 emitter（函数或小类），在节点开始/结束/异常时写：
  - `chat.status`：仅用 queued/running/done/error 四态
  - `chat.step.{i}.start/end/error`
  - `chat.plan`
  - `chat.final`
  - `chat.error`
  - `chat.interrupt`（新增，用于恢复）

## 2) timeout 统一 error
- 图执行中若遇到超时：
  - `chat.step.{i}.error` 仍可包含 `error:"timeout"`
  - `chat.status` 一律写 `{"status":"error"}`（不再写 timeout）
  - `chat.error` 写明 timeout 文本

# interrupt 可恢复（不改前端交互）
## 1) 为什么可以“只改后端”
- 前端已存在 `review_changeset` 卡片点击后调用：
  - `POST /memory/changeset/{id}/approve`
  - `POST /memory/changeset/{id}/reject`
- 我们让这两个接口在写入通过/拒绝后，自动触发“恢复该 run_id 的图执行”（后台线程），从而前端无需新增 resume API。

## 2) 恢复数据存哪里
- 在 interrupt 时，写一个新增 stage：`chat.interrupt`，包含：
  - `project_id_pk`, `run_id`, `kind:"review_changeset"`, `changeset_id`
  - `resume_state`：序列化后的最小 GraphState（或一个 resume token + state 存在 runs 目录）
- 恢复时从 runs-files 读取 `chat.interrupt`，重建 GraphState 并继续执行。

## 3) approve/reject 如何触发恢复
- 修改后端 [memory.py](file:///Users/linjiacheng/Documents/GitHub/aicomic-tool/backend/app/routers/memory.py) 的 approve/reject：
  - 强制校验 `run_id` 非空（当前 ApplyChangeSetRequest 已有 run_id 字段但未校验/使用）。
  - 在成功 apply/reject 后：
    - 读取对应 run 的 `chat.interrupt`
    - 把 human decision 写入 state（例如 `interrupt.result=approved|rejected`）
    - 启动 daemon thread 继续跑 LangGraph（继续写 step/end/final 等 stages）

# 文件与模块落点（计划改动范围）
- 新增：`backend/app/services/chat_graph.py`（或 `backend/app/workflows/chat_graph.py`，按现有结构选更顺的位置）
  - GraphState 定义
  - 图构建与执行入口 `run_chat_graph(state, emit_stages=True|False)`
  - stage emitter
- 修改： [ai_chat.py](file:///Users/linjiacheng/Documents/GitHub/aicomic-tool/backend/app/routers/ai_chat.py)
  - `_chat_act_core` 改为调用 LangGraph runner（同步/异步统一复用）
  - 保持现有 API/响应模型不变
- 修改： [memory.py](file:///Users/linjiacheng/Documents/GitHub/aicomic-tool/backend/app/routers/memory.py)
  - approve/reject 增加 run_id 校验与“自动恢复 run”逻辑
- 依赖：`requirements.txt` 增加 LangGraph 相关依赖（以 Python LangGraph 为主）

# 验证（最小但足够覆盖关键风险）
- 新增 pytest：
  - mock LLM planner 输出固定 steps（避免外部调用）
  - 断言：
    - act_async 写出 `chat.plan`、`chat.step.*`、`chat.final`（无 interrupt 流）
    - 遇到“changeset 审阅”会写 `chat.interrupt` 且不写 final
    - 调用 approve/reject（带 run_id）后，会继续写后续 stages 并最终写 `chat.final`/`done`
  - 断言 timeout 被映射为 `status=error`

# 交付形态（完成后你会得到什么）
- 行为上：前端不改即可继续显示步骤/卡片；当出现审阅卡片时，点击 approve/reject 会让同一个 run_id 继续跑完并产出 final。
- 架构上：chat 编排从“手写 for-loop”迁移到 LangGraph，可继续扩展更多分支/并行/更多 interrupt 点。
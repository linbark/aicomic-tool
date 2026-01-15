## 目标与原则
- 每一集（Episode）都有“执行”按钮；一旦执行开始，该集剧本文本永久只读（不可再编辑）。
- 执行是固定 5 步流水线：1大纲生成→2用户确认→3资产抽离+视觉DNA→4用户确认→5按长度分割并生成各集大纲→6资产入库（含用户确认）。
- UI 以“步骤+确认点”为核心，不再依赖自由聊天来表达进度。

## 前端信息架构（Script 页）
- 左侧 Episode 列表新增状态徽标：未执行 / 执行中 / 待确认（大纲|资产|分割|入库）/ 已完成 / 失败。
- 右侧 EpisodeEditor 顶部新增：
  - 主按钮：执行（仅当该集未锁定且剧本非空时可用）
  - 次按钮：查看执行记录/重跑（仅当已锁定时可用；重跑不解锁剧本，只生成新一轮 artifacts）
- 右侧下半区新增“执行面板”（Stepper）：5 个步骤卡片，每步包含：状态、耗时、输出预览、展开查看、以及“确认继续/重新生成”。

## 执行状态机（前端/后端一致）
- Episode 维度状态：
  - script_lock：draft | locked（执行开始即 locked）
  - exec_status：idle | running | waiting_outline_confirm | waiting_assets_confirm | waiting_split_confirm | waiting_ingest_confirm | done | error
- 每个确认点都需要一个明确的“用户动作”才能继续；不允许通过修改原剧本文本来推进流程（符合“执行后不可修改”）。

## 每一步的 UI 输出形态
1) 大纲生成
- 展示：大纲文本/JSON（支持复制、下载）。
- 操作：确认继续 / 重新生成（不会解锁剧本）。

2) 资产抽离 + 视觉DNA
- 展示：
  - 资产清单（人物/物品/场景等，表格形式）
  - 视觉DNA（JSON/卡片化字段：风格、色板、光照、镜头语言、统一 prompt 片段等）
- 操作：
  - 允许在“资产清单/视觉DNA”上做轻量编辑（不是编辑原剧本），然后确认继续。

3) 剧集分割 + 每集大纲
- 条件：若脚本长度超阈值或用户主动触发“分割”。
- 展示：分割结果列表（集标题/范围摘要/该集大纲）。
- 操作：允许重命名、排序、删除某段拆分，确认后写入 Episode 列表（创建/更新 episodes）。

4) 资产入库
- 展示：入库预览（变更摘要、数量统计）。
- 操作：确认入库 / 驳回。

## 后端对齐方案（两条实现路径，推荐 A）
**A. 新增“episode_execute”固定流水线（推荐）**
- 新增异步入口：`POST /ai/episode-execute/act_async`（输入：project_id、episode_id、run_id、script_text、可选用户编辑的 assets/visual_dna）
- 执行过程中写入 runs-files stages（复用现有轮询）：
  - `chat.plan`：固定 5 步 plan（为了复用前端 RunStatusPanel/旧轮询逻辑）
  - `chat.step.{i}.start/end/error`：每步状态
  - 在需要确认处写 `chat.interrupt`：kind=`confirm_outline|confirm_assets|confirm_split|confirm_ingest`，并包含 resume_state
- 新增确认/继续接口：`POST /ai/episode-execute/{run_id}/confirm`（输入：decision=confirmed|regenerate，附带用户修改后的 artifacts）

**B. 复用现有 chat_graph（不推荐，需扩 action_key）**
- 目前 chat_graph 允许的 action_key 不包含“资产抽离/分割/入库”，需要扩展 allowed set 与执行器，改动面更大且 planner 逻辑不必要。

## 数据持久化设计（用于“锁定”和“查看历史”）
- Episode 增加字段（建议最小集）：
  - `script_locked`(bool) / `script_locked_at`
  - `last_exec_run_id`(str) / `exec_status`(str)
  - `exec_artifacts`(JSON，可存 outline/assets/visual_dna/split_result/ingest_preview)
- 后端更新 storyboard 的 updateEpisode/updateScene/updateShot：若 episode.script_locked=true，拒绝对该 episode 的 description 等剧本文本字段的修改（从源头保证“不可修改”）。

## 前端实现落点（基于现有代码结构）
- 页面入口仍在 `/script` → [ScriptPage.refactored.tsx](file:///Users/linjiacheng/Documents/GitHub/aicomic-tool/react-frontend/src/pages/ScriptPage.refactored.tsx)
- API 层：在 [api/client.ts](file:///Users/linjiacheng/Documents/GitHub/aicomic-tool/react-frontend/src/api/client.ts) 新增 episode_execute 的 start/confirm/getStatus 方法；并沿用现有 `runs-files` 轮询。
- UI 组件：在 `react-frontend/src/components/script/` 增加/改造 `ExecutionPanel`（或扩展现有 RunStatusPanel），让其支持“确认点卡片”。
- 轮询逻辑：把旧版 [ScriptPage.tsx](file:///Users/linjiacheng/Documents/GitHub/aicomic-tool/react-frontend/src/pages/ScriptPage.tsx) 中成熟的 stage 消费逻辑迁回 refactored 版，确保：log、plan、step start/end/error、interrupt 都能驱动 UI。
- EpisodeEditor：根据 episode.script_locked 决定 textarea 是否可编辑，并在锁定后隐藏保存按钮，只保留“执行/查看结果”。

## 迁移策略（避免两套逻辑长期并存）
- 以 refactored 版为唯一入口；把旧版 ScriptPage.tsx 中“轮询 + steps UI”迁移并删掉旧实现的路由引用，避免代码膨胀。

## 验收标准
- 点击“执行”后：剧本文本立刻只读；刷新页面仍保持只读。
- 执行步骤按 1→5 展示；每个确认点必须点“确认继续”才进入下一步。
- 分割确认后：Episode 列表自动更新为拆分后的多集，并展示各集大纲。
- 入库阶段能出现可审阅的变更摘要，并能确认/驳回。
- 全流程出错时：状态进入 error，展示错误与重试入口，不会解锁剧本。

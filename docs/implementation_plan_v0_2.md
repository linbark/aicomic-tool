# 多代理创作接口落地方案（v0.2 对齐仓库版）

> 目标：在现有 `aicomic-tool` 仓库中，把“统一 Envelope + Artifacts + 图片反推 JSON 一致性层”落成**可运行、可校验、可迭代**的最小闭环。

## 1. 数据模型（后端 Pydantic）

### 1.1 Envelope（已存在）
- `backend/app/schemas.py`
  - `AgentRequest` / `AgentResponse`
  - `Constraints`（已强类型化：`PageTurnEngineeringConstraint` / `VisualDNALockingConstraint` / `RefinementLoopConstraint`）
  - `JsonConsistencyConstraint`
    - `enabled`
    - `locking_policy`：`field_whitelist` / `verbatim_json_block`
    - `required_fields`：JSONPath（简化版）
    - `enforce_allowlist`：是否强制 required_fields 必须落在系统白名单里（默认 true；更开放时可 false）

### 1.2 VisualProfile（开放 Schema，不限制死）
- `VisualProfile` 设置 `extra="allow"`：LLM/工具链可以产出额外字段而不报错。
- 内置建议字段（推荐尽量填充，不强制“只能这些”）：
  - `character_core.visual_dna.*`
  - `technical_specs.color_palette`（已强制 hex 校验：`#RRGGBB`）

### 1.3 Artifacts（已补齐骨架）
在 `backend/app/schemas.py` 中新增了：
- `SeriesBible` / `BeatSheet` / `FountainScript`
- `Storyboard`（`Panel.layout` + `Panel.page_hint` 预留翻页工程/排版）
- `PromptPack`（含 `locked_visual_dna_included` / `locked_visual_profile_included` 校验）
- `QCReport`（用 `JSONPatchOp` 表达 `after_patch`，可被工程化应用）
- `ManjuWorkflowRequest` / `ManjuWorkflowResponse`

## 2. 校验器（约束如何落地）

### 2.1 Visual DNA 锁定
策略：`SeriesBible.characters[].visual_dna` 作为“不可变字符串”，下游提示词必须逐字包含（或按 token 序列包含）。
- 当前实现：在 `PromptPack` 层面校验 `locked_visual_dna_included=true`
- 后续增强：
  - `policy=verbatim` 时，QC 逐字检查 `visual_dna` 子串是否出现
  - `policy=ordered_tokens` 时，对 `visual_dna` 做 token 化并校验顺序出现

### 2.2 JSON 一致性层（图片反推）
当 `constraints.json_consistency.enabled=true`：
- 强制 `required_fields` 非空（field_whitelist 模式）
- `PromptPack.items[].locked_visual_profile_included=true`
- 提示词“引用字段”必须来自：
  - 白名单字段（required_fields + 系统白名单）；
  - 其余字段不做数量限制（由实现侧/产品侧自行控制“提示词漂移风险”）。

## 3. 代理编排器（后端 Service）

### 3.1 现状与最小闭环
已实现（无 LLM 保底）：
- `VisualAssetIngestor`：`backend/app/services/visual_asset_ingestion.py`
  - 产出 `VisualProfileLibrary`
  - 生成确定性 `visual_dna_string`（见 3.2）

新增最小工作流骨架（无 LLM）：
- `backend/app/services/manju_workflow.py`
  - `run_manju_workflow(req, db)`：可选 ingest assets，其余产物返回 warnings（未实现）

### 3.2 VisualProfile -> Visual DNA（不可变字符串镜像）
落地原则：**DNA 由 JSON 生成并保持**。
- `backend/app/utils/visual_profile.py`
  - `build_visual_dna_string_from_profile_dict()`：对稳定字段做确定性 JSON 序列化（sorted keys）
- `backend/app/services/visual_asset_ingestion.py`
  - 在生成 `VisualProfile` 后写入 `profile.visual_dna_string = ...`

后续 `NarrativeArchitect` 生成 `SeriesBible` 时：
- 若存在 `VisualProfileLibrary`：直接把对应 profile 的 `visual_dna_string` 写入 `SeriesBible.characters[].visual_dna`

## 4. API 路由（后端）

### 4.1 已存在
- `POST /api/v1/assets/ingest`：图片反推 JSON
- `POST /api/v1/agents/run`：统一 Envelope（目前仅实现 VisualAssetIngestor）

### 4.2 新增
- `POST /api/v1/workflows/manju/run`
  - 文件：`backend/app/routers/v1_workflows.py`
  - 服务启动挂载：`backend/app/main.py` 已 include `v1_workflows.router`

## 5. 前端对接点（Vue）

### 5.1 API Client
- `frontend/src/api/client.js`
  - 已有：`ingestVisualAssets(data)`
  - 新增：`runManjuWorkflow(data)` -> `POST /api/v1/workflows/manju/run`

### 5.2 页面/组件接入建议
最小接入路径（先打通一致性层）：
- 在 `AssetLibraryView.vue` 或人设资产页：
  - 选中若干立绘 -> 调 `ingestVisualAssets`
  - 把返回的 `VisualProfileLibrary` 存入项目级状态（Pinia 或现有 store）
- 在后续“生成分镜/提示词”按钮：
  - 调 `runManjuWorkflow`（请求里带 `assets` + `constraints`）

## 6. 下一阶段实现清单（可直接编码）

1) `StoryboardTranslator`（先不做 LLM，也可以用模板/规则生成）
- 输入：`SeriesBible + FountainScript + VisualProfileLibrary`
- 输出：`Storyboard + PromptPacks`
- 严格把 `visual_dna_string` 和 required_fields 引用拼入 prompt，并计算/标注 `locked_*_included`

2) `QCInspector`
- 先做 schema/约束级 QC：
  - 气泡字数上限
  - 动作段行数上限（需要先有 Fountain parser/或简单规则）
  - prompt 锁定标志校验（已在 `PromptPack` 有 schema 校验）
- 输出 `QCReport`，fix 用 `JSONPatchOp[]`

3) Orchestrator 完整编排
- `VisualAssetIngestor` -> `NarrativeArchitect` -> `BeatSheetAgent` -> `Screenwriter` -> `QCInspector` -> `StoryboardTranslator` -> `QCInspector`
- 遵循 `refinement_loop.max_rounds`



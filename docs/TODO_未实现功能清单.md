# 未实现功能清单（基于接口文档与代码检查）

## 已实现的核心功能 ✅

- ✅ VisualAssetIngestor（图片反推 JSON）
- ✅ NarrativeArchitect（SeriesBible 生成）
- ✅ BeatSheetAgent（BeatSheet 生成）
- ✅ Screenwriter（FountainScript 生成）
- ✅ StoryboardTranslator（Storyboard + PromptPacks）
- ✅ QCInspector（基础检查 + RFC6902 fixes）
- ✅ refinement_loop（自动修正循环）
- ✅ RFC6902 JSON Patch 引擎
- ✅ Fountain 解析与 lint

## 未实现功能（按优先级）

### P3 优先级（功能增强）

#### 1. VisualAssetIngestor LLM 反推
- **现状**：`guess_face_stub()` 返回 `"unknown"`，没有真正的图片识别
- **需要**：接入 LLM/视觉模型 API，从图片反推 `character_core.visual_dna` 字段
- **文件**：`backend/app/utils/visual_profile.py`、`backend/app/services/visual_asset_ingestion.py`
- **影响**：当前只能提取色板/metadata，无法识别角色特征

#### 2. ordered_tokens policy 支持
- **现状**：只实现了 `verbatim`（逐字包含），`ordered_tokens` 未实现
- **需要**：对 `visual_dna` 做 token 化，校验顺序出现
- **文件**：`backend/app/services/agents/qc_inspector.py`
- **影响**：无法使用更灵活的 token 序列检查

#### 3. 受控词汇表（景别/角度/光影）
- **现状**：StoryboardTranslator 使用硬编码值（`"medium"`, `"eye_level"`）
- **需要**：定义受控词汇表，从 `TechnicalSpecsJson` 或规则生成
- **文件**：`backend/app/services/agents/storyboard_translator.py`
- **影响**：shot/lighting 生成不够规范

#### 4. PromptPack 参数生成（negative_prompt / params）
- **现状**：`negative_prompt=None`, `params={}`，注释为 "P0 暂不生成"
- **需要**：
  - SD/Flux：生成 `negative_prompt`（基于约束/禁用词）
  - Midjourney：生成 `params`（`--ar`, `--v`, `--stylize` 等）
- **文件**：`backend/app/services/agents/storyboard_translator.py`
- **影响**：提示词不够完整，需要手动补充参数

#### 5. Fountain lint 自动修复
- **现状**：只检查不修复，`fixes=[]`
- **需要**：对场景标题格式、动作段超行等生成 JSONPatch fixes
- **文件**：`backend/app/services/agents/qc_inspector.py`、`backend/app/utils/fountain_lint.py`
- **影响**：需要手动修复 Fountain 错误

### P4 优先级（高级功能）

#### 6. page_turn_engineering（翻页工程）
- **现状**：Schema 有定义，但 workflow/agents 中未使用
- **需要**：
  - 在 BeatSheetAgent 中标记 `page_turn_candidate`
  - 在 StoryboardTranslator 中设置 `Panel.page_hint`
  - 在 QCInspector 中检查偶数页末格
- **文件**：`backend/app/services/agents/beat_sheet_agent.py`、`storyboard_translator.py`、`qc_inspector.py`
- **影响**：无法自动优化翻页体验

#### 7. QC 高级检查点
- **缺失项**：
  - **穿帮检查**：检查 `SeriesBible.world_rules.taboos` 是否被违反
  - **物理定律检查**：检查 `world_rules.physics` 是否被违反
  - **连续性检查**：检查角色外观/对话/场景的连续性（跨 panel/scene）
- **文件**：`backend/app/services/agents/qc_inspector.py`
- **影响**：无法发现逻辑/连续性错误

#### 8. visual_metaphors（视觉隐喻）
- **现状**：接口文档提到，但 Schema 中未定义
- **需要**：在 `SeriesBible` 中增加 `visual_metaphors` 字段，用于冲突映射到视觉隐喻
- **文件**：`backend/app/schemas.py`、`backend/app/services/agents/narrative_architect.py`
- **影响**：无法表达高级视觉概念

#### 9. GET /api/v1/requests/{request_id}（请求追踪）
- **现状**：接口文档提到，但未实现
- **需要**：实现请求状态查询接口（可用于异步工作流）
- **文件**：`backend/app/routers/v1_agents.py` 或新建 `v1_requests.py`
- **影响**：无法追踪长时间运行的请求

#### 10. 前端 UI 集成
- **现状**：只有 `WorkflowTestView.vue` 测试页面
- **需要**：
  - 在主界面集成 workflow 调用
  - 展示 artifacts（SeriesBible/BeatSheet/Fountain/Storyboard/PromptPacks）
  - 可视化 QCReport 结果
- **文件**：`frontend/src/views/*.vue`、`frontend/src/components/*.vue`
- **影响**：无法在 UI 中使用完整工作流

### 代码清理

#### 11. v1_workflows.py 重复代码
- **问题**：文件中有重复的 router 定义（第 1-23 行和第 25-71 行）
- **需要**：删除重复代码，只保留一个正确的实现
- **文件**：`backend/app/routers/v1_workflows.py`

## 总结

**核心链路已打通**：从 assets/source_text 到 Storyboard/PromptPacks/QCReport 的完整流程已实现。

**主要缺失**：
1. LLM 反推（VisualAssetIngestor 的图片识别）
2. 高级 QC 检查（穿帮/物理/连续性）
3. 提示词参数生成（negative_prompt/params）
4. 前端 UI 集成
5. 翻页工程与受控词汇表

**建议优先级**：
- **P3**：先做 LLM 反推和参数生成（提升可用性）
- **P4**：再做高级 QC 和翻页工程（提升质量）


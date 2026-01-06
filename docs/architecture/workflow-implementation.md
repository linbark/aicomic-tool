# AI Workflows 功能实现文档

本文档详细说明 AI Workflows 系统的完整实现，包括前端集成、Context 管理、Run 快照审计、Visual DNA 摄取以及多平台提示词方言支持。

## 目录

- [Phase 0: 前端 Workflow 集成](#phase-0-前端-workflow-集成)
- [Phase 1: Context 管理（Series Bible / Visual DNA）](#phase-1-context-管理)
- [Phase 2: Run 快照审计与回放](#phase-2-run-快照审计与回放)
- [Phase 3: Visual DNA 摄取（图片→JSON）](#phase-3-visual-dna-摄取)
- [Phase 4: 多平台提示词方言支持](#phase-4-多平台提示词方言支持)
- [API 参考](#api-参考)
- [前端页面说明](#前端页面说明)

---

## Phase 0: 前端 Workflow 集成

### 实现内容

补齐了前端 `ScriptPage.tsx` 中缺失的 workflow handler 函数，使 workflow 按钮可正常使用。

### 新增函数

#### `handleWorkflowScript()`
- **位置**: `react-frontend/src/pages/ScriptPage.tsx`
- **功能**: 调用 `/ai/workflows/script` API，生成完整的剧本 workflow
- **输入**: Episode 的 `episodeDescription` 或 `workstationInput`
- **输出**: 
  - 保存 `run_id` 到 `lastWorkflowRunId`
  - 显示结果预览（Series Bible、Beat Sheet、Script Fountain、QC Report）
  - 自动填充 `script_fountain` 到 `episodeDescription`

#### `handleApplyWorkflowScript()`
- **功能**: 调用 `/ai/workflows/script/apply` API，将 workflow 结果写回数据库
- **行为**: 
  - 将 `script_fountain` 写入 `Episode.description`
  - 可选：根据 `overwrite_scenes` 参数覆盖现有 scenes

#### `handleWorkflowStoryboard()`
- **功能**: 调用 `/ai/workflows/storyboard` API，生成分镜 + prompt 翻译
- **输入**: Scene 的 `sceneDescription`
- **输出**:
  - 保存 `run_id` 到 `lastWorkflowRunId`
  - 将返回的 `shots` 转换为预览格式（包含 `prompt`、`negative_prompt`、`shot_size` 等）
  - 更新 `storyboardPreview` 状态

#### `handleApplyWorkflowStoryboard()`
- **功能**: 调用 `/ai/workflows/storyboard/apply` API，将 shots 写回数据库
- **行为**:
  - 根据 `overwrite_shots` 参数清空现有 shots
  - 创建新的 `Shot` 记录，包含 `action_text`、`dialogue`、`prompt`、`negative_prompt`

### UI 变更

1. **Episode 编辑区域**：
   - 添加 "Workflow剧本" 按钮（触发 `handleWorkflowScript`）
   - 添加 "应用Workflow" 按钮（触发 `handleApplyWorkflowScript`）
   - 显示最近 workflow 的 `run_id`

2. **Scene 编辑区域**：
   - 添加 "Workflow分镜" 按钮（触发 `handleWorkflowStoryboard`）
   - 添加 "应用Workflow" 按钮（触发 `handleApplyWorkflowStoryboard`）
   - 在 storyboard preview 中显示 `prompt` 和 `negative_prompt`（如果存在）

### 类型定义更新

```typescript
type SplitShotPreview = SplitShotItem & {
  _key: string
  prompt?: string
  negative_prompt?: string
  shot_size?: string
  camera_angle?: string
  lighting_style?: string
}
```

---

## Phase 1: Context 管理

### 实现内容

实现了 Series Bible 和 Visual DNA 的完整管理功能，包括前端 UI 和后端 API 增强。

### 后端增强

#### API 校验增强
- **文件**: `backend/app/routers/ai.py`
- **变更**:
  - `PUT /ai/context/series-bible`: 增加 `version` 格式校验（`v\d+`）
  - `PUT /ai/context/visual-dna`: 增加 `version` 格式校验和数据校验
  - 对非 object 类型的 `data` 返回 400 错误

### 前端实现

#### API 客户端方法
- **文件**: `react-frontend/src/api/client.ts`
- **新增方法**:
  ```typescript
  getSeriesBible(projectId, version?)
  putSeriesBible(projectId, { data, version? })
  getVisualDna(projectId, itemId, version?)
  putVisualDna(projectId, itemId, { data, version? })
  ```

#### Context 管理页面
- **文件**: `react-frontend/src/pages/ContextPage.tsx`
- **路由**: `/context`
- **功能**:
  1. **Series Bible 编辑器**:
     - JSON 文本编辑器（支持格式化）
     - 加载/保存功能
     - JSON 格式校验
     - 版本管理（当前默认 v1）

  2. **Visual DNA 编辑器**:
     - 按项目选择资产条目（Asset Item）
     - 每个条目独立的 Visual DNA JSON 编辑
     - 加载/保存功能
     - 支持从图片摄取（见 Phase 3）

### 使用流程

1. 访问 `/context` 页面
2. 选择项目
3. 编辑 Series Bible JSON（项目级世界观设定）
4. 选择资产条目，编辑其 Visual DNA JSON
5. 保存后，这些 Context 会在 workflow 中自动使用

---

## Phase 2: Run 快照审计与回放

### 实现内容

实现了完整的 run 快照浏览、查看和 stage 审计功能。

### 后端实现

#### ContextStore 扩展
- **文件**: `backend/app/services/context_store.py`
- **新增方法**:
  ```python
  list_runs(project_id) -> List[Dict]
  read_run(project_id, run_id) -> Optional[Dict]
  list_stages(project_id, run_id) -> List[str]
  read_stage(project_id, run_id, stage_name) -> Optional[Any]
  ```

#### API Endpoints
- **文件**: `backend/app/routers/ai.py`
- **新增路由**:
  - `GET /ai/runs-files?project_id=...` - 列出所有 runs（返回 meta 信息）
  - `GET /ai/runs-files/{run_id}?project_id=...` - 读取完整 run（request + response + meta）
  - `GET /ai/runs-files/{run_id}/stages?project_id=...` - 列出所有 stage 名称
  - `GET /ai/runs-files/{run_id}/stages/{stage_name}?project_id=...` - 读取指定 stage 内容

### 前端实现

#### API 客户端方法
- **文件**: `react-frontend/src/api/client.ts`
- **新增方法**:
  ```typescript
  listRunFiles(projectId)
  getRunFile(projectId, runId)
  listRunStages(projectId, runId)
  getRunStage(projectId, runId, stageName)
  ```

#### Run Inspector 页面
- **文件**: `react-frontend/src/pages/RunInspectorPage.tsx`
- **路由**: `/runs`
- **功能**:
  1. **左侧面板**: 列出所有 runs（按时间倒序）
  2. **右侧面板**: 
     - Tab 切换：Run 详情 / 各个 Stage
     - Run 详情：显示 request、response、meta 的完整 JSON
     - Stage 详情：显示每个 stage 的 raw/parsed 数据

### 使用场景

1. **调试 workflow**: 查看每次 workflow 的完整输入输出
2. **审计历史**: 回溯之前的生成结果
3. **参数调优**: 对比不同参数下的中间产物
4. **问题排查**: 定位 workflow 中某个 stage 的问题

---

## Phase 3: Visual DNA 摄取（图片→JSON）

### 实现内容

实现了从图片文件自动提取 Visual DNA JSON 的功能。

### 后端实现

#### Prompt 模板
- **文件**: `backend/app/services/prompt_registry.py`
- **新增模板**: `visual_dna_ingest_system`
- **功能**: 指导 LLM 分析图片并输出标准化的 Visual DNA JSON

#### API Endpoint
- **文件**: `backend/app/routers/ai.py`
- **路由**: `POST /ai/visual-dna/ingest`
- **请求体**:
  ```json
  {
    "project_id": 1,
    "item_id": 10,
    "asset_file_path": "characters/1/avatar.jpg",
    "version": "v1"
  }
  ```
- **响应**:
  ```json
  {
    "run_id": "...",
    "visual_dna": {
      "character_core": { ... },
      "technical_specs": { ... },
      "stable_diffusion_tags": "..."
    },
    "qc_report": null
  }
  ```

#### 安全约束
- 文件路径必须在 `/files` 目录下（通过 `data_dir()` 校验）
- 防止任意文件读取

#### 工作流
1. 读取 `series_bible`（作为上下文）
2. 调用 LLM 分析图片（当前通过文件路径描述，后续可扩展为真正的 vision API）
3. 解析 JSON 输出
4. 写入 `ContextStore.put_visual_dna()`
5. 落盘 run 快照

### 前端实现

#### API 客户端方法
- **文件**: `react-frontend/src/api/client.ts`
- **新增方法**: `ingestVisualDna(data: VisualDnaIngestRequest)`

#### UI 集成
- **位置**: `react-frontend/src/pages/ContextPage.tsx` 的 Visual DNA 区域
- **功能**: 
  - 当选中资产条目且该条目有图片资产时，显示下拉选择
  - 选择图片后自动调用摄取 API
  - 摄取成功后自动加载并显示生成的 Visual DNA JSON

### 使用流程

1. 在资产库中为角色上传立绘图片
2. 访问 `/context` 页面
3. 选择该角色资产条目
4. 在下拉中选择图片文件
5. 系统自动分析并生成 Visual DNA JSON
6. 可手动编辑后保存

### 注意事项

- **当前限制**: LLM 接口为纯 chat，不支持真正的图片识别。当前实现通过文件路径和文件名进行推断，效果有限。
- **后续扩展**: 可接入 GPT-4V、Claude Vision 等真正的 vision API。

---

## Phase 4: 多平台提示词方言支持

### 实现内容

实现了支持多种 AI 绘图平台提示词格式的功能，当前支持 SD/Flux tags 和 Midjourney v6。

### 后端实现

#### Prompt 模板扩展
- **文件**: `backend/app/services/prompt_registry.py`
- **新增模板**: `prompt_translate_mj_system`
- **功能**: 专门生成 Midjourney v6 风格的提示词

#### WorkflowStoryboardOptions 扩展
- **文件**: `backend/app/routers/ai.py`
- **新增字段**:
  ```python
  prompt_style: str = "sd_tags"  # "sd_tags" | "mj_v6"
  aspect_ratio: Optional[str] = None  # 如 "16:9", "9:16", "2:3"
  ```

#### 分支逻辑
在 `workflow_storyboard` 的 Step2（prompt 翻译阶段）：
- **SD/Flux 模式** (`prompt_style="sd_tags"`):
  - 使用 `prompt_translate_system` 模板
  - 输出：`prompt`（逗号分隔 tags）+ `negative_prompt`（tags）
  
- **Midjourney v6 模式** (`prompt_style="mj_v6"`):
  - 使用 `prompt_translate_mj_system` 模板
  - 输出：`prompt`（自然语言，`::` 分隔符，含 `--ar`、`--v 6.0`、`--stylize 250`）
  - `negative_prompt` 为空字符串

### 前端实现

#### 类型定义
- **文件**: `react-frontend/src/api/types.ts`
- **更新**: `WorkflowStoryboardOptions` 接口
  ```typescript
  {
    prompt_style?: 'sd_tags' | 'mj_v6'
    aspect_ratio?: string
  }
  ```

#### UI 控件
- **位置**: `react-frontend/src/pages/ScriptPage.tsx` 的 Scene 编辑区域
- **新增**:
  - Prompt Style 下拉选择器（SD/Flux Tags / Midjourney v6）
  - Aspect Ratio 输入框（仅在 MJ v6 模式下显示）

#### 状态管理
- **新增状态**:
  ```typescript
  const [storyboardPromptStyle, setStoryboardPromptStyle] = useState<'sd_tags' | 'mj_v6'>('sd_tags')
  const [storyboardAspectRatio, setStoryboardAspectRatio] = useState<string>('')
  ```

### 使用流程

1. 在 Scene 编辑区域，选择 prompt style（SD/Flux Tags 或 Midjourney v6）
2. 如果选择 Midjourney v6，可输入 aspect ratio（如 `16:9`）
3. 点击 "Workflow分镜" 按钮
4. 系统根据选择的 style 生成对应格式的提示词
5. 在预览中查看生成的 prompt

### 输出格式对比

#### SD/Flux Tags 模式
```json
{
  "prompt": "character, standing, rain, neon lights, cyberpunk, detailed",
  "negative_prompt": "bad anatomy, blurry, low quality, text, watermark"
}
```

#### Midjourney v6 模式
```json
{
  "prompt": "Cyberpunk City :: K standing in rain :: Neon lights, cinematic lighting --ar 16:9 --v 6.0 --stylize 250",
  "negative_prompt": ""
}
```

---

## API 参考

### Context 管理 API

#### 获取 Series Bible
```
GET /ai/context/series-bible?project_id=1&version=v1
```

**响应**:
```json
{
  "project_id": 1,
  "kind": "series_bible",
  "version": "v1",
  "exists": true,
  "data": { ... }
}
```

#### 写入 Series Bible
```
PUT /ai/context/series-bible?project_id=1
```

**请求体**:
```json
{
  "data": { ... },
  "version": "v1"
}
```

#### 获取 Visual DNA
```
GET /ai/context/visual-dna?project_id=1&item_id=10&version=v1
```

#### 写入 Visual DNA
```
PUT /ai/context/visual-dna?project_id=1&item_id=10
```

**请求体**:
```json
{
  "data": { ... },
  "version": "v1"
}
```

### Run 快照审计 API

#### 列出 Runs
```
GET /ai/runs-files?project_id=1
```

**响应**:
```json
{
  "project_id": 1,
  "runs": [
    {
      "project_id": 1,
      "run_id": "...",
      "created_at_ms": 1234567890,
      "workflow": "script"
    }
  ]
}
```

#### 读取 Run
```
GET /ai/runs-files/{run_id}?project_id=1
```

**响应**:
```json
{
  "project_id": 1,
  "run_id": "...",
  "request": { ... },
  "response": { ... },
  "meta": { ... }
}
```

#### 列出 Stages
```
GET /ai/runs-files/{run_id}/stages?project_id=1
```

**响应**:
```json
{
  "project_id": 1,
  "run_id": "...",
  "stages": ["architect.raw", "architect.parsed", "writer.raw", ...]
}
```

#### 读取 Stage
```
GET /ai/runs-files/{run_id}/stages/{stage_name}?project_id=1
```

**响应**:
```json
{
  "project_id": 1,
  "run_id": "...",
  "stage_name": "architect.parsed",
  "data": { ... }
}
```

### Visual DNA 摄取 API

#### 摄取 Visual DNA
```
POST /ai/visual-dna/ingest
```

**请求体**:
```json
{
  "project_id": 1,
  "item_id": 10,
  "asset_file_path": "characters/1/avatar.jpg",
  "version": "v1"
}
```

**响应**:
```json
{
  "run_id": "...",
  "visual_dna": {
    "character_core": { ... },
    "technical_specs": { ... },
    "stable_diffusion_tags": "..."
  },
  "qc_report": null
}
```

### Workflow API（已存在，补充说明）

#### Script Workflow
```
POST /ai/workflows/script
```

**请求体**:
```json
{
  "project_id": 1,
  "input_text": "...",
  "options": {
    "qc_loops": 1,
    "max_scenes": 50,
    "derived_split_scenes": false
  }
}
```

#### Storyboard Workflow（已扩展）
```
POST /ai/workflows/storyboard
```

**请求体**:
```json
{
  "project_id": 1,
  "scene_text": "...",
  "options": {
    "max_shots": 80,
    "asset_item_ids": [10, 11],
    "prompt_style": "mj_v6",
    "aspect_ratio": "16:9"
  }
}
```

---

## 前端页面说明

### ScriptPage (`/script`)

**新增功能**:
- Episode 编辑区域：Workflow Script 按钮组
- Scene 编辑区域：Workflow Storyboard 按钮组 + Prompt Style 选择器
- Storyboard Preview：显示 prompt 和 negative_prompt

### ContextPage (`/context`)

**功能**:
- Series Bible JSON 编辑器
- Visual DNA JSON 编辑器（按资产条目）
- 从图片摄取 Visual DNA 的下拉选择

### RunInspectorPage (`/runs`)

**功能**:
- 左侧：Runs 列表（按时间倒序）
- 右侧：Run 详情和 Stages 的 Tab 切换视图
- 支持查看完整的 request/response/meta 和每个 stage 的数据

---

## 文件结构

### 新增文件

```
react-frontend/src/
  pages/
    ContextPage.tsx          # Context 管理页面
    RunInspectorPage.tsx     # Run 审计页面

backend/app/
  (无新增文件，均为扩展现有文件)
```

### 主要修改文件

```
react-frontend/src/
  pages/ScriptPage.tsx       # 添加 workflow handlers
  api/client.ts              # 添加 Context/Run/Ingest API 方法
  api/types.ts               # 添加类型定义
  App.tsx                    # 添加新路由

backend/app/
  routers/ai.py              # 添加 Run 审计 API、Visual DNA 摄取 API、多方言支持
  services/
    context_store.py         # 添加 run/stage 浏览方法
    prompt_registry.py       # 添加新 prompt 模板
```

---

## 技术细节

### Context 存储路径

- Series Bible: `{app_data_dir}/projects/{project_id}/context/series_bible.v1.json`
- Visual DNA: `{app_data_dir}/projects/{project_id}/context/visual_dna.asset_item_{item_id}.v1.json`
- Run 快照: `{app_data_dir}/projects/{project_id}/runs/{run_id}/{request,response,meta}.json`
- Stage 快照: `{app_data_dir}/projects/{project_id}/runs/{run_id}/stages/{stage_name}.json`

### 版本管理

- 当前默认版本：`v1`
- 版本格式：`v\d+`（如 `v1`, `v2`）
- 后续可扩展为通过 API 显式指定版本

### 错误处理

- JSON 格式错误：返回 400 错误
- 文件路径无效：返回 400 错误
- Run/Stage 不存在：返回 404 错误
- LLM 输出不符合 schema：自动修复一次，失败则返回 422 错误

---

## 后续优化方向

1. **Visual DNA 摄取增强**:
   - 接入真正的 vision API（GPT-4V、Claude Vision）
   - 支持批量摄取

2. **一致性强锁定**:
   - 实现后处理插入/校验机制
   - 确保 Visual DNA 核心字段不被 LLM 改写

3. **QC 强化**:
   - 为 storyboard/prompt 阶段加入 QC loops
   - 更细粒度的错误检测和自动修复

4. **更多提示词方言**:
   - 支持更多平台（如 DALL-E、Imagen）
   - 可配置的方言参数

5. **Run 快照增强**:
   - 支持导出/导入 run 快照
   - 支持对比不同 runs
   - 支持回放/重跑某个 run

---

## 相关文档

- [AI Workflows 架构设计](./ai-workflows.md) - 整体架构与设计思路
- [JSON 一致性优化](../JSON一致性优化.md) - Visual DNA 的设计理念
- [AI 漫剧创作 Prompt 指导原则](../AI%20漫剧创作%20Prompt%20指导原则.md) - Prompt 工程最佳实践
- [剧本代理层示例讲解](../剧本代理层示例讲解.md) - 多代理工作流设计


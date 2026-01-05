# AI 漫剧多代理 Workflows（后端统一编排）代码设计文档

## 背景与目标

本项目现有后端已具备：
- 剧本骨架数据模型：`Project -> Episode -> Scene -> Shot`
- 原子 AI 能力：分场/分镜/大纲优化/剧本生成（`/ai/*`）
- 可编辑 Prompt 模板（`prompt_templates.json` 覆盖内置模板）

结合以下文档思想：
- `docs/剧本代理层示例讲解.md`：多代理分层（架构师/编剧/监理/QC/分镜师）
- `docs/AI 漫剧创作 Prompt 指导原则.md`：格式即法律、负向约束、受控词汇、元认知自检循环
- `docs/JSON一致性优化.md`：JSON 作为一致性中间层（Visual DNA/技术参数/标签串）

我们将 AI 能力从“单次调用 + 文本输出”升级为：
- **后端统一编排的 workflow**：多次 LLM 调用串联，产出可审计的结构化中间产物
- **file-first 的一致性上下文（Context）**：Series Bible / Visual DNA / run 快照
- **模块化 Prompt 组装**：role + context + constraints + instruction + output_format

## 非目标（阶段性不做）
- 不做“图片→JSON”视觉识别（当前 LLM 接口为纯 chat；后续可抽象 vision provider）
- 不强制把 workflow 结果自动写入数据库 Episode/Scene/Shot（先返回给前端 + 落 runs 快照；后续再做 Apply-to-DB）

---

## 代码结构与模块边界

### Services（可复用底座）

- `backend/app/services/app_paths.py`
  - 统一路径约定：`data_dir()` / `app_data_dir()` / `ai_settings_path()` / `prompt_templates_path()`
  - 项目级目录：`projects/{project_id}/context`、`projects/{project_id}/runs`

- `backend/app/services/llm_client.py`
  - `DeepSeekChatClient.chat(settings, messages)`：统一 DeepSeek(OpenAI兼容) chat 调用与错误透出
  - `LlmChatSettings`：base_url / api_key / model / temperature / max_tokens / timeout_seconds

- `backend/app/services/prompt_registry.py`
  - 内置模板 + 覆盖文件合并：`prompt_templates.json`
  - 关键函数：
    - `get_template_prompt(key, variables?)`
    - `render_template_with_validation(key, variables?) -> (prompt, missing_vars)`
    - `list_templates_read()`：给前端模板管理 UI
  - 新增 workflow 模板 keys：
    - `architect_system` / `writer_system` / `qc_system`
    - `storyboard_system` / `prompt_translate_system`

- `backend/app/services/prompt_composer.py`
  - `PromptModules`：role_definition / series_bible / constraints / instruction / output_format / extra_blocks
  - `compose_system_prompt_xml(modules)`：输出 XML wrapper（可读、稳定、便于模型遵循）

- `backend/app/services/context_store.py`
  - file-first 存储：
    - `series_bible.v1.json`
    - `visual_dna.asset_item_{item_id}.v1.json`
    - 每次 workflow：`runs/{run_id}/{request,response,meta}.json`
  - `snapshot_run()`：落盘可审计输入输出

---

## Context（file-first）规范

### 存储位置（不暴露给 `/files`）

`/files` 只挂载 `data_dir()`，因此我们把内部上下文放在 `app_data_dir()`（data 的父目录）：

- `app_data_dir/ai_settings.json`
- `app_data_dir/prompt_templates.json`
- `app_data_dir/projects/{project_id}/context/series_bible.v1.json`
- `app_data_dir/projects/{project_id}/context/visual_dna.asset_item_{item_id}.v1.json`
- `app_data_dir/projects/{project_id}/runs/{run_id}/request.json`
- `app_data_dir/projects/{project_id}/runs/{run_id}/response.json`
- `app_data_dir/projects/{project_id}/runs/{run_id}/meta.json`

### 版本策略

当前采用 **文件名版本**（`v1`）：
- 读取默认 `v1`
- 后续可扩展为 `v2` 并通过 API 显式指定 `version`

---

## API 设计

### 1) Context 管理 API

#### 获取 Series Bible
- `GET /ai/context/series-bible?project_id=1&version=v1`
- Response：`{ project_id, kind:\"series_bible\", version, exists, data }`

#### 写入 Series Bible
- `PUT /ai/context/series-bible?project_id=1`
- Body：`{ data: {...}, version: \"v1\" }`
- Response：`{ project_id, kind, version, path, updated_at_ms }`

#### 获取 / 写入 Visual DNA
- `GET /ai/context/visual-dna?project_id=1&item_id=10&version=v1`
- `PUT /ai/context/visual-dna?project_id=1&item_id=10`

> Visual DNA 的 `item_id` 对应当前“资产条目（asset-items）”，即 `Character` 表记录。

### 2) Workflow：剧本（script）

- `POST /ai/workflows/script`
- Request：
  - `project_id`
  - `input_text`（大纲/章节/需求）
  - `options`：
    - `qc_loops`：QC 迭代次数（0~5）
    - `max_scenes`：派生分场上限
    - `derived_split_scenes`：是否额外分场
- Response：
  - `run_id`
  - `series_bible`（object）
  - `beat_sheet`（array）
  - `script_fountain`（string）
  - `qc_report`（object）
  - `derived`（可选，包含 scenes）

工作流步骤：
1. ArchitectAgent：生成 `series_bible + beat_sheet`（严格 JSON）
2. WriterAgent：基于 `series_bible + beat_sheet` 生成 `script_fountain`（严格 JSON）
3. QCAgent：检查并可修订 `script_fountain`（严格 JSON，支持循环）
4. （可选）SplitScenes：从最终 `script_fountain` 派生场列表
5. 落盘：`runs/{run_id}` 快照

### 3) Workflow：分镜 + 提示词（storyboard）

- `POST /ai/workflows/storyboard`
- Request：
  - `project_id`
  - `scene_text`
  - `options`：
    - `max_shots`
    - `asset_item_ids`：用于锁定视觉 DNA（从 context 读取）
- Response：
  - `run_id`
  - `shots`：镜头列表（包含 prompt/negative_prompt）

工作流步骤：
1. StoryboardAgent：输出镜头结构（ShotSpec 列表，严格 JSON）
2. PromptTranslateAgent：为每个镜头生成 SD/Flux 风格 `prompt/negative_prompt`（严格 JSON）
3. 合并输出并落盘快照

---

## Prompt 模板与可定制点

### 模板文件

`prompt_templates.json` 的结构保持为：

```json
{
  "templates": {
    "split_scenes_system": { "title": "...", "category": "...", "prompt": "...", "variables": ["max_scenes"] }
  }
}
```

### 关键 keys

- 原子能力：`split_scenes_system`、`split_shots_system`、`outline_optimize_system`、`script_generate_system`
- Workflows：`architect_system`、`writer_system`、`qc_system`、`storyboard_system`、`prompt_translate_system`

workflow 的 system prompt 最终由 `PromptComposer` 组装，模板内容主要充当 `role_definition`（角色与风格锚点），而 “格式即法律/输出 schema” 由 composer 的 `constraints/instruction` 强约束。

---

## Run 快照（审计与可复现）

每次 workflow 会写入：
- `request.json`：原始输入（含 options）
- `response.json`：最终输出（含 run_id）
- `meta.json`：`{ project_id, run_id, created_at_ms, workflow }`

后续可扩展：
- `stages/architect.json`、`stages/writer.json`、`stages/qc_1.json` …（用于更细粒度追踪）

---

## 迁移与兼容

### 对现有前端的影响
- 原子 API (`/ai/split-scenes` 等) **保持不变**
- 前端可渐进式接入 workflows：先使用 `/ai/workflows/script` 拿到 `script_fountain/beat_sheet` 再写回 DB

### 对现有后端的影响
- `backend/app/routers/ai.py` 已复用 services（LLM client / prompt registry / json extract）
- 新增 endpoints 不影响旧 endpoints（兼容优先）

---

## 错误处理与限流策略（当前实现）

- 未配置 API Key：`400 AI API Key 未配置`
- LLM 输出不符合 schema：`422`
- `qc_loops` 做上限保护：最多 5 次（避免失控的成本/时间）
- `asset_item_ids` 上限 50（避免 prompt 过长）



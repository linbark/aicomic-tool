# aicomic-ai

原生 Rust AI 调用与工作流层。运行时不依赖 Python、Node、WebView、本地 HTTP 服务或 Tauri。

`WorkflowEngine::new(root, settings)` 的 `root` 是应用数据根目录，包含
`ai_settings.json`、`prompt_templates.json`、`data/` 和 `projects/`。
构造引擎、读取数据与修改模板不请求模型。只有 `execute`、`resume` 中需要重新生成的步骤、
以及用户主动调用 `test_connection` 或 `list_models` 才可能发出网络请求；列举模型只读取目录，不请求生成。

## 公共契约

```rust,ignore
let engine = WorkflowEngine::new(root, settings);
let result = engine.execute(ExecutionInput {
    project_id: numeric_project_pk.to_string(),
    episode_id: Some(episode_pk),
    script,
    context: serde_json::json!({
        "retrieved_memory": retrieved_memory,
        "artifacts": previous_artifacts,
    }),
    action: "episode_execute".into(),
    run_id,
}).await?;
```

旧项目目录使用数据库数字主键，不使用项目 UUID。`ExecutionInput.context` 原样进入创作上下文，
可提供已检索记忆、负向约束、已确认事实和各动作参数；引擎同时读取已有 Series Bible 和项目大纲。
AI 层不直接查询或修改核心数据库、记忆数据库。

新 Run 的 `context.artifacts` 会在执行入口按 `reusable_artifacts` 的创作产物白名单清理，
保留大纲、剧本、视觉资产等内容，剥离 `approved_*`、入库计划、待审 Changeset、Run ID、
状态及其他执行标记；应用也可在构造上下文前调用这个函数。原 Run 的恢复审核状态不经过此清理。

`WorkflowResult` 包含 `run_id/status/artifacts/output_text/waiting_for`。
通过 `with_events(tokio::sync::mpsc::UnboundedSender<WorkflowEvent>)` 可接收持久化后的阶段事件。
也可读取 `ContextStore::read_stage/list_stages/read_run/list_runs`。

## 工作流

| action | 行为 / 主要产物 |
| --- | --- |
| `episode_execute` | 大纲 → 视觉资产 → 分集 → 人工确认分集 → 入库计划 → 人工确认入库 |
| `episode_outline_generate` | `artifacts.outline` JSON object |
| `episode_assets_visual_dna` | `artifacts.assets_visual_dna` JSON object |
| `episode_split_episodes` | `artifacts.split_episodes` JSON object |
| `outline_generate/outline_optimize` | `artifacts.outline` 文本 |
| `script_generate/generate_script/script_optimize` | `artifacts.script` 文本 |
| `project_outline_generate/optimize` | 保存 `project_outline.v1.json`；优化另返回 `changes_summary` |
| `split_scenes/split_shots` | JSON 数组；参数 `context.max_scenes/max_shots` |
| `workflow_script` | Architect → Writer → 0–5 次 QC；`series_bible/beat_sheet/script_fountain/qc_report`；支持 `derived_split_scenes` |
| `workflow_storyboard` | ShotSpec → SD/MJ 提示词；`artifacts.shots`；支持 `max_shots/prompt_style/aspect_ratio/locked_visual_dna` |
| `visual_dna/visual_dna_ingest` | 保存 `visual_dna.asset_item_{id}.{version}.json`；参数 `item_id/asset_file_path/version` |
| `chat` | Planner 最多 4 个白名单动作；信息不足返回 `needs_clarification` |
| `memory_extract_changeset` | 生成 `changeset.v0` 待审 payload，暂停 `review_changeset` |

Visual DNA 保留原服务的能力限制：当前 provider 是文本 chat，只按文件名/提供文本推断，
没有读取图片像素或声称视觉识别；结果标记 `artifacts.visual_dna_source = "filename_text_only"`。

## 审核与应用层职责

`resume(project, run, ReviewDecision::{Approve, Reject, Regenerate}, edited_artifacts)`
可以跨进程重启恢复新任务；恢复状态使用 `chat.interrupt.resume_state.schema_version = 1`。
兼容旧 Python **episode workflow** 的 flat `resume_state` 与数字项目目录。
旧 ChatGraph 的任意内存节点状态不是相同格式，不进行猜测恢复；已保存记忆变更单应在记忆审核页处理。

确认产生的是提交许可，数据库写入仍属于应用事务层：

- 确认分集后返回 `artifacts.approved_split_episodes`。
- 确认入库后返回 `artifacts.approved_asset_ingest_plan`，包含 `items/series_style`。
- 确认记忆后返回 `artifacts.approved_changeset`。

应用层必须按 `run_id + operation` 幂等提交，拒绝跨项目写入。`approved_*` 可能在后续快照中继续存在，
不能因重新打开界面或重复收到回调而再次创建相同条目。引擎状态 `done` 仅代表 AI 工作流完成，
不代表数据库已经提交。记忆 ChangeSet 应由应用调用 `create_changeset/apply_changeset`；
自动 episodic 提取与语义检索也应由应用使用记忆层 API。

## 持久化与调用策略

- Prompt 默认值 15 项静态移植原 `prompt_registry.py`；覆盖文件保留 `{"templates":{...}}` 格式。
- Run 保留 `projects/{pk}/runs/{run}/request.json/response.json/meta.json/stages/{stage}.json`。
- Context 保留原 `projects/{pk}/context/*.v1.json` 格式。
- 文件先写临时文件再替换；Unix 配置/快照文件权限为 `0600`；调试输出遮盖 API key。
- OpenAI 兼容 `/v1/chat/completions`；支持 base URL 已带 `/v1` 或完整 endpoint。
- `AiSettings.provider` 为 `openai`（旧配置默认）或 `google`。Google 使用原生 `models/{model}:generateContent`，默认模型 `gemini-flash-latest`，以 `x-goog-api-key` 请求头认证；密钥不进入 URL。见 [Google 接口文档](https://ai.google.dev/api/generate-content)。
- `LlmClient::list_models(&settings)` 返回当前服务完整模型目录与能力元数据，不依赖当前模型名。Google 使用 `GET /v1beta/models`（或配置的 `/v1`）并处理 `nextPageToken`；OpenAI 兼容服务使用 `GET /v1/models`。目录保留不支持聊天的条目，调用方可用 `ModelInfo::supports_chat(provider)` 检查 Google 是否声明 `generateContent`；OpenAI 兼容目录缺少统一能力字段，不推断每个模型的聊天兼容性。模型被列出不保证当前账号有生成权限或额度。
- Google 的 system/developer 消息映射到 `systemInstruction`，assistant 映射为 `model`；输出仅拼接非 thought 文本，拒绝被拦截或不完整的回复。此适配只发送文本，不发送图片或调用工具。
- reqwest 使用 rustls；超时/网络断流至多 3 次请求，350ms/700ms 退避；HTTP 错误直接返回。
- 发送输出上限按原逻辑归一化为最多 8192；`length/max_tokens/MAX_TOKENS` 或明显未闭合 JSON 返回截断错误。Google 的连接探测预留 1024 tokens（包含思考额度），OpenAI 兼容探测为 8 tokens。
- 结构化动作支持 fenced JSON/前后说明提取；根类型错误使用同一模型修复一次，原始输出始终保留。
- ShotSpec/Prompt 对数量和受控参数进行校验；schema 不满足时明确失败，不自动补造有效镜头。

## 本地验证

`cargo test -p aicomic-ai` 使用临时目录和本地 mock HTTP，不调用真实模型。
`cargo clippy -p aicomic-ai --all-targets -- -D warnings` 检查代码质量。
库测试不能证明原生 UI 已接线，也不能代替 Mac/Windows 打包、中文输入、媒体播放或真实 provider 验收。

# Rust 桌面迁移：功能对照与验证边界

> 2026-11-09 界面重设计后注：本文的“当前 Rust 界面”描述对应迁移完成时的界面；此后新增的制作台、五阶段闸门、分镜、生成前确认与编辑器覆盖层见[《星晴工作流与界面设计》](../design/星晴工作流与界面设计.md)。本文的功能边界与验证证据仍有效，界面入口以新设计为准。

本文记录原版实际可达界面与当前 Rust 桌面界面的对应关系，不把“源码里有组件/接口”当作“用户已经能在界面操作”。旧 Python/React/Tauri 源码已从当前工作树移除；原版行为的审计基线为 Git 提交 `ce78de1`，可通过 `git show ce78de1:<原路径>` 查询历史源码。

## 运行架构

| 层 | 原版 | Rust 版 |
| --- | --- | --- |
| 界面 | React/TypeScript、浏览器 DOM、Tauri WebView | Iced 原生窗口、编辑器与 Canvas |
| 业务 | Python/FastAPI，Tauri Rust 代码启动 Python 子进程 | Rust 领域库与异步任务，直接调用本地存储与远程 provider |
| 通信 | 本地 HTTP、Axios；约每 800ms 查询 Run Stage | 进程内调用；持久化阶段、界面状态刷新及可选 Rust 阶段事件 |
| 存储 | 项目 SQLite、记忆 SQLite、JSON、素材目录、Qdrant 本地目录 | 兼容原 SQLite/JSON/相对素材；新的可重建 SQLite 密集向量缓存 |
| 安装运行 | 依赖 Python 环境，界面构建依赖 npm | 已构建应用不依赖 Python、Node、npm、WebView 或本地 API 服务 |

这里的“纯 Rust”指当前应用的界面和业务源码、执行流程均为 Rust。平台 SDK、系统图形/文件对话框和随程序编译的 SQLite 仍属于原生依赖。

## 原版实际界面基线

原版主导航为“剧本、事件、资产、AI 设置、Prompt 模板”。`/context` 和 `/runs` 有路由，但没有主导航入口。剧本页实际挂载 Step 0 原始剧本、Step 1 结构化拆解、Step 2 资产提取。

`Step3_Storyboard`、`ChatPanel`、`EpisodeEditor`、`SceneEditor`、`ShotEditor` 等文件存在，但没有接入该剧本页；`RunStatusPanel` 也只被未挂载组件引用。资产提取卡片上的“入库”仅弹出占位提示。原版没有富文本编辑器，也没有真正的图像/视频生成、剪辑或成片导出实现。

审计参考文件在 `ce78de1` 中的原路径为：`react-frontend/src/App.tsx`、`react-frontend/src/components/AppLayout.tsx`、`react-frontend/src/pages/ScriptPage.refactored.tsx`、`react-frontend/src/components/script/ScriptSidebar.tsx` 与 `react-frontend/src/components/script/steps/Step2_Assets.tsx`。例如，使用 `git show ce78de1:react-frontend/src/App.tsx` 查看旧版入口。

## 用户操作对照

| 功能 | 原版实际行为 | 当前 Rust 界面与差异 |
| --- | --- | --- |
| 项目与分集 | 新建/删除项目及集、选择项目，原始剧本为 textarea | 原生目录、标题/正文编辑、项目简介、保存及删除确认；编辑缓存按项目/集隔离 |
| 三步创作 | 原始剧本→结构拆解→资产提取；缺少结果时，进入 Step 1/2 可能自动请求模型 | 三步入口保留；使用明确的“结构拆解”“提取资产”“完整执行”动作发起生成，切换页签本身不触发模型 |
| 结构画布 | 拖动结构卡片、平移与缩放，JSON 是画布内可拖浮窗 | 原生画布保留卡片拖动、平移缩放；详情改为右侧固定面板，按字段标题、列表编号与层级缩进展示，不能视为原浮窗布局复刻 |
| 资产提取显示 | 人物、道具、场景、全局风格卡片有专门视觉样式，JSON 只读 | 用原生卡片与可读详情展示，正文按文字宽度折行并保留缩进；超出卡片明确提示在详情查看，完整内容不因预览省略而丢失 |
| 剧本优化 | 后端有生成/优化接口，当前挂载剧本页没有独立聊天或生成操作台 | 提供“优化剧本”及显式“应用生成剧本”，模型结果不会自动替换原稿 |
| 完整执行与审核 | 后端有大纲→资产→分集→入库计划及确认接口，当前 React 页未完整挂载审核控制 | 提供完整执行、分集确认、入库确认、拒绝、取消及重启后继续；写入失败保留产物并允许重试，按 Run/操作幂等提交 |
| 事件管理 | 搜索、多选、排序、名称/描述/颜色编辑；详情路由可添加场景关联 | 原生搜索、聚焦多选、名称/关联数量排序、元信息编辑和关联编辑；详情合并在同页，颜色为 `#RRGGBB` 文本输入，关联可取消 |
| 事件矩阵 | 分集为行、集内场景序号为列，事件连线串起其关联场景；缩放、拖动平移 | 原生 Canvas 保留分集行/集内场景列和空分集，事件按剧情顺序跨行连接；支持图例/节点/曲线选择、平移与 0.5–2 倍缩放，绘制外观不保证逐像素一致 |
| 资产库 | 12 类条目 CRUD、多文件上传、素材删除；图片灯箱、内嵌音视频；文档下载/打开 | 保留 12 类、条目名称/描述/基础 Prompt 编辑、系统多文件导入及删除；图片内嵌预览，音视频和文档交给系统应用，无内嵌播放器或图片灯箱 |
| AI 设置 | 服务地址、模型、Key、温度、输出上限、超时与测试连接 | 保留配置字段并增加 OpenAI 兼容/Google AI 服务选择，由 Rust 直接调用；Gemini 默认 `gemini-flash-latest`，使用原生 `generateContent`。新增显式获取完整模型目录并搜索选择，当前已保存模型保留显示；获取失败可重试，仍可保存其他设置。Google 不支持当前接口的条目标注并拒选，OpenAI 目录不推断统一能力。切换服务清除旧 Key，配置变化使旧目录请求失效；旧 JSON 设置继续兼容，只有主动操作才联网 |
| Prompt 模板 | 搜索、内置覆盖、自定义 CRUD、恢复内置 | 原生搜索、编辑/保存、删除和恢复；保留 15 个内置默认模板与原覆盖文件格式 |
| 项目上下文 | 隐藏路由可读写 Series Bible、项目大纲、Visual DNA；大纲可生成/优化 | 主导航提供“项目上下文”，用分层表单编辑故事设定、项目大纲与资产 Visual DNA；支持添加/删除字段和列表条目，保留未知字段与原数据类型，非法数字阻止保存。可生成/优化项目大纲，优化采用当前大纲表单的编辑；项目级操作不要求绑定一集 |
| 项目迁移 | 无单文件项目导入/导出 | 项目管理可导出单个 `.xqproj`；顶栏与首次启动页可导入。文件含项目、分集、场景/镜头、资产与素材、事件、设定/大纲/Visual DNA/闸门；不含运行历史、记忆库、AI 密钥与全局 Prompt。导入创建新项目并重写 ID、素材路径与闸门分集键 |
| Visual DNA 推断 | 文本 chat 只参考素材文件路径/名称，不能查看图片像素 | 素材图片提供推断入口，明确标注按文件名和描述推断；不是图片识别或视觉模型验收 |
| Run/Stage 审计 | 隐藏 `/runs` 路由和剧本页调试窗查看请求、结果、Stage/日志 | 独立“运行记录”页、运行过程入口及阶段刷新；日志、错误和原始响应保存在本地；不是旧调试浮窗的相同外观 |
| 记忆 | 通过后台检索、证据与人工审核相关接口参与生成，原主导航无独立记忆页 | 提供“记忆”页事实浏览、词面/语义检索、重建、变更批准/拒绝与冲突处理；事实、变更与冲突采用字段标题和缩进阅读，原始审核数据不变；生成流程接入任务上下文与自动记忆桥接 |

本表描述当前操作入口，不表示所有平台的人机交互均已通过人工验证。中文输入法、剪贴板、窗口尺寸、系统文件对话框、媒体关联等仍应在各平台实测。

## 库能力与界面入口分别计算

`aicomic-ai` 还实现了 `chat` planner、`workflow_script`、`workflow_storyboard`、`split_scenes`、`split_shots`、`script_generate` 等动作。它们有库测试与 Run 记录，但当前界面没有完整的聊天窗口或独立场景/镜头编辑工作台，不能将这些 API 的存在描述为相应 UI 已交付。

旧 Step 3、旧 ChatPanel 和未挂载编辑器不是此次“已有可用 UI”基线；原占位“入库”也不是现成的原功能。新版完整执行的审核入库是实际接线的流程，不能反过来将原占位描述为原版已支持。

## 数据兼容与审核边界

应用根目录的结构、默认位置、迁移前整目录备份及运行命令见 [README](../../README.md#旧数据与备份)。新旧版本应分别操作各自副本，避免同时写同一目录。

- 主库复用原项目数字主键/UUID、分集/场景/镜头/资产/事件结构，首次确需迁移时先备份主 SQLite；不会在每次启动反复生成迁移备份。
- `memory_store.db` 保留事实与审核来源；`projects/<数字主键>/context`、`runs`、AI 设置、Prompt 覆盖及相对素材路径继续使用。旧 WebView 的 localStorage 缓存不是原生存储来源，未承诺导入其中尚未落盘的文本或选择状态。
- 新 Run 只继承创作产物，剥离旧 Run 的 `approved_*`、入库计划和执行标记；批准许可不会因换 Run ID 而重新生效。同一 Run 的恢复状态保留，并由应用提交层按 Run/操作去重。
- 新原生恢复状态使用 `chat.interrupt.resume_state.schema_version = 1`，兼容旧 Python episode workflow 的 flat 状态。旧任意 ChatGraph 内存节点不能自动猜测恢复；已保存 ChangeSet 可在记忆页处理。
- AI 完成与数据库提交分别记录；取消不撤销已确认提交，也不将未完成任务标为成功。遇到写入失败，应先重试已保留产物的提交，再继续对应审核。

## 记忆与模型的具体差异

原 Python 的 SentenceTransformer/Qdrant 路径改为可配置的 HTTP Embedding 或 Rust/Candle CPU 推理。默认关闭语义模型时，结构化任务上下文、原文事实与审核仍可使用；不会伪造向量或将词面匹配称为语义检索。

新向量缓存位于 SQLite，使用精确余弦扫描；旧 Qdrant HNSW 目录不直接读取或改写，因此索引应由现有事实重建，不能承诺原有大规模性能。模型身份/维度不匹配需要重建；兼容模型目录、外置权重、内存占用说明见 [记忆模块](../../crates/aicomic-memory/README.md)。

自动记忆桥接保存来源证据，连接 Series Bible/Visual DNA 索引、结构化节拍/镜头/QC 的 episodic 信息及变更审核。原 Python 本来未应用到 canonical 表的 `events/state_changes`，以及 TODO/恒空的 Fountain 状态抽取、NER、L0 buffer、Prompt token 截断等，均不被宣称为迁移后已有的完整能力。

完整 BGE-M3 权重未下载或做质量等价评估；已有本地推理测试使用微型真实 XLM-RoBERTa 权重。真实远程模型服务、完整模型峰值内存/速度以及不同平台推理行为需要另行验收。

## 构建、发布与验证证据

[原生 CI](../../.github/workflows/rust-desktop.yml) 配置了 macOS arm64、macOS Intel x64、Windows x64，执行格式检查、workspace 测试、严格 Clippy、release 构建与对应平台打包。macOS 脚本生成 `.app`/ZIP/DMG，Windows 脚本生成 EXE/NSIS 当前用户安装器。

截至 2026-10-22 的本机证据（macOS 27，Apple Silicon）：

| 检查 | 结果与范围 |
| --- | --- |
| 自动化 | `cargo test --workspace --all-targets --locked`：159 项通过，AI 48、存储 21、桌面/集成 60、记忆 30；workspace 格式与严格 Clippy 通过。覆盖分层可读展示、画布长行折行、上下文表单类型保真/非法输入/项目隔离，以及大纲优化使用未保存编辑（含模型修复请求） |
| Google AI | 原生 `generateContent`、系统指令、对话角色、输出参数、响应拦截/截断、密钥脱敏及设置切换使用本地模拟验证；另以 `gemini-flash-latest` 实际执行一次简短连接探测并返回“连接成功”，未发送项目内容，未将密钥写入源码或安装包。新版 arm64 APP 在隔离演示目录实测 Google 设置页、模型下拉菜单，以及切换服务后旧 Key 清空、地址/模型恢复默认 |
| 模型目录 | 2026-10-22 在隔离演示 APP 中使用 Google 原生目录接口实际获取 63 项，46 项声明支持 `generateContent`；原生搜索下拉显示名称、ID 和不适用标记，选择 `gemini-2.5-flash` 并保存后核对临时设置落盘，默认用户设置仍为原模型。仅获取目录，未调用生成。OpenAI 兼容目录及各类分页/失败路径使用本地模拟服务验证 |
| 构建独立性 | `bash packaging/check-release.sh --offline --self-test` 通过；只包含 Cargo、crates、packaging 与 CI 的临时源码树可解析锁定 workspace，注入旧构建步骤会被拒绝 |
| Apple Silicon | Rust 1.93.1 release 构建、APP/ZIP/DMG 生成、架构/签名/DMG 校验通过；动态库仅为 macOS 系统库 |
| 原生窗口 | 使用系统窗口观察，确认中文粘贴、正文/标题保存、分集切换、事件矩阵选择/缩放、系统文件对话框导入和图片显示；素材导入后数据库条目数增加。保存快捷键额外插入字符的问题经实机复现、回归修复和发布版复验，正文与标题落盘内容精确匹配 |
| 可读界面 | 2026-10-22 在独立演示目录实测生成详情、记忆和运行记录的分层展示，以及画布放大后文字不重复折行；项目上下文表单将数字从 24 改为 25 后保存，核对落盘仍为数字，嵌套字段、开关、列表和段落换行保留。未调用生成接口 |
| 旧数据副本 | 对迁移前主库与现有记忆/Context 副本启动，主库先生成一次备份，项目 1 个、分集 1 集保持不变，主库与记忆库 `integrity_check` 均为 `ok`；原始正文在原生窗口可读 |
| Intel Mac | 使用官方 Rust 1.97.1 交叉构建 x86_64 release，APP/ZIP/DMG 校验通过；最终包在独立临时数据目录通过 Rosetta 启动检查，退出 0。尚无 Intel 硬件验收或 Intel CI 运行结果 |
| Windows | 已配置 MSVC 静态 CRT、EXE/NSIS 打包、原生 CI 测试及安装卸载检查；尚未生成并实际运行 Windows 产物，不能宣称 Windows 验收通过 |

自动化使用本地模拟 HTTP 与微型真实推理权重，不调用真实付费模型，也不证明完整 BGE-M3 质量或界面全覆盖。内置截图路径不能作为编辑器文字渲染的验收证据，文字显示采用真实系统窗口观察。

Intel 交叉构建为避开 [macOS 27 构建工具动态库对齐问题](https://github.com/rust-lang/rust/issues/157750)，使用显式官方 `RUSTC` 与 `--config profile.release.build-override.strip=false`，不改变最终应用的剥离设置。

macOS 使用 ad-hoc 签名，尚无 Developer ID 签名/Apple 公证；Windows EXE 与安装器未签名。默认部署目标 macOS 13 是配置，不是该系统版本的验收记录。当前交付不等于双平台所有交互的全量验收。

源码定位：界面 [app.rs](../../crates/aicomic-desktop/src/app.rs)，创作画布 [board.rs](../../crates/aicomic-desktop/src/board.rs)，事件画布 [event_matrix.rs](../../crates/aicomic-desktop/src/event_matrix.rs)，记忆桥接 [integration.rs](../../crates/aicomic-desktop/src/integration.rs)，[AI 契约](../../crates/aicomic-ai/README.md) 与 [打包说明](../../packaging/README.md)。

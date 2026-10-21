# 星晴纯 Rust 桌面迁移设计

目标：macOS 与 Windows 桌面应用；界面与业务均用 Rust；保留当前功能和布局习惯，不依赖 Python、npm、WebView 或本地 HTTP 服务。

## 已确认的范围

2026-10-21 用户确认保留现有功能与布局习惯，界面全部改为 Rust。审计基线为 `ce78de1`。旧项目包括 Python/FastAPI、React 和 Tauri 1；Rust 原本仅为 Python 进程启动器。没有发现此前已完成的业务 Rust 迁移。

旧代码约 1.7 万行 Python，88 个路由声明；其中 `ai_chat.py` 提前返回后的旧编排不属于移植对象。剧本页实际使用原始剧本、结构拆解、资产提取三步；Step3 和旧编辑器组件未接入。资产提取“入库”是占位，不计为已完成功能。原版并无图像或视频生成后端。

## 架构

采用 Cargo workspace：

- `aicomic-core`：领域模型、SQLite、项目/集/场/镜头/资产/事件、文件与兼容路径。
- `aicomic-ai`：可配置 HTTP 模型客户端、提示词、Context、Run/Stage、工作流与暂停恢复。
- `aicomic-memory`：记忆事实、证据、实体、快照、约束、变更集审核、冲突裁决与可重建向量索引。
- `aicomic-desktop`：Iced 0.14 窗口与编辑交互。沿用深色顶部导航、左侧剧本目录、主编辑/画布区。

Iced 和 egui 都支持目标平台。Iced 的消息/任务模式适合当前编辑与后台生成交互，作为首选；egui 作为画布工作流的备选。Slint 需采用另一种 UI 描述语言并处理发行许可，因此不作为严格 Rust 源码方案的首选。

模型请求用异步任务，数据库和文件工作避免阻塞界面。完成事件带项目/集/Run 身份，防止切换后将旧任务结果写到新集。失败必须显示原因并留下运行档案。退出时不得把未完成任务误标为成功。

## 数据保护与兼容

兼容主 SQLite 的旧字段、稳定 UUID 与数字主键；保留 `memory_store.db`、`projects/<数字ID>/context`、`runs`、相对媒体路径、AI 设置与 Prompt 覆盖。打开默认或显式指定的数据根时，必要的主库结构升级会先生成 SQLite 备份，再执行迁移；这不代替整目录备份。自动化回归和破坏性验证使用临时目录，旧数据兼容验收使用迁移前主库与现有记忆/Context 的独立副本。

Qdrant Python 本地目录不是 Rust 客户端可直接打开的嵌入式数据库。保留其目录，使用原始证据/记忆重建索引。embedding 原实现实际是 SentenceTransformer BGE-M3；需要纯 Rust 推理或显式配置的兼容 embedding 服务，不能静默生成假向量。语义检索未配置时应明确说明，并允许浏览事实记录。

## 功能验收

1. 项目/集 CRUD、中文剧本编辑、保存重启恢复。
2. AI 设置、连接测试、Prompt 搜索/编辑/覆盖/恢复。
3. 原始剧本 → 结构拆解 → 资产提取，保留画布缩放/平移/卡片拖动及原始输出。
4. Run/Stage、错误与暂停确认续跑、项目大纲/Series Bible/Visual DNA。
5. 资产分类、文件导入、图片/音频/视频、文件打开；媒体播放需双平台单独验收。
6. 事件矩阵、颜色、节点关联与曲线画布。
7. 记忆存储与审核、真实语义 embedding 与索引重建。
8. macOS arm64/x64 与 Windows x64 原生构建和启动；Windows CI 配置不能代替实际通过证据。

不得以空界面、仅图片预览、运行按钮占位、仅 Mac 编译通过作为全量迁移完成证据。旧实现保留作对照，所有功能验收通过后才清理。

## 官方资料

- Iced：https://github.com/iced-rs/iced
- egui：https://github.com/emilk/egui
- Iced 0.14 editor 示例：https://github.com/iced-rs/iced/tree/0.14.0/examples/editor

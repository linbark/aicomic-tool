# 星晴 · AI Comic Tool

面向 macOS 与 Windows 的原生 Rust 漫剧创作桌面应用。界面使用 Iced，项目与记忆保存在本机 SQLite；AI 请求由 Rust 直接调用可配置的 OpenAI 兼容服务。

发布程序不需要 Python、Node、npm、WebView、Tauri 或本地 API 服务。源码构建使用 Cargo 和平台编译工具；本地大模型权重不包含在安装包中。

当前可使用项目/分集编辑、剧本优化、结构拆解与资产提取、可缩放画布、事件矩阵、素材库、项目上下文、Prompt 模板、运行审计和记忆审核。迁移保留创作流程，但并非旧版的逐像素复刻或全部接口的界面化；具体差异见 [迁移对照](docs/rust-migration.md)。

## 开发与测试

需要 Rust 工具链，以及 macOS 的 Xcode Command Line Tools，或 Windows 的 MSVC C++ Build Tools 与 Windows SDK。workspace 声明 Rust 1.88 起，依赖版本以提交的 `Cargo.lock` 为准。

先使用独立目录预览，避免修改原工作区：

```sh
cargo run --locked -p aicomic-desktop -- --data-dir "./demo-data" --demo
```

`--demo` 仅在空库中创建演示内容，不下载模型或请求 AI。常用检查：

```sh
cargo fmt --all -- --check
cargo test --locked --workspace --all-targets
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo build --locked --release -p aicomic-desktop
```

release 可执行文件位于 `target/release/aicomic-tool`，Windows 为 `aicomic-tool.exe`。窗口冒烟检查会打开真实桌面窗口，需要图形会话：

```sh
cargo run --locked --release -p aicomic-desktop -- --data-dir "./smoke-data" --demo --smoke-test
```

自动化测试使用临时数据库、本地模拟 HTTP 和微型推理权重；它们不等于真实付费模型、完整 BGE-M3 或每个平台的人工验收。

## 旧数据与备份

`--data-dir` 指向**应用数据根目录**，其中应包含 `database.db`，以及同级的 `data/`、`projects/` 等内容；不要把它指向旧 `AICOMIC_DATA_DIR` 对应的素材 `data/` 子目录。

```text
应用数据根/
├── database.db                 # 项目、分集、资产、事件等
├── memory_store.db             # 记忆事实、证据、审核及原生向量缓存
├── ai_settings.json
├── embedding_settings.json
├── prompt_templates.json
├── data/                       # 相对路径素材
├── projects/<数字项目ID>/
│   ├── context/
│   └── runs/
└── backups/
```

首次复用旧数据前，关闭旧版与新版应用，复制整个根目录作为备份，再对副本运行：

```sh
cargo run --locked -p aicomic-desktop -- --data-dir "/path/to/copied-app-data"
```

Windows 可将路径替换为 `"C:\path\to\copied-app-data"`。也可使用环境变量 `AICOMIC_APP_DIR`；命令行参数优先。未指定时默认位置为：

- macOS：`~/Library/Application Support/com.ljc.aicomictool`
- Windows：`%APPDATA%\com.ljc.aicomictool`

主数据库确需结构迁移时，会先通过 SQLite 备份 API 写入 `backups/database-before-rust-*.db`。这只覆盖主库迁移，不是素材、记忆库或设置的整目录备份。恢复时先关闭应用，并使用一致的整套备份；AI/Embedding 设置可能含密钥，应按私密数据保存。外置模型目录也需单独保管。

旧项目数字主键、UUID、Context/Run 文件与相对素材路径继续使用；旧 Qdrant 目录不会直接作为 Rust 向量引擎打开。请保留它作为历史数据，原生语义索引从 SQLite 事实重建。

## AI 与记忆模型

在“AI 设置”填写服务地址、API Key 和模型名，保存后可主动点击“测试连接”。剧本生成、项目大纲、Visual DNA 和 HTTP Embedding 会访问所配置的远程服务；普通编辑、素材管理及事实浏览不需要这些服务。

记忆 Embedding 默认 `disabled`，仍可查看原文、词面检索、使用结构化记忆上下文及审核变更。启用语义检索有两种方式：

- `http`：配置 OpenAI 兼容 Embedding 服务。
- `local`：指定兼容 XLM-RoBERTa/BGE-M3 的模型目录，包含 `config.json`、`tokenizer.json`、`model.safetensors`。Rust/Candle 在 CPU 上执行，不自动下载或转换模型。

保存模型配置后，在“记忆”页显式重建已有事实的语义索引；更换模型身份或向量维度后也需要重建。索引就绪后，后续任务检索可增量更新变更的事实。原生缓存使用 SQLite 密集向量与精确余弦扫描，不承诺旧 Qdrant HNSW 的大规模性能。

完整 BGE-M3 权重的准确率、峰值内存和 CPU 速度尚未完成实测。现有推理测试验证的是微型 XLM-RoBERTa 权重的真实 Rust 张量路径；较大的 safetensors 会占用更多内存。详见 [记忆模块](crates/aicomic-memory/README.md)。

Visual DNA 当前保留原版文本接口的能力限制：按文件名、资产描述和已知上下文推断，不读取图片像素，不应当作已验证的图像识别。

## 桌面打包与验证状态

macOS 在对应平台构建 `.app`、应用 ZIP 和 DMG：

```sh
bash packaging/macos/package.sh --target aarch64-apple-darwin
bash packaging/macos/package.sh --target x86_64-apple-darwin
```

需要对应 Rust target 和原生构建环境，默认部署目标为 macOS 13。输出分别位于 `dist/macos-arm64/` 和 `dist/macos-x64/`。

Windows x64 在 Windows PowerShell 中安装 Rust MSVC 工具链与 NSIS 3 后运行：

```powershell
./packaging/windows/package.ps1
```

输出 `dist/windows-x64/` 下的独立 EXE 与当前用户安装器；卸载保留应用数据。详细参数见 [打包说明](packaging/README.md)。

| 目标 | 当前证据与边界 |
| --- | --- |
| macOS arm64 | release、APP/ZIP/DMG 及校验通过；在 macOS 27 实测中文编辑、保存快捷键、切集、事件矩阵选择/缩放、系统素材导入和图片预览。不是所有交互或最低系统版本的完整验收 |
| macOS Intel x64 | 本机交叉编译与 APP/ZIP/DMG 打包通过；Rosetta 隔离启动检查退出 0，尚无 Intel 硬件实测 |
| Windows x64 | 已配置原生 CI、EXE/NSIS 打包及安装/卸载检查；本次尚未在 Windows 实际运行 |
| 签名 | macOS 仅 ad-hoc 签名，无 Developer ID 签名或 Apple 公证；Windows EXE/安装器未签名 |

CI 配置见 [rust-desktop.yml](.github/workflows/rust-desktop.yml)；配置存在不代表运行成功。最低 macOS 版本配置也不代表已在该版本实测。音频、视频和文档目前通过系统默认应用打开，图片可在素材库内预览。

2026-10-21 最终本机检查：107 项测试通过（AI 24、存储 21、桌面与集成 32、记忆 30），workspace 格式与严格 Clippy 检查通过。旧数据副本启动后，项目/分集数量保持一致，主库与记忆库完整性检查通过。具体范围见 [验证证据](docs/rust-migration.md#构建发布与验证证据)。

## 源码布局

```text
crates/
  aicomic-core/       # SQLite 兼容迁移、项目/资产/事件与文件
  aicomic-ai/         # 模型客户端、Prompt、Context/Run、工作流与审核恢复
  aicomic-memory/     # 事实、证据、变更审核、检索与可选 Rust 推理
  aicomic-desktop/    # Iced 窗口、编辑器、画布与应用集成
packaging/           # 独立图标、macOS 与 Windows 打包脚本
legacy/              # 旧 Python/FastAPI、React/Tauri 历史参考，不参与 Cargo 编译或打包
```

旧架构文档与 `legacy/` 用于追溯行为，不作为当前运行方式的说明。当前能力、已知差异和验收范围以 [Rust 迁移说明](docs/rust-migration.md) 为准。

# Pure Rust Desktop Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 将星晴迁移为保留现有功能和布局习惯的 macOS/Windows Rust 桌面应用。

**Architecture:** Cargo workspace 分离持久化、AI、记忆和 Iced 界面。Rust 模块直接调用，不启动 Python、本地 HTTP 或 WebView；既有 SQLite 与 JSON 为兼容基线。

**Tech Stack:** Rust 2024、Iced 0.14、Tokio、rusqlite、reqwest/rustls、serde。

---

### Task 1: 存储与旧数据兼容

**Files:** `crates/aicomic-core/{Cargo.toml,src/lib.rs,src/models.rs,tests/*}`。

1. 建立临时旧结构 SQLite，写项目/集/场/镜头与资产关联，先写重启恢复和级联删除失败测试。
2. 运行 `cargo test -p aicomic-core`，确认失败原因是缺失能力。
3. 实现 `Store::open`、版本迁移、typed CRUD、资源路径和安全 JSON 原子写入；保留旧字段。
4. 运行测试，增加 UUID/数字 ID、孤立文件、路径越界和事务回滚回归。

### Task 2: AI 客户端与工作流

**Files:** `crates/aicomic-ai/{Cargo.toml,src/*,tests/*}`。

1. 写本地 HTTP 测试，覆盖兼容请求、限流/服务错误重试、输出截断、围栏 JSON。
2. 观察失败后实现客户端；再以临时目录测试 Settings/Prompt/Context/Run。
3. 移植实际剧本结构/资产流程、生成优化、审核暂停与恢复；保留原始请求/输出/Stage。
4. 模型调用使用本地测试服务，不触发真实付费请求。验证错误和不完整结果不会写成完成。

### Task 3: 记忆与向量索引

**Files:** `crates/aicomic-memory/{Cargo.toml,src/*,tests/*}`。

1. 为旧 memory SQLite 导入、变更集审批和冲突裁决写真实临时数据库测试。
2. 实现事实存储和审核事务，保留旧 ID、JSON 与原始证据。
3. 测试向量维度/模型身份/重建、缺失 embedding 错误，之后实现 provider 和索引。
4. 不将字面匹配或任意散列向量宣传为 BGE-M3 语义检索。

### Task 4: 桌面完整创作闭环

**Files:** `crates/aicomic-desktop/{Cargo.toml,src/*,tests/*}`。

1. 写编辑会话回归：切集前保留修改、过期 Run 结果不能覆盖当前集、取消不能标成功。
2. 实现深色顶部导航、项目和剧本树、中文编辑、保存/关闭保护、Settings 与异步 AI。
3. 连接原始剧本/结构/资产画布、事件矩阵、媒体、Context、Run 与记忆审核界面。
4. 以临时演示数据启动真实原生窗口，检查中文字体、输入、缩放和错误反馈。

### Task 5: 双平台交付与清理

**Files:** `.github/workflows/rust-desktop.yml`、`packaging/*`、`README.md`、`.gitignore`。

1. 提交根 Cargo.lock，构建矩阵验证 Mac arm64/x64、Windows x64；打包独立 .app 与 Windows 可执行文件/安装器。
2. 运行 `cargo fmt --all --check`、`cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`。
3. 审查功能表逐项证据，媒体与本地 embedding 是独立验收项。
4. 全量验收前保留旧源码与用户数据；只有实际通过的平台才能宣称可交付。

## 实施记录

- 2026-10-21：静态审计完成；建立隔离 worktree `codex/rust-desktop`。尚未宣称全量迁移完成。
- 2026-10-21：四个 Rust crate、原生三步创作/事件/素材/上下文/运行记录/记忆/设置界面落地；旧源码移入 `legacy/` 留作对照，不参与构建。
- 2026-10-21：最终 107 项测试、workspace 格式/严格 Clippy、发布独立性检查通过。Mac arm64 release 经原生交互复验，Intel x64 交叉构建与 Rosetta 启动通过；两个架构均已生成 APP/ZIP/DMG。
- Windows MSVC/NSIS/CI 已配置但尚未执行，完整 BGE-M3 与真实远程模型未做质量等价验收；音视频通过系统应用打开。详细证据和界面差异记录于 `docs/rust-migration.md`，不以本机测试代替双平台全量验收。

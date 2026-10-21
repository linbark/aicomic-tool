# 协作约定

- 执行前核对聊天上下文、已完成操作和可验证证据；用户说法与证据冲突时直接说明依据。
- 区分“未执行”“执行失败”和“已执行但未看到结果”，不凭缺少记录推断操作未执行。
- 先完成已授权且可逆的工作；只在确实缺少信息或授权时提问。

# 项目

星晴是面向 macOS 与 Windows 的 Rust 原生桌面应用，使用 Cargo workspace。

- `crates/aicomic-desktop`：Iced 界面、画布、编辑会话和应用集成。
- `crates/aicomic-core`：SQLite 迁移、业务数据与素材文件。
- `crates/aicomic-ai`：模型客户端、Prompt、工作流及 Run/Stage。
- `crates/aicomic-memory`：证据、实体、变更审核、检索和可选 Candle 推理。
- `packaging/`：Mac APP/DMG 与 Windows EXE/NSIS 打包；图标只使用 ICNS/ICO。
- `docs/`：迁移边界与领域设计资料。设计研究不等于已交付功能。

正式程序由 Rust 直接执行，不引入 Python、Node、WebView 或本地 HTTP 服务。

# 验证

```sh
cargo fmt --all -- --check
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
bash packaging/check-release.sh --offline --self-test
```

按变更范围补充实际验证；不以 Mac 测试代替 Windows 测试，不把 CI 配置存在当作已经运行成功。保持根 `Cargo.lock` 受版本控制。

# 数据保护

用独立临时数据目录测试，避免修改默认用户数据。保留旧 SQLite/Context/Run 的兼容性、项目与分集隔离以及同 Run 审核的幂等性。

`data/`、`projects/`、`qdrant_db/`、数据库、设置密钥、模型权重、`target/` 和 `dist/` 属于本地数据或构建产物，不提交到仓库。清理源码时保留用户素材和实际数据。

密钥不写入日志或测试输出。AI 测试使用本地模拟服务，不发起未授权的付费请求。

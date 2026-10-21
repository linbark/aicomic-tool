# 星晴原生桌面打包

应用由 Cargo 构建，不运行 Python、Node、npm 或 Tauri。图标从旧版资源复制，打包流程不读取旧版源码。

在 Mac 上先执行静态发布检查：

```sh
bash packaging/check-release.sh --offline --self-test
```

检查图标格式、脚本语法、遗留路径/运行时依赖，并将 Cargo、四个 Rust crate 和打包资源复制到没有旧版目录的临时树解析锁定 workspace。自检只向临时副本注入旧构建命令并验证其被拒绝。`--offline` 需要本机已有 Cargo 索引缓存；首次环境可去掉该参数。此检查不编译或启动应用，不替代平台原生测试。

macOS arm64 / x64 分别运行：

```sh
bash packaging/macos/package.sh --target aarch64-apple-darwin
bash packaging/macos/package.sh --target x86_64-apple-darwin
```

默认最低 macOS 13，输出 `dist/macos-arm64/` 或 `dist/macos-x64/`，包含 `星晴.app`、保留应用权限的 `.app.zip`、带 Applications 链接的 `.dmg`。跨架构需先 `rustup target add`，正式交付在对应架构 runner 构建和测试。

Windows x64 在 Windows PowerShell 安装 Rust MSVC 工具链与 NSIS 3 后运行：

```powershell
./packaging/windows/package.ps1
```

输出 `dist/windows-x64/` 下独立 `.exe` 与 NSIS 安装器。安装器安装到当前用户目录，无需管理员权限；卸载只删除应用，不删除项目、数据库、密钥或模型文件。`build.rs` 嵌入 Windows 图标、应用信息和 manifest。

Windows 打包与 CI 都使用 `-C target-feature=+crt-static`，并通过 MSVC `dumpbin /DEPENDENTS` 拒绝依赖未随包分发的 VC、Python、Node 或 WebView2 运行库的程序。`-SkipBuild` / `-Binary` 指定的现成程序同样接受导入检查。依据：[Rust CRT 链接规则](https://doc.rust-lang.org/reference/linkage.html#static-and-dynamic-c-runtimes)、[Cargo 目标专用编译参数](https://doc.rust-lang.org/cargo/reference/config.html#targettriplerustflags)。Windows 编译需要 MSVC Build Tools 与 Windows SDK，打包另需 NSIS；这些属于构建工具，程序运行不需要它们。

CI 配置为 macOS 15 arm64、macOS 15 Intel、Windows 2025 x64 三个原生 runner，各自执行格式检查、完整测试、Clippy、release 构建和打包。产物上传为 Actions artifacts，不创建发布、不推送远程。

签名状态：macOS 仅本地 ad-hoc 签名，无 Developer ID 签名或 Apple 公证；Windows 程序与安装器未签名。正式公网分发仍需持有者提供证书并执行签名/公证。此文件不代表 CI 已实际运行或 Windows 已验收。

官方依据：[GitHub runner 架构](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)、[Apple bundle 内容位置](https://developer.apple.com/documentation/bundleresources/placing-content-in-a-bundle)、[NSIS 命令行](https://nsis.sourceforge.io/Docs/Chapter3.html)、[NSIS 安装器命令](https://nsis.sourceforge.io/Docs/Chapter4.html)、[winresource](https://docs.rs/winresource/latest/winresource/)。DMG 使用系统 `hdiutil` 的 `create -srcfolder -format UDZO` 与 `verify`，语义按 macOS 本机手册核对。

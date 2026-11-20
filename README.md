<p align="center"><img src="assets/icon/png/256.png" width="128" alt="MDEasy"></p>

<h1 align="center">MDEasy</h1>

<p align="center">轻量、高性能的跨平台 Markdown / JSON / YAML / TOML 编辑器，纯 Rust 实现（GPUI，GPU 渲染）。</p>

## 功能

- **三种视图**：编辑 / 分栏（边写边看）/ 预览，`⌘1` `⌘2` `⌘3` 切换
- **Markdown**：GFM 表格、任务列表、代码块高亮，预览在后台线程解析
- **JSON / YAML / TOML**：语法高亮 + 实时校验，预览为可折叠树（虚拟列表，大文件不卡）；语法错误可点击跳转到出错位置；JSON 一键格式化（`⌥⇧F`）
- **侧栏**：打开文件夹或单独加入文件，也可以直接把文件/文件夹拖进窗口；只显示支持的文件类型，自动跳过 `node_modules`、`target`、`.git` 等目录
- **多标签页**、未保存提示、关闭/退出前确认
- 窗口重新获得焦点时检测外部修改：未改动则自动重新加载，有冲突时提醒
- 记住侧栏、打开的标签页、视图模式和亮/暗主题

## 快捷键

macOS 用 `⌘`，Windows / Linux 用 `Ctrl`。

| 操作 | 快捷键 |
|---|---|
| 新建 Markdown | `⌘N` |
| 打开文件 / 文件夹 | `⌘O` / `⌘⇧O` |
| 保存 / 另存为 | `⌘S` / `⌘⇧S` |
| 关闭标签页 | `⌘W` |
| 切换标签页 | `Ctrl+Tab` / `Ctrl+Shift+Tab` |
| 编辑 / 分栏 / 预览 | `⌘1` / `⌘2` / `⌘3` |
| 切换侧栏 | `⌘B` |
| 格式化 JSON | `⌥⇧F` |
| 查找 | `⌘F` |

## 构建

需要 Rust 1.90+。macOS 需安装 Xcode（Metal 工具链）。

```bash
cargo run --release                          # 运行
cargo run --release -- path/to/file.md dir/  # 启动时打开文件或文件夹
cargo test -p mdeasy-core                    # 核心逻辑测试
scripts/bundle-macos.sh                      # 打包 target/release/MDEasy.app，并设为 md/json/yaml/toml 默认打开程序
scripts/bundle-macos.sh --install            # 同上，但安装到 /Applications 并让关联指向那里（推荐）
scripts/bundle-macos.sh --no-default         # 只打包注册，不改默认打开程序
```

Windows 构建时自动把 `assets/icon/mdeasy.ico` 嵌入 exe。

## 下载与自动更新

在 [Releases](https://github.com/ifconfiger/MDEasy/releases) 下载对应平台的安装包：

| 平台 | 文件 |
|---|---|
| macOS（Apple 芯片） | `MDEasy-<版本>-macos-arm64.zip`，解压后把 `MDEasy.app` 拖进“应用程序” |
| Windows x64 | `MDEasy-<版本>-windows-x64.zip`，解压后运行 `mdeasy.exe` |
| Linux x64 | `MDEasy-<版本>-linux-x64.tar.gz`，内含 `mdeasy`、`mdeasy.desktop`、图标 |

macOS 安装包用 Developer ID 签名并经过 Apple 公证，可以直接打开。

应用每天启动时会在后台检查一次新版本（菜单“检查更新…”可手动检查）。发现新版本后可以一键下载，校验 SHA-256 后替换当前程序并重启；从 `cargo run` 或 `target/` 目录运行的开发构建只提示、不自动安装。设置环境变量 `MDEASY_NO_UPDATE_CHECK=1` 可关闭自动检查；网络需要代理时可设置 `HTTPS_PROXY`。

### 发布新版本

1. 修改根目录 `Cargo.toml` 的 `version`（如 `0.2.0`），提交
2. 打标签并推送：`git tag v0.2.0 && git push origin v0.2.0`
3. GitHub Actions（`.github/workflows/release.yml`）会在自带 runner 上构建三个平台的安装包（macOS 版签名 + 公证）、生成 `SHA256SUMS.txt` 并创建 Release，已安装的 MDEasy 随后就能检测到更新

macOS 签名与公证使用组织 `ifconfiger` 的 Secret：`APPLE_SIGN_P12`（Developer ID Application 证书 p12 的 base64）、`APPLE_SIGN_PASSWORD`、`APPLE_NOTARY_KEY`（App Store Connect API Key 的 .p8 内容或 base64）、`APPLE_NOTARY_KEY_ID`、`APPLE_NOTARY_ISSUER`。缺少时自动退回临时签名。

标签必须与 `Cargo.toml` 的版本一致，否则构建会失败。在 Actions 页面手动运行 Release workflow 只构建、不发布，可用来试构建。

## 目录结构

```
crates/
  mdeasy-core/   文件类型、JSON/YAML/TOML 解析、目录扫描、会话持久化（不依赖 UI，有单元测试）
  mdeasy-app/    GPUI 界面：工作区、侧栏、文档、数据树预览
tools/icon-gen/  从 assets/icon/mdeasy.svg 生成 PNG / iconset / ico
scripts/         打包脚本
```

修改图标：编辑 `assets/icon/mdeasy.svg` 后运行

```bash
cargo run -p icon-gen && iconutil -c icns assets/icon/MDEasy.iconset -o assets/icon/MDEasy.icns
```

## 开发：离屏截图

不需要屏幕录制权限，用于检查界面效果：

```bash
MDEASY_SCREENSHOT=/tmp/shots cargo run --features screenshot -- some/folder some/file.md
```

会依次截取分栏、预览、暗色、编辑模式，保存 PNG 后自动退出。

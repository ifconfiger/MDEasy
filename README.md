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
scripts/bundle-macos.sh                      # 打包 target/release/MDEasy.app（含图标与文件关联）
```

Windows 构建时自动把 `assets/icon/mdeasy.ico` 嵌入 exe。

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

// Windows 发布版不弹出控制台窗口
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod actions;
mod data_view;
#[cfg(feature = "screenshot")]
mod dev_screenshot;
mod document;
mod sidebar;
mod workspace;

use std::path::PathBuf;

use gpui_kit::assets::Assets;
use gpui_kit::component::{Theme, ThemeMode, TitleBar, input};
use gpui_kit::*;
use mdeasy_core::Session;

use crate::{actions::*, workspace::Workspace};

fn main() {
    // 命令行参数：要打开的文件或文件夹
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let session = Session::load();

    // macOS：从 Finder 双击 / “打开方式”打开文件时，系统通过 open-urls 事件传入路径
    let (url_tx, mut url_rx) = futures_channel::mpsc::unbounded::<Vec<PathBuf>>();
    let app = gpui_kit::application().with_assets(Assets);
    app.on_open_urls(move |urls| {
        let paths: Vec<PathBuf> = urls.iter().filter_map(|u| file_url_to_path(u)).collect();
        if !paths.is_empty() {
            let _ = url_tx.unbounded_send(paths);
        }
    });

    app.run(move |cx| {
        gpui_kit::init(cx);
        Theme::change(
            if session.dark { ThemeMode::Dark } else { ThemeMode::Light },
            None,
            cx,
        );
        actions::bind_keys(cx);
        cx.set_menus(app_menus());
        cx.activate(true);

        // 没有处理器时的兜底（例如窗口已关闭）
        cx.on_action(|_: &Quit, cx: &mut App| cx.quit());
        // 关闭最后一个窗口即退出
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let bounds = Bounds::centered(None, size(px(1280.), px(820.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(640.), px(420.))),
            #[cfg(target_os = "linux")]
            window_decorations: Some(WindowDecorations::Client),
            ..TitleBar::window_options()
        };

        let session = session.clone();
        let args = args.clone();
        let (_window_handle, _workspace) = gpui_kit::open_window(options, cx, move |window, cx| {
            window.set_window_title("MDEasy");
            let workspace = cx.new(|cx| Workspace::new(session.clone(), window, cx));
            workspace.update(cx, |ws, cx| ws.restore_tabs(&session, window, cx));
            if !args.is_empty() {
                workspace.update(cx, |ws, cx| {
                    for path in args {
                        ws.open_path(path, window, cx);
                    }
                });
            }
            workspace
        })
        .expect("无法打开窗口");
        {
            let workspace = _workspace.downgrade();
            let window = _window_handle;
            cx.spawn(async move |cx| {
                use futures_util::StreamExt as _;
                while let Some(paths) = url_rx.next().await {
                    _ = window.update(cx, |_, window, cx| {
                        window.activate_window();
                        _ = workspace.update(cx, |ws, cx| {
                            for path in paths {
                                ws.open_path(path, window, cx);
                            }
                        });
                    });
                }
            })
            .detach();
        }
        #[cfg(feature = "screenshot")]
        dev_screenshot::run(_workspace, _window_handle, cx);
    });
}

/// `file:///a/b%20c.md` → `/a/b c.md`
fn file_url_to_path(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(b) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?, 16)
        {
            out.push(b);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Some(PathBuf::from(String::from_utf8(out).ok()?))
}

fn app_menus() -> Vec<Menu> {
    vec![
        Menu {
            name: "MDEasy".into(),
            items: vec![
                MenuItem::action("切换亮色 / 暗色", ToggleTheme),
                MenuItem::separator(),
                MenuItem::action("退出 MDEasy", Quit),
            ],
            disabled: false,
        },
        Menu {
            name: "文件".into(),
            items: vec![
                MenuItem::action("新建 Markdown", NewMarkdown),
                MenuItem::action("新建 JSON", NewJson),
                MenuItem::action("新建 YAML", NewYaml),
                MenuItem::action("新建 TOML", NewToml),
                MenuItem::separator(),
                MenuItem::action("打开文件…", OpenFile),
                MenuItem::action("打开文件夹…", OpenFolder),
                MenuItem::separator(),
                MenuItem::action("保存", Save),
                MenuItem::action("另存为…", SaveAs),
                MenuItem::separator(),
                MenuItem::action("关闭标签页", CloseTab),
            ],
            disabled: false,
        },
        Menu {
            name: "编辑".into(),
            items: vec![
                MenuItem::action("撤销", input::Undo),
                MenuItem::action("重做", input::Redo),
                MenuItem::separator(),
                MenuItem::action("剪切", input::Cut),
                MenuItem::action("复制", input::Copy),
                MenuItem::action("粘贴", input::Paste),
                MenuItem::action("全选", input::SelectAll),
                MenuItem::separator(),
                MenuItem::action("查找", input::Search),
                MenuItem::action("格式化 JSON", FormatDocument),
            ],
            disabled: false,
        },
        Menu {
            name: "视图".into(),
            items: vec![
                MenuItem::action("编辑模式", EditMode),
                MenuItem::action("分栏模式", SplitMode),
                MenuItem::action("预览模式", PreviewMode),
                MenuItem::separator(),
                MenuItem::action("切换侧栏", ToggleSidebar),
                MenuItem::action("下一个标签页", NextTab),
                MenuItem::action("上一个标签页", PrevTab),
            ],
            disabled: false,
        },
    ]
}

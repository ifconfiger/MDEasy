//! 开发用离屏截图（`--features screenshot`）。
//!
//! 设置 `MDEASY_SCREENSHOT=<目录>` 启动后，依次切换几个界面状态，
//! 用 `Window::render_to_image` 离屏渲染保存 PNG，然后退出。不需要屏幕录制权限。

use std::{path::PathBuf, time::Duration};

use gpui_kit::*;

use crate::{actions::*, workspace::Workspace};

pub fn run(workspace: Entity<Workspace>, window: AnyWindowHandle, cx: &mut App) {
    let Some(dir) = std::env::var_os("MDEASY_SCREENSHOT").map(PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&dir).ok();
    let extra: Vec<PathBuf> = std::env::var("MDEASY_SCREENSHOT_OPEN")
        .ok()
        .map(|s| s.split(':').map(PathBuf::from).collect())
        .unwrap_or_default();

    cx.spawn(async move |cx| {
        let executor = cx.background_executor().clone();
        let wait = move |ms| executor.timer(Duration::from_millis(ms));
        let shot = |name: &str, cx: &mut AsyncApp| {
            let path = dir.join(format!("{name}.png"));
            let result = window.update(cx, |_, window, cx| {
                // 强制重新绘制一帧，避免拿到旧画面
                window.refresh();
                window.draw(cx).clear(cx);
                window.render_to_image()
            });
            match result {
                Ok(Ok(img)) => match img.save(&path) {
                    Ok(()) => eprintln!("screenshot: {}", path.display()),
                    Err(e) => eprintln!("screenshot {name} 保存失败: {e}"),
                },
                other => eprintln!("screenshot {name} 失败: {:?}", other.err()),
            }
        };
        let act = |action: Box<dyn Action>, cx: &mut AsyncApp| {
            _ = window.update(cx, |_, window, cx| window.dispatch_action(action, cx));
        };

        wait(1500).await;
        act(Box::new(SplitMode), cx);
        wait(800).await;
        shot("1-split", cx);

        act(Box::new(PreviewMode), cx);
        wait(600).await;
        shot("2-preview", cx);

        for (i, path) in extra.into_iter().enumerate() {
            _ = window.update(cx, |_, window, cx| {
                workspace.update(cx, |ws, cx| ws.open_path(path.clone(), window, cx))
            });
            act(Box::new(SplitMode), cx);
            wait(800).await;
            shot(&format!("{}-open-{i}", 3 + i), cx);
        }

        act(Box::new(ToggleTheme), cx);
        wait(600).await;
        shot("8-dark", cx);
        act(Box::new(ToggleTheme), cx);

        act(Box::new(EditMode), cx);
        wait(400).await;
        shot("9-edit", cx);

        act(Box::new(SplitMode), cx);
        cx.update(|cx| cx.quit());
    })
    .detach();
}

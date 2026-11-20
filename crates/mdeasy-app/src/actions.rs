use gpui_kit::*;

actions!(
    mdeasy,
    [
        NewMarkdown,
        NewJson,
        NewYaml,
        NewToml,
        OpenFile,
        OpenFolder,
        Save,
        SaveAs,
        CloseTab,
        NextTab,
        PrevTab,
        EditMode,
        SplitMode,
        PreviewMode,
        ToggleSidebar,
        ToggleTheme,
        FormatDocument,
        About,
        Quit,
    ]
);

/// macOS 上用 Cmd，Windows / Linux 上用 Ctrl。
/// （`KeyBinding::new` 只接受静态字符串，不解析 `secondary-`，所以在编译期拼接。）
macro_rules! sec {
    ($key:literal) => {
        if cfg!(target_os = "macos") {
            concat!("cmd-", $key)
        } else {
            concat!("ctrl-", $key)
        }
    };
}

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new(sec!("n"), NewMarkdown, None),
        KeyBinding::new(sec!("o"), OpenFile, None),
        KeyBinding::new(sec!("shift-o"), OpenFolder, None),
        KeyBinding::new(sec!("s"), Save, None),
        KeyBinding::new(sec!("shift-s"), SaveAs, None),
        KeyBinding::new(sec!("w"), CloseTab, None),
        KeyBinding::new("ctrl-tab", NextTab, None),
        KeyBinding::new("ctrl-shift-tab", PrevTab, None),
        KeyBinding::new(sec!("1"), EditMode, None),
        KeyBinding::new(sec!("2"), SplitMode, None),
        KeyBinding::new(sec!("3"), PreviewMode, None),
        KeyBinding::new(sec!("b"), ToggleSidebar, None),
        KeyBinding::new("alt-shift-f", FormatDocument, None),
        KeyBinding::new(sec!("q"), Quit, None),
    ]);
}

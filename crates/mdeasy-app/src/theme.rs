//! MDEasy 自己的亮色 / 暗色主题，以及界面里用到的彩色强调（文件类型图标等）。

use gpui_kit::component::{IconName, Theme, ThemeMode, ThemeRegistry};
use gpui_kit::*;
use mdeasy_core::FileKind;

const THEME_JSON: &str = include_str!("theme.json");

/// 注册并启用 MDEasy 主题；之后 `Theme::change` 切换亮暗时都会用这套配色。
pub fn install(mode: ThemeMode, cx: &mut App) {
    // 先让组件库建好全局主题，再替换成我们的配置
    Theme::change(mode, None, cx);
    let registry = ThemeRegistry::global_mut(cx);
    if let Err(err) = registry.load_themes_from_str(THEME_JSON) {
        eprintln!("加载内置主题失败：{err}");
        return;
    }
    let themes = registry.themes();
    let (Some(light), Some(dark)) =
        (themes.get("MDEasy Light").cloned(), themes.get("MDEasy Dark").cloned())
    else {
        return;
    };
    Theme::update(cx, |theme| {
        theme.light_theme = light;
        theme.dark_theme = dark;
    });
    Theme::change(mode, None, cx);
}

/// 文件类型对应的图标与颜色。
pub fn file_icon(kind: Option<FileKind>, dark: bool) -> (IconName, Hsla) {
    let c = |light: u32, dark_hex: u32| rgb(if dark { dark_hex } else { light }).into();
    match kind {
        Some(FileKind::Markdown) => (IconName::BookOpen, c(0x3b74f5, 0x6d99ff)),
        Some(FileKind::Json) => (IconName::FileText, c(0xd97706, 0xfbbf24)),
        Some(FileKind::Yaml) => (IconName::FileText, c(0xdb2777, 0xf472b6)),
        Some(FileKind::Toml) => (IconName::Settings2, c(0xea580c, 0xfb923c)),
        _ => (IconName::File, c(0x8a8a86, 0x8d8d94)),
    }
}

/// 文件夹图标颜色。
pub fn folder_color(dark: bool) -> Hsla {
    rgb(if dark { 0x7ba3ff } else { 0x5b8af7 }).into()
}

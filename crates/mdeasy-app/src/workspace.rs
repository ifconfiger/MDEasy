//! 主窗口：标题栏、侧栏、标签页、编辑/分栏/预览区域、状态栏。

use std::{collections::HashMap, path::PathBuf};

use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Selectable as _, Sizable as _, Theme, ThemeMode, TitleBar,
    WindowExt as _,
    button::{Button, ButtonGroup, ButtonVariants as _},
    h_flex,
    input::Editor,
    menu::DropdownMenu as _,
    notification::Notification,
    resizable::{h_resizable, resizable_panel},
    text::TextView,
    v_flex,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use mdeasy_core::{
    FileKind, Session,
    data::{JsonFormat, JsonIndent},
};

use crate::{
    actions::*,
    document::{DiskChange, Document, DocumentEvent},
    sidebar::{Sidebar, SidebarEvent},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Edit,
    Split,
    Preview,
}

impl ViewMode {
    fn from_str(s: &str) -> Self {
        match s {
            "edit" => Self::Edit,
            "preview" => Self::Preview,
            _ => Self::Split,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Edit => "edit",
            Self::Split => "split",
            Self::Preview => "preview",
        }
    }
}

pub struct Workspace {
    focus_handle: FocusHandle,
    sidebar: Entity<Sidebar>,
    docs: Vec<Entity<Document>>,
    active: usize,
    mode: ViewMode,
    sidebar_visible: bool,
    untitled_counter: usize,
    /// 退出流程进行中，避免重复弹窗
    quitting: bool,
    doc_subscriptions: HashMap<EntityId, Subscription>,
    _subscriptions: Vec<Subscription>,
}

impl Workspace {
    pub fn new(session: Session, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let sidebar = cx.new(|cx| Sidebar::new(session.roots.clone(), cx));
        let mut subscriptions = vec![cx.subscribe_in(
            &sidebar,
            window,
            |this, _, event: &SidebarEvent, window, cx| match event {
                SidebarEvent::Open(path) => this.open_path(path.clone(), window, cx),
                SidebarEvent::RootsChanged => this.save_session(cx),
            },
        )];
        // 窗口重新获得焦点时检查外部修改
        subscriptions.push(cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                this.check_external_changes(window, cx);
            }
        }));

        let focus_handle = cx.focus_handle();
        let handle = focus_handle.clone();
        window.defer(cx, move |window, cx| handle.focus(window, cx));

        // 窗口关闭前检查未保存的文件
        let weak = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            weak.update(cx, |this, cx| this.request_quit(window, cx))
                .unwrap_or(true)
        });

        Self {
            focus_handle,
            sidebar,
            docs: Vec::new(),
            active: 0,
            mode: ViewMode::from_str(&session.view_mode),
            sidebar_visible: session.sidebar_visible.unwrap_or(true),
            untitled_counter: 0,
            quitting: false,
            doc_subscriptions: HashMap::new(),
            _subscriptions: subscriptions,
        }
    }

    /// 恢复上次打开的标签页。
    pub fn restore_tabs(&mut self, session: &Session, window: &mut Window, cx: &mut Context<Self>) {
        for path in session.open_files.iter().filter(|p| p.is_file()) {
            self.open_path(path.clone(), window, cx);
        }
        if let Some(active) = &session.active_file
            && let Some(ix) = self
                .docs
                .iter()
                .position(|d| d.read(cx).path.as_ref() == Some(active))
        {
            self.activate(ix, window, cx);
        }
    }

    fn check_external_changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for doc in self.docs.clone() {
            let change = doc.update(cx, |d, cx| d.check_disk(window, cx));
            let title = doc.read(cx).title();
            let note = match change {
                DiskChange::None | DiskChange::Reloaded => continue,
                DiskChange::Conflict => Notification::warning(format!(
                    "“{title}”在其他程序中被修改，但这里也有未保存的修改。保存会覆盖磁盘上的版本。"
                )),
                DiskChange::Deleted => {
                    Notification::warning(format!("“{title}”已在磁盘上被删除或移动。"))
                }
            };
            window.push_notification(note, cx);
        }
    }

    fn active_doc(&self) -> Option<&Entity<Document>> {
        self.docs.get(self.active)
    }

    // ---------- 打开 / 新建 ----------

    pub fn open_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if path.is_dir() {
            self.sidebar.update(cx, |s, cx| s.add_paths([path], cx));
            return;
        }
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        if let Some(ix) = self
            .docs
            .iter()
            .position(|d| d.read(cx).path.as_deref() == Some(path.as_path()))
        {
            self.activate(ix, window, cx);
            return;
        }
        match Document::load(&path) {
            Ok((kind, content)) => {
                let doc = cx.new(|cx| Document::opened(path, kind, content, window, cx));
                self.add_doc(doc, window, cx)
            }
            Err(err) => window.push_notification(
                Notification::error(format!("{}：{err}", path.display())).title("无法打开文件"),
                cx,
            ),
        }
    }

    fn add_doc(&mut self, doc: Entity<Document>, window: &mut Window, cx: &mut Context<Self>) {
        let sub = cx.subscribe(&doc, |_, _, _: &DocumentEvent, cx| cx.notify());
        self.doc_subscriptions.insert(doc.entity_id(), sub);
        // 新标签插在当前标签右侧
        let ix = if self.docs.is_empty() { 0 } else { self.active + 1 };
        self.docs.insert(ix, doc);
        self.activate(ix, window, cx);
    }

    fn new_document(&mut self, kind: FileKind, window: &mut Window, cx: &mut Context<Self>) {
        self.untitled_counter += 1;
        let index = self.untitled_counter;
        let doc = cx.new(|cx| Document::untitled(kind, index, window, cx));
        self.add_doc(doc, window, cx);
        // 新建文档默认进入编辑模式，方便直接输入
        if self.mode == ViewMode::Preview {
            self.set_mode(ViewMode::Split, window, cx);
        }
    }

    fn activate(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.docs.len() {
            return;
        }
        self.active = ix;
        let doc = self.docs[ix].read(cx);
        window.set_window_title(&format!("{} — MDEasy", doc.title()));
        if let Some(path) = doc.path.clone() {
            self.sidebar.update(cx, |s, cx| s.reveal(&path, cx));
        }
        self.focus_active(window, cx);
        self.save_session(cx);
        cx.notify();
    }

    fn focus_active(&self, window: &mut Window, cx: &mut App) {
        match self.active_doc() {
            Some(doc) if self.mode != ViewMode::Preview || !doc.read(cx).kind.has_preview() => {
                let handle = doc.read(cx).editor.focus_handle(cx);
                handle.focus(window, cx)
            }
            _ => self.focus_handle.focus(window, cx),
        }
    }

    fn prompt_open(&mut self, directories: bool, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: !directories,
            directories,
            multiple: true,
            prompt: Some(if directories { "打开文件夹" } else { "打开" }.into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else { return };
            _ = this.update_in(cx, |this, window, cx| {
                this.sidebar.update(cx, |s, cx| s.add_paths(paths.clone(), cx));
                if !directories {
                    for path in paths {
                        this.open_path(path, window, cx);
                    }
                }
            });
        })
        .detach();
    }

    fn on_drop(&mut self, paths: &ExternalPaths, window: &mut Window, cx: &mut Context<Self>) {
        let paths: Vec<PathBuf> = paths.paths().to_vec();
        self.sidebar.update(cx, |s, cx| s.add_paths(paths.clone(), cx));
        if !self.sidebar_visible {
            self.toggle_sidebar(cx);
        }
        for path in paths.into_iter().filter(|p| p.is_file()) {
            if FileKind::from_path(&path).is_some() {
                self.open_path(path, window, cx);
            }
        }
    }

    // ---------- 保存 / 关闭 ----------

    fn save_active(&mut self, save_as: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc) = self.active_doc().cloned() else { return };
        self.save_doc(doc, save_as, window, cx).detach();
    }

    /// 保存文档；未命名或另存为时弹出保存对话框。返回的任务结果表示是否保存成功。
    fn save_doc(
        &mut self,
        doc: Entity<Document>,
        save_as: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<bool> {
        let (path, title) = {
            let d = doc.read(cx);
            (d.path.clone(), d.title())
        };
        if let (Some(_), false) = (&path, save_as) {
            let result = doc.update(cx, |d, cx| d.save_to(None, window, cx));
            return Task::ready(self.report_save(result, &title, window, cx));
        }

        let dir = path
            .as_ref()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .or_else(|| self.sidebar.read(cx).roots().iter().find(|p| p.is_dir()).cloned())
            .or_else(dirs_home)
            .unwrap_or_else(|| PathBuf::from("."));
        let new_path = cx.prompt_for_new_path(&dir, Some(&title));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(new_path))) = new_path.await else { return false };
            this.update_in(cx, |this, window, cx| {
                let result = doc.update(cx, |d, cx| d.save_to(Some(new_path.clone()), window, cx));
                let ok = this.report_save(result, &title, window, cx);
                if ok {
                    this.sidebar.update(cx, |s, cx| s.rescan(cx));
                    if let Some(ix) = this.docs.iter().position(|d| d == &doc) {
                        this.activate(ix, window, cx);
                    }
                }
                ok
            })
            .unwrap_or(false)
        })
    }

    fn report_save(
        &self,
        result: anyhow::Result<()>,
        title: &str,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        match result {
            Ok(()) => true,
            Err(err) => {
                window.push_notification(
                    Notification::error(format!("{title}：{err}")).title("保存失败"),
                    cx,
                );
                false
            }
        }
    }

    fn close_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc) = self.docs.get(ix).cloned() else { return };
        if !doc.read(cx).is_dirty() {
            self.remove_tab(ix, window, cx);
            return;
        }
        let title = doc.read(cx).title();
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("要保存对“{title}”的修改吗？"),
            Some("不保存的话，修改会丢失。"),
            &["保存", "不保存", "取消"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let Ok(answer) = answer.await else { return };
            let save = match answer {
                0 => this
                    .update_in(cx, |this, window, cx| this.save_doc(doc.clone(), false, window, cx))
                    .ok(),
                1 => None,
                _ => return,
            };
            if let Some(save) = save
                && !save.await
            {
                return;
            }
            _ = this.update_in(cx, |this, window, cx| {
                if let Some(ix) = this.docs.iter().position(|d| d == &doc) {
                    this.remove_tab(ix, window, cx);
                }
            });
        })
        .detach();
    }

    fn remove_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let doc = self.docs.remove(ix);
        self.doc_subscriptions.remove(&doc.entity_id());
        if self.docs.is_empty() {
            self.active = 0;
            window.set_window_title("MDEasy");
            self.focus_handle.focus(window, cx);
            self.save_session(cx);
            cx.notify();
        } else {
            let next = if ix <= self.active { self.active.saturating_sub(1) } else { self.active };
            self.activate(next.min(self.docs.len() - 1), window, cx);
        }
    }

    /// 退出/关窗前调用。没有未保存文件时返回 `true` 允许立即关闭；
    /// 否则弹窗询问，并返回 `false`，由弹窗结果决定是否退出。
    fn request_quit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let dirty: Vec<_> = self.docs.iter().filter(|d| d.read(cx).is_dirty()).cloned().collect();
        if dirty.is_empty() {
            return true;
        }
        if self.quitting {
            return false;
        }
        self.quitting = true;
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("有 {} 个文件尚未保存", dirty.len()),
            Some("退出前要保存这些修改吗？"),
            &["全部保存", "不保存", "取消"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let answer = answer.await.unwrap_or(2);
            let mut proceed = answer == 1;
            if answer == 0 {
                proceed = true;
                for doc in dirty {
                    let Ok(save) =
                        this.update_in(cx, |this, window, cx| this.save_doc(doc, false, window, cx))
                    else {
                        return;
                    };
                    if !save.await {
                        proceed = false;
                        break;
                    }
                }
            }
            _ = this.update(cx, |this, cx| {
                this.quitting = false;
                if proceed {
                    // 已处理完未保存内容，清掉文档后退出，避免再次触发询问
                    this.save_session(cx);
                    this.docs.clear();
                    cx.quit();
                }
            });
        })
        .detach();
        false
    }

    // ---------- 视图 ----------

    fn set_mode(&mut self, mode: ViewMode, window: &mut Window, cx: &mut Context<Self>) {
        self.mode = mode;
        self.focus_active(window, cx);
        self.save_session(cx);
        cx.notify();
    }

    fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_visible = !self.sidebar_visible;
        self.save_session(cx);
        cx.notify();
    }

    fn toggle_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mode = if cx.theme().mode.is_dark() { ThemeMode::Light } else { ThemeMode::Dark };
        Theme::change(mode, Some(window), cx);
        self.save_session(cx);
        cx.refresh_windows();
    }

    fn save_session(&self, cx: &App) {
        let session = Session {
            roots: self.sidebar.read(cx).roots().to_vec(),
            dark: cx.theme().mode.is_dark(),
            view_mode: self.mode.as_str().into(),
            sidebar_visible: Some(self.sidebar_visible),
            open_files: self.docs.iter().filter_map(|d| d.read(cx).path.clone()).collect(),
            active_file: self.active_doc().and_then(|d| d.read(cx).path.clone()),
        };
        if let Err(err) = session.save() {
            eprintln!("保存会话失败：{err}");
        }
    }

    fn format_active(&mut self, opts: JsonFormat, window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc) = self.active_doc().cloned() else { return };
        if let Err(err) = doc.update(cx, |d, cx| d.format(opts, window, cx)) {
            window.push_notification(Notification::warning(err.to_string()).title("无法格式化"), cx);
        }
    }

    // ---------- 渲染 ----------

    fn render_title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let doc = self.active_doc().map(|d| d.read(cx));
        let has_preview = doc.is_some_and(|d| d.kind.has_preview());
        let is_json = doc.is_some_and(|d| d.kind == FileKind::Json);
        let title = doc.map(|d| d.title()).unwrap_or_else(|| "MDEasy".into());
        let mode = self.mode;

        TitleBar::new().child(
            h_flex()
                .w_full()
                .pr_2()
                .gap_2()
                .justify_between()
                .child(
                    h_flex()
                        .gap_1()
                        .when(cfg!(not(target_os = "macos")), |this| {
                            this.child(app_menu_button())
                        })
                        .child(
                            Button::new("toggle-sidebar")
                                .ghost()
                                .small()
                                .icon(if self.sidebar_visible {
                                    IconName::PanelLeftClose
                                } else {
                                    IconName::PanelLeftOpen
                                })
                                .tooltip("切换侧栏")
                                .on_click(cx.listener(|this, _, _, cx| this.toggle_sidebar(cx))),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_center()
                        .text_size(px(13.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.foreground.opacity(0.75))
                        .child(title),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .when(is_json, |this| this.child(format_json_button()))
                        .when(has_preview, |this| {
                            this.child(
                                ButtonGroup::new("view-mode")
                                    .outline()
                                    .compact()
                                    .small()
                                    .child(
                                        Button::new("mode-edit")
                                            .label("编辑")
                                            .selected(mode == ViewMode::Edit),
                                    )
                                    .child(
                                        Button::new("mode-split")
                                            .label("分栏")
                                            .selected(mode == ViewMode::Split),
                                    )
                                    .child(
                                        Button::new("mode-preview")
                                            .label("预览")
                                            .selected(mode == ViewMode::Preview),
                                    )
                                    .on_click(cx.listener(|this, selected: &Vec<usize>, window, cx| {
                                        let mode = match selected.first() {
                                            Some(0) => ViewMode::Edit,
                                            Some(2) => ViewMode::Preview,
                                            _ => ViewMode::Split,
                                        };
                                        this.set_mode(mode, window, cx);
                                    })),
                            )
                        })
                        .child(
                            Button::new("toggle-theme")
                                .ghost()
                                .small()
                                .icon(if theme.mode.is_dark() { IconName::Sun } else { IconName::Moon })
                                .tooltip("切换亮色 / 暗色")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.toggle_theme(window, cx)
                                })),
                        ),
                ),
        )
    }

    fn render_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        h_flex()
            .id("tabs")
            .h(px(36.))
            .flex_shrink_0()
            .bg(theme.background)
            .border_b_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .id("tabs-scroll")
                    .flex_1()
                    .h_full()
                    .overflow_x_scroll()
                    .children(self.docs.iter().enumerate().map(|(ix, doc)| {
                        let d = doc.read(cx);
                        let active = ix == self.active;
                        let dirty = d.is_dirty();
                        let group: SharedString = format!("tab-{ix}").into();
                        h_flex()
                            .id(("tab", ix))
                            .group(group.clone())
                            .h_full()
                            .flex_shrink_0()
                            .pl_3()
                            .pr_1p5()
                            .gap_1p5()
                            .text_size(px(13.))
                            .relative()
                            .cursor_pointer()
                            .map(|this| {
                                if active {
                                    this.text_color(theme.foreground)
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(
                                            div()
                                                .absolute()
                                                .bottom_0()
                                                .left_2()
                                                .right_2()
                                                .h(px(2.))
                                                .rounded_t(px(2.))
                                                .bg(theme.primary),
                                        )
                                } else {
                                    this.text_color(theme.muted_foreground)
                                        .hover(|s| s.text_color(theme.foreground).bg(theme.muted))
                                }
                            })
                            .child({
                                let (icon, color) =
                                    crate::theme::file_icon(Some(d.kind), theme.mode.is_dark());
                                Icon::new(icon).size(px(14.)).text_color(color)
                            })
                            .child(div().max_w(px(220.)).truncate().child(d.title()))
                            .child(
                                div()
                                    .size(px(20.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        // 未保存时显示圆点，悬停时变成关闭按钮
                                        div()
                                            .when(dirty, |this| {
                                                this.size(px(8.))
                                                    .rounded_full()
                                                    .bg(theme.foreground.opacity(0.6))
                                                    .group_hover(group.clone(), |s| s.invisible())
                                            }),
                                    )
                                    .child(
                                        div()
                                            .id(("tab-close", ix))
                                            .absolute()
                                            .size(px(20.))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded(theme.radius)
                                            .invisible()
                                            .when(active && !dirty, |s| s.visible())
                                            .group_hover(group.clone(), |s| s.visible())
                                            .hover(|s| s.bg(theme.secondary_hover))
                                            .child(Icon::new(IconName::Close).xsmall())
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                cx.stop_propagation();
                                                this.close_tab(ix, window, cx);
                                            })),
                                    ),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.activate(ix, window, cx)
                            }))
                    })),
            )
            .child(div().px_1().child(new_file_button("tabs-new")))
    }

    fn render_editor(&self, doc: &Entity<Document>, cx: &App) -> AnyElement {
        let theme = cx.theme();
        div()
            .size_full()
            .pt_2()
            .bg(theme.background)
            .child(
                Editor::new(&doc.read(cx).editor)
                    .bordered(false)
                    .p_0()
                    .h_full()
                    .font_family(theme.mono_font_family.clone())
                    .text_size(theme.mono_font_size),
            )
            .into_any_element()
    }

    fn render_preview(&self, doc: &Entity<Document>, cx: &App) -> AnyElement {
        let d = doc.read(cx);
        let theme = cx.theme();
        if let Some(md) = &d.markdown {
            h_flex()
                .size_full()
                .justify_center()
                .bg(theme.background)
                .child(
                    TextView::new(md)
                        .h_full()
                        .w_full()
                        .max_w(px(900.))
                        .px_8()
                        .py_6(),
                )
                .into_any_element()
        } else if let Some(data) = &d.data {
            data.clone().into_any_element()
        } else {
            self.render_editor(doc, cx)
        }
    }

    fn render_content(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(doc) = self.active_doc() else {
            return self.render_welcome(cx).into_any_element();
        };
        let has_preview = doc.read(cx).kind.has_preview();
        let area = match (self.mode, has_preview) {
            (_, false) | (ViewMode::Edit, _) => self.render_editor(doc, cx),
            (ViewMode::Preview, _) => self.render_preview(doc, cx),
            (ViewMode::Split, _) => h_resizable("split")
                .child(resizable_panel().child(self.render_editor(doc, cx)))
                .child(resizable_panel().child(self.render_preview(doc, cx)))
                .into_any_element(),
        };
        v_flex()
            .size_full()
            .child(self.render_tabs(cx))
            .child(div().flex_1().min_h_0().child(area))
            .into_any_element()
    }

    fn render_welcome(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let key = if cfg!(target_os = "macos") { "⌘" } else { "Ctrl+" };
        let shortcut = |k: &str, desc: &str| {
            h_flex()
                .w(px(280.))
                .justify_between()
                .child(div().text_color(theme.muted_foreground).child(desc.to_string()))
                .child(
                    div()
                        .px_1p5()
                        .rounded(theme.radius)
                        .bg(theme.secondary)
                        .font_family(theme.mono_font_family.clone())
                        .text_xs()
                        .child(format!("{key}{k}")),
                )
        };
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_6()
            .bg(theme.background)
            .child(
                v_flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .size(px(64.))
                            .rounded(px(16.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(theme.primary)
                            .shadow_md()
                            .child(
                                Icon::new(IconName::BookOpen)
                                    .size(px(32.))
                                    .text_color(theme.primary_foreground),
                            ),
                    )
                    .child(div().text_3xl().font_weight(FontWeight::BOLD).child("MDEasy"))
                    .child(
                        div()
                            .text_color(theme.muted_foreground)
                            .child("轻量的 Markdown / JSON / YAML / TOML 编辑器"),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("welcome-new")
                            .primary()
                            .label("新建 Markdown")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.new_document(FileKind::Markdown, window, cx)
                            })),
                    )
                    .child(
                        Button::new("welcome-open")
                            .outline()
                            .label("打开文件")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.prompt_open(false, window, cx)
                            })),
                    )
                    .child(
                        Button::new("welcome-folder")
                            .outline()
                            .label("打开文件夹")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.prompt_open(true, window, cx)
                            })),
                    ),
            )
            .child(
                v_flex()
                    .gap_2()
                    .text_sm()
                    .child(shortcut("N", "新建"))
                    .child(shortcut("O", "打开文件"))
                    .child(shortcut("S", "保存"))
                    .child(shortcut("1 / 2 / 3", "编辑 / 分栏 / 预览"))
                    .child(shortcut("B", "切换侧栏")),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("也可以把文件或文件夹直接拖进窗口"),
            )
    }

    fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let doc = self.active_doc().map(|d| d.read(cx));
        h_flex()
            .h(px(24.))
            .flex_shrink_0()
            .px_3()
            .gap_4()
            .justify_between()
            .text_size(px(11.5))
            .text_color(theme.muted_foreground)
            .bg(theme.title_bar)
            .border_t_1()
            .border_color(theme.border)
            .child(
                div().min_w_0().truncate().child(
                    doc.map(|d| match &d.path {
                        Some(p) => p.display().to_string(),
                        None => "未保存到磁盘".into(),
                    })
                    .unwrap_or_default(),
                ),
            )
            .when_some(doc, |this, d| {
                let error = d.data.as_ref().and_then(|data| data.read(cx).error().cloned());
                let (lines, chars) = d.stats();
                this.child(
                    h_flex()
                        .flex_shrink_0()
                        .gap_4()
                        .when_some(error, |this, err| {
                            this.child(div().text_color(theme.danger).child(match err.line {
                                Some(l) => format!("语法错误 · 第 {l} 行"),
                                None => "语法错误".into(),
                            }))
                        })
                        .child(format!("{lines} 行 · {chars} 字符"))
                        .child(d.kind.label())
                        .child("UTF-8")
                        .child(if d.is_dirty() { "未保存" } else { "已保存" }),
                )
            })
    }
}

impl Focusable for Workspace {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Workspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = self.render_content(cx);
        let theme = cx.theme().clone();
        let body = if self.sidebar_visible {
            h_resizable("workspace")
                .child(
                    resizable_panel()
                        .size(px(250.))
                        .size_range(px(180.)..px(480.))
                        .child(
                            div()
                                .size_full()
                                .border_r_1()
                                .border_color(theme.sidebar_border)
                                .child(self.sidebar.clone()),
                        ),
                )
                .child(resizable_panel().child(content))
                .into_any_element()
        } else {
            content
        };

        v_flex()
            .id("workspace")
            .key_context("Workspace")
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .on_action(cx.listener(|this, _: &NewMarkdown, w, cx| this.new_document(FileKind::Markdown, w, cx)))
            .on_action(cx.listener(|this, _: &NewJson, w, cx| this.new_document(FileKind::Json, w, cx)))
            .on_action(cx.listener(|this, _: &NewYaml, w, cx| this.new_document(FileKind::Yaml, w, cx)))
            .on_action(cx.listener(|this, _: &NewToml, w, cx| this.new_document(FileKind::Toml, w, cx)))
            .on_action(cx.listener(|this, _: &OpenFile, w, cx| this.prompt_open(false, w, cx)))
            .on_action(cx.listener(|this, _: &OpenFolder, w, cx| this.prompt_open(true, w, cx)))
            .on_action(cx.listener(|this, _: &Save, w, cx| this.save_active(false, w, cx)))
            .on_action(cx.listener(|this, _: &SaveAs, w, cx| this.save_active(true, w, cx)))
            .on_action(cx.listener(|this, _: &CloseTab, w, cx| {
                // 没有标签页时，⌘W 直接退出程序
                if this.docs.is_empty() {
                    if this.request_quit(w, cx) {
                        cx.quit();
                    }
                    return;
                }
                let ix = this.active;
                this.close_tab(ix, w, cx)
            }))
            .on_action(cx.listener(|this, _: &NextTab, w, cx| {
                if !this.docs.is_empty() {
                    let ix = (this.active + 1) % this.docs.len();
                    this.activate(ix, w, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &PrevTab, w, cx| {
                if !this.docs.is_empty() {
                    let ix = (this.active + this.docs.len() - 1) % this.docs.len();
                    this.activate(ix, w, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &EditMode, w, cx| this.set_mode(ViewMode::Edit, w, cx)))
            .on_action(cx.listener(|this, _: &SplitMode, w, cx| this.set_mode(ViewMode::Split, w, cx)))
            .on_action(cx.listener(|this, _: &PreviewMode, w, cx| this.set_mode(ViewMode::Preview, w, cx)))
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| this.toggle_sidebar(cx)))
            .on_action(cx.listener(|this, _: &ToggleTheme, w, cx| this.toggle_theme(w, cx)))
            .on_action(cx.listener(|_, _: &CheckForUpdates, w, cx| crate::update::check(true, w, cx)))
            .on_action(cx.listener(|this, _: &FormatDocument, w, cx| {
                this.format_active(JsonFormat::default(), w, cx)
            }))
            .on_action(cx.listener(|this, _: &FormatJsonIndent4, w, cx| {
                let opts = JsonFormat { indent: JsonIndent::Spaces(4), sort_keys: false };
                this.format_active(opts, w, cx)
            }))
            .on_action(cx.listener(|this, _: &FormatJsonTab, w, cx| {
                let opts = JsonFormat { indent: JsonIndent::Tab, sort_keys: false };
                this.format_active(opts, w, cx)
            }))
            .on_action(cx.listener(|this, _: &MinifyJson, w, cx| {
                let opts = JsonFormat { indent: JsonIndent::Minify, sort_keys: false };
                this.format_active(opts, w, cx)
            }))
            .on_action(cx.listener(|this, _: &SortJsonKeys, w, cx| {
                let opts = JsonFormat { sort_keys: true, ..JsonFormat::default() };
                this.format_active(opts, w, cx)
            }))
            .on_action(cx.listener(|this, _: &Quit, w, cx| {
                if this.request_quit(w, cx) {
                    cx.quit();
                }
            }))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                this.on_drop(paths, window, cx)
            }))
            .drag_over::<ExternalPaths>(|style, _, _, cx| style.bg(cx.theme().drop_target))
            .child(self.render_title_bar(cx))
            .child(div().flex_1().min_h_0().child(body))
            .child(self.render_status_bar(cx))
    }
}

/// “+” 新建菜单。
fn new_file_button(id: &'static str) -> impl IntoElement {
    Button::new(id)
        .ghost()
        .small()
        .icon(IconName::Plus)
        .tooltip("新建")
        .dropdown_menu(|menu, _, _| {
            menu.menu("Markdown", Box::new(NewMarkdown))
                .menu("JSON", Box::new(NewJson))
                .menu("YAML", Box::new(NewYaml))
                .menu("TOML", Box::new(NewToml))
        })
}

/// JSON 格式化选项（仅 JSON 文档显示）。
fn format_json_button() -> impl IntoElement {
    Button::new("format-json")
        .outline()
        .compact()
        .small()
        .label("格式化")
        .dropdown_caret(true)
        .tooltip("格式化 JSON")
        .dropdown_menu(|menu, _, _| {
            menu.menu("格式化（2 空格）", Box::new(FormatDocument))
                .menu("格式化（4 空格）", Box::new(FormatJsonIndent4))
                .menu("格式化（Tab）", Box::new(FormatJsonTab))
                .separator()
                .menu("按键名排序", Box::new(SortJsonKeys))
                .menu("压缩为一行", Box::new(MinifyJson))
        })
}

/// Windows / Linux 没有系统菜单栏，标题栏左侧放一个应用菜单。
fn app_menu_button() -> impl IntoElement {
    Button::new("app-menu")
        .ghost()
        .small()
        .icon(IconName::Menu)
        .dropdown_menu(|menu, _, _| {
            menu.menu("新建 Markdown", Box::new(NewMarkdown))
                .menu("新建 JSON", Box::new(NewJson))
                .menu("新建 YAML", Box::new(NewYaml))
                .menu("新建 TOML", Box::new(NewToml))
                .separator()
                .menu("打开文件…", Box::new(OpenFile))
                .menu("打开文件夹…", Box::new(OpenFolder))
                .separator()
                .menu("保存", Box::new(Save))
                .menu("另存为…", Box::new(SaveAs))
                .menu("关闭标签页", Box::new(CloseTab))
                .separator()
                .menu("格式化 JSON", Box::new(FormatDocument))
                .menu("压缩 JSON", Box::new(MinifyJson))
                .menu("切换主题", Box::new(ToggleTheme))
                .menu("检查更新…", Box::new(CheckForUpdates))
                .separator()
                .menu("退出", Box::new(Quit))
        })
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

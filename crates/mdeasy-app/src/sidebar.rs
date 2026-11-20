//! 左侧资源管理器：可加入文件夹或单个文件（按钮添加或直接拖入）。

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    list::ListItem,
    menu::PopupMenu,
    tree::{TreeEvent, TreeItem, TreeState, tree},
    v_flex,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use mdeasy_core::{FileKind, FsNode, ScanOptions, fs_tree};

use crate::actions::{OpenFile, OpenFolder};

pub enum SidebarEvent {
    Open(PathBuf),
    /// 根列表变化，需要持久化会话
    RootsChanged,
}

pub struct Sidebar {
    roots: Vec<PathBuf>,
    tree: Entity<TreeState>,
    expanded: HashSet<SharedString>,
    root_ids: HashSet<SharedString>,
    scanning: bool,
    _scan_task: Task<()>,
    _subscription: Subscription,
}

impl EventEmitter<SidebarEvent> for Sidebar {}

impl Sidebar {
    pub fn new(roots: Vec<PathBuf>, cx: &mut Context<Self>) -> Self {
        let tree = cx.new(|cx| TreeState::new(cx));
        let _subscription = cx.subscribe(&tree, |this, _, event: &TreeEvent, _| match event {
            TreeEvent::Expanded(id) => {
                this.expanded.insert(id.clone());
            }
            TreeEvent::Collapsed(id) => {
                this.expanded.remove(id);
            }
        });
        let roots: Vec<PathBuf> = roots.into_iter().filter(|p| p.exists()).collect();
        // 根目录默认展开
        let expanded = roots.iter().map(|p| path_id(p)).collect();
        let mut this = Self {
            roots,
            tree,
            expanded,
            root_ids: HashSet::new(),
            scanning: false,
            _scan_task: Task::ready(()),
            _subscription,
        };
        this.rescan(cx);
        this
    }

    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// 加入文件夹或文件（去重）。
    pub fn add_paths(&mut self, paths: impl IntoIterator<Item = PathBuf>, cx: &mut Context<Self>) {
        let mut changed = false;
        for path in paths {
            let path = std::fs::canonicalize(&path).unwrap_or(path);
            if path.is_file() && FileKind::from_path(&path).is_none() {
                continue;
            }
            if !self.roots.contains(&path) {
                self.expanded.insert(path_id(&path));
                self.roots.push(path);
                changed = true;
            }
        }
        if changed {
            self.rescan(cx);
            cx.emit(SidebarEvent::RootsChanged);
        }
    }

    pub fn remove_root(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.roots.retain(|p| p != path);
        self.rescan(cx);
        cx.emit(SidebarEvent::RootsChanged);
    }

    /// 在后台线程重新扫描所有根。
    pub fn rescan(&mut self, cx: &mut Context<Self>) {
        let roots = self.roots.clone();
        self.scanning = true;
        cx.notify();
        self._scan_task = cx.spawn(async move |this, cx| {
            let nodes: Vec<FsNode> = cx
                .background_spawn(async move {
                    let opts = ScanOptions::default();
                    roots.iter().filter_map(|r| fs_tree::scan(r, &opts)).collect()
                })
                .await;
            _ = this.update(cx, |this, cx| {
                this.root_ids = nodes.iter().map(|n| path_id(&n.path)).collect();
                let items: Vec<TreeItem> =
                    nodes.iter().map(|n| to_tree_item(n, &this.expanded)).collect();
                this.tree.update(cx, |tree, cx| tree.set_items(items, cx));
                this.scanning = false;
                cx.notify();
            });
        });
    }

    /// 让侧栏选中并滚动到当前打开的文件。
    pub fn reveal(&mut self, path: &Path, cx: &mut Context<Self>) {
        let id = path_id(path);
        self.tree.update(cx, |tree, cx| {
            tree.reveal_item(&id, ScrollStrategy::Center, cx);
            let ix = tree.index_of(&id);
            tree.set_selected_index(ix, cx);
        });
    }

    fn render_tree(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let root_ids = self.root_ids.clone();
        let menu_view = view.clone();
        let menu_roots = root_ids.clone();

        tree(&self.tree, move |ix, entry, _selected, _window, cx| {
            let item = entry.item();
            let is_root = root_ids.contains(&item.id);
            let path = PathBuf::from(item.id.as_str());
            let icon = if entry.is_folder() || (is_root && path.is_dir()) {
                if entry.is_expanded() {
                    IconName::FolderOpen
                } else {
                    IconName::Folder
                }
            } else {
                match FileKind::from_path(&path) {
                    Some(FileKind::Markdown) => IconName::BookOpen,
                    Some(FileKind::Json | FileKind::Yaml | FileKind::Toml) => IconName::FileText,
                    _ => IconName::File,
                }
            };
            let theme = cx.theme();
            let is_folder = entry.is_folder();

            ListItem::new(ix)
                .w_full()
                .rounded(theme.radius)
                .py_0p5()
                .px_2()
                .pl(px(14.) * entry.depth() + px(8.))
                .child(
                    h_flex()
                        .gap_2()
                        .min_w_0()
                        .child(Icon::new(icon).small().text_color(if is_folder || is_root {
                            theme.sidebar_foreground.opacity(0.8)
                        } else {
                            theme.muted_foreground
                        }))
                        .child(
                            div()
                                .truncate()
                                .when(is_root, |d| d.font_weight(FontWeight::MEDIUM))
                                .child(item.label.clone()),
                        ),
                )
                .on_click({
                    let view = view.clone();
                    move |_, _, cx| {
                        if !is_folder && path.is_file() {
                            view.update(cx, |_, cx| cx.emit(SidebarEvent::Open(path.clone())));
                        }
                    }
                })
        })
        .context_menu(move |_ix, entry, menu: PopupMenu, _window, _cx| {
            let path = PathBuf::from(entry.item().id.as_str());
            let is_root = menu_roots.contains(&entry.item().id);
            let reveal_path = path.clone();
            let menu = menu.item(
                gpui_kit::component::menu::PopupMenuItem::new(reveal_label())
                    .on_click(move |_, _, cx| cx.reveal_path(&reveal_path)),
            );
            if is_root {
                let view = menu_view.clone();
                menu.item(
                    gpui_kit::component::menu::PopupMenuItem::new("从侧栏移除").on_click(
                        move |_, _, cx| view.update(cx, |this, cx| this.remove_root(&path, cx)),
                    ),
                )
            } else {
                menu
            }
        })
        .text_size(px(13.))
        .px_1()
        .flex_1()
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        v_flex()
            .flex_1()
            .m_2()
            .p_4()
            .gap_3()
            .items_center()
            .justify_center()
            .rounded(theme.radius_lg)
            .border_1()
            .border_dashed()
            .border_color(theme.sidebar_border)
            .text_sm()
            .text_color(theme.muted_foreground)
            .child(Icon::new(IconName::FolderOpen).large())
            .child("把文件或文件夹拖到这里")
            .child(
                v_flex()
                    .w_full()
                    .gap_2()
                    .child(
                        Button::new("empty-open-folder")
                            .outline()
                            .small()
                            .w_full()
                            .label("打开文件夹")
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(OpenFolder), cx)
                            }),
                    )
                    .child(
                        Button::new("empty-open-file")
                            .ghost()
                            .small()
                            .w_full()
                            .label("添加文件")
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(OpenFile), cx)
                            }),
                    ),
            )
    }
}

impl Render for Sidebar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let empty = self.roots.is_empty();
        v_flex()
            .size_full()
            .bg(theme.sidebar)
            .text_color(theme.sidebar_foreground)
            .child(
                h_flex()
                    .h(px(36.))
                    .flex_shrink_0()
                    .pl_3()
                    .pr_1()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.muted_foreground)
                            .child(if self.scanning { "资源管理器 · 扫描中" } else { "资源管理器" }),
                    )
                    .child(
                        h_flex()
                            .child(
                                Button::new("sidebar-open-folder")
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Folder)
                                    .tooltip("打开文件夹")
                                    .on_click(|_, window, cx| {
                                        window.dispatch_action(Box::new(OpenFolder), cx)
                                    }),
                            )
                            .child(
                                Button::new("sidebar-open-file")
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Plus)
                                    .tooltip("添加文件")
                                    .on_click(|_, window, cx| {
                                        window.dispatch_action(Box::new(OpenFile), cx)
                                    }),
                            )
                            .child(
                                Button::new("sidebar-refresh")
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::RefreshCw)
                                    .tooltip("刷新")
                                    .on_click(cx.listener(|this, _, _, cx| this.rescan(cx))),
                            ),
                    ),
            )
            .map(|this| {
                if empty {
                    this.child(self.render_empty(cx))
                } else {
                    this.child(self.render_tree(cx))
                }
            })
    }
}

fn path_id(path: &Path) -> SharedString {
    path.to_string_lossy().into_owned().into()
}

fn to_tree_item(node: &FsNode, expanded: &HashSet<SharedString>) -> TreeItem {
    let id = path_id(&node.path);
    let is_expanded = expanded.contains(&id);
    TreeItem::new(id, node.name.clone())
        .children(node.children.iter().map(|c| to_tree_item(c, expanded)))
        .expanded(is_expanded)
}

fn reveal_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "在 Finder 中显示"
    } else if cfg!(target_os = "windows") {
        "在资源管理器中显示"
    } else {
        "在文件管理器中显示"
    }
}

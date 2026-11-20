//! JSON / YAML / TOML 预览：校验状态 + 可折叠的树形视图。
//!
//! 解析在后台线程进行并做防抖；树被拍平成行列表，交给 `uniform_list`
//! 只渲染可见行，几万个节点也不卡。

use std::{collections::HashSet, sync::Arc, time::Duration};

use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use mdeasy_core::{DataNode, FileKind, ParseError, data};

/// 节点总数超过该值时，默认只展开第一层。
const LARGE_TREE: usize = 3_000;
const PARSE_DEBOUNCE: Duration = Duration::from_millis(150);
/// 字符串值在行内最多显示的字符数。
const MAX_INLINE_CHARS: usize = 240;

pub enum DataPreviewEvent {
    /// 用户点击了错误提示，跳到源码对应位置（1 起始）。
    JumpTo { line: usize, column: usize },
}

struct Row {
    path: String,
    depth: usize,
    key: Option<String>,
    /// 数组下标显示得淡一些
    key_is_index: bool,
    kind: RowKind,
}

enum RowKind {
    Container { open: char, len: usize, collapsed: bool },
    Scalar(DataNode),
}

pub struct DataPreview {
    kind: FileKind,
    /// 最近一次成功解析的树。出错时保留旧树，避免预览闪烁。
    tree: Option<Arc<DataNode>>,
    error: Option<ParseError>,
    parsing: bool,
    node_count: usize,
    rows: Arc<Vec<Row>>,
    /// 相对默认展开状态被切换过的路径
    toggled: HashSet<String>,
    expand_all: Option<bool>,
    generation: u64,
    scroll: UniformListScrollHandle,
    _task: Task<()>,
}

impl EventEmitter<DataPreviewEvent> for DataPreview {}

impl DataPreview {
    pub fn new(kind: FileKind, source: String, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            kind,
            tree: None,
            error: None,
            parsing: true,
            node_count: 0,
            rows: Arc::new(Vec::new()),
            toggled: HashSet::new(),
            expand_all: None,
            generation: 0,
            scroll: UniformListScrollHandle::new(),
            _task: Task::ready(()),
        };
        this.schedule(source, Duration::ZERO, cx);
        this
    }

    /// 源码变化后调用，防抖后在后台解析。
    pub fn update_source(&mut self, source: String, cx: &mut Context<Self>) {
        self.schedule(source, PARSE_DEBOUNCE, cx);
    }

    pub fn error(&self) -> Option<&ParseError> {
        self.error.as_ref()
    }

    fn schedule(&mut self, source: String, delay: Duration, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        let kind = self.kind;
        self._task = cx.spawn(async move |this, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
            }
            let result = cx
                .background_spawn(async move {
                    let result = data::parse(kind, &source).expect("仅用于数据文件");
                    result.map(|node| {
                        let count = node.node_count();
                        (Arc::new(node), count)
                    })
                })
                .await;
            _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                this.parsing = false;
                match result {
                    Ok((node, count)) => {
                        this.tree = Some(node);
                        this.node_count = count;
                        this.error = None;
                        this.rebuild_rows();
                    }
                    Err(err) => this.error = Some(err),
                }
                cx.notify();
            });
        });
    }

    fn default_collapsed(&self, depth: usize) -> bool {
        match self.expand_all {
            Some(expand) => !expand,
            None => self.node_count > LARGE_TREE && depth >= 1,
        }
    }

    fn rebuild_rows(&mut self) {
        let mut rows = Vec::new();
        if let Some(tree) = self.tree.clone() {
            self.flatten(&tree, String::new(), 0, None, false, &mut rows);
        }
        self.rows = Arc::new(rows);
    }

    fn flatten(
        &self,
        node: &DataNode,
        path: String,
        depth: usize,
        key: Option<String>,
        key_is_index: bool,
        rows: &mut Vec<Row>,
    ) {
        match node {
            DataNode::Array(_) | DataNode::Object(_) => {
                let collapsed = self.default_collapsed(depth) ^ self.toggled.contains(&path);
                let open = if matches!(node, DataNode::Array(_)) { '[' } else { '{' };
                rows.push(Row {
                    path: path.clone(),
                    depth,
                    key,
                    key_is_index,
                    kind: RowKind::Container { open, len: node.len(), collapsed },
                });
                if collapsed {
                    return;
                }
                match node {
                    DataNode::Array(items) => {
                        for (i, item) in items.iter().enumerate() {
                            let child = format!("{path}\u{1f}{i}");
                            self.flatten(item, child, depth + 1, Some(i.to_string()), true, rows);
                        }
                    }
                    DataNode::Object(entries) => {
                        for (k, v) in entries {
                            let child = format!("{path}\u{1f}{k}");
                            self.flatten(v, child, depth + 1, Some(k.clone()), false, rows);
                        }
                    }
                    _ => unreachable!(),
                }
            }
            scalar => rows.push(Row {
                path,
                depth,
                key,
                key_is_index,
                kind: RowKind::Scalar(scalar.clone()),
            }),
        }
    }

    fn toggle(&mut self, path: &str, cx: &mut Context<Self>) {
        if !self.toggled.remove(path) {
            self.toggled.insert(path.to_string());
        }
        self.rebuild_rows();
        cx.notify();
    }

    fn set_expand_all(&mut self, expand: bool, cx: &mut Context<Self>) {
        self.expand_all = Some(expand);
        self.toggled.clear();
        self.rebuild_rows();
        cx.notify();
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let label = self.kind.label();
        let status = if let Some(err) = &self.error {
            let jump = err.line.map(|l| (l, err.column.unwrap_or(1)));
            h_flex()
                .id("data-error")
                .flex_1()
                .min_w_0()
                .gap_2()
                .text_color(theme.danger)
                .child(Icon::new(IconName::CircleAlert).small())
                .child(
                    div()
                        .truncate()
                        .child(format!("{label} 语法错误 · {err}")),
                )
                .when_some(jump, |this, (line, column)| {
                    this.cursor_pointer()
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.emit(DataPreviewEvent::JumpTo { line, column })
                        }))
                })
                .into_any_element()
        } else if self.parsing {
            h_flex()
                .flex_1()
                .gap_2()
                .text_color(theme.muted_foreground)
                .child("解析中…")
                .into_any_element()
        } else {
            h_flex()
                .flex_1()
                .gap_2()
                .child(Icon::new(IconName::CircleCheck).small().text_color(theme.success))
                .child(format!("{label} 格式正确"))
                .child(
                    div()
                        .text_color(theme.muted_foreground)
                        .child(format!("· {} 个节点", self.node_count)),
                )
                .into_any_element()
        };

        h_flex()
            .flex_shrink_0()
            .h(px(36.))
            .px_3()
            .gap_2()
            .text_sm()
            .border_b_1()
            .border_color(theme.border)
            .child(status)
            .child(
                Button::new("expand-all")
                    .ghost()
                    .xsmall()
                    .label("全部展开")
                    .on_click(cx.listener(|this, _, _, cx| this.set_expand_all(true, cx))),
            )
            .child(
                Button::new("collapse-all")
                    .ghost()
                    .xsmall()
                    .label("全部折叠")
                    .on_click(cx.listener(|this, _, _, cx| this.set_expand_all(false, cx))),
            )
    }

    fn render_rows(
        &mut self,
        range: std::ops::Range<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let rows = self.rows.clone();
        // JSON 的键带引号，YAML / TOML 按各自习惯不加
        let quote_keys = self.kind == FileKind::Json;
        range
            .filter_map(|ix| rows.get(ix).map(|row| (ix, row)))
            .map(|(ix, row)| {
                let indent = px(12. + row.depth as f32 * 16.);
                let key = row.key.as_ref().map(|k| {
                    let color = if row.key_is_index {
                        theme.muted_foreground
                    } else {
                        theme.blue
                    };
                    h_flex()
                        .flex_shrink_0()
                        .child(div().text_color(color).child(if row.key_is_index || !quote_keys {
                            k.clone()
                        } else {
                            format!("\"{k}\"")
                        }))
                        .child(div().text_color(theme.muted_foreground).child(":"))
                });

                let base = h_flex()
                    .id(("data-row", ix))
                    .h(px(24.))
                    .w_full()
                    .pl(indent)
                    .pr_3()
                    .gap_1p5()
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .hover(|s| s.bg(theme.list_hover));

                match &row.kind {
                    RowKind::Container { open, len, collapsed } => {
                        let path = row.path.clone();
                        let close = if *open == '[' { ']' } else { '}' };
                        let unit = if *open == '[' { "项" } else { "个键" };
                        base.cursor_pointer()
                            .child(
                                Icon::new(if *collapsed {
                                    IconName::ChevronRight
                                } else {
                                    IconName::ChevronDown
                                })
                                .xsmall()
                                .text_color(theme.muted_foreground),
                            )
                            .children(key)
                            .child(div().text_color(theme.muted_foreground).child(
                                if *collapsed {
                                    format!("{open} … {close}")
                                } else {
                                    open.to_string()
                                },
                            ))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(format!("{len} {unit}")),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| this.toggle(&path, cx)))
                            .into_any_element()
                    }
                    RowKind::Scalar(value) => {
                        let (text, color) = scalar_display(value, &theme);
                        base.child(div().w(px(12.)).flex_shrink_0())
                            .children(key)
                            .child(div().min_w_0().truncate().text_color(color).child(text))
                            .into_any_element()
                    }
                }
            })
            .collect()
    }
}

fn scalar_display(value: &DataNode, theme: &gpui_kit::component::Theme) -> (String, Hsla) {
    match value {
        DataNode::Null => ("null".into(), theme.muted_foreground),
        DataNode::Bool(b) => (b.to_string(), theme.magenta),
        DataNode::Number(n) => (n.clone(), theme.yellow),
        DataNode::DateTime(d) => (d.clone(), theme.cyan),
        DataNode::String(s) => {
            let mut shown: String = s.chars().take(MAX_INLINE_CHARS).collect();
            if shown.len() < s.len() {
                shown.push('…');
            }
            let escaped = shown.replace('\\', "\\\\").replace('\n', "\\n").replace('"', "\\\"");
            (format!("\"{escaped}\""), theme.green)
        }
        DataNode::Array(_) | DataNode::Object(_) => (String::new(), theme.foreground),
    }
}

impl Render for DataPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let row_count = self.rows.len();
        let dimmed = self.error.is_some();
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .child(self.render_header(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .py_2()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_size(cx.theme().mono_font_size)
                    .when(dimmed, |this| this.opacity(0.55))
                    .child(
                        uniform_list(
                            "data-rows",
                            row_count,
                            cx.processor(|this, range, _, cx| this.render_rows(range, cx)),
                        )
                        .size_full()
                        .track_scroll(&self.scroll),
                    ),
            )
    }
}

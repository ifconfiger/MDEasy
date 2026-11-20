//! 一个打开的文件：源码编辑器 + 对应的预览，以及保存状态。

use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

use gpui_kit::component::{
    input::{EditorState, InputEvent, Position, TabSize},
    text::TextViewState,
};
use gpui_kit::*;
use mdeasy_core::{FileKind, data};

use crate::data_view::{DataPreview, DataPreviewEvent};

/// 检查磁盘文件后的结果。
pub enum DiskChange {
    None,
    /// 文件被外部修改且本地没有未保存内容，已自动重新加载
    Reloaded,
    /// 文件被外部修改，但本地也有未保存修改
    Conflict,
    Deleted,
}

pub enum DocumentEvent {
    /// 内容或保存状态变化（标签上的未保存标记、状态栏需要刷新）。
    Changed,
}

pub struct Document {
    pub path: Option<PathBuf>,
    pub kind: FileKind,
    pub editor: Entity<EditorState>,
    pub markdown: Option<Entity<TextViewState>>,
    pub data: Option<Entity<DataPreview>>,
    /// 未命名文档的序号，用于显示 "未命名-1.md"
    untitled: usize,
    saved_text: SharedString,
    dirty: bool,
    /// 最近一次读取/保存时磁盘文件的修改时间
    mtime: Option<SystemTime>,
    /// (行数, 字符数)，内容变化时更新
    stats: (usize, usize),
    _subscription: Subscription,
    _data_subscription: Option<Subscription>,
}

impl EventEmitter<DocumentEvent> for Document {}

impl Document {
    /// 读取文件内容（在创建实体前调用，读取失败不会产生空标签）。
    pub fn load(path: &Path) -> anyhow::Result<(FileKind, String)> {
        let kind = FileKind::from_path(path).unwrap_or(FileKind::Text);
        let bytes = std::fs::read(path)?;
        let content =
            String::from_utf8(bytes).map_err(|_| anyhow::anyhow!("不是 UTF-8 文本文件"))?;
        Ok((kind, content))
    }

    pub fn opened(
        path: PathBuf,
        kind: FileKind,
        content: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::new(Some(path), kind, content, 0, window, cx)
    }

    pub fn untitled(kind: FileKind, index: usize, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::new(None, kind, String::new(), index, window, cx)
    }

    fn new(
        path: Option<PathBuf>,
        kind: FileKind,
        content: String,
        untitled: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let wrap = matches!(kind, FileKind::Markdown | FileKind::Text);
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language(kind.language())
                .line_number(true)
                .soft_wrap(wrap)
                .searchable(true)
                .tab_size(TabSize {
                    tab_size: 2,
                    hard_tabs: false,
                })
                .placeholder(match kind {
                    FileKind::Markdown => "开始写作…",
                    _ => "",
                })
                .default_value(content.clone())
        });

        let _subscription = cx.subscribe(&editor, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.on_change(cx);
            }
        });

        let mut this = Self {
            path,
            kind,
            editor,
            markdown: None,
            data: None,
            untitled,
            saved_text: content.clone().into(),
            dirty: false,
            stats: text_stats(&content),
            mtime: None,
            _subscription,
            _data_subscription: None,
        };
        this.mtime = this.path.as_deref().and_then(disk_mtime);
        this.build_previews(&content, window, cx);
        this
    }

    /// 按当前类型创建预览：Markdown 用 TextView，数据文件用树形视图。
    fn build_previews(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let kind = self.kind;
        self.markdown = (kind == FileKind::Markdown).then(|| {
            cx.new(|cx| {
                TextViewState::markdown(text, cx)
                    .scrollable(true)
                    .selectable(true)
            })
        });
        self.data = kind
            .is_data()
            .then(|| cx.new(|cx| DataPreview::new(kind, text.to_string(), cx)));
        self._data_subscription = self.data.as_ref().map(|data| {
            cx.subscribe_in(data, window, |this, _, event: &DataPreviewEvent, window, cx| {
                match event {
                    DataPreviewEvent::JumpTo { line, column } => {
                        this.jump_to(*line, *column, window, cx)
                    }
                }
            })
        });
    }

    fn on_change(&mut self, cx: &mut Context<Self>) {
        let text = self.editor.read(cx).value();
        self.dirty = text != self.saved_text;
        self.stats = text_stats(&text);
        if let Some(md) = &self.markdown {
            // TextViewState 在后台线程解析，这里只是投递文本
            md.update(cx, |state, cx| state.set_text(&text, cx));
        }
        if let Some(data) = &self.data {
            data.update(cx, |state, cx| state.update_source(text.to_string(), cx));
        }
        cx.emit(DocumentEvent::Changed);
        cx.notify();
    }

    pub fn stats(&self) -> (usize, usize) {
        self.stats
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn title(&self) -> String {
        match &self.path {
            Some(p) => mdeasy_core::fs_tree::display_name(p),
            None => format!("未命名-{}.{}", self.untitled, self.kind.default_extension()),
        }
    }

    pub fn text(&self, cx: &App) -> SharedString {
        self.editor.read(cx).value()
    }

    /// 写入磁盘。`path` 为 `None` 时写到当前路径。
    pub fn save_to(
        &mut self,
        path: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        let path = path
            .or_else(|| self.path.clone())
            .ok_or_else(|| anyhow::anyhow!("没有保存路径"))?;
        let text = self.text(cx);
        std::fs::write(&path, text.as_bytes())?;

        // 另存为新扩展名时切换类型（例如未命名文档存成 .json）
        let new_kind = FileKind::from_path(&path).unwrap_or(FileKind::Text);
        let kind_changed = new_kind != self.kind;
        self.mtime = disk_mtime(&path);
        self.path = Some(path);
        self.saved_text = text;
        self.dirty = false;
        if kind_changed {
            self.kind = new_kind;
            let text = self.text(cx);
            self.editor
                .update(cx, |state, cx| state.set_highlighter(new_kind.language(), cx));
            self.build_previews(&text, window, cx);
        }
        cx.emit(DocumentEvent::Changed);
        cx.notify();
        Ok(())
    }

    /// 对比磁盘修改时间；外部改动且本地无未保存内容时自动重新加载。
    pub fn check_disk(&mut self, window: &mut Window, cx: &mut Context<Self>) -> DiskChange {
        let Some(path) = self.path.clone() else { return DiskChange::None };
        let Some(mtime) = disk_mtime(&path) else {
            return if self.mtime.take().is_some() { DiskChange::Deleted } else { DiskChange::None };
        };
        if self.mtime == Some(mtime) {
            return DiskChange::None;
        }
        self.mtime = Some(mtime);
        let Ok((_, content)) = Self::load(&path) else { return DiskChange::None };
        if content == self.saved_text.as_ref() {
            return DiskChange::None;
        }
        if self.dirty {
            return DiskChange::Conflict;
        }
        self.saved_text = content.clone().into();
        self.editor
            .update(cx, |state, cx| state.set_value(content, window, cx));
        self.on_change(cx);
        DiskChange::Reloaded
    }

    /// 仅 JSON 支持格式化（YAML/TOML 重新序列化会丢注释）。
    pub fn format(
        &mut self,
        opts: data::JsonFormat,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        if self.kind != FileKind::Json {
            anyhow::bail!("只有 JSON 支持格式化");
        }
        let text = self.text(cx);
        let formatted = data::format_json_with(&text, opts)?;
        if formatted == text.as_str() {
            return Ok(());
        }
        self.editor
            .update(cx, |state, cx| state.set_value(formatted, window, cx));
        // set_value 不触发 Change 事件，手动同步预览和保存状态
        self.on_change(cx);
        Ok(())
    }

    /// 跳到 1 起始的行列。
    pub fn jump_to(&mut self, line: usize, column: usize, window: &mut Window, cx: &mut Context<Self>) {
        let position = Position {
            line: line.saturating_sub(1) as u32,
            character: column.saturating_sub(1) as u32,
        };
        self.editor
            .update(cx, |state, cx| state.set_cursor_position(position, window, cx));
    }
}

fn text_stats(text: &str) -> (usize, usize) {
    let lines = text.bytes().filter(|&b| b == b'\n').count() + 1;
    (lines, text.chars().count())
}

fn disk_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

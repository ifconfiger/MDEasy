use std::path::Path;

/// 编辑器支持的文件类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileKind {
    Markdown,
    Json,
    Yaml,
    Toml,
    Text,
}

impl FileKind {
    /// 按扩展名识别；无法识别的返回 `None`（侧栏不显示）。
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        Self::from_extension(&ext)
    }

    pub fn from_extension(ext: &str) -> Option<Self> {
        Some(match ext {
            "md" | "markdown" | "mdown" | "mkd" => Self::Markdown,
            "json" | "jsonc" => Self::Json,
            "yaml" | "yml" => Self::Yaml,
            "toml" => Self::Toml,
            "txt" | "text" => Self::Text,
            _ => return None,
        })
    }

    /// 用于语法高亮的 tree-sitter 语言名。
    pub fn language(self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::Json => "json",
            Self::Yaml => "yaml",
            Self::Toml => "toml",
            Self::Text => "text",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Markdown => "Markdown",
            Self::Json => "JSON",
            Self::Yaml => "YAML",
            Self::Toml => "TOML",
            Self::Text => "纯文本",
        }
    }

    pub fn default_extension(self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Json => "json",
            Self::Yaml => "yaml",
            Self::Toml => "toml",
            Self::Text => "txt",
        }
    }

    /// 是否为可解析成树结构的数据文件。
    pub fn is_data(self) -> bool {
        matches!(self, Self::Json | Self::Yaml | Self::Toml)
    }

    /// 是否有预览视图。
    pub fn has_preview(self) -> bool {
        self != Self::Text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_by_extension_case_insensitive() {
        assert_eq!(FileKind::from_path(Path::new("a/README.MD")), Some(FileKind::Markdown));
        assert_eq!(FileKind::from_path(Path::new("c.yml")), Some(FileKind::Yaml));
        assert_eq!(FileKind::from_path(Path::new("Cargo.toml")), Some(FileKind::Toml));
        assert_eq!(FileKind::from_path(Path::new("x.json")), Some(FileKind::Json));
        assert_eq!(FileKind::from_path(Path::new("main.rs")), None);
        assert_eq!(FileKind::from_path(Path::new("Makefile")), None);
    }
}

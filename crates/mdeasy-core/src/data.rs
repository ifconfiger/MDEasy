//! JSON / YAML / TOML 统一解析成 [`DataNode`] 树，供预览和校验使用。

use std::fmt;

use crate::FileKind;

/// 与格式无关的数据树。对象保留源文件中的键顺序。
#[derive(Debug, Clone, PartialEq)]
pub enum DataNode {
    Null,
    Bool(bool),
    /// 数字按源文本保留，避免浮点精度变化。
    Number(String),
    String(String),
    /// TOML 的日期时间。
    DateTime(String),
    Array(Vec<DataNode>),
    Object(Vec<(String, DataNode)>),
}

impl DataNode {
    pub fn is_container(&self) -> bool {
        matches!(self, Self::Array(_) | Self::Object(_))
    }

    /// 子节点数量（标量为 0）。
    pub fn len(&self) -> usize {
        match self {
            Self::Array(items) => items.len(),
            Self::Object(entries) => entries.len(),
            _ => 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 树中节点总数（含自身）。
    pub fn node_count(&self) -> usize {
        1 + match self {
            Self::Array(items) => items.iter().map(Self::node_count).sum(),
            Self::Object(entries) => entries.iter().map(|(_, v)| v.node_count()).sum(),
            _ => 0,
        }
    }

    /// 类型名，用于预览中的标签。
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "bool",
            Self::Number(_) => "number",
            Self::String(_) => "string",
            Self::DateTime(_) => "datetime",
            Self::Array(_) => "array",
            Self::Object(_) => "object",
        }
    }
}

/// 解析错误，行列从 1 开始。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.line, self.column) {
            (Some(l), Some(c)) => write!(f, "第 {l} 行第 {c} 列：{}", self.message),
            (Some(l), None) => write!(f, "第 {l} 行：{}", self.message),
            _ => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for ParseError {}

/// 解析数据文件。Markdown / 纯文本返回 `None`。
pub fn parse(kind: FileKind, source: &str) -> Option<Result<DataNode, ParseError>> {
    Some(match kind {
        FileKind::Json => parse_json(source),
        FileKind::Yaml => parse_yaml(source),
        FileKind::Toml => parse_toml(source),
        FileKind::Markdown | FileKind::Text => return None,
    })
}

pub fn parse_json(source: &str) -> Result<DataNode, ParseError> {
    if source.trim().is_empty() {
        return Ok(DataNode::Null);
    }
    serde_json::from_str::<serde_json::Value>(source)
        .map(from_json)
        .map_err(|e| ParseError {
            message: strip_position_suffix(&e.to_string()),
            line: Some(e.line()),
            column: Some(e.column()),
        })
}

pub fn parse_yaml(source: &str) -> Result<DataNode, ParseError> {
    if source.trim().is_empty() {
        return Ok(DataNode::Null);
    }
    serde_yaml_ng::from_str::<serde_yaml_ng::Value>(source)
        .map(from_yaml)
        .map_err(|e| {
            let loc = e.location();
            ParseError {
                message: strip_position_suffix(&e.to_string()),
                line: loc.as_ref().map(|l| l.line()),
                column: loc.as_ref().map(|l| l.column()),
            }
        })
}

pub fn parse_toml(source: &str) -> Result<DataNode, ParseError> {
    source
        .parse::<toml::Table>()
        .map(|t| from_toml(toml::Value::Table(t)))
        .map_err(|e| {
            let (line, column) = match e.span() {
                Some(span) => {
                    let (l, c) = line_col(source, span.start);
                    (Some(l), Some(c))
                }
                None => (None, None),
            };
            ParseError {
                message: e.message().trim().to_string(),
                line,
                column,
            }
        })
}

/// 格式化 JSON（2 空格缩进，保持键顺序）。其他格式格式化会丢注释，不提供。
pub fn format_json(source: &str) -> Result<String, ParseError> {
    let value: serde_json::Value = serde_json::from_str(source).map_err(|e| ParseError {
        message: strip_position_suffix(&e.to_string()),
        line: Some(e.line()),
        column: Some(e.column()),
    })?;
    let mut out = serde_json::to_string_pretty(&value).expect("Value 序列化不会失败");
    out.push('\n');
    Ok(out)
}

fn from_json(v: serde_json::Value) -> DataNode {
    use serde_json::Value;
    match v {
        Value::Null => DataNode::Null,
        Value::Bool(b) => DataNode::Bool(b),
        Value::Number(n) => DataNode::Number(n.to_string()),
        Value::String(s) => DataNode::String(s),
        Value::Array(a) => DataNode::Array(a.into_iter().map(from_json).collect()),
        Value::Object(o) => DataNode::Object(o.into_iter().map(|(k, v)| (k, from_json(v))).collect()),
    }
}

fn from_yaml(v: serde_yaml_ng::Value) -> DataNode {
    use serde_yaml_ng::Value;
    match v {
        Value::Null => DataNode::Null,
        Value::Bool(b) => DataNode::Bool(b),
        Value::Number(n) => DataNode::Number(n.to_string()),
        Value::String(s) => DataNode::String(s),
        Value::Sequence(a) => DataNode::Array(a.into_iter().map(from_yaml).collect()),
        Value::Mapping(m) => DataNode::Object(
            m.into_iter()
                .map(|(k, v)| (yaml_key(k), from_yaml(v)))
                .collect(),
        ),
        Value::Tagged(t) => from_yaml(t.value),
    }
}

fn yaml_key(k: serde_yaml_ng::Value) -> String {
    use serde_yaml_ng::Value;
    match k {
        Value::String(s) => s,
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => serde_yaml_ng::to_string(&other)
            .map(|s| s.trim_end().to_string())
            .unwrap_or_else(|_| "?".into()),
    }
}

fn from_toml(v: toml::Value) -> DataNode {
    use toml::Value;
    match v {
        Value::String(s) => DataNode::String(s),
        Value::Integer(i) => DataNode::Number(i.to_string()),
        Value::Float(f) => DataNode::Number(f.to_string()),
        Value::Boolean(b) => DataNode::Bool(b),
        Value::Datetime(d) => DataNode::DateTime(d.to_string()),
        Value::Array(a) => DataNode::Array(a.into_iter().map(from_toml).collect()),
        Value::Table(t) => DataNode::Object(t.into_iter().map(|(k, v)| (k, from_toml(v))).collect()),
    }
}

/// 字节偏移转 1 起始的行列（列按字符计）。
fn line_col(source: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(source.len());
    let before = &source[..source.floor_char_boundary(offset)];
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    (line, column)
}

/// serde 错误信息末尾自带 " at line X column Y"，位置单独展示，这里去掉。
fn strip_position_suffix(msg: &str) -> String {
    match msg.rfind(" at line ") {
        Some(ix) => msg[..ix].to_string(),
        None => msg.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_keeps_key_order() {
        let node = parse_json(r#"{"b": 1, "a": [true, null, "x"]}"#).unwrap();
        let DataNode::Object(entries) = &node else { panic!() };
        assert_eq!(entries[0].0, "b");
        assert_eq!(entries[1].0, "a");
        assert_eq!(node.node_count(), 6);
    }

    #[test]
    fn json_error_has_position() {
        let err = parse_json("{\n  \"a\": 1,\n}").unwrap_err();
        assert_eq!(err.line, Some(3));
        assert!(!err.message.contains(" at line "));
    }

    #[test]
    fn yaml_parses_and_reports_errors() {
        let node = parse_yaml("name: mdeasy\nlist:\n  - 1\n  - two\n").unwrap();
        assert_eq!(node.len(), 2);
        let err = parse_yaml("a: [1, 2\nb: 3").unwrap_err();
        assert!(err.line.is_some());
    }

    #[test]
    fn toml_parses_and_reports_errors() {
        let node = parse_toml("[package]\nname = \"x\"\nversion = 1\n").unwrap();
        let DataNode::Object(entries) = &node else { panic!() };
        assert_eq!(entries[0].0, "package");
        let err = parse_toml("a = 1\nb = \n").unwrap_err();
        assert_eq!(err.line, Some(2));
    }

    #[test]
    fn empty_documents_are_valid() {
        assert_eq!(parse_json("  \n").unwrap(), DataNode::Null);
        assert_eq!(parse_yaml("").unwrap(), DataNode::Null);
        assert_eq!(parse_toml("").unwrap(), DataNode::Object(vec![]));
    }

    #[test]
    fn format_json_pretty_prints() {
        assert_eq!(format_json(r#"{"a":[1,2]}"#).unwrap(), "{\n  \"a\": [\n    1,\n    2\n  ]\n}\n");
    }

    #[test]
    fn line_col_handles_multibyte() {
        assert_eq!(line_col("中文\nab", 7), (2, 1));
        assert_eq!(line_col("中文", 3), (1, 2));
    }
}

//! 会话持久化：侧栏根目录、主题、视图模式。保存在系统配置目录下的 `MDEasy/session.json`。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Session {
    /// 侧栏中的文件夹和单独加入的文件。
    pub roots: Vec<PathBuf>,
    pub dark: bool,
    /// "edit" | "split" | "preview"
    pub view_mode: String,
    pub sidebar_visible: Option<bool>,
    /// 上次打开的标签页（仅已保存到磁盘的文件）
    pub open_files: Vec<PathBuf>,
    pub active_file: Option<PathBuf>,
}

impl Session {
    pub fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("MDEasy").join("session.json"))
    }

    /// 读取失败（首次启动、文件损坏）时返回默认值。
    pub fn load() -> Self {
        Self::path()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::path().ok_or_else(|| anyhow::anyhow!("找不到配置目录"))?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tolerates_missing_fields() {
        let s: Session = serde_json::from_str(r#"{"dark": true}"#).unwrap();
        assert!(s.dark);
        assert!(s.roots.is_empty());
    }
}

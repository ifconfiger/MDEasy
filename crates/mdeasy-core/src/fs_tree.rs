//! 扫描目录，生成侧栏文件树。只保留支持的文件类型，空目录会被剪掉。

use std::path::{Path, PathBuf};

use crate::FileKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsNode {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub children: Vec<FsNode>,
}

#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub max_depth: usize,
    /// 扫描的文件 + 目录总数上限，防止误开 `/` 之类的大目录卡死。
    pub max_entries: usize,
    pub show_hidden: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            max_depth: 16,
            max_entries: 50_000,
            show_hidden: false,
        }
    }
}

const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "__pycache__",
    "dist",
    "build",
    ".git",
    ".svn",
    ".hg",
    ".venv",
    "venv",
];

/// 扫描单个路径：目录返回带子节点的树，支持的文件返回叶子，其余返回 `None`。
pub fn scan(path: &Path, opts: &ScanOptions) -> Option<FsNode> {
    let mut budget = opts.max_entries;
    if path.is_dir() {
        let mut node = scan_dir(path, 0, opts, &mut budget).unwrap_or_else(|| leaf(path, true));
        // 根目录即使为空也保留，方便用户看到它已加入侧栏
        node.is_dir = true;
        Some(node)
    } else if FileKind::from_path(path).is_some() {
        Some(leaf(path, false))
    } else {
        None
    }
}

fn leaf(path: &Path, is_dir: bool) -> FsNode {
    FsNode {
        path: path.to_path_buf(),
        name: display_name(path),
        is_dir,
        children: Vec::new(),
    }
}

pub fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

fn scan_dir(dir: &Path, depth: usize, opts: &ScanOptions, budget: &mut usize) -> Option<FsNode> {
    let mut children = Vec::new();
    if depth <= opts.max_depth
        && let Ok(entries) = std::fs::read_dir(dir)
    {
        for entry in entries.flatten() {
            if *budget == 0 {
                break;
            }
            *budget -= 1;

            let name = entry.file_name().to_string_lossy().into_owned();
            if !opts.show_hidden && name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            // file_type 不跟随符号链接，避免链接成环
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_dir() {
                if SKIP_DIRS.contains(&name.as_str()) {
                    continue;
                }
                if let Some(child) = scan_dir(&path, depth + 1, opts, budget) {
                    children.push(child);
                }
            } else if (ft.is_file() || ft.is_symlink()) && FileKind::from_path(&path).is_some() {
                children.push(leaf(&path, false));
            }
        }
    }

    if children.is_empty() {
        return None;
    }
    children.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Some(FsNode {
        path: dir.to_path_buf(),
        name: display_name(dir),
        is_dir: true,
        children,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mdeasy-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn scans_supported_files_and_prunes_empty_dirs() {
        let root = tmpdir("scan");
        std::fs::create_dir_all(root.join("docs/sub")).unwrap();
        std::fs::create_dir_all(root.join("empty")).unwrap();
        std::fs::create_dir_all(root.join("node_modules")).unwrap();
        std::fs::write(root.join("node_modules/x.md"), "").unwrap();
        std::fs::write(root.join("docs/sub/a.md"), "").unwrap();
        std::fs::write(root.join("Cargo.toml"), "").unwrap();
        std::fs::write(root.join("main.rs"), "").unwrap();
        std::fs::write(root.join(".hidden.md"), "").unwrap();

        let node = scan(&root, &ScanOptions::default()).unwrap();
        let names: Vec<_> = node.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["docs", "Cargo.toml"]);
        assert_eq!(node.children[0].children[0].children[0].name, "a.md");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn empty_root_is_kept() {
        let root = tmpdir("empty-root");
        let node = scan(&root, &ScanOptions::default()).unwrap();
        assert!(node.is_dir && node.children.is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }
}

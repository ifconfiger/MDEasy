//! MDEasy 核心逻辑，不依赖任何 UI 框架，便于测试和替换界面层。

pub mod data;
pub mod file_kind;
pub mod fs_tree;
pub mod session;

pub use data::{DataNode, ParseError};
pub use file_kind::FileKind;
pub use fs_tree::{FsNode, ScanOptions};
pub use session::Session;

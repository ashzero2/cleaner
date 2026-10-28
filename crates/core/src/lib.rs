//! Parallel disk usage scanner for macOS.
//!
//! [`Scanner`] walks a directory tree with `getattrlistbulk(2)` on a dedicated
//! thread pool and produces an immutable, index-based [`Tree`] that can be shared
//! freely across threads.

#[cfg(not(target_os = "macos"))]
compile_error!("clrs-core currently supports macOS only");

mod error;
mod scan;
mod sys;
mod tree;

pub use error::{Error, ScanIssue};
pub use scan::{Progress, ProgressSnapshot, Scanner};
pub use sys::EntryKind;
pub use tree::{Node, NodeId, Tree};

//! Parallel disk usage scanner for macOS.
//!
//! [`Scanner`] walks a directory tree with `getattrlistbulk(2)` on a dedicated
//! thread pool and produces an immutable [`Tree`] that can be shared freely across
//! threads. [`Rules`] locate well-known cleanup targets in a tree, and [`clean`] acts on them.

#[cfg(not(target_os = "macos"))]
compile_error!("clrs-core currently supports macOS only");

mod cleanup;
mod error;
mod rules;
mod scan;
mod sys;
mod tree;

pub use cleanup::{Outcome, clean};
pub use error::{Error, ScanIssue};
pub use rules::{Finding, Rule, Rules, Safety};
pub use scan::{Progress, ProgressSnapshot, Scanner};
pub use sys::EntryKind;
pub use tree::{Node, NodeId, Tree};

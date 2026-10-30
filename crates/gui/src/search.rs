use std::os::unix::ffi::OsStrExt as _;
use std::sync::atomic::{AtomicBool, Ordering};

use clrs_core::{Cached, EntryKind, NodeId};

/// Most rows a search shows; the rest are counted but not listed.
pub(crate) const LIMIT: usize = 500;

#[derive(Debug)]
pub(crate) struct Matches {
    pub(crate) ids: Vec<NodeId>,
    pub(crate) total: usize,
}

/// Case-insensitive name search under `scope`, largest first. Returns early with whatever
/// was found when `cancel` is set.
pub(crate) fn find(cached: &Cached, scope: NodeId, query: &str, cancel: &AtomicBool) -> Matches {
    let tree = &cached.tree;
    let ascii = query.is_ascii();
    let needle = query.to_lowercase();
    let mut ids = Vec::new();
    let mut stack = vec![scope];
    while let Some(directory) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        for id in tree.children(directory) {
            let name = tree.name(id);
            let hit = if ascii {
                name.as_bytes()
                    .windows(needle.len())
                    .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
            } else {
                name.to_string_lossy().to_lowercase().contains(&needle)
            };
            if hit {
                ids.push(id);
            }
            if tree.node(id).kind() == EntryKind::Dir {
                stack.push(id);
            }
        }
    }
    let total = ids.len();
    ids.sort_by_key(|&id| std::cmp::Reverse(tree.node(id).size()));
    ids.truncate(LIMIT);
    Matches { ids, total }
}

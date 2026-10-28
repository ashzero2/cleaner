use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, VecDeque};
use std::ffi::OsStr;
use std::mem;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::error::Error;
use crate::scan::Subtree;
use crate::sys::EntryKind;

pub(crate) const NO_BLOCK: u32 = u32::MAX;
pub(crate) const FLAG_MULTI: u8 = 1;
pub(crate) const FLAG_REUSED: u8 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId {
    pub(crate) block: u32,
    pub(crate) index: u32,
}

impl NodeId {
    pub(crate) fn new(block: u32, index: u32) -> Self {
        Self { block, index }
    }

    pub(crate) fn block(self) -> usize {
        self.block as usize
    }

    pub(crate) fn index(self) -> usize {
        self.index as usize
    }
}

#[derive(Clone, Debug)]
pub struct Node {
    pub(crate) size: u64,
    pub(crate) name_start: u32,
    pub(crate) files: u32,
    pub(crate) block: u32,
    pub(crate) name_len: u16,
    pub(crate) kind: EntryKind,
    pub(crate) flags: u8,
}

impl Node {
    pub(crate) fn leaf(
        name_start: u32,
        name_len: u16,
        kind: EntryKind,
        size: u64,
        multi: bool,
    ) -> Self {
        Self {
            size,
            name_start,
            files: 1,
            block: NO_BLOCK,
            name_len,
            kind,
            flags: if multi { FLAG_MULTI } else { 0 },
        }
    }

    pub(crate) fn dir(name_start: u32, name_len: u16) -> Self {
        Self {
            size: 0,
            name_start,
            files: 0,
            block: NO_BLOCK,
            name_len,
            kind: EntryKind::Dir,
            flags: 0,
        }
    }

    pub(crate) fn reuse(name_start: u32, name_len: u16, old: &Node) -> Self {
        Self {
            size: old.size,
            name_start,
            files: old.files,
            block: old.block,
            name_len,
            kind: EntryKind::Dir,
            flags: FLAG_REUSED,
        }
    }

    pub(crate) fn name<'a>(&self, names: &'a [u8]) -> &'a [u8] {
        let start = self.name_start as usize;
        &names[start..start + usize::from(self.name_len)]
    }

    /// Bytes allocated on disk, with hard-linked files counted once.
    #[must_use]
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Number of non-directory entries in this subtree.
    #[must_use]
    pub fn files(&self) -> u64 {
        u64::from(self.files)
    }

    #[must_use]
    pub fn kind(&self) -> EntryKind {
        self.kind
    }
}

/// Identifies one hard-linked file within its directory block, so charges can be
/// moved between links when only part of a tree is rescanned.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Link {
    pub(crate) name_start: u32,
    pub(crate) dev: i32,
    pub(crate) ino: u64,
    pub(crate) alloc: u64,
    pub(crate) nlink: u32,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Block {
    pub(crate) names: Box<[u8]>,
    pub(crate) nodes: Box<[Node]>,
    pub(crate) links: Box<[Link]>,
}

/// An immutable snapshot of a scanned directory tree.
///
/// Each directory owns one block holding its children's names and records, sorted by
/// size, largest first. Blocks are reference-counted, so a patched copy of a tree
/// shares every block it did not change.
#[derive(Debug)]
pub struct Tree {
    pub(crate) root_path: PathBuf,
    pub(crate) blocks: Vec<Arc<Block>>,
    pub(crate) parents: Vec<Option<NodeId>>,
    pub(crate) len: usize,
}

impl Tree {
    pub(crate) fn empty(root_path: PathBuf) -> Self {
        Self {
            root_path,
            blocks: Vec::new(),
            parents: Vec::new(),
            len: 0,
        }
    }

    pub(crate) fn single_file(root_path: PathBuf, size: u64) -> Result<Self, Error> {
        let names: Box<[u8]> = root_path.as_os_str().as_bytes().into();
        let name_len = u16::try_from(names.len()).map_err(|_| Error::TooManyEntries)?;
        let block = Block {
            names,
            nodes: Box::from([Node::leaf(0, name_len, EntryKind::File, size, false)]),
            links: Box::default(),
        };
        Ok(Self {
            root_path,
            blocks: vec![Arc::new(block)],
            parents: vec![None],
            len: 1,
        })
    }

    pub(crate) fn build(root_path: PathBuf, root: Subtree) -> Result<Self, Error> {
        let len = usize::try_from(root.nodes + 1).map_err(|_| Error::TooManyEntries)?;
        u32::try_from(len).map_err(|_| Error::TooManyEntries)?;
        let capacity = usize::try_from(root.dirs + 2).map_err(|_| Error::TooManyEntries)?;

        let names: Box<[u8]> = root_path.as_os_str().as_bytes().into();
        let name_len = u16::try_from(names.len()).map_err(|_| Error::TooManyEntries)?;
        let mut top = Node::dir(0, name_len);
        top.size = root.size;
        top.files = u32::try_from(root.files).map_err(|_| Error::TooManyEntries)?;
        top.block = 0;
        let top = Subtree {
            names,
            children: Box::from([top]),
            subtrees: vec![root],
            ..Subtree::default()
        };

        let mut tree = Self::empty(root_path);
        tree.blocks.reserve_exact(capacity);
        tree.parents.reserve_exact(capacity);
        tree.attach(None, top, None)?;
        Ok(tree)
    }

    /// Links `subtree` into the tree breadth-first and returns the ids of the blocks it
    /// wrote. With `replace`, the first block overwrites an existing slot instead of
    /// taking a new one; reused children keep their blocks and are re-parented.
    pub(crate) fn attach(
        &mut self,
        parent: Option<NodeId>,
        subtree: Subtree,
        replace: Option<u32>,
    ) -> Result<Vec<u32>, Error> {
        let mut next = u32::try_from(self.blocks.len()).map_err(|_| Error::TooManyEntries)?;
        let first = if let Some(id) = replace {
            id
        } else {
            next = next.checked_add(1).ok_or(Error::TooManyEntries)?;
            next - 1
        };
        let mut written = Vec::new();
        let mut queue = VecDeque::from([(parent, subtree, first)]);
        while let Some((parent, subtree, id)) = queue.pop_front() {
            let Subtree {
                names,
                children: mut nodes,
                links,
                mut subtrees,
                ..
            } = subtree;
            for (index, node) in (0..).zip(&mut nodes) {
                if node.kind != EntryKind::Dir {
                    continue;
                }
                if node.flags & FLAG_REUSED != 0 {
                    node.flags &= !FLAG_REUSED;
                    if let Some(slot) = self.parents.get_mut(node.block as usize) {
                        *slot = Some(NodeId::new(id, index));
                    }
                    continue;
                }
                let inner = mem::take(&mut subtrees[node.block as usize]);
                if inner.children.is_empty() {
                    node.block = NO_BLOCK;
                } else {
                    node.block = next;
                    queue.push_back((Some(NodeId::new(id, index)), inner, next));
                    next = next.checked_add(1).ok_or(Error::TooManyEntries)?;
                }
            }

            let added = nodes.len();
            let block = Arc::new(Block {
                names,
                nodes,
                links,
            });
            if id as usize == self.blocks.len() {
                self.blocks.push(block);
                self.parents.push(parent);
            } else {
                let slot = self
                    .blocks
                    .get_mut(id as usize)
                    .ok_or(Error::Inconsistent("block slot out of range"))?;
                self.len -= slot.nodes.len();
                *slot = block;
                self.parents[id as usize] = parent;
            }
            self.len += added;
            written.push(id);
        }
        Ok(written)
    }

    #[must_use]
    pub fn root(&self) -> NodeId {
        NodeId { block: 0, index: 0 }
    }

    #[must_use]
    pub fn root_path(&self) -> &Path {
        &self.root_path
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[must_use]
    pub fn node(&self, id: NodeId) -> &Node {
        &self.blocks[id.block()].nodes[id.index()]
    }

    #[must_use]
    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.parents[id.block()]
    }

    #[must_use]
    pub fn name(&self, id: NodeId) -> &OsStr {
        let block = &self.blocks[id.block()];
        OsStr::from_bytes(block.nodes[id.index()].name(&block.names))
    }

    /// Children of `id`, largest first.
    #[must_use]
    pub fn children(&self, id: NodeId) -> impl ExactSizeIterator<Item = NodeId> + '_ {
        let block = self.node(id).block;
        let len = self.blocks.get(block as usize).map_or(0, |block| {
            u32::try_from(block.nodes.len()).unwrap_or(u32::MAX)
        });
        (0..len).map(move |index| NodeId { block, index })
    }

    #[must_use]
    pub fn path(&self, id: NodeId) -> PathBuf {
        let mut components = Vec::new();
        let mut current = id;
        while let Some(parent) = self.parent(current) {
            components.push(self.name(current));
            current = parent;
        }
        let mut path = self.root_path.clone();
        path.extend(components.into_iter().rev());
        path
    }

    /// Directories holding a hard link that no longer exists or now names another file.
    ///
    /// The change journal can report an unlinked hard link under a surviving link's
    /// directory, so recorded links are checked against the filesystem directly.
    pub(crate) fn stale_link_dirs(&self) -> Vec<PathBuf> {
        let mut stale = Vec::new();
        for (block, entry) in self.blocks.iter().enumerate() {
            if entry.links.is_empty() {
                continue;
            }
            let Some(owner) = self.parents[block] else {
                continue;
            };
            let dir = self.path(owner);
            let moved = entry.links.iter().any(|link| {
                let Some(node) = entry.nodes.iter().find(|n| n.name_start == link.name_start)
                else {
                    return true;
                };
                let path = dir.join(OsStr::from_bytes(node.name(&entry.names)));
                match path.symlink_metadata() {
                    Ok(m) => m.ino() != link.ino || i32::try_from(m.dev()).ok() != Some(link.dev),
                    Err(_) => true,
                }
            });
            if moved {
                stale.push(dir);
            }
        }
        stale
    }

    /// The `limit` largest non-directory entries, largest first.
    #[must_use]
    pub fn largest_files(&self, limit: usize) -> Vec<NodeId> {
        let mut heap = BinaryHeap::with_capacity(limit + 1);
        for (block, entry) in (0..).zip(&self.blocks) {
            for (index, node) in (0..).zip(&entry.nodes) {
                if node.kind == EntryKind::Dir {
                    continue;
                }
                heap.push(Reverse((node.size, NodeId { block, index })));
                if heap.len() > limit {
                    heap.pop();
                }
            }
        }
        heap.into_sorted_vec()
            .into_iter()
            .map(|Reverse((_, id))| id)
            .collect()
    }
}

pub(crate) fn by_size_then_name(a: (u64, &[u8]), b: (u64, &[u8])) -> Ordering {
    b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1))
}

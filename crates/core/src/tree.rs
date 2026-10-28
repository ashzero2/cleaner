use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, VecDeque};
use std::ffi::OsStr;
use std::mem;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::error::Error;
use crate::scan::Subtree;
use crate::sys::EntryKind;

const NO_BLOCK: u32 = u32::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId {
    block: u32,
    index: u32,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub(crate) size: u64,
    name_start: u32,
    pub(crate) files: u32,
    pub(crate) block: u32,
    name_len: u16,
    pub(crate) kind: EntryKind,
}

impl Node {
    pub(crate) fn leaf(name_start: u32, name_len: u16, kind: EntryKind, size: u64) -> Self {
        Self {
            size,
            name_start,
            files: 1,
            block: NO_BLOCK,
            name_len,
            kind,
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

#[derive(Debug)]
struct Block {
    parent: Option<NodeId>,
    names: Box<[u8]>,
    nodes: Box<[Node]>,
}

/// An immutable snapshot of a scanned directory tree.
///
/// Each directory owns one block holding its children's names and records, sorted by
/// size, largest first. Blocks are numbered breadth-first, so ordering [`NodeId`]s
/// follows breadth-first order.
#[derive(Debug)]
pub struct Tree {
    root_path: PathBuf,
    blocks: Vec<Block>,
    len: usize,
}

impl Tree {
    pub(crate) fn single_file(root_path: PathBuf, size: u64) -> Result<Self, Error> {
        let names: Box<[u8]> = root_path.as_os_str().as_bytes().into();
        let name_len = u16::try_from(names.len()).map_err(|_| Error::TooManyEntries)?;
        let block = Block {
            parent: None,
            names,
            nodes: Box::from([Node::leaf(0, name_len, EntryKind::File, size)]),
        };
        Ok(Self {
            root_path,
            blocks: vec![block],
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

        let mut blocks = Vec::with_capacity(capacity);
        let mut queue = VecDeque::from([(None, top)]);
        let mut assigned = 1u32;
        while let Some((parent, subtree)) = queue.pop_front() {
            let Subtree {
                names,
                children: mut nodes,
                mut subtrees,
                ..
            } = subtree;
            let block = u32::try_from(blocks.len()).map_err(|_| Error::TooManyEntries)?;
            for (index, node) in (0..).zip(&mut nodes) {
                if node.kind != EntryKind::Dir {
                    continue;
                }
                let inner = mem::take(&mut subtrees[node.block as usize]);
                if inner.children.is_empty() {
                    node.block = NO_BLOCK;
                } else {
                    node.block = assigned;
                    assigned += 1;
                    queue.push_back((Some(NodeId { block, index }), inner));
                }
            }
            blocks.push(Block {
                parent,
                names,
                nodes,
            });
        }
        Ok(Self {
            root_path,
            blocks,
            len,
        })
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
        &self.blocks[id.block as usize].nodes[id.index as usize]
    }

    #[must_use]
    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.blocks[id.block as usize].parent
    }

    #[must_use]
    pub fn name(&self, id: NodeId) -> &OsStr {
        let block = &self.blocks[id.block as usize];
        OsStr::from_bytes(block.nodes[id.index as usize].name(&block.names))
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

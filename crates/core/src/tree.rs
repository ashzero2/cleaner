use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};
use std::ffi::OsStr;
use std::ops::Range;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::error::Error;
use crate::scan::Entry;
use crate::sys::EntryKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(u32);

impl NodeId {
    fn index(self) -> usize {
        self.0 as usize
    }
}

impl TryFrom<usize> for NodeId {
    type Error = Error;

    fn try_from(index: usize) -> Result<Self, Error> {
        u32::try_from(index)
            .map(Self)
            .map_err(|_| Error::TooManyEntries)
    }
}

#[derive(Clone, Debug)]
pub struct Node {
    name: Range<u32>,
    parent: Option<NodeId>,
    children: Range<u32>,
    size: u64,
    files: u64,
    kind: EntryKind,
}

impl Node {
    /// Bytes allocated on disk, with hard-linked files counted once.
    #[must_use]
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Number of non-directory entries in this subtree.
    #[must_use]
    pub fn files(&self) -> u64 {
        self.files
    }

    #[must_use]
    pub fn kind(&self) -> EntryKind {
        self.kind
    }

    #[must_use]
    pub fn parent(&self) -> Option<NodeId> {
        self.parent
    }
}

/// An immutable snapshot of a scanned directory tree.
///
/// Nodes are stored breadth-first in one contiguous array, and each node's children
/// occupy a contiguous range sorted by size, largest first.
#[derive(Debug)]
pub struct Tree {
    root_path: PathBuf,
    nodes: Vec<Node>,
    names: Vec<u8>,
}

impl Tree {
    pub(crate) fn build(root_path: PathBuf, root: Entry) -> Result<Self, Error> {
        let mut tree = Self {
            root_path,
            nodes: Vec::new(),
            names: Vec::new(),
        };
        let mut queue = VecDeque::new();
        tree.push(root, None, &mut queue)?;

        while let Some((parent, mut children)) = queue.pop_front() {
            children.sort_unstable_by(|a, b| b.size.cmp(&a.size).then_with(|| a.name.cmp(&b.name)));
            let start = NodeId::try_from(tree.nodes.len())?.0;
            let end = NodeId::try_from(tree.nodes.len() + children.len())?.0;
            tree.nodes[parent.index()].children = start..end;
            for child in children {
                tree.push(child, Some(parent), &mut queue)?;
            }
        }
        Ok(tree)
    }

    fn push(
        &mut self,
        entry: Entry,
        parent: Option<NodeId>,
        queue: &mut VecDeque<(NodeId, Vec<Entry>)>,
    ) -> Result<(), Error> {
        let id = NodeId::try_from(self.nodes.len())?;
        let start = u32::try_from(self.names.len()).map_err(|_| Error::TooManyEntries)?;
        self.names.extend_from_slice(&entry.name);
        let end = u32::try_from(self.names.len()).map_err(|_| Error::TooManyEntries)?;
        self.nodes.push(Node {
            name: start..end,
            parent,
            children: 0..0,
            size: entry.size,
            files: entry.files,
            kind: entry.kind,
        });
        if !entry.children.is_empty() {
            queue.push_back((id, entry.children));
        }
        Ok(())
    }

    #[must_use]
    pub fn root(&self) -> NodeId {
        NodeId(0)
    }

    #[must_use]
    pub fn root_path(&self) -> &Path {
        &self.root_path
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    #[must_use]
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.index()]
    }

    #[must_use]
    pub fn name(&self, id: NodeId) -> &OsStr {
        let range = &self.node(id).name;
        OsStr::from_bytes(&self.names[range.start as usize..range.end as usize])
    }

    /// Children of `id`, largest first.
    pub fn children(&self, id: NodeId) -> impl ExactSizeIterator<Item = NodeId> + '_ {
        self.node(id).children.clone().map(NodeId)
    }

    #[must_use]
    pub fn path(&self, id: NodeId) -> PathBuf {
        let mut components = Vec::new();
        let mut current = id;
        while let Some(parent) = self.node(current).parent {
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
        for (index, node) in (0..).zip(&self.nodes) {
            if node.kind == EntryKind::Dir {
                continue;
            }
            heap.push(Reverse((node.size, NodeId(index))));
            if heap.len() > limit {
                heap.pop();
            }
        }
        heap.into_sorted_vec()
            .into_iter()
            .map(|Reverse((_, id))| id)
            .collect()
    }
}

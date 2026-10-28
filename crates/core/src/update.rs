//! Brings an existing [`Tree`] up to date by rescanning only the directories that
//! changed, instead of walking everything again.
//!
//! Every inconsistency found while patching is reported as [`Error::Inconsistent`]; the
//! caller then falls back to a full scan, so a bad patch can never leave a wrong tree.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::ffi::OsStr;
use std::io;
use std::mem;
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use rayon::prelude::*;

use crate::error::Error;
use crate::journal::Dirty;
use crate::scan::{Listing, Subtree, Walker};
use crate::sys::{self, EntryKind};
use crate::tree::{Block, FLAG_MULTI, Link, NO_BLOCK, Node, NodeId, Tree, by_size_then_name};

type LinkKey = (i32, u64);

/// What a patch rescanned, so stale unreadable-path reports can be dropped.
#[derive(Debug, Default)]
pub(crate) struct Rescans {
    dirs: Vec<(PathBuf, bool)>,
    replaced: Vec<PathBuf>,
}

impl Rescans {
    pub(crate) fn invalidates(&self, path: &Path) -> bool {
        self.dirs.iter().any(|(dir, recursive)| {
            path == dir
                || if *recursive {
                    path.starts_with(dir)
                } else {
                    path.parent() == Some(dir)
                }
        }) || self.replaced.iter().any(|dir| path.starts_with(dir))
    }
}

pub(crate) fn apply(
    walker: &Walker<'_>,
    tree: Tree,
    dirty: &[Dirty],
) -> Result<(Tree, Rescans), Error> {
    let mut patch = Patch::new(walker, tree);
    let mut queue: VecDeque<Dirty> = dirty.iter().cloned().collect();
    while let Some(item) = queue.pop_front() {
        if walker.is_cancelled() {
            return Err(Error::Cancelled);
        }
        patch.process(&item, &mut queue)?;
    }
    patch.finish()
}

enum Resolved {
    Dir(NodeId),
    Fallback(PathBuf),
}

struct Patch<'a, 'w> {
    walker: &'a Walker<'w>,
    tree: Tree,
    empty: Arc<Block>,
    index: HashMap<LinkKey, Vec<(u32, u32)>>,
    known: HashSet<LinkKey>,
    affected: HashMap<LinkKey, Option<(u64, u32)>>,
    resort: BTreeSet<u32>,
    rescans: Rescans,
    done: HashSet<PathBuf>,
}

impl<'a, 'w> Patch<'a, 'w> {
    fn new(walker: &'a Walker<'w>, tree: Tree) -> Self {
        let mut index: HashMap<LinkKey, Vec<(u32, u32)>> = HashMap::new();
        for (block, entry) in (0..).zip(&tree.blocks) {
            for link in &entry.links {
                index
                    .entry((link.dev, link.ino))
                    .or_default()
                    .push((block, link.name_start));
            }
        }
        Self {
            walker,
            tree,
            empty: Arc::default(),
            known: index.keys().copied().collect(),
            index,
            affected: HashMap::new(),
            resort: BTreeSet::new(),
            rescans: Rescans::default(),
            done: HashSet::new(),
        }
    }

    fn process(&mut self, item: &Dirty, queue: &mut VecDeque<Dirty>) -> Result<(), Error> {
        let root = self.tree.root_path.clone();
        let Ok(relative) = item.path.strip_prefix(&root) else {
            return Ok(());
        };
        if self.done.contains(&item.path) {
            return Ok(());
        }
        match self.resolve(relative) {
            Resolved::Dir(id) => self.rescan(id, &item.path, item.recursive, queue),
            Resolved::Fallback(ancestor) => {
                queue.push_back(Dirty {
                    path: root.join(ancestor),
                    recursive: false,
                });
                Ok(())
            }
        }
    }

    fn resolve(&self, relative: &Path) -> Resolved {
        let mut current = self.tree.root();
        let mut walked = PathBuf::new();
        for component in relative.components() {
            let Component::Normal(name) = component else {
                continue;
            };
            match self.find_child(current, name.as_bytes()) {
                Some(child) => {
                    current = child;
                    walked.push(name);
                }
                None => return Resolved::Fallback(walked),
            }
        }
        if self.tree.node(current).kind == EntryKind::Dir {
            Resolved::Dir(current)
        } else {
            let parent = walked.parent().map(Path::to_path_buf).unwrap_or_default();
            Resolved::Fallback(parent)
        }
    }

    fn find_child(&self, parent: NodeId, name: &[u8]) -> Option<NodeId> {
        let node = self.tree.node(parent);
        if node.kind != EntryKind::Dir || node.block == NO_BLOCK {
            return None;
        }
        let block = self.tree.blocks.get(node.block as usize)?;
        let index = block
            .nodes
            .iter()
            .position(|n| n.name(&block.names) == name)?;
        Some(NodeId::new(node.block, u32::try_from(index).ok()?))
    }

    fn rescan(
        &mut self,
        id: NodeId,
        path: &Path,
        recursive: bool,
        queue: &mut VecDeque<Dirty>,
    ) -> Result<(), Error> {
        self.done.insert(path.to_path_buf());
        let old = self.tree.node(id).clone();
        let old_block_id = old.block;
        self.rescans.dirs.push((path.to_path_buf(), recursive));

        let subtree = if recursive {
            self.free_subtree(old_block_id)?;
            self.rescans.replaced.push(path.to_path_buf());
            self.walker.walk_dir(path)
        } else {
            let listing = match self.walker.read_dir(path) {
                Ok(listing) => listing,
                Err(error) if sys::is_gone(&error) => return self.vanished(id, path, queue),
                Err(error) => {
                    self.walker.report(path.to_path_buf(), error);
                    Listing::empty()
                }
            };
            self.rebuild(path, old_block_id, listing)?
        };

        let new_size = subtree.size;
        let new_files = u32::try_from(subtree.files).map_err(|_| Error::TooManyEntries)?;
        let reuse_slot = (!recursive && old_block_id != NO_BLOCK).then_some(old_block_id);
        let new_block = if subtree.children.is_empty() {
            if let Some(slot) = reuse_slot {
                self.drop_block(slot)?;
            }
            NO_BLOCK
        } else {
            let written = self.tree.attach(Some(id), subtree, reuse_slot)?;
            for &block in &written {
                self.register(block);
            }
            *written
                .first()
                .ok_or(Error::Inconsistent("attach wrote no blocks"))?
        };

        let delta_size = signed(new_size)? - signed(old.size)?;
        let delta_files = i64::from(new_files) - i64::from(old.files);
        let block = Arc::make_mut(&mut self.tree.blocks[id.block()]);
        let node = block
            .nodes
            .get_mut(id.index())
            .ok_or(Error::Inconsistent("directory node missing"))?;
        node.size = new_size;
        node.files = new_files;
        node.block = new_block;
        self.resort.insert(id.block);
        if let Some(parent) = self.tree.parents[id.block()] {
            self.bump(parent, delta_size, delta_files)?;
        }
        Ok(())
    }

    /// Builds the replacement for a directory from a fresh listing: unchanged
    /// subdirectories keep their existing subtrees, new ones are scanned.
    fn rebuild(
        &mut self,
        path: &Path,
        old_block_id: u32,
        listing: Listing,
    ) -> Result<Subtree, Error> {
        let Listing {
            names,
            mut nodes,
            links,
            subdirs,
        } = listing;
        let old_block = (old_block_id != NO_BLOCK)
            .then(|| self.tree.blocks.get(old_block_id as usize).map(Arc::clone))
            .flatten();

        let mut kept = HashSet::new();
        let mut fresh = Vec::new();
        if let Some(old) = &old_block {
            let old_dirs: HashMap<&[u8], usize> = old
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, node)| node.kind == EntryKind::Dir)
                .map(|(index, node)| (node.name(&old.names), index))
                .collect();
            for &index in &subdirs {
                let node = &nodes[index];
                match old_dirs.get(node.name(&names)) {
                    Some(&position) => {
                        nodes[index] =
                            Node::reuse(node.name_start, node.name_len, &old.nodes[position]);
                        kept.insert(position);
                    }
                    None => fresh.push(index),
                }
            }
        } else {
            fresh.extend(&subdirs);
        }

        if let Some(old) = &old_block {
            for (position, node) in old.nodes.iter().enumerate() {
                if node.kind != EntryKind::Dir || kept.contains(&position) {
                    continue;
                }
                self.rescans
                    .replaced
                    .push(path.join(OsStr::from_bytes(node.name(&old.names))));
                if node.block != NO_BLOCK {
                    self.free_subtree(node.block)?;
                }
            }
            self.unregister(old_block_id, &old.links);
        }

        let walker = self.walker;
        let subtrees: Vec<Subtree> = fresh
            .par_iter()
            .map(|&index| walker.walk_dir(&path.join(OsStr::from_bytes(nodes[index].name(&names)))))
            .collect();
        for &index in &fresh {
            self.rescans
                .replaced
                .push(path.join(OsStr::from_bytes(nodes[index].name(&names))));
        }
        Ok(Subtree::new(names, nodes, links, subtrees))
    }

    /// The directory no longer exists; its parent's listing will drop it.
    fn vanished(
        &mut self,
        id: NodeId,
        path: &Path,
        queue: &mut VecDeque<Dirty>,
    ) -> Result<(), Error> {
        match (self.tree.parent(id), path.parent()) {
            (Some(_), Some(parent)) => {
                queue.push_back(Dirty {
                    path: parent.to_path_buf(),
                    recursive: false,
                });
                Ok(())
            }
            _ => Err(Error::Root {
                path: path.to_path_buf(),
                source: io::Error::from(io::ErrorKind::NotFound),
            }),
        }
    }

    fn free_subtree(&mut self, block: u32) -> Result<(), Error> {
        let mut stack = vec![block];
        while let Some(id) = stack.pop() {
            if id == NO_BLOCK {
                continue;
            }
            let slot = self
                .tree
                .blocks
                .get_mut(id as usize)
                .ok_or(Error::Inconsistent("block id out of range"))?;
            let block = mem::replace(slot, Arc::clone(&self.empty));
            stack.extend(
                block
                    .nodes
                    .iter()
                    .filter(|node| node.kind == EntryKind::Dir)
                    .map(|node| node.block),
            );
            self.tree.len = self
                .tree
                .len
                .checked_sub(block.nodes.len())
                .ok_or(Error::Inconsistent("node count underflow"))?;
            self.unregister(id, &block.links);
            self.tree.parents[id as usize] = None;
        }
        Ok(())
    }

    fn drop_block(&mut self, id: u32) -> Result<(), Error> {
        let slot = self
            .tree
            .blocks
            .get_mut(id as usize)
            .ok_or(Error::Inconsistent("block id out of range"))?;
        let block = mem::replace(slot, Arc::clone(&self.empty));
        self.tree.len = self
            .tree
            .len
            .checked_sub(block.nodes.len())
            .ok_or(Error::Inconsistent("node count underflow"))?;
        self.tree.parents[id as usize] = None;
        Ok(())
    }

    fn register(&mut self, block: u32) {
        let entry = Arc::clone(&self.tree.blocks[block as usize]);
        for link in &entry.links {
            let key = (link.dev, link.ino);
            self.index
                .entry(key)
                .or_default()
                .push((block, link.name_start));
            self.affected.insert(key, Some((link.alloc, link.nlink)));
        }
    }

    fn unregister(&mut self, block: u32, links: &[Link]) {
        for link in links {
            let key = (link.dev, link.ino);
            if let Some(locations) = self.index.get_mut(&key) {
                locations.retain(|&(owner, _)| owner != block);
            }
            self.affected.entry(key).or_insert(None);
        }
    }

    /// Adds a size and file-count change to `start` and every ancestor above it.
    fn bump(&mut self, start: NodeId, size: i64, files: i64) -> Result<(), Error> {
        let mut current = Some(start);
        while let Some(id) = current {
            let slot = self
                .tree
                .blocks
                .get_mut(id.block())
                .ok_or(Error::Inconsistent("block id out of range"))?;
            let node = Arc::make_mut(slot)
                .nodes
                .get_mut(id.index())
                .ok_or(Error::Inconsistent("node index out of range"))?;
            node.size = node
                .size
                .checked_add_signed(size)
                .ok_or(Error::Inconsistent("size underflow"))?;
            node.files = u32::try_from(i64::from(node.files) + files)
                .map_err(|_| Error::Inconsistent("file count underflow"))?;
            self.resort.insert(id.block);
            current = self.tree.parents[id.block()];
        }
        Ok(())
    }

    /// Re-establishes "each hard-linked file is charged exactly once" for every inode
    /// whose links were added or removed.
    fn settle(&mut self) -> Result<(), Error> {
        for (key, alloc_info) in mem::take(&mut self.affected) {
            let locations = self.index.get(&key).cloned().unwrap_or_default();
            if locations.is_empty() {
                self.index.remove(&key);
                continue;
            }

            let mut found = Vec::with_capacity(locations.len());
            let mut newest = 0;
            for &(block, name_start) in &locations {
                let entry = self
                    .tree
                    .blocks
                    .get(block as usize)
                    .ok_or(Error::Inconsistent("link block missing"))?;
                let node = entry
                    .nodes
                    .iter()
                    .position(|n| n.name_start == name_start && n.flags & FLAG_MULTI != 0)
                    .ok_or(Error::Inconsistent("linked node missing"))?;
                let link = entry
                    .links
                    .iter()
                    .find(|l| l.name_start == name_start)
                    .ok_or(Error::Inconsistent("link entry missing"))?;
                newest = newest.max(link.alloc);
                found.push((block, node, name_start));
            }
            found.sort_unstable();
            let alloc = alloc_info.map_or(newest, |(alloc, _)| alloc);
            if let Some((_, nlink)) = alloc_info
                && !self.known.contains(&key)
                && nlink as usize > locations.len()
            {
                return Err(Error::Inconsistent(
                    "hard link to a file not tracked as linked",
                ));
            }

            let keeper = found
                .iter()
                .position(|&(block, node, _)| self.tree.blocks[block as usize].nodes[node].size > 0)
                .unwrap_or(0);
            for (position, &(block, node, name_start)) in found.iter().enumerate() {
                let target = if position == keeper { alloc } else { 0 };
                let current = self.tree.blocks[block as usize].nodes[node].size;
                let index = u32::try_from(node).map_err(|_| Error::TooManyEntries)?;
                if current != target {
                    self.bump(
                        NodeId::new(block, index),
                        signed(target)? - signed(current)?,
                        0,
                    )?;
                }
                let entry = Arc::make_mut(&mut self.tree.blocks[block as usize]);
                if let Some(link) = entry.links.iter_mut().find(|l| l.name_start == name_start) {
                    link.alloc = alloc;
                }
            }
        }
        Ok(())
    }

    fn finish(mut self) -> Result<(Tree, Rescans), Error> {
        self.settle()?;
        for block in mem::take(&mut self.resort) {
            if Arc::ptr_eq(&self.tree.blocks[block as usize], &self.empty) {
                continue;
            }
            let entry = Arc::make_mut(&mut self.tree.blocks[block as usize]);
            let Block { names, nodes, .. } = entry;
            nodes.sort_unstable_by(|a, b| {
                by_size_then_name((a.size, a.name(names)), (b.size, b.name(names)))
            });
            for (index, node) in (0..).zip(nodes.iter()) {
                if node.kind == EntryKind::Dir && node.block != NO_BLOCK {
                    let slot = self
                        .tree
                        .parents
                        .get_mut(node.block as usize)
                        .ok_or(Error::Inconsistent("child block out of range"))?;
                    *slot = Some(NodeId::new(block, index));
                }
            }
        }
        Ok((self.tree, self.rescans))
    }
}

fn signed(value: u64) -> Result<i64, Error> {
    i64::try_from(value).map_err(|_| Error::Inconsistent("size out of range"))
}

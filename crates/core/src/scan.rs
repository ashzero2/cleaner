use std::cell::RefCell;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use rayon::prelude::*;

use crate::error::{Error, ScanIssue};
use crate::sys::{Attrs, DirHandle, EntryKind};
use crate::tree::{Node, Tree, by_size_then_name};

const BULK_BUFFER_SIZE: usize = 256 * 1024;
const SCRATCH_RETAIN: usize = 1 << 20;
const HARD_LINK_SHARDS: usize = 64;
const WORKER_STACK_SIZE: usize = 8 * 1024 * 1024;

struct Scratch {
    bulk: Vec<u8>,
    names: Vec<u8>,
    nodes: Vec<Node>,
}

thread_local! {
    static SCRATCH: RefCell<Scratch> = RefCell::new(Scratch {
        bulk: vec![0; BULK_BUFFER_SIZE],
        names: Vec::new(),
        nodes: Vec::new(),
    });
}

/// Live counters shared between a running scan and any observers.
#[derive(Debug, Default)]
pub struct Progress {
    files: AtomicU64,
    dirs: AtomicU64,
    bytes: AtomicU64,
    issues: AtomicU64,
    cancelled: AtomicBool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProgressSnapshot {
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    pub issues: u64,
}

impl Progress {
    #[must_use]
    pub fn snapshot(&self) -> ProgressSnapshot {
        ProgressSnapshot {
            files: self.files.load(Ordering::Relaxed),
            dirs: self.dirs.load(Ordering::Relaxed),
            bytes: self.bytes.load(Ordering::Relaxed),
            issues: self.issues.load(Ordering::Relaxed),
        }
    }

    /// Requests that the running scan stop; [`Scanner::run`] then fails with [`Error::Cancelled`].
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

#[derive(Debug, Clone)]
pub struct Scanner {
    root: PathBuf,
    threads: usize,
    cross_devices: bool,
    progress: Arc<Progress>,
}

impl Scanner {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            threads: 0,
            cross_devices: false,
            progress: Arc::default(),
        }
    }

    /// Number of worker threads. Zero uses one per logical CPU.
    #[must_use]
    pub fn threads(mut self, threads: usize) -> Self {
        self.threads = threads;
        self
    }

    /// Descend into directories that live on a different volume than the root.
    #[must_use]
    pub fn cross_devices(mut self, enabled: bool) -> Self {
        self.cross_devices = enabled;
        self
    }

    #[must_use]
    pub fn progress(&self) -> Arc<Progress> {
        Arc::clone(&self.progress)
    }

    /// Walks the tree and returns it together with every path that could not be read.
    ///
    /// # Errors
    ///
    /// Fails if the root cannot be accessed, the thread pool cannot start, the tree
    /// holds more entries than can be indexed, or the scan was cancelled.
    pub fn run(&self) -> Result<(Tree, Vec<ScanIssue>), Error> {
        let root = fs::canonicalize(&self.root).map_err(|source| Error::Root {
            path: self.root.clone(),
            source,
        })?;
        let metadata = fs::symlink_metadata(&root).map_err(|source| Error::Root {
            path: root.clone(),
            source,
        })?;

        if !metadata.is_dir() {
            let size = metadata.blocks() * 512;
            self.progress.files.fetch_add(1, Ordering::Relaxed);
            self.progress.bytes.fetch_add(size, Ordering::Relaxed);
            return Ok((Tree::single_file(root, size)?, Vec::new()));
        }

        let walker = Walker {
            device: (!self.cross_devices).then(|| metadata.dev()),
            progress: &self.progress,
            hard_links: HardLinks::default(),
            issues: Mutex::default(),
        };
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(self.threads)
            .stack_size(WORKER_STACK_SIZE)
            .thread_name(|i| format!("clrs-scan-{i}"))
            .build()?;
        let subtree = pool.install(|| walker.walk_dir(&root));
        if self.progress.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let issues = walker
            .issues
            .into_inner()
            .unwrap_or_else(PoisonError::into_inner);
        Ok((Tree::build(root, subtree)?, issues))
    }
}

#[derive(Debug, Default)]
pub(crate) struct Subtree {
    pub(crate) size: u64,
    pub(crate) files: u64,
    pub(crate) nodes: u64,
    pub(crate) dirs: u64,
    pub(crate) names: Box<[u8]>,
    pub(crate) children: Box<[Node]>,
    pub(crate) subtrees: Vec<Subtree>,
}

impl Subtree {
    fn new(names: Box<[u8]>, mut children: Box<[Node]>, subtrees: Vec<Subtree>) -> Self {
        let mut local = 0;
        let mut subtree = Self {
            nodes: children.len() as u64,
            ..Self::default()
        };
        for child in &mut *children {
            if child.kind == EntryKind::Dir {
                let inner = &subtrees[local as usize];
                child.size = inner.size;
                child.files = u32::try_from(inner.files).unwrap_or(u32::MAX);
                child.block = local;
                local += 1;
                subtree.files += inner.files;
                subtree.nodes += inner.nodes;
                subtree.dirs += inner.dirs + 1;
            } else {
                subtree.files += 1;
            }
            subtree.size += child.size;
        }
        children.sort_unstable_by(|a, b| {
            by_size_then_name((a.size, a.name(&names)), (b.size, b.name(&names)))
        });
        subtree.names = names;
        subtree.children = children;
        subtree.subtrees = subtrees;
        subtree
    }
}

fn pack_name(names: &mut Vec<u8>, name: &[u8]) -> Option<(u32, u16)> {
    let start = u32::try_from(names.len()).ok()?;
    let len = u16::try_from(name.len()).ok()?;
    names.extend_from_slice(name);
    Some((start, len))
}

struct Walker<'a> {
    device: Option<u64>,
    progress: &'a Progress,
    hard_links: HardLinks,
    issues: Mutex<Vec<ScanIssue>>,
}

impl Walker<'_> {
    fn walk_dir(&self, path: &Path) -> Subtree {
        if self.progress.is_cancelled() {
            return Subtree::default();
        }
        let handle = match DirHandle::open(path) {
            Ok(handle) => handle,
            Err(error) => {
                self.report(path.to_path_buf(), error);
                return Subtree::default();
            }
        };

        let mut subdirs: Vec<usize> = Vec::new();
        let (result, names, children) = SCRATCH.with_borrow_mut(|scratch| {
            let Scratch { bulk, names, nodes } = scratch;
            let result = handle.read_entries(bulk, |child, attrs| {
                let attrs = match attrs {
                    Ok(attrs) => attrs,
                    Err(error) => {
                        self.report(path.join(OsStr::from_bytes(child)), error);
                        return;
                    }
                };
                let is_dir = attrs.kind == EntryKind::Dir;
                if is_dir && !self.same_device(attrs.dev) {
                    return;
                }
                let Some((start, len)) = pack_name(names, child) else {
                    let error = io::Error::new(io::ErrorKind::InvalidData, "name too long");
                    self.report(path.join(OsStr::from_bytes(child)), error);
                    return;
                };
                if is_dir {
                    subdirs.push(nodes.len());
                    nodes.push(Node::dir(start, len));
                } else {
                    nodes.push(Node::leaf(start, len, attrs.kind, self.charge(&attrs)));
                }
            });
            let exact: (Box<[u8]>, Box<[Node]>) =
                (names.as_slice().into(), nodes.as_slice().into());
            names.clear();
            names.shrink_to(SCRATCH_RETAIN);
            nodes.clear();
            nodes.shrink_to(SCRATCH_RETAIN / size_of::<Node>());
            (result, exact.0, exact.1)
        });
        drop(handle);
        if let Err(error) = result {
            self.report(path.to_path_buf(), error);
        }
        self.progress.dirs.fetch_add(1, Ordering::Relaxed);

        let subtrees = subdirs
            .into_par_iter()
            .map(|index| {
                let name = children[index].name(&names);
                self.walk_dir(&path.join(OsStr::from_bytes(name)))
            })
            .collect();
        Subtree::new(names, children, subtrees)
    }

    fn same_device(&self, dev: i32) -> bool {
        self.device
            .is_none_or(|root| u64::from(dev.cast_unsigned()) == root)
    }

    fn charge(&self, attrs: &Attrs) -> u64 {
        let first_link = attrs.links <= 1 || self.hard_links.insert(attrs.dev, attrs.inode);
        let size = if first_link { attrs.allocated } else { 0 };
        self.progress.files.fetch_add(1, Ordering::Relaxed);
        self.progress.bytes.fetch_add(size, Ordering::Relaxed);
        size
    }

    fn report(&self, path: PathBuf, error: io::Error) {
        self.progress.issues.fetch_add(1, Ordering::Relaxed);
        self.issues
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(ScanIssue { path, error });
    }
}

/// Tracks `(device, inode)` pairs of multiply-linked files so each is counted once.
type InodeSet = HashSet<(i32, u64)>;

struct HardLinks {
    shards: Box<[Mutex<InodeSet>]>,
}

impl Default for HardLinks {
    fn default() -> Self {
        Self {
            shards: (0..HARD_LINK_SHARDS).map(|_| Mutex::default()).collect(),
        }
    }
}

impl HardLinks {
    fn insert(&self, dev: i32, inode: u64) -> bool {
        let shard = usize::try_from(inode).unwrap_or_default() % self.shards.len();
        self.shards[shard]
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert((dev, inode))
    }
}

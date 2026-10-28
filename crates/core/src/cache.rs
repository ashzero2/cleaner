//! Keeps scan results on disk and brings them up to date from the `FSEvents` journal, so a
//! repeat scan only has to look at what changed.
//!
//! The cache is only ever an optimisation: any failure to load, validate, replay or patch
//! a snapshot silently falls back to a full scan, and a snapshot is also discarded when
//! it is too old for the journal to be trusted.

use std::fs::{self, File, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::error::{Error, ScanIssue};
use crate::journal::{self, Dirty, Volume};
use crate::scan::Scanner;
use crate::snapshot::{self, Meta};
use crate::sys::fsevents;
use crate::sys::{self, DirHandle};
use crate::tree::Tree;

const MAX_JOURNAL_AGE: Duration = Duration::from_secs(3 * 24 * 60 * 60);
const MAX_SWEEP_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Where a result came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// A full scan of the filesystem.
    Scan,
    /// A saved snapshot, brought up to date by rescanning `changed_dirs` directories.
    Cache { changed_dirs: usize },
}

#[derive(Debug)]
pub struct Cache {
    dir: PathBuf,
}

#[derive(Debug)]
pub struct Cached {
    pub tree: Tree,
    pub issues: Vec<ScanIssue>,
    pub source: Source,
    pending: Option<Pending>,
}

#[derive(Debug)]
struct Pending {
    path: PathBuf,
    cross_devices: bool,
    uuid: [u8; 16],
    event_id: u64,
    full_scan_at: u64,
}

impl Cache {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Scans `scanner`'s root, reusing and refreshing a saved snapshot when it is still
    /// trustworthy. With `refresh`, the snapshot is ignored and replaced.
    ///
    /// Nothing is written until [`Cached::save`] is called.
    ///
    /// # Errors
    ///
    /// Fails only when a full scan fails or the scan is cancelled; cache problems fall
    /// back to a full scan.
    pub fn scan(&self, scanner: &Scanner, refresh: bool) -> Result<Cached, Error> {
        let root = scanner.canonical_root()?;
        let volume = (root.is_dir() && !scanner.crosses_devices())
            .then(|| Volume::of(&root).ok())
            .flatten();
        let Some(volume) = volume else {
            return plain(scanner);
        };

        let path = self.snapshot_path(&root);
        let checkpoint = fsevents::current_event_id();
        if !refresh && let Some(cached) = from_snapshot(scanner, &volume, &root, &path)? {
            return Ok(cached);
        }

        let (tree, issues) = scanner.run()?;
        Ok(Cached {
            tree,
            issues,
            source: Source::Scan,
            pending: Some(Pending {
                path,
                cross_devices: false,
                uuid: volume.uuid(),
                event_id: checkpoint,
                full_scan_at: now(),
            }),
        })
    }

    fn snapshot_path(&self, root: &Path) -> PathBuf {
        use std::os::unix::ffi::OsStrExt;
        let hash = root
            .as_os_str()
            .as_bytes()
            .iter()
            .fold(0xcbf2_9ce4_8422_2325_u64, |hash, &byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
            });
        self.dir.join(format!("{hash:016x}.snap"))
    }
}

fn from_snapshot(
    scanner: &Scanner,
    volume: &Volume,
    root: &Path,
    path: &Path,
) -> Result<Option<Cached>, Error> {
    let Ok((tree, meta)) = snapshot::load(path) else {
        return Ok(None);
    };
    let current = now();
    let usable = meta.root == root
        && !meta.cross_devices
        && meta.uuid == volume.uuid()
        && current.saturating_sub(meta.saved_at) <= MAX_JOURNAL_AGE.as_secs()
        && current.saturating_sub(meta.full_scan_at) <= MAX_SWEEP_AGE.as_secs();
    if !usable {
        return Ok(None);
    }
    let Ok(changes) = volume.changes_since(meta.event_id) else {
        return Ok(None);
    };
    if changes.rescan_all {
        return Ok(None);
    }

    let issues: Vec<ScanIssue> = meta
        .issues
        .iter()
        .map(|(path, errno)| ScanIssue {
            path: path.clone(),
            error: io::Error::from_raw_os_error(*errno),
        })
        .collect();
    let mut dirty = changes.dirty;
    dirty.extend(tree.stale_link_dirs().into_iter().map(|path| Dirty {
        path,
        recursive: false,
    }));
    dirty.extend(
        issues
            .iter()
            .filter(|issue| DirHandle::open(&issue.path).is_ok())
            .map(|issue| Dirty {
                path: issue.path.clone(),
                recursive: false,
            }),
    );
    let dirty = journal::normalize(dirty);

    let pending = Pending {
        path: path.to_path_buf(),
        cross_devices: false,
        uuid: meta.uuid,
        event_id: changes.checkpoint,
        full_scan_at: meta.full_scan_at,
    };
    if dirty.is_empty() {
        return Ok(Some(Cached {
            tree,
            issues,
            source: Source::Cache { changed_dirs: 0 },
            pending: None,
        }));
    }
    match scanner.update(tree, &dirty, issues) {
        Ok((tree, issues)) => Ok(Some(Cached {
            tree,
            issues,
            source: Source::Cache {
                changed_dirs: dirty.len(),
            },
            pending: Some(pending),
        })),
        Err(Error::Cancelled) => Err(Error::Cancelled),
        Err(_) => Ok(None),
    }
}

fn plain(scanner: &Scanner) -> Result<Cached, Error> {
    let (tree, issues) = scanner.run()?;
    Ok(Cached {
        tree,
        issues,
        source: Source::Scan,
        pending: None,
    })
}

impl Cached {
    /// Wraps a result that should not be saved.
    #[must_use]
    pub fn uncached(tree: Tree, issues: Vec<ScanIssue>) -> Self {
        Self {
            tree,
            issues,
            source: Source::Scan,
            pending: None,
        }
    }

    /// Writes the snapshot if this result differs from what is on disk. Another process
    /// already saving the same snapshot is not an error; this one simply skips.
    ///
    /// # Errors
    ///
    /// Fails if the cache directory cannot be written.
    pub fn save(&self) -> Result<(), Error> {
        let Some(pending) = &self.pending else {
            return Ok(());
        };
        let lock_path = pending.path.with_extension("lock");
        if let Some(dir) = lock_path.parent() {
            fs::create_dir_all(dir).map_err(|source| Error::CacheIo {
                path: dir.to_path_buf(),
                source,
            })?;
        }
        let lock = File::create(&lock_path).map_err(|source| Error::CacheIo {
            path: lock_path.clone(),
            source,
        })?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Ok(()),
            Err(TryLockError::Error(source)) => {
                return Err(Error::CacheIo {
                    path: lock_path,
                    source,
                });
            }
        }

        let meta = Meta {
            root: self.tree.root_path.clone(),
            cross_devices: pending.cross_devices,
            uuid: pending.uuid,
            event_id: pending.event_id,
            saved_at: now(),
            full_scan_at: pending.full_scan_at,
            issues: self
                .issues
                .iter()
                .map(|issue| {
                    (
                        issue.path.clone(),
                        issue.error.raw_os_error().unwrap_or(sys::EIO),
                    )
                })
                .collect(),
        };
        snapshot::sweep_stale_temp_files(&pending.path);
        snapshot::save(&pending.path, &self.tree, &meta)
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

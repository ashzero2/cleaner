use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::Error;
use crate::sys::fsevents::{
    self, HISTORY_DONE, IDS_WRAPPED, KERNEL_DROPPED, MOUNT, MUST_SCAN_SUBDIRS, ROOT_CHANGED,
    Stream, UNMOUNT, USER_DROPPED,
};
use crate::sys::mount_point;

const DATA_VOLUME: &str = "/System/Volumes/Data";
const LATENCY: Duration = Duration::from_millis(200);
const HISTORY_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Dirty {
    pub(crate) path: PathBuf,
    pub(crate) recursive: bool,
}

#[derive(Debug, Default)]
pub(crate) struct Changes {
    pub(crate) dirty: Vec<Dirty>,
    pub(crate) rescan_all: bool,
    /// Every event up to this id has been delivered; resume from here next time.
    pub(crate) checkpoint: u64,
}

/// The volume holding a scan root, as seen by the `FSEvents` journal.
#[derive(Debug)]
pub(crate) struct Volume {
    dev: libc::dev_t,
    uuid: [u8; 16],
    mount: PathBuf,
    relative: PathBuf,
    firmlinked: bool,
}

impl Volume {
    pub(crate) fn of(root: &Path) -> Result<Self, Error> {
        let metadata = fs::metadata(root).map_err(|_| Error::Journal("cannot stat the root"))?;
        let dev = libc::dev_t::try_from(metadata.dev())
            .map_err(|_| Error::Journal("device id out of range"))?;
        let uuid =
            fsevents::device_uuid(dev).ok_or(Error::Journal("this volume has no event history"))?;
        let mount = mount_point(root).map_err(|_| Error::Journal("cannot find the mount point"))?;
        let firmlinked = mount == Path::new(DATA_VOLUME);
        let relative = root
            .strip_prefix(&mount)
            .or_else(|_| root.strip_prefix("/"))
            .map_err(|_| Error::Journal("root is outside its volume"))?
            .to_path_buf();
        Ok(Self {
            dev,
            uuid,
            mount,
            relative,
            firmlinked,
        })
    }

    pub(crate) fn uuid(&self) -> [u8; 16] {
        self.uuid
    }

    fn watch_path(&self) -> &OsStr {
        if self.relative.as_os_str().is_empty() {
            OsStr::new("/")
        } else {
            self.relative.as_os_str()
        }
    }

    fn absolute(&self, event_path: &OsStr) -> PathBuf {
        let bytes = event_path.as_bytes();
        let trimmed = bytes.strip_suffix(b"/").unwrap_or(bytes);
        let base = if self.firmlinked {
            Path::new("/")
        } else {
            &self.mount
        };
        base.join(OsStr::from_bytes(trimmed))
    }

    /// Replays the journal after `since` and reports which directories changed.
    ///
    /// # Errors
    ///
    /// Fails if the stream cannot start or the history does not arrive, in which case the
    /// caller must fall back to a full scan.
    pub(crate) fn changes_since(&self, since: u64) -> Result<Changes, Error> {
        if since > fsevents::current_event_id() {
            return Err(Error::Journal("saved event id is newer than the journal"));
        }
        let stream = Stream::start(self.dev, self.watch_path(), since, LATENCY)
            .map_err(|_| Error::Journal("cannot start the event stream"))?;

        let mut pending = HashMap::new();
        let mut rescan_all = false;
        loop {
            let event = stream
                .recv(HISTORY_TIMEOUT)
                .ok_or(Error::Journal("timed out replaying event history"))?;
            if event.flags & HISTORY_DONE != 0 {
                break;
            }
            rescan_all |= self.absorb(&event, &mut pending);
        }
        stream.flush();
        while let Some(event) = stream.try_recv() {
            if event.flags & HISTORY_DONE == 0 {
                rescan_all |= self.absorb(&event, &mut pending);
            }
        }

        let checkpoint = fsevents::current_event_id().max(since);
        Ok(Changes {
            dirty: normalize(
                pending
                    .into_iter()
                    .map(|(path, recursive)| Dirty { path, recursive })
                    .collect(),
            ),
            rescan_all,
            checkpoint,
        })
    }

    /// Records one event; returns whether it invalidates everything known so far.
    fn absorb(&self, event: &fsevents::Event, pending: &mut HashMap<PathBuf, bool>) -> bool {
        let fatal = USER_DROPPED | KERNEL_DROPPED | IDS_WRAPPED | ROOT_CHANGED;
        if event.flags & fatal != 0 {
            return true;
        }
        let recursive = event.flags & (MUST_SCAN_SUBDIRS | MOUNT | UNMOUNT) != 0;
        *pending.entry(self.absolute(&event.path)).or_default() |= recursive;
        false
    }
}

/// Merges duplicates and drops entries already covered by a recursive ancestor, leaving
/// shallow paths first.
pub(crate) fn normalize(dirty: Vec<Dirty>) -> Vec<Dirty> {
    let mut merged: HashMap<PathBuf, bool> = HashMap::new();
    for item in dirty {
        *merged.entry(item.path).or_default() |= item.recursive;
    }
    let mut items: Vec<Dirty> = merged
        .into_iter()
        .map(|(path, recursive)| Dirty { path, recursive })
        .collect();
    items.sort_by(|a, b| {
        a.path
            .components()
            .count()
            .cmp(&b.path.components().count())
            .then_with(|| a.path.cmp(&b.path))
    });

    let mut covered: Vec<PathBuf> = Vec::new();
    items.retain(|item| {
        if covered.iter().any(|root| item.path.starts_with(root)) {
            return false;
        }
        if item.recursive {
            covered.push(item.path.clone());
        }
        true
    });
    items
}

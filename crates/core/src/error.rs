use std::io;
use std::path::PathBuf;
use std::process::ExitStatus;

use crate::sys;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("cannot open {}", path.display())]
    Root {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to start scanner threads")]
    ThreadPool(#[from] rayon::ThreadPoolBuildError),

    #[error("too many entries to index (limit is {})", u32::MAX)]
    TooManyEntries,

    #[error("scan cancelled")]
    Cancelled,

    #[error("cannot read or write cache file {}", path.display())]
    CacheIo {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("corrupt cache snapshot: {0}")]
    CorruptSnapshot(&'static str),

    #[error("change journal unavailable: {0}")]
    Journal(&'static str),

    #[error("cached tree is inconsistent: {0}")]
    Inconsistent(&'static str),

    #[error("invalid cleanup rules")]
    InvalidRules(#[from] toml::de::Error),

    #[error("invalid rule `{id}`: {reason}")]
    InvalidRule { id: String, reason: String },

    #[error("refusing to clean {}: {reason}", path.display())]
    Refused { path: PathBuf, reason: &'static str },

    #[error("cannot move {} to Trash", path.display())]
    Trash {
        path: PathBuf,
        #[source]
        source: trash::Error,
    },

    #[error("cannot run `{command}`")]
    Command {
        command: String,
        #[source]
        source: io::Error,
    },

    #[error("`{command}` failed ({status})")]
    CommandFailed { command: String, status: ExitStatus },
}

/// A path that could not be read during a scan. Scanning continues past these.
#[derive(Debug)]
pub struct ScanIssue {
    pub path: PathBuf,
    pub error: io::Error,
}

impl ScanIssue {
    /// Whether macOS blocked access, which is fixed by granting Full Disk Access.
    #[must_use]
    pub fn needs_full_disk_access(&self) -> bool {
        self.error.raw_os_error() == Some(sys::EPERM)
    }
}

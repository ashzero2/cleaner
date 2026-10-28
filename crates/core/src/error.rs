use std::io;
use std::path::PathBuf;

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
}

/// A path that could not be read during a scan. Scanning continues past these.
#[derive(Debug)]
pub struct ScanIssue {
    pub path: PathBuf,
    pub error: io::Error,
}

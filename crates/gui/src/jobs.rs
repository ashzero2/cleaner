use std::fmt;
use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use clrs_core::{Cache, Cached, Error, Progress, Scanner};

#[derive(Clone, Debug)]
pub(crate) struct ScanRequest {
    pub(crate) root: PathBuf,
    pub(crate) use_cache: bool,
    /// Ignore the saved snapshot and replace it.
    pub(crate) fresh: bool,
    pub(crate) cross_devices: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct Started {
    pub(crate) generation: u64,
    pub(crate) progress: Arc<Progress>,
}

#[derive(Debug)]
pub(crate) struct Completion {
    pub(crate) generation: u64,
    pub(crate) event: Event,
}

#[derive(Debug)]
pub(crate) enum Event {
    Finished {
        cached: Arc<Cached>,
        elapsed: Duration,
    },
    /// The snapshot of a [`Event::Finished`] result reached disk or needed no write.
    Saved,
    SaveFailed(Error),
    Cancelled,
    Failed(Error),
}

#[derive(Debug)]
pub(crate) struct WorkerStopped;

impl fmt::Display for WorkerStopped {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the background scan worker stopped unexpectedly")
    }
}

impl std::error::Error for WorkerStopped {}

#[derive(Debug)]
struct Job {
    generation: u64,
    scanner: Scanner,
    use_cache: bool,
    fresh: bool,
}

#[derive(Debug, Default)]
struct Queue {
    latest: Option<Job>,
    shutdown: bool,
}

#[derive(Debug, Default)]
struct Shared {
    queue: Mutex<Queue>,
    wake: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// One background thread running scans and cache saves one at a time. A new request cancels
/// the running scan and replaces any queued one, so work never accumulates.
///
/// Dropping it cancels the running scan and detaches the thread without waiting for it.
#[derive(Debug)]
pub(crate) struct Jobs {
    shared: Arc<Shared>,
    results: Receiver<Completion>,
    worker: JoinHandle<()>,
    generation: u64,
    active: Option<Arc<Progress>>,
}

/// `CLRS_CACHE_DIR`, else `~/Library/Caches/cl-rs`, matching the CLI.
pub(crate) fn default_cache_dir() -> Option<PathBuf> {
    std::env::var_os("CLRS_CACHE_DIR")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::home_dir().map(|home| home.join("Library/Caches/cl-rs")))
}

impl Jobs {
    /// A `cache_dir` of `None` disables the cache regardless of the request.
    pub(crate) fn new(cache_dir: Option<PathBuf>) -> io::Result<Self> {
        let shared = Arc::new(Shared::default());
        let (sender, results) = mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("clrs-gui-jobs".into())
            .spawn({
                let shared = Arc::clone(&shared);
                move || work(&shared, cache_dir.map(Cache::new).as_ref(), &sender)
            })?;
        Ok(Self {
            shared,
            results,
            worker,
            generation: 0,
            active: None,
        })
    }

    /// Cancels whatever is running or queued and starts `request`.
    ///
    /// # Errors
    ///
    /// Fails if the worker thread has died.
    pub(crate) fn start(&mut self, request: ScanRequest) -> Result<Started, WorkerStopped> {
        if self.worker.is_finished() {
            return Err(WorkerStopped);
        }
        self.cancel();
        self.generation += 1;
        let scanner = Scanner::new(request.root).cross_devices(request.cross_devices);
        let progress = scanner.progress();
        self.active = Some(Arc::clone(&progress));
        self.shared.lock().latest = Some(Job {
            generation: self.generation,
            scanner,
            use_cache: request.use_cache && !request.cross_devices,
            fresh: request.fresh,
        });
        self.shared.wake.notify_one();
        Ok(Started {
            generation: self.generation,
            progress,
        })
    }

    /// Cancels the latest scan; it completes with [`Event::Cancelled`] unless it already finished.
    pub(crate) fn cancel(&self) {
        if let Some(progress) = &self.active {
            progress.cancel();
        }
    }

    /// The most recently started generation, or zero before the first start.
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    /// Never blocks. Completions of superseded generations may still arrive; discard them.
    pub(crate) fn poll(&self) -> Option<Completion> {
        self.results.try_recv().ok()
    }
}

impl Drop for Jobs {
    fn drop(&mut self) {
        self.cancel();
        let mut queue = self.shared.lock();
        queue.shutdown = true;
        queue.latest = None;
        drop(queue);
        self.shared.wake.notify_one();
    }
}

fn work(shared: &Shared, cache: Option<&Cache>, results: &Sender<Completion>) {
    loop {
        let job = {
            let mut queue = shared.lock();
            loop {
                if queue.shutdown {
                    return;
                }
                if let Some(job) = queue.latest.take() {
                    break job;
                }
                queue = shared
                    .wake
                    .wait(queue)
                    .unwrap_or_else(PoisonError::into_inner);
            }
        };
        if !run(shared, cache, results, &job) {
            return;
        }
    }
}

/// Returns `false` once nobody is listening any more.
fn run(shared: &Shared, cache: Option<&Cache>, results: &Sender<Completion>, job: &Job) -> bool {
    let send = |event| {
        results
            .send(Completion {
                generation: job.generation,
                event,
            })
            .is_ok()
    };
    let progress = job.scanner.progress();
    if progress.is_cancelled() {
        return send(Event::Cancelled);
    }

    let started = Instant::now();
    let cache = cache.filter(|_| job.use_cache);
    let scanned = match cache {
        Some(cache) => cache.scan(&job.scanner, job.fresh),
        None => job
            .scanner
            .run()
            .map(|(tree, issues)| Cached::uncached(tree, issues)),
    };
    let cached = match scanned {
        Ok(_) if progress.is_cancelled() => return send(Event::Cancelled),
        Ok(cached) => Arc::new(cached),
        Err(Error::Cancelled) => return send(Event::Cancelled),
        Err(error) => return send(Event::Failed(error)),
    };

    let finished = Event::Finished {
        cached: Arc::clone(&cached),
        elapsed: started.elapsed(),
    };
    if !send(finished) {
        return false;
    }
    if cache.is_none() || shared.lock().shutdown {
        return true;
    }
    match cached.save() {
        Ok(()) => send(Event::Saved),
        Err(error) => send(Event::SaveFailed(error)),
    }
}

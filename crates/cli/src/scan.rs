use std::path::{Path, PathBuf};
use std::process;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clrs_core::{Cache, Cached, Progress, ScanIssue, Scanner, Source, Tree};
use indicatif::{ProgressBar, ProgressStyle};

use crate::cli::ScanArgs;
use crate::report;

const INTERRUPTED: i32 = 130;

pub(crate) struct Scan {
    cached: Cached,
    pub(crate) elapsed: Duration,
}

impl Scan {
    pub(crate) fn tree(&self) -> &Tree {
        &self.cached.tree
    }

    pub(crate) fn issues(&self) -> &[ScanIssue] {
        &self.cached.issues
    }

    pub(crate) fn source(&self) -> Source {
        self.cached.source
    }

    pub(crate) fn needs_full_disk_access(&self) -> bool {
        self.issues().iter().any(ScanIssue::needs_full_disk_access)
    }

    /// Saves the scan for next time. A failure only costs speed, so it is a warning.
    pub(crate) fn save(&self) {
        if let Err(error) = self.cached.save() {
            eprintln!("warning: {}", report::error_chain(&error));
        }
    }
}

pub(crate) fn run(args: &ScanArgs, root: &Path) -> Result<Scan> {
    let scanner = Scanner::new(root)
        .threads(args.threads)
        .cross_devices(args.cross_devices);
    let progress = scanner.progress();
    let scanning = Arc::new(AtomicBool::new(true));
    install_interrupt(&progress, &scanning)?;
    let spinner = (!args.quiet).then(|| spawn_spinner(Arc::clone(&progress)));

    let started = Instant::now();
    let result = match cache_dir(args) {
        Some(dir) if !args.no_cache => Cache::new(dir).scan(&scanner, args.fresh),
        _ => scanner
            .run()
            .map(|(tree, issues)| Cached::uncached(tree, issues)),
    };
    let elapsed = started.elapsed();
    scanning.store(false, Ordering::Relaxed);

    if let Some((bar, handle)) = spinner {
        bar.finish_and_clear();
        handle.join().ok();
    }
    let cached = result?;
    if !args.quiet {
        report::summary(&cached.tree, &cached.issues, elapsed, cached.source);
    }
    Ok(Scan { cached, elapsed })
}

fn cache_dir(args: &ScanArgs) -> Option<PathBuf> {
    args.cache_dir
        .clone()
        .or_else(|| std::env::home_dir().map(|home| home.join("Library/Caches/cl-rs")))
}

fn install_interrupt(progress: &Arc<Progress>, scanning: &Arc<AtomicBool>) -> Result<()> {
    let progress = Arc::clone(progress);
    let scanning = Arc::clone(scanning);
    ctrlc::set_handler(move || {
        if scanning.load(Ordering::Relaxed) && !progress.is_cancelled() {
            progress.cancel();
        } else {
            process::exit(INTERRUPTED);
        }
    })
    .context("cannot install interrupt handler")
}

fn spawn_spinner(progress: Arc<Progress>) -> (ProgressBar, thread::JoinHandle<()>) {
    let bar = ProgressBar::new_spinner().with_style(
        ProgressStyle::with_template("{spinner} {elapsed} {msg}")
            .unwrap_or_else(|_| ProgressStyle::default_spinner()),
    );
    let ticker = bar.clone();
    let handle = thread::spawn(move || {
        while !ticker.is_finished() {
            let snap = progress.snapshot();
            ticker.set_message(format!(
                "{} files · {} dirs · {}",
                report::count(snap.files),
                report::count(snap.dirs),
                report::bytes(snap.bytes),
            ));
            ticker.tick();
            thread::sleep(Duration::from_millis(80));
        }
    });
    (bar, handle)
}

mod cli;
mod report;

use std::process::ExitCode;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use clap::Parser;
use clrs_core::{Progress, ScanIssue, Scanner, Tree};
use indicatif::{ProgressBar, ProgressStyle};

use crate::cli::{Cli, Command, ScanArgs};

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Scan(args) => {
            let (tree, issues) = scan(&args.scan)?;
            report::tree(&tree, args.depth, args.limit);
            report::issues(&issues, args.scan.verbose);
        }
        Command::Top(args) => {
            let (tree, issues) = scan(&args.scan)?;
            report::largest_files(&tree, args.limit);
            report::issues(&issues, args.scan.verbose);
        }
    }
    Ok(())
}

fn scan(args: &ScanArgs) -> Result<(Tree, Vec<ScanIssue>)> {
    let scanner = Scanner::new(&args.path)
        .threads(args.threads)
        .cross_devices(args.cross_devices);
    let spinner = (!args.quiet).then(|| spawn_spinner(scanner.progress()));

    let started = Instant::now();
    let result = scanner.run();
    let elapsed = started.elapsed();

    if let Some((bar, handle)) = spinner {
        bar.finish_and_clear();
        handle.join().ok();
    }
    let (tree, issues) = result?;
    if !args.quiet {
        report::summary(&tree, &issues, elapsed);
    }
    Ok((tree, issues))
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

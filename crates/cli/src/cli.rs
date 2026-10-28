use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(version, about, propagate_version = true)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Show a directory tree sized by disk usage
    Scan(TreeArgs),
    /// List the largest files
    Top(TopArgs),
}

#[derive(Debug, Args)]
pub(crate) struct ScanArgs {
    /// Directory to scan
    #[arg(default_value = ".")]
    pub(crate) path: PathBuf,

    /// Worker threads (0 = one per CPU)
    #[arg(short = 'j', long, default_value_t = 0)]
    pub(crate) threads: usize,

    /// Descend into other mounted volumes
    #[arg(short = 'x', long)]
    pub(crate) cross_devices: bool,

    /// Hide progress and the summary line
    #[arg(short, long)]
    pub(crate) quiet: bool,

    /// List every path that could not be read
    #[arg(short, long)]
    pub(crate) verbose: bool,
}

#[derive(Debug, Args)]
pub(crate) struct TreeArgs {
    #[command(flatten)]
    pub(crate) scan: ScanArgs,

    /// Levels of the tree to display
    #[arg(short, long, default_value_t = 1)]
    pub(crate) depth: usize,

    /// Entries to show per directory
    #[arg(short = 'n', long, default_value_t = 20)]
    pub(crate) limit: usize,
}

#[derive(Debug, Args)]
pub(crate) struct TopArgs {
    #[command(flatten)]
    pub(crate) scan: ScanArgs,

    /// Number of files to list
    #[arg(short = 'n', long, default_value_t = 20)]
    pub(crate) limit: usize,
}

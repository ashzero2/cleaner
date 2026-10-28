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
    /// Find caches, build output and other space that can be reclaimed
    Recommend(RecommendArgs),
    /// Move recommended items to the Trash
    Clean(CleanArgs),
}

#[derive(Debug, Args)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent command-line flags"
)]
pub(crate) struct ScanArgs {
    /// Directory to scan (default: current directory; home for recommend and clean)
    pub(crate) path: Option<PathBuf>,

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

    /// Print machine-readable JSON to stdout
    #[arg(long)]
    pub(crate) json: bool,

    /// Ignore the saved scan and rescan everything, then replace it
    #[arg(long)]
    pub(crate) fresh: bool,

    /// Neither read nor write the saved scan
    #[arg(long, conflicts_with = "fresh")]
    pub(crate) no_cache: bool,

    /// Where saved scans live (default: ~/Library/Caches/cl-rs)
    #[arg(long, env = "CLRS_CACHE_DIR", value_name = "DIR")]
    pub(crate) cache_dir: Option<PathBuf>,
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

#[derive(Debug, Args)]
pub(crate) struct RulesArgs {
    /// Use rules from this TOML file instead of the built-in ones
    #[arg(long, value_name = "FILE")]
    pub(crate) rules: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct RecommendArgs {
    #[command(flatten)]
    pub(crate) scan: ScanArgs,

    #[command(flatten)]
    pub(crate) rules: RulesArgs,
}

#[derive(Debug, Args)]
pub(crate) struct CleanArgs {
    #[command(flatten)]
    pub(crate) scan: ScanArgs,

    #[command(flatten)]
    pub(crate) rules: RulesArgs,

    /// Clean every item marked safe to remove
    #[arg(long)]
    pub(crate) safe: bool,

    /// Clean the items of this rule (repeatable; see `recommend` for ids)
    #[arg(long = "rule", value_name = "ID")]
    pub(crate) rule: Vec<String>,

    /// Skip the confirmation prompt
    #[arg(short, long)]
    pub(crate) yes: bool,

    /// Show what would be cleaned without changing anything
    #[arg(long)]
    pub(crate) dry_run: bool,
}

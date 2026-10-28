mod cli;
mod commands;
mod json;
mod report;
mod scan;

use std::process::ExitCode;

use anyhow::Result;
use clap::Parser;
use clrs_core::Error;

use crate::cli::{Cli, Command};

const INTERRUPTED: u8 = 130;

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) if matches!(error.downcast_ref::<Error>(), Some(Error::Cancelled)) => {
            eprintln!("cancelled");
            ExitCode::from(INTERRUPTED)
        }
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Scan(args) => commands::tree(&args),
        Command::Top(args) => commands::top(&args),
        Command::Recommend(args) => commands::recommend(&args),
        Command::Clean(args) => commands::clean(&args),
    }
}

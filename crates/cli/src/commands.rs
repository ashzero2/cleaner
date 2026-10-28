use std::fs;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clrs_core::{Error, Finding, Outcome, Rules, Safety};

use crate::cli::{CleanArgs, RecommendArgs, RulesArgs, TopArgs, TreeArgs};
use crate::scan;
use crate::{json, report};

pub(crate) struct Cleaned<'a> {
    pub(crate) finding: Finding<'a>,
    pub(crate) result: Result<Outcome, Error>,
}

pub(crate) fn tree(args: &TreeArgs) -> Result<()> {
    let scan = scan::run(
        &args.scan,
        args.scan.path.as_deref().unwrap_or(Path::new(".")),
    )?;
    if args.scan.json {
        json::scan(&scan, args.depth, args.limit)?;
    } else {
        report::tree(scan.tree(), args.depth, args.limit);
        report::issues(scan.issues(), args.scan.verbose);
    }
    scan.save();
    Ok(())
}

pub(crate) fn top(args: &TopArgs) -> Result<()> {
    let scan = scan::run(
        &args.scan,
        args.scan.path.as_deref().unwrap_or(Path::new(".")),
    )?;
    if args.scan.json {
        json::top(&scan, args.limit)?;
    } else {
        report::largest_files(scan.tree(), args.limit);
        report::issues(scan.issues(), args.scan.verbose);
    }
    scan.save();
    Ok(())
}

pub(crate) fn recommend(args: &RecommendArgs) -> Result<()> {
    let home = home()?;
    let rules = load_rules(&args.rules, &home)?;
    let scan = scan::run(&args.scan, args.scan.path.as_deref().unwrap_or(&home))?;
    let findings = rules.find(scan.tree());
    if args.scan.json {
        json::recommend(&scan, &findings)?;
    } else {
        report::recommendations(&findings, &home);
        report::issues(scan.issues(), args.scan.verbose);
    }
    scan.save();
    Ok(())
}

pub(crate) fn clean(args: &CleanArgs) -> Result<()> {
    let home = home()?;
    let rules = load_rules(&args.rules, &home)?;
    validate_selection(args, &rules)?;
    if !args.dry_run && !args.yes && (args.scan.json || !io::stdin().is_terminal()) {
        bail!("refusing to clean without confirmation; pass --yes (or --dry-run to preview)");
    }

    let scan = scan::run(&args.scan, args.scan.path.as_deref().unwrap_or(&home))?;
    scan.save();
    let selected: Vec<Finding<'_>> = rules
        .find(scan.tree())
        .into_iter()
        .filter(|finding| selects(args, finding))
        .collect();

    if selected.is_empty() {
        if args.scan.json {
            json::cleaned(&[])?;
        } else {
            println!("Nothing to clean.");
        }
        return Ok(());
    }
    if args.dry_run {
        if args.scan.json {
            json::plan(&scan, &selected)?;
        } else {
            report::plan(&selected, &home);
        }
        return Ok(());
    }
    if !args.scan.json {
        report::plan(&selected, &home);
    }
    if !args.yes && !confirm(selected.len())? {
        eprintln!("Aborted.");
        return Ok(());
    }

    let results: Vec<Cleaned<'_>> = selected
        .into_iter()
        .map(|finding| Cleaned {
            result: clrs_core::clean(&finding),
            finding,
        })
        .collect();
    if args.scan.json {
        json::cleaned(&results)?;
    } else {
        report::cleaned(&results, &home);
    }

    let failed = results
        .iter()
        .filter(|cleaned| cleaned.result.is_err())
        .count();
    if failed > 0 {
        bail!("{failed} of {} items could not be cleaned", results.len());
    }
    Ok(())
}

fn validate_selection(args: &CleanArgs, rules: &Rules) -> Result<()> {
    if !args.safe && args.rule.is_empty() {
        bail!("choose what to clean: --safe or --rule <ID> (see `cl-rs recommend`)");
    }
    if let Some(unknown) = args
        .rule
        .iter()
        .find(|id| !rules.iter().any(|rule| rule.id == **id))
    {
        bail!("unknown rule `{unknown}` (see `cl-rs recommend`)");
    }
    Ok(())
}

fn selects(args: &CleanArgs, finding: &Finding<'_>) -> bool {
    (args.safe && finding.rule.safety == Safety::Safe) || args.rule.contains(&finding.rule.id)
}

fn confirm(count: usize) -> Result<bool> {
    eprint!("\nProceed with {count} items? [y/N] ");
    io::stderr().flush()?;
    let mut answer = String::new();
    io::stdin().lock().read_line(&mut answer)?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes"))
}

fn home() -> Result<PathBuf> {
    let home = std::env::home_dir().context("cannot determine the home directory")?;
    Ok(fs::canonicalize(&home).unwrap_or(home))
}

fn load_rules(args: &RulesArgs, home: &Path) -> Result<Rules> {
    let rules = match &args.rules {
        Some(path) => {
            let source = fs::read_to_string(path)
                .with_context(|| format!("cannot read {}", path.display()))?;
            Rules::parse(&source, home)
        }
        None => Rules::builtin(home),
    };
    Ok(rules?)
}

use std::error::Error;
use std::path::Path;
use std::time::Duration;

use clrs_core::{EntryKind, Finding, NodeId, Outcome, Safety, ScanIssue, Source, Tree};

use crate::commands::Cleaned;

const BAR_WIDTH: usize = 20;
const ISSUE_PREVIEW: usize = 5;
const RECOMMEND_PREVIEW: usize = 20;
const FULL_DISK_ACCESS_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles";

pub(crate) fn summary(tree: &Tree, issues: &[ScanIssue], elapsed: Duration, source: Source) {
    let root = tree.node(tree.root());
    let origin = match source {
        Source::Scan => "Scanned".to_owned(),
        Source::Cache { changed_dirs: 0 } => "Loaded from cache (no changes):".to_owned(),
        Source::Cache { changed_dirs } => format!(
            "Updated from cache ({} dirs changed):",
            count(changed_dirs as u64)
        ),
    };
    eprintln!(
        "{origin} {} files, {} in {:.2?}{}",
        count(root.files()),
        bytes(root.size()),
        elapsed,
        if issues.is_empty() {
            String::new()
        } else {
            format!(" ({} unreadable)", count(issues.len() as u64))
        },
    );
}

pub(crate) fn tree(tree: &Tree, depth: usize, limit: usize) {
    let root = tree.root();
    println!(
        "{:>10}  {}",
        bytes(tree.node(root).size()),
        tree.root_path().display()
    );
    print_children(tree, root, tree.node(root).size(), depth, limit, "");
}

fn print_children(tree: &Tree, id: NodeId, total: u64, depth: usize, limit: usize, indent: &str) {
    if depth == 0 {
        return;
    }
    let children = tree.children(id);
    let hidden = children.len().saturating_sub(limit);
    let shown: Vec<NodeId> = children.take(limit).collect();

    for (i, &child) in shown.iter().enumerate() {
        let last = i + 1 == shown.len() && hidden == 0;
        let node = tree.node(child);
        let suffix = if node.kind() == EntryKind::Dir {
            "/"
        } else {
            ""
        };
        println!(
            "{:>10}  {} {}{}{}{}",
            bytes(node.size()),
            bar(node.size(), total),
            indent,
            if last { "└─ " } else { "├─ " },
            tree.name(child).to_string_lossy(),
            suffix,
        );
        let next = format!("{indent}{}", if last { "   " } else { "│  " });
        print_children(tree, child, total, depth - 1, limit, &next);
    }

    if hidden > 0 {
        let rest: u64 = tree
            .children(id)
            .skip(limit)
            .map(|c| tree.node(c).size())
            .sum();
        println!(
            "{:>10}  {} {indent}└─ … {hidden} more",
            bytes(rest),
            bar(rest, total),
        );
    }
}

pub(crate) fn largest_files(tree: &Tree, limit: usize) {
    for id in tree.largest_files(limit) {
        println!(
            "{:>10}  {}",
            bytes(tree.node(id).size()),
            tree.path(id).display()
        );
    }
}

pub(crate) fn issues(issues: &[ScanIssue], verbose: bool) {
    if issues.is_empty() {
        return;
    }
    let shown = if verbose { issues.len() } else { ISSUE_PREVIEW };
    eprintln!();
    for issue in issues.iter().take(shown) {
        eprintln!("unreadable: {}: {}", issue.path.display(), issue.error);
    }
    if issues.len() > shown {
        eprintln!(
            "… {} more unreadable paths (use --verbose to list them)",
            issues.len() - shown
        );
    }

    let blocked = issues
        .iter()
        .filter(|issue| issue.needs_full_disk_access())
        .count();
    if blocked > 0 {
        eprintln!(
            "\n{} folders need Full Disk Access, so sizes above are incomplete.\n  \
             1. Open System Settings → Privacy & Security → Full Disk Access\n     \
             (shortcut: open \"{FULL_DISK_ACCESS_URL}\")\n  \
             2. Enable your terminal app, restart it, then scan again.",
            count(blocked as u64),
        );
    }
}

pub(crate) fn recommendations(findings: &[Finding<'_>], home: &Path) {
    if findings.is_empty() {
        println!("Nothing to recommend.");
        return;
    }
    for (title, safety) in [
        ("Safe to remove", Safety::Safe),
        ("Needs review", Safety::Review),
    ] {
        let items: Vec<&Finding<'_>> = findings
            .iter()
            .filter(|finding| finding.rule.safety == safety)
            .collect();
        if items.is_empty() {
            continue;
        }
        let total: u64 = items.iter().map(|finding| finding.size).sum();
        println!("{title}: {} in {} items", bytes(total), items.len());
        print_items(&items[..items.len().min(RECOMMEND_PREVIEW)], home);
        if items.len() > RECOMMEND_PREVIEW {
            println!("  … {} more", items.len() - RECOMMEND_PREVIEW);
        }
        println!();
    }
    println!("Clean with `cl-rs clean --safe` or `cl-rs clean --rule <id>`.");
}

pub(crate) fn plan(findings: &[Finding<'_>], home: &Path) {
    let total: u64 = findings.iter().map(|finding| finding.size).sum();
    println!("Will clean {} in {} items:", bytes(total), findings.len());
    print_items(&findings.iter().collect::<Vec<_>>(), home);
}

fn print_items(items: &[&Finding<'_>], home: &Path) {
    let width = items.iter().map(|f| f.rule.id.len()).max().unwrap_or(0);
    for finding in items {
        let action = match &finding.rule.command {
            Some(argv) => format!("  (runs `{}`)", argv.join(" ")),
            None => String::new(),
        };
        println!(
            "  {:>10}  {:<width$}  {}{action}",
            bytes(finding.size),
            finding.rule.id,
            tilde(&finding.path, home),
        );
    }
}

pub(crate) fn cleaned(results: &[Cleaned<'_>], home: &Path) {
    let mut trashed = 0;
    for cleaned in results {
        let path = tilde(&cleaned.finding.path, home);
        match &cleaned.result {
            Ok(Outcome::Trashed) => {
                trashed += cleaned.finding.size;
                println!("✓ moved to Trash  {path}");
            }
            Ok(Outcome::CommandRan) => println!("✓ cleaned         {path}"),
            Ok(Outcome::AlreadyGone) => println!("– already gone    {path}"),
            Err(error) => println!("✗ failed          {path}: {}", error_chain(error)),
        }
    }
    if trashed > 0 {
        println!(
            "\n{} moved to the Trash. Empty the Trash to free the space.",
            bytes(trashed)
        );
    }
}

pub(crate) fn error_chain(error: &dyn Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}

pub(crate) fn tilde(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

fn bar(size: u64, total: u64) -> String {
    let filled = if total == 0 {
        0
    } else {
        usize::try_from(u128::from(size) * BAR_WIDTH as u128 / u128::from(total))
            .unwrap_or(BAR_WIDTH)
    };
    format!("{}{}", "█".repeat(filled), "░".repeat(BAR_WIDTH - filled))
}

pub(crate) fn bytes(value: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut unit = 0;
    let mut scaled = value;
    while scaled >= 1000 && unit < UNITS.len() - 1 {
        scaled /= 1000;
        unit += 1;
    }
    if unit == 0 {
        return format!("{value} B");
    }
    let divisor = 1000u64.pow(u32::try_from(unit).unwrap_or(0));
    let whole = value / divisor;
    let tenths = value % divisor * 10 / divisor;
    format!("{whole}.{tenths} {}", UNITS[unit])
}

pub(crate) fn count(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

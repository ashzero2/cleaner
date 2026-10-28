use std::time::Duration;

use clrs_core::{EntryKind, NodeId, ScanIssue, Tree};

const BAR_WIDTH: usize = 20;
const ISSUE_PREVIEW: usize = 5;

pub(crate) fn summary(tree: &Tree, issues: &[ScanIssue], elapsed: Duration) {
    let root = tree.node(tree.root());
    eprintln!(
        "Scanned {} files, {} in {:.2?}{}",
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
            "… {} more unreadable paths (use --verbose; grant Full Disk Access for a complete scan)",
            issues.len() - shown
        );
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

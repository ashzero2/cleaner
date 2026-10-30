use std::io::{self, Write};
use std::path::Path;

use anyhow::Result;
use clrs_core::{EntryKind, Finding, NodeId, Outcome, Safety, Source, Tree};
use serde::Serialize;

use crate::commands::Cleaned;
use crate::scan::Scan;

#[derive(Serialize)]
struct Issue {
    path: String,
    error: String,
}

#[derive(Serialize)]
struct Envelope<T> {
    root: String,
    size: u64,
    files: u64,
    elapsed_ms: u64,
    unreadable: Vec<Issue>,
    full_disk_access_needed: bool,
    source: &'static str,
    changed_dirs: Option<usize>,
    #[serde(flatten)]
    data: T,
}

#[derive(Serialize)]
struct TreeNode {
    name: String,
    kind: &'static str,
    size: u64,
    files: u64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    children: Vec<TreeNode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    other: Option<Other>,
}

#[derive(Serialize)]
struct Other {
    count: usize,
    size: u64,
}

#[derive(Serialize)]
struct FileEntry {
    path: String,
    size: u64,
}

#[derive(Serialize)]
struct Item {
    id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    category: String,
    safety: &'static str,
    path: String,
    size: u64,
    files: u64,
    command: Option<Vec<String>>,
}

#[derive(Serialize)]
struct CleanResult {
    #[serde(flatten)]
    item: Item,
    status: &'static str,
    error: Option<String>,
}

pub(crate) fn scan(scan: &Scan, depth: usize, limit: usize) -> Result<()> {
    #[derive(Serialize)]
    struct Data {
        tree: TreeNode,
    }
    let tree = scan.tree();
    let mut root = tree_node(tree, tree.root(), depth, limit);
    root.name = lossy(tree.root_path());
    emit(&envelope(scan, Data { tree: root }))
}

pub(crate) fn top(scan: &Scan, limit: usize) -> Result<()> {
    #[derive(Serialize)]
    struct Data {
        files: Vec<FileEntry>,
    }
    let files = scan
        .tree()
        .largest_files(limit)
        .into_iter()
        .map(|id| FileEntry {
            path: lossy(&scan.tree().path(id)),
            size: scan.tree().node(id).size(),
        })
        .collect();
    emit(&envelope(scan, Data { files }))
}

pub(crate) fn recommend(scan: &Scan, findings: &[Finding<'_>]) -> Result<()> {
    #[derive(Serialize)]
    struct Data {
        safe_bytes: u64,
        review_bytes: u64,
        items: Vec<Item>,
    }
    let total = |safety| {
        findings
            .iter()
            .filter(|finding| finding.rule.safety == safety)
            .map(|finding| finding.size)
            .sum()
    };
    let data = Data {
        safe_bytes: total(Safety::Safe),
        review_bytes: total(Safety::Review),
        items: findings.iter().map(item).collect(),
    };
    emit(&envelope(scan, data))
}

pub(crate) fn plan(scan: &Scan, findings: &[Finding<'_>]) -> Result<()> {
    #[derive(Serialize)]
    struct Data {
        dry_run: bool,
        items: Vec<Item>,
    }
    let data = Data {
        dry_run: true,
        items: findings.iter().map(item).collect(),
    };
    emit(&envelope(scan, data))
}

pub(crate) fn cleaned(results: &[Cleaned<'_>]) -> Result<()> {
    #[derive(Serialize)]
    struct Data {
        dry_run: bool,
        results: Vec<CleanResult>,
    }
    let results = results
        .iter()
        .map(|cleaned| {
            let (status, error) = match &cleaned.result {
                Ok(Outcome::Trashed) => ("trashed", None),
                Ok(Outcome::CommandRan) => ("cleaned", None),
                Ok(Outcome::AlreadyGone) => ("gone", None),
                Err(error) => ("failed", Some(format!("{error:#}"))),
            };
            CleanResult {
                item: item(&cleaned.finding),
                status,
                error,
            }
        })
        .collect();
    emit(&Data {
        dry_run: false,
        results,
    })
}

fn envelope<T>(scan: &Scan, data: T) -> Envelope<T> {
    let root = scan.tree().node(scan.tree().root());
    Envelope {
        root: lossy(scan.tree().root_path()),
        size: root.size(),
        files: root.files(),
        elapsed_ms: u64::try_from(scan.elapsed.as_millis()).unwrap_or(u64::MAX),
        unreadable: scan
            .issues()
            .iter()
            .map(|issue| Issue {
                path: lossy(&issue.path),
                error: issue.error.to_string(),
            })
            .collect(),
        full_disk_access_needed: scan.needs_full_disk_access(),
        source: match scan.source() {
            Source::Scan => "scan",
            Source::Cache { .. } => "cache",
        },
        changed_dirs: match scan.source() {
            Source::Scan => None,
            Source::Cache { changed_dirs } => Some(changed_dirs),
        },
        data,
    }
}

fn tree_node(tree: &Tree, id: NodeId, depth: usize, limit: usize) -> TreeNode {
    let node = tree.node(id);
    let mut out = TreeNode {
        name: tree.name(id).to_string_lossy().into_owned(),
        kind: kind_name(node.kind()),
        size: node.size(),
        files: node.files(),
        children: Vec::new(),
        other: None,
    };
    if depth > 0 {
        let children = tree.children(id);
        let hidden = children.len().saturating_sub(limit);
        out.children = children
            .take(limit)
            .map(|child| tree_node(tree, child, depth - 1, limit))
            .collect();
        if hidden > 0 {
            out.other = Some(Other {
                count: hidden,
                size: tree
                    .children(id)
                    .skip(limit)
                    .map(|child| tree.node(child).size())
                    .sum(),
            });
        }
    }
    out
}

fn item(finding: &Finding<'_>) -> Item {
    Item {
        id: finding.rule.id.clone(),
        name: finding.rule.name.clone(),
        description: finding.rule.description.clone(),
        category: finding.rule.category.clone(),
        safety: match finding.rule.safety {
            Safety::Safe => "safe",
            Safety::Review => "review",
        },
        path: lossy(&finding.path),
        size: finding.size,
        files: finding.files,
        command: finding.rule.command.clone(),
    }
}

fn kind_name(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::File => "file",
        EntryKind::Dir => "dir",
        EntryKind::Symlink => "symlink",
        EntryKind::Other => "other",
    }
}

fn lossy(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn emit(value: &impl Serialize) -> Result<()> {
    let mut out = io::stdout().lock();
    serde_json::to_writer_pretty(&mut out, value)?;
    writeln!(out)?;
    Ok(())
}

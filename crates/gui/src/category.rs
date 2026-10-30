use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt as _;
use std::path::Path;

use clrs_core::{EntryKind, NodeId, Tree};

use crate::recommendations::Suggestions;

/// What kind of data a node holds, which sets its colour everywhere it appears.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Category {
    System,
    Files,
    Cache,
    Code,
    Tools,
    Apps,
    Other,
}

/// How many levels of largest children an unnamed folder looks through.
const INHERIT_DEPTH: usize = 3;

impl Category {
    pub(crate) const ALL: [Self; 7] = [
        Self::System,
        Self::Files,
        Self::Cache,
        Self::Code,
        Self::Tools,
        Self::Apps,
        Self::Other,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::System => "Mac & app data",
            Self::Files => "Your files",
            Self::Cache => "Caches",
            Self::Code => "Code",
            Self::Tools => "Dev tools & models",
            Self::Apps => "Apps",
            Self::Other => "Other",
        }
    }

    fn from_rule(category: &str) -> Self {
        match category {
            "package-caches" | "logs" | "build-output" => Self::Cache,
            "code-projects" => Self::Code,
            "android" | "ios-sdks" | "ai-models" => Self::Tools,
            _ => Self::Other,
        }
    }
}

/// Classifies `id` from names in the scanned tree and the rule that covers it; it never
/// reads the disk. The nearest named ancestor wins, so `~/Library/Caches` is a cache even
/// though `~/Library` is Mac data. Unnamed folders take the category of their largest child.
pub(crate) fn classify(tree: &Tree, id: NodeId, suggestions: &Suggestions) -> Category {
    if let Some(group) = suggestions.covering(tree, id) {
        return Category::from_rule(&group.category);
    }
    let mut current = Some(id);
    while let Some(node) = current {
        if let Some(category) = named(tree, node) {
            return category;
        }
        current = tree.parent(node);
    }
    inherited(tree, id, INHERIT_DEPTH).unwrap_or(Category::Other)
}

fn inherited(tree: &Tree, id: NodeId, depth: usize) -> Option<Category> {
    if depth == 0 || tree.node(id).kind() != EntryKind::Dir {
        return None;
    }
    let child = tree.children(id).next()?;
    named(tree, child).or_else(|| inherited(tree, child, depth - 1))
}

fn named(tree: &Tree, id: NodeId) -> Option<Category> {
    let name = tree.name(id);
    let node = tree.node(id);
    if node.kind() != EntryKind::Dir {
        return file(name);
    }
    if extension(name, &["app"]) || name == "Applications" {
        return Some(Category::Apps);
    }
    let category = match name.as_bytes() {
        b"Caches" | b".cache" | b".npm" | b".gradle" | b".cargo" | b".bun" | b".yarn"
        | b".pnpm-store" | b"pnpm" | b"DerivedData" | b"Logs" | b".m2" | b".nuget"
        | b"__pycache__" | b".next" | b".turbo" => Category::Cache,
        b"Library"
        | b"System"
        | b"private"
        | b"Containers"
        | b"Group Containers"
        | b"Application Support"
        | b"Preferences" => Category::System,
        b"Documents" | b"Desktop" | b"Downloads" | b"Pictures" | b"Movies" | b"Music"
        | b"Public" | b"Mobile Documents" => Category::Files,
        b".android" | b".lmstudio" | b".ollama" | b".sdkman" | b".rustup" | b".vscode"
        | b".cursor" | b".codex" | b".claude" | b"Developer" | b".nvm" | b".pyenv" | b".docker"
        | b".gem" | b".conda" | b".huggingface" | b"Android" => Category::Tools,
        _ if tree.children(id).any(|child| tree.name(child) == ".git") => Category::Code,
        _ => return None,
    };
    Some(category)
}

fn file(name: &OsStr) -> Option<Category> {
    const FILES: [&str; 22] = [
        "jpg", "jpeg", "png", "heic", "gif", "mov", "mp4", "m4v", "mkv", "avi", "mp3", "m4a",
        "wav", "aac", "flac", "pdf", "doc", "docx", "key", "pages", "numbers", "psd",
    ];
    const CACHE: [&str; 2] = ["log", "tmp"];
    if extension(name, &FILES) {
        Some(Category::Files)
    } else if extension(name, &CACHE) {
        Some(Category::Cache)
    } else {
        None
    }
}

fn extension(name: &OsStr, extensions: &[&str]) -> bool {
    Path::new(name).extension().is_some_and(|extension| {
        extensions
            .iter()
            .any(|candidate| extension.eq_ignore_ascii_case(candidate))
    })
}

use std::collections::HashSet;
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

use crate::error::Error;
use crate::sys::EntryKind;
use crate::tree::{NodeId, Tree};

const BUILTIN: &str = include_str!("../rules/default.toml");
const MAX_PARTS: usize = 63;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Safety {
    Safe,
    Review,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    #[serde(default)]
    rule: Vec<RawRule>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRule {
    id: String,
    name: String,
    description: Option<String>,
    category: String,
    safety: Safety,
    paths: Vec<String>,
    #[serde(default)]
    command: Vec<String>,
    requires_sibling: Option<String>,
}

/// A named cleanup target. When `command` is set it is run instead of moving the
/// matched paths to the Trash. When `requires_sibling` is set, a match only counts if
/// an entry with that name sits next to it.
#[derive(Debug)]
pub struct Rule {
    pub id: String,
    pub name: String,
    /// What the target is and what happens after it is cleaned, in plain language.
    pub description: Option<String>,
    pub category: String,
    pub safety: Safety,
    pub paths: Vec<String>,
    pub command: Option<Vec<String>>,
    pub requires_sibling: Option<String>,
    patterns: Vec<Pattern>,
}

impl Rule {
    fn has_required_sibling(&self, tree: &Tree, id: NodeId) -> bool {
        let Some(name) = &self.requires_sibling else {
            return true;
        };
        tree.parent(id).is_some_and(|parent| {
            tree.children(parent)
                .any(|child| tree.name(child) == name.as_str())
        })
    }
}

#[derive(Debug)]
pub struct Finding<'a> {
    pub rule: &'a Rule,
    pub path: PathBuf,
    pub size: u64,
    pub files: u64,
}

#[derive(Debug)]
pub struct Rules {
    rules: Vec<Rule>,
}

impl Rules {
    /// The rules shipped with the application, with `~` expanded to `home`.
    ///
    /// # Errors
    ///
    /// Fails only if the embedded rule file is malformed.
    pub fn builtin(home: &Path) -> Result<Self, Error> {
        Self::parse(BUILTIN, home)
    }

    /// Parses rules from TOML, expanding a leading `~/` in paths to `home`.
    ///
    /// # Errors
    ///
    /// Fails on malformed TOML, duplicate ids, or paths that are not absolute or
    /// home-relative.
    pub fn parse(source: &str, home: &Path) -> Result<Self, Error> {
        let document: Document = toml::from_str(source)?;
        let mut seen = HashSet::new();
        let mut rules = Vec::with_capacity(document.rule.len());
        for raw in document.rule {
            let invalid = |reason: &str| Error::InvalidRule {
                id: raw.id.clone(),
                reason: reason.to_owned(),
            };
            if !seen.insert(raw.id.clone()) {
                return Err(invalid("duplicate id"));
            }
            if raw.paths.is_empty() {
                return Err(invalid("no paths"));
            }
            let patterns = raw
                .paths
                .iter()
                .map(|path| Pattern::parse(path, home))
                .collect::<Result<_, _>>()
                .map_err(|reason| invalid(&reason))?;
            rules.push(Rule {
                id: raw.id,
                name: raw.name,
                description: raw.description,
                category: raw.category,
                safety: raw.safety,
                paths: raw.paths,
                command: (!raw.command.is_empty()).then_some(raw.command),
                requires_sibling: raw.requires_sibling,
                patterns,
            });
        }
        Ok(Self { rules })
    }

    pub fn iter(&self) -> impl Iterator<Item = &Rule> {
        self.rules.iter()
    }

    /// Locates every rule target inside `tree`, largest first. Nested matches are
    /// dropped in favour of the enclosing one, and each path belongs to the first rule
    /// that claims it.
    #[must_use]
    pub fn find(&self, tree: &Tree) -> Vec<Finding<'_>> {
        let mut claimed = HashSet::new();
        let mut hits = Vec::new();
        for rule in &self.rules {
            for pattern in &rule.patterns {
                for id in pattern.search(tree) {
                    if rule.has_required_sibling(tree, id) && claimed.insert(id) {
                        hits.push((rule, id));
                    }
                }
            }
        }

        let mut findings: Vec<_> = hits
            .into_iter()
            .filter(|&(_, id)| !has_claimed_ancestor(tree, id, &claimed))
            .map(|(rule, id)| {
                let node = tree.node(id);
                Finding {
                    rule,
                    path: tree.path(id),
                    size: node.size(),
                    files: node.files(),
                }
            })
            .filter(|finding| finding.size > 0)
            .collect();
        findings.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.path.cmp(&b.path)));
        findings
    }
}

fn has_claimed_ancestor(tree: &Tree, id: NodeId, claimed: &HashSet<NodeId>) -> bool {
    let mut current = id;
    while let Some(parent) = tree.parent(current) {
        if claimed.contains(&parent) {
            return true;
        }
        current = parent;
    }
    false
}

#[derive(Debug)]
enum Part {
    Literal(Box<[u8]>),
    Glob(Box<[u8]>),
    AnyDepth,
}

/// An absolute path pattern matched with a small NFA whose states are bits of a `u64`:
/// bit `i` means "the next path component must match part `i`". `**` spans any number
/// of directories except the inside of `.app` bundles.
#[derive(Debug)]
struct Pattern {
    parts: Vec<Part>,
}

impl Pattern {
    fn parse(raw: &str, home: &Path) -> Result<Self, String> {
        let expanded = if let Some(rest) = raw.strip_prefix("~/") {
            home.join(rest)
        } else if raw.starts_with('/') {
            PathBuf::from(raw)
        } else {
            return Err(format!("path `{raw}` must start with `~/` or `/`"));
        };
        let parts: Vec<Part> = expanded
            .components()
            .filter_map(|component| match component {
                Component::Normal(name) => Some(name.as_bytes()),
                _ => None,
            })
            .map(|name| match name {
                b"**" => Part::AnyDepth,
                _ if name.iter().any(|&b| b == b'*' || b == b'?') => Part::Glob(name.into()),
                _ => Part::Literal(name.into()),
            })
            .collect();
        if parts.is_empty() || parts.len() > MAX_PARTS {
            return Err(format!(
                "path `{raw}` must have 1 to {MAX_PARTS} components"
            ));
        }
        Ok(Self { parts })
    }

    fn closure(&self, mut states: u64) -> u64 {
        for (i, part) in self.parts.iter().enumerate() {
            if states >> i & 1 == 1 && matches!(part, Part::AnyDepth) {
                states |= 1 << (i + 1);
            }
        }
        states
    }

    fn step(&self, states: u64, name: &[u8]) -> u64 {
        let mut next = 0;
        for (i, part) in self.parts.iter().enumerate() {
            if states >> i & 1 == 0 {
                continue;
            }
            match part {
                Part::Literal(literal) if **literal == *name => next |= 1 << (i + 1),
                Part::Glob(glob) if wildcard(glob, name) => next |= 1 << (i + 1),
                Part::AnyDepth if !name.ends_with(b".app") => next |= 1 << i,
                _ => {}
            }
        }
        self.closure(next)
    }

    fn accepts(&self, states: u64) -> bool {
        states >> self.parts.len() & 1 == 1
    }

    fn search(&self, tree: &Tree) -> Vec<NodeId> {
        let mut states = self.closure(1);
        for component in tree.root_path().components() {
            if let Component::Normal(name) = component {
                states = self.step(states, name.as_bytes());
            }
        }

        let mut found = Vec::new();
        let mut stack = vec![(tree.root(), states)];
        while let Some((id, states)) = stack.pop() {
            for child in tree.children(id) {
                let next = self.step(states, tree.name(child).as_bytes());
                if next == 0 {
                    continue;
                }
                if self.accepts(next) {
                    found.push(child);
                } else if tree.node(child).kind() == EntryKind::Dir {
                    stack.push((child, next));
                }
            }
        }
        found
    }
}

fn wildcard(pattern: &[u8], text: &[u8]) -> bool {
    let (mut p, mut t) = (0, 0);
    let mut backtrack = None;
    while t < text.len() {
        match pattern.get(p) {
            Some(b'*') => {
                backtrack = Some((p, t));
                p += 1;
            }
            Some(&c) if c == b'?' || c == text[t] => {
                p += 1;
                t += 1;
            }
            _ => match backtrack {
                Some((star, matched)) => {
                    backtrack = Some((star, matched + 1));
                    p = star + 1;
                    t = matched + 1;
                }
                None => return false,
            },
        }
    }
    pattern[p..].iter().all(|&c| c == b'*')
}

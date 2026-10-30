use std::collections::HashMap;

use clrs_core::{Cached, NodeId, Rules, Safety, Tree};

/// Every scan location one cleanup rule matched.
#[derive(Debug)]
pub(crate) struct Group {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) category: String,
    pub(crate) safety: Safety,
    pub(crate) command: Option<Vec<String>>,
    pub(crate) items: Vec<NodeId>,
    pub(crate) size: u64,
}

/// Cleanup rule matches in one scan, largest group first, with each matched node's group.
#[derive(Debug, Default)]
pub(crate) struct Suggestions {
    pub(crate) groups: Vec<Group>,
    owner: HashMap<NodeId, usize>,
}

impl Suggestions {
    /// The group whose rule matched `id` itself.
    pub(crate) fn of(&self, id: NodeId) -> Option<&Group> {
        self.owner.get(&id).map(|&index| &self.groups[index])
    }

    /// The group whose rule matched `id` or one of its ancestors.
    pub(crate) fn covering(&self, tree: &Tree, id: NodeId) -> Option<&Group> {
        let mut current = Some(id);
        while let Some(node) = current {
            if let Some(group) = self.of(node) {
                return Some(group);
            }
            current = tree.parent(node);
        }
        None
    }

    /// The group with the most bytes matched inside `id`, when they make up at least a
    /// quarter of it: worth pointing out on a folder that only contains matches.
    pub(crate) fn inside(&self, tree: &Tree, id: NodeId) -> Option<&Group> {
        let mut bytes = vec![0u64; self.groups.len()];
        for (&item, &index) in &self.owner {
            if item != id && is_within(tree, item, id) {
                bytes[index] += tree.node(item).size();
            }
        }
        let (index, &most) = bytes.iter().enumerate().max_by_key(|&(_, &size)| size)?;
        (most > 0 && most.saturating_mul(4) >= tree.node(id).size()).then(|| &self.groups[index])
    }

    /// Bytes under `folder` that rules mark safe, and bytes they mark for review.
    pub(crate) fn reclaimable(&self, tree: &Tree, folder: NodeId) -> (u64, u64) {
        let mut totals = (0, 0);
        for (&item, &index) in &self.owner {
            if is_within(tree, item, folder) {
                let size = tree.node(item).size();
                match self.groups[index].safety {
                    Safety::Safe => totals.0 += size,
                    Safety::Review => totals.1 += size,
                }
            }
        }
        totals
    }

    pub(crate) fn count(&self) -> usize {
        self.groups.len()
    }
}

fn is_within(tree: &Tree, id: NodeId, ancestor: NodeId) -> bool {
    let mut current = Some(id);
    while let Some(node) = current {
        if node == ancestor {
            return true;
        }
        current = tree.parent(node);
    }
    false
}

pub(crate) fn find(cached: &Cached) -> Suggestions {
    let Some(home) = std::env::home_dir() else {
        return Suggestions::default();
    };
    let Ok(rules) = Rules::builtin(&home) else {
        return Suggestions::default();
    };
    let tree = &cached.tree;
    let mut groups: Vec<(String, Group)> = Vec::new();
    for finding in rules.find(tree) {
        let Some(id) = resolve(tree, &finding.path) else {
            continue;
        };
        let index = groups
            .iter()
            .position(|(rule, _)| *rule == finding.rule.id)
            .unwrap_or_else(|| {
                groups.push((
                    finding.rule.id.clone(),
                    Group {
                        name: finding.rule.name.clone(),
                        description: finding.rule.description.clone(),
                        category: finding.rule.category.clone(),
                        safety: finding.rule.safety,
                        command: finding.rule.command.clone(),
                        items: Vec::new(),
                        size: 0,
                    },
                ));
                groups.len() - 1
            });
        groups[index].1.items.push(id);
        groups[index].1.size += finding.size;
    }
    let mut groups: Vec<Group> = groups.into_iter().map(|(_, group)| group).collect();
    groups.sort_by_key(|group| std::cmp::Reverse(group.size));
    let owner = groups
        .iter()
        .enumerate()
        .flat_map(|(index, group)| group.items.iter().map(move |&id| (id, index)))
        .collect();
    Suggestions { groups, owner }
}

fn resolve(tree: &Tree, path: &std::path::Path) -> Option<NodeId> {
    let mut current = tree.root();
    for component in path.strip_prefix(tree.root_path()).ok()?.components() {
        current = tree
            .children(current)
            .find(|&child| tree.name(child) == component.as_os_str())?;
    }
    Some(current)
}

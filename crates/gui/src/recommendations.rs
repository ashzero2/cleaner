use clrs_core::{Cached, NodeId, Rules, Safety, Tree};

/// Every scan location one cleanup rule matched.
#[derive(Debug)]
pub(crate) struct Group {
    pub(crate) name: String,
    pub(crate) safety: Safety,
    pub(crate) command: Option<Vec<String>>,
    pub(crate) items: Vec<NodeId>,
    pub(crate) size: u64,
}

/// Cleanup rule matches in `cached`, largest group first.
pub(crate) fn find(cached: &Cached) -> Vec<Group> {
    let Some(home) = std::env::home_dir() else {
        return Vec::new();
    };
    let Ok(rules) = Rules::builtin(&home) else {
        return Vec::new();
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
    groups
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

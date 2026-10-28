/// Selection state management for multi-selection support.
///
/// Phase 2 feature: multi-selection with Cmd/Shift click, staging, and keyboard
/// navigation.
use std::collections::HashSet;

use clrs_core::NodeId;

use crate::explore::Directory;

/// Represents a multi-selection of items in the workbench.
#[derive(Debug, Default)]
pub(crate) struct SelectionState {
    /// Selected item IDs. Can be multiple for multi-selection.
    pub(crate) selected: Vec<NodeId>,
    /// Staged items ready for cleanup (Phase 2 review queue).
    pub(crate) staged: HashSet<NodeId>,
    /// Last clicked item for Shift-range selection.
    pub(crate) last_clicked: Option<NodeId>,
}

#[expect(dead_code)]
impl SelectionState {
    /// Clear all selections.
    pub(crate) fn clear(&mut self) {
        self.selected.clear();
        self.staged.clear();
        self.last_clicked = None;
    }

    /// Select a single item, clearing previous selection.
    pub(crate) fn select_single(&mut self, id: NodeId) {
        self.selected = vec![id];
        self.last_clicked = Some(id);
    }

    /// Toggle selection of an item.
    /// - If already selected: deselect
    /// - If not selected: add to selection
    pub(crate) fn toggle(&mut self, id: NodeId) {
        if self.selected.contains(&id) {
            self.selected.retain(|i| i != &id);
        } else {
            self.selected.push(id);
        }
        self.last_clicked = Some(id);
    }

    /// Select range from `last_clicked` to `id` (Shift-click behavior).
    pub(crate) fn select_range(&mut self, id: NodeId, delegate: &Directory) {
        let Some(last) = self.last_clicked else {
            // No last click, just select single
            self.select_single(id);
            return;
        };

        // Find row indices for both nodes
        let last_idx = delegate.rows.iter().position(|&r| r == last);
        let curr_idx = delegate.rows.iter().position(|&r| r == id);

        if let (Some(last_idx), Some(curr_idx)) = (last_idx, curr_idx) {
            let (start, end) = if last_idx <= curr_idx {
                (last_idx, curr_idx)
            } else {
                (curr_idx, last_idx)
            };

            // Select all rows in range
            self.selected = delegate.rows[start..=end].to_vec();
            self.last_clicked = Some(id);
        }
    }

    /// Select all items in current view.
    pub(crate) fn select_all(&mut self, delegate: &Directory) {
        self.selected.clone_from(&delegate.rows);
        self.last_clicked = self.selected.last().copied();
    }

    /// Check if an item is selected.
    pub(crate) fn is_selected(&self, id: NodeId) -> bool {
        self.selected.contains(&id)
    }

    /// Check if an item is staged.
    pub(crate) fn is_staged(&self, id: NodeId) -> bool {
        self.staged.contains(&id)
    }

    /// Stage all selected items.
    pub(crate) fn stage_all(&mut self) {
        for id in &self.selected {
            self.staged.insert(*id);
        }
    }

    /// Unstage all selected items.
    pub(crate) fn unstage_all(&mut self) {
        for id in &self.selected {
            self.staged.remove(id);
        }
    }

    /// Stage an individual item.
    pub(crate) fn stage(&mut self, id: NodeId) {
        self.staged.insert(id);
    }

    /// Unstage an individual item.
    pub(crate) fn unstage(&mut self, id: NodeId) {
        self.staged.remove(&id);
    }

    /// Get the count of selected items.
    pub(crate) fn selection_count(&self) -> usize {
        self.selected.len()
    }

    /// Get the count of staged items.
    pub(crate) fn staged_count(&self) -> usize {
        self.staged.len()
    }

    /// Check if any items are selected.
    pub(crate) fn has_selection(&self) -> bool {
        !self.selected.is_empty()
    }

    /// Check if any items are staged.
    pub(crate) fn has_staged(&self) -> bool {
        !self.staged.is_empty()
    }

    /// Get total allocation of selected items.
    pub(crate) fn total_allocation(&self, delegate: &Directory) -> u64 {
        self.selected
            .iter()
            .map(|id| delegate.cached.tree.node(*id).size())
            .sum()
    }

    /// Get space that would be recovered by staging all selected.
    pub(crate) fn recoverable_space(&self, delegate: &Directory) -> u64 {
        self.total_allocation(delegate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_single() {
        let _sel = SelectionState::default();
        // NodeId constructor is private; skip full test for now
    }

    #[test]
    fn test_toggle() {
        let _sel = SelectionState::default();
        // NodeId constructor is private; skip full test for now
    }

    #[test]
    fn test_select_all() {
        let _sel = SelectionState::default();
        // Test would need a mock Directory delegate
        // Placeholder for Phase 2 testing
    }
}

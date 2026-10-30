use std::collections::HashSet;
use std::sync::Arc;

use clrs_core::{Cached, EntryKind, NodeId};
use gpui_kit::component::table::{Column, TableDelegate, TableState};
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, h_flex};
use gpui_kit::{
    App, Context, IntoElement, ParentElement as _, SharedString, Styled as _, Window, div, px,
};

use crate::format;

#[derive(Debug)]
pub(crate) struct Directory {
    pub(crate) cached: Arc<Cached>,
    pub(crate) id: NodeId,
    pub(crate) rows: Vec<NodeId>,
    /// Rows are search or recommendation results, shown with paths relative to the scan root.
    results: bool,
    pub(crate) staged: HashSet<NodeId>,
}

impl Directory {
    pub(crate) fn new(cached: Arc<Cached>, id: NodeId) -> Self {
        let rows = cached.tree.children(id).collect();
        Self {
            cached,
            id,
            rows,
            results: false,
            staged: HashSet::new(),
        }
    }

    /// A table of `rows` from anywhere in the scan, with `id` as the folder to return to.
    pub(crate) fn results(cached: Arc<Cached>, id: NodeId, rows: Vec<NodeId>) -> Self {
        Self {
            cached,
            id,
            rows,
            results: true,
            staged: HashSet::new(),
        }
    }

    pub(crate) fn row(&self, index: usize) -> Option<NodeId> {
        self.rows.get(index).copied()
    }

    fn text(&self, row: usize, column: usize) -> String {
        let Some(id) = self.row(row) else {
            return String::new();
        };
        let tree = &self.cached.tree;
        let node = tree.node(id);
        match column {
            0 if self.results => tree.path(id).strip_prefix(tree.root_path()).map_or_else(
                |_| tree.name(id).to_string_lossy().into_owned(),
                |path| path.to_string_lossy().into_owned(),
            ),
            0 => tree.name(id).to_string_lossy().into_owned(),
            1 => format::allocated(node.size()),
            2 if node.kind() == EntryKind::Dir => format::count(node.files()),
            3 => format::kind(node.kind(), tree.name(id)).into(),
            _ => String::new(),
        }
    }
}

impl TableDelegate for Directory {
    fn columns_count(&self, _: &App) -> usize {
        4
    }
    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, column: usize, _: &App) -> Column {
        match column {
            0 => Column::new("name", "Name")
                .width(px(280.))
                .min_width(px(100.)),
            1 => Column::new("allocated", "Allocated")
                .width(px(120.))
                .min_width(px(80.))
                .text_right(),
            2 => Column::new("files", "Files")
                .width(px(90.))
                .min_width(px(70.))
                .text_right(),
            _ => Column::new("kind", "Kind")
                .width(px(90.))
                .min_width(px(60.)),
        }
    }

    fn render_td(
        &mut self,
        row: usize,
        column: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let text = self.text(row, column);
        let mut cell = div().w_full().min_w_0();
        if column == 0 {
            let icon = self.row(row).map_or(IconName::File, |id| {
                match self.cached.tree.node(id).kind() {
                    EntryKind::Dir => IconName::Folder,
                    EntryKind::Symlink => IconName::ExternalLink,
                    _ => IconName::File,
                }
            });
            cell = cell.child(
                h_flex()
                    .gap_2()
                    .child(
                        Icon::new(icon)
                            .small()
                            .text_color(cx.theme().muted_foreground),
                    )
                    .child(div().min_w_0().truncate().child(text))
                    .children(
                        self.row(row)
                            .filter(|id| self.staged.contains(id))
                            .map(|_| div().text_color(cx.theme().primary).child("Staged")),
                    ),
            );
        } else {
            cell = cell.child(text);
        }
        cell
    }

    fn cell_label(&self, row: usize, column: usize, _: &App) -> Option<SharedString> {
        Some(self.text(row, column).into())
    }

    fn cell_text(&self, row: usize, column: usize, _: &App) -> String {
        self.text(row, column)
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        div()
            .p_6()
            .text_color(cx.theme().muted_foreground)
            .child("No readable items in this folder. Check Scan Issues if access was restricted.")
    }
}

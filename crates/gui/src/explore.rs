use std::collections::HashSet;
use std::fmt::Write as _;
use std::rc::Rc;
use std::sync::Arc;

use clrs_core::{Cached, EntryKind, NodeId, Safety};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::table::{Column, TableDelegate, TableState};
use gpui_kit::component::{Icon, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    App, Context, Div, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _,
    Pixels, SharedString, Stateful, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px, relative,
};

use crate::category::{self, Category};
use crate::recommendations::Suggestions;
use crate::{format, theme, ui};

const BAR_WIDTH: f32 = 120.;
const SIZE_WIDTH: f32 = 92.;
const ADD_WIDTH: f32 = 46.;
/// Most rows whose descendants are searched for a rule match to hint at.
const HINT_ROWS: usize = 200;

/// Callbacks from rows back to the workbench.
#[derive(Clone)]
pub(crate) struct Hooks {
    pub(crate) hover: Hover,
    pub(crate) toggle: Toggle,
}

type Hover = Rc<dyn Fn(NodeId, bool, &mut App)>;
type Toggle = Rc<dyn Fn(NodeId, &mut App)>;

/// A rule match to show on a row: on the row itself, or somewhere inside it.
#[derive(Clone, Debug)]
pub(crate) struct Hint {
    pub(crate) name: String,
    pub(crate) safety: Safety,
    pub(crate) inside: bool,
}

pub(crate) struct Directory {
    pub(crate) cached: Arc<Cached>,
    pub(crate) id: NodeId,
    pub(crate) rows: Vec<NodeId>,
    /// Rows are search or recommendation results, shown with paths relative to the scan root.
    results: bool,
    pub(crate) staged: HashSet<NodeId>,
    pub(crate) hovered: Option<NodeId>,
    categories: Vec<Category>,
    hints: Vec<Option<Hint>>,
    name_width: Pixels,
    hooks: Hooks,
}

impl Directory {
    pub(crate) fn new(
        cached: Arc<Cached>,
        id: NodeId,
        suggestions: &Suggestions,
        hooks: Hooks,
    ) -> Self {
        let rows = cached.tree.children(id).collect();
        Self::build(cached, id, rows, false, suggestions, hooks)
    }

    /// A table of `rows` from anywhere in the scan, with `id` as the folder to return to.
    pub(crate) fn results(
        cached: Arc<Cached>,
        id: NodeId,
        rows: Vec<NodeId>,
        suggestions: &Suggestions,
        hooks: Hooks,
    ) -> Self {
        Self::build(cached, id, rows, true, suggestions, hooks)
    }

    fn build(
        cached: Arc<Cached>,
        id: NodeId,
        rows: Vec<NodeId>,
        results: bool,
        suggestions: &Suggestions,
        hooks: Hooks,
    ) -> Self {
        let mut directory = Self {
            cached,
            id,
            rows,
            results,
            staged: HashSet::new(),
            hovered: None,
            categories: Vec::new(),
            hints: Vec::new(),
            name_width: px(260.),
            hooks,
        };
        directory.classify(suggestions);
        directory
    }

    /// Recomputes row colours and rule hints, e.g. once suggestions finish loading.
    pub(crate) fn classify(&mut self, suggestions: &Suggestions) {
        let tree = &self.cached.tree;
        self.categories = self
            .rows
            .iter()
            .map(|&id| category::classify(tree, id, suggestions))
            .collect();
        self.hints = self
            .rows
            .iter()
            .enumerate()
            .map(|(index, &id)| {
                if let Some(group) = suggestions.of(id) {
                    return Some(Hint {
                        name: group.name.clone(),
                        safety: group.safety,
                        inside: false,
                    });
                }
                (index < HINT_ROWS && tree.node(id).kind() == EntryKind::Dir)
                    .then(|| suggestions.inside(tree, id))
                    .flatten()
                    .map(|group| Hint {
                        name: group.name.clone(),
                        safety: group.safety,
                        inside: true,
                    })
            })
            .collect();
    }

    pub(crate) fn row(&self, index: usize) -> Option<NodeId> {
        self.rows.get(index).copied()
    }

    pub(crate) fn category(&self, index: usize) -> Category {
        self.categories
            .get(index)
            .copied()
            .unwrap_or(Category::Other)
    }

    pub(crate) fn hint(&self, index: usize) -> Option<&Hint> {
        self.hints.get(index).and_then(Option::as_ref)
    }

    /// Fits the name column to `width`, the space the table has. Returns whether it changed.
    pub(crate) fn fit(&mut self, width: Pixels) -> bool {
        let name = (width - px(BAR_WIDTH + SIZE_WIDTH + ADD_WIDTH + 2.)).max(px(160.));
        let changed = (name - self.name_width).abs() >= px(1.);
        self.name_width = name;
        changed
    }

    fn name(&self, id: NodeId) -> String {
        let tree = &self.cached.tree;
        if self.results {
            tree.path(id).strip_prefix(tree.root_path()).map_or_else(
                |_| tree.name(id).to_string_lossy().into_owned(),
                |path| path.to_string_lossy().into_owned(),
            )
        } else {
            tree.name(id).to_string_lossy().into_owned()
        }
    }

    /// Share of the open folder, or of the whole scan for results from anywhere in it.
    fn share(&self, id: NodeId) -> f64 {
        let tree = &self.cached.tree;
        let whole = if self.results { tree.root() } else { self.id };
        ui::share(tree.node(id).size(), tree.node(whole).size())
    }

    fn text(&self, row: usize, column: usize) -> String {
        let Some(id) = self.row(row) else {
            return String::new();
        };
        let tree = &self.cached.tree;
        let node = tree.node(id);
        match column {
            0 => {
                let mut label = format!(
                    "{}, {}",
                    self.name(id),
                    format::kind(node.kind(), tree.name(id))
                );
                if let Some(hint) = self.hint(row) {
                    let _ = write!(label, ", {}", ui::hint_label(hint));
                }
                if self.staged.contains(&id) {
                    label.push_str(", in tray");
                }
                label
            }
            2 => format!(
                "{}, {} of {}",
                format::allocated(node.size()),
                format::percent(self.share(id)),
                if self.results { "scan" } else { "folder" }
            ),
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
                .width(self.name_width)
                .resizable(false),
            1 => Column::new("bar", "").width(px(BAR_WIDTH)).resizable(false),
            2 => Column::new("size", "Size")
                .width(px(SIZE_WIDTH))
                .text_right()
                .resizable(false),
            _ => Column::new("add", "")
                .width(px(ADD_WIDTH))
                .resizable(false)
                .selectable(false),
        }
    }

    fn render_th(
        &mut self,
        column: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let label = self.column(column, cx).name;
        let mut header = ui::eyebrow(label.to_uppercase(), cx)
            .size_full()
            .flex()
            .items_center();
        if column == 2 {
            header = header.justify_end();
        }
        header
    }

    fn render_tr(
        &mut self,
        row: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let id = self.row(row);
        let hover = Rc::clone(&self.hooks.hover);
        let mut tr = div().id(("row", row)).on_hover(move |hovered, _, cx| {
            if let Some(id) = id {
                hover(id, *hovered, cx);
            }
        });
        if id.is_some() && id == self.hovered {
            tr = tr.bg(theme::palette(cx).panel);
        }
        tr
    }

    #[expect(clippy::too_many_lines, reason = "one declarative view tree")]
    fn render_td(
        &mut self,
        row: usize,
        column: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let Some(id) = self.row(row) else {
            return div().into_any_element();
        };
        let p = *theme::palette(cx);
        let tree = &self.cached.tree;
        let node = tree.node(id);
        let color = p.category(self.category(row));
        match column {
            0 => {
                let icon = match node.kind() {
                    EntryKind::Dir => IconName::Folder,
                    EntryKind::Symlink => IconName::ExternalLink,
                    _ => IconName::File,
                };
                let name = self.name(id);
                let label = if self.results {
                    let (parent, leaf) = name
                        .rsplit_once('/')
                        .map_or((None, name.as_str()), |(parent, leaf)| (Some(parent), leaf));
                    h_flex()
                        .min_w_0()
                        .overflow_hidden()
                        .children(parent.map(|parent| {
                            div()
                                .min_w_0()
                                .whitespace_nowrap()
                                .overflow_hidden()
                                .text_ellipsis_start()
                                .text_color(p.ink_2)
                                .font_weight(FontWeight::NORMAL)
                                .child(format!("{parent}/"))
                        }))
                        .child(div().flex_shrink_0().child(leaf.to_owned()))
                } else {
                    h_flex()
                        .min_w_0()
                        .child(div().min_w_0().truncate().child(name))
                };
                let hint = self.hint(row);
                h_flex()
                    .w_full()
                    .min_w_0()
                    .gap_2()
                    .child(ui::swatch(color, px(10.)))
                    .child(Icon::new(icon).xsmall().text_color(p.ink_3))
                    .child(
                        div()
                            .min_w_0()
                            .font_weight(FontWeight::MEDIUM)
                            .map(|name| {
                                if hint.is_some() {
                                    name.flex_shrink_0().max_w(relative(0.62))
                                } else {
                                    name.flex_1()
                                }
                            })
                            .child(label),
                    )
                    .children(hint.map(|hint| {
                        ui::chip(hint, cx)
                            .flex_shrink(1.)
                            .min_w_0()
                            .overflow_hidden()
                    }))
                    .into_any_element()
            }
            1 => {
                let largest = self
                    .rows
                    .first()
                    .map_or(1, |&first| tree.node(first).size().max(1));
                ui::bar(ui::portion(node.size(), largest), color, p.line).into_any_element()
            }
            2 => v_flex()
                .w_full()
                .items_end()
                .gap_0()
                .line_height(px(14.))
                .font_features(theme::tabular())
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format::allocated(node.size())),
                )
                .child(
                    div()
                        .text_size(px(10.5))
                        .text_color(p.ink_2)
                        .child(format::percent(self.share(id))),
                )
                .into_any_element(),
            _ => {
                let staged = self.staged.contains(&id);
                let toggle = Rc::clone(&self.hooks.toggle);
                let name = tree.name(id).to_string_lossy().into_owned();
                let button = Button::new(("add", row))
                    .icon(if staged {
                        IconName::Check
                    } else {
                        IconName::Plus
                    })
                    .xsmall()
                    .tooltip(if staged {
                        "Remove from tray"
                    } else {
                        "Add to tray"
                    })
                    .accessibility_label(if staged {
                        format!("Remove {name} from tray")
                    } else {
                        format!("Add {name} to tray")
                    })
                    .on_click(move |_, _, cx| toggle(id, cx));
                h_flex()
                    .w_full()
                    .justify_center()
                    .child(if staged {
                        button.primary()
                    } else {
                        button.ghost()
                    })
                    .into_any_element()
            }
        }
    }

    fn cell_label(&self, row: usize, column: usize, _: &App) -> Option<SharedString> {
        let text = self.text(row, column);
        (!text.is_empty()).then(|| text.into())
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
            .text_color(theme::palette(cx).ink_2)
            .child(if self.results {
                "Nothing matches."
            } else {
                "Nothing in this folder takes up space. If access was blocked, check Locked folders."
            })
    }
}

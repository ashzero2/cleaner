use std::fmt::Write as _;
use std::rc::Rc;
use std::sync::Arc;

use clrs_core::{EntryKind, NodeId, Safety};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::table::DataTable;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Selectable as _, Sizable as _, Size, h_flex, v_flex};
use gpui_kit::{
    AnyElement, ClickEvent, Context, Div, FontWeight, InteractiveElement as _, IntoElement,
    KeyDownEvent, ParentElement as _, Pixels, Role, StatefulInteractiveElement as _, Styled as _,
    Window, canvas, div, prelude::FluentBuilder as _, px, relative, white,
};

use super::{ResultsKind, SearchScope, Workbench};
use crate::category::{self, Category};
use crate::settings::Density;
use crate::treemap::{self, Block};
use crate::{format, theme, ui};

/// Laid-out tiles for one folder at one size, reused until either changes.
pub(super) struct TreemapCache {
    folder: NodeId,
    width: i32,
    height: i32,
    suggestions: usize,
    blocks: Rc<Vec<Block>>,
}

/// Width bounds for one browse column.
pub(super) const COLUMN_MIN_WIDTH: f32 = 300.;
pub(super) const COLUMN_MAX_WIDTH: f32 = 480.;

#[expect(clippy::cast_possible_truncation, reason = "pixel sizes")]
fn key(value: f32) -> i32 {
    value.round() as i32
}

impl Workbench {
    pub(super) fn main(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let mut main = v_flex()
            .id("main")
            .role(Role::Main)
            .aria_label("Disk space explorer")
            .flex_1()
            .min_w_0()
            .min_h_0()
            .px(px(20.))
            .pt(px(18.))
            .pb(px(14.))
            .gap(px(16.));
        if let Some(warning) = &self.warning {
            main = main.child(Self::callout(warning.clone(), cx));
        }
        if self.columns.is_empty() && self.table.is_none() {
            return main.child(if self.running.is_some() {
                self.scanning(cx).into_any_element()
            } else {
                self.welcome(cx).into_any_element()
            });
        }
        let list = v_flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .gap(px(12.))
            .child(self.list(cx))
            .child(self.detail(cx));
        let body = h_flex()
            .flex_1()
            .min_h_0()
            .items_stretch()
            .gap(px(16.))
            .when(self.settings.map_visible, |body| {
                body.child(
                    v_flex()
                        .w(relative(0.40))
                        .flex_shrink_0()
                        .min_h_0()
                        .gap(px(12.))
                        .child(self.treemap(window, cx))
                        .child(self.legend(cx)),
                )
            });
        main.child(self.summary(window, cx)).child(body.child(list))
    }

    fn callout(text: String, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = *theme::palette(cx);
        h_flex()
            .id("warning")
            .role(Role::Alert)
            .aria_label(text.clone())
            .gap_2()
            .px(px(12.))
            .py(px(9.))
            .rounded(px(10.))
            .bg(p.review_soft)
            .text_color(p.ink)
            .child(
                ui::icon(IconName::TriangleAlert)
                    .small()
                    .text_color(p.review),
            )
            .child(div().flex_1().child(text))
            .child(
                Button::new("dismiss-warning")
                    .icon(IconName::X)
                    .ghost()
                    .xsmall()
                    .accessibility_label("Dismiss")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.warning = None;
                        cx.notify();
                    })),
            )
    }

    fn welcome(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = *theme::palette(cx);
        v_flex()
            .flex_1()
            .items_center()
            .justify_center()
            .gap(px(14.))
            .child(
                div()
                    .size(px(76.))
                    .rounded_full()
                    .bg(p.accent_soft)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(ui::icon(IconName::HardDrive).size(px(34.)).text_color(p.accent_fill)),
            )
            .child(ui::display("See what's using your space", 24.))
            .child(
                div()
                    .max_w(px(420.))
                    .text_center()
                    .text_color(p.ink_2)
                    .line_height(relative(1.45))
                    .child("Scan your home folder, or pick a project or drive. You'll see what takes up room and what's safe to clear."),
            )
            .child(
                h_flex()
                    .gap_2()
                    .pt_1()
                    .child(
                        Button::new("home-welcome")
                            .label("Scan Home")
                            .icon(IconName::House)
                            .primary()
                            .on_click(cx.listener(|this, _, _, cx| this.home(cx))),
                    )
                    .child(
                        Button::new("start-welcome")
                            .label("Choose folder…")
                            .outline()
                            .on_click(cx.listener(|_, _, window, cx| Self::choose(window, cx))),
                    ),
            )
            .when(!self.status.is_empty(), |welcome| {
                welcome.child(
                    div()
                        .id("status")
                        .role(Role::Status)
                        .aria_label(self.status.clone())
                        .text_color(p.ink_2)
                        .child(self.status.clone()),
                )
            })
            .child(
                h_flex()
                    .gap_1()
                    .pt_2()
                    .text_size(px(12.))
                    .text_color(p.ink_2)
                    .child(ui::icon(IconName::Eye).xsmall())
                    .child("Looking is read-only. Nothing moves until you review it."),
            )
    }

    fn scanning(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = *theme::palette(cx);
        let (counts, stopping) = self.running.as_ref().map_or_else(
            || (String::new(), false),
            |running| {
                let progress = running.progress.snapshot();
                (
                    format!(
                        "{} files · {} folders · {} found · {:.0} s",
                        format::count(progress.files),
                        format::count(progress.dirs),
                        format::allocated(progress.bytes),
                        running.started.elapsed().as_secs_f64()
                    ),
                    running.progress.is_cancelled(),
                )
            },
        );
        let name = self.root.as_deref().map_or_else(String::new, format::name);
        v_flex().flex_1().items_center().justify_center().child(
            ui::lifted(ui::card(cx), cx)
                .w(px(420.))
                .p(px(24.))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(10.))
                .child(Spinner::new().large().color(p.accent))
                .child(ui::display(
                    if stopping {
                        "Stopping…".to_owned()
                    } else {
                        format!("Looking through {name}…")
                    },
                    18.,
                ))
                .child(
                    div()
                        .id("scan-progress")
                        .role(Role::Status)
                        .aria_label(counts.clone())
                        .text_color(p.ink_2)
                        .font_features(theme::tabular())
                        .child(counts),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(p.ink_2)
                        .text_center()
                        .child("The first scan of a big folder takes a little while. Later scans reuse what's saved."),
                )
                .child(
                    Button::new("stop-scan")
                        .label("Stop")
                        .outline()
                        .small()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.jobs.cancel();
                            cx.notify();
                        })),
                ),
        )
    }

    #[expect(clippy::too_many_lines, reason = "one declarative view tree")]
    fn summary(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = *theme::palette(cx);
        let (Some(cached), Some(folder)) = (self.cached(cx), self.folder(cx)) else {
            return div().into_any_element();
        };
        let focus = window
            .use_keyed_state("reclaimable", cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let focused = focus.is_focused(window);
        let tree = &cached.tree;
        let node = tree.node(folder);
        let name = if folder == tree.root() {
            format::name(tree.root_path())
        } else {
            tree.name(folder).to_string_lossy().into_owned()
        };
        let unread = if cached.issues.is_empty() {
            String::new()
        } else {
            format!(
                " · {} folders unread",
                format::count(cached.issues.len() as u64)
            )
        };
        let status = if let Some(running) = &self.running {
            format!(
                "Rescanning… {} files so far",
                format::count(running.progress.snapshot().files)
            )
        } else if !self.status.is_empty() {
            self.status.clone()
        } else {
            let source = self.scanned.as_ref().map_or_else(String::new, |scanned| {
                if scanned.full {
                    format!(
                        " · Full scan in {:.1} s, {}",
                        scanned.elapsed.as_secs_f64(),
                        format::ago(scanned.at.elapsed())
                    )
                } else {
                    format!(
                        " · Updated saved scan, {}",
                        format::ago(scanned.at.elapsed())
                    )
                }
            });
            format!("{} files{unread}{source}", format::count(node.files()))
        };
        let (safe, review) = self.suggestions.reclaimable(tree, folder);
        let size = node.size();
        let reclaim_label = if safe + review == 0 {
            "Nothing to clear here".to_owned()
        } else {
            format!(
                "{} safe to clear, {} worth a look",
                format::allocated(safe),
                format::allocated(review)
            )
        };
        let has_suggestions = safe + review > 0;
        let meter = h_flex()
            .h(px(8.))
            .w_full()
            .rounded_full()
            .bg(p.line)
            .overflow_hidden()
            .child(
                div()
                    .h_full()
                    .bg(p.safe)
                    .w(relative(ui::portion(safe, size))),
            )
            .child(
                div()
                    .h_full()
                    .bg(p.review)
                    .w(relative(ui::portion(review, size))),
            );
        let headline = if safe + review == 0 {
            div()
                .text_color(p.ink_2)
                .font_weight(FontWeight::MEDIUM)
                .child("Nothing to clear here")
                .into_any_element()
        } else {
            h_flex()
                .gap_2()
                .items_baseline()
                .child(
                    div()
                        .text_color(p.safe)
                        .child(ui::display(format::allocated(safe), 20.)),
                )
                .child(div().text_color(p.ink).child("safe to clear"))
                .child(div().flex_1())
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(p.ink_2)
                        .font_features(theme::tabular())
                        .child(format!("+ {} worth a look", format::allocated(review))),
                )
                .child(ui::icon(IconName::ChevronRight).small().text_color(p.ink_2))
                .into_any_element()
        };
        h_flex()
            .items_end()
            .min_w_0()
            .gap(px(24.))
            .child(
                v_flex()
                    .min_w_0()
                    .gap(px(6.))
                    .child(
                        h_flex()
                            .id("folder-size")
                            .role(Role::Label)
                            .aria_label(if cached.issues.is_empty() {
                                format!("{} in {name}", format::allocated(size))
                            } else {
                                format!(
                                    "{} in {name}; {} folders unread",
                                    format::allocated(size),
                                    format::count(cached.issues.len() as u64)
                                )
                            })
                            .min_w_0()
                            .items_baseline()
                            .gap_2()
                            .child(ui::display(format::allocated(size), 32.))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(16.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(p.ink_2)
                                    .child(format!("in {name}")),
                            ),
                    )
                    .child(
                        div()
                            .id("scan-status")
                            .role(Role::Status)
                            .aria_label(status.clone())
                            .text_color(p.ink_2)
                            .font_features(theme::tabular())
                            .truncate()
                            .child(status),
                    ),
            )
            .child(div().flex_1())
            .child(
                v_flex()
                    .id("reclaimable")
                    .when(has_suggestions, |card| {
                        card.role(Role::Button)
                            .aria_label(format!("{reclaim_label}. Open suggestions"))
                            .relative()
                            .track_focus(&focus)
                            .tab_index(0)
                            .cursor_pointer()
                            .hover(|style| style.border_color(p.ink_3))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.show_suggestions(window, cx);
                            }))
                    })
                    .w(px(380.))
                    .flex_shrink_0()
                    .gap(px(8.))
                    .px(px(14.))
                    .py(px(12.))
                    .rounded(px(14.))
                    .bg(p.panel)
                    .border_1()
                    .border_color(p.line)
                    .child(headline)
                    .child(meter)
                    .when(has_suggestions && focused, |card| {
                        card.child(ui::focus_outline(px(14.), p.accent_fill))
                    }),
            )
            .into_any_element()
    }

    fn blocks(&self, cx: &Context<Self>) -> Option<Rc<Vec<Block>>> {
        let cached = self.cached(cx)?;
        let folder = self.folder(cx)?;
        let measured = self.treemap_size.get();
        let (width, height) = (f32::from(measured.width), f32::from(measured.height));
        let suggestions = Arc::as_ptr(&self.suggestions) as usize;
        let mut cache = self.treemap_cache.borrow_mut();
        if let Some(hit) = cache.as_ref().filter(|hit| {
            hit.folder == folder
                && hit.width == key(width)
                && hit.height == key(height)
                && hit.suggestions == suggestions
        }) {
            return Some(Rc::clone(&hit.blocks));
        }
        let blocks = Rc::new(treemap::blocks(
            &cached.tree,
            folder,
            &self.suggestions,
            width,
            height,
        ));
        *cache = Some(TreemapCache {
            folder,
            width: key(width),
            height: key(height),
            suggestions,
            blocks: Rc::clone(&blocks),
        });
        Some(blocks)
    }

    fn treemap(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = *theme::palette(cx);
        let focus = window
            .use_keyed_state("treemap", cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let focused = focus.is_focused(window);
        let measure = Rc::clone(&self.treemap_size);
        let view = cx.weak_entity();
        let mut area = div().relative().size_full().child(
            canvas(
                move |bounds, _, cx| {
                    if measure.get() != bounds.size {
                        measure.set(bounds.size);
                        let view = view.clone();
                        cx.defer(move |cx| {
                            let _ = view.update(cx, |_, cx| cx.notify());
                        });
                    }
                },
                |_, (), _, _| {},
            )
            .absolute()
            .size_full(),
        );
        let blocks = self.blocks(cx).unwrap_or_default();
        let ids = Rc::new(
            blocks
                .iter()
                .filter_map(|block| block.id)
                .collect::<Vec<_>>(),
        );
        let parent = self
            .cached(cx)
            .zip(self.folder(cx))
            .map_or(1, |(cached, folder)| cached.tree.node(folder).size());
        for (index, block) in blocks.iter().enumerate() {
            area = area.child(self.tile(index, block, parent, cx));
        }
        if blocks.is_empty() && self.treemap_size.get().width > px(0.) {
            area = area.child(
                div()
                    .p_4()
                    .text_color(p.ink_2)
                    .child("Nothing in this folder takes up space."),
            );
        }
        div()
            .id("treemap")
            .role(Role::List)
            .aria_label(format!(
                "Treemap of the open folder, {}",
                format::plural(ids.len(), "item")
            ))
            .track_focus(&focus)
            .tab_index(0)
            .on_key_down(cx.listener({
                let ids = Rc::clone(&ids);
                move |this, event: &KeyDownEvent, window, cx| {
                    this.treemap_key(event, &ids, window, cx);
                }
            }))
            .flex_1()
            .min_h(px(160.))
            .rounded(px(14.))
            .bg(p.panel)
            .border_1()
            .border_color(p.line)
            .p(px(8.))
            .overflow_hidden()
            .child(area)
            .when(focused, |area| {
                area.child(ui::focus_inside(px(14.), p.accent_fill))
            })
    }

    fn tile(&self, index: usize, block: &Block, parent: u64, cx: &mut Context<Self>) -> AnyElement {
        const GAP: f32 = treemap::TILE_GAP;
        let p = *theme::palette(cx);
        let color = p.category(block.category);
        let (width, height) = (block.width - GAP, block.height - GAP);
        let size = format::allocated(block.size);
        let share = format::percent(ui::share(block.size, parent));
        let mut label = format!(
            "{}, {size}, {share}, {}",
            block.name,
            block.category.label()
        );
        if let Some(safety) = block.safety {
            let _ = write!(label, ", {}", ui::safety_text(safety).to_lowercase());
        }
        let selected = block.id.is_some_and(|id| self.selection.is_selected(id));
        let hovered = block.id.is_some() && block.id == self.hovered;
        let fill = if hovered {
            color.blend(white().opacity(0.14))
        } else {
            color
        };
        let tooltip = format!("{}\n{size} · {share}", block.name);
        let mut tile = div()
            .id(("tile", index))
            .role(Role::ListItem)
            .aria_label(label)
            .aria_selected(selected)
            .absolute()
            .left(px(block.x))
            .top(px(block.y))
            .w(px(width.max(1.)))
            .h(px(height.max(1.)))
            .px(px(6.))
            .py(px(7.))
            .overflow_hidden()
            .rounded(px(8.))
            .bg(fill)
            .text_color(p.tile_ink)
            .when(selected, |tile| tile.border_1().border_color(p.ink))
            .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx));
        if let Some(id) = block.id {
            tile = tile
                .cursor_pointer()
                .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                    this.hover(id, *hovered, cx);
                }))
                .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                    this.pick(id, event.click_count(), window, cx);
                }));
        }
        if width >= treemap::LABEL_WIDTH && height >= treemap::LABEL_HEIGHT {
            tile = tile
                .child(
                    div()
                        .relative()
                        .truncate()
                        .pr(px(if block.safety.is_some() { 20. } else { 0. }))
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(block.name.clone()),
                )
                .child(
                    div()
                        .relative()
                        .truncate()
                        .text_size(px(11.5))
                        .opacity(0.75)
                        .font_features(theme::tabular())
                        .child(size),
                );
        }
        if let Some(safety) = block.safety.filter(|_| width > 60. && height > 40.) {
            tile = tile.child(
                div()
                    .absolute()
                    .top(px(6.))
                    .right(px(6.))
                    .size(px(18.))
                    .rounded_full()
                    .bg(p.raised)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        ui::icon(match safety {
                            Safety::Safe => IconName::ShieldCheck,
                            Safety::Review => IconName::Eye,
                        })
                        .size(px(11.))
                        .text_color(match safety {
                            Safety::Safe => p.safe,
                            Safety::Review => p.review,
                        }),
                    ),
            );
        }
        tile.into_any_element()
    }

    /// Arrow keys move through the drawn tiles and Enter opens one. The list stays the
    /// complete, scrollable version of the same data.
    fn treemap_key(
        &mut self,
        event: &KeyDownEvent,
        ids: &[NodeId],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if ids.is_empty() || event.keystroke.modifiers.modified() {
            return;
        }
        let key = event.keystroke.key.as_str();
        let current = self.selection.selected.last().copied();
        let index = current.and_then(|id| ids.iter().position(|&row| row == id));
        if key == "enter" || key == "space" {
            let id = current.or_else(|| ids.first().copied());
            if let Some(id) = id {
                let clicks = if key == "enter" && current.is_some() {
                    2
                } else {
                    1
                };
                self.pick(id, clicks, window, cx);
                cx.stop_propagation();
            }
            return;
        }
        let at = index.unwrap_or(0);
        let target = match key {
            "home" => 0,
            "end" => ids.len() - 1,
            "up" | "left" => at.saturating_sub(1),
            "down" | "right" => (at + 1).min(ids.len() - 1),
            _ => return,
        };
        if let Some(&id) = ids.get(target) {
            self.pick(id, 1, window, cx);
            cx.stop_propagation();
        }
    }

    pub(super) fn hover(&mut self, id: NodeId, hovered: bool, cx: &mut Context<Self>) {
        if hovered {
            self.set_hovered(Some(id), cx);
        } else if self.hovered == Some(id) {
            self.set_hovered(None, cx);
        }
    }

    fn legend(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = *theme::palette(cx);
        let blocks = self.blocks(cx).unwrap_or_default();
        let present: Vec<Category> = Category::ALL
            .into_iter()
            .filter(|category| blocks.iter().any(|block| block.category == *category))
            .collect();
        let mut legend = v_flex().gap_y_1().px_1();
        for row in present.chunks(4) {
            legend = legend.child(h_flex().flex_wrap().gap_x(px(14.)).gap_y_1().children(
                row.iter().map(|&category| {
                    h_flex()
                        .gap(px(6.))
                        .text_size(px(11.5))
                        .text_color(p.ink_2)
                        .child(ui::swatch(p.category(category), px(10.)))
                        .child(category.label())
                }),
            ));
        }
        legend
    }

    #[expect(clippy::too_many_lines, reason = "one declarative view tree")]
    fn detail(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = *theme::palette(cx);
        let card = ui::lifted(ui::card(cx), cx).px(px(14.)).py(px(12.));
        let (Some(cached), Some(subject)) = (self.cached(cx), self.subject(cx)) else {
            return card.into_any_element();
        };
        let tree = &cached.tree;
        let selected = &self.selection.selected;
        if selected.len() > 1 {
            let total: u64 = selected.iter().map(|&id| tree.node(id).size()).sum();
            return card
                .child(
                    h_flex()
                        .gap_3()
                        .child(ui::display(format!("{} items", selected.len()), 15.))
                        .child(
                            div()
                                .text_color(p.ink_2)
                                .font_features(theme::tabular())
                                .child(format::allocated(total)),
                        )
                        .child(div().flex_1())
                        .child(Self::stage_button(
                            selected.iter().all(|id| self.selection.is_staged(*id)),
                            cx,
                        )),
                )
                .into_any_element();
        }
        let node = tree.node(subject);
        let path = tree.path(subject);
        let name = if subject == tree.root() {
            format::name(&path)
        } else {
            tree.name(subject).to_string_lossy().into_owned()
        };
        let category = category::classify(tree, subject, &self.suggestions);
        let group = self.suggestions.of(subject);
        let can_stage = subject != tree.root();
        let mut actions = h_flex()
            .gap(px(6.))
            .flex_shrink_0()
            .child(
                Button::new("reveal")
                    .label("Reveal")
                    .icon(IconName::ArrowUpRight)
                    .outline()
                    .small()
                    .tooltip("Reveal in Finder")
                    .accessibility_label("Reveal in Finder")
                    .on_click(cx.listener(|this, _, _, cx| this.reveal(cx))),
            )
            .child(
                Button::new("copy-path")
                    .icon(IconName::Copy)
                    .outline()
                    .small()
                    .tooltip("Copy path")
                    .accessibility_label("Copy Path")
                    .on_click(cx.listener(|this, _, _, cx| this.copy_path(cx))),
            );
        if node.kind() == EntryKind::File {
            actions = actions.child(
                Button::new("preview")
                    .label("Preview")
                    .icon(IconName::Eye)
                    .outline()
                    .small()
                    .tooltip("Quick Look")
                    .accessibility_label("Quick Look preview")
                    .on_click(cx.listener(|this, _, _, cx| this.preview(cx))),
            );
        }
        if can_stage {
            actions = actions.child(Self::stage_button(self.selection.is_staged(subject), cx));
        }
        if selected.is_empty() {
            return card
                .child(
                    h_flex()
                        .min_w_0()
                        .items_center()
                        .gap(px(10.))
                        .child(ui::swatch(p.category(category), px(12.)))
                        .child(
                            div()
                                .min_w_0()
                                .max_w(relative(0.45))
                                .truncate()
                                .font_family(theme::DISPLAY_FONT)
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_size(px(15.))
                                .child(name),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .font_family(theme::MONO_FONT)
                                .text_size(px(11.5))
                                .text_color(p.ink_3)
                                .child(format::path(&path)),
                        )
                        .child(actions),
                )
                .into_any_element();
        }
        let parent = tree
            .parent(subject)
            .map_or(node.size(), |parent| tree.node(parent).size());
        let mut facts = h_flex()
            .gap(px(12.))
            .flex_wrap()
            .text_size(px(12.))
            .text_color(p.ink_2)
            .font_features(theme::tabular())
            .child(
                div()
                    .id("detail-size")
                    .role(Role::Label)
                    .aria_label(format!(
                        "Size: {} ({} bytes)",
                        format::allocated(node.size()),
                        node.size()
                    ))
                    .tooltip({
                        let text = format!(
                            "{} bytes on disk (allocated blocks)",
                            format::count(node.size())
                        );
                        move |window, cx| Tooltip::new(text.clone()).build(window, cx)
                    })
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(p.ink)
                    .child(format::allocated(node.size())),
            );
        if subject != tree.root() {
            facts = facts.child(format!(
                "{} of {}",
                format::percent(ui::share(node.size(), parent)),
                tree.parent(subject)
                    .map_or_else(String::new, |parent| format::name(&tree.path(parent)))
            ));
        }
        if node.kind() == EntryKind::Dir {
            facts = facts.child(format!("{} files", format::count(node.files())));
        }
        facts = facts.child(category.label());
        let kind = format::kind(node.kind(), tree.name(subject));
        if kind != "Folder" {
            facts = facts.child(kind);
        }
        let about = group
            .and_then(|group| group.description.clone())
            .map_or_else(
                || {
                    div()
                        .font_family(theme::MONO_FONT)
                        .text_size(px(11.5))
                        .text_color(p.ink_3)
                        .truncate()
                        .child(format::path(&path))
                },
                |text| {
                    div()
                        .text_size(px(12.))
                        .text_color(p.ink_2)
                        .line_clamp(2)
                        .child(text)
                },
            );
        card.child(
            v_flex()
                .min_w_0()
                .gap(px(4.))
                .child(
                    h_flex()
                        .gap_2()
                        .min_w_0()
                        .child(ui::swatch(p.category(category), px(12.)))
                        .child(div().min_w_0().truncate().child(ui::display(name, 15.)))
                        .children(group.map(|group| {
                            ui::safety_chip(group.safety, ui::safety_text(group.safety), true, cx)
                        }))
                        .child(div().flex_1())
                        .child(actions),
                )
                .child(facts)
                .child(about),
        )
        .into_any_element()
    }

    fn stage_button(staged: bool, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let button = Button::new("stage")
            .small()
            .on_click(cx.listener(move |this, _, _, cx| {
                let targets = this
                    .selection
                    .selected
                    .clone()
                    .into_iter()
                    .collect::<Vec<_>>();
                let targets = if targets.is_empty() {
                    this.subject(cx).into_iter().collect()
                } else {
                    targets
                };
                for id in targets {
                    if staged {
                        this.selection.unstage(id);
                    } else {
                        this.selection.stage(id);
                    }
                }
                this.sync_staged(cx);
            }));
        if staged {
            button.label("In tray").icon(IconName::Check).outline()
        } else {
            button.label("Add to tray").icon(IconName::Plus).primary()
        }
    }

    fn list(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = *theme::palette(cx);
        let measure = Rc::clone(&self.list_width);
        let view = cx.weak_entity();
        let measurer = canvas(
            move |bounds, _, cx| {
                if measure.get() != bounds.size.width {
                    measure.set(bounds.size.width);
                    let view = view.clone();
                    cx.defer(move |cx| {
                        let _ = view.update(cx, |_, cx| cx.notify());
                    });
                }
            },
            |_, (), _, _| {},
        )
        .absolute()
        .size_full();
        if let Some(table) = &self.table {
            let mut card = ui::card(cx)
                .flex_1()
                .min_w_0()
                .min_h_0()
                .flex()
                .flex_col()
                .overflow_hidden();
            if let Some(results) = &self.results {
                card = card.child(self.results_header(results, cx));
            }
            return card
                .child(
                    div().relative().flex_1().min_h_0().child(measurer).child(
                        DataTable::new(table)
                            .bordered(false)
                            .stripe(false)
                            .header_row_height(px(34.))
                            .with_size(Size::Size(match self.settings.density {
                                Density::Comfortable => px(42.),
                                Density::Compact => px(38.),
                            })),
                    ),
                )
                .text_color(p.ink)
                .into_any_element();
        }
        let width = self.column_width();
        let density = match self.settings.density {
            Density::Comfortable => px(42.),
            Density::Compact => px(38.),
        };
        let count = self.columns.len();
        let mut columns = h_flex()
            .id("columns")
            .h_full()
            .flex_1()
            .min_h_0()
            .overflow_x_scroll()
            .track_scroll(&self.columns_scroll)
            .items_stretch();
        for (index, table) in self.columns.iter().enumerate() {
            columns = columns.child(
                div()
                    .w(width)
                    .min_h_0()
                    .flex_shrink_0()
                    .when(index + 1 < count, |column| {
                        column.border_r_1().border_color(p.line)
                    })
                    .child(
                        DataTable::new(table)
                            .bordered(false)
                            .stripe(false)
                            .header_row_height(px(34.))
                            .with_size(Size::Size(density)),
                    ),
            );
        }
        ui::card(cx)
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(measurer)
                    .child(columns),
            )
            .text_color(p.ink)
            .into_any_element()
    }

    /// Equal share of the list area for each open column, clamped to a readable range.
    /// Past the minimum the card scrolls horizontally.
    #[expect(clippy::cast_precision_loss, reason = "column count is small")]
    pub(super) fn column_width(&self) -> Pixels {
        let count = self.columns.len().max(1);
        (self.list_width.get() / count as f32).clamp(px(COLUMN_MIN_WIDTH), px(COLUMN_MAX_WIDTH))
    }

    #[expect(clippy::too_many_lines, reason = "one declarative header")]
    fn results_header(&self, results: &super::Results, cx: &mut Context<Self>) -> Div {
        let p = *theme::palette(cx);
        let shown = self
            .table
            .as_ref()
            .map_or(0, |table| table.read(cx).delegate().rows.len());
        let truncated = results.total > shown;
        let detail = match &results.kind {
            ResultsKind::Search => {
                let base = if truncated {
                    format!(
                        "Largest {} of {} matches",
                        format::count(shown as u64),
                        format::count(results.total as u64)
                    )
                } else {
                    format::plural(shown, "match")
                };
                match self.search_scope {
                    SearchScope::Folder => format!("{base} · this folder"),
                    SearchScope::Scan => format!("{base} · everywhere"),
                }
            }
            ResultsKind::Largest => {
                format!("{} · across the scan", format::plural(shown, "file"))
            }
            ResultsKind::Suggestions => {
                let base = if truncated {
                    format!(
                        "Largest {} of {} items",
                        format::count(shown as u64),
                        format::count(results.total as u64)
                    )
                } else {
                    format::plural(shown, "item")
                };
                match results.rules {
                    Some(rules) => format!("{base} in {}", format::plural(rules, "cleanup rule")),
                    None => base,
                }
            }
        };
        let mut header = h_flex()
            .gap_2()
            .px(px(12.))
            .py(px(8.))
            .border_b_1()
            .border_color(p.line)
            .bg(p.panel)
            .child(
                div()
                    .id("results-title")
                    .role(Role::Label)
                    .aria_label(results.title.clone())
                    .font_weight(FontWeight::SEMIBOLD)
                    .flex_shrink_0()
                    .max_w(px(280.))
                    .truncate()
                    .child(results.title.clone()),
            )
            .child(if self.searching {
                div()
                    .id("results-detail")
                    .role(Role::Label)
                    .aria_label("Searching")
                    .flex_shrink_0()
                    .text_color(p.ink_2)
                    .child("Searching…")
            } else {
                div()
                    .id("results-detail")
                    .role(Role::Label)
                    .aria_label(detail.clone())
                    .text_color(p.ink_2)
                    .font_features(theme::tabular())
                    .min_w_0()
                    .truncate()
                    .child(detail)
            })
            .child(div().flex_1());
        if results.kind == ResultsKind::Search {
            for (label, scope) in [
                ("This folder", SearchScope::Folder),
                ("Everywhere", SearchScope::Scan),
            ] {
                header = header.child(
                    Button::new(label)
                        .label(label)
                        .xsmall()
                        .ghost()
                        .selected(self.search_scope == scope)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.search_scope = scope;
                            this.run_search(window, cx);
                            cx.notify();
                        })),
                );
            }
        }
        header
            .child(
                Button::new("stage-results")
                    .label("Add all")
                    .icon(IconName::Plus)
                    .xsmall()
                    .outline()
                    .on_click(cx.listener(|this, _, _, cx| this.stage_rows(cx))),
            )
            .child(
                Button::new("close-results")
                    .icon(IconName::X)
                    .xsmall()
                    .ghost()
                    .tooltip("Close (Esc)")
                    .accessibility_label("Close")
                    .on_click(cx.listener(|this, _, window, cx| this.close_results(window, cx))),
            )
    }
}

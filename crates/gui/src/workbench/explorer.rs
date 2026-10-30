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
    ParentElement as _, Role, StatefulInteractiveElement as _, Styled as _, Window, canvas, div,
    prelude::FluentBuilder as _, px, relative, white,
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
            .flex_1()
            .min_w_0()
            .min_h_0()
            .px(px(20.))
            .pt(px(18.))
            .pb(px(14.))
            .gap(px(14.));
        if let Some(warning) = &self.warning {
            main = main.child(Self::callout(warning.clone(), cx));
        }
        if self.table.is_none() {
            return main.child(if self.running.is_some() {
                self.scanning(cx).into_any_element()
            } else {
                self.welcome(cx).into_any_element()
            });
        }
        let chart = v_flex()
            .w(relative(0.52))
            .flex_shrink_0()
            .min_h_0()
            .gap(px(10.))
            .child(self.treemap(window, cx))
            .child(self.legend(cx))
            .child(self.detail(cx));
        main.child(self.summary(cx)).child(
            h_flex()
                .flex_1()
                .min_h_0()
                .items_stretch()
                .gap(px(16.))
                .child(chart)
                .child(self.list(cx)),
        )
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
    fn summary(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = *theme::palette(cx);
        let (Some(cached), Some(folder)) = (self.cached(cx), self.folder(cx)) else {
            return div().into_any_element();
        };
        let tree = &cached.tree;
        let node = tree.node(folder);
        let name =
            if folder == tree.root() && std::env::home_dir().as_deref() == Some(tree.root_path()) {
                "Home".to_owned()
            } else {
                tree.name(folder).to_string_lossy().into_owned()
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
            format!("{} files{source}", format::count(node.files()))
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
                .into_any_element()
        };
        h_flex()
            .items_end()
            .gap(px(24.))
            .child(
                v_flex()
                    .min_w_0()
                    .gap(px(6.))
                    .child(
                        h_flex()
                            .id("folder-size")
                            .role(Role::Label)
                            .aria_label(format!("{} in {name}", format::allocated(size)))
                            .items_baseline()
                            .gap_2()
                            .child(ui::display(format::allocated(size), 32.))
                            .child(
                                div()
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
                    .role(Role::Button)
                    .aria_label(reclaim_label)
                    .w(px(380.))
                    .flex_shrink_0()
                    .gap(px(8.))
                    .px(px(14.))
                    .py(px(12.))
                    .rounded(px(14.))
                    .bg(p.panel)
                    .border_1()
                    .border_color(p.line)
                    .cursor_pointer()
                    .hover(|style| style.border_color(p.ink_3))
                    .on_click(cx.listener(|this, _, window, cx| this.show_suggestions(window, cx)))
                    .child(headline)
                    .child(meter),
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

    fn treemap(&self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = *theme::palette(cx);
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
            .aria_label("Treemap of the open folder")
            .flex_1()
            .min_h(px(160.))
            .rounded(px(14.))
            .bg(p.panel)
            .border_1()
            .border_color(p.line)
            .p(px(8.))
            .overflow_hidden()
            .child(area)
    }

    #[expect(clippy::too_many_lines, reason = "one declarative view tree")]
    fn tile(&self, index: usize, block: &Block, parent: u64, cx: &mut Context<Self>) -> AnyElement {
        const GAP: f32 = 3.;
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
            .absolute()
            .left(px(block.x))
            .top(px(block.y))
            .w(px(width.max(1.)))
            .h(px(height.max(1.)))
            .px(px(8.))
            .py(px(7.))
            .overflow_hidden()
            .rounded(px(8.))
            .bg(fill)
            .text_color(p.tile_ink)
            .when(selected, |tile| tile.border_3().border_color(p.ink))
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
        for (offset, nested) in block.nested.iter().enumerate() {
            let id = nested.id;
            let tip = format!("{}\n{}", nested.name, format::allocated(nested.size));
            tile = tile.child(
                div()
                    .id(("nested", index * 64 + offset))
                    .absolute()
                    .left(px(nested.x + 1.))
                    .top(px(nested.y + 1.))
                    .w(px((nested.width - 3.).max(1.)))
                    .h(px((nested.height - 3.).max(1.)))
                    .rounded(px(5.))
                    .bg(p.shade)
                    .hover(|style| style.bg(p.shade.opacity(0.9).blend(white().opacity(0.12))))
                    .when(self.selection.is_selected(id), |tile| {
                        tile.border_2().border_color(p.ink)
                    })
                    .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        cx.stop_propagation();
                        this.pick(id, event.click_count(), window, cx);
                    }))
                    .when(nested.width > 80. && nested.height > 22., |tile| {
                        tile.child(
                            div()
                                .px(px(6.))
                                .pt(px(3.))
                                .truncate()
                                .text_size(px(10.5))
                                .font_weight(FontWeight::MEDIUM)
                                .opacity(0.8)
                                .child(nested.name.clone()),
                        )
                    }),
            );
        }
        if width >= 62. && height >= 34. {
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
                            Safety::Safe => IconName::Check,
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
        let mut legend = h_flex().flex_wrap().gap_x(px(14.)).gap_y_1().px_1();
        for category in Category::ALL {
            if blocks.iter().any(|block| block.category == category) {
                legend = legend.child(
                    h_flex()
                        .gap(px(6.))
                        .text_size(px(11.5))
                        .text_color(p.ink_2)
                        .child(ui::swatch(p.category(category), px(10.)))
                        .child(category.label()),
                );
            }
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
        let can_stage = subject != tree.root();
        let mut actions = h_flex()
            .gap(px(6.))
            .flex_shrink_0()
            .child(
                Button::new("reveal")
                    .label("Reveal")
                    .icon(IconName::Eye)
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
        if can_stage {
            actions = actions.child(Self::stage_button(self.selection.is_staged(subject), cx));
        }
        card.child(
            h_flex()
                .gap(px(16.))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(px(4.))
                        .child(
                            h_flex()
                                .gap_2()
                                .min_w_0()
                                .child(ui::swatch(p.category(category), px(12.)))
                                .child(div().min_w_0().truncate().child(ui::display(name, 15.)))
                                .children(group.map(|group| {
                                    ui::safety_chip(
                                        group.safety,
                                        ui::safety_text(group.safety),
                                        true,
                                        cx,
                                    )
                                })),
                        )
                        .child(facts)
                        .child(about),
                )
                .child(actions),
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
        let Some(table) = &self.table else {
            return div().into_any_element();
        };
        let measure = Rc::clone(&self.list_width);
        let view = cx.weak_entity();
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
        card.child(
            div()
                .relative()
                .flex_1()
                .min_h_0()
                .child(
                    canvas(
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
                    .size_full(),
                )
                .child(
                    DataTable::new(table)
                        .bordered(false)
                        .stripe(false)
                        .header_row_height(px(34.))
                        .with_size(Size::Size(match self.settings.density {
                            Density::Comfortable => px(38.),
                            Density::Compact => px(30.),
                        })),
                ),
        )
        .text_color(p.ink)
        .into_any_element()
    }

    fn results_header(&self, results: &super::Results, cx: &mut Context<Self>) -> Div {
        let p = *theme::palette(cx);
        let shown = self
            .table
            .as_ref()
            .map_or(0, |table| table.read(cx).delegate().rows.len());
        let detail = if results.total > shown {
            format!(
                "Largest {} of {}",
                format::count(shown as u64),
                format::count(results.total as u64)
            )
        } else {
            format::plural(shown, "item")
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
            .child(
                div()
                    .id("results-detail")
                    .role(Role::Label)
                    .aria_label(detail.clone())
                    .text_color(p.ink_2)
                    .font_features(theme::tabular())
                    .min_w_0()
                    .truncate()
                    .child(detail),
            )
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

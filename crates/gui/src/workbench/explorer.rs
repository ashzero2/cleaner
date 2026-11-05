use std::rc::Rc;
use std::sync::Arc;

use clrs_core::{EntryKind, NodeId};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::table::DataTable;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, Selectable as _, Sizable as _, Size, h_flex, v_flex};
use gpui_kit::{
    App, ClickEvent, Context, Div, FontWeight, Hsla, InteractiveElement as _, IntoElement,
    KeyDownEvent, MouseMoveEvent, ParentElement as _, Role, StatefulInteractiveElement as _,
    Styled as _, Window, canvas, div, prelude::FluentBuilder as _, px, relative, white,
};

use super::{ResultsKind, SearchScope, Workbench};
use crate::category::{self, Category};
use crate::settings::Density;
use crate::sunburst::{self, Geometry, Hit, Segment};
use crate::{format, theme, ui};

/// Laid-out rings for one folder of one scan, reused until either or the suggestions
/// change.
pub(super) struct MapCache {
    tree: usize,
    folder: NodeId,
    suggestions: usize,
    segments: Rc<Vec<Segment>>,
}

/// What the centre of the disk map describes.
struct Readout {
    name: String,
    size: u64,
    detail: String,
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
        if self.table.is_none() {
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
                        .w(relative(0.46))
                        .flex_shrink_0()
                        .min_h_0()
                        .gap(px(12.))
                        .child(self.disk_map(window, cx))
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

    fn segments(&self, cx: &Context<Self>) -> Option<Rc<Vec<Segment>>> {
        let cached = self.cached(cx)?;
        let folder = self.folder(cx)?;
        let tree = Arc::as_ptr(&cached) as usize;
        let suggestions = Arc::as_ptr(&self.suggestions) as usize;
        let mut cache = self.map_cache.borrow_mut();
        if let Some(hit) = cache.as_ref().filter(|hit| {
            hit.tree == tree && hit.folder == folder && hit.suggestions == suggestions
        }) {
            return Some(Rc::clone(&hit.segments));
        }
        let segments = Rc::new(sunburst::segments(&cached.tree, folder, &self.suggestions));
        *cache = Some(MapCache {
            tree,
            folder,
            suggestions,
            segments: Rc::clone(&segments),
        });
        Some(segments)
    }

    /// The segment the pointer is over, or else the one for the hovered list row.
    fn highlighted(&self, segments: &[Segment]) -> Option<usize> {
        match self.map_hover {
            Some(Hit::Segment(index)) => Some(index),
            Some(Hit::Center) => None,
            None => self
                .hovered
                .and_then(|id| segments.iter().position(|segment| segment.id == Some(id))),
        }
    }

    #[expect(clippy::too_many_lines, reason = "one declarative view tree")]
    fn disk_map(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = *theme::palette(cx);
        let focus = window
            .use_keyed_state("disk-map", cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let focused = focus.is_focused(window);
        let segments = self.segments(cx).unwrap_or_default();
        let highlighted = self.highlighted(&segments);
        let lift = white().opacity(if cx.theme().is_dark() { 0.22 } else { 0.32 });
        let fills: Vec<Hsla> = segments
            .iter()
            .enumerate()
            .map(|(index, segment)| {
                let base = if segment.id.is_none() {
                    p.ink_3.opacity(0.45)
                } else {
                    p.category(segment.category)
                };
                let fill = base.blend(p.panel.opacity(0.14 * f32::from(segment.depth - 1)));
                if highlighted.is_some_and(|at| sunburst::within(&segments, index, at)) {
                    fill.blend(lift)
                } else {
                    fill
                }
            })
            .collect();
        let selected: Vec<usize> = segments
            .iter()
            .enumerate()
            .filter(|(_, segment)| segment.id.is_some_and(|id| self.selection.is_selected(id)))
            .map(|(index, _)| index)
            .collect();
        let center_fill = if self.map_hover == Some(Hit::Center) {
            p.accent_soft
        } else {
            p.raised
        };
        let cell = Rc::clone(&self.map_geometry);
        let view = cx.weak_entity();
        let painted = Rc::clone(&segments);
        let chart = canvas(
            move |bounds, _, cx| {
                let geometry = Geometry::new(
                    bounds.center(),
                    f32::from(bounds.size.width),
                    f32::from(bounds.size.height),
                );
                if cell.get() != Some(geometry) {
                    cell.set(Some(geometry));
                    let view = view.clone();
                    cx.defer(move |cx| {
                        let _ = view.update(cx, |_, cx| cx.notify());
                    });
                }
                geometry
            },
            move |_, geometry, window, _| {
                for (segment, fill) in painted.iter().zip(&fills) {
                    if let Some(path) = geometry.wedge(segment, None) {
                        window.paint_path(path, *fill);
                    }
                }
                for &index in &selected {
                    if let Some(path) = geometry.wedge(&painted[index], Some(px(2.))) {
                        window.paint_path(path, p.ink);
                    }
                }
                if let Some(path) = geometry.disc() {
                    window.paint_path(path, center_fill);
                }
            },
        )
        .absolute()
        .size_full();
        let readout = self.readout(&segments, cx);
        let hole = self
            .map_geometry
            .get()
            .map_or(70., |geometry| geometry.hole);
        let label = readout.as_ref().map_or_else(String::new, |readout| {
            format!(
                ", {}, {}, {}",
                readout.name,
                format::allocated(readout.size),
                readout.detail
            )
        });
        let center = readout.map(|readout| {
            v_flex()
                .w(px((hole * 1.6).max(60.)))
                .items_center()
                .gap(px(2.))
                .child(
                    div()
                        .w_full()
                        .truncate()
                        .text_center()
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(p.ink)
                        .child(readout.name),
                )
                .child(ui::display(
                    format::allocated(readout.size),
                    (hole / 4.).clamp(13., 22.),
                ))
                .child(
                    div()
                        .w_full()
                        .truncate()
                        .text_center()
                        .text_size(px(11.))
                        .text_color(p.ink_2)
                        .font_features(theme::tabular())
                        .child(readout.detail),
                )
        });
        let ids = Rc::new(
            segments
                .iter()
                .filter(|segment| segment.depth == 1)
                .filter_map(|segment| segment.id)
                .collect::<Vec<_>>(),
        );
        let mut area = div().relative().size_full().child(chart).child(
            v_flex()
                .absolute()
                .size_full()
                .items_center()
                .justify_center()
                .children(center),
        );
        if segments.is_empty() && self.map_geometry.get().is_some() {
            area = area.child(
                div()
                    .absolute()
                    .bottom_0()
                    .w_full()
                    .p_4()
                    .text_center()
                    .text_color(p.ink_2)
                    .child("Nothing in this folder takes up space."),
            );
        }
        div()
            .id("disk-map")
            .role(Role::Group)
            .aria_label(format!(
                "Disk map of the open folder, {}{label}",
                format::plural(ids.len(), "item")
            ))
            .track_focus(&focus)
            .tab_index(0)
            .on_key_down(cx.listener({
                let ids = Rc::clone(&ids);
                move |this, event: &KeyDownEvent, window, cx| {
                    this.map_key(event, &ids, window, cx);
                }
            }))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                let hit = this
                    .map_geometry
                    .get()
                    .zip(this.segments(cx))
                    .and_then(|(geometry, segments)| geometry.hit(event.position, &segments));
                this.set_map_hover(hit, cx);
            }))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if !*hovered {
                    this.set_map_hover(None, cx);
                }
            }))
            .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                this.map_click(event, window, cx);
            }))
            .when(self.map_hover.is_some(), gpui_kit::Styled::cursor_pointer)
            .flex_1()
            .min_h(px(220.))
            .rounded(px(14.))
            .bg(p.panel)
            .border_1()
            .border_color(p.line)
            .p(px(8.))
            .overflow_hidden()
            .child(area)
            .when(focused, |map| {
                map.child(ui::focus_inside(px(14.), p.accent_fill))
            })
    }

    /// What the centre of the map names: the hovered wedge, the hovered list row, or the
    /// open folder.
    fn readout(&self, segments: &[Segment], cx: &App) -> Option<Readout> {
        let cached = self.cached(cx)?;
        let folder = self.folder(cx)?;
        let tree = &cached.tree;
        let total = tree.node(folder).size();
        let name = |id: NodeId| {
            if id == tree.root() {
                format::name(tree.root_path())
            } else {
                tree.name(id).to_string_lossy().into_owned()
            }
        };
        let item = |id: NodeId| {
            let size = tree.node(id).size();
            Readout {
                name: name(id),
                size,
                detail: format!(
                    "{} · {}",
                    format::percent(ui::share(size, total)),
                    category::classify(tree, id, &self.suggestions).label()
                ),
            }
        };
        let open = || Readout {
            name: name(folder),
            size: total,
            detail: format!("{} files", format::count(tree.node(folder).files())),
        };
        Some(match self.map_hover {
            Some(Hit::Segment(index)) => {
                let segment = segments.get(index)?;
                match segment.id {
                    Some(id) => item(id),
                    None => Readout {
                        name: format!("{} smaller items", format::count(segment.folded as u64)),
                        size: segment.size,
                        detail: format::percent(ui::share(segment.size, total)),
                    },
                }
            }
            Some(Hit::Center) => tree.parent(folder).map_or_else(open, |parent| Readout {
                name: format!("Back to {}", name(parent)),
                size: tree.node(parent).size(),
                detail: "Click to go up".to_owned(),
            }),
            None => self.hovered.map_or_else(open, item),
        })
    }

    fn set_map_hover(&mut self, hit: Option<Hit>, cx: &mut Context<Self>) {
        if self.map_hover == hit {
            return;
        }
        self.map_hover = hit;
        let id = match hit {
            Some(Hit::Segment(index)) => self
                .segments(cx)
                .and_then(|segments| segments.get(index).and_then(|segment| segment.id)),
            _ => None,
        };
        self.set_hovered(id, cx);
        cx.notify();
    }

    /// Clicking a folder's wedge opens it, a file's wedge selects it, and the centre goes
    /// up a level.
    fn map_click(&mut self, event: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.click_count() > 1 {
            return;
        }
        let (Some(cached), Some(segments), Some(geometry)) =
            (self.cached(cx), self.segments(cx), self.map_geometry.get())
        else {
            return;
        };
        match geometry.hit(event.position(), &segments) {
            Some(Hit::Center) => {
                self.map_hover = None;
                self.parent(window, cx);
            }
            Some(Hit::Segment(index)) => {
                let Some(id) = segments.get(index).and_then(|segment| segment.id) else {
                    return;
                };
                if cached.tree.node(id).kind() == EntryKind::Dir {
                    self.map_hover = None;
                    self.navigate(&cached, id, false, window, cx);
                } else {
                    self.pick(id, 1, window, cx);
                }
            }
            None => {}
        }
    }

    /// Arrow keys move through the inner ring, Return opens the selection and Delete goes
    /// up. The list stays the complete, scrollable version of the same data.
    fn map_key(
        &mut self,
        event: &KeyDownEvent,
        ids: &[NodeId],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.keystroke.modifiers.modified() {
            return;
        }
        let key = event.keystroke.key.as_str();
        if key == "backspace" {
            self.parent(window, cx);
            cx.stop_propagation();
            return;
        }
        if ids.is_empty() {
            return;
        }
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
        let segments = self.segments(cx).unwrap_or_default();
        let present: Vec<Category> = Category::ALL
            .into_iter()
            .filter(|category| {
                segments
                    .iter()
                    .any(|segment| segment.depth == 1 && segment.category == *category)
            })
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
        ui::card(cx).into_any_element()
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

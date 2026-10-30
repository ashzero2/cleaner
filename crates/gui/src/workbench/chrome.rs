use std::path::Path;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Disableable as _, Sizable as _, TitleBar, h_flex, v_flex};
use gpui_kit::{
    AnyElement, ClickEvent, Context, Div, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, Role, SharedString, Stateful, StatefulInteractiveElement as _, Styled as _,
    Window, div, prelude::FluentBuilder as _, px, relative,
};

use super::Workbench;
use crate::{format, panels, theme, ui};

/// Ancestors shown in the breadcrumb before the middle collapses to "…".
const CRUMBS: usize = 4;

impl Workbench {
    pub(super) fn toolbar(
        &self,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let p = *theme::palette(cx);
        let scanning = self.running.is_some();
        let has_scan = self.table.is_some();
        let back = Button::new("back")
            .icon(IconName::ChevronLeft)
            .ghost()
            .small()
            .tooltip("Back")
            .accessibility_label("Back")
            .disabled(self.history_pos == 0 || !has_scan)
            .on_click(cx.listener(|this, _, window, cx| this.travel(false, window, cx)));
        let forward = Button::new("forward")
            .icon(IconName::ChevronRight)
            .ghost()
            .small()
            .tooltip("Forward")
            .accessibility_label("Forward")
            .disabled(self.history_pos + 1 >= self.history.len() || !has_scan)
            .on_click(cx.listener(|this, _, window, cx| this.travel(true, window, cx)));
        let mut right = h_flex().gap_2().flex_shrink_0();
        if has_scan {
            right = right.child(
                div().w(px(250.)).child(
                    Input::new(&self.search)
                        .small()
                        .cleanable(true)
                        .prefix(ui::icon(IconName::Search).small().text_color(p.ink_3)),
                ),
            );
        }
        if let Some(cached) = self.cached(cx)
            && !cached.issues.is_empty()
        {
            let issues = cached.issues.len();
            let count = issues as u64;
            right = right.child(
                Button::new("locked")
                    .icon(IconName::Lock)
                    .label(format!("{} locked", format::count(count)))
                    .small()
                    .outline()
                    .rounded(px(999.))
                    .tooltip("Folders macOS didn't let cl-rs read")
                    .accessibility_label(format!(
                        "{} locked, view details",
                        format::plural(issues, "folder")
                    ))
                    .on_click(cx.listener(|this, _, window, cx| this.issues(window, cx))),
            );
        }
        if scanning {
            right = right.child(
                Button::new("stop")
                    .label("Stop")
                    .small()
                    .outline()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.jobs.cancel();
                        cx.notify();
                    })),
            );
        } else if self.root.is_some() {
            right = right.child(
                Button::new("refresh")
                    .icon(IconName::RefreshCw)
                    .when(self.failed, |button| button.label("Retry"))
                    .ghost()
                    .small()
                    .tooltip("Rescan (⌘R)")
                    .accessibility_label(if self.failed { "Retry" } else { "Rescan" })
                    .on_click(cx.listener(|this, _, _, cx| this.refresh(false, cx))),
            );
        }
        TitleBar::new()
            .h(px(54.))
            .bg(p.chrome)
            .border_color(p.line)
            .child(
                h_flex()
                    .w_full()
                    .h_full()
                    .pl(px(8.))
                    .pr(px(14.))
                    .gap_1()
                    .child(back)
                    .child(forward)
                    .child(self.breadcrumb(cx))
                    .child(div().flex_1())
                    .child(right),
            )
    }

    fn breadcrumb(&self, cx: &mut Context<Self>) -> Div {
        let p = *theme::palette(cx);
        let mut crumbs = h_flex().ml_2().gap_0p5().min_w_0().overflow_hidden();
        let (Some(cached), Some(folder)) = (self.cached(cx), self.folder(cx)) else {
            if let Some(root) = &self.root {
                crumbs = crumbs.child(
                    div()
                        .px_2()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format::name(root)),
                );
            }
            return crumbs;
        };
        let mut ancestors = Vec::new();
        let mut current = Some(folder);
        while let Some(id) = current {
            ancestors.push(id);
            current = cached.tree.parent(id);
        }
        ancestors.reverse();
        let hidden = ancestors.len().saturating_sub(CRUMBS);
        for (index, &id) in ancestors.iter().enumerate() {
            if index > 0 && index <= hidden {
                if index == 1 {
                    crumbs = crumbs
                        .child(div().text_color(p.ink_3).child("›"))
                        .child(div().px_1().text_color(p.ink_2).child("…"));
                }
                continue;
            }
            if index > 0 {
                crumbs = crumbs.child(div().text_color(p.ink_3).child("›"));
            }
            let path = cached.tree.path(id);
            let target = std::sync::Arc::clone(&cached);
            let last = index + 1 == ancestors.len();
            let mut button = Button::new(("crumb", index))
                .label(format::name(&path))
                .tooltip(format::path(&path))
                .ghost()
                .small()
                .max_w(px(180.))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.navigate(std::sync::Arc::clone(&target), id, true, window, cx);
                }));
            if index == 0 && is_home(&path) {
                button = button.icon(IconName::House);
            }
            if !last {
                button = button.text_color(p.ink_2);
            }
            crumbs = crumbs.child(button);
        }
        crumbs
    }

    #[expect(clippy::too_many_lines, reason = "one declarative view tree")]
    pub(super) fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = *theme::palette(cx);
        let cached = self.cached(cx);
        let home = std::env::home_dir();
        let at_home = self
            .root
            .as_deref()
            .is_some_and(|root| home.as_deref() == Some(root));
        let root_size = cached
            .as_ref()
            .map(|cached| format::allocated(cached.tree.node(cached.tree.root()).size()));
        let mut places = section("Places", cx).child(nav(
            "home",
            IconName::House,
            "Home",
            at_home.then(|| root_size.clone()).flatten(),
            at_home && self.results.is_none(),
            cx.listener(|this, _, _, cx| this.home(cx)),
            cx,
        ));
        if let Some(root) = self.root.as_deref().filter(|_| !at_home) {
            places = places.child(nav(
                "current-root",
                IconName::Folder,
                format::name(root),
                root_size.clone(),
                self.results.is_none(),
                cx.listener(|this, _, window, cx| {
                    if let (Some(cached), true) = (this.cached(cx), this.results.is_some()) {
                        let root = cached.tree.root();
                        this.navigate(cached, root, true, window, cx);
                    }
                }),
                cx,
            ));
        }
        places = places.child(nav(
            "choose-folder",
            IconName::FolderPlus,
            "Choose folder…",
            None,
            false,
            cx.listener(|_, _, window, cx| Self::choose(window, cx)),
            cx,
        ));
        let mut sidebar = v_flex()
            .id("sidebar")
            .w(px(theme::SIDEBAR_WIDTH))
            .flex_shrink_0()
            .h_full()
            .px(px(10.))
            .py(px(14.))
            .gap(px(18.))
            .bg(p.chrome)
            .border_r_1()
            .border_color(p.line)
            .child(places);
        if let Some(cached) = &cached {
            let kind = self.results.as_ref().map(|results| results.kind.clone());
            let suggestions = self.suggestions.count();
            let issues = cached.issues.len();
            let mut find = section("Find", cx).child(
                nav(
                    "suggestions",
                    IconName::Sparkles,
                    "Suggestions",
                    None,
                    kind == Some(super::ResultsKind::Suggestions),
                    cx.listener(|this, _, window, cx| this.show_suggestions(window, cx)),
                    cx,
                )
                .when(suggestions > 0, |item| item.child(badge(suggestions, cx))),
            );
            find = find.child(nav(
                "largest-files",
                IconName::FileText,
                "Largest files",
                None,
                kind == Some(super::ResultsKind::Largest),
                cx.listener(|this, _, window, cx| this.largest_files(window, cx)),
                cx,
            ));
            if issues > 0 {
                find = find.child(nav(
                    "locked-folders",
                    IconName::Lock,
                    "Locked folders",
                    Some(format::count(issues as u64)),
                    false,
                    cx.listener(|this, _, window, cx| this.issues(window, cx)),
                    cx,
                ));
            }
            sidebar = sidebar.child(find);
        }
        let recent: Vec<_> = self
            .settings
            .recent
            .iter()
            .filter(|root| Some(root.as_path()) != self.root.as_deref())
            .take(5)
            .cloned()
            .collect();
        if !recent.is_empty() {
            let mut section = section("Recent", cx);
            for (index, root) in recent.into_iter().enumerate() {
                let label = format::path(&root);
                section = section.child(
                    nav(
                        ("recent", index),
                        IconName::Clock,
                        label.trim_start_matches("~/").to_owned(),
                        None,
                        false,
                        cx.listener(move |this, _, _, cx| this.scan(root.clone(), false, cx)),
                        cx,
                    )
                    .tooltip(move |window, cx| Tooltip::new(label.clone()).build(window, cx)),
                );
            }
            sidebar = sidebar.child(section);
        }
        sidebar.child(div().flex_1()).children(self.volume_card(cx))
    }

    fn volume_card(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let p = *theme::palette(cx);
        let volume = self.volume?;
        let used = volume.total.saturating_sub(volume.available);
        let name = self
            .root
            .as_deref()
            .map_or_else(|| "Startup disk".into(), volume_name);
        let summary = format!(
            "{} free of {}",
            format::allocated(volume.available),
            format::allocated(volume.total)
        );
        Some(
            v_flex()
                .id("volume")
                .role(Role::Group)
                .aria_label(format!("{name}: {summary}"))
                .gap(px(6.))
                .p(px(10.))
                .rounded(px(10.))
                .bg(p.raised)
                .border_1()
                .border_color(p.line)
                .child(
                    h_flex()
                        .gap_1()
                        .child(ui::icon(IconName::HardDrive).small().text_color(p.ink_2))
                        .child(
                            div()
                                .flex_1()
                                .truncate()
                                .text_size(px(12.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(name),
                        )
                        .child(
                            Button::new("settings")
                                .icon(IconName::Settings)
                                .ghost()
                                .xsmall()
                                .tooltip("Settings (⌘,)")
                                .accessibility_label("Settings")
                                .on_click(cx.listener(|_, _, window, cx| {
                                    panels::settings(window, cx);
                                })),
                        ),
                )
                .child(
                    div()
                        .h(px(7.))
                        .w_full()
                        .rounded_full()
                        .bg(p.line)
                        .overflow_hidden()
                        .child(
                            div()
                                .h_full()
                                .rounded_full()
                                .bg(p.accent)
                                .w(relative(ui::portion(used, volume.total))),
                        ),
                )
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(p.ink_2)
                        .font_features(theme::tabular())
                        .child(summary),
                ),
        )
    }
}

fn is_home(path: &Path) -> bool {
    std::env::home_dir().is_some_and(|home| home == path)
}

/// The display name of the volume holding `path`: a mounted volume's name, or the startup disk.
fn volume_name(path: &Path) -> String {
    let mut components = path.components().skip(1);
    match (components.next(), components.next()) {
        (Some(first), Some(second)) if first.as_os_str() == "Volumes" => {
            second.as_os_str().to_string_lossy().into_owned()
        }
        _ => "Startup disk".into(),
    }
}

fn section(title: &'static str, cx: &Context<Workbench>) -> Div {
    v_flex()
        .gap(px(2.))
        .child(ui::eyebrow(title.to_uppercase(), cx).px(px(10.)).pb(px(4.)))
}

fn badge(count: usize, cx: &Context<Workbench>) -> impl IntoElement + use<> {
    let p = theme::palette(cx);
    div()
        .ml_auto()
        .min_w(px(20.))
        .h(px(18.))
        .px(px(6.))
        .rounded_full()
        .bg(p.accent_fill)
        .text_color(p.on_accent)
        .text_size(px(11.))
        .font_weight(FontWeight::BOLD)
        .flex()
        .items_center()
        .justify_center()
        .child(count.to_string())
}

/// A sidebar row: icon, label, and an optional value on the right.
fn nav(
    id: impl Into<gpui_kit::ElementId>,
    icon: IconName,
    label: impl Into<SharedString>,
    meta: Option<String>,
    active: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut gpui_kit::App) + 'static,
    cx: &Context<Workbench>,
) -> Stateful<Div> {
    let p = *theme::palette(cx);
    let label: SharedString = label.into();
    h_flex()
        .id(id)
        .role(Role::Button)
        .aria_label(label.clone())
        .h(px(30.))
        .px(px(10.))
        .gap(px(9.))
        .rounded(px(9.))
        .cursor_pointer()
        .when(active, |row| row.bg(p.accent_soft))
        .when(!active, |row| {
            row.hover(|style| style.bg(p.ink.opacity(0.05)))
        })
        .on_click(on_click)
        .child(ui::icon(icon).small().text_color(p.accent))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_weight(if active {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::MEDIUM
                })
                .child(label),
        )
        .children(meta.map(|meta| -> AnyElement {
            div()
                .text_size(px(12.))
                .text_color(p.ink_2)
                .font_features(theme::tabular())
                .child(meta)
                .into_any_element()
        }))
}

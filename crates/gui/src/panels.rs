use std::path::PathBuf;
use std::sync::Arc;

use clrs_core::{Cached, NodeId, Safety};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{
    Disableable as _, Selectable as _, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::{
    App, Context, Div, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _, Role,
    Stateful, StatefulInteractiveElement as _, Styled as _, Window, div, px, relative,
    uniform_list,
};

use crate::settings::{Appearance, Density};
use crate::workbench::{CleanReport, Cleaning, Workbench};
use crate::{format, theme, ui};

#[derive(Debug)]
pub(crate) struct ReviewItem {
    pub(crate) id: NodeId,
    pub(crate) target: PathBuf,
    pub(crate) path: String,
    /// The rule's name for rule matches, otherwise the file or folder name.
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) safety: Option<Safety>,
    pub(crate) size: u64,
    pub(crate) command: Option<Vec<String>>,
}

impl ReviewItem {
    fn action(&self) -> String {
        self.command.as_ref().map_or_else(
            || format!("Move {} to Trash", self.path),
            |command| {
                format!(
                    "Runs {} (moves to Trash if not installed)",
                    command.join(" ")
                )
            },
        )
    }
}

fn title(text: impl Into<gpui_kit::SharedString>) -> impl IntoElement {
    let text = text.into();
    div()
        .id("dialog-title")
        .role(Role::Heading)
        .aria_label(text.clone())
        .child(ui::display(text, 18.))
}

fn note(text: &'static str, cx: &App) -> Stateful<Div> {
    div()
        .id(text)
        .role(Role::Label)
        .aria_label(text)
        .text_color(theme::palette(cx).ink_2)
        .line_height(relative(1.45))
        .child(text)
}

/// Everything in the tray, what clearing each does, and the button that does it. The same
/// sheet then shows progress and the result.
#[expect(clippy::too_many_lines, reason = "one declarative view tree")]
pub(crate) fn review(window: &mut Window, cx: &mut Context<Workbench>) {
    let view = cx.entity();
    window.open_dialog(cx, move |dialog, _, cx| {
        let p = *theme::palette(cx);
        let workbench = view.read(cx);
        if let Cleaning::Done(report) = workbench.cleaning().clone() {
            return done(dialog, &report, view.clone(), cx);
        }
        let running = match workbench.cleaning() {
            Cleaning::Running { total } => Some(*total),
            _ => None,
        };
        let items = Arc::new(workbench.review(cx));
        let total: u64 = items.iter().map(|item| item.size).sum();
        let count = items.len();
        let can_clean = workbench.can_clean();
        let check_first = items
            .iter()
            .any(|item| item.safety != Some(Safety::Safe));
        let list = Arc::clone(&items);
        let body = if count == 0 {
            v_flex().child(note(
                "The tray is empty. Use + on a row or tile, or Add to tray in the detail card.",
                cx,
            ))
        } else {
            v_flex()
                .gap(px(14.))
                .child(note(
                    "These go to the Trash, so you can still put them back. Tools with their own cleanup command run that instead.",
                    cx,
                ))
                .child(
                    div()
                        .h(px(76. * f32::from(u8::try_from(count.min(5)).unwrap_or(5))))
                        .rounded(px(12.))
                        .border_1()
                        .border_color(p.line)
                        .overflow_hidden()
                        .child(
                            uniform_list("review-items", count, move |range, _, cx| {
                                let p = *theme::palette(cx);
                                range
                                    .map(|index| {
                                        let item = &list[index];
                                        let action = item.action();
                                        let chip = match item.safety {
                                            Some(safety) => ui::safety_chip(safety, ui::safety_text(safety), true, cx),
                                            None => ui::safety_chip(Safety::Review, "Your pick", true, cx),
                                        };
                                        h_flex()
                                            .id(index)
                                            .role(Role::ListItem)
                                            .aria_label(format!(
                                                "{}: {}. {action}",
                                                item.name,
                                                format::allocated(item.size)
                                            ))
                                            .w_full()
                                            .h(px(76.))
                                            .px(px(14.))
                                            .gap(px(12.))
                                            .border_b_1()
                                            .border_color(p.line)
                                            .child(
                                                v_flex()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .gap(px(3.))
                                                    .child(
                                                        h_flex()
                                                            .gap_2()
                                                            .child(div().truncate().font_weight(FontWeight::SEMIBOLD).child(item.name.clone()))
                                                            .child(chip),
                                                    )
                                                    .child(
                                                        div()
                                                            .truncate()
                                                            .text_size(px(12.))
                                                            .text_color(p.ink_2)
                                                            .child(item.description.clone().unwrap_or_else(|| item.path.clone())),
                                                    )
                                                    .child(
                                                        div()
                                                            .truncate()
                                                            .font_family(theme::MONO_FONT)
                                                            .text_size(px(11.))
                                                            .text_color(p.ink_3)
                                                            .child(action),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .flex_shrink_0()
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .font_features(theme::tabular())
                                                    .child(format::allocated(item.size)),
                                            )
                                    })
                                    .collect()
                            })
                            .size_full(),
                        ),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .text_size(px(12.))
                        .text_color(if check_first { p.review } else { p.ink_2 })
                        .child(ui::icon(IconName::Info).small())
                        .child(if check_first {
                            "Some items need a second look. They're marked “Check first” or “Your pick”."
                        } else {
                            "Everything here is safe to clear."
                        }),
                )
        };
        let clear = view.clone();
        let cleaner = view.clone();
        let heading = if count == 0 {
            "Nothing to review".to_owned()
        } else {
            format!("Clear {}?", format::allocated(total))
        };
        dialog
            .title(title(heading))
            .w(px(580.))
            .child(body)
            .footer(
                h_flex()
                    .w_full()
                    .gap_2()
                    .child(
                        Button::new("clear-staged")
                            .label("Empty tray")
                            .ghost()
                            .disabled(count == 0 || running.is_some())
                            .on_click(move |_, _, cx| {
                                clear.update(cx, Workbench::clear_staged);
                                cx.refresh_windows();
                            }),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("close-review")
                            .label(if count == 0 { "Done" } else { "Cancel" })
                            .outline()
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("clean-staged")
                            .label(if let Some(total) = running {
                                format!("Clearing {}…", format::plural(total, "item"))
                            } else {
                                "Move to Trash".to_owned()
                            })
                            .icon(IconName::Trash)
                            .loading(running.is_some())
                            .danger()
                            .disabled(count == 0 || !can_clean)
                            .on_click(move |_, _, cx| {
                                cleaner.update(cx, Workbench::clean_staged);
                                cx.refresh_windows();
                            }),
                    ),
            )
    });
}

fn done(
    dialog: gpui_kit::component::dialog::Dialog,
    report: &CleanReport,
    view: gpui_kit::Entity<Workbench>,
    cx: &App,
) -> gpui_kit::component::dialog::Dialog {
    let p = *theme::palette(cx);
    let headline = format!("{} is on its way out", format::allocated(report.freed));
    let mut body = v_flex()
        .items_center()
        .gap(px(10.))
        .py(px(8.))
        .child(
            div()
                .size(px(96.))
                .rounded_full()
                .border(px(10.))
                .border_color(p.safe)
                .flex()
                .items_center()
                .justify_center()
                .child(ui::icon(IconName::Check).size(px(40.)).text_color(p.safe)),
        )
        .child(
            div()
                .id("clean-result")
                .role(Role::Status)
                .aria_label(headline.clone())
                .child(ui::display(headline, 22.)),
        )
        .child(div().text_color(p.ink_2).text_center().child(format!(
            "Cleared {} of {} items. Empty the Trash to get the space back.",
            report.cleaned,
            report.cleaned + report.failures.len()
        )));
    if !report.failures.is_empty() {
        body = body.child(
            v_flex()
                .w_full()
                .gap_1()
                .p(px(10.))
                .rounded(px(10.))
                .bg(p.review_soft)
                .text_size(px(12.))
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(p.danger)
                        .child("These couldn't be cleared and are still in the tray:"),
                )
                .children(
                    report
                        .failures
                        .iter()
                        .map(|failure| div().child(failure.clone())),
                ),
        );
    }
    let closer = view.clone();
    dialog
        .w(px(480.))
        .child(body)
        .on_close(move |_, _, cx| closer.update(cx, Workbench::finish_review))
        .footer(
            h_flex()
                .w_full()
                .justify_center()
                .gap_2()
                .child(
                    Button::new("open-trash")
                        .label("Open Trash")
                        .outline()
                        .on_click(|_, _, cx| {
                            if let Some(home) = std::env::home_dir() {
                                cx.open_with_system(&home.join(".Trash"));
                            }
                        }),
                )
                .child(Button::new("done-review").label("Done").primary().on_click(
                    move |_, window, cx| {
                        view.update(cx, Workbench::finish_review);
                        window.close_dialog(cx);
                    },
                )),
        )
}

#[expect(clippy::too_many_lines, reason = "one declarative view tree")]
pub(crate) fn issues(cached: Arc<Cached>, window: &mut Window, cx: &mut App) {
    let needs_access = cached
        .issues
        .iter()
        .any(clrs_core::ScanIssue::needs_full_disk_access);
    window.open_dialog(cx, move |dialog, _, cx| {
        let p = *theme::palette(cx);
        let data = Arc::clone(&cached);
        let count = data.issues.len();
        let mut content = v_flex().gap(px(14.)).child(note(
            "macOS didn't let cl-rs read these folders, so what's inside isn't counted in the totals.",
            cx,
        ));
        if needs_access {
            let step = |number: &'static str, text: &'static str| {
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .size(px(20.))
                            .rounded_full()
                            .bg(p.accent_soft)
                            .text_color(p.accent_fill)
                            .text_size(px(11.))
                            .font_weight(FontWeight::BOLD)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(number),
                    )
                    .child(text)
            };
            content = content.child(
                v_flex()
                    .gap(px(8.))
                    .p(px(14.))
                    .rounded(px(12.))
                    .bg(p.panel)
                    .border_1()
                    .border_color(p.line)
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Let cl-rs see everything"))
                    .child(step("1", "Open Privacy & Security → Full Disk Access."))
                    .child(step("2", "Turn on cl-rs (add it with + if it isn't listed)."))
                    .child(step("3", "Quit and reopen cl-rs, then rescan."))
                    .child(
                        Button::new("privacy-settings")
                            .label("Open Privacy Settings")
                            .small()
                            .outline()
                            .on_click(|_, _, cx| {
                                cx.open_url("x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles");
                            }),
                    ),
            );
        }
        content = content.child(
            div()
                .h(px(260.))
                .rounded(px(12.))
                .border_1()
                .border_color(p.line)
                .overflow_hidden()
                .child(
                    uniform_list("scan-issues", count, move |range, _, cx| {
                        let p = *theme::palette(cx);
                        range
                            .map(|index| {
                                let issue = &data.issues[index];
                                let path = format::path(&issue.path);
                                let reason = reason(issue);
                                h_flex()
                                    .id(index)
                                    .role(Role::ListItem)
                                    .aria_label(format!("{path}: {reason}"))
                                    .w_full()
                                    .h(px(52.))
                                    .px(px(12.))
                                    .gap(px(10.))
                                    .border_b_1()
                                    .border_color(p.line)
                                    .child(ui::icon(IconName::Lock).small().text_color(p.ink_3))
                                    .child(
                                        v_flex()
                                            .min_w_0()
                                            .gap(px(2.))
                                            .child(
                                                div()
                                                    .truncate()
                                                    .font_family(theme::MONO_FONT)
                                                    .text_size(px(11.5))
                                                    .child(path),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .text_color(p.ink_2)
                                                    .child(reason),
                                            ),
                                    )
                            })
                            .collect()
                    })
                    .size_full(),
                ),
        );
        dialog
            .title(title(format!("{} couldn't be read", format::plural(count, "folder"))))
            .w(px(580.))
            .child(content)
            .footer(
                h_flex().w_full().justify_end().child(
                    Button::new("close-issues")
                        .label("Done")
                        .primary()
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                ),
            )
    });
}

fn reason(issue: &clrs_core::ScanIssue) -> &'static str {
    if issue.needs_full_disk_access() {
        return "macOS blocked access";
    }
    match issue.error.raw_os_error() {
        Some(2) => "Removed while scanning",
        Some(13) => "No permission to read",
        _ => "Couldn't be read",
    }
}

#[expect(clippy::too_many_lines, reason = "one declarative view tree")]
pub(crate) fn settings(window: &mut Window, cx: &mut Context<Workbench>) {
    let view = cx.entity();
    window.open_dialog(cx, move |dialog, _, cx| {
        let p = *theme::palette(cx);
        let settings = view.read(cx).settings.clone();
        let row = |label: &'static str, control: gpui_kit::AnyElement| {
            h_flex()
                .gap_4()
                .py(px(10.))
                .border_b_1()
                .border_color(p.line)
                .child(
                    div()
                        .id(label)
                        .role(Role::Label)
                        .aria_label(label)
                        .flex_1()
                        .font_weight(FontWeight::MEDIUM)
                        .child(label),
                )
                .child(control)
        };
        let mut appearance = h_flex().gap_1();
        for (label, value) in [
            ("System", Appearance::System),
            ("Light", Appearance::Light),
            ("Dark", Appearance::Dark),
        ] {
            let view = view.clone();
            appearance = appearance.child(
                Button::new(label)
                    .label(label)
                    .small()
                    .ghost()
                    .selected(settings.appearance == value)
                    .on_click(move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            this.settings.appearance = value;
                            theme::apply(value, cx);
                            this.persist(cx);
                            cx.notify();
                        });
                        cx.refresh_windows();
                    }),
            );
        }
        let mut density = h_flex().gap_1();
        for (label, value) in [
            ("Comfortable", Density::Comfortable),
            ("Compact", Density::Compact),
        ] {
            let view = view.clone();
            density = density.child(
                Button::new(label)
                    .label(label)
                    .small()
                    .ghost()
                    .selected(settings.density == value)
                    .on_click(move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            this.settings.density = value;
                            this.persist(cx);
                            cx.notify();
                        });
                        cx.refresh_windows();
                    }),
            );
        }
        let cache_view = view.clone();
        let volume_view = view.clone();
        dialog
            .title(title("Settings"))
            .w(px(480.))
            .child(
                v_flex()
                    .child(row("Appearance", appearance.into_any_element()))
                    .child(row("List density", density.into_any_element()))
                    .child(row(
                        "Reuse saved scans",
                        Switch::new("cache-enabled")
                            .checked(settings.cache_enabled)
                            .on_change(move |checked, _, cx| {
                                cache_view.update(cx, |this, cx| {
                                    this.settings.cache_enabled = *checked;
                                    this.persist(cx);
                                    cx.notify();
                                });
                                cx.refresh_windows();
                            })
                            .into_any_element(),
                    ))
                    .child(row(
                        "Scan other volumes",
                        Switch::new("cross-devices")
                            .checked(settings.cross_devices)
                            .on_change(move |checked, _, cx| {
                                volume_view.update(cx, |this, cx| {
                                    this.settings.cross_devices = *checked;
                                    this.persist(cx);
                                    cx.notify();
                                });
                                cx.refresh_windows();
                            })
                            .into_any_element(),
                    ))
                    .child(div().pt(px(12.)).child(note(
                        "Saved scans make repeat scans fast. Scans across volumes are never saved. Symlinks are never followed, and nothing is moved until you choose Move to Trash.",
                        cx,
                    ))),
            )
            .footer(
                h_flex().w_full().justify_end().child(
                    Button::new("done")
                        .label("Done")
                        .primary()
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                ),
            )
    });
}

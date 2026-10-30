use std::sync::Arc;

use clrs_core::Cached;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{
    ActiveTheme as _, Selectable as _, StyledExt as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::{
    App, Context, InteractiveElement as _, IntoElement, ParentElement as _, Role,
    StatefulInteractiveElement as _, Styled as _, Window, div, px, uniform_list,
};

use crate::settings::{Appearance, Density};
use crate::workbench::Workbench;
use crate::{format, theme};

pub(crate) fn detail(label: &'static str, value: String, cx: &App) -> impl IntoElement + use<> {
    v_flex()
        .gap_1()
        .child(div().text_color(cx.theme().muted_foreground).child(label))
        .child(
            div()
                .id(label)
                .role(Role::Label)
                .aria_label(format!("{label}: {value}"))
                .child(value),
        )
}

pub(crate) fn settings(window: &mut Window, cx: &mut Context<Workbench>) {
    let view = cx.entity();
    window.open_sheet(cx, move |sheet, _, cx| {
        let settings = view.read(cx).settings.clone();
        let mut appearance = h_flex().gap_2();
        for (label, value) in [("System", Appearance::System), ("Light", Appearance::Light), ("Dark", Appearance::Dark)] {
            let view = view.clone();
            appearance = appearance.child(Button::new(label).label(label).selected(settings.appearance == value).on_click(move |_, _, cx| {
                view.update(cx, |this, cx| { this.settings.appearance = value; theme::apply(value, cx); this.persist(cx); cx.notify(); });
                cx.refresh_windows();
            }));
        }
        let mut density = h_flex().gap_2();
        for (label, value) in [("Comfortable", Density::Comfortable), ("Compact", Density::Compact)] {
            let view = view.clone();
            density = density.child(Button::new(label).label(label).selected(settings.density == value).on_click(move |_, _, cx| {
                view.update(cx, |this, cx| { this.settings.density = value; this.persist(cx); cx.notify(); });
                cx.refresh_windows();
            }));
        }
        let cache_view = view.clone();
        let volume_view = view.clone();
        sheet.title("Settings").size(px(440.)).child(v_flex().gap_6()
            .child(
                v_flex()
                    .gap_2()
                    .child(div().font_semibold().child("Appearance"))
                    .child(appearance)
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(div().font_semibold().child("List density"))
                    .child(density)
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(div().font_semibold().child("Scanning"))
                    .child(Switch::new("cache-enabled").label("Reuse saved scans").checked(settings.cache_enabled).on_change(move |checked, _, cx| {
                        cache_view.update(cx, |this, cx| { this.settings.cache_enabled = *checked; this.persist(cx); cx.notify(); });
                        cx.refresh_windows();
                    }))
                    .child(Switch::new("cross-devices").label("Scan across mounted volumes").checked(settings.cross_devices).on_change(move |checked, _, cx| {
                        volume_view.update(cx, |this, cx| { this.settings.cross_devices = *checked; this.persist(cx); cx.notify(); });
                        cx.refresh_windows();
                    }))
            )
            .child(div().text_color(cx.theme().muted_foreground).child("Scan settings apply to the next scan. Cross-volume scans do not use saved scans. Symlinks are never followed."))
            .child(div().text_color(cx.theme().muted_foreground).child("cl-rs only reads your files. It never moves or deletes anything.")))
            .footer(Button::new("done").label("Done").primary().on_click(|_, window, cx| window.close_sheet(cx)))
    });
}

#[derive(Debug)]
pub(crate) struct ReviewItem {
    pub(crate) path: String,
    pub(crate) size: u64,
    pub(crate) command: Option<Vec<String>>,
}

/// Preview of everything staged. Nothing is moved or deleted from here.
pub(crate) fn review(window: &mut Window, cx: &mut Context<Workbench>) {
    let view = cx.entity();
    window.open_sheet(cx, move |sheet, _, cx| {
        let items = Arc::new(view.read(cx).review(cx));
        let total: u64 = items.iter().map(|item| item.size).sum();
        let count = items.len();
        let summary = if count == 0 {
            "Nothing is staged. Select items and choose Stage for Cleanup (Cmd-T), or stage a recommendation.".to_owned()
        } else {
            format!("{count} items · {} would be recovered once the Trash is emptied.", format::allocated(total))
        };
        let clear = view.clone();
        let list = Arc::clone(&items);
        sheet
            .title("Review Staged Items")
            .size(px(580.))
            .child(
                v_flex()
                    .h_full()
                    .gap_4()
                    .child(
                        div()
                            .id("review-summary")
                            .role(Role::Label)
                            .aria_label(summary.clone())
                            .font_semibold()
                            .child(summary),
                    )
                    .child(
                        uniform_list("review-items", count, move |range, _, cx| {
                            range
                                .map(|index| {
                                    let item = &list[index];
                                    let action = item.command.as_ref().map_or_else(
                                        || "Would be moved to the Trash".to_owned(),
                                        |command| format!("Would run `{}` if installed, otherwise move to the Trash", command.join(" ")),
                                    );
                                    v_flex()
                                        .id(index)
                                        .role(Role::ListItem)
                                        .aria_label(format!("{}: {}. {action}", item.path, format::allocated(item.size)))
                                        .h(px(72.))
                                        .py_2()
                                        .gap_1()
                                        .border_b_1()
                                        .border_color(cx.theme().border)
                                        .child(
                                            h_flex()
                                                .gap_2()
                                                .child(div().flex_1().min_w_0().truncate().child(item.path.clone()))
                                                .child(format::allocated(item.size)),
                                        )
                                        .child(div().truncate().text_color(cx.theme().muted_foreground).child(action))
                                })
                                .collect()
                        })
                        .flex_1()
                        .min_h_0(),
                    )
                    .child(
                        div()
                            .id("review-note")
                            .role(Role::Label)
                            .aria_label("Preview only. cl-rs does not move or delete anything yet.")
                            .text_color(cx.theme().muted_foreground)
                            .child("Preview only. cl-rs does not move or delete anything yet."),
                    ),
            )
            .footer(
                h_flex()
                    .gap_2()
                    .child(Button::new("clear-staged").label("Clear Staged").on_click(move |_, _, cx| {
                        clear.update(cx, Workbench::clear_staged);
                        cx.refresh_windows();
                    }))
                    .child(Button::new("close-review").label("Done").primary().on_click(|_, window, cx| window.close_sheet(cx))),
            )
    });
}

pub(crate) fn issues(cached: Arc<Cached>, window: &mut Window, cx: &mut App) {
    let needs_access = cached
        .issues
        .iter()
        .any(clrs_core::ScanIssue::needs_full_disk_access);
    window.open_sheet(cx, move |sheet, _, _| {
        let data = Arc::clone(&cached);
        let count = data.issues.len();
        let mut content = v_flex().h_full().gap_4()
            .child(div().child(format!("{count} paths could not be read. Their contents are excluded from the totals.")));
        if needs_access {
            content = content.child(div().child("macOS denied access to protected folders. In System Settings → Privacy & Security → Full Disk Access, add the packaged cl-rs app. Quit and reopen it, then refresh the scan."))
                .child(Button::new("privacy-settings").label("Open Privacy Settings").on_click(|_, _, cx| cx.open_url("x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles")));
        }
        content = content.child(uniform_list("scan-issues", count, move |range, _, cx| {
            range.map(|index| {
                let issue = &data.issues[index];
                let path = format::path(&issue.path);
                let error = issue.error.to_string();
                v_flex().id(index).role(Role::ListItem).aria_label(format!("{path}: {error}")).h(px(72.)).py_2().gap_1().border_b_1().border_color(cx.theme().border)
                    .child(div().truncate().child(path))
                    .child(div().truncate().text_color(cx.theme().muted_foreground).child(error))
            }).collect()
        }).flex_1().min_h_0());
        sheet.title("Scan Issues").size(px(540.)).child(content)
            .footer(Button::new("close-issues").label("Done").on_click(|_, window, cx| window.close_sheet(cx)))
    });
}

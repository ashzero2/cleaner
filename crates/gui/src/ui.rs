//! Small building blocks shared by several parts of the window.

use clrs_core::Safety;
use gpui_kit::assets::IconName;
use gpui_kit::component::{Icon, h_flex};
use gpui_kit::{
    App, BoxShadow, Div, FontWeight, Hsla, IntoElement, ParentElement as _, Pixels, SharedString,
    Styled as _, div, point, prelude::FluentBuilder as _, px, relative,
};

use crate::explore::Hint;
use crate::theme;

/// `part` as a fraction of `whole`, 0 when `whole` is empty.
#[expect(clippy::cast_precision_loss, reason = "only used for display")]
pub(crate) fn share(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        0.
    } else {
        part as f64 / whole as f64
    }
}

/// `part` as a fraction of `whole`, for sizing bars.
#[expect(clippy::cast_possible_truncation, reason = "a fraction in 0..=1")]
pub(crate) fn portion(part: u64, whole: u64) -> f32 {
    share(part, whole).clamp(0., 1.) as f32
}

pub(crate) fn icon(name: IconName) -> Icon {
    Icon::new(name)
}

pub(crate) fn swatch(color: Hsla, size: Pixels) -> Div {
    div().flex_shrink_0().size(size).rounded(px(3.)).bg(color)
}

/// Uppercase section label.
pub(crate) fn eyebrow(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::palette(cx).ink_2)
        .child(text.into())
}

/// A horizontal meter `fraction` full.
pub(crate) fn bar(fraction: f32, color: Hsla, track: Hsla) -> Div {
    div()
        .w_full()
        .h(px(6.))
        .rounded_full()
        .bg(track)
        .overflow_hidden()
        .child(
            div()
                .h_full()
                .rounded_full()
                .bg(color)
                .w(relative(fraction)),
        )
}

pub(crate) fn safety_text(safety: Safety) -> &'static str {
    match safety {
        Safety::Safe => "Safe to clear",
        Safety::Review => "Check first",
    }
}

pub(crate) fn hint_label(hint: &Hint) -> String {
    if hint.inside {
        format!("{} inside", hint.name)
    } else {
        safety_text(hint.safety).to_lowercase()
    }
}

/// A pill showing how safe a match is to clear; matches inside a folder are outlined.
pub(crate) fn chip(hint: &Hint, cx: &App) -> Div {
    let text = if hint.inside {
        format!("{} inside", hint.name)
    } else {
        safety_text(hint.safety).to_owned()
    };
    safety_chip(hint.safety, text, !hint.inside, cx)
}

pub(crate) fn safety_chip(
    safety: Safety,
    text: impl Into<SharedString>,
    filled: bool,
    cx: &App,
) -> Div {
    let p = theme::palette(cx);
    let (fg, bg) = match safety {
        Safety::Safe => (p.safe, p.safe_soft),
        Safety::Review => (p.review, p.review_soft),
    };
    let chip = h_flex()
        .h(px(20.))
        .px(px(7.))
        .gap_1()
        .rounded_full()
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(fg)
        .whitespace_nowrap();
    let chip = if filled {
        chip.bg(bg)
    } else {
        chip.border_1().border_color(bg.blend(fg.opacity(0.2)))
    };
    chip.when(filled && safety == Safety::Safe, |chip| {
        chip.child(icon(IconName::Check).size(px(10.)))
    })
    .child(text.into())
}

/// A raised card: the list, the detail card and the summary meter.
pub(crate) fn card(cx: &App) -> Div {
    let p = theme::palette(cx);
    div()
        .rounded(px(14.))
        .bg(p.raised)
        .border_1()
        .border_color(p.line)
}

pub(crate) fn lifted(element: Div, cx: &App) -> Div {
    let shadow = theme::palette(cx).shadow;
    element.shadow(vec![
        BoxShadow {
            color: shadow,
            offset: point(px(0.), px(1.)),
            blur_radius: px(2.),
            spread_radius: px(0.),
            inset: false,
        },
        BoxShadow {
            color: shadow,
            offset: point(px(0.), px(8.)),
            blur_radius: px(24.),
            spread_radius: px(0.),
            inset: false,
        },
    ])
}

/// A big rounded number, e.g. a folder's size.
pub(crate) fn display(text: impl Into<SharedString>, size: f32) -> impl IntoElement {
    div()
        .font_family(theme::DISPLAY_FONT)
        .font_weight(FontWeight::SEMIBOLD)
        .font_features(theme::tabular())
        .text_size(px(size))
        .line_height(px(size * 1.15))
        .child(text.into())
}

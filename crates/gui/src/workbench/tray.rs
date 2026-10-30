use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    Context, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _, Role,
    StatefulInteractiveElement as _, Styled as _, div, px,
};

use super::Workbench;
use crate::{format, panels, theme, ui};

/// Staged items shown as chips before the rest collapse into "+N more".
const CHIPS: usize = 4;

impl Workbench {
    #[expect(clippy::too_many_lines, reason = "one declarative view tree")]
    pub(super) fn tray(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = *theme::palette(cx);
        let items = self.review(cx);
        let count = items.len();
        let total: u64 = items.iter().map(|item| item.size).sum();
        let summary = if count == 0 {
            "Tray is empty".to_owned()
        } else {
            format!(
                "{} in tray, {}",
                format::plural(count, "item"),
                format::allocated(total)
            )
        };
        let mut bin = div()
            .relative()
            .flex_shrink_0()
            .size(px(42.))
            .rounded(px(12.))
            .bg(p.raised)
            .border_1()
            .border_color(p.line)
            .flex()
            .items_center()
            .justify_center()
            .child(ui::icon(IconName::Trash).size(px(20.)).text_color(p.accent));
        if count > 0 {
            bin = bin.child(
                div()
                    .absolute()
                    .top(px(-6.))
                    .right(px(-6.))
                    .min_w(px(18.))
                    .h(px(18.))
                    .px(px(5.))
                    .rounded_full()
                    .bg(p.accent_fill)
                    .text_color(p.on_accent)
                    .text_size(px(11.))
                    .font_weight(FontWeight::BOLD)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(count.to_string()),
            );
        }
        let mut middle = h_flex().flex_1().min_w_0().gap_2().overflow_hidden();
        if count == 0 {
            middle = middle.child(
                div()
                    .text_color(p.ink_2)
                    .truncate()
                    .child("Nothing set aside yet. Use + on anything you'd like to clear, then review it here."),
            );
        }
        for (index, item) in items.iter().take(CHIPS).enumerate() {
            let id = item.id;
            middle = middle.child(
                h_flex()
                    .flex_shrink_0()
                    .h(px(28.))
                    .pl(px(10.))
                    .pr(px(4.))
                    .gap(px(6.))
                    .rounded_full()
                    .bg(p.raised)
                    .border_1()
                    .border_color(p.line)
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(div().max_w(px(160.)).truncate().child(item.name.clone()))
                    .child(
                        div()
                            .text_color(p.ink_2)
                            .font_weight(FontWeight::NORMAL)
                            .font_features(theme::tabular())
                            .child(format::allocated(item.size)),
                    )
                    .child(
                        Button::new(("unstage", index))
                            .icon(IconName::X)
                            .ghost()
                            .xsmall()
                            .rounded(px(999.))
                            .accessibility_label(format!("Remove {} from tray", item.name))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.selection.unstage(id);
                                this.sync_staged(cx);
                            })),
                    ),
            );
        }
        if count > CHIPS {
            middle = middle.child(
                div()
                    .flex_shrink_0()
                    .text_color(p.ink_2)
                    .child(format!("+{} more", count - CHIPS)),
            );
        }
        let review = Button::new("review")
            .label(if count == 0 {
                "Review".to_owned()
            } else {
                format!("Review {}", format::plural(count, "item"))
            })
            .primary()
            .disabled(count == 0)
            .on_click(cx.listener(|_, _, window, cx| panels::review(window, cx)));
        h_flex()
            .id("tray")
            .role(Role::Status)
            .aria_label(summary)
            .flex_shrink_0()
            .h(px(68.))
            .pl(px(14.))
            .pr(px(16.))
            .gap(px(14.))
            .bg(p.chrome)
            .border_t_1()
            .border_color(p.line)
            .child(bin)
            .child(middle)
            .children((count > 0).then(|| {
                v_flex()
                    .items_end()
                    .flex_shrink_0()
                    .child(ui::display(format::allocated(total), 18.))
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(p.ink_2)
                            .child("to clear"),
                    )
            }))
            .child(review)
    }
}

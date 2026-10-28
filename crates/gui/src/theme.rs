use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, WindowAppearance, px, rgb};

use crate::settings::Appearance;

pub(crate) const SIDEBAR_WIDTH: f32 = 200.;
pub(crate) const INSPECTOR_WIDTH: f32 = 280.;
pub(crate) const MIN_WIDTH: f32 = 1040.;
pub(crate) const MIN_HEIGHT: f32 = 680.;

pub(crate) fn apply(appearance: Appearance, cx: &mut App) {
    let dark = match appearance {
        Appearance::System => matches!(
            cx.window_appearance(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        ),
        Appearance::Light => false,
        Appearance::Dark => true,
    };
    Theme::change(
        if dark {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        },
        None,
        cx,
    );
    Theme::update(cx, |theme| {
        let color = |light, dark_value| rgb(if dark { dark_value } else { light }).into();
        theme.font_family = ".SystemUIFont".into();
        theme.font_size = px(13.);
        theme.radius = px(6.);
        theme.radius_lg = px(10.);
        theme.background = color(0x00ff_ffff, 0x0022_252a);
        theme.foreground = color(0x0020_252c, 0x00f1_f3f5);
        theme.muted = color(0x00f4_f5f7, 0x0019_1b1f);
        theme.muted_foreground = color(0x0059_616c, 0x00ad_b5c0);
        theme.border = color(0x00d8_dde4, 0x0041_4751);
        theme.primary = color(0x0000_6c75, 0x0068_cfd6);
        theme.primary_hover = color(0x0000_5a63, 0x008a_dbe0);
        theme.primary_active = color(0x0000_4b53, 0x004d_b9c1);
        theme.primary_foreground = color(0x00ff_ffff, 0x0012_282b);
        theme.sheet.margin_top = px(48.);
        theme.button_primary = theme.primary;
        theme.button_primary_hover = theme.primary_hover;
        theme.button_primary_active = theme.primary_active;
        theme.button_primary_foreground = theme.primary_foreground;
        theme.ring = theme.primary;
        theme.selection = color(0x00dc_eff0, 0x0018_3e43);
        theme.accent = theme.selection;
        theme.accent_foreground = theme.foreground;
        theme.sidebar = color(0x00ec_eef1, 0x0019_1b1f);
        theme.sidebar_foreground = theme.foreground;
        theme.sidebar_border = theme.border;
        theme.sidebar_accent = theme.selection;
        theme.sidebar_accent_foreground = theme.foreground;
        theme.table = theme.background;
        theme.table_head = theme.muted;
        theme.table_head_foreground = theme.muted_foreground;
        theme.table_hover = theme.muted;
        theme.table_even = theme.background;
        theme.table_active = theme.selection;
        theme.table_active_border = theme.primary;
        theme.table_row_border = theme.border;
        theme.danger = color(0x00b2_2d3b, 0x00ff_99a5);
        theme.warning = color(0x0087_5400, 0x00f0_c36a);
    });
}

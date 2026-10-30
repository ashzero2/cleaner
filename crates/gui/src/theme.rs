use std::borrow::Cow;
use std::sync::Arc;

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, FontFeatures, Global, Hsla, WindowAppearance, px, rgb};

use crate::category::Category;
use crate::settings::Appearance;

pub(crate) const SIDEBAR_WIDTH: f32 = 210.;
pub(crate) const MIN_WIDTH: f32 = 1040.;
pub(crate) const MIN_HEIGHT: f32 = 680.;

/// SF Pro Rounded and SF Mono ship with macOS but are hidden families, so they are loaded
/// from the system font folder at startup instead of being bundled.
pub(crate) const DISPLAY_FONT: &str = ".SF NS Rounded";
pub(crate) const MONO_FONT: &str = ".SF NS Mono";
const SYSTEM_FONTS: [&str; 2] = [
    "/System/Library/Fonts/SFNSRounded.ttf",
    "/System/Library/Fonts/SFNSMono.ttf",
];

/// Colours the kit's `Theme` has no slot for. See `DESIGN.md` for what each one is for.
#[derive(Clone, Copy)]
pub(crate) struct Palette {
    pub(crate) window: Hsla,
    pub(crate) chrome: Hsla,
    pub(crate) panel: Hsla,
    pub(crate) raised: Hsla,
    pub(crate) ink: Hsla,
    pub(crate) ink_2: Hsla,
    pub(crate) ink_3: Hsla,
    pub(crate) line: Hsla,
    pub(crate) accent: Hsla,
    pub(crate) accent_fill: Hsla,
    pub(crate) on_accent: Hsla,
    pub(crate) accent_soft: Hsla,
    pub(crate) safe: Hsla,
    pub(crate) safe_soft: Hsla,
    pub(crate) review: Hsla,
    pub(crate) review_soft: Hsla,
    pub(crate) danger: Hsla,
    pub(crate) tile_ink: Hsla,
    /// Overlay for nested treemap tiles.
    pub(crate) shade: Hsla,
    pub(crate) shadow: Hsla,
    categories: [Hsla; Category::ALL.len()],
}

impl Global for Palette {}

impl Palette {
    pub(crate) fn category(&self, category: Category) -> Hsla {
        self.categories[category as usize]
    }

    fn new(dark: bool) -> Self {
        let c = |light: u32, dark_value: u32| -> Hsla {
            rgb(if dark { dark_value } else { light }).into()
        };
        Self {
            window: c(0x00ff_fcf9, 0x001e_1817),
            chrome: c(0x00f7_efe8, 0x0024_1d1b),
            panel: c(0x00fb_f5ef, 0x0022_1b19),
            raised: c(0x00ff_ffff, 0x002a_2220),
            ink: c(0x002e_2522, 0x00f5_ece6),
            ink_2: c(0x007a_6a62, 0x00b5_a69d),
            ink_3: c(0x00a8_988f, 0x007d_6f67),
            line: c(0x00ea_dfd6, 0x003a_302c),
            accent: c(0x00e2_703a, 0x00f3_925c),
            accent_fill: c(0x00b8_501d, 0x00f3_925c),
            on_accent: c(0x00ff_ffff, 0x002a_140a),
            accent_soft: c(0x00fc_e6d8, 0x004a_2a1a),
            safe: c(0x0027_6b41, 0x007c_cb98),
            safe_soft: c(0x00e1_f1e5, 0x001f_3a29),
            review: c(0x0087_570c, 0x00e8_b65c),
            review_soft: c(0x00fb_efd6, 0x003d_2e12),
            danger: c(0x00b2_2d3b, 0x00ff_99a5),
            tile_ink: c(0x002e_2522, 0x001e_1817),
            shade: if dark {
                gpui_kit::hsla(0., 0., 0., 0.16)
            } else {
                gpui_kit::hsla(0., 0., 1., 0.32)
            },
            shadow: if dark {
                gpui_kit::hsla(0., 0., 0., 0.3)
            } else {
                gpui_kit::hsla(0.06, 0.7, 0.14, 0.08)
            },
            categories: [
                c(0x009d_b4d6, 0x006f_8db8),
                c(0x00e8_b85c, 0x00c9_9a3e),
                c(0x00e9_906a, 0x00d0_7650),
                c(0x008d_be9a, 0x006f_a07c),
                c(0x00b9_a3d3, 0x0095_80b5),
                c(0x00e7_a0a8, 0x00c2_7c85),
                c(0x00d8_ccc2, 0x0096_8a80),
            ],
        }
    }
}

pub(crate) fn palette(cx: &App) -> &Palette {
    cx.global::<Palette>()
}

/// Tabular figures, so numbers line up in columns and don't shift while they change.
pub(crate) fn tabular() -> FontFeatures {
    FontFeatures(Arc::new(vec![("tnum".into(), 1)]))
}

pub(crate) fn load_fonts(cx: &App) {
    let fonts: Vec<Cow<'static, [u8]>> = SYSTEM_FONTS
        .iter()
        .filter_map(|path| std::fs::read(path).ok().map(Cow::Owned))
        .collect();
    if let Err(error) = cx.text_system().add_fonts(fonts) {
        eprintln!("Could not load system display fonts: {error}");
    }
}

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
    let p = Palette::new(dark);
    cx.set_global(p);
    let hover = |color: Hsla| color.blend(p.ink.opacity(0.06));
    Theme::update(cx, |theme| {
        theme.font_family = ".SystemUIFont".into();
        theme.mono_font_family = MONO_FONT.into();
        theme.font_size = px(13.);
        theme.radius = px(9.);
        theme.radius_lg = px(14.);
        theme.shadow = false;
        theme.sheet.margin_top = px(54.);
        theme.background = p.window;
        theme.foreground = p.ink;
        theme.muted = p.panel;
        theme.muted_foreground = p.ink_2;
        theme.border = p.line;
        theme.input = p.line;
        theme.ring = p.accent;
        theme.caret = p.accent;
        theme.selection = p.accent_soft;
        theme.accent = p.accent_soft;
        theme.accent_foreground = p.ink;
        theme.popover = p.raised;
        theme.popover_foreground = p.ink;
        theme.primary = p.accent_fill;
        theme.primary_hover = hover(p.accent_fill);
        theme.primary_active = hover(hover(p.accent_fill));
        theme.primary_foreground = p.on_accent;
        theme.button_primary = theme.primary;
        theme.button_primary_hover = theme.primary_hover;
        theme.button_primary_active = theme.primary_active;
        theme.button_primary_foreground = theme.primary_foreground;
        theme.secondary = p.raised;
        theme.secondary_hover = p.panel;
        theme.secondary_active = hover(p.panel);
        theme.secondary_foreground = p.ink;
        theme.button = p.raised;
        theme.button_hover = p.panel;
        theme.button_active = hover(p.panel);
        theme.button_foreground = p.ink;
        theme.danger = p.danger;
        theme.danger_hover = hover(p.danger);
        theme.danger_active = hover(hover(p.danger));
        theme.danger_foreground = p.on_accent;
        theme.button_danger = p.danger;
        theme.button_danger_hover = theme.danger_hover;
        theme.button_danger_active = theme.danger_active;
        theme.button_danger_foreground = if dark { p.window } else { p.on_accent };
        theme.warning = p.review;
        theme.success = p.safe;
        theme.sidebar = p.chrome;
        theme.sidebar_foreground = p.ink;
        theme.sidebar_border = p.line;
        theme.sidebar_accent = p.accent_soft;
        theme.sidebar_accent_foreground = p.ink;
        theme.title_bar = p.chrome;
        theme.title_bar_border = p.line;
        theme.table = p.raised;
        theme.table_head = p.raised;
        theme.table_head_foreground = p.ink_2;
        theme.table_hover = p.panel;
        theme.table_even = p.raised;
        theme.table_active = p.accent_soft;
        theme.table_active_border = p.accent_soft;
        theme.table_row_border = p.line;
        theme.list_hover = p.panel;
        theme.list_active = p.accent_soft;
        theme.list_active_border = p.accent_soft;
        theme.switch = p.line;
        theme.progress_bar = p.accent;
        theme.overlay = gpui_kit::hsla(0.05, 0.4, 0.06, 0.28);
        theme.scrollbar_thumb = p.ink.opacity(0.22);
        theme.scrollbar_thumb_hover = p.ink.opacity(0.35);
    });
}

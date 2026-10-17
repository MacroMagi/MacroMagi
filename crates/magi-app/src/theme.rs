//! MacroMagi's look.
//!
//! Every color in the UI comes from one [`Palette`], built from three
//! choices: light or dark, a theme color, and how see-through panels are
//! (for background images). The default theme color is none at all –
//! monochrome, mostly black – and picking one recolors everything, node
//! categories and value types included, as variations around it.
//!
//! Widgets read colors through the functions here (`theme::text()`,
//! `theme::accent()`, …) so that a theme change takes effect everywhere at once.

use std::cell::Cell;

use egui::epaint::Shadow;
use egui::{Color32, CornerRadius, FontFamily, FontId, Margin, Stroke, TextStyle, Vec2};
use magi_core::nodes::{Category, DataType, PortKind};
use serde::{Deserialize, Serialize};

pub const RADIUS: u8 = 6;
pub const RADIUS_LARGE: u8 = 10;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    /// Follow the operating system.
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeMode {
    pub fn label(self) -> &'static str {
        match self {
            ThemeMode::System => "System",
            ThemeMode::Light => "Light",
            ThemeMode::Dark => "Dark",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub dark: bool,
    pub bg_app: Color32,
    pub bg_panel: Color32,
    pub bg_canvas: Color32,
    pub bg_raised: Color32,
    pub bg_widget: Color32,
    pub bg_widget_hover: Color32,
    pub bg_widget_active: Color32,
    pub bg_input: Color32,
    pub stroke: Color32,
    pub stroke_strong: Color32,
    pub grid: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_faint: Color32,
    pub accent: Color32,
    pub accent_bright: Color32,
    /// Text drawn on top of the accent color.
    pub on_accent: Color32,
    pub success: Color32,
    pub danger: Color32,
    pub warning: Color32,
    pub flow: Color32,
    /// One per [`Category`], in [`Category::ALL`] order.
    categories: [Color32; 12],
    data: [Color32; 5],
}

thread_local! {
    static PALETTE: Cell<Palette> = Cell::new(Palette::build(true, None, 1.0));
}

/// The palette in use.
pub fn palette() -> Palette {
    PALETTE.get()
}

fn gray(level: u8) -> Color32 {
    Color32::from_rgb(level, level, level)
}

fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// HSV (all `0..=1`) to a color.
fn from_hsv(h: f32, s: f32, v: f32) -> Color32 {
    let h = h.rem_euclid(1.0) * 6.0;
    let c = v * s;
    let x = c * (1.0 - ((h % 2.0) - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    let to_u8 = |value: f32| ((value + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(to_u8(r), to_u8(g), to_u8(b))
}

/// A color's hue, saturation and value, all `0..=1`.
fn to_hsv(color: Color32) -> (f32, f32, f32) {
    let [r, g, b] = [color.r(), color.g(), color.b()].map(|c| f32::from(c) / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let hue = if delta == 0.0 {
        0.0
    } else if max == r {
        ((g - b) / delta).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / delta + 2.0) / 6.0
    } else {
        ((r - g) / delta + 4.0) / 6.0
    };
    (hue, if max == 0.0 { 0.0 } else { delta / max }, max)
}

fn luminance(color: Color32) -> f32 {
    (0.2126 * f32::from(color.r()) + 0.7152 * f32::from(color.g()) + 0.0722 * f32::from(color.b())) / 255.0
}

impl Palette {
    /// `accent`: the theme color, or `None` for monochrome.
    /// `panel_alpha`: how opaque panels are (below 1 to show a background image).
    pub fn build(dark: bool, accent: Option<Color32>, panel_alpha: f32) -> Self {
        let panel_alpha = panel_alpha.clamp(0.0, 1.0);
        let see_through = |color: Color32, alpha: f32| color.gamma_multiply(alpha);
        let base = if dark {
            Palette {
                dark,
                bg_app: gray(0x0B),
                bg_panel: gray(0x10),
                bg_canvas: gray(0x0C),
                bg_raised: gray(0x17),
                bg_widget: gray(0x1F),
                bg_widget_hover: gray(0x29),
                bg_widget_active: gray(0x33),
                bg_input: gray(0x08),
                stroke: gray(0x24),
                stroke_strong: gray(0x36),
                grid: gray(0x16),
                text: gray(0xEC),
                text_dim: gray(0xA2),
                text_faint: gray(0x6E),
                accent: gray(0xE6),
                accent_bright: Color32::WHITE,
                on_accent: gray(0x0B),
                success: hex(0x4C_C9_8A),
                danger: hex(0xF0_5D_6A),
                warning: hex(0xE9_B1_4F),
                flow: gray(0xD6),
                categories: [
                    gray(0xD8),
                    gray(0xBC),
                    gray(0xA4),
                    gray(0x9C),
                    gray(0xF0),
                    gray(0xE4),
                    gray(0xC4),
                    gray(0x94),
                    gray(0xB0),
                    gray(0xC8),
                    gray(0x8E),
                    gray(0x86),
                ],
                data: [gray(0xE0), gray(0xB0), gray(0xC8), gray(0x98), gray(0x80)],
            }
        } else {
            Palette {
                dark,
                bg_app: gray(0xE9),
                bg_panel: gray(0xF4),
                bg_canvas: gray(0xEE),
                bg_raised: gray(0xFF),
                bg_widget: gray(0xE6),
                bg_widget_hover: gray(0xDB),
                bg_widget_active: gray(0xCE),
                bg_input: gray(0xFF),
                stroke: gray(0xD6),
                stroke_strong: gray(0xBC),
                grid: gray(0xE2),
                text: gray(0x14),
                text_dim: gray(0x55),
                text_faint: gray(0x8A),
                accent: gray(0x16),
                accent_bright: Color32::BLACK,
                on_accent: gray(0xF4),
                success: hex(0x1E_8E_57),
                danger: hex(0xC7_2C_3B),
                warning: hex(0xA8_6A_00),
                flow: gray(0x3A),
                categories: [
                    gray(0x2A),
                    gray(0x48),
                    gray(0x60),
                    gray(0x5A),
                    gray(0x10),
                    gray(0x1E),
                    gray(0x40),
                    gray(0x72),
                    gray(0x52),
                    gray(0x3A),
                    gray(0x78),
                    gray(0x80),
                ],
                data: [gray(0x22), gray(0x50), gray(0x38), gray(0x66), gray(0x7C)],
            }
        };

        let mut palette = base;
        if let Some(accent) = accent {
            let (hue, saturation, _) = to_hsv(accent);
            let saturation = saturation.clamp(0.35, 0.85);
            let value = if dark { 0.92 } else { 0.62 };
            let around = |offset: f32| from_hsv(hue + offset, saturation, value);
            palette.accent = accent;
            palette.accent_bright =
                if dark { mix(accent, Color32::WHITE, 0.3) } else { mix(accent, Color32::BLACK, 0.25) };
            palette.on_accent = if luminance(accent) > 0.55 { gray(0x0B) } else { Color32::WHITE };
            // Flow, Input, Vision, Sound, AI, Window, Variables, Logic, System, Script, Notes, Add-ons.
            palette.categories = [
                around(0.0),
                around(0.58),
                around(0.42),
                around(0.32),
                around(0.66),
                around(0.72),
                around(0.19),
                around(0.11),
                around(0.8),
                around(0.88),
                mix(around(0.05), palette.text_dim, 0.5),
                around(0.0).gamma_multiply(0.7),
            ];
            palette.data =
                [around(0.95), around(0.5), around(0.14), around(0.75), mix(around(0.0), palette.text_dim, 0.6)];
            // A hint of the theme color in the greys keeps the whole UI in one key.
            let tint = |color: Color32, amount: f32| mix(color, accent, amount);
            palette.bg_app = tint(palette.bg_app, 0.03);
            palette.bg_panel = tint(palette.bg_panel, 0.035);
            palette.bg_raised = tint(palette.bg_raised, 0.04);
            palette.bg_widget = tint(palette.bg_widget, 0.06);
            palette.bg_widget_hover = tint(palette.bg_widget_hover, 0.08);
            palette.bg_widget_active = tint(palette.bg_widget_active, 0.12);
            palette.stroke_strong = tint(palette.stroke_strong, 0.1);
            palette.flow = tint(palette.flow, 0.15);
        }

        if panel_alpha < 1.0 {
            palette.bg_app = see_through(palette.bg_app, panel_alpha);
            palette.bg_panel = see_through(palette.bg_panel, panel_alpha);
            palette.bg_canvas = see_through(palette.bg_canvas, panel_alpha);
            palette.bg_raised = see_through(palette.bg_raised, (panel_alpha * 0.5 + 0.5).min(1.0));
        }
        palette
    }
}

pub fn bg_app() -> Color32 {
    palette().bg_app
}
pub fn bg_panel() -> Color32 {
    palette().bg_panel
}
pub fn bg_canvas() -> Color32 {
    palette().bg_canvas
}
pub fn bg_raised() -> Color32 {
    palette().bg_raised
}
pub fn bg_widget() -> Color32 {
    palette().bg_widget
}
pub fn bg_widget_hover() -> Color32 {
    palette().bg_widget_hover
}
pub fn bg_widget_active() -> Color32 {
    palette().bg_widget_active
}
pub fn bg_input() -> Color32 {
    palette().bg_input
}
pub fn stroke() -> Color32 {
    palette().stroke
}
pub fn stroke_strong() -> Color32 {
    palette().stroke_strong
}
pub fn grid() -> Color32 {
    palette().grid
}
pub fn text() -> Color32 {
    palette().text
}
pub fn text_dim() -> Color32 {
    palette().text_dim
}
pub fn text_faint() -> Color32 {
    palette().text_faint
}
pub fn accent() -> Color32 {
    palette().accent
}
pub fn accent_bright() -> Color32 {
    palette().accent_bright
}
pub fn on_accent() -> Color32 {
    palette().on_accent
}
pub fn success() -> Color32 {
    palette().success
}
pub fn danger() -> Color32 {
    palette().danger
}
pub fn warning() -> Color32 {
    palette().warning
}
pub fn flow() -> Color32 {
    palette().flow
}

/// Black or white, whichever reads better on `background`.
pub fn text_on(background: Color32) -> Color32 {
    if background == palette().accent {
        palette().on_accent
    } else if luminance(background) > 0.55 {
        gray(0x0B)
    } else {
        Color32::WHITE
    }
}

/// The theme color as stored in the settings.
pub fn accent_from(rgb: Option<[u8; 3]>) -> Option<Color32> {
    rgb.map(|[r, g, b]| Color32::from_rgb(r, g, b))
}

pub fn category_color(category: Category) -> Color32 {
    let index = Category::ALL.iter().position(|c| *c == category).unwrap_or(0);
    palette().categories[index.min(Category::ALL.len() - 1)]
}

pub fn data_color(ty: DataType) -> Color32 {
    let colors = palette().data;
    match ty {
        DataType::Bool => colors[0],
        DataType::Number => colors[1],
        DataType::Point => colors[2],
        DataType::Text => colors[3],
        DataType::Any => colors[4],
    }
}

pub fn port_color(kind: PortKind) -> Color32 {
    match kind {
        PortKind::Flow => flow(),
        PortKind::Data(ty) => data_color(ty),
    }
}

/// `a` blended towards `b` by `t` (0 = all `a`, 1 = all `b`).
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let lerp = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t.clamp(0.0, 1.0)).round() as u8;
    Color32::from_rgba_premultiplied(lerp(a.r(), b.r()), lerp(a.g(), b.g()), lerp(a.b(), b.b()), lerp(a.a(), b.a()))
}

pub fn with_alpha(color: Color32, alpha: f32) -> Color32 {
    color.gamma_multiply(alpha.clamp(0.0, 1.0))
}

/// The soft shadow under floating things (nodes, popups).
pub fn drop_shadow() -> Shadow {
    let alpha = if palette().dark { 120 } else { 40 };
    Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(alpha) }
}

/// Fonts are set once; the icon font is part of them.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    ctx.set_fonts(fonts);
}

/// Makes `palette` current and styles egui with it.
pub fn apply(ctx: &egui::Context, palette: Palette) {
    PALETTE.set(palette);
    let p = palette;
    ctx.set_theme(if p.dark { egui::Theme::Dark } else { egui::Theme::Light });
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, FontId::new(11.0, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(13.5, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(13.5, FontFamily::Proportional)),
            (TextStyle::Heading, FontId::new(18.0, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(12.5, FontFamily::Monospace)),
        ]
        .into();

        let spacing = &mut style.spacing;
        spacing.item_spacing = Vec2::new(8.0, 7.0);
        spacing.button_padding = Vec2::new(10.0, 5.0);
        spacing.interact_size = Vec2::new(40.0, 26.0);
        spacing.window_margin = Margin::same(12);
        spacing.menu_margin = Margin::same(6);
        spacing.combo_width = 120.0;
        spacing.slider_width = 150.0;
        spacing.icon_width = 16.0;
        spacing.scroll.bar_width = 8.0;
        spacing.scroll.floating = true;

        let radius = CornerRadius::same(RADIUS);
        let visuals = &mut style.visuals;
        visuals.dark_mode = p.dark;
        visuals.override_text_color = Some(p.text);
        visuals.panel_fill = p.bg_panel;
        visuals.window_fill = p.bg_panel.to_opaque();
        visuals.window_stroke = Stroke::new(1.0, p.stroke_strong);
        visuals.window_corner_radius = CornerRadius::same(RADIUS_LARGE);
        visuals.menu_corner_radius = CornerRadius::same(8);
        visuals.window_shadow = drop_shadow();
        visuals.popup_shadow = drop_shadow();
        visuals.extreme_bg_color = p.bg_input;
        visuals.text_edit_bg_color = Some(p.bg_input);
        visuals.faint_bg_color = p.bg_raised;
        visuals.code_bg_color = p.bg_input;
        visuals.hyperlink_color = p.accent_bright;
        visuals.warn_fg_color = p.warning;
        visuals.error_fg_color = p.danger;
        visuals.selection.bg_fill = with_alpha(p.accent, 0.4);
        visuals.selection.stroke = Stroke::new(1.0, p.accent_bright);
        visuals.slider_trailing_fill = true;
        visuals.striped = true;

        let widgets = &mut visuals.widgets;
        widgets.noninteractive.bg_fill = p.bg_panel;
        widgets.noninteractive.weak_bg_fill = p.bg_panel;
        widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.stroke);
        widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text_dim);
        widgets.noninteractive.corner_radius = radius;

        for (state, fill, stroke, fg) in [
            (&mut widgets.inactive, p.bg_widget, p.stroke, p.text),
            (&mut widgets.hovered, p.bg_widget_hover, p.stroke_strong, p.text),
            (&mut widgets.active, p.bg_widget_active, p.accent, p.text),
            (&mut widgets.open, p.bg_widget_active, p.stroke_strong, p.text),
        ] {
            state.bg_fill = fill;
            state.weak_bg_fill = fill;
            state.bg_stroke = Stroke::new(1.0, stroke);
            state.fg_stroke = Stroke::new(1.0, fg);
            state.corner_radius = radius;
            state.expansion = 0.0;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_survive_the_hsv_round_trip() {
        for color in [hex(0x9B_6D_FF), hex(0x2D_D4_A7), hex(0xFF_B5_4A), gray(0x80)] {
            let (h, s, v) = to_hsv(color);
            let back = from_hsv(h, s, v);
            for (a, b) in [(color.r(), back.r()), (color.g(), back.g()), (color.b(), back.b())] {
                assert!(a.abs_diff(b) <= 1, "{color:?} -> {back:?}");
            }
        }
    }

    #[test]
    fn monochrome_is_grey_and_a_theme_color_tints_everything() {
        let mono = Palette::build(true, None, 1.0);
        assert!(mono.categories.iter().all(|c| c.r() == c.g() && c.g() == c.b()));
        let violet = Palette::build(true, Some(hex(0x9B_6D_FF)), 1.0);
        assert_eq!(violet.accent, hex(0x9B_6D_FF));
        assert!(violet.categories.iter().any(|c| c.r() != c.g()));
        assert_ne!(mono.bg_panel, violet.bg_panel);
    }

    #[test]
    fn text_on_the_accent_stays_readable() {
        assert!(luminance(Palette::build(true, Some(hex(0xFF_FF_80)), 1.0).on_accent) < 0.2);
        assert!(luminance(Palette::build(true, Some(hex(0x20_20_80)), 1.0).on_accent) > 0.8);
        assert!(luminance(Palette::build(false, None, 1.0).text) < 0.2, "light mode has dark text");
    }

    #[test]
    fn see_through_panels_only_affect_backgrounds() {
        let solid = Palette::build(true, None, 1.0);
        let glass = Palette::build(true, None, 0.5);
        assert!(glass.bg_panel.a() < solid.bg_panel.a());
        assert_eq!(glass.text, solid.text);
    }
}

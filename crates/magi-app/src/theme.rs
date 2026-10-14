//! MacroMagi's look: one dark palette, applied to egui once at startup.
//!
//! Every colour used anywhere in the UI is named here. Widgets should take
//! colours from this module rather than inventing their own.

use egui::epaint::Shadow;
use egui::{Color32, CornerRadius, FontFamily, FontId, Margin, Stroke, TextStyle, Vec2};
use magi_core::nodes::{Category, DataType, PortKind};

pub const BG_APP: Color32 = Color32::from_rgb(0x12, 0x13, 0x18);
pub const BG_PANEL: Color32 = Color32::from_rgb(0x17, 0x19, 0x1F);
pub const BG_CANVAS: Color32 = Color32::from_rgb(0x10, 0x11, 0x15);
pub const BG_RAISED: Color32 = Color32::from_rgb(0x1E, 0x21, 0x29);
pub const BG_WIDGET: Color32 = Color32::from_rgb(0x26, 0x2A, 0x33);
pub const BG_WIDGET_HOVER: Color32 = Color32::from_rgb(0x30, 0x35, 0x40);
pub const BG_WIDGET_ACTIVE: Color32 = Color32::from_rgb(0x3A, 0x40, 0x4E);
pub const BG_INPUT: Color32 = Color32::from_rgb(0x0E, 0x0F, 0x13);

pub const STROKE: Color32 = Color32::from_rgb(0x2A, 0x2E, 0x38);
pub const STROKE_STRONG: Color32 = Color32::from_rgb(0x3D, 0x43, 0x50);
pub const GRID: Color32 = Color32::from_rgb(0x1E, 0x21, 0x28);

pub const TEXT: Color32 = Color32::from_rgb(0xE8, 0xEA, 0xF0);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x9E, 0xA6, 0xB6);
pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x6A, 0x72, 0x84);

pub const ACCENT: Color32 = Color32::from_rgb(0x9B, 0x6D, 0xFF);
pub const ACCENT_BRIGHT: Color32 = Color32::from_rgb(0xB9, 0x97, 0xFF);
pub const SUCCESS: Color32 = Color32::from_rgb(0x3D, 0xD6, 0x8C);
pub const DANGER: Color32 = Color32::from_rgb(0xFF, 0x5C, 0x6C);
pub const WARNING: Color32 = Color32::from_rgb(0xFF, 0xB5, 0x4A);

/// Colour of flow wires and flow ports.
pub const FLOW: Color32 = Color32::from_rgb(0xD5, 0xDA, 0xE5);

pub const RADIUS: u8 = 6;
pub const RADIUS_LARGE: u8 = 10;

pub fn category_color(category: Category) -> Color32 {
    match category {
        Category::Flow => ACCENT,
        Category::Input => Color32::from_rgb(0x4D, 0xA3, 0xFF),
        Category::Vision => Color32::from_rgb(0x2D, 0xD4, 0xA7),
        Category::Logic => Color32::from_rgb(0xFF, 0xB5, 0x4A),
        Category::Script => Color32::from_rgb(0xFF, 0x7A, 0xB8),
        Category::Addon => Color32::from_rgb(0x8E, 0x99, 0xAE),
    }
}

pub fn data_color(ty: DataType) -> Color32 {
    match ty {
        DataType::Bool => Color32::from_rgb(0xFF, 0x7A, 0x8A),
        DataType::Number => Color32::from_rgb(0x5B, 0xD6, 0xFF),
        DataType::Point => Color32::from_rgb(0xFF, 0xD1, 0x66),
        DataType::Text => Color32::from_rgb(0xC4, 0xA7, 0xFF),
        DataType::Any => Color32::from_rgb(0xA9, 0xB1, 0xC1),
    }
}

pub fn port_color(kind: PortKind) -> Color32 {
    match kind {
        PortKind::Flow => FLOW,
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
    Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(110) }
}

pub fn apply(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    ctx.set_fonts(fonts);

    ctx.set_theme(egui::Theme::Dark);
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
        visuals.dark_mode = true;
        visuals.override_text_color = None;
        visuals.panel_fill = BG_PANEL;
        visuals.window_fill = BG_PANEL;
        visuals.window_stroke = Stroke::new(1.0, STROKE_STRONG);
        visuals.window_corner_radius = CornerRadius::same(RADIUS_LARGE);
        visuals.menu_corner_radius = CornerRadius::same(8);
        visuals.window_shadow = drop_shadow();
        visuals.popup_shadow = drop_shadow();
        visuals.extreme_bg_color = BG_INPUT;
        visuals.text_edit_bg_color = Some(BG_INPUT);
        visuals.faint_bg_color = BG_RAISED;
        visuals.code_bg_color = BG_INPUT;
        visuals.hyperlink_color = ACCENT_BRIGHT;
        visuals.warn_fg_color = WARNING;
        visuals.error_fg_color = DANGER;
        visuals.selection.bg_fill = with_alpha(ACCENT, 0.45);
        visuals.selection.stroke = Stroke::new(1.0, ACCENT_BRIGHT);
        visuals.slider_trailing_fill = true;
        visuals.striped = false;

        let widgets = &mut visuals.widgets;
        widgets.noninteractive.bg_fill = BG_PANEL;
        widgets.noninteractive.weak_bg_fill = BG_PANEL;
        widgets.noninteractive.bg_stroke = Stroke::new(1.0, STROKE);
        widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_DIM);
        widgets.noninteractive.corner_radius = radius;

        widgets.inactive.bg_fill = BG_WIDGET;
        widgets.inactive.weak_bg_fill = BG_WIDGET;
        widgets.inactive.bg_stroke = Stroke::new(1.0, STROKE);
        widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
        widgets.inactive.corner_radius = radius;
        widgets.inactive.expansion = 0.0;

        widgets.hovered.bg_fill = BG_WIDGET_HOVER;
        widgets.hovered.weak_bg_fill = BG_WIDGET_HOVER;
        widgets.hovered.bg_stroke = Stroke::new(1.0, STROKE_STRONG);
        widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        widgets.hovered.corner_radius = radius;
        widgets.hovered.expansion = 0.0;

        widgets.active.bg_fill = BG_WIDGET_ACTIVE;
        widgets.active.weak_bg_fill = BG_WIDGET_ACTIVE;
        widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
        widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        widgets.active.corner_radius = radius;
        widgets.active.expansion = 0.0;

        widgets.open.bg_fill = BG_WIDGET_ACTIVE;
        widgets.open.weak_bg_fill = BG_WIDGET_ACTIVE;
        widgets.open.bg_stroke = Stroke::new(1.0, STROKE_STRONG);
        widgets.open.fg_stroke = Stroke::new(1.0, TEXT);
        widgets.open.corner_radius = radius;
    });
}

//! The screen picker: choose a point, an area, an image or a colour on screen.
//!
//! It works like a screenshot tool. The screen is captured once and shown
//! frozen in a full-screen window; the user points or drags on that picture.
//! Freezing means menus and hover states stay put while they are selected.

use egui::{Align2, Color32, FontId, Key, Pos2, Sense, Stroke, StrokeKind, TextureHandle, Vec2, pos2, vec2};
use magi_core::geom::{Point, Rect};
use magi_core::host::Frame;
use magi_core::text::format_hex_color;
use magi_vision::ImageRef;

use super::{SnipMode, SnipTarget};
use crate::theme;

/// Selections smaller than this (in screen pixels) are treated as a slip of the hand.
const MIN_SELECTION: u32 = 3;

pub enum SnipOutcome {
    Point(Point),
    Region(Rect),
    /// The pixels inside the box the user dragged, as a PNG file.
    Image {
        png: Vec<u8>,
    },
    Color([u8; 3]),
}

pub struct Snip {
    pub mode: SnipMode,
    pub target: SnipTarget,
    frame: Frame,
    texture: TextureHandle,
    /// Where the drag began, in frame pixels.
    anchor: Option<Pos2>,
}

impl Snip {
    /// Captures the primary screen and prepares the picker.
    pub fn begin(ctx: &egui::Context, mode: SnipMode, target: SnipTarget) -> Result<Self, String> {
        let frame = magi_platform::capture::capture(None).map_err(|e| e.to_string())?;
        let image =
            egui::ColorImage::from_rgba_unmultiplied([frame.width as usize, frame.height as usize], &frame.rgba);
        let texture = ctx.load_texture("magi-snip", image, egui::TextureOptions::LINEAR);
        Ok(Self { mode, target, frame, texture, anchor: None })
    }

    fn pixel(&self, x: u32, y: u32) -> [u8; 3] {
        let x = x.min(self.frame.width.saturating_sub(1));
        let y = y.min(self.frame.height.saturating_sub(1));
        let i = ((y * self.frame.width + x) * 4) as usize;
        [self.frame.rgba[i], self.frame.rgba[i + 1], self.frame.rgba[i + 2]]
    }

    /// Shows the picker. Returns `None` while the user is still choosing,
    /// `Some(None)` if they cancelled, `Some(Some(outcome))` when done.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<Option<SnipOutcome>> {
        let builder = egui::ViewportBuilder::default()
            .with_title("MacroMagi – pick on screen")
            .with_decorations(false)
            .with_fullscreen(true)
            .with_always_on_top()
            .with_active(true);
        let mut result = None;
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("magi-snip"), builder, |ui, _class| {
            result = self.contents(ui);
            if ui.input(|i| i.viewport().close_requested()) {
                result = Some(None);
            }
        });
        result
    }

    fn contents(&mut self, ui: &mut egui::Ui) -> Option<Option<SnipOutcome>> {
        let (area, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let painter = ui.painter_at(area);
        let uv = egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        painter.image(self.texture.id(), area, uv, Color32::WHITE);

        // The picture fills the window; convert between window points and screen pixels.
        let scale = vec2(self.frame.width as f32 / area.width(), self.frame.height as f32 / area.height());
        let to_pixels = |p: Pos2| pos2((p.x - area.left()) * scale.x, (p.y - area.top()) * scale.y);
        let to_points = |p: Pos2| pos2(area.left() + p.x / scale.x, area.top() + p.y / scale.y);
        let hover = response.hover_pos().or(response.interact_pointer_pos());
        let hover_px = hover.map(to_pixels);
        let origin = self.frame.origin;
        let screen_point = |p: Pos2| Point::new(origin.x + p.x.floor() as i32, origin.y + p.y.floor() as i32);

        if ui.input(|i| i.key_pressed(Key::Escape)) || response.secondary_clicked() {
            return Some(None);
        }

        let shade = Color32::from_black_alpha(110);
        let mut result = None;
        let hint = match self.mode {
            SnipMode::Point => "Click the position",
            SnipMode::Color => "Click the colour",
            SnipMode::Region => "Drag a box around the area to search",
            SnipMode::Image => "Drag a box around the thing to look for",
        };
        match self.mode {
            SnipMode::Point | SnipMode::Color => {
                painter.rect_filled(area, 0.0, Color32::from_black_alpha(50));
                if let (Some(pos), Some(px)) = (hover, hover_px) {
                    let line = Stroke::new(1.0, theme::accent_bright());
                    painter.hline(area.x_range(), pos.y, line);
                    painter.vline(pos.x, area.y_range(), line);
                    let point = screen_point(px);
                    let rgb = self.pixel(px.x.max(0.0) as u32, px.y.max(0.0) as u32);
                    let label = match self.mode {
                        SnipMode::Color => format_hex_color(rgb),
                        _ => format!("{}, {}", point.x, point.y),
                    };
                    self.tag(
                        &painter,
                        area,
                        pos + vec2(14.0, 14.0),
                        &label,
                        (self.mode == SnipMode::Color).then_some(rgb),
                    );
                    if response.clicked() {
                        result = Some(Some(match self.mode {
                            SnipMode::Color => SnipOutcome::Color(rgb),
                            _ => SnipOutcome::Point(point),
                        }));
                    }
                }
            }
            SnipMode::Region | SnipMode::Image => {
                if response.drag_started() {
                    self.anchor = hover_px;
                }
                let selection = match (self.anchor, hover_px) {
                    (Some(a), Some(b)) if response.dragged() || response.drag_stopped() => {
                        Some(egui::Rect::from_two_pos(a, b))
                    }
                    _ => None,
                };
                match selection {
                    Some(px) => {
                        let shown = egui::Rect::from_min_max(to_points(px.min), to_points(px.max));
                        // Darken everything except the selection.
                        for piece in [
                            egui::Rect::from_min_max(area.min, pos2(area.right(), shown.top())),
                            egui::Rect::from_min_max(pos2(area.left(), shown.bottom()), area.max),
                            egui::Rect::from_min_max(
                                pos2(area.left(), shown.top()),
                                pos2(shown.left(), shown.bottom()),
                            ),
                            egui::Rect::from_min_max(
                                pos2(shown.right(), shown.top()),
                                pos2(area.right(), shown.bottom()),
                            ),
                        ] {
                            painter.rect_filled(piece, 0.0, shade);
                        }
                        painter.rect_stroke(shown, 0.0, Stroke::new(1.5, theme::accent_bright()), StrokeKind::Outside);
                        let region = self.region(px);
                        let label = format!("{} × {}", region.w, region.h);
                        self.tag(&painter, area, shown.left_bottom() + vec2(0.0, 8.0), &label, None);

                        if response.drag_stopped() {
                            self.anchor = None;
                            if region.w >= MIN_SELECTION && region.h >= MIN_SELECTION {
                                result = Some(self.finish(region));
                            }
                        }
                    }
                    None => {
                        painter.rect_filled(area, 0.0, shade);
                        if let Some(pos) = hover {
                            let line = Stroke::new(1.0, theme::with_alpha(theme::accent_bright(), 0.7));
                            painter.hline(area.x_range(), pos.y, line);
                            painter.vline(pos.x, area.y_range(), line);
                        }
                    }
                }
            }
        }

        // Instructions, top centre.
        let text = format!("{hint}   ·   Esc or right-click to cancel");
        let galley = painter.layout_no_wrap(text, FontId::proportional(15.0), Color32::WHITE);
        let size = galley.size() + vec2(28.0, 16.0);
        let banner = egui::Rect::from_center_size(pos2(area.center().x, area.top() + 44.0), size);
        painter.rect_filled(banner, 10.0, Color32::from_black_alpha(200));
        painter.rect_stroke(banner, 10.0, Stroke::new(1.0, theme::stroke_strong()), StrokeKind::Inside);
        painter.galley(banner.center() - galley.size() / 2.0, galley, Color32::WHITE);

        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        result
    }

    /// A small label next to the pointer, kept inside the window.
    fn tag(&self, painter: &egui::Painter, area: egui::Rect, at: Pos2, text: &str, swatch: Option<[u8; 3]>) {
        let galley = painter.layout_no_wrap(text.to_owned(), FontId::monospace(12.5), Color32::WHITE);
        let swatch_width = if swatch.is_some() { 22.0 } else { 0.0 };
        let size = galley.size() + vec2(14.0 + swatch_width, 8.0);
        let mut rect = egui::Rect::from_min_size(at, size);
        rect = rect.translate(Vec2::new(
            (area.right() - 6.0 - rect.right()).min(0.0),
            (area.bottom() - 6.0 - rect.bottom()).min(0.0),
        ));
        painter.rect_filled(rect, 5.0, Color32::from_black_alpha(215));
        let mut text_pos = rect.min + vec2(7.0, 4.0);
        if let Some([r, g, b]) = swatch {
            let chip = egui::Rect::from_min_size(rect.min + vec2(6.0, 5.0), vec2(16.0, size.y - 10.0));
            painter.rect_filled(chip, 3.0, Color32::from_rgb(r, g, b));
            painter.rect_stroke(chip, 3.0, Stroke::new(1.0, Color32::from_white_alpha(120)), StrokeKind::Inside);
            text_pos.x += swatch_width;
        }
        painter.galley(text_pos, galley, Color32::WHITE);
        let _ = Align2::LEFT_TOP;
    }

    /// The screen rectangle for a selection given in frame pixels.
    fn region(&self, px: egui::Rect) -> Rect {
        let clamp = |v: f32, max: u32| (v.round().max(0.0) as u32).min(max);
        let (x0, y0) = (clamp(px.left(), self.frame.width), clamp(px.top(), self.frame.height));
        let (x1, y1) = (clamp(px.right(), self.frame.width), clamp(px.bottom(), self.frame.height));
        Rect::new(self.frame.origin.x + x0 as i32, self.frame.origin.y + y0 as i32, x1 - x0, y1 - y0)
    }

    fn finish(&self, region: Rect) -> Option<SnipOutcome> {
        if self.mode == SnipMode::Region {
            return Some(SnipOutcome::Region(region));
        }
        let crop = self.frame.crop(region)?;
        let png = magi_vision::encode_png(ImageRef::new(crop.width, crop.height, &crop.rgba)).ok()?;
        Some(SnipOutcome::Image { png })
    }
}

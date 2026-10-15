//! View → Screen targets: every position the macro clicks and every area it
//! searches, drawn over a frozen picture of the screen and numbered in the
//! order a run reaches them. Each can be dragged into place; areas can also
//! be resized by their corners. Changes go straight into the macro.

use egui::{Align2, Color32, FontId, Key, Pos2, Sense, Stroke, StrokeKind, TextureHandle, pos2, vec2};
use egui_phosphor::regular as icon;
use magi_core::geom::{Point, Rect};
use magi_core::nodes::Registry;
use magi_core::project::Project;
use magi_core::targets::{self, ScreenTarget, TargetShape, TargetSource};

use crate::theme;

/// How close (in window points) the pointer must be to grab a point or corner.
const GRAB: f32 = 12.0;

#[derive(Clone, Copy, PartialEq)]
enum Grip {
    Move,
    /// A region corner: 0 top-left, 1 top-right, 2 bottom-left, 3 bottom-right.
    Corner(u8),
}

struct Drag {
    source: TargetSource,
    grip: Grip,
    /// Where the drag began, in screen pixels.
    from: Pos2,
    original: TargetShape,
}

pub struct TargetOverlay {
    origin: Point,
    size: (u32, u32),
    texture: TextureHandle,
    selected: Option<TargetSource>,
    drag: Option<Drag>,
}

fn region_corners(r: Rect) -> [Pos2; 4] {
    let (l, t, rr, b) = (r.x as f32, r.y as f32, (r.x + r.w as i32) as f32, (r.y + r.h as i32) as f32);
    [pos2(l, t), pos2(rr, t), pos2(l, b), pos2(rr, b)]
}

fn moved(shape: TargetShape, grip: Grip, dx: i32, dy: i32) -> TargetShape {
    match (shape, grip) {
        (TargetShape::Point(p), _) => TargetShape::Point(Point::new(p.x + dx, p.y + dy)),
        (TargetShape::Region(r), Grip::Move) => TargetShape::Region(Rect::new(r.x + dx, r.y + dy, r.w, r.h)),
        (TargetShape::Region(r), Grip::Corner(corner)) => {
            let (mut l, mut t, mut right, mut bottom) = (r.x, r.y, r.x + r.w as i32, r.y + r.h as i32);
            match corner {
                0 => (l, t) = (l + dx, t + dy),
                1 => (right, t) = (right + dx, t + dy),
                2 => (l, bottom) = (l + dx, bottom + dy),
                _ => (right, bottom) = (right + dx, bottom + dy),
            }
            let (x0, x1) = (l.min(right), l.max(right));
            let (y0, y1) = (t.min(bottom), t.max(bottom));
            TargetShape::Region(Rect::new(x0, y0, ((x1 - x0) as u32).max(1), ((y1 - y0) as u32).max(1)))
        }
    }
}

impl TargetOverlay {
    /// Captures the primary screen to draw the targets on.
    pub fn begin(ctx: &egui::Context) -> Result<Self, String> {
        let frame = magi_platform::capture::capture(None).map_err(|e| e.to_string())?;
        let image =
            egui::ColorImage::from_rgba_unmultiplied([frame.width as usize, frame.height as usize], &frame.rgba);
        let texture = ctx.load_texture("magi-targets", image, egui::TextureOptions::LINEAR);
        Ok(Self { origin: frame.origin, size: (frame.width, frame.height), texture, selected: None, drag: None })
    }

    /// Shows the overlay. Returns true once the user has closed it.
    pub fn show(&mut self, ctx: &egui::Context, project: &mut Project, registry: &Registry) -> bool {
        let builder = egui::ViewportBuilder::default()
            .with_title("MacroMagi – screen targets")
            .with_decorations(false)
            .with_fullscreen(true)
            .with_always_on_top()
            .with_active(true);
        let mut closed = false;
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("magi-targets"), builder, |ui, _class| {
            closed = self.contents(ui, project, registry);
            if ui.input(|i| i.viewport().close_requested()) {
                closed = true;
            }
        });
        closed
    }

    fn contents(&mut self, ui: &mut egui::Ui, project: &mut Project, registry: &Registry) -> bool {
        let (area, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let painter = ui.painter_at(area);
        let uv = egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        painter.image(self.texture.id(), area, uv, Color32::WHITE);
        painter.rect_filled(area, 0.0, Color32::from_black_alpha(90));

        // Screen pixels <-> window points.
        let scale = vec2(self.size.0 as f32 / area.width(), self.size.1 as f32 / area.height());
        let origin = self.origin;
        let to_points = move |x: f32, y: f32| {
            pos2(area.left() + (x - origin.x as f32) / scale.x, area.top() + (y - origin.y as f32) / scale.y)
        };
        let to_pixels = move |p: Pos2| {
            pos2(origin.x as f32 + (p.x - area.left()) * scale.x, origin.y as f32 + (p.y - area.top()) * scale.y)
        };

        let list = targets::screen_targets(&project.graph, registry);
        let pointer = response.interact_pointer_pos().or(response.hover_pos());

        // What is under the pointer: the last drawn wins.
        let hit = |p: Pos2| -> Option<(TargetSource, Grip, TargetShape)> {
            list.iter().rev().find_map(|target| match target.shape {
                TargetShape::Point(point) => (to_points(point.x as f32, point.y as f32).distance(p) <= GRAB)
                    .then(|| (target.source.clone(), Grip::Move, target.shape)),
                TargetShape::Region(region) => {
                    let corners = region_corners(region).map(|c| to_points(c.x, c.y));
                    if let Some(corner) = corners.iter().position(|c| c.distance(p) <= GRAB) {
                        return Some((target.source.clone(), Grip::Corner(corner as u8), target.shape));
                    }
                    let rect = egui::Rect::from_two_pos(corners[0], corners[3]);
                    rect.contains(p).then(|| (target.source.clone(), Grip::Move, target.shape))
                }
            })
        };

        if response.drag_started()
            && let Some(p) = pointer
            && let Some((source, grip, original)) = hit(p)
        {
            self.selected = Some(source.clone());
            self.drag = Some(Drag { source, grip, from: to_pixels(p), original });
        }
        if let (Some(drag), Some(p)) = (&self.drag, pointer)
            && response.dragged()
        {
            let now = to_pixels(p);
            let (dx, dy) = ((now.x - drag.from.x).round() as i32, (now.y - drag.from.y).round() as i32);
            targets::apply(&mut project.graph, &drag.source, moved(drag.original, drag.grip, dx, dy));
        }
        if response.drag_stopped() {
            self.drag = None;
        }
        if response.clicked() {
            self.selected = pointer.and_then(hit).map(|(source, ..)| source);
        }
        let hovered = pointer.and_then(hit);
        match hovered.as_ref().map(|(_, grip, _)| *grip) {
            Some(Grip::Corner(0 | 3)) => ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe),
            Some(Grip::Corner(_)) => ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNeSw),
            Some(Grip::Move) => ui.ctx().set_cursor_icon(egui::CursorIcon::Grab),
            None => {}
        }

        // The run order, as a faint path between targets.
        let centers: Vec<Pos2> = list
            .iter()
            .map(|t| match t.shape {
                TargetShape::Point(p) => to_points(p.x as f32, p.y as f32),
                TargetShape::Region(r) => to_points(r.x as f32 + r.w as f32 / 2.0, r.y as f32 + r.h as f32 / 2.0),
            })
            .collect();
        for pair in centers.windows(2) {
            painter.line_segment([pair[0], pair[1]], Stroke::new(1.0, Color32::from_white_alpha(60)));
        }

        for (target, center) in list.iter().zip(&centers) {
            let selected = self.selected.as_ref() == Some(&target.source);
            let is_hovered = hovered.as_ref().is_some_and(|(source, ..)| *source == target.source);
            self.paint_target(&painter, target, *center, selected || is_hovered, &to_points);
        }

        self.side_list(ui, area, &list);
        self.banner(&painter, area, list.is_empty());
        ui.input(|i| i.key_pressed(Key::Escape) || i.key_pressed(Key::Enter)) || response.secondary_clicked()
    }

    fn paint_target(
        &self,
        painter: &egui::Painter,
        target: &ScreenTarget,
        center: Pos2,
        emphasis: bool,
        to_points: &dyn Fn(f32, f32) -> Pos2,
    ) {
        let color = if emphasis { theme::accent_bright() } else { Color32::from_rgb(0xFF, 0xD1, 0x4D) };
        let badge_at = match target.shape {
            TargetShape::Point(_) => {
                painter.circle_stroke(center, 9.0, Stroke::new(2.0, color));
                painter.line_segment([center - vec2(14.0, 0.0), center + vec2(14.0, 0.0)], Stroke::new(1.0, color));
                painter.line_segment([center - vec2(0.0, 14.0), center + vec2(0.0, 14.0)], Stroke::new(1.0, color));
                center + vec2(14.0, -14.0)
            }
            TargetShape::Region(region) => {
                let corners = region_corners(region).map(|c| to_points(c.x, c.y));
                let rect = egui::Rect::from_two_pos(corners[0], corners[3]);
                painter.rect_filled(rect, 0.0, color.gamma_multiply(0.08));
                painter.rect_stroke(rect, 0.0, Stroke::new(2.0, color), StrokeKind::Middle);
                for corner in corners {
                    painter.rect_filled(egui::Rect::from_center_size(corner, vec2(8.0, 8.0)), 1.0, color);
                }
                rect.left_top() + vec2(4.0, -14.0)
            }
        };
        painter.circle_filled(badge_at, 10.0, color);
        painter.text(
            badge_at,
            Align2::CENTER_CENTER,
            target.order.to_string(),
            FontId::proportional(12.0),
            Color32::BLACK,
        );
        if emphasis {
            let galley = painter.layout_no_wrap(target.label.clone(), FontId::proportional(13.0), Color32::WHITE);
            let at = badge_at + vec2(14.0, -galley.size().y / 2.0);
            let back = egui::Rect::from_min_size(at - vec2(5.0, 3.0), galley.size() + vec2(10.0, 6.0));
            painter.rect_filled(back, 5.0, Color32::from_black_alpha(210));
            painter.galley(at, galley, Color32::WHITE);
        }
    }

    /// The numbered list, top left; clicking a line selects that target.
    fn side_list(&mut self, ui: &mut egui::Ui, area: egui::Rect, list: &[ScreenTarget]) {
        egui::Area::new(egui::Id::new("magi-targets-list"))
            .order(egui::Order::Foreground)
            .fixed_pos(area.left_top() + vec2(16.0, 16.0))
            .show(ui.ctx(), |ui| {
                egui::Frame::new().fill(Color32::from_black_alpha(215)).corner_radius(8).inner_margin(10).show(
                    ui,
                    |ui| {
                        ui.set_max_width(300.0);
                        ui.label(
                            egui::RichText::new("SCREEN TARGETS").size(10.5).color(Color32::from_white_alpha(170)),
                        );
                        egui::ScrollArea::vertical().max_height(area.height() * 0.6).show(ui, |ui| {
                            for target in list {
                                let selected = self.selected.as_ref() == Some(&target.source);
                                let glyph = match target.shape {
                                    TargetShape::Point(_) => icon::CROSSHAIR,
                                    TargetShape::Region(_) => icon::SELECTION,
                                };
                                let text = format!("{}.  {glyph}  {}", target.order, target.label);
                                if ui
                                    .selectable_label(selected, egui::RichText::new(text).color(Color32::WHITE))
                                    .clicked()
                                {
                                    self.selected = Some(target.source.clone());
                                }
                            }
                        });
                    },
                );
            });
    }

    fn banner(&self, painter: &egui::Painter, area: egui::Rect, empty: bool) {
        let text = if empty {
            "This macro has no click positions or search areas yet   ·   Esc to close".to_owned()
        } else {
            "Drag a target to move it, or an area's corner to resize it   ·   Esc or Enter when done".to_owned()
        };
        let galley = painter.layout_no_wrap(text, FontId::proportional(15.0), Color32::WHITE);
        let size = galley.size() + vec2(28.0, 16.0);
        let banner = egui::Rect::from_center_size(pos2(area.center().x, area.top() + 44.0), size);
        painter.rect_filled(banner, 10.0, Color32::from_black_alpha(200));
        painter.rect_stroke(banner, 10.0, Stroke::new(1.0, Color32::from_white_alpha(60)), StrokeKind::Inside);
        painter.galley(banner.center() - galley.size() / 2.0, galley, Color32::WHITE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_resize_and_never_turn_inside_out() {
        let region = TargetShape::Region(Rect::new(10, 10, 100, 50));
        assert_eq!(moved(region, Grip::Corner(3), 20, 10), TargetShape::Region(Rect::new(10, 10, 120, 60)));
        assert_eq!(moved(region, Grip::Corner(0), -5, -5), TargetShape::Region(Rect::new(5, 5, 105, 55)));
        // Dragging the left edge past the right one flips it rather than breaking it.
        assert_eq!(moved(region, Grip::Corner(0), 150, 0), TargetShape::Region(Rect::new(110, 10, 50, 50)));
        assert_eq!(moved(region, Grip::Move, 3, 4), TargetShape::Region(Rect::new(13, 14, 100, 50)));
        let point = TargetShape::Point(Point::new(1, 2));
        assert_eq!(moved(point, Grip::Corner(1), 3, 4), TargetShape::Point(Point::new(4, 6)));
    }
}

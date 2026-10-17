//! View › Screen targets: every position the macro clicks and every area it
//! searches, drawn over a frozen picture of one display and numbered in the
//! order a run reaches them. Each can be dragged into place; areas can also
//! be resized by their corners. Changes go straight into the macro.
//!
//! The picture is shown as it is – not dimmed – so the window is
//! indistinguishable from the screen underneath, and only the targets stand
//! out. The window is a *deferred* viewport: it redraws on its own input only,
//! not every time the main window does, which is what made it flicker.
//! The two talk through [`Shared`]: the main window hands over the targets and
//! takes back the edits.

use std::sync::{Arc, Mutex, MutexGuard};

use egui::{Align2, Color32, FontId, Key, Pos2, Sense, Stroke, StrokeKind, TextureHandle, pos2, vec2};
use egui_phosphor::regular as icon;
use magi_core::geom::{Displays, Point, Rect};
use magi_core::graph::NodeId;
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
    /// The shape as dragged so far, shown until the macro has it.
    current: TargetShape,
}

/// What the main window learns from the overlay each frame.
pub enum OverlayOutcome {
    Open,
    Closed,
    /// A target was double-clicked: show the block it belongs to.
    ShowBlock(NodeId),
}

/// The picture of the display being shown.
struct Picture {
    texture: TextureHandle,
    /// The display's top-left corner on the desktop, and its size, in pixels.
    origin: Point,
    size: (u32, u32),
    display: Option<u8>,
}

/// State both windows see.
struct Shared {
    picture: Picture,
    displays: Displays,
    targets: Vec<ScreenTarget>,
    selected: Option<TargetSource>,
    drag: Option<Drag>,
    /// Moves and resizes for the main window to write into the macro.
    edits: Vec<(TargetSource, TargetShape)>,
    switch_to: Option<u8>,
    closed: bool,
    show_block: Option<NodeId>,
}

pub struct TargetOverlay {
    shared: Arc<Mutex<Shared>>,
}

fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(|p| p.into_inner())
}

fn region_corners(r: Rect) -> [Pos2; 4] {
    let (l, t, rr, b) = (r.x as f32, r.y as f32, (r.x + r.w as i32) as f32, (r.y + r.h as i32) as f32);
    [pos2(l, t), pos2(rr, t), pos2(l, b), pos2(rr, b)]
}

fn moved(shape: TargetShape, grip: Grip, dx: i32, dy: i32) -> TargetShape {
    match (shape, grip) {
        (TargetShape::Point(p), _) => TargetShape::Point(Point { x: p.x + dx, y: p.y + dy, display: p.display }),
        (TargetShape::Region(r), Grip::Move) => TargetShape::Region(Rect { x: r.x + dx, y: r.y + dy, ..r }),
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
            TargetShape::Region(Rect {
                x: x0,
                y: y0,
                w: ((x1 - x0) as u32).max(1),
                h: ((y1 - y0) as u32).max(1),
                display: r.display,
            })
        }
    }
}

/// A target's shape in desktop coordinates.
fn global(shape: TargetShape, displays: &Displays) -> TargetShape {
    match shape {
        TargetShape::Point(p) => TargetShape::Point(displays.to_global(p)),
        TargetShape::Region(r) => TargetShape::Region(displays.rect_to_global(r)),
    }
}

/// The display a target is on.
fn display_of(shape: TargetShape, displays: &Displays) -> Option<u8> {
    let point = match global(shape, displays) {
        TargetShape::Point(p) => p,
        TargetShape::Region(r) => r.center(),
    };
    displays.index_of(point)
}

fn capture(ctx: &egui::Context, displays: &Displays, display: Option<u8>) -> Result<Picture, String> {
    let area = display.and_then(|d| displays.get(d));
    let frame = magi_platform::capture::capture(area).map_err(|e| e.to_string())?;
    let texture = super::images::screen_texture(ctx, "magi-targets", frame.width, frame.height, &frame.rgba);
    Ok(Picture { texture, origin: frame.origin, size: (frame.width, frame.height), display })
}

impl TargetOverlay {
    /// Captures the display MacroMagi is on to draw the targets on.
    pub fn begin(ctx: &egui::Context) -> Result<Self, String> {
        let displays = crate::displays::current();
        let display = super::snip::display_of_window(ctx, &displays);
        let picture = capture(ctx, &displays, display)?;
        Ok(Self {
            shared: Arc::new(Mutex::new(Shared {
                picture,
                displays,
                targets: Vec::new(),
                selected: None,
                drag: None,
                edits: Vec::new(),
                switch_to: None,
                closed: false,
                show_block: None,
            })),
        })
    }

    /// Keeps the overlay up to date with the macro and shows it.
    pub fn show(&mut self, ctx: &egui::Context, project: &mut Project, registry: &Registry) -> OverlayOutcome {
        let id = egui::ViewportId::from_hash_of("magi-targets");
        let mut shared = lock(&self.shared);
        let edited = !shared.edits.is_empty();
        for (source, shape) in std::mem::take(&mut shared.edits) {
            targets::apply(&mut project.graph, &source, shape);
        }
        shared.targets = targets::screen_targets(&project.graph, registry);
        let mut switched = false;
        if let Some(display) = shared.switch_to.take()
            && let Ok(picture) = capture(ctx, &shared.displays, Some(display))
        {
            shared.picture = picture;
            shared.drag = None;
            switched = true;
        }
        // The overlay redraws on its own input only: tell it when there is news.
        if edited || switched {
            ctx.request_repaint_of(id);
        }
        if shared.closed {
            return OverlayOutcome::Closed;
        }
        if let Some(node) = shared.show_block.take() {
            return OverlayOutcome::ShowBlock(node);
        }
        let mut builder = egui::ViewportBuilder::default()
            .with_title("MacroMagi – screen targets")
            .with_decorations(false)
            .with_fullscreen(true)
            .with_always_on_top()
            .with_active(true);
        if let Some(index) = shared.picture.display.and_then(crate::displays::monitor_index) {
            builder = builder.with_monitor(index);
        }
        drop(shared);
        let shared = Arc::clone(&self.shared);
        ctx.show_viewport_deferred(id, builder, move |ui, _class| {
            let mut state = lock(&shared);
            let close = state.contents(ui);
            if close || ui.input(|i| i.viewport().close_requested()) {
                state.closed = true;
            }
            let wake =
                state.closed || state.show_block.is_some() || !state.edits.is_empty() || state.switch_to.is_some();
            drop(state);
            if wake {
                ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
            }
        });
        OverlayOutcome::Open
    }
}

impl Shared {
    /// Draws one frame. Returns true when the user closes the overlay.
    fn contents(&mut self, ui: &mut egui::Ui) -> bool {
        let (area, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let painter = ui.painter_at(area);
        let uv = egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        painter.image(self.picture.texture.id(), area, uv, Color32::WHITE);

        // Desktop pixels <-> window points.
        let scale = vec2(self.picture.size.0 as f32 / area.width(), self.picture.size.1 as f32 / area.height());
        let origin = self.picture.origin;
        let to_points = move |x: f32, y: f32| {
            pos2(area.left() + (x - origin.x as f32) / scale.x, area.top() + (y - origin.y as f32) / scale.y)
        };
        let to_pixels = move |p: Pos2| {
            pos2(origin.x as f32 + (p.x - area.left()) * scale.x, origin.y as f32 + (p.y - area.top()) * scale.y)
        };

        // The targets as shown: a target being dragged is where it is dragged to.
        let displays = self.displays.clone();
        let shown: Vec<(ScreenTarget, TargetShape)> = self
            .targets
            .iter()
            .map(|target| {
                let shape = match &self.drag {
                    Some(drag) if drag.source == target.source => drag.current,
                    _ => target.shape,
                };
                (target.clone(), global(shape, &displays))
            })
            .collect();
        let pointer = response.interact_pointer_pos().or(response.hover_pos());

        // What is under the pointer: the last drawn wins.
        let hit = |p: Pos2| -> Option<(usize, Grip)> {
            shown.iter().enumerate().rev().find_map(|(index, (_, shape))| match shape {
                TargetShape::Point(point) => {
                    (to_points(point.x as f32, point.y as f32).distance(p) <= GRAB).then_some((index, Grip::Move))
                }
                TargetShape::Region(region) => {
                    let corners = region_corners(*region).map(|c| to_points(c.x, c.y));
                    if let Some(corner) = corners.iter().position(|c| c.distance(p) <= GRAB) {
                        return Some((index, Grip::Corner(corner as u8)));
                    }
                    let rect = egui::Rect::from_two_pos(corners[0], corners[3]);
                    rect.contains(p).then_some((index, Grip::Move))
                }
            })
        };

        if response.drag_started()
            && let Some(p) = pointer
            && let Some((index, grip)) = hit(p)
        {
            let target = &shown[index].0;
            self.selected = Some(target.source.clone());
            self.drag = Some(Drag {
                source: target.source.clone(),
                grip,
                from: to_pixels(p),
                original: target.shape,
                current: target.shape,
            });
        }
        if let (Some(drag), Some(p)) = (&mut self.drag, pointer)
            && response.dragged()
        {
            let now = to_pixels(p);
            let (dx, dy) = ((now.x - drag.from.x).round() as i32, (now.y - drag.from.y).round() as i32);
            let shape = moved(drag.original, drag.grip, dx, dy);
            if shape != drag.current {
                drag.current = shape;
                self.edits.push((drag.source.clone(), shape));
            }
        }
        if response.drag_stopped() {
            self.drag = None;
        }
        if response.clicked() {
            self.selected = pointer.and_then(hit).map(|(index, _)| shown[index].0.source.clone());
        }
        if response.double_clicked()
            && let Some((index, _)) = pointer.and_then(hit)
        {
            self.show_block = Some(shown[index].0.node);
        }
        let hovered = pointer.and_then(hit);
        match hovered.map(|(_, grip)| grip) {
            Some(Grip::Corner(0 | 3)) => ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe),
            Some(Grip::Corner(_)) => ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNeSw),
            Some(Grip::Move) => ui.ctx().set_cursor_icon(egui::CursorIcon::Grab),
            None => {}
        }

        // The run order, as a faint path between targets.
        let centers: Vec<Pos2> = shown
            .iter()
            .map(|(_, shape)| match shape {
                TargetShape::Point(p) => to_points(p.x as f32, p.y as f32),
                TargetShape::Region(r) => to_points(r.x as f32 + r.w as f32 / 2.0, r.y as f32 + r.h as f32 / 2.0),
            })
            .collect();
        for pair in centers.windows(2) {
            painter.line_segment([pair[0], pair[1]], Stroke::new(1.5, Color32::from_black_alpha(90)));
            painter.line_segment([pair[0], pair[1]], Stroke::new(1.0, Color32::from_white_alpha(110)));
        }

        for (index, ((target, shape), center)) in shown.iter().zip(&centers).enumerate() {
            let selected = self.selected.as_ref() == Some(&target.source);
            let is_hovered = hovered.is_some_and(|(i, _)| i == index);
            paint_target(&painter, target, *shape, *center, selected || is_hovered, &to_points);
        }

        self.side_list(ui, area);
        banner(&painter, area, shown.is_empty());
        ui.input(|i| i.key_pressed(Key::Escape) || i.key_pressed(Key::Enter)) || response.secondary_clicked()
    }

    /// The numbered list, top left, and the display buttons. Clicking a line
    /// selects that target (showing its display); double-clicking it shows
    /// its block in the editor.
    fn side_list(&mut self, ui: &mut egui::Ui, area: egui::Rect) {
        let several = self.displays.len() > 1;
        egui::Area::new(egui::Id::new("magi-targets-list"))
            .order(egui::Order::Foreground)
            .fade_in(false)
            .fixed_pos(area.left_top() + vec2(16.0, 16.0))
            .show(ui.ctx(), |ui| {
                egui::Frame::new().fill(Color32::from_black_alpha(220)).corner_radius(8).inner_margin(10).show(
                    ui,
                    |ui| {
                        ui.set_max_width(320.0);
                        ui.label(
                            egui::RichText::new("SCREEN TARGETS").size(10.5).color(Color32::from_white_alpha(170)),
                        );
                        if several {
                            ui.horizontal_wrapped(|ui| {
                                for (d, _) in self.displays.iter() {
                                    let text = egui::RichText::new(format!("{}  {}", icon::MONITOR, d + 1))
                                        .color(Color32::WHITE);
                                    let current = self.picture.display == Some(d);
                                    if ui
                                        .selectable_label(current, text)
                                        .on_hover_text(self.displays.label(d))
                                        .clicked()
                                        && !current
                                    {
                                        self.switch_to = Some(d);
                                    }
                                }
                            });
                        }
                        egui::ScrollArea::vertical().max_height(area.height() * 0.6).show(ui, |ui| {
                            for target in &self.targets {
                                let selected = self.selected.as_ref() == Some(&target.source);
                                let glyph = match target.shape {
                                    TargetShape::Point(_) => icon::CROSSHAIR,
                                    TargetShape::Region(_) => icon::SELECTION,
                                };
                                let display = display_of(target.shape, &self.displays);
                                let elsewhere = several && display.is_some() && display != self.picture.display;
                                let mut text = format!("{}.  {glyph}  {}", target.order, target.label);
                                if elsewhere {
                                    text.push_str(&format!("  (display {})", display.map_or(1, |d| d + 1)));
                                }
                                let color = if elsewhere { Color32::from_white_alpha(140) } else { Color32::WHITE };
                                let response = ui
                                    .selectable_label(selected, egui::RichText::new(text).color(color))
                                    .on_hover_text("Click to select · double-click to show its block in the editor");
                                if response.double_clicked() {
                                    self.show_block = Some(target.node);
                                } else if response.clicked() {
                                    self.selected = Some(target.source.clone());
                                    if elsewhere {
                                        self.switch_to = display;
                                    }
                                }
                            }
                        });
                    },
                );
            });
    }
}

fn paint_target(
    painter: &egui::Painter,
    target: &ScreenTarget,
    shape: TargetShape,
    center: Pos2,
    emphasis: bool,
    to_points: &dyn Fn(f32, f32) -> Pos2,
) {
    let color = if emphasis { theme::accent_bright() } else { Color32::from_rgb(0xFF, 0xD1, 0x4D) };
    // A dark edge keeps the marks readable on any picture.
    let edge = Stroke::new(4.0, Color32::from_black_alpha(130));
    let badge_at = match shape {
        TargetShape::Point(_) => {
            painter.circle_stroke(center, 9.0, edge);
            painter.circle_stroke(center, 9.0, Stroke::new(2.0, color));
            for (a, b) in [(vec2(-14.0, 0.0), vec2(14.0, 0.0)), (vec2(0.0, -14.0), vec2(0.0, 14.0))] {
                painter.line_segment([center + a, center + b], Stroke::new(3.0, Color32::from_black_alpha(110)));
                painter.line_segment([center + a, center + b], Stroke::new(1.0, color));
            }
            center + vec2(14.0, -14.0)
        }
        TargetShape::Region(region) => {
            let corners = region_corners(region).map(|c| to_points(c.x, c.y));
            let rect = egui::Rect::from_two_pos(corners[0], corners[3]);
            painter.rect_filled(rect, 0.0, color.gamma_multiply(0.08));
            painter.rect_stroke(rect, 0.0, edge, StrokeKind::Middle);
            painter.rect_stroke(rect, 0.0, Stroke::new(2.0, color), StrokeKind::Middle);
            for corner in corners {
                painter.rect_filled(egui::Rect::from_center_size(corner, vec2(8.0, 8.0)), 1.0, color);
            }
            rect.left_top() + vec2(4.0, -14.0)
        }
    };
    painter.circle_filled(badge_at, 10.0, color);
    painter.text(badge_at, Align2::CENTER_CENTER, target.order.to_string(), FontId::proportional(12.0), Color32::BLACK);
    if emphasis {
        let galley = painter.layout_no_wrap(target.label.clone(), FontId::proportional(13.0), Color32::WHITE);
        let at = badge_at + vec2(14.0, -galley.size().y / 2.0);
        let back = egui::Rect::from_min_size(at - vec2(5.0, 3.0), galley.size() + vec2(10.0, 6.0));
        painter.rect_filled(back, 5.0, Color32::from_black_alpha(210));
        painter.galley(at, galley, Color32::WHITE);
    }
}

fn banner(painter: &egui::Painter, area: egui::Rect, empty: bool) {
    let text = if empty {
        "This macro has no click positions or search areas yet   ·   Esc to close".to_owned()
    } else {
        "Drag a target to move it, or an area's corner to resize it · double-click one to show its block   ·   \
         Esc or Enter when done"
            .to_owned()
    };
    let galley = painter.layout_no_wrap(text, FontId::proportional(15.0), Color32::WHITE);
    let size = galley.size() + vec2(28.0, 16.0);
    let banner = egui::Rect::from_center_size(pos2(area.center().x, area.top() + 44.0), size);
    painter.rect_filled(banner, 10.0, Color32::from_black_alpha(200));
    painter.rect_stroke(banner, 10.0, Stroke::new(1.0, Color32::from_white_alpha(60)), StrokeKind::Inside);
    painter.galley(banner.center() - galley.size() / 2.0, galley, Color32::WHITE);
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

    #[test]
    fn targets_keep_the_display_they_are_on_when_moved() {
        let displays = Displays::new(vec![Rect::new(0, 0, 100, 100), Rect::new(100, 0, 100, 100)]);
        let point = TargetShape::Point(Point::on(1, 10, 20));
        assert_eq!(global(point, &displays), TargetShape::Point(Point::new(110, 20)));
        assert_eq!(display_of(point, &displays), Some(1));
        assert_eq!(moved(point, Grip::Move, 5, 5), TargetShape::Point(Point::on(1, 15, 25)));
        let area = TargetShape::Region(Rect { x: 0, y: 0, w: 10, h: 10, display: Some(1) });
        assert_eq!(global(area, &displays), TargetShape::Region(Rect::new(100, 0, 10, 10)));
    }
}

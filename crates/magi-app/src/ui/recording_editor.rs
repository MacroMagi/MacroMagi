//! The editable view of a recording: a timeline strip and a table with one
//! row per recorded input – what it was, where, when it was pressed and for
//! how long it was held.
//!
//! The editor only *reads* the recording and reports at most one
//! [`RecordingEdit`] per frame. That way an untouched recording is never
//! copied, however often it is drawn.

use egui::{Color32, DragValue, FontId, Rect, RichText, Sense, Ui, pos2, vec2};
use egui_phosphor::regular as icon;
use magi_core::input::{Key, MouseButton};
use magi_core::recording::{Recording, StrokeInput};
use magi_core::text::format_ms;

use crate::theme;

const ROW_HEIGHT: f32 = 24.0;
const TIMELINE_HEIGHT: f32 = 30.0;

pub enum RecordingEdit {
    At(usize, f64),
    Hold(usize, f64),
    Key(usize, Key),
    Button(usize, MouseButton),
    Position(usize, i32, i32),
    Scroll(usize, i32, i32),
    Delete(usize),
    /// Multiply every time by a factor (0.5 = twice as fast).
    Stretch(f64),
    TrimStart,
}

impl RecordingEdit {
    pub fn apply(self, recording: &mut Recording) {
        match self {
            RecordingEdit::At(i, at) => {
                if let Some(stroke) = recording.strokes.get_mut(i) {
                    stroke.at = at.max(0.0);
                }
            }
            RecordingEdit::Hold(i, hold) => {
                if let Some(stroke) = recording.strokes.get_mut(i) {
                    stroke.hold = hold.max(0.0);
                }
            }
            RecordingEdit::Key(i, new_key) => {
                if let Some(StrokeInput::Key { key }) = recording.strokes.get_mut(i).map(|s| &mut s.input) {
                    *key = new_key;
                }
            }
            RecordingEdit::Button(i, new_button) => {
                if let Some(StrokeInput::Mouse { button, .. }) = recording.strokes.get_mut(i).map(|s| &mut s.input) {
                    *button = new_button;
                }
            }
            RecordingEdit::Position(i, new_x, new_y) => {
                if let Some(StrokeInput::Mouse { x, y, end, .. }) = recording.strokes.get_mut(i).map(|s| &mut s.input) {
                    // Moving a drag moves both of its ends.
                    if let Some(end) = end {
                        end.x += new_x - *x;
                        end.y += new_y - *y;
                    }
                    (*x, *y) = (new_x, new_y);
                }
            }
            RecordingEdit::Scroll(i, new_dx, new_dy) => {
                if let Some(StrokeInput::Scroll { dx, dy }) = recording.strokes.get_mut(i).map(|s| &mut s.input) {
                    (*dx, *dy) = (new_dx, new_dy);
                }
            }
            RecordingEdit::Delete(i) => {
                if i < recording.strokes.len() {
                    recording.strokes.remove(i);
                }
            }
            RecordingEdit::Stretch(factor) => {
                for stroke in &mut recording.strokes {
                    stroke.at *= factor;
                    stroke.hold *= factor;
                    if let StrokeInput::Move { path } = &mut stroke.input {
                        for point in path {
                            point.0 = (f64::from(point.0) * factor) as f32;
                        }
                    }
                }
                recording.duration_ms *= factor;
            }
            RecordingEdit::TrimStart => recording.trim_start(),
        }
        // Rows keep their order while being edited (so a row does not jump
        // away under the pointer); playback orders events by time regardless.
        recording.duration_ms = recording.duration_ms.max(recording.end_time());
    }
}

fn stroke_color(input: &StrokeInput) -> Color32 {
    match input {
        StrokeInput::Key { .. } => Color32::from_rgb(0x4D, 0xA3, 0xFF),
        StrokeInput::Mouse { .. } => theme::SUCCESS,
        StrokeInput::Move { .. } => theme::TEXT_FAINT,
        StrokeInput::Scroll { .. } => theme::WARNING,
        StrokeInput::PadButton { .. } | StrokeInput::PadAxis { .. } => Color32::from_rgb(0xFF, 0x7A, 0xB8),
    }
}

fn stroke_icon(input: &StrokeInput) -> &'static str {
    match input {
        StrokeInput::Key { .. } => icon::KEYBOARD,
        StrokeInput::Mouse { .. } => icon::CURSOR_CLICK,
        StrokeInput::Move { .. } => icon::PATH,
        StrokeInput::Scroll { .. } => icon::ARROWS_VERTICAL,
        StrokeInput::PadButton { .. } | StrokeInput::PadAxis { .. } => icon::GAME_CONTROLLER,
    }
}

/// Draws every stroke as a bar from its press to its release, one lane per kind.
fn timeline(ui: &mut Ui, recording: &Recording) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), TIMELINE_HEIGHT), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 5.0, theme::BG_INPUT);
    let duration = recording.duration_ms.max(recording.end_time()).max(1.0);
    let inner = rect.shrink2(vec2(4.0, 4.0));
    let lane_height = inner.height() / 4.0;
    for stroke in &recording.strokes {
        let lane = match stroke.input {
            StrokeInput::Key { .. } => 0.0,
            StrokeInput::Mouse { .. } | StrokeInput::Scroll { .. } => 1.0,
            StrokeInput::Move { .. } => 2.0,
            StrokeInput::PadButton { .. } | StrokeInput::PadAxis { .. } => 3.0,
        };
        let x0 = inner.left() + (stroke.at / duration) as f32 * inner.width();
        let x1 = inner.left() + ((stroke.at + stroke.hold) / duration) as f32 * inner.width();
        let top = inner.top() + lane * lane_height;
        let bar = Rect::from_min_max(pos2(x0, top + 1.0), pos2(x1.max(x0 + 2.0), top + lane_height - 1.0));
        painter.rect_filled(bar, 1.5, stroke_color(&stroke.input));
    }
}

/// Shows the editor. Returns the change the user made this frame, if any.
pub fn show(ui: &mut Ui, recording: &Recording, list_height: f32) -> Option<RecordingEdit> {
    let mut edit = None;

    ui.horizontal(|ui| {
        ui.label(RichText::new(recording.summary()).color(theme::TEXT));
        ui.weak(format!("· {}", format_ms(recording.duration_ms)));
    });
    timeline(ui, recording);

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if ui.small_button("Trim start").on_hover_text("Remove the idle time before the first input").clicked() {
            edit = Some(RecordingEdit::TrimStart);
        }
        if ui.small_button("½× time").on_hover_text("Make everything happen twice as fast").clicked() {
            edit = Some(RecordingEdit::Stretch(0.5));
        }
        if ui.small_button("2× time").on_hover_text("Make everything happen half as fast").clicked() {
            edit = Some(RecordingEdit::Stretch(2.0));
        }
    });

    if recording.strokes.is_empty() {
        ui.weak("This recording is empty.");
        return edit;
    }

    let small = FontId::proportional(11.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (label, width) in [("Input", 122.0), ("At", 64.0), ("Held", 60.0)] {
            ui.add_sized(
                [width, 12.0],
                egui::Label::new(RichText::new(label).font(small.clone()).color(theme::TEXT_FAINT))
                    .halign(egui::Align::Min),
            );
        }
    });

    egui::ScrollArea::vertical()
        .id_salt("recording-rows")
        .max_height(list_height)
        .auto_shrink([false, false])
        .show_rows(ui, ROW_HEIGHT, recording.strokes.len(), |ui, rows| {
            ui.spacing_mut().item_spacing = vec2(4.0, 2.0);
            ui.spacing_mut().interact_size.y = 20.0;
            ui.spacing_mut().button_padding = vec2(4.0, 1.0);
            ui.style_mut().override_font_id = Some(small.clone());
            for i in rows {
                let stroke = &recording.strokes[i];
                ui.push_id(i, |ui| {
                    ui.horizontal(|ui| {
                        ui.set_height(ROW_HEIGHT - 2.0);
                        ui.label(
                            RichText::new(stroke_icon(&stroke.input)).size(13.0).color(stroke_color(&stroke.input)),
                        )
                        .on_hover_text(stroke.input.kind_label());
                        ui.allocate_ui_with_layout(
                            vec2(100.0, ROW_HEIGHT - 2.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.set_width(100.0);
                                if let Some(change) = detail(ui, i, &stroke.input) {
                                    edit = Some(change);
                                }
                            },
                        );

                        let mut at = stroke.at / 1000.0;
                        let at_field =
                            DragValue::new(&mut at).speed(0.01).range(0.0..=86_400.0).fixed_decimals(3).suffix(" s");
                        if ui.add_sized([64.0, 20.0], at_field).on_hover_text("When it is pressed").changed() {
                            edit = Some(RecordingEdit::At(i, at * 1000.0));
                        }

                        if stroke.input.is_held() {
                            let mut hold = stroke.hold;
                            let hold_field = DragValue::new(&mut hold)
                                .speed(1.0)
                                .range(0.0..=3_600_000.0)
                                .max_decimals(0)
                                .suffix(" ms");
                            if ui.add_sized([60.0, 20.0], hold_field).on_hover_text("How long it is held").changed() {
                                edit = Some(RecordingEdit::Hold(i, hold));
                            }
                        } else {
                            ui.add_sized([60.0, 20.0], egui::Label::new(RichText::new("–").color(theme::TEXT_FAINT)));
                        }

                        let delete =
                            egui::Button::new(RichText::new(icon::X).size(11.0).color(theme::TEXT_FAINT)).frame(false);
                        if ui.add(delete).on_hover_text("Remove this input").clicked() {
                            edit = Some(RecordingEdit::Delete(i));
                        }
                    });
                });
            }
        });
    edit
}

/// The middle column: what the input is, editable where that makes sense.
fn detail(ui: &mut Ui, index: usize, input: &StrokeInput) -> Option<RecordingEdit> {
    let mut edit = None;
    match input {
        StrokeInput::Key { key } => {
            egui::ComboBox::from_id_salt("key").width(92.0).selected_text(key.label()).show_ui(ui, |ui| {
                for &option in Key::ALL {
                    if ui.selectable_label(option == *key, option.label()).clicked() {
                        edit = Some(RecordingEdit::Key(index, option));
                    }
                }
            });
        }
        StrokeInput::Mouse { button, x, y, end } => {
            let short = match button {
                MouseButton::Left => "L",
                MouseButton::Right => "R",
                MouseButton::Middle => "M",
                MouseButton::Back => "B",
                MouseButton::Forward => "F",
            };
            egui::ComboBox::from_id_salt("button").width(22.0).selected_text(short).show_ui(ui, |ui| {
                for &option in MouseButton::ALL {
                    if ui.selectable_label(option == *button, option.label()).clicked() {
                        edit = Some(RecordingEdit::Button(index, option));
                    }
                }
            });
            let (mut new_x, mut new_y) = (*x, *y);
            let hint = match end {
                Some(end) => format!("Pressed here, dragged to {}, {}", end.x, end.y),
                None => "Where it is clicked".to_owned(),
            };
            let changed_x = ui.add(DragValue::new(&mut new_x).speed(1.0)).on_hover_text(&hint).changed();
            let changed_y = ui.add(DragValue::new(&mut new_y).speed(1.0)).on_hover_text(&hint).changed();
            if changed_x || changed_y {
                edit = Some(RecordingEdit::Position(index, new_x, new_y));
            }
        }
        StrokeInput::Move { path } => {
            let text = match (path.first(), path.last()) {
                (Some(a), Some(b)) => format!("{},{} → {},{}", a.1, a.2, b.1, b.2),
                _ => "Movement".to_owned(),
            };
            ui.label(RichText::new(text).color(theme::TEXT_DIM))
                .on_hover_text(format!("{} pointer positions", path.len()));
        }
        StrokeInput::Scroll { dx, dy } => {
            let (mut new_dx, mut new_dy) = (*dx, *dy);
            let vertical =
                ui.add(DragValue::new(&mut new_dy).speed(0.1).prefix("↕ ")).on_hover_text("Notches up (+) or down (−)");
            let horizontal = ui
                .add(DragValue::new(&mut new_dx).speed(0.1).prefix("↔ "))
                .on_hover_text("Notches right (+) or left (−)");
            if vertical.changed() || horizontal.changed() {
                edit = Some(RecordingEdit::Scroll(index, new_dx, new_dy));
            }
        }
        StrokeInput::PadButton { button } => {
            ui.label(RichText::new(button.label()).color(theme::TEXT_DIM));
        }
        StrokeInput::PadAxis { axis, value } => {
            ui.label(RichText::new(format!("{} {value:+.2}", axis.label())).color(theme::TEXT_DIM));
        }
    }
    edit
}

#[cfg(test)]
mod tests {
    use magi_core::geom::Point;
    use magi_core::recording::Stroke;

    use super::*;

    fn sample() -> Recording {
        Recording {
            duration_ms: 500.0,
            screen: None,
            strokes: vec![
                Stroke { at: 100.0, hold: 50.0, input: StrokeInput::Key { key: Key::KeyA } },
                Stroke {
                    at: 200.0,
                    hold: 100.0,
                    input: StrokeInput::Mouse {
                        button: MouseButton::Left,
                        x: 10,
                        y: 10,
                        end: Some(Point::new(30, 40)),
                    },
                },
            ],
        }
    }

    #[test]
    fn edits_change_exactly_what_they_name() {
        let mut recording = sample();
        RecordingEdit::Hold(0, 900.0).apply(&mut recording);
        assert_eq!(recording.strokes[0].hold, 900.0);
        assert_eq!(recording.duration_ms, 1000.0, "the recording grows to fit its longest input");

        RecordingEdit::Position(1, 110, 10).apply(&mut recording);
        assert_eq!(
            recording.strokes[1].input,
            StrokeInput::Mouse { button: MouseButton::Left, x: 110, y: 10, end: Some(Point::new(130, 40)) }
        );

        RecordingEdit::Delete(0).apply(&mut recording);
        assert_eq!(recording.strokes.len(), 1);
        RecordingEdit::Delete(7).apply(&mut recording); // out of range: ignored
        assert_eq!(recording.strokes.len(), 1);
    }

    #[test]
    fn stretching_scales_every_time() {
        let mut recording = sample();
        RecordingEdit::Stretch(0.5).apply(&mut recording);
        assert_eq!((recording.strokes[0].at, recording.strokes[0].hold), (50.0, 25.0));
        assert_eq!(recording.duration_ms, 250.0);
    }
}

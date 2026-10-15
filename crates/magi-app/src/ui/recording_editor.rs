//! The editable view of a recording: a timeline with trim handles and a
//! spreadsheet with one row per recorded input – what it was, where, when it
//! was pressed and how long it was held. Every kind of input can be edited.
//!
//! The editor only *reads* the recording and reports at most one
//! [`RecordingEdit`] per frame. That way an untouched recording is never
//! copied, however often it is drawn.

use egui::{Color32, DragValue, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular as icon;
use magi_core::geom::Point;
use magi_core::input::{GamepadAxis, GamepadButton, Key, MouseButton};
use magi_core::recording::{LookPoint, PathPoint, Recording, Stroke as RecStroke, StrokeInput};
use magi_core::text::format_ms;

use super::widgets;
use crate::theme;

const ROW_HEIGHT: f32 = 24.0;
const TIMELINE_HEIGHT: f32 = 46.0;
/// Width of the area around a trim handle that grabs it.
const HANDLE_GRAB: f32 = 10.0;

/// The kinds a row can be switched between.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    Key,
    Click,
    Scroll,
    PadButton,
}

impl RowKind {
    const ALL: [RowKind; 4] = [RowKind::Key, RowKind::Click, RowKind::Scroll, RowKind::PadButton];

    fn label(self) -> &'static str {
        match self {
            RowKind::Key => "Key",
            RowKind::Click => "Click",
            RowKind::Scroll => "Scroll",
            RowKind::PadButton => "Pad button",
        }
    }

    fn input(self) -> StrokeInput {
        match self {
            RowKind::Key => StrokeInput::Key { key: Key::Space },
            RowKind::Click => StrokeInput::Mouse { button: MouseButton::Left, x: 0, y: 0, end: None, here: true },
            RowKind::Scroll => StrokeInput::Scroll { dx: 0, dy: -1 },
            RowKind::PadButton => StrokeInput::PadButton { button: GamepadButton::South },
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum RecordingEdit {
    At(usize, f64),
    Hold(usize, f64),
    Kind(usize, RowKind),
    Key(usize, Key),
    Button(usize, MouseButton),
    /// Where a click is pressed; a drag's end moves along.
    Position(usize, i32, i32),
    /// Where a drag is released (`None`: a click, not a drag).
    End(usize, Option<Point>),
    /// Click wherever the pointer is instead of at the stored position.
    Here(usize, bool),
    Scroll(usize, i32, i32),
    PadButton(usize, GamepadButton),
    PadAxis(usize, GamepadAxis, f32),
    /// Where a movement starts and ends; the path in between follows.
    MoveEnds(usize, Point, Point),
    /// The total relative motion of a look.
    LookTotal(usize, i32, i32),
    /// Movement between positions ⇄ relative motion.
    SwapMovement(usize),
    Delete(usize),
    Duplicate(usize),
    /// Add an input after this row (or at the start).
    Insert(Option<usize>, RowKind),
    /// Multiply every time by a factor (0.5 = twice as fast).
    Stretch(f64),
    /// Keep only what happens between two times, in milliseconds.
    Cut(f64, f64),
    /// Detect movement made in games that lock the pointer.
    MakeSmart,
}

/// What the editor reports for one frame.
#[derive(Default)]
pub struct EditorResponse {
    pub edit: Option<RecordingEdit>,
    /// The user asked for the editor in its own window.
    pub pop_out: bool,
}

fn rounded_split(total: i32, weights: &[f64]) -> Vec<i32> {
    // Distributes `total` in proportion to `weights`, with the parts summing exactly.
    let sum: f64 = weights.iter().sum();
    let mut out = Vec::with_capacity(weights.len());
    let (mut acc, mut given) = (0.0, 0);
    for w in weights {
        acc += if sum.abs() > f64::EPSILON { w / sum } else { 1.0 / weights.len() as f64 } * f64::from(total);
        let target = acc.round() as i32;
        out.push(target - given);
        given = target;
    }
    out
}

impl RecordingEdit {
    pub fn apply(self, recording: &mut Recording) {
        match self {
            RecordingEdit::At(i, at) => {
                if let Some(s) = recording.strokes.get_mut(i) {
                    s.at = at.max(0.0);
                }
            }
            RecordingEdit::Hold(i, hold) => {
                if let Some(s) = recording.strokes.get_mut(i) {
                    let old = s.hold.max(1e-9);
                    s.hold = hold.max(0.0);
                    // A movement keeps its shape and plays faster or slower.
                    let factor = (s.hold / old) as f32;
                    match &mut s.input {
                        StrokeInput::Move { path } => path.iter_mut().for_each(|p| p.0 *= factor),
                        StrokeInput::Look { path } => path.iter_mut().for_each(|p| p.0 *= factor),
                        _ => {}
                    }
                }
            }
            RecordingEdit::Kind(i, kind) => {
                if let Some(s) = recording.strokes.get_mut(i) {
                    s.input = kind.input();
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
                    if let Some(end) = end {
                        end.x += new_x - *x;
                        end.y += new_y - *y;
                    }
                    (*x, *y) = (new_x, new_y);
                }
            }
            RecordingEdit::End(i, new_end) => {
                if let Some(StrokeInput::Mouse { end, .. }) = recording.strokes.get_mut(i).map(|s| &mut s.input) {
                    *end = new_end;
                }
            }
            RecordingEdit::Here(i, new_here) => {
                if let Some(StrokeInput::Mouse { here, .. }) = recording.strokes.get_mut(i).map(|s| &mut s.input) {
                    *here = new_here;
                }
            }
            RecordingEdit::Scroll(i, new_dx, new_dy) => {
                if let Some(StrokeInput::Scroll { dx, dy }) = recording.strokes.get_mut(i).map(|s| &mut s.input) {
                    (*dx, *dy) = (new_dx, new_dy);
                }
            }
            RecordingEdit::PadButton(i, new_button) => {
                if let Some(StrokeInput::PadButton { button }) = recording.strokes.get_mut(i).map(|s| &mut s.input) {
                    *button = new_button;
                }
            }
            RecordingEdit::PadAxis(i, new_axis, new_value) => {
                if let Some(StrokeInput::PadAxis { axis, value }) = recording.strokes.get_mut(i).map(|s| &mut s.input) {
                    (*axis, *value) = (new_axis, new_value.clamp(-1.0, 1.0));
                }
            }
            RecordingEdit::MoveEnds(i, start, end) => {
                if let Some(StrokeInput::Move { path }) = recording.strokes.get_mut(i).map(|s| &mut s.input)
                    && let (Some(first), Some(last)) = (path.first().copied(), path.last().copied())
                {
                    let shift = (start.x - first.1, start.y - first.2);
                    let stretch = (end.x - (last.1 + shift.0), end.y - (last.2 + shift.1));
                    let n = (path.len() - 1).max(1) as f32;
                    for (k, point) in path.iter_mut().enumerate() {
                        let t = k as f32 / n;
                        point.1 += shift.0 + (stretch.0 as f32 * t).round() as i32;
                        point.2 += shift.1 + (stretch.1 as f32 * t).round() as i32;
                    }
                }
            }
            RecordingEdit::LookTotal(i, total_x, total_y) => {
                if let Some(StrokeInput::Look { path }) = recording.strokes.get_mut(i).map(|s| &mut s.input) {
                    let xs = rounded_split(total_x, &path.iter().map(|p| f64::from(p.1)).collect::<Vec<_>>());
                    let ys = rounded_split(total_y, &path.iter().map(|p| f64::from(p.2)).collect::<Vec<_>>());
                    for ((point, x), y) in path.iter_mut().zip(xs).zip(ys) {
                        (point.1, point.2) = (x, y);
                    }
                }
            }
            RecordingEdit::SwapMovement(i) => {
                let center = recording.screen.map_or((960, 540), |(w, h)| ((w / 2) as i32, (h / 2) as i32));
                if let Some(s) = recording.strokes.get_mut(i) {
                    s.input = match &s.input {
                        StrokeInput::Move { path } => StrokeInput::Look {
                            path: path
                                .windows(2)
                                .map(|w| LookPoint(w[1].0, w[1].1 - w[0].1, w[1].2 - w[0].2))
                                .collect(),
                        },
                        StrokeInput::Look { path } => {
                            let (mut x, mut y) = center;
                            let mut points = vec![PathPoint(0.0, x, y)];
                            for p in path {
                                (x, y) = (x + p.1, y + p.2);
                                points.push(PathPoint(p.0, x, y));
                            }
                            StrokeInput::Move { path: points }
                        }
                        other => other.clone(),
                    };
                }
            }
            RecordingEdit::Delete(i) => {
                if i < recording.strokes.len() {
                    recording.strokes.remove(i);
                }
            }
            RecordingEdit::Duplicate(i) => {
                if let Some(original) = recording.strokes.get(i).cloned() {
                    let mut copy = original;
                    copy.at += copy.hold.max(10.0) + 10.0;
                    recording.strokes.insert(i + 1, copy);
                }
            }
            RecordingEdit::Insert(after, kind) => {
                let at = after.and_then(|i| recording.strokes.get(i)).map_or(0.0, |s| s.at + s.hold + 50.0);
                let index = after.map_or(0, |i| (i + 1).min(recording.strokes.len()));
                let hold = if kind == RowKind::Scroll { 0.0 } else { 50.0 };
                recording.strokes.insert(index, RecStroke { at, hold, input: kind.input() });
            }
            RecordingEdit::Stretch(factor) => {
                for stroke in &mut recording.strokes {
                    stroke.at *= factor;
                    stroke.hold *= factor;
                    match &mut stroke.input {
                        StrokeInput::Move { path } => {
                            path.iter_mut().for_each(|p| p.0 = (f64::from(p.0) * factor) as f32)
                        }
                        StrokeInput::Look { path } => {
                            path.iter_mut().for_each(|p| p.0 = (f64::from(p.0) * factor) as f32)
                        }
                        _ => {}
                    }
                }
                recording.duration_ms *= factor;
            }
            RecordingEdit::Cut(start, end) => recording.cut(start, end),
            RecordingEdit::MakeSmart => {
                recording.make_smart();
            }
        }
        // Rows keep their order while being edited (so a row does not jump
        // away under the pointer); playback orders events by time regardless.
        recording.duration_ms = recording.duration_ms.max(recording.end_time());
    }
}

fn stroke_color(input: &StrokeInput) -> Color32 {
    let p = theme::palette();
    let data = |ty| theme::data_color(ty);
    use magi_core::nodes::DataType;
    match input {
        StrokeInput::Key { .. } => data(DataType::Number),
        StrokeInput::Mouse { .. } => p.success,
        StrokeInput::Move { .. } => p.text_faint,
        StrokeInput::Look { .. } => data(DataType::Point),
        StrokeInput::Scroll { .. } => p.warning,
        StrokeInput::PadButton { .. } | StrokeInput::PadAxis { .. } => data(DataType::Text),
    }
}

fn stroke_icon(input: &StrokeInput) -> &'static str {
    match input {
        StrokeInput::Key { .. } => icon::KEYBOARD,
        StrokeInput::Mouse { .. } => icon::CURSOR_CLICK,
        StrokeInput::Move { .. } => icon::PATH,
        StrokeInput::Look { .. } => icon::CROSSHAIR,
        StrokeInput::Scroll { .. } => icon::ARROWS_VERTICAL,
        StrokeInput::PadButton { .. } | StrokeInput::PadAxis { .. } => icon::GAME_CONTROLLER,
    }
}

fn lane(input: &StrokeInput) -> f32 {
    match input {
        StrokeInput::Key { .. } => 0.0,
        StrokeInput::Mouse { .. } | StrokeInput::Scroll { .. } => 1.0,
        StrokeInput::Move { .. } | StrokeInput::Look { .. } => 2.0,
        StrokeInput::PadButton { .. } | StrokeInput::PadAxis { .. } => 3.0,
    }
}

/// Per-editor state kept between frames: the trim selection and the selected row.
#[derive(Clone, Copy, Default)]
struct EditorState {
    /// Trim handles in milliseconds; `None` = the whole recording.
    trim: Option<(f64, f64)>,
    selected: Option<usize>,
    /// Which handle is being dragged: 0 = start, 1 = end.
    dragging: Option<u8>,
}

/// The timeline: every input as a bar from press to release, one lane per
/// kind, with two handles that select the part to keep.
fn timeline(ui: &mut Ui, recording: &Recording, state: &mut EditorState) -> Option<RecordingEdit> {
    let duration = recording.duration_ms.max(recording.end_time()).max(1.0);
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), TIMELINE_HEIGHT), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 5.0, theme::bg_input());
    let inner = rect.shrink2(vec2(8.0, 6.0));
    let x_of = |ms: f64| inner.left() + (ms / duration) as f32 * inner.width();
    let ms_of = |x: f32| (f64::from((x - inner.left()) / inner.width()) * duration).clamp(0.0, duration);
    let lane_height = (inner.height() - 10.0) / 4.0;

    for (i, stroke) in recording.strokes.iter().enumerate() {
        let x0 = x_of(stroke.at);
        let x1 = x_of(stroke.at + stroke.hold).max(x0 + 2.0);
        let top = inner.top() + lane(&stroke.input) * lane_height;
        let bar = Rect::from_min_max(pos2(x0, top + 1.0), pos2(x1, top + lane_height - 1.0));
        let mut color = stroke_color(&stroke.input);
        if state.selected == Some(i) {
            color = theme::accent_bright();
        }
        painter.rect_filled(bar, 1.5, color);
    }

    // Time ticks along the bottom.
    let tick_y = inner.bottom() - 4.0;
    for k in 0..=4 {
        let ms = duration * f64::from(k) / 4.0;
        let x = x_of(ms);
        painter.vline(x, (tick_y - 3.0)..=tick_y, Stroke::new(1.0, theme::stroke_strong()));
    }

    // Trim handles.
    let (start, end) = state.trim.unwrap_or((0.0, duration));
    let (start, end) = (start.clamp(0.0, duration), end.clamp(0.0, duration));
    let pointer = response.interact_pointer_pos().or(response.hover_pos());
    if response.drag_started()
        && let Some(p) = pointer
    {
        let near_start = (p.x - x_of(start)).abs();
        let near_end = (p.x - x_of(end)).abs();
        state.dragging = if near_start <= HANDLE_GRAB && near_start <= near_end {
            Some(0)
        } else if near_end <= HANDLE_GRAB {
            Some(1)
        } else {
            None
        };
    }
    if let (Some(handle), Some(p)) = (state.dragging, pointer)
        && response.dragged()
    {
        let ms = ms_of(p.x);
        state.trim = Some(if handle == 0 { (ms.min(end - 1.0).max(0.0), end) } else { (start, ms.max(start + 1.0)) });
    }
    if response.drag_stopped() {
        state.dragging = None;
    }
    let (start, end) = state.trim.unwrap_or((0.0, duration));
    let shade = Color32::from_black_alpha(if theme::palette().dark { 140 } else { 60 });
    painter.rect_filled(Rect::from_min_max(rect.min, pos2(x_of(start), rect.bottom())), 5.0, shade);
    painter.rect_filled(Rect::from_min_max(pos2(x_of(end), rect.top()), rect.max), 5.0, shade);
    for (i, ms) in [start, end].into_iter().enumerate() {
        let x = x_of(ms);
        let hot = state.dragging == Some(i as u8) || pointer.is_some_and(|p| (p.x - x).abs() <= HANDLE_GRAB);
        let color = if hot { theme::accent_bright() } else { theme::accent() };
        painter.vline(x, rect.y_range(), Stroke::new(2.0, color));
        let grip = Rect::from_center_size(pos2(x, rect.center().y), vec2(8.0, 18.0));
        painter.rect_filled(grip, 3.0, color);
        if hot {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
    }
    response.on_hover_text("Drag the handles at either end to choose the part to keep, then press Trim.");

    // Trim controls.
    let mut edit = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let changed = state.trim.is_some_and(|(s, e)| s > 0.5 || e < duration - 0.5);
        let label = format!("{} – {}", format_ms(start), format_ms(end));
        ui.label(RichText::new(label).size(11.0).color(theme::text_dim()));
        if ui
            .add_enabled(changed, egui::Button::new(format!("{}  Trim", icon::SCISSORS)).small())
            .on_hover_text("Keep only the part between the handles")
            .clicked()
        {
            edit = Some(RecordingEdit::Cut(start, end));
            state.trim = None;
        }
        if changed && ui.small_button("Reset").clicked() {
            state.trim = None;
        }
    });
    edit
}

/// Shows the editor. `table_height` limits the table; `wide` shows every column.
pub fn show(ui: &mut Ui, recording: &Recording, table_height: f32, wide: bool) -> EditorResponse {
    let mut out = EditorResponse::default();
    let state_id = ui.id().with("recording-editor-state");
    let mut state: EditorState = ui.data(|d| d.get_temp(state_id)).unwrap_or_default();

    ui.horizontal(|ui| {
        ui.label(RichText::new(recording.summary()).color(theme::text()));
        ui.weak(format!("· {}", format_ms(recording.duration_ms)));
        if !wide {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::icon_button(ui, icon::ARROW_SQUARE_OUT, "Open in its own window").clicked() {
                    out.pop_out = true;
                }
            });
        }
    });
    if let Some(edit) = timeline(ui, recording, &mut state) {
        out.edit = Some(edit);
    }

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if ui.small_button("½× time").on_hover_text("Make everything happen twice as fast").clicked() {
            out.edit = Some(RecordingEdit::Stretch(0.5));
        }
        if ui.small_button("2× time").on_hover_text("Make everything happen half as fast").clicked() {
            out.edit = Some(RecordingEdit::Stretch(2.0));
        }
        if ui
            .small_button(format!("{}  Smart", icon::CROSSHAIR))
            .on_hover_text(
                "Convert to a smart recording: movement made in a game that keeps re-centring the pointer becomes \
                 relative motion, so the camera turns when it is played back.",
            )
            .clicked()
        {
            out.edit = Some(RecordingEdit::MakeSmart);
        }
        ui.menu_button(format!("{}  Add", icon::PLUS), |ui| {
            for kind in RowKind::ALL {
                if ui.button(kind.label()).clicked() {
                    out.edit = Some(RecordingEdit::Insert(state.selected, kind));
                    ui.close();
                }
            }
        })
        .response
        .on_hover_text("Add an input after the selected row");
    });

    if recording.strokes.is_empty() {
        ui.weak("This recording is empty.");
        ui.data_mut(|d| d.insert_temp(state_id, state));
        return out;
    }

    // Rows sit close together, and the table is never taller than the space
    // left, so the last row is not cut in half.
    ui.spacing_mut().item_spacing = vec2(4.0, 2.0);
    let table_height = table_height.min(ui.available_height() - 30.0).max(ROW_HEIGHT * 2.0);
    let mut table = TableBuilder::new(ui)
        .id_salt("recording-table")
        .striped(true)
        .resizable(wide)
        .sense(Sense::click())
        .max_scroll_height(table_height)
        .auto_shrink([false, true])
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center));
    if wide {
        table = table.column(Column::exact(34.0)).column(Column::initial(100.0).at_least(70.0));
    } else {
        table = table.column(Column::exact(18.0));
    }
    table = table
        .column(Column::remainder().at_least(110.0).clip(true))
        .column(Column::exact(if wide { 76.0 } else { 62.0 }))
        .column(Column::exact(if wide { 70.0 } else { 56.0 }))
        .column(Column::exact(if wide { 46.0 } else { 20.0 }));

    let small = egui::FontId::proportional(11.0);
    table
        .header(18.0, |mut header| {
            let title = |header: &mut egui_extras::TableRow<'_, '_>, text: &str| {
                header.col(|ui| {
                    ui.label(RichText::new(text).font(small.clone()).color(theme::text_faint()));
                });
            };
            if wide {
                title(&mut header, "#");
                title(&mut header, "Type");
            } else {
                title(&mut header, "");
            }
            title(&mut header, "Input");
            title(&mut header, "At");
            title(&mut header, "Held");
            title(&mut header, "");
        })
        .body(|body| {
            body.rows(ROW_HEIGHT, recording.strokes.len(), |mut row| {
                let i = row.index();
                let stroke = &recording.strokes[i];
                row.set_selected(state.selected == Some(i));
                row.col(|ui| {
                    if wide {
                        ui.label(RichText::new(format!("{}", i + 1)).size(11.0).color(theme::text_faint()));
                    } else {
                        ui.label(RichText::new(stroke_icon(&stroke.input)).color(stroke_color(&stroke.input)))
                            .on_hover_text(stroke.input.kind_label());
                    }
                });
                if wide {
                    row.col(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        ui.label(RichText::new(stroke_icon(&stroke.input)).color(stroke_color(&stroke.input)));
                        kind_cell(ui, i, &stroke.input, &mut out.edit);
                    });
                }
                row.col(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    ui.spacing_mut().interact_size.y = 20.0;
                    ui.style_mut().override_font_id = Some(egui::FontId::proportional(12.0));
                    if let Some(change) = detail(ui, i, &stroke.input, wide) {
                        out.edit = Some(change);
                    }
                });
                row.col(|ui| {
                    let mut at = stroke.at / 1000.0;
                    let field =
                        DragValue::new(&mut at).speed(0.01).range(0.0..=31_536_000.0).fixed_decimals(3).suffix(" s");
                    if ui.add(field).on_hover_text("When it starts").changed() {
                        out.edit = Some(RecordingEdit::At(i, at * 1000.0));
                    }
                });
                row.col(|ui| {
                    if stroke.input.is_held() {
                        let mut hold = stroke.hold;
                        let field =
                            DragValue::new(&mut hold).speed(1.0).range(0.0..=3_600_000.0).max_decimals(0).suffix(" ms");
                        if ui.add(field).on_hover_text("How long it is held").changed() {
                            out.edit = Some(RecordingEdit::Hold(i, hold));
                        }
                    } else {
                        ui.label(RichText::new("–").color(theme::text_faint()));
                    }
                });
                row.col(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    if wide
                        && ui
                            .add(
                                egui::Button::new(RichText::new(icon::COPY).size(12.0).color(theme::text_faint()))
                                    .frame(false),
                            )
                            .on_hover_text("Duplicate")
                            .clicked()
                    {
                        out.edit = Some(RecordingEdit::Duplicate(i));
                    }
                    let delete =
                        egui::Button::new(RichText::new(icon::X).size(12.0).color(theme::text_faint())).frame(false);
                    if ui.add(delete).on_hover_text("Remove this input").clicked() {
                        out.edit = Some(RecordingEdit::Delete(i));
                    }
                });
                if row.response().clicked() {
                    state.selected = if state.selected == Some(i) { None } else { Some(i) };
                }
            });
        });

    if let Some(RecordingEdit::Delete(i)) = &out.edit {
        state.selected = state.selected.filter(|s| s < i);
    }
    ui.data_mut(|d| d.insert_temp(state_id, state));
    out
}

/// The type column: switchable for simple inputs, Move ⇄ Look for movement.
fn kind_cell(ui: &mut Ui, index: usize, input: &StrokeInput, edit: &mut Option<RecordingEdit>) {
    let current = match input {
        StrokeInput::Key { .. } => Some(RowKind::Key),
        StrokeInput::Mouse { .. } => Some(RowKind::Click),
        StrokeInput::Scroll { .. } => Some(RowKind::Scroll),
        StrokeInput::PadButton { .. } => Some(RowKind::PadButton),
        _ => None,
    };
    match current {
        Some(kind) => {
            egui::ComboBox::from_id_salt(("kind", index)).width(66.0).selected_text(input.kind_label()).show_ui(
                ui,
                |ui| {
                    for option in RowKind::ALL {
                        if ui.selectable_label(option == kind, option.label()).clicked() && option != kind {
                            *edit = Some(RecordingEdit::Kind(index, option));
                        }
                    }
                },
            );
        }
        None => {
            ui.label(input.kind_label());
        }
    }
}

fn mouse_short(button: MouseButton) -> &'static str {
    match button {
        MouseButton::Left => "L",
        MouseButton::Right => "R",
        MouseButton::Middle => "M",
        MouseButton::Back => "B",
        MouseButton::Forward => "F",
    }
}

/// The input column: what the input is, editable.
fn detail(ui: &mut Ui, index: usize, input: &StrokeInput, wide: bool) -> Option<RecordingEdit> {
    let mut edit = None;
    let coord = |ui: &mut Ui, value: &mut i32, hint: &str| {
        ui.add(DragValue::new(value).speed(1.0)).on_hover_text(hint).changed()
    };
    match input {
        StrokeInput::Key { key } => {
            let mut key = *key;
            if widgets::key_picker(ui, ("key", index), &mut key) {
                edit = Some(RecordingEdit::Key(index, key));
            }
        }
        StrokeInput::Mouse { button, x, y, end, here } => {
            let shown = if wide { button.label() } else { mouse_short(*button) };
            egui::ComboBox::from_id_salt(("button", index))
                .width(if wide { 70.0 } else { 22.0 })
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    for &option in MouseButton::ALL {
                        if ui.selectable_label(option == *button, option.label()).clicked() {
                            edit = Some(RecordingEdit::Button(index, option));
                        }
                    }
                });
            let at_pointer =
                RichText::new(icon::CURSOR).color(if *here { theme::accent_bright() } else { theme::text_faint() });
            if ui
                .add(egui::Button::new(at_pointer).frame(false).selected(*here))
                .on_hover_text("Click wherever the pointer is, instead of at the position")
                .clicked()
            {
                edit = Some(RecordingEdit::Here(index, !*here));
            }
            if !*here {
                let (mut nx, mut ny) = (*x, *y);
                if coord(ui, &mut nx, "Where it is pressed") | coord(ui, &mut ny, "Where it is pressed") {
                    edit = Some(RecordingEdit::Position(index, nx, ny));
                }
                if wide {
                    match end {
                        Some(end) => {
                            ui.label(RichText::new("→").color(theme::text_faint()));
                            let (mut ex, mut ey) = (end.x, end.y);
                            if coord(ui, &mut ex, "Where the drag ends") | coord(ui, &mut ey, "Where the drag ends") {
                                edit = Some(RecordingEdit::End(index, Some(Point::new(ex, ey))));
                            }
                            if ui.small_button(icon::X).on_hover_text("Make it a click, not a drag").clicked() {
                                edit = Some(RecordingEdit::End(index, None));
                            }
                        }
                        None => {
                            if ui.small_button("Drag…").on_hover_text("Make it a drag").clicked() {
                                edit = Some(RecordingEdit::End(index, Some(Point::new(*x + 50, *y))));
                            }
                        }
                    }
                }
            }
        }
        StrokeInput::Move { path } => {
            if let (Some(a), Some(b)) = (path.first(), path.last()) {
                let (mut ax, mut ay, mut bx, mut by) = (a.1, a.2, b.1, b.2);
                let mut changed =
                    coord(ui, &mut ax, "Where the movement starts") | coord(ui, &mut ay, "Where the movement starts");
                ui.label(RichText::new("→").color(theme::text_faint()));
                changed |=
                    coord(ui, &mut bx, "Where the movement ends") | coord(ui, &mut by, "Where the movement ends");
                if changed {
                    edit = Some(RecordingEdit::MoveEnds(index, Point::new(ax, ay), Point::new(bx, by)));
                }
            }
            if wide && ui.small_button("→ Look").on_hover_text("Replay as relative motion (for game cameras)").clicked()
            {
                edit = Some(RecordingEdit::SwapMovement(index));
            }
        }
        StrokeInput::Look { path } => {
            let (mut tx, mut ty) = path.iter().fold((0, 0), |(x, y), p| (x + p.1, y + p.2));
            ui.label(RichText::new("Δ").color(theme::text_faint()));
            let changed = coord(ui, &mut tx, "Total sideways motion") | coord(ui, &mut ty, "Total up/down motion");
            if changed {
                edit = Some(RecordingEdit::LookTotal(index, tx, ty));
            }
            if wide && ui.small_button("→ Move").on_hover_text("Replay as movement between screen positions").clicked()
            {
                edit = Some(RecordingEdit::SwapMovement(index));
            }
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
            egui::ComboBox::from_id_salt(("pad", index)).width(110.0).selected_text(button.label()).show_ui(ui, |ui| {
                for &option in GamepadButton::ALL {
                    if ui.selectable_label(option == *button, option.label()).clicked() {
                        edit = Some(RecordingEdit::PadButton(index, option));
                    }
                }
            });
        }
        StrokeInput::PadAxis { axis, value } => {
            let mut new_axis = *axis;
            egui::ComboBox::from_id_salt(("axis", index)).width(100.0).selected_text(axis.label()).show_ui(ui, |ui| {
                for &option in GamepadAxis::ALL {
                    ui.selectable_value(&mut new_axis, option, option.label());
                }
            });
            let mut new_value = *value;
            let field = ui.add(DragValue::new(&mut new_value).speed(0.01).range(-1.0..=1.0).fixed_decimals(2));
            if new_axis != *axis || field.changed() {
                edit = Some(RecordingEdit::PadAxis(index, new_axis, new_value));
            }
        }
    }
    edit
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Recording {
        Recording {
            duration_ms: 500.0,
            screen: None,
            strokes: vec![
                RecStroke { at: 100.0, hold: 50.0, input: StrokeInput::Key { key: Key::KeyA } },
                RecStroke {
                    at: 200.0,
                    hold: 100.0,
                    input: StrokeInput::Mouse {
                        button: MouseButton::Left,
                        x: 10,
                        y: 10,
                        end: Some(Point::new(30, 40)),
                        here: false,
                    },
                },
                RecStroke {
                    at: 320.0,
                    hold: 40.0,
                    input: StrokeInput::Move {
                        path: vec![PathPoint(0.0, 0, 0), PathPoint(20.0, 5, 5), PathPoint(40.0, 10, 10)],
                    },
                },
                RecStroke {
                    at: 400.0,
                    hold: 20.0,
                    input: StrokeInput::Look {
                        path: vec![LookPoint(0.0, 3, 0), LookPoint(10.0, 3, 1), LookPoint(20.0, 4, 1)],
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
            StrokeInput::Mouse {
                button: MouseButton::Left,
                x: 110,
                y: 10,
                end: Some(Point::new(130, 40)),
                here: false
            }
        );
        RecordingEdit::End(1, None).apply(&mut recording);
        RecordingEdit::Here(1, true).apply(&mut recording);
        assert!(matches!(recording.strokes[1].input, StrokeInput::Mouse { end: None, here: true, .. }));

        RecordingEdit::Delete(0).apply(&mut recording);
        assert_eq!(recording.strokes.len(), 3);
        RecordingEdit::Delete(7).apply(&mut recording); // out of range: ignored
        assert_eq!(recording.strokes.len(), 3);
    }

    #[test]
    fn movement_ends_and_look_totals_reshape_the_path() {
        let mut recording = sample();
        RecordingEdit::MoveEnds(2, Point::new(100, 100), Point::new(120, 100)).apply(&mut recording);
        let StrokeInput::Move { path } = &recording.strokes[2].input else { panic!() };
        assert_eq!((path[0].1, path[0].2), (100, 100));
        assert_eq!((path[2].1, path[2].2), (120, 100));
        assert_eq!((path[1].1, path[1].2), (110, 100), "the middle follows");

        RecordingEdit::LookTotal(3, 20, -4).apply(&mut recording);
        let StrokeInput::Look { path } = &recording.strokes[3].input else { panic!() };
        assert_eq!(path.iter().map(|p| p.1).sum::<i32>(), 20);
        assert_eq!(path.iter().map(|p| p.2).sum::<i32>(), -4);
    }

    #[test]
    fn movement_converts_between_positions_and_relative_motion() {
        let mut recording = sample();
        RecordingEdit::SwapMovement(2).apply(&mut recording);
        let StrokeInput::Look { path } = &recording.strokes[2].input else { panic!("now a look") };
        assert_eq!(path.iter().map(|p| (p.1, p.2)).collect::<Vec<_>>(), [(5, 5), (5, 5)]);
        RecordingEdit::SwapMovement(2).apply(&mut recording);
        let StrokeInput::Move { path } = &recording.strokes[2].input else { panic!("a move again") };
        assert_eq!(path.len(), 3);
        assert_eq!((path[2].1 - path[0].1, path[2].2 - path[0].2), (10, 10), "the same motion");
    }

    #[test]
    fn rows_can_change_kind_be_added_and_duplicated() {
        let mut recording = sample();
        RecordingEdit::Kind(0, RowKind::Scroll).apply(&mut recording);
        assert!(matches!(recording.strokes[0].input, StrokeInput::Scroll { .. }));
        RecordingEdit::Insert(Some(0), RowKind::Key).apply(&mut recording);
        assert_eq!(recording.strokes.len(), 5);
        assert!(matches!(recording.strokes[1].input, StrokeInput::Key { .. }));
        assert!(recording.strokes[1].at > recording.strokes[0].at);
        RecordingEdit::Duplicate(1).apply(&mut recording);
        assert_eq!(recording.strokes.len(), 6);
        assert_eq!(recording.strokes[2].input, recording.strokes[1].input);
    }

    #[test]
    fn trimming_keeps_the_selected_part() {
        let mut recording = sample();
        RecordingEdit::Cut(120.0, 330.0).apply(&mut recording);
        assert_eq!(recording.strokes[0].at, 0.0, "the key, held across the cut, starts at the new beginning");
        assert_eq!(recording.strokes[0].hold, 30.0);
        assert_eq!(recording.strokes[1].at, 80.0);
        assert!(recording.duration_ms <= 210.0 + 1e-9);
        assert!(recording.strokes.iter().all(|s| !matches!(s.input, StrokeInput::Look { .. })));
    }

    #[test]
    fn stretching_scales_every_time() {
        let mut recording = sample();
        RecordingEdit::Stretch(0.5).apply(&mut recording);
        assert_eq!((recording.strokes[0].at, recording.strokes[0].hold), (50.0, 25.0));
        assert_eq!(recording.duration_ms, 250.0);
    }

    #[test]
    fn splits_add_up() {
        assert_eq!(rounded_split(10, &[1.0, 1.0, 1.0]).iter().sum::<i32>(), 10);
        assert_eq!(rounded_split(-7, &[0.0, 0.0]).iter().sum::<i32>(), -7);
    }
}

//! Editing a recording.
//!
//! Two views share one set of edits:
//!
//! * [`controls`] – the timeline with its two handles, Trim and Cut, the
//!   speed and Smart buttons, and a button that opens the full editor. Shown
//!   in a block's settings, where there is no room for more.
//! * [`full`] – the same controls above a spreadsheet with one row per
//!   recorded input: what it was, where, when it was pressed and how long it
//!   was held. Rows are dragged by their grip to reorder them. Shown in the
//!   Recording Editor window.
//!
//! The views only *read* the recording and report at most one
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
    KeyDown,
    KeyUp,
    Click,
    ButtonDown,
    ButtonUp,
    Scroll,
    PadButton,
    Display,
}

impl RowKind {
    const ALL: [RowKind; 9] = [
        RowKind::Key,
        RowKind::KeyDown,
        RowKind::KeyUp,
        RowKind::Click,
        RowKind::ButtonDown,
        RowKind::ButtonUp,
        RowKind::Scroll,
        RowKind::PadButton,
        RowKind::Display,
    ];

    fn label(self) -> &'static str {
        match self {
            RowKind::Key => "Key (held)",
            RowKind::KeyDown => "Key down",
            RowKind::KeyUp => "Key up",
            RowKind::Click => "Click",
            RowKind::ButtonDown => "Button down",
            RowKind::ButtonUp => "Button up",
            RowKind::Scroll => "Scroll",
            RowKind::PadButton => "Pad button",
            RowKind::Display => "Display change",
        }
    }

    fn of(input: &StrokeInput) -> Option<RowKind> {
        Some(match input {
            StrokeInput::Key { .. } => RowKind::Key,
            StrokeInput::KeyDown { .. } => RowKind::KeyDown,
            StrokeInput::KeyUp { .. } => RowKind::KeyUp,
            StrokeInput::Mouse { .. } => RowKind::Click,
            StrokeInput::MouseDown { .. } => RowKind::ButtonDown,
            StrokeInput::MouseUp { .. } => RowKind::ButtonUp,
            StrokeInput::Scroll { .. } => RowKind::Scroll,
            StrokeInput::PadButton { .. } => RowKind::PadButton,
            StrokeInput::Display { .. } => RowKind::Display,
            StrokeInput::Move { .. } | StrokeInput::Look { .. } | StrokeInput::PadAxis { .. } => return None,
        })
    }

    fn input(self) -> StrokeInput {
        match self {
            RowKind::Key => StrokeInput::Key { key: Key::Space },
            RowKind::KeyDown => StrokeInput::KeyDown { key: Key::Space },
            RowKind::KeyUp => StrokeInput::KeyUp { key: Key::Space },
            RowKind::Click => StrokeInput::Mouse { button: MouseButton::Left, x: 0, y: 0, end: None, here: true },
            RowKind::ButtonDown => StrokeInput::MouseDown { button: MouseButton::Left, x: 0, y: 0, here: true },
            RowKind::ButtonUp => StrokeInput::MouseUp { button: MouseButton::Left, x: 0, y: 0, here: true },
            RowKind::Scroll => StrokeInput::Scroll { dx: 0, dy: -1 },
            RowKind::PadButton => StrokeInput::PadButton { button: GamepadButton::South },
            RowKind::Display => StrokeInput::Display { index: 0 },
        }
    }

    /// Whether a new row of this kind starts out held for a moment.
    fn held(self) -> bool {
        matches!(self, RowKind::Key | RowKind::Click | RowKind::PadButton)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum RecordingEdit {
    At(usize, f64),
    Hold(usize, f64),
    Kind(usize, RowKind),
    /// The key of a key, key-down or key-up row.
    Key(usize, Key),
    /// The button of a click, button-down or button-up row.
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
    /// Which display the rows after a display change are on (0 = primary).
    Display(usize, u8),
    /// Where a movement starts and ends; the path in between follows.
    MoveEnds(usize, Point, Point),
    /// The total relative motion of a look.
    LookTotal(usize, i32, i32),
    /// Movement between positions ⇄ relative motion.
    SwapMovement(usize),
    Delete(usize),
    Duplicate(usize),
    /// Moves a row to another place in the list; its time moves into place too.
    Reorder(usize, usize),
    /// Add an input after this row (or at the start).
    Insert(Option<usize>, RowKind),
    /// Multiply every time by a factor (0.5 = twice as fast).
    Stretch(f64),
    /// Keep only what happens between two times, in milliseconds.
    Trim(f64, f64),
    /// Remove what happens between two times and close the gap.
    Cut(f64, f64),
    /// Detect movement made in games that lock the pointer.
    MakeSmart,
}

/// What the editor reports for one frame.
#[derive(Default)]
pub struct EditorResponse {
    pub edit: Option<RecordingEdit>,
    /// The user asked for the full editor in its own window.
    pub open_editor: bool,
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
        fn input(recording: &mut Recording, i: usize) -> Option<&mut StrokeInput> {
            recording.strokes.get_mut(i).map(|s| &mut s.input)
        }
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
                    if !s.input.is_held() {
                        s.hold = 0.0;
                    }
                }
            }
            RecordingEdit::Key(i, new_key) => {
                if let Some(StrokeInput::Key { key } | StrokeInput::KeyDown { key } | StrokeInput::KeyUp { key }) =
                    input(recording, i)
                {
                    *key = new_key;
                }
            }
            RecordingEdit::Button(i, new_button) => {
                if let Some(
                    StrokeInput::Mouse { button, .. }
                    | StrokeInput::MouseDown { button, .. }
                    | StrokeInput::MouseUp { button, .. },
                ) = input(recording, i)
                {
                    *button = new_button;
                }
            }
            RecordingEdit::Position(i, new_x, new_y) => match input(recording, i) {
                Some(StrokeInput::Mouse { x, y, end, .. }) => {
                    if let Some(end) = end {
                        end.x += new_x - *x;
                        end.y += new_y - *y;
                    }
                    (*x, *y) = (new_x, new_y);
                }
                Some(StrokeInput::MouseDown { x, y, .. } | StrokeInput::MouseUp { x, y, .. }) => {
                    (*x, *y) = (new_x, new_y);
                }
                _ => {}
            },
            RecordingEdit::End(i, new_end) => {
                if let Some(StrokeInput::Mouse { end, .. }) = input(recording, i) {
                    *end = new_end;
                }
            }
            RecordingEdit::Here(i, new_here) => {
                if let Some(
                    StrokeInput::Mouse { here, .. }
                    | StrokeInput::MouseDown { here, .. }
                    | StrokeInput::MouseUp { here, .. },
                ) = input(recording, i)
                {
                    *here = new_here;
                }
            }
            RecordingEdit::Scroll(i, new_dx, new_dy) => {
                if let Some(StrokeInput::Scroll { dx, dy }) = input(recording, i) {
                    (*dx, *dy) = (new_dx, new_dy);
                }
            }
            RecordingEdit::PadButton(i, new_button) => {
                if let Some(StrokeInput::PadButton { button }) = input(recording, i) {
                    *button = new_button;
                }
            }
            RecordingEdit::PadAxis(i, new_axis, new_value) => {
                if let Some(StrokeInput::PadAxis { axis, value }) = input(recording, i) {
                    (*axis, *value) = (new_axis, new_value.clamp(-1.0, 1.0));
                }
            }
            RecordingEdit::Display(i, new_index) => {
                if let Some(StrokeInput::Display { index }) = input(recording, i) {
                    *index = new_index;
                }
            }
            RecordingEdit::MoveEnds(i, start, end) => {
                if let Some(StrokeInput::Move { path }) = input(recording, i)
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
                if let Some(StrokeInput::Look { path }) = input(recording, i) {
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
            RecordingEdit::Reorder(from, to) => {
                let len = recording.strokes.len();
                if from < len && to < len && from != to {
                    let stroke = recording.strokes.remove(from);
                    recording.strokes.insert(to, stroke);
                    // Playback goes by time: the row takes a time between its new neighbours.
                    let before = to.checked_sub(1).map(|i| recording.strokes[i].at);
                    let after = recording.strokes.get(to + 1).map(|s| s.at);
                    let at = &mut recording.strokes[to].at;
                    if let Some(before) = before {
                        *at = at.max(before);
                    }
                    if let Some(after) = after {
                        *at = at.min(after);
                    }
                }
            }
            RecordingEdit::Insert(after, kind) => {
                let at = after.and_then(|i| recording.strokes.get(i)).map_or(0.0, |s| s.at + s.hold + 50.0);
                let index = after.map_or(0, |i| (i + 1).min(recording.strokes.len()));
                let hold = if kind.held() { 50.0 } else { 0.0 };
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
            RecordingEdit::Trim(start, end) => recording.keep_between(start, end),
            RecordingEdit::Cut(start, end) => recording.remove_between(start, end),
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
        StrokeInput::Key { .. } | StrokeInput::KeyDown { .. } | StrokeInput::KeyUp { .. } => data(DataType::Number),
        StrokeInput::Mouse { .. } | StrokeInput::MouseDown { .. } | StrokeInput::MouseUp { .. } => p.success,
        StrokeInput::Move { .. } => p.text_faint,
        StrokeInput::Look { .. } => data(DataType::Point),
        StrokeInput::Scroll { .. } => p.warning,
        StrokeInput::PadButton { .. } | StrokeInput::PadAxis { .. } => data(DataType::Text),
        StrokeInput::Display { .. } => p.accent,
    }
}

fn stroke_icon(input: &StrokeInput) -> &'static str {
    match input {
        StrokeInput::Key { .. } => icon::KEYBOARD,
        StrokeInput::KeyDown { .. } | StrokeInput::MouseDown { .. } => icon::ARROW_LINE_DOWN,
        StrokeInput::KeyUp { .. } | StrokeInput::MouseUp { .. } => icon::ARROW_LINE_UP,
        StrokeInput::Mouse { .. } => icon::CURSOR_CLICK,
        StrokeInput::Move { .. } => icon::PATH,
        StrokeInput::Look { .. } => icon::CROSSHAIR,
        StrokeInput::Scroll { .. } => icon::ARROWS_VERTICAL,
        StrokeInput::PadButton { .. } | StrokeInput::PadAxis { .. } => icon::GAME_CONTROLLER,
        StrokeInput::Display { .. } => icon::MONITOR,
    }
}

fn lane(input: &StrokeInput) -> f32 {
    match input {
        StrokeInput::Key { .. } | StrokeInput::KeyDown { .. } | StrokeInput::KeyUp { .. } => 0.0,
        StrokeInput::Mouse { .. }
        | StrokeInput::MouseDown { .. }
        | StrokeInput::MouseUp { .. }
        | StrokeInput::Scroll { .. } => 1.0,
        StrokeInput::Move { .. } | StrokeInput::Look { .. } => 2.0,
        StrokeInput::PadButton { .. } | StrokeInput::PadAxis { .. } | StrokeInput::Display { .. } => 3.0,
    }
}

/// When the press a *down* row makes is released: the matching *up* row, if any.
fn release_of(recording: &Recording, index: usize) -> Option<f64> {
    let down = &recording.strokes[index];
    recording.strokes.iter().skip(index + 1).find_map(|s| match (&down.input, &s.input) {
        (StrokeInput::KeyDown { key: a }, StrokeInput::KeyUp { key: b }) if a == b => Some(s.at),
        (StrokeInput::MouseDown { button: a, .. }, StrokeInput::MouseUp { button: b, .. }) if a == b => Some(s.at),
        _ => None,
    })
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

/// The row being dragged to a new place.
#[derive(Clone, Copy)]
struct RowDrag(usize);

/// The timeline: every input as a bar from press to release, one lane per
/// kind, with two handles that select a part to keep (Trim) or remove (Cut).
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
        let end = match stroke.input {
            StrokeInput::KeyDown { .. } | StrokeInput::MouseDown { .. } => {
                release_of(recording, i).unwrap_or(stroke.at)
            }
            _ => stroke.at + stroke.hold,
        };
        let x0 = x_of(stroke.at);
        let x1 = x_of(end).max(x0 + 2.0);
        let top = inner.top() + lane(&stroke.input) * lane_height;
        let bar = Rect::from_min_max(pos2(x0, top + 1.0), pos2(x1, top + lane_height - 1.0));
        let mut color = stroke_color(&stroke.input);
        if state.selected == Some(i) {
            color = theme::accent_bright();
        }
        if matches!(stroke.input, StrokeInput::Display { .. }) {
            painter.vline(x0, inner.y_range(), Stroke::new(1.0, theme::with_alpha(color, 0.6)));
        }
        painter.rect_filled(bar, 1.5, color);
    }

    // Time ticks along the bottom.
    let tick_y = inner.bottom() - 4.0;
    for k in 0..=4 {
        let x = x_of(duration * f64::from(k) / 4.0);
        painter.vline(x, (tick_y - 3.0)..=tick_y, Stroke::new(1.0, theme::stroke_strong()));
    }

    // The handles.
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
    response.on_hover_text(
        "Drag the handles to choose a part. Trim keeps only that part; Cut removes it and closes the gap.",
    );

    let mut edit = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let changed = state.trim.is_some_and(|(s, e)| s > 0.5 || e < duration - 0.5);
        ui.label(
            RichText::new(format!("{} – {}", format_ms(start), format_ms(end))).size(11.0).color(theme::text_dim()),
        );
        if ui
            .add_enabled(changed, egui::Button::new(format!("{}  Trim", icon::SCISSORS)).small())
            .on_hover_text("Keep only the part between the handles")
            .clicked()
        {
            edit = Some(RecordingEdit::Trim(start, end));
            state.trim = None;
        }
        if ui
            .add_enabled(changed, egui::Button::new(format!("{}  Cut", icon::SCISSORS)).small())
            .on_hover_text("Remove the part between the handles; what comes after moves up to close the gap")
            .clicked()
        {
            edit = Some(RecordingEdit::Cut(start, end));
            state.trim = None;
        }
        if changed && ui.small_button("Reset").on_hover_text("Put the handles back at the ends").clicked() {
            state.trim = None;
        }
    });
    edit
}

/// The buttons below the timeline: speed and Smart.
fn tools(ui: &mut Ui) -> Option<RecordingEdit> {
    let mut edit = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(RichText::new(format!("{}  Speed", icon::GAUGE)).size(11.5).color(theme::text_dim()));
        for (label, factor, hint) in [
            ("×2", 0.5, "Twice as fast: every time is halved"),
            ("×1.25", 0.8, "A quarter faster"),
            ("×0.8", 1.25, "A little slower"),
            ("×0.5", 2.0, "Half as fast: every time is doubled"),
        ] {
            if ui.small_button(label).on_hover_text(hint).clicked() {
                edit = Some(RecordingEdit::Stretch(factor));
            }
        }
        if ui
            .small_button(format!("{}  Smart", icon::CROSSHAIR))
            .on_hover_text(
                "Convert to a smart recording: movement made in a game that keeps re-centring the pointer becomes \
                 relative motion, so the camera turns when it is played back.",
            )
            .clicked()
        {
            edit = Some(RecordingEdit::MakeSmart);
        }
    });
    edit
}

fn header(ui: &mut Ui, recording: &Recording) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(recording.summary()).color(theme::text()));
        ui.weak(format!("· {}", format_ms(recording.duration_ms)));
    });
}

fn load_state(ui: &Ui) -> (egui::Id, EditorState) {
    let id = ui.id().with("recording-editor-state");
    (id, ui.data(|d| d.get_temp(id)).unwrap_or_default())
}

/// The compact view for a block's settings: timeline, handles, Trim and Cut,
/// speed, Smart, and the button that opens the full editor. No rows.
pub fn controls(ui: &mut Ui, recording: &Recording) -> EditorResponse {
    let mut out = EditorResponse::default();
    let (state_id, mut state) = load_state(ui);
    header(ui, recording);
    out.edit = timeline(ui, recording, &mut state);
    if let Some(edit) = tools(ui) {
        out.edit = Some(edit);
    }
    ui.add_space(2.0);
    let button = egui::Button::new(RichText::new(format!("{}  Recording Editor", icon::ARROW_SQUARE_OUT)).strong())
        .min_size(vec2(ui.available_width(), 26.0));
    if ui.add(button).on_hover_text("Every recorded input, one per row, in its own window").clicked() {
        out.open_editor = true;
    }
    ui.data_mut(|d| d.insert_temp(state_id, state));
    out
}

/// The full editor: the controls above a table of every recorded input.
/// `table_height` limits the table.
pub fn full(ui: &mut Ui, recording: &Recording, table_height: f32) -> EditorResponse {
    let mut out = EditorResponse::default();
    let (state_id, mut state) = load_state(ui);
    header(ui, recording);
    out.edit = timeline(ui, recording, &mut state);
    ui.horizontal_wrapped(|ui| {
        if let Some(edit) = tools(ui) {
            out.edit = Some(edit);
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
    let small = egui::FontId::proportional(11.0);
    let dragging = egui::DragAndDrop::has_payload_of_type::<RowDrag>(ui.ctx());
    let mut drop: Option<(usize, usize)> = None;
    TableBuilder::new(ui)
        .id_salt("recording-table")
        .striped(true)
        .resizable(true)
        .sense(Sense::click())
        .max_scroll_height(table_height)
        .auto_shrink([false, true])
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::exact(52.0))
        .column(Column::initial(118.0).at_least(80.0))
        .column(Column::remainder().at_least(140.0).clip(true))
        .column(Column::exact(76.0))
        .column(Column::exact(70.0))
        .column(Column::exact(46.0))
        .header(18.0, |mut header| {
            for title in ["#", "Type", "Input", "At", "Held", ""] {
                header.col(|ui| {
                    ui.label(RichText::new(title).font(small.clone()).color(theme::text_faint()));
                });
            }
        })
        .body(|body| {
            body.rows(ROW_HEIGHT, recording.strokes.len(), |mut row| {
                let i = row.index();
                let stroke = &recording.strokes[i];
                row.set_selected(state.selected == Some(i));
                row.col(|ui| {
                    // The grip: drag it to move the row.
                    let id = ui.id().with(("row-grip", i));
                    ui.dnd_drag_source(id, RowDrag(i), |ui| {
                        ui.label(RichText::new(icon::DOTS_SIX_VERTICAL).color(theme::text_faint()));
                        ui.label(RichText::new(format!("{}", i + 1)).size(11.0).color(theme::text_faint()));
                    })
                    .response
                    .on_hover_text("Drag to move this input to another place");
                });
                row.col(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.label(RichText::new(stroke_icon(&stroke.input)).color(stroke_color(&stroke.input)));
                    kind_cell(ui, i, &stroke.input, &mut out.edit);
                });
                row.col(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    ui.spacing_mut().interact_size.y = 20.0;
                    ui.style_mut().override_font_id = Some(egui::FontId::proportional(12.0));
                    if let Some(change) = detail(ui, i, &stroke.input) {
                        out.edit = Some(change);
                    }
                });
                row.col(|ui| {
                    let mut at = stroke.at / 1000.0;
                    let field =
                        DragValue::new(&mut at).speed(0.01).range(0.0..=31_536_000.0).fixed_decimals(3).suffix(" s");
                    if ui.add(field).on_hover_text("When it happens").changed() {
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
                    let copy =
                        egui::Button::new(RichText::new(icon::COPY).size(12.0).color(theme::text_faint())).frame(false);
                    if ui.add(copy).on_hover_text("Duplicate").clicked() {
                        out.edit = Some(RecordingEdit::Duplicate(i));
                    }
                    let delete =
                        egui::Button::new(RichText::new(icon::X).size(12.0).color(theme::text_faint())).frame(false);
                    if ui.add(delete).on_hover_text("Remove this input").clicked() {
                        out.edit = Some(RecordingEdit::Delete(i));
                    }
                });
                let response = row.response();
                if dragging && response.dnd_hover_payload::<RowDrag>().is_some() {
                    // Where the row would go: a line above or below this one.
                    let rect = response.rect;
                    let below = response.hover_pos().or(ui_pointer(&response)).is_some_and(|p| p.y > rect.center().y);
                    let y = if below { rect.bottom() } else { rect.top() };
                    response.ctx.debug_painter().hline(rect.x_range(), y, Stroke::new(2.0, theme::accent_bright()));
                }
                if let Some(from) = response.dnd_release_payload::<RowDrag>() {
                    let below = ui_pointer(&response).is_some_and(|p| p.y > response.rect.center().y);
                    let mut to = if below { i + 1 } else { i };
                    if from.0 < to {
                        to -= 1;
                    }
                    drop = Some((from.0, to.min(recording.strokes.len() - 1)));
                }
                if response.clicked() {
                    state.selected = if state.selected == Some(i) { None } else { Some(i) };
                }
            });
        });

    if let Some((from, to)) = drop
        && from != to
    {
        out.edit = Some(RecordingEdit::Reorder(from, to));
        state.selected = Some(to);
    }
    if let Some(RecordingEdit::Delete(i)) = &out.edit {
        state.selected = state.selected.filter(|s| s < i);
    }
    ui.data_mut(|d| d.insert_temp(state_id, state));
    out
}

fn ui_pointer(response: &egui::Response) -> Option<egui::Pos2> {
    response.ctx.pointer_latest_pos()
}

/// The type column: switchable for simple inputs, Move ⇄ Look for movement.
fn kind_cell(ui: &mut Ui, index: usize, input: &StrokeInput, edit: &mut Option<RecordingEdit>) {
    match RowKind::of(input) {
        Some(kind) => {
            egui::ComboBox::from_id_salt(("kind", index)).width(92.0).selected_text(input.kind_label()).show_ui(
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

/// A button picker, the "at the pointer" switch and, unless it is on, the position.
fn button_detail(
    ui: &mut Ui,
    index: usize,
    button: MouseButton,
    (x, y): (i32, i32),
    here: bool,
    edit: &mut Option<RecordingEdit>,
) {
    egui::ComboBox::from_id_salt(("button", index)).width(70.0).selected_text(button.label()).show_ui(ui, |ui| {
        for &option in MouseButton::ALL {
            if ui.selectable_label(option == button, option.label()).clicked() {
                *edit = Some(RecordingEdit::Button(index, option));
            }
        }
    });
    let at_pointer = RichText::new(icon::CURSOR).color(if here { theme::accent_bright() } else { theme::text_faint() });
    if ui
        .add(egui::Button::new(at_pointer).frame(false).selected(here))
        .on_hover_text("Press wherever the pointer is, instead of at the position")
        .clicked()
    {
        *edit = Some(RecordingEdit::Here(index, !here));
    }
    if !here {
        let (mut nx, mut ny) = (x, y);
        let a = ui.add(DragValue::new(&mut nx).speed(1.0)).on_hover_text("Where it is pressed");
        let b = ui.add(DragValue::new(&mut ny).speed(1.0)).on_hover_text("Where it is pressed");
        if a.changed() || b.changed() {
            *edit = Some(RecordingEdit::Position(index, nx, ny));
        }
    }
}

/// The input column: what the input is, editable.
fn detail(ui: &mut Ui, index: usize, input: &StrokeInput) -> Option<RecordingEdit> {
    let mut edit = None;
    let coord = |ui: &mut Ui, value: &mut i32, hint: &str| {
        ui.add(DragValue::new(value).speed(1.0)).on_hover_text(hint).changed()
    };
    match input {
        StrokeInput::Key { key } | StrokeInput::KeyDown { key } | StrokeInput::KeyUp { key } => {
            let mut key = *key;
            if widgets::key_picker(ui, ("key", index), &mut key) {
                edit = Some(RecordingEdit::Key(index, key));
            }
        }
        StrokeInput::Mouse { button, x, y, end, here } => {
            button_detail(ui, index, *button, (*x, *y), *here, &mut edit);
            if !*here {
                match end {
                    Some(end) => {
                        ui.label(RichText::new("›").color(theme::text_faint()));
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
        StrokeInput::MouseDown { button, x, y, here } | StrokeInput::MouseUp { button, x, y, here } => {
            button_detail(ui, index, *button, (*x, *y), *here, &mut edit);
        }
        StrokeInput::Move { path } => {
            if let (Some(a), Some(b)) = (path.first(), path.last()) {
                let (mut ax, mut ay, mut bx, mut by) = (a.1, a.2, b.1, b.2);
                let mut changed =
                    coord(ui, &mut ax, "Where the movement starts") | coord(ui, &mut ay, "Where the movement starts");
                ui.label(RichText::new("›").color(theme::text_faint()));
                changed |=
                    coord(ui, &mut bx, "Where the movement ends") | coord(ui, &mut by, "Where the movement ends");
                if changed {
                    edit = Some(RecordingEdit::MoveEnds(index, Point::new(ax, ay), Point::new(bx, by)));
                }
            }
            if ui.small_button("› Look").on_hover_text("Replay as relative motion (for game cameras)").clicked() {
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
            if ui.small_button("› Move").on_hover_text("Replay as movement between screen positions").clicked() {
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
        StrokeInput::Display { index: display } => {
            let mut shown = u32::from(*display) + 1;
            let field = DragValue::new(&mut shown).range(1..=16).prefix("Display ");
            if ui
                .add(field)
                .on_hover_text("The positions after this row are on this display (1 is the primary display)")
                .changed()
            {
                edit = Some(RecordingEdit::Display(index, (shown - 1) as u8));
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
    fn down_up_and_display_rows_are_edited_like_the_others() {
        let mut recording = sample();
        RecordingEdit::Kind(0, RowKind::KeyDown).apply(&mut recording);
        assert_eq!(recording.strokes[0].hold, 0.0, "a key-down has no hold of its own");
        RecordingEdit::Key(0, Key::KeyQ).apply(&mut recording);
        assert_eq!(recording.strokes[0].input, StrokeInput::KeyDown { key: Key::KeyQ });
        RecordingEdit::Kind(1, RowKind::ButtonUp).apply(&mut recording);
        RecordingEdit::Button(1, MouseButton::Right).apply(&mut recording);
        RecordingEdit::Here(1, false).apply(&mut recording);
        RecordingEdit::Position(1, 7, 8).apply(&mut recording);
        assert_eq!(
            recording.strokes[1].input,
            StrokeInput::MouseUp { button: MouseButton::Right, x: 7, y: 8, here: false }
        );
        RecordingEdit::Insert(None, RowKind::Display).apply(&mut recording);
        RecordingEdit::Display(0, 2).apply(&mut recording);
        assert_eq!(recording.strokes[0].input, StrokeInput::Display { index: 2 });
    }

    #[test]
    fn rows_are_reordered_and_take_a_time_between_their_neighbours() {
        let mut recording = sample();
        RecordingEdit::Reorder(0, 2).apply(&mut recording);
        assert!(matches!(recording.strokes[2].input, StrokeInput::Key { .. }));
        assert_eq!(recording.strokes[2].at, 320.0, "placed after the move, so it now happens with it");
        RecordingEdit::Reorder(3, 0).apply(&mut recording);
        assert!(matches!(recording.strokes[0].input, StrokeInput::Look { .. }));
        assert_eq!(recording.strokes[0].at, 200.0, "moved to the top, it happens first");
        let mut sorted = recording.clone();
        sorted.normalize();
        assert_eq!(sorted.strokes, recording.strokes, "the new order is also the order in time");
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
    fn trim_keeps_the_part_between_the_handles() {
        let mut recording = sample();
        RecordingEdit::Trim(120.0, 330.0).apply(&mut recording);
        assert_eq!(recording.strokes[0].at, 0.0, "the key, held across the start, starts at the new beginning");
        assert_eq!(recording.strokes[0].hold, 30.0);
        assert_eq!(recording.strokes[1].at, 80.0);
        assert!(recording.duration_ms <= 210.0 + 1e-9);
        assert!(recording.strokes.iter().all(|s| !matches!(s.input, StrokeInput::Look { .. })));
    }

    #[test]
    fn cut_removes_the_part_between_the_handles_and_closes_the_gap() {
        let mut recording = sample();
        RecordingEdit::Cut(180.0, 380.0).apply(&mut recording);
        assert_eq!(recording.strokes.len(), 2, "the click and the move are gone");
        assert!(matches!(recording.strokes[0].input, StrokeInput::Key { .. }));
        assert!(matches!(recording.strokes[1].input, StrokeInput::Look { .. }));
        assert_eq!(recording.strokes[1].at, 200.0, "the look moved up by the 200 ms removed");
        assert_eq!(recording.duration_ms, 300.0);
    }

    #[test]
    fn stretching_scales_every_time() {
        let mut recording = sample();
        RecordingEdit::Stretch(0.5).apply(&mut recording);
        assert_eq!((recording.strokes[0].at, recording.strokes[0].hold), (50.0, 25.0));
        assert_eq!(recording.duration_ms, 250.0);
    }

    #[test]
    fn rows_are_dragged_to_a_new_place_by_their_grip() {
        use egui_kittest::Harness;
        use egui_kittest::kittest::Queryable;

        struct State {
            recording: Recording,
            edits: Vec<RecordingEdit>,
        }
        let mut harness = Harness::builder().with_size(vec2(900.0, 500.0)).build_ui_state(
            |ui, state: &mut State| {
                if let Some(edit) = full(ui, &state.recording, 300.0).edit {
                    state.edits.push(edit);
                }
            },
            State { recording: sample(), edits: Vec::new() },
        );
        harness.run();
        let from = harness.get_by_label("1").rect().center();
        // The lower half of the third row: the row goes after it.
        let to = harness.get_by_label("3").rect().center() + vec2(0.0, 6.0);
        harness.hover_at(from);
        harness.run();
        harness.drag_at(from);
        harness.run();
        for step in 1..=5 {
            harness.hover_at(from + (to - from) * (step as f32 / 5.0));
            harness.run();
        }
        harness.drop_at(to);
        harness.run();
        assert_eq!(harness.state().edits, [RecordingEdit::Reorder(0, 2)]);
    }

    #[test]
    fn splits_add_up() {
        assert_eq!(rounded_split(10, &[1.0, 1.0, 1.0]).iter().sum::<i32>(), 10);
        assert_eq!(rounded_split(-7, &[0.0, 0.0]).iter().sum::<i32>(), -7);
    }
}

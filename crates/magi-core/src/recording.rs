//! Recordings of real input.
//!
//! A recording is stored as a list of **strokes** rather than a raw event log.
//! A stroke is one thing the user did – a key held from press to release, a
//! click (or drag), a continuous mouse movement, a scroll – together with when
//! it started and how long it was held. That is both what a person wants to
//! edit ("this key, pressed at 1.2 s, held 85 ms") and enough to reproduce the
//! original press and release timing exactly: see [`Recording::timeline`].

use serde::{Deserialize, Serialize};

use crate::geom::Point;
use crate::input::{Button, GamepadAxis, GamepadButton, InputEvent, Key, MouseButton};

/// Mouse samples closer together than this are dropped while recording.
/// 5 ms keeps 200 Hz of motion, plenty to reproduce a gesture.
const MIN_PATH_SPACING_MS: f64 = 5.0;
/// A pause in movement longer than this starts a new move stroke.
const PATH_GAP_MS: f64 = 120.0;
/// Analogue axis changes smaller than this are noise, not intent.
const AXIS_EPSILON: f32 = 0.02;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Recording {
    /// Total length in milliseconds (may extend past the last stroke).
    #[serde(default)]
    pub duration_ms: f64,
    /// Size of the primary screen when recorded, for future rescaling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen: Option<(u32, u32)>,
    pub strokes: Vec<Stroke>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    /// Milliseconds from the start of the recording to the press.
    pub at: f64,
    /// Milliseconds between press and release. Zero for instantaneous strokes.
    #[serde(default)]
    pub hold: f64,
    #[serde(flatten)]
    pub input: StrokeInput,
}

/// One sample of a mouse movement: milliseconds since the stroke began, x, y.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PathPoint(pub f32, pub i32, pub i32);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StrokeInput {
    Key {
        key: Key,
    },
    /// A click, or a drag when `end` differs from the press position.
    Mouse {
        button: MouseButton,
        x: i32,
        y: i32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        end: Option<Point>,
    },
    /// A continuous pointer movement. `hold` is its duration.
    Move {
        path: Vec<PathPoint>,
    },
    Scroll {
        dx: i32,
        dy: i32,
    },
    PadButton {
        button: GamepadButton,
    },
    PadAxis {
        axis: GamepadAxis,
        value: f32,
    },
}

impl StrokeInput {
    pub fn kind_label(&self) -> &'static str {
        match self {
            StrokeInput::Key { .. } => "Key",
            StrokeInput::Mouse { end: Some(_), .. } => "Drag",
            StrokeInput::Mouse { .. } => "Click",
            StrokeInput::Move { .. } => "Move",
            StrokeInput::Scroll { .. } => "Scroll",
            StrokeInput::PadButton { .. } => "Pad",
            StrokeInput::PadAxis { .. } => "Axis",
        }
    }

    /// Whether the stroke has a meaningful hold time.
    pub fn is_held(&self) -> bool {
        !matches!(self, StrokeInput::Scroll { .. } | StrokeInput::PadAxis { .. })
    }
}

impl Recording {
    /// The time the last stroke ends.
    pub fn end_time(&self) -> f64 {
        self.strokes.iter().map(|s| s.at + s.hold).fold(0.0, f64::max)
    }

    /// Keeps strokes ordered by start time and the duration consistent.
    /// Call after editing times by hand.
    pub fn normalize(&mut self) {
        for stroke in &mut self.strokes {
            stroke.at = stroke.at.max(0.0);
            stroke.hold = stroke.hold.max(0.0);
        }
        self.strokes.sort_by(|a, b| a.at.total_cmp(&b.at));
        self.duration_ms = self.duration_ms.max(self.end_time());
    }

    /// Removes the idle time before the first stroke.
    pub fn trim_start(&mut self) {
        let Some(first) = self.strokes.iter().map(|s| s.at).reduce(f64::min) else {
            self.duration_ms = 0.0;
            return;
        };
        for stroke in &mut self.strokes {
            stroke.at -= first;
        }
        self.duration_ms = (self.duration_ms - first).max(self.end_time());
    }

    /// Flattens the strokes into the exact sequence of timed events to send:
    /// every press, every release, every pointer sample, ordered by time.
    pub fn timeline(&self, include_moves: bool) -> Vec<(f64, InputEvent)> {
        let mut events = Vec::with_capacity(self.strokes.len() * 2);
        for stroke in &self.strokes {
            let (start, end) = (stroke.at, stroke.at + stroke.hold);
            match &stroke.input {
                StrokeInput::Key { key } => {
                    events.push((start, InputEvent::KeyDown { key: *key }));
                    events.push((end, InputEvent::KeyUp { key: *key }));
                }
                StrokeInput::Mouse { button, x, y, end: release_at } => {
                    // The explicit move makes clicks land correctly even when
                    // movement strokes were not recorded or are skipped.
                    events.push((start, InputEvent::MoveTo { x: *x, y: *y }));
                    events.push((start, InputEvent::ButtonDown { button: *button }));
                    if let Some(p) = release_at {
                        events.push((end, InputEvent::MoveTo { x: p.x, y: p.y }));
                    }
                    events.push((end, InputEvent::ButtonUp { button: *button }));
                }
                StrokeInput::Move { path } => {
                    if include_moves {
                        events.extend(
                            path.iter()
                                .map(|&PathPoint(dt, x, y)| (start + f64::from(dt), InputEvent::MoveTo { x, y })),
                        );
                    }
                }
                StrokeInput::Scroll { dx, dy } => {
                    events.push((start, InputEvent::Scroll { dx: *dx, dy: *dy }));
                }
                StrokeInput::PadButton { button } => {
                    events.push((start, InputEvent::PadButtonDown { button: *button }));
                    events.push((end, InputEvent::PadButtonUp { button: *button }));
                }
                StrokeInput::PadAxis { axis, value } => {
                    events.push((start, InputEvent::PadAxis { axis: *axis, value: *value }));
                }
            }
        }
        // Stable, so events generated for the same instant keep their order
        // (move before press, release-move before release).
        events.sort_by(|a, b| a.0.total_cmp(&b.0));
        events
    }

    /// Short description such as `14 keys · 6 clicks · 3 moves`.
    pub fn summary(&self) -> String {
        let (mut keys, mut clicks, mut moves, mut scrolls, mut pad) = (0, 0, 0, 0, 0);
        for stroke in &self.strokes {
            match stroke.input {
                StrokeInput::Key { .. } => keys += 1,
                StrokeInput::Mouse { .. } => clicks += 1,
                StrokeInput::Move { .. } => moves += 1,
                StrokeInput::Scroll { .. } => scrolls += 1,
                StrokeInput::PadButton { .. } | StrokeInput::PadAxis { .. } => pad += 1,
            }
        }
        let parts: Vec<String> = [
            (keys, "key", "keys"),
            (clicks, "click", "clicks"),
            (moves, "move", "moves"),
            (scrolls, "scroll", "scrolls"),
            (pad, "pad input", "pad inputs"),
        ]
        .into_iter()
        .filter(|(n, ..)| *n > 0)
        .map(|(n, one, many)| format!("{n} {}", if n == 1 { one } else { many }))
        .collect();
        if parts.is_empty() { "Empty".to_owned() } else { parts.join(" · ") }
    }
}

/// What a recording session should capture.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecordOptions {
    pub keyboard: bool,
    pub mouse_buttons: bool,
    pub mouse_moves: bool,
    pub scroll: bool,
    pub gamepad: bool,
    /// Drop the idle time between starting the recording and the first input.
    pub trim_start: bool,
}

impl Default for RecordOptions {
    fn default() -> Self {
        Self { keyboard: true, mouse_buttons: true, mouse_moves: true, scroll: true, gamepad: true, trim_start: true }
    }
}

/// Builds a [`Recording`] from a live stream of input events.
#[derive(Debug)]
pub struct RecordingBuilder {
    options: RecordOptions,
    /// Buttons to leave out entirely – typically the record hotkey itself.
    ignored: Vec<Button>,
    cursor: Point,
    strokes: Vec<Stroke>,
    /// Buttons currently held, with the index of the stroke they opened.
    open: Vec<(Button, usize)>,
    /// The move stroke currently being extended, and when it was last extended.
    path: Option<(usize, f64)>,
    /// The newest pointer sample that thinning skipped. It is written out when
    /// the movement ends, so a gesture always finishes exactly where it did.
    skipped: Option<(f64, i32, i32)>,
    last_axis: Vec<(GamepadAxis, f32)>,
}

impl RecordingBuilder {
    /// `cursor` is where the pointer is right now, so that a click made before
    /// the first movement is still recorded at the right place.
    pub fn new(options: RecordOptions, ignored: Vec<Button>, cursor: Point) -> Self {
        Self {
            options,
            ignored: ignored.into_iter().map(Button::canonical).collect(),
            cursor,
            strokes: Vec::new(),
            open: Vec::new(),
            path: None,
            skipped: None,
            last_axis: Vec::new(),
        }
    }

    pub fn stroke_count(&self) -> usize {
        self.strokes.len()
    }

    /// Feeds one observed event. `t` is milliseconds since recording began.
    pub fn push(&mut self, t: f64, event: &InputEvent) {
        match *event {
            InputEvent::MoveTo { x, y } => self.on_move(t, x, y),
            InputEvent::Scroll { dx, dy } => {
                if self.options.scroll {
                    self.strokes.push(Stroke { at: t, hold: 0.0, input: StrokeInput::Scroll { dx, dy } });
                }
            }
            InputEvent::PadAxis { axis, value } => self.on_axis(t, axis, value),
            _ => {
                if let Some((button, is_down)) = event.button_edge() {
                    self.on_button(t, button, is_down);
                }
            }
        }
    }

    fn wants(&self, button: Button) -> bool {
        if self.ignored.contains(&button.canonical()) {
            return false;
        }
        match button {
            Button::Key(_) => self.options.keyboard,
            Button::Mouse(_) => self.options.mouse_buttons,
            Button::Pad(_) => self.options.gamepad,
        }
    }

    fn on_button(&mut self, t: f64, button: Button, is_down: bool) {
        if !self.wants(button) {
            return;
        }
        if is_down {
            if self.open.iter().any(|(b, _)| *b == button) {
                return; // keyboard auto-repeat
            }
            let input = match button {
                Button::Key(key) => StrokeInput::Key { key },
                Button::Mouse(button) => StrokeInput::Mouse { button, x: self.cursor.x, y: self.cursor.y, end: None },
                Button::Pad(button) => StrokeInput::PadButton { button },
            };
            self.open.push((button, self.strokes.len()));
            self.strokes.push(Stroke { at: t, hold: 0.0, input });
        } else if let Some(pos) = self.open.iter().position(|(b, _)| *b == button) {
            let (_, index) = self.open.swap_remove(pos);
            self.close(index, t);
        }
        // A release without a recorded press was held before recording began: skip it.
    }

    fn close(&mut self, index: usize, t: f64) {
        let cursor = self.cursor;
        let stroke = &mut self.strokes[index];
        stroke.hold = (t - stroke.at).max(0.0);
        if let StrokeInput::Mouse { x, y, end, .. } = &mut stroke.input
            && (*x, *y) != (cursor.x, cursor.y)
        {
            *end = Some(cursor);
        }
    }

    fn on_move(&mut self, t: f64, x: i32, y: i32) {
        // Always track the pointer: clicks need a position even if moves are not recorded.
        self.cursor = Point::new(x, y);
        if !self.options.mouse_moves {
            return;
        }
        if let Some((index, last)) = self.path
            && t - last <= PATH_GAP_MS
        {
            if t - last < MIN_PATH_SPACING_MS {
                self.skipped = Some((t, x, y));
                return;
            }
            self.extend_path(index, t, x, y);
            return;
        }
        self.flush_skipped();
        self.path = Some((self.strokes.len(), t));
        self.strokes.push(Stroke { at: t, hold: 0.0, input: StrokeInput::Move { path: vec![PathPoint(0.0, x, y)] } });
    }

    fn extend_path(&mut self, index: usize, t: f64, x: i32, y: i32) {
        let stroke = &mut self.strokes[index];
        stroke.hold = t - stroke.at;
        if let StrokeInput::Move { path } = &mut stroke.input {
            path.push(PathPoint(stroke.hold as f32, x, y));
        }
        self.path = Some((index, t));
        self.skipped = None;
    }

    fn flush_skipped(&mut self) {
        if let (Some((t, x, y)), Some((index, _))) = (self.skipped.take(), self.path) {
            self.extend_path(index, t, x, y);
        }
    }

    fn on_axis(&mut self, t: f64, axis: GamepadAxis, value: f32) {
        if !self.options.gamepad {
            return;
        }
        match self.last_axis.iter_mut().find(|(a, _)| *a == axis) {
            Some((_, last)) if (*last - value).abs() < AXIS_EPSILON => return,
            Some((_, last)) => *last = value,
            None => self.last_axis.push((axis, value)),
        }
        self.strokes.push(Stroke { at: t, hold: 0.0, input: StrokeInput::PadAxis { axis, value } });
    }

    /// Ends the recording at time `t`, releasing anything still held.
    pub fn finish(mut self, t: f64, screen: Option<(u32, u32)>) -> Recording {
        self.flush_skipped();
        for (_, index) in std::mem::take(&mut self.open) {
            self.close(index, t);
        }
        let mut recording = Recording { duration_ms: t.max(0.0), screen, strokes: self.strokes };
        recording.normalize();
        if self.options.trim_start {
            recording.trim_start();
        }
        recording
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn builder() -> RecordingBuilder {
        let options = RecordOptions { trim_start: false, ..RecordOptions::default() };
        RecordingBuilder::new(options, vec![Button::Key(Key::F9)], Point::new(10, 10))
    }

    #[test]
    fn keys_remember_press_and_release() {
        let mut b = builder();
        b.push(100.0, &InputEvent::KeyDown { key: Key::KeyA });
        b.push(130.0, &InputEvent::KeyDown { key: Key::KeyA }); // auto-repeat
        b.push(185.0, &InputEvent::KeyUp { key: Key::KeyA });
        let rec = b.finish(300.0, None);
        assert_eq!(rec.strokes.len(), 1);
        assert_eq!(rec.strokes[0].at, 100.0);
        assert_eq!(rec.strokes[0].hold, 85.0);
        assert_eq!(rec.duration_ms, 300.0);

        let timeline = rec.timeline(true);
        assert_eq!(timeline[0], (100.0, InputEvent::KeyDown { key: Key::KeyA }));
        assert_eq!(timeline[1], (185.0, InputEvent::KeyUp { key: Key::KeyA }));
    }

    #[test]
    fn the_record_hotkey_is_left_out() {
        let mut b = builder();
        b.push(1.0, &InputEvent::KeyDown { key: Key::F9 });
        b.push(2.0, &InputEvent::KeyUp { key: Key::F9 });
        assert_eq!(b.stroke_count(), 0);
    }

    #[test]
    fn drags_record_where_they_end() {
        let mut b = builder();
        b.push(0.0, &InputEvent::ButtonDown { button: MouseButton::Left });
        b.push(10.0, &InputEvent::MoveTo { x: 50, y: 60 });
        b.push(20.0, &InputEvent::ButtonUp { button: MouseButton::Left });
        let rec = b.finish(20.0, None);
        let click = rec.strokes.iter().find(|s| matches!(s.input, StrokeInput::Mouse { .. })).unwrap();
        assert_eq!(
            click.input,
            StrokeInput::Mouse { button: MouseButton::Left, x: 10, y: 10, end: Some(Point::new(50, 60)) }
        );

        let without_moves = rec.timeline(false);
        assert_eq!(
            without_moves.iter().map(|(_, e)| e.clone()).collect::<Vec<_>>(),
            vec![
                InputEvent::MoveTo { x: 10, y: 10 },
                InputEvent::ButtonDown { button: MouseButton::Left },
                InputEvent::MoveTo { x: 50, y: 60 },
                InputEvent::ButtonUp { button: MouseButton::Left },
            ]
        );
    }

    #[test]
    fn movement_is_grouped_and_thinned() {
        let mut b = builder();
        for i in 0..20 {
            b.push(f64::from(i), &InputEvent::MoveTo { x: i, y: 0 }); // 1 kHz mouse
        }
        b.push(500.0, &InputEvent::MoveTo { x: 99, y: 0 }); // after a pause
        let rec = b.finish(500.0, None);
        assert_eq!(rec.strokes.len(), 2);
        let StrokeInput::Move { path } = &rec.strokes[0].input else { panic!() };
        assert_eq!(path.len(), 5, "samples at 0, 5, 10, 15 ms plus the final resting point");
        assert_eq!(path.last(), Some(&PathPoint(19.0, 19, 0)));
    }

    #[test]
    fn held_keys_are_released_at_the_end_and_start_is_trimmed() {
        let mut b = RecordingBuilder::new(RecordOptions::default(), vec![], Point::default());
        b.push(1000.0, &InputEvent::KeyDown { key: Key::KeyW });
        let rec = b.finish(1500.0, None);
        assert_eq!(rec.strokes[0].at, 0.0);
        assert_eq!(rec.strokes[0].hold, 500.0);
        assert_eq!(rec.duration_ms, 500.0);
    }

    #[test]
    fn strokes_serialize_flat() {
        let stroke = Stroke { at: 1.0, hold: 2.0, input: StrokeInput::Key { key: Key::KeyA } };
        let json = serde_json::to_string(&stroke).unwrap();
        assert_eq!(json, r#"{"at":1.0,"hold":2.0,"type":"key","key":"KeyA"}"#);
        assert_eq!(serde_json::from_str::<Stroke>(&json).unwrap(), stroke);
    }
}

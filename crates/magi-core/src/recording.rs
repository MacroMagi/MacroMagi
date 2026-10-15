//! Recordings of real input.
//!
//! A recording is stored as a list of **strokes** rather than a raw event log.
//! A stroke is one thing the user did – a key held from press to release, a
//! click (or drag), a continuous mouse movement, a scroll – together with when
//! it started and how long it was held. That is both what a person wants to
//! edit ("this key, pressed at 1.2 s, held 85 ms") and enough to reproduce the
//! original press and release timing exactly: see [`Recording::timeline`].
//!
//! ## Pointer movement in games
//!
//! Most programs follow the pointer to a position on screen. Many games (any
//! first-person camera) instead lock the pointer in place and read how far the
//! mouse *moved*. Replaying screen positions into such a game turns the camera
//! once and then does nothing, because the game keeps putting the pointer back.
//!
//! So movement comes in two kinds of stroke: [`StrokeInput::Move`] (positions)
//! and [`StrokeInput::Look`] (relative motion). A **smart** recording records
//! both and keeps, for each stretch of movement, whichever matches what the
//! pointer really did. Existing recordings can be converted afterwards with
//! [`Recording::make_smart`], and playback can force either kind with
//! [`MouseReplay`].

use serde::{Deserialize, Serialize};

use crate::geom::Point;
use crate::input::{Button, GamepadAxis, GamepadButton, InputEvent, Key, MouseButton};

/// Mouse samples closer together than this are merged while recording.
/// 5 ms keeps 200 Hz of motion, plenty to reproduce a gesture.
const MIN_PATH_SPACING_MS: f64 = 5.0;
/// A pause in movement longer than this starts a new move stroke.
const PATH_GAP_MS: f64 = 120.0;
/// Analogue axis changes smaller than this are noise, not intent.
const AXIS_EPSILON: f32 = 0.02;
/// Below this much relative motion a stretch is too small to judge.
const MIN_LOOK_TRAVEL: f64 = 40.0;

fn is_false(value: &bool) -> bool {
    !*value
}

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

/// One sample of a pointer path: milliseconds since the stroke began, x, y.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PathPoint(pub f32, pub i32, pub i32);

/// One sample of relative motion: milliseconds since the stroke began, dx, dy.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LookPoint(pub f32, pub i32, pub i32);

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
        /// Click wherever the pointer is instead of moving to `x, y` first
        /// (clicks made while a game had the pointer locked).
        #[serde(default, skip_serializing_if = "is_false")]
        here: bool,
    },
    /// A continuous pointer movement between screen positions. `hold` is its duration.
    Move {
        path: Vec<PathPoint>,
    },
    /// Relative mouse motion, as a game camera reads it. `hold` is its duration.
    Look {
        path: Vec<LookPoint>,
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
            StrokeInput::Look { .. } => "Look",
            StrokeInput::Scroll { .. } => "Scroll",
            StrokeInput::PadButton { .. } => "Pad button",
            StrokeInput::PadAxis { .. } => "Pad stick",
        }
    }

    /// Whether the stroke has a meaningful hold time.
    pub fn is_held(&self) -> bool {
        !matches!(self, StrokeInput::Scroll { .. } | StrokeInput::PadAxis { .. })
    }
}

/// How a Play Recording block replays pointer movement.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseReplay {
    /// As recorded: screen positions where it moved to positions, relative
    /// motion where a game had the pointer locked.
    #[default]
    Recorded,
    /// Only screen positions. Relative motion is left out.
    Absolute,
    /// Everything as relative motion, and clicks where the pointer is.
    /// For games that keep re-centring the pointer.
    Relative,
}

impl MouseReplay {
    pub const ALL: [MouseReplay; 3] = [MouseReplay::Recorded, MouseReplay::Absolute, MouseReplay::Relative];

    pub fn label(self) -> &'static str {
        match self {
            MouseReplay::Recorded => "As recorded (smart)",
            MouseReplay::Absolute => "Screen positions",
            MouseReplay::Relative => "Relative motion (games)",
        }
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

    /// Keeps only what happens between `start` and `end` (milliseconds), and
    /// makes `start` the new beginning. Inputs held across a cut are shortened.
    pub fn cut(&mut self, start: f64, end: f64) {
        let (start, end) = (start.max(0.0), end.max(start));
        self.strokes.retain(|s| s.at < end && (s.at + s.hold > start || (s.hold == 0.0 && s.at >= start)));
        for stroke in &mut self.strokes {
            let (from, to) = (stroke.at.max(start), (stroke.at + stroke.hold).min(end));
            let shift = (from - stroke.at) as f32;
            match &mut stroke.input {
                StrokeInput::Move { path } => {
                    path.retain(|p| stroke.at + f64::from(p.0) >= from && stroke.at + f64::from(p.0) <= to);
                    path.iter_mut().for_each(|p| p.0 -= shift);
                }
                StrokeInput::Look { path } => {
                    path.retain(|p| stroke.at + f64::from(p.0) >= from && stroke.at + f64::from(p.0) <= to);
                    path.iter_mut().for_each(|p| p.0 -= shift);
                }
                _ => {}
            }
            stroke.at = from - start;
            stroke.hold = (to - from).max(0.0);
        }
        self.strokes.retain(|s| match &s.input {
            StrokeInput::Move { path } => !path.is_empty(),
            StrokeInput::Look { path } => !path.is_empty(),
            _ => true,
        });
        self.duration_ms = (end - start).min(self.duration_ms - start).max(self.end_time());
    }

    /// Flattens the strokes into the exact sequence of timed events to send,
    /// replaying movement as recorded.
    pub fn timeline(&self, include_moves: bool) -> Vec<(f64, InputEvent)> {
        self.timeline_with(include_moves, MouseReplay::Recorded)
    }

    /// Flattens the strokes into the exact sequence of timed events to send:
    /// every press, every release, every pointer sample, ordered by time.
    pub fn timeline_with(&self, include_moves: bool, mouse: MouseReplay) -> Vec<(f64, InputEvent)> {
        let mut events = Vec::with_capacity(self.strokes.len() * 2);
        // Where an absolute path last left the pointer, for converting to relative motion.
        let mut last: Option<(i32, i32)> = None;
        for stroke in &self.strokes {
            let (start, end) = (stroke.at, stroke.at + stroke.hold);
            match &stroke.input {
                StrokeInput::Key { key } => {
                    events.push((start, InputEvent::KeyDown { key: *key }));
                    events.push((end, InputEvent::KeyUp { key: *key }));
                }
                StrokeInput::Mouse { button, x, y, end: release_at, here } => {
                    // The explicit move makes clicks land correctly even when
                    // movement strokes were not recorded or are skipped.
                    let stay = *here || mouse == MouseReplay::Relative;
                    if !stay {
                        events.push((start, InputEvent::MoveTo { x: *x, y: *y }));
                    }
                    events.push((start, InputEvent::ButtonDown { button: *button }));
                    if let (Some(p), false) = (release_at, stay) {
                        events.push((end, InputEvent::MoveTo { x: p.x, y: p.y }));
                    }
                    events.push((end, InputEvent::ButtonUp { button: *button }));
                }
                StrokeInput::Move { path } if include_moves => match mouse {
                    MouseReplay::Relative => {
                        for &PathPoint(dt, x, y) in path {
                            let (dx, dy) = last.map_or((0, 0), |(lx, ly)| (x - lx, y - ly));
                            last = Some((x, y));
                            if (dx, dy) != (0, 0) {
                                events.push((start + f64::from(dt), InputEvent::MoveBy { dx, dy }));
                            }
                        }
                    }
                    _ => {
                        events.extend(
                            path.iter()
                                .map(|&PathPoint(dt, x, y)| (start + f64::from(dt), InputEvent::MoveTo { x, y })),
                        );
                    }
                },
                StrokeInput::Look { path } if include_moves && mouse != MouseReplay::Absolute => {
                    events.extend(
                        path.iter()
                            .map(|&LookPoint(dt, dx, dy)| (start + f64::from(dt), InputEvent::MoveBy { dx, dy })),
                    );
                }
                StrokeInput::Move { .. } | StrokeInput::Look { .. } => {}
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

    /// Converts movement recorded in a game that re-centres the pointer into
    /// relative motion, and makes clicks during it happen where the pointer is.
    ///
    /// A re-centring game shows up in a recording as positions that keep
    /// jumping back around one spot instead of travelling smoothly; each
    /// sample is then really "how far the mouse moved from the centre".
    /// Returns how many movements were converted.
    pub fn make_smart(&mut self) -> usize {
        // Games re-centre on the middle of the screen (or of their window).
        // The screen's middle is used when the samples sit around it;
        // otherwise their median is the best guess.
        let screen_center = self.screen.map(|(w, h)| ((w / 2) as i32, (h / 2) as i32));
        let mut locked = Vec::new();
        for stroke in &mut self.strokes {
            let StrokeInput::Move { path } = &stroke.input else { continue };
            if path.len() < 8 {
                continue;
            }
            let mut xs: Vec<i32> = path.iter().map(|p| p.1).collect();
            let mut ys: Vec<i32> = path.iter().map(|p| p.2).collect();
            xs.sort_unstable();
            ys.sort_unstable();
            let distance = |a: (i32, i32), b: (i32, i32)| f64::from((a.0 - b.0).pow(2) + (a.1 - b.1).pow(2)).sqrt();
            let median = (xs[xs.len() / 2], ys[ys.len() / 2]);
            let center = screen_center.filter(|c| distance(*c, median) < 250.0).unwrap_or(median);
            let spread = path.iter().map(|p| distance((p.1, p.2), median)).sum::<f64>() / path.len() as f64;
            let step = path.windows(2).map(|w| distance((w[0].1, w[0].2), (w[1].1, w[1].2))).sum::<f64>()
                / (path.len() - 1) as f64;
            // A smooth path wanders far compared with each step; re-centred
            // samples stay about one step away from the centre.
            if step <= 0.0 || spread / step > 3.0 {
                continue;
            }
            let look = path.iter().map(|p| LookPoint(p.0, p.1 - center.0, p.2 - center.1)).collect();
            stroke.input = StrokeInput::Look { path: look };
            locked.push((stroke.at, stroke.at + stroke.hold));
        }
        mark_clicks_here(&mut self.strokes, &locked);
        locked.len()
    }

    /// Short description such as `14 keys · 6 clicks · 3 moves`.
    pub fn summary(&self) -> String {
        let (mut keys, mut clicks, mut moves, mut looks, mut scrolls, mut pad) = (0, 0, 0, 0, 0, 0);
        for stroke in &self.strokes {
            match stroke.input {
                StrokeInput::Key { .. } => keys += 1,
                StrokeInput::Mouse { .. } => clicks += 1,
                StrokeInput::Move { .. } => moves += 1,
                StrokeInput::Look { .. } => looks += 1,
                StrokeInput::Scroll { .. } => scrolls += 1,
                StrokeInput::PadButton { .. } | StrokeInput::PadAxis { .. } => pad += 1,
            }
        }
        let parts: Vec<String> = [
            (keys, "key", "keys"),
            (clicks, "click", "clicks"),
            (moves, "move", "moves"),
            (looks, "look", "looks"),
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

/// Clicks inside a stretch where the pointer was locked happen where the pointer is.
fn mark_clicks_here(strokes: &mut [Stroke], locked: &[(f64, f64)]) {
    for stroke in strokes {
        if let StrokeInput::Mouse { here, .. } = &mut stroke.input
            && locked.iter().any(|(a, b)| stroke.at >= a - PATH_GAP_MS && stroke.at <= b + PATH_GAP_MS)
        {
            *here = true;
        }
    }
}

/// How pointer movement is recorded.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseRecording {
    /// Record both, and keep relative motion only where the pointer was
    /// locked (a game camera). Needs a system that reports relative motion;
    /// elsewhere this is the same as `Positions`.
    #[default]
    Smart,
    /// Screen positions only.
    Positions,
    /// Relative motion only.
    Relative,
}

impl MouseRecording {
    pub const ALL: [MouseRecording; 3] = [MouseRecording::Smart, MouseRecording::Positions, MouseRecording::Relative];

    pub fn label(self) -> &'static str {
        match self {
            MouseRecording::Smart => "Smart (detects games that lock the pointer)",
            MouseRecording::Positions => "Screen positions",
            MouseRecording::Relative => "Relative motion",
        }
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
    pub mouse_mode: MouseRecording,
}

impl Default for RecordOptions {
    fn default() -> Self {
        Self {
            keyboard: true,
            mouse_buttons: true,
            mouse_moves: true,
            scroll: true,
            gamepad: true,
            trim_start: true,
            mouse_mode: MouseRecording::Smart,
        }
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
    /// The look stroke currently being extended, and when it was last extended.
    look: Option<(usize, f64)>,
    /// Relative motion not yet written: time of the last sample, summed motion.
    look_pending: Option<(f64, i32, i32)>,
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
            look: None,
            look_pending: None,
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
            InputEvent::MoveBy { dx, dy } => self.on_look(t, dx, dy),
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
            let here = self.options.mouse_mode == MouseRecording::Relative;
            let input = match button {
                Button::Key(key) => StrokeInput::Key { key },
                Button::Mouse(button) => {
                    StrokeInput::Mouse { button, x: self.cursor.x, y: self.cursor.y, end: None, here }
                }
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
        if let StrokeInput::Mouse { x, y, end, here: false, .. } = &mut stroke.input
            && (*x, *y) != (cursor.x, cursor.y)
        {
            *end = Some(cursor);
        }
    }

    fn on_move(&mut self, t: f64, x: i32, y: i32) {
        // Always track the pointer: clicks need a position even if moves are not recorded.
        self.cursor = Point::new(x, y);
        if !self.options.mouse_moves || self.options.mouse_mode == MouseRecording::Relative {
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

    /// Relative motion is summed rather than thinned: dropping a sample would
    /// lose movement, not just detail.
    fn on_look(&mut self, t: f64, dx: i32, dy: i32) {
        if !self.options.mouse_moves || self.options.mouse_mode == MouseRecording::Positions {
            return;
        }
        let (since, sum_x, sum_y) = self.look_pending.unwrap_or((t, 0, 0));
        self.look_pending = Some((t, sum_x + dx, sum_y + dy));
        if t - since < MIN_PATH_SPACING_MS && self.look.is_some_and(|(_, last)| t - last <= PATH_GAP_MS) {
            return;
        }
        self.flush_look();
    }

    fn flush_look(&mut self) {
        let Some((t, dx, dy)) = self.look_pending.take() else { return };
        match self.look {
            Some((index, last)) if t - last <= PATH_GAP_MS => {
                let stroke = &mut self.strokes[index];
                stroke.hold = t - stroke.at;
                if let StrokeInput::Look { path } = &mut stroke.input {
                    path.push(LookPoint(stroke.hold as f32, dx, dy));
                }
                self.look = Some((index, t));
            }
            _ => {
                self.look = Some((self.strokes.len(), t));
                self.strokes.push(Stroke {
                    at: t,
                    hold: 0.0,
                    input: StrokeInput::Look { path: vec![LookPoint(0.0, dx, dy)] },
                });
            }
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
        self.flush_look();
        for (_, index) in std::mem::take(&mut self.open) {
            self.close(index, t);
        }
        if self.options.mouse_mode == MouseRecording::Smart {
            resolve_smart(&mut self.strokes);
        }
        let mut recording = Recording { duration_ms: t.max(0.0), screen, strokes: self.strokes };
        recording.normalize();
        if self.options.trim_start {
            recording.trim_start();
        }
        recording
    }
}

/// For each stretch of relative motion, decides whether the pointer really
/// moved across the screen (keep positions) or was held in place by a game
/// (keep relative motion), and drops the other.
fn resolve_smart(strokes: &mut Vec<Stroke>) {
    let span = |s: &Stroke| (s.at, s.at + s.hold);
    let overlaps = |a: (f64, f64), b: (f64, f64)| a.0 <= b.1 + PATH_GAP_MS && b.0 <= a.1 + PATH_GAP_MS;
    let path_length = |strokes: &[Stroke], window: (f64, f64)| -> f64 {
        strokes
            .iter()
            .filter(|s| overlaps(span(s), window))
            .filter_map(|s| match &s.input {
                StrokeInput::Move { path } => Some(path),
                _ => None,
            })
            .map(|path| {
                path.windows(2)
                    .map(|w| f64::from((w[1].1 - w[0].1).pow(2) + (w[1].2 - w[0].2).pow(2)).sqrt())
                    .sum::<f64>()
            })
            .sum()
    };

    let mut locked = Vec::new();
    let mut unlocked = Vec::new();
    for stroke in strokes.iter() {
        let StrokeInput::Look { path } = &stroke.input else { continue };
        let travel: f64 = path.iter().map(|p| f64::from(p.1 * p.1 + p.2 * p.2).sqrt()).sum();
        let window = span(stroke);
        // The pointer moved much less on screen than the mouse did: something held it.
        if travel >= MIN_LOOK_TRAVEL && path_length(strokes, window) < 0.5 * travel {
            locked.push(window);
        } else {
            unlocked.push(window);
        }
    }
    strokes.retain(|s| match &s.input {
        StrokeInput::Look { .. } => locked.contains(&span(s)),
        StrokeInput::Move { .. } => !locked.iter().any(|w| overlaps(span(s), *w)),
        _ => true,
    });
    mark_clicks_here(strokes, &locked);
    let _ = unlocked;
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
            StrokeInput::Mouse { button: MouseButton::Left, x: 10, y: 10, end: Some(Point::new(50, 60)), here: false }
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

    /// The pointer stays put (a game holds it) while the mouse moves a lot.
    fn locked_session(mode: MouseRecording) -> Recording {
        let options = RecordOptions { trim_start: false, mouse_mode: mode, ..RecordOptions::default() };
        let mut b = RecordingBuilder::new(options, vec![], Point::new(960, 540));
        for i in 0..60 {
            let t = f64::from(i) * 2.0;
            b.push(t, &InputEvent::MoveBy { dx: 3, dy: -1 });
            b.push(t, &InputEvent::MoveTo { x: 960 + (i % 2), y: 540 });
        }
        b.push(130.0, &InputEvent::ButtonDown { button: MouseButton::Left });
        b.push(160.0, &InputEvent::ButtonUp { button: MouseButton::Left });
        b.finish(200.0, None)
    }

    #[test]
    fn smart_recording_keeps_relative_motion_where_the_pointer_was_locked() {
        let rec = locked_session(MouseRecording::Smart);
        assert!(rec.strokes.iter().any(|s| matches!(s.input, StrokeInput::Look { .. })));
        assert!(!rec.strokes.iter().any(|s| matches!(s.input, StrokeInput::Move { .. })));
        let total: i32 = rec
            .strokes
            .iter()
            .filter_map(|s| match &s.input {
                StrokeInput::Look { path } => Some(path.iter().map(|p| p.1).sum::<i32>()),
                _ => None,
            })
            .sum();
        assert_eq!(total, 180, "no relative motion is lost to thinning");
        let click = rec.strokes.iter().find(|s| matches!(s.input, StrokeInput::Mouse { .. })).unwrap();
        assert!(matches!(click.input, StrokeInput::Mouse { here: true, .. }), "clicks do not snap the camera");

        let timeline = rec.timeline(true);
        assert!(timeline.iter().any(|(_, e)| matches!(e, InputEvent::MoveBy { .. })));
        assert!(!timeline.iter().any(|(_, e)| matches!(e, InputEvent::MoveTo { .. })));
    }

    #[test]
    fn smart_recording_keeps_positions_for_ordinary_pointer_use() {
        let options = RecordOptions { trim_start: false, ..RecordOptions::default() };
        let mut b = RecordingBuilder::new(options, vec![], Point::new(0, 0));
        for i in 0..60 {
            let t = f64::from(i) * 2.0;
            b.push(t, &InputEvent::MoveBy { dx: 3, dy: 0 });
            b.push(t, &InputEvent::MoveTo { x: 3 * (i + 1), y: 0 });
        }
        let rec = b.finish(200.0, None);
        assert!(rec.strokes.iter().all(|s| !matches!(s.input, StrokeInput::Look { .. })));
        assert!(rec.strokes.iter().any(|s| matches!(s.input, StrokeInput::Move { .. })));
    }

    #[test]
    fn existing_recordings_can_be_made_smart() {
        // Recorded in "positions" mode inside a game that re-centres the pointer.
        let mut rec = Recording {
            duration_ms: 100.0,
            screen: Some((1920, 1080)),
            strokes: vec![Stroke {
                at: 0.0,
                hold: 50.0,
                input: StrokeInput::Move {
                    path: (0..20).map(|i| PathPoint(i as f32 * 2.5, 960 + 4 + (i % 3), 540 - 2)).collect(),
                },
            }],
        };
        assert_eq!(rec.make_smart(), 1);
        let StrokeInput::Look { path } = &rec.strokes[0].input else { panic!("converted to relative motion") };
        assert!(path.iter().all(|p| p.1 >= 3 && p.1 <= 6 && p.2 == -2), "{path:?}");

        // A normal drag across the screen is left alone.
        let mut normal = Recording {
            duration_ms: 100.0,
            screen: None,
            strokes: vec![Stroke {
                at: 0.0,
                hold: 50.0,
                input: StrokeInput::Move { path: (0..20).map(|i| PathPoint(i as f32, i * 10, i * 5)).collect() },
            }],
        };
        assert_eq!(normal.make_smart(), 0);
    }

    #[test]
    fn relative_replay_turns_positions_into_motion() {
        let rec = Recording {
            duration_ms: 10.0,
            screen: None,
            strokes: vec![Stroke {
                at: 0.0,
                hold: 4.0,
                input: StrokeInput::Move {
                    path: vec![PathPoint(0.0, 100, 100), PathPoint(2.0, 110, 95), PathPoint(4.0, 112, 95)],
                },
            }],
        };
        let events: Vec<InputEvent> =
            rec.timeline_with(true, MouseReplay::Relative).into_iter().map(|(_, e)| e).collect();
        assert_eq!(events, vec![InputEvent::MoveBy { dx: 10, dy: -5 }, InputEvent::MoveBy { dx: 2, dy: 0 }]);
    }

    #[test]
    fn cutting_keeps_the_middle_and_shortens_inputs_held_across_the_cut() {
        let mut rec = Recording {
            duration_ms: 1000.0,
            screen: None,
            strokes: vec![
                Stroke { at: 100.0, hold: 50.0, input: StrokeInput::Key { key: Key::KeyA } },
                Stroke { at: 300.0, hold: 400.0, input: StrokeInput::Key { key: Key::KeyB } },
                Stroke { at: 900.0, hold: 10.0, input: StrokeInput::Key { key: Key::KeyC } },
            ],
        };
        rec.cut(200.0, 500.0);
        assert_eq!(rec.strokes.len(), 1);
        assert_eq!((rec.strokes[0].at, rec.strokes[0].hold), (100.0, 200.0));
        assert_eq!(rec.duration_ms, 300.0);
    }
}

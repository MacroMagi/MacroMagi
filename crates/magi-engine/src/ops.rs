//! Device operations: the things a macro does to, or reads from, the machine.
//!
//! Both engines funnel through this module. The native interpreter calls these
//! functions directly; the Python runtime calls them over its pipe. Having one
//! implementation is what guarantees a macro clicks, types and sees exactly
//! the same way whichever engine runs it.

use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use magi_core::action::{Action, PressMode};
use magi_core::autoclicker::{ClickLimit, ClickPosition, ClickTarget, ClickerSettings, DualMode, MULTI_CLICK_GAP_MS};
use magi_core::geom::{Point, Rect};
use magi_core::graph::NodeId;
use magi_core::host::{Host, WindowInfo, WindowQuery};
use magi_core::input::{Button, InputEvent, Key};
use magi_core::nodes::input::{ActionParams, PlayRecordingParams};
use magi_core::nodes::vision::{FindColorParams, FindImageParams};
use magi_core::nodes::window::{FocusWindowParams, ScreenKind};
use magi_core::options::{ProcessRules, RuleAction, Verdict};
use magi_core::text::parse_hex_color;
use magi_vision::ImageRef;

use crate::monitor::{LogLevel, Monitor};
use crate::program::Program;
use crate::timing::{StopToken, millis};
use crate::{EngineError, Halt, OpResult};

/// Time between pointer updates while gliding.
const GLIDE_STEP: Duration = Duration::from_millis(8);
/// How long a "which program is in front" answer is reused.
const FOREGROUND_CACHE: Duration = Duration::from_millis(25);
/// How often a paused run checks whether it may continue.
const PAUSE_POLL: Duration = Duration::from_millis(150);
/// How often Switch to Window looks again for a window that is not there yet.
const WINDOW_POLL: Duration = Duration::from_millis(200);

/// Shared, live-editable process rules. The app owns them; every run reads them.
pub type SharedRules = Arc<RwLock<ProcessRules>>;

/// Result of looking for something on screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sighting {
    pub found: bool,
    /// Where it is, in screen coordinates. `None` when not found.
    pub position: Option<Point>,
    /// Similarity of the best candidate, found or not.
    pub score: f64,
}

impl Sighting {
    /// The outputs of a vision block, as wired into other blocks.
    pub fn to_json(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut outputs = serde_json::Map::new();
        outputs.insert("found".into(), serde_json::json!(self.found));
        outputs.insert("position".into(), serde_json::json!(self.position));
        outputs.insert("score".into(), serde_json::json!(self.score));
        outputs
    }
}

/// Everything one run needs to act on the world.
pub struct RunCx {
    pub host: Arc<dyn Host>,
    pub program: Arc<Program>,
    pub monitor: Arc<Monitor>,
    pub stop: StopToken,
    /// Asks the owning session to stop every run, not just this one.
    pub stop_everything: Arc<dyn Fn() + Send + Sync>,
    /// Keys and buttons this run currently holds down.
    held: Vec<Button>,
    rng: fastrand::Rng,
    warned_gamepad: bool,
    /// Set whenever the run waited; lets the interpreter spot busy loops.
    pub(crate) waited: bool,
    /// Which programs may receive input.
    rules: Option<SharedRules>,
    /// The program input last went to, and when the program in front was last checked.
    last_allowed: Option<String>,
    checked_at: Option<Instant>,
}

impl RunCx {
    pub fn new(
        host: Arc<dyn Host>,
        program: Arc<Program>,
        monitor: Arc<Monitor>,
        stop: StopToken,
        stop_everything: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        Self {
            host,
            program,
            monitor,
            stop,
            stop_everything,
            held: Vec::new(),
            rng: fastrand::Rng::new(),
            warned_gamepad: false,
            waited: false,
            rules: None,
            last_allowed: None,
            checked_at: None,
        }
    }

    /// Makes this run respect process rules.
    pub fn with_rules(mut self, rules: Option<SharedRules>) -> Self {
        self.rules = rules;
        self
    }

    /// Checks the process rules before input is sent. Stops the run, or waits
    /// until an allowed program is in front again, as the rule says.
    fn guard(&mut self) -> OpResult<()> {
        let Some(rules) = self.rules.clone() else { return Ok(()) };
        if !rules.read().map(|r| r.enabled).unwrap_or(false) {
            return Ok(());
        }
        if self.checked_at.is_some_and(|at| at.elapsed() < FOREGROUND_CACHE) {
            return Ok(());
        }
        let mut paused = false;
        loop {
            let foreground = self.host.foreground_process();
            let verdict = match rules.read() {
                Ok(rules) => rules.verdict(foreground.as_deref(), self.last_allowed.as_deref()),
                Err(_) => Verdict::Allowed,
            };
            let name = foreground.clone().unwrap_or_else(|| "this program".into());
            match verdict {
                Verdict::Allowed => {
                    if paused {
                        self.monitor.info(format!("Resumed: {name} is in front."));
                    }
                    self.last_allowed = foreground;
                    self.checked_at = Some(Instant::now());
                    return Ok(());
                }
                Verdict::Denied(RuleAction::Stop) => {
                    self.monitor.warn(format!("Stopped: input to {name} is not allowed by the process rules."));
                    (self.stop_everything)();
                    return Err(Halt::Stopped);
                }
                Verdict::Denied(RuleAction::Pause) => {
                    if !paused {
                        paused = true;
                        self.monitor.info(format!("Paused while {name} is in front."));
                    }
                    self.sleep(PAUSE_POLL)?;
                }
            }
        }
    }

    pub fn check_stop(&self) -> OpResult<()> {
        if self.stop.is_stopped() { Err(Halt::Stopped) } else { Ok(()) }
    }

    pub fn log(&self, level: LogLevel, node: Option<NodeId>, message: impl Into<String>) {
        self.monitor.log(level, node, message);
    }

    /// Sends one event, remembering what is held so it can be released later.
    pub fn send(&mut self, event: &InputEvent) -> OpResult<()> {
        // Letting go is always allowed; anything else must pass the process rules.
        let release =
            matches!(event, InputEvent::KeyUp { .. } | InputEvent::ButtonUp { .. } | InputEvent::PadButtonUp { .. });
        if !release {
            self.guard()?;
        }
        self.host.send(event).map_err(EngineError::from)?;
        if let Some((button, is_down)) = event.button_edge() {
            self.held.retain(|held| *held != button);
            if is_down {
                self.held.push(button);
            }
        }
        Ok(())
    }

    /// Releases everything this run still holds. Called when a run ends for
    /// any reason, so a stopped macro never leaves a key stuck down.
    pub fn release_all(&mut self) {
        for button in std::mem::take(&mut self.held).into_iter().rev() {
            let event = match button {
                Button::Key(key) => InputEvent::KeyUp { key },
                Button::Mouse(button) => InputEvent::ButtonUp { button },
                Button::Pad(button) => InputEvent::PadButtonUp { button },
            };
            let _ = self.host.send(&event);
        }
    }

    pub fn sleep(&mut self, duration: Duration) -> OpResult<()> {
        self.waited = true;
        if self.stop.sleep(duration) { Ok(()) } else { Err(Halt::Stopped) }
    }

    pub fn sleep_ms(&mut self, ms: f64) -> OpResult<()> {
        self.sleep(millis(ms))
    }

    /// `ms` varied randomly by up to `jitter_ms` either way, never negative.
    pub fn jittered(&mut self, ms: f64, jitter_ms: f64) -> f64 {
        if jitter_ms <= 0.0 {
            return ms.max(0.0);
        }
        (ms + (self.rng.f64() * 2.0 - 1.0) * jitter_ms).max(0.0)
    }

    /// True with the given probability.
    pub fn chance(&mut self, probability: f64) -> bool {
        self.rng.f64() < probability
    }

    /// Performs a set of simultaneous actions (an **Input** node).
    ///
    /// `position`, when given, is where the pointer goes first; it overrides
    /// any fixed position stored in a mouse action.
    pub fn perform_actions(&mut self, params: &ActionParams, position: Option<Point>) -> OpResult<()> {
        if let Some(p) = position {
            self.send(&InputEvent::MoveTo { x: p.x, y: p.y })?;
        }
        let hold = millis(params.hold_ms);
        // Events that end the taps, in the order the taps began.
        let mut releases: Vec<InputEvent> = Vec::new();
        let mut extra_clicks = Vec::new();

        let pressed = self.press_all(params, position.is_some(), &mut releases, &mut extra_clicks);
        let held = if releases.is_empty() || pressed.is_err() { Ok(()) } else { self.sleep(hold) };
        // Whatever happened above, let go of what was pressed – in reverse order.
        for event in releases.iter().rev() {
            self.send(event)?;
        }
        pressed?;
        held?;

        for (button, clicks) in extra_clicks {
            for _ in 1..clicks {
                self.sleep_ms(MULTI_CLICK_GAP_MS)?;
                self.send(&InputEvent::ButtonDown { button })?;
                let held = self.sleep(hold);
                self.send(&InputEvent::ButtonUp { button })?;
                held?;
            }
        }
        Ok(())
    }

    fn press_all(
        &mut self,
        params: &ActionParams,
        position_overridden: bool,
        releases: &mut Vec<InputEvent>,
        extra_clicks: &mut Vec<(magi_core::input::MouseButton, u32)>,
    ) -> OpResult<()> {
        for action in &params.actions {
            match action {
                Action::Key { key, mode } => match mode {
                    PressMode::Tap => {
                        self.send(&InputEvent::KeyDown { key: *key })?;
                        releases.push(InputEvent::KeyUp { key: *key });
                    }
                    PressMode::Press => self.send(&InputEvent::KeyDown { key: *key })?,
                    PressMode::Release => self.send(&InputEvent::KeyUp { key: *key })?,
                },
                Action::Mouse { button, mode, clicks, at } => {
                    if let (Some(p), false) = (at, position_overridden) {
                        self.send(&InputEvent::MoveTo { x: p.x, y: p.y })?;
                    }
                    match mode {
                        PressMode::Tap => {
                            self.send(&InputEvent::ButtonDown { button: *button })?;
                            releases.push(InputEvent::ButtonUp { button: *button });
                            if *clicks > 1 {
                                extra_clicks.push((*button, (*clicks).min(10)));
                            }
                        }
                        PressMode::Press => self.send(&InputEvent::ButtonDown { button: *button })?,
                        PressMode::Release => self.send(&InputEvent::ButtonUp { button: *button })?,
                    }
                }
                Action::Move { x, y, relative, duration_ms } => {
                    self.move_pointer(*x, *y, *relative, millis(*duration_ms as f64))?;
                }
                Action::Scroll { dx, dy } => self.send(&InputEvent::Scroll { dx: *dx, dy: *dy })?,
                Action::Text { text } => self.send(&InputEvent::Text { text: text.clone() })?,
            }
        }
        Ok(())
    }

    /// Moves the pointer, either at once or gliding over `duration`.
    pub fn move_pointer(&mut self, x: i32, y: i32, relative: bool, duration: Duration) -> OpResult<()> {
        if duration.is_zero() {
            let event = if relative { InputEvent::MoveBy { dx: x, dy: y } } else { InputEvent::MoveTo { x, y } };
            return self.send(&event);
        }
        let from = self.host.cursor().map_err(EngineError::from)?;
        let to = if relative { Point::new(from.x + x, from.y + y) } else { Point::new(x, y) };
        let start = Instant::now();
        self.waited = true;
        loop {
            let t = (start.elapsed().as_secs_f64() / duration.as_secs_f64()).min(1.0);
            // Smoothstep: ease in, ease out.
            let eased = t * t * (3.0 - 2.0 * t);
            let lerp = |a: i32, b: i32| (f64::from(a) + f64::from(b - a) * eased).round() as i32;
            self.send(&InputEvent::MoveTo { x: lerp(from.x, to.x), y: lerp(from.y, to.y) })?;
            if t >= 1.0 {
                return Ok(());
            }
            if !self.stop.sleep(GLIDE_STEP) {
                return Err(Halt::Stopped);
            }
        }
    }

    /// Replays a recording (a **Play Recording** node), reproducing the
    /// original press and release times scaled by the playback speed.
    pub fn play_recording(&mut self, params: &PlayRecordingParams) -> OpResult<()> {
        if params.recording.is_empty() {
            return Err(EngineError::new("no recording has been chosen").into());
        }
        let program = Arc::clone(&self.program);
        let recording = program
            .assets
            .recording(&params.recording)
            .ok_or_else(|| EngineError::new(format!("recording `{}` is missing", params.recording)))?;
        let speed = if params.speed.is_finite() { params.speed.clamp(0.01, 100.0) } else { 1.0 };
        let at = |ms: f64| Duration::from_secs_f64((ms / speed / 1000.0).max(0.0));

        self.waited = true;
        let start = Instant::now();
        for (time, event) in recording.timeline_with(params.moves, params.mouse) {
            if !self.stop.sleep_until(start + at(time)) {
                return Err(Halt::Stopped);
            }
            if event.is_gamepad() {
                if !self.warned_gamepad {
                    self.warned_gamepad = true;
                    self.monitor.warn(
                        "This recording contains gamepad input, which cannot be replayed yet; it is being skipped.",
                    );
                }
                continue;
            }
            self.send(&event)?;
        }
        // Honour trailing idle time, so looping a recording keeps its rhythm.
        if !self.stop.sleep_until(start + at(recording.duration_ms)) {
            return Err(Halt::Stopped);
        }
        Ok(())
    }

    // ---- Auto Click -------------------------------------------------------

    /// Clicks at the configured speed until a limit is reached or the run is stopped.
    ///
    /// With course correction the clicks follow a fixed schedule measured from
    /// the start: a click that went out late makes the next wait shorter, so
    /// timer and input delays do not add up into a slower speed. If the
    /// schedule falls far behind (the system stalled), it is restarted rather
    /// than caught up with a burst of clicks.
    pub fn autoclick(&mut self, settings: &ClickerSettings) -> OpResult<()> {
        let started = Instant::now();
        let time_limit = settings.time_limit.get().map(|ms| millis(*ms));
        let count_limit = match settings.limit {
            ClickLimit::Count { count } => Some(count),
            ClickLimit::Forever => None,
        };
        let guarding = settings.corner_stop.on || settings.edge_stop.on;
        let screens = if guarding { self.host.screens().unwrap_or_default() } else { Vec::new() };
        if settings.is_capped() {
            self.monitor.info(format!(
                "Clicking at the maximum of {} per second. Turn on high-speed mode for more.",
                settings.max_cps()
            ));
        }

        let mut due = Instant::now();
        let mut done = 0u64;
        let mut alternate = false;
        self.waited = true;
        loop {
            self.check_stop()?;
            if count_limit.is_some_and(|limit| done >= limit) {
                return Ok(());
            }
            if time_limit.is_some_and(|limit| started.elapsed() >= limit) {
                self.monitor.info("Stopped: the time limit was reached.");
                return Ok(());
            }
            if guarding
                && let Ok(pointer) = self.host.cursor()
                && let Some(reason) = safety_stop(settings, &screens, pointer)
            {
                self.monitor.info(format!("Stopped: the pointer reached {reason}."));
                return Ok(());
            }

            let interval = settings.next_interval_ms(self.rng.f64());
            let hold = settings.hold_for(interval, self.rng.f64());
            self.click_once(settings, hold, &mut alternate)?;
            done += 1;
            self.monitor.add_clicks(u64::from(settings.effective_clicks()));

            if settings.course_correction {
                due += millis(interval);
                let now = Instant::now();
                if now > due + millis(interval.max(50.0)) {
                    due = now; // fell far behind: restart the schedule
                }
                if !self.stop.sleep_until(due) {
                    return Err(Halt::Stopped);
                }
            } else {
                let clicks = f64::from(settings.effective_clicks());
                let busy = hold * clicks + settings.double_click_delay_ms.max(0.0) * (clicks - 1.0);
                self.sleep_ms((interval - busy).max(0.0))?;
            }
        }
    }

    fn click_once(&mut self, settings: &ClickerSettings, hold_ms: f64, alternate: &mut bool) -> OpResult<()> {
        if let ClickPosition::Fixed { x, y } = settings.position {
            self.send(&InputEvent::MoveTo { x, y })?;
        }
        let hold = millis(hold_ms);
        match settings.target {
            ClickTarget::Mouse { button } => {
                for i in 0..settings.effective_clicks() {
                    if i > 0 {
                        self.sleep_ms(settings.double_click_delay_ms)?;
                    }
                    self.send(&InputEvent::ButtonDown { button })?;
                    let held = self.hold(hold);
                    self.send(&InputEvent::ButtonUp { button })?;
                    held?;
                }
            }
            ClickTarget::Key { key } => self.tap_keys(&[key], hold)?,
            ClickTarget::DualKeys { first, second, mode: DualMode::Together } => {
                self.tap_keys(&[first, second], hold)?
            }
            ClickTarget::DualKeys { first, second, mode: DualMode::Alternate } => {
                let key = if *alternate { second } else { first };
                *alternate = !*alternate;
                self.tap_keys(&[key], hold)?;
            }
        }
        Ok(())
    }

    /// Presses keys together, holds, and releases them in reverse order.
    fn tap_keys(&mut self, keys: &[Key], hold: Duration) -> OpResult<()> {
        let mut pressed = Vec::new();
        let mut outcome = Ok(());
        for key in keys {
            if let Err(halt) = self.send(&InputEvent::KeyDown { key: *key }) {
                outcome = Err(halt);
                break;
            }
            pressed.push(*key);
        }
        if outcome.is_ok() {
            outcome = self.hold(hold);
        }
        for key in pressed.into_iter().rev() {
            self.send(&InputEvent::KeyUp { key })?;
        }
        outcome
    }

    /// Waits while something is held. Very short holds are not slept at all.
    fn hold(&mut self, duration: Duration) -> OpResult<()> {
        if duration < Duration::from_micros(50) { self.check_stop() } else { self.sleep(duration) }
    }

    // ---- windows ------------------------------------------------------------

    /// Brings a window to the front (a **Switch to Window** block). `Ok(None)`
    /// if no matching window appeared within the block's time limit.
    pub fn focus_window(&mut self, params: &FocusWindowParams) -> OpResult<Option<WindowInfo>> {
        if params.target.trim().is_empty() {
            return Err(EngineError::new("no window has been chosen").into());
        }
        let query = WindowQuery { text: params.target.trim().to_owned(), by: params.match_by };
        let deadline = Instant::now() + millis(params.timeout_ms);
        loop {
            self.check_stop()?;
            if let Some(info) = self.host.focus_window(&query).map_err(EngineError::from)? {
                let kind = if params.screen == ScreenKind::Smart { info.kind } else { params.screen };
                // Give the program time to come back, especially from exclusive fullscreen.
                self.sleep_ms(kind.settle_ms())?;
                return Ok(Some(WindowInfo { kind, ..info }));
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            self.sleep(WINDOW_POLL)?;
        }
    }

    /// Looks for an image on screen (an **Image Detection** node).
    pub fn find_image(&self, params: &FindImageParams) -> OpResult<Sighting> {
        let needle = self.program.image(&params.image)?;
        let frame = self.host.capture(params.region).map_err(EngineError::from)?;
        let haystack = ImageRef::new(frame.width, frame.height, &frame.rgba);
        let Some(best) = magi_vision::find_template(haystack, needle.as_ref().as_ref()) else {
            // The image is larger than the area searched: it cannot be there.
            return Ok(Sighting { found: false, position: None, score: 0.0 });
        };
        let score = f64::from(best.score);
        let found = score >= params.threshold.clamp(0.0, 1.0);
        let (cx, cy) = best.center();
        let position = Point::new(frame.origin.x + cx as i32, frame.origin.y + cy as i32);
        Ok(Sighting { found, position: found.then_some(position), score })
    }

    /// Looks for a colour on screen (a **Color Detection** node).
    pub fn find_color(&self, params: &FindColorParams) -> OpResult<Sighting> {
        let rgb = parse_hex_color(&params.color)
            .ok_or_else(|| EngineError::new(format!("`{}` is not a colour like #FF8800", params.color)))?;
        let frame = self.host.capture(params.region).map_err(EngineError::from)?;
        let hit = magi_vision::find_color(ImageRef::new(frame.width, frame.height, &frame.rgba), rgb, params.tolerance);
        Ok(match hit {
            Some(hit) => Sighting {
                found: true,
                position: Some(Point::new(frame.origin.x + hit.x as i32, frame.origin.y + hit.y as i32)),
                score: 1.0,
            },
            None => Sighting { found: false, position: None, score: 0.0 },
        })
    }
}

/// If the pointer is in a stop corner or on a stop edge, says which.
fn safety_stop(settings: &ClickerSettings, screens: &[Rect], p: Point) -> Option<&'static str> {
    let screen = screens.iter().find(|r| r.contains(p)).or_else(|| screens.first())?;
    let (left, top) = (p.x - screen.x, p.y - screen.y);
    let (right, bottom) = (screen.right() - 1 - p.x, screen.bottom() - 1 - p.y);
    let within = |distance: i32, size: u32| distance <= size as i32;
    if let Some(size) = settings.corner_stop.get() {
        let corners = [
            (within(left, size[0]) && within(top, size[0]), "the top-left corner"),
            (within(right, size[1]) && within(top, size[1]), "the top-right corner"),
            (within(left, size[2]) && within(bottom, size[2]), "the bottom-left corner"),
            (within(right, size[3]) && within(bottom, size[3]), "the bottom-right corner"),
        ];
        if let Some((_, name)) = corners.into_iter().find(|(hit, _)| *hit) {
            return Some(name);
        }
    }
    if let Some(size) = settings.edge_stop.get() {
        let edges = [
            (within(top, size[0]), "the top edge"),
            (within(right, size[1]), "the right edge"),
            (within(bottom, size[2]), "the bottom edge"),
            (within(left, size[3]), "the left edge"),
        ];
        if let Some((_, name)) = edges.into_iter().find(|(hit, _)| *hit) {
            return Some(name);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use magi_core::autoclicker::Toggled;

    use super::*;

    #[test]
    fn corners_and_edges_are_detected_on_the_right_screen() {
        let screens = [Rect::new(0, 0, 1920, 1080), Rect::new(1920, 0, 1280, 1024)];
        let mut settings = ClickerSettings::default();
        assert_eq!(safety_stop(&settings, &screens, Point::new(0, 0)), None, "both are off by default");
        settings.corner_stop = Toggled { on: true, value: [25; 4] };
        assert_eq!(safety_stop(&settings, &screens, Point::new(3, 4)), Some("the top-left corner"));
        assert_eq!(safety_stop(&settings, &screens, Point::new(1920 + 1279, 1023)), Some("the bottom-right corner"));
        assert_eq!(safety_stop(&settings, &screens, Point::new(900, 2)), None);
        settings.edge_stop = Toggled { on: true, value: [5; 4] };
        assert_eq!(safety_stop(&settings, &screens, Point::new(900, 2)), Some("the top edge"));
        assert_eq!(safety_stop(&settings, &screens, Point::new(900, 500)), None);
    }
}

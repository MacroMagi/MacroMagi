//! Device operations: the things a macro does to, or reads from, the machine.
//!
//! Both engines funnel through this module. The native interpreter calls these
//! functions directly; the Python runtime calls them over its pipe. Having one
//! implementation is what guarantees a macro clicks, types and sees exactly
//! the same way whichever engine runs it.

use std::sync::Arc;
use std::time::{Duration, Instant};

use magi_core::action::{Action, PressMode};
use magi_core::autoclicker::MULTI_CLICK_GAP_MS;
use magi_core::geom::Point;
use magi_core::graph::NodeId;
use magi_core::host::Host;
use magi_core::input::{Button, InputEvent};
use magi_core::nodes::input::{ActionParams, PlayRecordingParams};
use magi_core::nodes::vision::{FindColorParams, FindImageParams};
use magi_core::text::parse_hex_color;
use magi_vision::ImageRef;

use crate::monitor::{LogLevel, Monitor};
use crate::program::Program;
use crate::timing::{StopToken, millis};
use crate::{EngineError, Halt, OpResult};

/// Time between pointer updates while gliding.
const GLIDE_STEP: Duration = Duration::from_millis(8);

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
        for (time, event) in recording.timeline(params.moves) {
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

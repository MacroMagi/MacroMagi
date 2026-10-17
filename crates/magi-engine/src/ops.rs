//! Device operations: the things a macro does to, or reads from, the machine.
//!
//! Both engines funnel through this module. The native interpreter calls these
//! functions directly; the Python runtime calls them over its pipe. Having one
//! implementation is what guarantees a macro clicks, types and sees exactly
//! the same way whichever engine runs it.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use magi_core::action::Action as InputAction;
use magi_core::action::{Action, PressMode};
use magi_core::autoclicker::{ClickLimit, ClickTarget, ClickerSettings, DualMode, MULTI_CLICK_GAP_MS};
use magi_core::geom::{Displays, Point, Rect};
use magi_core::graph::NodeId;
use magi_core::host::{Frame, Host, Notification, SoundSource, TextWord, WindowInfo, WindowQuery};
use magi_core::input::{Button, InputEvent, Key};
use magi_core::magic::Decision;
use magi_core::nodes::ai::{AiParams, AiWatchParams};
use magi_core::nodes::flow::RateParams;
use magi_core::nodes::input::{ActionParams, AutoTypeParams, NewLines, PlayRecordingParams};
use magi_core::nodes::sound::{SoundLevelParams, SoundMatchParams};
use magi_core::nodes::system::NotifyParams;
use magi_core::nodes::variables::{CounterParams, SetVariableParams};
use magi_core::nodes::vision::{FindColorParams, FindImageParams, FindMode, ReadTextParams};
use magi_core::nodes::window::{FocusWindowParams, ScreenKind};
use magi_core::options::{ProcessRules, RuleAction, Verdict};
use magi_core::text::parse_hex_color;
use magi_core::variables::{as_number, as_text};
use magi_vision::ImageRef;
use rayon::prelude::*;
use serde_json::Value;

use crate::monitor::{LogLevel, Monitor};
use crate::program::Program;
use crate::timing::{StopToken, millis};
use crate::vars::Variables;
use crate::{EngineError, Halt, OpResult};

/// Time between pointer updates while gliding.
const GLIDE_STEP: Duration = Duration::from_millis(8);
/// How long a "which program is in front" answer is reused.
const FOREGROUND_CACHE: Duration = Duration::from_millis(25);
/// How often a paused run checks whether it may continue.
const PAUSE_POLL: Duration = Duration::from_millis(150);
/// How often Switch to Window looks again for a window that is not there yet.
const WINDOW_POLL: Duration = Duration::from_millis(200);
/// How long the list of displays is reused before it is read again.
const DISPLAYS_CACHE: Duration = Duration::from_secs(2);
/// Sound Recognition listens to this much more than the sound's own length,
/// so a sound is not missed between two looks.
const LISTEN_SLACK_MS: f64 = 1000.0;

/// A sound source as a run listens to it.
#[derive(Default)]
struct Stream {
    listening: magi_audio::Listening,
    /// When sound was last taken from the source.
    fed_at: Option<Instant>,
}

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

/// Result of looking for several images at once (an Image Detection block).
#[derive(Clone, Debug, PartialEq)]
pub struct ImagesSighting {
    pub found: bool,
    /// Where the image reported is, in desktop coordinates.
    pub position: Option<Point>,
    pub score: f64,
    /// The name of the image reported, and its group.
    pub name: String,
    pub group: String,
    /// How many of the images are on screen.
    pub count: usize,
}

impl ImagesSighting {
    pub fn to_json(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut outputs = serde_json::Map::new();
        outputs.insert("found".into(), serde_json::json!(self.found));
        outputs.insert("position".into(), serde_json::json!(self.position));
        outputs.insert("score".into(), serde_json::json!(self.score));
        outputs.insert("name".into(), serde_json::json!(self.name));
        outputs.insert("group".into(), serde_json::json!(self.group));
        outputs.insert("count".into(), serde_json::json!(self.count));
        outputs
    }
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

/// The words where `find` is written, on one line. Spaces do not matter (text
/// recognition does not always place them), and case only when `match_case`.
fn find_words<'a>(words: &'a [TextWord], find: &str, match_case: bool) -> Option<Vec<&'a TextWord>> {
    let fold = |c: char| if match_case { vec![c] } else { c.to_lowercase().collect() };
    let needle: Vec<char> = find.chars().filter(|c| !c.is_whitespace()).flat_map(fold).collect();
    if needle.is_empty() {
        return None;
    }
    let lines = words.iter().map(|w| w.line).max().map_or(0, |last| last + 1);
    for line in 0..lines {
        let on_line: Vec<&TextWord> = words.iter().filter(|w| w.line == line).collect();
        // The line's letters, each with the word it belongs to.
        let mut letters: Vec<(char, usize)> = Vec::new();
        for (index, word) in on_line.iter().enumerate() {
            letters.extend(word.text.chars().filter(|c| !c.is_whitespace()).flat_map(fold).map(|c| (c, index)));
        }
        if let Some(start) =
            letters.windows(needle.len()).position(|w| w.iter().map(|(c, _)| *c).eq(needle.iter().copied()))
        {
            let (first, last) = (letters[start].1, letters[start + needle.len() - 1].1);
            return Some(on_line[first..=last].to_vec());
        }
    }
    None
}

/// The first number in a text: `-12`, `3.5`, `3,5` or `1,234`.
fn first_number(text: &str) -> Option<f64> {
    let chars: Vec<char> = text.chars().collect();
    let start = chars.iter().position(char::is_ascii_digit)?;
    let mut end = start;
    while end < chars.len()
        && (chars[end].is_ascii_digit()
            || (matches!(chars[end], '.' | ',') && chars.get(end + 1).is_some_and(char::is_ascii_digit)))
    {
        end += 1;
    }
    let token: String = chars[start..end].iter().collect();
    let negative = start > 0 && matches!(chars[start - 1], '-' | '−');
    let value = match token.rfind(['.', ',']) {
        None => token.parse::<f64>().ok()?,
        Some(at) => {
            let separator = &token[at..=at];
            let (whole, decimals) = (&token[..at], &token[at + 1..]);
            let whole: String = whole.chars().filter(char::is_ascii_digit).collect();
            // "1,234" and "1.234.567" group thousands; "3,5" and "3.25" have decimals.
            let thousands = decimals.len() == 3
                && (token.matches(separator).count() > 1 || (separator == "," && !token.contains('.')));
            if thousands { format!("{whole}{decimals}") } else { format!("{whole}.{decimals}") }.parse::<f64>().ok()?
        }
    };
    Some(if negative { -value } else { value })
}

/// An AI Watch block between the times it acts.
struct WatchState {
    started: Instant,
    hits: u64,
    last: String,
}

/// Where an AI Watch block got to: it acted (run the blocks under "Acted"),
/// or its time is up.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct WatchStep {
    pub acted: bool,
    pub hits: u64,
    pub label: String,
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
    /// The macro's variables.
    pub vars: Variables,
    /// The displays, and when they were read.
    displays: Option<(Instant, Displays)>,
    /// The sound being listened to, analysed as it arrives, per source and microphone.
    streams: HashMap<(SoundSource, String), Stream>,
    /// For each Sound Recognition block and sound, the stream frame where it
    /// was last recognised: what was heard before then has been counted.
    heard_until: HashMap<(NodeId, String), u64>,
    /// The sound each Sound Recognition block recognised last, and its group:
    /// what its "Which sound" output says until it recognises another.
    last_heard: HashMap<NodeId, (String, String)>,
    /// AI Watch blocks part-way through watching, between the times they act.
    watches: HashMap<NodeId, WatchState>,
}

impl RunCx {
    pub fn new(
        host: Arc<dyn Host>,
        program: Arc<Program>,
        monitor: Arc<Monitor>,
        stop: StopToken,
        stop_everything: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let vars = Variables::new(&program.variables, Arc::clone(&monitor));
        Self {
            host,
            program,
            monitor,
            vars,
            displays: None,
            streams: HashMap::new(),
            heard_until: HashMap::new(),
            last_heard: HashMap::new(),
            watches: HashMap::new(),
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

    /// Makes this run share variables with the other runs of its session.
    pub fn with_variables(mut self, vars: Variables) -> Self {
        self.vars = vars;
        self
    }

    /// A block's settings as it runs them, variables resolved.
    pub fn params_of(&self, node: &magi_core::graph::Node) -> serde_json::Map<String, Value> {
        let vars = self.vars.clone();
        self.program.resolved_params(node, move |name| vars.get(name))
    }

    // ---- displays -----------------------------------------------------------

    /// The displays, read again every couple of seconds.
    pub fn displays(&mut self) -> Displays {
        match &self.displays {
            Some((at, displays)) if at.elapsed() < DISPLAYS_CACHE => displays.clone(),
            _ => {
                let displays = Displays::new(self.host.screens().unwrap_or_default());
                self.displays = Some((Instant::now(), displays.clone()));
                displays
            }
        }
    }

    /// A position as global coordinates.
    pub fn global(&mut self, p: Point) -> Point {
        if p.display.is_none() { p } else { self.displays().to_global(p) }
    }

    /// An area as global coordinates.
    pub fn global_rect(&mut self, r: Rect) -> Rect {
        if r.display.is_none() { r } else { self.displays().rect_to_global(r) }
    }

    /// The area to capture for a search: the search area, or the whole display.
    fn search_area(&mut self, region: Option<Rect>, display: u8) -> Option<Rect> {
        match region {
            Some(region) => Some(self.global_rect(region)),
            None if display == 0 => None,
            None => self.displays().get(display),
        }
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
            let p = self.global(p);
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
                        let p = self.global(*p);
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
                Action::Move { x, y, display, relative, duration_ms } => {
                    let (x, y) = if *relative {
                        (*x, *y)
                    } else {
                        let p = self.global(Point { x: *x, y: *y, display: *display });
                        (p.x, p.y)
                    };
                    self.move_pointer(x, y, *relative, millis(*duration_ms as f64))?;
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
        let displays = self.displays();
        for (time, event) in recording.timeline_with(params.moves, params.mouse, &displays) {
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
        if let Some(p) = settings.position.point() {
            let p = self.global(p);
            self.send(&InputEvent::MoveTo { x: p.x, y: p.y })?;
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

    /// Looks for one or more images on screen (an **Image Detection** node).
    ///
    /// Images that share a search area share one capture of it; each image is
    /// matched at the block's level of detail, all of them in parallel.
    pub fn find_image(&mut self, params: &FindImageParams) -> OpResult<ImagesSighting> {
        let targets = params.targets();
        if targets.is_empty() {
            return Err(EngineError::new("no image has been chosen").into());
        }
        let factor = params.detail.factor();

        // One capture per distinct search area.
        let mut areas: Vec<Option<Rect>> = Vec::new();
        let mut area_of = Vec::with_capacity(targets.len());
        for target in &targets {
            let area = self.search_area(params.region_of(target), params.display);
            let index = areas.iter().position(|a| *a == area).unwrap_or_else(|| {
                areas.push(area);
                areas.len() - 1
            });
            area_of.push(index);
        }
        let mut frames = Vec::with_capacity(areas.len());
        for area in &areas {
            frames.push(self.host.capture(*area).map_err(EngineError::from)?);
        }

        // Each image at the detail it can take: a small image is never
        // shrunk below a few pixels.
        let mut jobs = Vec::with_capacity(targets.len());
        for (target, &area) in targets.iter().zip(&area_of) {
            let full = self.program.image(&target.image)?;
            let fits = (full.width.min(full.height) / 6).max(1);
            let f = factor.min(fits).max(1);
            jobs.push((area, f, self.program.image_shrunk(&target.image, f)?));
        }
        let mut shrunk: HashMap<(usize, u32), magi_vision::Bitmap> = HashMap::new();
        for &(area, f, _) in &jobs {
            if f > 1 && !shrunk.contains_key(&(area, f)) {
                let frame = &frames[area];
                let small = magi_vision::shrink(ImageRef::new(frame.width, frame.height, &frame.rgba), f);
                shrunk.insert((area, f), small);
            }
        }

        let results: Vec<Option<(f64, Point)>> = jobs
            .par_iter()
            .map(|(area, f, needle)| {
                let frame = &frames[*area];
                let haystack = match shrunk.get(&(*area, *f)) {
                    Some(small) => small.as_ref(),
                    None => ImageRef::new(frame.width, frame.height, &frame.rgba),
                };
                let best = magi_vision::find_template(haystack, needle.as_ref().as_ref())?;
                let (cx, cy) = best.center();
                let (x, y) = (cx * f + f / 2, cy * f + f / 2);
                Some((f64::from(best.score), Point::new(frame.origin.x + x as i32, frame.origin.y + y as i32)))
            })
            .collect();

        let threshold = params.threshold.clamp(0.0, 1.0);
        let is_found = |r: &Option<(f64, Point)>| r.is_some_and(|(score, _)| score >= threshold);
        let count = results.iter().filter(|r| is_found(r)).count();
        let found = match params.mode {
            FindMode::Any => count > 0,
            FindMode::All => count == targets.len(),
        };
        // Reported: the first image found, in list order; otherwise the closest miss.
        let reported = results.iter().position(is_found).or_else(|| {
            results
                .iter()
                .enumerate()
                .filter_map(|(i, r)| r.map(|(score, _)| (i, score)))
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i)
        });
        let name_of = |i: usize| self.program.assets.get(&targets[i].image).map(|a| a.name.clone()).unwrap_or_default();
        Ok(match reported {
            Some(i) => {
                let (score, position) = results[i].expect("reported results exist");
                let hit = is_found(&results[i]);
                ImagesSighting {
                    found,
                    position: hit.then_some(position),
                    score,
                    name: if hit { name_of(i) } else { String::new() },
                    group: if hit { targets[i].group.clone() } else { String::new() },
                    count,
                }
            }
            None => ImagesSighting {
                found: false,
                position: None,
                score: 0.0,
                name: String::new(),
                group: String::new(),
                count: 0,
            },
        })
    }

    /// Looks for a color on screen (a **Color Detection** node).
    pub fn find_color(&mut self, params: &FindColorParams) -> OpResult<Sighting> {
        let rgb = parse_hex_color(&params.color)
            .ok_or_else(|| EngineError::new(format!("`{}` is not a color like #FF8800", params.color)))?;
        let area = self.search_area(params.region, params.display);
        let frame = self.host.capture(area).map_err(EngineError::from)?;
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

/// The longest side of a picture the system's text recognition accepts.
const OCR_MAX_SIDE: u32 = 2600;

impl RunCx {
    // ---- text -----------------------------------------------------------------

    /// Types text character by character (an **Auto Typer** block). `wired`
    /// is a value wired into the block, typed instead of its own text.
    /// Returns how many characters were typed.
    pub fn autotype(&mut self, params: &AutoTypeParams, wired: Option<Value>, node: NodeId) -> OpResult<u64> {
        let source = match wired {
            Some(value) => as_text(&value),
            None => params.text.clone(),
        };
        let chars = params.characters(&source);
        if chars.is_empty() {
            self.monitor.log(LogLevel::Info, Some(node), "Nothing to type".to_owned());
        }
        let pause = 1000.0 / params.speed.clamp(0.5, 2000.0);
        let tap = |cx: &mut Self, key: Key| {
            cx.perform_actions(&ActionParams { actions: vec![InputAction::tap(key)], hold_ms: 15.0 }, None)
        };
        let mut typed = 0u64;
        for (index, &c) in chars.iter().enumerate() {
            self.check_stop()?;
            if index > 0 {
                let swing = params.variation.clamp(0.0, 1.0) * (self.rng.f64() * 2.0 - 1.0);
                let mut wait = pause * (1.0 + swing);
                if params.punctuation_pause && matches!(chars[index - 1], '.' | ',' | '!' | '?' | ';' | ':') {
                    wait += pause * 3.0;
                }
                self.sleep_ms(wait.max(0.0))?;
            }
            // Now and then a wrong letter, noticed and taken back.
            if c.is_alphabetic() && params.typos > 0.0 && self.rng.f64() < params.typos.clamp(0.0, 1.0) {
                let mut wrong = char::from(b'a' + self.rng.u8(0..26));
                if wrong == c.to_ascii_lowercase() {
                    wrong = if wrong == 'z' { 'a' } else { char::from(wrong as u8 + 1) };
                }
                let wrong = if c.is_uppercase() { wrong.to_ascii_uppercase() } else { wrong };
                self.send(&InputEvent::Text { text: wrong.to_string() })?;
                self.sleep_ms(pause * 2.0)?;
                tap(self, Key::Backspace)?;
                self.sleep_ms(pause)?;
            }
            match c {
                '\n' => match params.new_lines {
                    NewLines::Enter => tap(self, Key::Enter)?,
                    NewLines::ShiftEnter => {
                        self.send(&InputEvent::KeyDown { key: Key::ShiftLeft })?;
                        tap(self, Key::Enter)?;
                        self.send(&InputEvent::KeyUp { key: Key::ShiftLeft })?;
                    }
                    NewLines::Space => self.send(&InputEvent::Text { text: " ".into() })?,
                    NewLines::Skip => continue,
                },
                '\t' => tap(self, Key::Tab)?,
                c => self.send(&InputEvent::Text { text: c.to_string() })?,
            }
            typed += 1;
        }
        if params.enter_at_end {
            if !chars.is_empty() {
                self.sleep_ms(pause)?;
            }
            tap(self, Key::Enter)?;
        }
        Ok(typed)
    }

    /// Reads the text in an area of the screen (a **Text Scan** block).
    pub fn read_text(&mut self, params: &ReadTextParams) -> OpResult<serde_json::Map<String, Value>> {
        let area = self.search_area(params.region, params.display);
        let frame = self.host.capture(area).map_err(EngineError::from)?;
        let picture = ImageRef::new(frame.width, frame.height, &frame.rgba);
        let longest = frame.width.max(frame.height).max(1);
        // Enlarged as asked, but never past what text recognition accepts; a
        // picture that is too large already is shrunk instead.
        let mut factor = params.enlarge.factor();
        while factor > 1 && longest * factor > OCR_MAX_SIDE {
            factor -= 1;
        }
        let (prepared, scale) = if factor > 1 {
            (magi_vision::enlarge(picture, factor), f64::from(factor))
        } else if longest > OCR_MAX_SIDE {
            let shrink = longest.div_ceil(OCR_MAX_SIDE);
            (magi_vision::shrink(picture, shrink), 1.0 / f64::from(shrink))
        } else {
            (magi_vision::Bitmap { width: frame.width, height: frame.height, rgba: frame.rgba.clone() }, 1.0)
        };
        let words = self
            .host
            .read_text(&Frame::new(frame.origin, prepared.width, prepared.height, prepared.rgba), &params.language)
            .map_err(EngineError::from)?;

        let lines = words.iter().map(|w| w.line).max().map_or(0, |last| last + 1);
        let text = (0..lines)
            .map(|line| words.iter().filter(|w| w.line == line).map(|w| w.text.as_str()).collect::<Vec<_>>().join(" "))
            .collect::<Vec<_>>()
            .join("\n");
        let matched: Option<Vec<&TextWord>> = if params.find.trim().is_empty() {
            (!words.is_empty()).then(|| words.iter().collect())
        } else {
            find_words(&words, &params.find, params.match_case)
        };
        // Where the words found are, on the desktop.
        let position = matched.as_ref().and_then(|found| {
            let (left, top) = found.iter().fold((i32::MAX, i32::MAX), |(l, t), w| (l.min(w.rect.x), t.min(w.rect.y)));
            let (right, bottom) = found.iter().fold((i32::MIN, i32::MIN), |(r, b), w| {
                (r.max(w.rect.x + w.rect.w as i32), b.max(w.rect.y + w.rect.h as i32))
            });
            (left <= right).then(|| {
                let center = |a: i32, b: i32| (f64::from(a + b) / 2.0 / scale).round() as i32;
                Point::new(frame.origin.x + center(left, right), frame.origin.y + center(top, bottom))
            })
        });
        let mut outputs = serde_json::Map::new();
        outputs.insert("found".into(), serde_json::json!(matched.is_some()));
        outputs.insert("text".into(), serde_json::json!(text));
        outputs.insert("position".into(), serde_json::json!(position));
        outputs.insert("number".into(), serde_json::json!(first_number(&text)));
        Ok(outputs)
    }

    // ---- AI models ------------------------------------------------------------

    /// Shows the model's part of the screen to the model once.
    fn ai_look(
        &mut self,
        loaded: &crate::program::LoadedModel,
        config: &magi_core::magic::MagicModel,
        display: u8,
    ) -> OpResult<Decision> {
        let displays = self.displays();
        let rect = config
            .capture
            .rect(&displays, Some(display))
            .ok_or_else(|| EngineError::new("there is no display to look at"))?;
        let frame = self.host.capture(Some(rect)).map_err(EngineError::from)?;
        let probabilities = loaded
            .model
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .classify(&frame.rgba, frame.width, frame.height)
            .map_err(|e| EngineError::new(e.0))?;
        Ok(config.decide(&probabilities))
    }

    /// Asks an AI model about the screen once (an **AI Model** block).
    pub fn ai_classify(&mut self, params: &AiParams) -> OpResult<serde_json::Map<String, Value>> {
        let loaded = self.program.model(&params.model)?;
        let config = loaded.spec.configured(&params.settings);
        let decision = self.ai_look(&loaded, &config, params.display)?;
        let mut outputs = serde_json::Map::new();
        outputs.insert("act".into(), serde_json::json!(decision.act));
        outputs.insert("label".into(), serde_json::json!(decision.label));
        outputs.insert("class".into(), serde_json::json!(decision.class));
        outputs.insert("confidence".into(), serde_json::json!((decision.confidence * 1000.0).round() / 1000.0));
        outputs.insert("delay".into(), serde_json::json!(decision.delay_ms));
        Ok(outputs)
    }

    /// Keeps showing the screen to an AI model and acts on its answers (an
    /// **AI Watch** block). Returns how often it acted and its last answer.
    /// Watches until the model acts (`acted` is then true: the blocks under
    /// "Acted" run, and the block is entered again with `returning`), or until
    /// the time is up. Counts and the last answer carry over between entries.
    pub fn ai_watch(&mut self, params: &AiWatchParams, node: NodeId, returning: bool) -> OpResult<WatchStep> {
        let loaded = self.program.model(&params.model)?;
        let config = loaded.spec.configured(&params.settings);
        if !returning || !self.watches.contains_key(&node) {
            let engine = loaded.model.lock().unwrap_or_else(|p| p.into_inner()).engine();
            self.monitor.log(
                LogLevel::Info,
                Some(node),
                format!("“{}” is watching, on the {}", config.name, engine.label()),
            );
            self.watches.insert(node, WatchState { started: Instant::now(), hits: 0, last: String::new() });
        } else {
            // Back from the blocks under "Acted": let the event pass before looking again.
            self.sleep_ms(config.action.cooldown_ms.max(0.0))?;
        }
        let frame_time = (params.max_fps > 0.0).then(|| Duration::from_secs_f64(1.0 / params.max_fps));
        loop {
            self.check_stop()?;
            let state = &self.watches[&node];
            if params.duration_ms > 0.0 && state.started.elapsed() >= millis(params.duration_ms) {
                let state = self.watches.remove(&node).expect("checked above");
                return Ok(WatchStep { acted: false, hits: state.hits, label: state.last });
            }
            let look_started = Instant::now();
            let decision = self.ai_look(&loaded, &config, params.display)?;
            if decision.act {
                if decision.delay_ms > 0.0 {
                    self.sleep_ms(decision.delay_ms)?;
                }
                if params.press {
                    let press = ActionParams {
                        actions: vec![InputAction::press(config.action.key).ok_or_else(|| {
                            EngineError::new("the model's button is a gamepad button, which cannot be sent")
                        })?],
                        hold_ms: config.action.hold_ms,
                    };
                    self.perform_actions(&press, None)?;
                }
                self.monitor.log(
                    LogLevel::Info,
                    Some(node),
                    format!(
                        "{} “{}” ({:.0}% sure)",
                        if params.press { "Pressed for" } else { "Saw" },
                        decision.label,
                        decision.confidence * 100.0
                    ),
                );
                let state = self.watches.get_mut(&node).expect("set up above");
                state.hits += 1;
                state.last = decision.label;
                return Ok(WatchStep { acted: true, hits: state.hits, label: state.last.clone() });
            } else if let Some(frame_time) = frame_time {
                let spent = look_started.elapsed();
                if spent < frame_time {
                    self.sleep(frame_time - spent)?;
                }
            } else {
                // As fast as possible, but never a busy loop in the interpreter's eyes.
                self.waited = true;
            }
        }
    }

    // ---- sound --------------------------------------------------------------

    /// Listens to the most recent sound (a **Sound Detection** block).
    pub fn sound_level(&mut self, params: &SoundLevelParams) -> OpResult<serde_json::Map<String, Value>> {
        let clip = self
            .host
            .listen(params.source, &params.device, params.window_ms.clamp(20.0, 5000.0))
            .map_err(EngineError::from)?;
        let loudness = magi_audio::loudness_db(&clip.samples);
        let pitch = magi_audio::dominant_frequency(&clip.samples, clip.sample_rate);
        let mut outputs = serde_json::Map::new();
        outputs.insert("detected".into(), serde_json::json!(params.detects(loudness, pitch)));
        outputs.insert("loudness".into(), serde_json::json!((loudness * 10.0).round() / 10.0));
        outputs.insert("pitch".into(), serde_json::json!(pitch.map_or(0.0, f64::round)));
        Ok(outputs)
    }

    /// Listens for one or more sounds (a **Sound Recognition** block). Each
    /// time a sound is played it is recognised once.
    ///
    /// The sound is analysed as a stream: each call analyses only what is new,
    /// and every sound of the block is compared with the same analysis.
    pub fn sound_match(&mut self, params: &SoundMatchParams, node: NodeId) -> OpResult<serde_json::Map<String, Value>> {
        let targets = params.targets();
        if targets.is_empty() {
            return Err(EngineError::new("no sound has been chosen").into());
        }
        let templates = targets
            .iter()
            .map(|t| self.program.sound_start(&t.sound, params.match_ms))
            .collect::<Result<Vec<_>, _>>()?;
        let longest = templates.iter().map(|t| t.duration_ms()).fold(0.0, f64::max) + LISTEN_SLACK_MS;

        let key = (params.source, params.device.trim().to_owned());
        // Only what is new since the last look (with a margin), at most what the longest sound needs.
        let wanted = match self.streams.get(&key).and_then(|s| s.fed_at) {
            Some(at) => (at.elapsed().as_secs_f64() * 1000.0 + 250.0).min(longest),
            None => longest,
        };
        let clip = self.host.listen(params.source, &params.device, wanted.max(50.0)).map_err(EngineError::from)?;
        let stream = self.streams.entry(key).or_default();
        stream.listening.feed(&clip.samples, clip.sample_rate, clip.end);
        stream.fed_at = Some(Instant::now());

        let mut results = Vec::with_capacity(targets.len());
        for (target, template) in targets.iter().zip(&templates) {
            let after = self.heard_until.get(&(node, target.sound.clone())).copied().unwrap_or(0);
            let best = stream.listening.best_match(template, after);
            let hit = best.is_some_and(|(score, _)| score >= params.accuracy_of(target));
            results.push((best, hit));
        }
        let count = results.iter().filter(|(_, hit)| *hit).count();
        let found = match params.mode {
            FindMode::Any => count > 0,
            FindMode::All => count == targets.len(),
        };
        let reported = results.iter().position(|(_, hit)| *hit);
        // Mark what was recognised, so the same playing is not counted again.
        let counted: Vec<usize> = match (params.mode, found) {
            (FindMode::Any, true) => reported.into_iter().collect(),
            (FindMode::All, true) => (0..targets.len()).collect(),
            _ => Vec::new(),
        };
        for i in counted {
            if let Some((_, end)) = results[i].0 {
                self.heard_until.insert((node, targets[i].sound.clone()), end);
            }
        }

        let score = match reported {
            Some(i) => results[i].0.map_or(0.0, |(s, _)| s),
            None => results.iter().filter_map(|(r, _)| r.map(|(s, _)| s)).fold(0.0, f64::max),
        };
        if let Some(i) = reported.filter(|_| found) {
            let name = self.program.assets.get(&targets[i].sound).map(|a| a.name.clone()).unwrap_or_default();
            self.last_heard.insert(node, (name, targets[i].group.clone()));
        }
        let (name, group) = self.last_heard.get(&node).cloned().unwrap_or_default();
        let mut outputs = serde_json::Map::new();
        outputs.insert("found".into(), serde_json::json!(found));
        outputs.insert("score".into(), serde_json::json!((score * 1000.0).round() / 1000.0));
        outputs.insert("name".into(), serde_json::json!(name));
        outputs.insert("group".into(), serde_json::json!(group));
        outputs.insert("count".into(), serde_json::json!(count));
        Ok(outputs)
    }

    // ---- Rate ---------------------------------------------------------------

    /// Does each step in turn, waiting `rate_ms` between one and the next (a **Rate** block).
    pub fn rate(&mut self, params: &RateParams, rate_ms: f64) -> OpResult<()> {
        for (index, step) in params.steps.iter().enumerate() {
            if index > 0 {
                self.sleep_ms(rate_ms.max(0.0))?;
            }
            let one = ActionParams { actions: vec![step.clone()], hold_ms: params.hold_ms };
            self.perform_actions(&one, None)?;
        }
        Ok(())
    }

    // ---- variables ------------------------------------------------------------

    fn variable_name(name: &str) -> OpResult<&str> {
        let name = name.trim();
        if name.is_empty() {
            return Err(EngineError::new("no variable has been chosen").into());
        }
        Ok(name)
    }

    /// Changes a variable (a **Set Variable** block). `wired` is what the
    /// Value input received, if it is wired. Returns the new value.
    pub fn set_variable(&mut self, params: &SetVariableParams, wired: Option<Value>) -> OpResult<Value> {
        let name = Self::variable_name(&params.variable)?;
        let operand = wired.unwrap_or_else(|| Value::String(params.value.clone()));
        Ok(self.vars.update(name, |current| params.operation.apply(current, &operand)))
    }

    /// Counts one pass (a **Counter** block). Returns the count and whether
    /// the limit was reached (the count then starts again from zero).
    pub fn count(&mut self, params: &CounterParams) -> OpResult<(f64, bool)> {
        let name = Self::variable_name(&params.variable)?;
        let mut reached = false;
        let value = self.vars.update(name, |current| {
            let next = as_number(current).unwrap_or(0.0) + params.step;
            if params.limit > 0.0 && next >= params.limit {
                reached = true;
                serde_json::json!(0.0)
            } else {
                serde_json::json!(next)
            }
        });
        let count = if reached { params.limit } else { as_number(&value).unwrap_or(0.0) };
        Ok((count, reached))
    }

    /// A random number in `0..1`, for blocks that need one.
    pub fn random(&mut self) -> f64 {
        self.rng.f64()
    }

    // ---- system ---------------------------------------------------------------

    pub fn clipboard(&mut self) -> OpResult<String> {
        Ok(self.host.clipboard_text().map_err(EngineError::from)?)
    }

    /// Plays a sound and/or shows a notification (a **Notification** block).
    /// A failure is reported but does not stop the macro.
    pub fn notify(&mut self, params: &NotifyParams, wired: Option<Value>, node: NodeId) -> OpResult<()> {
        let message = match wired {
            Some(value) => format!("{} {}", params.message, as_text(&value)).trim().to_owned(),
            None => params.message.clone(),
        };
        let notification =
            Notification { title: params.title.clone(), message, sound: params.sound, popup: params.popup };
        if let Err(error) = self.host.notify(&notification) {
            self.monitor.log(LogLevel::Warning, Some(node), format!("Could not notify: {}", error.0));
        }
        Ok(())
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

//! The controller: everything that must keep working when the window is not
//! being drawn.
//!
//! Hotkeys, recording and running macros cannot depend on the UI thread – a
//! macro tool spends most of its life minimised behind a game. So this module
//! owns them, on its own thread, behind a thread-safe handle. The UI is one
//! client of the controller (it publishes what the user is editing and shows
//! what the controller reports); a global hotkey is another.
//!
//! A housekeeping thread keeps memory low while nothing happens: an idle
//! Python process is stopped (it starts again on the next run), and memory
//! is handed back to the system while the window is hidden.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use std::time::{Duration, Instant};

use magi_core::autoclicker::AutoClickerConfig;
use magi_core::geom::{Point, Rect};
use magi_core::graph::NodeId;
use magi_core::host::{Frame, Host, HostError, WindowInfo, WindowQuery};
use magi_core::hotkey::{CaptureResult, ChordCapture, Hotkey, HotkeyEdge, HotkeyTracker};
use magi_core::input::{Button, InputEvent, Key};
use magi_core::nodes::Registry;
use magi_core::nodes::flow::{StartMode, StartParams};
use magi_core::options::AutomationOptions;
use magi_core::project::{EnginePreference, Project};
use magi_core::recording::{RecordOptions, Recording, RecordingBuilder};
use magi_engine::{EngineKind, Monitor, Program, PythonConfig, PythonRuntime, PythonStatus, Session, SharedRules};
use magi_platform::{InputHub, Observed, SystemHost};

use crate::paths::Paths;
use crate::settings::{Settings, Tab};

/// An unused Python process is stopped after this long.
const PYTHON_IDLE: Duration = Duration::from_secs(180);
/// Memory is handed back once the window has been hidden this long.
const TRIM_AFTER_HIDDEN: Duration = Duration::from_secs(15);
const HOUSEKEEPING_INTERVAL: Duration = Duration::from_secs(5);

/// The parts of the settings the controller acts on.
#[derive(Clone, Debug, PartialEq)]
pub struct ControllerConfig {
    pub tab: Tab,
    pub stop_hotkey: Hotkey,
    pub record_hotkey: Hotkey,
    pub run_hotkey: Hotkey,
    pub options: AutomationOptions,
    pub engine: EnginePreference,
    pub python_path: Option<PathBuf>,
    pub dry_run: bool,
    pub record: RecordOptions,
    pub clicker: AutoClickerConfig,
}

impl ControllerConfig {
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            tab: settings.tab,
            stop_hotkey: settings.stop_hotkey.clone(),
            record_hotkey: settings.record_hotkey.clone(),
            run_hotkey: settings.run_hotkey.clone(),
            options: settings.options.clone(),
            engine: settings.engine,
            python_path: settings.python_path.clone(),
            dry_run: settings.dry_run,
            record: settings.record.clone(),
            clicker: settings.clicker.clone(),
        }
    }
}

/// What a hotkey is bound to.
#[derive(Clone, Debug, PartialEq)]
enum Binding {
    StopEverything,
    Record,
    RunMacro,
    Clicker(StartMode),
    Start(NodeId, StartMode),
}

/// The chords that switch tasks: Alt+Tab and Win+Tab, with or without Shift.
fn task_switch_hotkeys() -> Vec<Hotkey> {
    let mut hotkeys = Vec::new();
    for modifier in [Key::AltLeft, Key::MetaLeft] {
        hotkeys.push(Hotkey::from_buttons([Button::Key(modifier), Button::Key(Key::Tab)]));
        hotkeys.push(Hotkey::from_buttons([Button::Key(modifier), Button::Key(Key::ShiftLeft), Button::Key(Key::Tab)]));
    }
    hotkeys
}

/// The macro the user is editing, as last handed over by the UI.
struct Published {
    revision: u64,
    program: Arc<Program>,
    engine: EnginePreference,
    /// Start blocks that have a hotkey.
    hotkeys: Vec<(NodeId, Hotkey, StartMode)>,
}

struct ActiveRecording {
    builder: RecordingBuilder,
    started: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RecordingStatus {
    pub seconds: f32,
    pub strokes: usize,
}

struct State {
    config: ControllerConfig,
    tracker: HotkeyTracker<Binding>,
    published: Option<Arc<Published>>,
    /// The session for the published macro, tagged with the revision it was built from.
    session: Option<(u64, Session)>,
    clicker: Option<Session>,
    recording: Option<ActiveRecording>,
    finished_recordings: Vec<Recording>,
    capture: Option<ChordCapture>,
    capture_result: Option<CaptureResult>,
    python: Arc<PythonRuntime>,
    /// So "Python is unavailable, using the native engine" is said once, not on every run.
    warned_no_python: bool,
    /// When something last ran, for stopping an idle Python process.
    last_busy: Instant,
    /// Since when the window has been hidden, and whether memory was trimmed since.
    hidden_since: Option<Instant>,
    trimmed: bool,
}

struct Inner {
    registry: Arc<Registry>,
    paths: Paths,
    host: Arc<SystemHost>,
    hub: InputHub,
    /// Activity and log of the macro.
    monitor: Arc<Monitor>,
    /// The Auto Clicker reports separately: its node ids would otherwise
    /// collide with the macro's and light up the wrong blocks in the editor.
    clicker_monitor: Arc<Monitor>,
    /// Process rules, shared with every session so changes apply at once.
    rules: SharedRules,
    state: Mutex<State>,
    closing: AtomicBool,
}

#[derive(Clone)]
pub struct Controller {
    inner: Arc<Inner>,
}

/// A host that looks but does not touch: screen and pointer are real, input is only logged.
struct DryRunHost {
    real: Arc<dyn Host>,
    monitor: Arc<Monitor>,
}

impl Host for DryRunHost {
    fn send(&self, event: &InputEvent) -> Result<(), HostError> {
        self.monitor.info(format!("Dry run – would {event}"));
        Ok(())
    }

    fn cursor(&self) -> Result<Point, HostError> {
        self.real.cursor()
    }

    fn capture(&self, region: Option<Rect>) -> Result<Frame, HostError> {
        self.real.capture(region)
    }

    fn screens(&self) -> Result<Vec<Rect>, HostError> {
        self.real.screens()
    }

    fn foreground_process(&self) -> Option<String> {
        self.real.foreground_process()
    }

    fn focus_window(&self, query: &WindowQuery) -> Result<Option<WindowInfo>, HostError> {
        self.monitor.info(format!("Dry run – would switch to “{}”", query.text));
        Ok(None)
    }
}

fn python_runtime(paths: &Paths, interpreter: Option<PathBuf>, monitor: &Arc<Monitor>) -> Arc<PythonRuntime> {
    let config = PythonConfig { interpreter, app_dir: paths.app_dir.clone(), data_dir: paths.data_dir.clone() };
    Arc::new(PythonRuntime::new(config, Arc::clone(monitor)))
}

impl Controller {
    pub fn new(registry: Arc<Registry>, paths: Paths, config: ControllerConfig) -> Self {
        let monitor = Arc::new(Monitor::new());
        let hub = InputHub::start();
        let python = python_runtime(&paths, config.python_path.clone(), &monitor);
        let rules = Arc::new(RwLock::new(config.options.process_rules.clone()));
        let mut state = State {
            config,
            tracker: HotkeyTracker::new(),
            published: None,
            session: None,
            clicker: None,
            recording: None,
            finished_recordings: Vec::new(),
            capture: None,
            capture_result: None,
            python,
            warned_no_python: false,
            last_busy: Instant::now(),
            hidden_since: None,
            trimmed: false,
        };
        state.rebind();

        let inner = Arc::new(Inner {
            registry,
            paths,
            host: Arc::new(SystemHost::new()),
            hub: hub.clone(),
            monitor,
            clicker_monitor: Arc::new(Monitor::new()),
            rules,
            state: Mutex::new(state),
            closing: AtomicBool::new(false),
        });

        let events = hub.subscribe();
        let for_thread = Arc::clone(&inner);
        std::thread::Builder::new()
            .name("magi-controller".into())
            .spawn(move || {
                for observed in events {
                    for_thread.on_input(&observed);
                }
            })
            .expect("the OS refused to start a thread");

        let weak = Arc::downgrade(&inner);
        std::thread::Builder::new()
            .name("magi-housekeeping".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(HOUSEKEEPING_INTERVAL);
                    let Some(inner) = weak.upgrade() else { return };
                    if inner.closing.load(Ordering::Relaxed) {
                        return;
                    }
                    inner.housekeeping();
                }
            })
            .expect("the OS refused to start a thread");

        Self { inner }
    }

    pub fn monitor(&self) -> &Arc<Monitor> {
        &self.inner.monitor
    }

    pub fn clicker_monitor(&self) -> &Arc<Monitor> {
        &self.inner.clicker_monitor
    }

    /// Why hotkeys and recording are not working, if they are not.
    pub fn listener_error(&self) -> Option<String> {
        self.inner.hub.listener_error()
    }

    /// Tells the controller whether the window can be seen, so memory can be
    /// handed back while it cannot.
    pub fn set_window_hidden(&self, hidden: bool) {
        let mut state = self.inner.state();
        match (hidden, state.hidden_since) {
            (true, None) => state.hidden_since = Some(Instant::now()),
            (false, Some(_)) => {
                state.hidden_since = None;
                state.trimmed = false;
            }
            _ => {}
        }
    }

    /// Stops everything and the background threads. Call when the app closes.
    pub fn shut_down(&self) {
        self.inner.closing.store(true, Ordering::Relaxed);
        self.inner.stop_everything();
        let python = Arc::clone(&self.inner.state().python);
        python.shutdown();
    }

    // ---- configuration ----------------------------------------------------

    pub fn set_config(&self, config: ControllerConfig) {
        let mut state = self.inner.state();
        if state.config == config {
            return;
        }
        if state.config.python_path != config.python_path {
            state.python = python_runtime(&self.inner.paths, config.python_path.clone(), &self.inner.monitor);
            state.session = None;
            state.warned_no_python = false;
        }
        if state.config.dry_run != config.dry_run || state.config.engine != config.engine {
            // Sessions bake in their host and engine; rebuild on next start.
            state.session = None;
        }
        if state.config.options.process_rules != config.options.process_rules {
            *self.inner.rules.write().unwrap_or_else(|p| p.into_inner()) = config.options.process_rules.clone();
        }
        state.config = config;
        state.rebind();
    }

    /// Hands over the macro being edited. Cheap to call when nothing changed.
    pub fn publish(&self, revision: u64, project: &Project) {
        let mut state = self.inner.state();
        if state.published.as_ref().is_some_and(|p| p.revision == revision) {
            return;
        }
        let hotkeys = project
            .graph
            .starts()
            .filter(|node| node.enabled)
            .filter_map(|node| {
                let params: StartParams = node.params_or_default();
                (!params.hotkey.is_none()).then_some((node.id, params.hotkey, params.mode))
            })
            .collect();
        state.published = Some(Arc::new(Published {
            revision,
            program: Arc::new(Program::from_project(project, Arc::clone(&self.inner.registry))),
            engine: project.engine,
            hotkeys,
        }));
        state.rebind();
    }

    // ---- macro ------------------------------------------------------------

    /// Starts every Start block that responds to the Run button.
    pub fn run_macro(&self) {
        self.inner.start_macro(None);
    }

    /// Starts the flow at one block ("run from here" and Start hotkeys).
    pub fn run_from(&self, node: NodeId) {
        self.inner.start_macro(Some(node));
    }

    pub fn stop_node(&self, node: NodeId) {
        self.inner.stop_node(node);
    }

    pub fn stop_macro(&self) {
        self.inner.stop_macro();
    }

    /// Start blocks that are currently running.
    pub fn running_nodes(&self) -> Vec<NodeId> {
        self.inner.state().session.as_ref().map(|(_, session)| session.running_nodes()).unwrap_or_default()
    }

    /// The engine the macro last ran on (or is running on).
    pub fn macro_engine(&self) -> Option<EngineKind> {
        self.inner.state().session.as_ref().map(|(_, session)| session.engine())
    }

    /// The engine the next run would ask for, before checking it is available.
    pub fn preferred_engine(&self) -> EnginePreference {
        let state = self.inner.state();
        match state.published.as_ref().map(|p| p.engine) {
            Some(EnginePreference::Auto) | None => state.config.engine,
            Some(chosen) => chosen,
        }
    }

    pub fn python_status(&self) -> PythonStatus {
        let runtime = Arc::clone(&self.inner.state().python);
        runtime.status()
    }

    /// Tries to start Python and reports whether it worked. May take a moment.
    pub fn probe_python(&self) -> PythonStatus {
        let runtime = Arc::clone(&self.inner.state().python);
        runtime.probe()
    }

    // ---- auto clicker -----------------------------------------------------

    pub fn start_clicker(&self) {
        self.inner.start_clicker();
    }

    pub fn stop_clicker(&self) {
        self.inner.stop_clicker();
    }

    pub fn clicker_running(&self) -> bool {
        self.inner.state().clicker.as_ref().is_some_and(|session| session.active_runs() > 0)
    }

    // ---- everything -------------------------------------------------------

    pub fn stop_everything(&self) {
        self.inner.stop_everything();
    }

    pub fn anything_running(&self) -> bool {
        let state = self.inner.state();
        state.clicker.as_ref().is_some_and(|s| s.active_runs() > 0)
            || state.session.as_ref().is_some_and(|(_, s)| s.active_runs() > 0)
    }

    // ---- recording --------------------------------------------------------

    pub fn toggle_recording(&self) {
        self.inner.toggle_recording();
    }

    pub fn recording_status(&self) -> Option<RecordingStatus> {
        self.inner.state().recording.as_ref().map(|active| RecordingStatus {
            seconds: active.started.elapsed().as_secs_f32(),
            strokes: active.builder.stroke_count(),
        })
    }

    /// Recordings that have ended since the last call.
    pub fn take_recordings(&self) -> Vec<Recording> {
        std::mem::take(&mut self.inner.state().finished_recordings)
    }

    // ---- hotkey capture ---------------------------------------------------

    /// Starts listening for the next chord instead of acting on hotkeys.
    pub fn begin_capture(&self) {
        let stop_clicker = {
            let mut state = self.inner.state();
            state.capture = Some(ChordCapture::new());
            state.capture_result = None;
            state.config.options.stop_when_editing_hotkey
        };
        // A clicker clicking away would make it impossible to press anything.
        if stop_clicker {
            self.inner.stop_clicker();
        }
    }

    /// Stops listening for a chord.
    pub fn cancel_capture(&self) {
        let mut state = self.inner.state();
        state.capture = None;
        state.capture_result = None;
        state.tracker.reset();
    }

    /// Feeds an event as if the user had pressed it.
    #[cfg(test)]
    pub fn inject(&self, event: InputEvent) {
        self.inner.on_input(&Observed { event, at: Instant::now(), synthetic: false });
    }

    /// Progress of the capture. A finished result is returned once.
    pub fn poll_capture(&self) -> Option<CaptureResult> {
        let mut state = self.inner.state();
        match &state.capture_result {
            Some(CaptureResult::Pending(_)) | None => state.capture_result.clone(),
            Some(_) => state.capture_result.take(),
        }
    }
}

impl State {
    /// Rebuilds the hotkey table from the current configuration and macro.
    fn rebind(&mut self) {
        let options = &self.config.options;
        let mut bindings = vec![
            (self.config.stop_hotkey.clone(), Binding::StopEverything),
            (self.config.record_hotkey.clone(), Binding::Record),
        ];
        bindings.extend(options.stop_hotkeys.iter().map(|hotkey| (hotkey.clone(), Binding::StopEverything)));
        if options.stop_on_task_switch {
            bindings.extend(task_switch_hotkeys().into_iter().map(|hotkey| (hotkey, Binding::StopEverything)));
        }
        match self.config.tab {
            Tab::AutoClicker => {
                let clicker = &self.config.clicker;
                bindings.push((clicker.hotkey.clone(), Binding::Clicker(clicker.activation)));
            }
            Tab::Macro => {
                bindings.push((self.config.run_hotkey.clone(), Binding::RunMacro));
                if let Some(published) = &self.published {
                    bindings.extend(
                        published
                            .hotkeys
                            .iter()
                            .map(|(node, hotkey, mode)| (hotkey.clone(), Binding::Start(*node, *mode))),
                    );
                }
            }
        }
        self.tracker.set_strict(options.strict_modifiers);
        self.tracker.set_bindings(bindings);
    }

    fn busy(&self) -> bool {
        self.clicker.as_ref().is_some_and(|s| s.active_runs() > 0)
            || self.session.as_ref().is_some_and(|(_, s)| s.active_runs() > 0)
            || self.recording.is_some()
    }
}

impl Inner {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Called on the controller thread for every keyboard, mouse and gamepad event.
    fn on_input(&self, observed: &Observed) {
        // Input sent by a running macro must not trigger hotkeys or be recorded.
        if observed.synthetic {
            return;
        }
        let edges = {
            let mut state = self.state();
            if let Some(capture) = &mut state.capture {
                let result = capture.feed(&observed.event);
                if !matches!(result, CaptureResult::Pending(_)) {
                    state.capture = None;
                    state.tracker.reset();
                }
                state.capture_result = Some(result);
                self.monitor.wake();
                return;
            }
            let edges = state.tracker.feed(&observed.event);
            if let Some(active) = &mut state.recording {
                let elapsed = observed.at.saturating_duration_since(active.started);
                active.builder.push(elapsed.as_secs_f64() * 1000.0, &observed.event);
            }
            edges
        };
        for (binding, edge) in edges {
            self.on_hotkey(&binding, edge);
        }
    }

    fn on_hotkey(&self, binding: &Binding, edge: HotkeyEdge) {
        let pressed = edge == HotkeyEdge::Pressed;
        let recording = self.state().recording.is_some();
        match binding {
            Binding::StopEverything if pressed => self.stop_everything(),
            Binding::Record if pressed => self.toggle_recording(),
            // While recording, the user's keys are being captured, not obeyed.
            Binding::Clicker(_) | Binding::Start(..) | Binding::RunMacro if recording => {}
            Binding::RunMacro if pressed => {
                let running = self.state().session.as_ref().is_some_and(|(_, s)| s.active_runs() > 0);
                if running { self.stop_macro() } else { self.start_macro(None) }
            }
            Binding::Clicker(mode) => {
                let running = self.state().clicker.as_ref().is_some_and(|s| s.active_runs() > 0);
                match (mode, pressed, running) {
                    (StartMode::Hold, true, _) => self.start_clicker(),
                    (StartMode::Hold, false, _) => self.stop_clicker(),
                    (_, true, true) => self.stop_clicker(),
                    (_, true, false) => self.start_clicker(),
                    (_, false, _) => {}
                }
            }
            Binding::Start(node, mode) => {
                let running = self.state().session.as_ref().is_some_and(|(_, s)| s.is_running(*node));
                match (mode, pressed, running) {
                    (StartMode::Hold, true, false) | (StartMode::Press, true, false) => self.start_macro(Some(*node)),
                    (StartMode::Hold, false, true) => self.stop_node(*node),
                    (StartMode::Toggle, true, true) => self.stop_node(*node),
                    (StartMode::Toggle, true, false) => self.start_macro(Some(*node)),
                    _ => {}
                }
            }
            _ => {}
        }
        self.monitor.wake();
        self.clicker_monitor.wake();
    }

    /// Runs every few seconds: stops an idle Python process and hands memory back.
    fn housekeeping(&self) {
        let mut state = self.state();
        if state.busy() {
            state.last_busy = Instant::now();
            return;
        }
        let mut freed = false;
        let python_session = state.session.as_ref().is_some_and(|(_, s)| s.engine() == EngineKind::Python);
        if python_session && state.last_busy.elapsed() >= PYTHON_IDLE {
            state.session = None;
            let python = Arc::clone(&state.python);
            drop(state);
            python.shutdown();
            state = self.state();
            freed = true;
        }
        let hidden_long = state.hidden_since.is_some_and(|since| since.elapsed() >= TRIM_AFTER_HIDDEN);
        if freed || (hidden_long && !state.trimmed) {
            state.trimmed = hidden_long;
            drop(state);
            magi_platform::system::trim_memory();
        }
    }

    fn stop_node(&self, node: NodeId) {
        if let Some((_, session)) = &self.state().session {
            session.stop_node(node);
        }
    }

    fn stop_macro(&self) {
        if let Some((_, session)) = &self.state().session {
            session.stop_all();
        }
    }

    fn host(&self, dry_run: bool, monitor: &Arc<Monitor>) -> Arc<dyn Host> {
        if dry_run {
            Arc::new(DryRunHost { real: Arc::clone(&self.host) as Arc<dyn Host>, monitor: Arc::clone(monitor) })
        } else {
            Arc::clone(&self.host) as Arc<dyn Host>
        }
    }

    /// Starts the published macro: all Run-button Start blocks, or one block.
    fn start_macro(&self, node: Option<NodeId>) {
        // Decide what is needed while holding the lock, build it without the
        // lock (starting Python can take a moment), then store it.
        let (published, config, python, warned) = {
            let mut state = self.state();
            state.last_busy = Instant::now();
            let Some(published) = state.published.clone() else { return };
            if let Some((revision, session)) = &state.session
                && *revision == published.revision
            {
                self.launch(session, node);
                return;
            }
            (published, state.config.clone(), Arc::clone(&state.python), state.warned_no_python)
        };

        let preference = match published.engine {
            EnginePreference::Auto => config.engine,
            chosen => chosen,
        };
        let host = self.host(config.dry_run, &self.monitor);
        let native = || Session::native(Arc::clone(&published.program), Arc::clone(&host), Arc::clone(&self.monitor));
        let python_session =
            || Session::python(Arc::clone(&published.program), Arc::clone(&host), Arc::clone(&self.monitor), python);

        let mut warned_now = false;
        let session = match preference {
            EnginePreference::Native => native(),
            EnginePreference::Python => match python_session() {
                Ok(session) => session,
                Err(error) => {
                    self.monitor.error(error.message);
                    return;
                }
            },
            EnginePreference::Auto => match python_session() {
                Ok(session) => session,
                Err(error) => {
                    if !warned {
                        warned_now = true;
                        self.monitor.warn(format!("{} Using the native engine instead.", error.message));
                    }
                    native()
                }
            },
        };
        let session = session.with_rules(Arc::clone(&self.rules));

        let mut state = self.state();
        state.warned_no_python |= warned_now;
        self.launch(&session, node);
        // Replacing the previous session stops whatever an older revision was still running.
        state.session = Some((published.revision, session));
    }

    fn launch(&self, session: &Session, node: Option<NodeId>) {
        let outcome = match node {
            Some(node) => session.start(node).map(drop),
            None => session.start_all().map(|started| {
                if started == 0 {
                    self.monitor.warn("Nothing to run: add a Start block (with “Start with the Run button” on).");
                }
            }),
        };
        if let Err(error) = outcome {
            self.monitor.log(magi_engine::LogLevel::Error, error.node, error.message);
        }
    }

    fn start_clicker(&self) {
        let mut state = self.state();
        if state.clicker.as_ref().is_some_and(|session| session.active_runs() > 0) {
            return;
        }
        state.last_busy = Instant::now();
        let graph = state.config.clicker.to_graph(&self.registry);
        let Some(start) = graph.starts().next().map(|node| node.id) else { return };
        let program = Arc::new(Program::new(graph, Default::default(), Arc::clone(&self.registry)));
        let host = self.host(state.config.dry_run, &self.clicker_monitor);
        self.clicker_monitor.clear_activity();
        self.clicker_monitor.clear_logs();
        // The Auto Clicker always runs natively: it needs no scripting, and it
        // must work on machines without Python.
        let session =
            Session::native(program, host, Arc::clone(&self.clicker_monitor)).with_rules(Arc::clone(&self.rules));
        if let Err(error) = session.start(start) {
            self.clicker_monitor.error(error.message);
        }
        state.clicker = Some(session);
        drop(state);
        self.clicker_monitor.wake();
    }

    fn stop_clicker(&self) {
        if let Some(session) = &self.state().clicker {
            session.stop_all();
        }
        self.clicker_monitor.wake();
    }

    fn stop_everything(&self) {
        let state = self.state();
        if let Some(session) = &state.clicker {
            session.stop_all();
        }
        if let Some((_, session)) = &state.session {
            session.stop_all();
        }
    }

    fn toggle_recording(&self) {
        let mut state = self.state();
        match state.recording.take() {
            Some(active) => {
                let elapsed = active.started.elapsed().as_secs_f64() * 1000.0;
                let screen = magi_platform::capture::monitors()
                    .ok()
                    .and_then(|monitors| monitors.first().map(|m| (m.rect.w, m.rect.h)));
                let recording = active.builder.finish(elapsed, screen);
                if recording.strokes.is_empty() {
                    self.monitor.warn("Nothing was recorded.");
                } else {
                    state.finished_recordings.push(recording);
                }
            }
            None => {
                // Never record the keys that control recording itself.
                let ignored = state
                    .config
                    .record_hotkey
                    .buttons()
                    .iter()
                    .chain(state.config.stop_hotkey.buttons())
                    .copied()
                    .collect();
                let cursor = self.host.cursor().unwrap_or_default();
                state.recording = Some(ActiveRecording {
                    builder: RecordingBuilder::new(state.config.record.clone(), ignored, cursor),
                    started: Instant::now(),
                });
            }
        }
        state.last_busy = Instant::now();
        drop(state);
        self.monitor.wake();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_switch_chords_include_shift_variants() {
        let hotkeys = task_switch_hotkeys();
        assert_eq!(hotkeys.len(), 4);
        let alt_tab = Hotkey::from_buttons([Button::Key(Key::AltLeft), Button::Key(Key::Tab)]);
        assert!(hotkeys.contains(&alt_tab));
        assert!(hotkeys.iter().any(|h| h.contains(Button::Key(Key::ShiftLeft))));
    }
}

#[cfg(test)]
mod system_tests {
    //! Against the real machine; ignored by default. Keys used are F13/F14,
    //! which have no default action. The controller runs in dry-run mode, so
    //! nothing is ever clicked.

    use std::time::{Duration, Instant};

    use enigo::{Direction, Enigo, Keyboard, Settings as EnigoSettings};
    use magi_core::input::Key;

    use super::*;

    fn press(user: &mut Enigo, vks: &[u32]) {
        for vk in vks {
            user.key(enigo::Key::Other(*vk), Direction::Press).unwrap();
        }
        for vk in vks.iter().rev() {
            user.key(enigo::Key::Other(*vk), Direction::Release).unwrap();
        }
    }

    fn controller() -> Controller {
        magi_platform::init();
        let paths =
            Paths { app_dir: std::env::temp_dir(), data_dir: std::env::temp_dir().join("magi-controller-test") };
        let mut settings = Settings { dry_run: true, ..Settings::default() };
        settings.clicker.hotkey = Hotkey::key(Key::F14);
        let controller =
            Controller::new(Arc::new(Registry::builtin()), paths, ControllerConfig::from_settings(&settings));
        std::thread::sleep(Duration::from_millis(300));
        controller
    }

    #[test]
    #[ignore = "needs a desktop session"]
    fn a_pressed_chord_is_captured() {
        let controller = controller();
        let mut user = Enigo::new(&EnigoSettings::default()).unwrap();
        controller.begin_capture();
        press(&mut user, &[0xA2, 0x7C]); // Ctrl + F13
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut result = None;
        while Instant::now() < deadline {
            match controller.poll_capture() {
                Some(CaptureResult::Pending(_)) | None => std::thread::sleep(Duration::from_millis(20)),
                done => {
                    result = done;
                    break;
                }
            }
        }
        assert_eq!(result, Some(CaptureResult::Done("ControlLeft+F13".parse().unwrap())));
    }

    #[test]
    #[ignore = "needs a desktop session"]
    fn a_changed_hotkey_works_immediately() {
        let controller = controller();
        let mut user = Enigo::new(&EnigoSettings::default()).unwrap();
        // Rebind the clicker from F14 to F13 at run time, as the UI does.
        let mut settings = Settings { dry_run: true, ..Settings::default() };
        settings.clicker.hotkey = Hotkey::key(Key::F13);
        controller.set_config(ControllerConfig::from_settings(&settings));
        press(&mut user, &[0x7C]);
        std::thread::sleep(Duration::from_millis(300));
        assert!(controller.clicker_running(), "F13 should start the clicker");
        press(&mut user, &[0x7C]);
        std::thread::sleep(Duration::from_millis(300));
        assert!(!controller.clicker_running(), "F13 again should stop it");
    }

    #[test]
    #[ignore = "needs a desktop session"]
    fn custom_stop_hotkeys_and_task_switching_stop_the_clicker() {
        let controller = controller();
        let mut user = Enigo::new(&EnigoSettings::default()).unwrap();
        let mut settings = Settings { dry_run: true, ..Settings::default() };
        settings.clicker.hotkey = Hotkey::key(Key::F14);
        settings.options.stop_hotkeys = vec![Hotkey::key(Key::F13)];
        controller.set_config(ControllerConfig::from_settings(&settings));
        controller.start_clicker();
        std::thread::sleep(Duration::from_millis(100));
        assert!(controller.clicker_running());
        press(&mut user, &[0x7C]); // F13
        std::thread::sleep(Duration::from_millis(300));
        assert!(!controller.clicker_running(), "the extra stop hotkey stops the clicker");
    }
}

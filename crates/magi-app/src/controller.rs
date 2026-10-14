//! The controller: everything that must keep working when the window is not
//! being drawn.
//!
//! Hotkeys, recording and running macros cannot depend on the UI thread – a
//! macro tool spends most of its life minimised behind a game. So this module
//! owns them, on its own thread, behind a thread-safe handle. The UI is one
//! client of the controller (it publishes what the user is editing and shows
//! what the controller reports); a global hotkey is another.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use magi_core::autoclicker::AutoClickerConfig;
use magi_core::geom::{Point, Rect};
use magi_core::graph::NodeId;
use magi_core::host::{Frame, Host, HostError};
use magi_core::hotkey::{CaptureResult, ChordCapture, Hotkey, HotkeyEdge, HotkeyTracker};
use magi_core::input::InputEvent;
use magi_core::nodes::Registry;
use magi_core::nodes::flow::{StartMode, StartParams};
use magi_core::project::{EnginePreference, Project};
use magi_core::recording::{RecordOptions, Recording, RecordingBuilder};
use magi_engine::{EngineKind, Monitor, Program, PythonConfig, PythonRuntime, PythonStatus, Session};
use magi_platform::{InputHub, Observed, SystemHost};

use crate::paths::Paths;
use crate::settings::{Settings, Tab};

/// The parts of the settings the controller acts on.
#[derive(Clone, Debug, PartialEq)]
pub struct ControllerConfig {
    pub tab: Tab,
    pub stop_hotkey: Hotkey,
    pub record_hotkey: Hotkey,
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
    Clicker(StartMode),
    Start(NodeId, StartMode),
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
    state: Mutex<State>,
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
        };
        state.rebind();

        let inner = Arc::new(Inner {
            registry,
            paths,
            host: Arc::new(SystemHost::new()),
            hub: hub.clone(),
            monitor,
            clicker_monitor: Arc::new(Monitor::new()),
            state: Mutex::new(state),
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
        if let Some((_, session)) = &self.inner.state().session {
            session.stop_node(node);
        }
    }

    pub fn stop_macro(&self) {
        if let Some((_, session)) = &self.inner.state().session {
            session.stop_all();
        }
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

    pub fn toggle_clicker(&self) {
        if self.clicker_running() { self.inner.stop_clicker() } else { self.inner.start_clicker() }
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
        let mut state = self.inner.state();
        state.capture = Some(ChordCapture::new());
        state.capture_result = None;
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
        let mut bindings = vec![
            (self.config.stop_hotkey.clone(), Binding::StopEverything),
            (self.config.record_hotkey.clone(), Binding::Record),
        ];
        match self.config.tab {
            Tab::AutoClicker => {
                let clicker = &self.config.clicker;
                bindings.push((clicker.hotkey.clone(), Binding::Clicker(clicker.activation)));
            }
            Tab::Macro => {
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
        self.tracker.set_bindings(bindings);
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
            Binding::Clicker(_) | Binding::Start(..) if recording => {}
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

    fn stop_node(&self, node: NodeId) {
        if let Some((_, session)) = &self.state().session {
            session.stop_node(node);
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
            let state = self.state();
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
        let graph = state.config.clicker.to_graph(&self.registry);
        let Some(start) = graph.starts().next().map(|node| node.id) else { return };
        let program = Arc::new(Program::new(graph, Default::default(), Arc::clone(&self.registry)));
        let host = self.host(state.config.dry_run, &self.clicker_monitor);
        self.clicker_monitor.clear_activity();
        // The Auto Clicker always runs natively: it needs no scripting, and it
        // must work on machines without Python.
        let session = Session::native(program, host, Arc::clone(&self.clicker_monitor));
        if let Err(error) = session.start(start) {
            self.clicker_monitor.error(error.message);
        }
        state.clicker = Some(session);
    }

    fn stop_clicker(&self) {
        if let Some(session) = &self.state().clicker {
            session.stop_all();
        }
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
        drop(state);
        self.monitor.wake();
    }
}

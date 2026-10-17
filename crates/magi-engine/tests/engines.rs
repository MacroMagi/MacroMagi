//! Both engines must make the same macro do the same thing.
//!
//! Every scenario here runs on the native engine and, when a Python
//! interpreter is installed, on the Python engine too. Each is checked against
//! the expected input, so the engines are held to one specification rather
//! than merely to each other.
//!
//! Nothing here touches the real machine: input goes to a `SimHost`, and "the
//! screen" is a picture built in memory.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

use magi_core::action::{Action, PressMode};
use magi_core::autoclicker::{
    AutoClickerConfig, ClickLimit, ClickPosition, ClickTarget, ClickerSettings, DualMode, Toggled,
};
use magi_core::geom::{Point, Rect};
use magi_core::graph::{NodeId, PortRef};
use magi_core::host::{Frame, SimHost, SoundClip, WindowInfo};
use magi_core::input::{InputEvent, Key, MouseButton};
use magi_core::nodes::window::ScreenKind;
use magi_core::nodes::{Registry, ids, ports};
use magi_core::options::{ProcessRule, ProcessRules, RuleAction, RuleMode};
use magi_core::project::Project;
use magi_core::recording::{LookPoint, PathPoint, RecordOptions, Recording, RecordingBuilder, Stroke, StrokeInput};
use magi_core::variables::{Variable, binding};
use magi_engine::{EngineKind, LogLevel, Monitor, Program, PythonConfig, PythonRuntime, PythonStatus, Session};
use magi_vision::ImageRef;
use serde_json::json;

const TIMEOUT: Duration = Duration::from_secs(20);

// ---- harness ---------------------------------------------------------------

fn registry() -> Arc<Registry> {
    static REGISTRY: OnceLock<Arc<Registry>> = OnceLock::new();
    Arc::clone(REGISTRY.get_or_init(|| Arc::new(Registry::builtin())))
}

/// One Python process shared by the whole test binary, or `None` without Python.
fn python() -> Option<Arc<PythonRuntime>> {
    static RUNTIME: OnceLock<Option<Arc<PythonRuntime>>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            let dir = std::env::temp_dir().join(format!("magi-engine-tests-{}", std::process::id()));
            let config = PythonConfig { interpreter: None, app_dir: dir.clone(), data_dir: dir };
            let runtime = Arc::new(PythonRuntime::new(config, Arc::new(Monitor::new())));
            match runtime.probe() {
                PythonStatus::Running { version, command } => {
                    eprintln!("engine tests: using Python {version} ({command})");
                    Some(runtime)
                }
                other => {
                    eprintln!("engine tests: Python engine skipped ({other:?})");
                    None
                }
            }
        })
        .clone()
}

struct Builder {
    project: Project,
    registry: Arc<Registry>,
}

impl Builder {
    fn new() -> Self {
        Self { project: Project::new(), registry: registry() }
    }

    fn node(&mut self, kind: &str, params: serde_json::Value) -> NodeId {
        let id = self.project.graph.add(&self.registry, kind, [0.0, 0.0]).unwrap();
        if let serde_json::Value::Object(map) = params {
            self.project.graph.node_mut(id).unwrap().params.extend(map);
        }
        id
    }

    /// Wires `from`'s flow output `port` to `to`.
    fn flow(&mut self, from: NodeId, port: &str, to: NodeId) -> &mut Self {
        self.wire(from, port, to, ports::FLOW_IN)
    }

    fn wire(&mut self, from: NodeId, from_port: &str, to: NodeId, to_port: &str) -> &mut Self {
        self.project.graph.connect(&self.registry, PortRef::new(from, from_port), PortRef::new(to, to_port)).unwrap();
        self
    }

    /// Chains nodes through their default outputs.
    fn chain(&mut self, nodes: &[NodeId]) -> &mut Self {
        for pair in nodes.windows(2) {
            self.flow(pair[0], ports::OUT, pair[1]);
        }
        self
    }

    fn tap(&mut self, key: Key) -> NodeId {
        self.node(ids::INPUT, json!({ "actions": [Action::tap(key)], "hold_ms": 1.0 }))
    }

    fn click(&mut self) -> NodeId {
        self.node(ids::INPUT, json!({ "actions": [Action::click(MouseButton::Left)], "hold_ms": 1.0 }))
    }

    fn log(&mut self, message: &str) -> NodeId {
        self.node(ids::LOG, json!({ "message": message }))
    }
}

struct Outcome {
    sent: Vec<InputEvent>,
    logs: Vec<String>,
    errors: Vec<String>,
}

impl Outcome {
    fn taps(&self) -> Vec<Key> {
        self.sent
            .iter()
            .filter_map(|event| match event {
                InputEvent::KeyDown { key } => Some(*key),
                _ => None,
            })
            .collect()
    }

    fn clicks(&self) -> usize {
        self.sent.iter().filter(|e| matches!(e, InputEvent::ButtonDown { .. })).count()
    }
}

fn engines() -> Vec<EngineKind> {
    let mut kinds = vec![EngineKind::Native];
    if python().is_some() {
        kinds.push(EngineKind::Python);
    }
    kinds
}

fn session(kind: EngineKind, project: &Project, host: Arc<SimHost>, monitor: Arc<Monitor>) -> Session {
    let program = Arc::new(Program::from_project(project, registry()));
    match kind {
        EngineKind::Native => Session::native(program, host, monitor),
        EngineKind::Python => Session::python(program, host, monitor, python().unwrap()).unwrap(),
    }
}

/// Runs the flow from `start` to completion and reports what happened.
fn run_on(kind: EngineKind, project: &Project, start: NodeId, host: SimHost) -> Outcome {
    let host = Arc::new(host);
    let monitor = Arc::new(Monitor::new());
    let session = session(kind, project, Arc::clone(&host), Arc::clone(&monitor));
    session.start(start).unwrap();
    assert!(session.wait_idle(Some(TIMEOUT)), "{kind:?}: the macro did not finish");
    collect(&host, &monitor)
}

fn collect(host: &SimHost, monitor: &Monitor) -> Outcome {
    let (mut logs, mut errors) = (Vec::new(), Vec::new());
    for entry in monitor.logs() {
        match entry.level {
            LogLevel::Error => errors.push(entry.message),
            _ => logs.push(entry.message),
        }
    }
    Outcome { sent: host.sent(), logs, errors }
}

/// Runs on every available engine, checks each outcome, and returns them.
fn on_all(project: &Project, start: NodeId, host: impl Fn() -> SimHost, check: impl Fn(EngineKind, &Outcome)) {
    for kind in engines() {
        let outcome = run_on(kind, project, start, host());
        check(kind, &outcome);
    }
}

/// A small, unmistakable picture to look for.
fn target_image() -> (Vec<u8>, u32, u32) {
    let (w, h) = (24u32, 16u32);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let on = (x / 4 + y / 4) % 2 == 0;
            rgba.extend_from_slice(if on { &[230, 60, 40, 255] } else { &[20, 40, 200, 255] });
        }
    }
    (rgba, w, h)
}

/// A screen with the target image at `at`, or without it.
fn screen(at: Option<Point>) -> SimHost {
    let mut frame = Frame::solid(Point::new(0, 0), 320, 200, [90, 90, 90]);
    if let Some(at) = at {
        let (rgba, w, h) = target_image();
        frame.blit(&Frame::new(Point::new(0, 0), w, h, rgba), at);
    }
    SimHost::with_screen(frame)
}

fn add_target_asset(project: &mut Project) -> String {
    let (rgba, w, h) = target_image();
    let png = magi_vision::encode_png(ImageRef::new(w, h, &rgba)).unwrap();
    project.assets.add_image("target", png)
}

// ---- scenarios -------------------------------------------------------------

#[test]
fn blocks_run_in_wire_order() {
    let mut b = Builder::new();
    let start = b.node(ids::START, json!({}));
    let a = b.tap(Key::KeyA);
    let wait = b.node(ids::DELAY, json!({ "ms": 5.0 }));
    let click = b.click();
    let log = b.log("finished");
    b.chain(&[start, a, wait, click, log]);

    on_all(&b.project, start, SimHost::new, |kind, outcome| {
        assert_eq!(
            outcome.sent,
            vec![
                InputEvent::KeyDown { key: Key::KeyA },
                InputEvent::KeyUp { key: Key::KeyA },
                InputEvent::ButtonDown { button: MouseButton::Left },
                InputEvent::ButtonUp { button: MouseButton::Left },
            ],
            "{kind:?}"
        );
        assert_eq!(outcome.logs, ["finished"], "{kind:?}");
        assert!(outcome.errors.is_empty(), "{kind:?}: {:?}", outcome.errors);
    });
}

#[test]
fn simultaneous_inputs_release_in_reverse() {
    let mut b = Builder::new();
    let start = b.node(ids::START, json!({}));
    let chord = b.node(
        ids::INPUT,
        json!({ "actions": [Action::tap(Key::ControlLeft), Action::tap(Key::KeyC)], "hold_ms": 2.0 }),
    );
    b.chain(&[start, chord]);

    on_all(&b.project, start, SimHost::new, |kind, outcome| {
        assert_eq!(
            outcome.sent,
            vec![
                InputEvent::KeyDown { key: Key::ControlLeft },
                InputEvent::KeyDown { key: Key::KeyC },
                InputEvent::KeyUp { key: Key::KeyC },
                InputEvent::KeyUp { key: Key::ControlLeft },
            ],
            "{kind:?}"
        );
    });
}

#[test]
fn repeat_loops_then_continues() {
    let mut b = Builder::new();
    let start = b.node(ids::START, json!({}));
    let repeat = b.node(ids::REPEAT, json!({ "mode": "count", "count": 3 }));
    let click = b.click();
    let after = b.tap(Key::Enter);
    b.flow(start, ports::OUT, repeat).flow(repeat, ports::LOOP, click).flow(repeat, ports::DONE, after);

    on_all(&b.project, start, SimHost::new, |kind, outcome| {
        assert_eq!(outcome.clicks(), 3, "{kind:?}");
        assert_eq!(outcome.taps(), [Key::Enter], "{kind:?}");
        assert!(matches!(outcome.sent.last(), Some(InputEvent::KeyUp { key: Key::Enter })), "{kind:?}");
    });
}

#[test]
fn nested_repeats_multiply() {
    let mut b = Builder::new();
    let start = b.node(ids::START, json!({}));
    let outer = b.node(ids::REPEAT, json!({ "mode": "count", "count": 2 }));
    let inner = b.node(ids::REPEAT, json!({ "mode": "count", "count": 3 }));
    let click = b.click();
    let between = b.tap(Key::Tab);
    b.flow(start, ports::OUT, outer).flow(outer, ports::LOOP, inner).flow(inner, ports::LOOP, click).flow(
        inner,
        ports::DONE,
        between,
    );

    on_all(&b.project, start, SimHost::new, |kind, outcome| {
        assert_eq!(outcome.clicks(), 6, "{kind:?}");
        assert_eq!(outcome.taps(), [Key::Tab, Key::Tab], "{kind:?}");
    });
}

/// `Start › Decision`, with one case fed by Image Detection.
fn decision_project() -> (Project, NodeId) {
    let mut b = Builder::new();
    let image = add_target_asset(&mut b.project);
    let start = b.node(ids::START, json!({}));
    let find = b.node(ids::FIND_IMAGE, json!({ "image": image, "threshold": 0.8 }));
    let decision = b.node(ids::DECISION, json!({}));
    let seen = b.tap(Key::KeyY);
    let unseen = b.tap(Key::KeyN);
    b.flow(start, ports::OUT, decision).wire(find, "found", decision, "cond:1").flow(decision, "case:1", seen).flow(
        decision,
        ports::ELSE,
        unseen,
    );
    (b.project, start)
}

#[test]
fn decision_follows_what_is_on_screen() {
    let (project, start) = decision_project();
    on_all(
        &project,
        start,
        || screen(Some(Point::new(133, 71))),
        |kind, outcome| {
            assert_eq!(outcome.taps(), [Key::KeyY], "{kind:?}: the image is there");
            assert!(outcome.errors.is_empty(), "{kind:?}: {:?}", outcome.errors);
        },
    );
    on_all(
        &project,
        start,
        || screen(None),
        |kind, outcome| {
            assert_eq!(outcome.taps(), [Key::KeyN], "{kind:?}: the image is not there");
        },
    );
}

#[test]
fn decision_takes_the_first_true_case() {
    let mut b = Builder::new();
    let image = add_target_asset(&mut b.project);
    let start = b.node(ids::START, json!({}));
    let find = b.node(ids::FIND_IMAGE, json!({ "image": image }));
    let not = b.node(ids::GATE, json!({ "op": "not" }));
    let decision = b.node(
        ids::DECISION,
        json!({ "cases": [
            { "id": "a", "label": "missing" },
            { "id": "b", "label": "present" },
            { "id": "c", "label": "also present" },
        ]}),
    );
    let (first, second, third) = (b.tap(Key::Digit1), b.tap(Key::Digit2), b.tap(Key::Digit3));
    b.flow(start, ports::OUT, decision)
        .wire(find, "found", not, "a")
        .wire(not, "out", decision, "cond:a")
        .wire(find, "found", decision, "cond:b")
        .wire(find, "found", decision, "cond:c")
        .flow(decision, "case:a", first)
        .flow(decision, "case:b", second)
        .flow(decision, "case:c", third);

    on_all(
        &b.project,
        start,
        || screen(Some(Point::new(10, 10))),
        |kind, outcome| {
            assert_eq!(outcome.taps(), [Key::Digit2], "{kind:?}");
        },
    );
}

#[test]
fn a_found_position_can_be_clicked() {
    let mut b = Builder::new();
    let image = add_target_asset(&mut b.project);
    let start = b.node(ids::START, json!({}));
    let find = b.node(ids::FIND_IMAGE, json!({ "image": image, "region": Rect::new(100, 40, 200, 150) }));
    let click = b.click();
    b.flow(start, ports::OUT, click).wire(find, "position", click, "position");

    // The 24×16 image at (133, 71) has its centre at (145, 79).
    on_all(
        &b.project,
        start,
        || screen(Some(Point::new(133, 71))),
        |kind, outcome| {
            assert_eq!(outcome.sent.first(), Some(&InputEvent::MoveTo { x: 145, y: 79 }), "{kind:?}");
            assert_eq!(outcome.clicks(), 1, "{kind:?}");
        },
    );
    // Nothing to click: the block is skipped rather than clicking somewhere wrong.
    on_all(
        &b.project,
        start,
        || screen(None),
        |kind, outcome| {
            assert!(outcome.sent.is_empty(), "{kind:?}: {:?}", outcome.sent);
            assert!(outcome.logs.iter().any(|line| line.contains("No position")), "{kind:?}");
        },
    );
}

#[test]
fn accuracy_100_demands_identical_pixels() {
    let mut b = Builder::new();
    let image = add_target_asset(&mut b.project);
    let start = b.node(ids::START, json!({}));
    let find = b.node(ids::FIND_IMAGE, json!({ "image": image, "threshold": 1.0 }));
    let decision = b.node(ids::DECISION, json!({}));
    let seen = b.tap(Key::KeyY);
    b.flow(start, ports::OUT, decision).wire(find, "found", decision, "cond:1").flow(decision, "case:1", seen);

    let exact = || screen(Some(Point::new(50, 50)));
    let off_by_one_pixel = || {
        let host = screen(Some(Point::new(50, 50)));
        let mut frame = magi_core::host::Host::capture(&host, None).unwrap();
        let index = ((55 * frame.width + 55) * 4) as usize;
        frame.rgba[index] ^= 0x40;
        host.set_screen(Some(frame));
        host
    };
    on_all(&b.project, start, exact, |kind, outcome| assert_eq!(outcome.taps(), [Key::KeyY], "{kind:?}"));
    on_all(&b.project, start, off_by_one_pixel, |kind, outcome| assert!(outcome.taps().is_empty(), "{kind:?}"));
}

#[test]
fn color_detection_finds_a_pixel() {
    let mut b = Builder::new();
    let start = b.node(ids::START, json!({}));
    let find = b.node(ids::FIND_COLOR, json!({ "color": "#E63C28", "tolerance": 4 }));
    let click = b.click();
    b.flow(start, ports::OUT, click).wire(find, "position", click, "position");

    on_all(
        &b.project,
        start,
        || screen(Some(Point::new(200, 100))),
        |kind, outcome| {
            assert_eq!(outcome.sent.first(), Some(&InputEvent::MoveTo { x: 200, y: 100 }), "{kind:?}");
        },
    );
}

#[test]
fn wait_until_waits_and_times_out() {
    let mut b = Builder::new();
    let image = add_target_asset(&mut b.project);
    let start = b.node(ids::START, json!({}));
    let find = b.node(ids::FIND_IMAGE, json!({ "image": image }));
    let wait = b.node(ids::WAIT_UNTIL, json!({ "poll_ms": 5.0, "timeout_ms": 60.0 }));
    let appeared = b.tap(Key::KeyY);
    let gave_up = b.tap(Key::KeyN);
    b.flow(start, ports::OUT, wait).wire(find, "found", wait, "condition").flow(wait, ports::OUT, appeared).flow(
        wait,
        ports::TIMEOUT,
        gave_up,
    );

    on_all(
        &b.project,
        start,
        || screen(None),
        |kind, outcome| {
            assert_eq!(outcome.taps(), [Key::KeyN], "{kind:?}");
        },
    );

    // The image shows up while the macro is waiting.
    for kind in engines() {
        let host = Arc::new(screen(None));
        let monitor = Arc::new(Monitor::new());
        let mut project = b.project.clone();
        project.graph.node_mut(wait).unwrap().set_param("timeout_ms", 0.0);
        let session = session(kind, &project, Arc::clone(&host), Arc::clone(&monitor));
        session.start(start).unwrap();
        std::thread::sleep(Duration::from_millis(60));
        assert!(host.sent().is_empty(), "{kind:?}: still waiting");
        let appeared_screen = screen(Some(Point::new(30, 30)));
        host.set_screen(Some(magi_core::host::Host::capture(&appeared_screen, None).unwrap()));
        assert!(session.wait_idle(Some(TIMEOUT)), "{kind:?}");
        assert_eq!(collect(&host, &monitor).taps(), [Key::KeyY], "{kind:?}");
    }
}

#[test]
fn stopping_releases_held_keys() {
    let mut b = Builder::new();
    let start = b.node(ids::START, json!({}));
    let hold = b.node(ids::INPUT, json!({ "actions": [Action::Key { key: Key::KeyW, mode: PressMode::Press }] }));
    let forever = b.node(ids::REPEAT, json!({ "mode": "forever" }));
    let wait = b.node(ids::DELAY, json!({ "ms": 5.0 }));
    b.chain(&[start, hold, forever]).flow(forever, ports::LOOP, wait);

    for kind in engines() {
        let host = Arc::new(SimHost::new());
        let monitor = Arc::new(Monitor::new());
        let session = session(kind, &b.project, Arc::clone(&host), Arc::clone(&monitor));
        session.start(start).unwrap();
        std::thread::sleep(Duration::from_millis(80));
        assert!(session.is_running(start), "{kind:?}: a forever loop keeps running");
        session.stop_all();
        assert!(session.wait_idle(Some(TIMEOUT)), "{kind:?}: stop was ignored");
        assert_eq!(
            host.sent(),
            vec![InputEvent::KeyDown { key: Key::KeyW }, InputEvent::KeyUp { key: Key::KeyW }],
            "{kind:?}: W must be let go when the macro is stopped"
        );
        assert!(collect(&host, &monitor).errors.is_empty(), "{kind:?}: stopping is not an error");
    }
}

#[test]
fn stop_block_ends_one_run_or_all() {
    let mut b = Builder::new();
    // Run A stops everything after a moment; run B would otherwise loop forever.
    let start_a = b.node(ids::START, json!({}));
    let wait_a = b.node(ids::DELAY, json!({ "ms": 40.0 }));
    let stop = b.node(ids::STOP, json!({ "scope": "all" }));
    b.chain(&[start_a, wait_a, stop]);

    let start_b = b.node(ids::START, json!({}));
    let forever = b.node(ids::REPEAT, json!({ "mode": "forever" }));
    let wait_b = b.node(ids::DELAY, json!({ "ms": 5.0 }));
    b.flow(start_b, ports::OUT, forever).flow(forever, ports::LOOP, wait_b);

    for kind in engines() {
        let host = Arc::new(SimHost::new());
        let session = session(kind, &b.project, host, Arc::new(Monitor::new()));
        assert_eq!(session.start_all().unwrap(), 2, "{kind:?}");
        assert!(session.wait_idle(Some(TIMEOUT)), "{kind:?}: Stop (all) must end the other run too");
    }
}

#[test]
fn disabled_blocks_are_skipped() {
    let mut b = Builder::new();
    let start = b.node(ids::START, json!({}));
    let (a, skipped, c) = (b.tap(Key::KeyA), b.tap(Key::KeyB), b.tap(Key::KeyC));
    b.chain(&[start, a, skipped, c]);
    b.project.graph.node_mut(skipped).unwrap().enabled = false;

    on_all(&b.project, start, SimHost::new, |kind, outcome| {
        assert_eq!(outcome.taps(), [Key::KeyA, Key::KeyC], "{kind:?}");
    });
}

#[test]
fn recordings_replay_press_and_release_in_time() {
    let mut recorder = RecordingBuilder::new(RecordOptions::default(), vec![], Point::new(5, 5), Default::default());
    recorder.push(0.0, &InputEvent::KeyDown { key: Key::KeyA });
    recorder.push(20.0, &InputEvent::KeyDown { key: Key::KeyB });
    recorder.push(40.0, &InputEvent::KeyUp { key: Key::KeyA });
    recorder.push(60.0, &InputEvent::ButtonDown { button: MouseButton::Left });
    recorder.push(70.0, &InputEvent::KeyUp { key: Key::KeyB });
    recorder.push(80.0, &InputEvent::ButtonUp { button: MouseButton::Left });
    let recording = recorder.finish(100.0, None);

    let mut b = Builder::new();
    let asset = b.project.assets.add_recording("take 1", recording);
    let start = b.node(ids::START, json!({}));
    let play = b.node(ids::PLAY_RECORDING, json!({ "recording": asset, "speed": 2.0 }));
    b.chain(&[start, play]);

    for kind in engines() {
        let began = std::time::Instant::now();
        let outcome = run_on(kind, &b.project, start, SimHost::new());
        let elapsed = began.elapsed();
        assert_eq!(
            outcome.sent,
            vec![
                InputEvent::KeyDown { key: Key::KeyA },
                InputEvent::KeyDown { key: Key::KeyB },
                InputEvent::KeyUp { key: Key::KeyA },
                InputEvent::MoveTo { x: 5, y: 5 },
                InputEvent::ButtonDown { button: MouseButton::Left },
                InputEvent::KeyUp { key: Key::KeyB },
                InputEvent::ButtonUp { button: MouseButton::Left },
            ],
            "{kind:?}"
        );
        // 100 ms of recording at 2× speed.
        assert!(elapsed >= Duration::from_millis(50), "{kind:?}: finished too early ({elapsed:?})");
    }
}

#[test]
fn the_auto_clicker_graph_clicks_the_requested_number_of_times() {
    let settings = ClickerSettings {
        interval_ms: 10.0,
        hold_ms: 0.5,
        limit: ClickLimit::Count { count: 25 },
        ..ClickerSettings::default()
    };
    let config = AutoClickerConfig { settings, ..AutoClickerConfig::default() };
    let mut project = Project::new();
    project.graph = config.to_graph(&registry());
    let start = project.graph.starts().next().unwrap().id;

    for kind in engines() {
        let began = std::time::Instant::now();
        let outcome = run_on(kind, &project, start, SimHost::new());
        let elapsed = began.elapsed();
        assert_eq!(outcome.clicks(), 25, "{kind:?}");
        // Course correction keeps 25 clicks at 100/s to about a quarter of a second.
        assert!(elapsed >= Duration::from_millis(230), "{kind:?}: too fast ({elapsed:?})");
        assert!(elapsed < Duration::from_millis(600), "{kind:?}: too slow ({elapsed:?})");
    }
}

fn clicker_project(change: impl FnOnce(&mut AutoClickerConfig)) -> (Project, NodeId) {
    let settings = ClickerSettings {
        interval_ms: 10.0,
        hold_ms: 1.0,
        limit: ClickLimit::Count { count: 4 },
        ..ClickerSettings::default()
    };
    let mut config = AutoClickerConfig { settings, ..AutoClickerConfig::default() };
    change(&mut config);
    let mut project = Project::new();
    project.graph = config.to_graph(&registry());
    let start = project.graph.starts().next().unwrap().id;
    (project, start)
}

#[test]
fn dual_keys_alternate_or_go_together() {
    let (project, start) = clicker_project(|c| {
        c.target = ClickTarget::DualKeys { first: Key::KeyA, second: Key::KeyD, mode: DualMode::Alternate };
    });
    on_all(&project, start, SimHost::new, |kind, outcome| {
        assert_eq!(outcome.taps(), [Key::KeyA, Key::KeyD, Key::KeyA, Key::KeyD], "{kind:?}");
    });

    let (project, start) = clicker_project(|c| {
        c.target = ClickTarget::DualKeys { first: Key::KeyA, second: Key::KeyD, mode: DualMode::Together };
        c.limit = ClickLimit::Count { count: 1 };
    });
    on_all(&project, start, SimHost::new, |kind, outcome| {
        assert_eq!(
            outcome.sent,
            vec![
                InputEvent::KeyDown { key: Key::KeyA },
                InputEvent::KeyDown { key: Key::KeyD },
                InputEvent::KeyUp { key: Key::KeyD },
                InputEvent::KeyUp { key: Key::KeyA },
            ],
            "{kind:?}"
        );
    });
}

#[test]
fn double_clicks_and_fixed_positions() {
    let (project, start) = clicker_project(|c| {
        c.clicks = 2;
        c.double_click_delay_ms = 5.0;
        c.position = ClickPosition::Fixed { x: 40, y: 50, display: None };
        c.limit = ClickLimit::Count { count: 2 };
    });
    on_all(&project, start, SimHost::new, |kind, outcome| {
        assert_eq!(outcome.clicks(), 4, "{kind:?}");
        assert_eq!(outcome.sent.first(), Some(&InputEvent::MoveTo { x: 40, y: 50 }), "{kind:?}");
    });
}

#[test]
fn the_time_limit_and_corner_stop_end_clicking() {
    let (project, start) = clicker_project(|c| {
        c.limit = ClickLimit::Forever;
        c.time_limit = Toggled { on: true, value: 60.0 };
    });
    on_all(&project, start, SimHost::new, |kind, outcome| {
        let clicks = outcome.clicks();
        assert!((4..=9).contains(&clicks), "{kind:?}: {clicks} clicks in 60 ms at 100/s");
        assert!(outcome.logs.iter().any(|l| l.contains("time limit")), "{kind:?}");
    });

    let (project, start) = clicker_project(|c| {
        c.limit = ClickLimit::Forever;
        c.corner_stop = Toggled { on: true, value: [25; 4] };
    });
    // The pointer starts in the top-left corner of a 320×200 screen.
    on_all(
        &project,
        start,
        || screen(None),
        |kind, outcome| {
            assert_eq!(outcome.clicks(), 0, "{kind:?}");
            assert!(outcome.logs.iter().any(|l| l.contains("top-left corner")), "{kind:?}");
        },
    );
}

#[test]
fn process_rules_stop_or_pause() {
    let (project, start) = clicker_project(|c| c.limit = ClickLimit::Forever);
    let rules = |action| {
        Arc::new(RwLock::new(ProcessRules {
            enabled: true,
            mode: RuleMode::Blacklist,
            blacklist: vec![ProcessRule { name: "chat.exe".into(), enabled: true, action }],
            ..ProcessRules::default()
        }))
    };

    for kind in engines() {
        // Stop: nothing reaches a blacklisted program, and the run ends.
        let host = Arc::new(SimHost::new());
        host.set_foreground(Some("Chat.exe"));
        let monitor = Arc::new(Monitor::new());
        let stopping =
            session(kind, &project, Arc::clone(&host), Arc::clone(&monitor)).with_rules(rules(RuleAction::Stop));
        stopping.start(start).unwrap();
        assert!(stopping.wait_idle(Some(TIMEOUT)), "{kind:?}");
        assert!(host.sent().is_empty(), "{kind:?}");

        // Pause: waits while the program is in front, carries on when it is not.
        let host = Arc::new(SimHost::new());
        host.set_foreground(Some("chat.exe"));
        let monitor = Arc::new(Monitor::new());
        let pausing =
            session(kind, &project, Arc::clone(&host), Arc::clone(&monitor)).with_rules(rules(RuleAction::Pause));
        pausing.start(start).unwrap();
        std::thread::sleep(Duration::from_millis(120));
        assert!(host.sent().is_empty(), "{kind:?}: paused");
        host.set_foreground(Some("game.exe"));
        std::thread::sleep(Duration::from_millis(250));
        pausing.stop_all();
        assert!(pausing.wait_idle(Some(TIMEOUT)), "{kind:?}");
        assert!(!host.sent().is_empty(), "{kind:?}: resumed");
    }
}

#[test]
fn switch_to_window_finds_reports_and_falls_through() {
    let mut b = Builder::new();
    let start = b.node(ids::START, json!({}));
    let focus = b.node(ids::FOCUS_WINDOW, json!({ "target": "notepad", "match_by": "process", "timeout_ms": 0.0 }));
    let found = b.node(ids::LOG, json!({ "message": "screen" }));
    let missing = b.tap(Key::KeyM);
    b.flow(start, ports::OUT, focus).flow(focus, ports::OUT, found).flow(focus, "missing", missing);
    b.wire(focus, "screen", found, "value");

    let with_window = || {
        let host = SimHost::new();
        host.set_windows(vec![WindowInfo {
            title: "Untitled - Notepad".into(),
            process: "Notepad.exe".into(),
            kind: ScreenKind::Windowed,
            foreground: false,
        }]);
        host
    };
    on_all(&b.project, start, with_window, |kind, outcome| {
        assert_eq!(outcome.logs, ["screen windowed"], "{kind:?}");
        assert!(outcome.taps().is_empty(), "{kind:?}");
    });
    on_all(&b.project, start, SimHost::new, |kind, outcome| {
        assert_eq!(outcome.taps(), [Key::KeyM], "{kind:?}");
    });
}

#[test]
fn recordings_can_replay_movement_as_relative_motion() {
    let recording = Recording {
        duration_ms: 20.0,
        screen: None,
        strokes: vec![
            Stroke {
                at: 0.0,
                hold: 4.0,
                input: StrokeInput::Move { path: vec![PathPoint(0.0, 100, 100), PathPoint(4.0, 130, 90)] },
            },
            Stroke {
                at: 6.0,
                hold: 2.0,
                input: StrokeInput::Look { path: vec![LookPoint(0.0, 5, -5), LookPoint(2.0, 5, 0)] },
            },
        ],
    };
    let mut b = Builder::new();
    let asset = b.project.assets.add_recording("look", recording);
    let start = b.node(ids::START, json!({}));
    let play = b.node(ids::PLAY_RECORDING, json!({ "recording": asset, "mouse": "relative" }));
    b.chain(&[start, play]);

    on_all(&b.project, start, SimHost::new, |kind, outcome| {
        assert_eq!(
            outcome.sent,
            vec![
                InputEvent::MoveBy { dx: 30, dy: -10 },
                InputEvent::MoveBy { dx: 5, dy: -5 },
                InputEvent::MoveBy { dx: 5, dy: 0 },
            ],
            "{kind:?}"
        );
    });
}

#[test]
fn problems_are_reported_against_the_block() {
    let mut b = Builder::new();
    let start = b.node(ids::START, json!({}));
    let play = b.node(ids::PLAY_RECORDING, json!({})); // no recording chosen
    b.chain(&[start, play]);

    for kind in engines() {
        let monitor = Arc::new(Monitor::new());
        let session = session(kind, &b.project, Arc::new(SimHost::new()), Arc::clone(&monitor));
        session.start(start).unwrap();
        assert!(session.wait_idle(Some(TIMEOUT)));
        let errors: Vec<_> = monitor.logs().into_iter().filter(|e| e.level == LogLevel::Error).collect();
        assert_eq!(errors.len(), 1, "{kind:?}");
        assert_eq!(errors[0].node, Some(play), "{kind:?}");
        assert!(errors[0].message.contains("no recording"), "{kind:?}: {}", errors[0].message);
    }
}

// ---- engine-specific -------------------------------------------------------

#[test]
fn python_scripts_are_skipped_with_a_warning_on_the_native_engine() {
    let mut b = Builder::new();
    let start = b.node(ids::START, json!({}));
    let script = b.node(ids::PYTHON, json!({ "code": "result = 1" }));
    let after = b.tap(Key::KeyY);
    b.chain(&[start, script, after]);
    let program = Program::from_project(&b.project, registry());
    assert!(program.needs_python());

    let host = Arc::new(SimHost::new());
    let monitor = Arc::new(Monitor::new());
    let session = session(EngineKind::Native, &b.project, Arc::clone(&host), Arc::clone(&monitor));
    session.start(start).unwrap();
    assert!(session.wait_idle(Some(TIMEOUT)));
    let outcome = collect(&host, &monitor);
    assert_eq!(outcome.taps(), [Key::KeyY], "the rest of the macro still runs");
    assert!(outcome.errors.is_empty(), "{:?}", outcome.errors);
    let warning = monitor.logs().into_iter().find(|e| e.level == LogLevel::Warning).expect("a warning");
    assert!(warning.message.contains("Python engine") && warning.node == Some(script));
}

#[test]
fn python_scripts_can_act_and_feed_decisions() {
    if python().is_none() {
        return;
    }
    let mut b = Builder::new();
    let start = b.node(ids::START, json!({}));
    let script = b.node(
        ids::PYTHON,
        json!({ "code": "\
magi.vars['count'] = magi.vars.get('count', 0) + 1
magi.press('ctrl', 'a')
magi.click(40, 50, button='right')
magi.type('héllo')
print('printed from the script')
magi.log('cursor', magi.cursor())
result = magi.vars['count'] >= 1
" }),
    );
    let decision = b.node(ids::DECISION, json!({}));
    let yes = b.tap(Key::KeyY);
    b.chain(&[start, script, decision]).wire(script, "result", decision, "cond:1").flow(decision, "case:1", yes);

    let outcome = run_on(EngineKind::Python, &b.project, start, SimHost::new());
    assert!(outcome.errors.is_empty(), "{:?}", outcome.errors);
    assert_eq!(
        outcome.sent,
        vec![
            InputEvent::KeyDown { key: Key::ControlLeft },
            InputEvent::KeyDown { key: Key::KeyA },
            InputEvent::KeyUp { key: Key::KeyA },
            InputEvent::KeyUp { key: Key::ControlLeft },
            InputEvent::MoveTo { x: 40, y: 50 },
            InputEvent::ButtonDown { button: MouseButton::Right },
            InputEvent::ButtonUp { button: MouseButton::Right },
            InputEvent::Text { text: "héllo".into() },
            InputEvent::KeyDown { key: Key::KeyY },
            InputEvent::KeyUp { key: Key::KeyY },
        ]
    );
    assert!(outcome.logs.iter().any(|l| l == "printed from the script"), "{:?}", outcome.logs);
    assert!(outcome.logs.iter().any(|l| l == "cursor (40, 50)"), "{:?}", outcome.logs);
}

#[test]
fn python_script_errors_name_the_line() {
    if python().is_none() {
        return;
    }
    let mut b = Builder::new();
    let start = b.node(ids::START, json!({}));
    let script = b.node(ids::PYTHON, json!({ "code": "x = 1\ny = x / 0\n" }));
    b.chain(&[start, script]);

    let outcome = run_on(EngineKind::Python, &b.project, start, SimHost::new());
    assert_eq!(outcome.errors.len(), 1);
    assert!(
        outcome.errors[0].contains("line 2") && outcome.errors[0].contains("ZeroDivisionError"),
        "{:?}",
        outcome.errors
    );
}

#[test]
fn python_scripts_find_images_by_name() {
    if python().is_none() {
        return;
    }
    let mut b = Builder::new();
    add_target_asset(&mut b.project);
    let start = b.node(ids::START, json!({}));
    let script = b.node(
        ids::PYTHON,
        json!({ "code": "\
seen = magi.find_image('target')
if seen:
    magi.click(seen.x, seen.y)
" }),
    );
    b.chain(&[start, script]);

    let outcome = run_on(EngineKind::Python, &b.project, start, screen(Some(Point::new(133, 71))));
    assert!(outcome.errors.is_empty(), "{:?}", outcome.errors);
    assert_eq!(outcome.sent.first(), Some(&InputEvent::MoveTo { x: 145, y: 79 }));
}

#[test]
fn a_missing_interpreter_is_reported_not_fatal() {
    let dir = std::env::temp_dir().join(format!("magi-engine-missing-{}", std::process::id()));
    let config = PythonConfig {
        interpreter: Some(PathBuf::from("definitely-not-a-python-interpreter")),
        app_dir: dir.clone(),
        data_dir: dir.clone(),
    };
    let runtime = PythonRuntime::new(config, Arc::new(Monitor::new()));
    assert!(matches!(runtime.probe(), PythonStatus::Unavailable(_)));
    let _ = std::fs::remove_dir_all(dir);
}

// ---- variables, counters and calculations ------------------------------------

fn with_variables(b: &mut Builder, variables: &[(&str, serde_json::Value)]) {
    b.project.variables = variables.iter().map(|(name, value)| Variable::new(*name, value.clone())).collect();
}

#[test]
fn bound_settings_follow_their_variable() {
    let mut b = Builder::new();
    with_variables(&mut b, &[("greeting", json!("hello"))]);
    let start = b.node(ids::START, json!({}));
    let first = b.node(ids::LOG, json!({ "message": binding("greeting") }));
    let change = b.node(ids::SET_VARIABLE, json!({ "variable": "greeting", "operation": "set", "value": "bye" }));
    let second = b.node(ids::LOG, json!({ "message": binding("greeting") }));
    b.chain(&[start, first, change, second]);
    on_all(&b.project, start, SimHost::new, |kind, outcome| {
        assert_eq!(outcome.logs, ["hello", "bye"], "{kind:?}");
    });
    // The next run starts from the starting value again.
    on_all(&b.project, start, SimHost::new, |kind, outcome| {
        assert_eq!(outcome.logs, ["hello", "bye"], "{kind:?}");
    });
}

#[test]
fn counters_reach_their_limit_and_start_again() {
    let mut b = Builder::new();
    let start = b.node(ids::START, json!({}));
    let repeat = b.node(ids::REPEAT, json!({ "mode": "count", "count": 5 }));
    let counter = b.node(ids::COUNTER, json!({ "variable": "n", "step": 1.0, "limit": 3.0 }));
    let reached = b.log("three");
    let shown = b.log("count");
    b.flow(start, ports::OUT, repeat).flow(repeat, ports::LOOP, counter);
    b.flow(counter, "reached", reached).flow(counter, ports::OUT, shown);
    b.wire(counter, "count", shown, "value");
    on_all(&b.project, start, SimHost::new, |kind, outcome| {
        assert_eq!(outcome.logs, ["count 1", "count 2", "three", "count 1", "count 2"], "{kind:?}");
    });
}

#[test]
fn math_compare_and_text_calculate_the_same_everywhere() {
    let mut b = Builder::new();
    let start = b.node(ids::START, json!({}));
    let sum = b.node(ids::MATH, json!({ "operation": "add", "a": 2.0, "b": 3.0 }));
    let log_sum = b.log("sum");
    let joined = b.node(ids::TEXT, json!({ "operation": "join", "a": "ab", "b": "cd" }));
    let contains = b.node(ids::COMPARE, json!({ "operation": "contains", "b": "BC" }));
    let decide = b.node(ids::DECISION, json!({}));
    let yes = b.tap(Key::KeyY);
    let no = b.tap(Key::KeyN);
    b.chain(&[start, log_sum, decide]);
    b.wire(sum, ports::OUT, log_sum, "value");
    b.wire(joined, ports::OUT, contains, "a");
    b.wire(contains, ports::OUT, decide, "cond:1");
    b.flow(decide, "case:1", yes).flow(decide, ports::ELSE, no);
    on_all(&b.project, start, SimHost::new, |kind, outcome| {
        assert_eq!(outcome.logs, ["sum 5"], "{kind:?}");
        assert_eq!(outcome.taps(), [Key::KeyY], "{kind:?}: “abcd” contains “BC”, ignoring case");
    });
}

#[test]
fn repeat_until_checks_its_condition_before_every_round() {
    let mut b = Builder::new();
    with_variables(&mut b, &[("n", json!(0.0))]);
    let start = b.node(ids::START, json!({}));
    let repeat = b.node(ids::REPEAT, json!({ "mode": "until" }));
    let n = b.node(ids::VARIABLE, json!({ "variable": "n" }));
    let done = b.node(ids::COMPARE, json!({ "operation": "greater_or_equal", "b": "3" }));
    let counter = b.node(ids::COUNTER, json!({ "variable": "n" }));
    let round = b.log("round");
    let after = b.log("after");
    b.flow(start, ports::OUT, repeat).flow(repeat, ports::LOOP, counter).flow(counter, ports::OUT, round);
    b.flow(repeat, ports::DONE, after);
    b.wire(n, ports::OUT, done, "a").wire(done, ports::OUT, repeat, "condition");
    on_all(&b.project, start, SimHost::new, |kind, outcome| {
        assert_eq!(outcome.logs, ["round", "round", "round", "after"], "{kind:?}");
    });
}

#[test]
fn stop_can_skip_a_round_leave_a_loop_or_jump() {
    // Skip the second round.
    let mut b = Builder::new();
    let start = b.node(ids::START, json!({}));
    let repeat = b.node(ids::REPEAT, json!({ "mode": "count", "count": 4 }));
    let counter = b.node(ids::COUNTER, json!({ "variable": "c" }));
    let second = b.node(ids::COMPARE, json!({ "operation": "equal", "b": "2" }));
    let decide = b.node(ids::DECISION, json!({}));
    let skip = b.node(ids::STOP, json!({ "scope": "next" }));
    let key = b.tap(Key::KeyK);
    b.flow(start, ports::OUT, repeat).flow(repeat, ports::LOOP, counter).flow(counter, ports::OUT, decide);
    b.wire(counter, "count", second, "a").wire(second, ports::OUT, decide, "cond:1");
    b.flow(decide, "case:1", skip).flow(decide, ports::ELSE, key);
    on_all(&b.project, start, SimHost::new, |kind, outcome| {
        assert_eq!(outcome.taps(), [Key::KeyK; 3], "{kind:?}");
    });

    // Leave a loop that would run for ever.
    let mut b = Builder::new();
    let start = b.node(ids::START, json!({}));
    let repeat = b.node(ids::REPEAT, json!({ "mode": "forever" }));
    let counter = b.node(ids::COUNTER, json!({ "variable": "c" }));
    let third = b.node(ids::COMPARE, json!({ "operation": "greater_or_equal", "b": "3" }));
    let decide = b.node(ids::DECISION, json!({}));
    let leave = b.node(ids::STOP, json!({ "scope": "exit_loop" }));
    let key = b.tap(Key::KeyK);
    let after = b.tap(Key::KeyD);
    b.flow(start, ports::OUT, repeat).flow(repeat, ports::LOOP, counter).flow(counter, ports::OUT, decide);
    b.flow(repeat, ports::DONE, after);
    b.wire(counter, "count", third, "a").wire(third, ports::OUT, decide, "cond:1");
    b.flow(decide, "case:1", leave).flow(decide, ports::ELSE, key);
    on_all(&b.project, start, SimHost::new, |kind, outcome| {
        assert_eq!(outcome.taps(), [Key::KeyK, Key::KeyK, Key::KeyD], "{kind:?}");
    });

    // Jump to a block nothing is wired to.
    let mut b = Builder::new();
    let start = b.node(ids::START, json!({}));
    let target = b.log("jumped");
    let jump = b.node(ids::STOP, json!({ "scope": "go_to", "target": target }));
    b.chain(&[start, jump]);
    on_all(&b.project, start, SimHost::new, |kind, outcome| {
        assert_eq!(outcome.logs, ["jumped"], "{kind:?}");
    });
}

#[test]
fn rate_blocks_press_their_steps_a_set_time_apart() {
    let mut b = Builder::new();
    let start = b.node(ids::START, json!({}));
    let steps = [Action::tap(Key::KeyA), Action::tap(Key::KeyD), Action::tap(Key::KeyW)];
    let rate = b.node(ids::RATE, json!({ "rate_ms": 20.0, "hold_ms": 1.0, "steps": steps }));
    b.chain(&[start, rate]);
    for kind in engines() {
        let began = std::time::Instant::now();
        let outcome = run_on(kind, &b.project, start, SimHost::new());
        assert_eq!(outcome.taps(), [Key::KeyA, Key::KeyD, Key::KeyW], "{kind:?}");
        assert!(began.elapsed() >= Duration::from_millis(40), "{kind:?}: two gaps of 20 ms");
    }
}

#[test]
fn function_blocks_run_a_definition_and_pass_it_a_value() {
    let mut b = Builder::new();
    let define = b.node(ids::DEFINE, json!({ "name": "greet", "input_variable": "who" }));
    let greet = b.node(ids::LOG, json!({ "message": binding("who") }));
    b.chain(&[define, greet]);
    let start = b.node(ids::START, json!({}));
    let call = b.node(ids::CALL, json!({ "definition": "greet" }));
    let name = b.node(ids::TEXT, json!({ "operation": "join", "a": "Ada", "b": "" }));
    let after = b.log("after");
    b.chain(&[start, call, after]);
    b.wire(name, ports::OUT, call, "input");
    on_all(&b.project, start, SimHost::new, |kind, outcome| {
        assert_eq!(outcome.logs, ["Ada", "after"], "{kind:?}");
        assert!(outcome.errors.is_empty(), "{kind:?}: {:?}", outcome.errors);
    });
}

#[test]
fn the_clipboard_and_notifications_reach_the_machine() {
    let mut b = Builder::new();
    let start = b.node(ids::START, json!({}));
    let clipboard = b.node(ids::CLIPBOARD, json!({}));
    let keep = b.node(ids::SET_VARIABLE, json!({ "variable": "clip" }));
    let read = b.node(ids::VARIABLE, json!({ "variable": "clip" }));
    let notify = b.node(ids::NOTIFY, json!({ "title": "T", "message": "Copied", "sound": false }));
    b.chain(&[start, keep, notify]);
    b.wire(clipboard, ports::OUT, keep, "value").wire(read, ports::OUT, notify, "value");
    for kind in engines() {
        let host = Arc::new(SimHost::new());
        host.set_clipboard("hello");
        let monitor = Arc::new(Monitor::new());
        let session = session(kind, &b.project, Arc::clone(&host), monitor);
        session.start(start).unwrap();
        assert!(session.wait_idle(Some(TIMEOUT)), "{kind:?}");
        let shown = host.notifications();
        assert_eq!(shown.len(), 1, "{kind:?}");
        assert_eq!(
            (shown[0].message.as_str(), shown[0].sound, shown[0].popup),
            ("Copied hello", false, true),
            "{kind:?}"
        );
    }
}

// ---- typing and reading text ------------------------------------------------

#[test]
fn the_auto_typer_types_its_text_with_line_breaks_skips_and_limits() {
    let mut b = Builder::new();
    let start = b.node(ids::START, json!({}));
    let fast = json!({ "speed": 1000.0, "variation": 0.0, "punctuation_pause": false });
    let mut first = fast.clone();
    first["text"] = json!(
        "Hi
yo"
    );
    let mut second = fast;
    (second["text"], second["skip"], second["limit"]) = (json!("abcd"), json!(1), json!(2));
    let lines = b.node(ids::AUTOTYPE, first);
    let part = b.node(ids::AUTOTYPE, second);
    let log = b.node(ids::LOG, json!({ "message": "typed" }));
    b.chain(&[start, lines, part, log]);
    b.wire(part, "typed", log, "value");
    on_all(&b.project, start, SimHost::new, |kind, outcome| {
        assert!(outcome.errors.is_empty(), "{kind:?}: {:?}", outcome.errors);
        let typed: Vec<String> = outcome
            .sent
            .iter()
            .filter_map(|e| match e {
                InputEvent::Text { text } => Some(text.clone()),
                InputEvent::KeyDown { key: Key::Enter } => Some("⏎".into()),
                _ => None,
            })
            .collect();
        assert_eq!(typed.concat(), "Hi⏎yobc", "{kind:?}");
        assert_eq!(outcome.logs, ["typed 2"], "{kind:?}");
    });
}

#[test]
fn text_scan_reads_the_words_finds_text_and_gives_the_first_number() {
    use magi_core::host::TextWord;
    let word =
        |text: &str, x: i32, y: i32, line: usize| TextWord { text: text.into(), rect: Rect::new(x, y, 30, 12), line };
    let host = || {
        let host = SimHost::with_screen(Frame::solid(Point::new(0, 0), 200, 100, [0, 0, 0]));
        host.set_text(vec![word("Gold:", 10, 10, 0), word("1,250", 50, 10, 0), word("Play", 10, 40, 1)]);
        host
    };
    let mut b = Builder::new();
    let start = b.node(ids::START, json!({}));
    let scan = b.node(ids::READ_TEXT, json!({ "find": "play" }));
    let text = b.node(ids::LOG, json!({ "message": "read" }));
    let number = b.node(ids::LOG, json!({ "message": "number" }));
    let click = b.node(ids::INPUT, json!({ "actions": [Action::click(MouseButton::Left)] }));
    b.chain(&[start, text, number, click]);
    b.wire(scan, "text", text, "value").wire(scan, "number", number, "value").wire(scan, "position", click, "position");
    on_all(&b.project, start, host, |kind, outcome| {
        assert!(outcome.errors.is_empty(), "{kind:?}: {:?}", outcome.errors);
        assert_eq!(
            outcome.logs,
            [
                "read Gold: 1,250
Play",
                "number 1250"
            ],
            "{kind:?}"
        );
        assert!(
            outcome.sent.contains(&InputEvent::MoveTo { x: 25, y: 46 }),
            "{kind:?}: clicks the middle of “Play”: {:?}",
            outcome.sent
        );
    });
}

// ---- several displays ------------------------------------------------------

fn two_displays() -> SimHost {
    let host = SimHost::new();
    host.set_screens(vec![Rect::new(0, 0, 320, 200), Rect::new(320, 0, 320, 200)]);
    host
}

#[test]
fn positions_on_a_display_land_on_that_display() {
    let mut b = Builder::new();
    let start = b.node(ids::START, json!({}));
    let click = b.node(
        ids::INPUT,
        json!({ "actions": [Action::Mouse { button: MouseButton::Left, mode: PressMode::Tap, clicks: 1, at: Some(Point::on(1, 10, 20)) }], "hold_ms": 1.0 }),
    );
    b.chain(&[start, click]);
    on_all(&b.project, start, two_displays, |kind, outcome| {
        assert_eq!(outcome.sent.first(), Some(&InputEvent::MoveTo { x: 330, y: 20 }), "{kind:?}");
    });
}

#[test]
fn recordings_replay_on_the_display_they_were_made_on() {
    let recording = Recording {
        duration_ms: 10.0,
        screen: None,
        strokes: vec![
            Stroke { at: 0.0, hold: 0.0, input: StrokeInput::Display { index: 1 } },
            Stroke {
                at: 1.0,
                hold: 2.0,
                input: StrokeInput::Mouse { button: MouseButton::Left, x: 5, y: 6, end: None, here: false },
            },
        ],
    };
    let mut b = Builder::new();
    let asset = b.project.assets.add_recording("on two", recording);
    let start = b.node(ids::START, json!({}));
    let play = b.node(ids::PLAY_RECORDING, json!({ "recording": asset }));
    b.chain(&[start, play]);
    on_all(&b.project, start, two_displays, |kind, outcome| {
        assert_eq!(outcome.sent.first(), Some(&InputEvent::MoveTo { x: 325, y: 6 }), "{kind:?}");
    });
}

#[test]
fn runs_that_end_by_themselves_are_reported() {
    let mut b = Builder::new();
    let start = b.node(ids::START, json!({}));
    let wait = b.node(ids::DELAY, json!({ "ms": 1.0 }));
    b.chain(&[start, wait]);
    for kind in engines() {
        let monitor = Arc::new(Monitor::new());
        let session = session(kind, &b.project, Arc::new(SimHost::new()), Arc::clone(&monitor));
        session.start(start).unwrap();
        assert!(session.wait_idle(Some(TIMEOUT)), "{kind:?}");
        let finished = monitor.take_finished();
        assert_eq!(finished.len(), 1, "{kind:?}");
        assert!(finished[0].by_itself && !finished[0].failed, "{kind:?}");
    }
}

// ---- sound -----------------------------------------------------------------

const RATE: u32 = magi_audio::ANALYSIS_RATE;

fn tone(hz: f32, seconds: f32, amplitude: f32) -> Vec<f32> {
    let n = (seconds * RATE as f32) as usize;
    (0..n).map(|i| amplitude * (2.0 * std::f32::consts::PI * hz * i as f32 / RATE as f32).sin()).collect()
}

fn hearing(samples: Vec<f32>) -> SimHost {
    let host = SimHost::new();
    host.set_sound(SoundClip { sample_rate: RATE, end: samples.len() as u64, samples });
    host
}

#[test]
fn sound_detection_reacts_to_loudness_and_pitch() {
    let mut b = Builder::new();
    let start = b.node(ids::START, json!({}));
    let listen = b.node(ids::SOUND_LEVEL, json!({ "threshold_db": -20.0 }));
    let decision = b.node(ids::DECISION, json!({}));
    let heard = b.tap(Key::KeyY);
    let quiet = b.tap(Key::KeyN);
    b.flow(start, ports::OUT, decision)
        .wire(listen, "detected", decision, "cond:1")
        .flow(decision, "case:1", heard)
        .flow(decision, ports::ELSE, quiet);
    let loud = || hearing(tone(440.0, 1.0, 0.5)); // about -9 dB
    on_all(&b.project, start, loud, |kind, outcome| assert_eq!(outcome.taps(), [Key::KeyY], "{kind:?}"));
    on_all(
        &b.project,
        start,
        || hearing(tone(440.0, 1.0, 0.01)),
        |kind, outcome| {
            assert_eq!(outcome.taps(), [Key::KeyN], "{kind:?}: too quiet");
        },
    );

    let mut high = b.project.clone();
    let node = high.graph.node_mut(listen).unwrap();
    node.params.extend(json!({ "use_pitch": true, "min_hz": 1000.0, "max_hz": 3000.0 }).as_object().unwrap().clone());
    on_all(&high, start, loud, |kind, outcome| assert_eq!(outcome.taps(), [Key::KeyN], "{kind:?}: pitch too low"));
    on_all(
        &high,
        start,
        || hearing(tone(1500.0, 1.0, 0.5)),
        |kind, outcome| {
            assert_eq!(outcome.taps(), [Key::KeyY], "{kind:?}");
        },
    );
}

#[test]
fn sound_recognition_hears_an_uploaded_sound_once_each_time_it_plays() {
    let tune: Vec<f32> = [440.0, 660.0, 550.0].iter().flat_map(|&hz| tone(hz, 0.25, 0.5)).collect();
    let mut b = Builder::new();
    let wav = magi_audio::prepare_upload(&magi_audio::encode_wav(&tune, RATE)).unwrap();
    let sound = b.project.assets.add_audio("tune.wav", wav);
    let start = b.node(ids::START, json!({}));
    let repeat = b.node(ids::REPEAT, json!({ "mode": "count", "count": 3 }));
    let listen = b.node(ids::SOUND_MATCH, json!({ "sound": sound }));
    let decision = b.node(ids::DECISION, json!({}));
    let heard = b.tap(Key::KeyY);
    let not_heard = b.tap(Key::KeyN);
    b.flow(start, ports::OUT, repeat).flow(repeat, ports::LOOP, decision);
    b.wire(listen, "found", decision, "cond:1").flow(decision, "case:1", heard).flow(decision, ports::ELSE, not_heard);

    let playing = || {
        let mut samples = vec![0.0; RATE as usize / 2];
        samples.extend(tune.iter().map(|s| s * 0.4));
        samples.extend(vec![0.0; RATE as usize / 10]);
        hearing(samples)
    };
    on_all(&b.project, start, playing, |kind, outcome| {
        assert_eq!(outcome.taps(), [Key::KeyY, Key::KeyN, Key::KeyN], "{kind:?}: heard once, not again");
    });
    let other = || hearing([300.0, 900.0, 1200.0].iter().flat_map(|&hz| tone(hz, 0.25, 0.5)).collect());
    on_all(&b.project, start, other, |kind, outcome| {
        assert_eq!(outcome.taps(), [Key::KeyN; 3], "{kind:?}: a different sound");
    });
}

// ---- several images and sounds in one block ---------------------------------

/// A second picture, unlike the first: green and yellow stripes.
fn other_image() -> (Vec<u8>, u32, u32) {
    let (w, h) = (24u32, 16u32);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for _y in 0..h {
        for x in 0..w {
            rgba.extend_from_slice(if (x / 3) % 2 == 0 { &[40, 200, 60, 255] } else { &[240, 220, 30, 255] });
        }
    }
    (rgba, w, h)
}

fn add_image(project: &mut Project, name: &str, (rgba, w, h): (Vec<u8>, u32, u32)) -> String {
    let png = magi_vision::encode_png(ImageRef::new(w, h, &rgba)).unwrap();
    project.assets.add_image(name, png)
}

/// A screen with the first picture at (30, 30) and, optionally, the second at (200, 120).
fn two_pictures(second: bool) -> SimHost {
    let mut frame = Frame::solid(Point::new(0, 0), 320, 200, [90, 90, 90]);
    let (rgba, w, h) = target_image();
    frame.blit(&Frame::new(Point::new(0, 0), w, h, rgba), Point::new(30, 30));
    if second {
        let (rgba, w, h) = other_image();
        frame.blit(&Frame::new(Point::new(0, 0), w, h, rgba), Point::new(200, 120));
    }
    SimHost::with_screen(frame)
}

/// `Start › Decision`, fed by an Image Detection block with `params`; logs which image was found.
fn images_project(params: impl FnOnce(&str, &str) -> serde_json::Value) -> (Project, NodeId) {
    let mut b = Builder::new();
    let first = add_image(&mut b.project, "checkers", target_image());
    let second = add_image(&mut b.project, "stripes", other_image());
    let start = b.node(ids::START, json!({}));
    let find = b.node(ids::FIND_IMAGE, params(&first, &second));
    let decision = b.node(ids::DECISION, json!({}));
    let seen = b.node(ids::LOG, json!({ "message": "seen" }));
    let unseen = b.tap(Key::KeyN);
    b.flow(start, ports::OUT, decision).wire(find, "found", decision, "cond:1").flow(decision, "case:1", seen).flow(
        decision,
        ports::ELSE,
        unseen,
    );
    b.wire(find, "name", seen, "value");
    (b.project, start)
}

#[test]
fn image_detection_looks_for_several_images_in_order_of_priority() {
    // Any: the stripes come first in the list, but only the checkers are on screen.
    let (project, start) =
        images_project(|first, second| json!({ "images": [{ "image": second }, { "image": first }], "mode": "any" }));
    on_all(
        &project,
        start,
        || two_pictures(false),
        |kind, outcome| {
            assert!(outcome.taps().is_empty(), "{kind:?}");
            assert!(outcome.logs.iter().any(|l| l.contains("checkers")), "{kind:?}: {:?}", outcome.logs);
        },
    );
    // Both on screen: the first in the list is the one reported.
    on_all(
        &project,
        start,
        || two_pictures(true),
        |kind, outcome| {
            assert!(outcome.logs.iter().any(|l| l.contains("stripes")), "{kind:?}: {:?}", outcome.logs);
        },
    );

    // All: only found once both are there.
    let (project, start) =
        images_project(|first, second| json!({ "images": [{ "image": first }, { "image": second }], "mode": "all" }));
    on_all(
        &project,
        start,
        || two_pictures(false),
        |kind, outcome| {
            assert_eq!(outcome.taps(), [Key::KeyN], "{kind:?}");
        },
    );
    on_all(
        &project,
        start,
        || two_pictures(true),
        |kind, outcome| {
            assert!(outcome.taps().is_empty(), "{kind:?}");
        },
    );
}

#[test]
fn groups_and_images_can_have_search_areas_of_their_own() {
    // The block searches the top-left only; the stripes' group searches the bottom-right.
    let (project, start) = images_project(|_, second| {
        json!({
            "images": [{ "image": second, "group": "corner" }],
            "groups": [{ "name": "corner", "region": Rect::new(160, 100, 160, 100) }],
            "region": Rect::new(0, 0, 100, 100),
        })
    });
    on_all(
        &project,
        start,
        || two_pictures(true),
        |kind, outcome| {
            assert!(outcome.logs.iter().any(|l| l.contains("stripes")), "{kind:?}: {:?}", outcome.logs);
        },
    );
    // The same image with its own area, where it is not.
    let (project, start) = images_project(|_, second| {
        json!({
            "images": [{ "image": second, "group": "corner", "region": Rect::new(0, 0, 100, 100) }],
            "groups": [{ "name": "corner", "region": Rect::new(160, 100, 160, 100) }],
        })
    });
    on_all(
        &project,
        start,
        || two_pictures(true),
        |kind, outcome| {
            assert_eq!(outcome.taps(), [Key::KeyN], "{kind:?}: its own area wins over its group's");
        },
    );
}

#[test]
fn less_detail_still_finds_a_clear_image_and_reports_where() {
    let mut b = Builder::new();
    let image = add_target_asset(&mut b.project);
    let start = b.node(ids::START, json!({}));
    let find = b.node(ids::FIND_IMAGE, json!({ "image": image, "detail": "half", "threshold": 0.7 }));
    let click = b.click();
    b.chain(&[start, click]);
    b.wire(find, "position", click, "position");
    on_all(
        &b.project,
        start,
        || screen(Some(Point::new(100, 60))),
        |kind, outcome| {
            let moved = outcome.sent.iter().find_map(|e| match e {
                InputEvent::MoveTo { x, y } => Some((*x, *y)),
                _ => None,
            });
            let (x, y) = moved.expect("it clicked where the image is");
            assert!((x - 112).abs() <= 2 && (y - 68).abs() <= 2, "{kind:?}: ({x}, {y})");
        },
    );
}

#[test]
fn sound_recognition_listens_for_several_sounds_and_says_which() {
    let tune = |notes: [f32; 3]| -> Vec<f32> { notes.iter().flat_map(|&hz| tone(hz, 0.25, 0.5)).collect() };
    let first = tune([440.0, 660.0, 550.0]);
    let second = tune([300.0, 900.0, 1200.0]);
    let mut b = Builder::new();
    let a = b
        .project
        .assets
        .add_audio("doorbell", magi_audio::prepare_upload(&magi_audio::encode_wav(&first, RATE)).unwrap());
    let c = b
        .project
        .assets
        .add_audio("alarm", magi_audio::prepare_upload(&magi_audio::encode_wav(&second, RATE)).unwrap());
    let start = b.node(ids::START, json!({}));
    let listen = b.node(ids::SOUND_MATCH, json!({ "sounds": [{ "sound": a }, { "sound": c, "group": "loud" }] }));
    let decision = b.node(ids::DECISION, json!({}));
    let heard = b.node(ids::LOG, json!({ "message": "heard" }));
    let not_heard = b.tap(Key::KeyN);
    b.flow(start, ports::OUT, decision).wire(listen, "found", decision, "cond:1").flow(decision, "case:1", heard).flow(
        decision,
        ports::ELSE,
        not_heard,
    );
    b.wire(listen, "name", heard, "value");
    let playing = move || {
        let mut samples = vec![0.0; RATE as usize / 2];
        samples.extend(second.iter().map(|s| s * 0.4));
        samples.extend(vec![0.0; RATE as usize / 10]);
        hearing(samples)
    };
    on_all(&b.project, start, playing, |kind, outcome| {
        assert!(outcome.logs.iter().any(|l| l.contains("alarm")), "{kind:?}: {:?}", outcome.logs);
    });
}

// ---- AI models -----------------------------------------------------------------

/// A model that averages each color of the middle of the screen: its
/// "classes" are red, green and blue. Acting on red presses R.
fn color_model(project: &mut Project) -> String {
    use magi_core::magic::*;
    let model = MagicModel {
        name: "Colors".into(),
        input: InputSpec { width: 4, height: 4, mean: [0.0; 3], std: [1.0; 3], ..InputSpec::default() },
        capture: CaptureSpec { area: CaptureArea::Center { height_share: 0.5 }, display: 0 },
        classes: vec![
            ClassSpec { label: "red".into(), act: true, threshold: 0.4, delay_ms: 0.0 },
            ClassSpec { label: "green".into(), ..ClassSpec::default() },
            ClassSpec { label: "blue".into(), ..ClassSpec::default() },
        ],
        action: ActionSpec { key: magi_core::input::Button::Key(Key::KeyR), hold_ms: 1.0, cooldown_ms: 200.0 },
        settings: vec![ModelSetting {
            key: "thresh".into(),
            label: "Confidence".into(),
            kind: SettingKind::Percent,
            min: 0.0,
            max: 100.0,
            default: 40.0,
            value: 40.0,
            targets: vec![SettingTarget::Threshold { class: 0 }],
            ..ModelSetting::default()
        }],
        ..MagicModel::default()
    };
    let file = MagicFile { model, onnx: include_bytes!("data/channel_means.onnx").to_vec(), ..MagicFile::default() };
    project.assets.add_model("Colors", file.for_macro().unwrap())
}

fn colored(rgb: [u8; 3]) -> impl Fn() -> SimHost {
    move || SimHost::with_screen(Frame::solid(Point::new(0, 0), 320, 200, rgb))
}

#[test]
fn an_ai_model_answers_about_the_middle_of_the_screen() {
    let mut b = Builder::new();
    let model = color_model(&mut b.project);
    let start = b.node(ids::START, json!({}));
    let ask = b.node(ids::AI_CLASSIFY, json!({ "model": model }));
    let decision = b.node(ids::DECISION, json!({}));
    let yes = b.node(ids::LOG, json!({ "message": "saw" }));
    let no = b.tap(Key::KeyN);
    b.flow(start, ports::OUT, decision).wire(ask, "act", decision, "cond:1").flow(decision, "case:1", yes).flow(
        decision,
        ports::ELSE,
        no,
    );
    b.wire(ask, "label", yes, "value");
    on_all(&b.project, start, colored([250, 10, 10]), |kind, outcome| {
        assert!(outcome.logs.iter().any(|l| l.contains("saw red")), "{kind:?}: {:?}", outcome.logs);
    });
    on_all(&b.project, start, colored([10, 250, 10]), |kind, outcome| {
        assert_eq!(outcome.taps(), [Key::KeyN], "{kind:?}: green is not acted on");
    });

    // The block's own settings: demanding 90% certainty, red is no longer enough.
    let mut strict = b.project.clone();
    strict.graph.node_mut(ask).unwrap().set_param("settings", json!({ "thresh": 90 }));
    on_all(&strict, start, colored([250, 10, 10]), |kind, outcome| {
        assert_eq!(outcome.taps(), [Key::KeyN], "{kind:?}");
    });
}

#[test]
fn ai_watch_presses_the_model_key_once_per_event() {
    let mut b = Builder::new();
    let model = color_model(&mut b.project);
    let start = b.node(ids::START, json!({}));
    let watch = b.node(ids::AI_WATCH, json!({ "model": model, "duration_ms": 60.0 }));
    let report = b.log("hits");
    b.chain(&[start, watch, report]);
    b.wire(watch, "hits", report, "value");
    on_all(&b.project, start, colored([250, 10, 10]), |kind, outcome| {
        assert_eq!(outcome.taps(), [Key::KeyR], "{kind:?}: once, then the cooldown");
        assert!(outcome.logs.iter().any(|l| l == "hits 1"), "{kind:?}: {:?}", outcome.logs);
    });
    let mut quiet = b.project.clone();
    quiet.graph.node_mut(watch).unwrap().set_param("press", false);
    on_all(&quiet, start, colored([250, 10, 10]), |kind, outcome| {
        assert!(outcome.taps().is_empty(), "{kind:?}: watching only");
    });
}

#[test]
fn ai_watch_runs_the_blocks_under_acted_each_time_it_acts() {
    let mut b = Builder::new();
    let model = color_model(&mut b.project);
    let start = b.node(ids::START, json!({}));
    let watch = b.node(ids::AI_WATCH, json!({ "model": model, "press": false }));
    let seen = b.log("saw");
    let count = b.log("times");
    let leave = b.node(ids::STOP, json!({ "scope": "exit_loop" }));
    let done = b.log("done");
    b.flow(start, ports::OUT, watch).flow(watch, "acted", seen).flow(seen, ports::OUT, count);
    b.flow(count, ports::OUT, leave).flow(watch, ports::OUT, done);
    b.wire(watch, "label", seen, "value").wire(watch, "hits", count, "value");
    on_all(&b.project, start, colored([250, 10, 10]), |kind, outcome| {
        assert!(outcome.errors.is_empty(), "{kind:?}: {:?}", outcome.errors);
        assert!(outcome.taps().is_empty(), "{kind:?}: watching only");
        let ours: Vec<&String> =
            outcome.logs.iter().filter(|l| ["saw", "times", "done"].iter().any(|w| l.starts_with(w))).collect();
        assert_eq!(ours, ["saw red", "times 1", "done"], "{kind:?}: the branch ran with the answer, then left");
    });
}

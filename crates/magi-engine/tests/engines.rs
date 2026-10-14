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
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use magi_core::action::{Action, PressMode};
use magi_core::autoclicker::{AutoClickerConfig, ClickLimit};
use magi_core::geom::{Point, Rect};
use magi_core::graph::{NodeId, PortRef};
use magi_core::host::{Frame, SimHost};
use magi_core::input::{InputEvent, Key, MouseButton};
use magi_core::nodes::{Registry, ids, ports};
use magi_core::project::Project;
use magi_core::recording::{RecordOptions, RecordingBuilder};
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

/// `Start → Decision`, with one case fed by Image Detection.
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
    let mut recorder = RecordingBuilder::new(RecordOptions::default(), vec![], Point::new(5, 5));
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
    let config = AutoClickerConfig {
        interval_ms: 2.0,
        hold_ms: 0.5,
        limit: ClickLimit::Count { count: 25 },
        ..AutoClickerConfig::default()
    };
    let mut project = Project::new();
    project.graph = config.to_graph(&registry());
    let start = project.graph.starts().next().unwrap().id;

    on_all(&project, start, SimHost::new, |kind, outcome| {
        assert_eq!(outcome.clicks(), 25, "{kind:?}");
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
fn python_scripts_need_the_python_engine() {
    let mut b = Builder::new();
    let start = b.node(ids::START, json!({}));
    let script = b.node(ids::PYTHON, json!({ "code": "result = 1" }));
    b.chain(&[start, script]);
    let program = Program::from_project(&b.project, registry());
    assert!(program.needs_python());

    let monitor = Arc::new(Monitor::new());
    let session = session(EngineKind::Native, &b.project, Arc::new(SimHost::new()), Arc::clone(&monitor));
    session.start(start).unwrap();
    assert!(session.wait_idle(Some(TIMEOUT)));
    let logs = monitor.logs();
    assert!(logs.iter().any(|e| e.level == LogLevel::Error && e.message.contains("Python engine")));
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

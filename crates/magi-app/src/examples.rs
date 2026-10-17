//! Built-in example macros, offered under File › Examples.
//!
//! They are built in code rather than shipped as files, so they can never go
//! stale against the node definitions and the app stays a single file.

use magi_core::action::Action;
use magi_core::autoclicker::AutoClickerConfig;
use magi_core::geom::Point;
use magi_core::graph::{NodeId, PortRef};
use magi_core::hotkey::Hotkey;
use magi_core::input::{Button, InputEvent, Key, MouseButton};
use magi_core::nodes::{Registry, ids, ports};
use magi_core::project::Project;
use magi_core::recording::{RecordOptions, RecordingBuilder};
use magi_core::variables::{Variable, binding};
use magi_vision::ImageRef;
use serde_json::{Value, json};

pub const NAMES: [&str; 4] =
    ["React to what is on screen", "Auto clicker, as blocks", "Replay a recording", "Variables, functions and groups"];

/// The examples' Start hotkey. Plain F7 is the Run hotkey.
fn shift_f7() -> Hotkey {
    Hotkey::from_buttons([Button::Key(Key::ShiftLeft), Button::Key(Key::F7)])
}

pub fn build(index: usize, registry: &Registry) -> Project {
    match index {
        0 => react_to_screen(registry),
        1 => auto_clicker(registry),
        2 => replay_recording(registry),
        _ => functions(registry),
    }
}

struct Builder<'a> {
    project: Project,
    registry: &'a Registry,
}

impl Builder<'_> {
    fn node(&mut self, kind: &str, pos: [f32; 2], label: &str, params: Value) -> NodeId {
        let id = self.project.graph.add(self.registry, kind, pos).expect("example uses built-in nodes");
        let node = self.project.graph.node_mut(id).expect("just added");
        node.label = label.to_owned();
        if let Value::Object(map) = params {
            node.params.extend(map);
        }
        id
    }

    fn flow(&mut self, from: NodeId, port: &str, to: NodeId) {
        self.wire(from, port, to, ports::FLOW_IN);
    }

    fn wire(&mut self, from: NodeId, from_port: &str, to: NodeId, to_port: &str) {
        self.project
            .graph
            .connect(self.registry, PortRef::new(from, from_port), PortRef::new(to, to_port))
            .expect("example wiring is valid");
    }
}

/// A small rounded "button" picture, so the example has something to look for.
fn button_image(rgb: [u8; 3]) -> Vec<u8> {
    let (w, h) = (72u32, 30u32);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
            let radius = h as f32 / 2.0;
            // Distance to the nearest point of the pill's centre line.
            let nearest_x = cx.clamp(radius, w as f32 - radius);
            let inside = ((cx - nearest_x).powi(2) + (cy - radius).powi(2)).sqrt() <= radius - 1.5;
            let stripe = inside && (10..62).contains(&x) && (13..17).contains(&y);
            let pixel = if stripe {
                [245, 245, 250]
            } else if inside {
                rgb
            } else {
                [32, 34, 40]
            };
            rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
        }
    }
    magi_vision::encode_png(ImageRef::new(w, h, &rgba)).expect("encoding a small image cannot fail")
}

/// Watches for two buttons and reacts differently to each.
fn react_to_screen(registry: &Registry) -> Project {
    let mut b = Builder { project: Project::new(), registry };
    b.project.meta.name = "React to what is on screen".into();
    let accept = b.project.assets.add_image("Accept button", button_image([46, 190, 120]));
    let decline = b.project.assets.add_image("Decline button", button_image([225, 70, 80]));

    let start = b.node(ids::START, [372.0, 0.0], "", json!({ "hotkey": shift_f7() }));
    let repeat = b.node(ids::REPEAT, [372.0, 110.0], "Keep watching", json!({ "mode": "forever" }));
    let decision = b.node(
        ids::DECISION,
        [349.0, 262.0],
        "What do I see?",
        json!({ "cases": [{ "id": "1", "label": "Accept" }, { "id": "2", "label": "Decline" }] }),
    );
    let see_accept = b.node(ids::FIND_IMAGE, [-20.0, 60.0], "See Accept", json!({ "images": [{ "image": accept }] }));
    let see_decline = b.node(
        ids::FIND_IMAGE,
        [-20.0, 390.0],
        "See Decline",
        json!({ "images": [{ "image": decline }], "threshold": 0.85 }),
    );
    let click = b.node(
        ids::INPUT,
        [286.0, 500.0],
        "Click it",
        json!({ "actions": [Action::click(MouseButton::Left)], "hold_ms": 40.0 }),
    );
    let escape = b.node(ids::INPUT, [526.0, 500.0], "Dismiss", json!({ "actions": [Action::tap(Key::Escape)] }));
    let idle = b.node(ids::DELAY, [766.0, 500.0], "Look again soon", json!({ "ms": 250.0, "jitter_ms": 50.0 }));
    let settle = b.node(ids::DELAY, [286.0, 668.0], "Let it react", json!({ "ms": 600.0 }));

    b.flow(start, ports::OUT, repeat);
    b.flow(repeat, ports::LOOP, decision);
    b.wire(see_accept, "found", decision, "cond:1");
    b.wire(see_decline, "found", decision, "cond:2");
    b.flow(decision, "case:1", click);
    b.flow(decision, "case:2", escape);
    b.flow(decision, ports::ELSE, idle);
    b.wire(see_accept, "position", click, "position");
    b.flow(click, ports::OUT, settle);
    b.project
}

/// The Auto Clicker's default settings, as the graph it really runs.
fn auto_clicker(registry: &Registry) -> Project {
    let mut project = Project::new();
    project.meta.name = "Auto clicker, as blocks".into();
    project.graph = AutoClickerConfig::default().to_graph(registry);
    project
}

/// A short recording wired to a hotkey, with its information card worth opening.
fn replay_recording(registry: &Registry) -> Project {
    let mut b = Builder { project: Project::new(), registry };
    b.project.meta.name = "Replay a recording".into();

    let mut recorder =
        RecordingBuilder::new(RecordOptions::default(), Vec::new(), Point::new(640, 360), Default::default());
    let mut t = 0.0;
    for (key, hold) in [(Key::KeyH, 70.0), (Key::KeyE, 55.0), (Key::KeyL, 60.0), (Key::KeyL, 62.0), (Key::KeyO, 80.0)] {
        recorder.push(t, &InputEvent::KeyDown { key });
        recorder.push(t + hold, &InputEvent::KeyUp { key });
        t += hold + 65.0;
    }
    for step in 0..=30 {
        let f = f64::from(step) / 30.0;
        let (x, y) = (640.0 + 260.0 * f, 360.0 + 90.0 * (f * std::f64::consts::PI).sin());
        recorder.push(t + f * 420.0, &InputEvent::MoveTo { x: x as i32, y: y as i32 });
    }
    t += 470.0;
    recorder.push(t, &InputEvent::ButtonDown { button: MouseButton::Left });
    recorder.push(t + 85.0, &InputEvent::ButtonUp { button: MouseButton::Left });
    recorder.push(t + 300.0, &InputEvent::Scroll { dx: 0, dy: -2 });
    let recording = b.project.assets.add_recording("Type and click", recorder.finish(t + 500.0, Some((1920, 1080))));

    let start = b.node(ids::START, [0.0, 0.0], "", json!({ "hotkey": shift_f7(), "mode": "press" }));
    let play = b.node(ids::PLAY_RECORDING, [0.0, 120.0], "", json!({ "recording": recording }));
    let done = b.node(ids::LOG, [0.0, 290.0], "", json!({ "message": "Replay finished" }));
    b.flow(start, ports::OUT, play);
    b.flow(play, ports::OUT, done);
    b.project
}

/// A variable that sets the pace, a function that uses and changes it, a
/// group around the function, a comment explaining it all and a notification
/// at the end.
fn functions(registry: &Registry) -> Project {
    let mut b = Builder { project: Project::new(), registry };
    b.project.meta.name = "Variables, functions and groups".into();
    b.project.variables.push(Variable::new("pause", json!(400)));

    b.node(
        ids::COMMENT,
        [-330.0, -10.0],
        "How this works",
        json!({
            "text": "Presses Space five times, a little faster each time, then lets you know it is done.\n\n\
                     The pause between presses is the variable “pause”: the Wait block follows it, and the \
                     function makes it 60 ms shorter on every round. Watch it change in the Variables panel.",
            "width": 280.0,
            "height": 170.0,
        }),
    );
    let start = b.node(ids::START, [40.0, 0.0], "", json!({ "hotkey": shift_f7() }));
    let repeat = b.node(ids::REPEAT, [40.0, 120.0], "Five rounds", json!({ "mode": "count", "count": 5 }));
    let call = b.node(ids::CALL, [40.0, 270.0], "", json!({ "definition": "Press and speed up" }));
    let notify = b.node(
        ids::NOTIFY,
        [300.0, 270.0],
        "",
        json!({ "title": "MacroMagi", "message": "Five presses done", "popup": true, "sound": true }),
    );

    let define = b.node(ids::DEFINE, [620.0, 40.0], "", json!({ "name": "Press and speed up" }));
    let press = b.node(ids::INPUT, [620.0, 150.0], "Press Space", json!({ "actions": [Action::tap(Key::Space)] }));
    let wait = b.node(ids::DELAY, [620.0, 290.0], "Pause", json!({ "ms": binding("pause") }));
    let faster = b.node(
        ids::SET_VARIABLE,
        [620.0, 420.0],
        "A bit faster",
        json!({ "variable": "pause", "operation": "subtract", "value": "60" }),
    );

    b.flow(start, ports::OUT, repeat);
    b.flow(repeat, ports::LOOP, call);
    b.flow(repeat, ports::DONE, notify);
    b.flow(define, ports::OUT, press);
    b.flow(press, ports::OUT, wait);
    b.flow(wait, ports::OUT, faster);
    let members = [define, press, wait, faster].into_iter().collect();
    b.project.graph.add_group("Function: press and speed up", &members);
    b.project
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_example_builds_and_is_consistent() {
        let registry = Registry::builtin();
        for (index, name) in NAMES.iter().enumerate() {
            let mut project = build(index, &registry);
            assert!(project.graph.starts().next().is_some(), "{name}");
            assert_eq!(project.graph.prune(&registry), 0, "{name} has wires to missing ports");
            for asset in project.graph.referenced_assets(&registry) {
                assert!(project.assets.contains(&asset), "{name} refers to a missing asset");
            }
        }
    }

    #[test]
    fn the_function_example_presses_faster_and_faster_then_notifies() {
        use std::sync::Arc;

        use magi_core::host::SimHost;
        use magi_engine::{Monitor, Program, Session};

        let registry = Arc::new(Registry::builtin());
        let mut project = build(3, &registry);
        project.variables[0].value = json!(250);
        let start = project.graph.starts().next().expect("a Start block").id;
        let host = Arc::new(SimHost::new());
        let monitor = Arc::new(Monitor::new());
        let program = Arc::new(Program::from_project(&project, registry));
        let session =
            Session::native(program, Arc::clone(&host) as Arc<dyn magi_core::host::Host>, Arc::clone(&monitor));
        session.start(start).unwrap();
        assert!(session.wait_idle(Some(std::time::Duration::from_secs(20))));

        let presses = host.sent().iter().filter(|e| matches!(e, InputEvent::KeyDown { key: Key::Space })).count();
        assert_eq!(presses, 5);
        let pause = monitor.variables().get("pause").and_then(Value::as_f64);
        assert_eq!(pause, Some(250.0 - 5.0 * 60.0), "the function shortened the pause every round");
        assert_eq!(host.notifications().len(), 1);
    }
}

//! Input nodes: sending keyboard, mouse and (in future) gamepad input.

use serde::{Deserialize, Serialize};

use super::{Category, DataType, NodeDef, ParamDef, ParamKind, Registry, ids, ports};
use crate::action::Action;
use crate::autoclicker::{AutoClickParams, ClickerSettings};
use crate::input::MouseButton;
use crate::recording::MouseReplay;
use crate::text::{format_ms, trim_float};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ActionParams {
    /// Performed together: all pressed in order, held, released in reverse.
    pub actions: Vec<Action>,
    /// How long taps stay pressed.
    pub hold_ms: f64,
}

impl Default for ActionParams {
    fn default() -> Self {
        Self { actions: vec![Action::click(MouseButton::Left)], hold_ms: 30.0 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayRecordingParams {
    /// Id of the recording asset.
    pub recording: String,
    /// Playback speed multiplier. `2.0` plays twice as fast.
    pub speed: f64,
    /// Replay recorded pointer movement, not only clicks and keys.
    pub moves: bool,
    /// How pointer movement is replayed.
    pub mouse: MouseReplay,
}

impl Default for PlayRecordingParams {
    fn default() -> Self {
        Self { recording: String::new(), speed: 1.0, moves: true, mouse: MouseReplay::Recorded }
    }
}

/// How many actions a node lists before summarising the rest.
const MAX_SUMMARY_ACTIONS: usize = 5;

pub(super) fn register(registry: &mut Registry) {
    registry.register(
        NodeDef::new(
            ids::INPUT,
            "Input",
            Category::Input,
            "Sends input: a key, a mouse click, a move, a scroll or typed text. Add several \
             inputs to one block to perform them at the same time, like Ctrl + C.",
        )
        .keywords(&["click", "key", "press", "mouse", "keyboard", "type", "scroll", "move", "raw"])
        .flow_in()
        .data_in("position", "Position", DataType::Point)
        .flow_out(ports::OUT, "")
        .param(ParamDef::new("actions", "Inputs", ParamKind::Actions))
        .param(
            ParamDef::new("hold_ms", "Hold time", ParamKind::Duration)
                .help("How long taps and clicks stay pressed before they are released."),
        )
        .defaults(&ActionParams::default())
        .summarize(|node, _| {
            let p: ActionParams = node.params_or_default();
            if p.actions.is_empty() {
                return vec!["Nothing to do yet".into()];
            }
            let mut lines: Vec<String> = p.actions.iter().take(MAX_SUMMARY_ACTIONS).map(Action::summary).collect();
            if p.actions.len() > MAX_SUMMARY_ACTIONS {
                lines.push(format!("+ {} more", p.actions.len() - MAX_SUMMARY_ACTIONS));
            }
            lines
        }),
    );

    registry.register(
        NodeDef::new(
            ids::PLAY_RECORDING,
            "Play Recording",
            Category::Input,
            "Replays recorded input with its original timing: every key and button is pressed \
             and released exactly when it was during recording.",
        )
        .keywords(&["record", "replay", "playback", "macro"])
        .flow_in()
        .flow_out(ports::OUT, "")
        .param(ParamDef::new("recording", "Recording", ParamKind::Recording))
        .param(ParamDef::new("speed", "Speed", ParamKind::float(0.1, 20.0, "×")))
        .param(ParamDef::new("moves", "Replay mouse movement", ParamKind::Bool))
        .param(
            ParamDef::new(
                "mouse",
                "Mouse movement as",
                ParamKind::choice(&[
                    ("recorded", "As recorded (smart)"),
                    ("absolute", "Screen positions"),
                    ("relative", "Relative motion – for games"),
                ]),
            )
            .help(
                "Games with a first-person camera lock the pointer and read how far the mouse moves. \
                 If the camera turns once and then stops, choose relative motion.",
            )
            .show_if("moves", true),
        )
        .defaults(&PlayRecordingParams::default())
        .summarize(|node, assets| {
            let p: PlayRecordingParams = node.params_or_default();
            let Some(asset) = assets.get(&p.recording) else {
                return vec!["No recording selected".into()];
            };
            let Some(recording) = assets.recording(&p.recording) else {
                return vec!["Not a recording".into()];
            };
            let mut timing = format_ms(recording.duration_ms);
            if (p.speed - 1.0).abs() > f64::EPSILON {
                timing.push_str(&format!(" at {}×", trim_float(p.speed, 2)));
            }
            vec![asset.name.clone(), recording.summary(), timing]
        }),
    );

    registry.register(
        NodeDef::new(
            ids::AUTOCLICK,
            "Auto Click",
            Category::Input,
            "Clicks (or presses keys) over and over at a steady speed, with everything the Auto \
             Clicker tab offers: speed variation, duty cycle, limits, corner and edge stops.",
        )
        .keywords(&["auto clicker", "autoclick", "spam", "rapid", "repeat click", "cps"])
        .flow_in()
        .flow_out(ports::OUT, "")
        .param(ParamDef::new("clicker", "Clicking", ParamKind::Clicker))
        .defaults(&AutoClickParams::default())
        .summarize(|node, _| {
            let settings: ClickerSettings = node.params_or_default::<AutoClickParams>().clicker;
            let mut lines = vec![settings.summary()];
            match settings.limit {
                crate::autoclicker::ClickLimit::Count { count } => lines.push(format!("{count} times")),
                crate::autoclicker::ClickLimit::Forever => lines.push("Until stopped".into()),
            }
            if let Some(limit) = settings.time_limit.get() {
                lines.push(format!("At most {}", format_ms(*limit)));
            }
            lines
        }),
    );
}

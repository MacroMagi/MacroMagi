//! Input nodes: sending keyboard, mouse and (in future) gamepad input.

use serde::{Deserialize, Serialize};

use super::{Category, DataType, NodeDef, ParamDef, ParamKind, Registry, ids, ports};
use crate::action::Action;
use crate::input::MouseButton;
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
}

impl Default for PlayRecordingParams {
    fn default() -> Self {
        Self { recording: String::new(), speed: 1.0, moves: true }
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
}

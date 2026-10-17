//! Input nodes: sending keyboard, mouse and (in future) gamepad input.

use serde::{Deserialize, Serialize};

use super::{Category, DataType, NodeDef, ParamDef, ParamKind, Registry, ids, ports};
use crate::action::Action;
use crate::autoclicker::{AutoClickParams, ClickerSettings};
use crate::input::MouseButton;
use crate::recording::MouseReplay;
use crate::text::{format_ms, trim_float};

/// What an Auto Typer does with a line break.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NewLines {
    #[default]
    Enter,
    ShiftEnter,
    Space,
    Skip,
}

/// Settings of an **Auto Typer** block.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoTypeParams {
    pub text: String,
    /// Characters per second.
    pub speed: f64,
    /// How much each pause may differ, `0.0..=1.0`.
    pub variation: f64,
    /// A longer pause after punctuation.
    pub punctuation_pause: bool,
    /// The chance of a typo (typed, then corrected) per letter, `0.0..=1.0`.
    pub typos: f64,
    /// Leave out spaces and line breaks at both ends.
    pub trim: bool,
    /// Characters to leave out at the start.
    pub skip: u64,
    /// Type at most this many characters; 0 for all.
    pub limit: u64,
    pub new_lines: NewLines,
    pub enter_at_end: bool,
}

impl Default for AutoTypeParams {
    fn default() -> Self {
        Self {
            text: String::new(),
            speed: 12.0,
            variation: 0.3,
            punctuation_pause: true,
            typos: 0.0,
            trim: false,
            skip: 0,
            limit: 0,
            new_lines: NewLines::Enter,
            enter_at_end: false,
        }
    }
}

impl AutoTypeParams {
    /// The characters to type, after trimming, skipping and the limit.
    pub fn characters(&self, text: &str) -> Vec<char> {
        let text = if self.trim { text.trim() } else { text };
        let chars = text.chars().filter(|c| *c != '\r').skip(self.skip as usize);
        if self.limit > 0 { chars.take(self.limit as usize).collect() } else { chars.collect() }
    }
}

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
            ids::AUTOTYPE,
            "Auto Typer",
            Category::Input,
            "Types text, character by character, at a speed that can vary like a person's. The text is \
             its own setting, a variable it is linked to, or whatever is wired into “Text”.",
        )
        .keywords(&["type", "typing", "text", "keyboard", "write", "paste", "autotype", "typer"])
        .flow_in()
        .flow_out(ports::OUT, "")
        .data_in("text", "Text", DataType::Text)
        .data_out("typed", "Characters typed", DataType::Number)
        .param(
            ParamDef::new("text", "Text", ParamKind::Text { multiline: true })
                .help("What to type. A value wired into “Text” is typed instead."),
        )
        .param(
            ParamDef::new("speed", "Characters per second", ParamKind::float(0.5, 2000.0, "per second"))
                .help("A person types about 5 per second (60 words a minute)."),
        )
        .param(
            ParamDef::new("variation", "Speed variation", ParamKind::Percent)
                .help("How much each pause between characters may differ, so the rhythm is not mechanical."),
        )
        .param(
            ParamDef::new("punctuation_pause", "Pause after punctuation", ParamKind::Bool)
                .help("A longer pause after . , ! ? ; and :, as people make."),
        )
        .param(
            ParamDef::new("typos", "Typos", ParamKind::Percent)
                .help("The chance of mistyping a letter and correcting it with Backspace. 0% types it right."),
        )
        .param(ParamDef::new("trim", "Trim spaces at the ends", ParamKind::Bool))
        .param(ParamDef::new("skip", "Skip the first", ParamKind::int(0, 10_000_000, "characters")))
        .param(
            ParamDef::new("limit", "Type at most", ParamKind::int(0, 10_000_000, "characters"))
                .help("0 types everything."),
        )
        .param(ParamDef::new(
            "new_lines",
            "Line breaks",
            ParamKind::choice(&[
                ("enter", "Press Enter"),
                ("shift_enter", "Press Shift+Enter – a new line in a chat box"),
                ("space", "Type a space"),
                ("skip", "Leave them out"),
            ]),
        ))
        .param(ParamDef::new("enter_at_end", "Press Enter at the end", ParamKind::Bool))
        .defaults(&AutoTypeParams::default())
        .summarize(|node, _| {
            let p: AutoTypeParams = node.params_or_default();
            let text = p.text.trim();
            let headline = if text.is_empty() {
                "Types the text wired in".to_owned()
            } else {
                let short: String = text.chars().take(28).collect();
                let more = if text.chars().count() > 28 { "…" } else { "" };
                format!("“{}{more}”", short.replace('\n', " "))
            };
            let mut speed = format!("{} per second", trim_float(p.speed, 1));
            if p.variation > 0.0 {
                speed.push_str(&format!(" ± {:.0}%", p.variation * 100.0));
            }
            let mut lines = vec![headline, speed];
            if p.limit > 0 {
                lines.push(format!("At most {} characters", p.limit));
            }
            lines
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

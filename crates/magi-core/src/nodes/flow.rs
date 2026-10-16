//! Flow nodes: where a macro starts, waits, loops, branches and stops.

use serde::{Deserialize, Serialize};

use super::{Category, DataType, DynamicPorts, NodeDef, ParamDef, ParamKind, Registry, ids, ports};
use crate::action::Action;
use crate::assets::AssetStore;
use crate::graph::{Node, NodeId};
use crate::hotkey::Hotkey;
use crate::text::format_ms;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartMode {
    /// Press the hotkey to start, press it again to stop.
    #[default]
    Toggle,
    /// Runs only while the hotkey is held down.
    Hold,
    /// Each press starts the macro; it runs until it finishes.
    Press,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StartParams {
    pub hotkey: Hotkey,
    pub mode: StartMode,
    /// Whether the editor's Run button starts this block.
    pub run_on_start: bool,
}

impl Default for StartParams {
    fn default() -> Self {
        Self { hotkey: Hotkey::none(), mode: StartMode::Toggle, run_on_start: true }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DelayParams {
    pub ms: f64,
    /// The wait is lengthened or shortened by a random amount up to this.
    pub jitter_ms: f64,
}

impl Default for DelayParams {
    fn default() -> Self {
        Self { ms: 500.0, jitter_ms: 0.0 }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepeatMode {
    #[default]
    Count,
    Forever,
    /// Until the Condition input is true (checked before each round).
    Until,
    /// While the Condition input is true (checked before each round).
    While,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RepeatParams {
    pub mode: RepeatMode,
    pub count: u64,
}

impl Default for RepeatParams {
    fn default() -> Self {
        Self { mode: RepeatMode::Count, count: 10 }
    }
}

impl RepeatParams {
    /// Number of iterations, or `None` for "until stopped" (or until a condition).
    pub fn iterations(&self) -> Option<u64> {
        match self.mode {
            RepeatMode::Count => Some(self.count),
            RepeatMode::Forever | RepeatMode::Until | RepeatMode::While => None,
        }
    }

    /// Whether the condition input decides when the loop ends.
    pub fn uses_condition(&self) -> bool {
        matches!(self.mode, RepeatMode::Until | RepeatMode::While)
    }
}

/// One branch of a Decision. The id is stable so wires survive renaming and reordering.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Case {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DecisionParams {
    pub cases: Vec<Case>,
}

impl Default for DecisionParams {
    fn default() -> Self {
        Self { cases: vec![Case { id: "1".into(), label: "Case 1".into() }] }
    }
}

impl DecisionParams {
    /// Appends a case with an id not used by any existing case.
    pub fn add_case(&mut self) -> &Case {
        let n = (1..)
            .find(|n| self.cases.iter().all(|c| c.id != n.to_string()))
            .expect("an unbounded range always yields a free id");
        self.cases.push(Case { id: n.to_string(), label: format!("Case {}", self.cases.len() + 1) });
        self.cases.last().expect("just pushed")
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WaitUntilParams {
    /// How often the condition is checked.
    pub poll_ms: f64,
    /// Give up after this long. Zero waits forever.
    pub timeout_ms: f64,
}

impl Default for WaitUntilParams {
    fn default() -> Self {
        Self { poll_ms: 100.0, timeout_ms: 0.0 }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopScope {
    /// End only the Start block this branch belongs to.
    #[default]
    Branch,
    /// End every running Start block of the macro.
    All,
    /// End this branch: inside a Repeat, go on with its next round.
    Next,
    /// Leave the innermost Repeat and continue from its Done.
    ExitLoop,
    /// Continue from another block.
    GoTo,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StopParams {
    pub scope: StopScope,
    /// The block to continue from, for `GoTo`.
    pub target: Option<NodeId>,
}

/// A Rate block: inputs done one after another, a set time apart.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RateParams {
    /// Time between one step and the next.
    pub rate_ms: f64,
    /// How long taps stay pressed.
    pub hold_ms: f64,
    pub steps: Vec<Action>,
}

impl Default for RateParams {
    fn default() -> Self {
        Self {
            rate_ms: 100.0,
            hold_ms: 30.0,
            steps: vec![Action::tap(crate::input::Key::KeyA), Action::tap(crate::input::Key::KeyD)],
        }
    }
}

/// A Definition block: the start of a reusable piece of a macro.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DefineParams {
    pub name: String,
    /// The variable that receives what is wired into a Function block's Input.
    pub input_variable: String,
}

impl Default for DefineParams {
    fn default() -> Self {
        Self { name: "My function".into(), input_variable: String::new() }
    }
}

/// A Function block: runs a definition, then carries on.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CallParams {
    /// The name of the Definition block to run.
    pub definition: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LogParams {
    pub message: String,
}

pub(super) fn register(registry: &mut Registry) {
    registry.register(
        NodeDef::new(
            ids::START,
            "Start",
            Category::Flow,
            "Where a macro begins. A macro can have several Start blocks; each one runs \
             independently and can have its own hotkey.",
        )
        .keywords(&["begin", "hotkey", "trigger", "entry"])
        .flow_out(ports::OUT, "")
        .param(
            ParamDef::new("hotkey", "Hotkey", ParamKind::Hotkey)
                .help("Any keyboard key, mouse button or gamepad button, alone or combined."),
        )
        .param(ParamDef::new(
            "mode",
            "Hotkey behaviour",
            ParamKind::choice(&[
                ("toggle", "Toggle – press to start, press again to stop"),
                ("hold", "Hold – run while the hotkey is held"),
                ("press", "Press – start on every press"),
            ]),
        ))
        .param(
            ParamDef::new("run_on_start", "Start with the Run button", ParamKind::Bool)
                .help("Turn off for blocks that should only ever be started by their hotkey."),
        )
        .defaults(&StartParams::default())
        .summarize(summarize_start),
    );

    registry.register(
        NodeDef::new(ids::DELAY, "Wait", Category::Flow, "Pauses for a set time before continuing.")
            .keywords(&["delay", "sleep", "pause", "timer"])
            .flow_in()
            .flow_out(ports::OUT, "")
            .param(ParamDef::new("ms", "Duration", ParamKind::Duration))
            .param(
                ParamDef::new("jitter_ms", "Random ±", ParamKind::Duration)
                    .help("Vary each wait by a random amount up to this, so the timing looks less mechanical."),
            )
            .defaults(&DelayParams::default())
            .summarize(|node, _| {
                let p: DelayParams = node.params_or_default();
                let mut line = format_ms(p.ms);
                if p.jitter_ms > 0.0 {
                    line.push_str(&format!(" ± {}", format_ms(p.jitter_ms)));
                }
                vec![line]
            }),
    );

    registry.register(
        NodeDef::new(
            ids::REPEAT,
            "Repeat",
            Category::Flow,
            "Runs the blocks under “Loop” over and over, then continues from “Done”. \
             When the looped branch reaches its end it automatically returns here.",
        )
        .keywords(&["loop", "for", "while", "forever", "times", "until", "repeat until"])
        .flow_in()
        .data_in("condition", "Condition", DataType::Bool)
        .flow_out(ports::LOOP, "Loop")
        .flow_out(ports::DONE, "Done")
        .param(ParamDef::new(
            "mode",
            "Repeat",
            ParamKind::choice(&[
                ("count", "A number of times"),
                ("forever", "Until stopped"),
                ("until", "Until the condition is true"),
                ("while", "While the condition is true"),
            ]),
        ))
        .param(ParamDef::new("count", "Times", ParamKind::int(0, 1_000_000_000, "×")).show_if("mode", "count"))
        .defaults(&RepeatParams::default())
        .summarize(|node, _| {
            let p: RepeatParams = node.params_or_default();
            vec![match (p.mode, p.count) {
                (RepeatMode::Count, 1) => "1 time".into(),
                (RepeatMode::Count, n) => format!("{n} times"),
                (RepeatMode::Forever, _) => "Until stopped".into(),
                (RepeatMode::Until, _) => "Until the condition is true".into(),
                (RepeatMode::While, _) => "While the condition is true".into(),
            }]
        }),
    );

    registry.register(
        NodeDef::new(
            ids::DECISION,
            "Decision",
            Category::Flow,
            "Chooses which branch to follow. Each case has a condition input on the left; the \
             first case whose condition is true wins. If none is true, “Else” is followed.",
        )
        .keywords(&["if", "else", "switch", "branch", "condition", "case"])
        .flow_in()
        .flow_out(ports::ELSE, "Else")
        .param(ParamDef::new("cases", "Cases", ParamKind::Cases))
        .defaults(&DecisionParams::default())
        .dynamic_ports(DynamicPorts::Cases { param: "cases".into() }),
    );

    registry.register(
        NodeDef::new(
            ids::WAIT_UNTIL,
            "Wait Until",
            Category::Flow,
            "Pauses until its condition becomes true, checking at a regular interval.",
        )
        .keywords(&["wait for", "until", "appear", "poll"])
        .flow_in()
        .data_in("condition", "Condition", DataType::Bool)
        .flow_out(ports::OUT, "Then")
        .flow_out(ports::TIMEOUT, "Timed out")
        .param(ParamDef::new("poll_ms", "Check every", ParamKind::Duration))
        .param(
            ParamDef::new("timeout_ms", "Give up after", ParamKind::Duration)
                .help("Zero means wait for as long as it takes."),
        )
        .defaults(&WaitUntilParams::default())
        .summarize(|node, _| {
            let p: WaitUntilParams = node.params_or_default();
            let mut lines = vec![format!("Check every {}", format_ms(p.poll_ms))];
            if p.timeout_ms > 0.0 {
                lines.push(format!("Give up after {}", format_ms(p.timeout_ms)));
            }
            lines
        }),
    );

    registry.register(
        NodeDef::new(
            ids::STOP,
            "Stop",
            Category::Flow,
            "Ends the macro here – or only this branch, or the loop it is in, or jumps to another block.",
        )
        .keywords(&["end", "exit", "halt", "quit", "break", "continue", "skip", "go to", "jump"])
        .flow_in()
        .param(ParamDef::new(
            "scope",
            "Stop",
            ParamKind::choice(&[
                ("branch", "This Start block"),
                ("all", "The whole macro"),
                ("next", "This branch – go on with the loop's next round"),
                ("exit_loop", "The loop – continue from its Done"),
                ("go_to", "Here, and continue from another block"),
            ]),
        ))
        .param(
            ParamDef::new("target", "Continue from", ParamKind::Block)
                .help("The block to run next.")
                .show_if("scope", "go_to"),
        )
        .defaults(&StopParams::default())
        .summarize(|node, _| {
            let p: StopParams = node.params_or_default();
            vec![match p.scope {
                StopScope::Branch => "This Start block".into(),
                StopScope::All => "The whole macro".into(),
                StopScope::Next => "Next round of the loop".into(),
                StopScope::ExitLoop => "Leave the loop".into(),
                StopScope::GoTo => match p.target {
                    Some(id) => format!("Go to block #{id}"),
                    None => "Go to … (not chosen)".into(),
                },
            }]
        }),
    );

    registry.register(
        NodeDef::new(
            ids::LOG,
            "Log",
            Category::Flow,
            "Writes a line to the console. Handy for checking which path a macro took.",
        )
        .keywords(&["print", "debug", "message", "console"])
        .flow_in()
        .data_in("value", "Value", DataType::Any)
        .flow_out(ports::OUT, "")
        .param(ParamDef::new("message", "Message", ParamKind::Text { multiline: false }))
        .defaults(&LogParams::default())
        .summarize(|node, _| {
            let message = node.params_or_default::<LogParams>().message;
            vec![if message.is_empty() { "Logs the value wired in".into() } else { format!("“{message}”") }]
        }),
    );
}

pub(super) fn register_structure(registry: &mut Registry) {
    registry.register(
        NodeDef::new(
            ids::RATE,
            "Rate",
            Category::Flow,
            "Does a list of inputs one after another, a set time apart – a whole chain of Input and \
             Wait blocks in one. The time can come from a variable or be wired in.",
        )
        .keywords(&["rate", "sequence", "steps", "chain", "interval", "every", "variable rate"])
        .flow_in()
        .data_in("rate", "Rate (ms)", DataType::Number)
        .flow_out(ports::OUT, "")
        .param(ParamDef::new("rate_ms", "Time between steps", ParamKind::Duration))
        .param(ParamDef::new("hold_ms", "Hold taps for", ParamKind::Duration))
        .param(ParamDef::new("steps", "Steps", ParamKind::Actions))
        .defaults(&RateParams::default())
        .summarize(|node, _| {
            let p: RateParams = node.params_or_default();
            let mut lines = vec![format!("Every {} · {} steps", format_ms(p.rate_ms), p.steps.len())];
            lines.extend(p.steps.iter().take(4).map(|a| a.summary()));
            if p.steps.len() > 4 {
                lines.push(format!("… {} more", p.steps.len() - 4));
            }
            lines
        }),
    );

    registry.register(
        NodeDef::new(
            ids::DEFINE,
            "Definition",
            Category::Flow,
            "The start of a reusable piece of a macro. Build it once under this block, then run it \
             from anywhere with a Function block. Never started by Run or a hotkey.",
        )
        .keywords(&["function", "define", "subroutine", "procedure", "reuse"])
        .flow_out(ports::OUT, "")
        .param(ParamDef::new("name", "Name", ParamKind::Text { multiline: false }))
        .param(
            ParamDef::new("input_variable", "Input goes into variable", ParamKind::Variable).help(
                "What is wired into a Function block's Input is stored in this variable before the definition runs.",
            ),
        )
        .defaults(&DefineParams::default())
        .summarize(|node, _| {
            let p: DefineParams = node.params_or_default();
            let mut lines = vec![format!("“{}”", p.name)];
            if !p.input_variable.is_empty() {
                lines.push(format!("Input → {}", p.input_variable));
            }
            lines
        }),
    );

    registry.register(
        NodeDef::new(
            ids::CALL,
            "Function",
            Category::Flow,
            "Runs the blocks under a Definition block, then carries on from here.",
        )
        .keywords(&["function", "call", "run", "definition", "subroutine", "reuse"])
        .flow_in()
        .data_in("input", "Input", DataType::Any)
        .flow_out(ports::OUT, "")
        .param(ParamDef::new("definition", "Definition", ParamKind::Definition))
        .defaults(&CallParams::default())
        .summarize(|node, _| {
            let p: CallParams = node.params_or_default();
            vec![if p.definition.is_empty() {
                "No definition chosen".into()
            } else {
                format!("Run “{}”", p.definition)
            }]
        }),
    );
}

fn summarize_start(node: &Node, _: &AssetStore) -> Vec<String> {
    let p: StartParams = node.params_or_default();
    if p.hotkey.is_none() {
        return vec![if p.run_on_start { "Run button".into() } else { "No trigger set".into() }];
    }
    let mode = match p.mode {
        StartMode::Toggle => "toggle",
        StartMode::Hold => "while held",
        StartMode::Press => "on press",
    };
    vec![format!("{} · {mode}", p.hotkey.label())]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_ports_follow_its_cases() {
        let registry = Registry::builtin();
        let mut node = Node::new(1, ids::DECISION, [0.0, 0.0], registry.get(ids::DECISION).unwrap().default_params());
        let mut params: DecisionParams = node.params_or_default();
        params.add_case();
        node.set_params(&params);

        let ports = registry.ports(&node);
        let outs: Vec<&str> = ports.flow_out.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(outs, ["case:1", "case:2", "else"]);
        let conds: Vec<&str> = ports.data_in.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(conds, ["cond:1", "cond:2"]);
    }

    #[test]
    fn new_cases_never_reuse_an_id() {
        let mut params = DecisionParams::default();
        params.add_case();
        params.cases.remove(0);
        assert_eq!(params.add_case().id, "1");
        let ids: Vec<&str> = params.cases.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["2", "1"]);
    }
}

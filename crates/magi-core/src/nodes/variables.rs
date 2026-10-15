//! Variable nodes: reading, changing and counting with the macro's variables.
//! See [`crate::variables`].

use serde::{Deserialize, Serialize};

use super::{Category, DataType, NodeDef, ParamDef, ParamKind, Registry, ids, ports};
use crate::text::trim_float;
use crate::values::VarOp;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VariableParams {
    pub variable: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SetVariableParams {
    pub variable: String,
    pub operation: VarOp,
    /// Used when nothing is wired into the Value input. Read as a number,
    /// true/false or text, whichever it looks like.
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CounterParams {
    pub variable: String,
    /// Added on every pass.
    pub step: f64,
    /// When the count reaches this, "Reached" is followed and the count
    /// starts again from zero. Zero: no limit.
    pub limit: f64,
}

impl Default for CounterParams {
    fn default() -> Self {
        Self { variable: String::new(), step: 1.0, limit: 0.0 }
    }
}

pub const REACHED: &str = "reached";

fn named(variable: &str) -> String {
    if variable.is_empty() { "No variable chosen".into() } else { variable.to_owned() }
}

pub(super) fn register(registry: &mut Registry) {
    registry.register(
        NodeDef::new(
            ids::VARIABLE,
            "Variable",
            Category::Variables,
            "The current value of a variable, for any block that reads values.",
        )
        .keywords(&["variable", "get", "read", "value", "state"])
        .data_out(ports::OUT, "Value", DataType::Any)
        .param(ParamDef::new("variable", "Variable", ParamKind::Variable))
        .defaults(&VariableParams::default())
        .summarize(|node, _| vec![named(&node.params_or_default::<VariableParams>().variable)]),
    );

    registry.register(
        NodeDef::new(
            ids::SET_VARIABLE,
            "Set Variable",
            Category::Variables,
            "Changes a variable: sets it, adds to it, multiplies it… Every block bound to the \
             variable follows the new value.",
        )
        .keywords(&["variable", "set", "assign", "store", "save", "add", "increase", "change"])
        .flow_in()
        .data_in("value", "Value", DataType::Any)
        .flow_out(ports::OUT, "")
        .param(ParamDef::new("variable", "Variable", ParamKind::Variable))
        .param(ParamDef::new(
            "operation",
            "Do",
            ParamKind::choice(&VarOp::ALL.iter().map(|(_, id, label)| (*id, *label)).collect::<Vec<_>>()),
        ))
        .param(
            ParamDef::new("value", "Value", ParamKind::Text { multiline: false })
                .help("Used when nothing is wired into the Value input. 42, true or any text."),
        )
        .defaults(&SetVariableParams::default())
        .summarize(|node, _| {
            let p: SetVariableParams = node.params_or_default();
            vec![format!("{}: {} {}", named(&p.variable), p.operation.label(), p.value).trim().to_owned()]
        }),
    );

    registry.register(
        NodeDef::new(
            ids::COUNTER,
            "Counter",
            Category::Variables,
            "Counts passes: adds the step to a variable each time it runs. When a limit is set and \
             reached, “Reached” is followed and counting starts again from zero.",
        )
        .keywords(&["count", "counter", "increment", "every", "times", "tally"])
        .flow_in()
        .flow_out(ports::OUT, "Next")
        .flow_out(REACHED, "Reached")
        .data_out("count", "Count", DataType::Number)
        .param(ParamDef::new("variable", "Variable", ParamKind::Variable))
        .param(ParamDef::new("step", "Step", ParamKind::float(-1e9, 1e9, "")))
        .param(
            ParamDef::new("limit", "Limit", ParamKind::float(0.0, 1e12, ""))
                .help("Zero: count for ever, never take “Reached”."),
        )
        .defaults(&CounterParams::default())
        .summarize(|node, _| {
            let p: CounterParams = node.params_or_default();
            let mut line = format!("{} + {}", named(&p.variable), trim_float(p.step, 3));
            if p.limit > 0.0 {
                line.push_str(&format!(" up to {}", trim_float(p.limit, 3)));
            }
            vec![line]
        }),
    );
}

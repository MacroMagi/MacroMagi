//! Logic nodes: combining and producing true/false values.

use serde::{Deserialize, Serialize};

use super::{Category, DataType, NodeDef, ParamDef, ParamKind, Registry, ids, ports};
use crate::text::trim_float;
use crate::values::{CompareOp, MathOp, TextOp};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateOp {
    #[default]
    And,
    Or,
    Xor,
    Not,
}

impl GateOp {
    pub fn apply(self, a: bool, b: bool) -> bool {
        match self {
            GateOp::And => a && b,
            GateOp::Or => a || b,
            GateOp::Xor => a != b,
            GateOp::Not => !a,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GateParams {
    pub op: GateOp,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChanceParams {
    /// Probability of producing `true`, `0.0..=1.0`.
    pub probability: f64,
}

impl Default for ChanceParams {
    fn default() -> Self {
        Self { probability: 0.5 }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MathParams {
    pub operation: MathOp,
    /// Used when nothing is wired into A / B.
    pub a: f64,
    pub b: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CompareParams {
    pub operation: CompareOp,
    /// Used when nothing is wired into B.
    pub b: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextParams {
    pub operation: TextOp,
    /// Used when nothing is wired into A / B.
    pub a: String,
    pub b: String,
    /// What B is replaced with, for Replace.
    pub replacement: String,
}

fn options<T>(all: &[(T, &'static str, &'static str)]) -> ParamKind {
    ParamKind::choice(&all.iter().map(|(_, id, label)| (*id, *label)).collect::<Vec<_>>())
}

pub(super) fn register(registry: &mut Registry) {
    registry.register(
        NodeDef::new(
            ids::MATH,
            "Math",
            Category::Logic,
            "Calculates with numbers: add, subtract, multiply, divide, round, random… A and B \
             come from their inputs, or from the numbers typed here when nothing is wired.",
        )
        .keywords(&["math", "add", "plus", "minus", "multiply", "divide", "calculate", "random", "round", "sum"])
        .data_in("a", "A", DataType::Number)
        .data_in("b", "B", DataType::Number)
        .data_out(ports::OUT, "Result", DataType::Number)
        .param(ParamDef::new("operation", "Operation", options(&MathOp::ALL)))
        .param(ParamDef::new("a", "A", ParamKind::float(-1e12, 1e12, "")))
        .param(ParamDef::new("b", "B", ParamKind::float(-1e12, 1e12, "")))
        .defaults(&MathParams::default())
        .summarize(|node, _| {
            let p: MathParams = node.params_or_default();
            vec![p.operation.label().replace('A', &trim_float(p.a, 3)).replace('B', &trim_float(p.b, 3))]
        }),
    );

    registry.register(
        NodeDef::new(
            ids::COMPARE,
            "Compare",
            Category::Logic,
            "True when A relates to B in the chosen way: equal, less, greater, contains… Numbers \
             compare as numbers, everything else as text.",
        )
        .keywords(&["compare", "equal", "less", "greater", "if", "contains", "match", "search", "same"])
        .data_in("a", "A", DataType::Any)
        .data_in("b", "B", DataType::Any)
        .data_out(ports::OUT, "Result", DataType::Bool)
        .param(ParamDef::new("operation", "Operation", options(&CompareOp::ALL)))
        .param(ParamDef::new("b", "B", ParamKind::Text { multiline: false }).help("Used when nothing is wired into B."))
        .defaults(&CompareParams::default())
        .summarize(|node, _| {
            let p: CompareParams = node.params_or_default();
            vec![p.operation.label().replace('B', &format!("“{}”", p.b))]
        }),
    );

    registry.register(
        NodeDef::new(
            ids::TEXT,
            "Text",
            Category::Logic,
            "Works with text: joins it, measures it, changes case, replaces words, or reads a number from it.",
        )
        .keywords(&["text", "string", "join", "concat", "replace", "length", "upper", "lower", "words"])
        .data_in("a", "A", DataType::Any)
        .data_in("b", "B", DataType::Any)
        .data_out(ports::OUT, "Result", DataType::Any)
        .param(ParamDef::new("operation", "Operation", options(&TextOp::ALL)))
        .param(ParamDef::new("a", "A", ParamKind::Text { multiline: false }).help("Used when nothing is wired into A."))
        .param(ParamDef::new("b", "B", ParamKind::Text { multiline: false }).help("Used when nothing is wired into B."))
        .param(
            ParamDef::new("replacement", "Replace with", ParamKind::Text { multiline: false })
                .show_if("operation", "replace"),
        )
        .defaults(&TextParams::default())
        .summarize(|node, _| vec![node.params_or_default::<TextParams>().operation.label().to_owned()]),
    );

    registry.register(
        NodeDef::new(
            ids::GATE,
            "Logic",
            Category::Logic,
            "Combines true/false values: AND, OR, XOR, or NOT (which only uses input A).",
        )
        .keywords(&["and", "or", "not", "xor", "both", "either", "invert", "boolean"])
        .data_in("a", "A", DataType::Bool)
        .data_in("b", "B", DataType::Bool)
        .data_out(ports::OUT, "Result", DataType::Bool)
        .param(ParamDef::new(
            "op",
            "Operation",
            ParamKind::choice(&[
                ("and", "A and B"),
                ("or", "A or B"),
                ("xor", "Exactly one of A, B"),
                ("not", "Not A"),
            ]),
        ))
        .defaults(&GateParams::default())
        .summarize(|node, _| {
            vec![match node.params_or_default::<GateParams>().op {
                GateOp::And => "A and B".into(),
                GateOp::Or => "A or B".into(),
                GateOp::Xor => "A xor B".into(),
                GateOp::Not => "Not A".into(),
            }]
        }),
    );

    registry.register(
        NodeDef::new(
            ids::CHANCE,
            "Random Chance",
            Category::Logic,
            "True some of the time, at random. Use it to make a macro less predictable.",
        )
        .keywords(&["random", "probability", "dice", "luck", "sometimes"])
        .data_out(ports::OUT, "Result", DataType::Bool)
        .param(ParamDef::new("probability", "Chance of true", ParamKind::Percent))
        .defaults(&ChanceParams::default())
        .summarize(|node, _| {
            vec![format!("True {:.0}% of the time", node.params_or_default::<ChanceParams>().probability * 100.0)]
        }),
    );
}

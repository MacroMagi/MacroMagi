//! Logic nodes: combining and producing true/false values.

use serde::{Deserialize, Serialize};

use super::{Category, DataType, NodeDef, ParamDef, ParamKind, Registry, ids, ports};

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

pub(super) fn register(registry: &mut Registry) {
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

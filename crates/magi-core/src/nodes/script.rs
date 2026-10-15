//! Script nodes: escape hatches into real code.

use serde::{Deserialize, Serialize};

use super::{Category, DataType, NodeDef, ParamDef, ParamKind, Registry, ids, ports};

pub const DEFAULT_PYTHON: &str = "\
# Runs when the flow reaches this block.
#   magi   – the scripting API: magi.click(), magi.press(), magi.sleep(), magi.log() …
#   value  – whatever is wired into the Value input (None if nothing is)
#   magi.vars['name'] reads and changes the macro's variables.
# Assign to `result` to send a value out of the Result output.
result = True
";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PythonParams {
    pub code: String,
    /// The variable `result` is stored in when the script ends. Empty: none.
    pub result_variable: String,
}

impl Default for PythonParams {
    fn default() -> Self {
        Self { code: DEFAULT_PYTHON.into(), result_variable: String::new() }
    }
}

pub(super) fn register(registry: &mut Registry) {
    registry.register(
        NodeDef::new(
            ids::PYTHON,
            "Python Script",
            Category::Script,
            "Runs your own Python code as a step of the macro. The script can send input, read \
             the screen and compute a result for other blocks. Needs the Python engine.",
        )
        .keywords(&["code", "python", "custom", "script", "program"])
        .flow_in()
        .data_in("value", "Value", DataType::Any)
        .flow_out(ports::OUT, "")
        .data_out("result", "Result", DataType::Any)
        .param(ParamDef::new("code", "Code", ParamKind::Code { language: "python".into() }))
        .param(
            ParamDef::new("result_variable", "Store the result in", ParamKind::Variable)
                .help("When the script ends, `result` is stored in this variable."),
        )
        .defaults(&PythonParams::default())
        .python_only()
        .summarize(|node, _| {
            let code = node.params_or_default::<PythonParams>().code;
            let lines = code.lines().filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'));
            let count = lines.clone().count();
            let first: String = lines.clone().next().unwrap_or("").trim().chars().take(28).collect();
            match count {
                0 => vec!["Empty script".into()],
                1 => vec![first],
                n => vec![first, format!("… {n} lines")],
            }
        }),
    );
}

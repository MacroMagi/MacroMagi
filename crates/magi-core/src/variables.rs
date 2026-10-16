//! Variables: named values a macro reads and changes while it runs.
//!
//! A project lists its variables with their starting values. Any block
//! setting can be **bound** to a variable instead of holding a value of its
//! own: the setting is then stored as `{"$var": "name"}`, and every block bound
//! to the same variable follows it. Change the variable once – by hand, from a
//! Set Variable block or from a Python script – and all of them change.
//!
//! Engines resolve bindings just before a block runs, so a block always sees
//! the variable's current value, converted to the kind of value the setting
//! expects (see [`coerce`]).
//!
//! A variable keeps its value from one run to the next unless it is set to
//! *reset*: then every run starts it at its starting value.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::text::trim_float;

/// The key that marks a bound setting: `{"$var": "name"}`.
pub const BINDING_KEY: &str = "$var";

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Variable {
    pub name: String,
    /// The value it starts with: at every run when `reset` is on, otherwise
    /// only the first time.
    #[serde(default)]
    pub value: Value,
    /// Back to `value` whenever a run starts. Files from before this option
    /// existed always reset, so that is what a missing field means.
    #[serde(default = "yes")]
    pub reset: bool,
}

impl Variable {
    /// A variable that every run starts at `value`.
    pub fn new(name: impl Into<String>, value: Value) -> Self {
        Self { name: name.into(), value, reset: true }
    }

    /// A variable that starts empty and keeps its value from one run to the next.
    pub fn kept(name: impl Into<String>) -> Self {
        Self { name: name.into(), value: Value::Null, reset: false }
    }
}

/// The stored form of a setting bound to `name`.
pub fn binding(name: &str) -> Value {
    serde_json::json!({ BINDING_KEY: name })
}

/// The variable a setting is bound to, if it is bound.
pub fn bound_name(value: &Value) -> Option<&str> {
    match value {
        Value::Object(map) if map.len() == 1 => map.get(BINDING_KEY)?.as_str(),
        _ => None,
    }
}

/// Whether any setting of a block is bound to a variable.
pub fn has_bindings(params: &Map<String, Value>) -> bool {
    params.values().any(|v| bound_name(v).is_some())
}

/// Reads a value as a number: numbers as they are, text that looks like one,
/// true/false as 1/0.
pub fn as_number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        Value::String(s) => s.trim().replace(',', ".").parse().ok(),
        _ => None,
    }
}

/// How a value reads as text.
pub fn as_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Number(n) => n.as_f64().map_or_else(|| n.to_string(), |f| trim_float(f, 6)),
        Value::Bool(b) => b.to_string(),
        other => other.to_string(),
    }
}

/// How values are read as true/false: `null`, `false`, `0` and `""` are false.
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// Reads typed-in text as the value it looks like: a number, true/false, or text.
pub fn parse_literal(text: &str) -> Value {
    let trimmed = text.trim();
    if let Ok(n) = trimmed.parse::<f64>() {
        return serde_json::json!(n);
    }
    match trimmed.to_ascii_lowercase().as_str() {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        _ => Value::String(text.to_owned()),
    }
}

/// Converts a variable's value to the kind of value a setting expects, taking
/// the setting's default as the example. A value that cannot be converted
/// falls back to the default.
pub fn coerce(value: &Value, like: &Value) -> Value {
    match like {
        Value::Number(n) => match as_number(value) {
            Some(f) if n.is_i64() || n.is_u64() => {
                // Whole-number settings get whole numbers.
                if n.is_u64() {
                    serde_json::json!(f.round().max(0.0) as u64)
                } else {
                    serde_json::json!(f.round() as i64)
                }
            }
            Some(f) => serde_json::json!(f),
            None => like.clone(),
        },
        Value::Bool(_) => Value::Bool(truthy(value)),
        Value::String(_) => Value::String(as_text(value)),
        // Positions, areas and lists: the value must already have the right shape.
        Value::Null | Value::Array(_) | Value::Object(_) => match value {
            Value::Null => like.clone(),
            other => other.clone(),
        },
    }
}

/// A block's settings with every binding replaced by the variable's current
/// value. `defaults` are the block's default settings; `lookup` gives a
/// variable's value. A binding to a variable that does not exist uses the default.
pub fn resolve(
    params: &Map<String, Value>,
    defaults: &Map<String, Value>,
    lookup: impl Fn(&str) -> Option<Value>,
) -> Map<String, Value> {
    params
        .iter()
        .map(|(key, value)| {
            let resolved = match bound_name(value) {
                Some(name) => {
                    let default = defaults.get(key).cloned().unwrap_or(Value::Null);
                    match lookup(name) {
                        Some(current) => coerce(&current, &default),
                        None => default,
                    }
                }
                None => value.clone(),
            };
            (key.clone(), resolved)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn bindings_are_recognised_and_resolved() {
        let params = Map::from_iter([("ms".to_owned(), binding("delay")), ("jitter_ms".to_owned(), json!(5.0))]);
        assert!(has_bindings(&params));
        assert_eq!(bound_name(&params["ms"]), Some("delay"));
        let defaults = Map::from_iter([("ms".to_owned(), json!(500.0)), ("jitter_ms".to_owned(), json!(0.0))]);
        let resolved = resolve(&params, &defaults, |name| (name == "delay").then(|| json!("250")));
        assert_eq!(resolved["ms"], json!(250.0), "text that is a number becomes the number");
        assert_eq!(resolved["jitter_ms"], json!(5.0));
        let missing = resolve(&params, &defaults, |_| None);
        assert_eq!(missing["ms"], json!(500.0), "an unknown variable falls back to the default");
    }

    #[test]
    fn values_are_converted_to_what_the_setting_expects() {
        assert_eq!(coerce(&json!(2.6), &json!(10)), json!(3), "whole-number settings round");
        assert_eq!(coerce(&json!("abc"), &json!(1.5)), json!(1.5));
        assert_eq!(coerce(&json!(0), &json!(true)), json!(false));
        assert_eq!(coerce(&json!(12.5), &json!("text")), json!("12.5"));
        assert_eq!(coerce(&json!({"x": 1, "y": 2}), &Value::Null), json!({"x": 1, "y": 2}));
    }

    #[test]
    fn typed_text_reads_as_what_it_looks_like() {
        assert_eq!(parse_literal("42"), json!(42.0));
        assert_eq!(parse_literal(" TRUE "), json!(true));
        assert_eq!(parse_literal("hello"), json!("hello"));
        assert_eq!(as_text(&json!(3.0)), "3");
        assert_eq!(as_number(&json!("1,5")), Some(1.5));
    }
}

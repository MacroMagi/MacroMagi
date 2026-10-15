//! Operations on values: the arithmetic, comparisons and text handling of the
//! Math, Compare, Text and Set Variable blocks.
//!
//! Written once here and used by both engines, so `"10" + 5` gives the same
//! answer whichever engine runs the macro.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::variables::{as_number, as_text, parse_literal};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MathOp {
    #[default]
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Power,
    Min,
    Max,
    Round,
    Floor,
    Ceil,
    Abs,
    /// A random number from A up to B.
    Random,
}

impl MathOp {
    pub const ALL: [(MathOp, &'static str, &'static str); 13] = [
        (MathOp::Add, "add", "A + B"),
        (MathOp::Subtract, "subtract", "A − B"),
        (MathOp::Multiply, "multiply", "A × B"),
        (MathOp::Divide, "divide", "A ÷ B"),
        (MathOp::Remainder, "remainder", "Remainder of A ÷ B"),
        (MathOp::Power, "power", "A to the power of B"),
        (MathOp::Min, "min", "The smaller of A, B"),
        (MathOp::Max, "max", "The larger of A, B"),
        (MathOp::Round, "round", "A rounded"),
        (MathOp::Floor, "floor", "A rounded down"),
        (MathOp::Ceil, "ceil", "A rounded up"),
        (MathOp::Abs, "abs", "A without its sign"),
        (MathOp::Random, "random", "Random number from A to B"),
    ];

    /// Whether the operation uses B.
    pub fn uses_b(self) -> bool {
        !matches!(self, MathOp::Round | MathOp::Floor | MathOp::Ceil | MathOp::Abs)
    }

    pub fn label(self) -> &'static str {
        Self::ALL.iter().find(|(op, ..)| *op == self).map_or("", |(_, _, label)| label)
    }

    /// `random` is uniform in `0..1`. `None` for an impossible result (dividing by zero).
    pub fn apply(self, a: f64, b: f64, random: f64) -> Option<f64> {
        let result = match self {
            MathOp::Add => a + b,
            MathOp::Subtract => a - b,
            MathOp::Multiply => a * b,
            MathOp::Divide if b == 0.0 => return None,
            MathOp::Divide => a / b,
            MathOp::Remainder if b == 0.0 => return None,
            MathOp::Remainder => a.rem_euclid(b),
            MathOp::Power => a.powf(b),
            MathOp::Min => a.min(b),
            MathOp::Max => a.max(b),
            MathOp::Round => a.round(),
            MathOp::Floor => a.floor(),
            MathOp::Ceil => a.ceil(),
            MathOp::Abs => a.abs(),
            MathOp::Random => a + (b - a) * random,
        };
        result.is_finite().then_some(result)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompareOp {
    #[default]
    Equal,
    NotEqual,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
    /// A contains B (ignoring upper and lower case).
    Contains,
    StartsWith,
    EndsWith,
}

impl CompareOp {
    pub const ALL: [(CompareOp, &'static str, &'static str); 9] = [
        (CompareOp::Equal, "equal", "A equals B"),
        (CompareOp::NotEqual, "not_equal", "A is not B"),
        (CompareOp::Less, "less", "A is less than B"),
        (CompareOp::LessOrEqual, "less_or_equal", "A is at most B"),
        (CompareOp::Greater, "greater", "A is greater than B"),
        (CompareOp::GreaterOrEqual, "greater_or_equal", "A is at least B"),
        (CompareOp::Contains, "contains", "A contains B"),
        (CompareOp::StartsWith, "starts_with", "A starts with B"),
        (CompareOp::EndsWith, "ends_with", "A ends with B"),
    ];

    pub fn label(self) -> &'static str {
        Self::ALL.iter().find(|(op, ..)| *op == self).map_or("", |(_, _, label)| label)
    }

    /// Numbers (and text that looks like numbers) compare as numbers;
    /// everything else compares as text.
    pub fn apply(self, a: &Value, b: &Value) -> bool {
        let numbers = as_number(a).zip(as_number(b)).filter(|_| !a.is_boolean() || b.is_boolean());
        let (ta, tb) = (as_text(a), as_text(b));
        let ordering = match numbers {
            Some((x, y)) => x.partial_cmp(&y),
            None => Some(ta.cmp(&tb)),
        };
        let lower = |s: &str| s.to_lowercase();
        match self {
            CompareOp::Equal => ordering == Some(std::cmp::Ordering::Equal),
            CompareOp::NotEqual => ordering != Some(std::cmp::Ordering::Equal),
            CompareOp::Less => ordering == Some(std::cmp::Ordering::Less),
            CompareOp::LessOrEqual => ordering.is_some_and(|o| o.is_le()),
            CompareOp::Greater => ordering == Some(std::cmp::Ordering::Greater),
            CompareOp::GreaterOrEqual => ordering.is_some_and(|o| o.is_ge()),
            CompareOp::Contains => lower(&ta).contains(&lower(&tb)),
            CompareOp::StartsWith => lower(&ta).starts_with(&lower(&tb)),
            CompareOp::EndsWith => lower(&ta).ends_with(&lower(&tb)),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextOp {
    /// A followed by B.
    #[default]
    Join,
    Length,
    Upper,
    Lower,
    Trim,
    /// A with every B replaced by the replacement.
    Replace,
    /// A read as a number.
    Number,
}

impl TextOp {
    pub const ALL: [(TextOp, &'static str, &'static str); 7] = [
        (TextOp::Join, "join", "A followed by B"),
        (TextOp::Length, "length", "Length of A"),
        (TextOp::Upper, "upper", "A in capitals"),
        (TextOp::Lower, "lower", "A in small letters"),
        (TextOp::Trim, "trim", "A without spaces around it"),
        (TextOp::Replace, "replace", "A with B replaced"),
        (TextOp::Number, "number", "A as a number"),
    ];

    pub fn label(self) -> &'static str {
        Self::ALL.iter().find(|(op, ..)| *op == self).map_or("", |(_, _, label)| label)
    }

    pub fn apply(self, a: &Value, b: &Value, replacement: &str) -> Value {
        let ta = as_text(a);
        match self {
            TextOp::Join => Value::String(ta + &as_text(b)),
            TextOp::Length => serde_json::json!(ta.chars().count()),
            TextOp::Upper => Value::String(ta.to_uppercase()),
            TextOp::Lower => Value::String(ta.to_lowercase()),
            TextOp::Trim => Value::String(ta.trim().to_owned()),
            TextOp::Replace => {
                let tb = as_text(b);
                Value::String(if tb.is_empty() { ta } else { ta.replace(&tb, replacement) })
            }
            TextOp::Number => as_number(a).map_or(Value::Null, |n| serde_json::json!(n)),
        }
    }
}

/// What a Set Variable block does to its variable.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VarOp {
    #[default]
    Set,
    Add,
    Subtract,
    Multiply,
    Divide,
    /// Adds text to the end.
    Append,
}

impl VarOp {
    pub const ALL: [(VarOp, &'static str, &'static str); 6] = [
        (VarOp::Set, "set", "Set to"),
        (VarOp::Add, "add", "Add"),
        (VarOp::Subtract, "subtract", "Subtract"),
        (VarOp::Multiply, "multiply", "Multiply by"),
        (VarOp::Divide, "divide", "Divide by"),
        (VarOp::Append, "append", "Add text to the end"),
    ];

    pub fn label(self) -> &'static str {
        Self::ALL.iter().find(|(op, ..)| *op == self).map_or("", |(_, _, label)| label)
    }

    /// The variable's new value. `operand` is what was typed or wired in;
    /// typed text is read as the value it looks like.
    pub fn apply(self, current: &Value, operand: &Value) -> Value {
        let operand = match operand {
            Value::String(text) => parse_literal(text),
            other => other.clone(),
        };
        let number = |v: &Value| as_number(v).unwrap_or(0.0);
        let math = |op: MathOp| {
            op.apply(number(current), number(&operand), 0.0).map_or_else(|| current.clone(), |n| serde_json::json!(n))
        };
        match self {
            VarOp::Set => operand,
            VarOp::Add => math(MathOp::Add),
            VarOp::Subtract => math(MathOp::Subtract),
            VarOp::Multiply => math(MathOp::Multiply),
            VarOp::Divide => math(MathOp::Divide),
            VarOp::Append => Value::String(as_text(current) + &as_text(&operand)),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn math_handles_the_edge_cases() {
        assert_eq!(MathOp::Add.apply(2.0, 3.0, 0.0), Some(5.0));
        assert_eq!(MathOp::Divide.apply(1.0, 0.0, 0.0), None);
        assert_eq!(MathOp::Remainder.apply(-1.0, 3.0, 0.0), Some(2.0));
        assert_eq!(MathOp::Random.apply(10.0, 20.0, 0.5), Some(15.0));
        assert!(!MathOp::Abs.uses_b());
    }

    #[test]
    fn comparisons_use_numbers_when_both_sides_are_numbers() {
        assert!(CompareOp::Equal.apply(&json!("10"), &json!(10.0)));
        assert!(CompareOp::Less.apply(&json!(9), &json!("10")), "9 < 10 as numbers, not as text");
        assert!(CompareOp::Less.apply(&json!("apple"), &json!("banana")));
        assert!(CompareOp::Contains.apply(&json!("Hello World"), &json!("world")));
        assert!(CompareOp::NotEqual.apply(&json!(true), &json!(1.5)));
    }

    #[test]
    fn text_and_variable_operations() {
        assert_eq!(TextOp::Join.apply(&json!("Round "), &json!(3.0), ""), json!("Round 3"));
        assert_eq!(TextOp::Length.apply(&json!("héllo"), &Value::Null, ""), json!(5));
        assert_eq!(TextOp::Replace.apply(&json!("a-b-c"), &json!("-"), "+"), json!("a+b+c"));
        assert_eq!(VarOp::Add.apply(&json!(5.0), &json!("2")), json!(7.0));
        assert_eq!(VarOp::Set.apply(&json!(5.0), &json!("hello")), json!("hello"));
        assert_eq!(VarOp::Append.apply(&json!("a"), &json!("b")), json!("ab"));
        assert_eq!(VarOp::Divide.apply(&json!(4.0), &json!(0)), json!(4.0), "dividing by zero leaves it alone");
    }
}

//! The variables of a loaded macro, shared by every run of it.
//!
//! A session holds one [`Variables`]. They start at the values the project
//! lists. Variables set to *reset* go back to those values whenever a run
//! starts while nothing else is running; the others keep what the last run
//! left in them, also into the next session (see [`Variables::carry_over`]).
//! Every change is reported to the [`Monitor`] so the editor can show the
//! current values.

use std::sync::{Arc, Mutex, MutexGuard};

use magi_core::variables::Variable;
use serde_json::{Map, Value};

use crate::monitor::Monitor;

#[derive(Default)]
struct State {
    /// Starting values of the variables that reset at every run.
    initial: Map<String, Value>,
    /// Names of the variables that keep their value between runs.
    kept: Vec<String>,
    values: Map<String, Value>,
}

#[derive(Clone)]
pub struct Variables {
    state: Arc<Mutex<State>>,
    monitor: Arc<Monitor>,
}

impl Variables {
    pub fn new(variables: &[Variable], monitor: Arc<Monitor>) -> Self {
        let values: Map<String, Value> = variables.iter().map(|v| (v.name.clone(), v.value.clone())).collect();
        let initial = variables.iter().filter(|v| v.reset).map(|v| (v.name.clone(), v.value.clone())).collect();
        let kept = variables.iter().filter(|v| !v.reset).map(|v| v.name.clone()).collect();
        let variables = Self { state: Arc::new(Mutex::new(State { initial, kept, values })), monitor };
        variables.report();
        variables
    }

    /// Takes over the values of the variables that keep theirs, from the
    /// session this one replaces (the macro was edited in between).
    pub fn carry_over(&self, previous: &Map<String, Value>) {
        {
            let mut state = self.lock();
            for name in state.kept.clone() {
                if let Some(value) = previous.get(&name) {
                    state.values.insert(name, value.clone());
                }
            }
        }
        self.report();
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn report(&self) {
        let values = self.lock().values.clone();
        self.monitor.set_variables(values);
    }

    /// Variables that reset go back to their starting values; the ones that
    /// keep their value keep it. Variables made by blocks or scripts that the
    /// project does not list are forgotten.
    pub fn reset(&self) {
        {
            let mut state = self.lock();
            let mut values = state.initial.clone();
            for name in &state.kept {
                values.insert(name.clone(), state.values.get(name).cloned().unwrap_or(Value::Null));
            }
            state.values = values;
        }
        self.report();
    }

    pub fn get(&self, name: &str) -> Option<Value> {
        self.lock().values.get(name).cloned()
    }

    /// Sets a variable, creating it if it does not exist yet.
    pub fn set(&self, name: &str, value: Value) {
        self.lock().values.insert(name.to_owned(), value);
        self.report();
    }

    /// Changes a variable in one step, so two runs counting at once never
    /// lose a count. Returns the new value.
    pub fn update(&self, name: &str, change: impl FnOnce(&Value) -> Value) -> Value {
        let new = {
            let mut state = self.lock();
            let current = state.values.get(name).cloned().unwrap_or(Value::Null);
            let new = change(&current);
            state.values.insert(name.to_owned(), new.clone());
            new
        };
        self.report();
        new
    }

    pub fn snapshot(&self) -> Map<String, Value> {
        self.lock().values.clone()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn variables_reset_to_their_starting_values() {
        let monitor = Arc::new(Monitor::new());
        let vars = Variables::new(&[Variable::new("delay", json!(100.0))], Arc::clone(&monitor));
        vars.set("delay", json!(5.0));
        vars.update("count", |v| json!(v.as_f64().unwrap_or(0.0) + 1.0));
        assert_eq!(vars.get("count"), Some(json!(1.0)));
        assert_eq!(monitor.variables().get("delay"), Some(&json!(5.0)), "the monitor sees changes");
        vars.reset();
        assert_eq!(vars.get("delay"), Some(json!(100.0)));
        assert_eq!(vars.get("count"), None);
    }

    #[test]
    fn kept_variables_keep_their_value_across_runs_and_sessions() {
        let monitor = Arc::new(Monitor::new());
        let list = [Variable::kept("total"), Variable::new("step", json!(1))];
        let vars = Variables::new(&list, Arc::clone(&monitor));
        assert_eq!(vars.get("total"), Some(Value::Null), "it starts empty");
        vars.set("total", json!(7));
        vars.set("step", json!(5));
        vars.reset();
        assert_eq!(vars.get("total"), Some(json!(7)), "a new run keeps it");
        assert_eq!(vars.get("step"), Some(json!(1)), "a reset variable starts over");

        // The macro was edited: a new session takes the kept value over.
        let next = Variables::new(&list, monitor);
        next.carry_over(&vars.snapshot());
        assert_eq!(next.get("total"), Some(json!(7)));
        assert_eq!(next.get("step"), Some(json!(1)));
    }
}

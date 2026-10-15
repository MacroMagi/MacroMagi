//! The variables of a loaded macro, shared by every run of it.
//!
//! A session holds one [`Variables`]. They start at the values the project
//! lists, are reset to those values whenever a run starts while nothing else
//! is running, and every change is reported to the [`Monitor`] so the editor
//! can show the current values.

use std::sync::{Arc, Mutex, MutexGuard};

use magi_core::variables::Variable;
use serde_json::{Map, Value};

use crate::monitor::Monitor;

#[derive(Default)]
struct State {
    initial: Map<String, Value>,
    values: Map<String, Value>,
}

#[derive(Clone)]
pub struct Variables {
    state: Arc<Mutex<State>>,
    monitor: Arc<Monitor>,
}

impl Variables {
    pub fn new(initial: &[Variable], monitor: Arc<Monitor>) -> Self {
        let initial: Map<String, Value> = initial.iter().map(|v| (v.name.clone(), v.value.clone())).collect();
        let variables = Self { state: Arc::new(Mutex::new(State { values: initial.clone(), initial })), monitor };
        variables.report();
        variables
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn report(&self) {
        let values = self.lock().values.clone();
        self.monitor.set_variables(values);
    }

    /// Back to the starting values.
    pub fn reset(&self) {
        {
            let mut state = self.lock();
            state.values = state.initial.clone();
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
}

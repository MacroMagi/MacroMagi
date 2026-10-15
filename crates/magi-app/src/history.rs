//! Undo and redo for a value edited directly by the UI, such as the Auto
//! Clicker's settings. Works like [`crate::document::Document`]: once per
//! frame [`History::track`] compares the value with what it last saw, and a
//! settled change becomes one undo step.

const MAX_STEPS: usize = 100;

pub struct History<T> {
    /// The value as of the last undo step.
    committed: T,
    undo: Vec<T>,
    redo: Vec<T>,
}

impl<T: Clone + PartialEq> History<T> {
    pub fn new(value: &T) -> Self {
        Self { committed: value.clone(), undo: Vec::new(), redo: Vec::new() }
    }

    /// Call once per frame. `settled` is false while an edit is still in
    /// progress (a drag, a focused text field), so one gesture is one step.
    pub fn track(&mut self, current: &T, settled: bool) {
        if settled && *current != self.committed {
            let previous = std::mem::replace(&mut self.committed, current.clone());
            self.undo.push(previous);
            if self.undo.len() > MAX_STEPS {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
    }

    pub fn can_undo(&self, current: &T) -> bool {
        !self.undo.is_empty() || *current != self.committed
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self, current: &mut T) {
        self.track(current, true);
        if let Some(previous) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.committed, previous));
            *current = self.committed.clone();
        }
    }

    pub fn redo(&mut self, current: &mut T) {
        self.track(current, true);
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.committed, next));
            *current = self.committed.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settled_changes_are_steps_and_redo_is_dropped_by_new_edits() {
        let mut value = 1;
        let mut history = History::new(&value);
        assert!(!history.can_undo(&value));

        value = 2;
        history.track(&value, false);
        value = 3;
        history.track(&value, true);
        value = 4;
        history.track(&value, true);

        history.undo(&mut value);
        assert_eq!(value, 3);
        history.undo(&mut value);
        assert_eq!(value, 1, "2 was never settled, so it is not a step");
        assert!(!history.can_undo(&value));

        history.redo(&mut value);
        assert_eq!(value, 3);
        value = 9;
        history.track(&value, true);
        assert!(!history.can_redo());
    }
}

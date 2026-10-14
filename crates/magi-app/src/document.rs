//! The macro being edited: the project, where it is saved, and its undo history.
//!
//! Editing code never has to say "I changed something". Once per frame
//! [`Document::track`] compares the project with what it last saw; from that
//! one comparison come the revision counter (which tells the controller to
//! pick up the new version), the unsaved-changes marker, and undo steps.

use std::path::PathBuf;

use magi_core::assets::AssetStore;
use magi_core::graph::Graph;
use magi_core::nodes::Registry;
use magi_core::project::Project;

const MAX_UNDO_STEPS: usize = 200;

/// The part of a project that undo covers.
#[derive(Clone, PartialEq)]
struct Snapshot {
    graph: Graph,
    assets: AssetStore,
}

impl Snapshot {
    fn of(project: &Project) -> Self {
        // Assets are reference-counted, so this copies the graph and a few pointers.
        Self { graph: project.graph.clone(), assets: project.assets.clone() }
    }

    fn matches(&self, project: &Project) -> bool {
        self.graph == project.graph && self.assets == project.assets
    }
}

pub struct Document {
    pub project: Project,
    pub path: Option<PathBuf>,
    /// Increases every time the graph or its assets change.
    revision: u64,
    saved_revision: u64,
    /// The project as of the last frame.
    seen: Snapshot,
    /// The project as of the last undo step.
    committed: Snapshot,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
}

impl Document {
    pub fn new(project: Project, path: Option<PathBuf>) -> Self {
        let snapshot = Snapshot::of(&project);
        Self {
            project,
            path,
            revision: 1,
            saved_revision: 1,
            seen: snapshot.clone(),
            committed: snapshot,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    pub fn starter(registry: &Registry) -> Self {
        Self::new(Project::starter(registry), None)
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn is_dirty(&self) -> bool {
        self.revision != self.saved_revision
    }

    pub fn mark_saved(&mut self, path: PathBuf) {
        self.path = Some(path);
        self.saved_revision = self.revision;
    }

    /// File name for the title bar.
    pub fn title(&self) -> String {
        self.path
            .as_ref()
            .and_then(|path| path.file_stem())
            .map_or_else(|| "Untitled".to_owned(), |stem| stem.to_string_lossy().into_owned())
    }

    /// Call once per frame. `settled` should be false while an edit is still
    /// in progress (a drag, a text field with focus), so that one gesture
    /// becomes one undo step instead of dozens.
    pub fn track(&mut self, settled: bool) {
        if !self.seen.matches(&self.project) {
            self.seen = Snapshot::of(&self.project);
            self.revision += 1;
        }
        if settled && self.committed != self.seen {
            let previous = std::mem::replace(&mut self.committed, self.seen.clone());
            self.undo.push(previous);
            if self.undo.len() > MAX_UNDO_STEPS {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty() || self.committed != self.seen
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) {
        // Fold any in-progress edit into a step first, so undo reverts it.
        self.track(true);
        if let Some(previous) = self.undo.pop() {
            let current = std::mem::replace(&mut self.committed, previous);
            self.redo.push(current);
            self.restore();
        }
    }

    pub fn redo(&mut self) {
        self.track(true);
        if let Some(next) = self.redo.pop() {
            let current = std::mem::replace(&mut self.committed, next);
            self.undo.push(current);
            self.restore();
        }
    }

    fn restore(&mut self) {
        self.project.graph = self.committed.graph.clone();
        self.project.assets = self.committed.assets.clone();
        self.seen = self.committed.clone();
        self.revision += 1;
    }
}

#[cfg(test)]
mod tests {
    use magi_core::nodes::ids;

    use super::*;

    #[test]
    fn edits_become_undo_steps_only_once_settled() {
        let registry = Registry::builtin();
        let mut doc = Document::starter(&registry);
        assert!(!doc.is_dirty() && !doc.can_undo());

        // A drag in progress: the node moves every frame.
        let id = doc.project.graph.starts().next().unwrap().id;
        for x in 1..=5 {
            doc.project.graph.node_mut(id).unwrap().pos = [x as f32, 0.0];
            doc.track(false);
        }
        assert!(doc.is_dirty());
        doc.track(true);

        doc.project.graph.add(&registry, ids::DELAY, [0.0, 100.0]).unwrap();
        doc.track(true);
        assert_eq!(doc.project.graph.len(), 2);

        doc.undo();
        assert_eq!(doc.project.graph.len(), 1);
        assert_eq!(doc.project.graph.node(id).unwrap().pos, [5.0, 0.0]);
        doc.undo();
        assert_eq!(doc.project.graph.node(id).unwrap().pos, [0.0, 0.0], "the whole drag is one step");
        assert!(!doc.can_undo());

        doc.redo();
        doc.redo();
        assert_eq!(doc.project.graph.len(), 2);
        assert!(!doc.can_redo());
    }

    #[test]
    fn a_new_edit_discards_the_redo_branch() {
        let registry = Registry::builtin();
        let mut doc = Document::starter(&registry);
        doc.project.graph.add(&registry, ids::DELAY, [0.0, 0.0]).unwrap();
        doc.track(true);
        doc.undo();
        assert!(doc.can_redo());
        doc.project.graph.add(&registry, ids::LOG, [0.0, 0.0]).unwrap();
        doc.track(true);
        assert!(!doc.can_redo());
    }

    #[test]
    fn saving_clears_the_dirty_flag_until_the_next_change() {
        let registry = Registry::builtin();
        let mut doc = Document::starter(&registry);
        doc.project.graph.add(&registry, ids::DELAY, [0.0, 0.0]).unwrap();
        doc.track(true);
        doc.mark_saved(PathBuf::from("x.magi"));
        assert!(!doc.is_dirty());
        assert_eq!(doc.title(), "x");
        doc.undo();
        assert!(doc.is_dirty());
    }
}

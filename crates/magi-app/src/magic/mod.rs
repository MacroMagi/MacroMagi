//! The Magic tab's side of things that is not drawing: importing models, the
//! model being edited, and trying it out live.

pub mod import;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use magi_core::geom::Displays;
use magi_core::graph::{Fragment, Node, PortRef, Wire};
use magi_core::host::Host;
use magi_core::input::InputEvent;
use magi_core::magic::{Decision, MagicFile, MagicModel};
use magi_core::nodes::ai::AiWatchParams;
use magi_core::nodes::flow::StartParams;
use magi_core::nodes::notes::CommentParams;
use magi_core::nodes::{Registry, ids, ports};

/// The model open in the Magic tab.
pub struct MagicDoc {
    pub file: MagicFile,
    pub path: Option<PathBuf>,
    /// What the importer found and guessed.
    pub notes: Vec<String>,
    /// The model's shapes, read when it was loaded.
    pub shape: Option<magi_ml::Shape>,
    /// The description as last saved, to tell whether there are changes.
    saved: Option<(MagicModel, usize)>,
    /// The description as last put into the macro, where it is kept too.
    kept: Option<(MagicModel, usize)>,
}

impl MagicDoc {
    pub fn new(mut file: MagicFile, path: Option<PathBuf>, notes: Vec<String>) -> Self {
        // The values the settings decide, as the tab keeps them, so a file
        // written by hand does not look changed as soon as it is open.
        file.model = file.model.configured(&serde_json::Map::new());
        let shape = (!file.onnx.is_empty()).then(|| magi_ml::inspect(&file.onnx).ok()).flatten();
        let saved = path.as_ref().map(|_| (file.model.clone(), file.onnx.len()));
        Self { file, path, notes, shape, saved, kept: None }
    }

    pub fn is_dirty(&self) -> bool {
        self.saved.as_ref().is_none_or(|(model, onnx)| *model != self.file.model || *onnx != self.file.onnx.len())
    }

    /// Whether closing it would lose changes: ones neither saved to a file
    /// nor put into the macro.
    pub fn would_lose_changes(&self) -> bool {
        self.is_dirty()
            && self.kept.as_ref().is_none_or(|(model, onnx)| *model != self.file.model || *onnx != self.file.onnx.len())
    }

    /// Notes that the model as it is now is in the macro.
    pub fn mark_kept(&mut self) {
        self.kept = Some((self.file.model.clone(), self.file.onnx.len()));
    }

    pub fn mark_saved(&mut self, path: PathBuf) {
        self.path = Some(path);
        self.saved = Some((self.file.model.clone(), self.file.onnx.len()));
    }

    pub fn title(&self) -> String {
        if self.file.model.name.trim().is_empty() { "Untitled model".into() } else { self.file.model.name.clone() }
    }

    /// Replaces the model itself, keeping the configuration.
    pub fn replace_onnx(&mut self, onnx: Vec<u8>) -> Result<(), String> {
        let shape = magi_ml::inspect(&onnx).map_err(|e| e.0)?;
        self.file.model.input = shape.suggested(self.file.model.input.clone());
        self.file.onnx = onnx;
        self.shape = Some(shape);
        Ok(())
    }
}

/// The blocks a model comes with: a Start block and an AI Watch block, with
/// a comment saying how to use them. `model` is the asset id in the macro.
pub fn starter_blocks(model: &MagicModel, asset: &str, registry: &Registry) -> Fragment {
    let defaults = |kind: &str| registry.get(kind).map(|def| def.default_params()).unwrap_or_default();
    let start = Node::new(1, ids::START, [0.0, 0.0], {
        let mut params = defaults(ids::START);
        params.extend(
            serde_json::to_value(StartParams::default()).ok().and_then(|v| v.as_object().cloned()).unwrap_or_default(),
        );
        params
    });
    let watch_params =
        AiWatchParams { model: asset.to_owned(), display: model.capture.display, ..AiWatchParams::default() };
    let mut watch = Node::new(2, ids::AI_WATCH, [0.0, 130.0], defaults(ids::AI_WATCH));
    watch.set_params(&watch_params);
    let note = CommentParams {
        text: format!(
            "“{}” watches the screen while this runs and presses {} when it should. Press Run (or give the Start \
             block a hotkey) to start, and the stop hotkey to stop. Its settings are in the AI Watch block.",
            model.name,
            model.action.key.label()
        ),
        width: 260.0,
        height: 130.0,
        ..CommentParams::default()
    };
    let mut comment = Node::new(3, ids::COMMENT, [260.0, 0.0], defaults(ids::COMMENT));
    comment.label = model.name.clone();
    comment.set_params(&note);
    Fragment {
        nodes: vec![start, watch, comment],
        wires: vec![Wire { from: PortRef::new(1, ports::OUT), to: PortRef::new(2, ports::FLOW_IN) }],
        groups: Vec::new(),
    }
}

/// What the live test shows.
#[derive(Default)]
pub struct TestState {
    /// The picture the model saw last (RGBA, as captured) and its size.
    pub picture: Option<(Vec<u8>, u32, u32)>,
    /// Bumped with every new picture, so the screen knows to update.
    pub frame: u64,
    pub probabilities: Vec<f32>,
    pub decision: Option<Decision>,
    pub looks_per_second: f64,
    pub engine: String,
    pub acted: u64,
    pub error: Option<String>,
}

/// Runs a model on the screen, over and over, in the background.
pub struct Tester {
    pub state: Arc<Mutex<TestState>>,
    /// The configuration in use; changes made while testing take effect at once.
    pub config: Arc<Mutex<MagicModel>>,
    stop: Arc<AtomicBool>,
}

impl Tester {
    /// Starts testing. With `press`, the model's key is pressed (through
    /// `host`) whenever the model says to act.
    pub fn start(
        onnx: Vec<u8>,
        config: MagicModel,
        displays: Displays,
        host: Arc<dyn Host>,
        press: bool,
        repaint: impl Fn() + Send + 'static,
    ) -> Self {
        let state = Arc::new(Mutex::new(TestState::default()));
        let config = Arc::new(Mutex::new(config));
        let stop = Arc::new(AtomicBool::new(false));
        let (for_state, for_config, for_stop) = (Arc::clone(&state), Arc::clone(&config), Arc::clone(&stop));
        std::thread::Builder::new()
            .name("magi-magic-test".into())
            .spawn(move || {
                run_test(&onnx, &for_config, &displays, host.as_ref(), press, &for_state, &for_stop, &repaint)
            })
            .expect("a thread can be started");
        Self { state, config, stop }
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for Tester {
    fn drop(&mut self) {
        self.stop();
    }
}

#[allow(clippy::too_many_arguments)]
fn run_test(
    onnx: &[u8],
    config: &Mutex<MagicModel>,
    displays: &Displays,
    host: &dyn Host,
    press: bool,
    state: &Mutex<TestState>,
    stop: &AtomicBool,
    repaint: &dyn Fn(),
) {
    let lock = || state.lock().unwrap_or_else(|p| p.into_inner());
    let spec = config.lock().unwrap_or_else(|p| p.into_inner()).input.clone();
    let mut model = match magi_ml::Model::load(onnx, &spec) {
        Ok(model) => model,
        Err(error) => {
            lock().error = Some(error.0);
            repaint();
            return;
        }
    };
    lock().engine = model.engine().label();
    let (mut looks, mut counted_from, mut last_repaint) = (0u32, Instant::now(), Instant::now());
    let mut resting_until: Option<Instant> = None;
    while !stop.load(Ordering::Relaxed) {
        let current = config.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let Some(rect) = current.capture.rect(displays, None) else {
            lock().error = Some("there is no display to look at".into());
            break;
        };
        let frame = match host.capture(Some(rect)) {
            Ok(frame) => frame,
            Err(error) => {
                lock().error = Some(error.0);
                break;
            }
        };
        let probabilities = match model.classify(&frame.rgba, frame.width, frame.height) {
            Ok(p) => p,
            Err(error) => {
                lock().error = Some(error.0);
                break;
            }
        };
        let decision = current.decide(&probabilities);
        looks += 1;
        let resting = resting_until.is_some_and(|until| Instant::now() < until);
        let act = decision.act && !resting;
        if act {
            resting_until = Some(
                Instant::now()
                    + Duration::from_secs_f64((decision.delay_ms + current.action.cooldown_ms).max(0.0) / 1000.0),
            );
        }
        {
            let mut shown = lock();
            shown.probabilities = probabilities;
            shown.decision = Some(decision.clone());
            if act {
                shown.acted += 1;
            }
            if last_repaint.elapsed() >= Duration::from_millis(33) {
                shown.picture = Some((frame.rgba, frame.width, frame.height));
                shown.frame += 1;
            }
            if counted_from.elapsed() >= Duration::from_secs(1) {
                shown.looks_per_second = f64::from(looks) / counted_from.elapsed().as_secs_f64();
                (looks, counted_from) = (0, Instant::now());
            }
        }
        if act && press {
            std::thread::sleep(Duration::from_secs_f64(decision.delay_ms.max(0.0) / 1000.0));
            let key = current.action.key;
            let _ = host.send(&InputEvent::KeyDown { key });
            std::thread::sleep(Duration::from_secs_f64(current.action.hold_ms.max(0.0) / 1000.0));
            let _ = host.send(&InputEvent::KeyUp { key });
        }
        if last_repaint.elapsed() >= Duration::from_millis(33) {
            last_repaint = Instant::now();
            repaint();
        }
    }
    repaint();
}

#[cfg(test)]
mod tests {
    use magi_core::magic::{ClassSpec, InputSpec};

    use super::*;

    #[test]
    fn a_model_comes_with_blocks_that_run_it() {
        let registry = Registry::builtin();
        let model = MagicModel {
            name: "Watcher".into(),
            classes: vec![ClassSpec { label: "go".into(), act: true, ..ClassSpec::default() }],
            input: InputSpec::default(),
            ..MagicModel::default()
        };
        let blocks = starter_blocks(&model, "mdl-1", &registry);
        let mut graph = magi_core::graph::Graph::new();
        let ids = graph.insert_fragment(&blocks, [0.0, 0.0]);
        assert_eq!(ids.len(), 3);
        let watch = graph.nodes().find(|n| n.kind == ids::AI_WATCH).unwrap();
        assert_eq!(watch.param::<String>("model").as_deref(), Some("mdl-1"));
        let watch = watch.id;
        assert_eq!(graph.prune(&registry), 0, "the wire fits");
        let start = graph.nodes().find(|n| n.kind == ids::START).unwrap().id;
        assert_eq!(graph.flow_target(start, ports::OUT).map(|t| t.node), Some(watch));
    }
}

//! The native engine: a small interpreter that walks the graph in Rust.
//!
//! Execution follows flow wires from a Start block, one block at a time.
//! Value blocks (Image Detection, Logic, …) are not part of that order; they
//! are evaluated on demand when a flow block reads one of its inputs, and each
//! is evaluated at most once per step so that, for example, a Decision with
//! three cases fed by one Image Detection takes a single screenshot.
//!
//! Behaviour is registered per node type in [`NativeNodes`], keyed by the same
//! ids as the definitions in `magi_core::nodes`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use magi_core::autoclicker::AutoClickParams;
use magi_core::geom::Point;
use magi_core::graph::{Node, NodeId};
use magi_core::nodes::flow::{
    DecisionParams, DelayParams, LogParams, RepeatParams, StopParams, StopScope, WaitUntilParams,
};
use magi_core::nodes::input::{ActionParams, PlayRecordingParams};
use magi_core::nodes::logic::{ChanceParams, GateOp, GateParams};
use magi_core::nodes::vision::{FindColorParams, FindImageParams};
use magi_core::nodes::window::FocusWindowParams;
use magi_core::nodes::{ids, ports};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use crate::monitor::LogLevel;
use crate::ops::RunCx;
use crate::program::pass_through;
use crate::{EngineError, Halt, OpResult};

/// Value blocks may feed value blocks; this bounds the chain.
const MAX_VALUE_DEPTH: u32 = 64;
/// After this many steps without any waiting, the run yields briefly so that
/// a loop with nothing to wait for cannot pin a CPU core.
const BUSY_STEPS: u32 = 2000;

/// What a flow block wants to happen next.
pub enum Next {
    /// Follow the wire from this flow output (if nothing is wired, the branch ends).
    Port(String),
    /// End this Start block's run.
    StopRun,
    /// End every run of the macro.
    StopAll,
}

impl Next {
    pub fn out() -> Self {
        Next::Port(ports::OUT.to_owned())
    }
}

pub type FlowFn = fn(&mut Interp<'_>, &Node) -> OpResult<Next>;
/// Computes all outputs of a value block at once.
pub type ValueFn = fn(&mut Interp<'_>, &Node) -> OpResult<Map<String, Value>>;

/// The native implementations of node types.
#[derive(Clone, Default)]
pub struct NativeNodes {
    flow: HashMap<String, FlowFn>,
    value: HashMap<String, ValueFn>,
}

impl NativeNodes {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn builtin() -> Self {
        let mut nodes = Self::default();
        nodes.flow(ids::START, |_, _| Ok(Next::out()));
        nodes.flow(ids::DELAY, delay);
        nodes.flow(ids::REPEAT, repeat);
        nodes.flow(ids::DECISION, decision);
        nodes.flow(ids::WAIT_UNTIL, wait_until);
        nodes.flow(ids::STOP, stop);
        nodes.flow(ids::LOG, log);
        nodes.flow(ids::INPUT, input);
        nodes.flow(ids::PLAY_RECORDING, play_recording);
        nodes.flow(ids::AUTOCLICK, autoclick);
        nodes.flow(ids::FOCUS_WINDOW, focus_window);
        nodes.flow(ids::PYTHON, |_, _| {
            Err(EngineError::new(
                "Python Script blocks need the Python engine. Choose it in Settings (Python 3.8+ must be installed).",
            )
            .into())
        });
        nodes.value(ids::FIND_IMAGE, find_image);
        nodes.value(ids::FIND_COLOR, find_color);
        nodes.value(ids::GATE, gate);
        nodes.value(ids::CHANCE, chance);
        nodes
    }

    /// Registers (or replaces) the behaviour of a flow block.
    pub fn flow(&mut self, id: &str, run: FlowFn) {
        self.flow.insert(id.to_owned(), run);
    }

    /// Registers (or replaces) the behaviour of a value block.
    pub fn value(&mut self, id: &str, eval: ValueFn) {
        self.value.insert(id.to_owned(), eval);
    }
}

struct LoopFrame {
    node: NodeId,
    /// Iterations still to run; `None` loops until stopped.
    remaining: Option<u64>,
}

pub struct Interp<'a> {
    pub cx: &'a mut RunCx,
    nodes: &'a NativeNodes,
    loops: Vec<LoopFrame>,
    /// Outputs of value blocks evaluated during the current step.
    memo: HashMap<NodeId, Map<String, Value>>,
    /// Outputs of flow blocks (e.g. the screen type Switch to Window found),
    /// kept until the block runs again.
    pub outputs: HashMap<NodeId, Map<String, Value>>,
    depth: u32,
}

/// Runs the flow that begins at `start` until it ends, fails, or is stopped.
pub(crate) fn run(cx: &mut RunCx, nodes: &NativeNodes, start: NodeId) -> Result<(), EngineError> {
    let mut interp = Interp { cx, nodes, loops: Vec::new(), memo: HashMap::new(), outputs: HashMap::new(), depth: 0 };
    match interp.run_from(start) {
        Ok(()) | Err(Halt::Stopped) => Ok(()),
        Err(Halt::Failed(error)) => Err(error),
    }
}

impl Interp<'_> {
    fn run_from(&mut self, start: NodeId) -> OpResult<()> {
        let program = Arc::clone(&self.cx.program);
        let mut current = Some(start);
        let mut busy_steps = 0;
        loop {
            self.cx.check_stop()?;
            let Some(id) = current else {
                // The branch ran out. Inside a Repeat that means "next iteration".
                match self.loops.last() {
                    Some(frame) => current = Some(frame.node),
                    None => return Ok(()),
                }
                continue;
            };

            if std::mem::take(&mut self.cx.waited) {
                busy_steps = 0;
            } else {
                busy_steps += 1;
                if busy_steps >= BUSY_STEPS {
                    busy_steps = 0;
                    self.cx.sleep(Duration::from_millis(1))?;
                }
            }

            let node = program.node(id)?;
            if !node.enabled {
                current = pass_through(&program.registry.ports(node))
                    .and_then(|port| program.graph.flow_target(id, &port))
                    .map(|target| target.node);
                continue;
            }

            self.memo.clear();
            self.cx.monitor.enter(id);
            let Some(run) = self.nodes.flow.get(&node.kind) else {
                return Err(
                    EngineError::at(node.id, format!("“{}” cannot be run by the native engine", node.kind)).into()
                );
            };
            match run(self, node).map_err(|halt| halt.located(id))? {
                Next::Port(port) => current = program.graph.flow_target(id, &port).map(|target| target.node),
                Next::StopRun => return Ok(()),
                Next::StopAll => {
                    (self.cx.stop_everything)();
                    return Ok(());
                }
            }
        }
    }

    /// Reads a data input. `None` means nothing is wired to it.
    pub fn input(&mut self, node: &Node, port: &str) -> OpResult<Option<Value>> {
        let program = Arc::clone(&self.cx.program);
        let Some(source) = program.graph.data_source(node.id, port) else {
            return Ok(None);
        };
        let source_node = program.node(source.node)?;
        if !source_node.enabled {
            return Ok(Some(Value::Null));
        }
        if let Some(values) = self.outputs.get(&source.node) {
            return Ok(Some(values.get(&source.port).cloned().unwrap_or(Value::Null)));
        }
        if !self.memo.contains_key(&source.node) {
            let Some(eval) = self.nodes.value.get(&source_node.kind) else {
                // A flow block that has not run yet, or an unknown block.
                return Ok(Some(Value::Null));
            };
            if self.depth >= MAX_VALUE_DEPTH {
                return Err(EngineError::at(source.node, "values are wired in a loop").into());
            }
            self.cx.monitor.enter(source.node);
            self.depth += 1;
            let outputs = eval(self, source_node);
            self.depth -= 1;
            self.memo.insert(source.node, outputs.map_err(|halt| halt.located(source.node))?);
        }
        Ok(Some(self.memo[&source.node].get(&source.port).cloned().unwrap_or(Value::Null)))
    }

    /// Reads a data input as true/false. Unwired inputs are false.
    pub fn input_bool(&mut self, node: &Node, port: &str) -> OpResult<bool> {
        Ok(self.input(node, port)?.is_some_and(|value| truthy(&value)))
    }
}

/// How values are read as true/false.
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// Reads `{ "x": …, "y": … }` as a screen position.
pub fn point_from(value: &Value) -> Option<Point> {
    let coordinate = |key: &str| value.get(key)?.as_f64().map(|v| v.round() as i32);
    Some(Point::new(coordinate("x")?, coordinate("y")?))
}

fn params<T: DeserializeOwned>(node: &Node) -> OpResult<T> {
    node.params_as().map_err(|e| EngineError::new(format!("its settings are invalid ({e})")).into())
}

fn single(value: Value) -> Map<String, Value> {
    Map::from_iter([(ports::OUT.to_owned(), value)])
}

// ---- flow blocks ----------------------------------------------------------

fn delay(interp: &mut Interp<'_>, node: &Node) -> OpResult<Next> {
    let p: DelayParams = params(node)?;
    let ms = interp.cx.jittered(p.ms, p.jitter_ms);
    interp.cx.sleep_ms(ms)?;
    Ok(Next::out())
}

fn repeat(interp: &mut Interp<'_>, node: &Node) -> OpResult<Next> {
    let p: RepeatParams = params(node)?;
    // Arriving while our own frame is on top means the looped branch came back.
    let returning = interp.loops.last().is_some_and(|frame| frame.node == node.id);
    if !returning {
        interp.loops.push(LoopFrame { node: node.id, remaining: p.iterations() });
    }
    let frame = interp.loops.last_mut().expect("a frame was just ensured");
    match frame.remaining {
        Some(0) => {
            interp.loops.pop();
            Ok(Next::Port(ports::DONE.to_owned()))
        }
        Some(n) => {
            frame.remaining = Some(n - 1);
            Ok(Next::Port(ports::LOOP.to_owned()))
        }
        None => Ok(Next::Port(ports::LOOP.to_owned())),
    }
}

fn decision(interp: &mut Interp<'_>, node: &Node) -> OpResult<Next> {
    let p: DecisionParams = params(node)?;
    for case in &p.cases {
        if interp.input_bool(node, &ports::condition(&case.id))? {
            return Ok(Next::Port(ports::case(&case.id)));
        }
    }
    Ok(Next::Port(ports::ELSE.to_owned()))
}

fn wait_until(interp: &mut Interp<'_>, node: &Node) -> OpResult<Next> {
    let p: WaitUntilParams = params(node)?;
    let started = Instant::now();
    loop {
        interp.memo.clear(); // look again, don't reuse the last answer
        if interp.input_bool(node, "condition")? {
            return Ok(Next::out());
        }
        if p.timeout_ms > 0.0 && started.elapsed().as_secs_f64() * 1000.0 >= p.timeout_ms {
            return Ok(Next::Port(ports::TIMEOUT.to_owned()));
        }
        interp.cx.sleep_ms(p.poll_ms.max(1.0))?;
    }
}

fn stop(_: &mut Interp<'_>, node: &Node) -> OpResult<Next> {
    Ok(match params::<StopParams>(node)?.scope {
        StopScope::Branch => Next::StopRun,
        StopScope::All => Next::StopAll,
    })
}

fn log(interp: &mut Interp<'_>, node: &Node) -> OpResult<Next> {
    let p: LogParams = params(node)?;
    let message = match interp.input(node, "value")? {
        Some(Value::String(text)) => format!("{} {text}", p.message),
        Some(value) => format!("{} {value}", p.message),
        None => p.message,
    };
    interp.cx.log(LogLevel::Info, Some(node.id), message.trim());
    Ok(Next::out())
}

fn input(interp: &mut Interp<'_>, node: &Node) -> OpResult<Next> {
    let p: ActionParams = params(node)?;
    let position = match interp.input(node, "position")? {
        None => None,
        Some(value) => match point_from(&value) {
            Some(point) => Some(point),
            None => {
                // The position was supposed to come from somewhere (say, an
                // image that was not found). Acting at the wrong place would
                // be worse than not acting.
                interp.cx.log(LogLevel::Warning, Some(node.id), "No position available – input skipped");
                return Ok(Next::out());
            }
        },
    };
    interp.cx.perform_actions(&p, position)?;
    Ok(Next::out())
}

fn autoclick(interp: &mut Interp<'_>, node: &Node) -> OpResult<Next> {
    let p: AutoClickParams = params(node)?;
    interp.cx.autoclick(&p.clicker)?;
    Ok(Next::out())
}

fn focus_window(interp: &mut Interp<'_>, node: &Node) -> OpResult<Next> {
    let p: FocusWindowParams = params(node)?;
    let found = interp.cx.focus_window(&p)?;
    let screen = found.as_ref().map(|info| info.kind.id());
    interp.outputs.insert(node.id, Map::from_iter([("screen".to_owned(), json!(screen))]));
    Ok(match found {
        Some(_) => Next::out(),
        None => {
            interp.cx.log(LogLevel::Warning, Some(node.id), format!("No window matching “{}” was found", p.target));
            Next::Port("missing".to_owned())
        }
    })
}

fn play_recording(interp: &mut Interp<'_>, node: &Node) -> OpResult<Next> {
    let p: PlayRecordingParams = params(node)?;
    interp.cx.play_recording(&p)?;
    Ok(Next::out())
}

// ---- value blocks ---------------------------------------------------------

fn find_image(interp: &mut Interp<'_>, node: &Node) -> OpResult<Map<String, Value>> {
    let p: FindImageParams = params(node)?;
    Ok(interp.cx.find_image(&p)?.to_json())
}

fn find_color(interp: &mut Interp<'_>, node: &Node) -> OpResult<Map<String, Value>> {
    let p: FindColorParams = params(node)?;
    Ok(interp.cx.find_color(&p)?.to_json())
}

fn gate(interp: &mut Interp<'_>, node: &Node) -> OpResult<Map<String, Value>> {
    let p: GateParams = params(node)?;
    let a = interp.input_bool(node, "a")?;
    let b = if p.op == GateOp::Not { false } else { interp.input_bool(node, "b")? };
    Ok(single(json!(p.op.apply(a, b))))
}

fn chance(interp: &mut Interp<'_>, node: &Node) -> OpResult<Map<String, Value>> {
    let p: ChanceParams = params(node)?;
    Ok(single(json!(interp.cx.chance(p.probability))))
}

//! # magi-engine
//!
//! Runs macros. There are two engines behind one API:
//!
//! * **native** ([`native`]) – a Rust interpreter. No dependencies, starts
//!   instantly, always available. The Auto Clicker always uses it.
//! * **Python** ([`python`]) – the interpreter in `runtime/python`, running in
//!   a Python subprocess. It adds Python Script blocks and is where add-on
//!   nodes written in Python will run.
//!
//! Both engines make the same graph do the same thing. They share every
//! device-level operation ([`ops`]): the Python engine does not click or look
//! at the screen itself, it asks this process to, over a pipe.
//!
//! The entry point is [`Session`]: one loaded macro on one engine, from which
//! any number of runs (one per Start block) can be started and stopped.

pub mod monitor;
pub mod native;
pub mod ops;
pub mod program;
pub mod python;
pub mod timing;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

use magi_core::graph::NodeId;
use magi_core::host::{Host, HostError};
use thiserror::Error;

pub use monitor::{Activity, LogEntry, LogLevel, Monitor};
pub use native::NativeNodes;
pub use ops::RunCx;
pub use program::Program;
pub use python::{PythonConfig, PythonRuntime, PythonStatus};
pub use timing::StopToken;

/// A failure while running a macro, phrased for the person who built it.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("{message}")]
pub struct EngineError {
    pub message: String,
    /// The block the failure happened in, when known.
    pub node: Option<NodeId>,
}

impl EngineError {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into(), node: None }
    }

    pub fn at(node: NodeId, message: impl Into<String>) -> Self {
        Self { message: message.into(), node: Some(node) }
    }
}

impl From<HostError> for EngineError {
    fn from(error: HostError) -> Self {
        Self::new(error.0)
    }
}

/// Why an operation did not complete.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Halt {
    /// The run was asked to stop. Not a failure.
    Stopped,
    Failed(EngineError),
}

impl Halt {
    /// Attaches the block a failure happened in, unless one is already attached.
    pub fn located(self, node: NodeId) -> Self {
        match self {
            Halt::Failed(mut error) => {
                error.node.get_or_insert(node);
                Halt::Failed(error)
            }
            Halt::Stopped => Halt::Stopped,
        }
    }
}

impl From<EngineError> for Halt {
    fn from(error: EngineError) -> Self {
        Halt::Failed(error)
    }
}

pub type OpResult<T> = Result<T, Halt>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineKind {
    Native,
    Python,
}

impl EngineKind {
    pub fn label(self) -> &'static str {
        match self {
            EngineKind::Native => "Native",
            EngineKind::Python => "Python",
        }
    }
}

pub type RunId = u64;

static NEXT_RUN_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_PROGRAM_ID: AtomicU64 = AtomicU64::new(1);

enum Backend {
    Native(Arc<NativeNodes>),
    Python { runtime: Arc<PythonRuntime>, program_id: u64 },
}

struct ActiveRun {
    node: NodeId,
    stop: StopToken,
}

struct Shared {
    program: Arc<Program>,
    host: Arc<dyn Host>,
    monitor: Arc<Monitor>,
    backend: Backend,
    runs: Mutex<HashMap<RunId, ActiveRun>>,
    idle: Condvar,
}

impl Shared {
    fn runs(&self) -> MutexGuard<'_, HashMap<RunId, ActiveRun>> {
        self.runs.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn stop(&self, run: RunId) {
        if let Some(active) = self.runs().get(&run) {
            active.stop.stop();
        }
        if let Backend::Python { runtime, .. } = &self.backend {
            runtime.stop_run(run);
        }
    }

    fn stop_all(&self) {
        let ids: Vec<RunId> = self.runs().keys().copied().collect();
        for id in ids {
            self.stop(id);
        }
    }

    fn finish(&self, run: RunId, outcome: Result<(), EngineError>) {
        if let Err(error) = outcome {
            self.monitor.log(LogLevel::Error, error.node, error.message);
        }
        self.runs().remove(&run);
        self.idle.notify_all();
        self.monitor.wake();
    }
}

/// One macro loaded on one engine.
///
/// Dropping the session stops everything it started.
pub struct Session {
    shared: Arc<Shared>,
}

impl Session {
    /// A session on the native engine.
    pub fn native(program: Arc<Program>, host: Arc<dyn Host>, monitor: Arc<Monitor>) -> Self {
        Self::with_backend(program, host, monitor, Backend::Native(Arc::new(NativeNodes::builtin())))
    }

    /// A session on the native engine with a custom set of node implementations.
    pub fn native_with(
        program: Arc<Program>,
        host: Arc<dyn Host>,
        monitor: Arc<Monitor>,
        nodes: Arc<NativeNodes>,
    ) -> Self {
        Self::with_backend(program, host, monitor, Backend::Native(nodes))
    }

    /// A session on the Python engine. Fails if Python cannot be started.
    pub fn python(
        program: Arc<Program>,
        host: Arc<dyn Host>,
        monitor: Arc<Monitor>,
        runtime: Arc<PythonRuntime>,
    ) -> Result<Self, EngineError> {
        runtime.ensure_started()?;
        let program_id = NEXT_PROGRAM_ID.fetch_add(1, Ordering::Relaxed);
        Ok(Self::with_backend(program, host, monitor, Backend::Python { runtime, program_id }))
    }

    fn with_backend(program: Arc<Program>, host: Arc<dyn Host>, monitor: Arc<Monitor>, backend: Backend) -> Self {
        let shared = Arc::new(Shared {
            program,
            host,
            monitor,
            backend,
            runs: Mutex::new(HashMap::new()),
            idle: Condvar::new(),
        });
        Self { shared }
    }

    pub fn engine(&self) -> EngineKind {
        match self.shared.backend {
            Backend::Native(_) => EngineKind::Native,
            Backend::Python { .. } => EngineKind::Python,
        }
    }

    pub fn program(&self) -> &Arc<Program> {
        &self.shared.program
    }

    pub fn monitor(&self) -> &Arc<Monitor> {
        &self.shared.monitor
    }

    /// Starts running the flow that begins at `node` (normally a Start block).
    pub fn start(&self, node: NodeId) -> Result<RunId, EngineError> {
        let shared = &self.shared;
        shared.program.node(node)?;
        let run = NEXT_RUN_ID.fetch_add(1, Ordering::Relaxed);
        let stop = StopToken::new();

        let weak: Weak<Shared> = Arc::downgrade(shared);
        let stop_everything = Arc::new(move || {
            if let Some(shared) = weak.upgrade() {
                shared.stop_all();
            }
        });
        let mut cx = RunCx::new(
            Arc::clone(&shared.host),
            Arc::clone(&shared.program),
            Arc::clone(&shared.monitor),
            stop.clone(),
            stop_everything,
        );
        shared.runs().insert(run, ActiveRun { node, stop });

        let for_run = Arc::clone(shared);
        let started = match &shared.backend {
            Backend::Native(nodes) => {
                let nodes = Arc::clone(nodes);
                std::thread::Builder::new()
                    .name(format!("magi-run-{run}"))
                    .spawn(move || {
                        let outcome = native::run(&mut cx, &nodes, node);
                        cx.release_all();
                        for_run.finish(run, outcome);
                    })
                    .map(drop)
                    .map_err(|e| EngineError::new(format!("could not start a thread for the macro: {e}")))
            }
            Backend::Python { runtime, program_id } => runtime.start_run(
                *program_id,
                &shared.program,
                run,
                node,
                cx,
                Box::new(move |outcome| for_run.finish(run, outcome)),
            ),
        };
        if let Err(error) = started {
            shared.runs().remove(&run);
            return Err(error);
        }
        Ok(run)
    }

    /// Starts every Start block that is set to start with the Run button.
    /// Returns how many were started.
    pub fn start_all(&self) -> Result<usize, EngineError> {
        use magi_core::nodes::flow::StartParams;
        let starts: Vec<NodeId> = self
            .shared
            .program
            .graph
            .starts()
            .filter(|node| node.enabled && node.params_or_default::<StartParams>().run_on_start)
            .map(|node| node.id)
            .collect();
        for &node in &starts {
            if !self.is_running(node) {
                self.start(node)?;
            }
        }
        Ok(starts.len())
    }

    pub fn stop(&self, run: RunId) {
        self.shared.stop(run);
    }

    /// Stops every run that began at `node`.
    pub fn stop_node(&self, node: NodeId) {
        let ids: Vec<RunId> =
            self.shared.runs().iter().filter(|(_, active)| active.node == node).map(|(id, _)| *id).collect();
        for id in ids {
            self.shared.stop(id);
        }
    }

    pub fn stop_all(&self) {
        self.shared.stop_all();
    }

    pub fn is_running(&self, node: NodeId) -> bool {
        self.shared.runs().values().any(|active| active.node == node)
    }

    /// The Start blocks that currently have a run.
    pub fn running_nodes(&self) -> Vec<NodeId> {
        self.shared.runs().values().map(|active| active.node).collect()
    }

    pub fn active_runs(&self) -> usize {
        self.shared.runs().len()
    }

    /// Blocks until no run is active, or until `timeout` passes.
    /// Returns `true` if the session became idle.
    pub fn wait_idle(&self, timeout: Option<Duration>) -> bool {
        let deadline = timeout.map(|t| Instant::now() + t);
        let mut runs = self.shared.runs();
        while !runs.is_empty() {
            let wait = match deadline {
                Some(deadline) => match deadline.checked_duration_since(Instant::now()) {
                    Some(left) if !left.is_zero() => left,
                    _ => return false,
                },
                None => Duration::from_secs(3600),
            };
            runs = self.shared.idle.wait_timeout(runs, wait).unwrap_or_else(|p| p.into_inner()).0;
        }
        true
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.shared.stop_all();
        if let Backend::Python { runtime, program_id } = &self.shared.backend {
            runtime.unload(*program_id);
        }
    }
}

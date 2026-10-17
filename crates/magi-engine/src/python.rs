//! The Python engine: a bridge to the interpreter in `runtime/python`.
//!
//! One Python process serves the whole application. Programs are loaded into
//! it by id and runs are started inside it; each run is a thread on the Python
//! side and has a matching worker thread here that performs its device
//! operations (see [`crate::ops`]) and writes the results back.
//!
//! The Python sources are compiled into this crate and unpacked into the
//! app's data folder on first use, so the app stays a single portable file.
//! The protocol is documented in `runtime/python/magi/protocol.py`.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use include_dir::{Dir, DirEntry, include_dir};
use magi_core::autoclicker::AutoClickParams;
use magi_core::graph::NodeId;
use magi_core::input::InputEvent;
use magi_core::nodes::flow::RateParams;
use magi_core::nodes::logic::{CompareParams, MathParams, TextParams};
use magi_core::variables::{as_number, parse_literal};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::monitor::{LogLevel, Monitor};
use crate::native::point_from;
use crate::ops::RunCx;
use crate::program::Program;
use crate::timing::millis;
use crate::{EngineError, Halt, OpResult, RunId};

/// Must match `PROTOCOL` in `runtime/python/magi/__init__.py`.
const PROTOCOL: u64 = 1;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(12);
/// A run that ignores a stop request for this long takes the process down with it.
const STOP_GRACE: Duration = Duration::from_secs(3);
/// Overrides where the runtime sources are loaded from. For runtime development.
const RUNTIME_OVERRIDE_ENV: &str = "MACROMAGI_PYTHON_RUNTIME";

static RUNTIME_SOURCES: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/../../runtime/python");

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PythonConfig {
    /// An interpreter chosen by the user. When set, nothing else is tried.
    pub interpreter: Option<PathBuf>,
    /// The folder holding the app. A `python` folder next to the app is
    /// preferred over any system-wide installation, for fully portable setups.
    pub app_dir: PathBuf,
    /// The app's data folder; the runtime sources are unpacked here.
    pub data_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PythonStatus {
    /// Not tried yet.
    Unknown,
    Running {
        version: String,
        command: String,
    },
    Unavailable(String),
}

pub struct PythonRuntime {
    config: PythonConfig,
    monitor: Arc<Monitor>,
    state: Mutex<State>,
}

struct State {
    process: Option<Arc<Process>>,
    status: PythonStatus,
}

/// A request from a Python run, routed to its worker thread.
enum Call {
    Op {
        id: u64,
        op: String,
        args: Value,
    },
    /// The run reached a block.
    Enter(NodeId),
    Log {
        level: LogLevel,
        node: Option<NodeId>,
        message: String,
    },
    StopAll,
    Done {
        error: Option<String>,
        node: Option<NodeId>,
    },
}

struct Process {
    child: Mutex<Child>,
    stdin: Mutex<ChildStdin>,
    alive: AtomicBool,
    /// Programs this process has been sent.
    loaded: Mutex<HashSet<u64>>,
    runs: Mutex<HashMap<RunId, Sender<Call>>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Process {
    fn send(&self, message: &Value) -> Result<(), EngineError> {
        let mut stdin = lock(&self.stdin);
        let mut line = message.to_string();
        line.push('\n');
        stdin
            .write_all(line.as_bytes())
            .and_then(|()| stdin.flush())
            .map_err(|_| EngineError::new("the Python engine is not responding"))
    }

    fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    fn kill(&self) {
        self.alive.store(false, Ordering::SeqCst);
        let mut child = lock(&self.child);
        let _ = child.kill();
        let _ = child.wait();
    }
}

impl PythonRuntime {
    pub fn new(config: PythonConfig, monitor: Arc<Monitor>) -> Self {
        Self { config, monitor, state: Mutex::new(State { process: None, status: PythonStatus::Unknown }) }
    }

    pub fn config(&self) -> &PythonConfig {
        &self.config
    }

    /// What is known about Python without trying to start it.
    pub fn status(&self) -> PythonStatus {
        let state = lock(&self.state);
        match &state.process {
            Some(process) if !process.is_alive() && matches!(state.status, PythonStatus::Running { .. }) => {
                PythonStatus::Unknown
            }
            _ => state.status.clone(),
        }
    }

    /// Starts Python if it is not running, and reports the outcome.
    pub fn probe(&self) -> PythonStatus {
        let _ = self.ensure_started();
        self.status()
    }

    pub(crate) fn ensure_started(&self) -> Result<(), EngineError> {
        self.process().map(drop)
    }

    fn process(&self) -> Result<Arc<Process>, EngineError> {
        let mut state = lock(&self.state);
        if let Some(process) = &state.process
            && process.is_alive()
        {
            return Ok(Arc::clone(process));
        }
        match self.spawn() {
            Ok((process, version, command)) => {
                state.status = PythonStatus::Running { version, command };
                state.process = Some(Arc::clone(&process));
                Ok(process)
            }
            Err(reason) => {
                state.status = PythonStatus::Unavailable(reason.clone());
                state.process = None;
                Err(EngineError::new(format!("The Python engine could not start: {reason}")))
            }
        }
    }

    fn spawn(&self) -> Result<(Arc<Process>, String, String), String> {
        let runtime = runtime_dir(&self.config.data_dir)
            .map_err(|e| format!("could not prepare the Python runtime in the data folder ({e})"))?;
        let script = runtime.join("magi_host.py");

        let mut failures = Vec::new();
        for candidate in candidates(&self.config) {
            let label = candidate.iter().map(|part| part.to_string_lossy()).collect::<Vec<_>>().join(" ");
            match self.spawn_with(&candidate, &script) {
                Ok((process, version)) => return Ok((process, version, label)),
                Err(reason) => failures.push(format!("{label}: {reason}")),
            }
        }
        Err(match (&self.config.interpreter, failures.is_empty()) {
            (_, true) => "no Python interpreter was found".to_owned(),
            (Some(_), _) => failures.join("; "),
            (None, _) => format!(
                "no working Python 3.8+ was found. Install Python, or choose an interpreter in Settings. ({})",
                failures.join("; ")
            ),
        })
    }

    fn spawn_with(&self, command: &[OsString], script: &Path) -> Result<(Arc<Process>, String), String> {
        let mut builder = Command::new(&command[0]);
        builder
            .args(&command[1..])
            .arg("-u")
            .arg(script)
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .env("PYTHONIOENCODING", "utf-8")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt as _;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            builder.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = builder.spawn().map_err(|e| e.to_string())?;
        let stdin = child.stdin.take().expect("stdin was piped");
        let stdout = child.stdout.take().expect("stdout was piped");
        let stderr = child.stderr.take().expect("stderr was piped");

        let process = Arc::new(Process {
            child: Mutex::new(child),
            stdin: Mutex::new(stdin),
            alive: AtomicBool::new(true),
            loaded: Mutex::new(HashSet::new()),
            runs: Mutex::new(HashMap::new()),
        });

        // Anything Python prints before it takes over its own stderr (crashes
        // on startup, mostly) is kept to explain a failed start.
        let early_errors = Arc::new(Mutex::new(String::new()));
        {
            let (early_errors, monitor) = (Arc::clone(&early_errors), Arc::clone(&self.monitor));
            spawn_thread("magi-python-stderr", move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    let mut early = lock(&early_errors);
                    if early.len() < 4000 {
                        early.push_str(&line);
                        early.push('\n');
                    }
                    drop(early);
                    monitor.log(LogLevel::Warning, None, format!("[python] {line}"));
                }
            });
        }

        let (ready_tx, ready_rx) = mpsc::channel();
        {
            let (process, monitor) = (Arc::clone(&process), Arc::clone(&self.monitor));
            spawn_thread("magi-python-reader", move || read_messages(&process, stdout, &monitor, &ready_tx));
        }

        match ready_rx.recv_timeout(STARTUP_TIMEOUT) {
            Ok(Ok(version)) => Ok((process, version)),
            Ok(Err(reason)) => {
                process.kill();
                Err(reason)
            }
            Err(_) => {
                process.kill();
                let errors = lock(&early_errors);
                let detail = errors.lines().last().unwrap_or("it did not answer").trim().to_owned();
                Err(detail)
            }
        }
    }

    /// Starts a run inside Python. `on_done` is called exactly once, from the
    /// run's worker thread, when the run has ended for any reason.
    pub(crate) fn start_run(
        &self,
        program_id: u64,
        program: &Arc<Program>,
        run: RunId,
        node: NodeId,
        cx: RunCx,
        on_done: Box<dyn FnOnce(Result<(), EngineError>) + Send>,
    ) -> Result<(), EngineError> {
        let process = self.process()?;
        if lock(&process.loaded).insert(program_id) {
            process.send(&json!({ "t": "load", "program": program_id, "data": program.to_python() }))?;
        }

        let (calls, inbox) = mpsc::channel();
        lock(&process.runs).insert(run, calls);
        {
            let process = Arc::clone(&process);
            spawn_thread(&format!("magi-pyrun-{run}"), move || serve_run(&process, &inbox, cx, on_done));
        }
        if process.send(&json!({ "t": "start", "program": program_id, "run": run, "node": node })).is_err() {
            // Dropping the sender makes the worker report the failure through `on_done`.
            lock(&process.runs).remove(&run);
        }
        Ok(())
    }

    pub(crate) fn stop_run(&self, run: RunId) {
        let Some(process) = lock(&self.state).process.clone() else { return };
        if !process.is_alive() || !lock(&process.runs).contains_key(&run) {
            return;
        }
        let _ = process.send(&json!({ "t": "stop", "run": run }));

        // A script stuck in its own loop never sees the request. Python
        // threads cannot be killed, so after a grace period the process is.
        let monitor = Arc::clone(&self.monitor);
        spawn_thread("magi-python-watchdog", move || {
            std::thread::sleep(STOP_GRACE);
            if process.is_alive() && lock(&process.runs).contains_key(&run) {
                monitor.warn("A Python script did not stop when asked; the Python engine was restarted.");
                process.kill();
            }
        });
    }

    pub(crate) fn unload(&self, program_id: u64) {
        let Some(process) = lock(&self.state).process.clone() else { return };
        if process.is_alive() && lock(&process.loaded).remove(&program_id) {
            let _ = process.send(&json!({ "t": "unload", "program": program_id }));
        }
    }

    /// Stops the Python process. It is started again on demand.
    pub fn shutdown(&self) {
        let mut state = lock(&self.state);
        if let Some(process) = state.process.take() {
            let _ = process.send(&json!({ "t": "shutdown" }));
            process.kill();
        }
        if matches!(state.status, PythonStatus::Running { .. }) {
            state.status = PythonStatus::Unknown;
        }
    }
}

impl Drop for PythonRuntime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn spawn_thread(name: &str, body: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new().name(name.to_owned()).spawn(body).expect("the OS refused to start a thread");
}

/// Reads Python's stdout until it closes, dispatching each message.
fn read_messages(
    process: &Process,
    stdout: impl std::io::Read,
    monitor: &Monitor,
    ready: &Sender<Result<String, String>>,
) {
    let route = |run: Option<u64>, call: Call, remove: bool| {
        let Some(run) = run else { return };
        let mut runs = lock(&process.runs);
        if let Some(sender) = runs.get(&run) {
            let _ = sender.send(call);
        }
        if remove {
            runs.remove(&run);
        }
    };

    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            if !line.trim().is_empty() {
                monitor.info(format!("[python] {line}"));
            }
            continue;
        };
        let run = message.get("run").and_then(Value::as_u64);
        let node = message.get("node").and_then(Value::as_u64);
        match message.get("t").and_then(Value::as_str) {
            Some("ready") => {
                let version = message.get("python").and_then(Value::as_str).unwrap_or("?").to_owned();
                let outcome = match message.get("protocol").and_then(Value::as_u64) {
                    Some(PROTOCOL) => Ok(version),
                    other => Err(format!("its runtime speaks protocol {other:?}, expected {PROTOCOL}")),
                };
                let _ = ready.send(outcome);
            }
            Some("call") => {
                let call = Call::Op {
                    id: message.get("id").and_then(Value::as_u64).unwrap_or(0),
                    op: message.get("op").and_then(Value::as_str).unwrap_or("").to_owned(),
                    args: message.get("args").cloned().unwrap_or(Value::Null),
                };
                route(run, call, false);
            }
            // Activity and log lines travel through the run's own queue, so they
            // reach that run's monitor and stay in order with its other messages.
            Some("enter") => {
                if let Some(node) = node {
                    route(run, Call::Enter(node), false);
                }
            }
            Some("log") => {
                let level = match message.get("level").and_then(Value::as_str) {
                    Some("error") => LogLevel::Error,
                    Some("warning") => LogLevel::Warning,
                    _ => LogLevel::Info,
                };
                let text = message.get("message").and_then(Value::as_str).unwrap_or("").to_owned();
                match run {
                    Some(_) => route(run, Call::Log { level, node, message: text }, false),
                    // Output that belongs to no run (e.g. printed while a script was imported).
                    None => monitor.log(level, node, text),
                }
            }
            Some("stop_all") => route(run, Call::StopAll, false),
            Some("done") => {
                let error = message.get("error").and_then(Value::as_str).map(str::to_owned);
                route(run, Call::Done { error, node }, true);
            }
            _ => {}
        }
    }

    // The pipe closed: Python is gone. Dropping the senders ends every worker.
    process.alive.store(false, Ordering::SeqCst);
    lock(&process.runs).clear();
    let _ = ready.send(Err("it exited immediately".to_owned()));
}

/// Serves one run: performs the device operations Python asks for.
fn serve_run(
    process: &Process,
    inbox: &Receiver<Call>,
    mut cx: RunCx,
    on_done: Box<dyn FnOnce(Result<(), EngineError>) + Send>,
) {
    let outcome = loop {
        match inbox.recv() {
            Ok(Call::Op { id, op, args }) => {
                let reply = match perform(&mut cx, &op, &args) {
                    Ok(value) => json!({ "t": "reply", "id": id, "ok": true, "value": value }),
                    Err(Halt::Stopped) => json!({ "t": "reply", "id": id, "ok": false, "stopped": true }),
                    Err(Halt::Failed(error)) => {
                        json!({ "t": "reply", "id": id, "ok": false, "error": error.message })
                    }
                };
                let _ = process.send(&reply);
            }
            Ok(Call::Enter(node)) => cx.monitor.enter(node),
            Ok(Call::Log { level, node, message }) => cx.monitor.log(level, node, message),
            Ok(Call::StopAll) => (cx.stop_everything)(),
            Ok(Call::Done { error: None, .. }) => break Ok(()),
            Ok(Call::Done { error: Some(message), node }) => break Err(EngineError { message, node }),
            Err(_) if cx.stop.is_stopped() => break Ok(()),
            Err(_) => break Err(EngineError::new("the Python engine stopped unexpectedly")),
        }
    };
    cx.release_all();
    on_done(outcome);
}

fn arg<T: DeserializeOwned>(args: &Value, key: &str) -> OpResult<T> {
    let value = args.get(key).cloned().unwrap_or(Value::Null);
    serde_json::from_value(value).map_err(|e| EngineError::new(format!("bad `{key}` argument: {e}")).into())
}

/// The device operations available to the Python runtime.
fn perform(cx: &mut RunCx, op: &str, args: &Value) -> OpResult<Value> {
    match op {
        "sleep" => {
            let ms = cx.jittered(arg(args, "ms")?, arg::<Option<f64>>(args, "jitter_ms")?.unwrap_or(0.0));
            cx.sleep_ms(ms)?;
            Ok(Value::Null)
        }
        "actions" => {
            let position = args.get("position").and_then(point_from);
            cx.perform_actions(&arg(args, "params")?, position)?;
            Ok(Value::Null)
        }
        "play_recording" => {
            cx.play_recording(&arg(args, "params")?)?;
            Ok(Value::Null)
        }
        "find_image" => Ok(Value::Object(cx.find_image(&arg(args, "params")?)?.to_json())),
        "find_color" => Ok(Value::Object(cx.find_color(&arg(args, "params")?)?.to_json())),
        "send" => {
            cx.send(&arg::<InputEvent>(args, "event")?)?;
            Ok(Value::Null)
        }
        "autoclick" => {
            let params: AutoClickParams = arg(args, "params")?;
            cx.autoclick(&params.clicker)?;
            Ok(Value::Null)
        }
        "focus_window" => {
            let found = cx.focus_window(&arg(args, "params")?)?;
            Ok(found.map_or(
                Value::Null,
                |info| json!({ "title": info.title, "process": info.process, "kind": info.kind.id() }),
            ))
        }
        "move" => {
            let duration = millis(arg::<Option<f64>>(args, "duration_ms")?.unwrap_or(0.0));
            let relative = arg::<Option<bool>>(args, "relative")?.unwrap_or(false);
            cx.move_pointer(arg(args, "x")?, arg(args, "y")?, relative, duration)?;
            Ok(Value::Null)
        }
        "cursor" => {
            let position = cx.host.cursor().map_err(EngineError::from)?;
            Ok(json!(position))
        }
        "resolve" => {
            let program = Arc::clone(&cx.program);
            let node = program.node(arg(args, "node")?)?;
            Ok(Value::Object(cx.params_of(node)))
        }
        "var_get" => Ok(cx.vars.get(&arg::<String>(args, "name")?).unwrap_or(Value::Null)),
        "var_set" => {
            cx.vars.set(&arg::<String>(args, "name")?, args.get("value").cloned().unwrap_or(Value::Null));
            Ok(Value::Null)
        }
        "vars" => Ok(Value::Object(cx.vars.snapshot())),
        "set_variable" => cx.set_variable(&arg(args, "params")?, args.get("value").cloned()),
        "counter" => {
            let (count, reached) = cx.count(&arg(args, "params")?)?;
            Ok(json!({ "count": count, "reached": reached }))
        }
        "rate" => {
            let params: RateParams = arg(args, "params")?;
            let rate_ms = args.get("rate").and_then(as_number).unwrap_or(params.rate_ms);
            cx.rate(&params, rate_ms)?;
            Ok(Value::Null)
        }
        "math" => {
            let params: MathParams = arg(args, "params")?;
            let a = args.get("a").and_then(as_number).unwrap_or(params.a);
            let b = args.get("b").and_then(as_number).unwrap_or(params.b);
            let random = cx.random();
            Ok(params.operation.apply(a, b, random).map_or(Value::Null, |n| json!(n)))
        }
        "compare" => {
            let params: CompareParams = arg(args, "params")?;
            let a = args.get("a").cloned().unwrap_or(Value::Null);
            let b = args.get("b").cloned().unwrap_or_else(|| parse_literal(&params.b));
            Ok(json!(params.operation.apply(&a, &b)))
        }
        "text" => {
            let params: TextParams = arg(args, "params")?;
            let a = args.get("a").cloned().unwrap_or_else(|| Value::String(params.a.clone()));
            let b = args.get("b").cloned().unwrap_or_else(|| Value::String(params.b.clone()));
            Ok(params.operation.apply(&a, &b, &params.replacement))
        }
        "clipboard" => Ok(Value::String(cx.clipboard()?)),
        "sound_level" => Ok(Value::Object(cx.sound_level(&arg(args, "params")?)?)),
        "ai_classify" => Ok(Value::Object(cx.ai_classify(&arg(args, "params")?)?)),
        "ai_watch" => {
            let returning: Option<bool> = arg(args, "returning")?;
            let step = cx.ai_watch(&arg(args, "params")?, arg(args, "node")?, returning.unwrap_or(false))?;
            Ok(serde_json::to_value(step).map_err(|e| EngineError::new(e.to_string()))?)
        }
        "sound_match" => Ok(Value::Object(cx.sound_match(&arg(args, "params")?, arg(args, "node")?)?)),
        "autotype" => {
            let wired: Option<bool> = arg(args, "wired")?;
            let value = wired.unwrap_or(false).then(|| args.get("value").cloned().unwrap_or(Value::Null));
            Ok(json!(cx.autotype(&arg(args, "params")?, value, arg(args, "node")?)?))
        }
        "read_text" => Ok(Value::Object(cx.read_text(&arg(args, "params")?)?)),
        "notify" => {
            let node: Option<NodeId> = arg(args, "node")?;
            cx.notify(&arg(args, "params")?, args.get("value").cloned(), node.unwrap_or(0))?;
            Ok(Value::Null)
        }
        other => Err(EngineError::new(format!("the Python runtime asked for an unknown operation `{other}`")).into()),
    }
}

/// The interpreters to try, best first. Each entry is a program and its leading arguments.
fn candidates(config: &PythonConfig) -> Vec<Vec<OsString>> {
    if let Some(chosen) = &config.interpreter {
        return vec![vec![chosen.clone().into_os_string()]];
    }
    let mut list: Vec<Vec<OsString>> = Vec::new();
    let bundled = if cfg!(windows) {
        config.app_dir.join("python").join("python.exe")
    } else {
        config.app_dir.join("python").join("bin").join("python3")
    };
    if bundled.is_file() {
        list.push(vec![bundled.into_os_string()]);
    }
    // The one downloaded from the AI settings page, unless it is waiting to be removed.
    let downloaded = config.data_dir.join("components").join("python");
    if downloaded.join("python.exe").is_file() && !downloaded.join(".remove").exists() {
        list.push(vec![downloaded.join("python.exe").into_os_string()]);
    }
    let system: &[&[&str]] =
        if cfg!(windows) { &[&["py", "-3"], &["python"], &["python3"]] } else { &[&["python3"], &["python"]] };
    list.extend(system.iter().map(|command| command.iter().map(OsString::from).collect()));
    list
}

/// The interpreter the engine would use with `config`, without starting the
/// runtime: its full path and version. "Automatic" resolves to a real file
/// this way, so other features (installing packages) use the same Python as
/// macros. Starts Python briefly: call it off the UI thread.
pub fn resolve_interpreter(config: &PythonConfig) -> Option<(PathBuf, String)> {
    const ASK: &str = "import sys; print(sys.executable); print('%d.%d.%d' % sys.version_info[:3])";
    for candidate in candidates(config) {
        let mut command = Command::new(&candidate[0]);
        command.args(&candidate[1..]).args(["-c", ASK]).env("PYTHONDONTWRITEBYTECODE", "1").stdin(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt as _;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let Ok(output) = command.output() else { continue };
        if !output.status.success() {
            continue;
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
        let (Some(path), Some(version)) = (lines.next(), lines.next()) else { continue };
        let parts: Vec<u32> = version.split('.').take(2).map(|n| n.parse().unwrap_or(0)).collect();
        if parts.len() == 2 && (parts[0], parts[1]) >= (3, 8) {
            return Some((PathBuf::from(path), version.to_owned()));
        }
    }
    None
}

/// Where the runtime sources live, unpacking the built-in copy if needed.
fn runtime_dir(data_dir: &Path) -> std::io::Result<PathBuf> {
    if let Some(dir) = std::env::var_os(RUNTIME_OVERRIDE_ENV) {
        return Ok(PathBuf::from(dir));
    }
    let mut files = Vec::new();
    collect_files(&RUNTIME_SOURCES, &mut files);
    files.sort_by(|a, b| a.0.cmp(b.0));

    // The folder is named after its contents, so a new app version unpacks a
    // fresh copy and never runs against stale sources.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for (path, contents) in &files {
        for &byte in path.to_string_lossy().as_bytes().iter().chain(*contents) {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    let parent = data_dir.join("runtime");
    let dir = parent.join(format!("python-{hash:016x}"));
    let marker = dir.join(".complete");
    if marker.is_file() {
        return Ok(dir);
    }

    for (path, contents) in &files {
        let target = dir.join(path);
        if let Some(folder) = target.parent() {
            std::fs::create_dir_all(folder)?;
        }
        std::fs::write(target, contents)?;
    }
    std::fs::write(&marker, b"")?;

    // Tidy up copies left by other versions; failing to is harmless.
    if let Ok(entries) = std::fs::read_dir(&parent) {
        for entry in entries.flatten() {
            if entry.path() != dir && entry.file_name().to_string_lossy().starts_with("python-") {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
    Ok(dir)
}

/// Every runtime source file. Tests and caches are left out.
fn collect_files<'a>(dir: &'a Dir<'a>, out: &mut Vec<(&'a Path, &'a [u8])>) {
    for entry in dir.entries() {
        match entry {
            DirEntry::Dir(sub) => {
                let name = sub.path().file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name != "tests" && name != "__pycache__" {
                    collect_files(sub, out);
                }
            }
            DirEntry::File(file) => {
                if file.path().extension().is_some_and(|ext| ext == "py") {
                    out.push((file.path(), file.contents()));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_runtime_is_compiled_in() {
        let mut files = Vec::new();
        collect_files(&RUNTIME_SOURCES, &mut files);
        let names: Vec<String> = files.iter().map(|(path, _)| path.to_string_lossy().replace('\\', "/")).collect();
        for expected in ["magi_host.py", "magi/host.py", "magi/interpreter.py", "magi/nodes/flow.py", "magi/api.py"] {
            assert!(names.iter().any(|name| name == expected), "{expected} is missing from the build");
        }
        assert!(names.iter().all(|name| !name.starts_with("tests/")), "tests must not be shipped");
    }

    #[test]
    fn an_explicit_interpreter_is_the_only_candidate() {
        let config = PythonConfig { interpreter: Some("/opt/py/bin/python".into()), ..PythonConfig::default() };
        assert_eq!(candidates(&config), vec![vec![OsString::from("/opt/py/bin/python")]]);
        assert!(candidates(&PythonConfig::default()).len() >= 2);
    }

    #[test]
    fn a_python_beside_the_app_comes_first_then_a_downloaded_one() {
        let root = std::env::temp_dir().join(format!("magi-candidates-{}", std::process::id()));
        let (app_dir, data_dir) = (root.join("app"), root.join("data"));
        let beside = if cfg!(windows) {
            app_dir.join("python").join("python.exe")
        } else {
            app_dir.join("python").join("bin").join("python3")
        };
        let downloaded = data_dir.join("components").join("python");
        std::fs::create_dir_all(beside.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&downloaded).unwrap();
        std::fs::write(&beside, b"").unwrap();
        std::fs::write(downloaded.join("python.exe"), b"").unwrap();
        let config = PythonConfig { interpreter: None, app_dir, data_dir };
        let list = candidates(&config);
        assert_eq!(list[0], vec![beside.into_os_string()]);
        assert_eq!(list[1], vec![downloaded.join("python.exe").into_os_string()]);
        // Marked for removal: skipped.
        std::fs::write(downloaded.join(".remove"), b"").unwrap();
        assert!(!candidates(&config).contains(&vec![downloaded.join("python.exe").into_os_string()]));
        let _ = std::fs::remove_dir_all(root);
    }
}

//! What a running macro reports back: which nodes are executing, and a log.
//!
//! Engines write here from their own threads; the UI reads snapshots whenever
//! it repaints. Nothing is queued without bound, so a macro spinning at full
//! speed cannot grow memory even if nobody is looking.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};

use magi_core::graph::NodeId;
use serde_json::{Map, Value};

const MAX_LOG_LINES: usize = 2000;
/// Node activity alone wakes the UI at most this often.
const WAKE_INTERVAL: Duration = Duration::from_millis(33);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug)]
pub struct LogEntry {
    /// Monotonic sequence number; lets readers ask for "everything after N".
    pub seq: u64,
    pub time: SystemTime,
    pub level: LogLevel,
    /// The node the message is about, if any.
    pub node: Option<NodeId>,
    pub message: String,
}

#[derive(Clone, Copy, Debug)]
pub struct Activity {
    /// How many times the node has executed.
    pub count: u64,
    /// When it last started executing.
    pub last: Instant,
}

type Waker = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
pub struct Monitor {
    state: Mutex<State>,
    /// Clicks made by Auto Click blocks. Counted outside the lock: at a
    /// thousand clicks a second, every bit of contention shows.
    clicks: AtomicU64,
}

/// A run that came to an end, as reported to whoever started it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FinishedRun {
    /// The block the run started at.
    pub node: NodeId,
    /// It ended on its own – not because someone pressed stop.
    pub by_itself: bool,
    pub failed: bool,
}

#[derive(Default)]
struct State {
    activity: HashMap<NodeId, Activity>,
    variables: Map<String, Value>,
    finished: Vec<FinishedRun>,
    logs: VecDeque<LogEntry>,
    next_seq: u64,
    waker: Option<Waker>,
    last_wake: Option<Instant>,
}

impl Monitor {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Registers a callback invoked (from engine threads) when there is
    /// something new to show. The UI uses it to request a repaint.
    pub fn set_waker(&self, waker: impl Fn() + Send + Sync + 'static) {
        self.lock().waker = Some(Arc::new(waker));
    }

    /// Wakes the UI now.
    pub fn wake(&self) {
        let waker = {
            let mut state = self.lock();
            state.last_wake = Some(Instant::now());
            state.waker.clone()
        };
        if let Some(waker) = waker {
            waker();
        }
    }

    /// Wakes the UI unless it was woken very recently.
    fn wake_throttled(&self) {
        let now = Instant::now();
        let waker = {
            let mut state = self.lock();
            let due = state.last_wake.is_none_or(|last| now.duration_since(last) >= WAKE_INTERVAL);
            if !due {
                return;
            }
            state.last_wake = Some(now);
            state.waker.clone()
        };
        if let Some(waker) = waker {
            waker();
        }
    }

    /// The current values of the macro's variables.
    pub fn set_variables(&self, values: Map<String, Value>) {
        self.lock().variables = values;
        self.wake_throttled();
    }

    pub fn variables(&self) -> Map<String, Value> {
        self.lock().variables.clone()
    }

    pub fn run_finished(&self, run: FinishedRun) {
        {
            let mut state = self.lock();
            // Nobody may be collecting these; keep only the most recent.
            if state.finished.len() >= 64 {
                state.finished.remove(0);
            }
            state.finished.push(run);
        }
        self.wake();
    }

    /// Runs that ended since the last call.
    pub fn take_finished(&self) -> Vec<FinishedRun> {
        std::mem::take(&mut self.lock().finished)
    }

    /// Records that a node started executing.
    pub fn enter(&self, node: NodeId) {
        let now = Instant::now();
        let waker = {
            let mut state = self.lock();
            let entry = state.activity.entry(node).or_insert(Activity { count: 0, last: now });
            entry.count += 1;
            entry.last = now;
            let due = state.last_wake.is_none_or(|last| now.duration_since(last) >= WAKE_INTERVAL);
            if due {
                state.last_wake = Some(now);
                state.waker.clone()
            } else {
                None
            }
        };
        if let Some(waker) = waker {
            waker();
        }
    }

    pub fn log(&self, level: LogLevel, node: Option<NodeId>, message: impl Into<String>) {
        {
            let mut state = self.lock();
            let seq = state.next_seq;
            state.next_seq += 1;
            if state.logs.len() == MAX_LOG_LINES {
                state.logs.pop_front();
            }
            state.logs.push_back(LogEntry { seq, time: SystemTime::now(), level, node, message: message.into() });
        }
        self.wake();
    }

    pub fn info(&self, message: impl Into<String>) {
        self.log(LogLevel::Info, None, message);
    }

    pub fn warn(&self, message: impl Into<String>) {
        self.log(LogLevel::Warning, None, message);
    }

    pub fn error(&self, message: impl Into<String>) {
        self.log(LogLevel::Error, None, message);
    }

    /// A copy of every node's activity.
    pub fn activity(&self) -> HashMap<NodeId, Activity> {
        self.lock().activity.clone()
    }

    pub fn activity_of(&self, node: NodeId) -> Option<Activity> {
        self.lock().activity.get(&node).copied()
    }

    /// Log lines with a sequence number of `from` or higher, oldest first.
    pub fn logs_from(&self, from: u64) -> Vec<LogEntry> {
        self.lock().logs.iter().filter(|entry| entry.seq >= from).cloned().collect()
    }

    pub fn logs(&self) -> Vec<LogEntry> {
        self.logs_from(0)
    }

    pub fn clear_logs(&self) {
        self.lock().logs.clear();
    }

    pub fn clear_activity(&self) {
        self.lock().activity.clear();
        self.clicks.store(0, Ordering::Relaxed);
    }

    pub fn add_clicks(&self, count: u64) {
        self.clicks.fetch_add(count, Ordering::Relaxed);
    }

    /// Clicks made since the activity was last cleared.
    pub fn clicks(&self) -> u64 {
        self.clicks.load(Ordering::Relaxed)
    }
}

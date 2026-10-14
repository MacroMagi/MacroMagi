//! # magi-platform
//!
//! Everything in MacroMagi that touches the operating system lives here, and
//! nowhere else:
//!
//! * [`SystemHost`] – the real [`Host`]: sends input, reads the pointer, captures the screen.
//! * [`InputHub`] – a stream of everything the user does on keyboard, mouse and gamepad,
//!   used for hotkeys and recording.
//! * [`capture`] – monitor enumeration and screenshots.
//!
//! Platform status: Windows is the primary, tested target. macOS and Linux
//! (X11) are implemented through cross-platform crates and need the usual
//! permissions (Accessibility / Screen Recording on macOS).

pub mod capture;
mod gamepad;
mod inject;
pub mod keymap;
mod listen;

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use magi_core::geom::{Point, Rect};
use magi_core::host::{Frame, Host, HostError};
use magi_core::input::InputEvent;

pub use inject::{INJECTION_MARKER, Injector};

/// Call once, first thing in `main`.
///
/// On Windows this opts the process into per-monitor DPI awareness, so that
/// pointer positions, screen captures and injected coordinates all use the
/// same unit: physical pixels. Without it, a display scaled to 150% would
/// make clicks land in the wrong place.
pub fn init() {
    #[cfg(windows)]
    // SAFETY: a plain Win32 call with a constant argument. Failure (e.g. the
    // awareness was already set by a manifest) is harmless and ignored.
    unsafe {
        use windows_sys::Win32::UI::HiDpi::{
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
        };
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

/// Connects standard output to the console the program was started from.
///
/// Release builds on Windows are GUI programs and start without a console, so
/// command-line output would otherwise vanish. Does nothing elsewhere.
pub fn attach_parent_console() {
    #[cfg(windows)]
    // SAFETY: a plain Win32 call. It fails harmlessly when there is no parent
    // console (started from Explorer) or one is already attached.
    unsafe {
        use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

/// The real machine.
#[derive(Default)]
pub struct SystemHost {
    injector: Injector,
}

impl SystemHost {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Host for SystemHost {
    fn send(&self, event: &InputEvent) -> Result<(), HostError> {
        self.injector.send(event)
    }

    fn cursor(&self) -> Result<Point, HostError> {
        self.injector.cursor()
    }

    fn capture(&self, region: Option<Rect>) -> Result<Frame, HostError> {
        capture::capture(region)
    }
}

/// One input event seen by the [`InputHub`].
#[derive(Clone, Debug)]
pub struct Observed {
    pub event: InputEvent,
    pub at: Instant,
    /// True if MacroMagi itself injected this event (detected on Windows only).
    /// Hotkeys and recordings ignore synthetic input so a macro cannot trigger
    /// or record itself.
    pub synthetic: bool,
}

/// Fans the system-wide input stream out to any number of subscribers.
///
/// Creating a hub starts two background threads (keyboard/mouse and gamepad)
/// that live for the rest of the process.
#[derive(Clone)]
pub struct InputHub {
    shared: Arc<Shared>,
}

#[derive(Default)]
struct Shared {
    subscribers: Mutex<Vec<Sender<Observed>>>,
    /// Why keyboard/mouse listening is not working, if it is not.
    listener_error: Mutex<Option<String>>,
    gamepad_error: Mutex<Option<String>>,
}

impl Shared {
    fn publish(&self, observed: Observed) {
        let mut subscribers = self.subscribers.lock().unwrap_or_else(|p| p.into_inner());
        subscribers.retain(|subscriber| subscriber.send(observed.clone()).is_ok());
    }
}

impl InputHub {
    pub fn start() -> Self {
        let shared = Arc::new(Shared::default());

        let for_listener = Arc::clone(&shared);
        spawn("magi-listen", move || {
            let sink_target = Arc::clone(&for_listener);
            if let Err(error) = listen::run(move |observed| sink_target.publish(observed)) {
                *for_listener.listener_error.lock().unwrap_or_else(|p| p.into_inner()) = Some(error);
            }
        });

        let for_gamepad = Arc::clone(&shared);
        spawn("magi-gamepad", move || {
            if let Err(error) = gamepad::run(|observed| for_gamepad.publish(observed)) {
                *for_gamepad.gamepad_error.lock().unwrap_or_else(|p| p.into_inner()) = Some(error);
            }
        });

        Self { shared }
    }

    /// A new receiver that gets every event from now on. Dropping it unsubscribes.
    pub fn subscribe(&self) -> Receiver<Observed> {
        let (sender, receiver) = mpsc::channel();
        self.shared.subscribers.lock().unwrap_or_else(|p| p.into_inner()).push(sender);
        receiver
    }

    /// Why hotkeys and recording are not working, if they are not.
    pub fn listener_error(&self) -> Option<String> {
        self.shared.listener_error.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    pub fn gamepad_error(&self) -> Option<String> {
        self.shared.gamepad_error.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}

fn spawn(name: &str, body: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new().name(name.into()).spawn(body).expect("the OS refused to start a thread");
}

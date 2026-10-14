//! Sending input to the operating system.
//!
//! All injection happens on one dedicated thread that owns the OS connection.
//! Callers talk to it through [`Injector`], which is cheap to share. This keeps
//! every event strictly ordered and sidesteps the question of whether the
//! underlying OS handles may move between threads.

use std::sync::Mutex;
use std::sync::mpsc::{self, Sender, SyncSender};

use enigo::{Axis, Coordinate, Direction, Enigo, Keyboard as _, Mouse as _, Settings};
use magi_core::geom::Point;
use magi_core::host::HostError;
use magi_core::input::{InputEvent, Key, MouseButton};

use crate::keymap;

/// Tag attached to every event MacroMagi injects on Windows (`dwExtraInfo`),
/// so the listener can tell our own input from the user's. Spells "MAGI".
pub const INJECTION_MARKER: usize = 0x4D41_4749;

enum Request {
    Send(InputEvent, SyncSender<Result<(), HostError>>),
    Cursor(SyncSender<Result<Point, HostError>>),
}

pub struct Injector {
    requests: Mutex<Sender<Request>>,
}

impl Default for Injector {
    fn default() -> Self {
        Self::new()
    }
}

impl Injector {
    pub fn new() -> Self {
        let (requests, inbox) = mpsc::channel::<Request>();
        std::thread::Builder::new()
            .name("magi-inject".into())
            .spawn(move || {
                let mut backend = Backend::connect();
                for request in inbox {
                    match request {
                        Request::Send(event, reply) => {
                            let _ = reply.send(backend.send(&event));
                        }
                        Request::Cursor(reply) => {
                            let _ = reply.send(backend.cursor());
                        }
                    }
                }
            })
            .expect("the OS refused to start a thread");
        Self { requests: Mutex::new(requests) }
    }

    fn ask<T>(&self, make: impl FnOnce(SyncSender<Result<T, HostError>>) -> Request) -> Result<T, HostError> {
        let (reply, answer) = mpsc::sync_channel(1);
        let gone = || HostError::new("the input thread has stopped");
        self.requests.lock().unwrap_or_else(|p| p.into_inner()).send(make(reply)).map_err(|_| gone())?;
        answer.recv().map_err(|_| gone())?
    }

    pub fn send(&self, event: &InputEvent) -> Result<(), HostError> {
        self.ask(|reply| Request::Send(event.clone(), reply))
    }

    pub fn cursor(&self) -> Result<Point, HostError> {
        self.ask(Request::Cursor)
    }
}

/// The OS connection, or the reason there is none.
struct Backend {
    enigo: Result<Enigo, HostError>,
}

impl Backend {
    fn connect() -> Self {
        let settings = Settings {
            windows_dw_extra_info: Some(INJECTION_MARKER),
            // If MacroMagi exits while a macro holds a key, let it go.
            release_keys_when_dropped: true,
            ..Settings::default()
        };
        let enigo = Enigo::new(&settings).map_err(|e| {
            HostError::new(format!(
                "cannot send input on this system: {e}. On macOS, grant MacroMagi the Accessibility \
                 permission; on Linux, an X11 session is required."
            ))
        });
        Self { enigo }
    }

    fn cursor(&mut self) -> Result<Point, HostError> {
        let enigo = self.enigo.as_mut().map_err(|e| e.clone())?;
        let (x, y) = enigo.location().map_err(error)?;
        Ok(Point::new(x, y))
    }

    fn send(&mut self, event: &InputEvent) -> Result<(), HostError> {
        let enigo = self.enigo.as_mut().map_err(|e| e.clone())?;
        match event {
            InputEvent::KeyDown { key } => key_event(enigo, *key, Direction::Press),
            InputEvent::KeyUp { key } => key_event(enigo, *key, Direction::Release),
            InputEvent::ButtonDown { button } => enigo.button(mouse_button(*button), Direction::Press).map_err(error),
            InputEvent::ButtonUp { button } => enigo.button(mouse_button(*button), Direction::Release).map_err(error),
            InputEvent::MoveTo { x, y } => enigo.move_mouse(*x, *y, Coordinate::Abs).map_err(error),
            InputEvent::MoveBy { dx, dy } => enigo.move_mouse(*dx, *dy, Coordinate::Rel).map_err(error),
            InputEvent::Scroll { dx, dy } => {
                // enigo counts "down" as positive; MacroMagi counts "up" as positive.
                if *dy != 0 {
                    enigo.scroll(-*dy, Axis::Vertical).map_err(error)?;
                }
                if *dx != 0 {
                    enigo.scroll(*dx, Axis::Horizontal).map_err(error)?;
                }
                Ok(())
            }
            InputEvent::Text { text } => enigo.text(text).map_err(error),
            InputEvent::PadButtonDown { .. } | InputEvent::PadButtonUp { .. } | InputEvent::PadAxis { .. } => {
                Err(HostError::new(
                    "sending gamepad input needs a virtual gamepad device, which MacroMagi does not provide yet",
                ))
            }
        }
    }
}

fn error(e: enigo::InputError) -> HostError {
    HostError::new(format!("the system rejected the input: {e}"))
}

fn key_event(enigo: &mut Enigo, key: Key, direction: Direction) -> Result<(), HostError> {
    let code = keymap::native_code(key)
        .ok_or_else(|| HostError::new(format!("the {} key does not exist on this system", key.label())))?;
    // Raw keys carry whatever code the listener saw. On Windows that is a
    // virtual-key code like any other; elsewhere it is a hardware keycode.
    if matches!(key, Key::Raw(_)) && !cfg!(target_os = "windows") {
        let code = u16::try_from(code).map_err(|_| HostError::new("raw key code out of range"))?;
        return enigo.raw(code, direction).map_err(error);
    }
    enigo.key(enigo::Key::Other(code), direction).map_err(error)
}

fn mouse_button(button: MouseButton) -> enigo::Button {
    match button {
        MouseButton::Left => enigo::Button::Left,
        MouseButton::Right => enigo::Button::Right,
        MouseButton::Middle => enigo::Button::Middle,
        MouseButton::Back => enigo::Button::Back,
        MouseButton::Forward => enigo::Button::Forward,
    }
}

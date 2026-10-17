//! Sending input to the operating system.
//!
//! All injection happens on one dedicated thread that owns the OS connection.
//! Callers talk to it through [`Injector`], which is cheap to share. This keeps
//! every event strictly ordered and sidesteps the question of whether the
//! underlying OS handles may move between threads.

use std::sync::Mutex;
use std::sync::mpsc::{self, Sender, SyncSender};

#[cfg(not(windows))]
use enigo::Coordinate;
use enigo::{Axis, Direction, Enigo, Keyboard as _, Mouse as _, Settings};
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
            #[cfg(windows)]
            InputEvent::MoveTo { x, y } => windows_mouse::move_to(*x, *y),
            #[cfg(windows)]
            InputEvent::MoveBy { dx, dy } => windows_mouse::move_by(*dx, *dy),
            #[cfg(not(windows))]
            InputEvent::MoveTo { x, y } => enigo.move_mouse(*x, *y, Coordinate::Abs).map_err(error),
            #[cfg(not(windows))]
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
    // Many games read the keyboard by scan code and ignore keys sent as
    // virtual keys, so keys go out as scan codes – as a real keyboard sends
    // them. Extended keys (arrows, the right-hand Ctrl and Alt…) keep the
    // virtual-key path, which marks them correctly.
    #[cfg(windows)]
    if let Some(scan) = windows_scan_code(code) {
        return enigo.raw(scan, direction).map_err(error);
    }
    enigo.key(enigo::Key::Other(code), direction).map_err(error)
}

/// The scan code of a virtual key that is not an extended key, in the current layout.
#[cfg(windows)]
fn windows_scan_code(virtual_key: u32) -> Option<u16> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MAPVK_VK_TO_VSC_EX, MapVirtualKeyW};
    // SAFETY: a plain lookup with no pointers.
    let scan = unsafe { MapVirtualKeyW(virtual_key, MAPVK_VK_TO_VSC_EX) };
    (scan != 0 && scan <= 0xFF).then_some(scan as u16)
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

/// Pointer movement on Windows, sent directly rather than through enigo.
///
/// * Absolute moves are mapped over the whole virtual desktop, so positions on
///   any monitor work (enigo maps them over the primary monitor only).
/// * Relative moves are sent as genuine relative motion. enigo turns them into
///   absolute moves to avoid pointer acceleration, but a game with a locked
///   pointer only reacts to relative motion – that is what turns its camera.
#[cfg(windows)]
mod windows_mouse {
    use magi_core::host::HostError;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_MOVE, MOUSEEVENTF_VIRTUALDESK, MOUSEINPUT,
        SendInput,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    };

    use super::INJECTION_MARKER;

    fn send(dx: i32, dy: i32, flags: u32) -> Result<(), HostError> {
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT { dx, dy, mouseData: 0, dwFlags: flags, time: 0, dwExtraInfo: INJECTION_MARKER },
            },
        };
        // SAFETY: one fully initialised INPUT, with its size.
        let sent = unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) };
        if sent == 1 {
            Ok(())
        } else {
            Err(HostError::new(format!("the system rejected the pointer move ({})", std::io::Error::last_os_error())))
        }
    }

    /// Windows maps an absolute coordinate `n` (0–65535) to pixel
    /// `n × size / 65536`, rounded down; this is the smallest `n` that lands on `pixel`.
    pub(super) fn normalize(pixel: i32, origin: i32, size: i32) -> i32 {
        let offset = i64::from((pixel - origin).clamp(0, size.max(1) - 1));
        ((offset * 65536 + i64::from(size.max(1)) - 1) / i64::from(size.max(1))) as i32
    }

    pub(super) fn move_to(x: i32, y: i32) -> Result<(), HostError> {
        // SAFETY: plain metric queries.
        let (vx, vy, vw, vh) = unsafe {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN),
                GetSystemMetrics(SM_CYVIRTUALSCREEN),
            )
        };
        send(
            normalize(x, vx, vw),
            normalize(y, vy, vh),
            MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
        )
    }

    pub(super) fn move_by(dx: i32, dy: i32) -> Result<(), HostError> {
        send(dx, dy, MOUSEEVENTF_MOVE)
    }

    #[cfg(test)]
    mod tests {
        use super::normalize;

        #[test]
        fn normalised_coordinates_land_on_the_requested_pixel() {
            for size in [1920, 2560, 3840 + 1920, 1366] {
                for pixel in [0, 1, size / 2, size - 2, size - 1] {
                    let n = i64::from(normalize(pixel, 0, size));
                    assert_eq!((n * i64::from(size)) >> 16, i64::from(pixel), "size {size}, pixel {pixel}");
                }
            }
            assert_eq!(normalize(-1920, -1920, 3840), 0, "monitors left of the primary have negative x");
        }
    }
}

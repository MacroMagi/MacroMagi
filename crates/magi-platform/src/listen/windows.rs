//! Windows listener: low-level keyboard and mouse hooks.
//!
//! Written against the Win32 API directly rather than through a wrapper crate
//! for two reasons that matter to a tool which listens the whole time it is open:
//!
//! * it never asks Windows to translate key presses into characters, which is
//!   what breaks dead keys (´ ` ^ ~) for international layouts in other hooks;
//! * it can read each event's `dwExtraInfo`, so input that MacroMagi itself
//!   injected is recognised and never mistaken for the user's.

use std::cell::{Cell, RefCell};
use std::ptr;
use std::time::Instant;

use magi_core::input::{InputEvent, MouseButton};
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_EXTENDED, MSG, MSLLHOOKSTRUCT, SetWindowsHookExW,
    UnhookWindowsHookEx, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_RBUTTONDOWN, WM_RBUTTONUP,
    WM_SYSKEYDOWN, WM_SYSKEYUP, WM_XBUTTONDOWN, WM_XBUTTONUP, XBUTTON1,
};

use crate::Observed;
use crate::inject::INJECTION_MARKER;
use crate::keymap;

/// Placeholder virtual key that carries injected Unicode text, not a real key.
const VK_PACKET: u32 = 0xE7;
/// One notch of a standard mouse wheel.
const WHEEL_DELTA: i32 = 120;

type Sink = Box<dyn FnMut(Observed)>;

thread_local! {
    // Low-level hooks are called on the thread that installed them, so the
    // sink can live in thread-local storage and needs no locking.
    static SINK: RefCell<Option<Sink>> = const { RefCell::new(None) };
    // High-resolution wheels report fractions of a notch; these carry the remainder.
    static WHEEL_REMAINDER: Cell<(i32, i32)> = const { Cell::new((0, 0)) };
}

fn emit(event: InputEvent, extra_info: usize) {
    let observed = Observed { event, at: Instant::now(), synthetic: extra_info == INJECTION_MARKER };
    SINK.with_borrow_mut(|sink| {
        if let Some(sink) = sink {
            sink(observed);
        }
    });
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        // SAFETY: for WH_KEYBOARD_LL with HC_ACTION, lparam points to a KBDLLHOOKSTRUCT
        // that is valid for the duration of this call.
        let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        if info.vkCode != VK_PACKET {
            let key = keymap::key_from_vk(info.vkCode, info.flags & LLKHF_EXTENDED != 0);
            match wparam as u32 {
                WM_KEYDOWN | WM_SYSKEYDOWN => emit(InputEvent::KeyDown { key }, info.dwExtraInfo),
                WM_KEYUP | WM_SYSKEYUP => emit(InputEvent::KeyUp { key }, info.dwExtraInfo),
                _ => {}
            }
        }
    }
    // SAFETY: forwarding the unmodified arguments is always valid.
    unsafe { CallNextHookEx(ptr::null_mut(), code, wparam, lparam) }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        // SAFETY: for WH_MOUSE_LL with HC_ACTION, lparam points to an MSLLHOOKSTRUCT
        // that is valid for the duration of this call.
        let info = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
        let high_word = (info.mouseData >> 16) as u16;
        let x_button = if high_word == XBUTTON1 { MouseButton::Back } else { MouseButton::Forward };
        let event = match wparam as u32 {
            WM_MOUSEMOVE => Some(InputEvent::MoveTo { x: info.pt.x, y: info.pt.y }),
            WM_LBUTTONDOWN => Some(InputEvent::ButtonDown { button: MouseButton::Left }),
            WM_LBUTTONUP => Some(InputEvent::ButtonUp { button: MouseButton::Left }),
            WM_RBUTTONDOWN => Some(InputEvent::ButtonDown { button: MouseButton::Right }),
            WM_RBUTTONUP => Some(InputEvent::ButtonUp { button: MouseButton::Right }),
            WM_MBUTTONDOWN => Some(InputEvent::ButtonDown { button: MouseButton::Middle }),
            WM_MBUTTONUP => Some(InputEvent::ButtonUp { button: MouseButton::Middle }),
            WM_XBUTTONDOWN => Some(InputEvent::ButtonDown { button: x_button }),
            WM_XBUTTONUP => Some(InputEvent::ButtonUp { button: x_button }),
            WM_MOUSEWHEEL => wheel(i32::from(high_word as i16), true),
            WM_MOUSEHWHEEL => wheel(i32::from(high_word as i16), false),
            _ => None,
        };
        if let Some(event) = event {
            emit(event, info.dwExtraInfo);
        }
    }
    // SAFETY: forwarding the unmodified arguments is always valid.
    unsafe { CallNextHookEx(ptr::null_mut(), code, wparam, lparam) }
}

/// Converts a wheel delta into whole notches, keeping the remainder for next time.
fn wheel(delta: i32, vertical: bool) -> Option<InputEvent> {
    let (mut dx, mut dy) = WHEEL_REMAINDER.get();
    let total = if vertical { &mut dy } else { &mut dx };
    *total += delta;
    let notches = *total / WHEEL_DELTA;
    *total -= notches * WHEEL_DELTA;
    WHEEL_REMAINDER.set((dx, dy));
    match (notches, vertical) {
        (0, _) => None,
        (n, true) => Some(InputEvent::Scroll { dx: 0, dy: n }),
        (n, false) => Some(InputEvent::Scroll { dx: n, dy: 0 }),
    }
}

/// Installs the hooks and pumps messages until the thread's message loop ends.
/// The sink runs inside the hook, so it must return quickly: the system stalls
/// all keyboard and mouse input while it is running.
pub(crate) fn run(sink: impl FnMut(Observed) + 'static) -> Result<(), String> {
    SINK.set(Some(Box::new(sink)));
    // SAFETY: plain Win32 calls. The hook procedures have the required
    // signature, and both hooks are removed before this function returns.
    unsafe {
        let module = GetModuleHandleW(ptr::null());
        let keyboard = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), module, 0);
        if keyboard.is_null() {
            return Err(format!("could not install the keyboard hook ({})", std::io::Error::last_os_error()));
        }
        let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0);
        if mouse.is_null() {
            let error = std::io::Error::last_os_error();
            UnhookWindowsHookEx(keyboard);
            return Err(format!("could not install the mouse hook ({error})"));
        }

        // Hooks are delivered through this thread's message queue.
        let mut message: MSG = std::mem::zeroed();
        while GetMessageW(&mut message, ptr::null_mut(), 0, 0) > 0 {}

        UnhookWindowsHookEx(keyboard);
        UnhookWindowsHookEx(mouse);
    }
    SINK.set(None);
    Ok(())
}

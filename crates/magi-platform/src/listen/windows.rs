//! Windows listener: low-level keyboard and mouse hooks, plus Raw Input for
//! relative mouse motion.
//!
//! Written against the Win32 API directly rather than through a wrapper crate
//! for reasons that matter to a tool which listens the whole time it is open:
//!
//! * it never asks Windows to translate key presses into characters, which is
//!   what breaks dead keys (´ ` ^ ~) for international layouts in other hooks;
//! * it can read each event's `dwExtraInfo`, so input that MacroMagi itself
//!   injected is recognised and never mistaken for the user's;
//! * it survives Windows removing its hooks. If a hook does not answer within
//!   the system's time limit (for instance while every core is busy), Windows
//!   silently unhooks it – and with it every hotkey. A watchdog compares the
//!   system's "last input" time with the last event the hooks saw and
//!   reinstalls them when they have gone quiet;
//! * it reads relative mouse motion through Raw Input, which is what games
//!   with a locked pointer see, for smart recordings.
//!
//! ## Raw Input and the keyboard hook
//!
//! Raw Input registrations are per process: one window per device type. The
//! window library registers the process for raw *keyboard* input when its
//! event loop starts. While that registration exists and a MacroMagi window
//! is in front, Windows does not call this process's low-level keyboard hook:
//! keys pressed in MacroMagi never reach the hotkey system (so a hotkey could
//! not be set), while mouse buttons keep working. MacroMagi needs no raw
//! keyboard input, so the listener removes that registration, claims raw
//! mouse input for its own window, and the watchdog keeps it that way.

use std::cell::{Cell, RefCell};
use std::ptr;
use std::time::Instant;

use magi_core::input::{InputEvent, MouseButton};
use windows_sys::Win32::Devices::HumanInterfaceDevice::{
    HID_USAGE_GENERIC_KEYBOARD, HID_USAGE_GENERIC_MOUSE, HID_USAGE_PAGE_GENERIC,
};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_TIME_CRITICAL};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows_sys::Win32::UI::Input::{
    GetRawInputData, GetRegisteredRawInputDevices, HRAWINPUT, MOUSE_MOVE_ABSOLUTE, RAWINPUT, RAWINPUTDEVICE,
    RAWINPUTHEADER, RID_INPUT, RIDEV_INPUTSINK, RIDEV_REMOVE, RIM_TYPEMOUSE, RegisterRawInputDevices,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, DispatchMessageW, HC_ACTION, HHOOK, HWND_MESSAGE, KBDLLHOOKSTRUCT,
    LLKHF_EXTENDED, MSG, MSLLHOOKSTRUCT, MsgWaitForMultipleObjects, PM_REMOVE, PeekMessageW, QS_ALLINPUT,
    RegisterClassExW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_INPUT,
    WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE,
    WM_MOUSEWHEEL, WM_QUIT, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_XBUTTONDOWN, WM_XBUTTONUP,
    WNDCLASSEXW, XBUTTON1,
};

use crate::Observed;
use crate::inject::INJECTION_MARKER;
use crate::keymap;

/// Placeholder virtual key that carries injected Unicode text, not a real key.
const VK_PACKET: u32 = 0xE7;
/// One notch of a standard mouse wheel.
const WHEEL_DELTA: i32 = 120;
/// How often the watchdog looks at the hooks, in milliseconds.
const WATCHDOG_MS: u32 = 1000;
/// The hooks are presumed removed when the system saw input this much later
/// than they did, in milliseconds.
const HOOK_SILENCE_MS: u32 = 1500;

type Sink = Box<dyn FnMut(Observed)>;

thread_local! {
    // Hooks and the window procedure are called on the thread that installed
    // them, so the sink can live in thread-local storage and needs no locking.
    static SINK: RefCell<Option<Sink>> = const { RefCell::new(None) };
    // High-resolution wheels report fractions of a notch; these carry the remainder.
    static WHEEL_REMAINDER: Cell<(i32, i32)> = const { Cell::new((0, 0)) };
    // System tick of the newest event either hook saw.
    static LAST_HOOK_EVENT: Cell<u32> = const { Cell::new(0) };
}

fn emit(event: InputEvent, synthetic: bool) {
    let observed = Observed { event, at: Instant::now(), synthetic };
    SINK.with_borrow_mut(|sink| {
        if let Some(sink) = sink {
            sink(observed);
        }
    });
}

fn note_hook_event(tick: u32) {
    LAST_HOOK_EVENT.set(tick);
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        // SAFETY: for WH_KEYBOARD_LL with HC_ACTION, lparam points to a KBDLLHOOKSTRUCT
        // that is valid for the duration of this call.
        let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        note_hook_event(info.time);
        if info.vkCode != VK_PACKET {
            let key = keymap::key_from_vk(info.vkCode, info.flags & LLKHF_EXTENDED != 0);
            let synthetic = info.dwExtraInfo == INJECTION_MARKER;
            match wparam as u32 {
                WM_KEYDOWN | WM_SYSKEYDOWN => emit(InputEvent::KeyDown { key }, synthetic),
                WM_KEYUP | WM_SYSKEYUP => emit(InputEvent::KeyUp { key }, synthetic),
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
        note_hook_event(info.time);
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
            emit(event, info.dwExtraInfo == INJECTION_MARKER);
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

/// Receives Raw Input: relative mouse motion as the device reported it.
unsafe extern "system" fn raw_window_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if message == WM_INPUT {
        // SAFETY: RAWINPUT is plain data; the buffer is large enough for a mouse
        // report and GetRawInputData is told its size.
        let mut input: RAWINPUT = unsafe { std::mem::zeroed() };
        let mut size = std::mem::size_of::<RAWINPUT>() as u32;
        let header = std::mem::size_of::<RAWINPUTHEADER>() as u32;
        let read =
            unsafe { GetRawInputData(lparam as HRAWINPUT, RID_INPUT, (&raw mut input).cast(), &mut size, header) };
        if read != u32::MAX && input.header.dwType == RIM_TYPEMOUSE {
            // SAFETY: dwType says the union holds a mouse report.
            let mouse = unsafe { input.data.mouse };
            let relative = mouse.usFlags & MOUSE_MOVE_ABSOLUTE == 0;
            if relative && (mouse.lLastX != 0 || mouse.lLastY != 0) {
                // Input injected with SendInput has no device.
                let synthetic = input.header.hDevice.is_null();
                emit(InputEvent::MoveBy { dx: mouse.lLastX, dy: mouse.lLastY }, synthetic);
            }
        }
    }
    // SAFETY: default handling for everything, including WM_INPUT's cleanup.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

/// A hidden window that receives Raw Input mouse reports even while other
/// programs have focus. `None` only costs relative motion, so it is not fatal.
fn create_raw_input_window() -> Option<HWND> {
    let class: Vec<u16> = "MacroMagiRawInput\0".encode_utf16().collect();
    // SAFETY: plain Win32 calls with valid, null-terminated strings and a
    // window procedure of the required signature.
    unsafe {
        let module = GetModuleHandleW(ptr::null());
        let mut window_class: WNDCLASSEXW = std::mem::zeroed();
        window_class.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
        window_class.lpfnWndProc = Some(raw_window_proc);
        window_class.hInstance = module;
        window_class.lpszClassName = class.as_ptr();
        RegisterClassExW(&window_class);
        let window = CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            ptr::null_mut(),
            module,
            ptr::null(),
        );
        (!window.is_null()).then_some(window)
    }
}

/// This process's Raw Input registrations, as (usage, target window).
fn raw_input_registrations() -> Vec<(u16, HWND)> {
    let size = std::mem::size_of::<RAWINPUTDEVICE>() as u32;
    let mut count = 0u32;
    // SAFETY: a null buffer asks only for the count; the second call gets a
    // buffer of exactly that many entries.
    unsafe {
        GetRegisteredRawInputDevices(ptr::null_mut(), &mut count, size);
        let mut devices: Vec<RAWINPUTDEVICE> = vec![std::mem::zeroed(); count as usize];
        let read = GetRegisteredRawInputDevices(devices.as_mut_ptr(), &mut count, size);
        if read == u32::MAX {
            return Vec::new();
        }
        devices.truncate(read as usize);
        devices
            .into_iter()
            .filter(|d| d.usUsagePage == HID_USAGE_PAGE_GENERIC)
            .map(|d| (d.usUsage, d.hwndTarget))
            .collect()
    }
}

/// Removes any raw keyboard registration (it silences the keyboard hook) and
/// points raw mouse input at `window`. Cheap when nothing needs changing.
fn claim_raw_input(window: Option<HWND>) {
    let registrations = raw_input_registrations();
    let size = std::mem::size_of::<RAWINPUTDEVICE>() as u32;
    if registrations.iter().any(|(usage, _)| *usage == HID_USAGE_GENERIC_KEYBOARD) {
        let keyboard = RAWINPUTDEVICE {
            usUsagePage: HID_USAGE_PAGE_GENERIC,
            usUsage: HID_USAGE_GENERIC_KEYBOARD,
            dwFlags: RIDEV_REMOVE,
            hwndTarget: ptr::null_mut(),
        };
        // SAFETY: one fully initialised entry, with its size.
        unsafe { RegisterRawInputDevices(&keyboard, 1, size) };
    }
    let Some(window) = window else { return };
    let mouse_is_ours =
        registrations.iter().any(|(usage, target)| *usage == HID_USAGE_GENERIC_MOUSE && *target == window);
    if !mouse_is_ours {
        let mouse = RAWINPUTDEVICE {
            usUsagePage: HID_USAGE_PAGE_GENERIC,
            usUsage: HID_USAGE_GENERIC_MOUSE,
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: window,
        };
        // SAFETY: one fully initialised entry, with its size.
        unsafe { RegisterRawInputDevices(&mouse, 1, size) };
    }
}

struct Hooks {
    keyboard: HHOOK,
    mouse: HHOOK,
}

impl Hooks {
    fn install() -> Result<Self, String> {
        // SAFETY: plain Win32 calls; the procedures have the required signature.
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
            Ok(Self { keyboard, mouse })
        }
    }
}

impl Drop for Hooks {
    fn drop(&mut self) {
        // SAFETY: the handles came from SetWindowsHookExW; unhooking a hook
        // Windows already removed fails harmlessly.
        unsafe {
            UnhookWindowsHookEx(self.keyboard);
            UnhookWindowsHookEx(self.mouse);
        }
    }
}

/// System tick of the last input in this session.
fn last_system_input() -> u32 {
    let mut info = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    // SAFETY: a plain call with a correctly sized struct.
    unsafe { GetLastInputInfo(&mut info) };
    info.dwTime
}

/// Installs the hooks and pumps messages forever. The sink runs inside the
/// hooks, so it must return quickly: the system stalls all keyboard and mouse
/// input while it is running.
pub(crate) fn run(sink: impl FnMut(Observed) + 'static) -> Result<(), String> {
    SINK.set(Some(Box::new(sink)));
    // The hooks must answer quickly even when the machine is busy, or Windows
    // drops them. This thread does almost nothing, so it may run first.
    // SAFETY: changes only this thread's priority.
    unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL) };

    let mut hooks = Hooks::install()?;
    let raw_window = create_raw_input_window();
    claim_raw_input(raw_window);
    let mut last_claim = Instant::now();
    LAST_HOOK_EVENT.set(last_system_input());

    loop {
        // SAFETY: waits for messages or the timeout; no handles are passed.
        unsafe { MsgWaitForMultipleObjects(0, ptr::null(), 0, WATCHDOG_MS, QS_ALLINPUT) };
        // Hooks and Raw Input are delivered while messages are retrieved.
        // SAFETY: standard message pump on this thread's queue.
        unsafe {
            let mut message: MSG = std::mem::zeroed();
            while PeekMessageW(&mut message, ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                if message.message == WM_QUIT {
                    SINK.set(None);
                    return Ok(());
                }
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }

        // The window library may register raw keyboard input again.
        if last_claim.elapsed().as_millis() >= u128::from(WATCHDOG_MS) {
            claim_raw_input(raw_window);
            last_claim = Instant::now();
        }

        // The system saw input that the hooks did not: they were removed.
        let silent_for = last_system_input().wrapping_sub(LAST_HOOK_EVENT.get());
        if silent_for > HOOK_SILENCE_MS && silent_for < u32::MAX / 2 {
            drop(hooks);
            hooks = Hooks::install()?;
            LAST_HOOK_EVENT.set(last_system_input());
        }
    }
}

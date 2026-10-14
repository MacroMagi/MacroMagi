//! macOS and Linux (X11) listener, built on the `rdev` crate.
//!
//! Limitations compared with the Windows listener: input injected by MacroMagi
//! itself cannot be told apart from the user's, and Wayland sessions are not
//! supported by `rdev`. macOS requires the Accessibility permission.

use std::time::Instant;

use magi_core::input::{InputEvent, Key, MouseButton};
use rdev::{Button, EventType, Key as R};

use crate::Observed;

pub(crate) fn run(mut sink: impl FnMut(Observed) + 'static) -> Result<(), String> {
    rdev::listen(move |event| {
        let mapped = match event.event_type {
            EventType::KeyPress(key) => Some(InputEvent::KeyDown { key: map_key(key) }),
            EventType::KeyRelease(key) => Some(InputEvent::KeyUp { key: map_key(key) }),
            EventType::ButtonPress(button) => map_button(button).map(|button| InputEvent::ButtonDown { button }),
            EventType::ButtonRelease(button) => map_button(button).map(|button| InputEvent::ButtonUp { button }),
            EventType::MouseMove { x, y } => Some(InputEvent::MoveTo { x: x.round() as i32, y: y.round() as i32 }),
            EventType::Wheel { delta_x, delta_y } => {
                Some(InputEvent::Scroll { dx: delta_x as i32, dy: delta_y as i32 })
            }
        };
        if let Some(event) = mapped {
            sink(Observed { event, at: Instant::now(), synthetic: false });
        }
    })
    .map_err(|e| format!("could not listen for input: {e:?}"))
}

fn map_button(button: Button) -> Option<MouseButton> {
    Some(match button {
        Button::Left => MouseButton::Left,
        Button::Right => MouseButton::Right,
        Button::Middle => MouseButton::Middle,
        // X11 numbers the side buttons 8 and 9; other values are wheel "buttons".
        Button::Unknown(8) => MouseButton::Back,
        Button::Unknown(9) => MouseButton::Forward,
        Button::Unknown(_) => return None,
    })
}

#[rustfmt::skip]
fn map_key(key: R) -> Key {
    match key {
        R::KeyA => Key::KeyA, R::KeyB => Key::KeyB, R::KeyC => Key::KeyC, R::KeyD => Key::KeyD,
        R::KeyE => Key::KeyE, R::KeyF => Key::KeyF, R::KeyG => Key::KeyG, R::KeyH => Key::KeyH,
        R::KeyI => Key::KeyI, R::KeyJ => Key::KeyJ, R::KeyK => Key::KeyK, R::KeyL => Key::KeyL,
        R::KeyM => Key::KeyM, R::KeyN => Key::KeyN, R::KeyO => Key::KeyO, R::KeyP => Key::KeyP,
        R::KeyQ => Key::KeyQ, R::KeyR => Key::KeyR, R::KeyS => Key::KeyS, R::KeyT => Key::KeyT,
        R::KeyU => Key::KeyU, R::KeyV => Key::KeyV, R::KeyW => Key::KeyW, R::KeyX => Key::KeyX,
        R::KeyY => Key::KeyY, R::KeyZ => Key::KeyZ,
        R::Num0 => Key::Digit0, R::Num1 => Key::Digit1, R::Num2 => Key::Digit2, R::Num3 => Key::Digit3,
        R::Num4 => Key::Digit4, R::Num5 => Key::Digit5, R::Num6 => Key::Digit6, R::Num7 => Key::Digit7,
        R::Num8 => Key::Digit8, R::Num9 => Key::Digit9,
        R::F1 => Key::F1, R::F2 => Key::F2, R::F3 => Key::F3, R::F4 => Key::F4, R::F5 => Key::F5,
        R::F6 => Key::F6, R::F7 => Key::F7, R::F8 => Key::F8, R::F9 => Key::F9, R::F10 => Key::F10,
        R::F11 => Key::F11, R::F12 => Key::F12,
        R::ControlLeft => Key::ControlLeft, R::ControlRight => Key::ControlRight,
        R::ShiftLeft => Key::ShiftLeft, R::ShiftRight => Key::ShiftRight,
        R::Alt => Key::AltLeft, R::AltGr => Key::AltRight,
        R::MetaLeft => Key::MetaLeft, R::MetaRight => Key::MetaRight,
        R::Space => Key::Space, R::Return => Key::Enter, R::Tab => Key::Tab,
        R::Backspace => Key::Backspace, R::Escape => Key::Escape, R::Delete => Key::Delete,
        R::Insert => Key::Insert, R::Home => Key::Home, R::End => Key::End,
        R::PageUp => Key::PageUp, R::PageDown => Key::PageDown,
        R::UpArrow => Key::ArrowUp, R::DownArrow => Key::ArrowDown,
        R::LeftArrow => Key::ArrowLeft, R::RightArrow => Key::ArrowRight,
        R::CapsLock => Key::CapsLock, R::NumLock => Key::NumLock, R::ScrollLock => Key::ScrollLock,
        R::PrintScreen => Key::PrintScreen, R::Pause => Key::Pause,
        R::Minus => Key::Minus, R::Equal => Key::Equal,
        R::LeftBracket => Key::BracketLeft, R::RightBracket => Key::BracketRight,
        R::BackSlash => Key::Backslash, R::SemiColon => Key::Semicolon, R::Quote => Key::Quote,
        R::BackQuote => Key::Backquote, R::Comma => Key::Comma, R::Dot => Key::Period,
        R::Slash => Key::Slash, R::IntlBackslash => Key::IntlBackslash,
        R::Kp0 => Key::Numpad0, R::Kp1 => Key::Numpad1, R::Kp2 => Key::Numpad2, R::Kp3 => Key::Numpad3,
        R::Kp4 => Key::Numpad4, R::Kp5 => Key::Numpad5, R::Kp6 => Key::Numpad6, R::Kp7 => Key::Numpad7,
        R::Kp8 => Key::Numpad8, R::Kp9 => Key::Numpad9,
        R::KpPlus => Key::NumpadAdd, R::KpMinus => Key::NumpadSubtract,
        R::KpMultiply => Key::NumpadMultiply, R::KpDivide => Key::NumpadDivide,
        R::KpDelete => Key::NumpadDecimal, R::KpReturn => Key::NumpadEnter,
        // The Fn key has no MacroMagi name; keep it replayable by code.
        R::Function => Key::Raw(0x3F),
        R::Unknown(code) => Key::Raw(code),
    }
}

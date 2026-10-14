//! Translation between MacroMagi's [`Key`] and each operating system's key codes.
//!
//! One table, one row per key, one column per platform:
//!
//! * **Windows** – virtual-key codes (`VK_*`), used both to send and to listen.
//! * **macOS** – virtual keycodes (`kVK_*`), used to send.
//! * **Linux** – X11 keysyms, used to send.
//!
//! Listening on macOS and Linux goes through `rdev`, which has its own key
//! enum; that mapping lives in `listen/rdev.rs`.

use magi_core::input::Key;

/// Marks a key that does not exist on a platform.
const NONE: u32 = u32::MAX;

/// `(key, Windows VK, macOS keycode, X11 keysym)`
#[rustfmt::skip]
const TABLE: &[(Key, u32, u32, u32)] = &[
    (Key::KeyA, 0x41, 0x00, 0x61), (Key::KeyB, 0x42, 0x0B, 0x62), (Key::KeyC, 0x43, 0x08, 0x63),
    (Key::KeyD, 0x44, 0x02, 0x64), (Key::KeyE, 0x45, 0x0E, 0x65), (Key::KeyF, 0x46, 0x03, 0x66),
    (Key::KeyG, 0x47, 0x05, 0x67), (Key::KeyH, 0x48, 0x04, 0x68), (Key::KeyI, 0x49, 0x22, 0x69),
    (Key::KeyJ, 0x4A, 0x26, 0x6A), (Key::KeyK, 0x4B, 0x28, 0x6B), (Key::KeyL, 0x4C, 0x25, 0x6C),
    (Key::KeyM, 0x4D, 0x2E, 0x6D), (Key::KeyN, 0x4E, 0x2D, 0x6E), (Key::KeyO, 0x4F, 0x1F, 0x6F),
    (Key::KeyP, 0x50, 0x23, 0x70), (Key::KeyQ, 0x51, 0x0C, 0x71), (Key::KeyR, 0x52, 0x0F, 0x72),
    (Key::KeyS, 0x53, 0x01, 0x73), (Key::KeyT, 0x54, 0x11, 0x74), (Key::KeyU, 0x55, 0x20, 0x75),
    (Key::KeyV, 0x56, 0x09, 0x76), (Key::KeyW, 0x57, 0x0D, 0x77), (Key::KeyX, 0x58, 0x07, 0x78),
    (Key::KeyY, 0x59, 0x10, 0x79), (Key::KeyZ, 0x5A, 0x06, 0x7A),

    (Key::Digit0, 0x30, 0x1D, 0x30), (Key::Digit1, 0x31, 0x12, 0x31), (Key::Digit2, 0x32, 0x13, 0x32),
    (Key::Digit3, 0x33, 0x14, 0x33), (Key::Digit4, 0x34, 0x15, 0x34), (Key::Digit5, 0x35, 0x17, 0x35),
    (Key::Digit6, 0x36, 0x16, 0x36), (Key::Digit7, 0x37, 0x1A, 0x37), (Key::Digit8, 0x38, 0x1C, 0x38),
    (Key::Digit9, 0x39, 0x19, 0x39),

    (Key::F1, 0x70, 0x7A, 0xFFBE), (Key::F2, 0x71, 0x78, 0xFFBF), (Key::F3, 0x72, 0x63, 0xFFC0),
    (Key::F4, 0x73, 0x76, 0xFFC1), (Key::F5, 0x74, 0x60, 0xFFC2), (Key::F6, 0x75, 0x61, 0xFFC3),
    (Key::F7, 0x76, 0x62, 0xFFC4), (Key::F8, 0x77, 0x64, 0xFFC5), (Key::F9, 0x78, 0x65, 0xFFC6),
    (Key::F10, 0x79, 0x6D, 0xFFC7), (Key::F11, 0x7A, 0x67, 0xFFC8), (Key::F12, 0x7B, 0x6F, 0xFFC9),
    (Key::F13, 0x7C, 0x69, 0xFFCA), (Key::F14, 0x7D, 0x6B, 0xFFCB), (Key::F15, 0x7E, 0x71, 0xFFCC),
    (Key::F16, 0x7F, 0x6A, 0xFFCD), (Key::F17, 0x80, 0x40, 0xFFCE), (Key::F18, 0x81, 0x4F, 0xFFCF),
    (Key::F19, 0x82, 0x50, 0xFFD0), (Key::F20, 0x83, 0x5A, 0xFFD1), (Key::F21, 0x84, NONE, 0xFFD2),
    (Key::F22, 0x85, NONE, 0xFFD3), (Key::F23, 0x86, NONE, 0xFFD4), (Key::F24, 0x87, NONE, 0xFFD5),

    (Key::ControlLeft, 0xA2, 0x3B, 0xFFE3), (Key::ControlRight, 0xA3, 0x3E, 0xFFE4),
    (Key::ShiftLeft, 0xA0, 0x38, 0xFFE1), (Key::ShiftRight, 0xA1, 0x3C, 0xFFE2),
    (Key::AltLeft, 0xA4, 0x3A, 0xFFE9), (Key::AltRight, 0xA5, 0x3D, 0xFFEA),
    (Key::MetaLeft, 0x5B, 0x37, 0xFFEB), (Key::MetaRight, 0x5C, 0x36, 0xFFEC),

    (Key::Space, 0x20, 0x31, 0x20), (Key::Enter, 0x0D, 0x24, 0xFF0D), (Key::Tab, 0x09, 0x30, 0xFF09),
    (Key::Backspace, 0x08, 0x33, 0xFF08), (Key::Escape, 0x1B, 0x35, 0xFF1B),
    (Key::Delete, 0x2E, 0x75, 0xFFFF), (Key::Insert, 0x2D, 0x72, 0xFF63),
    (Key::Home, 0x24, 0x73, 0xFF50), (Key::End, 0x23, 0x77, 0xFF57),
    (Key::PageUp, 0x21, 0x74, 0xFF55), (Key::PageDown, 0x22, 0x79, 0xFF56),
    (Key::ArrowUp, 0x26, 0x7E, 0xFF52), (Key::ArrowDown, 0x28, 0x7D, 0xFF54),
    (Key::ArrowLeft, 0x25, 0x7B, 0xFF51), (Key::ArrowRight, 0x27, 0x7C, 0xFF53),
    (Key::CapsLock, 0x14, 0x39, 0xFFE5), (Key::NumLock, 0x90, 0x47, 0xFF7F),
    (Key::ScrollLock, 0x91, NONE, 0xFF14), (Key::PrintScreen, 0x2C, NONE, 0xFF61),
    (Key::Pause, 0x13, NONE, 0xFF13), (Key::ContextMenu, 0x5D, 0x6E, 0xFF67),

    (Key::Minus, 0xBD, 0x1B, 0x2D), (Key::Equal, 0xBB, 0x18, 0x3D),
    (Key::BracketLeft, 0xDB, 0x21, 0x5B), (Key::BracketRight, 0xDD, 0x1E, 0x5D),
    (Key::Backslash, 0xDC, 0x2A, 0x5C), (Key::Semicolon, 0xBA, 0x29, 0x3B),
    (Key::Quote, 0xDE, 0x27, 0x27), (Key::Backquote, 0xC0, 0x32, 0x60),
    (Key::Comma, 0xBC, 0x2B, 0x2C), (Key::Period, 0xBE, 0x2F, 0x2E), (Key::Slash, 0xBF, 0x2C, 0x2F),
    (Key::IntlBackslash, 0xE2, 0x0A, 0x3C),

    (Key::Numpad0, 0x60, 0x52, 0xFFB0), (Key::Numpad1, 0x61, 0x53, 0xFFB1),
    (Key::Numpad2, 0x62, 0x54, 0xFFB2), (Key::Numpad3, 0x63, 0x55, 0xFFB3),
    (Key::Numpad4, 0x64, 0x56, 0xFFB4), (Key::Numpad5, 0x65, 0x57, 0xFFB5),
    (Key::Numpad6, 0x66, 0x58, 0xFFB6), (Key::Numpad7, 0x67, 0x59, 0xFFB7),
    (Key::Numpad8, 0x68, 0x5B, 0xFFB8), (Key::Numpad9, 0x69, 0x5C, 0xFFB9),
    (Key::NumpadAdd, 0x6B, 0x45, 0xFFAB), (Key::NumpadSubtract, 0x6D, 0x4E, 0xFFAD),
    (Key::NumpadMultiply, 0x6A, 0x43, 0xFFAA), (Key::NumpadDivide, 0x6F, 0x4B, 0xFFAF),
    (Key::NumpadDecimal, 0x6E, 0x41, 0xFFAE),
    // Windows has no separate code for the numpad Enter key: it is VK_RETURN
    // with the "extended" flag, which the listener checks for.
    (Key::NumpadEnter, 0x0D, 0x4C, 0xFF8D),

    (Key::AudioVolumeUp, 0xAF, 0x48, 0x1008_FF13), (Key::AudioVolumeDown, 0xAE, 0x49, 0x1008_FF11),
    (Key::AudioVolumeMute, 0xAD, 0x4A, 0x1008_FF12), (Key::MediaPlayPause, 0xB3, NONE, 0x1008_FF14),
    (Key::MediaTrackNext, 0xB0, NONE, 0x1008_FF17), (Key::MediaTrackPrevious, 0xB1, NONE, 0x1008_FF16),
    (Key::MediaStop, 0xB2, NONE, 0x1008_FF15),
];

fn column(row: &(Key, u32, u32, u32)) -> u32 {
    if cfg!(target_os = "windows") {
        row.1
    } else if cfg!(target_os = "macos") {
        row.2
    } else {
        row.3
    }
}

/// The code to send for a key on this platform, if the key exists here.
/// For [`Key::Raw`] this is the raw code itself.
pub fn native_code(key: Key) -> Option<u32> {
    if let Key::Raw(code) = key {
        return Some(code);
    }
    TABLE.iter().find(|row| row.0 == key).map(column).filter(|&code| code != NONE)
}

/// The key for a Windows virtual-key code, as reported by the keyboard hook.
/// `extended` is the hook's "extended key" flag.
pub fn key_from_vk(vk: u32, extended: bool) -> Key {
    const VK_SHIFT: u32 = 0x10;
    const VK_CONTROL: u32 = 0x11;
    const VK_MENU: u32 = 0x12;
    const VK_RETURN: u32 = 0x0D;
    match (vk, extended) {
        (VK_RETURN, true) => Key::NumpadEnter,
        (VK_RETURN, false) => Key::Enter,
        // Side-less codes only appear in input synthesised by other programs.
        (VK_SHIFT, _) => Key::ShiftLeft,
        (VK_CONTROL, _) => Key::ControlLeft,
        (VK_MENU, _) => Key::AltLeft,
        _ => TABLE.iter().find(|row| row.1 == vk).map_or(Key::Raw(vk), |row| row.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_named_key_is_in_the_table_once() {
        for &key in Key::ALL {
            assert_eq!(TABLE.iter().filter(|row| row.0 == key).count(), 1, "{key:?}");
        }
    }

    #[test]
    fn codes_are_unique_per_platform() {
        for (name, pick) in [
            ("windows", (|row| row.1) as fn(&(Key, u32, u32, u32)) -> u32),
            ("macos", |row| row.2),
            ("x11", |row| row.3),
        ] {
            let mut codes: Vec<u32> = TABLE
                .iter()
                // Enter and numpad Enter intentionally share a Windows code.
                .filter(|row| !(name == "windows" && row.0 == Key::NumpadEnter))
                .map(pick)
                .filter(|&code| code != NONE)
                .collect();
            let before = codes.len();
            codes.sort_unstable();
            codes.dedup();
            assert_eq!(codes.len(), before, "duplicate {name} key code");
        }
    }

    #[test]
    fn windows_codes_round_trip() {
        for &key in Key::ALL {
            let vk = TABLE.iter().find(|row| row.0 == key).unwrap().1;
            assert_eq!(key_from_vk(vk, key == Key::NumpadEnter), key);
        }
        assert_eq!(key_from_vk(0xE8, false), Key::Raw(0xE8));
        assert_eq!(native_code(Key::Raw(77)), Some(77));
    }
}

//! The input vocabulary shared by every part of MacroMagi: keys, mouse
//! buttons, gamepad controls, and the events built from them.
//!
//! Identifiers are stable strings (they are written into `.magi` files), so
//! renaming a variant is a file-format change. Keyboard keys follow the W3C
//! `KeyboardEvent.code` names and describe *physical* keys, not characters.

use std::borrow::Cow;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Declares a field-less enum whose variants serialize as their own names and
/// carry a human-readable label.
macro_rules! id_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident { $( $variant:ident => $label:expr ),+ $(,)? }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum $name { $( $variant ),+ }

        impl $name {
            pub const ALL: &'static [$name] = &[ $( $name::$variant ),+ ];

            /// Stable identifier used in files.
            pub fn id(self) -> &'static str {
                match self { $( $name::$variant => stringify!($variant) ),+ }
            }

            /// Text shown to users.
            pub fn label(self) -> &'static str {
                match self { $( $name::$variant => $label ),+ }
            }

            pub fn from_id(id: &str) -> Option<Self> {
                match id { $( stringify!($variant) => Some($name::$variant), )+ _ => None }
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(self.id())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let id = String::deserialize(d)?;
                Self::from_id(&id).ok_or_else(|| {
                    serde::de::Error::custom(format!("unknown {} `{id}`", stringify!($name)))
                })
            }
        }
    };
}

macro_rules! keys {
    ( $( $variant:ident => $label:expr ),+ $(,)? ) => {
        /// A physical keyboard key.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Key {
            $( $variant, )+
            /// A key MacroMagi has no name for, identified by its platform key code.
            /// It can still be recorded, replayed and used as a hotkey on the same OS.
            Raw(u32),
        }

        impl Key {
            /// Every named key, in a sensible order for pickers.
            pub const ALL: &'static [Key] = &[ $( Key::$variant ),+ ];

            /// Stable identifier used in files.
            pub fn id(self) -> Cow<'static, str> {
                match self {
                    $( Key::$variant => Cow::Borrowed(stringify!($variant)), )+
                    Key::Raw(code) => Cow::Owned(format!("Raw{code}")),
                }
            }

            pub fn from_id(id: &str) -> Option<Key> {
                match id {
                    $( stringify!($variant) => Some(Key::$variant), )+
                    _ => id.strip_prefix("Raw")?.parse().ok().map(Key::Raw),
                }
            }

            fn base_label(self) -> Cow<'static, str> {
                match self {
                    $( Key::$variant => Cow::Borrowed($label), )+
                    Key::Raw(code) => Cow::Owned(format!("Key #{code}")),
                }
            }
        }
    };
}

keys! {
    KeyA => "A", KeyB => "B", KeyC => "C", KeyD => "D", KeyE => "E", KeyF => "F", KeyG => "G",
    KeyH => "H", KeyI => "I", KeyJ => "J", KeyK => "K", KeyL => "L", KeyM => "M", KeyN => "N",
    KeyO => "O", KeyP => "P", KeyQ => "Q", KeyR => "R", KeyS => "S", KeyT => "T", KeyU => "U",
    KeyV => "V", KeyW => "W", KeyX => "X", KeyY => "Y", KeyZ => "Z",
    Digit0 => "0", Digit1 => "1", Digit2 => "2", Digit3 => "3", Digit4 => "4",
    Digit5 => "5", Digit6 => "6", Digit7 => "7", Digit8 => "8", Digit9 => "9",
    F1 => "F1", F2 => "F2", F3 => "F3", F4 => "F4", F5 => "F5", F6 => "F6",
    F7 => "F7", F8 => "F8", F9 => "F9", F10 => "F10", F11 => "F11", F12 => "F12",
    F13 => "F13", F14 => "F14", F15 => "F15", F16 => "F16", F17 => "F17", F18 => "F18",
    F19 => "F19", F20 => "F20", F21 => "F21", F22 => "F22", F23 => "F23", F24 => "F24",
    ControlLeft => "Left Ctrl", ControlRight => "Right Ctrl",
    ShiftLeft => "Left Shift", ShiftRight => "Right Shift",
    AltLeft => "Left Alt", AltRight => "Right Alt",
    MetaLeft => "Left Meta", MetaRight => "Right Meta",
    Space => "Space", Enter => "Enter", Tab => "Tab", Backspace => "Backspace",
    Escape => "Esc", Delete => "Delete", Insert => "Insert",
    Home => "Home", End => "End", PageUp => "Page Up", PageDown => "Page Down",
    ArrowUp => "Up", ArrowDown => "Down", ArrowLeft => "Left", ArrowRight => "Right",
    CapsLock => "Caps Lock", NumLock => "Num Lock", ScrollLock => "Scroll Lock",
    PrintScreen => "Print Screen", Pause => "Pause", ContextMenu => "Menu",
    Minus => "-", Equal => "=", BracketLeft => "[", BracketRight => "]", Backslash => "\\",
    Semicolon => ";", Quote => "'", Backquote => "`", Comma => ",", Period => ".", Slash => "/",
    IntlBackslash => "Intl \\",
    Numpad0 => "Num 0", Numpad1 => "Num 1", Numpad2 => "Num 2", Numpad3 => "Num 3",
    Numpad4 => "Num 4", Numpad5 => "Num 5", Numpad6 => "Num 6", Numpad7 => "Num 7",
    Numpad8 => "Num 8", Numpad9 => "Num 9",
    NumpadAdd => "Num +", NumpadSubtract => "Num -", NumpadMultiply => "Num *",
    NumpadDivide => "Num /", NumpadDecimal => "Num .", NumpadEnter => "Num Enter",
    AudioVolumeUp => "Volume Up", AudioVolumeDown => "Volume Down", AudioVolumeMute => "Mute",
    MediaPlayPause => "Play/Pause", MediaTrackNext => "Next Track",
    MediaTrackPrevious => "Previous Track", MediaStop => "Media Stop",
}

/// What the "meta" key is called on the OS this build targets.
pub const fn meta_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "Cmd"
    } else if cfg!(target_os = "windows") {
        "Win"
    } else {
        "Super"
    }
}

impl Key {
    /// Text shown to users, e.g. `Left Ctrl`.
    pub fn label(self) -> Cow<'static, str> {
        match self {
            Key::MetaLeft => Cow::Owned(format!("Left {}", meta_name())),
            Key::MetaRight => Cow::Owned(format!("Right {}", meta_name())),
            other => other.base_label(),
        }
    }

    pub fn is_modifier(self) -> bool {
        matches!(
            self,
            Key::ControlLeft
                | Key::ControlRight
                | Key::ShiftLeft
                | Key::ShiftRight
                | Key::AltLeft
                | Key::AltRight
                | Key::MetaLeft
                | Key::MetaRight
        )
    }

    /// Folds right-hand modifiers onto their left-hand twin, so that a hotkey
    /// bound to "Ctrl" fires for either Ctrl key.
    pub fn canonical(self) -> Key {
        match self {
            Key::ControlRight => Key::ControlLeft,
            Key::ShiftRight => Key::ShiftLeft,
            Key::AltRight => Key::AltLeft,
            Key::MetaRight => Key::MetaLeft,
            other => other,
        }
    }

    /// Label for a hotkey chord, where the side of a modifier is irrelevant.
    pub fn chord_label(self) -> Cow<'static, str> {
        match self.canonical() {
            Key::ControlLeft => Cow::Borrowed("Ctrl"),
            Key::ShiftLeft => Cow::Borrowed("Shift"),
            Key::AltLeft => Cow::Borrowed("Alt"),
            Key::MetaLeft => Cow::Borrowed(meta_name()),
            other => other.label(),
        }
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.id())
    }
}

impl Serialize for Key {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.id())
    }
}

impl<'de> Deserialize<'de> for Key {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let id = String::deserialize(d)?;
        Key::from_id(&id).ok_or_else(|| serde::de::Error::custom(format!("unknown key `{id}`")))
    }
}

id_enum! {
    pub enum MouseButton {
        Left => "Left",
        Right => "Right",
        Middle => "Middle",
        Back => "Back",
        Forward => "Forward",
    }
}

id_enum! {
    /// Gamepad buttons, named by position so they are layout-neutral
    /// (`South` is A on Xbox pads and Cross on PlayStation pads).
    pub enum GamepadButton {
        South => "A / Cross",
        East => "B / Circle",
        West => "X / Square",
        North => "Y / Triangle",
        LeftBumper => "LB / L1",
        RightBumper => "RB / R1",
        LeftTrigger => "LT / L2",
        RightTrigger => "RT / R2",
        Select => "Back / Share",
        Start => "Start / Options",
        Guide => "Guide",
        LeftStick => "Left Stick Click",
        RightStick => "Right Stick Click",
        DPadUp => "D-Pad Up",
        DPadDown => "D-Pad Down",
        DPadLeft => "D-Pad Left",
        DPadRight => "D-Pad Right",
    }
}

id_enum! {
    pub enum GamepadAxis {
        LeftStickX => "Left Stick X",
        LeftStickY => "Left Stick Y",
        RightStickX => "Right Stick X",
        RightStickY => "Right Stick Y",
        LeftTrigger => "Left Trigger",
        RightTrigger => "Right Trigger",
    }
}

/// Anything that can be held down: the unit hotkeys are built from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Button {
    Key(Key),
    Mouse(MouseButton),
    Pad(GamepadButton),
}

impl Button {
    const MOUSE_PREFIX: &'static str = "Mouse:";
    const PAD_PREFIX: &'static str = "Pad:";

    pub fn id(self) -> Cow<'static, str> {
        match self {
            Button::Key(key) => key.id(),
            Button::Mouse(b) => Cow::Owned(format!("{}{}", Self::MOUSE_PREFIX, b.id())),
            Button::Pad(b) => Cow::Owned(format!("{}{}", Self::PAD_PREFIX, b.id())),
        }
    }

    pub fn from_id(id: &str) -> Option<Button> {
        if let Some(rest) = id.strip_prefix(Self::MOUSE_PREFIX) {
            MouseButton::from_id(rest).map(Button::Mouse)
        } else if let Some(rest) = id.strip_prefix(Self::PAD_PREFIX) {
            GamepadButton::from_id(rest).map(Button::Pad)
        } else {
            Key::from_id(id).map(Button::Key)
        }
    }

    pub fn is_modifier(self) -> bool {
        matches!(self, Button::Key(key) if key.is_modifier())
    }

    pub fn canonical(self) -> Button {
        match self {
            Button::Key(key) => Button::Key(key.canonical()),
            other => other,
        }
    }

    pub fn label(self) -> Cow<'static, str> {
        match self {
            Button::Key(key) => key.chord_label(),
            Button::Mouse(b) => Cow::Owned(format!("Mouse {}", b.label())),
            Button::Pad(b) => Cow::Owned(format!("Pad {}", b.label())),
        }
    }
}

impl Serialize for Button {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.id())
    }
}

impl<'de> Deserialize<'de> for Button {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let id = String::deserialize(d)?;
        Button::from_id(&id).ok_or_else(|| serde::de::Error::custom(format!("unknown button `{id}`")))
    }
}

/// One low-level input event.
///
/// The same type is used in both directions: engines *send* these to the
/// [`Host`](crate::host::Host), and the platform listener *reports* them for
/// hotkeys and recording.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InputEvent {
    KeyDown {
        key: Key,
    },
    KeyUp {
        key: Key,
    },
    ButtonDown {
        button: MouseButton,
    },
    ButtonUp {
        button: MouseButton,
    },
    /// Move the pointer to an absolute screen position.
    MoveTo {
        x: i32,
        y: i32,
    },
    /// Move the pointer relative to where it is now.
    MoveBy {
        dx: i32,
        dy: i32,
    },
    /// Scroll by wheel notches. Positive `dy` scrolls up, positive `dx` right.
    Scroll {
        dx: i32,
        dy: i32,
    },
    /// Type a string, independent of keyboard layout. Send-only.
    Text {
        text: String,
    },
    PadButtonDown {
        button: GamepadButton,
    },
    PadButtonUp {
        button: GamepadButton,
    },
    PadAxis {
        axis: GamepadAxis,
        value: f32,
    },
}

impl InputEvent {
    /// If this event is a button changing state, returns it and whether it went down.
    pub fn button_edge(&self) -> Option<(Button, bool)> {
        Some(match *self {
            InputEvent::KeyDown { key } => (Button::Key(key), true),
            InputEvent::KeyUp { key } => (Button::Key(key), false),
            InputEvent::ButtonDown { button } => (Button::Mouse(button), true),
            InputEvent::ButtonUp { button } => (Button::Mouse(button), false),
            InputEvent::PadButtonDown { button } => (Button::Pad(button), true),
            InputEvent::PadButtonUp { button } => (Button::Pad(button), false),
            _ => return None,
        })
    }

    /// True for events that need a virtual gamepad device to be sent.
    pub fn is_gamepad(&self) -> bool {
        matches!(self, InputEvent::PadButtonDown { .. } | InputEvent::PadButtonUp { .. } | InputEvent::PadAxis { .. })
    }
}

impl fmt::Display for InputEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InputEvent::KeyDown { key } => write!(f, "key down {}", key.label()),
            InputEvent::KeyUp { key } => write!(f, "key up {}", key.label()),
            InputEvent::ButtonDown { button } => write!(f, "mouse down {}", button.label()),
            InputEvent::ButtonUp { button } => write!(f, "mouse up {}", button.label()),
            InputEvent::MoveTo { x, y } => write!(f, "move to ({x}, {y})"),
            InputEvent::MoveBy { dx, dy } => write!(f, "move by ({dx}, {dy})"),
            InputEvent::Scroll { dx, dy } => write!(f, "scroll ({dx}, {dy})"),
            InputEvent::Text { text } => write!(f, "type {text:?}"),
            InputEvent::PadButtonDown { button } => write!(f, "pad down {}", button.label()),
            InputEvent::PadButtonUp { button } => write!(f, "pad up {}", button.label()),
            InputEvent::PadAxis { axis, value } => write!(f, "pad axis {} = {value:.2}", axis.label()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_ids_round_trip() {
        for &key in Key::ALL {
            assert_eq!(Key::from_id(&key.id()), Some(key), "{key:?}");
        }
        assert_eq!(Key::from_id("Raw231"), Some(Key::Raw(231)));
        assert_eq!(Key::from_id("NotAKey"), None);
    }

    #[test]
    fn button_ids_round_trip() {
        let buttons = [Button::Key(Key::F6), Button::Mouse(MouseButton::Back), Button::Pad(GamepadButton::South)];
        for button in buttons {
            assert_eq!(Button::from_id(&button.id()), Some(button));
        }
    }

    #[test]
    fn events_serialize_with_a_type_tag() {
        let json = serde_json::to_string(&InputEvent::KeyDown { key: Key::KeyA }).unwrap();
        assert_eq!(json, r#"{"type":"key_down","key":"KeyA"}"#);
    }
}

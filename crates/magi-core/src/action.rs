//! Actions: the individual inputs an **Input** node performs.
//!
//! A node holds a list of actions that happen *together*: every action is
//! started in order, held for the node's hold time, then released in reverse
//! order. That is how one node expresses `Ctrl + C`, or "hold W and click".

use serde::{Deserialize, Serialize};

use crate::geom::Point;
use crate::input::{Key, MouseButton};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PressMode {
    /// Press, hold for the node's hold time, release.
    #[default]
    Tap,
    /// Press and leave held (until a later `Release`, or the macro stops).
    Press,
    /// Release a button that an earlier action pressed.
    Release,
}

impl PressMode {
    pub const ALL: [PressMode; 3] = [PressMode::Tap, PressMode::Press, PressMode::Release];

    pub fn label(self) -> &'static str {
        match self {
            PressMode::Tap => "Tap",
            PressMode::Press => "Hold down",
            PressMode::Release => "Release",
        }
    }
}

fn one() -> u32 {
    1
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Key {
        key: Key,
        #[serde(default)]
        mode: PressMode,
    },
    Mouse {
        button: MouseButton,
        #[serde(default)]
        mode: PressMode,
        /// 2 for a double click, 3 for a triple click. Only used by `Tap`.
        #[serde(default = "one")]
        clicks: u32,
        /// Where to click. `None` clicks wherever the pointer already is
        /// (or at the node's `position` input, when that is wired).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        at: Option<Point>,
    },
    Move {
        x: i32,
        y: i32,
        /// The display `x`/`y` are on (0 = primary). `None`: global coordinates.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display: Option<u8>,
        /// Treat `x`/`y` as an offset from the current pointer position.
        #[serde(default)]
        relative: bool,
        /// Glide there over this many milliseconds instead of jumping.
        #[serde(default)]
        duration_ms: u64,
    },
    Scroll {
        #[serde(default)]
        dx: i32,
        #[serde(default)]
        dy: i32,
    },
    Text {
        text: String,
    },
}

impl Action {
    pub const KINDS: [&'static str; 5] = ["key", "mouse", "move", "scroll", "text"];

    pub fn click(button: MouseButton) -> Self {
        Action::Mouse { button, mode: PressMode::Tap, clicks: 1, at: None }
    }

    pub fn tap(key: Key) -> Self {
        Action::Key { key, mode: PressMode::Tap }
    }

    /// A tap of a key or a click of a mouse button. Gamepad buttons cannot be sent.
    pub fn press(button: crate::input::Button) -> Option<Self> {
        match button {
            crate::input::Button::Key(key) => Some(Action::tap(key)),
            crate::input::Button::Mouse(button) => Some(Action::click(button)),
            crate::input::Button::Pad(_) => None,
        }
    }

    /// A fresh action of the given kind, used by the "add input" menu.
    pub fn default_of(kind: &str) -> Option<Self> {
        Some(match kind {
            "key" => Action::tap(Key::Space),
            "mouse" => Action::click(MouseButton::Left),
            "move" => Action::Move { x: 0, y: 0, display: Some(0), relative: false, duration_ms: 0 },
            "scroll" => Action::Scroll { dx: 0, dy: -1 },
            "text" => Action::Text { text: String::new() },
            _ => return None,
        })
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Action::Key { .. } => "key",
            Action::Mouse { .. } => "mouse",
            Action::Move { .. } => "move",
            Action::Scroll { .. } => "scroll",
            Action::Text { .. } => "text",
        }
    }

    pub fn kind_label(kind: &str) -> &'static str {
        match kind {
            "key" => "Keyboard key",
            "mouse" => "Mouse button",
            "move" => "Mouse move",
            "scroll" => "Scroll wheel",
            "text" => "Type text",
            _ => "Input",
        }
    }

    /// True if this action presses something that is released after the hold time.
    pub fn is_tap(&self) -> bool {
        matches!(self, Action::Key { mode: PressMode::Tap, .. } | Action::Mouse { mode: PressMode::Tap, .. })
    }

    /// One-line description shown on the node.
    pub fn summary(&self) -> String {
        fn verb(mode: PressMode) -> &'static str {
            match mode {
                PressMode::Tap => "Tap",
                PressMode::Press => "Hold",
                PressMode::Release => "Release",
            }
        }
        match self {
            Action::Key { key, mode } => format!("{} {}", verb(*mode), key.label()),
            Action::Mouse { button, mode, clicks, at } => {
                let what = match (mode, clicks) {
                    (PressMode::Tap, 2) => "Double-click".to_owned(),
                    (PressMode::Tap, 3) => "Triple-click".to_owned(),
                    (PressMode::Tap, n) if *n > 3 => format!("Click ×{n}"),
                    (PressMode::Tap, _) => "Click".to_owned(),
                    (mode, _) => verb(*mode).to_owned(),
                };
                let place = at.map(|p| format!(" at {}, {}", p.x, p.y)).unwrap_or_default();
                format!("{what} {}{place}", button.label().to_lowercase())
            }
            Action::Move { x, y, relative: true, .. } => format!("Move by {x}, {y}"),
            Action::Move { x, y, relative: false, .. } => format!("Move to {x}, {y}"),
            Action::Scroll { dx, dy } => match (dx, dy) {
                (0, dy) if *dy > 0 => format!("Scroll up {dy}"),
                (0, dy) => format!("Scroll down {}", -dy),
                (dx, 0) if *dx > 0 => format!("Scroll right {dx}"),
                (dx, 0) => format!("Scroll left {}", -dx),
                (dx, dy) => format!("Scroll {dx}, {dy}"),
            },
            Action::Text { text } => {
                let mut preview: String = text.chars().take(18).collect();
                if text.chars().count() > 18 {
                    preview.push('…');
                }
                format!("Type “{preview}”")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_omitted_and_restored() {
        let json = serde_json::to_string(&Action::click(MouseButton::Left)).unwrap();
        assert_eq!(json, r#"{"kind":"mouse","button":"Left","mode":"tap","clicks":1}"#);
        let parsed: Action = serde_json::from_str(r#"{"kind":"mouse","button":"Right"}"#).unwrap();
        assert_eq!(parsed, Action::click(MouseButton::Right));
    }

    #[test]
    fn every_kind_has_a_default() {
        for kind in Action::KINDS {
            assert_eq!(Action::default_of(kind).unwrap().kind(), kind);
        }
    }
}

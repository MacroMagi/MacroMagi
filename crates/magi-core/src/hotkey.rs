//! Hotkeys: chords of buttons from any device, and the logic that detects them
//! in a stream of raw input events.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::input::{Button, InputEvent, Key, MouseButton};

/// A set of buttons that must be held together. May mix devices, e.g.
/// `Ctrl + Mouse Back` or a single gamepad button. An empty hotkey means "none".
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Hotkey(Vec<Button>);

impl Hotkey {
    pub const fn none() -> Self {
        Hotkey(Vec::new())
    }

    pub fn key(key: Key) -> Self {
        Hotkey::from_buttons([Button::Key(key)])
    }

    /// Builds a hotkey; modifiers are folded to their canonical side and listed
    /// first, duplicates are dropped.
    pub fn from_buttons(buttons: impl IntoIterator<Item = Button>) -> Self {
        let mut out: Vec<Button> = Vec::new();
        for button in buttons {
            let button = button.canonical();
            if !out.contains(&button) {
                out.push(button);
            }
        }
        // Stable: modifiers first, everything else keeps the order it was pressed in.
        out.sort_by_key(|b| !b.is_modifier());
        Hotkey(out)
    }

    pub fn is_none(&self) -> bool {
        self.0.is_empty()
    }

    pub fn buttons(&self) -> &[Button] {
        &self.0
    }

    pub fn contains(&self, button: Button) -> bool {
        self.0.contains(&button.canonical())
    }

    /// Text shown to users, e.g. `Ctrl + Shift + F6`.
    pub fn label(&self) -> String {
        if self.is_none() {
            return "None".to_owned();
        }
        let parts: Vec<_> = self.0.iter().map(|b| b.label()).collect();
        parts.join(" + ")
    }
}

impl fmt::Display for Hotkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, button) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str("+")?;
            }
            f.write_str(&button.id())?;
        }
        Ok(())
    }
}

impl FromStr for Hotkey {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.trim().is_empty() {
            return Ok(Hotkey::none());
        }
        let buttons = s
            .split('+')
            .map(|part| Button::from_id(part.trim()).ok_or_else(|| format!("unknown button `{part}` in hotkey")))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Hotkey::from_buttons(buttons))
    }
}

impl Serialize for Hotkey {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Hotkey {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d)?.parse().map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotkeyEdge {
    /// The last button of the chord went down.
    Pressed,
    /// Any button of a pressed chord was let go.
    Released,
}

/// Turns raw button events into hotkey presses and releases.
///
/// Matching is deliberately forgiving: a hotkey fires when all of its buttons
/// are held, even if other buttons are held too (so `F6` still works while you
/// hold `W` and `Shift` in a game). When several bindings match at once only
/// the most specific fire, so `F6` stays quiet when `Ctrl + F6` is also bound
/// and pressed.
#[derive(Clone, Debug)]
pub struct HotkeyTracker<T> {
    bindings: Vec<(Hotkey, T)>,
    down: Vec<Button>,
    active: Vec<usize>,
}

impl<T> Default for HotkeyTracker<T> {
    fn default() -> Self {
        Self { bindings: Vec::new(), down: Vec::new(), active: Vec::new() }
    }
}

impl<T: Clone> HotkeyTracker<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces all bindings. Chords that are currently held are forgotten
    /// without a `Released` edge; callers that care should stop what they started.
    pub fn set_bindings(&mut self, bindings: Vec<(Hotkey, T)>) {
        self.bindings = bindings.into_iter().filter(|(hotkey, _)| !hotkey.is_none()).collect();
        self.active.clear();
    }

    /// Buttons currently held, in the order they went down.
    pub fn held(&self) -> &[Button] {
        &self.down
    }

    /// Forgets all held buttons. Use after input was missed (e.g. listener restart).
    pub fn reset(&mut self) {
        self.down.clear();
        self.active.clear();
    }

    pub fn feed(&mut self, event: &InputEvent) -> Vec<(T, HotkeyEdge)> {
        let Some((button, is_down)) = event.button_edge() else {
            return Vec::new();
        };
        let button = button.canonical();
        let mut out = Vec::new();

        if is_down {
            if self.down.contains(&button) {
                return out; // keyboard auto-repeat
            }
            self.down.push(button);

            let matches: Vec<usize> = (0..self.bindings.len())
                .filter(|i| !self.active.contains(i))
                .filter(|&i| {
                    let hotkey = &self.bindings[i].0;
                    hotkey.contains(button) && hotkey.buttons().iter().all(|b| self.down.contains(b))
                })
                .collect();
            let most_specific = matches.iter().map(|&i| self.bindings[i].0.buttons().len()).max();
            for i in matches {
                if Some(self.bindings[i].0.buttons().len()) == most_specific {
                    self.active.push(i);
                    out.push((self.bindings[i].1.clone(), HotkeyEdge::Pressed));
                }
            }
        } else {
            self.down.retain(|b| *b != button);
            let bindings = &self.bindings;
            self.active.retain(|&i| {
                let released = bindings[i].0.contains(button);
                if released {
                    out.push((bindings[i].1.clone(), HotkeyEdge::Released));
                }
                !released
            });
        }
        out
    }
}

/// Records the chord a user presses when rebinding a hotkey.
///
/// The chord is complete as soon as a button is released: whatever was held at
/// its peak becomes the hotkey. `Esc` alone cancels. Left and right mouse
/// clicks are ignored so that clicking elsewhere can never bind "click".
#[derive(Clone, Debug, Default)]
pub struct ChordCapture {
    down: Vec<Button>,
    peak: Vec<Button>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaptureResult {
    /// Still waiting; holds the buttons pressed so far, for live feedback.
    Pending(Hotkey),
    Done(Hotkey),
    Cancelled,
}

impl ChordCapture {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn feed(&mut self, event: &InputEvent) -> CaptureResult {
        let Some((button, is_down)) = event.button_edge() else {
            return self.pending();
        };
        if matches!(button, Button::Mouse(MouseButton::Left | MouseButton::Right)) {
            return self.pending();
        }
        let button = button.canonical();
        if is_down {
            if !self.down.contains(&button) {
                self.down.push(button);
                self.peak = self.down.clone();
            }
            self.pending()
        } else if self.peak.is_empty() {
            // A release of something pressed before the capture began.
            self.pending()
        } else if self.peak == [Button::Key(Key::Escape)] {
            CaptureResult::Cancelled
        } else {
            CaptureResult::Done(Hotkey::from_buttons(self.peak.iter().copied()))
        }
    }

    fn pending(&self) -> CaptureResult {
        CaptureResult::Pending(Hotkey::from_buttons(self.peak.iter().copied()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn down(key: Key) -> InputEvent {
        InputEvent::KeyDown { key }
    }
    fn up(key: Key) -> InputEvent {
        InputEvent::KeyUp { key }
    }

    #[test]
    fn hotkey_text_round_trips_and_canonicalises() {
        let hotkey: Hotkey = "F6+ControlRight".parse().unwrap();
        assert_eq!(hotkey.to_string(), "ControlLeft+F6");
        assert_eq!(hotkey.label(), "Ctrl + F6");
        assert_eq!("".parse::<Hotkey>().unwrap(), Hotkey::none());
        assert!("Nope".parse::<Hotkey>().is_err());
    }

    #[test]
    fn fires_on_press_and_release() {
        let mut tracker = HotkeyTracker::new();
        tracker.set_bindings(vec![(Hotkey::key(Key::F6), "toggle")]);
        assert_eq!(tracker.feed(&down(Key::F6)), vec![("toggle", HotkeyEdge::Pressed)]);
        assert!(tracker.feed(&down(Key::F6)).is_empty(), "auto-repeat is ignored");
        assert_eq!(tracker.feed(&up(Key::F6)), vec![("toggle", HotkeyEdge::Released)]);
    }

    #[test]
    fn extra_held_keys_do_not_block_a_hotkey() {
        let mut tracker = HotkeyTracker::new();
        tracker.set_bindings(vec![(Hotkey::key(Key::F6), 1)]);
        tracker.feed(&down(Key::KeyW));
        tracker.feed(&down(Key::ShiftLeft));
        assert_eq!(tracker.feed(&down(Key::F6)), vec![(1, HotkeyEdge::Pressed)]);
    }

    #[test]
    fn most_specific_binding_wins() {
        let mut tracker = HotkeyTracker::new();
        tracker.set_bindings(vec![(Hotkey::key(Key::F6), "plain"), ("ControlLeft+F6".parse().unwrap(), "ctrl")]);
        tracker.feed(&down(Key::ControlRight));
        assert_eq!(tracker.feed(&down(Key::F6)), vec![("ctrl", HotkeyEdge::Pressed)]);
        assert_eq!(tracker.feed(&up(Key::ControlRight)), vec![("ctrl", HotkeyEdge::Released)]);
    }

    #[test]
    fn capture_takes_the_peak_chord() {
        let mut capture = ChordCapture::new();
        capture.feed(&down(Key::ControlLeft));
        capture.feed(&down(Key::KeyK));
        let result = capture.feed(&up(Key::KeyK));
        assert_eq!(result, CaptureResult::Done("ControlLeft+KeyK".parse().unwrap()));
    }

    #[test]
    fn capture_cancels_on_escape_and_ignores_clicks() {
        let mut capture = ChordCapture::new();
        let click = InputEvent::ButtonDown { button: MouseButton::Left };
        assert_eq!(capture.feed(&click), CaptureResult::Pending(Hotkey::none()));
        capture.feed(&down(Key::Escape));
        assert_eq!(capture.feed(&up(Key::Escape)), CaptureResult::Cancelled);
    }
}

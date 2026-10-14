//! Reading game controllers.
//!
//! Controllers are *observed* – their buttons can trigger macros and be
//! recorded. Sending controller input would need a virtual gamepad driver
//! installed on the system, which a portable app cannot provide; see
//! `docs/ARCHITECTURE.md` for the plan.

use std::time::{Duration, Instant};

use gilrs::{Axis, Button, EventType, Gilrs};
use magi_core::input::{GamepadAxis, GamepadButton, InputEvent};

use crate::Observed;

/// Blocks forever, reporting every button and axis change of every connected controller.
pub(crate) fn run(mut sink: impl FnMut(Observed)) -> Result<(), String> {
    let mut gilrs = Gilrs::new().map_err(|e| format!("gamepad support is unavailable: {e}"))?;
    loop {
        let Some(event) = gilrs.next_event_blocking(Some(Duration::from_millis(500))) else {
            continue;
        };
        let mapped = match event.event {
            EventType::ButtonPressed(button, _) => {
                map_button(button).map(|button| InputEvent::PadButtonDown { button })
            }
            EventType::ButtonReleased(button, _) => map_button(button).map(|button| InputEvent::PadButtonUp { button }),
            EventType::AxisChanged(axis, value, _) => map_axis(axis).map(|axis| InputEvent::PadAxis { axis, value }),
            // Analogue triggers report as buttons with a value.
            EventType::ButtonChanged(Button::LeftTrigger2, value, _) => {
                Some(InputEvent::PadAxis { axis: GamepadAxis::LeftTrigger, value })
            }
            EventType::ButtonChanged(Button::RightTrigger2, value, _) => {
                Some(InputEvent::PadAxis { axis: GamepadAxis::RightTrigger, value })
            }
            _ => None,
        };
        if let Some(event) = mapped {
            sink(Observed { event, at: Instant::now(), synthetic: false });
        }
    }
}

fn map_button(button: Button) -> Option<GamepadButton> {
    Some(match button {
        Button::South => GamepadButton::South,
        Button::East => GamepadButton::East,
        Button::West => GamepadButton::West,
        Button::North => GamepadButton::North,
        Button::LeftTrigger => GamepadButton::LeftBumper,
        Button::RightTrigger => GamepadButton::RightBumper,
        Button::LeftTrigger2 => GamepadButton::LeftTrigger,
        Button::RightTrigger2 => GamepadButton::RightTrigger,
        Button::Select => GamepadButton::Select,
        Button::Start => GamepadButton::Start,
        Button::Mode => GamepadButton::Guide,
        Button::LeftThumb => GamepadButton::LeftStick,
        Button::RightThumb => GamepadButton::RightStick,
        Button::DPadUp => GamepadButton::DPadUp,
        Button::DPadDown => GamepadButton::DPadDown,
        Button::DPadLeft => GamepadButton::DPadLeft,
        Button::DPadRight => GamepadButton::DPadRight,
        Button::C | Button::Z | Button::Unknown => return None,
    })
}

fn map_axis(axis: Axis) -> Option<GamepadAxis> {
    Some(match axis {
        Axis::LeftStickX => GamepadAxis::LeftStickX,
        Axis::LeftStickY => GamepadAxis::LeftStickY,
        Axis::RightStickX => GamepadAxis::RightStickX,
        Axis::RightStickY => GamepadAxis::RightStickY,
        Axis::LeftZ => GamepadAxis::LeftTrigger,
        Axis::RightZ => GamepadAxis::RightTrigger,
        Axis::DPadX | Axis::DPadY | Axis::Unknown => return None,
    })
}

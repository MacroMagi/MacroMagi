//! Small reusable widgets.

use egui::{Color32, DragValue, Response, RichText, Stroke, Ui, Vec2, vec2};
use egui_phosphor::regular as icon;
use magi_core::geom::Point;
use magi_core::hotkey::Hotkey;
use magi_core::input::Key;
use magi_core::text::{format_ms, trim_float};

use crate::theme;

/// A small dim caption above a control.
pub fn caption(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(11.5).color(theme::TEXT_DIM));
}

/// A caption with a "?" that explains the control on hover.
pub fn caption_with_help(ui: &mut Ui, text: &str, help: &str) {
    if help.is_empty() {
        caption(ui, text);
        return;
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        caption(ui, text);
        ui.label(RichText::new(icon::QUESTION).size(11.5).color(theme::TEXT_FAINT)).on_hover_text(help);
    });
}

/// A borderless button showing a single icon.
pub fn icon_button(ui: &mut Ui, glyph: &str, hint: &str) -> Response {
    ui.add(egui::Button::new(RichText::new(glyph).size(15.0)).frame(false).min_size(Vec2::splat(24.0)))
        .on_hover_text(hint)
}

/// A filled, coloured call-to-action button.
pub fn accent_button(ui: &mut Ui, text: impl Into<String>, color: Color32, min_size: Vec2) -> Response {
    // Taller buttons get proportionally larger text.
    let size = if min_size.y >= 40.0 { 15.5 } else { 13.5 };
    let text = RichText::new(text).color(Color32::WHITE).strong().size(size);
    ui.scope(|ui| {
        let widgets = &mut ui.visuals_mut().widgets;
        widgets.inactive.weak_bg_fill = theme::mix(theme::BG_WIDGET, color, 0.72);
        widgets.inactive.bg_stroke = Stroke::new(1.0, theme::mix(color, Color32::WHITE, 0.15));
        widgets.hovered.weak_bg_fill = theme::mix(theme::BG_WIDGET, color, 0.9);
        widgets.hovered.bg_stroke = Stroke::new(1.0, theme::mix(color, Color32::WHITE, 0.35));
        widgets.active.weak_bg_fill = color;
        ui.add(egui::Button::new(text).min_size(min_size))
    })
    .inner
}

/// A row of mutually exclusive choices, drawn as one joined control.
pub fn segmented<T: PartialEq + Clone>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        egui::Frame::new().fill(theme::BG_INPUT).corner_radius(theme::RADIUS + 1).inner_margin(2).show(ui, |ui| {
            for (option, label) in options {
                let selected = *value == *option;
                let text = RichText::new(*label).color(if selected { Color32::WHITE } else { theme::TEXT_DIM });
                let fill = if selected { theme::BG_WIDGET_ACTIVE } else { Color32::TRANSPARENT };
                let button = egui::Button::new(text).fill(fill).stroke(Stroke::NONE).min_size(vec2(0.0, 22.0));
                if ui.add(button).clicked() && !selected {
                    *value = option.clone();
                    changed = true;
                }
            }
        });
    });
    changed
}

/// Parses what a person types for a duration: `250`, `250ms`, `1.5s`, `2m`, `1h`,
/// or combinations such as `1m 30s`. A bare number is milliseconds.
pub fn parse_duration(text: &str) -> Option<f64> {
    let text = text.trim().to_lowercase();
    if text.is_empty() {
        return None;
    }
    let mut total = 0.0;
    let mut number = String::new();
    let mut unit = String::new();
    let mut seen_any = false;
    let mut flush = |number: &mut String, unit: &mut String| -> Option<()> {
        if number.is_empty() {
            return unit.is_empty().then_some(());
        }
        let value: f64 = number.parse().ok()?;
        let scale = match unit.as_str() {
            "" | "ms" => 1.0,
            "s" | "sec" | "secs" => 1000.0,
            "m" | "min" | "mins" => 60_000.0,
            "h" | "hr" | "hrs" => 3_600_000.0,
            _ => return None,
        };
        total += value * scale;
        number.clear();
        unit.clear();
        Some(())
    };
    for c in text.chars() {
        if c.is_ascii_digit() || c == '.' || c == ',' {
            if !unit.is_empty() {
                flush(&mut number, &mut unit)?;
            }
            number.push(if c == ',' { '.' } else { c });
            seen_any = true;
        } else if c.is_alphabetic() {
            unit.push(c);
        } else if !c.is_whitespace() {
            return None;
        }
    }
    flush(&mut number, &mut unit)?;
    seen_any.then_some(total)
}

/// A duration field. Drag to change, or click and type `1.5s`, `200ms`, `2m`…
pub fn duration_edit(ui: &mut Ui, ms: &mut f64) -> Response {
    // Dragging changes the value by about 2% per pixel, so it works for 5 ms and 5 minutes alike.
    let speed = (*ms * 0.02).max(0.5);
    ui.add(
        DragValue::new(ms)
            .speed(speed)
            .range(0.0..=86_400_000.0)
            .custom_formatter(|value, _| format_ms(value))
            .custom_parser(parse_duration),
    )
    .on_hover_text("Drag to change, or click and type e.g. 250ms, 1.5s, 2m")
}

/// A drop-down listing every key.
pub fn key_picker(ui: &mut Ui, id: &str, key: &mut Key) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(id).width(120.0).selected_text(key.label()).show_ui(ui, |ui| {
        for &option in Key::ALL {
            if ui.selectable_label(option == *key, option.label()).clicked() {
                *key = option;
                changed = true;
            }
        }
    });
    changed
}

pub enum HotkeyAction {
    None,
    /// The user wants to press a new chord.
    Capture,
    Clear,
}

/// Shows a hotkey as a button. `capturing` carries the chord pressed so far
/// while this particular hotkey is being rebound.
pub fn hotkey_button(ui: &mut Ui, hotkey: &Hotkey, capturing: Option<&Hotkey>) -> HotkeyAction {
    let mut action = HotkeyAction::None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        match capturing {
            Some(pending) => {
                let text =
                    if pending.is_none() { "Press keys…".to_owned() } else { format!("{} …", pending.label()) };
                let button = egui::Button::new(RichText::new(text).color(Color32::WHITE))
                    .fill(theme::with_alpha(theme::ACCENT, 0.45))
                    .stroke(Stroke::new(1.0, theme::ACCENT_BRIGHT))
                    .min_size(vec2(96.0, 0.0));
                ui.add(button).on_hover_text("Press the key, mouse button or gamepad button to use. Esc cancels.");
                ui.ctx().request_repaint();
            }
            None => {
                let label = if hotkey.is_none() { "Not set".to_owned() } else { hotkey.label() };
                let button = egui::Button::new(format!("{}  {label}", icon::KEYBOARD)).min_size(vec2(96.0, 0.0));
                if ui.add(button).on_hover_text("Click, then press the new hotkey").clicked() {
                    action = HotkeyAction::Capture;
                }
                if !hotkey.is_none() && icon_button(ui, icon::X, "Remove this hotkey").clicked() {
                    action = HotkeyAction::Clear;
                }
            }
        }
    });
    action
}

/// Two number fields for a screen position.
pub fn point_edit(ui: &mut Ui, point: &mut Point) -> bool {
    let x = ui.add(DragValue::new(&mut point.x).speed(1.0).prefix("X "));
    let y = ui.add(DragValue::new(&mut point.y).speed(1.0).prefix("Y "));
    x.changed() || y.changed()
}

/// Formats clicks per second for display.
pub fn format_rate(per_second: f64) -> String {
    if !per_second.is_finite() {
        "as fast as possible".to_owned()
    } else if per_second >= 1.0 {
        format!("{} clicks per second", trim_float(per_second, 1))
    } else {
        format!("one click every {}", format_ms(1000.0 / per_second))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_parse_the_way_people_type_them() {
        assert_eq!(parse_duration("250"), Some(250.0));
        assert_eq!(parse_duration("250 ms"), Some(250.0));
        assert_eq!(parse_duration("1.5s"), Some(1500.0));
        assert_eq!(parse_duration("1,5 s"), Some(1500.0));
        assert_eq!(parse_duration("2m"), Some(120_000.0));
        assert_eq!(parse_duration("1 min 30 s"), Some(90_000.0));
        assert_eq!(parse_duration("1h"), Some(3_600_000.0));
        assert_eq!(parse_duration("soon"), None);
        assert_eq!(parse_duration("5 parsecs"), None);
        assert_eq!(parse_duration(""), None);
    }

    #[test]
    fn typed_durations_round_trip_through_the_display_format() {
        for ms in [0.0, 12.5, 999.0, 1500.0, 60_000.0, 125_000.0, 3_780_000.0] {
            assert_eq!(parse_duration(&format_ms(ms)), Some(ms), "{}", format_ms(ms));
        }
    }

    #[test]
    fn rates_read_naturally() {
        assert_eq!(format_rate(10.0), "10 clicks per second");
        assert_eq!(format_rate(0.5), "one click every 2 s");
        assert_eq!(format_rate(f64::INFINITY), "as fast as possible");
    }
}

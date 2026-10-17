//! Small reusable widgets.

use egui::{Color32, DragValue, Response, RichText, Sense, Stroke, Ui, Vec2, vec2};
use egui_phosphor::regular as icon;
use magi_core::autoclicker::{MAX_DURATION_MS, TimeUnit};
use magi_core::geom::{Displays, Point, Rect};
use magi_core::hotkey::Hotkey;
use magi_core::input::{Button, Key, MouseButton};
use magi_core::text::{format_ms, trim_float};

use crate::theme;

/// A small dim caption above a control.
pub fn caption(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(11.5).color(theme::text_dim()));
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
        help_mark(ui, help);
    });
}

/// A left-aligned label in a column of fixed width, so the controls after it line up.
pub fn fixed_label(ui: &mut Ui, text: impl Into<egui::WidgetText>, width: f32) -> Response {
    let text = text.into();
    ui.allocate_ui_with_layout(vec2(width, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.set_min_width(width);
        ui.add(egui::Label::new(text).sense(Sense::click()).truncate())
    })
    .inner
}

/// A text field that takes the rest of the row after `trailing` (drawn at
/// the right end). Never asks for more width than there is, so the window
/// or panel around it cannot creep wider frame by frame.
pub fn field_with_trailing<R>(
    ui: &mut Ui,
    field: egui::TextEdit<'_>,
    trailing: impl FnOnce(&mut Ui) -> R,
) -> (Response, R) {
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let result = trailing(ui);
            let response = ui.add_sized([ui.available_width(), 22.0], field);
            (response, result)
        })
        .inner
    })
    .inner
}

/// A faint "?" that explains something on hover.
pub fn help_mark(ui: &mut Ui, help: &str) -> Response {
    ui.label(RichText::new(icon::QUESTION).size(12.0).color(theme::text_faint())).on_hover_text(help)
}

/// Faint explanatory text under a control.
pub fn note(ui: &mut Ui, text: impl Into<String>) {
    ui.add(egui::Label::new(RichText::new(text).size(11.5).color(theme::text_faint())).wrap());
}

/// A colored line of text with an icon, for warnings and problems.
pub fn notice(ui: &mut Ui, glyph: &str, text: impl Into<String>, color: Color32) {
    ui.add(egui::Label::new(RichText::new(format!("{glyph}  {}", text.into())).size(11.5).color(color)).wrap());
}

/// A borderless button showing a single icon.
pub fn icon_button(ui: &mut Ui, glyph: &str, hint: &str) -> Response {
    ui.add(egui::Button::new(RichText::new(glyph).size(15.0)).frame(false).min_size(Vec2::splat(24.0)))
        .on_hover_text(hint)
}

/// A borderless icon button that shows whether something is on.
pub fn icon_toggle(ui: &mut Ui, glyph: &str, on: bool, hint: &str) -> Response {
    let color = if on { theme::accent_bright() } else { theme::text_dim() };
    let button = egui::Button::new(RichText::new(glyph).size(15.0).color(color))
        .frame(false)
        .selected(on)
        .min_size(Vec2::splat(24.0));
    ui.add(button).on_hover_text(hint)
}

/// A filled, colored call-to-action button.
pub fn accent_button(ui: &mut Ui, text: impl Into<String>, color: Color32, min_size: Vec2) -> Response {
    // Taller buttons get proportionally larger text.
    let size = if min_size.y >= 40.0 { 15.5 } else { 13.5 };
    let text = RichText::new(text).color(theme::text_on(color)).strong().size(size);
    ui.scope(|ui| {
        let widgets = &mut ui.visuals_mut().widgets;
        widgets.inactive.weak_bg_fill = color;
        widgets.inactive.bg_stroke = Stroke::new(1.0, theme::mix(color, theme::text(), 0.15));
        widgets.hovered.weak_bg_fill = theme::mix(color, theme::text(), 0.12);
        widgets.hovered.bg_stroke = Stroke::new(1.0, theme::mix(color, theme::text(), 0.35));
        widgets.active.weak_bg_fill = theme::mix(color, theme::bg_app(), 0.2);
        ui.add(egui::Button::new(text).min_size(min_size))
    })
    .inner
}

/// A row of mutually exclusive choices, drawn as one joined control.
pub fn segmented<T: PartialEq + Clone>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        egui::Frame::new().fill(theme::bg_input()).corner_radius(theme::RADIUS + 1).inner_margin(2).show(ui, |ui| {
            for (option, label) in options {
                let selected = *value == *option;
                let (fill, text) = if selected {
                    (theme::accent(), theme::on_accent())
                } else {
                    (Color32::TRANSPARENT, theme::text_dim())
                };
                let button = egui::Button::new(RichText::new(*label).color(text))
                    .fill(fill)
                    .stroke(Stroke::NONE)
                    .min_size(vec2(0.0, 22.0));
                if ui.add(button).clicked() && !selected {
                    *value = option.clone();
                    changed = true;
                }
            }
        });
    });
    changed
}

/// An on/off switch. Returns the response; `changed()` is set when flipped.
pub fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let size = vec2(32.0, 18.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_responsive(response.id, *on);
        let track = theme::mix(theme::bg_widget_active(), theme::accent(), t);
        let painter = ui.painter();
        let radius = rect.height() / 2.0;
        painter.rect(rect, radius, track, Stroke::new(1.0, theme::stroke_strong()), egui::StrokeKind::Inside);
        let knob_x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), t);
        let knob = if *on { theme::on_accent() } else { theme::text_dim() };
        painter.circle_filled(egui::pos2(knob_x, rect.center().y), radius - 3.0, knob);
    }
    response
}

/// A label on the left and a switch on the right, with an optional hint.
pub fn toggle_row(ui: &mut Ui, label: &str, hint: &str, on: &mut bool) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let text = ui.add(egui::Label::new(label).sense(Sense::click()));
        let text = if hint.is_empty() { text } else { text.on_hover_text(hint) };
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let switch = toggle(ui, on);
            let switch = if hint.is_empty() { switch } else { switch.on_hover_text(hint) };
            changed = switch.changed();
        });
        if text.clicked() {
            *on = !*on;
            changed = true;
        }
    });
    changed
}

/// Parses what a person types for a duration: `250`, `250ms`, `1.5s`, `2m`,
/// `1h`, `3d`, or combinations such as `1m 30s`. A bare number is milliseconds.
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
            "d" | "day" | "days" => 86_400_000.0,
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
            .range(0.0..=MAX_DURATION_MS)
            .custom_formatter(|value, _| format_ms(value))
            .custom_parser(parse_duration),
    )
    .on_hover_text("Drag to change, or click and type e.g. 250ms, 1.5s, 2m")
}

/// A drop-down of time units, named in the plural ("seconds").
pub fn unit_picker(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    unit: &mut TimeUnit,
    allowed: &[TimeUnit],
) -> bool {
    unit_picker_named(ui, id, unit, allowed, TimeUnit::label)
}

/// A drop-down of time units, named in the singular ("per second").
pub fn unit_picker_singular(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    unit: &mut TimeUnit,
    allowed: &[TimeUnit],
) -> bool {
    unit_picker_named(ui, id, unit, allowed, TimeUnit::singular)
}

fn unit_picker_named(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    unit: &mut TimeUnit,
    allowed: &[TimeUnit],
    name: fn(TimeUnit) -> &'static str,
) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(id).width(46.0).selected_text(unit.short()).show_ui(ui, |ui| {
        for &option in allowed {
            if ui.selectable_label(option == *unit, name(option)).clicked() && option != *unit {
                *unit = option;
                changed = true;
            }
        }
    });
    changed
}

/// A number in a chosen unit, stored as milliseconds: `[ 1.5 ] [ s ▾ ]`.
/// Limited to `max_ms` (at most 365 days).
pub fn duration_in_unit(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug + Copy,
    ms: &mut f64,
    unit: &mut TimeUnit,
    min_ms: f64,
    max_ms: f64,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        let scale = unit.ms();
        let mut value = *ms / scale;
        let speed = (value * 0.01).max(0.01);
        let field = DragValue::new(&mut value)
            .range(min_ms / scale..=max_ms.min(MAX_DURATION_MS) / scale)
            .speed(speed)
            .min_decimals(0)
            .max_decimals(3);
        if ui.add_sized([64.0, 22.0], field).changed() {
            *ms = (value * scale).clamp(min_ms, max_ms.min(MAX_DURATION_MS));
            changed = true;
        }
        // Changing the unit keeps the duration and shows it in the new unit.
        changed |= unit_picker(ui, id, unit, &TimeUnit::ALL);
    });
    changed
}

/// A drop-down for choosing a key: searchable, with a button that takes the
/// next key pressed.
pub fn key_picker(ui: &mut Ui, id: impl std::hash::Hash + std::fmt::Debug, key: &mut Key) -> bool {
    match input_picker(ui, id, Button::Key(*key), false) {
        Some(Button::Key(picked)) => {
            *key = picked;
            true
        }
        _ => false,
    }
}

/// A drop-down for choosing a key or (with `mouse`) a mouse button. It can be
/// searched, and "Press a key…" takes the next key or button pressed while
/// MacroMagi is in front. Returns the choice when it changes.
pub fn input_picker(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    current: Button,
    mouse: bool,
) -> Option<Button> {
    let id = ui.make_persistent_id(&id);
    let (search_id, capture_id) = (id.with("search"), id.with("capture"));
    let mut picked = None;
    let response = egui::ComboBox::from_id_salt(id)
        .width(118.0)
        .height(340.0)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .selected_text(current.label())
        .show_ui(ui, |ui| {
            ui.set_min_width(200.0);
            let capturing = ui.data(|d| d.get_temp::<bool>(capture_id)).unwrap_or(false);
            if capturing {
                if let Some(button) = pressed_input(ui, mouse) {
                    picked = Some(button);
                    ui.data_mut(|d| d.insert_temp(capture_id, false));
                    ui.close();
                }
                let hint = if mouse { "Press a key or a mouse button…" } else { "Press a key…" };
                if ui
                    .button(RichText::new(hint).color(theme::accent_bright()))
                    .on_hover_text("Click to stop listening")
                    .clicked()
                    && !mouse
                {
                    ui.data_mut(|d| d.insert_temp(capture_id, false));
                }
                ui.ctx().request_repaint();
            } else if ui
                .button(format!("{}  Press a key…", icon::KEYBOARD))
                .on_hover_text(if mouse {
                    "Takes the next key or mouse button you press"
                } else {
                    "Takes the next key you press"
                })
                .clicked()
            {
                ui.data_mut(|d| d.insert_temp(capture_id, true));
            }
            let mut search: String = ui.data(|d| d.get_temp(search_id)).unwrap_or_default();
            let field = ui.add(
                egui::TextEdit::singleline(&mut search)
                    .hint_text(format!("{}  Search", icon::MAGNIFYING_GLASS))
                    .desired_width(f32::INFINITY),
            );
            if !capturing && !field.has_focus() && search.is_empty() {
                field.request_focus();
            }
            ui.data_mut(|d| d.insert_temp(search_id, search.clone()));
            let wanted = search.trim().to_lowercase();
            let matches = |label: &str, code: &str| {
                wanted.is_empty() || label.to_lowercase().contains(&wanted) || code.to_lowercase().contains(&wanted)
            };
            let options: Vec<Button> = mouse
                .then(|| MouseButton::ALL.iter().map(|b| Button::Mouse(*b)))
                .into_iter()
                .flatten()
                .chain(Key::ALL.iter().map(|k| Button::Key(*k)))
                .filter(|b| matches(&b.label(), &b.id()))
                .collect();
            egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                if options.is_empty() {
                    ui.weak("Nothing matches.");
                }
                for option in options {
                    if ui.selectable_label(option == current, option.label()).clicked() {
                        picked = Some(option);
                        ui.close();
                    }
                }
            });
        });
    if response.inner.is_none() {
        // Closed: start fresh next time.
        ui.data_mut(|d| {
            d.remove::<String>(search_id);
            d.remove::<bool>(capture_id);
        });
    }
    picked.filter(|p| *p != current)
}

/// The key (or mouse button) pressed this frame, as MacroMagi names it.
fn pressed_input(ui: &Ui, mouse: bool) -> Option<Button> {
    ui.input(|i| {
        i.events.iter().find_map(|event| match event {
            egui::Event::Key { key, physical_key, pressed: true, repeat: false, .. } => {
                key_from_egui(physical_key.unwrap_or(*key)).map(Button::Key)
            }
            egui::Event::PointerButton { button, pressed: true, .. } if mouse => {
                let button = match button {
                    egui::PointerButton::Primary => MouseButton::Left,
                    egui::PointerButton::Secondary => MouseButton::Right,
                    egui::PointerButton::Middle => MouseButton::Middle,
                    egui::PointerButton::Extra1 => MouseButton::Back,
                    egui::PointerButton::Extra2 => MouseButton::Forward,
                };
                Some(Button::Mouse(button))
            }
            _ => None,
        })
    })
}

/// egui's name for a key, as the key id MacroMagi uses (the W3C code).
fn key_from_egui(key: egui::Key) -> Option<Key> {
    let name = key.name();
    let code = match name {
        "Up" | "Down" | "Left" | "Right" => format!("Arrow{name}"),
        "OpenBracket" => "BracketLeft".into(),
        "CloseBracket" => "BracketRight".into(),
        "Backtick" => "Backquote".into(),
        "Equals" | "Plus" => "Equal".into(),
        _ if name.len() == 1 && name.chars().all(|c| c.is_ascii_uppercase()) => format!("Key{name}"),
        _ if name.len() == 1 && name.chars().all(|c| c.is_ascii_digit()) => format!("Digit{name}"),
        _ => name.to_owned(),
    };
    Key::from_id(&code)
}

pub fn mouse_button_picker(ui: &mut Ui, id: impl std::hash::Hash + std::fmt::Debug, button: &mut MouseButton) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(id).width(96.0).selected_text(button.label()).show_ui(ui, |ui| {
        for &option in MouseButton::ALL {
            if ui.selectable_label(option == *button, option.label()).clicked() {
                *button = option;
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
    /// Stop listening without changing anything.
    Cancel,
    Clear,
    /// Picked from the menu (mouse buttons, which cannot always be pressed on cue).
    Set(Hotkey),
}

/// Shows a hotkey as a button. `capturing` carries the chord pressed so far
/// while this particular hotkey is being rebound.
pub fn hotkey_button(ui: &mut Ui, hotkey: &Hotkey, capturing: Option<&Hotkey>) -> HotkeyAction {
    let mut action = HotkeyAction::None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        match capturing {
            Some(pending) => {
                let text =
                    if pending.is_none() { "Press keys…".to_owned() } else { format!("{} …", pending.label()) };
                let button = egui::Button::new(RichText::new(text).color(theme::on_accent()))
                    .fill(theme::accent())
                    .stroke(Stroke::new(1.0, theme::accent_bright()))
                    .min_size(vec2(104.0, 0.0));
                ui.add(button).on_hover_text("Press a key, a key combination, a mouse button or a gamepad button.");
                if icon_button(ui, icon::X, "Stop listening").clicked() {
                    action = HotkeyAction::Cancel;
                }
                ui.ctx().request_repaint();
            }
            None => {
                let label = if hotkey.is_none() { "Not set".to_owned() } else { hotkey.label() };
                let color = if hotkey.is_none() { theme::text_faint() } else { theme::text() };
                let button = egui::Button::new(RichText::new(format!("{}  {label}", icon::KEYBOARD)).color(color))
                    .min_size(vec2(104.0, 0.0));
                if ui.add(button).on_hover_text("Click, then press the new hotkey").clicked() {
                    action = HotkeyAction::Capture;
                }
                let menu = egui::Button::new(RichText::new(icon::MOUSE).size(14.0)).frame(false);
                egui::containers::menu::MenuButton::from_button(menu).ui(ui, |ui| {
                    ui.label(RichText::new("Use a mouse button").size(11.0).color(theme::text_faint()));
                    for &button in MouseButton::ALL {
                        if ui.button(button.label()).clicked() {
                            action = HotkeyAction::Set(Hotkey::from_buttons([Button::Mouse(button)]));
                            ui.close();
                        }
                    }
                });
                if !hotkey.is_none() && icon_button(ui, icon::X, "Remove this hotkey").clicked() {
                    action = HotkeyAction::Clear;
                }
            }
        }
    });
    action
}

/// A small "back to the default" button, drawn only while `changed`.
/// Returns true when clicked.
/// A row of buttons that continues on the next line when the window is too
/// narrow, moving whole buttons rather than breaking their labels.
pub fn button_row<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal_wrapped(|ui| {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
        add(ui)
    })
    .inner
}

/// A slim progress bar. With no share known, a band moves along it instead.
pub fn progress_bar(ui: &mut Ui, share: Option<f32>) {
    let width = ui.available_width().min(320.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, 5.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 2.5, theme::bg_widget());
    let filled = match share {
        Some(share) => egui::Rect::from_min_size(rect.min, vec2(rect.width() * share.clamp(0.0, 1.0), rect.height())),
        None => {
            let band = rect.width() * 0.3;
            let phase = (ui.input(|i| i.time) * 0.7).fract() as f32;
            let left = rect.left() - band + (rect.width() + band) * phase;
            ui.ctx().request_repaint();
            egui::Rect::from_x_y_ranges(left.max(rect.left())..=(left + band).min(rect.right()), rect.y_range())
        }
    };
    painter.rect_filled(filled, 2.5, theme::accent());
}

pub fn reset_button(ui: &mut Ui, changed: bool) -> bool {
    if !changed {
        return false;
    }
    let button = egui::Button::new(RichText::new(icon::ARROW_COUNTER_CLOCKWISE).size(12.5).color(theme::text_faint()))
        .frame(false)
        .min_size(Vec2::splat(18.0));
    ui.add(button).on_hover_text("Back to the default").clicked()
}

/// Which display a position is on: one of the displays, or (when `global` is
/// allowed) "all displays", meaning the position is on the whole desktop.
pub fn display_picker(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    display: &mut Option<u8>,
    displays: &Displays,
    global: bool,
) -> bool {
    let shown = |d: Option<u8>| match d {
        None => "All displays".to_owned(),
        Some(d) if displays.is_empty() => format!("Display {}", d + 1),
        Some(d) => displays.label(d),
    };
    let short = |d: Option<u8>| match d {
        None => format!("{}  All", icon::MONITOR),
        Some(d) => format!("{}  {}", icon::MONITOR, d + 1),
    };
    let mut changed = false;
    let count = (displays.len() as u8).max(display.map_or(1, |d| d + 1)).max(1);
    egui::ComboBox::from_id_salt(id)
        .width(58.0)
        .selected_text(short(*display))
        .show_ui(ui, |ui| {
            ui.set_min_width(220.0);
            if global
                && ui
                    .selectable_label(display.is_none(), shown(None))
                    .on_hover_text("Positions on the whole desktop")
                    .clicked()
            {
                *display = None;
                changed = true;
            }
            for d in 0..count {
                if ui.selectable_label(*display == Some(d), shown(Some(d))).clicked() && *display != Some(d) {
                    *display = Some(d);
                    changed = true;
                }
            }
        })
        .response
        .on_hover_text(format!("On {}", shown(*display)));
    changed
}

/// The same place, said relative to another display (or to the whole desktop).
pub fn point_on(point: Point, display: Option<u8>, displays: &Displays) -> Point {
    match display {
        None => displays.to_global(point),
        Some(d) => {
            let local = displays.to_local(point);
            if local.display == Some(d) { local } else { displays.move_to_display(point, d) }
        }
    }
}

/// An area moved to another display (or made global), keeping its size.
pub fn rect_on(rect: Rect, display: Option<u8>, displays: &Displays) -> Rect {
    let origin = point_on(rect.origin(), display, displays);
    Rect { x: origin.x, y: origin.y, w: rect.w, h: rect.h, display: origin.display }
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

/// A titled group of settings with a faint frame.
pub fn card(ui: &mut Ui, title: &str, body: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(theme::bg_raised())
        .stroke(Stroke::new(1.0, theme::stroke()))
        .corner_radius(theme::RADIUS_LARGE)
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !title.is_empty() {
                ui.label(RichText::new(title.to_uppercase()).size(10.5).color(theme::text_faint()));
                ui.add_space(1.0);
            }
            body(ui);
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_pressed_in_the_picker_get_macromagi_names() {
        assert_eq!(key_from_egui(egui::Key::A), Some(Key::KeyA));
        assert_eq!(key_from_egui(egui::Key::Num7), Some(Key::Digit7));
        assert_eq!(key_from_egui(egui::Key::ArrowUp), Some(Key::ArrowUp));
        assert_eq!(key_from_egui(egui::Key::F6), Some(Key::F6));
        assert_eq!(key_from_egui(egui::Key::Space), Some(Key::Space));
        assert_eq!(key_from_egui(egui::Key::OpenBracket), Some(Key::BracketLeft));
        assert_eq!(key_from_egui(egui::Key::ShiftLeft), Some(Key::ShiftLeft));
    }

    #[test]
    fn durations_parse_the_way_people_type_them() {
        assert_eq!(parse_duration("250"), Some(250.0));
        assert_eq!(parse_duration("250 ms"), Some(250.0));
        assert_eq!(parse_duration("1.5s"), Some(1500.0));
        assert_eq!(parse_duration("1,5 s"), Some(1500.0));
        assert_eq!(parse_duration("2m"), Some(120_000.0));
        assert_eq!(parse_duration("1 min 30 s"), Some(90_000.0));
        assert_eq!(parse_duration("1h"), Some(3_600_000.0));
        assert_eq!(parse_duration("2d"), Some(172_800_000.0));
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

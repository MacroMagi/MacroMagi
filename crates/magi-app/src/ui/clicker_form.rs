//! The form for [`ClickerSettings`]: what to click, how fast, for how long and
//! where. Shared by the Auto Clicker tab and the Auto Click block's settings.

use egui::{DragValue, RichText, Ui};
use egui_phosphor::regular as icon;
use magi_core::autoclicker::{
    ClickLimit, ClickPosition, ClickTarget, ClickerSettings, DualMode, HIGH_SPEED_MAX_CPS, MAX_DURATION_MS,
    NORMAL_MAX_CPS, SpeedMode, TimeUnit,
};
use magi_core::input::{Key, MouseButton};
use magi_core::text::{format_ms, trim_float};

use super::widgets::{self, card};
use super::{SnipMode, SnipTarget, UiRequest};
use crate::theme;

const LABEL_WIDTH: f32 = 112.0;

/// A label in a fixed-width column, then the controls.
fn row(ui: &mut Ui, label: &str, hint: &str, body: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        let response = column_label(ui, label);
        if !hint.is_empty() {
            response.on_hover_text(hint);
        }
        body(ui);
    });
}

fn column_label(ui: &mut Ui, label: &str) -> egui::Response {
    widgets::fixed_label(ui, RichText::new(label).color(theme::text_dim()), LABEL_WIDTH)
}

/// A label, a switch, and the controls the switch enables.
fn switch_row<T>(
    ui: &mut Ui,
    label: &str,
    hint: &str,
    toggled: &mut magi_core::autoclicker::Toggled<T>,
    body: impl FnOnce(&mut Ui, &mut T),
) {
    ui.horizontal(|ui| {
        if column_label(ui, label).on_hover_text(hint).clicked() {
            toggled.on = !toggled.on;
        }
        widgets::toggle(ui, &mut toggled.on).on_hover_text(hint);
        ui.add_space(4.0);
        ui.add_enabled_ui(toggled.on, |ui| body(ui, &mut toggled.value));
    });
}

/// A label and a switch. Returns true when flipped.
fn switch(ui: &mut Ui, label: &str, hint: &str, on: &mut bool) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        if column_label(ui, label).on_hover_text(hint).clicked() {
            *on = !*on;
            changed = true;
        }
        changed |= widgets::toggle(ui, on).on_hover_text(hint).changed();
    });
    changed
}

/// Shows the form. `id` keeps two forms on screen apart; `pick` is where a
/// position picked on screen should go.
pub fn show(ui: &mut Ui, settings: &mut ClickerSettings, id: &str, pick: SnipTarget) -> Vec<UiRequest> {
    let mut requests = Vec::new();
    ui.push_id(id, |ui| {
        card(ui, "Click", |ui| target(ui, settings));
        ui.add_space(6.0);
        card(ui, "Speed", |ui| speed(ui, settings));
        ui.add_space(6.0);
        card(ui, "Hold", |ui| hold(ui, settings));
        ui.add_space(6.0);
        card(ui, "Stop after", |ui| limits(ui, settings));
        ui.add_space(6.0);
        card(ui, "Where", |ui| position(ui, settings, pick, &mut requests));
    });
    requests
}

fn target(ui: &mut Ui, settings: &mut ClickerSettings) {
    #[derive(Clone, PartialEq)]
    enum Kind {
        Mouse,
        Key,
        Dual,
    }
    let mut kind = match settings.target {
        ClickTarget::Mouse { .. } => Kind::Mouse,
        ClickTarget::Key { .. } => Kind::Key,
        ClickTarget::DualKeys { .. } => Kind::Dual,
    };
    if widgets::segmented(ui, &mut kind, &[(Kind::Mouse, "Mouse"), (Kind::Key, "Key"), (Kind::Dual, "Two keys")]) {
        settings.target = match kind {
            Kind::Mouse => ClickTarget::Mouse { button: MouseButton::Left },
            Kind::Key => ClickTarget::Key { key: Key::Space },
            Kind::Dual => ClickTarget::DualKeys { first: Key::KeyA, second: Key::KeyD, mode: DualMode::Together },
        };
    }
    match &mut settings.target {
        ClickTarget::Mouse { button } => {
            row(ui, "Button", "", |ui| {
                widgets::mouse_button_picker(ui, "button", button);
            });
            row(ui, "Clicks", "Clicks made each time: a double-click is two quick clicks.", |ui| {
                widgets::segmented(ui, &mut settings.clicks, &[(1, "Single"), (2, "Double"), (3, "Triple")]);
            });
            if settings.clicks > 1 {
                row(ui, "Gap", "Pause between the clicks of a double or triple click.", |ui| {
                    widgets::duration_edit(ui, &mut settings.double_click_delay_ms);
                });
            }
        }
        ClickTarget::Key { key } => {
            row(ui, "Key", "", |ui| {
                widgets::key_picker(ui, "key", key);
            });
        }
        ClickTarget::DualKeys { first, second, mode } => {
            row(ui, "Keys", "", |ui| {
                widgets::key_picker(ui, "first", first);
                ui.label(RichText::new("and").color(theme::text_faint()));
                widgets::key_picker(ui, "second", second);
            });
            row(
                ui,
                "Pressed",
                "Together: both keys go down and up as one.\nAlternating: the first key, then the second, and so on.",
                |ui| {
                    widgets::segmented(
                        ui,
                        mode,
                        &[(DualMode::Together, "Together"), (DualMode::Alternate, "Alternating")],
                    );
                },
            );
        }
    }
}

fn speed(ui: &mut Ui, settings: &mut ClickerSettings) {
    row(ui, "Set as", "", |ui| {
        widgets::segmented(
            ui,
            &mut settings.speed_mode,
            &[(SpeedMode::Rate, "Clicks per time"), (SpeedMode::Interval, "Time between")],
        );
    });
    let fastest = 1000.0 / settings.max_cps();
    match settings.speed_mode {
        SpeedMode::Rate => row(ui, "Speed", "", |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let mut rate = settings.rate();
            let speed = (rate * 0.01).max(0.01);
            let field = DragValue::new(&mut rate)
                .range(0.000_001..=settings.rate_unit.ms())
                .speed(speed)
                .max_decimals(3)
                .min_decimals(0);
            if ui.add_sized([72.0, 22.0], field).changed() {
                settings.set_rate(rate);
            }
            ui.label(RichText::new("per").color(theme::text_faint()));
            let mut unit = settings.rate_unit;
            if widgets::unit_picker(ui, "rate-unit", &mut unit, &TimeUnit::ALL[1..]) {
                // Keep the number the user typed; it now means "per <new unit>".
                let rate = settings.rate();
                settings.rate_unit = unit;
                settings.set_rate(rate);
            }
        }),
        SpeedMode::Interval => row(ui, "Every", "Time from one click to the next.", |ui| {
            widgets::duration_in_unit(
                ui,
                "interval",
                &mut settings.interval_ms,
                &mut settings.interval_unit,
                fastest.min(1.0),
                MAX_DURATION_MS,
            );
        }),
    }
    let per_second = settings.clicks_per_second();
    let shown = if settings.is_capped() {
        (
            format!("{}  Limited to {} clicks per second", icon::GAUGE, trim_float(settings.max_cps(), 0)),
            theme::warning(),
        )
    } else {
        (format!("= {}", widgets::format_rate(per_second)), theme::text_faint())
    };
    ui.label(RichText::new(shown.0).size(11.5).color(shown.1));

    switch_row(
        ui,
        "Variation",
        "Make every interval randomly longer or shorter by up to this much, so the rhythm is less mechanical.",
        &mut settings.variation,
        |ui, percent| {
            ui.add(egui::Slider::new(percent, 1.0..=100.0).suffix("%").fixed_decimals(0));
        },
    );

    switch(
        ui,
        "Course correction",
        "Keep clicks on schedule: when one click comes late, the next wait is shortened, so the speed you set \
         is the speed you get.",
        &mut settings.course_correction,
    );

    high_speed(ui, settings);
}

/// The 1000-clicks-per-second switch, which asks before turning on.
fn high_speed(ui: &mut Ui, settings: &mut ClickerSettings) {
    let confirm_id = ui.id().with("confirm-high-speed");
    let mut confirming = ui.data(|d| d.get_temp::<bool>(confirm_id)).unwrap_or(false);
    let mut wanted = settings.high_speed;
    let hint = format!(
        "Allows up to {} clicks per second instead of {}.",
        trim_float(HIGH_SPEED_MAX_CPS, 0),
        trim_float(NORMAL_MAX_CPS, 0)
    );
    if switch(ui, "High-speed mode", &hint, &mut wanted) {
        if wanted {
            confirming = true;
        } else {
            settings.high_speed = false;
            confirming = false;
        }
    }
    if confirming && !settings.high_speed {
        egui::Frame::new()
            .fill(theme::mix(theme::bg_raised(), theme::warning(), 0.12))
            .stroke(egui::Stroke::new(1.0, theme::with_alpha(theme::warning(), 0.6)))
            .corner_radius(theme::RADIUS)
            .inner_margin(8)
            .show(ui, |ui| {
                widgets::notice(
                    ui,
                    icon::WARNING,
                    "Up to 1000 clicks per second can make programs, or the whole computer, stop responding, and \
                     games may treat it as cheating. Each click is released almost at once at these speeds.",
                    theme::warning(),
                );
                ui.horizontal(|ui| {
                    if widgets::accent_button(ui, "Turn on", theme::warning(), egui::vec2(80.0, 24.0)).clicked() {
                        settings.high_speed = true;
                        confirming = false;
                    }
                    if ui.button("Cancel").clicked() {
                        confirming = false;
                    }
                });
            });
    }
    ui.data_mut(|d| d.insert_temp(confirm_id, confirming));
}

fn hold(ui: &mut Ui, settings: &mut ClickerSettings) {
    ui.add_enabled_ui(!settings.duty_cycle.on, |ui| {
        row(ui, "Hold for", "How long each click stays pressed. Some games ignore clicks that are too short.", |ui| {
            widgets::duration_edit(ui, &mut settings.hold_ms);
        });
    });
    switch_row(
        ui,
        "Duty cycle",
        "Hold each click for a share of the interval instead of a fixed time. Each click picks a random share \
         between the two values.",
        &mut settings.duty_cycle,
        |ui, duty| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.add(DragValue::new(&mut duty.min).range(0.0..=100.0).suffix("%").max_decimals(0));
            ui.label(RichText::new("to").color(theme::text_faint()));
            ui.add(DragValue::new(&mut duty.max).range(0.0..=100.0).suffix("%").max_decimals(0));
        },
    );
    if settings.hold_is_clamped() {
        let shortened = settings.hold_for(settings.effective_interval_ms(), 0.5);
        widgets::notice(
            ui,
            icon::INFO,
            format!("Shortened to {} to keep up with the speed.", format_ms(shortened)),
            theme::text_faint(),
        );
    }
}

fn limits(ui: &mut Ui, settings: &mut ClickerSettings) {
    let mut limited = matches!(settings.limit, ClickLimit::Count { .. });
    let count = match settings.limit {
        ClickLimit::Count { count } => count,
        ClickLimit::Forever => 100,
    };
    let mut toggled = magi_core::autoclicker::Toggled { on: limited, value: count };
    switch_row(ui, "Click limit", "Stop after this many clicks.", &mut toggled, |ui, count| {
        ui.add(DragValue::new(count).range(1..=1_000_000_000).suffix(" clicks"));
    });
    if toggled.on != limited || (toggled.on && toggled.value != count) {
        limited = toggled.on;
        settings.limit = if limited { ClickLimit::Count { count: toggled.value } } else { ClickLimit::Forever };
    }

    let unit = &mut settings.time_limit_unit;
    switch_row(ui, "Time limit", "Stop after this long (at most 365 days).", &mut settings.time_limit, |ui, ms| {
        widgets::duration_in_unit(ui, "time-limit", ms, unit, 1.0, MAX_DURATION_MS);
    });

    let corner_hint = "Stop when the pointer comes within this many pixels of a screen corner. \
                       Throw the mouse into a corner to stop.";
    switch_row(ui, "Corner stop", corner_hint, &mut settings.corner_stop, |ui, _| {
        ui.label(RichText::new("pixels per corner:").size(11.5).color(theme::text_faint()));
    });
    if settings.corner_stop.on {
        pixel_grid(
            ui,
            "corners",
            &mut settings.corner_stop.value,
            ["Top left", "Top right", "Bottom left", "Bottom right"],
        );
    }
    let edge_hint = "Stop when the pointer comes within this many pixels of a screen edge.";
    switch_row(ui, "Edge stop", edge_hint, &mut settings.edge_stop, |ui, _| {
        ui.label(RichText::new("pixels per edge:").size(11.5).color(theme::text_faint()));
    });
    if settings.edge_stop.on {
        pixel_grid(ui, "edges", &mut settings.edge_stop.value, ["Top", "Right", "Bottom", "Left"]);
    }
}

fn pixel_grid(ui: &mut Ui, id: &str, values: &mut [u32; 4], names: [&str; 4]) {
    egui::Grid::new(id).num_columns(4).spacing([6.0, 4.0]).show(ui, |ui| {
        for (i, name) in names.iter().enumerate() {
            ui.label(RichText::new(*name).size(11.5).color(theme::text_faint()));
            ui.add(DragValue::new(&mut values[i]).range(0..=2000).suffix(" px"));
            if i % 2 == 1 {
                ui.end_row();
            }
        }
    });
}

fn position(ui: &mut Ui, settings: &mut ClickerSettings, pick: SnipTarget, requests: &mut Vec<UiRequest>) {
    let mut fixed = matches!(settings.position, ClickPosition::Fixed { .. });
    if widgets::segmented(ui, &mut fixed, &[(false, "At the pointer"), (true, "Fixed position")]) {
        settings.position = if fixed { ClickPosition::Fixed { x: 0, y: 0 } } else { ClickPosition::Cursor };
    }
    if let ClickPosition::Fixed { x, y } = &mut settings.position {
        ui.horizontal(|ui| {
            ui.add(DragValue::new(x).prefix("X "));
            ui.add(DragValue::new(y).prefix("Y "));
            if ui.button(format!("{}  Pick", icon::CROSSHAIR)).on_hover_text("Choose the position on screen").clicked()
            {
                requests.push(UiRequest::Snip(SnipMode::Point, pick));
            }
        });
    }
}

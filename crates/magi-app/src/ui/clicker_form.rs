//! The form for [`ClickerSettings`]: what to click, how fast, for how long and
//! where. Shared by the Auto Clicker tab and the Auto Click block's settings.
//!
//! Every value that differs from its default has a small reset button after
//! its controls.

use egui::{DragValue, RichText, Ui};
use egui_phosphor::regular as icon;
use magi_core::autoclicker::{
    ClickLimit, ClickPosition, ClickTarget, ClickerSettings, DualMode, HIGH_SPEED_MAX_CPS, MAX_DURATION_MS,
    NORMAL_MAX_CPS, SpeedMode, TimeUnit, Toggled,
};
use magi_core::geom::{Displays, Point};
use magi_core::input::{Key, MouseButton};
use magi_core::text::{format_ms, trim_float};

use super::widgets::{self, card};
use super::{SnipMode, SnipTarget, UiRequest};
use crate::theme;

const LABEL_WIDTH: f32 = 112.0;

/// The keys a Key or Two Keys target starts with.
const DEFAULT_KEY: Key = Key::Space;
const DEFAULT_KEYS: (Key, Key) = (Key::KeyA, Key::KeyD);

/// A label in a fixed-width column, the controls, and a reset button while
/// `changed`. Returns true when reset was clicked.
fn row(ui: &mut Ui, label: &str, hint: &str, changed: bool, body: impl FnOnce(&mut Ui)) -> bool {
    let mut reset = false;
    ui.horizontal(|ui| {
        let response = column_label(ui, label);
        if !hint.is_empty() {
            response.on_hover_text(hint);
        }
        body(ui);
        reset = widgets::reset_button(ui, changed);
    });
    reset
}

fn column_label(ui: &mut Ui, label: &str) -> egui::Response {
    widgets::fixed_label(ui, RichText::new(label).color(theme::text_dim()), LABEL_WIDTH)
}

/// A label, a switch, the controls the switch enables, and a reset button
/// that puts both the switch and the value back.
fn switch_row<T: PartialEq + Clone>(
    ui: &mut Ui,
    label: &str,
    hint: &str,
    toggled: &mut Toggled<T>,
    default: &Toggled<T>,
    body: impl FnOnce(&mut Ui, &mut T),
) {
    ui.horizontal(|ui| {
        if column_label(ui, label).on_hover_text(hint).clicked() {
            toggled.on = !toggled.on;
        }
        widgets::toggle(ui, &mut toggled.on).on_hover_text(hint);
        ui.add_space(4.0);
        ui.add_enabled_ui(toggled.on, |ui| body(ui, &mut toggled.value));
        if widgets::reset_button(ui, toggled != default) {
            *toggled = default.clone();
        }
    });
}

/// A label, a switch and a reset button. Returns true when flipped.
fn switch(ui: &mut Ui, label: &str, hint: &str, on: &mut bool, default: bool) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        if column_label(ui, label).on_hover_text(hint).clicked() {
            *on = !*on;
            changed = true;
        }
        changed |= widgets::toggle(ui, on).on_hover_text(hint).changed();
        if widgets::reset_button(ui, *on != default) {
            *on = default;
            changed = true;
        }
    });
    changed
}

/// Shows the form. `id` keeps two forms on screen apart; `pick` is where a
/// position picked on screen should go.
pub fn show(
    ui: &mut Ui,
    settings: &mut ClickerSettings,
    id: &str,
    pick: SnipTarget,
    displays: &Displays,
) -> Vec<UiRequest> {
    let mut requests = Vec::new();
    let defaults = ClickerSettings::default();
    ui.push_id(id, |ui| {
        card(ui, "Click", |ui| target(ui, settings, &defaults));
        ui.add_space(6.0);
        card(ui, "Speed", |ui| speed(ui, settings, &defaults));
        ui.add_space(6.0);
        card(ui, "Hold", |ui| hold(ui, settings, &defaults));
        ui.add_space(6.0);
        card(ui, "Stop after", |ui| limits(ui, settings, &defaults));
        // Where the pointer is only matters for mouse clicks.
        if matches!(settings.target, ClickTarget::Mouse { .. }) {
            ui.add_space(6.0);
            card(ui, "Where", |ui| position(ui, settings, pick, displays, &mut requests));
        }
    });
    requests
}

fn target(ui: &mut Ui, settings: &mut ClickerSettings, defaults: &ClickerSettings) {
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
    ui.horizontal(|ui| {
        if widgets::segmented(ui, &mut kind, &[(Kind::Mouse, "Mouse"), (Kind::Key, "Key"), (Kind::Dual, "Two keys")]) {
            settings.target = match kind {
                Kind::Mouse => ClickTarget::Mouse { button: MouseButton::Left },
                Kind::Key => ClickTarget::Key { key: DEFAULT_KEY },
                Kind::Dual => {
                    ClickTarget::DualKeys { first: DEFAULT_KEYS.0, second: DEFAULT_KEYS.1, mode: DualMode::Alternate }
                }
            };
        }
        if widgets::reset_button(ui, kind != Kind::Mouse) {
            settings.target = defaults.target;
        }
    });
    match &mut settings.target {
        ClickTarget::Mouse { button } => {
            let changed = *button != MouseButton::Left;
            if row(ui, "Button", "", changed, |ui| {
                widgets::mouse_button_picker(ui, "button", button);
            }) {
                *button = MouseButton::Left;
            }
            let clicks = &mut settings.clicks;
            if row(ui, "Clicks", "Clicks made each time: a double-click is two quick clicks.", *clicks != 1, |ui| {
                widgets::segmented(ui, clicks, &[(1, "Single"), (2, "Double"), (3, "Triple")]);
            }) {
                *clicks = defaults.clicks;
            }
            if settings.clicks > 1 {
                let gap = &mut settings.double_click_delay_ms;
                if row(
                    ui,
                    "Gap",
                    "Pause between the clicks of a double or triple click.",
                    *gap != defaults.double_click_delay_ms,
                    |ui| {
                        widgets::duration_edit(ui, gap);
                    },
                ) {
                    *gap = defaults.double_click_delay_ms;
                }
            }
        }
        ClickTarget::Key { key } => {
            if row(ui, "Key", "", *key != DEFAULT_KEY, |ui| {
                widgets::key_picker(ui, "key", key);
            }) {
                *key = DEFAULT_KEY;
            }
        }
        ClickTarget::DualKeys { first, second, mode } => {
            let changed = (*first, *second) != DEFAULT_KEYS;
            if row(ui, "Keys", "", changed, |ui| {
                widgets::key_picker(ui, "first", first);
                ui.label(RichText::new("and").color(theme::text_faint()));
                widgets::key_picker(ui, "second", second);
            }) {
                (*first, *second) = DEFAULT_KEYS;
            }
            if row(
                ui,
                "Pressed",
                "Alternating: the first key, then the second, and so on.\nTogether: both keys go down and up as one.",
                *mode != DualMode::Alternate,
                |ui| {
                    widgets::segmented(
                        ui,
                        mode,
                        &[(DualMode::Alternate, "Alternating"), (DualMode::Together, "Together")],
                    );
                },
            ) {
                *mode = DualMode::Alternate;
            }
        }
    }
}

fn speed(ui: &mut Ui, settings: &mut ClickerSettings, defaults: &ClickerSettings) {
    let mode = &mut settings.speed_mode;
    if row(ui, "Set as", "", *mode != defaults.speed_mode, |ui| {
        widgets::segmented(ui, mode, &[(SpeedMode::Rate, "Clicks per time"), (SpeedMode::Interval, "Time between")]);
    }) {
        *mode = defaults.speed_mode;
    }
    let fastest = 1000.0 / settings.max_cps();
    match settings.speed_mode {
        SpeedMode::Rate => {
            let changed = settings.interval_ms != defaults.interval_ms || settings.rate_unit != defaults.rate_unit;
            let reset = row(ui, "Speed", "", changed, |ui| {
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
                if widgets::unit_picker_singular(ui, "rate-unit", &mut unit, &TimeUnit::ALL[1..]) {
                    // Keep the number the user typed; it now means "per <new unit>".
                    let rate = settings.rate();
                    settings.rate_unit = unit;
                    settings.set_rate(rate);
                }
            });
            if reset {
                settings.interval_ms = defaults.interval_ms;
                settings.rate_unit = defaults.rate_unit;
            }
        }
        SpeedMode::Interval => {
            let changed =
                settings.interval_ms != defaults.interval_ms || settings.interval_unit != defaults.interval_unit;
            let reset = row(ui, "Every", "Time from one click to the next.", changed, |ui| {
                widgets::duration_in_unit(
                    ui,
                    "interval",
                    &mut settings.interval_ms,
                    &mut settings.interval_unit,
                    fastest.min(1.0),
                    MAX_DURATION_MS,
                );
            });
            if reset {
                settings.interval_ms = defaults.interval_ms;
                settings.interval_unit = defaults.interval_unit;
            }
        }
    }
    let per_second = settings.clicks_per_second();
    let (text, color) = match (settings.is_capped(), settings.high_speed) {
        (true, false) => (
            format!(
                "{}  Limited to {} clicks per second. Turn on High-speed mode below to go faster.",
                icon::GAUGE,
                trim_float(settings.max_cps(), 0)
            ),
            theme::warning(),
        ),
        (true, true) => (
            format!("{}  Limited to {} clicks per second", icon::GAUGE, trim_float(settings.max_cps(), 0)),
            theme::warning(),
        ),
        (false, _) => (format!("= {}", widgets::format_rate(per_second)), theme::text_faint()),
    };
    ui.add(egui::Label::new(RichText::new(text).size(11.5).color(color)).wrap());

    switch(
        ui,
        "Course correction",
        "Keep clicks on schedule: when one click comes late, the next wait is shortened, so the speed you set \
         is the speed you get.",
        &mut settings.course_correction,
        defaults.course_correction,
    );

    high_speed(ui, settings);

    switch_row(
        ui,
        "Variation",
        "Make every interval randomly longer or shorter by up to this much, so the rhythm is less mechanical.",
        &mut settings.variation,
        &defaults.variation,
        |ui, percent| {
            ui.add(egui::Slider::new(percent, 1.0..=100.0).suffix("%").fixed_decimals(0));
        },
    );
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
    if switch(ui, "High-speed mode", &hint, &mut wanted, false) {
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

fn hold(ui: &mut Ui, settings: &mut ClickerSettings, defaults: &ClickerSettings) {
    ui.add_enabled_ui(!settings.duty_cycle.on, |ui| {
        let hold = &mut settings.hold_ms;
        if row(
            ui,
            "Hold for",
            "How long each click stays pressed. Some games ignore clicks that are too short.",
            *hold != defaults.hold_ms,
            |ui| {
                widgets::duration_edit(ui, hold);
            },
        ) {
            *hold = defaults.hold_ms;
        }
    });
    switch_row(
        ui,
        "Duty cycle",
        "Hold each click for a share of the interval instead of a fixed time. Each click picks a random share \
         between the two values.",
        &mut settings.duty_cycle,
        &defaults.duty_cycle,
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

fn limits(ui: &mut Ui, settings: &mut ClickerSettings, defaults: &ClickerSettings) {
    let limited = matches!(settings.limit, ClickLimit::Count { .. });
    let count = match settings.limit {
        ClickLimit::Count { count } => count,
        ClickLimit::Forever => 100,
    };
    let mut toggled = Toggled { on: limited, value: count };
    let default = Toggled { on: matches!(defaults.limit, ClickLimit::Count { .. }), value: 100 };
    switch_row(ui, "Click limit", "Stop after this many clicks.", &mut toggled, &default, |ui, count| {
        ui.add(DragValue::new(count).range(1..=1_000_000_000).suffix(" clicks"));
    });
    if toggled.on != limited || (toggled.on && toggled.value != count) {
        settings.limit = if toggled.on { ClickLimit::Count { count: toggled.value } } else { ClickLimit::Forever };
    }

    let unit = &mut settings.time_limit_unit;
    let time_default = defaults.time_limit;
    switch_row(
        ui,
        "Time limit",
        "Stop after this long (at most 365 days).",
        &mut settings.time_limit,
        &time_default,
        |ui, ms| {
            widgets::duration_in_unit(ui, "time-limit", ms, unit, 1.0, MAX_DURATION_MS);
        },
    );

    let corner_hint = "Stop when the pointer comes within this many pixels of a screen corner. \
                       Throw the mouse into a corner to stop.";
    switch_row(ui, "Corner stop", corner_hint, &mut settings.corner_stop, &defaults.corner_stop, |ui, _| {
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
    switch_row(ui, "Edge stop", edge_hint, &mut settings.edge_stop, &defaults.edge_stop, |ui, _| {
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

fn position(
    ui: &mut Ui,
    settings: &mut ClickerSettings,
    pick: SnipTarget,
    displays: &Displays,
    requests: &mut Vec<UiRequest>,
) {
    let mut fixed = matches!(settings.position, ClickPosition::Fixed { .. });
    ui.horizontal(|ui| {
        if widgets::segmented(ui, &mut fixed, &[(false, "At the pointer"), (true, "Fixed position")]) {
            settings.position =
                if fixed { ClickPosition::Fixed { x: 0, y: 0, display: Some(0) } } else { ClickPosition::Cursor };
        }
        if widgets::reset_button(ui, fixed) {
            settings.position = ClickPosition::Cursor;
        }
    });
    if let ClickPosition::Fixed { x, y, display } = &mut settings.position {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.add(DragValue::new(x).prefix("X "));
            ui.add(DragValue::new(y).prefix("Y "));
            let mut chosen = *display;
            if widgets::display_picker(ui, "display", &mut chosen, displays, true) {
                let moved = widgets::point_on(Point { x: *x, y: *y, display: *display }, chosen, displays);
                (*x, *y, *display) = (moved.x, moved.y, moved.display);
            }
            if ui.button(format!("{}  Pick", icon::CROSSHAIR)).on_hover_text("Choose the position on screen").clicked()
            {
                requests.push(UiRequest::Snip(SnipMode::Point, pick));
            }
        });
    }
}

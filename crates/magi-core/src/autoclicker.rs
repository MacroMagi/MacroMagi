//! The Auto Clicker.
//!
//! The Auto Clicker tab is a front end, not a separate engine. Its settings
//! ([`ClickerSettings`]) are the parameters of one block, **Auto Click**, and
//! [`AutoClickerConfig::to_graph`] builds the macro `Start → Auto Click` that
//! the tab runs. "Open as macro" hands that same graph to the editor, where
//! the block can be combined with anything else.
//!
//! Older files stored a handful of flat fields (`interval_ms`, `jitter_ms`,
//! `hold_ms`, …). Those keep their names and meaning, so they still load.

use serde::{Deserialize, Serialize};

use crate::geom::Point;
use crate::graph::{Graph, PortRef};
use crate::hotkey::Hotkey;
use crate::input::{Key, MouseButton};
use crate::nodes::flow::{StartMode, StartParams};
use crate::nodes::{Registry, ids, ports};
use crate::text::{format_ms, trim_float};

/// Gap between the clicks of a double or triple click made by an Input block.
pub const MULTI_CLICK_GAP_MS: f64 = 40.0;
/// The longest interval or time limit that can be set: 365 days.
pub const MAX_DURATION_MS: f64 = 365.0 * 86_400_000.0;
/// Fastest clicking without high-speed mode.
pub const NORMAL_MAX_CPS: f64 = 100.0;
/// Fastest clicking with high-speed mode.
pub const HIGH_SPEED_MAX_CPS: f64 = 1000.0;
/// Above this speed a click is released almost as soon as it is pressed.
const VERY_FAST_CPS: f64 = 500.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeUnit {
    #[serde(rename = "ms")]
    Milliseconds,
    #[default]
    Seconds,
    Minutes,
    Hours,
    Days,
}

impl TimeUnit {
    pub const ALL: [TimeUnit; 5] =
        [TimeUnit::Milliseconds, TimeUnit::Seconds, TimeUnit::Minutes, TimeUnit::Hours, TimeUnit::Days];

    pub fn ms(self) -> f64 {
        match self {
            TimeUnit::Milliseconds => 1.0,
            TimeUnit::Seconds => 1000.0,
            TimeUnit::Minutes => 60_000.0,
            TimeUnit::Hours => 3_600_000.0,
            TimeUnit::Days => 86_400_000.0,
        }
    }

    /// Short label: `ms`, `s`, `m`, `h`, `d`.
    pub fn short(self) -> &'static str {
        match self {
            TimeUnit::Milliseconds => "ms",
            TimeUnit::Seconds => "s",
            TimeUnit::Minutes => "m",
            TimeUnit::Hours => "h",
            TimeUnit::Days => "d",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TimeUnit::Milliseconds => "milliseconds",
            TimeUnit::Seconds => "seconds",
            TimeUnit::Minutes => "minutes",
            TimeUnit::Hours => "hours",
            TimeUnit::Days => "days",
        }
    }

    /// The largest unit in which `ms` is at least 1, for showing a duration naturally.
    pub fn natural_for(ms: f64) -> TimeUnit {
        TimeUnit::ALL.into_iter().rev().find(|unit| ms >= unit.ms()).unwrap_or(TimeUnit::Milliseconds)
    }
}

/// A setting that can be switched off without forgetting its value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Toggled<T> {
    pub on: bool,
    pub value: T,
}

impl<T> Toggled<T> {
    pub const fn off(value: T) -> Self {
        Self { on: false, value }
    }

    /// The value when switched on.
    pub fn get(&self) -> Option<&T> {
        self.on.then_some(&self.value)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DualMode {
    /// Both keys go down together and come up together.
    #[default]
    Together,
    /// The first key on one click, the second on the next, and so on.
    Alternate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClickTarget {
    Mouse {
        button: MouseButton,
    },
    Key {
        key: Key,
    },
    DualKeys {
        first: Key,
        second: Key,
        #[serde(default)]
        mode: DualMode,
    },
}

impl ClickTarget {
    pub fn label(&self) -> String {
        match self {
            ClickTarget::Mouse { button } => format!("{} click", button.label()),
            ClickTarget::Key { key } => format!("Key {}", key.label()),
            ClickTarget::DualKeys { first, second, mode: DualMode::Together } => {
                format!("Keys {} + {}", first.label(), second.label())
            }
            ClickTarget::DualKeys { first, second, mode: DualMode::Alternate } => {
                format!("Keys {} / {}", first.label(), second.label())
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClickLimit {
    Forever,
    Count { count: u64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClickPosition {
    /// Wherever the pointer happens to be.
    Cursor,
    /// Move the pointer here before every click.
    Fixed { x: i32, y: i32 },
}

/// How the speed is entered. Both describe the same `interval_ms`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeedMode {
    /// "10 clicks per second".
    #[default]
    Rate,
    /// "One click every 100 ms".
    Interval,
}

/// How long the button stays down, as a share of each interval.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DutyCycle {
    /// Percent of the interval, `0..=100`. Each click picks a random value between the two.
    pub min: f64,
    pub max: f64,
}

impl Default for DutyCycle {
    fn default() -> Self {
        Self { min: 30.0, max: 50.0 }
    }
}

/// Everything about how the Auto Clicker clicks. These are the parameters of
/// the Auto Click block.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClickerSettings {
    pub target: ClickTarget,
    /// 1 = single, 2 = double, 3 = triple. Mouse targets only.
    pub clicks: u32,
    /// Pause between the clicks of a double or triple click.
    pub double_click_delay_ms: f64,

    pub speed_mode: SpeedMode,
    /// Time from the start of one click to the start of the next. The
    /// canonical speed; the rate and the units are only how it is shown.
    pub interval_ms: f64,
    pub rate_unit: TimeUnit,
    pub interval_unit: TimeUnit,
    /// Vary each interval randomly by up to this many percent.
    pub variation: Toggled<f64>,
    /// Vary each interval randomly by up to this many milliseconds (older files).
    pub jitter_ms: f64,

    /// How long each click stays pressed, unless the duty cycle is on.
    pub hold_ms: f64,
    pub duty_cycle: Toggled<DutyCycle>,

    pub limit: ClickLimit,
    /// Stop after this long.
    pub time_limit: Toggled<f64>,
    pub time_limit_unit: TimeUnit,

    pub position: ClickPosition,
    /// Stop when the pointer comes within this many pixels of a screen corner
    /// (top-left, top-right, bottom-left, bottom-right).
    pub corner_stop: Toggled<[u32; 4]>,
    /// Stop when the pointer comes within this many pixels of a screen edge
    /// (top, right, bottom, left).
    pub edge_stop: Toggled<[u32; 4]>,

    /// Keep clicks on a fixed schedule: if one click is late, the next wait is
    /// shortened, so the average speed is exactly what was asked for.
    pub course_correction: bool,
    /// Allow up to 1000 clicks per second instead of 100.
    pub high_speed: bool,
}

impl Default for ClickerSettings {
    fn default() -> Self {
        Self {
            target: ClickTarget::Mouse { button: MouseButton::Left },
            clicks: 1,
            double_click_delay_ms: MULTI_CLICK_GAP_MS,
            speed_mode: SpeedMode::Rate,
            interval_ms: 100.0,
            rate_unit: TimeUnit::Seconds,
            interval_unit: TimeUnit::Milliseconds,
            variation: Toggled::off(15.0),
            jitter_ms: 0.0,
            hold_ms: 10.0,
            duty_cycle: Toggled::off(DutyCycle::default()),
            limit: ClickLimit::Forever,
            time_limit: Toggled::off(60_000.0),
            time_limit_unit: TimeUnit::Seconds,
            position: ClickPosition::Cursor,
            corner_stop: Toggled::off([25; 4]),
            edge_stop: Toggled::off([5; 4]),
            course_correction: true,
            high_speed: false,
        }
    }
}

impl ClickerSettings {
    pub fn max_cps(&self) -> f64 {
        if self.high_speed { HIGH_SPEED_MAX_CPS } else { NORMAL_MAX_CPS }
    }

    /// The interval actually used: the setting, limited by the speed cap.
    pub fn effective_interval_ms(&self) -> f64 {
        let fastest = 1000.0 / self.max_cps();
        if self.interval_ms.is_finite() { self.interval_ms.clamp(fastest, MAX_DURATION_MS) } else { fastest }
    }

    /// Whether the setting asks for more speed than the cap allows.
    pub fn is_capped(&self) -> bool {
        self.interval_ms < 1000.0 / self.max_cps() - 1e-9
    }

    pub fn clicks_per_second(&self) -> f64 {
        f64::from(self.effective_clicks()) * 1000.0 / self.effective_interval_ms()
    }

    /// The rate in clicks per `rate_unit`.
    pub fn rate(&self) -> f64 {
        self.rate_unit.ms() / self.interval_ms.max(1e-9)
    }

    pub fn set_rate(&mut self, rate: f64) {
        if rate > 0.0 && rate.is_finite() {
            self.interval_ms = (self.rate_unit.ms() / rate).clamp(0.001, MAX_DURATION_MS);
        }
    }

    pub fn effective_clicks(&self) -> u32 {
        match self.target {
            ClickTarget::Mouse { .. } => self.clicks.clamp(1, 3),
            ClickTarget::Key { .. } | ClickTarget::DualKeys { .. } => 1,
        }
    }

    /// The interval before the next click. `random` is uniform in `0..1`.
    pub fn next_interval_ms(&self, random: f64) -> f64 {
        let base = self.effective_interval_ms();
        let spread = 2.0 * random - 1.0; // -1..1
        let mut interval = base;
        if let Some(percent) = self.variation.get() {
            interval += base * percent.clamp(0.0, 100.0) / 100.0 * spread;
        }
        if self.jitter_ms > 0.0 {
            interval += self.jitter_ms * spread;
        }
        interval.max(1000.0 / self.max_cps())
    }

    /// How long one click stays down within an interval. `random` is uniform in `0..1`.
    ///
    /// The press always ends well before the next one starts: at very high
    /// speeds the button is released almost immediately (at most 1% of the
    /// interval), otherwise at most 99% of it. Without that limit those speeds
    /// could not be reached at all.
    pub fn hold_for(&self, interval_ms: f64, random: f64) -> f64 {
        let clicks = f64::from(self.effective_clicks());
        // A double or triple click shares its interval with the pauses between its clicks.
        let budget = ((interval_ms - self.double_click_delay_ms.max(0.0) * (clicks - 1.0)) / clicks).max(0.0);
        let wanted = match self.duty_cycle.get() {
            Some(duty) => {
                let (low, high) = (duty.min.min(duty.max), duty.min.max(duty.max));
                budget * (low + (high - low) * random).clamp(0.0, 100.0) / 100.0
            }
            None => self.hold_ms.max(0.0),
        };
        let ceiling = if self.clicks_per_second() > VERY_FAST_CPS { 0.01 } else { 0.99 };
        wanted.min(budget * ceiling)
    }

    /// Whether the hold time is being shortened to reach the speed.
    pub fn hold_is_clamped(&self) -> bool {
        self.duty_cycle.get().is_none() && self.hold_for(self.effective_interval_ms(), 0.5) + 1e-9 < self.hold_ms
    }

    /// One line describing the settings, e.g. `Left click · 10 per second`.
    pub fn summary(&self) -> String {
        let per_second = self.clicks_per_second();
        let speed = if per_second >= 1.0 {
            format!("{} per second", trim_float(per_second, 1))
        } else {
            format!("every {}", format_ms(self.effective_interval_ms()))
        };
        let what = match (self.target, self.effective_clicks()) {
            (ClickTarget::Mouse { button }, 2) => format!("{} double-click", button.label()),
            (ClickTarget::Mouse { button }, 3) => format!("{} triple-click", button.label()),
            (target, _) => target.label(),
        };
        format!("{what} · {speed}")
    }
}

/// The Auto Clicker tab's state: the click settings plus how it is started.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoClickerConfig {
    #[serde(flatten)]
    pub settings: ClickerSettings,
    pub hotkey: Hotkey,
    /// `Toggle` or `Hold`.
    pub activation: StartMode,
}

impl Default for AutoClickerConfig {
    fn default() -> Self {
        Self { settings: ClickerSettings::default(), hotkey: Hotkey::key(Key::F6), activation: StartMode::Toggle }
    }
}

impl std::ops::Deref for AutoClickerConfig {
    type Target = ClickerSettings;

    fn deref(&self) -> &ClickerSettings {
        &self.settings
    }
}

impl std::ops::DerefMut for AutoClickerConfig {
    fn deref_mut(&mut self) -> &mut ClickerSettings {
        &mut self.settings
    }
}

/// Parameters of the Auto Click block.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoClickParams {
    pub clicker: ClickerSettings,
}

impl AutoClickerConfig {
    /// The macro the Auto Clicker runs: `Start → Auto Click`.
    pub fn to_graph(&self, registry: &Registry) -> Graph {
        let mut graph = Graph::new();
        let start = graph.add(registry, ids::START, [0.0, 0.0]).expect("Start is built in");
        let click = graph.add(registry, ids::AUTOCLICK, [0.0, 130.0]).expect("Auto Click is built in");
        if let Some(node) = graph.node_mut(start) {
            node.set_params(&StartParams { hotkey: self.hotkey.clone(), mode: self.activation, run_on_start: true });
        }
        if let Some(node) = graph.node_mut(click) {
            node.set_params(&AutoClickParams { clicker: self.settings.clone() });
        }
        graph
            .connect(registry, PortRef::new(start, ports::OUT), PortRef::new(click, ports::FLOW_IN))
            .expect("Start always connects to Auto Click");
        graph
    }

    /// The fixed click position, if any.
    pub fn fixed_position(&self) -> Option<Point> {
        match self.position {
            ClickPosition::Fixed { x, y } => Some(Point::new(x, y)),
            ClickPosition::Cursor => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nodes::flow::StartParams;

    #[test]
    fn graph_is_start_then_auto_click() {
        let registry = Registry::builtin();
        let settings =
            ClickerSettings { interval_ms: 250.0, limit: ClickLimit::Count { count: 7 }, ..ClickerSettings::default() };
        let config = AutoClickerConfig { settings, ..AutoClickerConfig::default() };
        let graph = config.to_graph(&registry);
        assert_eq!(graph.len(), 2);
        let start = graph.starts().next().unwrap();
        assert_eq!(start.params_as::<StartParams>().unwrap().hotkey, Hotkey::key(Key::F6));
        let click = graph.nodes().find(|n| n.kind == ids::AUTOCLICK).unwrap();
        let params: AutoClickParams = click.params_as().unwrap();
        assert_eq!(params.clicker, config.settings);
        assert_eq!(graph.flow_target(start.id, ports::OUT).unwrap().node, click.id);
    }

    #[test]
    fn files_from_the_first_version_still_load() {
        let old = r#"{"target":{"kind":"mouse","button":"Right"},"clicks":2,"interval_ms":250.0,"jitter_ms":5.0,
            "hold_ms":20.0,"limit":{"kind":"count","count":9},"position":{"kind":"fixed","x":3,"y":4},
            "hotkey":"F7","activation":"hold"}"#;
        let config: AutoClickerConfig = serde_json::from_str(old).unwrap();
        assert_eq!(config.target, ClickTarget::Mouse { button: MouseButton::Right });
        assert_eq!((config.clicks, config.interval_ms, config.jitter_ms, config.hold_ms), (2, 250.0, 5.0, 20.0));
        assert_eq!(config.limit, ClickLimit::Count { count: 9 });
        assert_eq!(config.hotkey, Hotkey::key(Key::F7));
        assert_eq!(config.activation, StartMode::Hold);
        assert!(config.course_correction, "new settings take their defaults");
    }

    #[test]
    fn config_survives_json() {
        let settings = ClickerSettings {
            target: ClickTarget::DualKeys { first: Key::KeyA, second: Key::KeyD, mode: DualMode::Alternate },
            duty_cycle: Toggled { on: true, value: DutyCycle { min: 20.0, max: 40.0 } },
            corner_stop: Toggled { on: true, value: [10; 4] },
            ..ClickerSettings::default()
        };
        let config = AutoClickerConfig { settings, ..AutoClickerConfig::default() };
        let json = serde_json::to_string(&config).unwrap();
        assert_eq!(serde_json::from_str::<AutoClickerConfig>(&json).unwrap(), config);
    }

    #[test]
    fn speed_is_capped_unless_high_speed_mode_is_on() {
        let mut settings = ClickerSettings { interval_ms: 1.0, ..ClickerSettings::default() };
        assert!(settings.is_capped());
        assert!((settings.clicks_per_second() - NORMAL_MAX_CPS).abs() < 1e-9);
        settings.high_speed = true;
        assert!(!settings.is_capped());
        assert!((settings.clicks_per_second() - HIGH_SPEED_MAX_CPS).abs() < 1e-9);
    }

    #[test]
    fn hold_never_reaches_the_next_click() {
        let mut settings = ClickerSettings { interval_ms: 20.0, hold_ms: 50.0, ..ClickerSettings::default() };
        assert!(settings.hold_for(20.0, 0.5) <= 20.0 * 0.99);
        assert!(settings.hold_is_clamped());
        settings.high_speed = true;
        settings.interval_ms = 1.0;
        assert!(settings.hold_for(1.0, 0.5) <= 0.01 + 1e-12, "at 1000/s the release is immediate");
    }

    #[test]
    fn duty_cycle_is_a_share_of_the_interval() {
        let settings = ClickerSettings {
            interval_ms: 200.0,
            duty_cycle: Toggled { on: true, value: DutyCycle { min: 20.0, max: 40.0 } },
            ..ClickerSettings::default()
        };
        assert_eq!(settings.hold_for(200.0, 0.0), 40.0);
        assert_eq!(settings.hold_for(200.0, 1.0), 80.0);
    }

    #[test]
    fn variation_spreads_intervals_around_the_speed() {
        let settings = ClickerSettings {
            interval_ms: 100.0,
            variation: Toggled { on: true, value: 35.0 },
            ..ClickerSettings::default()
        };
        assert_eq!(settings.next_interval_ms(0.5), 100.0);
        assert!((settings.next_interval_ms(0.0) - 65.0).abs() < 1e-9);
        assert!((settings.next_interval_ms(1.0) - 135.0).abs() < 1e-9);
    }

    #[test]
    fn rates_and_units_convert() {
        let mut settings = ClickerSettings { rate_unit: TimeUnit::Minutes, ..ClickerSettings::default() };
        settings.set_rate(120.0);
        assert!((settings.interval_ms - 500.0).abs() < 1e-9);
        assert!((settings.rate() - 120.0).abs() < 1e-9);
        assert_eq!(TimeUnit::natural_for(90_000.0), TimeUnit::Minutes);
        assert_eq!(TimeUnit::natural_for(5.0), TimeUnit::Milliseconds);
    }
}

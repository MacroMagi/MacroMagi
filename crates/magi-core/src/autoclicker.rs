//! The Auto Clicker.
//!
//! The Auto Clicker tab is a simple front end, not a separate engine: its
//! settings are turned into an ordinary node graph by
//! [`AutoClickerConfig::to_graph`] and run like any other macro. That is also
//! what "Open as macro" does – the same graph, handed to the editor.

use serde::{Deserialize, Serialize};

use crate::action::{Action, PressMode};
use crate::geom::Point;
use crate::graph::{Graph, PortRef};
use crate::hotkey::Hotkey;
use crate::input::{Key, MouseButton};
use crate::nodes::flow::{DelayParams, RepeatMode, RepeatParams, StartMode, StartParams};
use crate::nodes::input::ActionParams;
use crate::nodes::{Registry, ids, ports};

/// Gap between the clicks of a double or triple click.
pub const MULTI_CLICK_GAP_MS: f64 = 40.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClickTarget {
    Mouse { button: MouseButton },
    Key { key: Key },
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
    Fixed {
        x: i32,
        y: i32,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoClickerConfig {
    pub target: ClickTarget,
    /// 1 = single, 2 = double, 3 = triple. Mouse targets only.
    pub clicks: u32,
    /// Time from the start of one click to the start of the next.
    pub interval_ms: f64,
    /// Each interval varies randomly by up to this much.
    pub jitter_ms: f64,
    /// How long each click stays pressed.
    pub hold_ms: f64,
    pub limit: ClickLimit,
    pub position: ClickPosition,
    pub hotkey: Hotkey,
    /// `Toggle` or `Hold`.
    pub activation: StartMode,
}

impl Default for AutoClickerConfig {
    fn default() -> Self {
        Self {
            target: ClickTarget::Mouse { button: MouseButton::Left },
            clicks: 1,
            interval_ms: 100.0,
            jitter_ms: 0.0,
            hold_ms: 10.0,
            limit: ClickLimit::Forever,
            position: ClickPosition::Cursor,
            hotkey: Hotkey::key(Key::F6),
            activation: StartMode::Toggle,
        }
    }
}

impl AutoClickerConfig {
    /// Clicks per second this configuration aims for.
    pub fn clicks_per_second(&self) -> f64 {
        let clicks = f64::from(self.effective_clicks());
        let period = self.interval_ms.max(self.busy_ms());
        if period <= 0.0 { f64::INFINITY } else { clicks * 1000.0 / period }
    }

    fn effective_clicks(&self) -> u32 {
        match self.target {
            ClickTarget::Mouse { .. } => self.clicks.clamp(1, 3),
            ClickTarget::Key { .. } => 1,
        }
    }

    /// How long one round of clicking keeps the Input block busy.
    fn busy_ms(&self) -> f64 {
        let clicks = f64::from(self.effective_clicks());
        self.hold_ms.max(0.0) * clicks + MULTI_CLICK_GAP_MS * (clicks - 1.0)
    }

    /// The node graph that does what these settings describe:
    /// `Start → Repeat ⟲ (Input → Wait)`.
    pub fn to_graph(&self, registry: &Registry) -> Graph {
        let mut graph = Graph::new();
        let mut add = |kind: &str, y: f32| {
            graph.add(registry, kind, [0.0, y]).expect("built-in node types are always registered")
        };
        let start = add(ids::START, 0.0);
        let repeat = add(ids::REPEAT, 120.0);
        let input = add(ids::INPUT, 290.0);
        let wait = add(ids::DELAY, 470.0);

        let action = match self.target {
            ClickTarget::Mouse { button } => Action::Mouse {
                button,
                mode: PressMode::Tap,
                clicks: self.effective_clicks(),
                at: match self.position {
                    ClickPosition::Cursor => None,
                    ClickPosition::Fixed { x, y } => Some(Point::new(x, y)),
                },
            },
            ClickTarget::Key { key } => Action::tap(key),
        };

        fn node_of(graph: &mut Graph, id: u64) -> &mut crate::graph::Node {
            graph.node_mut(id).expect("node was just added")
        }
        node_of(&mut graph, start).set_params(&StartParams {
            hotkey: self.hotkey.clone(),
            mode: self.activation,
            run_on_start: true,
        });
        node_of(&mut graph, repeat).set_params(&match self.limit {
            ClickLimit::Forever => RepeatParams { mode: RepeatMode::Forever, ..RepeatParams::default() },
            ClickLimit::Count { count } => RepeatParams { mode: RepeatMode::Count, count },
        });
        node_of(&mut graph, input).set_params(&ActionParams { actions: vec![action], hold_ms: self.hold_ms.max(0.0) });
        node_of(&mut graph, wait).set_params(&DelayParams {
            // The interval is measured click-to-click, so the time the click
            // itself takes comes out of the wait.
            ms: (self.interval_ms - self.busy_ms()).max(0.0),
            jitter_ms: self.jitter_ms.max(0.0),
        });

        let mut wire = |from: (u64, &str), to: u64| {
            graph
                .connect(registry, PortRef::new(from.0, from.1), PortRef::new(to, ports::FLOW_IN))
                .expect("auto clicker wiring is always valid");
        };
        wire((start, ports::OUT), repeat);
        wire((repeat, ports::LOOP), input);
        wire((input, ports::OUT), wait);
        graph
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_matches_the_settings() {
        let registry = Registry::builtin();
        let config = AutoClickerConfig {
            interval_ms: 250.0,
            hold_ms: 20.0,
            limit: ClickLimit::Count { count: 7 },
            position: ClickPosition::Fixed { x: 300, y: 400 },
            ..AutoClickerConfig::default()
        };
        let graph = config.to_graph(&registry);
        assert_eq!(graph.len(), 4);
        assert_eq!(graph.wires().len(), 3);

        let start = graph.starts().next().unwrap();
        assert_eq!(start.params_as::<StartParams>().unwrap().hotkey, Hotkey::key(Key::F6));

        let repeat = graph.nodes().find(|n| n.kind == ids::REPEAT).unwrap();
        assert_eq!(repeat.params_as::<RepeatParams>().unwrap().iterations(), Some(7));

        let wait = graph.nodes().find(|n| n.kind == ids::DELAY).unwrap();
        assert_eq!(wait.params_as::<DelayParams>().unwrap().ms, 230.0);

        let input = graph.nodes().find(|n| n.kind == ids::INPUT).unwrap();
        let actions = input.params_as::<ActionParams>().unwrap().actions;
        assert_eq!(
            actions,
            vec![Action::Mouse {
                button: MouseButton::Left,
                mode: PressMode::Tap,
                clicks: 1,
                at: Some(Point::new(300, 400)),
            }]
        );
    }

    #[test]
    fn config_survives_json_and_tolerates_missing_fields() {
        let config = AutoClickerConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        assert_eq!(serde_json::from_str::<AutoClickerConfig>(&json).unwrap(), config);
        let partial: AutoClickerConfig = serde_json::from_str(r#"{"interval_ms": 5}"#).unwrap();
        assert_eq!(partial.interval_ms, 5.0);
        assert_eq!(partial.hotkey, Hotkey::key(Key::F6));
    }

    #[test]
    fn speed_estimate_is_sane() {
        let config = AutoClickerConfig { interval_ms: 100.0, ..AutoClickerConfig::default() };
        assert!((config.clicks_per_second() - 10.0).abs() < 1e-9);
    }
}

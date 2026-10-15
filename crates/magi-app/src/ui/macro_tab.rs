//! The Macro tab: Run and Record, the node graph, the inspector and the console.
//! File, Add, Presets and Undo/Redo live in the app's banner.

use std::collections::HashMap;

use egui::{RichText, Ui, vec2};
use egui_phosphor::regular as icon;
use magi_core::graph::NodeId;
use magi_core::hotkey::Hotkey;
use magi_core::nodes::Registry;
use magi_core::project::EnginePreference;
use magi_engine::{Activity, EngineKind, LogEntry, LogLevel, PythonStatus};

use super::graph::{GraphEnv, GraphEvent, GraphView};
use super::inspector::{self, InspectorEnv};
use super::widgets;
use super::{HotkeyTarget, UiRequest};
use crate::controller::RecordingStatus;
use crate::document::Document;
use crate::theme;

/// Below this width the inspector floats over the canvas instead of docking beside it.
const DOCK_INSPECTOR_FROM: f32 = 760.0;
const INSPECTOR_WIDTH: f32 = 300.0;
/// Below this width the toolbar shows icons without labels.
const COMPACT_TOOLBAR_BELOW: f32 = 520.0;

pub enum MacroEvent {
    Run,
    Stop,
    ToggleStart(NodeId),
    RunFrom(NodeId),
    ToggleRecording,
    Request(UiRequest),
    Toast(String),
}

pub struct MacroTab {
    pub view: GraphView,
    /// Whether the docked inspector (wide windows) is shown for the selected block.
    pub docked_inspector: bool,
    /// Whether the floating inspector (narrow windows) is open. It covers part
    /// of the canvas, so it only opens when asked for.
    pub floating_inspector: bool,
}

impl Default for MacroTab {
    fn default() -> Self {
        Self { view: GraphView::default(), docked_inspector: true, floating_inspector: false }
    }
}

/// Everything the tab shows but does not own.
pub struct MacroEnv<'a> {
    pub registry: &'a Registry,
    pub activity: &'a HashMap<NodeId, Activity>,
    pub running: &'a [NodeId],
    pub error_node: Option<NodeId>,
    pub logs: &'a [LogEntry],
    pub recording: Option<RecordingStatus>,
    pub run_hotkey: &'a Hotkey,
    pub record_hotkey: &'a Hotkey,
    pub capturing: Option<(&'a HotkeyTarget, &'a Hotkey)>,
    pub engine_running: Option<EngineKind>,
    pub engine_preference: EnginePreference,
    pub python: &'a PythonStatus,
    pub dry_run: bool,
    pub show_console: &'a mut bool,
}

/// A button label with its hotkey: `▶  Run  F7`.
fn with_hotkey(glyph: &str, word: &str, hotkey: &Hotkey, compact: bool) -> String {
    match (compact, hotkey.is_none()) {
        (true, _) => glyph.to_owned(),
        (false, true) => format!("{glyph}  {word}"),
        (false, false) => format!("{glyph}  {word}  ·  {}", hotkey.label()),
    }
}

impl MacroTab {
    /// Whether the inspector is shown for a selected block at this width.
    pub fn inspector_shown(&self, width: f32) -> bool {
        if width >= DOCK_INSPECTOR_FROM { self.docked_inspector } else { self.floating_inspector }
    }

    pub fn toggle_inspector(&mut self, width: f32) {
        if width >= DOCK_INSPECTOR_FROM {
            self.docked_inspector = !self.docked_inspector;
        } else {
            self.floating_inspector = !self.floating_inspector;
        }
    }

    pub fn show(&mut self, ui: &mut Ui, doc: &mut Document, env: &mut MacroEnv<'_>) -> Vec<MacroEvent> {
        let mut events = Vec::new();
        let total_width = ui.available_width();

        egui::Panel::top("macro-toolbar")
            .frame(egui::Frame::new().fill(theme::bg_panel()).inner_margin(egui::Margin::symmetric(10, 6)))
            .show(ui, |ui| self.toolbar(ui, doc, env, &mut events));

        if *env.show_console {
            egui::Panel::bottom("macro-console")
                .resizable(true)
                .default_size(130.0)
                .size_range(70.0..=420.0)
                .frame(egui::Frame::new().fill(theme::bg_panel()).inner_margin(egui::Margin::symmetric(10, 6)))
                .show(ui, |ui| self.console(ui, env));
        }

        let selected =
            (self.view.selection.len() == 1).then(|| *self.view.selection.iter().next().expect("one element"));
        let docked = total_width >= DOCK_INSPECTOR_FROM;
        let inspecting = selected.filter(|_| self.inspector_shown(total_width));
        if let (Some(node), true) = (inspecting, docked) {
            egui::Panel::right("macro-inspector")
                .resizable(true)
                .default_size(INSPECTOR_WIDTH)
                .size_range(240.0..=560.0)
                .frame(egui::Frame::new().fill(theme::bg_panel()).inner_margin(egui::Margin::same(12)))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        self.inspector(ui, doc, node, env, &mut events);
                    });
                });
        }

        egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, |ui| {
            let graph_env = GraphEnv {
                registry: env.registry,
                activity: env.activity,
                running: env.running,
                error_node: env.error_node,
            };
            for event in self.view.show(ui, &mut doc.project, &graph_env) {
                match event {
                    GraphEvent::ToggleStart(node) => events.push(MacroEvent::ToggleStart(node)),
                    GraphEvent::RunFrom(node) => events.push(MacroEvent::RunFrom(node)),
                    GraphEvent::Inspect(node) => {
                        self.view.focus_node(node);
                        self.docked_inspector = true;
                        self.floating_inspector = true;
                    }
                    GraphEvent::EditRecording(asset) => {
                        events.push(MacroEvent::Request(UiRequest::EditRecording(asset)))
                    }
                    GraphEvent::Toast(text) => events.push(MacroEvent::Toast(text)),
                }
            }
        });

        // In a narrow window the inspector floats, so the canvas keeps its space.
        if let (Some(node), false) = (inspecting, docked) {
            let mut open = true;
            egui::Window::new("Block settings")
                .id(egui::Id::new("macro-inspector-window"))
                .open(&mut open)
                .collapsible(false)
                .resizable(true)
                .default_width(INSPECTOR_WIDTH)
                .max_height(ui.ctx().content_rect().height() - 120.0)
                .anchor(egui::Align2::RIGHT_TOP, vec2(-10.0, 96.0))
                .show(ui.ctx(), |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        self.inspector(ui, doc, node, env, &mut events);
                    });
                });
            if !open {
                self.floating_inspector = false;
            }
        }
        events
    }

    fn inspector(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        node: NodeId,
        env: &mut MacroEnv<'_>,
        events: &mut Vec<MacroEvent>,
    ) {
        let mut inspector_env = InspectorEnv {
            registry: env.registry,
            images: &mut self.view.images,
            capturing: env.capturing,
            recording: env.recording.is_some(),
        };
        for request in inspector::show(ui, &mut doc.project, node, &mut inspector_env) {
            events.push(MacroEvent::Request(request));
        }
    }

    fn toolbar(&mut self, ui: &mut Ui, doc: &Document, env: &mut MacroEnv<'_>, events: &mut Vec<MacroEvent>) {
        // In a small window the buttons keep their icons and drop their words.
        let compact = ui.available_width() < COMPACT_TOOLBAR_BELOW;
        let width = ui.available_width();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = if compact { 4.0 } else { 6.0 };

            // Run / stop.
            let running = !env.running.is_empty();
            let (label, color) = if running {
                (with_hotkey(icon::STOP, "Stop", env.run_hotkey, compact), theme::danger())
            } else {
                (with_hotkey(icon::PLAY, "Run", env.run_hotkey, compact), theme::accent())
            };
            let hint = match (running, env.run_hotkey.is_none()) {
                (true, _) => "Stop the macro".to_owned(),
                (false, true) => {
                    "Start every Start block that responds to the Run button. No hotkey is set.".to_owned()
                }
                (false, false) => format!(
                    "Start every Start block that responds to the Run button ({} starts and stops it from anywhere)",
                    env.run_hotkey.label()
                ),
            };
            let min = if compact { 40.0 } else { 76.0 };
            if widgets::accent_button(ui, label, color, vec2(min, 28.0)).on_hover_text(hint).clicked() {
                events.push(if running { MacroEvent::Stop } else { MacroEvent::Run });
            }

            // Record.
            let record = match env.recording {
                Some(status) => {
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                    let label = format!("{}  {:.1} s · {}", icon::STOP, status.seconds, status.strokes);
                    egui::Button::new(RichText::new(label).color(theme::text_on(theme::danger()))).fill(theme::danger())
                }
                None => egui::Button::new(
                    RichText::new(with_hotkey(icon::RECORD, "Record", env.record_hotkey, compact))
                        .color(theme::danger()),
                ),
            };
            let record_hint = if env.record_hotkey.is_none() {
                "Record your keyboard, mouse and gamepad input. No hotkey is set.".to_owned()
            } else {
                format!(
                    "Record your keyboard, mouse and gamepad input ({} starts and stops)",
                    env.record_hotkey.label()
                )
            };
            if ui.add(record.min_size(vec2(0.0, 28.0))).on_hover_text(record_hint).clicked() {
                events.push(MacroEvent::ToggleRecording);
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::icon_toggle(ui, icon::TERMINAL_WINDOW, *env.show_console, "Show or hide the console")
                    .clicked()
                {
                    *env.show_console = !*env.show_console;
                }
                let shown = self.inspector_shown(width);
                if widgets::icon_toggle(
                    ui,
                    icon::SIDEBAR_SIMPLE,
                    shown,
                    "Show or hide the settings of the selected block",
                )
                .clicked()
                {
                    self.toggle_inspector(width);
                }
                engine_badge(ui, env, compact);
                if !compact {
                    let name = if doc.is_dirty() { format!("{} •", doc.title()) } else { doc.title() };
                    ui.add(egui::Label::new(RichText::new(name).color(theme::text_dim())).truncate()).on_hover_text(
                        doc.path.as_ref().map_or("Not saved yet".to_owned(), |p| p.display().to_string()),
                    );
                }
            });
        });
    }

    fn console(&mut self, ui: &mut Ui, env: &mut MacroEnv<'_>) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("CONSOLE").size(10.5).color(theme::text_faint()));
            if env.dry_run {
                ui.label(RichText::new("Dry run: no input is sent").size(11.0).color(theme::warning()));
            }
        });
        egui::ScrollArea::vertical().auto_shrink([false, false]).stick_to_bottom(true).show_rows(
            ui,
            16.0,
            env.logs.len().max(1),
            |ui, rows| {
                ui.spacing_mut().item_spacing.y = 2.0;
                if env.logs.is_empty() {
                    ui.weak("Nothing yet. Log blocks, errors and Python output appear here.");
                    return;
                }
                for entry in &env.logs[rows] {
                    let color = match entry.level {
                        LogLevel::Info => theme::text_dim(),
                        LogLevel::Warning => theme::warning(),
                        LogLevel::Error => theme::danger(),
                    };
                    let text = RichText::new(&entry.message).font(egui::FontId::monospace(12.0)).color(color);
                    match entry.node {
                        // Lines about a block select that block when clicked.
                        Some(node) => {
                            if ui
                                .add(egui::Label::new(text).sense(egui::Sense::click()).truncate())
                                .on_hover_text("Click to show the block")
                                .clicked()
                            {
                                self.view.focus_node(node);
                            }
                        }
                        None => {
                            ui.add(egui::Label::new(text).truncate());
                        }
                    }
                }
            },
        );
    }
}

/// Shows which engine runs (or would run) the macro.
fn engine_badge(ui: &mut Ui, env: &MacroEnv<'_>, compact: bool) {
    let (text, color, hint) = match (env.engine_running, env.engine_preference, env.python) {
        (Some(EngineKind::Python), _, PythonStatus::Running { version, .. }) => {
            (format!("Python {version}"), theme::success(), "This macro is running on the Python engine.".to_owned())
        }
        (Some(EngineKind::Python), ..) => {
            ("Python".to_owned(), theme::success(), "Running on the Python engine.".to_owned())
        }
        (Some(EngineKind::Native), ..) => {
            ("Native".to_owned(), theme::success(), "Running on the native (Rust) engine.".to_owned())
        }
        (None, EnginePreference::Native, _) => {
            ("Native".to_owned(), theme::text_faint(), "This macro will run on the native (Rust) engine.".to_owned())
        }
        (None, _, PythonStatus::Unavailable(reason)) => (
            "Native".to_owned(),
            theme::warning(),
            format!("Python is not available, so the native engine will be used. {reason}"),
        ),
        (None, _, PythonStatus::Running { version, .. }) => {
            (format!("Python {version}"), theme::text_faint(), "This macro will run on the Python engine.".to_owned())
        }
        (None, _, PythonStatus::Unknown) => (
            "Python".to_owned(),
            theme::text_faint(),
            "This macro will run on the Python engine if Python is installed, otherwise on the native engine."
                .to_owned(),
        ),
    };
    let label = if compact { icon::LIGHTNING.to_owned() } else { format!("{}  {text}", icon::LIGHTNING) };
    ui.label(RichText::new(label).size(11.5).color(color))
        .on_hover_text(format!("{text} engine. {hint}\nChange the engine in Settings."));
}

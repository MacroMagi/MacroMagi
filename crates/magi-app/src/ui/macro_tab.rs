//! The Macro tab: Run and Record, the node graph, the inspector, the
//! variables and the console. File, Add, Presets and Undo/Redo live in the
//! app's banner.

use std::collections::HashMap;

use egui::{RichText, Ui, vec2};
use egui_phosphor::regular as icon;
use magi_core::geom::Displays;
use magi_core::graph::NodeId;
use magi_core::hotkey::Hotkey;
use magi_core::nodes::{ParamKind, Registry};
use magi_core::project::{EnginePreference, Project};
use magi_core::recording::{MouseRecording, RecordOptions};
use magi_core::variables::{self, Variable};
use magi_engine::{Activity, EngineKind, LogEntry, LogLevel, PythonStatus};
use serde_json::{Map, Value};

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
    ClearConsole,
    /// Empty a variable's current value.
    ClearVariable(String),
    OpenEngineSettings,
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
    pub show_variables: &'a mut bool,
    /// Only Run, Stop and the console, without the editor.
    pub lightweight: &'a mut bool,
    pub show_grid: bool,
    pub show_activity: bool,
    /// What Record records.
    pub record: &'a mut RecordOptions,
    pub displays: &'a Displays,
    /// The variables' values in the latest run.
    pub live: &'a Map<String, Value>,
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
        if *env.lightweight {
            self.lightweight(ui, doc, env, &mut events);
            return events;
        }
        let total_width = ui.available_width();

        egui::Panel::top("macro-toolbar")
            .frame(egui::Frame::new().fill(theme::bg_panel()).inner_margin(egui::Margin::symmetric(10, 6)))
            .show(ui, |ui| self.toolbar(ui, doc, env, &mut events));

        if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::F)) && !ui.ctx().text_edit_focused() {
            self.view.open_find();
        }

        if *env.show_variables {
            egui::Panel::left("macro-variables")
                .resizable(true)
                .default_size(230.0)
                .size_range(180.0..=480.0)
                .frame(egui::Frame::new().fill(theme::bg_panel()).inner_margin(egui::Margin::same(10)))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        events.extend(variables_panel(
                            ui,
                            &mut doc.project,
                            env.registry,
                            env.live,
                            env.show_variables,
                        ));
                    });
                });
        }

        if *env.show_console {
            egui::Panel::bottom("macro-console")
                .resizable(true)
                .default_size(130.0)
                .size_range(70.0..=420.0)
                .frame(egui::Frame::new().fill(theme::bg_panel()).inner_margin(egui::Margin::symmetric(10, 6)))
                .show(ui, |ui| self.console(ui, env, &mut events));
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
                    // Content wider than the panel scrolls; the panel keeps the width it was given.
                    egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
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
                show_grid: env.show_grid,
                show_activity: env.show_activity,
                python_missing: python_missing(env),
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
                    GraphEvent::OpenEngineSettings => events.push(MacroEvent::OpenEngineSettings),
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
                .scroll(true)
                .default_width(INSPECTOR_WIDTH)
                .default_height(ui.ctx().content_rect().height() - 140.0)
                .default_pos(egui::pos2(ui.ctx().content_rect().right() - INSPECTOR_WIDTH - 20.0, 96.0))
                .show(ui.ctx(), |ui| {
                    self.inspector(ui, doc, node, env, &mut events);
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
            displays: env.displays,
            live: env.live,
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
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.x = 1.0;
                if ui.add(record.min_size(vec2(0.0, 28.0))).on_hover_text(record_hint).clicked() {
                    events.push(MacroEvent::ToggleRecording);
                }
                let caret = egui::Button::new(RichText::new(icon::CARET_DOWN).size(12.0)).min_size(vec2(18.0, 28.0));
                egui::containers::menu::MenuButton::from_button(caret)
                    .ui(ui, |ui| record_options(ui, env.record))
                    .0
                    .on_hover_text(format!(
                        "What Record records · movement: {}",
                        match env.record.mouse_mode {
                            MouseRecording::Smart => "smart",
                            MouseRecording::Positions => "screen positions",
                            MouseRecording::Relative => "relative motion",
                        }
                    ));
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::icon_toggle(ui, icon::TERMINAL_WINDOW, *env.show_console, "Show or hide the console")
                    .clicked()
                {
                    *env.show_console = !*env.show_console;
                }
                if widgets::icon_toggle(ui, icon::BRACKETS_CURLY, *env.show_variables, "Show or hide the variables")
                    .clicked()
                {
                    *env.show_variables = !*env.show_variables;
                }
                if widgets::icon_button(ui, icon::MAGNIFYING_GLASS, "Find blocks and groups (Ctrl+F)").clicked() {
                    self.view.open_find();
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

    /// The lightweight view: the macro's name, Run, Stop and the console, over
    /// the background, with no editor.
    fn lightweight(&mut self, ui: &mut Ui, doc: &Document, env: &mut MacroEnv<'_>, events: &mut Vec<MacroEvent>) {
        egui::CentralPanel::default().frame(egui::Frame::new().inner_margin(16)).show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.set_max_width(720.0);
                ui.add_space(6.0);
                ui.label(RichText::new(doc.title()).size(16.0).strong());
                ui.add_space(8.0);
                let running = !env.running.is_empty();
                let size = vec2(120.0, 34.0);
                ui.horizontal(|ui| {
                    ui.add_space(((ui.available_width() - 2.0 * size.x - 8.0) / 2.0).max(0.0));
                    let run = with_hotkey(icon::PLAY, "Run", env.run_hotkey, false);
                    ui.add_enabled_ui(!running, |ui| {
                        if widgets::accent_button(ui, run, theme::accent(), size).clicked() {
                            events.push(MacroEvent::Run);
                        }
                    });
                    ui.add_enabled_ui(running, |ui| {
                        if widgets::accent_button(ui, format!("{}  Stop", icon::STOP), theme::danger(), size).clicked()
                        {
                            events.push(MacroEvent::Stop);
                        }
                    });
                });
                ui.add_space(4.0);
                let state = if running { "Running" } else { "Stopped" };
                ui.label(RichText::new(state).size(11.5).color(if running {
                    theme::success()
                } else {
                    theme::text_faint()
                }));
                ui.add_space(10.0);
                egui::Frame::new()
                    .fill(theme::bg_panel())
                    .corner_radius(theme::RADIUS)
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.set_height((ui.available_height() - 44.0).max(120.0));
                        self.console(ui, env, events);
                    });
                ui.add_space(8.0);
                if ui.button(format!("{}  Show the editor", icon::TREE_STRUCTURE)).clicked() {
                    *env.lightweight = false;
                }
            });
        });
    }

    fn console(&mut self, ui: &mut Ui, env: &mut MacroEnv<'_>, events: &mut Vec<MacroEvent>) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("CONSOLE").size(10.5).color(theme::text_faint()));
            if env.dry_run {
                ui.label(RichText::new("Dry run: no input is sent").size(11.0).color(theme::warning()));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !*env.lightweight && widgets::icon_button(ui, icon::X, "Close the console").clicked() {
                    *env.show_console = false;
                }
                let clear = egui::Button::new(RichText::new(format!("{}  Clear", icon::BROOM)).size(11.0)).small();
                if ui.add_enabled(!env.logs.is_empty(), clear).on_hover_text("Empty the console").clicked() {
                    events.push(MacroEvent::ClearConsole);
                }
            });
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

/// The menu beside Record: what is recorded, and how movement is recorded.
fn record_options(ui: &mut Ui, options: &mut RecordOptions) {
    ui.set_min_width(250.0);
    ui.label(RichText::new("RECORD").size(10.5).color(theme::text_faint()));
    ui.checkbox(&mut options.keyboard, "Keyboard");
    ui.checkbox(&mut options.mouse_buttons, "Mouse buttons");
    ui.checkbox(&mut options.mouse_moves, "Mouse movement");
    ui.checkbox(&mut options.scroll, "Scroll wheel");
    ui.checkbox(&mut options.gamepad, "Gamepad");
    ui.separator();
    ui.label(RichText::new("MOVEMENT").size(10.5).color(theme::text_faint()));
    for mode in MouseRecording::ALL {
        let label = if mode == MouseRecording::Smart { "Smart Recording (recommended)" } else { mode.label() };
        ui.radio_value(&mut options.mouse_mode, mode, label).on_hover_text(match mode {
            MouseRecording::Smart => {
                "Screen positions, and relative motion where a game keeps the pointer locked, so cameras turn on \
                 playback"
            }
            MouseRecording::Positions => "Only screen positions",
            MouseRecording::Relative => "Only relative motion, for games",
        });
    }
    ui.separator();
    ui.checkbox(&mut options.trim_start, "Skip the pause before the first input");
}

/// Renames a variable everywhere it is used: settings that follow it and
/// blocks that name it.
fn rename_variable(project: &mut Project, registry: &Registry, old: &str, new: &str) {
    for node in project.graph.nodes_mut() {
        let kinds: Vec<String> = registry
            .get(&node.kind)
            .map(|def| def.params.iter().filter(|p| p.kind == ParamKind::Variable).map(|p| p.key.clone()).collect())
            .unwrap_or_default();
        for (key, value) in node.params.iter_mut() {
            if variables::bound_name(value) == Some(old) {
                *value = variables::binding(new);
            } else if kinds.contains(key) && value.as_str() == Some(old) {
                *value = Value::String(new.to_owned());
            }
        }
    }
}

/// The variables: their names, the values they start with, and the values
/// they had in the latest run.
fn variables_panel(
    ui: &mut Ui,
    project: &mut Project,
    registry: &Registry,
    live: &Map<String, Value>,
    shown: &mut bool,
) -> Vec<MacroEvent> {
    let mut events = Vec::new();
    ui.horizontal(|ui| {
        ui.label(RichText::new("VARIABLES").size(10.5).color(theme::text_faint()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::icon_button(ui, icon::X, "Close the variables").clicked() {
                *shown = false;
            }
            if widgets::icon_button(ui, icon::PLUS, "Add a variable").clicked() {
                let taken = |name: &str| project.variables.iter().any(|v| v.name == name);
                let name = (1..).map(|n| format!("variable {n}")).find(|n| !taken(n)).expect("unbounded");
                project.variables.push(Variable::kept(name));
            }
        });
    });
    widgets::note(
        ui,
        "Blocks and Python scripts read and change these while a macro runs, and they keep their value from one \
         run to the next unless set to reset. Settings follow one through their link button.",
    );
    ui.add_space(4.0);
    if project.variables.is_empty() {
        ui.weak("No variables yet.");
        return events;
    }
    let mut remove = None;
    let mut rename = None;
    for index in 0..project.variables.len() {
        ui.push_id(index, |ui| {
            egui::Frame::new().fill(theme::bg_raised()).corner_radius(theme::RADIUS).inner_margin(6).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let variable = &mut project.variables[index];
                let mut name = variable.name.clone();
                let (field, delete) = widgets::field_with_trailing(
                    ui,
                    egui::TextEdit::singleline(&mut name).font(egui::TextStyle::Monospace),
                    |ui| widgets::icon_button(ui, icon::TRASH, "Remove this variable").clicked(),
                );
                if field.changed() && !name.trim().is_empty() {
                    rename = Some((variable.name.clone(), name.trim().to_owned()));
                }
                if delete {
                    remove = Some(index);
                }
                ui.checkbox(&mut variable.reset, "Reset at the start of each run").on_hover_text(
                    "On: every run starts it at the value below. Off: it keeps whatever the last run left in it.",
                );
                if variable.reset {
                    ui.horizontal(|ui| {
                        widgets::caption(ui, "to");
                        let mut text = variables::as_text(&variable.value);
                        let field = egui::TextEdit::singleline(&mut text).desired_width(ui.available_width());
                        if ui.add(field).changed() {
                            variable.value = variables::parse_literal(&text);
                        }
                    });
                }
                if let Some(value) = live.get(&variable.name).filter(|v| !v.is_null()) {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("{}  Now {}", icon::LIGHTNING, variables::as_text(value)))
                                .size(11.5)
                                .color(theme::accent_bright()),
                        )
                        .on_hover_text("Its current value");
                        if !variable.reset
                            && ui
                                .add(egui::Button::new(RichText::new(icon::X).size(11.0)).frame(false))
                                .on_hover_text("Empty it, so the next run starts without a value")
                                .clicked()
                        {
                            events.push(MacroEvent::ClearVariable(variable.name.clone()));
                        }
                    });
                }
            });
        });
        ui.add_space(3.0);
    }
    if let Some((old, new)) = rename
        && !project.variables.iter().any(|v| v.name == new)
    {
        if let Some(variable) = project.variables.iter_mut().find(|v| v.name == old) {
            variable.name = new.clone();
        }
        rename_variable(project, registry, &old, &new);
    }
    if let Some(index) = remove {
        project.variables.remove(index);
    }
    events
}

/// Whether the macro runs, or would run, on the native engine – which skips
/// blocks only Python can run.
fn python_missing(env: &MacroEnv<'_>) -> bool {
    match (env.engine_running, env.engine_preference, env.python) {
        (Some(engine), ..) => engine == EngineKind::Native,
        (None, EnginePreference::Native, _) => true,
        (None, _, PythonStatus::Unavailable(_)) => true,
        _ => false,
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

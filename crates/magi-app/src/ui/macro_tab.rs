//! The Macro tab: toolbar, node graph, inspector and console.

use std::collections::HashMap;
use std::path::PathBuf;

use egui::{Color32, RichText, Ui, vec2};
use egui_phosphor::regular as icon;
use magi_core::graph::NodeId;
use magi_core::hotkey::Hotkey;
use magi_core::nodes::Registry;
use magi_core::preset::{Preset, PresetBody};
use magi_core::project::EnginePreference;
use magi_engine::{Activity, EngineKind, LogEntry, LogLevel, PythonStatus};

use super::graph::{GraphEnv, GraphEvent, GraphView};
use super::inspector::{self, InspectorEnv};
use super::widgets;
use super::{HotkeyTarget, UiRequest};
use crate::controller::RecordingStatus;
use crate::document::Document;
use crate::presets::PresetLibrary;
use crate::theme;

/// Below this width the inspector floats over the canvas instead of docking beside it.
const DOCK_INSPECTOR_FROM: f32 = 760.0;
const INSPECTOR_WIDTH: f32 = 300.0;
/// Below this width the toolbar shows icons without labels.
const COMPACT_TOOLBAR_BELOW: f32 = 640.0;

pub enum MacroEvent {
    Run,
    Stop,
    ToggleStart(NodeId),
    RunFrom(NodeId),
    ToggleRecording,
    New,
    Open,
    OpenPath(PathBuf),
    OpenExample(usize),
    Save,
    SaveAs,
    Undo,
    Redo,
    Request(UiRequest),
    Toast(String),
}

pub struct MacroTab {
    pub view: GraphView,
    /// Whether the docked inspector (wide windows) is shown for the selected block.
    docked_inspector: bool,
    /// Whether the floating inspector (narrow windows) is open. It covers part
    /// of the canvas, so it only opens when asked for.
    floating_inspector: bool,
    /// Name being typed for a new node preset.
    new_preset: Option<String>,
}

impl Default for MacroTab {
    fn default() -> Self {
        Self { view: GraphView::default(), docked_inspector: true, floating_inspector: false, new_preset: None }
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
    pub record_hotkey: &'a Hotkey,
    pub capturing: Option<(&'a HotkeyTarget, &'a Hotkey)>,
    pub engine_running: Option<EngineKind>,
    pub engine_preference: EnginePreference,
    pub python: &'a PythonStatus,
    pub dry_run: bool,
    pub show_console: &'a mut bool,
    pub recent_files: &'a [PathBuf],
    pub examples: &'a [&'static str],
    pub presets: &'a mut PresetLibrary,
}

impl MacroTab {
    pub fn show(&mut self, ui: &mut Ui, doc: &mut Document, env: &mut MacroEnv<'_>) -> Vec<MacroEvent> {
        let mut events = Vec::new();
        let total_width = ui.available_width();

        egui::Panel::top("macro-toolbar")
            .frame(egui::Frame::new().fill(theme::BG_PANEL).inner_margin(egui::Margin::symmetric(10, 6)))
            .show(ui, |ui| self.toolbar(ui, doc, env, &mut events));

        if *env.show_console {
            egui::Panel::bottom("macro-console")
                .resizable(true)
                .default_size(130.0)
                .size_range(70.0..=420.0)
                .frame(egui::Frame::new().fill(theme::BG_PANEL).inner_margin(egui::Margin::symmetric(10, 6)))
                .show(ui, |ui| self.console(ui, env));
        }

        let selected =
            (self.view.selection.len() == 1).then(|| *self.view.selection.iter().next().expect("one element"));
        let docked = total_width >= DOCK_INSPECTOR_FROM;
        let inspecting = selected.filter(|_| if docked { self.docked_inspector } else { self.floating_inspector });
        if let (Some(node), true) = (inspecting, docked) {
            egui::Panel::right("macro-inspector")
                .resizable(true)
                .default_size(INSPECTOR_WIDTH)
                .size_range(240.0..=520.0)
                .frame(egui::Frame::new().fill(theme::BG_PANEL).inner_margin(egui::Margin::same(12)))
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

    fn toolbar(&mut self, ui: &mut Ui, doc: &mut Document, env: &mut MacroEnv<'_>, events: &mut Vec<MacroEvent>) {
        // In a small window the buttons keep their icons and drop their words.
        let compact = ui.available_width() < COMPACT_TOOLBAR_BELOW;
        let docked = ui.available_width() >= DOCK_INSPECTOR_FROM;
        let text = |glyph: &str, word: &str| if compact { glyph.to_owned() } else { format!("{glyph}  {word}") };
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = if compact { 4.0 } else { 6.0 };

            // Run / stop.
            let running = !env.running.is_empty();
            let (label, color) = if running {
                (text(icon::STOP, "Stop"), theme::DANGER)
            } else {
                (text(icon::PLAY, "Run"), theme::ACCENT)
            };
            let hint =
                if running { "Stop the macro" } else { "Start every Start block that responds to the Run button" };
            let width = if compact { 40.0 } else { 76.0 };
            if widgets::accent_button(ui, label, color, vec2(width, 28.0)).on_hover_text(hint).clicked() {
                events.push(if running { MacroEvent::Stop } else { MacroEvent::Run });
            }

            // Record.
            let record = match env.recording {
                Some(status) => {
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                    let label = format!("{}  {:.1} s · {}", icon::STOP, status.seconds, status.strokes);
                    egui::Button::new(RichText::new(label).color(Color32::WHITE)).fill(theme::mix(
                        theme::BG_WIDGET,
                        theme::DANGER,
                        0.7,
                    ))
                }
                None => egui::Button::new(RichText::new(text(icon::RECORD, "Record")).color(theme::DANGER)),
            };
            let record_hint = if env.record_hotkey.is_none() {
                "Record your keyboard, mouse and gamepad input".to_owned()
            } else {
                format!("Record your keyboard, mouse and gamepad input ({})", env.record_hotkey.label())
            };
            if ui.add(record.min_size(vec2(0.0, 28.0))).on_hover_text(record_hint).clicked() {
                events.push(MacroEvent::ToggleRecording);
            }

            ui.separator();
            self.file_menu(ui, &text(icon::FILE, "File"), doc, env, events);
            if ui.button(text(icon::PLUS, "Add")).on_hover_text("Add a block (or double-click the canvas)").clicked() {
                let below = ui.min_rect().left_bottom() + vec2(60.0, 60.0);
                self.view.open_palette(below);
            }
            self.presets_menu(ui, &text(icon::BOOKMARK_SIMPLE, "Presets"), doc, env, events);

            if ui
                .add_enabled(doc.can_undo(), egui::Button::new(icon::ARROW_COUNTER_CLOCKWISE).frame(false))
                .on_hover_text("Undo (Ctrl+Z)")
                .clicked()
            {
                events.push(MacroEvent::Undo);
            }
            if ui
                .add_enabled(doc.can_redo(), egui::Button::new(icon::ARROW_CLOCKWISE).frame(false))
                .on_hover_text("Redo (Ctrl+Y)")
                .clicked()
            {
                events.push(MacroEvent::Redo);
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let console = egui::Button::new(icon::TERMINAL_WINDOW).frame(false).selected(*env.show_console);
                if ui.add(console).on_hover_text("Show or hide the console").clicked() {
                    *env.show_console = !*env.show_console;
                }
                let shown = if docked { &mut self.docked_inspector } else { &mut self.floating_inspector };
                let inspector = egui::Button::new(icon::SIDEBAR_SIMPLE).frame(false).selected(*shown);
                if ui.add(inspector).on_hover_text("Show or hide the settings of the selected block").clicked() {
                    *shown = !*shown;
                }
                engine_badge(ui, env, compact);
                if !compact {
                    let name = if doc.is_dirty() { format!("{} •", doc.title()) } else { doc.title() };
                    ui.add(egui::Label::new(RichText::new(name).color(theme::TEXT_DIM)).truncate()).on_hover_text(
                        doc.path.as_ref().map_or("Not saved yet".to_owned(), |p| p.display().to_string()),
                    );
                }
            });
        });
    }

    fn file_menu(
        &mut self,
        ui: &mut Ui,
        label: &str,
        doc: &Document,
        env: &MacroEnv<'_>,
        events: &mut Vec<MacroEvent>,
    ) {
        ui.menu_button(label, |ui| {
            ui.set_min_width(190.0);
            let mut item = |ui: &mut Ui, text: String, event: MacroEvent| {
                if ui.button(text).clicked() {
                    events.push(event);
                    ui.close();
                }
            };
            item(ui, format!("{}  New", icon::FILE_PLUS), MacroEvent::New);
            item(ui, format!("{}  Open…", icon::FOLDER_OPEN), MacroEvent::Open);
            item(ui, format!("{}  Save", icon::FLOPPY_DISK), MacroEvent::Save);
            item(ui, format!("{}  Save as…", icon::EXPORT), MacroEvent::SaveAs);
            ui.separator();
            ui.menu_button("Open recent", |ui| {
                if env.recent_files.is_empty() {
                    ui.weak("Nothing yet");
                }
                for path in env.recent_files {
                    let name = path
                        .file_name()
                        .map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
                    if ui.button(name).on_hover_text(path.display().to_string()).clicked() {
                        events.push(MacroEvent::OpenPath(path.clone()));
                        ui.close();
                    }
                }
            });
            ui.menu_button("Examples", |ui| {
                for (index, name) in env.examples.iter().enumerate() {
                    if ui.button(*name).clicked() {
                        events.push(MacroEvent::OpenExample(index));
                        ui.close();
                    }
                }
            });
            ui.separator();
            ui.weak(format!("{} blocks", doc.project.graph.len()));
        });
    }

    /// Node presets: save the selected blocks, or insert saved ones.
    fn presets_menu(
        &mut self,
        ui: &mut Ui,
        label: &str,
        doc: &mut Document,
        env: &mut MacroEnv<'_>,
        events: &mut Vec<MacroEvent>,
    ) {
        let mut delete = None;
        let mut insert: Option<magi_core::preset::NodePreset> = None;
        ui.menu_button(label, |ui| {
            ui.set_min_width(220.0);
            let library = env.presets.entries().iter().map(|entry| (&entry.preset, Some(&entry.path)));
            let bundled = doc.project.presets.iter().map(|preset| (preset, None));
            let mut any = false;
            for (preset, path) in library.chain(bundled) {
                let PresetBody::Nodes(nodes) = &preset.body else { continue };
                any = true;
                ui.horizontal(|ui| {
                    let label =
                        if path.is_some() { preset.name.clone() } else { format!("{}  (in this file)", preset.name) };
                    if ui.button(label).on_hover_text(format!("{} blocks", nodes.fragment.nodes.len())).clicked() {
                        insert = Some(nodes.clone());
                        ui.close();
                    }
                    if let Some(path) = path {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if widgets::icon_button(ui, icon::TRASH, "Delete this preset").clicked() {
                                delete = Some(path.clone());
                            }
                        });
                    }
                });
            }
            if !any {
                ui.weak("No block presets yet.");
            }
            ui.separator();
            let has_selection = !self.view.selection.is_empty();
            if ui
                .add_enabled(has_selection, egui::Button::new(format!("{}  Save selected blocks…", icon::FLOPPY_DISK)))
                .on_disabled_hover_text("Select one or more blocks first")
                .clicked()
            {
                self.new_preset = Some(String::new());
                ui.close();
            }
            if ui.button(format!("{}  Import a .mp file…", icon::DOWNLOAD_SIMPLE)).clicked() {
                if let Some(file) = rfd::FileDialog::new().add_filter("MacroMagi preset", &["mp"]).pick_file() {
                    events.push(MacroEvent::Toast(match env.presets.import(&file) {
                        Ok(preset) => format!("Imported “{}”", preset.name),
                        Err(error) => format!("Could not import: {error}"),
                    }));
                }
                ui.close();
            }
        });
        if let Some(path) = delete {
            env.presets.delete(&path);
        }
        if let Some(nodes) = insert {
            let at = self.view.view_center();
            self.view.insert_preset(&mut doc.project, env.registry, &nodes, at);
        }

        if let Some(name) = &mut self.new_preset {
            let mut done = false;
            let field = ui.add(egui::TextEdit::singleline(name).hint_text("Preset name").desired_width(110.0));
            let confirmed = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button("Save").clicked() || confirmed) && !name.trim().is_empty() {
                let nodes = magi_core::preset::NodePreset::capture(
                    &doc.project.graph,
                    &self.view.selection,
                    &doc.project.assets,
                    env.registry,
                );
                let preset =
                    Preset { name: name.trim().to_owned(), description: String::new(), body: PresetBody::Nodes(nodes) };
                events.push(MacroEvent::Toast(match env.presets.save(&preset) {
                    Ok(_) => format!("Saved preset “{}”", preset.name),
                    Err(error) => format!("Could not save the preset: {error}"),
                }));
                done = true;
            }
            if widgets::icon_button(ui, icon::X, "Cancel").clicked() {
                done = true;
            }
            if done {
                self.new_preset = None;
            }
        }
    }

    fn console(&mut self, ui: &mut Ui, env: &mut MacroEnv<'_>) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("CONSOLE").size(10.5).color(theme::TEXT_FAINT));
            if env.dry_run {
                ui.label(RichText::new("Dry run: no input is sent").size(11.0).color(theme::WARNING));
            }
        });
        egui::ScrollArea::vertical().auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            if env.logs.is_empty() {
                ui.weak("Nothing yet. Log blocks, errors and Python output appear here.");
            }
            for entry in env.logs {
                let color = match entry.level {
                    LogLevel::Info => theme::TEXT_DIM,
                    LogLevel::Warning => theme::WARNING,
                    LogLevel::Error => theme::DANGER,
                };
                let text = RichText::new(&entry.message).font(egui::FontId::monospace(12.0)).color(color);
                match entry.node {
                    // Lines about a block select that block when clicked.
                    Some(node) => {
                        if ui
                            .add(egui::Label::new(text).sense(egui::Sense::click()))
                            .on_hover_text("Click to show the block")
                            .clicked()
                        {
                            self.view.focus_node(node);
                        }
                    }
                    None => {
                        ui.label(text);
                    }
                }
            }
        });
    }
}

/// Shows which engine runs (or would run) the macro.
fn engine_badge(ui: &mut Ui, env: &MacroEnv<'_>, compact: bool) {
    let (text, color, hint) = match (env.engine_running, env.engine_preference, env.python) {
        (Some(EngineKind::Python), _, PythonStatus::Running { version, .. }) => {
            (format!("Python {version}"), theme::SUCCESS, "This macro is running on the Python engine.".to_owned())
        }
        (Some(EngineKind::Python), ..) => {
            ("Python".to_owned(), theme::SUCCESS, "Running on the Python engine.".to_owned())
        }
        (Some(EngineKind::Native), ..) => {
            ("Native".to_owned(), theme::SUCCESS, "Running on the native (Rust) engine.".to_owned())
        }
        (None, EnginePreference::Native, _) => {
            ("Native".to_owned(), theme::TEXT_FAINT, "This macro will run on the native (Rust) engine.".to_owned())
        }
        (None, _, PythonStatus::Unavailable(reason)) => (
            "Native".to_owned(),
            theme::WARNING,
            format!("Python is not available, so the native engine will be used. {reason}"),
        ),
        (None, _, PythonStatus::Running { version, .. }) => {
            (format!("Python {version}"), theme::TEXT_FAINT, "This macro will run on the Python engine.".to_owned())
        }
        (None, _, PythonStatus::Unknown) => (
            "Python".to_owned(),
            theme::TEXT_FAINT,
            "This macro will run on the Python engine if Python is installed, otherwise on the native engine."
                .to_owned(),
        ),
    };
    let label = if compact { icon::LIGHTNING.to_owned() } else { format!("{}  {text}", icon::LIGHTNING) };
    ui.label(RichText::new(label).size(11.5).color(color))
        .on_hover_text(format!("{text} engine. {hint}\nChange the engine in Settings."));
}

//! The application shell: owns the settings, the open document and the tabs,
//! and carries out what the UI modules ask for.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Align2, Color32, RichText, Ui, vec2};
use egui_phosphor::regular as icon;
use magi_core::action::Action;
use magi_core::autoclicker::ClickPosition;
use magi_core::geom::Point;
use magi_core::graph::NodeId;
use magi_core::hotkey::{CaptureResult, Hotkey};
use magi_core::nodes::{Registry, ids};
use magi_core::project::Project;
use magi_core::text::format_hex_color;
use magi_engine::{LogEntry, LogLevel};

use crate::controller::{Controller, ControllerConfig};
use crate::document::Document;
use crate::examples;
use crate::paths::Paths;
use crate::presets::PresetLibrary;
use crate::settings::{Settings, Tab};
use crate::theme;
use crate::ui::autoclicker::{ClickerEnv, ClickerEvent, ClickerTab};
use crate::ui::macro_tab::{MacroEnv, MacroEvent, MacroTab};
use crate::ui::settings_window::{self, SettingsEnv, SettingsEvent};
use crate::ui::snip::{Snip, SnipOutcome};
use crate::ui::{HotkeyTarget, SnipTarget, UiRequest, widgets};

const TOAST_SECONDS: f32 = 4.0;
const SETTINGS_SAVE_DELAY: Duration = Duration::from_millis(800);
const MACRO_FILTER: (&str, &[&str]) = ("MacroMagi macro", &["magi"]);

/// What to do once the user has decided about unsaved changes.
#[derive(Clone)]
enum Pending {
    New,
    Open(Option<PathBuf>),
    Example(usize),
    ClickerGraph,
    Quit,
}

struct Toast {
    text: String,
    born: Instant,
    is_error: bool,
}

/// Options for the developer screenshot mode (`--screenshot`).
pub struct Screenshot {
    pub path: PathBuf,
    /// Frames still to draw before the picture is taken, so layouts settle.
    pub frames_left: u32,
    pub requested: bool,
}

pub struct StartupOptions {
    pub open: Option<PathBuf>,
    pub example: Option<usize>,
    pub tab: Option<Tab>,
    pub screenshot: Option<PathBuf>,
    /// Open the first recording block's information card (for screenshots).
    pub show_cards: bool,
    /// Select the first block of this type (for screenshots).
    pub select_kind: Option<String>,
}

pub struct MagiApp {
    paths: Paths,
    registry: Arc<Registry>,
    controller: Controller,
    settings: Settings,
    /// The settings as last written to disk, and when they last differed.
    saved_settings: Settings,
    settings_changed: Option<Instant>,
    data_dir_error: Option<String>,

    doc: Document,
    clicker_tab: ClickerTab,
    macro_tab: MacroTab,
    presets: PresetLibrary,

    /// The hotkey being rebound, and the chord pressed so far.
    capture: Option<(HotkeyTarget, Hotkey)>,
    snip: Option<Snip>,
    /// The block a running recording should be linked to when it ends.
    record_into: Option<(NodeId, String)>,
    settings_open: bool,
    pending: Option<Pending>,
    allow_close: bool,
    toasts: Vec<Toast>,

    logs: Vec<LogEntry>,
    next_log: u64,
    error_node: Option<NodeId>,
    activity: HashMap<NodeId, magi_engine::Activity>,

    applied_window: Option<(bool, bool, f32)>,
    applied_title: String,
    screenshot: Option<Screenshot>,
}

impl MagiApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        paths: Paths,
        mut settings: Settings,
        options: StartupOptions,
    ) -> Self {
        theme::apply(&cc.egui_ctx);
        let registry = Arc::new(Registry::builtin());
        let data_dir_error = paths.ensure_data_dir().err();
        if let Some(tab) = options.tab {
            settings.tab = tab;
        }

        let controller =
            Controller::new(Arc::clone(&registry), paths.clone(), ControllerConfig::from_settings(&settings));
        let ctx = cc.egui_ctx.clone();
        controller.monitor().set_waker(move || ctx.request_repaint());
        let ctx = cc.egui_ctx.clone();
        controller.clicker_monitor().set_waker(move || ctx.request_repaint());

        let mut app = Self {
            presets: PresetLibrary::load(paths.presets_dir()),
            paths,
            doc: Document::starter(&registry),
            registry,
            controller,
            saved_settings: settings.clone(),
            settings,
            settings_changed: None,
            data_dir_error,
            clicker_tab: ClickerTab::default(),
            macro_tab: MacroTab::default(),
            capture: None,
            snip: None,
            record_into: None,
            settings_open: false,
            pending: None,
            allow_close: false,
            toasts: Vec::new(),
            logs: Vec::new(),
            next_log: 0,
            error_node: None,
            activity: HashMap::new(),
            applied_window: None,
            applied_title: String::new(),
            screenshot: options.screenshot.map(|path| Screenshot { path, frames_left: 45, requested: false }),
        };

        if let Some(path) = options.open {
            app.open_path(&path);
        } else if let Some(index) = options.example {
            app.replace_document(Document::new(examples::build(index, &app.registry), None));
        }
        if let Some(kind) = &options.select_kind
            && let Some(node) = app.doc.project.graph.nodes().find(|n| n.kind == *kind).map(|n| n.id)
        {
            app.macro_tab.view.focus_node(node);
        }
        if options.show_cards {
            let recordings: Vec<NodeId> =
                app.doc.project.graph.nodes().filter(|n| n.kind == ids::PLAY_RECORDING).map(|n| n.id).collect();
            for node in recordings {
                app.macro_tab.view.show_card(node);
            }
        }
        app
    }

    fn toast(&mut self, text: impl Into<String>) {
        self.toasts.push(Toast { text: text.into(), born: Instant::now(), is_error: false });
    }

    fn toast_error(&mut self, text: impl Into<String>) {
        self.toasts.push(Toast { text: text.into(), born: Instant::now(), is_error: true });
    }

    // ---- documents --------------------------------------------------------

    fn replace_document(&mut self, doc: Document) {
        self.controller.stop_macro();
        self.doc = doc;
        self.macro_tab = MacroTab::default();
        self.error_node = None;
        self.settings.tab = Tab::Macro;
    }

    fn open_path(&mut self, path: &Path) {
        match Project::load(path) {
            Ok(project) => {
                self.replace_document(Document::new(project, Some(path.to_path_buf())));
                self.settings.remember_file(path);
            }
            Err(error) => self.toast_error(format!("Could not open {}: {error}", path.display())),
        }
    }

    /// Saves the document. Returns false if the user cancelled or it failed.
    fn save(&mut self, choose_path: bool) -> bool {
        let path = match (&self.doc.path, choose_path) {
            (Some(path), false) => path.clone(),
            _ => {
                let dialog = rfd::FileDialog::new()
                    .add_filter(MACRO_FILTER.0, MACRO_FILTER.1)
                    .set_file_name(format!("{}.magi", self.doc.title()));
                match dialog.save_file() {
                    Some(path) => path,
                    None => return false,
                }
            }
        };
        if self.settings.include_presets {
            // Bundle the library, keeping presets that already travel with this file.
            for preset in self.presets.presets() {
                if !self.doc.project.presets.iter().any(|bundled| bundled.name == preset.name) {
                    self.doc.project.presets.push(preset.clone());
                }
            }
        }
        if self.doc.project.meta.name == Project::new().meta.name
            && let Some(stem) = path.file_stem()
        {
            self.doc.project.meta.name = stem.to_string_lossy().into_owned();
        }
        match self.doc.project.save(&path) {
            Ok(()) => {
                self.settings.remember_file(&path);
                self.doc.mark_saved(path);
                true
            }
            Err(error) => {
                self.toast_error(format!("Could not save: {error}"));
                false
            }
        }
    }

    /// Runs an action that replaces the document, asking first if there are unsaved changes.
    fn guarded(&mut self, action: Pending) {
        if self.doc.is_dirty() {
            self.pending = Some(action);
        } else {
            self.perform(action);
        }
    }

    fn perform(&mut self, action: Pending) {
        match action {
            Pending::New => self.replace_document(Document::starter(&self.registry)),
            Pending::Open(Some(path)) => self.open_path(&path),
            Pending::Open(None) => {
                if let Some(path) = rfd::FileDialog::new().add_filter(MACRO_FILTER.0, MACRO_FILTER.1).pick_file() {
                    self.open_path(&path);
                }
            }
            Pending::Example(index) => {
                self.replace_document(Document::new(examples::build(index, &self.registry), None));
            }
            Pending::ClickerGraph => {
                let mut project = Project::new();
                project.meta.name = "Auto clicker".into();
                project.graph = self.settings.clicker.to_graph(&self.registry);
                self.replace_document(Document::new(project, None));
            }
            Pending::Quit => self.allow_close = true,
        }
    }

    // ---- requests from the UI ---------------------------------------------

    fn handle_request(&mut self, ctx: &egui::Context, request: UiRequest) {
        match request {
            UiRequest::Snip(mode, target) => match Snip::begin(ctx, mode, target) {
                Ok(snip) => self.snip = Some(snip),
                Err(error) => self.toast_error(error),
            },
            UiRequest::CaptureHotkey(target) => {
                self.controller.begin_capture();
                self.capture = Some((target, Hotkey::none()));
            }
            UiRequest::LoadImage(node, key) => {
                let dialog = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "bmp", "webp", "gif"]);
                let Some(path) = dialog.pick_file() else { return };
                let loaded = std::fs::read(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|bytes| magi_vision::decode(&bytes).map_err(|e| e.to_string()))
                    .and_then(|bitmap| magi_vision::encode_png(bitmap.as_ref()).map_err(|e| e.to_string()));
                match loaded {
                    Ok(png) => {
                        let name =
                            path.file_stem().map_or_else(|| "Image".to_owned(), |s| s.to_string_lossy().into_owned());
                        let id = self.doc.project.assets.add_image(name, png);
                        if let Some(node) = self.doc.project.graph.node_mut(node) {
                            node.set_param(&key, id);
                        }
                    }
                    Err(error) => self.toast_error(format!("Could not load the image: {error}")),
                }
            }
            UiRequest::RecordInto(node, key) => {
                if self.controller.recording_status().is_none() {
                    self.record_into = Some((node, key));
                }
                self.controller.toggle_recording();
            }
        }
    }

    fn apply_snip(&mut self, target: SnipTarget, outcome: SnipOutcome) {
        let set = |app: &mut Self, node: NodeId, key: &str, value: serde_json::Value| {
            if let Some(node) = app.doc.project.graph.node_mut(node) {
                node.params.insert(key.to_owned(), value);
            }
        };
        match (target, outcome) {
            (SnipTarget::NodeParam(node, key), SnipOutcome::Point(point)) => {
                set(self, node, &key, serde_json::json!(point))
            }
            (SnipTarget::NodeParam(node, key), SnipOutcome::Region(region)) => {
                set(self, node, &key, serde_json::json!(region))
            }
            (SnipTarget::NodeParam(node, key), SnipOutcome::Color(rgb)) => {
                set(self, node, &key, serde_json::json!(format_hex_color(rgb)));
            }
            (SnipTarget::NodeParam(node, key), SnipOutcome::Image { png }) => {
                let count = self.doc.project.assets.of_kind(magi_core::assets::AssetKind::Image).count();
                let id = self.doc.project.assets.add_image(format!("Image {}", count + 1), png);
                set(self, node, &key, serde_json::json!(id));
            }
            (SnipTarget::Action(node, index), SnipOutcome::Point(point)) => {
                let Some(node) = self.doc.project.graph.node_mut(node) else { return };
                let mut actions: Vec<Action> = node.param("actions").unwrap_or_default();
                match actions.get_mut(index) {
                    Some(Action::Mouse { at, .. }) => *at = Some(point),
                    Some(Action::Move { x, y, relative, .. }) => (*x, *y, *relative) = (point.x, point.y, false),
                    _ => return,
                }
                node.set_param("actions", actions);
            }
            (SnipTarget::ClickerPosition, SnipOutcome::Point(Point { x, y })) => {
                self.settings.clicker.position = ClickPosition::Fixed { x, y };
            }
            _ => {}
        }
    }

    fn apply_hotkey(&mut self, target: HotkeyTarget, hotkey: Hotkey) {
        match target {
            HotkeyTarget::Clicker => self.settings.clicker.hotkey = hotkey,
            HotkeyTarget::StopEverything => self.settings.stop_hotkey = hotkey,
            HotkeyTarget::Record => self.settings.record_hotkey = hotkey,
            HotkeyTarget::Node(node, key) => {
                if let Some(node) = self.doc.project.graph.node_mut(node) {
                    node.set_param(&key, hotkey);
                }
            }
        }
    }

    // ---- per-frame bookkeeping (runs even while the window is hidden) -------

    fn tick(&mut self, ctx: &egui::Context) {
        // Recordings that ended (possibly by hotkey, with the window minimised).
        for recording in self.controller.take_recordings() {
            let count = self.doc.project.assets.of_kind(magi_core::assets::AssetKind::Recording).count();
            let name = format!("Recording {}", count + 1);
            let summary = recording.summary();
            let id = self.doc.project.assets.add_recording(name.clone(), recording);
            match self.record_into.take() {
                Some((node, key)) if self.doc.project.graph.contains(node) => {
                    if let Some(node) = self.doc.project.graph.node_mut(node) {
                        node.set_param(&key, &id);
                    }
                }
                _ if self.settings.auto_insert_recording => {
                    let at = self.macro_tab.view.free_spot(&self.doc.project);
                    if let Some(node) =
                        self.macro_tab.view.add_node(&mut self.doc.project, &self.registry, ids::PLAY_RECORDING, at)
                    {
                        if let Some(target) = self.doc.project.graph.node_mut(node) {
                            target.set_param("recording", &id);
                        }
                        self.macro_tab.view.show_card(node);
                    }
                    self.settings.tab = Tab::Macro;
                }
                _ => {}
            }
            self.toast(format!("{name} saved: {summary}"));
        }

        // Hotkey capture.
        if let Some((target, pending)) = &mut self.capture {
            match self.controller.poll_capture() {
                Some(CaptureResult::Pending(chord)) => *pending = chord,
                Some(CaptureResult::Done(hotkey)) => {
                    let target = target.clone();
                    self.capture = None;
                    self.apply_hotkey(target, hotkey);
                }
                Some(CaptureResult::Cancelled) => self.capture = None,
                None => {}
            }
        }

        // Hand the latest macro and settings to the controller.
        let settled = !ctx.input(|i| i.pointer.any_down()) && !ctx.text_edit_focused();
        self.doc.track(settled);
        self.controller.publish(self.doc.revision(), &self.doc.project);
        self.controller.set_config(ControllerConfig::from_settings(&self.settings));

        // Console.
        let fresh = self.controller.monitor().logs_from(self.next_log);
        if let Some(last) = fresh.last() {
            self.next_log = last.seq + 1;
            for entry in &fresh {
                if entry.level == LogLevel::Error {
                    self.error_node = entry.node;
                    self.settings.show_console = true;
                    self.toasts.push(Toast { text: entry.message.clone(), born: Instant::now(), is_error: true });
                }
            }
            self.logs.extend(fresh);
            let excess = self.logs.len().saturating_sub(1000);
            self.logs.drain(..excess);
        }
        self.activity = self.controller.monitor().activity();

        // Settings are written a moment after they stop changing.
        if self.settings != self.saved_settings {
            let since = *self.settings_changed.get_or_insert_with(Instant::now);
            if since.elapsed() >= SETTINGS_SAVE_DELAY {
                self.save_settings();
            } else {
                ctx.request_repaint_after(SETTINGS_SAVE_DELAY);
            }
        }
    }

    fn save_settings(&mut self) {
        self.settings_changed = None;
        self.saved_settings = self.settings.clone();
        if self.data_dir_error.is_none()
            && let Err(error) = self.settings.save(&self.paths.settings_file())
        {
            self.data_dir_error = Some(format!("Settings could not be saved: {error}"));
        }
    }

    fn apply_window_state(&mut self, ctx: &egui::Context) {
        let wanted = (self.settings.always_on_top, self.settings.hide_from_capture, self.settings.ui_scale);
        if self.applied_window != Some(wanted) {
            self.applied_window = Some(wanted);
            let level = if wanted.0 { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal };
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
            ctx.send_viewport_cmd(egui::ViewportCommand::ContentProtected(wanted.1));
            ctx.set_zoom_factor(wanted.2.clamp(0.75, 1.75));
        }
        let title = match self.settings.tab {
            Tab::AutoClicker => "MacroMagi".to_owned(),
            Tab::Macro => format!("{}{} – MacroMagi", self.doc.title(), if self.doc.is_dirty() { " •" } else { "" }),
        };
        if title != self.applied_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.applied_title = title;
        }
    }

    // ---- drawing ------------------------------------------------------------

    fn top_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.label(RichText::new(icon::MAGIC_WAND).size(19.0).color(theme::ACCENT_BRIGHT));
            if ui.available_width() > 400.0 {
                ui.label(RichText::new("MacroMagi").strong().size(15.0));
            }
            ui.add_space(4.0);
            widgets::segmented(
                ui,
                &mut self.settings.tab,
                &[(Tab::AutoClicker, "Auto Clicker"), (Tab::Macro, "Macro")],
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if widgets::icon_button(ui, icon::GEAR, "Settings").clicked() {
                    self.settings_open = !self.settings_open;
                }
                let pinned = self.settings.always_on_top;
                let pin = RichText::new(icon::PUSH_PIN).size(15.0).color(if pinned {
                    theme::ACCENT_BRIGHT
                } else {
                    theme::TEXT_DIM
                });
                if ui.add(egui::Button::new(pin).frame(false)).on_hover_text("Keep on top of other windows").clicked() {
                    self.settings.always_on_top = !pinned;
                }
                if self.controller.anything_running() {
                    let label = if self.settings.stop_hotkey.is_none() {
                        format!("{}  Stop all", icon::STOP)
                    } else {
                        format!("{}  Stop all ({})", icon::STOP, self.settings.stop_hotkey.label())
                    };
                    let stop = egui::Button::new(RichText::new(label).size(12.0).color(theme::DANGER)).frame(false);
                    if ui.add(stop).on_hover_text("Stop the auto clicker and every macro").clicked() {
                        self.controller.stop_everything();
                    }
                }
                if let Some(status) = self.controller.recording_status() {
                    ui.label(
                        RichText::new(format!("{}  REC {:.0}s", icon::RECORD, status.seconds))
                            .size(12.0)
                            .color(theme::DANGER),
                    );
                    ui.ctx().request_repaint_after(Duration::from_millis(250));
                }
            });
        });
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        let problem = self.data_dir_error.clone().or_else(|| {
            self.controller.listener_error().map(|e| format!("Hotkeys and recording are unavailable: {e}"))
        });
        let Some(problem) = problem else { return };
        egui::Panel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(theme::mix(theme::BG_PANEL, theme::WARNING, 0.12))
                    .inner_margin(egui::Margin::symmetric(10, 5)),
            )
            .show(ui, |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(format!("{}  {problem}", icon::WARNING)).size(11.5).color(theme::WARNING),
                    )
                    .wrap(),
                );
            });
    }

    fn clicker(&mut self, ui: &mut Ui) {
        let monitor = self.controller.clicker_monitor();
        // Node 3 of the Auto Clicker's graph is its Input block.
        let rounds = monitor.activity_of(3).map_or(0, |activity| activity.count);
        let error = monitor.logs().into_iter().rev().find(|e| e.level == LogLevel::Error).map(|e| e.message);
        let running = self.controller.clicker_running();
        let clicks_per_round = u64::from(match self.settings.clicker.target {
            magi_core::autoclicker::ClickTarget::Mouse { .. } => self.settings.clicker.clicks.clamp(1, 3),
            magi_core::autoclicker::ClickTarget::Key { .. } => 1,
        });
        let mut env = ClickerEnv {
            running,
            clicks: rounds * clicks_per_round,
            error,
            capturing: self.capture.as_ref().map(|(target, pending)| (target, pending)),
            presets: &mut self.presets,
        };
        let events = self.clicker_tab.show(ui, &mut self.settings.clicker, &mut env);
        if running {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
        for event in events {
            match event {
                ClickerEvent::Toggle => {
                    // Publish first, so a setting changed this very frame is used.
                    self.controller.set_config(ControllerConfig::from_settings(&self.settings));
                    self.controller.clicker_monitor().clear_logs();
                    self.controller.toggle_clicker();
                }
                ClickerEvent::OpenAsMacro => self.guarded(Pending::ClickerGraph),
                ClickerEvent::Request(request) => self.handle_request(ui.ctx(), request),
                ClickerEvent::Toast(text) => self.toast(text),
            }
        }
    }

    fn macro_editor(&mut self, ui: &mut Ui) {
        let running = self.controller.running_nodes();
        let python = self.controller.python_status();
        let mut env = MacroEnv {
            registry: &self.registry,
            activity: &self.activity,
            running: &running,
            error_node: self.error_node,
            logs: &self.logs,
            recording: self.controller.recording_status(),
            record_hotkey: &self.settings.record_hotkey,
            capturing: self.capture.as_ref().map(|(target, pending)| (target, pending)),
            engine_running: (!running.is_empty()).then(|| self.controller.macro_engine()).flatten(),
            engine_preference: self.controller.preferred_engine(),
            python: &python,
            dry_run: self.settings.dry_run,
            show_console: &mut self.settings.show_console,
            recent_files: &self.settings.recent_files,
            examples: &examples::NAMES,
            presets: &mut self.presets,
        };
        let events = self.macro_tab.show(ui, &mut self.doc, &mut env);
        for event in events {
            match event {
                MacroEvent::Run => {
                    self.error_node = None;
                    self.sync_controller(ui.ctx());
                    self.controller.run_macro();
                }
                MacroEvent::Stop => self.controller.stop_macro(),
                MacroEvent::ToggleStart(node) => {
                    if running.contains(&node) {
                        self.controller.stop_node(node);
                    } else {
                        self.error_node = None;
                        self.sync_controller(ui.ctx());
                        self.controller.run_from(node);
                    }
                }
                MacroEvent::RunFrom(node) => {
                    self.error_node = None;
                    self.sync_controller(ui.ctx());
                    self.controller.run_from(node);
                }
                MacroEvent::ToggleRecording => self.controller.toggle_recording(),
                MacroEvent::New => self.guarded(Pending::New),
                MacroEvent::Open => self.guarded(Pending::Open(None)),
                MacroEvent::OpenPath(path) => self.guarded(Pending::Open(Some(path))),
                MacroEvent::OpenExample(index) => self.guarded(Pending::Example(index)),
                MacroEvent::Save => {
                    self.save(false);
                }
                MacroEvent::SaveAs => {
                    self.save(true);
                }
                MacroEvent::Undo => self.doc.undo(),
                MacroEvent::Redo => self.doc.redo(),
                MacroEvent::Request(request) => self.handle_request(ui.ctx(), request),
                MacroEvent::Toast(text) => self.toast(text),
            }
        }
    }

    /// Makes sure the controller has this frame's edits before something is started.
    fn sync_controller(&mut self, _ctx: &egui::Context) {
        self.doc.track(false);
        self.controller.publish(self.doc.revision(), &self.doc.project);
        self.controller.set_config(ControllerConfig::from_settings(&self.settings));
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.text_edit_focused() || self.settings.tab != Tab::Macro {
            return;
        }
        let (save, save_as, open, new, undo, redo) = ctx.input(|i| {
            let cmd = i.modifiers.command;
            (
                cmd && !i.modifiers.shift && i.key_pressed(egui::Key::S),
                cmd && i.modifiers.shift && i.key_pressed(egui::Key::S),
                cmd && i.key_pressed(egui::Key::O),
                cmd && i.key_pressed(egui::Key::N),
                cmd && !i.modifiers.shift && i.key_pressed(egui::Key::Z),
                cmd && (i.key_pressed(egui::Key::Y) || (i.modifiers.shift && i.key_pressed(egui::Key::Z))),
            )
        });
        if save {
            self.save(false);
        }
        if save_as {
            self.save(true);
        }
        if open {
            self.guarded(Pending::Open(None));
        }
        if new {
            self.guarded(Pending::New);
        }
        if undo {
            self.doc.undo();
        }
        if redo {
            self.doc.redo();
        }
    }

    fn windows(&mut self, ctx: &egui::Context) {
        // Settings.
        if self.settings_open {
            let mut open = true;
            let python = self.controller.python_status();
            let mut events = Vec::new();
            egui::Window::new(format!("{}  Settings", icon::GEAR))
                .id(egui::Id::new("settings-window"))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .default_width(330.0)
                .max_height(ctx.content_rect().height() - 40.0)
                .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        let env = SettingsEnv {
                            capturing: self.capture.as_ref().map(|(target, pending)| (target, pending)),
                            python: &python,
                            paths: &self.paths,
                        };
                        events = settings_window::show(ui, &mut self.settings, &env);
                    });
                });
            self.settings_open = open;
            for event in events {
                match event {
                    SettingsEvent::Request(request) => self.handle_request(ctx, request),
                    SettingsEvent::CheckPython => {
                        self.controller.set_config(ControllerConfig::from_settings(&self.settings));
                        // Starting Python can take a second; do it off the UI thread.
                        let (controller, ctx) = (self.controller.clone(), ctx.clone());
                        std::thread::spawn(move || {
                            controller.probe_python();
                            ctx.request_repaint();
                        });
                    }
                }
            }
        }

        // Unsaved changes.
        if let Some(action) = self.pending.clone() {
            let mut decision = None;
            egui::Modal::new(egui::Id::new("unsaved-changes")).show(ctx, |ui| {
                ui.set_width(300.0);
                ui.label(RichText::new("Save your changes?").strong().size(15.0));
                ui.label(format!("“{}” has changes that have not been saved.", self.doc.title()));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if widgets::accent_button(ui, "Save", theme::ACCENT, vec2(70.0, 26.0)).clicked() {
                        decision = Some(Some(true));
                    }
                    if ui.button("Don't save").clicked() {
                        decision = Some(Some(false));
                    }
                    if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        decision = Some(None);
                    }
                });
            });
            match decision {
                Some(Some(true)) => {
                    self.pending = None;
                    if self.save(false) {
                        self.perform(action);
                    }
                }
                Some(Some(false)) => {
                    self.pending = None;
                    self.perform(action);
                }
                Some(None) => self.pending = None,
                None => {}
            }
        }

        // Toasts.
        self.toasts.retain(|toast| toast.born.elapsed().as_secs_f32() < TOAST_SECONDS);
        if !self.toasts.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(200));
            egui::Area::new(egui::Id::new("toasts"))
                .order(egui::Order::Tooltip)
                .anchor(Align2::CENTER_BOTTOM, vec2(0.0, -18.0))
                .interactable(false)
                .show(ctx, |ui| {
                    for toast in self.toasts.iter().rev().take(3).rev() {
                        let age = toast.born.elapsed().as_secs_f32();
                        let fade = ((TOAST_SECONDS - age) / 0.4).clamp(0.0, 1.0);
                        let border = if toast.is_error { theme::DANGER } else { theme::STROKE_STRONG };
                        egui::Frame::new()
                            .fill(theme::with_alpha(theme::BG_RAISED, fade))
                            .stroke(egui::Stroke::new(1.0, theme::with_alpha(border, fade)))
                            .corner_radius(8)
                            .inner_margin(egui::Margin::symmetric(12, 7))
                            .show(ui, |ui| {
                                ui.set_max_width(360.0);
                                ui.label(RichText::new(&toast.text).color(theme::with_alpha(theme::TEXT, fade)));
                            });
                        ui.add_space(4.0);
                    }
                });
        }
    }

    /// Developer screenshot mode: draw a few frames, save the picture, quit.
    fn screenshot(&mut self, ctx: &egui::Context) {
        let Some(job) = &mut self.screenshot else { return };
        let image = ctx.input(|i| {
            i.events.iter().find_map(|event| match event {
                egui::Event::Screenshot { image, .. } => Some(Arc::clone(image)),
                _ => None,
            })
        });
        if let Some(image) = image {
            let [width, height] = image.size;
            let rgba: Vec<u8> = image.pixels.iter().flat_map(|pixel| pixel.to_srgba_unmultiplied()).collect();
            let saved = magi_vision::encode_png(magi_vision::ImageRef::new(width as u32, height as u32, &rgba))
                .map_err(|e| e.to_string())
                .and_then(|png| std::fs::write(&job.path, png).map_err(|e| e.to_string()));
            if let Err(error) = saved {
                eprintln!("screenshot failed: {error}");
            }
            self.allow_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if job.frames_left > 0 {
            job.frames_left -= 1;
        } else if !job.requested {
            job.requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        ctx.request_repaint();
    }
}

impl eframe::App for MagiApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.tick(ctx);
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.apply_window_state(&ctx);
        self.shortcuts(&ctx);

        // Closing with unsaved changes asks first.
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close && self.doc.is_dirty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.pending = Some(Pending::Quit);
        }
        if self.allow_close && self.pending.is_none() && self.screenshot.is_none() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let (size, minimized) =
            ctx.input(|i| (i.viewport().inner_rect.map(|rect| rect.size()), i.viewport().minimized));
        if let Some(size) = size
            && minimized != Some(true)
            && size.x >= crate::MIN_SIZE[0]
            && size.y >= crate::MIN_SIZE[1]
        {
            self.settings.window_size = Some([size.x.round(), size.y.round()]);
        }

        // The screen picker, if one is open.
        if let Some(snip) = &mut self.snip
            && let Some(result) = snip.show(&ctx)
        {
            let snip = self.snip.take().expect("checked above");
            if let Some(outcome) = result {
                self.apply_snip(snip.target, outcome);
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }

        egui::Panel::top("top-bar")
            .frame(egui::Frame::new().fill(theme::BG_APP).inner_margin(egui::Margin::symmetric(12, 8)))
            .show(ui, |ui| self.top_bar(ui));
        self.status_bar(ui);
        egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::BG_PANEL)).show(ui, |ui| {
            match self.settings.tab {
                Tab::AutoClicker => self.clicker(ui),
                Tab::Macro => self.macro_editor(ui),
            }
        });

        self.windows(&ctx);
        self.screenshot(&ctx);
        let _ = Color32::WHITE;
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.controller.stop_everything();
        if self.screenshot.is_none() {
            self.save_settings();
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        theme::BG_APP.to_normalized_gamma_f32()
    }
}

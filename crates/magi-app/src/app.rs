//! The application shell: owns the settings, the open document and the tabs,
//! draws the banner with its menus, and carries out what the UI modules ask for.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use egui::{Align2, RichText, Ui, vec2};
use egui_phosphor::regular as icon;
use magi_core::action::Action;
use magi_core::autoclicker::{AutoClickerConfig, ClickPosition};
use magi_core::geom::Point;
use magi_core::graph::NodeId;
use magi_core::hotkey::{CaptureResult, Hotkey};
use magi_core::nodes::flow::StartParams;
use magi_core::nodes::{Registry, ids};
use magi_core::options::AutomationOptions;
use magi_core::preset::{NodePreset, Preset, PresetBody};
use magi_core::project::Project;
use magi_core::targets::{self, TargetShape, TargetSource};
use magi_core::text::format_hex_color;
use magi_engine::{LogEntry, LogLevel};

use crate::background::{self, BackgroundLayer};
use crate::controller::{Controller, ControllerConfig};
use crate::document::Document;
use crate::examples;
use crate::history::History;
use crate::paths::Paths;
use crate::presets::PresetLibrary;
use crate::settings::{ExportChoices, Settings, SettingsPage, Tab};
use crate::theme::{self, Palette, ThemeMode};
use crate::ui::autoclicker::{ClickerEnv, ClickerEvent, ClickerTab};
use crate::ui::macro_tab::{MacroEnv, MacroEvent, MacroTab};
use crate::ui::options::{self, OptionsEvent, RulesWindow};
use crate::ui::overlay::TargetOverlay;
use crate::ui::recording_editor;
use crate::ui::settings_window::{SettingsEnv, SettingsEvent, SettingsWindow, StartHotkey};
use crate::ui::snip::{Snip, SnipOutcome};
use crate::ui::{HotkeyTarget, SnipTarget, UiRequest, widgets};

const TOAST_SECONDS: f32 = 4.0;
const SETTINGS_SAVE_DELAY: Duration = Duration::from_millis(800);
const MACRO_FILTER: (&str, &[&str]) = ("MacroMagi macro", &["magi"]);
const PRESET_FILTER: (&str, &[&str]) = ("MacroMagi preset", &["mp"]);
/// Below this width the banner's menus show icons only.
const COMPACT_BANNER_BELOW: f32 = 720.0;
/// Below this width the banner's menus are folded into one.
const TINY_BANNER_BELOW: f32 = 520.0;

/// What to do once the user has decided about unsaved changes.
#[derive(Clone)]
enum Pending {
    New,
    Open(Option<PathBuf>),
    Example(usize),
    ClickerGraph,
    Quit,
}

/// Something that was about to start when it turned out to have no hotkey.
#[derive(Clone)]
enum Unguarded {
    Run,
    Record(Option<(NodeId, String)>),
}

struct HotkeyWarning {
    target: HotkeyTarget,
    then: Unguarded,
}

/// Settings found in an opened file, waiting for the user to accept them.
struct FileExtras {
    options: Option<AutomationOptions>,
    clicker: Option<AutoClickerConfig>,
}

#[derive(Clone, Copy, PartialEq)]
enum PresetKind {
    Clicker,
    Blocks,
}

/// Work done off the UI thread, reporting back.
enum Job {
    Toast(String, bool),
    BackgroundReady(String),
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
    /// Open the settings window on this page (for screenshots).
    pub settings_page: Option<SettingsPage>,
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
    clicker_history: History<AutoClickerConfig>,
    macro_tab: MacroTab,
    presets: PresetLibrary,

    /// The hotkey being rebound, and the chord pressed so far.
    capture: Option<(HotkeyTarget, Hotkey)>,
    snip: Option<Snip>,
    overlay: Option<TargetOverlay>,
    /// The block a running recording should be linked to when it ends.
    record_into: Option<(NodeId, String)>,
    settings_window: SettingsWindow,
    rules_window: RulesWindow,
    stop_hotkeys_open: bool,
    /// The recording shown in the pop-out editor.
    recording_window: Option<String>,
    pending: Option<Pending>,
    export: Option<ExportChoices>,
    preset_prompt: Option<(PresetKind, String)>,
    hotkey_warning: Option<HotkeyWarning>,
    file_extras: Option<FileExtras>,
    allow_close: bool,
    toasts: Vec<Toast>,
    jobs: Vec<Receiver<Job>>,
    downloading: bool,
    backgrounds: Vec<String>,

    logs: Vec<LogEntry>,
    next_log: u64,
    error_node: Option<NodeId>,
    activity: HashMap<NodeId, magi_engine::Activity>,

    background: BackgroundLayer,
    applied_palette: Option<(bool, Option<[u8; 3]>, u32)>,
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
        theme::install_fonts(&cc.egui_ctx);
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
            backgrounds: background::list(&paths.backgrounds_dir()),
            paths,
            doc: Document::starter(&registry),
            registry,
            controller,
            saved_settings: settings.clone(),
            clicker_history: History::new(&settings.clicker),
            settings,
            settings_changed: None,
            data_dir_error,
            clicker_tab: ClickerTab,
            macro_tab: MacroTab::default(),
            capture: None,
            snip: None,
            overlay: None,
            record_into: None,
            settings_window: SettingsWindow::default(),
            rules_window: RulesWindow::default(),
            stop_hotkeys_open: false,
            recording_window: None,
            pending: None,
            export: None,
            preset_prompt: None,
            hotkey_warning: None,
            file_extras: None,
            allow_close: false,
            toasts: Vec::new(),
            jobs: Vec::new(),
            downloading: false,
            logs: Vec::new(),
            next_log: 0,
            error_node: None,
            activity: HashMap::new(),
            background: BackgroundLayer::default(),
            applied_palette: None,
            applied_window: None,
            applied_title: String::new(),
            screenshot: options.screenshot.map(|path| Screenshot { path, frames_left: 45, requested: false }),
        };
        app.macro_tab.view.palette_size = vec2(app.settings.palette_size[0], app.settings.palette_size[1]);

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
        if let Some(page) = options.settings_page {
            app.settings.settings_page = page;
            app.settings_window.open = true;
        }
        app
    }

    fn toast(&mut self, text: impl Into<String>) {
        self.toasts.push(Toast { text: text.into(), born: Instant::now(), is_error: false });
    }

    fn toast_error(&mut self, text: impl Into<String>) {
        self.toasts.push(Toast { text: text.into(), born: Instant::now(), is_error: true });
    }

    /// Runs `work` on another thread; its result comes back as a [`Job`].
    fn spawn_job(&mut self, ctx: &egui::Context, work: impl FnOnce() -> Job + Send + 'static) {
        let (sender, receiver) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = sender.send(work());
            ctx.request_repaint();
        });
        self.jobs.push(receiver);
    }

    // ---- documents --------------------------------------------------------

    fn replace_document(&mut self, doc: Document) {
        self.controller.stop_macro();
        self.doc = doc;
        let palette_size = self.macro_tab.view.palette_size;
        self.macro_tab = MacroTab::default();
        self.macro_tab.view.palette_size = palette_size;
        self.recording_window = None;
        self.error_node = None;
        self.settings.tab = Tab::Macro;
    }

    fn open_path(&mut self, path: &Path) {
        match Project::load(path) {
            Ok(project) => {
                let extras = FileExtras { options: project.options.clone(), clicker: project.clicker.clone() };
                self.replace_document(Document::new(project, Some(path.to_path_buf())));
                self.settings.remember_file(path);
                let differs = extras.options.as_ref().is_some_and(|o| *o != self.settings.options)
                    || extras.clicker.as_ref().is_some_and(|c| *c != self.settings.clicker);
                if differs {
                    self.file_extras = Some(extras);
                }
            }
            Err(error) => self.toast_error(format!("Could not open {}: {error}", path.display())),
        }
    }

    fn write_document(&mut self, path: PathBuf) -> bool {
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

    fn pick_save_path(&self) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .add_filter(MACRO_FILTER.0, MACRO_FILTER.1)
            .set_file_name(format!("{}.magi", self.doc.title()))
            .save_file()
    }

    /// Saves the document. Returns false if the user cancelled or it failed.
    fn save(&mut self, choose_path: bool) -> bool {
        let path = match (&self.doc.path, choose_path) {
            (Some(path), false) => path.clone(),
            _ => match self.pick_save_path() {
                Some(path) => path,
                None => return false,
            },
        };
        self.write_document(path)
    }

    /// Saves to a chosen file with the extras picked in the export dialog.
    fn export(&mut self, choices: &ExportChoices) {
        let Some(path) = self.pick_save_path() else { return };
        if choices.presets {
            // Bundle the library, keeping presets that already travel with this file.
            for preset in self.presets.presets() {
                if !self.doc.project.presets.iter().any(|bundled| bundled.name == preset.name) {
                    self.doc.project.presets.push(preset.clone());
                }
            }
        }
        self.doc.project.options = choices.options.then(|| self.settings.options.clone());
        self.doc.project.clicker = choices.clicker.then(|| self.settings.clicker.clone());
        if self.write_document(path.clone()) {
            let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            self.toast(format!("Exported {name}"));
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

    /// Loads Auto Clicker settings from a `.magi` or `.mp` file.
    fn load_clicker_settings(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("MacroMagi file", &["magi", "mp"]).pick_file() else {
            return;
        };
        let loaded = if path.extension().is_some_and(|e| e == "mp") {
            Preset::load(&path).map_err(|e| e.to_string()).and_then(|preset| match preset.body {
                PresetBody::AutoClicker(config) => Ok(config),
                PresetBody::Nodes(_) => Err("this preset holds blocks, not Auto Clicker settings".to_owned()),
            })
        } else {
            Project::load(&path)
                .map_err(|e| e.to_string())
                .and_then(|project| project.clicker.ok_or_else(|| "this file has no Auto Clicker settings".to_owned()))
        };
        match loaded {
            Ok(config) => {
                self.settings.clicker = config;
                self.toast("Auto Clicker settings loaded");
            }
            Err(error) => self.toast_error(format!("Could not load the settings: {error}")),
        }
    }

    // ---- starting things --------------------------------------------------

    /// Where the user can stop a run from, for warnings.
    fn stop_hint(&self) -> String {
        if self.settings.stop_hotkey.is_none() {
            "from this window".to_owned()
        } else {
            format!("from this window or with the stop-everything hotkey ({})", self.settings.stop_hotkey.label())
        }
    }

    fn run_macro(&mut self, check: bool) {
        if check && self.settings.run_hotkey.is_none() {
            self.hotkey_warning = Some(HotkeyWarning { target: HotkeyTarget::Run, then: Unguarded::Run });
            return;
        }
        self.error_node = None;
        self.sync_controller();
        self.controller.run_macro();
    }

    fn toggle_recording(&mut self, into: Option<(NodeId, String)>, check: bool) {
        let recording = self.controller.recording_status().is_some();
        if !recording && check && self.settings.record_hotkey.is_none() {
            self.hotkey_warning = Some(HotkeyWarning { target: HotkeyTarget::Record, then: Unguarded::Record(into) });
            return;
        }
        if !recording {
            self.record_into = into;
        }
        self.sync_controller();
        self.controller.toggle_recording();
    }

    fn start_capture(&mut self, target: HotkeyTarget) {
        self.controller.begin_capture();
        self.capture = Some((target, Hotkey::none()));
    }

    // ---- requests from the UI ---------------------------------------------

    fn handle_request(&mut self, ctx: &egui::Context, request: UiRequest) {
        match request {
            UiRequest::Snip(mode, target) => match Snip::begin(ctx, mode, target) {
                Ok(snip) => self.snip = Some(snip),
                Err(error) => self.toast_error(error),
            },
            UiRequest::CaptureHotkey(target) => self.start_capture(target),
            UiRequest::CancelCapture => {
                self.controller.cancel_capture();
                self.capture = None;
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
            UiRequest::RecordInto(node, key) => self.toggle_recording(Some((node, key)), true),
            UiRequest::EditRecording(asset) => self.recording_window = Some(asset),
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
            (SnipTarget::ClickerNode(node), SnipOutcome::Point(point)) => {
                targets::apply(&mut self.doc.project.graph, &TargetSource::Clicker { node }, TargetShape::Point(point));
            }
            _ => {}
        }
    }

    fn apply_hotkey(&mut self, target: HotkeyTarget, hotkey: Hotkey) {
        match target {
            HotkeyTarget::Clicker => self.settings.clicker.hotkey = hotkey,
            HotkeyTarget::StopEverything => self.settings.stop_hotkey = hotkey,
            HotkeyTarget::Record => self.settings.record_hotkey = hotkey,
            HotkeyTarget::Run => self.settings.run_hotkey = hotkey,
            HotkeyTarget::ExtraStop(Some(index)) => {
                if let Some(slot) = self.settings.options.stop_hotkeys.get_mut(index) {
                    *slot = hotkey;
                }
            }
            HotkeyTarget::ExtraStop(None) => {
                if !self.settings.options.stop_hotkeys.contains(&hotkey) {
                    self.settings.options.stop_hotkeys.push(hotkey);
                }
            }
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

        // Work finished on other threads.
        let mut finished = Vec::new();
        self.jobs.retain(|receiver| match receiver.try_recv() {
            Ok(job) => {
                finished.push(job);
                false
            }
            Err(mpsc::TryRecvError::Empty) => true,
            Err(mpsc::TryRecvError::Disconnected) => false,
        });
        for job in finished {
            match job {
                Job::Toast(text, false) => self.toast(text),
                Job::Toast(text, true) => {
                    self.downloading = false;
                    self.toast_error(text);
                }
                Job::BackgroundReady(name) => {
                    self.downloading = false;
                    self.settings.appearance.background = Some(name);
                    self.backgrounds = background::list(&self.paths.backgrounds_dir());
                }
            }
        }

        // Hand the latest macro and settings to the controller.
        let settled = !ctx.input(|i| i.pointer.any_down()) && !ctx.text_edit_focused();
        self.doc.track(settled);
        self.clicker_history.track(&self.settings.clicker, settled);
        self.sync_controller();
        let minimized = ctx.input(|i| i.viewport().minimized) == Some(true);
        self.controller.set_window_hidden(minimized);

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

    /// Makes sure the controller has this frame's edits before something is started.
    fn sync_controller(&mut self) {
        self.controller.publish(self.doc.revision(), &self.doc.project);
        self.controller.set_config(ControllerConfig::from_settings(&self.settings));
    }

    fn apply_look(&mut self, ctx: &egui::Context) {
        let look = &self.settings.appearance;
        let dark = match look.theme {
            ThemeMode::Dark => true,
            ThemeMode::Light => false,
            ThemeMode::System => ctx.system_theme() != Some(egui::Theme::Light),
        };
        let panel_alpha = if look.background.is_some() { look.panel_opacity } else { 1.0 };
        let wanted = (dark, look.accent, panel_alpha.to_bits());
        if self.applied_palette != Some(wanted) {
            self.applied_palette = Some(wanted);
            theme::apply(ctx, Palette::build(dark, theme::accent_from(look.accent), panel_alpha));
        }
        let (file, blur) = (look.background.clone(), look.background_blur);
        self.background.sync(ctx, &self.paths.backgrounds_dir(), file.as_deref(), blur);
        if let Some(error) = self.background.take_error() {
            self.toast_error(error);
        }

        let wanted = (self.settings.always_on_top, self.settings.hide_from_capture, self.settings.ui_scale);
        if self.applied_window != Some(wanted) {
            self.applied_window = Some(wanted);
            let level = if wanted.0 { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal };
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
            ctx.send_viewport_cmd(egui::ViewportCommand::ContentProtected(wanted.1));
            let range = crate::settings::UI_SCALE_RANGE;
            ctx.set_zoom_factor(wanted.2.clamp(*range.start(), *range.end()));
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

    // ---- the banner ---------------------------------------------------------

    fn banner(&mut self, ui: &mut Ui) {
        let width = ui.available_width();
        let compact = width < COMPACT_BANNER_BELOW;
        let label = |glyph: &str, word: &str| if compact { glyph.to_owned() } else { format!("{glyph}  {word}") };
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = if compact { 2.0 } else { 6.0 };
            if width > 470.0 {
                ui.label(RichText::new(icon::MAGIC_WAND).size(18.0).color(theme::accent_bright()));
            }
            let names = if width < 430.0 { ("Clicker", "Macro") } else { ("Auto Clicker", "Macro") };
            widgets::segmented(ui, &mut self.settings.tab, &[(Tab::AutoClicker, names.0), (Tab::Macro, names.1)]);
            ui.add_space(if compact { 2.0 } else { 6.0 });

            if width < TINY_BANNER_BELOW {
                // Too narrow for a row of menus: one menu holds them all.
                ui.menu_button(RichText::new(icon::LIST).size(15.0), |ui| {
                    ui.set_min_width(180.0);
                    self.menus(ui, &ctx, |glyph, word| format!("{glyph}  {word}"));
                })
                .response
                .on_hover_text("Menu");
            } else {
                self.menus(ui, &ctx, label);
            }

            let (can_undo, can_redo) = match self.settings.tab {
                Tab::AutoClicker => {
                    (self.clicker_history.can_undo(&self.settings.clicker), self.clicker_history.can_redo())
                }
                Tab::Macro => (self.doc.can_undo(), self.doc.can_redo()),
            };
            let undo = egui::Button::new(icon::ARROW_COUNTER_CLOCKWISE).frame(false);
            if ui.add_enabled(can_undo, undo).on_hover_text("Undo (Ctrl+Z)").clicked() {
                self.undo();
            }
            let redo = egui::Button::new(icon::ARROW_CLOCKWISE).frame(false);
            if ui.add_enabled(can_redo, redo).on_hover_text("Redo (Ctrl+Y)").clicked() {
                self.redo();
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if widgets::icon_toggle(ui, icon::GEAR, self.settings_window.open, "Settings").clicked() {
                    self.settings_window.open = !self.settings_window.open;
                }
                let pinned = self.settings.always_on_top;
                if widgets::icon_toggle(ui, icon::PUSH_PIN, pinned, "Keep on top of other windows").clicked() {
                    self.settings.always_on_top = !pinned;
                }
                let hidden = self.settings.hide_from_capture;
                let (glyph, hint) = if hidden {
                    (
                        icon::EYE_SLASH,
                        "Hidden from screen captures – click to show MacroMagi in screenshots and recordings",
                    )
                } else {
                    (icon::EYE, "Visible in screen captures – click to hide MacroMagi from screenshots and recordings")
                };
                if widgets::icon_toggle(ui, glyph, hidden, hint).clicked() {
                    self.settings.hide_from_capture = !hidden;
                }
                if self.controller.anything_running() {
                    let text = if compact || self.settings.stop_hotkey.is_none() {
                        icon::STOP.to_owned()
                    } else {
                        format!("{}  Stop all ({})", icon::STOP, self.settings.stop_hotkey.label())
                    };
                    let stop = egui::Button::new(RichText::new(text).size(12.0).color(theme::danger())).frame(false);
                    if ui.add(stop).on_hover_text("Stop the auto clicker and every macro").clicked() {
                        self.controller.stop_everything();
                    }
                }
                if let Some(status) = self.controller.recording_status() {
                    let text = format!("{}  {:.0}s", icon::RECORD, status.seconds);
                    ui.label(RichText::new(text).size(12.0).color(theme::danger()));
                    ui.ctx().request_repaint_after(Duration::from_millis(250));
                }
            });
        });
    }

    /// File, Options, Presets and the tab's own menus.
    fn menus(&mut self, ui: &mut Ui, ctx: &egui::Context, label: impl Fn(&str, &str) -> String) {
        self.file_menu(ui, &label(icon::FILE, "File"));
        let mut option_events = Vec::new();
        ui.menu_button(label(icon::SLIDERS, "Options"), |ui| {
            option_events = options::menu(ui, &mut self.settings.options, &mut self.settings.dry_run);
        })
        .response
        .on_hover_text("Options for the Auto Clicker and macros");
        for event in option_events {
            match event {
                OptionsEvent::OpenStopHotkeys => self.stop_hotkeys_open = true,
                OptionsEvent::OpenProcessRules => self.rules_window.open = true,
                OptionsEvent::CreateShortcut => {
                    self.spawn_job(ctx, || match magi_platform::system::create_desktop_shortcut() {
                        Ok(path) => Job::Toast(format!("Shortcut created: {}", path.display()), false),
                        Err(error) => Job::Toast(format!("Could not create the shortcut: {error}"), true),
                    })
                }
            }
        }
        self.presets_menu(ui, &label(icon::BOOKMARK_SIMPLE, "Presets"));

        match self.settings.tab {
            Tab::AutoClicker => {
                if ui
                    .button(label(icon::TREE_STRUCTURE, "Open as macro"))
                    .on_hover_text("Turn these settings into blocks you can extend in the Macro tab")
                    .clicked()
                {
                    ui.close();
                    self.guarded(Pending::ClickerGraph);
                }
            }
            Tab::Macro => {
                if ui
                    .button(label(icon::PLUS, "Add"))
                    .on_hover_text("Add a block (or double-click the canvas)")
                    .clicked()
                {
                    ui.close();
                    self.macro_tab.view.open_palette_in_view();
                }
                self.view_menu(ui, &label(icon::MONITOR, "View"));
            }
        }
    }

    fn undo(&mut self) {
        match self.settings.tab {
            Tab::AutoClicker => self.clicker_history.undo(&mut self.settings.clicker),
            Tab::Macro => self.doc.undo(),
        }
    }

    fn redo(&mut self) {
        match self.settings.tab {
            Tab::AutoClicker => self.clicker_history.redo(&mut self.settings.clicker),
            Tab::Macro => self.doc.redo(),
        }
    }

    fn file_menu(&mut self, ui: &mut Ui, label: &str) {
        type Action = Box<dyn FnOnce(&mut MagiApp)>;
        let mut chosen: Option<Action> = None;
        let tab = self.settings.tab;
        let recent = self.settings.recent_files.clone();
        let blocks = self.doc.project.graph.len();
        ui.menu_button(label, |ui| {
            ui.set_min_width(240.0);
            let mut item = |ui: &mut Ui, text: String, shortcut: &str, action: Action| {
                if ui.add(egui::Button::new(text).shortcut_text(shortcut)).clicked() {
                    chosen = Some(action);
                    ui.close();
                }
            };
            item(ui, format!("{}  New macro", icon::FILE_PLUS), "Ctrl+N", Box::new(|app| app.guarded(Pending::New)));
            item(
                ui,
                format!("{}  Open…", icon::FOLDER_OPEN),
                "Ctrl+O",
                Box::new(|app| app.guarded(Pending::Open(None))),
            );
            ui.menu_button(format!("{}  Open recent", icon::CLOCK_COUNTER_CLOCKWISE), |ui| {
                if recent.is_empty() {
                    ui.weak("Nothing yet");
                }
                for path in &recent {
                    let name = path
                        .file_name()
                        .map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
                    if ui.button(name).on_hover_text(path.display().to_string()).clicked() {
                        let path = path.clone();
                        chosen = Some(Box::new(move |app: &mut MagiApp| app.guarded(Pending::Open(Some(path)))));
                        ui.close();
                    }
                }
            });
            ui.menu_button(format!("{}  Examples", icon::LIGHTBULB), |ui| {
                for (index, name) in examples::NAMES.iter().enumerate() {
                    if ui.button(*name).clicked() {
                        chosen = Some(Box::new(move |app: &mut MagiApp| app.guarded(Pending::Example(index))));
                        ui.close();
                    }
                }
            });
            ui.separator();
            let mut item = |ui: &mut Ui, text: String, shortcut: &str, action: Action| {
                if ui.add(egui::Button::new(text).shortcut_text(shortcut)).clicked() {
                    chosen = Some(action);
                    ui.close();
                }
            };
            item(
                ui,
                format!("{}  Save", icon::FLOPPY_DISK),
                "Ctrl+S",
                Box::new(|app| {
                    app.save(false);
                }),
            );
            item(
                ui,
                format!("{}  Save as…", icon::FLOPPY_DISK_BACK),
                "Ctrl+Shift+S",
                Box::new(|app| {
                    app.save(true);
                }),
            );
            item(
                ui,
                format!("{}  Export…", icon::EXPORT),
                "Ctrl+E",
                Box::new(|app| app.export = Some(app.settings.export.clone())),
            );
            match tab {
                Tab::AutoClicker => {
                    ui.separator();
                    item(
                        ui,
                        format!("{}  Load clicker settings from a file…", icon::DOWNLOAD_SIMPLE),
                        "",
                        Box::new(|app| app.load_clicker_settings()),
                    );
                }
                Tab::Macro => {
                    ui.separator();
                    ui.weak(format!("{blocks} blocks"));
                }
            }
        });
        if let Some(action) = chosen {
            action(self);
        }
    }

    /// Presets for the open tab: Auto Clicker settings, or groups of blocks.
    fn presets_menu(&mut self, ui: &mut Ui, label: &str) {
        let tab = self.settings.tab;
        let mut delete = None;
        let mut load: Option<AutoClickerConfig> = None;
        let mut insert: Option<NodePreset> = None;
        let mut prompt = None;
        let mut import = false;
        let has_selection = !self.macro_tab.view.selection.is_empty();
        ui.menu_button(label, |ui| {
            ui.set_min_width(230.0);
            let library = self.presets.entries().iter().map(|entry| (&entry.preset, Some(&entry.path)));
            let bundled = self.doc.project.presets.iter().map(|preset| (preset, None));
            let mut any = false;
            for (preset, path) in library.chain(bundled) {
                let fits = matches!(
                    (&preset.body, tab),
                    (PresetBody::AutoClicker(_), Tab::AutoClicker) | (PresetBody::Nodes(_), Tab::Macro)
                );
                if !fits {
                    continue;
                }
                any = true;
                ui.horizontal(|ui| {
                    let name =
                        if path.is_some() { preset.name.clone() } else { format!("{}  (in this file)", preset.name) };
                    if ui.button(name).clicked() {
                        match &preset.body {
                            PresetBody::AutoClicker(config) => load = Some(config.clone()),
                            PresetBody::Nodes(nodes) => insert = Some(nodes.clone()),
                        }
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
                ui.weak(if tab == Tab::AutoClicker { "No clicker presets yet." } else { "No block presets yet." });
            }
            ui.separator();
            match tab {
                Tab::AutoClicker => {
                    if ui.button(format!("{}  Save current settings…", icon::FLOPPY_DISK)).clicked() {
                        prompt = Some(PresetKind::Clicker);
                        ui.close();
                    }
                }
                Tab::Macro => {
                    let save = egui::Button::new(format!("{}  Save selected blocks…", icon::FLOPPY_DISK));
                    if ui
                        .add_enabled(has_selection, save)
                        .on_disabled_hover_text("Select one or more blocks first")
                        .clicked()
                    {
                        prompt = Some(PresetKind::Blocks);
                        ui.close();
                    }
                }
            }
            if ui.button(format!("{}  Import a .mp file…", icon::DOWNLOAD_SIMPLE)).clicked() {
                import = true;
                ui.close();
            }
        });
        if let Some(path) = delete {
            self.presets.delete(&path);
        }
        if let Some(config) = load {
            self.settings.clicker = config;
        }
        if let Some(nodes) = insert {
            let at = self.macro_tab.view.view_center();
            self.macro_tab.view.insert_preset(&mut self.doc.project, &self.registry, &nodes, at);
        }
        if let Some(kind) = prompt {
            self.preset_prompt = Some((kind, String::new()));
        }
        if import && let Some(file) = rfd::FileDialog::new().add_filter(PRESET_FILTER.0, PRESET_FILTER.1).pick_file() {
            match self.presets.import(&file) {
                Ok(preset) => self.toast(format!("Imported “{}”", preset.name)),
                Err(error) => self.toast_error(format!("Could not import: {error}")),
            }
        }
    }

    fn view_menu(&mut self, ui: &mut Ui, label: &str) {
        let mut show_targets = false;
        let width = ui.ctx().content_rect().width();
        ui.menu_button(label, |ui| {
            ui.set_min_width(230.0);
            if ui
                .button(format!("{}  Screen targets…", icon::CROSSHAIR))
                .on_hover_text(
                    "Show every click position and search area on the screen, numbered in run order, to check and \
                     move them",
                )
                .clicked()
            {
                show_targets = true;
                ui.close();
            }
            ui.separator();
            ui.checkbox(&mut self.settings.show_console, "Console");
            let mut inspector = self.macro_tab.inspector_shown(width);
            if ui.checkbox(&mut inspector, "Block settings").changed() {
                self.macro_tab.toggle_inspector(width);
            }
            ui.separator();
            if ui.add(egui::Button::new(format!("{}  Fit to view", icon::CORNERS_OUT)).shortcut_text("F")).clicked() {
                self.macro_tab.view.request_fit();
                ui.close();
            }
            if ui.button(format!("{}  Actual size", icon::MAGNIFYING_GLASS)).clicked() {
                self.macro_tab.view.reset_zoom();
                ui.close();
            }
        });
        if show_targets {
            match TargetOverlay::begin(ui.ctx()) {
                Ok(overlay) => self.overlay = Some(overlay),
                Err(error) => self.toast_error(error),
            }
        }
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        let problem = self.data_dir_error.clone().or_else(|| {
            self.controller.listener_error().map(|e| format!("Hotkeys and recording are unavailable: {e}"))
        });
        let Some(problem) = problem else { return };
        egui::Panel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(theme::mix(theme::bg_panel(), theme::warning(), 0.12))
                    .inner_margin(egui::Margin::symmetric(10, 5)),
            )
            .show(ui, |ui| widgets::notice(ui, icon::WARNING, problem, theme::warning()));
    }

    // ---- the tabs -------------------------------------------------------------

    fn clicker(&mut self, ui: &mut Ui) {
        let monitor = self.controller.clicker_monitor();
        let error = monitor.logs().into_iter().rev().find(|e| e.level == LogLevel::Error).map(|e| e.message);
        let running = self.controller.clicker_running();
        let env = ClickerEnv {
            running,
            clicks: monitor.clicks(),
            error,
            capturing: self.capture.as_ref().map(|(target, pending)| (target, pending)),
        };
        let events = self.clicker_tab.show(ui, &mut self.settings.clicker, &env);
        if running {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
        for event in events {
            match event {
                ClickerEvent::Start => {
                    // Publish first, so a setting changed this very frame is used.
                    self.sync_controller();
                    self.controller.start_clicker();
                }
                ClickerEvent::Stop => self.controller.stop_clicker(),
                ClickerEvent::Request(request) => self.handle_request(ui.ctx(), request),
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
            run_hotkey: &self.settings.run_hotkey,
            record_hotkey: &self.settings.record_hotkey,
            capturing: self.capture.as_ref().map(|(target, pending)| (target, pending)),
            engine_running: (!running.is_empty()).then(|| self.controller.macro_engine()).flatten(),
            engine_preference: self.controller.preferred_engine(),
            python: &python,
            dry_run: self.settings.dry_run,
            show_console: &mut self.settings.show_console,
        };
        let events = self.macro_tab.show(ui, &mut self.doc, &mut env);
        let size = self.macro_tab.view.palette_size;
        self.settings.palette_size = [size.x.round(), size.y.round()];
        for event in events {
            match event {
                MacroEvent::Run => self.run_macro(true),
                MacroEvent::Stop => self.controller.stop_macro(),
                MacroEvent::ToggleStart(node) => {
                    if running.contains(&node) {
                        self.controller.stop_node(node);
                    } else {
                        self.error_node = None;
                        self.sync_controller();
                        self.controller.run_from(node);
                    }
                }
                MacroEvent::RunFrom(node) => {
                    self.error_node = None;
                    self.sync_controller();
                    self.controller.run_from(node);
                }
                MacroEvent::ToggleRecording => self.toggle_recording(None, true),
                MacroEvent::Request(request) => self.handle_request(ui.ctx(), request),
                MacroEvent::Toast(text) => self.toast(text),
            }
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.text_edit_focused() || self.capture.is_some() {
            return;
        }
        let (save, save_as, open, new, export, undo, redo) = ctx.input(|i| {
            let cmd = i.modifiers.command;
            (
                cmd && !i.modifiers.shift && i.key_pressed(egui::Key::S),
                cmd && i.modifiers.shift && i.key_pressed(egui::Key::S),
                cmd && i.key_pressed(egui::Key::O),
                cmd && i.key_pressed(egui::Key::N),
                cmd && i.key_pressed(egui::Key::E),
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
        if export {
            self.export = Some(self.settings.export.clone());
        }
        if undo {
            self.undo();
        }
        if redo {
            self.redo();
        }
    }

    // ---- windows and dialogs ----------------------------------------------

    fn windows(&mut self, ctx: &egui::Context) {
        self.settings_window_frame(ctx);
        if self.stop_hotkeys_open {
            let capturing = self.capture.as_ref().map(|(target, pending)| (target, pending));
            let mut open = true;
            let requests = options::stop_hotkeys_window(ctx, &mut open, &mut self.settings.options, capturing);
            self.stop_hotkeys_open = open;
            for request in requests {
                self.handle_request(ctx, request);
            }
        }
        self.rules_window.show(ctx, &mut self.settings.options);
        self.recording_popout(ctx);
        self.dialogs(ctx);
        self.toasts(ctx);
    }

    fn settings_window_frame(&mut self, ctx: &egui::Context) {
        if !self.settings_window.open {
            return;
        }
        let python = self.controller.python_status();
        let starts: Vec<StartHotkey> = self
            .doc
            .project
            .graph
            .starts()
            .map(|node| StartHotkey {
                node: node.id,
                name: format!("“{}” block", self.registry.title(node)),
                hotkey: node.params_or_default::<StartParams>().hotkey,
            })
            .collect();
        let env = SettingsEnv {
            capturing: self.capture.as_ref().map(|(target, pending)| (target, pending)),
            python: &python,
            starts: &starts,
            backgrounds: &self.backgrounds,
            downloading: self.downloading,
        };
        let events = self.settings_window.show(ctx, &mut self.settings, &env);
        for event in events {
            match event {
                SettingsEvent::Request(request) => self.handle_request(ctx, request),
                SettingsEvent::CheckPython => {
                    self.sync_controller();
                    // Starting Python can take a second; do it off the UI thread.
                    let (controller, ctx) = (self.controller.clone(), ctx.clone());
                    std::thread::spawn(move || {
                        controller.probe_python();
                        ctx.request_repaint();
                    });
                }
                SettingsEvent::ChooseBackgroundFile => {
                    let dialog = rfd::FileDialog::new().add_filter("Pictures", background::EXTENSIONS);
                    if let Some(file) = dialog.pick_file() {
                        match background::import_file(&self.paths.backgrounds_dir(), &file) {
                            Ok(name) => {
                                self.settings.appearance.background = Some(name);
                                self.backgrounds = background::list(&self.paths.backgrounds_dir());
                            }
                            Err(error) => self.toast_error(error),
                        }
                    }
                }
                SettingsEvent::DownloadBackground(url) => {
                    self.downloading = true;
                    let dir = self.paths.backgrounds_dir();
                    self.spawn_job(ctx, move || match background::download(&dir, &url) {
                        Ok(name) => Job::BackgroundReady(name),
                        Err(error) => Job::Toast(error, true),
                    });
                }
                SettingsEvent::RemoveBackground(name) => {
                    if self.settings.appearance.background.as_ref() == Some(&name) {
                        self.settings.appearance.background = None;
                    }
                    background::remove(&self.paths.backgrounds_dir(), &name);
                    self.backgrounds = background::list(&self.paths.backgrounds_dir());
                }
                SettingsEvent::ResetHotkeys => {
                    self.settings.reset_hotkeys();
                    self.toast("Hotkeys reset");
                }
                SettingsEvent::ResetAll => {
                    self.settings.reset_all();
                    self.toast("All settings reset");
                }
                SettingsEvent::SetStartHotkey(node, hotkey) => {
                    if let Some(node) = self.doc.project.graph.node_mut(node) {
                        node.set_param("hotkey", hotkey);
                    }
                }
            }
        }
    }

    /// The recording editor in its own window.
    fn recording_popout(&mut self, ctx: &egui::Context) {
        let Some(asset) = self.recording_window.clone() else { return };
        let Some(recording) = self.doc.project.assets.recording(&asset).cloned() else {
            self.recording_window = None;
            return;
        };
        let name = self.doc.project.assets.get(&asset).map_or_else(|| "Recording".to_owned(), |a| a.name.clone());
        let size = self.settings.recording_window_size.unwrap_or([760.0, 560.0]);
        let builder = egui::ViewportBuilder::default()
            .with_title(format!("{name} – MacroMagi"))
            .with_inner_size(size)
            .with_min_inner_size([420.0, 300.0]);
        let mut edit = None;
        let mut closed = false;
        let mut new_size = None;
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of(("recording", &asset)), builder, |ui, class| {
            let mut contents = |ui: &mut Ui| {
                let height = (ui.available_height() - 150.0).max(120.0);
                edit = recording_editor::show(ui, &recording, height, true).edit;
            };
            match class {
                egui::ViewportClass::EmbeddedWindow => contents(ui),
                _ => {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::new().fill(theme::bg_panel().to_opaque()).inner_margin(12))
                        .show(ui, |ui| contents(ui));
                }
            }
            let (close, rect) = ui.input(|i| (i.viewport().close_requested(), i.viewport().inner_rect));
            closed = close;
            new_size = rect.map(|r| [r.width().round(), r.height().round()]);
        });
        if let Some(edit) = edit
            && let Some(target) = self.doc.project.assets.recording_mut(&asset)
        {
            edit.apply(target);
        }
        if let Some(size) = new_size.filter(|[w, h]| *w >= 300.0 && *h >= 200.0) {
            self.settings.recording_window_size = Some(size);
        }
        if closed {
            self.recording_window = None;
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        self.unsaved_dialog(ctx);
        self.export_dialog(ctx);
        self.preset_dialog(ctx);
        self.hotkey_warning_dialog(ctx);
        self.file_extras_dialog(ctx);
    }

    fn unsaved_dialog(&mut self, ctx: &egui::Context) {
        let Some(action) = self.pending.clone() else { return };
        let mut decision = None;
        egui::Modal::new(egui::Id::new("unsaved-changes")).show(ctx, |ui| {
            ui.set_width(300.0);
            ui.label(RichText::new("Save your changes?").strong().size(15.0));
            ui.label(format!("“{}” has changes that have not been saved.", self.doc.title()));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if widgets::accent_button(ui, "Save", theme::accent(), vec2(70.0, 26.0)).clicked() {
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

    fn export_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut choices) = self.export.take() else { return };
        let mut decision = None;
        let library = self.presets.entries().len();
        egui::Modal::new(egui::Id::new("export")).show(ctx, |ui| {
            ui.set_width(320.0);
            ui.label(RichText::new(format!("{}  Export", icon::EXPORT)).strong().size(15.0));
            widgets::note(ui, "Saves the macro as a .magi file. Choose what else goes into it:");
            ui.add_space(4.0);
            widgets::toggle_row(
                ui,
                &format!("My presets ({library})"),
                "Your preset library, so it travels with the file.",
                &mut choices.presets,
            );
            widgets::toggle_row(
                ui,
                "Options",
                "Stop hotkeys, program rules and the other Options.",
                &mut choices.options,
            );
            widgets::toggle_row(ui, "Auto Clicker settings", "", &mut choices.clicker);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if widgets::accent_button(ui, "Export…", theme::accent(), vec2(90.0, 26.0)).clicked() {
                    decision = Some(true);
                }
                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    decision = Some(false);
                }
            });
        });
        match decision {
            Some(true) => self.export(&choices),
            Some(false) => {}
            None => self.export = Some(choices),
        }
    }

    fn preset_dialog(&mut self, ctx: &egui::Context) {
        let Some((kind, mut name)) = self.preset_prompt.take() else { return };
        let mut decision = None;
        egui::Modal::new(egui::Id::new("preset-name")).show(ctx, |ui| {
            ui.set_width(300.0);
            let title = match kind {
                PresetKind::Clicker => "Save Auto Clicker settings as a preset",
                PresetKind::Blocks => "Save the selected blocks as a preset",
            };
            ui.label(RichText::new(title).strong());
            let field =
                ui.add(egui::TextEdit::singleline(&mut name).hint_text("Preset name").desired_width(f32::INFINITY));
            field.request_focus();
            let entered = ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.horizontal(|ui| {
                let ready = !name.trim().is_empty();
                if (ui.add_enabled(ready, egui::Button::new("Save")).clicked() || entered) && ready {
                    decision = Some(true);
                }
                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    decision = Some(false);
                }
            });
        });
        match decision {
            Some(true) => {
                let body = match kind {
                    PresetKind::Clicker => PresetBody::AutoClicker(self.settings.clicker.clone()),
                    PresetKind::Blocks => PresetBody::Nodes(NodePreset::capture(
                        &self.doc.project.graph,
                        &self.macro_tab.view.selection,
                        &self.doc.project.assets,
                        &self.registry,
                    )),
                };
                let preset = Preset { name: name.trim().to_owned(), description: String::new(), body };
                match self.presets.save(&preset) {
                    Ok(_) => self.toast(format!("Saved preset “{}”", preset.name)),
                    Err(error) => self.toast_error(format!("Could not save the preset: {error}")),
                }
            }
            Some(false) => {}
            None => self.preset_prompt = Some((kind, name)),
        }
    }

    /// Asks before starting something that has no hotkey to stop it.
    fn hotkey_warning_dialog(&mut self, ctx: &egui::Context) {
        let Some(warning) = self.hotkey_warning.take() else { return };
        let mut decision = None;
        let what = match warning.then {
            Unguarded::Run => "Run",
            Unguarded::Record(_) => "Record",
        };
        let stop_hint = self.stop_hint();
        egui::Modal::new(egui::Id::new("hotkey-warning")).show(ctx, |ui| {
            ui.set_width(340.0);
            ui.label(RichText::new(format!("{}  {what} has no hotkey", icon::KEYBOARD)).strong().size(15.0));
            let text = match warning.then {
                Unguarded::Run => format!("Without one, the macro can only be stopped {stop_hint}."),
                Unguarded::Record(_) => {
                    "Without one, recording has to be stopped with the Stop button, and that click is recorded too."
                        .to_owned()
                }
            };
            widgets::note(ui, text);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if widgets::accent_button(ui, "Set a hotkey", theme::accent(), vec2(100.0, 26.0)).clicked() {
                    decision = Some(0);
                }
                if ui.button("Start anyway").clicked() {
                    decision = Some(1);
                }
                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    decision = Some(2);
                }
            });
        });
        match decision {
            Some(0) => {
                self.settings.settings_page = SettingsPage::Hotkeys;
                self.settings_window.open = true;
                self.start_capture(warning.target);
            }
            Some(1) => match warning.then {
                Unguarded::Run => self.run_macro(false),
                Unguarded::Record(into) => self.toggle_recording(into, false),
            },
            Some(_) => {}
            None => self.hotkey_warning = Some(warning),
        }
    }

    /// Offers the Options and Auto Clicker settings that came with an opened file.
    fn file_extras_dialog(&mut self, ctx: &egui::Context) {
        let Some(extras) = self.file_extras.take() else { return };
        let mut decision = None;
        egui::Modal::new(egui::Id::new("file-extras")).show(ctx, |ui| {
            ui.set_width(330.0);
            ui.label(RichText::new("Use the settings in this file?").strong().size(15.0));
            let mut parts = Vec::new();
            if extras.options.is_some() {
                parts.push("Options (stop hotkeys, program rules…)");
            }
            if extras.clicker.is_some() {
                parts.push("Auto Clicker settings");
            }
            widgets::note(ui, format!("“{}” includes: {}.", self.doc.title(), parts.join(", ")));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if widgets::accent_button(ui, "Use them", theme::accent(), vec2(90.0, 26.0)).clicked() {
                    decision = Some(true);
                }
                if ui.button("Keep mine").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    decision = Some(false);
                }
            });
        });
        match decision {
            Some(true) => {
                if let Some(options) = extras.options {
                    self.settings.options = options;
                }
                if let Some(clicker) = extras.clicker {
                    self.settings.clicker = clicker;
                }
            }
            Some(false) => {}
            None => self.file_extras = Some(extras),
        }
    }

    fn toasts(&mut self, ctx: &egui::Context) {
        self.toasts.retain(|toast| toast.born.elapsed().as_secs_f32() < TOAST_SECONDS);
        if self.toasts.is_empty() {
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(200));
        egui::Area::new(egui::Id::new("toasts"))
            .order(egui::Order::Tooltip)
            .anchor(Align2::CENTER_BOTTOM, vec2(0.0, -18.0))
            .interactable(false)
            .show(ctx, |ui| {
                for toast in self.toasts.iter().rev().take(3).rev() {
                    let age = toast.born.elapsed().as_secs_f32();
                    let fade = ((TOAST_SECONDS - age) / 0.4).clamp(0.0, 1.0);
                    let border = if toast.is_error { theme::danger() } else { theme::stroke_strong() };
                    egui::Frame::new()
                        .fill(theme::with_alpha(theme::bg_raised().to_opaque(), fade))
                        .stroke(egui::Stroke::new(1.0, theme::with_alpha(border, fade)))
                        .corner_radius(8)
                        .inner_margin(egui::Margin::symmetric(12, 7))
                        .show(ui, |ui| {
                            ui.set_max_width(360.0);
                            ui.label(RichText::new(&toast.text).color(theme::with_alpha(theme::text(), fade)));
                        });
                    ui.add_space(4.0);
                }
            });
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
        self.apply_look(&ctx);
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

        // The background picture, under everything.
        let look = &self.settings.appearance;
        let painter = ctx.layer_painter(egui::LayerId::background());
        self.background.paint(
            &painter,
            ctx.content_rect(),
            look.background_opacity,
            look.background_darkness,
            theme::palette().dark,
        );

        // The screen picker or the targets overlay, if one is open.
        if let Some(snip) = &mut self.snip
            && let Some(result) = snip.show(&ctx)
        {
            let snip = self.snip.take().expect("checked above");
            if let Some(outcome) = result {
                self.apply_snip(snip.target, outcome);
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        if let Some(overlay) = &mut self.overlay
            && overlay.show(&ctx, &mut self.doc.project, &self.registry)
        {
            self.overlay = None;
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }

        egui::Panel::top("top-bar")
            .frame(egui::Frame::new().fill(theme::bg_app()).inner_margin(egui::Margin::symmetric(10, 7)))
            .show(ui, |ui| self.banner(ui));
        self.status_bar(ui);
        egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::bg_panel())).show(ui, |ui| {
            match self.settings.tab {
                Tab::AutoClicker => self.clicker(ui),
                Tab::Macro => self.macro_editor(ui),
            }
        });

        self.windows(&ctx);
        self.screenshot(&ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.controller.shut_down();
        if self.screenshot.is_none() {
            self.save_settings();
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        theme::bg_app().to_opaque().to_normalized_gamma_f32()
    }
}

#[cfg(test)]
mod tests {
    //! The whole app, driven headlessly. Input the user presses is fed to the
    //! controller exactly as the system listener would.

    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use magi_core::input::{InputEvent, Key};

    use super::*;

    fn app() -> Harness<'static, MagiApp> {
        app_sized(480.0, 1500.0)
    }

    fn app_sized(width: f32, height: f32) -> Harness<'static, MagiApp> {
        let dir = std::env::temp_dir().join(format!(
            "magi-app-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let paths = Paths { app_dir: dir.clone(), data_dir: dir.join("data") };
        let settings = Settings { dry_run: true, ..Settings::default() };
        let options = StartupOptions {
            open: None,
            example: None,
            tab: Some(Tab::AutoClicker),
            screenshot: None,
            show_cards: false,
            select_kind: None,
            settings_page: None,
        };
        let mut harness = Harness::builder()
            .with_size(vec2(width, height))
            .build_eframe(move |cc| MagiApp::new(cc, paths, settings, options));
        harness.run_steps(4);
        harness
    }

    fn press(harness: &mut Harness<'_, MagiApp>, keys: &[Key]) {
        let controller = harness.state().controller.clone();
        for key in keys {
            controller.inject(InputEvent::KeyDown { key: *key });
        }
        for key in keys.iter().rev() {
            controller.inject(InputEvent::KeyUp { key: *key });
        }
        harness.run_steps(3);
    }

    #[test]
    fn hotkeys_set_in_the_ui_take_effect_at_once() {
        let mut harness = app();
        // The Auto Clicker's hotkey field, showing F6.
        harness.get_by_label(&format!("{}  F6", icon::KEYBOARD)).click();
        harness.run_steps(3);
        assert!(harness.state().capture.is_some(), "the field is listening");

        // Ctrl + Alt + Y, as the user presses it.
        press(&mut harness, &[Key::ControlLeft, Key::AltLeft, Key::KeyY]);
        let expected: Hotkey = "ControlLeft+AltLeft+KeyY".parse().unwrap();
        assert_eq!(harness.state().settings.clicker.hotkey, expected);
        assert!(harness.state().capture.is_none());

        // No restart needed: the new hotkey starts the clicker straight away.
        press(&mut harness, &[Key::ControlLeft, Key::AltLeft, Key::KeyY]);
        std::thread::sleep(Duration::from_millis(100));
        assert!(harness.state().controller.clicker_running(), "the new hotkey works");
        harness.state().controller.stop_everything();
    }

    #[test]
    fn escape_and_function_keys_can_be_hotkeys() {
        let mut harness = app();
        for key in [Key::Escape, Key::F6, Key::F12] {
            let label = format!("{}  {}", icon::KEYBOARD, harness.state().settings.clicker.hotkey.label());
            harness.get_by_label(&label).click();
            harness.run_steps(3);
            press(&mut harness, &[key]);
            assert_eq!(harness.state().settings.clicker.hotkey, Hotkey::key(key));
        }
    }

    #[test]
    fn the_add_button_opens_the_block_list_and_it_stays_open() {
        let mut harness = app_sized(1000.0, 700.0);
        harness.state_mut().settings.tab = Tab::Macro;
        harness.run_steps(3);
        harness.get_by_label(&format!("{}  Add", icon::PLUS)).click();
        harness.run_steps(4);
        assert!(harness.state().macro_tab.view.palette_is_open(), "the click that opened it does not close it");
    }

    #[test]
    fn the_pop_out_recording_editor_edits_the_recording() {
        let mut harness = app_sized(1000.0, 800.0);
        let project = examples::build(2, &harness.state().registry);
        harness.state_mut().replace_document(Document::new(project, None));
        let asset = harness
            .state()
            .doc
            .project
            .assets
            .of_kind(magi_core::assets::AssetKind::Recording)
            .next()
            .expect("the example has a recording")
            .id
            .clone();
        let before = harness.state().doc.project.assets.recording(&asset).unwrap().duration_ms;
        harness.state_mut().handle_request(&egui::Context::default(), UiRequest::EditRecording(asset.clone()));
        harness.run_steps(4);
        harness.get_by_label("½× time").click();
        harness.run_steps(3);
        let after = harness.state().doc.project.assets.recording(&asset).unwrap().duration_ms;
        assert!((after - before / 2.0).abs() < 1e-6, "{before} -> {after}");
    }

    #[test]
    fn hotkeys_can_be_reset_from_the_settings_window() {
        let mut harness = app_sized(900.0, 900.0);
        harness.state_mut().settings.run_hotkey = Hotkey::none();
        harness.state_mut().settings.settings_page = SettingsPage::Hotkeys;
        harness.state_mut().settings_window.open = true;
        harness.run_steps(4);
        harness.get_by_label_contains("Reset all hotkeys").click();
        harness.run_steps(3);
        assert_eq!(harness.state().settings.run_hotkey, Hotkey::key(Key::F7));
    }
}

//! Application settings, stored as JSON in the data folder.

use std::path::{Path, PathBuf};

use magi_core::autoclicker::AutoClickerConfig;
use magi_core::hotkey::Hotkey;
use magi_core::input::Key;
use magi_core::options::AutomationOptions;
use magi_core::project::EnginePreference;
use magi_core::recording::RecordOptions;
use serde::{Deserialize, Serialize};

use crate::theme::ThemeMode;

const MAX_RECENT_FILES: usize = 8;
pub const UI_SCALE_RANGE: std::ops::RangeInclusive<f32> = 0.5..=2.5;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tab {
    #[default]
    AutoClicker,
    Macro,
    Magic,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingsPage {
    #[default]
    General,
    Appearance,
    Hotkeys,
    Recording,
    Engine,
    Ai,
}

impl SettingsPage {
    pub const ALL: [SettingsPage; 6] = [
        SettingsPage::General,
        SettingsPage::Appearance,
        SettingsPage::Hotkeys,
        SettingsPage::Recording,
        SettingsPage::Engine,
        SettingsPage::Ai,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SettingsPage::General => "General",
            SettingsPage::Appearance => "Appearance",
            SettingsPage::Hotkeys => "Hotkeys",
            SettingsPage::Recording => "Recording",
            SettingsPage::Engine => "Engine",
            SettingsPage::Ai => "AI",
        }
    }
}

/// How MacroMagi looks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub theme: ThemeMode,
    /// The theme color. `None` is monochrome.
    pub accent: Option<[u8; 3]>,
    /// A picture behind the interface: a file name in the data folder's `backgrounds`.
    pub background: Option<String>,
    /// How strongly the picture shows, `0..=1`.
    pub background_opacity: f32,
    /// Blur radius in pixels.
    pub background_blur: f32,
    /// How much the picture is darkened (lightened in light mode), `0..=1`.
    pub background_darkness: f32,
    /// How opaque panels are over the picture, `0..=1`.
    pub panel_opacity: f32,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: ThemeMode::System,
            accent: None,
            background: None,
            background_opacity: 1.0,
            background_blur: 0.0,
            background_darkness: 0.35,
            panel_opacity: 0.75,
        }
    }
}

/// The choices offered when exporting a `.magi` file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportChoices {
    /// Bundle the preset library.
    pub presets: bool,
    /// Include the automation options (stop rules, process list…).
    pub options: bool,
    /// Include the Auto Clicker's settings.
    pub clicker: bool,
}

impl Default for ExportChoices {
    fn default() -> Self {
        Self { presets: false, options: true, clicker: true }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub tab: Tab,
    /// The Auto Clicker tab's current settings.
    pub clicker: AutoClickerConfig,
    /// Stops everything, always.
    pub stop_hotkey: Hotkey,
    /// Starts and stops recording.
    pub record_hotkey: Hotkey,
    /// Runs and stops the macro, like the Run button.
    pub run_hotkey: Hotkey,
    /// Rules shared by the Auto Clicker and macros.
    pub options: AutomationOptions,
    /// Engine used for macros that do not ask for one.
    pub engine: EnginePreference,
    /// A specific Python interpreter to use instead of searching for one.
    pub python_path: Option<PathBuf>,
    /// The Python interpreters the last check found, with their versions.
    pub pythons_found: Vec<(PathBuf, String)>,
    /// The interpreter "automatic" stood for at the last check.
    pub python_auto: Option<(PathBuf, String)>,
    /// Python has been looked for at least once.
    pub python_checked: bool,
    /// The Macro tab shows only Run, Stop and the console.
    pub lightweight: bool,
    pub record: RecordOptions,
    /// Add a Play Recording block to the graph as soon as a recording ends.
    pub auto_insert_recording: bool,
    pub appearance: Appearance,
    pub ui_scale: f32,
    pub always_on_top: bool,
    /// Keep MacroMagi's own window out of screen captures, so it can neither
    /// hide what a macro is looking for nor show up in template images.
    pub hide_from_capture: bool,
    /// Run macros without sending any input; the console shows what would be sent.
    pub dry_run: bool,
    /// What the export dialog offers by default.
    pub export: ExportChoices,
    pub show_console: bool,
    /// View › Grid: the canvas grid.
    pub show_grid: bool,
    /// View › Block activity: blocks glow while they run.
    pub show_activity: bool,
    /// View › Variables: the panel listing the macro's variables.
    pub show_variables: bool,
    /// A sound and a desktop notification when a macro finishes by itself.
    pub notify_on_finish: bool,
    pub window_size: Option<[f32; 2]>,
    pub settings_size: Option<[f32; 2]>,
    pub settings_page: SettingsPage,
    /// Size of the "add block" list.
    pub palette_size: [f32; 2],
    /// Size of the pop-out recording editor.
    pub recording_window_size: Option<[f32; 2]>,
    /// Size of the Variable Rate Editor.
    pub rate_window_size: Option<[f32; 2]>,
    /// Where AI models run.
    pub ai_device: magi_ml::Device,
    pub recent_files: Vec<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            tab: Tab::AutoClicker,
            clicker: AutoClickerConfig::default(),
            stop_hotkey: Hotkey::key(Key::F8),
            record_hotkey: Hotkey::key(Key::F9),
            run_hotkey: Hotkey::key(Key::F7),
            options: AutomationOptions::default(),
            engine: EnginePreference::Auto,
            python_path: None,
            pythons_found: Vec::new(),
            python_auto: None,
            python_checked: false,
            lightweight: false,
            record: RecordOptions::default(),
            auto_insert_recording: true,
            appearance: Appearance::default(),
            ui_scale: 1.0,
            always_on_top: false,
            hide_from_capture: true,
            dry_run: false,
            export: ExportChoices::default(),
            show_console: false,
            show_grid: true,
            show_activity: true,
            show_variables: false,
            notify_on_finish: false,
            window_size: None,
            settings_size: None,
            settings_page: SettingsPage::General,
            palette_size: [280.0, 420.0],
            recording_window_size: None,
            rate_window_size: None,
            ai_device: magi_ml::Device::Auto,
            recent_files: Vec::new(),
        }
    }
}

impl Settings {
    /// Loads settings, falling back to defaults if the file is missing or damaged.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, text)
    }

    pub fn remember_file(&mut self, path: &Path) {
        self.recent_files.retain(|recent| recent != path);
        self.recent_files.insert(0, path.to_path_buf());
        self.recent_files.truncate(MAX_RECENT_FILES);
    }

    /// Puts every app hotkey back to its default. Start blocks keep theirs:
    /// they belong to the macro, not to the app.
    pub fn reset_hotkeys(&mut self) {
        let defaults = Settings::default();
        self.clicker.hotkey = defaults.clicker.hotkey;
        self.clicker.activation = defaults.clicker.activation;
        self.stop_hotkey = defaults.stop_hotkey;
        self.record_hotkey = defaults.record_hotkey;
        self.run_hotkey = defaults.run_hotkey;
        self.options.stop_hotkeys.clear();
    }

    /// Everything back to its default, except the list of recent files and
    /// where the windows are.
    pub fn reset_all(&mut self) {
        let keep = (
            std::mem::take(&mut self.recent_files),
            self.window_size,
            self.settings_size,
            self.settings_page,
            self.tab,
        );
        *self = Settings::default();
        (self.recent_files, self.window_size, self.settings_size, self.settings_page, self.tab) = keep;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_tolerate_old_files() {
        let settings = Settings { ui_scale: 1.25, tab: Tab::Macro, ..Settings::default() };
        let json = serde_json::to_string(&settings).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), settings);
        // A file from an older version, missing most fields.
        let old: Settings = serde_json::from_str(r#"{"always_on_top": true, "include_presets": true}"#).unwrap();
        assert!(old.always_on_top);
        assert_eq!(old.stop_hotkey, Hotkey::key(Key::F8));
        assert_eq!(old.appearance.theme, ThemeMode::System, "the system theme is the default");
    }

    #[test]
    fn recent_files_are_unique_and_newest_first() {
        let mut settings = Settings::default();
        for name in ["a", "b", "a"] {
            settings.remember_file(Path::new(name));
        }
        assert_eq!(settings.recent_files, [PathBuf::from("a"), PathBuf::from("b")]);
    }

    #[test]
    fn resetting_keeps_what_is_not_a_preference() {
        let mut settings = Settings::default();
        settings.remember_file(Path::new("a.magi"));
        settings.run_hotkey = Hotkey::none();
        settings.options.stop_hotkeys.push(Hotkey::key(Key::Escape));
        settings.ui_scale = 2.0;

        settings.reset_hotkeys();
        assert_eq!(settings.run_hotkey, Hotkey::key(Key::F7));
        assert!(settings.options.stop_hotkeys.is_empty());
        assert_eq!(settings.ui_scale, 2.0, "only hotkeys are reset");

        settings.reset_all();
        assert_eq!(settings.ui_scale, 1.0);
        assert_eq!(settings.recent_files, [PathBuf::from("a.magi")]);
    }
}

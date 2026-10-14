//! Application settings, stored as JSON in the portable data folder.

use std::path::{Path, PathBuf};

use magi_core::autoclicker::AutoClickerConfig;
use magi_core::hotkey::Hotkey;
use magi_core::input::Key;
use magi_core::project::EnginePreference;
use magi_core::recording::RecordOptions;
use serde::{Deserialize, Serialize};

const MAX_RECENT_FILES: usize = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tab {
    #[default]
    AutoClicker,
    Macro,
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
    /// Engine used for macros that do not ask for one.
    pub engine: EnginePreference,
    /// A specific Python interpreter to use instead of searching for one.
    pub python_path: Option<PathBuf>,
    pub record: RecordOptions,
    /// Add a Play Recording block to the graph as soon as a recording ends.
    pub auto_insert_recording: bool,
    pub always_on_top: bool,
    /// Keep MacroMagi's own window out of screen captures, so it can neither
    /// hide what a macro is looking for nor show up in template images.
    pub hide_from_capture: bool,
    /// Run macros without sending any input; the console shows what would be sent.
    pub dry_run: bool,
    pub ui_scale: f32,
    /// Bundle the preset library into `.magi` files when saving.
    pub include_presets: bool,
    pub show_console: bool,
    pub window_size: Option<[f32; 2]>,
    pub recent_files: Vec<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            tab: Tab::AutoClicker,
            clicker: AutoClickerConfig::default(),
            stop_hotkey: Hotkey::key(Key::F8),
            record_hotkey: Hotkey::key(Key::F9),
            engine: EnginePreference::Auto,
            python_path: None,
            record: RecordOptions::default(),
            auto_insert_recording: true,
            always_on_top: false,
            hide_from_capture: true,
            dry_run: false,
            ui_scale: 1.0,
            include_presets: false,
            show_console: false,
            window_size: None,
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
        let old: Settings = serde_json::from_str(r#"{"always_on_top": true}"#).unwrap();
        assert!(old.always_on_top);
        assert_eq!(old.stop_hotkey, Hotkey::key(Key::F8));
    }

    #[test]
    fn recent_files_are_unique_and_newest_first() {
        let mut settings = Settings::default();
        for name in ["a", "b", "a"] {
            settings.remember_file(Path::new(name));
        }
        assert_eq!(settings.recent_files, [PathBuf::from("a"), PathBuf::from("b")]);
    }
}

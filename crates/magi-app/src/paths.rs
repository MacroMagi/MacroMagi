//! Where MacroMagi keeps its files.
//!
//! MacroMagi is portable: everything it writes goes into a `data` folder next
//! to the executable. Nothing is written to the registry, the user profile or
//! any system location – except a desktop shortcut, when the user asks for one.

use std::path::{Path, PathBuf};

/// Overrides the data folder. Mainly for development and tests.
pub const DATA_DIR_ENV: &str = "MACROMAGI_DATA_DIR";

#[derive(Clone, Debug)]
pub struct Paths {
    /// The folder containing the executable.
    pub app_dir: PathBuf,
    /// `<app_dir>/data` unless overridden.
    pub data_dir: PathBuf,
}

impl Paths {
    pub fn detect() -> Self {
        let app_dir = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."));
        let data_dir = std::env::var_os(DATA_DIR_ENV).map_or_else(|| app_dir.join("data"), PathBuf::from);
        Self { app_dir, data_dir }
    }

    pub fn settings_file(&self) -> PathBuf {
        self.data_dir.join("settings.json")
    }

    /// The user's preset library: one `.mp` file per preset.
    pub fn presets_dir(&self) -> PathBuf {
        self.data_dir.join("presets")
    }

    /// Background pictures chosen in Settings → Appearance.
    pub fn backgrounds_dir(&self) -> PathBuf {
        self.data_dir.join("backgrounds")
    }

    /// Creates the data folder. Returns a message if it cannot be written to,
    /// e.g. when the app was unpacked into a read-only location.
    pub fn ensure_data_dir(&self) -> Result<(), String> {
        std::fs::create_dir_all(&self.data_dir).map_err(|e| {
            format!(
                "MacroMagi cannot write to its data folder ({}): {e}. Settings and presets will not be saved. \
                 Move the app to a folder you can write to.",
                self.data_dir.display()
            )
        })
    }
}

//! The preset library: the `.mp` files in the data folder's `presets` directory.

use std::path::{Path, PathBuf};

use magi_core::preset::{EXTENSION, Preset};

pub struct LibraryEntry {
    pub path: PathBuf,
    pub preset: Preset,
}

#[derive(Default)]
pub struct PresetLibrary {
    dir: PathBuf,
    entries: Vec<LibraryEntry>,
}

impl PresetLibrary {
    pub fn load(dir: PathBuf) -> Self {
        let mut library = Self { dir, entries: Vec::new() };
        library.reload();
        library
    }

    /// Re-reads the folder. Files that are not valid presets are skipped.
    pub fn reload(&mut self) {
        self.entries.clear();
        let Ok(files) = std::fs::read_dir(&self.dir) else { return };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().is_some_and(|ext| ext == EXTENSION)
                && let Ok(preset) = Preset::load(&path)
            {
                self.entries.push(LibraryEntry { path, preset });
            }
        }
        self.entries.sort_by_key(|a| a.preset.name.to_lowercase());
    }

    pub fn entries(&self) -> &[LibraryEntry] {
        &self.entries
    }

    pub fn presets(&self) -> impl Iterator<Item = &Preset> {
        self.entries.iter().map(|entry| &entry.preset)
    }

    /// Saves a preset into the library, replacing one with the same name.
    pub fn save(&mut self, preset: &Preset) -> Result<PathBuf, String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let path = self.dir.join(format!("{}.{EXTENSION}", preset.file_stem()));
        preset.save(&path).map_err(|e| e.to_string())?;
        self.reload();
        Ok(path)
    }

    /// Copies a preset file from elsewhere into the library.
    pub fn import(&mut self, file: &Path) -> Result<Preset, String> {
        let preset = Preset::load(file).map_err(|e| e.to_string())?;
        self.save(&preset)?;
        Ok(preset)
    }

    pub fn delete(&mut self, path: &Path) {
        let _ = std::fs::remove_file(path);
        self.reload();
    }
}

#[cfg(test)]
mod tests {
    use magi_core::autoclicker::AutoClickerConfig;
    use magi_core::preset::PresetBody;

    use super::*;

    #[test]
    fn presets_are_saved_listed_and_deleted() {
        let dir = std::env::temp_dir().join(format!("magi-presets-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut library = PresetLibrary::load(dir.clone());
        assert!(library.entries().is_empty());

        let preset = Preset {
            name: "Rapid fire".into(),
            description: String::new(),
            body: PresetBody::AutoClicker(AutoClickerConfig { interval_ms: 20.0, ..Default::default() }),
        };
        let path = library.save(&preset).unwrap();
        assert_eq!(path.file_name().unwrap(), "Rapid fire.mp");
        std::fs::write(dir.join("junk.mp"), "not a preset").unwrap();
        library.reload();
        assert_eq!(library.presets().collect::<Vec<_>>(), vec![&preset], "invalid files are ignored");

        library.delete(&path);
        assert!(library.entries().is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}

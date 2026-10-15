//! Projects: the `.magi` file format.
//!
//! A `.magi` file is a standard ZIP archive (rename it to `.zip` to look inside):
//!
//! ```text
//! project.json        the macro itself – a readable text file with the node graph
//! assets/<id>.png     images linked from nodes
//! assets/<id>.json    recordings linked from nodes
//! presets/<name>.mp   presets bundled on export (optional)
//! ```
//!
//! `project.json` lists every other file, so nothing in the archive is found
//! by guessing. See `docs/FILE_FORMAT.md` for the full specification.

use std::io::{Read, Seek, Write};
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::assets::{Asset, AssetData, AssetKind, AssetStore};
use crate::autoclicker::AutoClickerConfig;
use crate::error::{Error, Result};
use crate::graph::Graph;
use crate::nodes::{Registry, ids};
use crate::options::AutomationOptions;
use crate::preset::Preset;

pub const FORMAT: &str = "macromagi.project";
pub const FORMAT_VERSION: u32 = 1;
pub const EXTENSION: &str = "magi";

const MANIFEST: &str = "project.json";
/// Upper bound for any single file read from an archive, as a guard against
/// corrupt or hostile files.
const MAX_ENTRY_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectMeta {
    pub name: String,
    pub description: String,
    pub author: String,
}

impl Default for ProjectMeta {
    fn default() -> Self {
        Self { name: "Untitled macro".into(), description: String::new(), author: String::new() }
    }
}

/// Which engine a macro asks to be run with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnginePreference {
    /// Follow the application setting.
    #[default]
    Auto,
    Python,
    Native,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Project {
    pub meta: ProjectMeta,
    pub engine: EnginePreference,
    pub graph: Graph,
    pub assets: AssetStore,
    /// Presets bundled with the project.
    pub presets: Vec<Preset>,
    /// Automation options (process rules, auto-stop keys, …) saved with the
    /// project. Applied when the project is opened.
    pub options: Option<AutomationOptions>,
    /// Auto Clicker settings saved with the project.
    pub clicker: Option<AutoClickerConfig>,
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    format: String,
    version: u32,
    /// Version of MacroMagi that wrote the file. Informational.
    #[serde(default)]
    app: String,
    #[serde(default)]
    meta: ProjectMeta,
    #[serde(default)]
    engine: EnginePreference,
    graph: Graph,
    #[serde(default)]
    assets: Vec<AssetEntry>,
    /// Paths of bundled presets inside the archive.
    #[serde(default)]
    presets: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    options: Option<AutomationOptions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    clicker: Option<AutoClickerConfig>,
}

#[derive(Serialize, Deserialize)]
struct AssetEntry {
    id: String,
    name: String,
    kind: String,
    file: String,
}

impl Project {
    /// An empty project.
    pub fn new() -> Self {
        Self { graph: Graph::new(), ..Self::default() }
    }

    /// A new project with a single Start block, ready to build on.
    pub fn starter(registry: &Registry) -> Self {
        let mut project = Self::new();
        project.graph.add(registry, ids::START, [0.0, 0.0]).expect("the Start node is built in");
        project
    }

    pub fn write_to<W: Write + Seek>(&self, writer: W) -> Result<()> {
        let mut zip = ZipWriter::new(writer);
        let text = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        // PNGs are already compressed; storing them keeps saving fast.
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);

        let mut entries = Vec::new();
        for asset in self.assets.iter() {
            let kind = asset.kind();
            let file = format!("assets/{}.{}", asset.id, kind.extension());
            match &asset.data {
                AssetData::Image(bytes) => {
                    zip.start_file(file.as_str(), stored)?;
                    zip.write_all(bytes)?;
                }
                AssetData::Recording(recording) => {
                    zip.start_file(file.as_str(), text)?;
                    zip.write_all(serde_json::to_string(&**recording)?.as_bytes())?;
                }
            }
            entries.push(AssetEntry { id: asset.id.clone(), name: asset.name.clone(), kind: kind.id().into(), file });
        }

        let mut preset_files = Vec::new();
        for (index, preset) in self.presets.iter().enumerate() {
            let file = format!("presets/{:02}-{}.{}", index + 1, preset.file_stem(), crate::preset::EXTENSION);
            zip.start_file(file.as_str(), text)?;
            zip.write_all(preset.to_json()?.as_bytes())?;
            preset_files.push(file);
        }

        let manifest = Manifest {
            format: FORMAT.to_owned(),
            version: FORMAT_VERSION,
            app: crate::VERSION.to_owned(),
            meta: self.meta.clone(),
            engine: self.engine,
            graph: self.graph.clone(),
            assets: entries,
            presets: preset_files,
            options: self.options.clone(),
            clicker: self.clicker.clone(),
        };
        zip.start_file(MANIFEST, text)?;
        zip.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())?;
        zip.finish()?;
        Ok(())
    }

    pub fn read_from<R: Read + Seek>(reader: R) -> Result<Self> {
        let mut zip =
            ZipArchive::new(reader).map_err(|_| Error::format("this is not a MacroMagi project (.magi) file"))?;

        let manifest = read_entry(&mut zip, MANIFEST)
            .map_err(|_| Error::format("this archive has no project.json; it is not a MacroMagi project"))?;
        let header: serde_json::Value = serde_json::from_slice(&manifest)?;
        if header.get("format").and_then(|v| v.as_str()) != Some(FORMAT) {
            return Err(Error::format("project.json is not a MacroMagi project description"));
        }
        let version = header.get("version").and_then(|v| v.as_u64()).unwrap_or(0);
        if version > u64::from(FORMAT_VERSION) {
            return Err(Error::format(format!(
                "this project was saved by a newer MacroMagi (project format {version}); please update"
            )));
        }
        let manifest: Manifest = serde_json::from_value(header)?;

        let mut assets = AssetStore::new();
        for entry in manifest.assets {
            let kind = AssetKind::from_id(&entry.kind)
                .ok_or_else(|| Error::format(format!("asset `{}` has unknown kind `{}`", entry.id, entry.kind)))?;
            let bytes = read_entry(&mut zip, &entry.file)?;
            let data = match kind {
                AssetKind::Image => AssetData::Image(Arc::new(bytes)),
                AssetKind::Recording => AssetData::Recording(Arc::new(serde_json::from_slice(&bytes)?)),
            };
            assets.insert(Asset { id: entry.id, name: entry.name, data });
        }

        let mut presets = Vec::new();
        for file in &manifest.presets {
            let bytes = read_entry(&mut zip, file)?;
            let json = String::from_utf8(bytes).map_err(|_| Error::format(format!("`{file}` is not text")))?;
            presets.push(Preset::from_json(&json)?);
        }

        Ok(Project {
            meta: manifest.meta,
            engine: manifest.engine,
            graph: manifest.graph,
            assets,
            presets,
            options: manifest.options,
            clicker: manifest.clicker,
        })
    }

    /// Saves atomically: the file at `path` is either the old project or the
    /// complete new one, never a half-written archive.
    pub fn save(&self, path: &Path) -> Result<()> {
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        let tmp = std::path::PathBuf::from(tmp);
        let result = (|| {
            let file = std::fs::File::create(&tmp)?;
            self.write_to(std::io::BufWriter::new(file))?;
            std::fs::rename(&tmp, path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result
    }

    pub fn load(path: &Path) -> Result<Self> {
        Self::read_from(std::io::BufReader::new(std::fs::File::open(path)?))
    }
}

fn read_entry<R: Read + Seek>(zip: &mut ZipArchive<R>, name: &str) -> Result<Vec<u8>> {
    let entry = zip.by_name(name).map_err(|_| Error::format(format!("the project is missing `{name}`")))?;
    let mut bytes = Vec::new();
    entry.take(MAX_ENTRY_BYTES).read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::autoclicker::AutoClickerConfig;
    use crate::graph::PortRef;
    use crate::input::{InputEvent, Key};
    use crate::nodes::ports;
    use crate::preset::PresetBody;
    use crate::recording::{RecordOptions, RecordingBuilder};

    fn sample() -> Project {
        let registry = Registry::builtin();
        let mut project = Project::starter(&registry);
        project.meta.name = "Sample".into();
        let start = project.graph.starts().next().unwrap().id;
        let play = project.graph.add(&registry, ids::PLAY_RECORDING, [0.0, 120.0]).unwrap();
        let find = project.graph.add(&registry, ids::FIND_IMAGE, [-260.0, 120.0]).unwrap();
        project.graph.connect(&registry, PortRef::new(start, ports::OUT), PortRef::new(play, ports::FLOW_IN)).unwrap();

        let mut builder = RecordingBuilder::new(RecordOptions::default(), vec![], Default::default());
        builder.push(0.0, &InputEvent::KeyDown { key: Key::KeyA });
        builder.push(80.0, &InputEvent::KeyUp { key: Key::KeyA });
        let recording = project.assets.add_recording("Recording 1", builder.finish(100.0, Some((1920, 1080))));
        project.graph.node_mut(play).unwrap().set_param("recording", recording);

        let image = project.assets.add_image("button.png", vec![0x89, b'P', b'N', b'G', 1, 2, 3]);
        project.graph.node_mut(find).unwrap().set_param("image", image);

        project.options = Some(AutomationOptions { stop_on_task_switch: true, ..AutomationOptions::default() });
        project.clicker = Some(AutoClickerConfig::default());
        project.presets.push(Preset {
            name: "Clicker".into(),
            description: "bundled".into(),
            body: PresetBody::AutoClicker(AutoClickerConfig::default()),
        });
        project
    }

    #[test]
    fn projects_round_trip_through_the_archive() {
        let project = sample();
        let mut buffer = Cursor::new(Vec::new());
        project.write_to(&mut buffer).unwrap();
        buffer.set_position(0);
        let loaded = Project::read_from(buffer).unwrap();
        assert_eq!(loaded, project);
    }

    #[test]
    fn the_archive_contains_a_readable_text_manifest() {
        let mut buffer = Cursor::new(Vec::new());
        sample().write_to(&mut buffer).unwrap();
        let mut zip = ZipArchive::new(buffer).unwrap();
        let manifest = String::from_utf8(read_entry(&mut zip, MANIFEST).unwrap()).unwrap();
        assert!(manifest.contains(r#""format": "macromagi.project""#));
        assert!(manifest.contains(r#""type": "flow.start""#));
        assert!(manifest.contains("assets/rec-1.json"));
        assert!(read_entry(&mut zip, "presets/01-Clicker.mp").is_ok());
    }

    #[test]
    fn other_files_are_refused_with_a_clear_message() {
        let err = Project::read_from(Cursor::new(b"definitely not a zip".to_vec())).unwrap_err();
        assert!(err.to_string().contains("not a MacroMagi project"));
    }

    #[test]
    fn saving_is_atomic_and_loadable() {
        let dir = std::env::temp_dir().join(format!("magi-core-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sample.magi");
        let project = sample();
        project.save(&path).unwrap();
        project.save(&path).unwrap(); // overwriting works too
        assert_eq!(Project::load(&path).unwrap(), project);
        assert!(!dir.join("sample.magi.tmp").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

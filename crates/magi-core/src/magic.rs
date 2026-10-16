//! AI models, as the Magic tab prepares them and macros use them.
//!
//! A `.magic` file is a ZIP archive holding a small vision model (an ONNX
//! file) together with everything needed to use it: how to prepare a picture
//! for it, which part of the screen to show it, what its answers mean, what to
//! do when it gives one, which of those settings a person may adjust, the
//! blocks to use it with, and – optionally – the pictures it was trained or
//! fine-tuned on.
//!
//! ```text
//! magic.json        required – the description below
//! model/model.onnx  the model (left out when "include the model" is off)
//! data/train/…      training pictures, one folder per class (optional)
//! data/tune/…       fine-tuning pictures, one folder per class (optional)
//! ```
//!
//! Macros embed a model as an asset: the `.magic` file without its data.

use std::collections::BTreeMap;
use std::io::{Read, Seek, Write};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::error::{Error, Result};
use crate::geom::{Displays, Rect};
use crate::graph::Fragment;
use crate::input::Key;

pub const FORMAT: &str = "macromagi.magic";
pub const FORMAT_VERSION: u32 = 1;
pub const EXTENSION: &str = "magic";
const MANIFEST: &str = "magic.json";
const MODEL_FILE: &str = "model/model.onnx";

/// The standard ImageNet colour statistics most vision models are trained with.
pub const IMAGENET_MEAN: [f32; 3] = [0.485, 0.456, 0.406];
pub const IMAGENET_STD: [f32; 3] = [0.229, 0.224, 0.225];

/// How the numbers of a picture are laid out in the model's input.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Layout {
    /// Channels first: `[1, 3, height, width]` (PyTorch's habit).
    #[default]
    Nchw,
    /// Channels last: `[1, height, width, 3]` (TensorFlow's habit).
    Nhwc,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorOrder {
    #[default]
    Rgb,
    Bgr,
}

/// How a picture is turned into the numbers the model reads: resized to
/// `width × height`, each colour value `v` (0–255) becoming
/// `(v × scale − mean) / std`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InputSpec {
    /// The name of the model's input.
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub layout: Layout,
    pub color: ColorOrder,
    pub scale: f32,
    pub mean: [f32; 3],
    pub std: [f32; 3],
    /// The model gives raw scores: turn them into probabilities (softmax).
    pub softmax: bool,
}

impl Default for InputSpec {
    fn default() -> Self {
        Self {
            name: String::new(),
            width: 224,
            height: 224,
            layout: Layout::Nchw,
            color: ColorOrder::Rgb,
            scale: 1.0 / 255.0,
            mean: IMAGENET_MEAN,
            std: IMAGENET_STD,
            softmax: true,
        }
    }
}

/// Which part of the screen the model looks at.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CaptureArea {
    /// A square in the middle of the display, as tall as this share of the
    /// display's height – so it covers the same part of the picture at any
    /// resolution.
    Center { height_share: f64 },
    /// A fixed area.
    Region { rect: Rect },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CaptureSpec {
    pub area: CaptureArea,
    /// The display a centred square is on (0 = primary).
    pub display: u8,
}

impl Default for CaptureSpec {
    fn default() -> Self {
        // 224 pixels on a 1080-pixel-high display.
        Self { area: CaptureArea::Center { height_share: 224.0 / 1080.0 }, display: 0 }
    }
}

impl CaptureSpec {
    /// The area on the desktop, given the displays. `display` overrides the
    /// spec's own display for a centred square.
    pub fn rect(&self, displays: &Displays, display: Option<u8>) -> Option<Rect> {
        match self.area {
            CaptureArea::Region { rect } => Some(displays.rect_to_global(rect)),
            CaptureArea::Center { height_share } => {
                let screen = displays.get(display.unwrap_or(self.display)).or_else(|| displays.get(0))?;
                let side = ((f64::from(screen.h) * height_share).round() as u32).clamp(8, screen.w.min(screen.h));
                Some(Rect::new(
                    screen.x + ((screen.w - side) / 2) as i32,
                    screen.y + ((screen.h - side) / 2) as i32,
                    side,
                    side,
                ))
            }
        }
    }
}

/// One answer a classifying model can give.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClassSpec {
    pub label: String,
    /// Whether this answer calls for the action.
    pub act: bool,
    /// How sure the model must be, `0.0..=1.0`, before acting on it.
    pub threshold: f64,
    /// Milliseconds to wait before acting.
    pub delay_ms: f64,
}

/// What is done when the model gives an answer that calls for action.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ActionSpec {
    pub key: Key,
    /// How long the key is held.
    pub hold_ms: f64,
    /// After acting, answers are ignored for this long, so one event is not
    /// acted on twice.
    pub cooldown_ms: f64,
}

impl Default for ActionSpec {
    fn default() -> Self {
        Self { key: Key::Space, hold_ms: 5.0, cooldown_ms: 500.0 }
    }
}

/// What an adjustable setting changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SettingTarget {
    /// A class's threshold; the setting is a percentage.
    Threshold {
        class: usize,
    },
    /// A class's delay, in milliseconds.
    Delay {
        class: usize,
    },
    Cooldown,
    Hold,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingKind {
    #[default]
    Percent,
    Milliseconds,
}

/// A setting a person may adjust – in the Magic tab, or per block in a macro –
/// that changes one or more values of the model's configuration at once.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelSetting {
    pub key: String,
    pub label: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub help: String,
    pub kind: SettingKind,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub value: f64,
    pub targets: Vec<SettingTarget>,
}

/// A picture kept for training or fine-tuning, with the class it shows.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DataItem {
    /// Its path inside the archive.
    pub file: String,
    pub class: usize,
}

/// What a `.magic` file carries besides its description.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Includes {
    pub model: bool,
    pub blocks: bool,
    pub training: bool,
    pub finetune: bool,
}

impl Default for Includes {
    fn default() -> Self {
        Self { model: true, blocks: true, training: true, finetune: true }
    }
}

/// The description of a model: `magic.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MagicModel {
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub author: String,
    /// Where the model came from, and its licence, as found when importing.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub source: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub license: String,
    pub input: InputSpec,
    pub capture: CaptureSpec,
    pub classes: Vec<ClassSpec>,
    pub action: ActionSpec,
    pub settings: Vec<ModelSetting>,
    /// Blocks to use the model with, ready to add to a macro.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocks: Option<Fragment>,
    pub include: Includes,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub training: Vec<DataItem>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub finetune: Vec<DataItem>,
}

impl Default for MagicModel {
    fn default() -> Self {
        Self {
            name: "New model".into(),
            description: String::new(),
            author: String::new(),
            source: String::new(),
            license: String::new(),
            input: InputSpec::default(),
            capture: CaptureSpec::default(),
            classes: Vec::new(),
            action: ActionSpec::default(),
            settings: Vec::new(),
            blocks: None,
            include: Includes::default(),
            training: Vec::new(),
            finetune: Vec::new(),
        }
    }
}

/// What a model's answer means, after its configuration is applied.
#[derive(Clone, Debug, PartialEq)]
pub struct Decision {
    pub class: usize,
    pub label: String,
    /// The probability of that class, `0.0..=1.0`.
    pub confidence: f64,
    /// The answer calls for the action, and the model is sure enough.
    pub act: bool,
    pub delay_ms: f64,
}

impl MagicModel {
    /// The configuration with the adjustable settings applied: each setting's
    /// own value, or the one in `overrides` (by key) when it has one.
    pub fn configured(&self, overrides: &Map<String, Value>) -> MagicModel {
        let mut out = self.clone();
        for setting in &self.settings {
            let value = overrides.get(&setting.key).and_then(Value::as_f64).unwrap_or(setting.value);
            let value = value.clamp(setting.min.min(setting.max), setting.max.max(setting.min));
            for target in &setting.targets {
                match *target {
                    SettingTarget::Threshold { class } => {
                        if let Some(c) = out.classes.get_mut(class) {
                            c.threshold = match setting.kind {
                                SettingKind::Percent => value / 100.0,
                                SettingKind::Milliseconds => value,
                            };
                        }
                    }
                    SettingTarget::Delay { class } => {
                        if let Some(c) = out.classes.get_mut(class) {
                            c.delay_ms = value;
                        }
                    }
                    SettingTarget::Cooldown => out.action.cooldown_ms = value,
                    SettingTarget::Hold => out.action.hold_ms = value,
                }
            }
        }
        out
    }

    /// What the probabilities a model gave mean: the likeliest class, and
    /// whether to act on it.
    pub fn decide(&self, probabilities: &[f32]) -> Decision {
        let (class, confidence) = probabilities
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map_or((0, 0.0), |(i, p)| (i, f64::from(*p)));
        let spec = self.classes.get(class);
        Decision {
            class,
            label: spec.map_or_else(|| format!("Class {class}"), |c| c.label.clone()),
            confidence,
            act: spec.is_some_and(|c| c.act && confidence >= c.threshold),
            delay_ms: spec.map_or(0.0, |c| c.delay_ms),
        }
    }

    /// The label of a class.
    pub fn label(&self, class: usize) -> String {
        self.classes.get(class).map_or_else(|| format!("Class {class}"), |c| c.label.clone())
    }
}

/// A whole `.magic` file in memory.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MagicFile {
    pub model: MagicModel,
    /// The ONNX model. Empty when the file does not include it.
    pub onnx: Vec<u8>,
    /// Training and fine-tuning pictures, by path inside the archive.
    pub data: BTreeMap<String, Vec<u8>>,
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    format: String,
    version: u32,
    #[serde(default)]
    app: String,
    #[serde(flatten)]
    model: MagicModel,
}

impl MagicFile {
    pub fn write_to<W: Write + Seek>(&self, writer: W) -> Result<()> {
        let mut zip = ZipWriter::new(writer);
        let text = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let mut model = self.model.clone();
        let include = model.include;
        if include.model && !self.onnx.is_empty() {
            zip.start_file(MODEL_FILE, text)?;
            zip.write_all(&self.onnx)?;
        }
        if !include.blocks {
            model.blocks = None;
        }
        let keep = |item: &DataItem, on: bool| on && self.data.contains_key(&item.file);
        model.training.retain(|item| keep(item, include.training));
        model.finetune.retain(|item| keep(item, include.finetune));
        for item in model.training.iter().chain(&model.finetune) {
            zip.start_file(item.file.as_str(), stored)?;
            zip.write_all(&self.data[&item.file])?;
        }
        let manifest = Manifest { format: FORMAT.into(), version: FORMAT_VERSION, app: crate::VERSION.into(), model };
        zip.start_file(MANIFEST, text)?;
        zip.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())?;
        zip.finish()?;
        Ok(())
    }

    pub fn read_from<R: Read + Seek>(reader: R) -> Result<Self> {
        let mut zip =
            ZipArchive::new(reader).map_err(|_| Error::format("this is not a MacroMagi model (.magic) file"))?;
        let manifest = read(&mut zip, MANIFEST)
            .map_err(|_| Error::format("this archive has no magic.json; it is not a MacroMagi model"))?;
        let header: Value = serde_json::from_slice(&manifest)?;
        if header.get("format").and_then(Value::as_str) != Some(FORMAT) {
            return Err(Error::format("magic.json is not a MacroMagi model description"));
        }
        let version = header.get("version").and_then(Value::as_u64).unwrap_or(0);
        if version > u64::from(FORMAT_VERSION) {
            return Err(Error::format(format!(
                "this model was saved by a newer MacroMagi (model format {version}); please update"
            )));
        }
        let manifest: Manifest = serde_json::from_value(header)?;
        let onnx = read(&mut zip, MODEL_FILE).unwrap_or_default();
        let mut data = BTreeMap::new();
        for item in manifest.model.training.iter().chain(&manifest.model.finetune) {
            if let Ok(bytes) = read(&mut zip, &item.file) {
                data.insert(item.file.clone(), bytes);
            }
        }
        Ok(Self { model: manifest.model, onnx, data })
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut out = std::io::Cursor::new(Vec::new());
        self.write_to(&mut out)?;
        Ok(out.into_inner())
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        Self::read_from(std::io::Cursor::new(bytes))
    }

    /// The file as a macro keeps it: the model and its configuration, without
    /// the training data.
    pub fn for_macro(&self) -> Result<Vec<u8>> {
        let mut copy = self.clone();
        copy.model.include = Includes { model: true, blocks: true, training: false, finetune: false };
        copy.to_bytes()
    }

    pub fn save(&self, path: &std::path::Path) -> Result<()> {
        Ok(std::fs::write(path, self.to_bytes()?)?)
    }

    pub fn load(path: &std::path::Path) -> Result<Self> {
        Self::from_bytes(&std::fs::read(path)?)
    }
}

fn read<R: Read + Seek>(zip: &mut ZipArchive<R>, name: &str) -> Result<Vec<u8>> {
    let mut entry = zip.by_name(name).map_err(|_| Error::format(format!("`{name}` is missing")))?;
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry.read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_classes() -> MagicModel {
        MagicModel {
            name: "Test".into(),
            classes: vec![
                ClassSpec { label: "Nothing".into(), ..ClassSpec::default() },
                ClassSpec { label: "Press now".into(), act: true, threshold: 0.5, delay_ms: 0.0 },
            ],
            settings: vec![ModelSetting {
                key: "thresh".into(),
                label: "Confidence".into(),
                kind: SettingKind::Percent,
                min: 50.0,
                max: 100.0,
                default: 60.0,
                value: 60.0,
                targets: vec![SettingTarget::Threshold { class: 1 }],
                ..ModelSetting::default()
            }],
            ..MagicModel::default()
        }
    }

    #[test]
    fn settings_change_what_the_model_acts_on() {
        let model = two_classes().configured(&Map::new());
        assert_eq!(model.classes[1].threshold, 0.6);
        let sure = model.decide(&[0.3, 0.7]);
        assert!(sure.act && sure.label == "Press now");
        assert!(!model.decide(&[0.45, 0.55]).act, "not sure enough");
        let mut overrides = Map::new();
        overrides.insert("thresh".into(), serde_json::json!(51));
        assert!(two_classes().configured(&overrides).decide(&[0.45, 0.55]).act);
        assert!(!model.decide(&[0.9, 0.1]).act, "the likeliest class does not act");
    }

    #[test]
    fn a_centred_square_scales_with_the_display() {
        let displays = Displays::new(vec![Rect::new(0, 0, 1920, 1080), Rect::new(1920, 0, 2560, 1440)]);
        let spec = CaptureSpec::default();
        assert_eq!(spec.rect(&displays, None), Some(Rect::new(848, 428, 224, 224)));
        let other = spec.rect(&displays, Some(1)).unwrap();
        assert_eq!((other.w, other.x, other.y), (299, 1920 + 1130, 570));
    }

    #[test]
    fn files_round_trip_and_leave_out_what_is_switched_off() {
        let mut file = MagicFile { model: two_classes(), onnx: vec![1, 2, 3], data: BTreeMap::new() };
        file.data.insert("data/train/1/a.png".into(), vec![9; 10]);
        file.model.training.push(DataItem { file: "data/train/1/a.png".into(), class: 1 });
        let back = MagicFile::from_bytes(&file.to_bytes().unwrap()).unwrap();
        assert_eq!(back, file);

        let slim = MagicFile::from_bytes(&file.for_macro().unwrap()).unwrap();
        assert!(slim.data.is_empty() && slim.model.training.is_empty());
        assert_eq!(slim.onnx, vec![1, 2, 3]);
        assert!(MagicFile::from_bytes(b"not a zip").is_err());
    }
}

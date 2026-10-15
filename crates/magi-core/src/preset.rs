//! Presets: the `.mp` file format.
//!
//! A preset is a small, shareable JSON file holding either a set of Auto
//! Clicker settings or a group of pre-configured nodes (with the images and
//! recordings they use embedded, so the file is self-contained).
//!
//! Presets live in the user's preset library, and can also travel inside a
//! `.magi` project when it is exported with "include presets".

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::assets::{AssetData, AssetStore};
use crate::autoclicker::AutoClickerConfig;
use crate::error::{Error, Result};
use crate::graph::{Fragment, Graph, NodeId};
use crate::nodes::{ParamKind, Registry};
use crate::recording::Recording;

pub const FORMAT: &str = "macromagi.preset";
pub const FORMAT_VERSION: u32 = 1;
pub const EXTENSION: &str = "mp";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(flatten)]
    pub body: PresetBody,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum PresetBody {
    AutoClicker(AutoClickerConfig),
    Nodes(NodePreset),
}

impl PresetBody {
    pub fn kind_label(&self) -> &'static str {
        match self {
            PresetBody::AutoClicker(_) => "Auto Clicker",
            PresetBody::Nodes(_) => "Nodes",
        }
    }
}

/// A group of nodes plus the assets they reference.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NodePreset {
    #[serde(flatten)]
    pub fragment: Fragment,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assets: Vec<EmbeddedAsset>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EmbeddedAsset {
    pub id: String,
    pub name: String,
    #[serde(flatten)]
    pub data: EmbeddedData,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EmbeddedData {
    /// Base64 of the encoded image file.
    Image {
        base64: String,
    },
    Recording {
        recording: Recording,
    },
}

/// Parameter keys of a node that hold asset ids.
fn asset_params<'a>(registry: &'a Registry, kind: &str) -> impl Iterator<Item = &'a str> {
    registry
        .get(kind)
        .into_iter()
        .flat_map(|def| def.params.iter())
        .filter(|p| matches!(p.kind, ParamKind::Image | ParamKind::Recording))
        .map(|p| p.key.as_str())
}

impl NodePreset {
    /// Captures the given nodes, the wires between them and every asset they use.
    pub fn capture(graph: &Graph, ids: &BTreeSet<NodeId>, assets: &AssetStore, registry: &Registry) -> Self {
        let fragment = graph.fragment(ids);
        let mut used = BTreeSet::new();
        for node in &fragment.nodes {
            for key in asset_params(registry, &node.kind) {
                if let Some(Value::String(id)) = node.params.get(key) {
                    used.insert(id.clone());
                }
            }
        }
        let assets = used
            .iter()
            .filter_map(|id| assets.get(id))
            .map(|asset| EmbeddedAsset {
                id: asset.id.clone(),
                name: asset.name.clone(),
                data: match &asset.data {
                    AssetData::Image(bytes) => EmbeddedData::Image { base64: BASE64.encode(bytes.as_slice()) },
                    AssetData::Recording(recording) => EmbeddedData::Recording { recording: (**recording).clone() },
                },
            })
            .collect();
        Self { fragment, assets }
    }

    /// Adds the preset's nodes to a graph at `at`, importing its assets.
    /// Returns the ids of the new nodes.
    pub fn insert_into(
        &self,
        graph: &mut Graph,
        assets: &mut AssetStore,
        registry: &Registry,
        at: [f32; 2],
    ) -> Result<Vec<NodeId>> {
        // Assets get ids that are valid in the destination, which may differ
        // from the ids they had where the preset was made.
        let mut renamed: HashMap<&str, String> = HashMap::new();
        for embedded in &self.assets {
            let id = match &embedded.data {
                EmbeddedData::Image { base64 } => {
                    let bytes = BASE64
                        .decode(base64)
                        .map_err(|e| Error::format(format!("preset image `{}` is corrupt: {e}", embedded.name)))?;
                    assets.add_image(embedded.name.clone(), bytes)
                }
                EmbeddedData::Recording { recording } => assets.add_recording(embedded.name.clone(), recording.clone()),
            };
            renamed.insert(&embedded.id, id);
        }

        let mut fragment = self.fragment.clone();
        for node in &mut fragment.nodes {
            for key in asset_params(registry, &node.kind.clone()) {
                if let Some(Value::String(id)) = node.params.get_mut(key)
                    && let Some(new_id) = renamed.get(id.as_str())
                {
                    *id = new_id.clone();
                }
            }
        }
        Ok(graph.insert_fragment(&fragment, at))
    }
}

#[derive(Serialize, Deserialize)]
struct PresetFile {
    format: String,
    version: u32,
    #[serde(default)]
    app: String,
    #[serde(flatten)]
    preset: Preset,
}

impl Preset {
    pub fn to_json(&self) -> Result<String> {
        let file = PresetFile {
            format: FORMAT.to_owned(),
            version: FORMAT_VERSION,
            app: crate::VERSION.to_owned(),
            preset: self.clone(),
        };
        Ok(serde_json::to_string_pretty(&file)?)
    }

    pub fn from_json(json: &str) -> Result<Self> {
        let header: Value = serde_json::from_str(json)?;
        if header.get("format").and_then(Value::as_str) != Some(FORMAT) {
            return Err(Error::format("this is not a MacroMagi preset (.mp) file"));
        }
        let version = header.get("version").and_then(Value::as_u64).unwrap_or(0);
        if version > u64::from(FORMAT_VERSION) {
            return Err(Error::format(format!(
                "this preset was made by a newer MacroMagi (preset format {version}); please update"
            )));
        }
        let file: PresetFile = serde_json::from_value(header)?;
        Ok(file.preset)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        Ok(std::fs::write(path, self.to_json()?)?)
    }

    pub fn load(path: &Path) -> Result<Self> {
        Self::from_json(&std::fs::read_to_string(path)?)
    }

    /// A safe file name (without extension) derived from the preset's name.
    pub fn file_stem(&self) -> String {
        let stem: String = self
            .name
            .chars()
            .map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | ' ') { c } else { '_' })
            .collect();
        let stem = stem.trim();
        if stem.is_empty() { "preset".to_owned() } else { stem.to_owned() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nodes::ids;

    #[test]
    fn auto_clicker_presets_round_trip() {
        let preset = Preset {
            name: "Fast left click".into(),
            description: String::new(),
            body: PresetBody::AutoClicker(AutoClickerConfig {
                settings: crate::autoclicker::ClickerSettings { interval_ms: 10.0, ..Default::default() },
                ..AutoClickerConfig::default()
            }),
        };
        let json = preset.to_json().unwrap();
        assert!(json.contains(r#""kind": "auto_clicker""#));
        assert_eq!(Preset::from_json(&json).unwrap(), preset);
    }

    #[test]
    fn foreign_and_future_files_are_refused() {
        assert!(Preset::from_json(r#"{"format":"something.else"}"#).is_err());
        let future = r#"{"format":"macromagi.preset","version":999,"name":"x","kind":"auto_clicker","data":{}}"#;
        assert!(Preset::from_json(future).unwrap_err().to_string().contains("newer"));
    }

    #[test]
    fn node_presets_carry_their_assets() {
        let registry = Registry::builtin();
        let mut assets = AssetStore::new();
        let image = assets.add_image("button", vec![9, 9, 9]);
        let mut graph = Graph::new();
        let find = graph.add(&registry, ids::FIND_IMAGE, [40.0, 40.0]).unwrap();
        graph.node_mut(find).unwrap().set_param("image", &image);

        let preset = NodePreset::capture(&graph, &BTreeSet::from([find]), &assets, &registry);
        assert_eq!(preset.assets.len(), 1);

        // Insert into a brand new project: the image comes along.
        let (mut other_graph, mut other_assets) = (Graph::new(), AssetStore::new());
        let ids = preset.insert_into(&mut other_graph, &mut other_assets, &registry, [0.0, 0.0]).unwrap();
        let node = other_graph.node(ids[0]).unwrap();
        let linked: String = node.param("image").unwrap();
        assert_eq!(other_assets.image(&linked).unwrap().as_slice(), [9, 9, 9]);
        assert_eq!(node.pos, [0.0, 0.0]);
    }

    #[test]
    fn file_stems_are_filesystem_safe() {
        let preset = Preset {
            name: "a/b:c?".into(),
            description: String::new(),
            body: PresetBody::AutoClicker(Default::default()),
        };
        assert_eq!(preset.file_stem(), "a_b_c_");
    }
}

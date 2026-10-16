//! Assets: the data files a macro links to – template images, recordings,
//! sounds and AI models.
//!
//! Nodes refer to assets by id, so the node graph itself stays a small,
//! readable text document while the bulky data lives next to it.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::recording::Recording;

#[derive(Clone, Debug)]
pub enum AssetData {
    /// An encoded image file (PNG), exactly as stored in the project archive.
    Image(Arc<Vec<u8>>),
    Recording(Arc<Recording>),
    /// A sound to listen for, as a WAV file (mono, 16-bit).
    Audio(Arc<Vec<u8>>),
    /// An AI model: a `.magic` file without its training data.
    Model(Arc<Vec<u8>>),
}

impl PartialEq for AssetData {
    fn eq(&self, other: &Self) -> bool {
        // Assets are shared by pointer between undo snapshots; comparing the
        // pointers first keeps "did anything change?" checks cheap.
        match (self, other) {
            (AssetData::Image(a), AssetData::Image(b)) => Arc::ptr_eq(a, b) || a == b,
            (AssetData::Recording(a), AssetData::Recording(b)) => Arc::ptr_eq(a, b) || a == b,
            (AssetData::Audio(a), AssetData::Audio(b)) => Arc::ptr_eq(a, b) || a == b,
            (AssetData::Model(a), AssetData::Model(b)) => Arc::ptr_eq(a, b) || a == b,
            _ => false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetKind {
    Image,
    Recording,
    Audio,
    Model,
}

impl AssetKind {
    pub fn id(self) -> &'static str {
        match self {
            AssetKind::Image => "image",
            AssetKind::Recording => "recording",
            AssetKind::Audio => "audio",
            AssetKind::Model => "model",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "image" => Some(AssetKind::Image),
            "recording" => Some(AssetKind::Recording),
            "audio" => Some(AssetKind::Audio),
            "model" => Some(AssetKind::Model),
            _ => None,
        }
    }

    /// File extension used inside project archives.
    pub fn extension(self) -> &'static str {
        match self {
            AssetKind::Image => "png",
            AssetKind::Recording => "json",
            AssetKind::Audio => "wav",
            AssetKind::Model => crate::magic::EXTENSION,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub id: String,
    /// Name shown to users. Not unique, freely editable.
    pub name: String,
    pub data: AssetData,
}

impl Asset {
    pub fn kind(&self) -> AssetKind {
        match self.data {
            AssetData::Image(_) => AssetKind::Image,
            AssetData::Recording(_) => AssetKind::Recording,
            AssetData::Audio(_) => AssetKind::Audio,
            AssetData::Model(_) => AssetKind::Model,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AssetStore {
    assets: BTreeMap<String, Asset>,
}

impl AssetStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.assets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.assets.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Asset> {
        self.assets.values()
    }

    pub fn of_kind(&self, kind: AssetKind) -> impl Iterator<Item = &Asset> {
        self.iter().filter(move |a| a.kind() == kind)
    }

    pub fn get(&self, id: &str) -> Option<&Asset> {
        self.assets.get(id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Asset> {
        self.assets.get_mut(id)
    }

    pub fn contains(&self, id: &str) -> bool {
        self.assets.contains_key(id)
    }

    pub fn image(&self, id: &str) -> Option<&Arc<Vec<u8>>> {
        match &self.get(id)?.data {
            AssetData::Image(bytes) => Some(bytes),
            _ => None,
        }
    }

    pub fn audio(&self, id: &str) -> Option<&Arc<Vec<u8>>> {
        match &self.get(id)?.data {
            AssetData::Audio(bytes) => Some(bytes),
            _ => None,
        }
    }

    /// Adds a sound (a WAV file). Identical sounds share one asset.
    pub fn add_audio(&mut self, name: impl Into<String>, wav: Vec<u8>) -> String {
        let id = format!("snd-{:016x}", fnv1a(&wav));
        self.assets.entry(id.clone()).or_insert_with(|| Asset {
            id: id.clone(),
            name: name.into(),
            data: AssetData::Audio(Arc::new(wav)),
        });
        id
    }

    /// An AI model (`.magic` bytes).
    pub fn model(&self, id: &str) -> Option<&Arc<Vec<u8>>> {
        match &self.get(id)?.data {
            AssetData::Model(bytes) => Some(bytes),
            _ => None,
        }
    }

    /// Adds an AI model (`.magic` bytes). Identical models share one asset.
    pub fn add_model(&mut self, name: impl Into<String>, magic: Vec<u8>) -> String {
        let id = format!("mdl-{:016x}", fnv1a(&magic));
        self.assets.entry(id.clone()).or_insert_with(|| Asset {
            id: id.clone(),
            name: name.into(),
            data: AssetData::Model(Arc::new(magic)),
        });
        id
    }

    pub fn recording(&self, id: &str) -> Option<&Arc<Recording>> {
        match &self.get(id)?.data {
            AssetData::Recording(recording) => Some(recording),
            _ => None,
        }
    }

    /// Mutable access to a recording, copying it first if an undo snapshot still shares it.
    pub fn recording_mut(&mut self, id: &str) -> Option<&mut Recording> {
        match &mut self.assets.get_mut(id)?.data {
            AssetData::Recording(recording) => Some(Arc::make_mut(recording)),
            _ => None,
        }
    }

    /// Adds an encoded image. Identical images share one asset.
    pub fn add_image(&mut self, name: impl Into<String>, png: Vec<u8>) -> String {
        let id = format!("img-{:016x}", fnv1a(&png));
        self.assets.entry(id.clone()).or_insert_with(|| Asset {
            id: id.clone(),
            name: name.into(),
            data: AssetData::Image(Arc::new(png)),
        });
        id
    }

    pub fn add_recording(&mut self, name: impl Into<String>, recording: Recording) -> String {
        let id = self.unused_id("rec");
        self.assets.insert(
            id.clone(),
            Asset { id: id.clone(), name: name.into(), data: AssetData::Recording(Arc::new(recording)) },
        );
        id
    }

    /// Inserts an asset under its own id, replacing any asset with that id.
    pub fn insert(&mut self, asset: Asset) {
        self.assets.insert(asset.id.clone(), asset);
    }

    pub fn remove(&mut self, id: &str) -> Option<Asset> {
        self.assets.remove(id)
    }

    /// The first of `prefix-1`, `prefix-2`, … that is not taken.
    pub fn unused_id(&self, prefix: &str) -> String {
        (1..)
            .map(|n| format!("{prefix}-{n}"))
            .find(|id| !self.assets.contains_key(id))
            .expect("an unbounded range always yields a free id")
    }
}

/// 64-bit FNV-1a. Used only to derive stable ids from content, not for security.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &b| (hash ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_images_are_stored_once() {
        let mut store = AssetStore::new();
        let a = store.add_image("first", vec![1, 2, 3]);
        let b = store.add_image("second", vec![1, 2, 3]);
        assert_eq!(a, b);
        assert_eq!(store.len(), 1);
        assert_eq!(store.get(&a).unwrap().name, "first");
    }

    #[test]
    fn recordings_get_fresh_ids_and_copy_on_write() {
        let mut store = AssetStore::new();
        let a = store.add_recording("one", Recording::default());
        let b = store.add_recording("two", Recording::default());
        assert_ne!(a, b);

        let snapshot = store.clone();
        store.recording_mut(&a).unwrap().duration_ms = 5.0;
        assert_eq!(snapshot.recording(&a).unwrap().duration_ms, 0.0);
        assert_ne!(snapshot, store);
    }
}

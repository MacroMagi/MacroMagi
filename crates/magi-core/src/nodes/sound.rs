//! Sound nodes: listening to what the computer plays, or to the microphone.
//! Like the vision nodes these are value nodes, evaluated whenever a block
//! reads them – typically a Wait Until or a Decision.

use serde::{Deserialize, Serialize};

use super::vision::FindMode;
use super::{Category, DataType, NodeDef, ParamDef, ParamKind, Registry, ids};
use crate::host::SoundSource;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundLevelParams {
    pub source: SoundSource,
    /// The microphone to use; empty for the system's default.
    pub device: String,
    /// Sound at least this loud (dBFS, 0 is the loudest possible) is detected.
    pub threshold_db: f64,
    /// Also require the pitch to be within `min_hz..=max_hz`.
    pub use_pitch: bool,
    pub min_hz: f64,
    pub max_hz: f64,
    /// How much of the most recent sound is measured, in milliseconds.
    pub window_ms: f64,
}

impl Default for SoundLevelParams {
    fn default() -> Self {
        Self {
            source: SoundSource::Output,
            device: String::new(),
            threshold_db: -30.0,
            use_pitch: false,
            min_hz: 200.0,
            max_hz: 2000.0,
            window_ms: 200.0,
        }
    }
}

impl SoundLevelParams {
    /// Whether sound of this loudness and pitch counts as detected.
    pub fn detects(&self, loudness_db: f64, pitch_hz: Option<f64>) -> bool {
        let (low, high) = (self.min_hz.min(self.max_hz), self.min_hz.max(self.max_hz));
        loudness_db >= self.threshold_db && (!self.use_pitch || pitch_hz.is_some_and(|hz| (low..=high).contains(&hz)))
    }
}

/// One of the sounds a Sound Recognition block listens for.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundTarget {
    /// Id of the sound asset.
    pub sound: String,
    /// The group it belongs to (empty: none).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub group: String,
}

/// Sounds of one block that share an accuracy.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundGroup {
    pub name: String,
    /// The group's accuracy; `None` uses the block's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accuracy: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundMatchParams {
    pub source: SoundSource,
    /// The microphone to use; empty for the system's default.
    pub device: String,
    /// The sounds to listen for, in order of priority.
    pub sounds: Vec<SoundTarget>,
    pub groups: Vec<SoundGroup>,
    /// The one sound of blocks made before a block could listen for several.
    pub sound: String,
    /// Minimum similarity, `0.0..=1.0`, for a sound to count as heard.
    pub accuracy: f64,
    pub mode: FindMode,
    /// Recognise a sound by its first milliseconds only (0: the whole sound).
    pub match_ms: f64,
}

impl Default for SoundMatchParams {
    fn default() -> Self {
        Self {
            source: SoundSource::Output,
            device: String::new(),
            sounds: Vec::new(),
            groups: Vec::new(),
            sound: String::new(),
            accuracy: 0.75,
            mode: FindMode::Any,
            match_ms: 0.0,
        }
    }
}

impl SoundMatchParams {
    /// Every sound to listen for, the one of an older block included.
    pub fn targets(&self) -> Vec<SoundTarget> {
        let mut targets = self.sounds.clone();
        if !self.sound.is_empty() && !targets.iter().any(|t| t.sound == self.sound) {
            targets.insert(0, SoundTarget { sound: self.sound.clone(), ..SoundTarget::default() });
        }
        targets.retain(|t| !t.sound.is_empty());
        targets
    }

    /// The accuracy a sound must be heard with: its group's, else the block's.
    pub fn accuracy_of(&self, target: &SoundTarget) -> f64 {
        self.groups
            .iter()
            .find(|g| !target.group.is_empty() && g.name == target.group)
            .and_then(|g| g.accuracy)
            .unwrap_or(self.accuracy)
            .clamp(0.0, 1.0)
    }
}

fn source_kind() -> ParamKind {
    ParamKind::choice(&[("output", "What the computer plays"), ("input", "Microphone")])
}

fn microphone() -> ParamDef {
    ParamDef::new("device", "Microphone", ParamKind::AudioDevice)
        .help("Which microphone to listen to; the system's default when none is chosen.")
        .show_if("source", "input")
}

fn source_line(source: SoundSource) -> &'static str {
    match source {
        SoundSource::Output => "Computer sound",
        SoundSource::Input => "Microphone",
    }
}

pub(super) fn register(registry: &mut Registry) {
    registry.register(
        NodeDef::new(
            ids::SOUND_LEVEL,
            "Sound Detection",
            Category::Sound,
            "Listens for sound that is loud enough – and, optionally, within a pitch range. \
             “Detected” is true while it is heard; “Loudness” is in dB (0 is the loudest \
             possible, quiet sound is around -60) and “Pitch” in Hz. Wire “Detected” into a Wait \
             Until or a Decision.",
        )
        .keywords(&["sound", "audio", "loud", "volume", "pitch", "frequency", "hear", "listen", "microphone", "detect"])
        .data_out("detected", "Detected", DataType::Bool)
        .data_out("loudness", "Loudness (dB)", DataType::Number)
        .data_out("pitch", "Pitch (Hz)", DataType::Number)
        .param(ParamDef::new("source", "Listen to", source_kind()))
        .param(microphone())
        .param(
            ParamDef::new("threshold_db", "Loud enough at", ParamKind::float(-100.0, 0.0, "dB"))
                .help("0 dB is the loudest sound possible. Normal sound is around -30 dB, quiet sound -60 dB."),
        )
        .param(ParamDef::new("use_pitch", "Only within a pitch range", ParamKind::Bool))
        .param(ParamDef::new("min_hz", "Lowest pitch", ParamKind::float(20.0, 8000.0, "Hz")).show_if("use_pitch", true))
        .param(
            ParamDef::new("max_hz", "Highest pitch", ParamKind::float(20.0, 8000.0, "Hz"))
                .help("The strongest frequency heard must be in this range. A man's voice is around 100–150 Hz, a whistle 1000–3000 Hz.")
                .show_if("use_pitch", true),
        )
        .param(
            ParamDef::new("window_ms", "Measure the last", ParamKind::Duration)
                .help("Longer smooths out short clicks and gaps; shorter reacts faster."),
        )
        .defaults(&SoundLevelParams::default())
        .summarize(|node, _| {
            let p: SoundLevelParams = node.params_or_default();
            let mut lines = vec![source_line(p.source).into(), format!("Louder than {:.0} dB", p.threshold_db)];
            if p.use_pitch {
                lines.push(format!("Pitch {:.0}–{:.0} Hz", p.min_hz.min(p.max_hz), p.min_hz.max(p.max_hz)));
            }
            lines
        }),
    );

    registry.register(
        NodeDef::new(
            ids::SOUND_MATCH,
            "Sound Recognition",
            Category::Sound,
            "Listens for particular sounds – a notification, an alarm, a voice line – that you \
             upload as sound files. “Found” is true once one is heard at least as closely as the \
             accuracy setting, however loud it is played; “Which sound” says which one was \
             recognised last. Each time a sound is heard counts once.",
        )
        .keywords(&["sound", "audio", "recognize", "recognise", "match", "hear", "listen", "file", "wav", "mp3", "sounds", "group"])
        .data_out("found", "Found", DataType::Bool)
        .data_out("score", "Similarity", DataType::Number)
        .data_out("name", "Which sound", DataType::Text)
        .data_out("group", "Its group", DataType::Text)
        .data_out("count", "How many", DataType::Number)
        .param(ParamDef::new("source", "Listen to", source_kind()))
        .param(microphone())
        .param(
            ParamDef::new("sounds", "Sounds", ParamKind::Sounds).help(
                "WAV, MP3, OGG or FLAC files; the first 10 seconds of each are used. Short and distinctive works \
                 best. Put sounds in a group to give them an accuracy of their own.",
            ),
        )
        .param(ParamDef::new("sound", "Sound", ParamKind::Audio).hidden())
        .param(ParamDef::new(
            "mode",
            "Found when",
            ParamKind::choice(&[("any", "Any sound is heard"), ("all", "Every sound has been heard")]),
        ))
        .param(
            ParamDef::new("accuracy", "Accuracy", ParamKind::Percent)
                .help("How closely what is heard must match. Lower it if the sound is missed, raise it if other sounds set it off."),
        )
        .param(
            ParamDef::new("match_ms", "Recognise from the first", ParamKind::Duration).help(
                "0 waits for the whole sound. A second or two is enough for most sounds, and long sounds are \
                 then recognised while they are still playing.",
            ),
        )
        .defaults(&SoundMatchParams::default())
        .summarize(|node, assets| {
            let p: SoundMatchParams = node.params_or_default();
            let targets = p.targets();
            let headline = match targets.as_slice() {
                [] => "No sound selected".to_owned(),
                [one] => assets.get(&one.sound).map_or_else(|| "A missing sound".to_owned(), |a| a.name.clone()),
                many if p.mode == FindMode::All => format!("All of {} sounds", many.len()),
                many => format!("Any of {} sounds", many.len()),
            };
            vec![headline, format!("Accuracy ≥ {:.0}%", p.accuracy * 100.0), source_line(p.source).into()]
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loudness_and_pitch_decide_detection() {
        let mut p = SoundLevelParams::default();
        assert!(p.detects(-20.0, None));
        assert!(!p.detects(-45.0, Some(440.0)));
        p.use_pitch = true;
        assert!(p.detects(-20.0, Some(440.0)));
        assert!(!p.detects(-20.0, Some(3000.0)));
        assert!(!p.detects(-20.0, None), "no pitch is heard in silence or noise");
        (p.min_hz, p.max_hz) = (2000.0, 200.0);
        assert!(p.detects(-20.0, Some(440.0)), "a reversed range still works");
    }
}

//! Sound nodes: listening to what the computer plays, or to the microphone.
//! Like the vision nodes these are value nodes, evaluated whenever a block
//! reads them – typically a Wait Until or a Decision.

use serde::{Deserialize, Serialize};

use super::{Category, DataType, NodeDef, ParamDef, ParamKind, Registry, ids};
use crate::host::SoundSource;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundLevelParams {
    pub source: SoundSource,
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundMatchParams {
    pub source: SoundSource,
    /// Id of the sound asset to listen for.
    pub sound: String,
    /// Minimum similarity, `0.0..=1.0`, for the sound to count as heard.
    pub accuracy: f64,
}

impl Default for SoundMatchParams {
    fn default() -> Self {
        Self { source: SoundSource::Output, sound: String::new(), accuracy: 0.75 }
    }
}

fn source_kind() -> ParamKind {
    ParamKind::choice(&[("output", "What the computer plays"), ("input", "Microphone")])
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
            "Listens for a particular sound – a notification, an alarm, a voice line – that you \
             upload as a sound file. “Found” is true once it is heard at least as closely as the \
             accuracy setting, however loud it is played. Each time the sound is heard counts \
             once.",
        )
        .keywords(&["sound", "audio", "recognize", "recognise", "match", "hear", "listen", "file", "wav", "mp3"])
        .data_out("found", "Found", DataType::Bool)
        .data_out("score", "Similarity", DataType::Number)
        .param(ParamDef::new("source", "Listen to", source_kind()))
        .param(
            ParamDef::new("sound", "Sound", ParamKind::Audio)
                .help("A WAV, MP3, OGG or FLAC file. Short and distinctive works best; the first 10 seconds are used."),
        )
        .param(
            ParamDef::new("accuracy", "Accuracy", ParamKind::Percent)
                .help("How closely what is heard must match. Lower it if the sound is missed, raise it if other sounds set it off."),
        )
        .defaults(&SoundMatchParams::default())
        .summarize(|node, assets| {
            let p: SoundMatchParams = node.params_or_default();
            let name = match assets.get(&p.sound) {
                Some(asset) => asset.name.clone(),
                None => "No sound selected".into(),
            };
            vec![name, format!("Accuracy ≥ {:.0}%", p.accuracy * 100.0), source_line(p.source).into()]
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

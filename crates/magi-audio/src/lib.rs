//! # magi-audio
//!
//! Sound analysis for the Sound Detection and Sound Recognition blocks:
//!
//! * [`loudness_db`] and [`dominant_frequency`] describe what is heard now;
//! * [`Fingerprint`] and [`similarity`] recognise a recorded sound inside
//!   what is heard, however loud it is; [`Listening`] does the same on a live
//!   stream, analysing each piece of sound only once;
//! * [`decode`] reads the sound files people upload (WAV, MP3, OGG, FLAC),
//!   and [`encode_wav`] stores them compactly inside a macro.
//!
//! Pure functions on samples: where the sound comes from is somebody else's
//! business (`magi-platform`).

use std::io::Cursor;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Sounds are analysed, and stored, at this rate: plenty for recognising them.
pub const ANALYSIS_RATE: u32 = 16_000;
/// Longest sound kept from an upload.
pub const MAX_SECONDS: f32 = 10.0;
/// Quietest level reported, in dBFS. Silence reads as this.
pub const SILENCE_DB: f64 = -100.0;

const FRAME: usize = 512;
const HOP: usize = 256;
const BANDS: usize = 24;
const LOW_HZ: f32 = 150.0;
const HIGH_HZ: f32 = 6000.0;
/// Bands this far below a frame's loudest band (as a power ratio, 30 dB) are
/// all treated alike.
const DYNAMIC_RANGE: f32 = 1e-3;
/// Frames quieter than this (total band energy) are treated as silence.
const SILENT_FRAME: f32 = 1e-4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioError(pub String);

impl std::fmt::Display for AudioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for AudioError {}

/// Mono sound.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Audio {
    pub sample_rate: u32,
    pub samples: Vec<f32>,
}

impl Audio {
    pub fn seconds(&self) -> f32 {
        self.samples.len() as f32 / self.sample_rate.max(1) as f32
    }
}

/// Reads a sound file and mixes it down to mono.
pub fn decode(bytes: &[u8]) -> Result<Audio, AudioError> {
    let error = |what: &str, e: &dyn std::fmt::Display| AudioError(format!("{what}: {e}"));
    let source = MediaSourceStream::new(Box::new(Cursor::new(bytes.to_vec())), Default::default());
    let probed = symphonia::default::get_probe()
        .format(&Hint::new(), source, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| error("this is not a sound file MacroMagi can read (WAV, MP3, OGG or FLAC)", &e))?;
    let mut format = probed.format;
    let track = format.default_track().ok_or_else(|| AudioError("the file has no sound in it".into()))?;
    let track_id = track.id;
    let sample_rate = track.codec_params.sample_rate.unwrap_or(44_100);
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| error("this kind of sound file is not supported", &e))?;

    let mut samples = Vec::new();
    let limit = (MAX_SECONDS * sample_rate as f32) as usize;
    while samples.len() < limit {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(_) => break, // end of the file
        };
        if packet.track_id() != track_id {
            continue;
        }
        let Ok(decoded) = decoder.decode(&packet) else { continue };
        let channels = decoded.spec().channels.count().max(1);
        let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
        buffer.copy_interleaved_ref(decoded);
        for frame in buffer.samples().chunks(channels) {
            samples.push(frame.iter().sum::<f32>() / channels as f32);
        }
    }
    samples.truncate(limit);
    if samples.is_empty() {
        return Err(AudioError("the file has no sound in it".into()));
    }
    Ok(Audio { sample_rate, samples })
}

/// Changes the sample rate (linear interpolation: plenty for analysis).
pub fn resample(samples: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || samples.is_empty() || from == 0 || to == 0 {
        return samples.to_vec();
    }
    let ratio = f64::from(from) / f64::from(to);
    let count = ((samples.len() as f64) / ratio).floor() as usize;
    (0..count)
        .map(|i| {
            let position = i as f64 * ratio;
            let index = position.floor() as usize;
            let t = (position - index as f64) as f32;
            let a = samples[index.min(samples.len() - 1)];
            let b = samples[(index + 1).min(samples.len() - 1)];
            a + (b - a) * t
        })
        .collect()
}

/// A 16-bit mono WAV file.
pub fn encode_wav(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + samples.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16;
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

/// Prepares an uploaded sound for storing in a macro: mono, at the analysis
/// rate, at most [`MAX_SECONDS`] long, as a WAV file.
pub fn prepare_upload(bytes: &[u8]) -> Result<Vec<u8>, AudioError> {
    let audio = decode(bytes)?;
    let samples = resample(&audio.samples, audio.sample_rate, ANALYSIS_RATE);
    Ok(encode_wav(&samples, ANALYSIS_RATE))
}

/// How loud the sound is, in dBFS: 0 is the loudest possible, silence is [`SILENCE_DB`].
pub fn loudness_db(samples: &[f32]) -> f64 {
    if samples.is_empty() {
        return SILENCE_DB;
    }
    let mean_square = samples.iter().map(|s| f64::from(*s) * f64::from(*s)).sum::<f64>() / samples.len() as f64;
    if mean_square <= 0.0 { SILENCE_DB } else { (10.0 * mean_square.log10()).max(SILENCE_DB) }
}

/// The strongest frequency in the sound, in hertz – roughly its pitch.
/// `None` for silence.
pub fn dominant_frequency(samples: &[f32], sample_rate: u32) -> Option<f64> {
    if samples.len() < 64 || loudness_db(samples) < -70.0 {
        return None;
    }
    let size = samples.len().min(8192).next_power_of_two().min(8192);
    let tail = &samples[samples.len().saturating_sub(size)..];
    let mut re = vec![0.0f32; size];
    for (i, sample) in tail.iter().enumerate() {
        re[i] = sample * hann(i, tail.len());
    }
    let mut im = vec![0.0f32; size];
    fft(&mut re, &mut im);
    let magnitude = |k: usize| (re[k] * re[k] + im[k] * im[k]).sqrt();
    let resolution = sample_rate as f32 / size as f32;
    let first = ((40.0 / resolution).ceil() as usize).max(1);
    let (peak, _) = (first..size / 2 - 1).map(|k| (k, magnitude(k))).max_by(|a, b| a.1.total_cmp(&b.1))?;
    // Parabolic interpolation between neighbouring bins sharpens the estimate.
    let (a, b, c) = (magnitude(peak - 1), magnitude(peak), magnitude(peak + 1));
    let denominator = a - 2.0 * b + c;
    let shift = if denominator.abs() > f32::EPSILON { 0.5 * (a - c) / denominator } else { 0.0 };
    Some(f64::from((peak as f32 + shift) * resolution))
}

/// One short frame of a sound, described by how its energy is spread over
/// frequency bands: a unit vector, so comparing two frames is a dot product.
type Bands = [f32; BANDS];

/// What every frame is analysed with, worked out once.
struct Analyzer {
    window: Vec<f32>,
    /// FFT bins where each band starts (and the last one ends).
    edges: Vec<usize>,
}

fn analyzer() -> &'static Analyzer {
    static ANALYZER: std::sync::OnceLock<Analyzer> = std::sync::OnceLock::new();
    ANALYZER.get_or_init(|| Analyzer {
        window: (0..FRAME).map(|i| hann(i, FRAME)).collect(),
        edges: (0..=BANDS)
            .map(|b| {
                let hz = LOW_HZ * (HIGH_HZ / LOW_HZ).powf(b as f32 / BANDS as f32);
                ((hz * FRAME as f32 / ANALYSIS_RATE as f32).round() as usize).clamp(1, FRAME / 2)
            })
            .collect(),
    })
}

/// Describes one frame of [`FRAME`] samples at the analysis rate. The flag
/// says whether the frame is silent.
fn analyze(samples: &[f32]) -> (Bands, bool) {
    let analyzer = analyzer();
    let mut re: Vec<f32> = samples.iter().zip(&analyzer.window).map(|(s, w)| s * w).collect();
    let mut im = vec![0.0f32; FRAME];
    fft(&mut re, &mut im);
    let mut bands = [0.0f32; BANDS];
    for (band, value) in bands.iter_mut().enumerate() {
        let (from, to) = (analyzer.edges[band], analyzer.edges[band + 1].max(analyzer.edges[band] + 1));
        *value = (from..to).map(|k| re[k] * re[k] + im[k] * im[k]).sum::<f32>() / (to - from) as f32;
    }
    let total: f32 = bands.iter().sum();
    // Only the loudest part of each frame counts: quiet bands (mostly
    // background noise) are flattened to one level.
    let peak = bands.iter().copied().fold(0.0f32, f32::max);
    let floor = (peak * DYNAMIC_RANGE).max(1e-12);
    bands.iter_mut().for_each(|v| *v = v.max(floor).ln());
    let mean = bands.iter().sum::<f32>() / BANDS as f32;
    bands.iter_mut().for_each(|v| *v -= mean);
    let norm = bands.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > f32::EPSILON {
        bands.iter_mut().for_each(|v| *v /= norm);
    } else {
        bands = [0.0; BANDS];
    }
    (bands, total < SILENT_FRAME)
}

/// The shape of a sound over time: for each short frame, how its energy is
/// spread over frequency bands, independent of how loud it is.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Fingerprint {
    frames: Vec<Bands>,
    silent: Vec<bool>,
}

impl Fingerprint {
    pub fn of(audio: &Audio) -> Self {
        let samples = resample(&audio.samples, audio.sample_rate, ANALYSIS_RATE);
        let mut out = Self::default();
        let mut start = 0;
        while start + FRAME <= samples.len() {
            let (bands, silent) = analyze(&samples[start..start + FRAME]);
            out.frames.push(bands);
            out.silent.push(silent);
            start += HOP;
        }
        out
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// How long the sound is, in milliseconds.
    pub fn duration_ms(&self) -> f64 {
        frames_to_ms(self.frames.len())
    }

    /// Only its first `ms` milliseconds (all of it for zero or more than it has).
    /// A long sound is recognised sooner – and with less work – by its beginning.
    pub fn prefix(&self, ms: f64) -> Self {
        if ms <= 0.0 || ms >= self.duration_ms() {
            return self.clone();
        }
        let frames = ms_to_frames(ms).max(1).min(self.frames.len());
        Self { frames: self.frames[..frames].to_vec(), silent: self.silent[..frames].to_vec() }
    }
}

fn frames_to_ms(frames: usize) -> f64 {
    if frames == 0 { 0.0 } else { ((frames - 1) * HOP + FRAME) as f64 * 1000.0 / f64::from(ANALYSIS_RATE) }
}

fn ms_to_frames(ms: f64) -> usize {
    let samples = ms / 1000.0 * f64::from(ANALYSIS_RATE);
    ((samples - FRAME as f64) / HOP as f64).floor().max(0.0) as usize + 1
}

/// The score of `template` laid over `frames` starting at `offset`, looking
/// at every `step`-th template frame. `None` when what is heard there is
/// mostly silence: a match must be heard, not inferred from silence.
fn score_at(
    template: &Fingerprint,
    wanted: &[usize],
    frames: &[Bands],
    silent: &[bool],
    offset: usize,
    step: usize,
) -> Option<f32> {
    let mut sum = 0.0f32;
    let (mut counted, mut quiet) = (0usize, 0usize);
    for &i in wanted.iter().step_by(step) {
        if silent[offset + i] {
            quiet += 1;
        }
        let (a, b) = (&template.frames[i], &frames[offset + i]);
        sum += a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
        counted += 1;
    }
    (counted > 0 && quiet * 2 <= counted).then(|| sum / counted as f32)
}

/// Where `template` best matches inside `frames`: the score (0 to 1) and the
/// index just past the end of the match. Long sounds are first compared
/// coarsely – every other frame at every other position – and only the
/// best few positions in full.
fn best_match(template: &Fingerprint, frames: &[Bands], silent: &[bool]) -> Option<(f64, usize)> {
    let wanted: Vec<usize> = (0..template.len()).filter(|&i| !template.silent[i]).collect();
    if wanted.is_empty() || frames.len() < template.len() {
        return None;
    }
    let last = frames.len() - template.len();
    let full = |offset: usize| score_at(template, &wanted, frames, silent, offset, 1);
    let best = if template.len() < 48 {
        (0..=last).filter_map(|o| full(o).map(|s| (s, o))).max_by(|a, b| a.0.total_cmp(&b.0))
    } else {
        let mut coarse: Vec<(f32, usize)> = (0..=last)
            .step_by(2)
            .filter_map(|o| score_at(template, &wanted, frames, silent, o, 2).map(|s| (s, o)))
            .collect();
        coarse.sort_by(|a, b| b.0.total_cmp(&a.0));
        coarse
            .iter()
            .take(3)
            .flat_map(|&(_, o)| o.saturating_sub(2)..=(o + 2).min(last))
            .filter_map(|o| full(o).map(|s| (s, o)))
            .max_by(|a, b| a.0.total_cmp(&b.0))
    }?;
    Some((f64::from(best.0.max(0.0)), best.1 + template.len()))
}

/// How well `template` matches somewhere inside `heard`, from 0 (not at all)
/// to 1 (identical). Silence never matches.
pub fn similarity(template: &Fingerprint, heard: &Fingerprint) -> f64 {
    best_match(template, &heard.frames, &heard.silent).map_or(0.0, |(score, _)| score)
}

/// Sound arriving as a stream, analysed as it arrives: every call to
/// [`Listening::feed`] only analyses what is new, however long the sounds
/// being listened for are, and any number of them can be compared with the
/// same analysis.
#[derive(Debug, Default)]
pub struct Listening {
    rate: u32,
    /// Source samples per analysis sample.
    step: f64,
    /// How many source samples have been fed since the stream (re)started.
    fed: u64,
    /// Where in the stream the source samples received come from: absolute
    /// index of the first one fed after a restart.
    origin: u64,
    previous: f32,
    /// The time (in source samples since the restart) of the next analysis sample.
    next: f64,
    /// Analysis samples not yet covered by a complete frame.
    pending: Vec<f32>,
    frames: std::collections::VecDeque<Bands>,
    silent: std::collections::VecDeque<bool>,
    /// Stream index of `frames[0]`.
    first_frame: u64,
}

/// Frames kept: a little more than the longest sound that can be listened for.
const KEEP_FRAMES: usize = ((MAX_SECONDS as usize + 2) * ANALYSIS_RATE as usize) / HOP;

impl Listening {
    pub fn new() -> Self {
        Self::default()
    }

    fn restart(&mut self, rate: u32, origin: u64) {
        // Frame numbers keep counting, so marks made before the restart stay valid.
        let first_frame = self.end_frame();
        *self = Self { rate, step: f64::from(rate) / f64::from(ANALYSIS_RATE), origin, first_frame, ..Self::default() };
    }

    /// Adds the latest sound. `samples` are the most recent mono samples at
    /// `rate`, and `end` counts every sample the source has produced, so
    /// samples already seen are skipped. A gap, or another rate, starts over.
    pub fn feed(&mut self, samples: &[f32], rate: u32, end: u64) {
        let start = end.saturating_sub(samples.len() as u64);
        let seen = self.origin + self.fed;
        if rate != self.rate || seen < start || seen > end {
            self.restart(rate, start);
        }
        let skip = (self.origin + self.fed - start) as usize;
        for &sample in &samples[skip.min(samples.len())..] {
            self.push(sample);
        }
    }

    fn push(&mut self, sample: f32) {
        // Linear resampling: analysis samples fall between the previous source
        // sample and this one.
        let index = self.fed as f64;
        while self.next <= index {
            let t = (self.next - (index - 1.0)).clamp(0.0, 1.0) as f32;
            self.pending.push(self.previous + (sample - self.previous) * t);
            self.next += self.step;
        }
        self.previous = sample;
        self.fed += 1;
        while self.pending.len() >= FRAME {
            let (bands, silent) = analyze(&self.pending[..FRAME]);
            self.frames.push_back(bands);
            self.silent.push_back(silent);
            self.pending.drain(..HOP);
        }
        while self.frames.len() > KEEP_FRAMES {
            self.frames.pop_front();
            self.silent.pop_front();
            self.first_frame += 1;
        }
    }

    /// Stream index just past the newest frame.
    pub fn end_frame(&self) -> u64 {
        self.first_frame + self.frames.len() as u64
    }

    /// The best match of `template` among the frames after `after` (a stream
    /// frame index, so a sound already recognised is not recognised again):
    /// its score, and the stream frame index where it ends.
    pub fn best_match(&mut self, template: &Fingerprint, after: u64) -> Option<(f64, u64)> {
        let from = after.saturating_sub(self.first_frame) as usize;
        if from >= self.frames.len() {
            return None;
        }
        let first = self.first_frame;
        let frames = &self.frames.make_contiguous()[from..];
        let silent = &self.silent.make_contiguous()[from..];
        best_match(template, frames, silent).map(|(score, end)| (score, first + (from + end) as u64))
    }

    /// How many frames a sound of `ms` milliseconds covers.
    pub fn frames_in(ms: f64) -> u64 {
        ms_to_frames(ms) as u64
    }
}

fn hann(i: usize, n: usize) -> f32 {
    if n <= 1 {
        return 1.0;
    }
    0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (n - 1) as f32).cos()
}

/// In-place radix-2 FFT. The length must be a power of two.
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    debug_assert!(n.is_power_of_two() && im.len() == n);
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let angle = -2.0 * std::f32::consts::PI / len as f32;
        let (w_re, w_im) = (angle.cos(), angle.sin());
        for start in (0..n).step_by(len) {
            let (mut cur_re, mut cur_im) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let t_re = re[b] * cur_re - im[b] * cur_im;
                let t_im = re[b] * cur_im + im[b] * cur_re;
                re[b] = re[a] - t_re;
                im[b] = im[a] - t_im;
                re[a] += t_re;
                im[a] += t_im;
                let next_re = cur_re * w_re - cur_im * w_im;
                cur_im = cur_re * w_im + cur_im * w_re;
                cur_re = next_re;
            }
        }
        len <<= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(hz: f32, seconds: f32, amplitude: f32) -> Vec<f32> {
        let n = (seconds * ANALYSIS_RATE as f32) as usize;
        (0..n).map(|i| amplitude * (2.0 * std::f32::consts::PI * hz * i as f32 / ANALYSIS_RATE as f32).sin()).collect()
    }

    /// A short three-note tune.
    fn tune() -> Vec<f32> {
        [440.0, 660.0, 550.0].iter().flat_map(|&hz| tone(hz, 0.25, 0.5)).collect()
    }

    fn noise(seconds: f32, amplitude: f32) -> Vec<f32> {
        let mut state = 0x2545_f491_u32;
        (0..(seconds * ANALYSIS_RATE as f32) as usize)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                (state as f32 / u32::MAX as f32 * 2.0 - 1.0) * amplitude
            })
            .collect()
    }

    fn audio(samples: Vec<f32>) -> Audio {
        Audio { sample_rate: ANALYSIS_RATE, samples }
    }

    #[test]
    fn loudness_and_pitch_are_measured() {
        let a = tone(440.0, 0.5, 0.5);
        assert!((loudness_db(&a) - -9.03).abs() < 0.2, "{}", loudness_db(&a));
        assert_eq!(loudness_db(&[0.0; 100]), SILENCE_DB);
        let pitch = dominant_frequency(&a, ANALYSIS_RATE).unwrap();
        assert!((pitch - 440.0).abs() < 3.0, "{pitch}");
        assert_eq!(dominant_frequency(&[0.0; 1000], ANALYSIS_RATE), None);
    }

    #[test]
    fn a_sound_is_recognised_inside_other_sound_at_any_volume() {
        let template = Fingerprint::of(&audio(tune()));
        let mut heard = noise(0.6, 0.02);
        heard.extend(tune().iter().map(|s| s * 0.3)); // quieter than the original
        heard.extend(noise(0.4, 0.02));
        let score = similarity(&template, &Fingerprint::of(&audio(heard)));
        assert!(score > 0.8, "found with {score}");

        let other: Vec<f32> = [300.0, 900.0, 1200.0].iter().flat_map(|&hz| tone(hz, 0.25, 0.5)).collect();
        let mut heard = noise(0.6, 0.02);
        heard.extend(other);
        let score = similarity(&template, &Fingerprint::of(&audio(heard)));
        assert!(score < 0.4, "a different tune scored {score}");

        let silence = Fingerprint::of(&audio(vec![0.0; ANALYSIS_RATE as usize * 2]));
        assert_eq!(similarity(&template, &silence), 0.0);
    }

    /// A melody of `notes` short tones, different every time `seed` differs.
    fn melody(notes: usize, seed: u32, rate: u32) -> Vec<f32> {
        let mut state = seed.max(1);
        let mut out = Vec::new();
        for _ in 0..notes {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let hz = 300.0 + (state % 1500) as f32;
            let n = (0.12 * rate as f32) as usize;
            out.extend((0..n).map(|i| 0.4 * (2.0 * std::f32::consts::PI * hz * i as f32 / rate as f32).sin()));
        }
        out
    }

    #[test]
    fn a_stream_fed_in_overlapping_pieces_recognises_a_long_sound_once() {
        // A 6-second melody, played at 48 kHz in the middle of other sound.
        let rate = 48_000;
        let template = Fingerprint::of(&Audio { sample_rate: rate, samples: melody(50, 7, rate) });
        assert!(template.len() > 300, "long enough to be compared coarsely first");
        let mut stream: Vec<f32> = melody(10, 99, rate);
        stream.extend(melody(50, 7, rate));
        stream.extend(melody(10, 123, rate));

        // The source hands out its latest second each time, overlapping what was seen.
        let mut listening = Listening::new();
        let mut found = None;
        let mut end = 0usize;
        while end < stream.len() {
            end = (end + rate as usize / 10).min(stream.len());
            let start = end.saturating_sub(rate as usize);
            listening.feed(&stream[start..end], rate, end as u64);
            if found.is_none()
                && let Some((score, at)) = listening.best_match(&template, 0)
                && score > 0.8
            {
                found = Some((score, at));
            }
        }
        let (score, at) = found.expect("the melody was recognised");
        assert!(score > 0.9, "{score}");
        // Nothing after that occurrence matches again.
        let again = listening.best_match(&template, at).map_or(0.0, |(score, _)| score);
        assert!(again < 0.6, "{again}");
        // Another melody of the same length is not mistaken for it.
        let other = Fingerprint::of(&Audio { sample_rate: rate, samples: melody(50, 8, rate) });
        let wrong = listening.best_match(&other, 0).map_or(0.0, |(score, _)| score);
        assert!(wrong < 0.6, "{wrong}");
    }

    #[test]
    fn the_beginning_of_a_long_sound_is_enough_to_recognise_it() {
        let rate = ANALYSIS_RATE;
        let whole = Fingerprint::of(&Audio { sample_rate: rate, samples: melody(40, 3, rate) });
        let start = whole.prefix(1000.0);
        assert!(start.len() < whole.len() / 3);
        assert!((start.duration_ms() - 1000.0).abs() < 40.0, "{}", start.duration_ms());
        // Only the first second has been played so far.
        let heard = Fingerprint::of(&Audio {
            sample_rate: rate,
            samples: melody(40, 3, rate)[..rate as usize + 2000].to_vec(),
        });
        assert!(similarity(&start, &heard) > 0.9);
        assert_eq!(similarity(&whole, &heard), 0.0, "the whole sound has not been heard yet");
    }

    #[test]
    fn uploads_are_stored_as_small_wav_files_and_read_back() {
        let original = tone(440.0, 0.5, 0.5);
        let wav = encode_wav(&original, 44_100);
        let decoded = decode(&wav).unwrap();
        assert_eq!(decoded.sample_rate, 44_100);
        assert_eq!(decoded.samples.len(), original.len());
        assert!((decoded.samples[100] - original[100]).abs() < 1e-3);
        let stored = decode(&prepare_upload(&wav).unwrap()).unwrap();
        assert_eq!(stored.sample_rate, ANALYSIS_RATE);
        assert!(decode(b"definitely not a sound").is_err());
    }
}

//! Listening to sound, for the Sound Detection and Sound Recognition blocks.
//!
//! A source – what the computer is playing, or the microphone – is only
//! opened while a macro is listening. A thread owns the audio stream (they
//! cannot move between threads on every system) and keeps the last few
//! seconds in a ring buffer; when nobody has asked for sound for a while, it
//! closes the stream and ends.
//!
//! "What the computer is playing" uses loopback capture, which Windows
//! offers for every output device. Elsewhere it needs a monitor or loopback
//! device to be set up; without one, an error explains that.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SizedSample};
use magi_core::host::{HostError, SoundClip, SoundSource};

/// Seconds of sound kept.
const KEEP_SECONDS: usize = 12;
/// A source nobody listened to for this long is closed.
const IDLE: Duration = Duration::from_secs(15);

#[derive(Default)]
struct Buffer {
    sample_rate: u32,
    samples: VecDeque<f32>,
    /// Why the source could not be opened, if it could not.
    error: Option<String>,
    last_request: Option<Instant>,
    running: bool,
}

/// One source being listened to.
#[derive(Default)]
struct Source {
    buffer: Arc<Mutex<Buffer>>,
}

fn lock(buffer: &Mutex<Buffer>) -> MutexGuard<'_, Buffer> {
    buffer.lock().unwrap_or_else(|p| p.into_inner())
}

#[derive(Default)]
pub struct Listener {
    output: Source,
    input: Source,
}

impl Listener {
    pub fn new() -> Self {
        Self::default()
    }

    /// The last `ms` milliseconds from `source`, opening it if needed.
    pub fn listen(&self, source: SoundSource, ms: f64) -> Result<SoundClip, HostError> {
        let src = match source {
            SoundSource::Output => &self.output,
            SoundSource::Input => &self.input,
        };
        let start = {
            let mut buffer = lock(&src.buffer);
            buffer.last_request = Some(Instant::now());
            if let Some(error) = buffer.error.take() {
                return Err(HostError::new(error));
            }
            let start = !buffer.running;
            buffer.running = true;
            start
        };
        if start {
            let buffer = Arc::clone(&src.buffer);
            std::thread::Builder::new()
                .name("magi-audio".into())
                .spawn(move || capture(source, &buffer))
                .map_err(|e| HostError::new(format!("could not start listening: {e}")))?;
            // Give the stream a moment, so the first answer is not empty.
            std::thread::sleep(Duration::from_millis(ms.clamp(50.0, 300.0) as u64));
        }
        let buffer = lock(&src.buffer);
        if let Some(error) = &buffer.error {
            return Err(HostError::new(error.clone()));
        }
        let wanted = ((ms / 1000.0) * f64::from(buffer.sample_rate)) as usize;
        let skip = buffer.samples.len().saturating_sub(wanted);
        Ok(SoundClip { sample_rate: buffer.sample_rate, samples: buffer.samples.iter().skip(skip).copied().collect() })
    }
}

/// Runs on its own thread: opens the stream and keeps it open while wanted.
fn capture(source: SoundSource, buffer: &Arc<Mutex<Buffer>>) {
    let result = open(source, buffer);
    match result {
        Ok(stream) => {
            loop {
                std::thread::sleep(Duration::from_secs(1));
                let idle = lock(buffer).last_request.is_none_or(|at| at.elapsed() > IDLE);
                if idle {
                    break;
                }
            }
            drop(stream);
            let mut state = lock(buffer);
            state.samples = VecDeque::new();
            state.running = false;
        }
        Err(error) => {
            let mut state = lock(buffer);
            state.error = Some(error);
            state.running = false;
        }
    }
}

fn open(source: SoundSource, buffer: &Arc<Mutex<Buffer>>) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let (device, config) = match source {
        SoundSource::Output => {
            let device = host.default_output_device().ok_or("there is no sound output device")?;
            let config = device.default_output_config().map_err(|e| e.to_string())?;
            (device, config)
        }
        SoundSource::Input => {
            let device = host.default_input_device().ok_or("there is no microphone")?;
            let config = device.default_input_config().map_err(|e| e.to_string())?;
            (device, config)
        }
    };
    let channels = usize::from(config.channels().max(1));
    {
        let mut state = lock(buffer);
        state.sample_rate = config.sample_rate().0;
        state.samples = VecDeque::with_capacity(config.sample_rate().0 as usize * KEEP_SECONDS);
    }
    let format = config.sample_format();
    let config: cpal::StreamConfig = config.into();
    let stream = match format {
        SampleFormat::F32 => build::<f32>(&device, &config, channels, buffer),
        SampleFormat::I16 => build::<i16>(&device, &config, channels, buffer),
        SampleFormat::U16 => build::<u16>(&device, &config, channels, buffer),
        SampleFormat::I32 => build::<i32>(&device, &config, channels, buffer),
        other => return Err(format!("the sound device uses a sample format MacroMagi cannot read ({other})")),
    }
    .map_err(|e| match source {
        SoundSource::Output if !cfg!(windows) => {
            format!("listening to what the computer plays needs a loopback or monitor input on this system ({e})")
        }
        _ => e.to_string(),
    })?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

fn build<T: SizedSample + Send + 'static>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    buffer: &Arc<Mutex<Buffer>>,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    f32: cpal::FromSample<T>,
{
    let buffer = Arc::clone(buffer);
    device.build_input_stream(
        config,
        move |data: &[T], _| {
            let mut state = lock(&buffer);
            let capacity = state.sample_rate as usize * KEEP_SECONDS;
            for frame in data.chunks(channels) {
                // Mono: the average of the channels.
                let sum: f32 = frame.iter().map(|s| s.to_sample::<f32>()).sum();
                state.samples.push_back(sum / frame.len() as f32);
            }
            let excess = state.samples.len().saturating_sub(capacity);
            state.samples.drain(..excess);
        },
        |error| eprintln!("sound input error: {error}"),
        None,
    )
}

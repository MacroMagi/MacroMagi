//! The optional picture behind the interface: a still image or an animated
//! GIF or WebP, from a file or a web address.
//!
//! Pictures are copied into the data folder's `backgrounds` directory, so the
//! settings only ever point inside the app's own folder. Decoding happens on a
//! worker thread; frames are scaled down to what a window can show (and
//! further when blurred, since blur hides the detail anyway), which keeps an
//! animated background to a few megabytes.

use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use egui::{Color32, ColorImage, Context, Rect, TextureHandle, TextureOptions, pos2};
use image::{AnimationDecoder, ImageFormat, RgbaImage};

/// Longest side of a still picture.
const MAX_STILL_SIDE: u32 = 2560;
/// Longest side of an animation frame.
const MAX_FRAME_SIDE: u32 = 1280;
/// Pixels kept for all frames of an animation together (×4 bytes).
const ANIMATION_PIXEL_BUDGET: u64 = 12_000_000;
const MAX_FRAMES: usize = 600;
/// Downloads larger than this are refused.
const MAX_DOWNLOAD_BYTES: u64 = 64 * 1024 * 1024;
pub const EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp"];

struct Frames {
    images: Vec<(RgbaImage, Duration)>,
}

pub struct Background {
    /// File name and blur the textures were made for.
    key: (String, u32),
    frames: Vec<(TextureHandle, Duration)>,
    total: Duration,
    started: Instant,
    aspect: f32,
}

/// A picture file and the blur it is shown with.
type Key = (String, u32);

#[derive(Default)]
pub struct BackgroundLayer {
    current: Option<Background>,
    loading: Option<(Key, Receiver<Result<Frames, String>>)>,
    /// The last failure, reported once.
    error: Option<String>,
    failed: Option<(String, u32)>,
}

impl BackgroundLayer {
    /// Makes sure `file` (in `dir`) is what is shown, loading it if needed.
    /// `None` removes the picture.
    pub fn sync(&mut self, ctx: &Context, dir: &Path, file: Option<&str>, blur: f32) {
        let Some(file) = file else {
            self.current = None;
            self.loading = None;
            return;
        };
        let key = (file.to_owned(), blur.round().max(0.0) as u32);
        if self.current.as_ref().is_some_and(|bg| bg.key == key) || self.failed.as_ref() == Some(&key) {
            return;
        }
        match &self.loading {
            Some((loading, receiver)) if *loading == key => {
                if let Ok(result) = receiver.try_recv() {
                    self.loading = None;
                    match result {
                        Ok(frames) => self.current = Some(Background::upload(ctx, key, frames)),
                        Err(error) => {
                            self.error = Some(error);
                            self.failed = Some(key);
                        }
                    }
                } else {
                    ctx.request_repaint_after(Duration::from_millis(50));
                }
            }
            _ => {
                let (sender, receiver) = mpsc::channel();
                let path = dir.join(file);
                let radius = key.1;
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    let _ = sender.send(decode_file(&path, radius));
                    ctx.request_repaint();
                });
                self.loading = Some((key, receiver));
            }
        }
    }

    /// A problem loading the picture, once.
    pub fn take_error(&mut self) -> Option<String> {
        self.error.take()
    }

    /// Paints the picture over `rect`, cropped to cover it.
    pub fn paint(&self, painter: &egui::Painter, rect: Rect, opacity: f32, darkness: f32, dark_mode: bool) {
        let Some(bg) = &self.current else { return };
        let Some(texture) = bg.frame() else { return };
        let window_aspect = rect.width() / rect.height().max(1.0);
        // Crop the picture's UV range so it covers the window without stretching.
        let uv = if bg.aspect > window_aspect {
            let w = window_aspect / bg.aspect;
            Rect::from_min_max(pos2((1.0 - w) / 2.0, 0.0), pos2((1.0 + w) / 2.0, 1.0))
        } else {
            let h = bg.aspect / window_aspect;
            Rect::from_min_max(pos2(0.0, (1.0 - h) / 2.0), pos2(1.0, (1.0 + h) / 2.0))
        };
        painter.image(texture.id(), rect, uv, Color32::WHITE.gamma_multiply(opacity.clamp(0.0, 1.0)));
        let veil = if dark_mode { Color32::BLACK } else { Color32::WHITE };
        painter.rect_filled(rect, 0.0, veil.gamma_multiply(darkness.clamp(0.0, 1.0)));
        if bg.frames.len() > 1 {
            painter.ctx().request_repaint_after(bg.until_next_frame());
        }
    }
}

impl Background {
    fn upload(ctx: &Context, key: (String, u32), frames: Frames) -> Self {
        let aspect =
            frames.images.first().map_or(1.0, |(image, _)| image.width() as f32 / image.height().max(1) as f32);
        let frames: Vec<(TextureHandle, Duration)> = frames
            .images
            .into_iter()
            .enumerate()
            .map(|(i, (image, delay))| {
                let size = [image.width() as usize, image.height() as usize];
                let color = ColorImage::from_rgba_unmultiplied(size, image.as_raw());
                (ctx.load_texture(format!("background-{i}"), color, TextureOptions::LINEAR), delay)
            })
            .collect();
        let total = frames.iter().map(|(_, delay)| *delay).sum();
        Self { key, frames, total, started: Instant::now(), aspect }
    }

    fn position(&self) -> Duration {
        if self.total.is_zero() {
            return Duration::ZERO;
        }
        let elapsed = self.started.elapsed().as_nanos() % self.total.as_nanos();
        Duration::from_nanos(elapsed as u64)
    }

    fn frame(&self) -> Option<&TextureHandle> {
        let mut at = self.position();
        for (texture, delay) in &self.frames {
            if at < *delay {
                return Some(texture);
            }
            at -= *delay;
        }
        self.frames.last().map(|(texture, _)| texture)
    }

    fn until_next_frame(&self) -> Duration {
        let mut at = self.position();
        for (_, delay) in &self.frames {
            if at < *delay {
                return *delay - at;
            }
            at -= *delay;
        }
        Duration::from_millis(16)
    }
}

fn decode_file(path: &Path, blur: u32) -> Result<Frames, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("Could not read the background picture: {e}"))?;
    decode(&bytes, blur)
}

/// The window size a picture is assumed to be shown at, for turning the blur
/// (in screen pixels) into pixels of the stored picture.
const DISPLAY_SIDE: f32 = 1920.0;

/// Scale that fits `w × h` into the size limits. A blurred picture keeps no
/// detail finer than the blur, so it is stored smaller.
fn fit_scale(w: u32, h: u32, frames: usize, blur: u32) -> f32 {
    let (w, h) = (w.max(1) as f32, h.max(1) as f32);
    let mut side = if frames > 1 { MAX_FRAME_SIDE } else { MAX_STILL_SIDE } as f32;
    if blur > 0 {
        side = side.min((DISPLAY_SIDE * 4.0 / blur as f32).max(96.0));
    }
    let mut scale = (side / w.max(h)).min(1.0);
    if frames > 1 {
        let budget = ANIMATION_PIXEL_BUDGET as f32 / frames as f32;
        scale = scale.min((budget / (w * h)).sqrt());
    }
    scale
}

fn prepare(image: RgbaImage, scale: f32, blur: u32) -> RgbaImage {
    let (w, h) = image.dimensions();
    let mut image = if scale < 0.999 {
        let (nw, nh) = (((w as f32 * scale).round() as u32).max(1), ((h as f32 * scale).round() as u32).max(1));
        image::imageops::resize(&image, nw, nh, image::imageops::FilterType::Triangle)
    } else {
        image
    };
    if blur > 0 {
        let long_side = image.width().max(image.height()) as f32;
        let sigma = (blur as f32 * long_side / DISPLAY_SIDE).max(0.5);
        image = image::imageops::fast_blur(&image, sigma);
    }
    image
}

fn decode(bytes: &[u8], blur: u32) -> Result<Frames, String> {
    let format = image::guess_format(bytes).map_err(|_| "This file is not a picture MacroMagi can show.".to_owned())?;
    let animated: Option<Vec<image::Frame>> = match format {
        ImageFormat::Gif => {
            let decoder = image::codecs::gif::GifDecoder::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
            Some(decoder.into_frames().take(MAX_FRAMES).collect::<Result<_, _>>().map_err(|e| e.to_string())?)
        }
        ImageFormat::WebP => {
            let decoder = image::codecs::webp::WebPDecoder::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
            if decoder.has_animation() {
                Some(decoder.into_frames().take(MAX_FRAMES).collect::<Result<_, _>>().map_err(|e| e.to_string())?)
            } else {
                None
            }
        }
        _ => None,
    };
    let images = match animated {
        Some(frames) if frames.len() > 1 => {
            let (w, h) = frames[0].buffer().dimensions();
            let scale = fit_scale(w, h, frames.len(), blur);
            frames
                .into_iter()
                .map(|frame| {
                    let (numer, denom) = frame.delay().numer_denom_ms();
                    let ms = numer.checked_div(denom).unwrap_or(100);
                    // Browsers treat very short GIF delays as 100 ms; so do we.
                    let delay = Duration::from_millis(if ms < 20 { 100 } else { u64::from(ms) });
                    (prepare(frame.into_buffer(), scale, blur), delay)
                })
                .collect()
        }
        Some(mut frames) if !frames.is_empty() => {
            let image = frames.remove(0).into_buffer();
            let scale = fit_scale(image.width(), image.height(), 1, blur);
            vec![(prepare(image, scale, blur), Duration::ZERO)]
        }
        _ => {
            let image = image::load_from_memory(bytes).map_err(|e| e.to_string())?.into_rgba8();
            let scale = fit_scale(image.width(), image.height(), 1, blur);
            vec![(prepare(image, scale, blur), Duration::ZERO)]
        }
    };
    Ok(Frames { images })
}

/// A file name for the picture inside the backgrounds folder that does not clash.
fn unique_name(dir: &Path, stem: &str, extension: &str) -> String {
    let stem: String =
        stem.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).take(48).collect();
    let stem = if stem.trim_matches('-').is_empty() { "background".to_owned() } else { stem };
    let mut name = format!("{stem}.{extension}");
    let mut n = 2;
    while dir.join(&name).exists() {
        name = format!("{stem}-{n}.{extension}");
        n += 1;
    }
    name
}

fn extension_of(bytes: &[u8]) -> Result<&'static str, String> {
    match image::guess_format(bytes) {
        Ok(ImageFormat::Png) => Ok("png"),
        Ok(ImageFormat::Jpeg) => Ok("jpg"),
        Ok(ImageFormat::Gif) => Ok("gif"),
        Ok(ImageFormat::WebP) => Ok("webp"),
        Ok(ImageFormat::Bmp) => Ok("bmp"),
        _ => Err("This is not a PNG, JPEG, GIF, WebP or BMP picture.".to_owned()),
    }
}

/// Copies a picture into the backgrounds folder. Returns its new file name.
pub fn import_file(dir: &Path, file: &Path) -> Result<String, String> {
    let bytes = std::fs::read(file).map_err(|e| format!("Could not read {}: {e}", file.display()))?;
    let extension = extension_of(&bytes)?;
    decode(&bytes, 0)?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let stem = file.file_stem().map_or_else(|| "background".into(), |s| s.to_string_lossy().into_owned());
    let name = unique_name(dir, &stem, extension);
    std::fs::write(dir.join(&name), bytes).map_err(|e| e.to_string())?;
    Ok(name)
}

/// Downloads a picture into the backgrounds folder. Blocking: call it off the UI thread.
pub fn download(dir: &Path, url: &str) -> Result<String, String> {
    let url = url.trim();
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("Enter a web address starting with https://".to_owned());
    }
    let mut response = ureq::get(url).call().map_err(|e| format!("Could not download the picture: {e}"))?;
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(MAX_DOWNLOAD_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Could not download the picture: {e}"))?;
    if bytes.len() as u64 > MAX_DOWNLOAD_BYTES {
        return Err("The picture is larger than 64 MB.".to_owned());
    }
    let extension = extension_of(&bytes)?;
    decode(&bytes, 0)?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let stem = url
        .split(['?', '#'])
        .next()
        .and_then(|path| path.rsplit('/').find(|part| !part.is_empty()))
        .map(|last| last.rsplit_once('.').map_or(last, |(stem, _)| stem))
        .unwrap_or("background");
    let name = unique_name(dir, stem, extension);
    std::fs::write(dir.join(&name), bytes).map_err(|e| e.to_string())?;
    Ok(name)
}

/// Pictures already in the backgrounds folder, by name.
pub fn list(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut names: Vec<String> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().is_some_and(|ext| EXTENSIONS.contains(&ext.to_string_lossy().to_lowercase().as_str()))
        })
        .filter_map(|path| path.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    names.sort();
    names
}

pub fn remove(dir: &Path, name: &str) {
    let path: PathBuf = dir.join(name);
    if path.parent() == Some(dir) {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gif(frames: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
            for i in 0..frames {
                let image = RgbaImage::from_pixel(40, 20, image::Rgba([i as u8 * 50, 0, 0, 255]));
                let frame = image::Frame::from_parts(image, 0, 0, image::Delay::from_numer_denom_ms(70, 1));
                encoder.encode_frame(frame).unwrap();
            }
        }
        bytes
    }

    #[test]
    fn animated_gifs_keep_their_frames_and_timing() {
        let frames = decode(&gif(3), 0).unwrap();
        assert_eq!(frames.images.len(), 3);
        assert!(frames.images.iter().all(|(image, delay)| image.dimensions() == (40, 20) && delay.as_millis() == 70));
    }

    #[test]
    fn large_and_blurred_pictures_are_stored_small() {
        assert!(fit_scale(8000, 4000, 1, 0) * 8000.0 <= MAX_STILL_SIDE as f32 + 1.0);
        let animated = fit_scale(1920, 1080, 300, 0);
        assert!((1920.0 * 1080.0 * animated * animated * 300.0) as u64 <= ANIMATION_PIXEL_BUDGET + 1000);
        assert!(fit_scale(1920, 1080, 1, 24) < fit_scale(1920, 1080, 1, 0));
    }

    #[test]
    fn imported_pictures_get_safe_unique_names() {
        let dir = std::env::temp_dir().join(format!("magi-bg-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let source = dir.join("source");
        std::fs::create_dir_all(&source).unwrap();
        let file = source.join("my picture!.gif");
        std::fs::write(&file, gif(2)).unwrap();
        let backgrounds = dir.join("backgrounds");
        assert_eq!(import_file(&backgrounds, &file).unwrap(), "my-picture-.gif");
        assert_eq!(import_file(&backgrounds, &file).unwrap(), "my-picture--2.gif");
        assert_eq!(list(&backgrounds).len(), 2);
        std::fs::write(source.join("notes.txt"), "hello").unwrap();
        assert!(import_file(&backgrounds, &source.join("notes.txt")).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}

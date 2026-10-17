//! # magi-vision
//!
//! Screen analysis on raw pixel buffers. Nothing here touches the operating
//! system: callers capture pixels however they like and pass them in, which
//! keeps these algorithms easy to test and reusable from any engine.
//!
//! * [`find_template`] – locate an image inside a larger one (template matching).
//! * [`find_color`] – locate pixels of a given color.
//! * [`decode`] / [`encode_png`] – convert between image files and pixel buffers.
//!
//! This is the crate where a future local vision model would be added, as
//! another function that takes pixels and returns an answer.

mod color;
mod template;

pub use color::{ColorMatch, find_color};
pub use template::{Match, find_template};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum VisionError {
    #[error("could not read the image: {0}")]
    Decode(String),
    #[error("could not write the image: {0}")]
    Encode(String),
}

/// A borrowed image: tightly packed RGBA8 rows.
#[derive(Clone, Copy, Debug)]
pub struct ImageRef<'a> {
    pub width: u32,
    pub height: u32,
    pub rgba: &'a [u8],
}

impl<'a> ImageRef<'a> {
    /// Panics if the buffer does not hold exactly `width × height` pixels.
    pub fn new(width: u32, height: u32, rgba: &'a [u8]) -> Self {
        assert_eq!(rgba.len(), width as usize * height as usize * 4, "pixel buffer has the wrong size");
        Self { width, height, rgba }
    }
}

/// An owned image: tightly packed RGBA8 rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Bitmap {
    pub fn as_ref(&self) -> ImageRef<'_> {
        ImageRef::new(self.width, self.height, &self.rgba)
    }
}

/// Enlarges an image by a whole factor with bilinear filtering, which keeps
/// the edges of letters smooth (text recognition reads small text better
/// enlarged).
pub fn enlarge(image: ImageRef<'_>, factor: u32) -> Bitmap {
    let factor = factor.max(1);
    let (width, height) = (image.width * factor, image.height * factor);
    if factor == 1 || image.width == 0 || image.height == 0 {
        return Bitmap { width: image.width, height: image.height, rgba: image.rgba.to_vec() };
    }
    let stride = image.width as usize * 4;
    let mut rgba = vec![0u8; width as usize * height as usize * 4];
    let scale = 1.0 / factor as f32;
    for y in 0..height {
        // The centre of the output pixel, in source pixels.
        let sy = ((y as f32 + 0.5) * scale - 0.5).clamp(0.0, (image.height - 1) as f32);
        let (y0, fy) = (sy.floor() as usize, sy.fract());
        let y1 = (y0 + 1).min(image.height as usize - 1);
        for x in 0..width {
            let sx = ((x as f32 + 0.5) * scale - 0.5).clamp(0.0, (image.width - 1) as f32);
            let (x0, fx) = (sx.floor() as usize, sx.fract());
            let x1 = (x0 + 1).min(image.width as usize - 1);
            let o = (y as usize * width as usize + x as usize) * 4;
            for c in 0..4 {
                let at = |xx: usize, yy: usize| f32::from(image.rgba[yy * stride + xx * 4 + c]);
                let top = at(x0, y0) * (1.0 - fx) + at(x1, y0) * fx;
                let bottom = at(x0, y1) * (1.0 - fx) + at(x1, y1) * fx;
                rgba[o + c] = (top * (1.0 - fy) + bottom * fy).round() as u8;
            }
        }
    }
    Bitmap { width, height, rgba }
}

/// Shrinks an image by a whole factor, each pixel the average of a
/// `factor × factor` block. Matching shrunk images is much faster, at the
/// cost of fine detail.
pub fn shrink(image: ImageRef<'_>, factor: u32) -> Bitmap {
    let factor = factor.max(1);
    let (width, height) = ((image.width / factor).max(1), (image.height / factor).max(1));
    if factor == 1 {
        return Bitmap { width: image.width, height: image.height, rgba: image.rgba.to_vec() };
    }
    let mut rgba = vec![0u8; width as usize * height as usize * 4];
    let stride = image.width as usize * 4;
    for y in 0..height as usize {
        for x in 0..width as usize {
            let mut sum = [0u32; 4];
            let mut count = 0;
            for dy in 0..factor as usize {
                let row = y * factor as usize + dy;
                if row >= image.height as usize {
                    break;
                }
                for dx in 0..factor as usize {
                    let col = x * factor as usize + dx;
                    if col >= image.width as usize {
                        break;
                    }
                    let i = row * stride + col * 4;
                    for (total, value) in sum.iter_mut().zip(&image.rgba[i..i + 4]) {
                        *total += u32::from(*value);
                    }
                    count += 1;
                }
            }
            let o = (y * width as usize + x) * 4;
            for c in 0..4 {
                rgba[o + c] = (sum[c] / count.max(1)) as u8;
            }
        }
    }
    Bitmap { width, height, rgba }
}

/// Decodes an image file (PNG, JPEG, BMP, WebP or GIF) into pixels.
pub fn decode(bytes: &[u8]) -> Result<Bitmap, VisionError> {
    let image = image::load_from_memory(bytes).map_err(|e| VisionError::Decode(e.to_string()))?.into_rgba8();
    Ok(Bitmap { width: image.width(), height: image.height(), rgba: image.into_raw() })
}

/// Encodes pixels as a PNG file.
pub fn encode_png(image: ImageRef<'_>) -> Result<Vec<u8>, VisionError> {
    use image::ImageEncoder as _;
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(image.rgba, image.width, image.height, image::ExtendedColorType::Rgba8)
        .map_err(|e| VisionError::Encode(e.to_string()))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_round_trip_preserves_pixels() {
        let rgba: Vec<u8> = (0..4 * 3 * 4).map(|i| (i * 7) as u8).collect();
        let png = encode_png(ImageRef::new(4, 3, &rgba)).unwrap();
        let back = decode(&png).unwrap();
        assert_eq!((back.width, back.height), (4, 3));
        assert_eq!(back.rgba, rgba);
    }

    #[test]
    fn garbage_is_a_decode_error() {
        assert!(matches!(decode(b"not an image"), Err(VisionError::Decode(_))));
    }
}

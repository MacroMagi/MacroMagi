//! # magi-vision
//!
//! Screen analysis on raw pixel buffers. Nothing here touches the operating
//! system: callers capture pixels however they like and pass them in, which
//! keeps these algorithms easy to test and reusable from any engine.
//!
//! * [`find_template`] – locate an image inside a larger one (template matching).
//! * [`find_color`] – locate pixels of a given colour.
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

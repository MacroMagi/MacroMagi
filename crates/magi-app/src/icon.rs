//! MacroMagi's icon, compiled into the executable. The sources are in
//! `assets/icon`; the Windows executable also carries it as a resource (see
//! `build.rs`).

use std::path::{Path, PathBuf};

use egui::{Color32, ColorImage, Context, TextureHandle, TextureOptions};

const PNG_256: &[u8] = include_bytes!("../../../assets/icon/magi-256.png");
const PNG_128: &[u8] = include_bytes!("../../../assets/icon/magi-128.png");
const ICO: &[u8] = include_bytes!("../../../assets/icon/magi.ico");

fn decode(png: &[u8]) -> image::RgbaImage {
    image::load_from_memory(png).expect("the built-in icon is a valid PNG").into_rgba8()
}

/// The icon of the window and taskbar button.
pub fn window_icon() -> egui::IconData {
    let image = decode(PNG_256);
    let (width, height) = image.dimensions();
    egui::IconData { rgba: image.into_raw(), width, height }
}

/// The logo for the banner, in `color`: the theme color the buttons use. The
/// drawing is dark with white details; the drawing takes the color and the
/// details are cut out, so whatever is behind the banner shows through them.
pub fn logo(ctx: &Context, color: Color32) -> TextureHandle {
    let image = tint(decode(PNG_128), color);
    let size = [image.width() as usize, image.height() as usize];
    let picture = ColorImage::from_rgba_unmultiplied(size, image.as_raw());
    let [r, g, b, _] = color.to_array();
    ctx.load_texture(format!("logo-{r:02x}{g:02x}{b:02x}"), picture, TextureOptions::LINEAR)
}

/// Recolors the drawing by brightness: its dark ink becomes `color` and its
/// white parts become see-through, with the soft edges in between kept soft.
fn tint(mut image: image::RgbaImage, color: Color32) -> image::RgbaImage {
    // The drawing's ink, #18181b.
    const INK: f32 = 0x18 as f32 / 255.0;
    let [r, g, b, _] = color.to_array();
    for pixel in image.pixels_mut() {
        let [pr, pg, pb, a] = pixel.0;
        let brightness = (0.2126 * f32::from(pr) + 0.7152 * f32::from(pg) + 0.0722 * f32::from(pb)) / 255.0;
        let white = ((brightness - INK) / (1.0 - INK)).clamp(0.0, 1.0);
        pixel.0 = [r, g, b, (f32::from(a) * (1.0 - white)).round() as u8];
    }
    image
}

/// Writes the icon files a desktop shortcut points at into `dir`.
/// Returns the `.ico` and `.png` paths.
pub fn write_shortcut_icons(dir: &Path) -> std::io::Result<(PathBuf, PathBuf)> {
    std::fs::create_dir_all(dir)?;
    let (ico, png) = (dir.join("macromagi.ico"), dir.join("macromagi.png"));
    std::fs::write(&ico, ICO)?;
    std::fs::write(&png, PNG_256)?;
    Ok((ico, png))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_icons_decode() {
        let icon = window_icon();
        assert_eq!((icon.width, icon.height), (256, 256));
        assert_eq!(decode(PNG_128).dimensions(), (128, 128));
        assert_eq!(&ICO[..4], &[0, 0, 1, 0], "an ICO header");
    }

    #[test]
    fn the_logo_takes_the_theme_color_and_cuts_out_the_white() {
        let mut image = image::RgbaImage::new(3, 1);
        image.put_pixel(0, 0, image::Rgba([0x18, 0x18, 0x1b, 255]));
        image.put_pixel(1, 0, image::Rgba([255, 255, 255, 255]));
        image.put_pixel(2, 0, image::Rgba([0x18, 0x18, 0x1b, 0]));
        let purple = Color32::from_rgb(140, 80, 220);
        let tinted = tint(image, purple);
        assert_eq!(tinted.get_pixel(0, 0).0, [140, 80, 220, 255], "the ink takes the color");
        assert_eq!(tinted.get_pixel(1, 0).0[3], 0, "the white is cut out");
        assert_eq!(tinted.get_pixel(2, 0).0[3], 0, "the transparent stays transparent");
        // The real logo has both.
        let logo = tint(decode(PNG_128), purple);
        assert!(logo.pixels().any(|p| p.0[3] == 255) && logo.pixels().any(|p| p.0[3] == 0));
    }
}

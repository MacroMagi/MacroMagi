//! MacroMagi's icon, compiled into the executable. The sources are in
//! `assets/icon`; the Windows executable also carries it as a resource (see
//! `build.rs`).

use std::path::{Path, PathBuf};

use egui::{ColorImage, Context, TextureHandle, TextureOptions};

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

/// The logo for the banner. The drawing is black with white details; in
/// dark mode it is inverted so it stands out against the dark background.
pub fn logo(ctx: &Context, dark: bool) -> TextureHandle {
    let mut image = decode(PNG_128);
    if dark {
        for pixel in image.pixels_mut() {
            let [r, g, b, a] = pixel.0;
            pixel.0 = [255 - r, 255 - g, 255 - b, a];
        }
    }
    let size = [image.width() as usize, image.height() as usize];
    let color = ColorImage::from_rgba_unmultiplied(size, image.as_raw());
    ctx.load_texture(if dark { "logo-dark" } else { "logo-light" }, color, TextureOptions::LINEAR)
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
}

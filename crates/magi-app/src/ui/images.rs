//! Turns image assets into GPU textures, once, for display in the UI.

use std::collections::HashMap;

use egui::{ColorImage, Context, TextureHandle, TextureOptions, Vec2};
use magi_core::assets::AssetStore;

#[derive(Default)]
pub struct ImageCache {
    /// `None` marks an asset that could not be decoded, so it is not retried every frame.
    textures: HashMap<String, Option<TextureHandle>>,
}

impl ImageCache {
    /// The texture for an image asset, loading it on first use.
    pub fn get(&mut self, ctx: &Context, assets: &AssetStore, id: &str) -> Option<TextureHandle> {
        if let Some(cached) = self.textures.get(id) {
            return cached.clone();
        }
        let texture = assets.image(id).and_then(|bytes| magi_vision::decode(bytes).ok()).map(|bitmap| {
            let image =
                ColorImage::from_rgba_unmultiplied([bitmap.width as usize, bitmap.height as usize], &bitmap.rgba);
            // Template images are usually tiny; nearest-neighbour keeps them crisp when enlarged.
            ctx.load_texture(format!("asset:{id}"), image, TextureOptions::NEAREST)
        });
        self.textures.insert(id.to_owned(), texture.clone());
        texture
    }

    /// Drops textures of assets that no longer exist.
    pub fn retain_existing(&mut self, assets: &AssetStore) {
        self.textures.retain(|id, _| assets.contains(id));
    }
}

/// The largest size with `texture`'s aspect ratio that fits inside `bounds`,
/// never enlarging beyond `max_scale` times the image's own size.
pub fn fit_size(texture: &TextureHandle, bounds: Vec2, max_scale: f32) -> Vec2 {
    let size = texture.size_vec2();
    if size.x <= 0.0 || size.y <= 0.0 {
        return Vec2::ZERO;
    }
    let scale = (bounds.x / size.x).min(bounds.y / size.y).min(max_scale);
    size * scale
}

/// A picture of the screen as a texture. Pictures larger than the graphics
/// card allows (or than egui assumes before the first frame) are shrunk by a
/// whole factor to fit; they are only ever shown stretched over a window.
pub fn screen_texture(ctx: &Context, name: &str, width: u32, height: u32, rgba: &[u8]) -> TextureHandle {
    let max_side = ctx.input(|i| i.max_texture_side).max(256);
    let (w, h) = (width as usize, height as usize);
    let factor = w.max(h).div_ceil(max_side).max(1);
    let image = if factor == 1 {
        ColorImage::from_rgba_unmultiplied([w, h], rgba)
    } else {
        let (sw, sh) = (w / factor, h / factor);
        let mut small = Vec::with_capacity(sw * sh * 4);
        for y in 0..sh {
            for x in 0..sw {
                let i = ((y * factor) * w + x * factor) * 4;
                small.extend_from_slice(&rgba[i..i + 4]);
            }
        }
        ColorImage::from_rgba_unmultiplied([sw, sh], &small)
    };
    ctx.load_texture(name, image, TextureOptions::LINEAR)
}

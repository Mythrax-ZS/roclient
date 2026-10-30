//! Interface textures (item icons, window art, minimaps, status icons, loading screens),
//! upscaled per the `[upscale]` settings.
//!
//! NEVER CHARACTER SPRITES: sprite previews, cursors, cut-ins and emblems are made
//! elsewhere and never come through here; `class_of` returns None for any sprite path, and
//! the upscaler refuses them again on its own.
//!
//! An upscaled texture is stored larger than its art but must lay out at its original
//! size, so windows that place art pixel-for-pixel don't move. Use `ui_size()` (not egui's
//! `size_vec2()`) for any texture loaded here: it divides the stored size back down.

use std::collections::HashMap;
use std::sync::{Mutex, RwLock};

use ro_render::{AssetClass, Upscaler};

static UPSCALER: RwLock<Option<Upscaler>> = RwLock::new(None);
/// Stored-to-art size ratio of the textures that were upscaled (absent = 1).
static SCALES: Mutex<Option<HashMap<egui::TextureId, egui::Vec2>>> = Mutex::new(None);

/// Applies new upscaling settings to interface textures loaded from now on.
pub fn set_upscaler(up: Upscaler) {
    if let Ok(mut u) = UPSCALER.write() {
        *u = Some(up);
    }
}

/// Pixel art drawn for this client (the bundled Samurai skill icons) is enlarged by whole
/// pixels, never smoothed: its one-pixel details would blur. Drawn with linear filtering at
/// the UI size, it stays crisp at any UI scale.
const CRISP_FACTOR: u32 = 4;

fn crisp(path: &str) -> bool {
    path.to_ascii_lowercase().contains(r"\item\sam_")
}

fn enlarge(img: &ro_render::Rgba, k: u32) -> ro_render::Rgba {
    let (w, h) = (img.width * k, img.height * k);
    let mut pixels = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let i = (((y / k) * img.width + x / k) * 4) as usize;
            pixels.extend_from_slice(&img.pixels[i..i + 4]);
        }
    }
    ro_render::Rgba { width: w, height: h, pixels }
}

/// The asset class for an interface texture path, or None when it must not be upscaled.
pub fn class_of(path: &str) -> Option<AssetClass> {
    if ro_render::is_sprite_path(path) {
        return None;
    }
    let p = path.to_ascii_lowercase();
    Some(if p.contains("\\item\\") || p.contains("\\collection\\") { AssetClass::ItemIcon } else { AssetClass::Interface })
}

/// Decodes `path` and uploads it to egui, upscaled if the settings say so. `options` is
/// used as-is for textures left at their own size (classic pixel art keeps NEAREST);
/// upscaled ones are filtered linearly with mipmaps, so they stay smooth at any UI scale.
pub fn load(ctx: &egui::Context, vfs: &ro_vfs::Vfs, path: &str, options: egui::TextureOptions) -> Option<egui::TextureHandle> {
    let data = vfs.read(path).ok()??;
    let img = ro_formats::image::decode(&data, path).ok()?;
    let (w0, h0) = (img.width, img.height);
    let img = match (class_of(path), UPSCALER.read().ok().and_then(|u| u.clone())) {
        _ if crisp(path) => enlarge(&img, CRISP_FACTOR),
        (Some(class), Some(up)) => up.apply(class, path, img),
        _ => img,
    };
    let color = egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.pixels);
    if (img.width, img.height) == (w0, h0) {
        return Some(ctx.load_texture(path, color, options));
    }
    let smooth = egui::TextureOptions { mipmap_mode: Some(egui::TextureFilter::Linear), ..egui::TextureOptions::LINEAR };
    let tex = ctx.load_texture(path, color, if crisp(path) { egui::TextureOptions::LINEAR } else { smooth });
    if let Ok(mut s) = SCALES.lock() {
        s.get_or_insert_with(HashMap::new).insert(tex.id(), egui::vec2(img.width as f32 / w0 as f32, img.height as f32 / h0 as f32));
    }
    Some(tex)
}

pub trait UiSize {
    /// The art's own size in points (the stored size divided by any upscale factor).
    fn ui_size(&self) -> egui::Vec2;
}

impl UiSize for egui::TextureHandle {
    fn ui_size(&self) -> egui::Vec2 {
        let scale = SCALES.lock().ok().and_then(|s| s.as_ref()?.get(&self.id()).copied()).unwrap_or(egui::Vec2::splat(1.0));
        self.size_vec2() / scale
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_pixel_art_is_enlarged_by_whole_pixels() {
        assert!(crisp(r"data\texture\유저인터페이스\item\sam_kusanagi_guard.bmp"));
        assert!(!crisp(r"data\texture\유저인터페이스\item\kn_autocounter.bmp"));
        let img = ro_render::Rgba { width: 2, height: 1, pixels: vec![1, 2, 3, 255, 4, 5, 6, 0] };
        let big = enlarge(&img, 4);
        assert_eq!((big.width, big.height), (8, 4));
        assert_eq!(&big.pixels[12..16], &[1, 2, 3, 255]); // (3, 0): still the first pixel
        assert_eq!(&big.pixels[16..20], &[4, 5, 6, 0]); // (4, 0): the second, transparency kept
        assert_eq!(&big.pixels[(3 * 8 + 7) * 4..(3 * 8 + 8) * 4], &[4, 5, 6, 0]);
    }
}

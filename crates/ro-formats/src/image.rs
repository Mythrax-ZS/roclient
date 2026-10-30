//! Texture decoding. RO uses pure magenta as the transparent color in BMPs.

use anyhow::{Context, Result};

#[derive(Clone)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// Decodes BMP/TGA/PNG/JPG/GIF. Near-magenta pixels (the RO color key) become transparent.
/// `name` supplies the extension, which TGA needs since it has no signature.
pub fn decode(data: &[u8], name: &str) -> Result<Rgba> {
    let img = match image::ImageFormat::from_path(name) {
        Ok(fmt) => image::load_from_memory_with_format(data, fmt).or_else(|_| image::load_from_memory(data)),
        Err(_) => image::load_from_memory(data),
    }
    .context("decode texture")?
    .to_rgba8();
    let (width, height) = img.dimensions();
    let mut pixels = img.into_raw();
    for p in pixels.chunks_exact_mut(4) {
        if p[0] > 0xf0 && p[1] < 0x10 && p[2] > 0xf0 {
            p.copy_from_slice(&[0, 0, 0, 0]);
        }
    }
    Ok(Rgba { width, height, pixels })
}

impl std::fmt::Debug for Rgba {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Rgba({}x{})", self.width, self.height)
    }
}

impl Rgba {
    /// Next mip level by 2x2 box filter, weighting color by alpha so transparent
    /// texels do not bleed black into edges.
    pub fn downsample(&self) -> Option<Rgba> {
        if self.width == 1 && self.height == 1 {
            return None;
        }
        let (w, h) = ((self.width / 2).max(1), (self.height / 2).max(1));
        let mut out = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let mut acc = [0u32; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let sx = (x * 2 + dx).min(self.width - 1);
                    let sy = (y * 2 + dy).min(self.height - 1);
                    let i = ((sy * self.width + sx) * 4) as usize;
                    let a = self.pixels[i + 3] as u32;
                    for c in 0..3 {
                        acc[c] += self.pixels[i + c] as u32 * a;
                    }
                    acc[3] += a;
                }
                let o = ((y * w + x) * 4) as usize;
                if acc[3] > 0 {
                    for c in 0..3 {
                        out[o + c] = (acc[c] / acc[3]) as u8;
                    }
                }
                out[o + 3] = (acc[3] / 4) as u8;
            }
        }
        Some(Rgba { width: w, height: h, pixels: out })
    }
}

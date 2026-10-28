//! SPR: sprite images. Palette-indexed frames (index 0 transparent) followed by
//! optional RGBA frames, with the 256-color palette at the end of the file.

use anyhow::{ensure, Result};

use crate::{image::Rgba, reader::Reader};

#[derive(Debug, Clone)]
pub struct IndexedImage {
    pub width: u16,
    pub height: u16,
    pub pixels: Vec<u8>,
}

#[derive(Debug)]
pub struct Spr {
    pub version: (u8, u8),
    pub indexed: Vec<IndexedImage>,
    /// Already RGBA, top-down.
    pub rgba: Vec<Rgba>,
    /// RGB + unused byte per entry.
    pub palette: [[u8; 4]; 256],
}

impl Spr {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        ensure!(r.bytes(2)? == b"SP", "not an SPR file");
        let minor = r.u8()?;
        let major = r.u8()?;
        let version = (major, minor);
        let indexed_count = r.u16()? as usize;
        let rgba_count = if version >= (2, 0) { r.u16()? as usize } else { 0 };

        let mut indexed = Vec::with_capacity(indexed_count);
        for _ in 0..indexed_count {
            let width = r.u16()?;
            let height = r.u16()?;
            let n = width as usize * height as usize;
            let pixels = if version >= (2, 1) {
                // RLE: a 0 byte is followed by a run length of zeros.
                let size = r.u16()? as usize;
                let src = r.bytes(size)?;
                let mut out = Vec::with_capacity(n);
                let mut i = 0;
                while i < src.len() && out.len() < n {
                    let b = src[i];
                    i += 1;
                    if b == 0 {
                        let run = src.get(i).copied().unwrap_or(1).max(1);
                        i += 1;
                        out.resize(out.len() + run as usize, 0);
                    } else {
                        out.push(b);
                    }
                }
                out.resize(n, 0);
                out
            } else {
                r.bytes(n)?.to_vec()
            };
            indexed.push(IndexedImage { width, height, pixels });
        }

        let mut rgba = Vec::with_capacity(rgba_count);
        for _ in 0..rgba_count {
            let width = r.u16()? as u32;
            let height = r.u16()? as u32;
            let src = r.bytes((width * height * 4) as usize)?;
            // Stored bottom-up as ABGR.
            let mut pixels = vec![0u8; src.len()];
            for y in 0..height {
                for x in 0..width {
                    let s = (((height - 1 - y) * width + x) * 4) as usize;
                    let d = ((y * width + x) * 4) as usize;
                    pixels[d..d + 4].copy_from_slice(&[src[s + 3], src[s + 2], src[s + 1], src[s]]);
                }
            }
            rgba.push(Rgba { width, height, pixels });
        }

        let mut palette = [[0u8; 4]; 256];
        if let Ok(p) = r.bytes(1024) {
            for (i, c) in p.chunks_exact(4).enumerate() {
                palette[i] = [c[0], c[1], c[2], c[3]];
            }
        }
        Ok(Self { version, indexed, rgba, palette })
    }

    /// All frames as RGBA: indexed frames (through `palette`, or the sprite's own)
    /// followed by the RGBA frames. ACT layers index into this list by type.
    pub fn images(&self, palette: Option<&[[u8; 4]; 256]>) -> (Vec<Rgba>, Vec<Rgba>) {
        let pal = palette.unwrap_or(&self.palette);
        let indexed = self
            .indexed
            .iter()
            .map(|img| {
                let mut pixels = Vec::with_capacity(img.pixels.len() * 4);
                for &i in &img.pixels {
                    let c = pal[i as usize];
                    pixels.extend([c[0], c[1], c[2], if i == 0 { 0 } else { 255 }]);
                }
                Rgba { width: img.width as u32, height: img.height as u32, pixels }
            })
            .collect();
        let rgba = self.rgba.iter().map(|i| Rgba { width: i.width, height: i.height, pixels: i.pixels.clone() }).collect();
        (indexed, rgba)
    }
}

/// A standalone .pal file (hair / body dyes): 256 RGBX entries.
pub fn parse_palette(data: &[u8]) -> Result<[[u8; 4]; 256]> {
    ensure!(data.len() >= 1024, "palette too short");
    let mut pal = [[0u8; 4]; 256];
    for (i, c) in data[..1024].chunks_exact(4).enumerate() {
        pal[i] = [c[0], c[1], c[2], c[3]];
    }
    Ok(pal)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sample_if_present() {
        let Ok(path) = std::env::var("RO_SPR_SAMPLE") else { return };
        let spr = Spr::parse(&std::fs::read(path).unwrap()).unwrap();
        assert!(!spr.indexed.is_empty() || !spr.rgba.is_empty());
        for img in &spr.indexed {
            assert_eq!(img.pixels.len(), img.width as usize * img.height as usize);
        }
    }
}

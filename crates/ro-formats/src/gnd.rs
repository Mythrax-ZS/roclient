//! GND: ground mesh. A grid of cubes (one per 2x2 GAT cells), each with four corner
//! heights and up to three textured faces: top, front (+y side) and right (+x side).

use anyhow::{bail, ensure, Result};

use crate::reader::Reader;

#[derive(Debug, Clone, Copy)]
pub struct Surface {
    /// Corner UVs in order: (x,y), (x+1,y), (x,y+1), (x+1,y+1).
    pub u: [f32; 4],
    pub v: [f32; 4],
    /// Index into `Gnd::textures`; negative means untextured.
    pub texture: i16,
    pub lightmap: u16,
    /// BGRA vertex tint.
    pub color: [u8; 4],
}

#[derive(Debug, Clone, Copy)]
pub struct Cube {
    /// Corner heights, same order as `Surface::u`. RO heights grow downward.
    pub heights: [f32; 4],
    /// Surface indices, -1 when absent.
    pub top: i32,
    pub front: i32,
    pub right: i32,
}

#[derive(Debug)]
pub struct Gnd {
    pub version: (u8, u8),
    pub width: u32,
    pub height: u32,
    /// World units per cube edge (10).
    pub zoom: f32,
    pub textures: Vec<String>,
    pub lightmaps: Lightmaps,
    pub surfaces: Vec<Surface>,
    pub cubes: Vec<Cube>,
    /// GND 1.8+ carries the map's water (RSW 2.6+ no longer does): the main settings, then
    /// a grid of zones with their own levels (and, from 1.9, their own wave settings).
    pub water: Option<crate::rsw::Water>,
    /// (columns, rows, per-zone water, row-major).
    pub water_zones: Option<(u32, u32, Vec<crate::rsw::Water>)>,
}

/// Baked per-surface lighting: an 8x8 shadow (brightness) map plus an RGB color map.
#[derive(Debug, Default)]
pub struct Lightmaps {
    pub width: u32,
    pub height: u32,
    data: Vec<u8>,
}

impl Lightmaps {
    pub fn len(&self) -> usize {
        let stride = (self.width * self.height * 4) as usize;
        if stride == 0 { 0 } else { self.data.len() / stride }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// (shadow bytes, RGB bytes) of lightmap `i`.
    pub fn get(&self, i: usize) -> Option<(&[u8], &[u8])> {
        let n = (self.width * self.height) as usize;
        let lm = self.data.get(i * n * 4..(i + 1) * n * 4)?;
        Some(lm.split_at(n))
    }
}

impl Gnd {
    pub fn cube(&self, x: u32, y: u32) -> Option<&Cube> {
        (x < self.width && y < self.height).then(|| &self.cubes[(y * self.width + x) as usize])
    }

    pub fn surface(&self, index: i32) -> Option<&Surface> {
        usize::try_from(index).ok().and_then(|i| self.surfaces.get(i))
    }

    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        ensure!(r.bytes(4)? == b"GRGN", "not a GND file");
        let version = (r.u8()?, r.u8()?);
        if version < (1, 5) {
            bail!("GND {}.{} unsupported", version.0, version.1);
        }
        let width = r.u32()?;
        let height = r.u32()?;
        let zoom = r.f32()?;

        let tex_count = r.u32()? as usize;
        let tex_len = r.u32()? as usize;
        let textures = (0..tex_count).map(|_| r.string(tex_len)).collect::<Result<_>>()?;

        // Lightmaps: count, texel width/height, cells per grid, then per lightmap
        // w*h shadow bytes followed by w*h RGB color triples.
        let lm_count = r.u32()? as usize;
        let lm_w = r.i32()? as u32;
        let lm_h = r.i32()? as u32;
        let _lm_cells = r.i32()?;
        let lightmaps = Lightmaps { width: lm_w, height: lm_h, data: r.bytes(lm_count * (lm_w * lm_h * 4) as usize)?.to_vec() };

        let surf_count = r.u32()? as usize;
        let mut surfaces = Vec::with_capacity(surf_count);
        for _ in 0..surf_count {
            let u = [r.f32()?, r.f32()?, r.f32()?, r.f32()?];
            let v = [r.f32()?, r.f32()?, r.f32()?, r.f32()?];
            let texture = r.i16()?;
            let lightmap = r.u16()?;
            let c = r.bytes(4)?;
            surfaces.push(Surface { u, v, texture, lightmap, color: [c[0], c[1], c[2], c[3]] });
        }

        let n = (width * height) as usize;
        let mut cubes = Vec::with_capacity(n);
        for _ in 0..n {
            let heights = [r.f32()?, r.f32()?, r.f32()?, r.f32()?];
            let (top, front, right) = if version >= (1, 6) {
                (r.i32()?, r.i32()?, r.i32()?)
            } else {
                (r.i16()? as i32, r.i16()? as i32, r.i16()? as i32)
            };
            cubes.push(Cube { heights, top, front, right });
        }
        // GND 1.8+: water settings, then a grid of zones (1.8: a level each; 1.9: all fields).
        let (mut water, mut water_zones) = (None, None);
        if version >= (1, 8) {
            let read = |r: &mut Reader, level: f32| -> Result<crate::rsw::Water> {
                Ok(crate::rsw::Water { level, kind: r.i32()?, wave_height: r.f32()?, wave_speed: r.f32()?, wave_pitch: r.f32()?, anim_speed: r.i32()? })
            };
            let level = r.f32()?;
            let w = read(&mut r, level)?;
            let (cols, rows) = (r.u32()?, r.u32()?);
            let mut zones = Vec::with_capacity((cols * rows) as usize);
            for _ in 0..cols.saturating_mul(rows).min(4096) {
                let level = r.f32()?;
                zones.push(if version >= (1, 9) { read(&mut r, level)? } else { crate::rsw::Water { level, ..w } });
            }
            water = Some(w);
            water_zones = Some((cols, rows, zones));
        }

        Ok(Self { version, width, height, zoom, textures, lightmaps, surfaces, cubes, water, water_zones })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs against an installed client when RO_GND_SAMPLE points at an extracted .gnd.
    #[test]
    fn parses_sample_if_present() {
        let Ok(path) = std::env::var("RO_GND_SAMPLE") else { return };
        let gnd = Gnd::parse(&std::fs::read(path).unwrap()).unwrap();
        assert!(gnd.width > 0 && gnd.height > 0);
        assert_eq!(gnd.cubes.len(), (gnd.width * gnd.height) as usize);
        assert!(gnd.cubes.iter().all(|c| c.top < gnd.surfaces.len() as i32));
    }

    #[test]
    fn rejects_garbage() {
        assert!(Gnd::parse(b"nope").is_err());
    }
}

//! STR: keyframed 2D effect animations (data\texture\effect\*.str). Each layer is a
//! textured quad in a 640x480 screen space centred on (320, 320); keyframes come as a base
//! key (type 0) optionally followed by a per-frame delta key (type 1).

use anyhow::{ensure, Result};

use crate::reader::Reader;

#[derive(Debug, Clone, Default)]
pub struct Key {
    pub frame: i32,
    /// 0: base values, 1: per-frame deltas for the preceding base key.
    pub kind: u32,
    pub pos: [f32; 2],
    pub uv: [f32; 8],
    /// Corner offsets: x for corners 0..4, then y for corners 0..4.
    pub xy: [f32; 8],
    pub aniframe: f32,
    pub anitype: u32,
    pub delay: f32,
    pub angle: f32,
    /// 0..255 RGBA.
    pub color: [f32; 4],
    /// D3DBLEND values; dest 6 (INVSRCALPHA) is normal blending, anything else adds.
    pub src_blend: u32,
    pub dst_blend: u32,
}

#[derive(Debug, Clone, Default)]
pub struct Layer {
    pub textures: Vec<String>,
    pub keys: Vec<Key>,
}

#[derive(Debug, Clone)]
pub struct Str {
    pub fps: u32,
    pub max_key: u32,
    pub layers: Vec<Layer>,
}

/// One layer's quad at some frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Quad {
    pub layer: usize,
    pub texture: usize,
    /// Corner offsets from the effect's centre in effect pixels (x right, y down),
    /// clockwise from top-left.
    pub corners: [[f32; 2]; 4],
    /// 0..1 RGBA.
    pub color: [f32; 4],
    pub additive: bool,
}

impl Str {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        ensure!(r.bytes(4)? == b"STRM", "not a STR file");
        let version = r.u32()?;
        ensure!(version == 0x94, "STR version {version:#x} unsupported");
        let fps = r.u32()?;
        let max_key = r.u32()?;
        let layer_count = r.u32()?;
        r.bytes(16)?;
        let mut layers = Vec::with_capacity(layer_count as usize);
        for _ in 0..layer_count {
            let tex_count = r.i32()?.max(0);
            let mut textures = Vec::with_capacity(tex_count as usize);
            for _ in 0..tex_count {
                textures.push(r.string(128)?);
            }
            let key_count = r.i32()?.max(0);
            let mut keys = Vec::with_capacity(key_count as usize);
            for _ in 0..key_count {

                let frame = r.i32()?;
                let kind = r.u32()?;
                let pos = floats(&mut r, 2)?;
                let uv = floats(&mut r, 8)?;
                let xy = floats(&mut r, 8)?;
                let aniframe = r.f32()?;
                let anitype = r.u32()?;
                let delay = r.f32()?;
                let angle = r.f32()?;
                let color = floats(&mut r, 4)?;
                let src_blend = r.u32()?;
                let dst_blend = r.u32()?;
                r.u32()?; // mtpreset
                keys.push(Key {
                    frame,
                    kind,
                    pos: [pos[0], pos[1]],
                    uv: uv.try_into().unwrap(),
                    xy: xy.try_into().unwrap(),
                    aniframe,
                    anitype,
                    delay,
                    angle,
                    color: color.try_into().unwrap(),
                    src_blend,
                    dst_blend,
                });
            }
            layers.push(Layer { textures, keys });
        }
        Ok(Self { fps: fps.max(1), max_key, layers })
    }

    /// Length in milliseconds.
    pub fn duration_ms(&self) -> f32 {
        self.max_key as f32 * 1000.0 / self.fps as f32
    }

    /// The quads to draw at `frame`.
    pub fn quads(&self, frame: f32) -> Vec<Quad> {
        let mut out = vec![];
        for (li, layer) in self.layers.iter().enumerate() {
            if layer.textures.is_empty() {
                continue;
            }
            // Latest base key at or before the frame, and its delta key if it has one.
            let Some(bi) = layer.keys.iter().rposition(|k| k.kind == 0 && k.frame as f32 <= frame) else { continue };
            let base = &layer.keys[bi];
            let delta = layer.keys.get(bi + 1).filter(|k| k.kind == 1);
            // A base key without a delta only shows on its own frame.
            if delta.is_none() && frame.floor() as i32 != base.frame {
                continue;
            }
            let t = frame - base.frame as f32;
            let lerp = |b: f32, d: Option<f32>| b + d.unwrap_or(0.0) * t;
            let pos = [lerp(base.pos[0], delta.map(|d| d.pos[0])), lerp(base.pos[1], delta.map(|d| d.pos[1]))];
            let xy: [f32; 8] = std::array::from_fn(|i| lerp(base.xy[i], delta.map(|d| d.xy[i])));
            let color: [f32; 4] = std::array::from_fn(|i| (lerp(base.color[i], delta.map(|d| d.color[i])) / 255.0).clamp(0.0, 1.0));
            let angle = lerp(base.angle, delta.map(|d| d.angle));
            let n = layer.textures.len() as f32;
            let step = delta.map_or(0.0, |d| d.delay) * t;
            let ani = match delta.map(|d| d.anitype).unwrap_or(0) {
                1 => base.aniframe + delta.map_or(0.0, |d| d.aniframe) * t,
                2 => (base.aniframe + step).min(n - 1.0),
                3 => (base.aniframe + step).rem_euclid(n),
                4 => (base.aniframe - step).rem_euclid(n),
                _ => base.aniframe,
            };
            if color[3] <= 0.0 {
                continue;
            }
            // Angle is in 1/1024ths of a turn.
            let (s, c) = (angle / 1024.0 * std::f32::consts::TAU).sin_cos();
            let corners = std::array::from_fn(|i| {
                let (x, y) = (xy[i], xy[i + 4]);
                [pos[0] - 320.0 + x * c - y * s, pos[1] - 320.0 + x * s + y * c]
            });
            out.push(Quad {
                layer: li,
                texture: (ani.max(0.0) as usize).min(layer.textures.len() - 1),
                corners,
                color,
                // Only INVSRCALPHA (6) is normal blending; ONE / DESTALPHA effectively add.
                additive: base.dst_blend != 6,
            });
        }
        out
    }
}

fn floats(r: &mut Reader, n: usize) -> Result<Vec<f32>> {
    (0..n).map(|_| r.f32()).collect()
}

//! ACT: sprite animation. Actions (usually 8 directions each) made of frames, each frame
//! a stack of layers referencing SPR images, plus anchors used to attach heads.

use anyhow::{ensure, Result};

use crate::reader::Reader;

#[derive(Debug, Clone, Copy)]
pub struct Layer {
    /// Pixel offset from the actor's feet (y down).
    pub x: i32,
    pub y: i32,
    /// Image index within its type; -1 for none.
    pub sprite: i32,
    pub mirror: bool,
    pub color: [u8; 4],
    pub scale: [f32; 2],
    /// Degrees.
    pub rotation: i32,
    /// 0 = indexed, 1 = RGBA.
    pub kind: i32,
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub layers: Vec<Layer>,
    pub sound: i32,
    pub anchors: Vec<[i32; 2]>,
}

#[derive(Debug, Clone)]
pub struct Action {
    pub frames: Vec<Frame>,
    /// Frame delay in milliseconds.
    pub delay_ms: f32,
}

#[derive(Debug)]
pub struct Act {
    pub version: (u8, u8),
    pub actions: Vec<Action>,
    pub sounds: Vec<String>,
}

impl Act {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        ensure!(r.bytes(2)? == b"AC", "not an ACT file");
        let minor = r.u8()?;
        let major = r.u8()?;
        let version = (major, minor);
        let v = |maj, min| version >= (maj, min);
        let action_count = r.u16()? as usize;
        r.bytes(10)?;

        let mut actions = Vec::with_capacity(action_count);
        for _ in 0..action_count {
            let frame_count = r.u32()? as usize;
            let mut frames = Vec::with_capacity(frame_count.min(1024));
            for _ in 0..frame_count {
                r.bytes(32)?; // unused clip ranges
                let layer_count = r.u32()? as usize;
                let mut layers = Vec::with_capacity(layer_count.min(64));
                for _ in 0..layer_count {
                    let x = r.i32()?;
                    let y = r.i32()?;
                    let sprite = r.i32()?;
                    let mirror = r.i32()? != 0;
                    let (color, scale, rotation, kind) = if v(2, 0) {
                        let c = r.bytes(4)?;
                        let sx = r.f32()?;
                        let sy = if v(2, 4) { r.f32()? } else { sx };
                        let rotation = r.i32()?;
                        let kind = r.i32()?;
                        if v(2, 5) {
                            r.bytes(8)?; // width, height
                        }
                        ([c[0], c[1], c[2], c[3]], [sx, sy], rotation, kind)
                    } else {
                        ([255; 4], [1.0, 1.0], 0, 0)
                    };
                    layers.push(Layer { x, y, sprite, mirror, color, scale, rotation, kind });
                }
                let sound = if v(2, 0) { r.i32()? } else { -1 };
                let mut anchors = vec![];
                if v(2, 3) {
                    let n = r.i32()?.max(0) as usize;
                    for _ in 0..n {
                        r.bytes(4)?;
                        anchors.push([r.i32()?, r.i32()?]);
                        r.bytes(4)?;
                    }
                }
                frames.push(Frame { layers, sound, anchors });
            }
            actions.push(Action { frames, delay_ms: 150.0 });
        }

        let mut sounds = vec![];
        if v(2, 1) {
            let n = r.i32()?.max(0) as usize;
            for _ in 0..n {
                sounds.push(r.string(40)?);
            }
        }
        if v(2, 2) {
            for a in &mut actions {
                // Intervals are in units of 25 ms.
                a.delay_ms = r.f32()? * 25.0;
            }
        }
        Ok(Self { version, actions, sounds })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sample_if_present() {
        let Ok(path) = std::env::var("RO_ACT_SAMPLE") else { return };
        let act = Act::parse(&std::fs::read(path).unwrap()).unwrap();
        assert!(!act.actions.is_empty());
        assert!(act.actions.iter().all(|a| a.delay_ms > 0.0));
    }
}

//! RSW: world description. Names the GND/GAT, sets water and sun lighting, and places
//! models, lights, sounds and effects. Positions are in RO space (y down) relative to
//! the map center.

use anyhow::{bail, ensure, Result};

use crate::reader::Reader;

#[derive(Debug, Clone, Copy)]
pub struct Water {
    /// RO-space height (y down).
    pub level: f32,
    pub kind: i32,
    pub wave_height: f32,
    pub wave_speed: f32,
    pub wave_pitch: f32,
    /// Frames per texture step.
    pub anim_speed: i32,
}

#[derive(Debug, Clone, Copy)]
pub struct Light {
    /// Degrees.
    pub longitude: i32,
    pub latitude: i32,
    pub diffuse: [f32; 3],
    pub ambient: [f32; 3],
    /// Strength of lightmap shadows.
    pub opacity: f32,
}

impl Light {
    /// Direction the light travels, in RO space (y down): from the sun at the given
    /// longitude/latitude toward the ground.
    pub fn direction(&self) -> [f32; 3] {
        let (lon, lat) = ((self.longitude as f32).to_radians(), (self.latitude as f32).to_radians());
        [lon.cos() * lat.sin(), lat.cos(), lon.sin() * lat.sin()]
    }
}

#[derive(Debug, Clone)]
pub struct Model {
    pub name: String,
    pub anim_type: i32,
    pub anim_speed: f32,
    pub file: String,
    pub pos: [f32; 3],
    /// Degrees.
    pub rot: [f32; 3],
    pub scale: [f32; 3],
}

#[derive(Debug, Clone)]
pub struct PointLight {
    pub name: String,
    pub pos: [f32; 3],
    pub color: [f32; 3],
    pub range: f32,
}

#[derive(Debug, Clone)]
pub struct Sound {
    pub file: String,
    pub pos: [f32; 3],
    pub volume: f32,
    pub range: f32,
    pub cycle: f32,
}

#[derive(Debug, Clone)]
pub struct Effect {
    pub pos: [f32; 3],
    pub id: i32,
    pub delay: f32,
}

#[derive(Debug)]
pub struct Rsw {
    pub version: (u8, u8),
    pub gnd: String,
    pub gat: String,
    /// None from RSW 2.6, where water moved into the GND.
    pub water: Option<Water>,
    pub light: Light,
    pub models: Vec<Model>,
    pub lights: Vec<PointLight>,
    pub sounds: Vec<Sound>,
    pub effects: Vec<Effect>,
}

fn vec3(r: &mut Reader) -> Result<[f32; 3]> {
    Ok([r.f32()?, r.f32()?, r.f32()?])
}

impl Rsw {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        ensure!(r.bytes(4)? == b"GRSW", "not an RSW file");
        let version = (r.u8()?, r.u8()?);
        let v = |maj: u8, min: u8| version >= (maj, min);
        if !v(1, 2) {
            bail!("RSW {}.{} unsupported", version.0, version.1);
        }
        // 2.5+ carries a build number; 2.2..2.4 a single byte of it.
        let build = if v(2, 5) {
            r.i32()?
        } else if v(2, 2) {
            r.u8()? as i32
        } else {
            0
        };
        if v(2, 5) {
            r.i32()?; // unknown
        }

        let _ini = r.string(40)?;
        let gnd = r.string(40)?;
        let gat = if v(1, 4) { r.string(40)? } else { String::new() };
        let _src = r.string(40)?;

        let water = if v(2, 6) {
            None
        } else {
            let level = if v(1, 3) { r.f32()? } else { 0.0 };
            let (kind, wave_height, wave_speed, wave_pitch) =
                if v(1, 8) { (r.i32()?, r.f32()?, r.f32()?, r.f32()?) } else { (0, 1.0, 2.0, 50.0) };
            let anim_speed = if v(1, 9) { r.i32()? } else { 3 };
            Some(Water { level, kind, wave_height, wave_speed, wave_pitch, anim_speed })
        };

        let light = if v(1, 5) {
            let longitude = r.i32()?;
            let latitude = r.i32()?;
            let diffuse = vec3(&mut r)?;
            let ambient = vec3(&mut r)?;
            let opacity = if v(1, 7) { r.f32()? } else { 1.0 };
            Light { longitude, latitude, diffuse, ambient, opacity }
        } else {
            Light { longitude: 45, latitude: 45, diffuse: [1.0; 3], ambient: [0.3; 3], opacity: 1.0 }
        };

        if v(1, 6) {
            r.bytes(16)?; // ground bounds: top, bottom, left, right
        }

        let count = r.u32()? as usize;
        let (mut models, mut lights, mut sounds, mut effects) = (vec![], vec![], vec![], vec![]);
        for _ in 0..count {
            match r.i32()? {
                1 => {
                    let (name, anim_type, anim_speed) = if v(1, 3) {
                        let name = r.string(40)?;
                        let anim_type = r.i32()?;
                        let anim_speed = r.f32()?;
                        r.i32()?; // block type
                        (name, anim_type, anim_speed)
                    } else {
                        (String::new(), 0, 1.0)
                    };
                    if v(2, 6) && build >= 186 {
                        r.u8()?; // unknown
                    }
                    let file = r.string(80)?;
                    let _node = r.string(80)?;
                    let pos = vec3(&mut r)?;
                    let rot = vec3(&mut r)?;
                    let scale = vec3(&mut r)?;
                    models.push(Model { name, anim_type, anim_speed, file, pos, rot, scale });
                }
                2 => {
                    let name = r.string(80)?;
                    let pos = vec3(&mut r)?;
                    let color = [r.i32()? as f32, r.i32()? as f32, r.i32()? as f32];
                    let range = r.f32()?;
                    lights.push(PointLight { name, pos, color, range });
                }
                3 => {
                    let _name = r.string(80)?;
                    let file = r.string(80)?;
                    let pos = vec3(&mut r)?;
                    let volume = r.f32()?;
                    r.bytes(8)?; // width, height
                    let range = r.f32()?;
                    let cycle = if v(2, 0) { r.f32()? } else { 4.0 };
                    sounds.push(Sound { file, pos, volume, range, cycle });
                }
                4 => {
                    let _name = r.string(80)?;
                    let pos = vec3(&mut r)?;
                    let id = r.i32()?;
                    let delay = r.f32()?;
                    r.bytes(16)?; // params
                    effects.push(Effect { pos, id, delay });
                }
                t => bail!("unknown RSW object type {t}"),
            }
        }
        // A quadtree follows; it is derivable from the geometry and not needed.

        Ok(Self { version, gnd, gat, water, light, models, lights, sounds, effects })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sun_shines_downward() {
        let light = Light { longitude: 45, latitude: 45, diffuse: [1.0; 3], ambient: [0.3; 3], opacity: 0.5 };
        // RO space is y-down, so light reaching the ground travels toward +y.
        assert!(light.direction()[1] > 0.0);
        let overhead = Light { latitude: 0, ..light };
        assert!((overhead.direction()[1] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn parses_sample_if_present() {
        let Ok(path) = std::env::var("RO_RSW_SAMPLE") else { return };
        let rsw = Rsw::parse(&std::fs::read(path).unwrap()).unwrap();
        assert!(!rsw.models.is_empty());
        assert!(rsw.models.iter().all(|m| m.file.to_ascii_lowercase().ends_with(".rsm") || m.file.to_ascii_lowercase().ends_with(".rsm2")));
    }
}

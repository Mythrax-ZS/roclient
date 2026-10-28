//! RSM 1.x: hierarchical static/animated models placed by RSW.

use anyhow::{bail, ensure, Result};

use crate::reader::Reader;

#[derive(Debug, Clone, Copy)]
pub struct Face {
    pub vertices: [u16; 3],
    pub tex_coords: [u16; 3],
    /// Index into the node's `textures`.
    pub texture: u16,
    pub two_sided: bool,
    pub smooth_group: i32,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub name: String,
    pub parent: String,
    /// Indices into `Rsm::textures`.
    pub textures: Vec<i32>,
    /// Row-major 3x3 applied to vertices before `offset`.
    pub mat3: [f32; 9],
    pub offset: [f32; 3],
    pub pos: [f32; 3],
    pub rot_angle: f32,
    pub rot_axis: [f32; 3],
    pub scale: [f32; 3],
    pub vertices: Vec<[f32; 3]>,
    pub tex_coords: Vec<[f32; 2]>,
    pub faces: Vec<Face>,
    /// (frame, quaternion xyzw)
    pub rot_keys: Vec<(i32, [f32; 4])>,
}

#[derive(Debug)]
pub struct Rsm {
    pub version: (u8, u8),
    pub anim_len: i32,
    /// 0 none, 1 flat, 2 smooth.
    pub shade_type: i32,
    pub alpha: f32,
    pub textures: Vec<String>,
    pub main_node: String,
    pub nodes: Vec<Node>,
}

fn vec3(r: &mut Reader) -> Result<[f32; 3]> {
    Ok([r.f32()?, r.f32()?, r.f32()?])
}

impl Rsm {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        ensure!(r.bytes(4)? == b"GRSM", "not an RSM file");
        let version = (r.u8()?, r.u8()?);
        if version.0 != 1 {
            bail!("RSM {}.{} unsupported (RSM2 not implemented)", version.0, version.1);
        }
        let v = |min: u8| version.1 >= min;
        let anim_len = r.i32()?;
        let shade_type = r.i32()?;
        let alpha = if v(4) { r.u8()? as f32 / 255.0 } else { 1.0 };
        r.bytes(16)?; // reserved

        let tex_count = r.i32()? as usize;
        let textures = (0..tex_count).map(|_| r.string(40)).collect::<Result<_>>()?;
        let main_node = r.string(40)?;

        let node_count = r.i32()? as usize;
        let mut nodes = Vec::with_capacity(node_count);
        for _ in 0..node_count {
            let name = r.string(40)?;
            let parent = r.string(40)?;
            let n = r.i32()? as usize;
            let textures = (0..n).map(|_| r.i32()).collect::<Result<_>>()?;
            let mut mat3 = [0.0; 9];
            for m in &mut mat3 {
                *m = r.f32()?;
            }
            let offset = vec3(&mut r)?;
            let pos = vec3(&mut r)?;
            let rot_angle = r.f32()?;
            let rot_axis = vec3(&mut r)?;
            let scale = vec3(&mut r)?;

            let n = r.i32()? as usize;
            let vertices = (0..n).map(|_| vec3(&mut r)).collect::<Result<_>>()?;
            let n = r.i32()? as usize;
            let tex_coords = (0..n)
                .map(|_| {
                    if v(2) {
                        r.u32()?; // vertex color
                    }
                    Ok([r.f32()?, r.f32()?])
                })
                .collect::<Result<_>>()?;
            let n = r.i32()? as usize;
            let faces = (0..n)
                .map(|_| {
                    let vertices = [r.u16()?, r.u16()?, r.u16()?];
                    let tex_coords = [r.u16()?, r.u16()?, r.u16()?];
                    let texture = r.u16()?;
                    r.u16()?; // padding
                    let two_sided = r.i32()? != 0;
                    let smooth_group = if v(2) { r.i32()? } else { 0 };
                    Ok(Face { vertices, tex_coords, texture, two_sided, smooth_group })
                })
                .collect::<Result<_>>()?;
            // RSM 1.x has no per-node position keyframes; 1.6 adds scale keyframes
            // (frame, scale xyz, reserved) before the rotation keyframes.
            if v(6) {
                let n = r.i32()? as usize;
                r.bytes(n * 20)?;
            }
            let n = r.i32()? as usize;
            let rot_keys = (0..n)
                .map(|_| Ok((r.i32()?, [r.f32()?, r.f32()?, r.f32()?, r.f32()?])))
                .collect::<Result<_>>()?;
            nodes.push(Node {
                name, parent, textures, mat3, offset, pos, rot_angle, rot_axis, scale, vertices, tex_coords, faces, rot_keys,
            });
        }
        // Older versions append model-level position keyframes and volume boxes; unused.

        Ok(Self { version, anim_len, shade_type, alpha, textures, main_node, nodes })
    }
}

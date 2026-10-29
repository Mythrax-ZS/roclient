//! RSM models placed by RSW: 1.x and RSM2 (2.2 / 2.3, the newer maps' models).

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
    /// Row-major 3x3 applied to vertices before `offset` (1.x); the node's base transform (2.x).
    pub mat3: [f32; 9],
    /// 1.x: translation after `mat3`. 2.x: the node's position (BrowEdit's `pos_`).
    pub offset: [f32; 3],
    /// 1.x only (zero / identity in 2.x).
    pub pos: [f32; 3],
    pub rot_angle: f32,
    pub rot_axis: [f32; 3],
    pub scale: [f32; 3],
    pub vertices: Vec<[f32; 3]>,
    pub tex_coords: Vec<[f32; 2]>,
    pub faces: Vec<Face>,
    /// (frame, quaternion xyzw)
    pub rot_keys: Vec<(i32, [f32; 4])>,
    /// (frame, scale xyz)
    pub scale_keys: Vec<(i32, [f32; 3])>,
    /// (frame, position xyz); RSM2 only.
    pub pos_keys: Vec<(i32, [f32; 3])>,
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

/// RSM2's length-prefixed string.
fn dyn_string(r: &mut Reader) -> Result<String> {
    let n = r.i32()?;
    ensure!((0..=1024).contains(&n), "bad string length {n}");
    let b = r.bytes(n as usize)?;
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    let (s, _, _) = encoding_rs::EUC_KR.decode(&b[..end]);
    Ok(s.into_owned())
}

impl Rsm {
    pub fn is_rsm2(&self) -> bool {
        self.version.0 >= 2
    }

    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        ensure!(r.bytes(4)? == b"GRSM", "not an RSM file");
        let version = (r.u8()?, r.u8()?);
        let rsm2 = version.0 >= 2;
        if rsm2 && version < (2, 2) {
            bail!("RSM {}.{} unsupported", version.0, version.1);
        }
        // `v(1, 4)`: version 1.4 or newer (any 2.x qualifies).
        let v = |maj: u8, min: u8| version >= (maj, min);
        let anim_len = r.i32()?;
        let shade_type = r.i32()?;
        let alpha = if v(1, 4) { r.u8()? as f32 / 255.0 } else { 1.0 };

        let mut textures: Vec<String> = vec![];
        let main_node;
        if rsm2 {
            r.f32()?; // frames per second (keyframe times are frames)
            if !v(2, 3) {
                // 2.2: a shared texture list; 2.3 names textures per node instead.
                let n = r.i32()?;
                ensure!((0..=100).contains(&n), "bad texture count {n}");
                for _ in 0..n {
                    textures.push(dyn_string(&mut r)?);
                }
            }
            let roots = r.i32()?;
            let mut last = String::new();
            for _ in 0..roots.max(0) {
                last = dyn_string(&mut r)?;
            }
            main_node = last;
        } else {
            r.bytes(16)?; // reserved
            let n = r.i32()? as usize;
            textures = (0..n).map(|_| r.string(40)).collect::<Result<_>>()?;
            main_node = r.string(40)?;
        }

        let node_count = r.i32()? as usize;
        let mut nodes = Vec::with_capacity(node_count);
        for _ in 0..node_count {
            let (name, parent) = if rsm2 { (dyn_string(&mut r)?, dyn_string(&mut r)?) } else { (r.string(40)?, r.string(40)?) };
            let n = r.i32()? as usize;
            let node_textures = if v(2, 3) {
                // Per-node names, merged into the model's list.
                (0..n)
                    .map(|_| {
                        let t = dyn_string(&mut r)?;
                        Ok(match textures.iter().position(|x| x.eq_ignore_ascii_case(&t)) {
                            Some(i) => i as i32,
                            None => {
                                textures.push(t);
                                textures.len() as i32 - 1
                            }
                        })
                    })
                    .collect::<Result<_>>()?
            } else {
                (0..n).map(|_| r.i32()).collect::<Result<_>>()?
            };
            let mut mat3 = [0.0; 9];
            for m in &mut mat3 {
                *m = r.f32()?;
            }
            let offset = vec3(&mut r)?;
            let (pos, rot_angle, rot_axis, scale) = if rsm2 { ([0.0; 3], 0.0, [0.0; 3], [1.0; 3]) } else { (vec3(&mut r)?, r.f32()?, vec3(&mut r)?, vec3(&mut r)?) };

            let n = r.i32()? as usize;
            let vertices = (0..n).map(|_| vec3(&mut r)).collect::<Result<_>>()?;
            let n = r.i32()? as usize;
            let tex_coords = (0..n)
                .map(|_| {
                    if v(1, 2) {
                        r.u32()?; // vertex color
                    }
                    Ok([r.f32()?, r.f32()?])
                })
                .collect::<Result<_>>()?;
            let n = r.i32()? as usize;
            let faces = (0..n)
                .map(|_| {
                    // RSM2 faces are length-prefixed: extra smoothing-group words may follow.
                    let len = if rsm2 { r.i32()? } else { 0 };
                    let vertices = [r.u16()?, r.u16()?, r.u16()?];
                    let tex_coords = [r.u16()?, r.u16()?, r.u16()?];
                    let texture = r.u16()?;
                    r.u16()?; // padding
                    let two_sided = r.i32()? != 0;
                    let smooth_group = if v(1, 2) { r.i32()? } else { 0 };
                    if rsm2 && len > 24 {
                        r.bytes(len as usize - 24)?;
                    }
                    Ok(Face { vertices, tex_coords, texture, two_sided, smooth_group })
                })
                .collect::<Result<_>>()?;
            // 1.6+ (and RSM2): scale keyframes (frame, scale xyz, reserved).
            let scale_keys = if v(1, 6) {
                let n = r.i32()? as usize;
                (0..n)
                    .map(|_| {
                        let k = (r.i32()?, vec3(&mut r)?);
                        r.f32()?;
                        Ok(k)
                    })
                    .collect::<Result<_>>()?
            } else {
                vec![]
            };
            let n = r.i32()? as usize;
            let rot_keys = (0..n).map(|_| Ok((r.i32()?, [r.f32()?, r.f32()?, r.f32()?, r.f32()?]))).collect::<Result<_>>()?;
            let mut pos_keys = vec![];
            if rsm2 {
                let n = r.i32()? as usize;
                pos_keys = (0..n)
                    .map(|_| {
                        let k = (r.i32()?, vec3(&mut r)?);
                        r.f32()?;
                        Ok(k)
                    })
                    .collect::<Result<_>>()?;
                if v(2, 3) {
                    // Texture (UV) animation: per texture, per type, (frame, value) pairs.
                    let n = r.i32()?;
                    for _ in 0..n.max(0) {
                        r.i32()?; // texture
                        let types = r.i32()?;
                        for _ in 0..types.max(0) {
                            r.i32()?; // type
                            let frames = r.i32()?.max(0) as usize;
                            r.bytes(frames * 8)?;
                        }
                    }
                }
            }
            nodes.push(Node {
                name, parent, textures: node_textures, mat3, offset, pos, rot_angle, rot_axis, scale, vertices, tex_coords, faces, rot_keys, scale_keys, pos_keys,
            });
        }
        // Model-level keyframes (old versions) and volume boxes follow; unused.

        Ok(Self { version, anim_len, shade_type, alpha, textures, main_node, nodes })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(d: &mut Vec<u8>, t: &str) {
        d.extend((t.len() as i32).to_le_bytes());
        d.extend(t.as_bytes());
    }

    /// One RSM2 node with a single triangle, as 2.2 (shared textures) or 2.3 (per node).
    fn sample(minor: u8) -> Vec<u8> {
        let mut d = b"GRSM".to_vec();
        d.extend([2, minor]);
        d.extend(0i32.to_le_bytes()); // anim len
        d.extend(2i32.to_le_bytes()); // shade
        d.push(255);
        d.extend(30f32.to_le_bytes()); // fps
        if minor == 2 {
            d.extend(1i32.to_le_bytes());
            s(&mut d, "a.bmp");
        }
        d.extend(1i32.to_le_bytes());
        s(&mut d, "root");
        d.extend(1i32.to_le_bytes()); // nodes
        s(&mut d, "root");
        s(&mut d, "");
        d.extend(1i32.to_le_bytes());
        if minor == 2 {
            d.extend(0i32.to_le_bytes());
        } else {
            s(&mut d, "a.bmp");
        }
        d.extend([1f32, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 5.0, 6.0, 7.0].iter().flat_map(|f| f.to_le_bytes()));
        d.extend(3i32.to_le_bytes());
        d.extend([0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0].iter().flat_map(|f| f.to_le_bytes()));
        d.extend(1i32.to_le_bytes());
        d.extend(0u32.to_le_bytes());
        d.extend([0.5f32, 0.5].iter().flat_map(|f| f.to_le_bytes()));
        d.extend(1i32.to_le_bytes()); // faces
        d.extend(28i32.to_le_bytes()); // length: 24 + one extra smoothing word
        d.extend([0u16, 1, 2, 0, 0, 0, 0, 0].iter().flat_map(|v| v.to_le_bytes()));
        d.extend(0i32.to_le_bytes()); // two-sided
        d.extend(1i32.to_le_bytes()); // smooth group
        d.extend(0i32.to_le_bytes()); // extra
        d.extend(0i32.to_le_bytes()); // scale keys
        d.extend(0i32.to_le_bytes()); // rot keys
        d.extend(1i32.to_le_bytes()); // pos keys
        d.extend(0i32.to_le_bytes());
        d.extend([5f32, 6.0, 7.0, 0.0].iter().flat_map(|f| f.to_le_bytes()));
        if minor == 3 {
            d.extend(0i32.to_le_bytes()); // texture animations
        }
        d.extend(0i32.to_le_bytes()); // volume boxes
        d
    }

    #[test]
    fn parses_rsm2() {
        for minor in [2, 3] {
            let m = Rsm::parse(&sample(minor)).unwrap_or_else(|e| panic!("2.{minor}: {e:#}"));
            assert_eq!(m.textures, vec!["a.bmp".to_string()]);
            assert_eq!(m.main_node, "root");
            let n = &m.nodes[0];
            assert_eq!(n.textures, vec![0]);
            assert_eq!(n.offset, [5.0, 6.0, 7.0]);
            assert_eq!(n.vertices.len(), 3);
            assert_eq!(n.faces[0].smooth_group, 1);
            assert_eq!(n.pos_keys.len(), 1);
        }
    }
}

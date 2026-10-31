//! Granny 2 models (.gr2): the official client's few 3D monsters (the castle guardians, the
//! Emperium, guild flags and treasure boxes) and their animations (data\model\3dmob_bone).
//!
//! A file is a header, a table of sections (each optionally Oodle0-compressed), and pointer
//! fixups between them. Its contents are self-describing: a type tree (member name, kind,
//! child type) laid over the root object, walked here by member name.
//!
//! The reading of the type tree, meshes, skeletons, textures and curves follows noclip.website's
//! src/RagnarokOnline/granny.ts (MIT, Copyright (c) 2017-present noclip.website contributors).

use std::collections::HashMap;

use anyhow::{bail, ensure, Context, Result};

const MAGIC: [u8; 16] = [0xb8, 0x67, 0xb0, 0xca, 0xf8, 0x6d, 0xb1, 0x0f, 0x84, 0x72, 0x8c, 0x7e, 0x5e, 0x19, 0x00, 0x1e];

// Member kinds.
const T_NONE: u32 = 0;
const T_INLINE: u32 = 1;
const T_REFERENCE: u32 = 2;
const T_REFERENCE_TO_ARRAY: u32 = 3;
const T_ARRAY_OF_REFERENCES: u32 = 4;
const T_VARIANT_REFERENCE: u32 = 5;
const T_REFERENCE_TO_VARIANT_ARRAY: u32 = 7;
const T_STRING: u32 = 8;
const T_TRANSFORM: u32 = 9;
const T_REAL32: u32 = 10;
const T_UINT8: u32 = 12;
const T_NORMAL_UINT8: u32 = 14;
const T_EMPTY_REFERENCE: u32 = 22;

const TYPE_NODE: u32 = 32;

fn scalar_size(t: u32) -> u32 {
    match t {
        10 | 19 | 20 => 4,
        15..=18 | 21 => 2,
        11..=14 => 1,
        T_TRANSFORM => 68,
        _ => 0,
    }
}

#[derive(Debug, Clone, Default)]
pub struct Mesh {
    pub name: String,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub bone_weights: Vec<[f32; 4]>,
    /// Into `bone_names` (the mesh's own bone bindings).
    pub bone_indices: Vec<[u8; 4]>,
    pub indices: Vec<u32>,
    pub bone_names: Vec<String>,
    /// Into `Gr2::textures`.
    pub texture: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct Bone {
    pub name: String,
    pub parent: i32,
    pub translation: [f32; 3],
    /// Quaternion xyzw.
    pub rotation: [f32; 4],
    /// Row-major 3x3.
    pub scale_shear: [f32; 9],
    /// Column-major 4x4.
    pub inverse_world: [f32; 16],
}

#[derive(Debug, Clone, Default)]
pub struct Skeleton {
    pub name: String,
    pub bones: Vec<Bone>,
}

/// A keyed curve: `dimension` floats per knot, sampled linearly between knots.
#[derive(Debug, Clone)]
pub struct Curve {
    pub degree: i32,
    pub dimension: usize,
    pub knots: Vec<f32>,
    pub controls: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct Track {
    pub bone: String,
    pub position: Option<Curve>,
    pub orientation: Option<Curve>,
    pub scale_shear: Option<Curve>,
}

#[derive(Debug, Clone)]
pub struct Animation {
    pub name: String,
    pub duration: f32,
    pub tracks: Vec<Track>,
}

#[derive(Debug, Clone)]
pub struct Texture {
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// None when the image is stored in an encoding not decoded here.
    pub rgba: Option<Vec<u8>>,
    pub encoding: i32,
}

/// A Granny transform: translation, rotation, then scale/shear.
#[derive(Debug, Clone, Copy)]
pub struct Transform {
    pub translation: [f32; 3],
    /// Quaternion xyzw.
    pub rotation: [f32; 4],
    /// Row-major 3x3.
    pub scale_shear: [f32; 9],
}

#[derive(Debug, Clone, Default)]
pub struct Gr2 {
    /// The first model's initial placement (where the artist left it in the scene).
    pub placement: Option<Transform>,
    pub meshes: Vec<Mesh>,
    pub skeletons: Vec<Skeleton>,
    pub animations: Vec<Animation>,
    pub textures: Vec<Texture>,
}

struct File {
    blob: Vec<u8>,
    fixups: HashMap<u32, u32>,
    type_abs: u32,
    root_abs: u32,
}

#[derive(Debug, Clone, Default)]
struct Member {
    kind: u32,
    child: Option<u32>,
    reference: Option<u32>,
    variant: Option<u32>,
    count: u32,
    array: Option<u32>,
    string: Option<String>,
    at: Option<u32>,
}

impl Gr2 {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let f = File::parse(data)?;
        Ok(f.model())
    }
}

impl File {
    fn parse(data: &[u8]) -> Result<Self> {
        ensure!(data.len() >= 0x60, "gr2: file too small");
        ensure!(data[..16] == MAGIC, "gr2: bad magic");
        let u = |o: usize| -> Result<u32> { Ok(u32::from_le_bytes(data.get(o..o + 4).context("gr2: truncated")?.try_into().unwrap())) };
        ensure!(u(0x14)? == 0, "gr2: unsupported header format");
        let version = u(0x20)?;
        ensure!(version == 6 || version == 7, "gr2: unsupported version {version}");
        let table = 0x20 + u(0x2c)? as usize;
        let count = u(0x30)? as usize;
        let (type_section, type_offset, root_section, root_offset) = (u(0x34)?, u(0x38)?, u(0x3c)?, u(0x40)?);
        struct Section {
            compression: u32,
            offset: u32,
            compressed: u32,
            expanded: u32,
            stop0: u32,
            stop1: u32,
            fixup_offset: u32,
            fixup_count: u32,
        }
        let mut sections = Vec::with_capacity(count);
        for i in 0..count {
            let o = table + i * 44;
            sections.push(Section {
                compression: u(o)?,
                offset: u(o + 4)?,
                compressed: u(o + 8)?,
                expanded: u(o + 12)?,
                stop0: u(o + 20)?,
                stop1: u(o + 24)?,
                fixup_offset: u(o + 28)?,
                fixup_count: u(o + 32)?,
            });
        }
        let mut blob = Vec::with_capacity(sections.iter().map(|s| s.expanded as usize).sum());
        let mut base = Vec::with_capacity(count);
        for (i, s) in sections.iter().enumerate() {
            base.push(blob.len() as u32);
            if s.expanded == 0 {
                continue;
            }
            let start = s.offset as usize;
            match s.compression {
                0 => blob.extend_from_slice(data.get(start..start + s.expanded as usize).context("gr2: section out of range")?),
                1 => {
                    let src = data.get(start..start + s.compressed as usize).context("gr2: section out of range")?;
                    blob.extend(crate::oodle0::decompress(src, s.expanded as usize, s.stop0, s.stop1).with_context(|| format!("gr2: section {i}"))?);
                }
                c => bail!("gr2: section {i} uses compression {c}, not supported"),
            }
        }
        let mut fixups = HashMap::new();
        for (i, s) in sections.iter().enumerate() {
            for k in 0..s.fixup_count as usize {
                let o = s.fixup_offset as usize + k * 12;
                let (src, dst_section, dst) = (u(o)?, u(o + 4)? as usize, u(o + 8)?);
                let dst_base = *base.get(dst_section).context("gr2: fixup to a missing section")?;
                fixups.insert(base[i] + src, dst_base + dst);
            }
        }
        let type_abs = base.get(type_section as usize).context("gr2: bad type section")? + type_offset;
        let root_abs = base.get(root_section as usize).context("gr2: bad root section")? + root_offset;
        Ok(Self { blob, fixups, type_abs, root_abs })
    }

    fn u32(&self, o: u32) -> u32 {
        self.blob.get(o as usize..o as usize + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()))
    }
    fn i32(&self, o: u32) -> i32 {
        self.u32(o) as i32
    }
    fn f32(&self, o: u32) -> f32 {
        f32::from_bits(self.u32(o))
    }
    fn u16(&self, o: u32) -> u16 {
        self.blob.get(o as usize..o as usize + 2).map_or(0, |b| u16::from_le_bytes(b.try_into().unwrap()))
    }
    fn u8(&self, o: u32) -> u8 {
        self.blob.get(o as usize).copied().unwrap_or(0)
    }
    fn ptr(&self, o: u32) -> Option<u32> {
        self.fixups.get(&o).copied()
    }
    fn string(&self, o: u32) -> String {
        let s = &self.blob[(o as usize).min(self.blob.len())..];
        let end = s.iter().position(|&b| b == 0).unwrap_or(s.len());
        String::from_utf8_lossy(&s[..end]).into_owned()
    }

    /// The type nodes from `t` to the list's end: (kind, name, child type, array size, node).
    fn nodes(&self, mut t: u32) -> Vec<(u32, Option<String>, Option<u32>, i32)> {
        let mut out = Vec::new();
        while (t + TYPE_NODE) as usize <= self.blob.len() {
            let kind = self.u32(t);
            if kind == T_NONE || kind > T_EMPTY_REFERENCE {
                break;
            }
            out.push((kind, self.ptr(t + 4).map(|p| self.string(p)), self.ptr(t + 8), self.i32(t + 12)));
            t += TYPE_NODE;
        }
        out
    }

    fn member_size(&self, kind: u32, child: Option<u32>, array_size: i32, depth: u32) -> u32 {
        let unit = match kind {
            T_INLINE => child.filter(|_| depth < 16).map_or(0, |c| self.inline_size(c, depth + 1)),
            T_REFERENCE | T_EMPTY_REFERENCE | T_STRING => 4,
            T_REFERENCE_TO_ARRAY | T_ARRAY_OF_REFERENCES | T_VARIANT_REFERENCE => 8,
            T_REFERENCE_TO_VARIANT_ARRAY => 12,
            k => scalar_size(k),
        };
        unit * array_size.max(1) as u32
    }

    fn inline_size(&self, t: u32, depth: u32) -> u32 {
        self.nodes(t).iter().map(|(k, _, c, n)| self.member_size(*k, *c, *n, depth)).sum()
    }

    /// The members of the object at `d`, laid out by the type at `t`, by name.
    fn walk(&self, t: u32, mut d: u32) -> HashMap<String, Member> {
        let mut out = HashMap::new();
        for (kind, name, child, array_size) in self.nodes(t) {
            let mut m = Member { kind, child, ..Default::default() };
            match kind {
                T_REFERENCE | T_EMPTY_REFERENCE => {
                    m.reference = self.ptr(d);
                    d += 4;
                }
                T_STRING => {
                    m.string = self.ptr(d).map(|p| self.string(p));
                    d += 4;
                }
                T_REFERENCE_TO_ARRAY | T_ARRAY_OF_REFERENCES => {
                    m.count = self.u32(d);
                    m.array = self.ptr(d + 4);
                    d += 8;
                }
                T_VARIANT_REFERENCE => {
                    m.variant = self.ptr(d);
                    m.reference = self.ptr(d + 4);
                    d += 8;
                }
                T_REFERENCE_TO_VARIANT_ARRAY => {
                    m.variant = self.ptr(d);
                    m.count = self.u32(d + 4);
                    m.array = self.ptr(d + 8);
                    d += 12;
                }
                _ => {
                    m.at = Some(d);
                    d += self.member_size(kind, child, array_size, 0);
                }
            }
            if let Some(n) = name {
                out.insert(n, m);
            }
        }
        out
    }

    fn references(&self, m: &Member) -> Vec<u32> {
        let Some(a) = m.array else { return Vec::new() };
        (0..m.count).filter_map(|i| self.ptr(a + i * 4)).collect()
    }

    /// The objects of an array member (array of references, or inline array of structs).
    fn elements(&self, m: &Member) -> Vec<u32> {
        match m.kind {
            T_ARRAY_OF_REFERENCES => self.references(m),
            _ => {
                let (Some(a), Some(c)) = (m.array, m.child) else { return Vec::new() };
                let stride = self.inline_size(c, 0);
                (0..m.count).map(|i| a + i * stride).collect()
            }
        }
    }

    fn model(&self) -> Gr2 {
        let top = self.walk(self.type_abs, self.root_abs);
        let mut out = Gr2::default();
        let mut tex_index = HashMap::new();
        if let Some(m) = top.get("Textures").filter(|m| m.child.is_some()) {
            for p in self.references(m) {
                tex_index.insert(p, out.textures.len());
                out.textures.push(self.texture(m.child.unwrap(), p));
            }
        }
        if let Some(m) = top.get("Meshes").filter(|m| m.child.is_some()) {
            for p in self.references(m) {
                if let Some(mut mesh) = self.mesh(m.child.unwrap(), p).filter(|me| !me.positions.is_empty() && !me.indices.is_empty()) {
                    mesh.texture = self.mesh_texture(m.child.unwrap(), p, &tex_index);
                    out.meshes.push(mesh);
                }
            }
        }
        if let Some(m) = top.get("Skeletons").filter(|m| m.child.is_some()) {
            out.skeletons = self.references(m).into_iter().map(|p| self.skeleton(m.child.unwrap(), p)).collect();
        }
        if let Some(m) = top.get("Models").filter(|m| m.child.is_some()) {
            if let Some(p) = self.references(m).first() {
                let mw = self.walk(m.child.unwrap(), *p);
                out.placement = mw.get("InitialPlacement").and_then(|m| m.at).map(|o| self.transform(o));
            }
        }
        if let Some(m) = top.get("Animations").filter(|m| m.child.is_some()) {
            out.animations = self.references(m).into_iter().map(|p| self.animation(m.child.unwrap(), p)).collect();
        }
        out
    }

    fn mesh(&self, t: u32, d: u32) -> Option<Mesh> {
        let mw = self.walk(t, d);
        let name = mw.get("Name").and_then(|m| m.string.clone()).unwrap_or_default();
        let pvd = mw.get("PrimaryVertexData")?;
        let topo = mw.get("PrimaryTopology")?;
        let vw = self.walk(pvd.child?, pvd.reference?);
        let verts = vw.get("Vertices")?;
        let (arr, layout) = (verts.array?, verts.variant?);
        // The vertex layout: fields and their offsets.
        let mut fields = HashMap::new();
        let mut stride = 0;
        for (kind, fname, _, n) in self.nodes(layout) {
            let count = n.max(1) as u32;
            if let Some(fname) = fname {
                fields.insert(fname, (kind, count, stride));
            }
            stride += scalar_size(kind) * count;
        }
        let floats = |base: u32, f: Option<&(u32, u32, u32)>, out: &mut [f32]| {
            if let Some(&(kind, count, off)) = f {
                for (i, o) in out.iter_mut().enumerate() {
                    *o = if (i as u32) < count && kind == T_REAL32 { self.f32(base + off + i as u32 * 4) } else { 0.0 };
                }
            }
        };
        let fpos = fields.get("Position")?;
        let fnrm = fields.get("Normal");
        let fuv = fields.get("TextureCoordinates0").or_else(|| fields.get("TextureCoordinates"));
        let fbw = fields.get("BoneWeights");
        let fbi = fields.get("BoneIndices");
        let mut mesh = Mesh { name, ..Default::default() };
        for v in 0..verts.count {
            let base = arr + v * stride;
            let (mut p, mut n, mut uv) = ([0.0; 3], [0.0; 3], [0.0; 2]);
            floats(base, Some(fpos), &mut p);
            floats(base, fnrm, &mut n);
            floats(base, fuv, &mut uv);
            let mut w = [0.0f32; 4];
            if let Some(&(kind, count, off)) = fbw {
                for (i, x) in w.iter_mut().enumerate().take(count as usize) {
                    *x = match kind {
                        T_NORMAL_UINT8 | T_UINT8 => self.u8(base + off + i as u32) as f32 / 255.0,
                        T_REAL32 => self.f32(base + off + i as u32 * 4),
                        _ => 0.0,
                    };
                }
            }
            let mut bi = [0u8; 4];
            if let Some(&(_, count, off)) = fbi {
                for (i, x) in bi.iter_mut().enumerate().take(count as usize) {
                    *x = self.u8(base + off + i as u32);
                }
            }
            mesh.positions.push(p);
            mesh.normals.push(n);
            mesh.uvs.push(uv);
            mesh.bone_weights.push(w);
            mesh.bone_indices.push(bi);
        }
        let tw = self.walk(topo.child?, topo.reference?);
        if let Some(m) = tw.get("Indices").filter(|m| m.array.is_some() && m.count > 0) {
            mesh.indices = (0..m.count).map(|i| self.u32(m.array.unwrap() + i * 4)).collect();
        } else if let Some(m) = tw.get("Indices16").filter(|m| m.array.is_some()) {
            mesh.indices = (0..m.count).map(|i| self.u16(m.array.unwrap() + i * 2) as u32).collect();
        }
        if let Some(bb) = mw.get("BoneBindings").filter(|m| m.child.is_some()) {
            mesh.bone_names = self.elements(bb).into_iter().map(|e| self.walk(bb.child.unwrap(), e).get("BoneName").and_then(|m| m.string.clone()).unwrap_or_default()).collect();
        }
        Some(mesh)
    }

    fn material_texture(&self, t: u32, d: u32, depth: u32) -> Option<u32> {
        if depth > 8 {
            return None;
        }
        let mw = self.walk(t, d);
        if let Some(r) = mw.get("Texture").and_then(|m| m.reference) {
            return Some(r);
        }
        let maps = mw.get("Maps").filter(|m| m.child.is_some())?;
        self.elements(maps).into_iter().find_map(|e| {
            let sub = self.walk(maps.child.unwrap(), e);
            let m = sub.get("Map")?;
            self.material_texture(m.child?, m.reference?, depth + 1)
        })
    }

    fn mesh_texture(&self, t: u32, d: u32, index: &HashMap<u32, usize>) -> Option<usize> {
        let mw = self.walk(t, d);
        let mb = mw.get("MaterialBindings").filter(|m| m.child.is_some())?;
        self.elements(mb).into_iter().find_map(|e| {
            let bw = self.walk(mb.child.unwrap(), e);
            let mat = bw.get("Material")?;
            index.get(&self.material_texture(mat.child?, mat.reference?, 0)?).copied()
        })
    }

    fn texture(&self, t: u32, d: u32) -> Texture {
        let tw = self.walk(t, d);
        let int = |w: &HashMap<String, Member>, n: &str| w.get(n).and_then(|m| m.at).map(|a| self.i32(a));
        let name = tw.get("FromFileName").and_then(|m| m.string.clone()).unwrap_or_default();
        let width = int(&tw, "Width").unwrap_or(0).max(0) as u32;
        let height = int(&tw, "Height").unwrap_or(0).max(0) as u32;
        let encoding = int(&tw, "Encoding").unwrap_or(-1);
        let rgba = match encoding {
            3 => self.mip0(&tw).and_then(|px| {
                // Alpha unless the layout is 3 bytes a pixel.
                let alpha = tw.get("Layout").and_then(|l| Some(self.walk(l.child?, l.at?))).and_then(|lw| int(&lw, "BytesPerPixel")) != Some(3);
                crate::igc::decode(px, width, height, alpha).inspect_err(|e| eprintln!("gr2 texture {name}: {e:#}")).ok()
            }),
            _ => self.texture_pixels(&tw, width, height),
        };
        Texture { name, width, height, rgba, encoding }
    }

    /// The bytes of the first image's first MIP level.
    fn mip0(&self, tw: &HashMap<String, Member>) -> Option<&[u8]> {
        let images = tw.get("Images").filter(|m| m.count > 0)?;
        let iw = self.walk(images.child?, *self.elements(images).first()?);
        let mips = iw.get("MIPLevels").filter(|m| m.count > 0)?;
        let mw = self.walk(mips.child?, *self.elements(mips).first()?);
        let pixels = mw.get("Pixels").or_else(|| mw.get("PixelBytes")).filter(|m| m.count > 0)?;
        let a = pixels.array? as usize;
        self.blob.get(a..a + pixels.count as usize)
    }

    fn texture_pixels(&self, tw: &HashMap<String, Member>, width: u32, height: u32) -> Option<Vec<u8>> {
        if width == 0 || height == 0 {
            return None;
        }
        let int = |w: &HashMap<String, Member>, n: &str| w.get(n).and_then(|m| m.at).map(|a| self.i32(a));
        let (mut bpp, mut shifts, mut bits) = (4u32, [0i32, 8, 16, 24], [8i32; 4]);
        if let Some(l) = tw.get("Layout") {
            if let (Some(c), Some(a)) = (l.child, l.at) {
                let lw = self.walk(c, a);
                bpp = int(&lw, "BytesPerPixel").unwrap_or(4) as u32;
                if let Some(a) = lw.get("ShiftForComponent").and_then(|m| m.at) {
                    shifts = std::array::from_fn(|i| self.i32(a + i as u32 * 4));
                }
                if let Some(a) = lw.get("BitsForComponent").and_then(|m| m.at) {
                    bits = std::array::from_fn(|i| self.i32(a + i as u32 * 4));
                }
            }
        }
        let images = tw.get("Images").filter(|m| m.count > 0)?;
        let iw = self.walk(images.child?, *self.elements(images).first()?);
        let mips = iw.get("MIPLevels").filter(|m| m.count > 0)?;
        let mw = self.walk(mips.child?, *self.elements(mips).first()?);
        let pixels = mw.get("Pixels").or_else(|| mw.get("PixelBytes")).filter(|m| m.count > 0)?;
        let src = pixels.array?;
        let row = int(&mw, "Stride").filter(|&s| s > 0).map_or(width * bpp, |s| s as u32);
        if pixels.count < width * height * bpp {
            return None;
        }
        let mut rgba = vec![0u8; (width * height * 4) as usize];
        for y in 0..height {
            for x in 0..width {
                let s = src + y * row + x * bpp;
                let mut px = 0u32;
                for b in 0..bpp.min(4) {
                    px |= (self.u8(s + b) as u32) << (b * 8);
                }
                let d = ((y * width + x) * 4) as usize;
                for c in 0..4 {
                    let nb = bits[c];
                    rgba[d + c] = if nb <= 0 {
                        if c == 3 {
                            255
                        } else {
                            0
                        }
                    } else {
                        let v = (px >> shifts[c]) & ((1u32 << nb) - 1);
                        if nb >= 8 {
                            (v >> (nb - 8)) as u8
                        } else {
                            (v << (8 - nb)) as u8
                        }
                    };
                }
            }
        }
        Some(rgba)
    }

    fn skeleton(&self, t: u32, d: u32) -> Skeleton {
        let sw = self.walk(t, d);
        let name = sw.get("Name").and_then(|m| m.string.clone()).unwrap_or_default();
        let mut bones = Vec::new();
        if let Some(bm) = sw.get("Bones").filter(|m| m.child.is_some()) {
            for e in self.elements(bm) {
                let bw = self.walk(bm.child.unwrap(), e);
                let mut bone = Bone {
                    name: bw.get("Name").and_then(|m| m.string.clone()).unwrap_or_default(),
                    parent: bw.get("ParentIndex").and_then(|m| m.at).map_or(-1, |a| self.i32(a)),
                    translation: [0.0; 3],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    scale_shear: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
                    inverse_world: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
                };
                if let Some(o) = bw.get("Transform").and_then(|m| m.at) {
                    let t = self.transform(o);
                    (bone.translation, bone.rotation, bone.scale_shear) = (t.translation, t.rotation, t.scale_shear);
                }
                if let Some(o) = bw.get("InverseWorldTransform").and_then(|m| m.at) {
                    bone.inverse_world = std::array::from_fn(|i| self.f32(o + i as u32 * 4));
                }
                bones.push(bone);
            }
        }
        Skeleton { name, bones }
    }

    /// A 68-byte transform: flags, translation, rotation, scale/shear.
    fn transform(&self, o: u32) -> Transform {
        Transform {
            translation: std::array::from_fn(|i| self.f32(o + 4 + i as u32 * 4)),
            rotation: std::array::from_fn(|i| self.f32(o + 16 + i as u32 * 4)),
            scale_shear: std::array::from_fn(|i| self.f32(o + 32 + i as u32 * 4)),
        }
    }

    fn curve(&self, t: u32, d: u32, dimension: usize) -> Option<Curve> {
        let cw = self.walk(t, d);
        let degree = cw.get("Degree").and_then(|m| m.at).map_or(0, |a| self.i32(a));
        let knots = cw.get("Knots").filter(|m| m.count > 0 && m.array.is_some())?;
        let controls = cw.get("Controls").filter(|m| m.count > 0 && m.array.is_some())?;
        Some(Curve {
            degree,
            dimension,
            knots: (0..knots.count).map(|i| self.f32(knots.array.unwrap() + i * 4)).collect(),
            controls: (0..controls.count).map(|i| self.f32(controls.array.unwrap() + i * 4)).collect(),
        })
    }

    fn animation(&self, t: u32, d: u32) -> Animation {
        let aw = self.walk(t, d);
        let name = aw.get("Name").and_then(|m| m.string.clone()).unwrap_or_default();
        let duration = aw.get("Duration").and_then(|m| m.at).map_or(0.0, |a| self.f32(a));
        let mut tracks = Vec::new();
        if let Some(tg) = aw.get("TrackGroups").filter(|m| m.child.is_some()) {
            for p in self.references(tg) {
                let gw = self.walk(tg.child.unwrap(), p);
                let Some(tt) = gw.get("TransformTracks").filter(|m| m.child.is_some()) else { continue };
                for e in self.elements(tt) {
                    let tw = self.walk(tt.child.unwrap(), e);
                    let curve = |n: &str, dim: usize| tw.get(n).and_then(|m| self.curve(m.child?, m.at?, dim));
                    tracks.push(Track {
                        bone: tw.get("Name").and_then(|m| m.string.clone()).unwrap_or_default(),
                        position: curve("PositionCurve", 3),
                        orientation: curve("OrientationCurve", 4),
                        scale_shear: curve("ScaleShearCurve", 9),
                    });
                }
            }
        }
        Animation { name, duration, tracks }
    }
}

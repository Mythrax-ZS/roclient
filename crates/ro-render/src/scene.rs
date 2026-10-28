//! Static world geometry: GND ground plus RSM models placed by the RSW, merged into one
//! vertex buffer sorted by texture. Built in RO space (y down) and flipped to y-up on output.

use std::collections::HashMap;

use glam::{Mat3, Mat4, Quat, Vec3};
use ro_formats::{gnd::Gnd, image::Rgba, rsm::Rsm, rsw};
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub lightmap_uv: [f32; 2],
    pub color: [f32; 3],
}

impl Vertex {
    const ATTRS: [wgpu::VertexAttribute; 5] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32x2, 4 => Float32x3];

    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}

/// RO space is y-down; the renderer is y-up.
fn flip(v: Vec3) -> [f32; 3] {
    [v.x, -v.y, v.z]
}

/// Lightmap texels per slot edge (GND lightmaps are 8x8).
const LM: u32 = 8;
/// The useful interior of a lightmap: its outer ring duplicates the neighbouring tiles.
const LM_IN: u32 = LM - 2;

/// Packs GND lightmaps into one atlas.
///
/// - Slots (top part): every lightmap in its own 8x8 cell, for walls. Slot 0 is reserved:
///   full brightness, no colour, used by models and unlit surfaces.
/// - Ground grid (below the slots): each tile's 6x6 interior placed at its map position,
///   so filtering blends shadows smoothly across tile edges instead of showing a grid of
///   soft squares.
struct LightmapAtlas {
    per_row: u32,
    /// Atlas size in texels, and the row where the ground grid starts.
    width: u32,
    height: u32,
    grid_y: u32,
    image: Rgba,
}

impl LightmapAtlas {
    fn build(gnd: &Gnd) -> Self {
        let slots = gnd.lightmaps.len() as u32 + 1;
        let per_row = (slots as f32).sqrt().ceil() as u32;
        let slot_rows = slots.div_ceil(per_row);
        let has_maps = gnd.lightmaps.width == LM && gnd.lightmaps.height == LM;
        let grid_w = if has_maps { gnd.width * LM_IN } else { 0 };
        let grid_h = if has_maps { gnd.height * LM_IN } else { 0 };
        let width = (per_row * LM).max(grid_w);
        let grid_y = slot_rows * LM;
        let height = grid_y + grid_h;
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        // Unlit by default: full shadow value (no darkening), no colour.
        for p in pixels.chunks_exact_mut(4) {
            p[3] = 255;
        }
        let mut texel = |x: u32, y: u32, shadow: u8, rgb: &[u8]| {
            let p = ((y * width + x) * 4) as usize;
            pixels[p..p + 3].copy_from_slice(rgb);
            pixels[p + 3] = shadow;
        };
        if has_maps {
            for i in 0..gnd.lightmaps.len() {
                let (shadow, color) = gnd.lightmaps.get(i).unwrap();
                let slot = i as u32 + 1;
                let (sx, sy) = ((slot % per_row) * LM, (slot / per_row) * LM);
                for y in 0..LM {
                    for x in 0..LM {
                        let t = (y * LM + x) as usize;
                        texel(sx + x, sy + y, shadow[t], &color[t * 3..t * 3 + 3]);
                    }
                }
            }
            for ty in 0..gnd.height {
                for tx in 0..gnd.width {
                    let Some(s) = gnd.cube(tx, ty).and_then(|c| gnd.surface(c.top)) else { continue };
                    let Some((shadow, color)) = gnd.lightmaps.get(s.lightmap as usize) else { continue };
                    for y in 0..LM_IN {
                        for x in 0..LM_IN {
                            let t = ((y + 1) * LM + x + 1) as usize;
                            texel(tx * LM_IN + x, grid_y + ty * LM_IN + y, shadow[t], &color[t * 3..t * 3 + 3]);
                        }
                    }
                }
            }
        }
        // Baked shadows are only ~6 texels per tile, so building shadows come out as hard
        // steps. A small blur over the ground grid gives soft, painterly edges.
        if grid_h > 0 {
            blur_region(&mut pixels, width, grid_y, grid_w, grid_h, 2);
        }
        Self { per_row, width, height, grid_y, image: Rgba { width, height, pixels } }
    }

    /// Corner UVs (same order as GND surfaces) for a lightmap slot, inset by one texel so
    /// linear filtering never reads a neighbouring slot.
    fn uvs(&self, lightmap: Option<u16>) -> [[f32; 2]; 4] {
        let slot = lightmap.map_or(0, |l| l as u32 + 1);
        let (w, h) = (self.width as f32, self.height as f32);
        let (sx, sy) = ((slot % self.per_row * LM) as f32, (slot / self.per_row * LM) as f32);
        let (u0, u1) = ((sx + 1.0) / w, (sx + LM as f32 - 1.0) / w);
        let (v0, v1) = ((sy + 1.0) / h, (sy + LM as f32 - 1.0) / h);
        [[u0, v0], [u1, v0], [u0, v1], [u1, v1]]
    }

    /// Corner UVs of tile (x, y) in the continuous ground grid.
    fn ground_uvs(&self, x: u32, y: u32) -> [[f32; 2]; 4] {
        let (w, h) = (self.width as f32, self.height as f32);
        let (u0, u1) = ((x * LM_IN) as f32 / w, ((x + 1) * LM_IN) as f32 / w);
        let (v0, v1) = ((self.grid_y + y * LM_IN) as f32 / h, (self.grid_y + (y + 1) * LM_IN) as f32 / h);
        [[u0, v0], [u1, v0], [u0, v1], [u1, v1]]
    }

    fn white(&self) -> [f32; 2] {
        [(LM as f32 / 2.0) / self.width as f32, (LM as f32 / 2.0) / self.height as f32]
    }
}

/// Separable box blur (radius `r`) over all four channels of a `w` x `h` block starting
/// at row `y0` of an RGBA image `stride` texels wide.
fn blur_region(pixels: &mut [u8], stride: u32, y0: u32, w: u32, h: u32, r: i32) {
    let idx = |x: u32, y: u32| ((y0 + y) * stride + x) as usize * 4;
    let mut tmp = vec![0u8; (w * h * 4) as usize];
    let n = (2 * r + 1) as u32;
    // Horizontal into tmp, then vertical back into pixels (edges clamp).
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0u32; 4];
            for d in -r..=r {
                let sx = (x as i32 + d).clamp(0, w as i32 - 1) as u32;
                let p = idx(sx, y);
                for c in 0..4 {
                    sum[c] += pixels[p + c] as u32;
                }
            }
            let t = ((y * w + x) * 4) as usize;
            for c in 0..4 {
                tmp[t + c] = (sum[c] / n) as u8;
            }
        }
    }
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0u32; 4];
            for d in -r..=r {
                let sy = (y as i32 + d).clamp(0, h as i32 - 1) as u32;
                let t = ((sy * w + x) * 4) as usize;
                for c in 0..4 {
                    sum[c] += tmp[t + c] as u32;
                }
            }
            let p = idx(x, y);
            for c in 0..4 {
                pixels[p + c] = (sum[c] / n) as u8;
            }
        }
    }
}

/// CPU-side world mesh: `vertices` grouped so `ranges[i]` uses `textures[i]`.
pub struct SceneMesh {
    pub vertices: Vec<Vertex>,
    pub ranges: Vec<std::ops::Range<u32>>,
    /// Texture paths relative to `data\texture\`.
    pub textures: Vec<String>,
    lightmap: Rgba,
    width: u32,
    height: u32,
    zoom: f32,
    /// y-up heights per cube corner.
    heights: Vec<[f32; 4]>,
}

pub struct SceneBuilder<'a> {
    gnd: &'a Gnd,
    atlas: LightmapAtlas,
    textures: Vec<String>,
    index: HashMap<String, usize>,
    per_tex: Vec<Vec<Vertex>>,
}

impl<'a> SceneBuilder<'a> {
    pub fn new(gnd: &'a Gnd) -> Self {
        let mut b = Self { gnd, atlas: LightmapAtlas::build(gnd), textures: vec![], index: HashMap::new(), per_tex: vec![] };
        b.add_ground();
        b
    }

    fn texture(&mut self, name: &str) -> usize {
        let key = name.to_ascii_lowercase();
        if let Some(&i) = self.index.get(&key) {
            return i;
        }
        self.textures.push(name.to_string());
        self.per_tex.push(vec![]);
        self.index.insert(key, self.textures.len() - 1);
        self.textures.len() - 1
    }

    fn add_ground(&mut self) {
        let gnd = self.gnd;
        let z = gnd.zoom;
        let tex: Vec<usize> = gnd.textures.iter().map(|t| self.texture(t)).collect();

        // Smooth normals: average the face normals of the tiles touching each corner.
        let (w, h) = (gnd.width as i64, gnd.height as i64);
        let corner = |c: &ro_formats::gnd::Cube, x: i64, y: i64| {
            [
                Vec3::new(x as f32 * z, -c.heights[0], y as f32 * z),
                Vec3::new((x + 1) as f32 * z, -c.heights[1], y as f32 * z),
                Vec3::new(x as f32 * z, -c.heights[2], (y + 1) as f32 * z),
                Vec3::new((x + 1) as f32 * z, -c.heights[3], (y + 1) as f32 * z),
            ]
        };
        let tile_normal: Vec<Vec3> = gnd
            .cubes
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let p = corner(c, i as i64 % w, i as i64 / w);
                let n = (p[2] - p[0]).cross(p[1] - p[0]) + (p[1] - p[3]).cross(p[2] - p[3]);
                n.normalize_or(Vec3::Y)
            })
            .collect();
        let tn = |x: i64, y: i64| (x >= 0 && y >= 0 && x < w && y < h).then(|| tile_normal[(y * w + x) as usize]);
        let smooth = |x: i64, y: i64, k: usize| {
            // Corner k of tile (x,y) touches the tiles around grid point (x + k%2, y + k/2).
            let (gx, gy) = (x + (k % 2) as i64, y + (k / 2) as i64);
            let sum: Vec3 = [tn(gx - 1, gy - 1), tn(gx, gy - 1), tn(gx - 1, gy), tn(gx, gy)].into_iter().flatten().sum();
            sum.normalize_or(Vec3::Y)
        };

        // Tile tint colours blend across tiles at shared corners, like the original
        // client's filtered colour map, instead of changing in hard steps.
        let tile_color = |x: i64, y: i64| -> Option<Vec3> {
            if x < 0 || y < 0 || x >= w || y >= h {
                return None;
            }
            let s = gnd.cube(x as u32, y as u32).and_then(|c| gnd.surface(c.top))?;
            Some(Vec3::new(s.color[2] as f32, s.color[1] as f32, s.color[0] as f32) / 255.0)
        };
        let smooth_color = |x: i64, y: i64, k: usize| -> [f32; 3] {
            let (gx, gy) = (x + (k % 2) as i64, y + (k / 2) as i64);
            let near: Vec<Vec3> = [tile_color(gx - 1, gy - 1), tile_color(gx, gy - 1), tile_color(gx - 1, gy), tile_color(gx, gy)].into_iter().flatten().collect();
            (near.iter().sum::<Vec3>() / near.len().max(1) as f32).to_array()
        };
        for y in 0..h {
            for x in 0..w {
                let c = gnd.cube(x as u32, y as u32).unwrap();
                let p = corner(c, x, y);
                if let Some(s) = gnd.surface(c.top) {
                    let normals = [0, 1, 2, 3].map(|k| smooth(x, y, k));
                    let colors = [0, 1, 2, 3].map(|k| smooth_color(x, y, k));
                    self.ground_quad(&tex, s, p, normals, Some((x as u32, y as u32)), Some(colors));
                }
                // Walls face the lower neighbour, which is the side that can be seen.
                if let (Some(s), Some(n)) = (gnd.surface(c.front), gnd.cube(x as u32, y as u32 + 1)) {
                    let np = corner(n, x, y + 1);
                    let quad = [p[2], p[3], np[0], np[1]];
                    let facing = if p[2].y + p[3].y > np[0].y + np[1].y { Vec3::Z } else { -Vec3::Z };
                    self.ground_quad(&tex, s, quad, [facing; 4], None, None);
                }
                if let (Some(s), Some(n)) = (gnd.surface(c.right), gnd.cube(x as u32 + 1, y as u32)) {
                    let np = corner(n, x + 1, y);
                    let quad = [p[3], p[1], np[2], np[0]];
                    let facing = if p[1].y + p[3].y > np[0].y + np[2].y { Vec3::X } else { -Vec3::X };
                    self.ground_quad(&tex, s, quad, [facing; 4], None, None);
                }
            }
        }
    }

    /// `p`/`normals` are already y-up.
    /// `tile`: the ground tile for top surfaces (continuous lightmap grid); walls pass None.
    fn ground_quad(&mut self, tex: &[usize], s: &ro_formats::gnd::Surface, p: [Vec3; 4], normals: [Vec3; 4], tile: Option<(u32, u32)>, colors: Option<[[f32; 3]; 4]>) {
        let Some(&t) = usize::try_from(s.texture).ok().and_then(|i| tex.get(i)) else { return };
        let lm = match tile {
            Some((x, y)) if self.atlas.grid_y < self.atlas.height => self.atlas.ground_uvs(x, y),
            _ => self.atlas.uvs(Some(s.lightmap)),
        };
        let color = [s.color[2] as f32 / 255.0, s.color[1] as f32 / 255.0, s.color[0] as f32 / 255.0];
        let v = |i: usize| Vertex { pos: p[i].into(), normal: normals[i].into(), uv: [s.u[i], s.v[i]], lightmap_uv: lm[i], color: colors.map_or(color, |c| c[i]) };
        self.per_tex[t].extend([v(0), v(1), v(3), v(0), v(3), v(2)]);
    }

    /// Places one RSW model instance.
    pub fn add_model(&mut self, rsm: &Rsm, inst: &rsw::Model) {
        let g = self.gnd;
        let world = Mat4::from_translation(Vec3::new(
            inst.pos[0] + g.width as f32 * g.zoom / 2.0,
            inst.pos[1],
            inst.pos[2] + g.height as f32 * g.zoom / 2.0,
        )) * Mat4::from_rotation_z(inst.rot[2].to_radians())
            * Mat4::from_rotation_x(inst.rot[0].to_radians())
            * Mat4::from_rotation_y(inst.rot[1].to_radians())
            * Mat4::from_scale(Vec3::from(inst.scale));

        let matrices = node_matrices(rsm);
        let white = self.atlas.white();
        let tex: Vec<usize> = rsm.textures.iter().map(|t| self.texture(t)).collect();

        for (node, m) in rsm.nodes.iter().zip(matrices) {
            let full = world * m;
            let normal_m = Mat3::from_mat4(full).inverse().transpose();
            let pos: Vec<Vec3> = node.vertices.iter().map(|&v| full.transform_point3(Vec3::from(v))).collect();

            let face_normal = |f: &ro_formats::rsm::Face| {
                let [a, b, c] = f.vertices.map(|i| node.vertices.get(i as usize).copied().map(Vec3::from).unwrap_or(Vec3::ZERO));
                (normal_m * (b - a).cross(c - a)).normalize_or(Vec3::NEG_Y)
            };
            // Smooth shading: average normals per (vertex, smoothing group).
            let mut smooth: HashMap<(u16, i32), Vec3> = HashMap::new();
            if rsm.shade_type == 2 {
                for f in &node.faces {
                    let n = face_normal(f);
                    for &vi in &f.vertices {
                        *smooth.entry((vi, f.smooth_group)).or_default() += n;
                    }
                }
            }

            for f in &node.faces {
                let Some(&t) = node.textures.get(f.texture as usize).and_then(|&i| usize::try_from(i).ok()).and_then(|i| tex.get(i)) else {
                    continue;
                };
                let flat = face_normal(f);
                let mut tri = [Vertex { pos: [0.0; 3], normal: [0.0; 3], uv: [0.0; 2], lightmap_uv: white, color: [1.0; 3] }; 3];
                for k in 0..3 {
                    let vi = f.vertices[k];
                    let Some(&p) = pos.get(vi as usize) else { continue };
                    let n = smooth.get(&(vi, f.smooth_group)).map_or(flat, |n| n.normalize_or(flat));
                    tri[k].pos = flip(p);
                    tri[k].normal = flip(n);
                    tri[k].uv = node.tex_coords.get(f.tex_coords[k] as usize).copied().unwrap_or_default();
                }
                self.per_tex[t].extend(tri);
            }
        }
    }

    pub fn finish(self) -> SceneMesh {
        let mut vertices = Vec::new();
        let mut ranges = Vec::new();
        for list in self.per_tex {
            let start = vertices.len() as u32;
            vertices.extend(list);
            ranges.push(start..vertices.len() as u32);
        }
        let g = self.gnd;
        SceneMesh {
            vertices,
            ranges,
            textures: self.textures,
            lightmap: self.atlas.image,
            width: g.width,
            height: g.height,
            zoom: g.zoom,
            heights: g.cubes.iter().map(|c| c.heights.map(|h| -h)).collect(),
        }
    }
}

/// Per-node vertex matrices in RO space, following the RSM 1.x hierarchy.
fn node_matrices(rsm: &Rsm) -> Vec<Mat4> {
    let mat3 = |n: &ro_formats::rsm::Node| Mat4::from_mat3(Mat3::from_cols_array(&n.mat3));
    let local_rot = |n: &ro_formats::rsm::Node| match n.rot_keys.first() {
        Some((_, q)) => Mat4::from_quat(Quat::from_array(*q).normalize()),
        None if Vec3::from(n.rot_axis).length_squared() > 0.0 => {
            Mat4::from_axis_angle(Vec3::from(n.rot_axis).normalize(), n.rot_angle)
        }
        None => Mat4::IDENTITY,
    };

    let by_name: HashMap<&str, usize> = rsm.nodes.iter().enumerate().map(|(i, n)| (n.name.as_str(), i)).collect();
    let mut global: Vec<Option<Mat4>> = vec![None; rsm.nodes.len()];

    fn resolve(
        i: usize,
        rsm: &Rsm,
        by_name: &HashMap<&str, usize>,
        global: &mut Vec<Option<Mat4>>,
        root_shift: Mat4,
        local_rot: &dyn Fn(&ro_formats::rsm::Node) -> Mat4,
        depth: usize,
    ) -> Mat4 {
        if let Some(m) = global[i] {
            return m;
        }
        let n = &rsm.nodes[i];
        let parent = by_name.get(n.parent.as_str()).copied().filter(|&p| p != i && depth < 64);
        let base = match parent {
            Some(p) => resolve(p, rsm, by_name, global, root_shift, local_rot, depth + 1) * Mat4::from_translation(Vec3::from(n.pos)),
            // The root node is placed by its own position too (it cancels out in the shift below).
            None => root_shift * Mat4::from_translation(Vec3::from(n.pos)),
        };
        let m = base * local_rot(n) * Mat4::from_scale(Vec3::from(n.scale));
        global[i] = Some(m);
        m
    }

    // Every node's matrix starts with the root shift on the left, so build them unshifted,
    // measure the fully transformed model, then centre it in x/z and rest its lowest
    // point (max y, RO is y-down) on the placement height.
    let unshifted: Vec<Mat4> = (0..rsm.nodes.len())
        .map(|i| {
            let g = resolve(i, rsm, &by_name, &mut global, Mat4::IDENTITY, &local_rot, 0);
            g * Mat4::from_translation(Vec3::from(rsm.nodes[i].offset)) * mat3(&rsm.nodes[i])
        })
        .collect();
    let (mut min, mut max) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for (n, m) in rsm.nodes.iter().zip(&unshifted) {
        for &v in &n.vertices {
            let p = m.transform_point3(Vec3::from(v));
            min = min.min(p);
            max = max.max(p);
        }
    }
    if min.x > max.x {
        (min, max) = (Vec3::ZERO, Vec3::ZERO);
    }
    let center = (min + max) / 2.0;
    // Same for single- and multi-node models: centre in x/z, lowest point on the ground.
    let shift = Vec3::new(-center.x, -max.y, -center.z);
    unshifted.into_iter().map(|m| Mat4::from_translation(shift) * m).collect()
}

impl SceneMesh {
    /// Ground height (y-up) under a world position, bilinear within the cube's top.
    pub fn height_at(&self, wx: f32, wz: f32) -> f32 {
        let (fx, fz) = (wx / self.zoom, wz / self.zoom);
        let (cx, cz) = (fx.floor(), fz.floor());
        if cx < 0.0 || cz < 0.0 || cx as u32 >= self.width || cz as u32 >= self.height {
            return 0.0;
        }
        let h = self.heights[(cz as u32 * self.width + cx as u32) as usize];
        let (tx, tz) = (fx - cx, fz - cz);
        let near = h[0] + (h[1] - h[0]) * tx;
        let far = h[2] + (h[3] - h[2]) * tx;
        near + (far - near) * tz
    }
}

/// GPU-side scene.
pub(crate) struct Scene {
    pub vertices: wgpu::Buffer,
    pub ranges: Vec<std::ops::Range<u32>>,
    pub textures: Vec<wgpu::BindGroup>,
}

impl Scene {
    pub fn upload(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        lightmap_sampler: &wgpu::Sampler,
        mesh: &SceneMesh,
        images: Vec<Option<Rgba>>,
    ) -> Self {
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("scene"),
            contents: bytemuck::cast_slice(&mesh.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let lightmap = upload_texture(device, queue, &mesh.lightmap, false);
        let fallback = Rgba { width: 1, height: 1, pixels: vec![255, 0, 255, 255] };
        let textures = images
            .into_iter()
            .map(|img| {
                let view = upload_texture(device, queue, img.as_ref().unwrap_or(&fallback), true);
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("scene texture"),
                    layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
                        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&lightmap) },
                        wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(lightmap_sampler) },
                    ],
                })
            })
            .collect();
        Self { vertices, ranges: mesh.ranges.clone(), textures }
    }
}

/// Uploads an RGBA8 texture (gamma-space values; the shader blends like the original client),
/// optionally with a CPU-generated mip chain.
pub(crate) fn upload_texture(device: &wgpu::Device, queue: &wgpu::Queue, img: &Rgba, mips: bool) -> wgpu::TextureView {
    let mut levels = vec![];
    let mut cur = Some(Rgba { width: img.width, height: img.height, pixels: img.pixels.clone() });
    while let Some(l) = cur {
        cur = if mips { l.downsample() } else { None };
        levels.push(l);
    }
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("texture"),
        size: wgpu::Extent3d { width: img.width, height: img.height, depth_or_array_layers: 1 },
        mip_level_count: levels.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (mip, l) in levels.iter().enumerate() {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: mip as u32, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &l.pixels,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(l.width * 4), rows_per_image: Some(l.height) },
            wgpu::Extent3d { width: l.width, height: l.height, depth_or_array_layers: 1 },
        );
    }
    tex.create_view(&Default::default())
}

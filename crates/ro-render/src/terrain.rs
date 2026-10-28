//! GND terrain: one vertex buffer sorted by texture, drawn as one call per texture.

use ro_formats::{
    gnd::{Gnd, Surface},
    image::Rgba,
};
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    pub color: [f32; 3],
}

impl Vertex {
    const ATTRS: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x3];

    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}

/// CPU-side mesh: `vertices` grouped so `ranges[i]` draws texture i.
pub struct TerrainMesh {
    pub vertices: Vec<Vertex>,
    pub ranges: Vec<std::ops::Range<u32>>,
    pub width: u32,
    pub height: u32,
    pub zoom: f32,
    /// World-space height (+Y up) per cube corner, for placing the camera and actors.
    heights: Vec<[f32; 4]>,
}

impl TerrainMesh {
    pub fn build(gnd: &Gnd) -> Self {
        let mut per_tex: Vec<Vec<Vertex>> = vec![Vec::new(); gnd.textures.len()];
        let z = gnd.zoom;
        // Corner order (x,y), (x+1,y), (x,y+1), (x+1,y+1); RO height grows downward.
        let mut quad = |s: &Surface, corners: [[f32; 3]; 4]| {
            let Some(list) = usize::try_from(s.texture).ok().and_then(|t| per_tex.get_mut(t)) else { return };
            let color = [s.color[2] as f32 / 255.0, s.color[1] as f32 / 255.0, s.color[0] as f32 / 255.0];
            let v = |i: usize| Vertex { pos: corners[i], uv: [s.u[i], s.v[i]], color };
            list.extend([v(0), v(1), v(3), v(0), v(3), v(2)]);
        };

        for y in 0..gnd.height {
            for x in 0..gnd.width {
                let c = gnd.cube(x, y).unwrap();
                let h = c.heights.map(|h| -h);
                let (x0, x1, y0, y1) = (x as f32 * z, (x + 1) as f32 * z, y as f32 * z, (y + 1) as f32 * z);

                if let Some(s) = gnd.surface(c.top) {
                    quad(s, [[x0, h[0], y0], [x1, h[1], y0], [x0, h[2], y1], [x1, h[3], y1]]);
                }
                // Front: the wall along y+1 down/up to the neighbouring cube.
                if let (Some(s), Some(n)) = (gnd.surface(c.front), gnd.cube(x, y + 1)) {
                    let nh = n.heights.map(|h| -h);
                    quad(s, [[x0, h[2], y1], [x1, h[3], y1], [x0, nh[0], y1], [x1, nh[1], y1]]);
                }
                // Right: the wall along x+1.
                if let (Some(s), Some(n)) = (gnd.surface(c.right), gnd.cube(x + 1, y)) {
                    let nh = n.heights.map(|h| -h);
                    quad(s, [[x1, h[3], y1], [x1, h[1], y0], [x1, nh[2], y1], [x1, nh[0], y0]]);
                }
            }
        }

        let mut vertices = Vec::new();
        let mut ranges = Vec::new();
        for list in per_tex {
            let start = vertices.len() as u32;
            vertices.extend(list);
            ranges.push(start..vertices.len() as u32);
        }
        let heights = gnd.cubes.iter().map(|c| c.heights.map(|h| -h)).collect();
        Self { vertices, ranges, width: gnd.width, height: gnd.height, zoom: z, heights }
    }

    /// Ground height under a world position, bilinear within the cube's top.
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

/// GPU-side terrain.
pub(crate) struct Terrain {
    pub vertices: wgpu::Buffer,
    pub ranges: Vec<std::ops::Range<u32>>,
    /// Per texture; None when the texture failed to load (drawn with a fallback).
    pub textures: Vec<wgpu::BindGroup>,
}

impl Terrain {
    pub fn upload(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        mesh: &TerrainMesh,
        images: Vec<Option<Rgba>>,
    ) -> Self {
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("terrain"),
            contents: bytemuck::cast_slice(&mesh.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let fallback = Rgba { width: 1, height: 1, pixels: vec![255, 0, 255, 255] };
        let textures = images
            .into_iter()
            .map(|img| {
                let view = upload_texture(device, queue, img.as_ref().unwrap_or(&fallback));
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("terrain texture"),
                    layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
                    ],
                })
            })
            .collect();
        Self { vertices, ranges: mesh.ranges.clone(), textures }
    }
}

/// Uploads an sRGB texture with a full CPU-generated mip chain.
pub(crate) fn upload_texture(device: &wgpu::Device, queue: &wgpu::Queue, img: &Rgba) -> wgpu::TextureView {
    let mut levels = vec![];
    let mut cur = Some(Rgba { width: img.width, height: img.height, pixels: img.pixels.clone() });
    while let Some(l) = cur {
        cur = l.downsample();
        levels.push(l);
    }
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("texture"),
        size: wgpu::Extent3d { width: img.width, height: img.height, depth_or_array_layers: 1 },
        mip_level_count: levels.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
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

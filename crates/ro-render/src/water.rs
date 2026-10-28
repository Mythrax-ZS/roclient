//! RSW water: an animated, wave-displaced surface over tiles whose ground lies below the
//! water level.

use ro_formats::{gnd::Gnd, image::Rgba, rsw};
use wgpu::util::DeviceExt;

pub const FRAMES: usize = 32;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    /// x, z in world units; tile coordinates for the wave phase.
    pub pos: [f32; 2],
    pub tile: [f32; 2],
}

impl Vertex {
    const ATTRS: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2];

    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Params {
    /// y-up level, wave height, wave speed (degrees per frame), wave pitch (degrees per tile)
    pub wave: [f32; 4],
    /// frames per texture step
    pub anim: [f32; 4],
}

pub struct WaterMesh {
    pub vertices: Vec<Vertex>,
    pub water: rsw::Water,
    /// `data\texture\` relative paths of the animation frames.
    pub textures: Vec<String>,
}

impl WaterMesh {
    pub fn build(gnd: &Gnd, water: &rsw::Water) -> Self {
        let level = -water.level;
        let z = gnd.zoom;
        let mut vertices = Vec::new();
        for y in 0..gnd.height {
            for x in 0..gnd.width {
                let c = gnd.cube(x, y).unwrap();
                // Visible only where some of the ground dips below the surface (y-up).
                if !c.heights.iter().any(|&h| -h < level) {
                    continue;
                }
                let v = |dx: u32, dy: u32| Vertex {
                    pos: [(x + dx) as f32 * z, (y + dy) as f32 * z],
                    tile: [(x + dx) as f32, (y + dy) as f32],
                };
                vertices.extend([v(0, 0), v(1, 0), v(1, 1), v(0, 0), v(1, 1), v(0, 1)]);
            }
        }
        let textures = (0..FRAMES).map(|i| format!("워터\\water{}{i:02}.jpg", water.kind)).collect();
        Self { vertices, water: *water, textures }
    }
}

pub(crate) struct Water {
    pub vertices: wgpu::Buffer,
    pub count: u32,
    /// Kept alive for the bind groups that reference it.
    pub _params: wgpu::Buffer,
    pub frames: Vec<wgpu::BindGroup>,
    pub anim_speed: f32,
}

impl Water {
    pub fn upload(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        mesh: &WaterMesh,
        images: Vec<Option<Rgba>>,
    ) -> Option<Self> {
        if mesh.vertices.is_empty() {
            return None;
        }
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("water"),
            contents: bytemuck::cast_slice(&mesh.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let w = &mesh.water;
        let anim_speed = (w.anim_speed.max(1)) as f32;
        let params = Params { wave: [-w.level, w.wave_height, w.wave_speed, w.wave_pitch], anim: [anim_speed, 0.0, 0.0, 0.0] };
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("water params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let fallback = Rgba { width: 1, height: 1, pixels: vec![40, 80, 140, 255] };
        let frames = images
            .into_iter()
            .map(|img| {
                let view = crate::scene::upload_texture(device, queue, img.as_ref().unwrap_or(&fallback), true);
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("water frame"),
                    layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
                        wgpu::BindGroupEntry { binding: 2, resource: params.as_entire_binding() },
                    ],
                })
            })
            .collect();
        Some(Self { vertices, count: mesh.vertices.len() as u32, _params: params, frames, anim_speed })
    }

    /// Texture frame for a time in seconds (the original animates at 60 ticks/s).
    pub fn frame(&self, time: f32) -> usize {
        ((time * 60.0 / self.anim_speed) as usize) % self.frames.len().max(1)
    }
}

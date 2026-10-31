//! Shadows of sprites. Characters, monsters and NPCs are drawn into a depth map from the sun
//! (and, in a map of their own each, from the torches nearest the player) as alpha-tested
//! upright cards; the ground and models sample it (scene.wgsl), so sprites cast silhouette
//! shadows onto the world. Static geometry does not cast here: RO maps already bake its
//! shadows into the lightmaps (or they are ray traced, rt.rs).

use glam::{camera::rh, Mat4, Vec3};

use crate::sprite::Vertex;

pub const SIZE: u32 = 2048;
/// Each torch's shadow map.
pub const TORCH_SIZE: u32 = 1024;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniform {
    light_vp: [[f32; 4]; 4],
    right: [f32; 4],
    /// xyz a torch's position, w 1 to turn each card toward it (else all face along `right`).
    light: [f32; 4],
}

pub struct Shadow {
    pub view: wgpu::TextureView,
    pub compare: wgpu::Sampler,
    uniform: wgpu::Buffer,
    bind: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
}

impl Shadow {
    /// A `size`² shadow map.
    pub fn new(device: &wgpu::Device, sprite_layout: &wgpu::BindGroupLayout, size: u32) -> Self {
        let view = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("shadow map"),
                size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let compare = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow compare"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shadow"),
            size: std::mem::size_of::<Uniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() }],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("shadow"), source: wgpu::ShaderSource::Wgsl(include_str!("shadow.wgsl").into()) });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("shadow"), bind_group_layouts: &[Some(&layout), Some(sprite_layout)], ..Default::default() });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow"),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_main"), compilation_options: Default::default(), buffers: &[Some(Vertex::layout())] },
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                // No bias: only sprites cast, so the ground can't shadow itself, and any bias
                // detaches the shadow from the feet.
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs_main"), compilation_options: Default::default(), targets: &[] }),
            multiview_mask: None,
            cache: None,
        });
        Self { view, compare, uniform, bind, pipeline }
    }

    /// The sun's view-projection for a square of half-size `radius` around `center`.
    /// `dir` is the direction the light travels.
    pub fn light_vp(center: Vec3, dir: Vec3, radius: f32) -> Mat4 {
        let dir = if dir.length_squared() < 1e-6 { Vec3::new(0.3, -1.0, 0.4) } else { dir.normalize() };
        let up = if dir.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
        let eye = center - dir * 2000.0;
        // Same conventions as the main camera (D3D-style 0..1 depth).
        rh::proj::directx::orthographic(-radius, radius, -radius, radius, 1.0, 4000.0) * rh::view::look_at_mat4(eye, center, up)
    }

    /// A torch's view-projection: from `light` toward `target`, wide enough for the sprites
    /// around it, out to the light's `range`.
    pub fn torch_vp(light: Vec3, target: Vec3, range: f32) -> Mat4 {
        let dir = (target - light).try_normalize().unwrap_or(Vec3::NEG_Y);
        let up = if dir.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
        rh::proj::directx::perspective(120f32.to_radians(), 1.0, 1.0, range.max(10.0) * 1.5) * rh::view::look_at_mat4(light, light + dir, up)
    }

    /// Draws the sprite casters (`draws`: sheet bind group per vertex range).
    pub fn record<'a>(
        &self,
        queue: &wgpu::Queue,
        enc: &mut wgpu::CommandEncoder,
        light_vp: Mat4,
        right: Vec3,
        light: Option<Vec3>,
        units_per_pixel: f32,
        vertices: Option<&wgpu::Buffer>,
        draws: impl Iterator<Item = (&'a wgpu::BindGroup, std::ops::Range<u32>)>,
    ) {
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&Uniform { light_vp: light_vp.to_cols_array_2d(), right: right.extend(units_per_pixel).to_array(), light: light.map_or([0.0; 4], |l| l.extend(1.0).to_array()) }));
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("sprite shadows"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.view,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        let Some(vertices) = vertices else { return };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind, &[]);
        pass.set_vertex_buffer(0, vertices.slice(..));
        for (bind, range) in draws {
            pass.set_bind_group(1, bind, &[]);
            pass.draw(range, 0..1);
        }
    }
}

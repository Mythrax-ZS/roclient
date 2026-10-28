//! wgpu renderer. Vulkan preferred; DX12 / GL selectable via config.

mod camera;
mod terrain;

use std::sync::Arc;

pub use camera::{Camera, CELL};
pub use ro_formats::image::Rgba;
pub use terrain::TerrainMesh;
use winit::window::Window;

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// sRGB (0.05, 0.06, 0.09) expressed in linear space.
const CLEAR: wgpu::Color = wgpu::Color { r: 0.0039, g: 0.0050, b: 0.0080, a: 1.0 };

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    focus: [f32; 4],
    eye: [f32; 4],
}

/// Size-dependent render targets, rebuilt on resize.
struct Targets {
    depth: wgpu::TextureView,
    /// None when MSAA is off.
    msaa: Option<wgpu::TextureView>,
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    samples: u32,
    targets: Targets,
    globals: wgpu::Buffer,
    globals_bind: wgpu::BindGroup,
    grid: wgpu::RenderPipeline,
    terrain_pipeline: wgpu::RenderPipeline,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// The grid is drawn instead while no map is loaded.
    terrain: Option<terrain::Terrain>,
    pub camera: Camera,
    pub scale_factor: f64,
}

pub fn parse_backends(name: &str) -> wgpu::Backends {
    match name.to_ascii_lowercase().as_str() {
        "vulkan" => wgpu::Backends::VULKAN,
        "dx12" => wgpu::Backends::DX12,
        "gl" => wgpu::Backends::GL,
        "metal" => wgpu::Backends::METAL,
        _ => wgpu::Backends::all(),
    }
}

impl Renderer {
    pub async fn new(window: Arc<Window>, backends: wgpu::Backends, msaa: u32) -> anyhow::Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends,
            ..wgpu::InstanceDescriptor::new_without_display_handle_from_env()
        });
        let surface = instance.create_surface(window.clone())?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await?;
        let info = adapter.get_info();
        tracing::info!(backend = ?info.backend, name = %info.name, "adapter");

        let (device, queue) = adapter.request_device(&wgpu::DeviceDescriptor::default()).await?;
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or_else(|| anyhow::anyhow!("surface not supported by adapter"))?;
        // Shaders output linear color; let the sRGB view encode it.
        let caps = surface.get_capabilities(&adapter);
        if let Some(srgb) = caps.formats.iter().copied().find(|f| f.is_srgb()) {
            config.format = srgb;
        }
        config.present_mode = wgpu::PresentMode::AutoVsync;
        config.desired_maximum_frame_latency = 2;
        surface.configure(&device, &config);

        let supported = adapter.get_texture_format_features(config.format).flags;
        let samples = [msaa, 4, 2, 1]
            .into_iter()
            .find(|&s| s == 1 || (s <= msaa && supported.sample_count_supported(s)))
            .unwrap_or(1);
        tracing::info!(format = ?config.format, samples, "surface");

        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let globals_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() }],
        });

        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("texture"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 16,
            ..Default::default()
        });

        let pipeline = |label: &str, src: &str, layouts: &[Option<&wgpu::BindGroupLayout>], buffers: &[Option<wgpu::VertexBufferLayout>]| {
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(src.into()),
            });
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: layouts,
                ..Default::default()
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(config.format.into())],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let grid = pipeline("grid", include_str!("grid.wgsl"), &[Some(&globals_layout)], &[]);
        let terrain_pipeline = pipeline(
            "terrain",
            include_str!("terrain.wgsl"),
            &[Some(&globals_layout), Some(&texture_layout)],
            &[Some(terrain::Vertex::layout())],
        );

        let targets = Self::make_targets(&device, &config, samples);
        Ok(Self {
            surface,
            device,
            queue,
            config,
            samples,
            targets,
            globals,
            globals_bind,
            grid,
            terrain_pipeline,
            texture_layout,
            sampler,
            terrain: None,
            camera: Camera::default(),
            scale_factor: window.scale_factor(),
        })
    }

    /// Replaces the ground with a map. `images[i]` is texture i of the GND (None if missing).
    pub fn set_terrain(&mut self, mesh: &TerrainMesh, images: Vec<Option<Rgba>>) {
        self.terrain = Some(terrain::Terrain::upload(&self.device, &self.queue, &self.texture_layout, &self.sampler, mesh, images));
    }

    fn make_targets(device: &wgpu::Device, config: &wgpu::SurfaceConfiguration, samples: u32) -> Targets {
        let size = wgpu::Extent3d { width: config.width, height: config.height, depth_or_array_layers: 1 };
        let tex = |label, format| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size,
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        Targets { depth: tex("depth", DEPTH_FORMAT), msaa: (samples > 1).then(|| tex("msaa", config.format)) }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return; // minimized
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.targets = Self::make_targets(&self.device, &self.config, self.samples);
    }

    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    pub fn aspect(&self) -> f32 {
        self.config.width as f32 / self.config.height as f32
    }

    pub fn frame(&mut self) -> anyhow::Result<()> {
        let tex = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            other => {
                tracing::warn!(?other, "skipping frame");
                return Ok(());
            }
        };

        let cam = &self.camera;
        let globals = Globals {
            view_proj: cam.view_proj(self.aspect()).to_cols_array_2d(),
            focus: [cam.target.x, CELL, cam.target.z, cam.distance * 4.0],
            eye: cam.eye().extend(1.0).to_array(),
        };
        self.queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));

        let view = tex.texture.create_view(&Default::default());
        let (color, resolve) = match &self.targets.msaa {
            Some(m) => (m, Some(&view)),
            None => (&view, None),
        };
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color,
                    depth_slice: None,
                    resolve_target: resolve,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(CLEAR), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.targets.depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &self.globals_bind, &[]);
            match &self.terrain {
                Some(t) => {
                    pass.set_pipeline(&self.terrain_pipeline);
                    pass.set_vertex_buffer(0, t.vertices.slice(..));
                    for (range, tex) in t.ranges.iter().zip(&t.textures) {
                        if !range.is_empty() {
                            pass.set_bind_group(1, tex, &[]);
                            pass.draw(range.clone(), 0..1);
                        }
                    }
                }
                None => {
                    pass.set_pipeline(&self.grid);
                    pass.draw(0..6, 0..1);
                }
            }
        }
        self.queue.submit([enc.finish()]);
        self.queue.present(tex);
        Ok(())
    }
}

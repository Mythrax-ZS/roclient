//! wgpu renderer. Vulkan preferred; DX12 / GL selectable via config.

mod camera;
mod scene;
mod effect;
mod fx;
mod sprite;
mod water;

use std::sync::Arc;

use wgpu::util::DeviceExt;

pub use camera::{Camera, CELL};
pub use ro_formats::image::Rgba;
pub use scene::{SceneBuilder, SceneMesh};
pub use water::WaterMesh;
pub use effect::{ProcFx, Shape, TexId};
pub use fx::{Fx, FxId};
pub use sprite::{Actor, Dir, SheetId};
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
    light_dir: [f32; 4],
    ambient: [f32; 4],
    diffuse: [f32; 4],
    /// x: seconds since start
    params: [f32; 4],
}

/// One frame of tessellated egui output.
pub struct UiFrame {
    pub primitives: Vec<egui::ClippedPrimitive>,
    pub textures: egui::TexturesDelta,
    pub pixels_per_point: f32,
}

/// Map-wide sun lighting (from the RSW), in the renderer's y-up space.
#[derive(Clone, Copy, Debug)]
pub struct Lighting {
    /// Direction the light travels.
    pub direction: [f32; 3],
    pub ambient: [f32; 3],
    pub diffuse: [f32; 3],
}

impl Default for Lighting {
    fn default() -> Self {
        Self { direction: [-0.5, -0.7, -0.5], ambient: [0.5; 3], diffuse: [1.0; 3] }
    }
}

impl Lighting {
    pub fn from_rsw(light: &ro_formats::rsw::Light) -> Self {
        let [x, y, z] = light.direction();
        Self { direction: [x, -y, z], ambient: light.ambient, diffuse: light.diffuse }
    }
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
    scene_pipeline: wgpu::RenderPipeline,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    lightmap_sampler: wgpu::Sampler,
    water_pipeline: wgpu::RenderPipeline,
    water_layout: wgpu::BindGroupLayout,
    repeat_sampler: wgpu::Sampler,
    water: Option<water::Water>,
    sprite_pipeline: wgpu::RenderPipeline,
    sprite_layout: wgpu::BindGroupLayout,
    sheets: Vec<sprite::Sheet>,
    ui_renderer: egui_wgpu::Renderer,
    /// UI to draw over the next frame (set by the caller each frame).
    ui: Option<UiFrame>,
    /// Actors drawn each frame; the caller updates positions, actions and times.
    pub actors: Vec<Actor>,
    /// Warp portal positions drawn each frame.
    pub warps: Vec<glam::Vec3>,
    warp_texture: Option<wgpu::BindGroup>,
    fx_pipeline: wgpu::RenderPipeline,
    fx_assets: Vec<fx::FxAsset>,
    effect_textures: Vec<wgpu::BindGroup>,
    /// Code-drawn effects playing this frame; the client advances and removes them.
    pub procs: Vec<ProcFx>,
    /// STR effects playing this frame; the client advances and removes them.
    pub effects: Vec<Fx>,
    effect_pipeline: wgpu::RenderPipeline,
    started: std::time::Instant,
    /// The grid is drawn instead while no map is loaded.
    scene: Option<scene::Scene>,
    pub lighting: Lighting,
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
        config.view_formats = vec![config.format.remove_srgb_suffix()];
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
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let lightmap_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("lightmap"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
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

        let water_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("water"),
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
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let repeat_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("repeat"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 16,
            ..Default::default()
        });

        // `blend: Some(..)` makes a transparent pass: blended, depth-tested but not written.
        let pipeline = |label: &str,
                        src: &str,
                        layouts: &[Option<&wgpu::BindGroupLayout>],
                        buffers: &[Option<wgpu::VertexBufferLayout>],
                        blend: Option<wgpu::BlendState>| {
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
                    depth_write_enabled: Some(blend.is_none()),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: config.format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let grid = pipeline("grid", include_str!("grid.wgsl"), &[Some(&globals_layout)], &[], None);
        let scene_pipeline = pipeline(
            "scene",
            include_str!("scene.wgsl"),
            &[Some(&globals_layout), Some(&texture_layout)],
            &[Some(scene::Vertex::layout())],
            None,
        );
        let water_pipeline = pipeline(
            "water",
            include_str!("water.wgsl"),
            &[Some(&globals_layout), Some(&water_layout)],
            &[Some(water::Vertex::layout())],
            Some(wgpu::BlendState::ALPHA_BLENDING),
        );
        let sprite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sprite"),
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
        let effect_pipeline = pipeline(
            "effect",
            include_str!("effect.wgsl"),
            &[Some(&globals_layout), Some(&sprite_layout)],
            &[Some(effect::Vertex::layout())],
            Some(wgpu::BlendState {
                color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
                alpha: wgpu::BlendComponent::OVER,
            }),
        );
        let sprite_pipeline = pipeline(
            "sprite",
            include_str!("sprite.wgsl"),
            &[Some(&globals_layout), Some(&sprite_layout)],
            &[Some(sprite::Vertex::layout())],
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
        );
        let fx_pipeline = pipeline(
            "str effects",
            include_str!("strfx.wgsl"),
            &[Some(&globals_layout), Some(&sprite_layout)],
            &[Some(sprite::Vertex::layout())],
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
        );

        let ui_renderer = egui_wgpu::Renderer::new(&device, config.format.remove_srgb_suffix(), Default::default());
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
            scene_pipeline,
            texture_layout,
            sampler,
            lightmap_sampler,
            scene: None,
            water_pipeline,
            water_layout,
            repeat_sampler,
            water: None,
            sprite_pipeline,
            sprite_layout,
            sheets: vec![],
            ui_renderer,
            ui: None,
            actors: vec![],
            warps: vec![],
            warp_texture: None,
            fx_pipeline,
            fx_assets: vec![],
            effect_textures: vec![],
            procs: vec![],
            effects: vec![],
            effect_pipeline,
            started: std::time::Instant::now(),
            lighting: Lighting::default(),
            camera: Camera::default(),
            scale_factor: window.scale_factor(),
        })
    }

    /// Replaces the world. `images[i]` is `mesh.textures[i]` (None if missing).
    pub fn set_scene(&mut self, mesh: &SceneMesh, images: Vec<Option<Rgba>>, lighting: Lighting) {
        self.scene = Some(scene::Scene::upload(
            &self.device,
            &self.queue,
            &self.texture_layout,
            &self.sampler,
            &self.lightmap_sampler,
            mesh,
            images,
        ));
        self.lighting = lighting;
        self.water = None;
    }

    /// Uploads a sprite sheet. `palette` overrides the SPR's own (hair / body dyes).
    /// Texture for warp portals (effect\ring_blue.tga).
    pub fn set_warp_texture(&mut self, img: &Rgba) {
        let view = scene::upload_texture(&self.device, &self.queue, img, true);
        self.warp_texture = Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("warp texture"),
            layout: &self.sprite_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.repeat_sampler) },
            ],
        }));
    }

    /// Uploads a texture for code-drawn effects (light columns, ground circles).
    pub fn add_effect_texture(&mut self, img: &Rgba) -> TexId {
        let view = scene::upload_texture(&self.device, &self.queue, img, true);
        self.effect_textures.push(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("effect texture"),
            layout: &self.sprite_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.repeat_sampler) },
            ],
        }));
        TexId(self.effect_textures.len() - 1)
    }

    /// How long one pass of an actor action takes (sprite effects play it once).
    pub fn action_ms(&self, sheet: SheetId, action: usize) -> f32 {
        self.sheets.get(sheet.0).map_or(0.0, |s| s.action_ms(action))
    }

    pub fn add_sheet(&mut self, spr: &ro_formats::spr::Spr, act: ro_formats::act::Act, palette: Option<&[[u8; 4]; 256]>) -> SheetId {
        let sheet = sprite::Sheet::upload(&self.device, &self.queue, &self.sprite_layout, &self.sampler, spr, act, palette);
        self.sheets.push(sheet);
        SheetId(self.sheets.len() - 1)
    }

    /// Uploads an STR effect; `images[layer][texture]` as named in the STR.
    pub fn add_fx(&mut self, str: ro_formats::strm::Str, images: Vec<Vec<Option<Rgba>>>) -> FxId {
        let asset = fx::FxAsset::upload(&self.device, &self.queue, &self.sprite_layout, &self.sampler, str, images);
        self.fx_assets.push(asset);
        FxId(self.fx_assets.len() - 1)
    }

    /// How long an effect plays, in ms.
    pub fn fx_duration(&self, id: FxId) -> f32 {
        self.fx_assets.get(id.0).map_or(0.0, |a| a.duration_ms())
    }

    /// The sound (data\wav name) an actor's body frame asks for, if it entered a new frame
    /// since `from_ms` (negative: the action just started).
    pub fn actor_sound(&self, a: &Actor, from_ms: f32) -> Option<String> {
        let index = a.action * 8 + a.dir as usize;
        self.sheets.get(a.body.0)?.sound_between(index, from_ms, a.time_ms, a.once).map(str::to_string)
    }

    /// Adds the map's water. `images[i]` is `mesh.textures[i]`.
    pub fn set_water(&mut self, mesh: &WaterMesh, images: Vec<Option<Rgba>>) {
        self.water = water::Water::upload(&self.device, &self.queue, &self.water_layout, &self.repeat_sampler, mesh, images);
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

    /// World-space ray (origin, unit direction) through a window pixel.
    pub fn ray(&self, px: f32, py: f32) -> (glam::Vec3, glam::Vec3) {
        let (w, h) = (self.config.width as f32, self.config.height as f32);
        let ndc = glam::Vec2::new(2.0 * px / w - 1.0, 1.0 - 2.0 * py / h);
        let inv = self.camera.view_proj(self.aspect()).inverse();
        let near = inv.project_point3(ndc.extend(0.0));
        let far = inv.project_point3(ndc.extend(1.0));
        (near, (far - near).normalize())
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

        let view = tex.texture.create_view(&Default::default());
        // egui blends in gamma space: give it a non-sRGB view of the same surface.
        let ui_view = tex.texture.create_view(&wgpu::TextureViewDescriptor { format: Some(self.config.format.remove_srgb_suffix()), ..Default::default() });
        let cmd = self.encode(&view, &ui_view, None, self.size());

        self.queue.submit([cmd]);
        self.queue.present(tex);
        Ok(())
    }

    /// Renders one frame offscreen at `width`x`height` and returns it as RGBA8 (sRGB).
    pub fn capture(&mut self, width: u32, height: u32) -> anyhow::Result<Rgba> {
        let mut config = self.config.clone();
        config.width = width;
        config.height = height;
        let targets = Self::make_targets(&self.device, &config, self.samples);
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("capture"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: config.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[config.format.remove_srgb_suffix()],
        });
        let view = texture.create_view(&Default::default());
        let ui_view = texture.create_view(&wgpu::TextureViewDescriptor { format: Some(config.format.remove_srgb_suffix()), ..Default::default() });
        let cmd = self.encode(&view, &ui_view, Some(&targets), (width, height));


        // Rows must be padded to 256 bytes for buffer copies.
        let row = (width * 4).div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("capture readback"),
            size: (row * height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(height) },
            },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        self.queue.submit([cmd, enc.finish()]);

        let slice = buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device.poll(wgpu::PollType::wait_indefinitely())?;
        rx.recv()??;
        let data = slice.get_mapped_range()?;
        let bgra = matches!(config.format, wgpu::TextureFormat::Bgra8UnormSrgb | wgpu::TextureFormat::Bgra8Unorm);
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height as usize {
            for px in data[y * row as usize..][..width as usize * 4].chunks_exact(4) {
                pixels.extend(if bgra { [px[2], px[1], px[0], 255] } else { [px[0], px[1], px[2], 255] });
            }
        }
        Ok(Rgba { width, height, pixels })
    }

    /// Records the whole frame into `view` using `targets` (depth / MSAA of the same size).
    fn encode(&mut self, view: &wgpu::TextureView, ui_view: &wgpu::TextureView, targets: Option<&Targets>, size: (u32, u32)) -> wgpu::CommandBuffer {
        let targets = targets.unwrap_or(&self.targets);
        let aspect = size.0 as f32 / size.1 as f32;
        let cam = &self.camera;
        let globals = Globals {
            view_proj: cam.view_proj(aspect).to_cols_array_2d(),
            focus: [cam.target.x, CELL, cam.target.z, cam.distance * 4.0],
            eye: cam.eye().extend(1.0).to_array(),
            light_dir: glam::Vec3::from(self.lighting.direction).normalize_or_zero().extend(0.0).to_array(),
            ambient: glam::Vec3::from(self.lighting.ambient).extend(0.0).to_array(),
            diffuse: glam::Vec3::from(self.lighting.diffuse).extend(0.0).to_array(),
            params: {
                // Clip units per sprite pixel at w = 1 (horizontal, vertical).
                let f = 1.0 / (cam.fovy / 2.0).tan();
                let s = CELL / sprite::PIXELS_PER_CELL;
                [self.started.elapsed().as_secs_f32(), s * f / aspect, s * f, 0.0]
            },
        };
        self.queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));

        let (color, resolve) = match &targets.msaa {
            Some(m) => (m, Some(view)),
            None => (view, None),
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
                    view: &targets.depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &self.globals_bind, &[]);
            match &self.scene {
                Some(t) => {
                    pass.set_pipeline(&self.scene_pipeline);
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
            // Transparent passes after all opaque geometry.
            if let Some(w) = &self.water {
                pass.set_pipeline(&self.water_pipeline);
                pass.set_bind_group(1, &w.frames[w.frame(globals.params[0])], &[]);
                pass.set_vertex_buffer(0, w.vertices.slice(..));
                pass.draw(0..w.count, 0..1);
            }
            // Warp portals (additive), then actors.
            if let (false, Some(bind)) = (self.warps.is_empty(), self.warp_texture.as_ref()) {
                let verts = effect::warp_portals(&self.warps, globals.params[0]);
                let buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("effects"),
                    contents: bytemuck::cast_slice(&verts),
                    usage: wgpu::BufferUsages::VERTEX,
                });
                pass.set_pipeline(&self.effect_pipeline);
                pass.set_bind_group(1, bind, &[]);
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(0..verts.len() as u32, 0..1);
            }
            // Actors back to front (by distance from the eye) so blended edges composite.
            if !self.actors.is_empty() {
                let eye = cam.eye();
                let mut order: Vec<&Actor> = self.actors.iter().collect();
                order.sort_by(|a, b| b.pos.distance_squared(eye).total_cmp(&a.pos.distance_squared(eye)));
                let mut vertices = vec![];
                let mut draws = vec![];
                for a in order {
                    for (sheet, v) in sprite::actor_quads(&self.sheets, a) {
                        let start = vertices.len() as u32;
                        vertices.extend(v);
                        draws.push((sheet, start..vertices.len() as u32));
                    }
                }
                if !vertices.is_empty() {
                    let buffer = sprite::vertex_buffer(&self.device, &vertices);
                    pass.set_pipeline(&self.sprite_pipeline);
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    for (sheet, range) in draws {
                        pass.set_bind_group(1, &self.sheets[sheet].bind, &[]);
                        pass.draw(range, 0..1);
                    }
                }
            }
            // Code-drawn skill effects (additive, world space).
            if !self.procs.is_empty() {
                let mut vertices = vec![];
                let mut draws = vec![];
                for p in &self.procs {
                    if p.tex.0 >= self.effect_textures.len() {
                        continue;
                    }
                    let start = vertices.len() as u32;
                    p.vertices(&mut vertices);
                    draws.push((p.tex.0, start..vertices.len() as u32));
                }
                if !vertices.is_empty() {
                    let buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("skill effects"),
                        contents: bytemuck::cast_slice(&vertices),
                        usage: wgpu::BufferUsages::VERTEX,
                    });
                    pass.set_pipeline(&self.effect_pipeline);
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    for (tex, range) in draws {
                        pass.set_bind_group(1, &self.effect_textures[tex], &[]);
                        pass.draw(range, 0..1);
                    }
                }
            }
            // Skill effects over everything else in the world.
            if !self.effects.is_empty() {
                let mut vertices = vec![];
                let mut draws = vec![];
                for e in &self.effects {
                    let Some(asset) = self.fx_assets.get(e.asset.0) else { continue };
                    let start = vertices.len() as u32;
                    asset.vertices(e, &mut vertices);
                    if vertices.len() as u32 > start {
                        draws.push((e.asset.0, start..vertices.len() as u32));
                    }
                }
                if !vertices.is_empty() {
                    let buffer = sprite::vertex_buffer(&self.device, &vertices);
                    pass.set_pipeline(&self.fx_pipeline);
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    for (asset, range) in draws {
                        pass.set_bind_group(1, &self.fx_assets[asset].bind, &[]);
                        pass.draw(range, 0..1);
                    }
                }
            }
        }

        // UI on top of the resolved frame.
        if let Some(ui) = self.ui.take() {
            for (id, delta) in &ui.textures.set {
                for d in delta {
                    self.ui_renderer.update_texture(&self.device, &self.queue, *id, d);
                }
            }
            let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: [size.0, size.1], pixels_per_point: ui.pixels_per_point };
            self.ui_renderer.update_buffers(&self.device, &self.queue, &mut enc, &ui.primitives, &screen);
            {
                let mut pass = enc
                    .begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("ui"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: ui_view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                        })],
                        ..Default::default()
                    })
                    .forget_lifetime();
                self.ui_renderer.render(&mut pass, &ui.primitives, &screen);
            }
            for id in &ui.textures.free {
                self.ui_renderer.free_texture(id);
            }
        }
        enc.finish()
    }

    /// Supplies the UI drawn over the next frame.
    pub fn set_ui(&mut self, ui: UiFrame) {
        // Texture changes (e.g. the font atlas) from frames that were never drawn must still
        // reach the GPU, so pending ones are carried over.
        let mut ui = ui;
        if let Some(mut old) = self.ui.take() {
            old.textures.append(ui.textures);
            ui.textures = old.textures;
        }
        self.ui = Some(ui);
    }

    /// Window pixel position of a world point, or None when behind the camera.
    pub fn project(&self, p: glam::Vec3, size: (u32, u32)) -> Option<glam::Vec2> {
        let (w, h) = (size.0 as f32, size.1 as f32);
        let clip = self.camera.view_proj(w / h) * p.extend(1.0);
        if clip.w <= 0.0 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        Some(glam::Vec2::new((ndc.x + 1.0) * 0.5 * w, (1.0 - ndc.y) * 0.5 * h))
    }
}

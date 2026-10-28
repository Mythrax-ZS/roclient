//! HD-2D post-processing. The world is drawn into an HDR target; this chain adds bloom,
//! tilt-shift depth of field (the "diorama" blur toward the top and bottom of the screen),
//! a soft highlight shoulder, colour grading and a vignette, then writes the result to the
//! swapchain format. The UI is drawn afterwards, untouched.

/// Scene colour format: linear, with headroom above 1.0 for bloom.
pub const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Artist-facing look controls. `enabled = false` gives the classic, unprocessed look.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PostSettings {
    pub enabled: bool,
    /// Brightness above which things glow, and how much they glow.
    pub bloom_threshold: f32,
    pub bloom_intensity: f32,
    /// 0..1: how blurred the top and bottom of the screen get.
    pub dof_strength: f32,
    /// Half-height (fraction of the screen) of the sharp band around the player.
    pub dof_band: f32,
    pub vignette: f32,
    pub exposure: f32,
    pub contrast: f32,
    pub saturation: f32,
    /// -1 cool .. 1 warm.
    pub warmth: f32,
    /// 0..1: how dark sprite shadows on the ground are (0 turns them off).
    pub shadow_strength: f32,
    /// Shadow edge blur in shadow-map texels.
    pub shadow_softness: f32,
    /// Sun height for sprite shadows in degrees (lower = longer shadows); 0 uses the map's.
    pub sun_elevation: f32,
}

impl Default for PostSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            bloom_threshold: 0.92,
            bloom_intensity: 0.22,
            dof_strength: 0.85,
            dof_band: 0.16,
            vignette: 0.45,
            exposure: 1.0,
            contrast: 1.12,
            saturation: 1.1,
            warmth: 0.35,
            shadow_strength: 0.7,
            shadow_softness: 1.5,
            sun_elevation: 35.0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    texel: [f32; 4],
    bloom: [f32; 4],
    dof: [f32; 4],
    grade: [f32; 4],
    misc: [f32; 4],
}

/// Intermediate textures, sized from the window.
pub struct PostTargets {
    /// The resolved scene (render target + sampled).
    pub hdr: wgpu::TextureView,
    half_bloom: wgpu::TextureView,
    half_dof: wgpu::TextureView,
    bloom: [wgpu::TextureView; 2],
    dof: [wgpu::TextureView; 2],
    size: (u32, u32),
}

fn target(device: &wgpu::Device, label: &str, w: u32, h: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: HDR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

impl PostTargets {
    pub fn new(device: &wgpu::Device, w: u32, h: u32) -> Self {
        let (hw, hh, qw, qh) = (w / 2, h / 2, w / 4, h / 4);
        Self {
            hdr: target(device, "hdr scene", w, h),
            half_bloom: target(device, "bloom half", hw, hh),
            half_dof: target(device, "dof half", hw, hh),
            bloom: [target(device, "bloom a", qw, qh), target(device, "bloom b", qw, qh)],
            dof: [target(device, "dof a", qw, qh), target(device, "dof b", qw, qh)],
            size: (w.max(1), h.max(1)),
        }
    }
}

pub struct Post {
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    down: wgpu::RenderPipeline,
    bright: wgpu::RenderPipeline,
    blur: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    /// Plain copy to the output when post-processing is off.
    copy: wgpu::RenderPipeline,
    /// One uniform buffer per pass (each has its own texel size / direction).
    uniforms: Vec<wgpu::Buffer>,
}

const PASSES: usize = 10;

impl Post {
    pub fn new(device: &wgpu::Device, output: wgpu::TextureFormat) -> Self {
        let tex = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post"),
            entries: &[
                tex(0),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                tex(3),
                tex(4),
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("post"), source: wgpu::ShaderSource::Wgsl(include_str!("post.wgsl").into()) });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("post"), bind_group_layouts: &[Some(&layout)], ..Default::default() });
        let pipe = |entry: &str, format| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pl),
                vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_main"), compilation_options: Default::default(), buffers: &[] },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("post"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let uniforms = (0..PASSES)
            .map(|_| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("post params"),
                    size: std::mem::size_of::<Params>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            })
            .collect();
        Self {
            down: pipe("fs_down", HDR_FORMAT),
            bright: pipe("fs_bright", HDR_FORMAT),
            blur: pipe("fs_blur", HDR_FORMAT),
            composite: pipe("fs_composite", output),
            copy: pipe("fs_down", output),
            layout,
            sampler,
            uniforms,
        }
    }

    /// Runs the chain from `t.hdr` into `out`.
    pub fn record(&self, device: &wgpu::Device, queue: &wgpu::Queue, enc: &mut wgpu::CommandEncoder, t: &PostTargets, out: &wgpu::TextureView, s: &PostSettings) {
        let (w, h) = (t.size.0 as f32, t.size.1 as f32);
        let base = Params {
            bloom: [s.bloom_threshold, 0.5, s.bloom_intensity, 0.0],
            // The camera keeps the player at the centre; their body sits a little above it.
            dof: [s.dof_strength, 0.47, s.dof_band, 0.22],
            grade: [s.exposure, s.contrast, s.saturation, s.warmth],
            misc: [s.vignette, w / h, 1.0, 0.0],
            ..Default::default()
        };
        let mut n = 0;
        let mut pass = |enc: &mut wgpu::CommandEncoder, pipe: &wgpu::RenderPipeline, src: &wgpu::TextureView, dst: &wgpu::TextureView, texel: [f32; 4], extra: [&wgpu::TextureView; 2]| {
            let buf = &self.uniforms[n];
            n += 1;
            queue.write_buffer(buf, 0, bytemuck::bytes_of(&Params { texel, ..base }));
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("post"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(src) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                    wgpu::BindGroupEntry { binding: 2, resource: buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(extra[0]) },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(extra[1]) },
                ],
            });
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("post"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: dst,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                ..Default::default()
            });
            rp.set_pipeline(pipe);
            rp.set_bind_group(0, &bind, &[]);
            rp.draw(0..3, 0..1);
        };
        if !s.enabled {
            // Same-size copy: half-texel offsets land on texel centres.
            pass(enc, &self.copy, &t.hdr, out, [0.0, 0.0, 0.0, 0.0], [&t.hdr, &t.hdr]);
            return;
        }
        let full = [1.0 / w, 1.0 / h, 0.0, 0.0];
        let (hw, hh) = (2.0 / w, 2.0 / h);
        let (qw, qh) = (4.0 / w, 4.0 / h);
        // Bloom: bright pass to half, down to quarter, blur twice.
        pass(enc, &self.bright, &t.hdr, &t.half_bloom, full, [&t.hdr, &t.hdr]);
        pass(enc, &self.down, &t.half_bloom, &t.bloom[0], [hw, hh, 0.0, 0.0], [&t.hdr, &t.hdr]);
        pass(enc, &self.blur, &t.bloom[0], &t.bloom[1], [qw, qh, qw, 0.0], [&t.hdr, &t.hdr]);
        pass(enc, &self.blur, &t.bloom[1], &t.bloom[0], [qw, qh, 0.0, qh], [&t.hdr, &t.hdr]);
        // Depth of field source: the whole scene, softly blurred at quarter size.
        pass(enc, &self.down, &t.hdr, &t.half_dof, full, [&t.hdr, &t.hdr]);
        pass(enc, &self.down, &t.half_dof, &t.dof[0], [hw, hh, 0.0, 0.0], [&t.hdr, &t.hdr]);
        pass(enc, &self.blur, &t.dof[0], &t.dof[1], [qw, qh, qw, 0.0], [&t.hdr, &t.hdr]);
        pass(enc, &self.blur, &t.dof[1], &t.dof[0], [qw, qh, 0.0, qh], [&t.hdr, &t.hdr]);
        pass(enc, &self.composite, &t.hdr, out, full, [&t.dof[0], &t.bloom[0]]);
    }
}

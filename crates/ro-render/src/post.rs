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
    /// Minutes for the sun to cross the sky outdoors, from the map's own bearing swung
    /// 70° either way (0 keeps it still). It sets with the server's night and rises at day.
    pub sun_cycle_minutes: f32,
    /// 0..1: how much map light, sun shadow and point lights tint sprites.
    pub sprite_light: f32,
    /// Sun-coloured rim on sprite edges facing the sun.
    pub rim_light: f32,
    /// Brightness of map light sources (torches, lamps).
    pub point_lights: f32,
    /// Distance haze toward the far parts of the view (0 off).
    pub fog: f32,
    /// Light shafts streaming from the sun.
    pub god_rays: f32,
    /// Ambient occlusion in creases and under overhangs.
    pub ambient_occlusion: f32,
    /// Sprites snap to whole screen pixels (crisp pixel art).
    pub pixel_snap: bool,
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
            sun_cycle_minutes: 60.0,
            sprite_light: 0.8,
            rim_light: 0.35,
            point_lights: 1.0,
            fog: 0.35,
            god_rays: 0.25,
            ambient_occlusion: 0.6,
            pixel_snap: true,
        }
    }
}


/// Per-pixel depth info written by the world and sprites: x linear distance from the eye,
/// y 1 on sprites. Resolved from MSAA and sampled by AO, fog and depth of field.
pub const INFO_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Float;

#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    texel: [f32; 4],
    bloom: [f32; 4],
    dof: [f32; 4],
    grade: [f32; 4],
    misc: [f32; 4],
    /// x: focus distance, y: depth blur range, z: fog start, w: fog end
    depth: [f32; 4],
    /// rgb: fog colour, a: fog strength
    fog: [f32; 4],
    /// xy: sun position (uv), z: ray intensity, w: 1 when the sun is in front of the camera
    rays: [f32; 4],
    /// x: AO strength, y: sample radius (pixels at the focus distance)
    ao: [f32; 4],
}

/// What the post chain needs from the frame besides the settings.
pub struct FrameInfo {
    /// Distance from the eye to the player (the focal plane).
    pub focus: f32,
    /// Haze colour (from the map's ambient light).
    pub fog_color: [f32; 3],
    /// Where the sun sits on screen (uv), if in front of the camera.
    pub sun_uv: Option<[f32; 2]>,
}

/// Intermediate textures, sized from the window.
pub struct PostTargets {
    /// The resolved scene (render target + sampled).
    pub hdr: wgpu::TextureView,
    /// Resolved depth info (see INFO_FORMAT).
    pub info: wgpu::TextureView,
    half_bloom: wgpu::TextureView,
    half_dof: wgpu::TextureView,
    ao: wgpu::TextureView,
    bloom: [wgpu::TextureView; 2],
    dof: [wgpu::TextureView; 2],
    rays: wgpu::TextureView,
    size: (u32, u32),
}

fn target(device: &wgpu::Device, label: &str, w: u32, h: u32, format: wgpu::TextureFormat) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

impl PostTargets {
    pub fn new(device: &wgpu::Device, w: u32, h: u32) -> Self {
        let (hw, hh, qw, qh) = (w / 2, h / 2, w / 4, h / 4);
        let hdr = |label, w, h| target(device, label, w, h, HDR_FORMAT);
        Self {
            hdr: hdr("hdr scene", w, h),
            info: target(device, "depth info", w, h, INFO_FORMAT),
            half_bloom: hdr("bloom half", hw, hh),
            half_dof: hdr("dof half", hw, hh),
            ao: hdr("ambient occlusion", hw, hh),
            bloom: [hdr("bloom a", qw, qh), hdr("bloom b", qw, qh)],
            dof: [hdr("dof a", qw, qh), hdr("dof b", qw, qh)],
            rays: hdr("god rays", qw, qh),
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
    ao: wgpu::RenderPipeline,
    rays: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    /// Plain copy to the output when post-processing is off.
    copy: wgpu::RenderPipeline,
    /// One uniform buffer per pass (each has its own texel size / direction).
    uniforms: Vec<wgpu::Buffer>,
}

const PASSES: usize = 12;

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
                tex(5),
                tex(6),
                tex(7),
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
            ao: pipe("fs_ao", HDR_FORMAT),
            rays: pipe("fs_rays", HDR_FORMAT),
            composite: pipe("fs_composite", output),
            copy: pipe("fs_down", output),
            layout,
            sampler,
            uniforms,
        }
    }

    /// Runs the chain from `t.hdr` (and `t.info`) into `out`.
    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        enc: &mut wgpu::CommandEncoder,
        t: &PostTargets,
        out: &wgpu::TextureView,
        s: &PostSettings,
        frame: &FrameInfo,
    ) {
        let (w, h) = (t.size.0 as f32, t.size.1 as f32);
        let base = Params {
            bloom: [s.bloom_threshold, 0.5, s.bloom_intensity, 0.0],
            // The camera keeps the player at the centre; their body sits a little above it.
            dof: [s.dof_strength, 0.47, s.dof_band, 0.22],
            grade: [s.exposure, s.contrast, s.saturation, s.warmth],
            misc: [s.vignette, w / h, 1.0, 0.0],
            depth: [frame.focus, frame.focus * 0.6, frame.focus * 1.15, frame.focus * 2.6],
            fog: [frame.fog_color[0], frame.fog_color[1], frame.fog_color[2], s.fog],
            rays: match frame.sun_uv {
                Some(uv) => [uv[0], uv[1], s.god_rays, 1.0],
                None => [0.5, -0.5, 0.0, 0.0],
            },
            ao: [s.ambient_occlusion, 14.0 * h / 1080.0, 0.0, 0.0],
            ..Default::default()
        };
        let mut n = 0;
        let mut pass = |enc: &mut wgpu::CommandEncoder, pipe: &wgpu::RenderPipeline, src: &wgpu::TextureView, dst: &wgpu::TextureView, texel: [f32; 4], extra: [&wgpu::TextureView; 5]| {
            let buf = &self.uniforms[n];
            n += 1;
            queue.write_buffer(buf, 0, bytemuck::bytes_of(&Params { texel, ..base }));
            let view = |v| wgpu::BindingResource::TextureView(v);
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("post"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: view(src) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                    wgpu::BindGroupEntry { binding: 2, resource: buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: view(extra[0]) },
                    wgpu::BindGroupEntry { binding: 4, resource: view(extra[1]) },
                    wgpu::BindGroupEntry { binding: 5, resource: view(extra[2]) },
                    wgpu::BindGroupEntry { binding: 6, resource: view(extra[3]) },
                    wgpu::BindGroupEntry { binding: 7, resource: view(extra[4]) },
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
        // Textures a pass does not read: any valid view of the right kind.
        let none = [&t.hdr, &t.hdr, &t.info, &t.hdr, &t.hdr];
        if !s.enabled {
            pass(enc, &self.copy, &t.hdr, out, [0.0; 4], none);
            return;
        }
        let full = [1.0 / w, 1.0 / h, 0.0, 0.0];
        let (hw, hh) = (2.0 / w, 2.0 / h);
        let (qw, qh) = (4.0 / w, 4.0 / h);
        // Bloom: bright pass to half, down to quarter, blur twice.
        pass(enc, &self.bright, &t.hdr, &t.half_bloom, full, none);
        pass(enc, &self.down, &t.half_bloom, &t.bloom[0], [hw, hh, 0.0, 0.0], none);
        // God rays: march the bright pass toward the sun.
        pass(enc, &self.rays, &t.bloom[0], &t.rays, [qw, qh, 0.0, 0.0], none);
        pass(enc, &self.blur, &t.bloom[0], &t.bloom[1], [qw, qh, qw, 0.0], none);
        pass(enc, &self.blur, &t.bloom[1], &t.bloom[0], [qw, qh, 0.0, qh], none);
        // Depth of field source: the whole scene, softly blurred at quarter size.
        pass(enc, &self.down, &t.hdr, &t.half_dof, full, none);
        pass(enc, &self.down, &t.half_dof, &t.dof[0], [hw, hh, 0.0, 0.0], none);
        pass(enc, &self.blur, &t.dof[0], &t.dof[1], [qw, qh, qw, 0.0], none);
        pass(enc, &self.blur, &t.dof[1], &t.dof[0], [qw, qh, 0.0, qh], none);
        // Ambient occlusion from the depth info, at half size.
        pass(enc, &self.ao, &t.hdr, &t.ao, full, none);
        pass(enc, &self.composite, &t.hdr, out, full, [&t.dof[0], &t.bloom[0], &t.info, &t.ao, &t.rays]);
    }
}

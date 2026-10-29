//! Map particle emitters: RSW effect objects with id 974 (EF_EMITTER), defined per map in
//! `data\luafiles514\lua files\effecttool\<map>.lub` (`_<map>_emitterInfo`, the Nth 974
//! object takes entry N). Smoke, steam, sparks, fireflies, falling leaves, glowing dust.
//!
//! Simulation follows BrowEdit 3's reading of the format: particles spawn in a box of
//! `radius` around the emitter, fly with a velocity picked between `dir1` and `dir2`, fall
//! with `gravity` (both scaled by `speed`), live `life` seconds fading in and out, and are
//! drawn `size` across as camera-facing quads (or fixed quads turned by `rotate_angle` when
//! `billboard_off`), blended with the map's Direct3D source/destination modes.

use std::collections::HashMap;

use glam::{Mat3, Vec3};

use crate::effect::{TexId, Vertex};

/// One emitter's definition (lub units: RO y-down; colours 0..1).
#[derive(Clone, Debug)]
pub struct EmitterDef {
    pub dir1: [f32; 3],
    pub dir2: [f32; 3],
    pub gravity: [f32; 3],
    pub radius: [f32; 3],
    pub color: [f32; 4],
    /// Particles per second (min, max).
    pub rate: [f32; 2],
    pub size: [f32; 2],
    pub life: [f32; 2],
    pub speed: f32,
    /// Direct3D blend factors (D3DBLEND numbers).
    pub src: u8,
    pub dst: u8,
    pub max: u32,
    pub zenable: bool,
    pub billboard: bool,
    /// Degrees, used when not a billboard.
    pub rotate: [f32; 3],
    pub tex: TexId,
    /// Animated emitters (EF_ANIMATED_EMITTER): frame sets; each particle picks one and
    /// steps a frame every 0.13 s. Empty: every particle shows `tex`.
    pub frames: Vec<Vec<TexId>>,
}

/// An emitter placed in the world (y-up render space).
#[derive(Clone, Debug)]
pub struct Emitter {
    pub pos: Vec3,
    pub def: EmitterDef,
}

struct Particle {
    start: Vec3,
    vel: Vec3,
    born: f32,
    life: f32,
    size: f32,
    /// Frame set and start offset (s) for animated emitters.
    set: usize,
    phase: f32,
}

struct Live {
    e: Emitter,
    particles: Vec<Particle>,
    next: f32,
}

type Key = (u8, u8, bool);

pub(crate) struct Particles {
    shader: wgpu::ShaderModule,
    layout: wgpu::PipelineLayout,
    samples: u32,
    pipelines: HashMap<Key, wgpu::RenderPipeline>,
    live: Vec<Live>,
    rng: u64,
}

/// D3DBLEND -> wgpu (BrowEdit's table; 12/13 differ between source and destination).
fn factor(v: u8, dst: bool) -> wgpu::BlendFactor {
    use wgpu::BlendFactor as F;
    match v {
        1 => F::Zero,
        2 => F::One,
        3 => F::Src,
        4 => F::OneMinusSrc,
        5 => F::SrcAlpha,
        6 => F::OneMinusSrcAlpha,
        7 => F::DstAlpha,
        8 => F::OneMinusDstAlpha,
        9 => F::Dst,
        10 => F::OneMinusDst,
        11 => F::SrcAlphaSaturated,
        12 if dst => F::OneMinusSrcAlpha,
        12 => F::SrcAlpha,
        13 if dst => F::SrcAlpha,
        13 => F::OneMinusSrcAlpha,
        _ => F::Zero,
    }
}

impl Particles {
    pub fn new(device: &wgpu::Device, globals: &wgpu::BindGroupLayout, texture: &wgpu::BindGroupLayout, samples: u32) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("particles"), source: wgpu::ShaderSource::Wgsl(include_str!("particle.wgsl").into()) });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("particles"), bind_group_layouts: &[Some(globals), Some(texture)], ..Default::default() });
        Self { shader, layout, samples, pipelines: HashMap::new(), live: vec![], rng: 0x9e3779b97f4a7c15 }
    }

    fn pipeline(&self, device: &wgpu::Device, (src, dst, z): Key) -> wgpu::RenderPipeline {
        let blend = wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: factor(src, false), dst_factor: factor(dst, true), operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent::OVER,
        };
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("particles"),
            layout: Some(&self.layout),
            vertex: wgpu::VertexState { module: &self.shader, entry_point: Some("vs_main"), compilation_options: Default::default(), buffers: &[Some(Vertex::layout())] },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: crate::DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(if z { wgpu::CompareFunction::Less } else { wgpu::CompareFunction::Always }),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState { count: self.samples, ..Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &self.shader,
                entry_point: Some(if dst == 2 { "fs_add" } else { "fs_main" }),
                compilation_options: Default::default(),
                targets: &[
                    Some(wgpu::ColorTargetState { format: crate::post::HDR_FORMAT, blend: Some(blend), write_mask: wgpu::ColorWrites::ALL }),
                    Some(wgpu::ColorTargetState { format: crate::post::INFO_FORMAT, blend: None, write_mask: wgpu::ColorWrites::empty() }),
                ],
            }),
            multiview_mask: None,
            cache: None,
        })
    }

    /// Replaces the map's emitters (pre-warmed, so a map never starts empty) and builds a
    /// pipeline for each blend they use.
    pub fn set(&mut self, device: &wgpu::Device, emitters: Vec<Emitter>, now: f32) {
        self.live = emitters.into_iter().map(|e| Live { e, particles: vec![], next: now }).collect();
        for i in 0..self.live.len() {
            let d = &self.live[i].e.def;
            let key = (d.src, d.dst, d.zenable);
            if !self.pipelines.contains_key(&key) {
                let p = self.pipeline(device, key);
                self.pipelines.insert(key, p);
            }
            self.warm(i, now);
        }
    }

    /// Fills emitter `i` with the particles it would have in steady state (random ages), for
    /// map entry and after any gap in updates (minimised window, far away, a slow frame).
    fn warm(&mut self, i: usize, now: f32) {
        self.live[i].particles.clear();
        let d = &self.live[i].e.def;
        let count = (d.max as f32).min(avg(d.rate) * avg(d.life)).ceil() as usize;
        for _ in 0..count {
            let age = self.rand() * avg(self.live[i].e.def.life);
            let p = self.spawn(i, now - age);
            if now - p.born < p.life {
                self.live[i].particles.push(p);
            }
        }
        self.live[i].next = now;
    }

    fn rand(&mut self) -> f32 {
        // xorshift64*
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        ((self.rng.wrapping_mul(0x2545f4914f6cdd1d) >> 40) as f32) / (1u64 << 24) as f32
    }

    fn spawn(&mut self, i: usize, born: f32) -> Particle {
        let d = self.live[i].e.def.clone();
        let mut lerp = |a: f32, b: f32| a + self.rand() * (b - a);
        // Lub vectors are RO y-down; render space is y-up.
        let start = Vec3::new(lerp(-d.radius[0], d.radius[0]), lerp(-d.radius[1], d.radius[1]), lerp(-d.radius[2], d.radius[2]));
        // BrowEdit mirrors z relative to RO render space, so z flips too.
        let vel = Vec3::new(lerp(d.dir1[0], d.dir2[0]), -lerp(d.dir1[1], d.dir2[1]), -lerp(d.dir1[2], d.dir2[2]));
        let life = lerp(d.life[0], d.life[1]).max(0.05);
        let size = lerp(d.size[0], d.size[1]);
        let set = if d.frames.is_empty() { 0 } else { ((self.rand() * d.frames.len() as f32) as usize).min(d.frames.len() - 1) };
        let phase = self.rand() * 2.0;
        Particle { start, vel, born, life, size, set, phase }
    }

    /// Ages particles and emits new ones near `focus` (render space), within `range`.
    pub fn update(&mut self, now: f32, focus: Vec3, range: f32) {
        for i in 0..self.live.len() {
            let near = self.live[i].e.pos.distance(focus) < range;
            self.live[i].particles.retain(|p| now - p.born < p.life);
            if !near {
                self.live[i].next = now;
                continue;
            }
            if now - self.live[i].next > 1.0 {
                self.warm(i, now);
                continue;
            }
            let mut budget = 4;
            while now >= self.live[i].next && budget > 0 && (self.live[i].particles.len() as u32) < self.live[i].e.def.max {
                let p = self.spawn(i, self.live[i].next);
                self.live[i].particles.push(p);
                let rate = self.live[i].e.def.rate;
                let r = (rate[0] + self.rand() * (rate[1] - rate[0])).max(0.05);
                self.live[i].next += 1.0 / r;
                budget -= 1;
            }
        }
    }

    /// Quads for this frame, grouped by (blend, texture).
    pub fn vertices(&self, now: f32, eye: Vec3, focus: Vec3, range: f32) -> Vec<(Key, TexId, Vec<Vertex>)> {
        let mut groups: HashMap<(Key, usize), Vec<Vertex>> = HashMap::new();
        for l in &self.live {
            if l.particles.is_empty() || l.e.pos.distance(focus) > range {
                continue;
            }
            let d = &l.e.def;
            let base = l.e.pos + Vec3::new(0.0, 10.0, 0.0);
            let grav = Vec3::new(d.gravity[0], -d.gravity[1], -d.gravity[2]);
            let fixed = Mat3::from_rotation_z(d.rotate[2].to_radians()) * Mat3::from_rotation_y(-d.rotate[1].to_radians()) * Mat3::from_rotation_x(d.rotate[0].to_radians());
            for p in &l.particles {
                let t = now - p.born;
                let pos = base + p.start + p.vel * d.speed * t + grav * d.speed * t * t * 0.5;
                // Fade in over the first second and out over the last (or a share of a
                // short life), as the original.
                let edge = (p.life * 0.5).min(1.0);
                let fade = (t / edge).min((p.life - t) / edge).clamp(0.0, 1.0);
                let color = [d.color[0], d.color[1], d.color[2], d.color[3] * fade];
                let (right, up) = if d.billboard {
                    let fwd = (pos - eye).normalize_or_zero();
                    let right = fwd.cross(Vec3::Y).normalize_or(Vec3::X);
                    (right, right.cross(fwd))
                } else {
                    (fixed * Vec3::X, fixed * Vec3::Y)
                };
                let (a, b) = (right * p.size, up * p.size);
                let v = |q: Vec3, u: f32, vv: f32| Vertex { pos: q.into(), uv: [u, vv], color };
                let (p0, p1, p2, p3) = (v(pos - a - b, 0.0, 1.0), v(pos + a - b, 1.0, 1.0), v(pos + a + b, 1.0, 0.0), v(pos - a + b, 0.0, 0.0));
                let tex = match d.frames.get(p.set) {
                    Some(f) if !f.is_empty() => f[((t + p.phase) / 0.13) as usize % f.len()],
                    _ => d.tex,
                };
                groups.entry(((d.src, d.dst, d.zenable), tex.0)).or_default().extend([p0, p1, p2, p0, p2, p3]);
            }
        }
        groups.into_iter().map(|((k, t), v)| (k, TexId(t), v)).collect()
    }

    pub fn pipeline_for(&self, key: &Key) -> Option<&wgpu::RenderPipeline> {
        self.pipelines.get(key)
    }

    pub fn is_empty(&self) -> bool {
        self.live.is_empty()
    }
}

fn avg(v: [f32; 2]) -> f32 {
    (v[0] + v[1]) * 0.5
}

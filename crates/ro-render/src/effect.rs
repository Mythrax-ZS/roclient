//! Code-drawn 3D effects: the warp portal (counter-rotating cylinders textured with
//! effect\ring_blue.tga) and skill effects the original client also builds in code — light
//! columns (Heal, Increase AGI, Blessing) and spinning ground circles (casting). All are
//! additively blended and rebuilt each frame (a few hundred vertices).

use glam::Vec3;

use crate::CELL;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    /// RGB tint and strength.
    pub color: [f32; 4],
}

impl Vertex {
    const ATTRS: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x4];

    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}

const SEGMENTS: usize = 32;

/// An open cylinder (cone frustum) whose texture scrolls around it by `spin` turns.
fn cylinder(out: &mut Vec<Vertex>, base: Vec3, bottom: f32, top: f32, height: f32, spin: f32, color: [f32; 4]) {
    for i in 0..SEGMENTS {
        let (a0, a1) = (i as f32 / SEGMENTS as f32, (i + 1) as f32 / SEGMENTS as f32);
        let ring = |t: f32, r: f32, y: f32| {
            let ang = t * std::f32::consts::TAU;
            base + Vec3::new(ang.cos() * r, y, ang.sin() * r)
        };
        // The texture wraps twice around the cylinder.
        let (u0, u1) = (a0 * 2.0 + spin, a1 * 2.0 + spin);
        let v = |p: Vec3, u: f32, vv: f32| Vertex { pos: p.into(), uv: [u, vv], color };
        let (b0, b1, t0, t1) = (ring(a0, bottom, 0.0), ring(a1, bottom, 0.0), ring(a0, top, height), ring(a1, top, height));
        out.extend([v(b0, u0, 1.0), v(b1, u1, 1.0), v(t1, u1, 0.0), v(b0, u0, 1.0), v(t1, u1, 0.0), v(t0, u0, 0.0)]);
    }
}

/// Geometry for warp portals at `positions` (ground points) at time `t` seconds.
pub fn warp_portals(positions: &[Vec3], t: f32) -> Vec<Vertex> {
    let mut out = Vec::with_capacity(positions.len() * SEGMENTS * 12);
    for &p in positions {
        cylinder(&mut out, p, 1.7 * CELL, 2.2 * CELL, 1.4 * CELL, t * 0.25, [1.0, 1.0, 1.0, 0.9]);
        cylinder(&mut out, p, 1.1 * CELL, 1.5 * CELL, 2.6 * CELL, -t * 0.4, [1.0, 1.0, 1.0, 0.7]);
    }
    out
}

/// A texture uploaded for code-drawn effects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TexId(pub usize);

#[derive(Clone, Copy, Debug)]
pub enum Shape {
    /// A column of light that shoots up and fades (radius and full height in cells).
    Column { radius: f32, height: f32 },
    /// A flat circle on the ground, turning (radius in cells, turns per second).
    Disc { radius: f32, spin: f32 },
    /// Spikes erupting in a wave along `reach` (world offset from `pos`), like Grimtooth.
    Spikes { reach: Vec3, count: u32 },
    /// A shockwave wall expanding to `radius` cells.
    Wave { radius: f32 },
    /// A glowing ball flying along `reach` and bursting at its end.
    Orb { reach: Vec3 },
    /// A camera-facing crescent (effect\purpleslash.tga) flying along `travel`, its curved
    /// edge leading, growing from `size.0` to `size.1` cells (Soul Breaker, Meteor Assault).
    Slash { travel: Vec3, size: (f32, f32) },
    /// A camera-facing patch of motes (effect\pok1.bmp) drifting along `drift` and slowly
    /// turning, `size` cells across (map fireflies).
    Motes { drift: Vec3, size: f32 },
    /// A flat rectangle on the ground, `size` cells across (skill areas being cast).
    Area { size: (f32, f32) },
    /// One layer of the level 99 aura, held under its owner while it stands in view
    /// (`time_ms` runs from when it appeared; `seed` varies the sparkles between owners).
    Aura { layer: AuraLayer, seed: u32, style: AuraStyle },
}

/// Whose colours an aura wears.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuraStyle {
    /// The original's level 99 aura: blue throughout.
    Level,
    /// Tenseiga-no-Hayate's (a custom hat effect): its blade from crimson hilt to cyan tip.
    /// Drawn with whitened textures, so the colours here are the whole of it.
    Tenseiga,
}

const CRIMSON: [f32; 3] = [1.0, 0.06, 0.18];
const MAGENTA: [f32; 3] = [1.0, 0.3, 0.72];
const VIOLET: [f32; 3] = [0.45, 0.15, 1.0];
const CYAN: [f32; 3] = [0.1, 0.75, 1.0];

impl AuraStyle {
    /// Ribbon colour at its foot and at its top.
    fn ribbon(self) -> ([f32; 4], [f32; 4]) {
        match self {
            AuraStyle::Level => {
                let c = [100.0 / 255.0, 100.0 / 255.0, 1.0, 120.0 / 255.0];
                (c, c)
            }
            AuraStyle::Tenseiga => ([CRIMSON[0], CRIMSON[1], CRIMSON[2], 2.0], [VIOLET[0], VIOLET[1], VIOLET[2], 2.0]),
        }
    }

    /// The two ground stars' colours, and how fast they turn (turns per step).
    fn ground(self) -> ([[f32; 4]; 2], f32) {
        match self {
            AuraStyle::Level => ([[1.0, 1.0, 1.0, 0.8]; 2], 0.0),
            AuraStyle::Tenseiga => ([[CRIMSON[0], CRIMSON[1], CRIMSON[2], 1.0], [CYAN[0], CYAN[1], CYAN[2], 0.8]], 1.0 / 800.0),
        }
    }

    /// The dark pool under the aura (the blade's indigo body), if it has one.
    fn shade(self) -> Option<[f32; 4]> {
        match self {
            AuraStyle::Level => None,
            AuraStyle::Tenseiga => Some([0.05, 0.02, 0.14, 0.92]),
        }
    }

    /// The `j`th sparkle's colour (alpha 1).
    fn sparkle(self, j: u32) -> [f32; 3] {
        match self {
            AuraStyle::Level => [80.0 / 255.0, 80.0 / 255.0, 1.0],
            // The runes' magenta, the edge's cyan and the body's violet.
            AuraStyle::Tenseiga => [MAGENTA, CYAN, VIOLET][j as usize % 3],
        }
    }
}

/// The level 99 aura's layers. The original builds them in code; the shapes and numbers
/// follow roBrowser's reconstruction of its Level99 effects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuraLayer {
    /// EF_LEVEL99: three ribbons (effect\ring_blue.tga) spiralling up and around.
    Swirl,
    /// EF_LEVEL99_2: two light stars (effect\pikapika2.bmp) on the ground, breathing.
    Ground,
    /// EF_LEVEL99_3: blue motes (effect\whitelight.tga) drifting up from the feet.
    Sparkles,
    /// A dark pool on the ground under the light layers (alpha-blended), so coloured auras
    /// read on bright ground. Custom auras only; the level aura has none.
    Shade,
}

impl ProcFx {
    /// Drawn alpha-blended (darkening) rather than added.
    pub fn shades(&self) -> bool {
        matches!(self.shape, Shape::Aura { layer: AuraLayer::Shade, .. })
    }
}

/// The original's effect clock: one step every 25 ms.
const AURA_TICK_MS: f32 = 25.0;
/// roBrowser's scale from the original's aura units to cells.
const AURA_UNIT: f32 = 0.22 * CELL;

fn aura(layer: AuraLayer, seed: u32, style: AuraStyle, pos: Vec3, time_ms: f32, eye: Vec3, out: &mut Vec<Vertex>) {
    let ticks = time_ms / AURA_TICK_MS;
    match layer {
        AuraLayer::Shade => {
            let Some(color) = style.shade() else { return };
            // Breathes with the ground stars.
            let half = 2.4 * CELL * (1.0 + 0.05 * (ticks / 120.0 * std::f32::consts::TAU).sin());
            let v = |x: f32, z: f32, u: f32, vv: f32| Vertex { pos: (pos + Vec3::new(x, 0.08, z)).into(), uv: [u, vv], color };
            let (a, b, c, d) = (v(-half, -half, 0.0, 0.0), v(half, -half, 1.0, 0.0), v(half, half, 1.0, 1.0), v(-half, half, 0.0, 1.0));
            out.extend([a, b, c, a, c, d]);
        }
        AuraLayer::Swirl => {
            const DIVISIONS: usize = 21;
            // The ribbons grow to full height over the first 90 steps.
            let grow = ticks.min(90.0).to_radians().sin();
            let (foot, top_color) = style.ribbon();
            for band in 0..3 {
                let b = band as f32;
                let start = b * 90.0 + ticks * (b + 3.0);
                let radius = (3.9 + 0.2 * b) * AURA_UNIT * 0.6;
                let height = (15.0 - 2.0 * b) * AURA_UNIT;
                let (rise_sin, rise_cos) = (55.0 - 5.0 * b).to_radians().sin_cos();
                let edge = |k: usize| {
                    // Each ribbon spans 315°, tallest in its middle and flat at both ends.
                    let h = height * (90.0 + (k as f32 - 10.0) * 9.0).to_radians().sin() * grow;
                    let (s, c) = (start + k as f32 * 315.0 / 20.0).to_radians().sin_cos();
                    let base = pos + Vec3::new(radius * c, 0.0, radius * s);
                    let top = base + Vec3::new(rise_cos * h * c, rise_sin * h, rise_cos * h * s);
                    let u = k as f32 / 20.0;
                    (Vertex { pos: base.into(), uv: [u, 1.0], color: foot }, Vertex { pos: top.into(), uv: [u, 0.0], color: top_color })
                };
                for k in 0..DIVISIONS - 1 {
                    let ((b0, t0), (b1, t1)) = (edge(k), edge(k + 1));
                    out.extend([b0, t0, b1, t0, t1, b1]);
                }
            }
        }
        AuraLayer::Ground => {
            // Two stars, 23° apart, swelling and shrinking out of step over 120 steps
            // (Tenseiga's also turn slowly, against each other).
            let (colors, spin) = style.ground();
            for (i, base) in [(0, 115.0), (1, 130.0)] {
                let breathe = (ticks / 120.0 * std::f32::consts::TAU + i as f32 * std::f32::consts::PI).sin();
                let half = base / 35.0 * CELL * 0.5 * (1.0 + 0.07 * breathe);
                let turn = ticks * spin * 360.0 * if i == 0 { 1.0 } else { -1.0 };
                let (s, c) = (i as f32 * 23.0 + turn).to_radians().sin_cos();
                let corner = |x: f32, z: f32| pos + Vec3::new(x * c - z * s, 0.1, x * s + z * c);
                let color = colors[i];
                let v = |p: Vec3, u: f32, vv: f32| Vertex { pos: p.into(), uv: [u, vv], color };
                let (a, b, cc, d) = (v(corner(-half, -half), 0.0, 0.0), v(corner(half, -half), 1.0, 0.0), v(corner(half, half), 1.0, 1.0), v(corner(-half, half), 0.0, 1.0));
                out.extend([a, b, cc, a, cc, d]);
            }
        }
        AuraLayer::Sparkles => {
            // 16 motes, each rising 30 units in 200 steps, then resting a while before
            // starting again from the feet. Drawn twice over in the original, so doubled here.
            const RISE: f32 = 200.0;
            let fwd = (pos - eye).normalize_or_zero();
            let right = fwd.cross(Vec3::Y).normalize_or_zero();
            let up = right.cross(fwd);
            let half = 2.4 * AURA_UNIT * 0.6;
            for j in 0..16u32 {
                let r = |n: u32| (hash(seed ^ j.wrapping_mul(0x9e37_79b9) ^ n.wrapping_mul(0x85eb_ca6b)) & 0xffff) as f32 / 65535.0;
                let rest = r(1) * 660.0;
                let cycle = RISE + rest;
                let t = (ticks + r(2) * cycle) % cycle;
                if t < rest {
                    continue;
                }
                let lift = (t - rest) * 0.15;
                // Full until 20 units up, gone by 28.
                let alpha = ((250.0 - 30.0 * (lift - 20.0)).clamp(0.0, 250.0)) / 255.0;
                if alpha <= 0.0 {
                    continue;
                }
                let (f1, f2, p1, p2) = (0.02 + r(3) * 0.03, 0.02 + r(4) * 0.03, r(5) * 6.3, r(6) * 6.3);
                let drift = |f: f32, p: f32| ((t - rest) * f + p).sin() - p.sin();
                let centre = pos + Vec3::new(drift(f1, p1) * 3.0 * AURA_UNIT, lift * AURA_UNIT, drift(f2, p2) * 3.0 * AURA_UNIT);
                let [r, g, b] = style.sparkle(j);
                let color = [r, g, b, alpha * 2.0];
                let (a, b) = (right * half, up * half);
                let v = |p: Vec3, u: f32, vv: f32| Vertex { pos: p.into(), uv: [u, vv], color };
                let (p0, p1, p2, p3) = (v(centre - a - b, 0.0, 1.0), v(centre + a - b, 1.0, 1.0), v(centre + a + b, 1.0, 0.0), v(centre - a + b, 0.0, 0.0));
                out.extend([p0, p1, p2, p0, p2, p3]);
            }
        }
    }
}

fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^ (x >> 16)
}

/// A playing code-drawn effect.
#[derive(Clone, Debug)]
pub struct ProcFx {
    pub tex: TexId,
    pub shape: Shape,
    pub pos: Vec3,
    pub color: [f32; 3],
    pub time_ms: f32,
    pub duration_ms: f32,
}

impl ProcFx {
    pub fn vertices(&self, eye: Vec3, out: &mut Vec<Vertex>) {
        let t = (self.time_ms / self.duration_ms.max(1.0)).clamp(0.0, 1.0);
        // Quick fade in, long fade out.
        let fade = (t / 0.1).min(1.0) * ((1.0 - t) / 0.5).min(1.0);
        let color = [self.color[0], self.color[1], self.color[2], fade];
        match self.shape {
            Shape::Aura { layer, seed, style } => aura(layer, seed, style, self.pos, self.time_ms, eye, out),
            Shape::Column { radius, height } => {
                let h = height * CELL * (t / 0.3).min(1.0);
                let r = radius * CELL;
                cylinder(out, self.pos, r, r * 0.8, h, t * 0.5, color);
                cylinder(out, self.pos, r * 0.6, r * 0.5, h * 1.2, -t * 0.7, color);
            }
            Shape::Spikes { reach, count } => {
                // Each spike rises and sinks in turn, the wave running from pos to pos + reach.
                let side = Vec3::new(-reach.z, 0.0, reach.x).normalize_or_zero();
                for i in 0..count {
                    let f = (i as f32 + 1.0) / count as f32;
                    let local = (t * 1.6 - f * 0.9).clamp(0.0, 1.0);
                    if local <= 0.0 || local >= 1.0 {
                        continue;
                    }
                    let rise = (local * std::f32::consts::PI).sin();
                    // Alternate sides a little so the line looks jagged.
                    let jitter = side * if i % 2 == 0 { 0.35 } else { -0.35 } * CELL;
                    let base = self.pos + reach * f + jitter;
                    let h = CELL * (2.2 + 1.0 * ((i * 7 % 5) as f32 / 4.0)) * rise;
                    let r = CELL * 0.4;
                    let c = [color[0], color[1], color[2], color[3] * rise.min(1.0)];
                    let apex = base + Vec3::new(0.0, h, 0.0);
                    for k in 0..5 {
                        let a0 = k as f32 / 5.0 * std::f32::consts::TAU;
                        let a1 = (k + 1) as f32 / 5.0 * std::f32::consts::TAU;
                        let p0 = base + Vec3::new(a0.cos() * r, 0.0, a0.sin() * r);
                        let p1 = base + Vec3::new(a1.cos() * r, 0.0, a1.sin() * r);
                        let (u0, u1) = (k as f32 / 5.0, (k + 1) as f32 / 5.0);
                        out.extend([
                            Vertex { pos: p0.into(), uv: [u0, 1.0], color: c },
                            Vertex { pos: p1.into(), uv: [u1, 1.0], color: c },
                            Vertex { pos: apex.into(), uv: [(u0 + u1) * 0.5, 0.0], color: c },
                        ]);
                    }
                }
            }
            Shape::Wave { radius } => {
                // A low wall rushing outward and thinning (Meteor Assault's shockwave).
                let ease = 1.0 - (1.0 - t).powi(3);
                let r = radius * CELL * ease.max(0.05);
                let h = CELL * 1.2 * (1.0 - t * 0.6);
                cylinder(out, self.pos, r, r * 1.15, h, t * 0.3, color);
                cylinder(out, self.pos, r * 0.7, r * 0.85, h * 0.6, -t * 0.4, color);
            }
            Shape::Orb { reach } => {
                // Flies to pos + reach over the first half, then swells and fades there.
                let fly = (t / 0.5).min(1.0);
                let swell = ((t - 0.5) / 0.5).max(0.0);
                let centre = self.pos + reach * fly + Vec3::new(0.0, CELL * (1.2 - 0.6 * swell), 0.0);
                let s = CELL * (0.7 + 2.3 * swell);
                let c = [color[0], color[1], color[2], (1.0 - swell).min(fade * 2.0).max(0.0)];
                let v = |p: Vec3, u: f32, vv: f32| Vertex { pos: p.into(), uv: [u, vv], color: c };
                // Three crossed quads read as a ball from any camera angle.
                for (a, b) in [(Vec3::X, Vec3::Y), (Vec3::Z, Vec3::Y), (Vec3::X, Vec3::Z)] {
                    let (a, b) = (a * s, b * s);
                    let (p0, p1, p2, p3) = (v(centre - a - b, 0.0, 1.0), v(centre + a - b, 1.0, 1.0), v(centre + a + b, 1.0, 0.0), v(centre - a + b, 0.0, 0.0));
                    out.extend([p0, p1, p2, p0, p2, p3]);
                }
            }
            Shape::Slash { travel, size } => {
                let centre = self.pos + travel * t + Vec3::new(0.0, CELL, 0.0);
                let fwd = (centre - eye).normalize_or_zero();
                let right = fwd.cross(Vec3::Y).normalize_or_zero();
                let up = right.cross(fwd);
                // The crescent bulges toward -u: point that side along the on-screen travel.
                let (dx, dy) = (travel.dot(right), travel.dot(up));
                let lead = if dx * dx + dy * dy > 1e-6 { (right * dx + up * dy).normalize() } else { -right };
                let side = lead.cross(fwd);
                let s = CELL * (size.0 + (size.1 - size.0) * t);
                let (a, b) = (-lead * s, side * s);
                let v = |p: Vec3, u: f32, vv: f32| Vertex { pos: p.into(), uv: [u, vv], color };
                let (p0, p1, p2, p3) = (v(centre - a - b, 0.0, 1.0), v(centre + a - b, 1.0, 1.0), v(centre + a + b, 1.0, 0.0), v(centre - a + b, 0.0, 0.0));
                out.extend([p0, p1, p2, p0, p2, p3]);
            }
            Shape::Motes { drift, size } => {
                let secs = self.time_ms / 1000.0;
                let wobble = Vec3::new((secs * 1.3).sin(), (secs * 0.9).sin() * 0.5, (secs * 1.1).cos()) * CELL * 0.3;
                let centre = self.pos + drift * t + wobble;
                let fwd = (centre - eye).normalize_or_zero();
                let right = fwd.cross(Vec3::Y).normalize_or_zero();
                let up = right.cross(fwd);
                let (s, c) = (secs * 0.4).sin_cos();
                let half = CELL * size * 0.5;
                let (a, b) = ((right * c + up * s) * half, (up * c - right * s) * half);
                let v = |p: Vec3, u: f32, vv: f32| Vertex { pos: p.into(), uv: [u, vv], color };
                let (p0, p1, p2, p3) = (v(centre - a - b, 0.0, 1.0), v(centre + a - b, 1.0, 1.0), v(centre + a + b, 1.0, 0.0), v(centre - a + b, 0.0, 0.0));
                out.extend([p0, p1, p2, p0, p2, p3]);
            }
            Shape::Disc { radius, spin } => {
                let r = radius * CELL;
                let ang = self.time_ms / 1000.0 * spin * std::f32::consts::TAU;
                let (s, c) = ang.sin_cos();
                let corner = |x: f32, z: f32| self.pos + Vec3::new(x * c - z * s, 0.15, x * s + z * c);
                let v = |p: Vec3, u: f32, vv: f32| Vertex { pos: p.into(), uv: [u, vv], color };
                let (a, b, cc, d) = (v(corner(-r, -r), 0.0, 0.0), v(corner(r, -r), 1.0, 0.0), v(corner(r, r), 1.0, 1.0), v(corner(-r, r), 0.0, 1.0));
                out.extend([a, b, cc, a, cc, d]);
            }
            Shape::Area { size } => {
                let (hx, hz) = (size.0 * CELL / 2.0, size.1 * CELL / 2.0);
                let v = |x: f32, z: f32, u: f32, vv: f32| Vertex { pos: (self.pos + Vec3::new(x, 0.2, z)).into(), uv: [u, vv], color };
                let (a, b, cc, d) = (v(-hx, -hz, 0.0, 0.0), v(hx, -hz, 1.0, 0.0), v(hx, hz, 1.0, 1.0), v(-hx, hz, 0.0, 1.0));
                out.extend([a, b, cc, a, cc, d]);
            }
        }
    }
}

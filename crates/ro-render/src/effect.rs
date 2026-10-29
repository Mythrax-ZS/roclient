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
        }
    }
}

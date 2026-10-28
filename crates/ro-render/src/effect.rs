//! Simple 3D effects. Currently the warp portal: counter-rotating, additively blended
//! cylinders textured with effect\ring_blue.tga, rebuilt each frame (a few hundred vertices).

use glam::Vec3;

use crate::CELL;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    pub alpha: f32,
}

impl Vertex {
    const ATTRS: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32];

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
fn cylinder(out: &mut Vec<Vertex>, base: Vec3, bottom: f32, top: f32, height: f32, spin: f32, alpha: f32) {
    for i in 0..SEGMENTS {
        let (a0, a1) = (i as f32 / SEGMENTS as f32, (i + 1) as f32 / SEGMENTS as f32);
        let ring = |t: f32, r: f32, y: f32| {
            let ang = t * std::f32::consts::TAU;
            base + Vec3::new(ang.cos() * r, y, ang.sin() * r)
        };
        // The texture wraps twice around the cylinder.
        let (u0, u1) = (a0 * 2.0 + spin, a1 * 2.0 + spin);
        let v = |p: Vec3, u: f32, vv: f32| Vertex { pos: p.into(), uv: [u, vv], alpha };
        let (b0, b1, t0, t1) = (ring(a0, bottom, 0.0), ring(a1, bottom, 0.0), ring(a0, top, height), ring(a1, top, height));
        out.extend([v(b0, u0, 1.0), v(b1, u1, 1.0), v(t1, u1, 0.0), v(b0, u0, 1.0), v(t1, u1, 0.0), v(t0, u0, 0.0)]);
    }
}

/// Geometry for warp portals at `positions` (ground points) at time `t` seconds.
pub fn warp_portals(positions: &[Vec3], t: f32) -> Vec<Vertex> {
    let mut out = Vec::with_capacity(positions.len() * SEGMENTS * 12);
    for &p in positions {
        cylinder(&mut out, p, 1.7 * CELL, 2.2 * CELL, 1.4 * CELL, t * 0.25, 0.9);
        cylinder(&mut out, p, 1.1 * CELL, 1.5 * CELL, 2.6 * CELL, -t * 0.4, 0.7);
    }
    out
}

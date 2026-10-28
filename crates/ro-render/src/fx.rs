//! STR effects (skill animations): each asset's layer textures share one atlas, and live
//! effects become screen-aligned quads around a world point, drawn after the actors.

use glam::Vec3;
use ro_formats::{image::Rgba, strm::Str};

use crate::sprite::Vertex;

/// An uploaded STR effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FxId(pub usize);

pub(crate) struct FxAsset {
    pub str: Str,
    /// uv rect per layer, per texture.
    pub rects: Vec<Vec<[f32; 4]>>,
    pub bind: wgpu::BindGroup,
}

/// A playing effect.
#[derive(Clone, Debug)]
pub struct Fx {
    pub asset: FxId,
    /// Where it plays (usually a unit's feet).
    pub pos: Vec3,
    pub time_ms: f32,
}

impl FxAsset {
    /// `images[layer][texture]`; missing textures draw nothing.
    pub fn upload(device: &wgpu::Device, queue: &wgpu::Queue, layout: &wgpu::BindGroupLayout, sampler: &wgpu::Sampler, str: Str, images: Vec<Vec<Option<Rgba>>>) -> Self {
        let blank = Rgba { width: 1, height: 1, pixels: vec![0; 4] };
        let flat: Vec<Rgba> = images.iter().flatten().map(|i| i.clone().unwrap_or_else(|| blank.clone())).collect();
        let flat = if flat.is_empty() { vec![blank] } else { flat };
        let (atlas, placed) = crate::sprite::pack(&flat);
        let (aw, ah) = (atlas.width as f32, atlas.height as f32);
        let mut placed = flat.iter().zip(placed).map(|(img, (x, y))| {
            // Half-texel inset so filtering doesn't pull in the neighbours.
            let (x, y, w, h) = (x as f32 + 0.5, y as f32 + 0.5, img.width as f32 - 1.0, img.height as f32 - 1.0);
            [x / aw, y / ah, (x + w) / aw, (y + h) / ah]
        });
        let rects = images.iter().map(|layer| layer.iter().map(|_| placed.next().unwrap_or_default()).collect()).collect();
        let view = crate::scene::upload_texture(device, queue, &atlas, false);
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("str effect"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
            ],
        });
        Self { str, rects, bind }
    }

    /// Vertices for one playing instance.
    pub fn vertices(&self, fx: &Fx, out: &mut Vec<Vertex>) {
        let frame = fx.time_ms / 1000.0 * self.str.fps as f32;
        for q in self.str.quads(frame) {
            let Some(&[u0, v0, u1, v1]) = self.rects.get(q.layer).and_then(|l| l.get(q.texture)) else { continue };
            let alpha = if q.additive { -q.color[3].max(1e-3) } else { q.color[3] };
            let color = [q.color[0], q.color[1], q.color[2], alpha];
            let uv = [[u0, v0], [u1, v0], [u1, v1], [u0, v1]];
            let v = |i: usize| Vertex { anchor: fx.pos.into(), offset: q.corners[i], uv: uv[i], color };
            out.extend([v(0), v(1), v(2), v(0), v(2), v(3)]);
        }
    }

    pub fn duration_ms(&self) -> f32 {
        self.str.duration_ms()
    }
}

//! SPR/ACT actors: atlased sprite sheets, body + head composition, and screen-aligned
//! billboards that keep the depth of the actor's feet.

use glam::Vec3;
use ro_formats::{act::Act, image::Rgba, spr::Spr};
use wgpu::util::DeviceExt;

/// Screen pixels of sprite art per GAT cell at the reference camera distance.
pub const PIXELS_PER_CELL: f32 = 35.0;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    /// Actor feet, world space.
    pub anchor: [f32; 3],
    /// Sprite-pixel offset from the feet (x right, y down).
    pub offset: [f32; 2],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}

impl Vertex {
    const ATTRS: [wgpu::VertexAttribute; 4] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x2, 3 => Float32x4];

    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}

/// Handle to an uploaded sprite sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SheetId(pub(crate) usize);

/// RO actor directions, counter-clockwise from south as the ACT files order them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    S = 0,
    SW,
    W,
    NW,
    N,
    NE,
    E,
    SE,
}

/// One actor to draw this frame.
#[derive(Clone, Debug)]
pub struct Actor {
    pub pos: Vec3,
    pub body: SheetId,
    pub head: Option<SheetId>,
    pub shadow: Option<SheetId>,
    /// Action group (0 idle, 1 walk, 2 sit, ...); the ACT index is `action * 8 + dir`.
    pub action: usize,
    pub dir: Dir,
    /// Milliseconds into the current action.
    pub time_ms: f32,
}

pub(crate) struct Sheet {
    act: Act,
    /// uv rect (u0, v0, u1, v1) and pixel size per image: indexed first, then RGBA.
    rects: Vec<([f32; 4], [f32; 2])>,
    indexed_len: usize,
    pub bind: wgpu::BindGroup,
}

impl Sheet {
    pub fn upload(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        spr: &Spr,
        act: Act,
        palette: Option<&[[u8; 4]; 256]>,
    ) -> Self {
        let (indexed, rgba) = spr.images(palette);
        let indexed_len = indexed.len();
        let images: Vec<Rgba> = indexed.into_iter().chain(rgba).collect();
        let (atlas, placed) = pack(&images);
        let (aw, ah) = (atlas.width as f32, atlas.height as f32);
        let rects = images
            .iter()
            .zip(placed)
            .map(|(img, (x, y))| {
                let (w, h) = (img.width as f32, img.height as f32);
                ([x as f32 / aw, y as f32 / ah, (x as f32 + w) / aw, (y as f32 + h) / ah], [w, h])
            })
            .collect();
        let view = crate::scene::upload_texture(device, queue, &atlas, false);
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sprite sheet"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
            ],
        });
        Self { act, rects, indexed_len, bind }
    }

    fn action(&self, action: usize) -> Option<&ro_formats::act::Action> {
        self.act.actions.get(action).or_else(|| self.act.actions.get(action % 8)).filter(|a| !a.frames.is_empty())
    }

    /// Frame index of `action` at `time_ms`, looping.
    fn frame_index(&self, action: usize, time_ms: f32) -> usize {
        self.action(action).map_or(0, |a| (time_ms / a.delay_ms.max(1.0)) as usize % a.frames.len())
    }

    /// Frame `i` of `action`, wrapping if this sheet has fewer frames.
    fn frame(&self, action: usize, i: usize) -> Option<&ro_formats::act::Frame> {
        let a = self.action(action)?;
        a.frames.get(i % a.frames.len())
    }

    /// Emits the frame's layers as quads, shifted by `shift` pixels.
    fn emit(&self, frame: &ro_formats::act::Frame, pos: Vec3, shift: [f32; 2], out: &mut Vec<Vertex>) {
        for l in &frame.layers {
            if l.sprite < 0 {
                continue;
            }
            let idx = if l.kind == 1 { self.indexed_len + l.sprite as usize } else { l.sprite as usize };
            let Some(&(uv, [w, h])) = self.rects.get(idx) else { continue };
            let (sin, cos) = (l.rotation as f32).to_radians().sin_cos();
            let corner = |cx: f32, cy: f32| {
                let mut x = cx * w * 0.5 * l.scale[0];
                let y = cy * h * 0.5 * l.scale[1];
                if l.mirror {
                    x = -x;
                }
                // Clockwise on screen (y down).
                [x * cos - y * sin + l.x as f32 + shift[0], x * sin + y * cos + l.y as f32 + shift[1]]
            };
            let color = l.color.map(|c| c as f32 / 255.0);
            let v = |cx, cy, u, vv| Vertex { anchor: pos.into(), offset: corner(cx, cy), uv: [u, vv], color };
            let (tl, tr, bl, br) = (v(-1.0, -1.0, uv[0], uv[1]), v(1.0, -1.0, uv[2], uv[1]), v(-1.0, 1.0, uv[0], uv[3]), v(1.0, 1.0, uv[2], uv[3]));
            out.extend([tl, tr, br, tl, br, bl]);
        }
    }
}

/// Builds the quads for an actor: shadow, body, then the head attached at the anchors.
pub(crate) fn actor_quads(sheets: &[Sheet], a: &Actor) -> Vec<(usize, Vec<Vertex>)> {
    let mut out = vec![];
    if let Some(s) = a.shadow.and_then(|id| sheets.get(id.0)) {
        if let Some(f) = s.frame(0, 0) {
            let mut v = vec![];
            s.emit(f, a.pos, [0.0, 0.0], &mut v);
            out.push((a.shadow.unwrap().0, v));
        }
    }
    let index = a.action * 8 + a.dir as usize;
    // Idle and sitting heads hold frame 0: their extra frames are head-turn poses, not animation.
    let still_head = matches!(a.action, 0 | 2);
    let Some(body) = sheets.get(a.body.0) else { return out };
    let body_frame = body.frame_index(index, a.time_ms);
    let Some(bf) = body.frame(index, body_frame) else { return out };
    let mut v = vec![];
    body.emit(bf, a.pos, [0.0, 0.0], &mut v);
    out.push((a.body.0, v));

    // The head follows the body's frame index so its anchor lines up every frame
    // (a separate timer lets the two drift and the head bobs).
    if let Some(head) = a.head.and_then(|id| sheets.get(id.0)) {
        if let Some(hf) = head.frame(index, if still_head { 0 } else { body_frame }) {
            let shift = match (bf.anchors.first(), hf.anchors.first()) {
                (Some(b), Some(h)) => [(b[0] - h[0]) as f32, (b[1] - h[1]) as f32],
                _ => [0.0, 0.0],
            };
            let mut v = vec![];
            head.emit(hf, a.pos, shift, &mut v);
            out.push((a.head.unwrap().0, v));
        }
    }
    out
}

/// Shelf-packs images into one atlas with a 1px transparent gutter.
fn pack(images: &[Rgba]) -> (Rgba, Vec<(u32, u32)>) {
    const W: u32 = 1024;
    let (mut x, mut y, mut row_h) = (1u32, 1u32, 0u32);
    let mut placed = Vec::with_capacity(images.len());
    for img in images {
        if x + img.width + 1 > W {
            x = 1;
            y += row_h + 1;
            row_h = 0;
        }
        placed.push((x, y));
        x += img.width + 1;
        row_h = row_h.max(img.height);
    }
    let height = (y + row_h + 1).max(1);
    let width = W.max(images.iter().map(|i| i.width + 2).max().unwrap_or(1));
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    for (img, &(px, py)) in images.iter().zip(&placed) {
        for row in 0..img.height {
            let src = &img.pixels[(row * img.width * 4) as usize..((row + 1) * img.width * 4) as usize];
            let dst = (((py + row) * width + px) * 4) as usize;
            pixels[dst..dst + src.len()].copy_from_slice(src);
        }
    }
    (Rgba { width, height, pixels }, placed)
}

pub(crate) fn vertex_buffer(device: &wgpu::Device, vertices: &[Vertex]) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("sprites"),
        contents: bytemuck::cast_slice(vertices),
        usage: wgpu::BufferUsages::VERTEX,
    })
}

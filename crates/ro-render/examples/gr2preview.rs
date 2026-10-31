//! Renders a Granny model posed through a clip, from the game camera's angle, to a PNG strip:
//! cargo run -p ro-render --example gr2preview -- out.png model.gr2 [clip.gr2] [yaw_degrees]
use glam::{Mat4, Vec3, Vec4};
use ro_formats::gr2::Gr2;
use ro_render::model3d::{Clip, Model3d};

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let model = Gr2::parse(&std::fs::read(&a[2])?)?;
    let clip = a.get(3).filter(|p| p.ends_with(".gr2")).map(|p| Gr2::parse(&std::fs::read(p).unwrap()).unwrap());
    let yaw: f32 = a.iter().skip(3).find_map(|s| s.parse().ok()).unwrap_or(0.0);
    let m = Model3d::new(&model, &clip.map(|c| vec![(Clip::Attack, c)]).unwrap_or_default());
    let which = if m.has_clip(Clip::Attack) { Clip::Attack } else { Clip::Idle };
    let frames = 6;
    const W: usize = 220;
    const H: usize = 300;
    let mut img = vec![[60u8, 60, 70, 255]; W * frames * H];
    // A camera like the game's: south of the model, 40 degrees up.
    let eye = Vec3::new(0.0, 45.0, -55.0);
    // East (+x) on the right, like the game's (mirrored) camera.
    let target = Vec3::new(0.0, 12.0, 0.0);
    let f = (target - eye).normalize();
    let r = Vec3::Y.cross(f).normalize();
    let u = f.cross(r);
    let vp = Mat4::from_cols(
        Vec4::new(r.x / 22.0, u.x / 30.0, f.x / 400.0, 0.0),
        Vec4::new(r.y / 22.0, u.y / 30.0, f.y / 400.0, 0.0),
        Vec4::new(r.z / 22.0, u.z / 30.0, f.z / 400.0, 0.0),
        Vec4::new(-r.dot(target) / 22.0, -u.dot(target) / 30.0, (-f.dot(target) + 200.0) / 400.0, 1.0),
    );
    for f in 0..frames {
        let t = m.duration(which) * f as f32 / frames as f32;
        let tris = m.pose(which, t, false, Vec3::ZERO, yaw.to_radians(), 1.0, [1.0; 3]);
        let mut depth = vec![f32::MAX; W * H];
        for (tex, verts) in &tris {
            let texture = &model.textures[*tex];
            let Some(px) = &texture.rgba else { continue };
            for tri in verts.chunks(3) {
                let s: Vec<(Vec3, [f32; 2], f32)> = tri
                    .iter()
                    .map(|v| {
                        let c = vp * Vec4::new(v.pos[0], v.pos[1], v.pos[2], 1.0);
                        let shade = 0.55 + 0.45 * Vec3::from(v.normal).dot(Vec3::new(-0.3, 0.8, -0.5).normalize()).max(0.0);
                        (Vec3::new((c.x * 0.5 + 0.5) * W as f32, (0.5 - c.y * 0.5) * H as f32, c.z), v.uv, shade)
                    })
                    .collect();
                let (x0, x1) = (s.iter().map(|p| p.0.x).fold(f32::MAX, f32::min).max(0.0) as usize, s.iter().map(|p| p.0.x).fold(f32::MIN, f32::max).min(W as f32 - 1.0) as usize);
                let (y0, y1) = (s.iter().map(|p| p.0.y).fold(f32::MAX, f32::min).max(0.0) as usize, s.iter().map(|p| p.0.y).fold(f32::MIN, f32::max).min(H as f32 - 1.0) as usize);
                let area = (s[1].0.x - s[0].0.x) * (s[2].0.y - s[0].0.y) - (s[2].0.x - s[0].0.x) * (s[1].0.y - s[0].0.y);
                if area.abs() < 1e-6 {
                    continue;
                }
                for y in y0..=y1 {
                    for x in x0..=x1 {
                        let (px_, py) = (x as f32 + 0.5, y as f32 + 0.5);
                        let w0 = ((s[1].0.x - px_) * (s[2].0.y - py) - (s[2].0.x - px_) * (s[1].0.y - py)) / area;
                        let w1 = ((s[2].0.x - px_) * (s[0].0.y - py) - (s[0].0.x - px_) * (s[2].0.y - py)) / area;
                        let w2 = 1.0 - w0 - w1;
                        if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                            continue;
                        }
                        let z = w0 * s[0].0.z + w1 * s[1].0.z + w2 * s[2].0.z;
                        if z >= depth[y * W + x] {
                            continue;
                        }
                        let u = w0 * s[0].1[0] + w1 * s[1].1[0] + w2 * s[2].1[0];
                        let v = w0 * s[0].1[1] + w1 * s[1].1[1] + w2 * s[2].1[1];
                        let (tx, ty) = ((u.rem_euclid(1.0) * texture.width as f32) as usize, (v.rem_euclid(1.0) * texture.height as f32) as usize);
                        let i = (ty.min(texture.height as usize - 1) * texture.width as usize + tx.min(texture.width as usize - 1)) * 4;
                        if px[i + 3] < 128 {
                            continue;
                        }
                        let shade = w0 * s[0].2 + w1 * s[1].2 + w2 * s[2].2;
                        depth[y * W + x] = z;
                        img[y * W * frames + f * W + x] = [(px[i] as f32 * shade) as u8, (px[i + 1] as f32 * shade) as u8, (px[i + 2] as f32 * shade) as u8, 255];
                    }
                }
            }
        }
        // Feet marker: the world origin.
        let o = vp * Vec4::new(0.0, 0.0, 0.0, 1.0);
        let (ox, oy) = (((o.x * 0.5 + 0.5) * W as f32) as usize, ((0.5 - o.y * 0.5) * H as f32) as usize);
        for d in 0..5 {
            if oy < H && ox + d < W {
                img[oy * W * frames + f * W + ox + d] = [255, 0, 0, 255];
            }
        }
    }
    image::save_buffer(&a[1], bytemuck::cast_slice(&img), (W * frames) as u32, H as u32, image::ExtendedColorType::Rgba8)?;
    println!("{:?} {:.2}s", which, m.duration(which));
    Ok(())
}

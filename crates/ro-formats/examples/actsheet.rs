//! Renders frame 0 of every action into one PNG row: cargo run --example actsheet -- file.spr file.act out.png
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let spr = ro_formats::spr::Spr::parse(&std::fs::read(&a[1])?)?;
    let act = ro_formats::act::Act::parse(&std::fs::read(&a[2])?)?;
    let (imgs, rgba) = spr.images(None);
    let imgs: Vec<_> = imgs.into_iter().chain(rgba).collect();
    const C: u32 = 96;
    let n = act.actions.len() as u32;
    let mut out = image::RgbaImage::from_pixel(C * n, C, image::Rgba([60, 60, 70, 255]));
    for (ai, action) in act.actions.iter().enumerate() {
        let Some(f) = action.frames.first() else { continue };
        for l in &f.layers {
            let Some(img) = (l.sprite >= 0).then(|| imgs.get(l.sprite as usize)).flatten() else { continue };
            let (ox, oy) = (ai as i32 * C as i32 + C as i32 / 2 + l.x - img.width as i32 / 2, C as i32 / 2 + l.y - img.height as i32 / 2);
            for y in 0..img.height {
                for x in 0..img.width {
                    let p = &img.pixels[((y * img.width + x) * 4) as usize..][..4];
                    let (px, py) = (ox + x as i32, oy + y as i32);
                    if p[3] > 0 && px >= 0 && py >= 0 && (px as u32) < C * n && (py as u32) < C {
                        out.put_pixel(px as u32, py as u32, image::Rgba([p[0], p[1], p[2], 255]));
                    }
                }
            }
        }
        // Marker at the hotspot (action origin).
        out.put_pixel(ai as u32 * C + C / 2, C / 2, image::Rgba([255, 0, 0, 255]));
    }
    out.save(&a[3])?;
    Ok(())
}

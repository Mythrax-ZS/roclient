//! texpack: an AI-upscaled pack of world textures (ground and model art) for the client's
//! `overlays` (docs/upscaling.md). Each texture is read through the GRFs in priority order,
//! padded with its own opposite edges (so tiling ground stays seamless), upscaled 4x by
//! Real-ESRGAN (realesrgan-ncnn-vulkan), cropped back, and given its colour-keyed cut-out
//! again from the original's alpha. Written as PNG under the original name: the client's
//! decoder goes by content. Never sprites, effects or interface art.
//!
//!   texpack --out <dir> --esrgan <realesrgan-ncnn-vulkan.exe> [--model <name>]
//!           [--map <name>]... <grf>...
//!
//! Without --map: every texture under data\texture\ outside the interface and effect
//! folders. Textures already in <dir> are skipped, so a stopped run picks up where it was.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context};
use image::{GrayImage, RgbImage, RgbaImage};
use ro_formats::image::Rgba;

/// Texels of wrapped border around each texture while it is upscaled.
const PAD: u32 = 8;
const SCALE: u32 = 4;
/// Textures per Real-ESRGAN run (progress, and a bounded work folder).
const BATCH: usize = 400;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let (mut out, mut esrgan, mut model, mut maps, mut grfs) = (None, None, "realesrgan-x4plus".to_string(), vec![], vec![]);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--out" => out = args.next().map(PathBuf::from),
            "--esrgan" => esrgan = args.next().map(PathBuf::from),
            "--model" => model = args.next().context("--model needs a name")?,
            "--map" => maps.push(args.next().context("--map needs a name")?),
            _ => grfs.push(PathBuf::from(a)),
        }
    }
    let (Some(out), Some(esrgan), false) = (out, esrgan, grfs.is_empty()) else {
        bail!("usage: texpack --out <dir> --esrgan <exe> [--model <name>] [--map <name>]... <grf>...");
    };

    let open = || grfs.iter().map(|g| ro_vfs::Grf::open(g).with_context(|| format!("open {}", g.display()))).collect::<anyhow::Result<Vec<_>>>();
    let vfs = ro_vfs::Vfs::new(vec![], None, open()?);
    let names = if maps.is_empty() { world_textures(&open()?) } else { map_textures(&vfs, &maps)? };
    let todo: Vec<String> = names.into_iter().filter(|n| !out.join(n.replace('\\', "/")).exists()).collect();
    eprintln!("{} textures to upscale", todo.len());

    let work = std::env::temp_dir().join("texpack");
    for (b, batch) in todo.chunks(BATCH).enumerate() {
        let (inp, upd) = (work.join("in"), work.join("out"));
        for d in [&inp, &upd] {
            let _ = std::fs::remove_dir_all(d);
            std::fs::create_dir_all(d)?;
        }
        let mut jobs = vec![];
        for (i, name) in batch.iter().enumerate() {
            let Some(data) = vfs.read(name)? else { continue };
            let Ok(img) = ro_formats::image::decode(&data, name) else { continue };
            if img.width < 4 || img.height < 4 {
                continue;
            }
            padded(&filled(&img)).save(inp.join(format!("{i}.png")))?;
            jobs.push((i, name, img));
        }
        let status = Command::new(&esrgan).current_dir(esrgan.parent().unwrap_or(Path::new("."))).args(["-i".as_ref(), inp.as_os_str(), "-o".as_ref(), upd.as_os_str(), "-n".as_ref(), model.as_ref(), "-s".as_ref(), "4".as_ref(), "-f".as_ref(), "png".as_ref()]).output()?;
        if !status.status.success() {
            bail!("Real-ESRGAN failed: {}", String::from_utf8_lossy(&status.stderr));
        }
        for (i, name, img) in jobs {
            let up = image::open(upd.join(format!("{i}.png")))?.to_rgb8();
            let dest = out.join(name.replace('\\', "/"));
            std::fs::create_dir_all(dest.parent().unwrap())?;
            finished(&up, &img).save_with_format(&dest, image::ImageFormat::Png)?;
        }
        eprintln!("batch {}: {} of {} done", b + 1, (b * BATCH + batch.len()).min(todo.len()), todo.len());
    }
    let _ = std::fs::remove_dir_all(&work);
    Ok(())
}

/// Every world texture in the GRFs: data\texture\ without the interface and effect art.
fn world_textures(grfs: &[ro_vfs::Grf]) -> BTreeSet<String> {
    const SKIP: [&str; 3] = ["data\\texture\\유저인터페이스\\", "data\\texture\\userinterface\\", "data\\texture\\effect\\"];
    let image = |n: &str| [".bmp", ".tga", ".png", ".jpg"].iter().any(|e| n.ends_with(e));
    grfs.iter()
        .flat_map(|g| g.names())
        .map(|n| n.to_lowercase())
        .filter(|n| n.starts_with("data\\texture\\") && image(n) && !SKIP.iter().any(|s| n.starts_with(s)))
        .collect()
}

/// The ground and model textures these maps use.
fn map_textures(vfs: &ro_vfs::Vfs, maps: &[String]) -> anyhow::Result<BTreeSet<String>> {
    let mut names = BTreeSet::new();
    for map in maps {
        let rsw = ro_formats::rsw::Rsw::parse(&vfs.read(&format!("data\\{map}.rsw"))?.with_context(|| format!("{map}.rsw not found"))?)?;
        if let Some(gnd) = vfs.read(&format!("data\\{}", rsw.gnd))? {
            names.extend(ro_formats::gnd::Gnd::parse(&gnd)?.textures.iter().map(|t| format!("data\\texture\\{t}").to_lowercase()));
        }
        let models: BTreeSet<_> = rsw.models.iter().map(|m| m.file.clone()).collect();
        for m in models {
            let Some(rsm) = vfs.read(&format!("data\\model\\{m}"))? else { continue };
            let Ok(rsm) = ro_formats::rsm::Rsm::parse(&rsm) else { continue };
            names.extend(rsm.textures.iter().map(|t| format!("data\\texture\\{t}").to_lowercase()));
        }
    }
    Ok(names)
}

/// The colour with see-through texels filled from their opaque neighbours, so the
/// upscaler sees no hard edge (or magenta) where the cut-out is.
fn filled(img: &Rgba) -> RgbImage {
    let (w, h) = (img.width as i32, img.height as i32);
    let mut px: Vec<Option<[u8; 3]>> = img.pixels.chunks_exact(4).map(|p| (p[3] > 0).then(|| [p[0], p[1], p[2]])).collect();
    for _ in 0..16 {
        let before = px.clone();
        let mut changed = false;
        for y in 0..h {
            for x in 0..w {
                if before[(y * w + x) as usize].is_some() {
                    continue;
                }
                let near: Vec<[u8; 3]> = [(-1, 0), (1, 0), (0, -1), (0, 1)].iter().filter_map(|(dx, dy)| before[((y + dy).rem_euclid(h) * w + (x + dx).rem_euclid(w)) as usize]).collect();
                if !near.is_empty() {
                    let avg = |c: usize| (near.iter().map(|p| p[c] as u32).sum::<u32>() / near.len() as u32) as u8;
                    px[(y * w + x) as usize] = Some([avg(0), avg(1), avg(2)]);
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    let raw = px.into_iter().flat_map(|p| p.unwrap_or([128; 3])).collect();
    RgbImage::from_raw(img.width, img.height, raw).expect("size")
}

/// `img` with PAD texels of its own opposite edges around it (as if tiled).
fn padded(img: &RgbImage) -> RgbImage {
    let (w, h) = img.dimensions();
    RgbImage::from_fn(w + 2 * PAD, h + 2 * PAD, |x, y| *img.get_pixel((x + w * PAD - PAD) % w, (y + h * PAD - PAD) % h))
}

/// The upscaled padded colour cropped back to the texture, with the original's cut-out
/// (smoothly scaled, then made hard again) as alpha.
fn finished(up: &RgbImage, original: &Rgba) -> RgbaImage {
    let (w, h) = (original.width * SCALE, original.height * SCALE);
    let colour = image::imageops::crop_imm(up, PAD * SCALE, PAD * SCALE, w, h).to_image();
    let keyed = original.pixels.chunks_exact(4).any(|p| p[3] == 0);
    let alpha = keyed.then(|| {
        let mask = GrayImage::from_raw(original.width, original.height, original.pixels.chunks_exact(4).map(|p| p[3]).collect()).expect("size");
        image::imageops::resize(&mask, w, h, image::imageops::FilterType::Triangle)
    });
    RgbaImage::from_fn(w, h, |x, y| {
        let [r, g, b] = colour.get_pixel(x, y).0;
        let a = alpha.as_ref().map_or(255, |m| if m.get_pixel(x, y).0[0] >= 128 { 255 } else { 0 });
        image::Rgba([r, g, b, a])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pads_by_wrapping_and_crops_back() {
        let img = RgbImage::from_fn(5, 4, |x, y| image::Rgb([x as u8, y as u8, 0]));
        let p = padded(&img);
        assert_eq!(p.dimensions(), (5 + 2 * PAD, 4 + 2 * PAD));
        // The texture sits at (PAD, PAD); left of it is its own right edge.
        assert_eq!(p.get_pixel(PAD, PAD).0, [0, 0, 0]);
        assert_eq!(p.get_pixel(PAD - 1, PAD).0, [4, 0, 0]);
        assert_eq!(p.get_pixel(PAD, PAD - 1).0, [0, 3, 0]);
        // A 4x "upscale" by nearest neighbour crops back to the texture.
        let up = image::imageops::resize(&p, p.width() * SCALE, p.height() * SCALE, image::imageops::FilterType::Nearest);
        let orig = Rgba { width: 5, height: 4, pixels: img.pixels().flat_map(|q| [q[0], q[1], q[2], 255]).collect() };
        let f = finished(&up, &orig);
        assert_eq!(f.dimensions(), (20, 16));
        assert_eq!(f.get_pixel(19, 15).0, [4, 3, 0, 255]);
    }

    #[test]
    fn keeps_the_cut_out() {
        // Left half see-through: it stays see-through, and its colour comes from the right.
        let orig = Rgba { width: 4, height: 4, pixels: (0..16).flat_map(|i| if i % 4 < 2 { [0, 0, 0, 0] } else { [200, 100, 50, 255] }).collect() };
        let fill = filled(&orig);
        assert_eq!(fill.get_pixel(0, 0).0, [200, 100, 50]);
        let up = image::imageops::resize(&padded(&fill), (4 + 2 * PAD) * SCALE, (4 + 2 * PAD) * SCALE, image::imageops::FilterType::Nearest);
        let f = finished(&up, &orig);
        assert_eq!(f.get_pixel(0, 0).0[3], 0);
        assert_eq!(f.get_pixel(15, 15).0[3], 255);
    }
}

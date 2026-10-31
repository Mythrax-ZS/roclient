//! Makes rAthena's walkability agree with the client's map data. Reads the server's map
//! caches, compares each map with the client's .gat (as rAthena reads cells), and writes the
//! maps that differ, built from the client's data the way rAthena's mapcache tool does, to an
//! import cache the map server prefers:
//!   servermapcache <rathena dir> <out.dat> <grf>...
//! e.g. servermapcache ../rathena ../rathena/db/import/map_cache.dat data.grf kro/data.grf
use std::io::{Read, Write};

/// rAthena's map_gat2cell: which cell types can be walked on (the cache keeps one byte).
fn walkable(t: u8) -> bool {
    matches!(t, 0 | 2 | 3 | 4 | 6)
}

/// grfio_read_rsw_water_level.
fn water_level(rsw: &[u8]) -> Option<i32> {
    if rsw.len() < 175 || &rsw[..4] != b"GRSW" {
        return None;
    }
    let version = u16::from_be_bytes([rsw[4], rsw[5]]);
    let at = match version {
        0x205 => 171,
        0x202..=0x204 => 167,
        0x104..=0x201 => 166,
        _ => return None,
    };
    Some(f32::from_le_bytes(rsw[at..at + 4].try_into().ok()?) as i32)
}

/// mapcache.cpp read_map: one byte per cell, walkable ground under water becomes 3.
fn cells(gat: &[u8], water: Option<i32>) -> Option<(usize, usize, Vec<u8>)> {
    let (w, h) = (u32::from_le_bytes(gat[6..10].try_into().ok()?) as usize, u32::from_le_bytes(gat[10..14].try_into().ok()?) as usize);
    let mut out = Vec::with_capacity(w * h);
    for i in 0..w * h {
        let off = 14 + i * 20;
        let height = f32::from_le_bytes(gat.get(off..off + 4)?.try_into().ok()?);
        let mut t = u32::from_le_bytes(gat.get(off + 16..off + 20)?.try_into().ok()?);
        if t == 0 && water.is_some_and(|l| height > l as f32) {
            t = 3;
        }
        out.push(t as u8);
    }
    Some((w, h, out))
}

/// Every map in an rAthena map cache: name -> (width, height, cells).
fn read_cache(path: &std::path::Path) -> anyhow::Result<Vec<(String, usize, usize, Vec<u8>)>> {
    let d = std::fs::read(path)?;
    let count = u16::from_le_bytes([d[4], d[5]]) as usize;
    let (mut p, mut maps) = (8, vec![]);
    for _ in 0..count {
        let name = String::from_utf8_lossy(&d[p..p + 12]).trim_end_matches('\0').to_string();
        let xs = i16::from_le_bytes([d[p + 12], d[p + 13]]) as usize;
        let ys = i16::from_le_bytes([d[p + 14], d[p + 15]]) as usize;
        let len = i32::from_le_bytes(d[p + 16..p + 20].try_into()?) as usize;
        let mut cells = vec![];
        flate2::read::ZlibDecoder::new(&d[p + 20..p + 20 + len]).read_to_end(&mut cells)?;
        maps.push((name, xs, ys, cells));
        p += 20 + len;
    }
    Ok(maps)
}

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let root = std::path::Path::new(&a[1]);
    let grfs = a[3..].iter().map(|p| ro_vfs::Grf::open(std::path::Path::new(p))).collect::<anyhow::Result<Vec<_>>>()?;
    let vfs = ro_vfs::Vfs::new(vec![], None, grfs);
    // The map server reads the renewal cache before the base one; the first copy of a map wins.
    let mut seen = std::collections::HashSet::new();
    let mut out: Vec<(String, usize, usize, Vec<u8>)> = vec![];
    for cache in ["db/re/map_cache.dat", "db/map_cache.dat"] {
        for (name, xs, ys, server) in read_cache(&root.join(cache))? {
            if !seen.insert(name.clone()) {
                continue;
            }
            let Some(gat) = vfs.read(&format!("data\\{name}.gat"))? else { continue };
            let water = vfs.read(&format!("data\\{name}.rsw"))?.and_then(|r| water_level(&r));
            let Some((w, h, client)) = cells(&gat, water) else { continue };
            let differ = if (w, h) != (xs, ys) { w * h } else { client.iter().zip(&server).filter(|(c, s)| walkable(**c) != walkable(**s)).count() };
            if differ > 0 {
                println!("{name}: {differ} cells differ ({cache})");
                out.push((name, w, h, client));
            }
        }
    }
    let mut body = vec![];
    for (name, w, h, cells) in &out {
        let mut z = flate2::write::ZlibEncoder::new(vec![], flate2::Compression::default());
        z.write_all(cells)?;
        let z = z.finish()?;
        let mut n = [0u8; 12];
        n[..name.len().min(11)].copy_from_slice(&name.as_bytes()[..name.len().min(11)]);
        body.extend(n);
        body.extend((*w as i16).to_le_bytes());
        body.extend((*h as i16).to_le_bytes());
        body.extend((z.len() as i32).to_le_bytes());
        body.extend(z);
    }
    let mut file = vec![];
    file.extend(((8 + body.len()) as u32).to_le_bytes());
    file.extend((out.len() as u16).to_le_bytes());
    file.extend([0, 0]);
    file.extend(body);
    std::fs::write(&a[2], file)?;
    println!("wrote {} maps to {}", out.len(), a[2]);
    Ok(())
}

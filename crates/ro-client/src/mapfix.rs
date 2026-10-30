//! Corrections to Gravity's map data, applied as a map loads. Each one deliberately draws a
//! map differently from the original client (which shows the data as it is); each says
//! what it repairs. Only the drawn ground changes: walkability and heights come from the
//! GAT and are untouched.

use ro_formats::gnd::Gnd;

/// Applies `map`'s fixes to its ground; returns how many tiles changed.
pub fn ground(map: &str, gnd: &mut Gnd) -> usize {
    match map {
        "prontera" => mirror_axis(gnd, "prt_도시01.bmp", 79, 80..=81),
        _ => 0,
    }
}

/// Prontera's south avenue is paved with square-framed cobble rosettes, mirrored across
/// the avenue's centre line: the tiles right of `left` repeat the texture flipped
/// left-to-right, so the rosette on the line is symmetric. But the map sets the mirrored
/// tiles half a rosette lower than their partners (two of them upside down too), so the
/// rosette frames step at the centre line. Each mirrored tile takes its partner's vertical
/// texture coordinates, row by row, wherever both use `texture`.
fn mirror_axis(gnd: &mut Gnd, texture: &str, left: u32, mirrored: std::ops::RangeInclusive<u32>) -> usize {
    let Some(tex) = gnd.textures.iter().position(|t| t.to_ascii_lowercase().ends_with(texture)) else { return 0 };
    let tex = tex as i16;
    let top = |gnd: &Gnd, x: u32, y: u32| gnd.cube(x, y).filter(|c| gnd.surface(c.top).is_some_and(|s| s.texture == tex)).map(|c| c.top as usize);
    let mut fixed = 0;
    for y in 0..gnd.height {
        let Some(partner) = top(gnd, left, y) else { continue };
        let v = gnd.surfaces[partner].v;
        for x in mirrored.clone() {
            let Some(index) = top(gnd, x, y) else { continue };
            if gnd.surfaces[index].v == v {
                continue;
            }
            // Surfaces can be shared between tiles: give this tile its own copy.
            let mut surface = gnd.surfaces[index];
            surface.v = v;
            gnd.surfaces.push(surface);
            gnd.cubes[(y * gnd.width + x) as usize].top = (gnd.surfaces.len() - 1) as i32;
            fixed += 1;
        }
    }
    fixed
}

#[cfg(test)]
mod tests {
    use super::*;
    use ro_formats::gnd::{Cube, Surface};

    /// A 4x3 ground of one texture: columns 0-1 run the texture left to right, 2-3 mirror
    /// it, every mirrored tile half a texture lower (and one upside down).
    fn ground() -> Gnd {
        let quarter = |q: u32, rising: bool| {
            let (a, b) = (q as f32 / 4.0, (q + 1) as f32 / 4.0);
            if rising { [a, a, b, b] } else { [b, b, a, a] }
        };
        let mut surfaces = vec![];
        let mut cubes = vec![];
        for y in 0..3 {
            for x in 0..4u32 {
                let u = match x {
                    0 => [0.5, 0.75, 0.5, 0.75],
                    1 => [0.75, 1.0, 0.75, 1.0],
                    2 => [1.0, 0.75, 1.0, 0.75],
                    _ => [0.75, 0.5, 0.75, 0.5],
                };
                let v = if x < 2 { quarter(y, true) } else { quarter((y + 2) % 4, !(x == 2 && y == 1)) };
                surfaces.push(Surface { u, v, texture: 1, lightmap: 0, color: [255; 4] });
                cubes.push(Cube { heights: [0.0; 4], top: (surfaces.len() - 1) as i32, front: -1, right: -1 });
            }
        }
        Gnd {
            version: (1, 7),
            width: 4,
            height: 3,
            zoom: 10.0,
            textures: vec!["other.bmp".into(), "필드바닥\\prt_도시01.bmp".into()],
            lightmaps: Default::default(),
            surfaces,
            cubes,
            water: None,
            water_zones: None,
        }
    }

    #[test]
    fn mirrored_tiles_take_their_partners_rows() {
        let mut gnd = ground();
        assert_eq!(mirror_axis(&mut gnd, "prt_도시01.bmp", 1, 2..=3), 6);
        for y in 0..3 {
            let partner = gnd.surface(gnd.cube(1, y).unwrap().top).unwrap().v;
            for x in 2..4 {
                let s = gnd.surface(gnd.cube(x, y).unwrap().top).unwrap();
                assert_eq!(s.v, partner, "tile ({x}, {y})");
                assert_eq!(s.u[0] > s.u[1], true, "stays mirrored left to right");
            }
        }
        // Already aligned: nothing more to do.
        assert_eq!(mirror_axis(&mut gnd, "prt_도시01.bmp", 1, 2..=3), 0);
        // Another texture on the axis is left alone.
        assert_eq!(mirror_axis(&mut ground(), "prt_도시02.bmp", 1, 2..=3), 0);
    }
}

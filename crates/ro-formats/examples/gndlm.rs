//! Average lightmap shadow (0..9 scale) per ground tile in a window:
//! cargo run --example gndlm -- file.gnd cx cy radius
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let gnd = ro_formats::gnd::Gnd::parse(&std::fs::read(&a[1])?)?;
    let (cx, cy, r): (i32, i32, i32) = (a[2].parse()?, a[3].parse()?, a[4].parse()?);
    for y in (cy - r..=cy + r).rev() {
        let mut line = String::new();
        for x in cx - r..=cx + r {
            let c = gnd.cube(x as u32, y as u32).and_then(|c| gnd.surface(c.top));
            line += &match c.and_then(|s| gnd.lightmaps.get(s.lightmap as usize)) {
                Some((sh, _)) => format!("{}", sh.iter().map(|&v| v as u32).sum::<u32>() / sh.len() as u32 * 9 / 255),
                None => "-".into(),
            };
        }
        println!("{line}");
    }
    Ok(())
}

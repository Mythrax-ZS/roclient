//! Water depth per ground tile around a cell (debugging water edges):
//! cargo run --example gndwater -- file.gnd file.rsw cx cy radius
//! Each tile prints its shallowest and deepest corner below the water ('.' = all dry).
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let gnd = ro_formats::gnd::Gnd::parse(&std::fs::read(&a[1])?)?;
    let rsw = ro_formats::rsw::Rsw::parse(&std::fs::read(&a[2])?)?;
    let (cx, cy, r): (i32, i32, i32) = (a[3].parse()?, a[4].parse()?, a[5].parse()?);
    let level = -rsw.water.map_or(0.0, |w| w.level);
    println!("water level {level}");
    for y in (cy - r..=cy + r).rev() {
        let mut line = format!("{y:4} ");
        for x in cx - r..=cx + r {
            line += &match gnd.cube(x as u32, y as u32) {
                Some(c) => {
                    let d: Vec<f32> = c.heights.iter().map(|h| level + h).collect();
                    let (lo, hi) = (d.iter().cloned().fold(f32::MAX, f32::min), d.iter().cloned().fold(f32::MIN, f32::max));
                    if hi <= 0.0 { "     .     ".into() } else { format!("{lo:5.1}/{hi:<5.1}") }
                }
                None => "     -     ".into(),
            };
        }
        println!("{line}");
    }
    Ok(())
}

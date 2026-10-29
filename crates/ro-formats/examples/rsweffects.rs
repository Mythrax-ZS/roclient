//! Counts the effect emitters in each RSW by effect id: rsweffects <file.rsw> [gnd cx cy]
//! With a GND and a cell, also lists the emitters within 25 cells of it.
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let rsw = ro_formats::rsw::Rsw::parse(&std::fs::read(&a[1])?)?;
    let mut ids = std::collections::BTreeMap::<i32, usize>::new();
    for e in &rsw.effects {
        *ids.entry(e.id).or_default() += 1;
    }
    println!("{}: {} effects {ids:?}", a[1], rsw.effects.len());
    if a.len() >= 5 {
        let gnd = ro_formats::gnd::Gnd::parse(&std::fs::read(&a[2])?)?;
        let (cx, cy): (f32, f32) = (a[3].parse()?, a[4].parse()?);
        for e in &rsw.effects {
            let x = (e.pos[0] + gnd.width as f32 * gnd.zoom / 2.0) / 5.0;
            let y = (e.pos[2] + gnd.height as f32 * gnd.zoom / 2.0) / 5.0;
            if (x - cx).hypot(y - cy) < 25.0 {
                println!("id {} cell ({x:.1}, {y:.1}) height {:.1} delay {}", e.id, -e.pos[1], e.delay);
            }
        }
    }
    Ok(())
}

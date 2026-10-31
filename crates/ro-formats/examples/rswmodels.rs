//! Models placed near a cell: rswmodels <file.rsw> <file.gnd> <cx> <cy> [radius]
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let rsw = ro_formats::rsw::Rsw::parse(&std::fs::read(&a[1])?)?;
    let gnd = ro_formats::gnd::Gnd::parse(&std::fs::read(&a[2])?)?;
    let (cx, cy): (f32, f32) = (a[3].parse()?, a[4].parse()?);
    let radius: f32 = a.get(5).map_or(Ok(8.0), |r| r.parse())?;
    for m in &rsw.models {
        let x = (m.pos[0] + gnd.width as f32 * gnd.zoom / 2.0) / 5.0;
        let y = (m.pos[2] + gnd.height as f32 * gnd.zoom / 2.0) / 5.0;
        if (x - cx).hypot(y - cy) < radius {
            println!("cell ({x:.1}, {y:.1}) height {:.1} {} rot {:?} scale {:?} anim {}", -m.pos[1], m.file, m.rot, m.scale, m.anim_type);
        }
    }
    Ok(())
}

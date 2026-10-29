//! Lists a map's EF_EMITTER objects with their effecttool definition:
//!   emitterlist <map.rsw> <map.gnd> <map.lub>
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let rsw = ro_formats::rsw::Rsw::parse(&std::fs::read(&a[1])?)?;
    let gnd = ro_formats::gnd::Gnd::parse(&std::fs::read(&a[2])?)?;
    let c = ro_formats::lub::eval(&std::fs::read(&a[3])?)?;
    let map = std::path::Path::new(&a[1]).file_stem().unwrap().to_string_lossy().replace('@', "");
    let table = c.globals.get(&format!("_{map}_emitterInfo")).cloned();
    let list = |e: &ro_formats::lub::Value, k: &str| -> Vec<f64> {
        c.field(e, k).map(|t| c.entries(t).iter().filter_map(|(_, v)| if let ro_formats::lub::Value::Number(n) = v { Some(*n) } else { None }).collect()).unwrap_or_default()
    };
    for (i, e) in rsw.effects.iter().filter(|e| e.id == 974).enumerate() {
        let x = (e.pos[0] + gnd.width as f32 * gnd.zoom / 2.0) / 5.0;
        let y = (e.pos[2] + gnd.height as f32 * gnd.zoom / 2.0) / 5.0;
        let Some(d) = table.as_ref().and_then(|t| c.field_index(t, i as u32)) else {
            println!("{i:3} ({x:.0},{y:.0}) no definition");
            continue;
        };
        let tex = match c.field(d, "texture") {
            Some(ro_formats::lub::Value::Str(s)) => s.clone(),
            _ => "?".into(),
        };
        println!("{i:3} ({x:.0},{y:.0}) h {:.0} {tex} color {:?} blend {:?}/{:?} size {:?} speed {:?}", -e.pos[1], list(d, "color"), list(d, "srcmode"), list(d, "destmode"), list(d, "size"), list(d, "speed"));
    }
    Ok(())
}

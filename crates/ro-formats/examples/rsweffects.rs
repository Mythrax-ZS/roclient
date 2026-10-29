//! Effect emitters in RSWs, by effect id:
//!   rsweffects <file.rsw> <file.gnd> <cx> <cy>   emitters within 25 cells of a cell
//!   rsweffects <file.rsw or folder>...           per-id totals (emitters, maps) over all files
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    if a.len() == 5 && a[2].ends_with(".gnd") {
        let rsw = ro_formats::rsw::Rsw::parse(&std::fs::read(&a[1])?)?;
        let gnd = ro_formats::gnd::Gnd::parse(&std::fs::read(&a[2])?)?;
        let (cx, cy): (f32, f32) = (a[3].parse()?, a[4].parse()?);
        for e in &rsw.effects {
            let x = (e.pos[0] + gnd.width as f32 * gnd.zoom / 2.0) / 5.0;
            let y = (e.pos[2] + gnd.height as f32 * gnd.zoom / 2.0) / 5.0;
            if (x - cx).hypot(y - cy) < 25.0 {
                println!("id {} cell ({x:.1}, {y:.1}) height {:.1} delay {} name {:?} params {:?} raw {:?}", e.id, -e.pos[1], e.delay, e.name, e.params, e.pos);
            }
        }
        return Ok(());
    }
    // id -> (emitters, maps, example map)
    let mut ids = std::collections::BTreeMap::<i32, (usize, usize, String)>::new();
    let mut files = vec![];
    for p in &a[1..] {
        match std::fs::read_dir(p) {
            Ok(dir) => files.extend(dir.flatten().map(|e| e.path().to_string_lossy().into_owned()).filter(|f| f.ends_with(".rsw"))),
            Err(_) => files.push(p.clone()),
        }
    }
    for p in &files {
        let Ok(rsw) = std::fs::read(p).map_err(anyhow::Error::from).and_then(|d| ro_formats::rsw::Rsw::parse(&d)) else { continue };
        let name = std::path::Path::new(p).file_stem().unwrap().to_string_lossy().into_owned();
        let mut seen = std::collections::HashSet::new();
        for e in &rsw.effects {
            let s = ids.entry(e.id).or_insert((0, 0, name.clone()));
            s.0 += 1;
            if seen.insert(e.id) {
                s.1 += 1;
            }
        }
    }
    let mut rows: Vec<_> = ids.into_iter().collect();
    rows.sort_by_key(|(_, (n, m, _))| std::cmp::Reverse((*m, *n)));
    for (id, (n, m, ex)) in rows {
        println!("{id:5} maps {m:4} emitters {n:6} e.g. {ex}");
    }
    Ok(())
}

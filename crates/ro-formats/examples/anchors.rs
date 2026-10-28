//! Lists frame/anchor counts per action: `cargo run --example anchors -- file.act`
fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).expect("act path");
    let act = ro_formats::act::Act::parse(&std::fs::read(path)?)?;
    for (i, a) in act.actions.iter().enumerate().take(24) {
        let anchors: Vec<String> = a.frames.iter().map(|f| f.anchors.first().map_or("-".into(), |p| format!("{},{}", p[0], p[1]))).collect();
        println!("action {i:2} ({:>5.0}ms) frames {:2}: {}", a.delay_ms, a.frames.len(), anchors.join(" "));
    }
    Ok(())
}

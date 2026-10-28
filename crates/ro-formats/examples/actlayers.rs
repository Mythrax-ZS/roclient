//! Layers per action's first frame: `cargo run --example actlayers -- file.act`
fn main() -> anyhow::Result<()> {
    let act = ro_formats::act::Act::parse(&std::fs::read(std::env::args().nth(1).expect("act path"))?)?;
    for (i, a) in act.actions.iter().enumerate().step_by(8).take(8) {
        let f = &a.frames[a.frames.len() / 2];
        let l: Vec<_> = f.layers.iter().map(|l| (l.sprite, l.x, l.y)).collect();
        println!("action {i:2} mid frame layers {l:?}");
    }
    Ok(())
}

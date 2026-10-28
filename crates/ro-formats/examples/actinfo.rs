//! Every action's frame count, delay and first layer: cargo run --example actinfo -- file.act
fn main() -> anyhow::Result<()> {
    let act = ro_formats::act::Act::parse(&std::fs::read(std::env::args().nth(1).expect("act path"))?)?;
    for (i, a) in act.actions.iter().enumerate() {
        let l = a.frames.first().and_then(|f| f.layers.first());
        println!("action {i}: {} frames, {} ms, first layer {:?}", a.frames.len(), a.delay_ms, l.map(|l| (l.sprite, l.x, l.y)));
    }
    Ok(())
}

//! Visible layer count per action group (all frames): `cargo run --example actlayers -- file.act`
fn main() -> anyhow::Result<()> {
    let act = ro_formats::act::Act::parse(&std::fs::read(std::env::args().nth(1).expect("act path"))?)?;
    for g in 0..act.actions.len() / 8 {
        let a = &act.actions[g * 8];
        let vis: Vec<usize> = a.frames.iter().map(|f| f.layers.iter().filter(|l| l.sprite >= 0).count()).collect();
        println!("group {g:2} frames {:2} visible layers per frame {vis:?}", a.frames.len());
    }
    Ok(())
}

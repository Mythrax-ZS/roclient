//! Visible layers per direction for one action group: cargo run --example actdirs -- file.act group
fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let act = ro_formats::act::Act::parse(&std::fs::read(&args[1])?)?;
    let g: usize = args[2].parse()?;
    for d in 0..8 {
        let a = &act.actions[g * 8 + d];
        let vis: Vec<usize> = a.frames.iter().map(|f| f.layers.iter().filter(|l| l.sprite >= 0).count()).collect();
        println!("dir {d}: {vis:?}");
    }
    Ok(())
}

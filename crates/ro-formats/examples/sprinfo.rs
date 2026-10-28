//! Prints a summary of an SPR/ACT pair: `cargo run --example sprinfo -- file.spr file.act`
fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let spr = ro_formats::spr::Spr::parse(&std::fs::read(&args[0])?)?;
    println!("SPR {:?}: {} indexed, {} rgba", spr.version, spr.indexed.len(), spr.rgba.len());
    for (i, img) in spr.indexed.iter().enumerate().take(3) {
        let used: std::collections::BTreeSet<u8> = img.pixels.iter().copied().collect();
        let colors: Vec<_> = used.iter().take(6).map(|&c| (c, spr.palette[c as usize])).collect();
        println!("  #{i} {}x{} indices {colors:?}", img.width, img.height);
    }
    if let Some(path) = args.get(1) {
        let act = ro_formats::act::Act::parse(&std::fs::read(path)?)?;
        println!("ACT {:?}: {} actions", act.version, act.actions.len());
        if let Some(f) = act.actions.first().and_then(|a| a.frames.first()) {
            println!("  action 0 frame 0: {:?}", f.layers);
        }
    }
    Ok(())
}

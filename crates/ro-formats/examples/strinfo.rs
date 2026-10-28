//! Prints an STR effect's layers and keys: cargo run --example strinfo -- file.str [frame]
fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let s = ro_formats::strm::Str::parse(&std::fs::read(&args[1])?)?;
    println!("fps {} keys {} layers {} ({} ms)", s.fps, s.max_key, s.layers.len(), s.duration_ms());
    for (i, l) in s.layers.iter().enumerate() {
        println!("layer {i}: {:?}, {} keys", l.textures, l.keys.len());
        for k in l.keys.iter().take(4) {
            println!("  f{} t{} pos {:?} xy {:?} col {:?} ani {} {} {} ang {} blend {}/{}", k.frame, k.kind, k.pos, k.xy, k.color, k.aniframe, k.anitype, k.delay, k.angle, k.src_blend, k.dst_blend);
        }
    }
    if let Some(f) = args.get(2) {
        for q in s.quads(f.parse()?) {
            println!("{q:?}");
        }
    }
    Ok(())
}

//! Summarises Granny models: cargo run -p ro-formats --example gr2info -- file.gr2...
fn main() -> anyhow::Result<()> {
    for p in std::env::args().skip(1) {
        let t = std::time::Instant::now();
        let g = ro_formats::gr2::Gr2::parse(&std::fs::read(&p)?)?;
        println!("{p} ({:?}) placement {:?}", t.elapsed(), g.placement);
        for m in &g.meshes {
            println!("  mesh {:?}: {} verts, {} tris, {} bones bound, texture {:?}", m.name, m.positions.len(), m.indices.len() / 3, m.bone_names.len(), m.texture);
        }
        for s in &g.skeletons {
            println!("  skeleton {:?}: {} bones", s.name, s.bones.len());
            for b in s.bones.iter().take(4) {
                println!("    {:?} parent {} t {:?} r {:?}", b.name, b.parent, b.translation, b.rotation);
            }
        }
        for a in &g.animations {
            let keyed = a.tracks.iter().filter(|t| t.position.is_some() || t.orientation.is_some()).count();
            let degrees: std::collections::BTreeSet<_> = a.tracks.iter().flat_map(|t| [&t.position, &t.orientation, &t.scale_shear]).flatten().map(|c| c.degree).collect();
            println!("  animation {:?}: {:.2}s, {} tracks ({keyed} keyed), degrees {degrees:?}", a.name, a.duration, a.tracks.len());
        }
        for (i, t) in g.textures.iter().enumerate() {
            println!("  texture {:?}: {}x{} encoding {} decoded {}", t.name, t.width, t.height, t.encoding, t.rgba.is_some());
            if let Some(px) = &t.rgba {
                image::save_buffer(format!("{p}.tex{i}.png"), px, t.width, t.height, image::ExtendedColorType::Rgba8)?;
            }
        }
        if let Some(m) = g.meshes.first() {
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for p in g.meshes.iter().flat_map(|m| &m.positions) {
                for i in 0..3 {
                    lo[i] = lo[i].min(p[i]);
                    hi[i] = hi[i].max(p[i]);
                }
            }
            println!("  bounds {lo:?} .. {hi:?}; first verts {:?}", &m.positions[..2.min(m.positions.len())]);
        }
    }
    Ok(())
}

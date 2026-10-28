//! grfdump <file.grf> list [filter]
//! grfdump <file.grf> extract <path-in-grf> <out-file>

use std::path::Path;

use anyhow::{bail, Context};

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [grf, cmd, rest @ ..] = args.as_slice() else {
        bail!("usage: grfdump <file.grf> list [filter] | extract <path> <out>");
    };
    let grf = ro_vfs::Grf::open(Path::new(grf))?;
    match (cmd.as_str(), rest) {
        ("list", filter) => {
            let filter = filter.first().map(|f| f.to_lowercase());
            let mut names: Vec<_> = grf
                .names()
                .filter(|n| filter.as_ref().is_none_or(|f| n.to_lowercase().contains(f.as_str())))
                .collect();
            names.sort_unstable();
            for n in &names {
                println!("{n}");
            }
            eprintln!("{} of {} entries", names.len(), grf.len());
        }
        ("extract", [path, out]) => {
            let data = grf.read(path)?.with_context(|| format!("{path} not found"))?;
            std::fs::write(out, &data)?;
            eprintln!("wrote {} bytes to {out}", data.len());
        }
        ("models", [map]) => {
            // Which model files and format versions a map uses.
            let rsw = grf.read(&format!("data\\{map}.rsw"))?.with_context(|| format!("{map}.rsw not found"))?;
            let rsw = ro_formats::rsw::Rsw::parse(&rsw)?;
            let files: std::collections::BTreeSet<_> = rsw.models.iter().map(|m| m.file.clone()).collect();
            let mut versions = std::collections::BTreeMap::new();
            for f in &files {
                let key = match grf.read(&format!("data\\model\\{f}"))? {
                    Some(d) if d.len() >= 6 => {
                        let parsed = match ro_formats::rsm::Rsm::parse(&d) {
                            Ok(_) => "parsed".to_string(),
                            Err(e) => format!("FAILED: {e}"),
                        };
                        format!("{} {}.{} {parsed}", String::from_utf8_lossy(&d[..4]), d[4], d[5])
                    }
                    Some(_) => "truncated".into(),
                    None => "missing".into(),
                };
                *versions.entry(key).or_insert(0) += 1;
            }
            println!("RSW {}.{}: {} placements, {} unique models", rsw.version.0, rsw.version.1, rsw.models.len(), files.len());
            println!("light {:?}  water {:?}", rsw.light, rsw.water);
            for (v, n) in versions {
                println!("  {v}: {n}");
            }
        }
        _ => bail!("unknown command"),
    }
    Ok(())
}

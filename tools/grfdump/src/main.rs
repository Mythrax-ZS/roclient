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
        _ => bail!("unknown command"),
    }
    Ok(())
}

//! Extracts GRF entries to a folder: cargo run -p ro-vfs --example grfget -- data.grf out_dir path...
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let grf = ro_vfs::grf::Grf::open(std::path::Path::new(&a[1]))?;
    for p in &a[3..] {
        let data = grf.read(p)?.ok_or_else(|| anyhow::anyhow!("{p} not found"))?;
        std::fs::write(std::path::Path::new(&a[2]).join(p.rsplit(char::from(92u8)).next().unwrap()), data)?;
    }
    Ok(())
}

//! Lists GRF entries containing every needle: cargo run -p ro-vfs --example grffind -- data.grf needle...
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let grf = ro_vfs::grf::Grf::open(std::path::Path::new(&a[1]))?;
    for n in grf.names().filter(|n| a[2..].iter().all(|x| n.contains(x.as_str()))) {
        println!("{n}");
    }
    Ok(())
}

//! Prints globals and the first entries of each table: `cargo run --example lubdump -- file.lub`
fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).expect("lub path");
    let c = ro_formats::lub::eval(&std::fs::read(path)?)?;
    for (name, v) in &c.globals {
        let e = c.entries(v);
        println!("{name}: {} entries", e.len());
        for (k, val) in e.iter().take(4) {
            println!("  {k:?} = {val:?}");
        }
    }
    Ok(())
}

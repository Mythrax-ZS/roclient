//! Prints table entries whose key contains a string: cargo run --example lubfind -- file.lub needle
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let c = ro_formats::lub::eval(&std::fs::read(&a[1])?)?;
    for (name, v) in &c.globals {
        for (k, val) in c.entries(v) {
            if format!("{k:?}").contains(&a[2]) {
                println!("{name}: {k:?} = {val:?}");
            }
        }
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let c = ro_formats::lub::eval(&std::fs::read(&a[1])?)?;
    for (name, v) in &c.globals {
        for (k, val) in c.entries(v) {
            if format!("{k:?}").contains(&a[2]) {
                println!("{name}: {k:?}");
                for (k2, v2) in c.entries(&val) { println!("   {k2:?} = {v2:?}"); }
            }
        }
    }
    Ok(())
}

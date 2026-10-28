// Prints the globals and a sample of a .lub data table: lubdump <file> [n]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let d = std::fs::read(&args[1]).unwrap();
    let n: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(3);
    let c = ro_formats::lub::eval(&d).unwrap();
    for (name, v) in &c.globals {
        let e = c.entries(v);
        println!("{name}: {} entries", e.len());
        for (k, v) in e.iter().take(n) {
            println!("  {k:?} = {v:?}");
            for (k2, v2) in c.entries(v).iter().take(12) {
                println!("      {k2:?} = {v2:?}");
                for (k3, v3) in c.entries(v2).iter().take(6) {
                    println!("          {k3:?} = {v3:?}");
                }
            }
        }
    }
}

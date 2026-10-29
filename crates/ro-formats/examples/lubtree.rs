// Prints a .lub table as a tree, to any depth: lubtree <file> [global] [max entries per table]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let d = std::fs::read(&args[1]).unwrap();
    let only = args.get(2).filter(|s| !s.is_empty());
    let n: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(4);
    let c = ro_formats::lub::eval(&d).unwrap();
    fn walk(c: &ro_formats::lub::Chunk, v: &ro_formats::lub::Value, depth: usize, n: usize) {
        for (k, v) in c.entries(v).iter().take(n) {
            println!("{}{k:?} = {v:?}", "  ".repeat(depth));
            walk(c, v, depth + 1, n);
        }
    }
    for (name, v) in &c.globals {
        if only.is_some_and(|o| o != name) {
            continue;
        }
        println!("{name}");
        walk(&c, v, 1, n);
    }
}

//! Walkable (.) and blocked (#) cells around a cell, north up: gatgrid <file.gat> <cx> <cy> [radius]
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let gat = ro_formats::gat::Gat::parse(&std::fs::read(&a[1])?)?;
    let (cx, cy): (i32, i32) = (a[2].parse()?, a[3].parse()?);
    let r: i32 = a.get(4).map_or(Ok(12), |r| r.parse())?;
    for y in (cy - r..=cy + r).rev() {
        let row: String = (cx - r..=cx + r).map(|x| if (x, y) == (cx, cy) { 'X' } else if gat.walkable(x, y) { '.' } else { '#' }).collect();
        println!("{y:4} {row}");
    }
    println!("     x from {}", cx - r);
    Ok(())
}

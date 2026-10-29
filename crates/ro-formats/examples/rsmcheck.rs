//! Parses every RSM/RSM2 in a folder and reports failures: rsmcheck <folder>
fn main() {
    let dir = std::env::args().nth(1).unwrap();
    let (mut ok, mut bad) = (0, 0);
    let mut versions = std::collections::BTreeMap::<(u8, u8), usize>::new();
    for e in std::fs::read_dir(&dir).unwrap().flatten() {
        let p = e.path();
        let d = std::fs::read(&p).unwrap();
        match ro_formats::rsm::Rsm::parse(&d) {
            Ok(m) => {
                ok += 1;
                *versions.entry(m.version).or_default() += 1;
            }
            Err(err) => {
                bad += 1;
                if bad <= 10 {
                    println!("{}: {err:#}", p.display());
                }
            }
        }
    }
    println!("ok {ok} failed {bad} versions {versions:?}");
}

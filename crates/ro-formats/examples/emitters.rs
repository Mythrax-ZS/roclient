//! Surveys effecttool .lub files (EF_EMITTER particle definitions): emitters <folder>
use std::collections::BTreeMap;
fn main() {
    let dir = std::env::args().nth(1).unwrap();
    let mut blends = BTreeMap::<String, usize>::new();
    let mut textures = BTreeMap::<String, usize>::new();
    let mut flags = BTreeMap::<String, usize>::new();
    let mut tables = BTreeMap::<String, usize>::new();
    let (mut files, mut failed, mut emitters) = (0, 0, 0);
    for e in std::fs::read_dir(&dir).unwrap().flatten() {
        let d = std::fs::read(e.path()).unwrap();
        let Ok(c) = ro_formats::lub::eval(&d) else {
            failed += 1;
            continue;
        };
        files += 1;
        for (name, v) in &c.globals {
            let kind = name.rsplit('_').next().unwrap_or("").to_string();
            *tables.entry(kind.clone()).or_default() += 1;
            if kind != "emitterInfo" {
                continue;
            }
            for (_, em) in c.entries(v) {
                emitters += 1;
                let mut get = |k: &str| c.entries(&em).iter().find(|(kk, _)| format!("{kk:?}") == format!("Str({k:?})")).map(|(_, v)| format!("{:?}", c.entries(v).iter().map(|(_, x)| x.clone()).collect::<Vec<_>>()).replace("Number(", "").replace(".0)", "").replace(')', ""));
                let blend = format!("{:?}/{:?}", get("srcmode"), get("destmode"));
                *blends.entry(blend).or_default() += 1;
                *flags.entry(format!("billboard_off={:?} zenable={:?}", get("billboard_off"), get("zenable"))).or_default() += 1;
                let tex = c.entries(&em).iter().find(|(k, _)| format!("{k:?}") == "Str(\"texture\")").map(|(_, v)| format!("{v:?}")).unwrap_or_default();
                *textures.entry(tex).or_default() += 1;
            }
        }
    }
    println!("files {files} failed {failed} emitters {emitters}\ntables {tables:?}\nblends {blends:?}\nflags {flags:?}");
    let mut t: Vec<_> = textures.into_iter().collect();
    t.sort_by_key(|x| std::cmp::Reverse(x.1));
    println!("textures {:?}", &t[..t.len().min(30)]);
}

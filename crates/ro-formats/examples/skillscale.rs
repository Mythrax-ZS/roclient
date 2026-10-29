//! Prints SKILL_INFO_LIST entries that carry a SkillScale: skillscale <skillid.lub> <skillinfolist.lub>
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().collect();
    let lua = unsafe { mlua::Lua::unsafe_new() };
    lua.load("JOBID = setmetatable({}, { __index = function() return 0 end })").exec()?;
    for p in &a[1..3] {
        lua.load(&ro_formats::lub::widen_size_t(&std::fs::read(p)?)?[..]).exec()?;
    }
    let list: mlua::Table = lua.globals().get("SKILL_INFO_LIST")?;
    for (id, e) in list.pairs::<i64, mlua::Table>().filter_map(Result::ok) {
        if let Ok(s) = e.get::<mlua::Table>("SkillScale") {
            let lv: Vec<String> = s.sequence_values::<mlua::Table>().filter_map(Result::ok).map(|t| format!("({:?},{:?})", t.get::<f64>("x").ok(), t.get::<f64>("y").ok())).collect();
            println!("{id} {:?} {}", e.get::<String>(1).ok(), lv.join(" "));
        }
    }
    Ok(())
}

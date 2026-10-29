//! Runs a client .lub with every unknown global function logging its arguments:
//!   luacalls <file.lub> [max calls]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().collect();
    let data = ro_formats::lub::widen_size_t(&std::fs::read(&a[1])?)?;
    let max: usize = a.get(2).and_then(|n| n.parse().ok()).unwrap_or(20);
    let lua = unsafe { mlua::Lua::unsafe_new() };
    let count = std::rc::Rc::new(std::cell::Cell::new(0usize));
    let meta = lua.create_table()?;
    let c = count.clone();
    meta.set(
        "__index",
        lua.create_function(move |lua, (_, name): (mlua::Value, String)| {
            let c = c.clone();
            lua.create_function(move |_, args: mlua::MultiValue| {
                c.set(c.get() + 1);
                if c.get() <= max {
                    let shown: Vec<String> = args.iter().map(|v| format!("{v:?}")).collect();
                    println!("{name}({})", shown.join(", "));
                }
                Ok(true)
            })
        })?,
    )?;
    lua.globals().set_metatable(Some(meta))?;
    lua.load(&data[..]).exec()?;
    if let Ok(main) = lua.globals().raw_get::<mlua::Function>("main") {
        main.call::<()>(())?;
    }
    println!("{} calls", count.get());
    Ok(())
}

//! Runs the original client's companion AI scripts: AI\AI.lua (homunculus) and
//! AI\AI_M.lua (mercenary), or the player's own from AI\USER_AI\. Like the original,
//! the scripts are loaded first (Const.lua declares empty stand-ins for the client's
//! functions) and then the real functions are registered over them. `AI(myid)` runs
//! every tick against a snapshot of the units in sight; the moves, attacks and skills it
//! asks for come back as requests.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use mlua::{Lua, Value};

/// A unit as the scripts see it.
#[derive(Clone, Copy, Default)]
pub struct AiUnit {
    pub cell: (i32, i32),
    /// MOTION_STAND 0, MOVE 1, ATTACK 2, DEAD 3.
    pub motion: i32,
    /// Who it attacks / casts on (0: none).
    pub target: u32,
    pub monster: bool,
    pub hp: (i32, i32),
    pub sp: (i32, i32),
    pub range: i32,
    /// V_HOMUNTYPE / V_MERTYPE.
    pub kind: i32,
}

/// What the scripts asked for this tick.
#[derive(Debug, Clone, PartialEq)]
pub enum AiOrder {
    Move { id: u32, x: i32, y: i32 },
    Attack { id: u32, target: u32 },
    Skill { id: u32, level: u16, skill: u16, target: u32 },
    SkillGround { id: u32, level: u16, skill: u16, x: i32, y: i32 },
}

#[derive(Default)]
pub struct World {
    pub owner: u32,
    pub units: HashMap<u32, AiUnit>,
    pub orders: Vec<AiOrder>,
    /// A command the player gave this tick (GetMsg): e.g. {1 = MOVE_CMD, x, y}.
    pub msg: Vec<i64>,
}

pub struct Brain {
    lua: Lua,
    pub world: Rc<RefCell<World>>,
    started: std::time::Instant,
}

/// Loads `AI\<file>` (or USER_AI) under the client folder.
pub fn load(client_dir: &Path, file: &str, user: bool) -> mlua::Result<Brain> {
    let lua = Lua::new();
    let root = client_dir.to_path_buf();
    // `require "AI\\Const"` reads files relative to the client folder, once each.
    let loaded: Rc<RefCell<std::collections::HashSet<String>>> = Default::default();
    let read = {
        let root = root.clone();
        move |name: &str| -> mlua::Result<Vec<u8>> {
            let rel = name.replace('/', "\\");
            let mut p: PathBuf = root.clone();
            for part in rel.split('\\') {
                p.push(part);
            }
            if p.extension().is_none() {
                p.set_extension("lua");
            }
            std::fs::read(&p).map_err(|e| mlua::Error::external(format!("{}: {e}", p.display())))
        }
    };
    let req = {
        let read = read.clone();
        let loaded = loaded.clone();
        lua.create_function(move |lua, name: String| {
            if !loaded.borrow_mut().insert(name.to_lowercase()) {
                return Ok(());
            }
            let src = read(&name)?;
            lua.load(src).set_name(name).exec()
        })?
    };
    lua.globals().set("require", req)?;
    let dir = if user { "AI\\USER_AI" } else { "AI" };
    let main = read(&format!("{dir}\\{file}"))?;
    lua.load(main).set_name(file).exec()?;

    let world: Rc<RefCell<World>> = Default::default();
    let started = std::time::Instant::now();
    let g = lua.globals();
    g.set("TraceAI", lua.create_function(|_, s: String| {
        tracing::trace!(target: "ai", "{s}");
        Ok(())
    })?)?;
    let w = world.clone();
    g.set("MoveToOwner", lua.create_function(move |_, id: u32| {
        let mut w = w.borrow_mut();
        if let Some(o) = w.units.get(&w.owner).copied() {
            w.orders.push(AiOrder::Move { id, x: o.cell.0, y: o.cell.1 });
        }
        Ok(())
    })?)?;
    let w = world.clone();
    g.set("Move", lua.create_function(move |_, (id, x, y): (u32, i32, i32)| {
        w.borrow_mut().orders.push(AiOrder::Move { id, x, y });
        Ok(())
    })?)?;
    let w = world.clone();
    g.set("Attack", lua.create_function(move |_, (id, target): (u32, u32)| {
        w.borrow_mut().orders.push(AiOrder::Attack { id, target });
        Ok(())
    })?)?;
    let w = world.clone();
    g.set("SkillObject", lua.create_function(move |_, (id, level, skill, target): (u32, u16, u16, u32)| {
        w.borrow_mut().orders.push(AiOrder::Skill { id, level, skill, target });
        Ok(())
    })?)?;
    let w = world.clone();
    g.set("SkillGround", lua.create_function(move |_, (id, level, skill, x, y): (u32, u16, u16, i32, i32)| {
        w.borrow_mut().orders.push(AiOrder::SkillGround { id, level, skill, x, y });
        Ok(())
    })?)?;
    let w = world.clone();
    g.set("GetActors", lua.create_function(move |lua, ()| {
        let t = lua.create_table()?;
        for (i, id) in w.borrow().units.keys().enumerate() {
            t.set(i + 1, *id)?;
        }
        Ok(t)
    })?)?;
    g.set("GetTick", lua.create_function(move |_, ()| Ok(started.elapsed().as_millis() as i64))?)?;
    let w = world.clone();
    g.set("GetMsg", lua.create_function(move |lua, _id: u32| {
        let t = lua.create_table()?;
        let msg = std::mem::take(&mut w.borrow_mut().msg);
        if msg.is_empty() {
            t.set(1, 0)?;
        }
        for (i, v) in msg.into_iter().enumerate() {
            t.set(i + 1, v)?;
        }
        Ok(t)
    })?)?;
    g.set("GetResMsg", lua.create_function(|lua, _id: u32| {
        let t = lua.create_table()?;
        t.set(1, 0)?;
        Ok(t)
    })?)?;
    let w = world.clone();
    g.set("IsMonster", lua.create_function(move |_, id: u32| Ok(w.borrow().units.get(&id).is_some_and(|u| u.monster) as i32))?)?;
    let w = world.clone();
    g.set("GetV", lua.create_function(move |_, (v, id): (i32, u32)| -> mlua::Result<mlua::MultiValue> {
        let w = w.borrow();
        let u = w.units.get(&id).copied();
        let one = |n: i64| Ok(mlua::MultiValue::from_vec(vec![Value::Integer(n)]));
        match (v, u) {
            (0, _) => one(w.owner as i64),
            (1 | 13, Some(u)) => Ok(mlua::MultiValue::from_vec(vec![Value::Integer(u.cell.0 as i64), Value::Integer(u.cell.1 as i64)])),
            (1 | 13, None) => Ok(mlua::MultiValue::from_vec(vec![Value::Integer(-1), Value::Integer(-1)])),
            (3, Some(u)) => one(u.motion as i64),
            (4 | 6 | 14, Some(u)) => one(u.range.max(1) as i64),
            (5, Some(u)) => one(u.target as i64),
            (7 | 12, Some(u)) => one(u.kind as i64),
            (8, Some(u)) => one(u.hp.0 as i64),
            (9, Some(u)) => one(u.sp.0 as i64),
            (10, Some(u)) => one(u.hp.1 as i64),
            (11, Some(u)) => one(u.sp.1 as i64),
            _ => one(0),
        }
    })?)?;
    Ok(Brain { lua, world, started })
}

impl Brain {
    /// Runs `AI(myid)` once; returns what it asked for.
    pub fn tick(&self, myid: u32) -> Vec<AiOrder> {
        let f: mlua::Result<mlua::Function> = self.lua.globals().get("AI");
        if let Err(e) = f.and_then(|f| f.call::<()>(myid)) {
            tracing::warn!("AI script: {e}");
        }
        let _ = self.started;
        std::mem::take(&mut self.world.borrow_mut().orders)
    }
}

//! Skill areas shown while a monster casts (ZC_SKILL_SCALE): skillinfolist's `SkillScale`
//! gives each level's area in cells, drawn on the ground until the cast ends.

use std::collections::HashMap;

#[derive(Default)]
pub struct SkillScales {
    /// Skill -> (width, height) in cells per level.
    sizes: HashMap<u16, Vec<(f32, f32)>>,
}

impl SkillScales {
    pub fn load(vfs: &ro_vfs::Vfs) -> Self {
        let dir = r"data\luafiles514\lua files\skillinfoz";
        // Compiled chunks are widened for this Lua; translation packs ship plain text.
        let read = |n: &str| {
            let d = vfs.read(&format!(r"{dir}\{n}.lub")).ok().flatten()?;
            if d.starts_with(b"\x1bLua") { ro_formats::lub::widen_size_t(&d).ok() } else { Some(d) }
        };
        let (Some(ids), Some(list)) = (read("skillid"), read("skillinfolist")) else { return Self::default() };
        let lua = unsafe { mlua::Lua::unsafe_new() };
        let run = lua
            .load("JOBID = setmetatable({}, { __index = function() return 0 end })")
            .exec()
            .and_then(|_| lua.load(&ids[..]).exec())
            .and_then(|_| lua.load(&list[..]).exec())
            .and_then(|_| lua.globals().get::<mlua::Table>("SKILL_INFO_LIST"));
        let table = match run {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("skill scales: {e}");
                return Self::default();
            }
        };
        let mut sizes = HashMap::new();
        for (id, e) in table.pairs::<i64, mlua::Table>().filter_map(Result::ok) {
            let Ok(s) = e.get::<mlua::Table>("SkillScale") else { continue };
            let levels = s.sequence_values::<mlua::Table>().filter_map(Result::ok).map(|t| (t.get::<f32>("x").unwrap_or(1.0), t.get::<f32>("y").unwrap_or(1.0))).collect();
            sizes.insert(id as u16, levels);
        }
        tracing::info!(skills = sizes.len(), "skill scales");
        Self { sizes }
    }

    /// The area a cast of `skill` at `level` covers, in cells.
    pub fn size(&self, skill: u16, level: u16) -> Option<(f32, f32)> {
        let levels = self.sizes.get(&skill)?;
        levels.get(level.max(1) as usize - 1).or(levels.last()).copied()
    }
}

/// The area's look: a faint fill with a bright rim (drawn additively, so tinted red).
pub fn area_texture() -> ro_render::Rgba {
    const N: u32 = 64;
    let mut pixels = Vec::with_capacity((N * N * 4) as usize);
    for y in 0..N {
        for x in 0..N {
            let edge = x.min(y).min(N - 1 - x).min(N - 1 - y);
            let v = match edge {
                0..=2 => 255,
                3..=5 => 140,
                _ => 60,
            };
            pixels.extend([v, v, v, 255]);
        }
    }
    ro_render::Rgba { width: N, height: N, pixels }
}

//! Pet evolution recipes, as the original reads them: System\PetEvolutionCln.lub calls
//! InsertEvolutionRecipeLGU(egg, evolved egg, material, amount) for each material, and the
//! pet's egg comes from its class through npcidentity (JT_ name -> class) and petinfo's
//! PetEggItemID_PetJobID (egg -> JT_ name).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Evolved egg -> materials (item, amount).
pub type Recipes = Vec<(u32, Vec<(u32, u32)>)>;

#[derive(Default)]
pub struct PetEvolution {
    /// Pet class -> its egg.
    pub egg_of: HashMap<u16, u32>,
    /// Egg -> what it can evolve into.
    pub recipes: HashMap<u32, Recipes>,
}

impl PetEvolution {
    pub fn load(vfs: &ro_vfs::Vfs) -> Self {
        let mut out = Self::default();
        let dir = "data\\luafiles514\\lua files\\datainfo";
        let table = |name: &str, global: &str| -> Vec<(ro_formats::lub::Value, ro_formats::lub::Value)> {
            let Some(chunk) = vfs.read(&format!("{dir}\\{name}.lub")).ok().flatten().and_then(|d| ro_formats::lub::eval(&d).ok()) else { return vec![] };
            chunk.globals.get(global).map(|t| chunk.entries(t).to_vec()).unwrap_or_default()
        };
        let ids: HashMap<String, u16> = vfs
            .read(&format!("{dir}\\npcidentity.lub"))
            .ok()
            .flatten()
            .and_then(|d| ro_formats::lub::assignments(&d).ok())
            .unwrap_or_default()
            .iter()
            .filter_map(|(k, v)| Some((k.as_str()?.to_string(), v.as_number()? as u16)))
            .collect();
        for (egg, jt) in table("petinfo", "PetEggItemID_PetJobID") {
            if let (Some(egg), Some(class)) = (egg.as_number(), jt.as_str().and_then(|j| ids.get(j))) {
                out.egg_of.insert(*class, egg as u32);
            }
        }
        let Some(data) = vfs.read(r"System\PetEvolutionCln.lub").ok().flatten() else { return out };
        let Ok(data) = ro_formats::lub::widen_size_t(&data) else { return out };
        let lua = unsafe { mlua::Lua::unsafe_new() };
        let found: Rc<RefCell<HashMap<u32, Recipes>>> = Default::default();
        let sink = found.clone();
        let insert = lua.create_function(move |_, (from, to, item, amount): (u32, u32, u32, u32)| {
            let mut all = sink.borrow_mut();
            let list = all.entry(from).or_default();
            match list.iter_mut().find(|(t, _)| *t == to) {
                Some((_, materials)) => materials.push((item, amount)),
                None => list.push((to, vec![(item, amount)])),
            }
            Ok(true)
        });
        let run = insert.and_then(|f| lua.globals().set("InsertEvolutionRecipeLGU", f)).and_then(|_| {
            lua.globals().set("InsertPetAutoFeeding", lua.create_function(|_, _: mlua::MultiValue| Ok(true))?)?;
            lua.load(&data[..]).exec()?;
            lua.globals().get::<mlua::Function>("main")?.call::<()>(())
        });
        if let Err(e) = run {
            tracing::warn!("pet evolution recipes: {e}");
        }
        out.recipes = found.take();
        tracing::info!(pets = out.egg_of.len(), evolutions = out.recipes.values().map(Vec::len).sum::<usize>(), "pet evolution");
        out
    }

    /// What the pet of this class can evolve into.
    pub fn for_class(&self, class: u16) -> &[(u32, Vec<(u32, u32)>)] {
        self.egg_of.get(&class).and_then(|egg| self.recipes.get(egg)).map_or(&[], Vec::as_slice)
    }
}

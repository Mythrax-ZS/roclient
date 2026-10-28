//! Equipment sprite names: headgear view id -> "_name" (accessoryid.lub + accname.lub) and
//! weapon view -> "_name" (weapontable.lub). Paths:
//!   headgear: sprite\악세사리\{sex}\{sex}{name}
//!   weapon:   sprite\인간족\{job}\{job}_{sex}{name}

use std::collections::HashMap;

use ro_formats::lub;

#[derive(Default)]
pub struct LookTables {
    accessories: HashMap<u32, String>,
    weapons: HashMap<u32, String>,
}

impl LookTables {
    /// `accessory_ids` = accessoryid.lub, `accessory_names` = accname.lub, `weapons` = weapontable.lub.
    pub fn from_lubs(accessory_ids: &[u8], accessory_names: &[u8], weapons: &[u8]) -> anyhow::Result<Self> {
        let ids = lub::eval(accessory_ids)?;
        let names = lub::eval(accessory_names)?;
        let id_table = ids.globals.get("ACCESSORY_IDs").ok_or_else(|| anyhow::anyhow!("no ACCESSORY_IDs"))?;
        let name_table = names.globals.get("AccNameTable").ok_or_else(|| anyhow::anyhow!("no AccNameTable"))?;
        let by_name: HashMap<&str, &str> =
            names.entries(name_table).iter().filter_map(|(k, v)| Some((k.as_str()?, v.as_str()?))).collect();
        let accessories = ids
            .entries(id_table)
            .iter()
            .filter_map(|(k, v)| Some((v.as_number()? as u32, by_name.get(k.as_str()?)?.to_string())))
            .collect();

        let w = lub::eval(weapons)?;
        let table = w.globals.get("WeaponNameTable").ok_or_else(|| anyhow::anyhow!("no WeaponNameTable"))?;
        let weapons = w
            .entries(table)
            .iter()
            .filter_map(|(k, v)| Some((k.as_number()? as u32, v.as_str().filter(|s| !s.is_empty())?.to_string())))
            .collect();
        Ok(Self { accessories, weapons })
    }

    pub fn accessory(&self, view: u32) -> Option<&str> {
        self.accessories.get(&view).map(String::as_str)
    }

    /// Sprite suffix for a weapon view: a weapon type name, or `_{item id}` for weapons
    /// with their own sprite.
    pub fn weapon(&self, view: u32) -> Option<String> {
        match self.weapons.get(&view) {
            Some(n) => Some(n.clone()),
            None if view > 100 => Some(format!("_{view}")),
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_if_present() {
        let Ok(dir) = std::env::var("RO_LUB_DIR") else { return };
        let read = |n: &str| std::fs::read(format!("{dir}/{n}.lub")).ok();
        let (Some(a), Some(b), Some(c)) = (read("accessoryid"), read("accname"), read("weapontable")) else { return };
        let t = LookTables::from_lubs(&a, &b, &c).unwrap();
        assert_eq!(t.accessory(1), Some("_고글"));
        assert_eq!(t.weapon(1).as_deref(), Some("_단검"));
    }
}

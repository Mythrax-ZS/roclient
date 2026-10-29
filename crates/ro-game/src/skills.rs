//! Skill display names from the client's skillinfolist.lub
//! (`SKILL_INFO_LIST[SKID.NV_BASIC] = { "NV_BASIC", SkillName = "Basic Skill", ... }`).

use std::collections::HashMap;

use ro_formats::lub::{self, Value};

#[derive(Default)]
pub struct SkillNames {
    /// Internal name (as the server sends it) -> display name.
    names: HashMap<String, String>,
    /// Internal name -> description lines (skilldescript.lub; may hold ^RRGGBB colour codes).
    descriptions: HashMap<String, Vec<String>>,
    /// Skill id -> internal name (skillid.lub), for skills that are not ours (monsters, others).
    ids: HashMap<u16, String>,
}

impl SkillNames {
    /// Adds server-specific metadata after the stock tables have been loaded.
    pub fn insert(&mut self, id: u16, internal: &str, name: &str, description: Vec<String>) {
        self.ids.insert(id, internal.into());
        self.names.insert(internal.into(), name.into());
        self.descriptions.insert(internal.into(), description);
    }

    pub fn from_lub(data: &[u8]) -> anyhow::Result<Self> {
        let chunk = lub::eval(data)?;
        let list = chunk.globals.get("SKILL_INFO_LIST").ok_or_else(|| anyhow::anyhow!("no SKILL_INFO_LIST"))?;
        let names = chunk
            .entries(list)
            .iter()
            .filter_map(|(k, t)| {
                let internal = k.as_str()?.to_string();
                let name = chunk.field(t, "SkillName").and_then(Value::as_str)?.to_string();
                Some((internal, name))
            })
            .collect();
        Ok(Self { names, descriptions: HashMap::new(), ids: HashMap::new() })
    }

    /// Adds descriptions from skilldescript.lub (`SKILL_DESCRIPT[SKID.X] = { "line", ... }`).
    pub fn add_descriptions(&mut self, data: &[u8]) -> anyhow::Result<()> {
        let chunk = lub::eval(data)?;
        let list = chunk.globals.get("SKILL_DESCRIPT").ok_or_else(|| anyhow::anyhow!("no SKILL_DESCRIPT"))?;
        for (k, t) in chunk.entries(list) {
            let Some(internal) = k.as_str() else { continue };
            let lines = chunk.entries(t).iter().filter_map(|(_, v)| v.as_str().map(str::to_string)).collect();
            self.descriptions.insert(internal.to_string(), lines);
        }
        Ok(())
    }

    /// Adds the id table from skillid.lub (`SKID = { NV_BASIC = 1, ... }`).
    pub fn add_ids(&mut self, data: &[u8]) -> anyhow::Result<()> {
        let chunk = lub::eval(data)?;
        let list = chunk.globals.get("SKID").ok_or_else(|| anyhow::anyhow!("no SKID"))?;
        for (k, v) in chunk.entries(list) {
            if let (Some(name), Value::Number(id)) = (k.as_str(), v) {
                self.ids.insert(*id as u16, name.to_string());
            }
        }
        Ok(())
    }

    /// Every (id, internal name) pair known.
    pub fn all_ids(&self) -> impl Iterator<Item = (u16, &str)> {
        self.ids.iter().map(|(id, n)| (*id, n.as_str()))
    }

    /// Internal name (e.g. AS_GRIMTOOTH) for a skill id.
    pub fn internal(&self, id: u16) -> Option<&str> {
        self.ids.get(&id).map(String::as_str)
    }

    /// Skill id for an internal name.
    pub fn id_of(&self, internal: &str) -> Option<u16> {
        self.ids.iter().find(|(_, n)| n.as_str() == internal).map(|(id, _)| *id)
    }

    pub fn description(&self, internal: &str) -> Option<&[String]> {
        self.descriptions.get(internal).map(Vec::as_slice)
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// Display name for an internal skill name; falls back to a tidied internal name.
    pub fn display(&self, internal: &str) -> String {
        if let Some(n) = self.names.get(internal) {
            return n.clone();
        }
        internal.split_once('_').map_or(internal, |(_, rest)| rest).replace('_', " ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skillinfo_if_present() {
        let Ok(path) = std::env::var("RO_SKILLINFO") else { return };
        let names = SkillNames::from_lub(&std::fs::read(path).unwrap()).unwrap();
        assert!(names.len() > 100);
        assert_eq!(names.display("NV_BASIC"), "Basic Skill");
    }
}

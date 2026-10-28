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
}

impl SkillNames {
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
        Ok(Self { names, descriptions: HashMap::new() })
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

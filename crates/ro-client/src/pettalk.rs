//! Pet speech from data\pettalktable.xml, picked by ZC_PET_ACT's compound value
//! ((class - 100) * 100 + act * 10 + hunger), as rAthena's CZ_PET_ACT notes describe.

use std::collections::HashMap;

const HUNGER: [&str; 5] = ["hungry", "bit_hungry", "noting", "full", "so_full"];
/// Act ids 0..=9; rAthena sends 5 for the log-in greeting, so 5 reads `connect`
/// and falls back to `perfor_s`.
const ACTS: [&[&str]; 10] = [
    &["feeding"],
    &["hunting"],
    &["danger"],
    &["dead"],
    &["stand"],
    &["connect", "perfor_s"],
    &["levelup"],
    &["perfor_1"],
    &["perfor_2"],
    &["perfor_3"],
];

#[derive(Default)]
pub struct PetTalk {
    /// (MONSTER sprite name upper-case, hunger tag, act tag) -> lines.
    lines: HashMap<(String, String, String), Vec<String>>,
    turn: usize,
}

impl PetTalk {
    pub fn load(vfs: &ro_vfs::Vfs) -> Self {
        let Ok(Some(raw)) = vfs.read("data\\pettalktable.xml") else { return Self::default() };
        Self::parse(&String::from_utf8_lossy(&raw))
    }

    fn parse(xml: &str) -> Self {
        let mut lines: HashMap<(String, String, String), Vec<String>> = HashMap::new();
        let mut stack: Vec<String> = vec![];
        let mut rest = xml;
        while let Some(open) = rest.find('<') {
            let text = &rest[..open];
            let Some(close) = rest[open..].find('>') else { break };
            let tag = &rest[open + 1..open + close];
            rest = &rest[open + close + 1..];
            if tag.starts_with('?') || tag.starts_with('!') {
                continue;
            }
            if let Some(name) = tag.strip_prefix('/') {
                // Closing an act tag: stack is [table, monster, hunger, act].
                if stack.len() == 4 && stack[3].eq_ignore_ascii_case(name) {
                    let t = text.trim();
                    if !t.is_empty() {
                        lines.entry((stack[1].to_ascii_uppercase(), stack[2].to_ascii_lowercase(), stack[3].to_ascii_lowercase())).or_default().push(t.to_string());
                    }
                }
                stack.pop();
            } else {
                stack.push(tag.trim().to_string());
            }
        }
        Self { lines, turn: 0 }
    }

    /// The line for a compound value, given the pet's sprite name; None for plain emotions.
    pub fn line(&mut self, value: i32, sprite: impl Fn(i16) -> Option<String>) -> Option<String> {
        if value < 10_000 {
            return None;
        }
        let class = (value / 100 + 100) as i16;
        let act = ((value % 100) / 10) as usize;
        let hunger = HUNGER.get((value % 10) as usize)?;
        let name = sprite(class)?.to_ascii_uppercase();
        let list = ACTS[act].iter().find_map(|a| self.lines.get(&(name.clone(), hunger.to_string(), a.to_string())))?;
        self.turn = self.turn.wrapping_add(1 + std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos() as usize));
        Some(list[self.turn % list.len()].clone())
    }
}

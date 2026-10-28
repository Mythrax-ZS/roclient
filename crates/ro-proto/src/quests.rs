//! The quest log and quest markers over NPCs (rAthena clif_quest_* at PACKETVER 20250402).

use bytes::{Buf, BufMut, BytesMut};

use crate::codec::Encode;
use crate::packets::get_fixed_str;

#[derive(Debug, Clone, PartialEq)]
pub struct Objective {
    /// quest id * 1000 + index; progress updates name it.
    pub hunt_id: u32,
    pub mob_id: u32,
    pub count: u16,
    pub total: u16,
    /// Monster (or race/size/element) to hunt, as the server words it.
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Quest {
    pub id: u32,
    pub active: bool,
    /// Unix time it expires (0: no limit).
    pub end: u32,
    pub objectives: Vec<Objective>,
}

/// Objectives as the list sends them (44 bytes) or as a new quest does (42: no total,
/// which the first progress update fills in).
fn objectives(b: &mut &[u8], n: usize, with_total: bool) -> Vec<Objective> {
    let mut out = vec![];
    for _ in 0..n {
        if b.len() < if with_total { 44 } else { 42 } {
            break;
        }
        let hunt_id = b.get_u32_le();
        let _kind = b.get_u32_le();
        let mob_id = b.get_u32_le();
        let (_min, _max) = (b.get_u16_le(), b.get_u16_le());
        let count = b.get_u16_le();
        let total = if with_total { b.get_u16_le() } else { 0 };
        let name = get_fixed_str(b, 24);
        out.push(Objective { hunt_id, mob_id, count, total, name });
    }
    out
}

/// ZC_ALL_QUEST_LIST3 (0x09f8): every quest in the log.
pub fn parse_quest_list(data: &[u8]) -> Vec<Quest> {
    let Some(mut b) = data.get(8..) else { return vec![] };
    let mut out = vec![];
    while b.len() >= 15 {
        let id = b.get_u32_le();
        let active = b.get_u8() != 0;
        let _start = b.get_u32_le();
        let end = b.get_u32_le();
        let n = b.get_u16_le() as usize;
        let objectives = objectives(&mut b, n, true);
        out.push(Quest { id, active, end, objectives });
    }
    out
}

/// ZC_ADD_QUEST3 (0x09f9): a quest was taken.
pub fn parse_quest_add(data: &[u8]) -> Option<Quest> {
    let mut b = data.get(2..)?;
    let id = b.get_u32_le();
    let active = b.get_u8() != 0;
    let _start = b.get_u32_le();
    let end = b.get_u32_le();
    let n = b.get_u16_le() as usize;
    Some(Quest { id, active, end, objectives: objectives(&mut b, n, false) })
}

/// ZC_UPDATE_MISSION_HUNT_EX (0x09fa): hunting progress, (quest, hunt id, total, count).
pub fn parse_hunt_update(data: &[u8]) -> Vec<(u32, u32, u16, u16)> {
    let Some(mut b) = data.get(6..) else { return vec![] };
    let mut out = vec![];
    while b.len() >= 12 {
        out.push((b.get_u32_le(), b.get_u32_le(), b.get_u16_le(), b.get_u16_le()));
    }
    out
}

/// ZC_QUEST_NOTIFY_EFFECT (0x0446): a marker over an NPC, (npc, mark, colour). Mark 9999
/// removes it.
pub fn parse_quest_mark(data: &[u8]) -> Option<(u32, u16, u16)> {
    let mut b = data.get(2..14)?;
    let npc = b.get_u32_le();
    let (_x, _y) = (b.get_u16_le(), b.get_u16_le());
    Some((npc, b.get_u16_le(), b.get_u16_le()))
}

/// CZ_ACTIVE_QUEST: show or hide a quest in the active list.
pub struct CzActiveQuest {
    pub quest: u32,
    pub active: bool,
}

impl Encode for CzActiveQuest {
    fn encode(&self, d: &mut BytesMut) {
        d.put_u16_le(0x02b6);
        d.put_u32_le(self.quest);
        d.put_u8(self.active as u8);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_list() {
        let mut p = vec![0xf8, 0x09, 0, 0];
        p.extend(1u32.to_le_bytes());
        p.extend(7000u32.to_le_bytes());
        p.push(1);
        p.extend(0u32.to_le_bytes());
        p.extend(0u32.to_le_bytes());
        p.extend(1u16.to_le_bytes());
        p.extend(7000000u32.to_le_bytes());
        p.extend(0u32.to_le_bytes());
        p.extend(1002u32.to_le_bytes());
        p.extend([0u8; 4]);
        p.extend(3u16.to_le_bytes());
        p.extend(10u16.to_le_bytes());
        let mut name = [0u8; 24];
        name[..6].copy_from_slice(b"Poring");
        p.extend(name);
        let q = parse_quest_list(&p);
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].objectives[0], Objective { hunt_id: 7000000, mob_id: 1002, count: 3, total: 10, name: "Poring".into() });
    }
}

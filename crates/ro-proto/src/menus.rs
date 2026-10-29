//! Pick-one menus that skills and items open: poisons and arrows to make or apply, items to
//! brew or forge, warp destinations, auto-spells, items to appraise, eggs to hatch. Each is
//! a list from the server answered by one choice (packets as rAthena sends them at
//! PACKETVER 20250402).

use bytes::{Buf, BufMut, BytesMut};

use crate::codec::Encode;
use crate::packets::get_fixed_str;

#[derive(Debug, Clone, PartialEq)]
pub enum Menu {
    /// ZC_MAKINGARROW_LIST (0x01ad): arrows to craft, poisons to coat a weapon with
    /// (Poisoning Weapon), decoys, elemental converters... Answered with an item id.
    Items(Vec<u32>),
    /// ZC_MAKABLEITEMLIST (0x018d): items to forge or brew, with their material ids.
    Produce(Vec<(u32, [u32; 3])>),
    /// ZC_MAKINGITEM_LIST (0x025a): cooking and other "make" lists of a kind.
    Make { kind: u16, items: Vec<u32> },
    /// ZC_WARPLIST (0x0abe): Warp Portal / Teleport destinations.
    Warp { skill: u16, maps: Vec<String> },
    /// ZC_AUTOSPELLLIST (0x0afb): spells Auto Spell can cast.
    AutoSpell(Vec<u32>),
    /// 0x0177: unidentified items Item Appraisal can identify (inventory indices).
    Identify(Vec<u16>),
    /// ZC_SKILL_SELECT_REQUEST (0x0442): a skill to pick (Auto Shadow Spell).
    Skill { flag: i32, skills: Vec<u16> },
    /// 0x01a6: pet eggs to hatch (inventory indices).
    Egg(Vec<u16>),
    /// ZC_REPAIRITEMLIST (0x0b65): damaged items Weapon Repair can fix: (index, item id, refine).
    /// The index is in the inventory of whoever is being repaired.
    Repair(Vec<(u16, u32, u8)>),
    /// ZC_NOTIFY_WEAPONITEMLIST (0x0221): weapons Weapon Refine can upgrade: (index, item id, refine).
    WeaponRefine(Vec<(u16, u32, u8)>),
    /// ZC_ITEMLISTWIN_OPEN (0x07e3): inventory items to hand in, with amounts. Kind 0 is
    /// Change Material; 1 and 2 are Four Spirit Analysis (pure to rough, rough to pure).
    Materials(u32),
}

pub fn parse_menu(opcode: u16, data: &[u8]) -> Option<Menu> {
    let body = data.get(4..)?;
    let u32s = |b: &[u8]| b.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect::<Vec<_>>();
    let u16s = |b: &[u8]| b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>();
    Some(match opcode {
        0x01ad => Menu::Items(u32s(body)),
        0x018d => Menu::Produce(
            body.chunks_exact(16)
                .map(|c| {
                    let v = u32s(c);
                    (v[0], [v[1], v[2], v[3]])
                })
                .collect(),
        ),
        0x025a => Menu::Make { kind: u16::from_le_bytes([*body.first()?, *body.get(1)?]), items: u32s(body.get(2..)?) },
        0x0abe => {
            let skill = u16::from_le_bytes([*body.first()?, *body.get(1)?]);
            let maps = body[2..].chunks_exact(16).map(|mut c| get_fixed_str(&mut c, 16)).filter(|m| !m.is_empty()).collect();
            Menu::Warp { skill, maps }
        }
        0x0afb => Menu::AutoSpell(u32s(body).into_iter().filter(|&s| s != 0).collect()),
        0x0177 => Menu::Identify(u16s(body)),
        0x0442 => {
            let mut b = body;
            let flag = b.get_i32_le();
            Menu::Skill { flag, skills: u16s(b).into_iter().filter(|&s| s != 0).collect() }
        }
        0x01a6 => Menu::Egg(u16s(body)),
        0x0b65 => Menu::Repair(body.chunks_exact(24).map(|c| (u16::from_le_bytes([c[0], c[1]]), u32::from_le_bytes([c[2], c[3], c[4], c[5]]), c[22])).collect()),
        0x0221 => Menu::WeaponRefine(body.chunks_exact(23).map(|c| (u16::from_le_bytes([c[0], c[1]]), u32::from_le_bytes([c[2], c[3], c[4], c[5]]), c[6])).collect()),
        0x07e3 => Menu::Materials(u32::from_le_bytes(data.get(2..6)?.try_into().ok()?)),
        _ => return None,
    })
}

/// The answer to a [`Menu`].
#[derive(Debug, Clone, PartialEq)]
pub enum MenuReply {
    Item(u32),
    Produce { item: u32, materials: [u32; 3] },
    Make { kind: u16, item: u32 },
    /// "cancel" closes the portal menu without casting.
    Warp { skill: u16, map: String },
    AutoSpell(u32),
    Identify(u16),
    Skill { flag: i32, skill: u16 },
    Egg(u16),
    Repair { index: u16, item: u32, refine: u8 },
    WeaponRefine(u16),
    /// (inventory index, amount) pairs; empty cancels.
    Materials { kind: u32, items: Vec<(u16, u16)> },
}

impl Encode for MenuReply {
    fn encode(&self, d: &mut BytesMut) {
        match self {
            MenuReply::Item(id) => {
                d.put_u16_le(0x01ae);
                d.put_u32_le(*id);
            }
            MenuReply::Produce { item, materials } => {
                d.put_u16_le(0x018e);
                d.put_u32_le(*item);
                for m in materials {
                    d.put_u32_le(*m);
                }
            }
            MenuReply::Make { kind, item } => {
                d.put_u16_le(0x025b);
                d.put_u16_le(*kind);
                d.put_u32_le(*item);
            }
            MenuReply::Warp { skill, map } => {
                d.put_u16_le(0x011b);
                d.put_u16_le(*skill);
                let mut name = [0u8; 16];
                let n = map.len().min(15);
                name[..n].copy_from_slice(&map.as_bytes()[..n]);
                d.put_slice(&name);
            }
            MenuReply::AutoSpell(skill) => {
                d.put_u16_le(0x01ce);
                d.put_u32_le(*skill);
            }
            MenuReply::Identify(index) => {
                d.put_u16_le(0x0178);
                d.put_u16_le(*index);
            }
            MenuReply::Skill { flag, skill } => {
                d.put_u16_le(0x0443);
                d.put_i32_le(*flag);
                d.put_u16_le(*skill);
            }
            MenuReply::Egg(index) => {
                d.put_u16_le(0x01a7);
                d.put_u16_le(*index);
            }
            MenuReply::Repair { index, item, refine } => {
                d.put_u16_le(0x0b66);
                d.put_u16_le(*index);
                d.put_u32_le(*item);
                d.put_slice(&[0; 16]);
                d.put_u8(*refine);
                d.put_u8(0);
            }
            MenuReply::WeaponRefine(index) => {
                d.put_u16_le(0x0222);
                d.put_u32_le(*index as u32);
            }
            MenuReply::Materials { kind, items } => {
                d.put_u16_le(0x07e4);
                d.put_u16_le(12 + 4 * items.len() as u16);
                d.put_u32_le(*kind);
                d.put_u32_le(!items.is_empty() as u32);
                for &(index, amount) in items {
                    d.put_u16_le(index);
                    d.put_u16_le(amount);
                }
            }
        }
    }
}

/// ZC_ACK_REQMAKINGITEM (0x018f): (succeeded, item id).
pub fn parse_make_result(data: &[u8]) -> Option<(bool, u32)> {
    let mut b = data.get(2..8)?;
    let result = b.get_u16_le();
    Some((result == 0, b.get_u32_le()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_poison_list() {
        let mut p = vec![0xad, 0x01, 12, 0];
        p.extend(12717u32.to_le_bytes());
        p.extend(12718u32.to_le_bytes());
        assert_eq!(parse_menu(0x01ad, &p), Some(Menu::Items(vec![12717, 12718])));
    }

    #[test]
    fn encodes_materials() {
        let mut d = BytesMut::new();
        MenuReply::Materials { kind: 1, items: vec![(5, 3)] }.encode(&mut d);
        assert_eq!(&d[..], &[0xe4, 0x07, 16, 0, 1, 0, 0, 0, 1, 0, 0, 0, 5, 0, 3, 0]);
        let mut d = BytesMut::new();
        MenuReply::Materials { kind: 0, items: vec![] }.encode(&mut d);
        assert_eq!(&d[8..12], &[0, 0, 0, 0]);
    }

    #[test]
    fn parses_weapon_refine_list() {
        let mut p = vec![0x21, 0x02, 27, 0, 7, 0];
        p.extend(1201u32.to_le_bytes());
        p.push(4);
        p.extend([0; 16]);
        assert_eq!(parse_menu(0x0221, &p), Some(Menu::WeaponRefine(vec![(7, 1201, 4)])));
    }

    #[test]
    fn encodes_warp() {
        let mut d = BytesMut::new();
        MenuReply::Warp { skill: 27, map: "prontera.gat".into() }.encode(&mut d);
        assert_eq!(d.len(), 20);
        assert_eq!(&d[4..16], b"prontera.gat");
    }
}

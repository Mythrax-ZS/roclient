//! Companions: the pet, the homunculus and the mercenary, as rAthena sends them at
//! PACKETVER 20250402.

use bytes::{Buf, BufMut, BytesMut};

use crate::codec::Encode;
use crate::packets::get_fixed_str;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PetInfo {
    pub name: String,
    pub renamed: bool,
    pub level: u16,
    pub hunger: u16,
    pub intimacy: u16,
    pub accessory: u16,
    pub class: u16,
}

/// ZC_PROPERTY_PET (0x01a2).
pub fn parse_pet(data: &[u8]) -> Option<PetInfo> {
    let mut b = data.get(2..37)?;
    let name = get_fixed_str(&mut b, 24);
    Some(PetInfo { name, renamed: b.get_u8() != 0, level: b.get_u16_le(), hunger: b.get_u16_le(), intimacy: b.get_u16_le(), accessory: b.get_u16_le(), class: b.get_u16_le() })
}

/// ZC_CHANGESTATE_PET (0x01a4) / ZC_CHANGESTATE_MER (0x0230): (state, unit id, value).
/// Pet states: 0 init, 1 intimacy, 2 hunger, 3 accessory, 4 performance.
/// Homunculus states: 0 init, 1 intimacy, 2 hunger.
pub fn parse_changestate(opcode: u16, data: &[u8]) -> Option<(u8, u32, i32)> {
    let mut b = data.get(2..)?;
    let state = if opcode == 0x0230 {
        let s = b.get(1).copied()?;
        b.advance(2);
        s
    } else {
        b.get_u8()
    };
    if b.len() < 8 {
        return None;
    }
    Some((state, b.get_u32_le(), b.get_i32_le()))
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct HomunInfo {
    pub name: String,
    pub renamed: bool,
    pub resting: bool,
    pub alive: bool,
    pub level: u16,
    pub hunger: u16,
    pub intimacy: u16,
    pub atk: u16,
    pub matk: u16,
    pub hit: u16,
    pub crit: u16,
    pub def: u16,
    pub mdef: u16,
    pub flee: u16,
    pub aspd: u16,
    pub hp: u32,
    pub max_hp: u32,
    pub sp: u32,
    pub max_sp: u32,
    pub exp: i64,
    pub exp_next: i64,
    pub skill_points: u16,
}

/// ZC_PROPERTY_HOMUN4 (0x0ba4).
pub fn parse_homun(data: &[u8]) -> Option<HomunInfo> {
    let mut b = data.get(2..85)?;
    let name = get_fixed_str(&mut b, 24);
    let flags = b.get_u8();
    let mut h = HomunInfo { name, renamed: flags & 1 != 0, resting: flags & 2 != 0, alive: flags & 4 != 0, ..Default::default() };
    h.level = b.get_u16_le();
    h.hunger = b.get_u16_le();
    h.intimacy = b.get_u16_le();
    h.atk = b.get_u16_le();
    h.matk = b.get_u16_le();
    h.hit = b.get_u16_le();
    h.crit = b.get_u16_le();
    h.def = b.get_u16_le();
    h.mdef = b.get_u16_le();
    h.flee = b.get_u16_le();
    h.aspd = b.get_u16_le();
    h.hp = b.get_u32_le();
    h.max_hp = b.get_u32_le();
    h.sp = b.get_u32_le();
    h.max_sp = b.get_u32_le();
    h.exp = b.get_i64_le();
    h.exp_next = b.get_i64_le();
    h.skill_points = b.get_u16_le();
    Some(h)
}

/// A homunculus or mercenary skill: (id, level, sp, range, name, upgradable).
#[derive(Debug, Clone, PartialEq)]
pub struct CompanionSkill {
    pub id: u16,
    pub level: u16,
    pub sp: u16,
    pub range: u16,
    pub name: String,
    pub upgradable: bool,
}

/// ZC_HOSKILLINFO_LIST (0x0235) / ZC_MER_SKILLINFO_LIST (0x029d), 37 bytes per skill.
pub fn parse_companion_skills(opcode: u16, data: &[u8]) -> Vec<CompanionSkill> {
    let Some(mut b) = data.get(4..) else { return vec![] };
    let mut out = vec![];
    while b.len() >= 37 {
        let id = b.get_u16_le();
        b.advance(4); // target type (homunculus: inf + unknown; mercenary: inf.L)
        let _ = opcode;
        let level = b.get_u16_le();
        let sp = b.get_u16_le();
        let range = b.get_u16_le();
        let name = get_fixed_str(&mut b, 24);
        let upgradable = b.get_u8() != 0;
        out.push(CompanionSkill { id, level, sp, range, name, upgradable });
    }
    out
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MercInfo {
    pub id: u32,
    pub atk: u16,
    pub matk: u16,
    pub hit: u16,
    pub crit: u16,
    pub def: u16,
    pub mdef: u16,
    pub flee: u16,
    pub aspd: u16,
    pub name: String,
    pub level: u16,
    pub hp: u32,
    pub max_hp: u32,
    pub sp: u32,
    pub max_sp: u32,
    pub faith: u16,
    pub calls: u32,
    pub kills: u32,
}

/// ZC_MER_INIT (0x029b).
pub fn parse_merc(data: &[u8]) -> Option<MercInfo> {
    let mut b = data.get(2..80)?;
    let mut m = MercInfo { id: b.get_u32_le(), ..Default::default() };
    m.atk = b.get_u16_le();
    m.matk = b.get_u16_le();
    m.hit = b.get_u16_le();
    m.crit = b.get_u16_le();
    m.def = b.get_u16_le();
    m.mdef = b.get_u16_le();
    m.flee = b.get_u16_le();
    m.aspd = b.get_u16_le();
    m.name = get_fixed_str(&mut b, 24);
    m.level = b.get_u16_le();
    m.hp = b.get_u32_le();
    m.max_hp = b.get_u32_le();
    m.sp = b.get_u32_le();
    m.max_sp = b.get_u32_le();
    b.advance(4); // expiry tick
    m.faith = b.get_u16_le();
    m.calls = b.get_u32_le();
    m.kills = b.get_u32_le();
    Some(m)
}

/// ZC_MER_PAR_CHANGE (0x02a2): (status id, value); 5/6/7/8 are hp/max hp/sp/max sp.
pub fn parse_merc_param(data: &[u8]) -> Option<(u16, i32)> {
    let mut b = data.get(2..8)?;
    Some((b.get_u16_le(), b.get_i32_le()))
}

fn name24(d: &mut BytesMut, name: &str) {
    let (t, _, _) = encoding_rs::EUC_KR.encode(name);
    let mut buf = [0u8; 24];
    let n = t.len().min(23);
    buf[..n].copy_from_slice(&t[..n]);
    d.put_slice(&buf);
}

#[derive(Debug, Clone, PartialEq)]
pub enum CompanionRequest {
    /// CZ_TRYCAPTURE_MONSTER: try to tame this monster.
    Capture(u32),
    /// CZ_COMMAND_PET: 0 info, 1 feed, 2 performance, 3 return to egg, 4 unequip accessory.
    PetMenu(u8),
    /// CZ_RENAME_PET.
    PetRename(String),
    /// CZ_COMMAND_MER: 0 info, 1 feed, 2 delete.
    HomMenu(u8),
    /// CZ_RENAME_MER.
    HomRename(String),
    /// CZ_REQUEST_ACTNPC: the homunculus or mercenary attacks a target.
    Attack { id: u32, target: u32 },
    /// CZ_REQUEST_MOVETOOWNER: come back to the owner.
    Return(u32),
    /// CZ_MER_COMMAND: 1 info, 2 dismiss.
    MercMenu(u8),
    /// CZ_REQUEST_MOVENPC: walk the homunculus or mercenary to a cell (the client runs their AI).
    Move { id: u32, x: u16, y: u16 },
}

impl Encode for CompanionRequest {
    fn encode(&self, d: &mut BytesMut) {
        match self {
            CompanionRequest::Capture(id) => {
                d.put_u16_le(0x019f);
                d.put_u32_le(*id);
            }
            CompanionRequest::PetMenu(c) => {
                d.put_u16_le(0x01a1);
                d.put_u8(*c);
            }
            CompanionRequest::PetRename(n) => {
                d.put_u16_le(0x01a5);
                name24(d, n);
            }
            CompanionRequest::HomMenu(c) => {
                d.put_u16_le(0x022d);
                d.put_u16_le(0);
                d.put_u8(*c);
            }
            CompanionRequest::HomRename(n) => {
                d.put_u16_le(0x0231);
                name24(d, n);
            }
            CompanionRequest::Attack { id, target } => {
                d.put_u16_le(0x0233);
                d.put_u32_le(*id);
                d.put_u32_le(*target);
                d.put_u8(0);
            }
            CompanionRequest::Return(id) => {
                d.put_u16_le(0x0234);
                d.put_u32_le(*id);
            }
            CompanionRequest::Move { id, x, y } => {
                d.put_u16_le(0x0232);
                d.put_u32_le(*id);
                d.put_slice(&crate::packets::pack_pos(*x, *y, 0));
            }
            CompanionRequest::MercMenu(c) => {
                d.put_u16_le(0x029f);
                d.put_u8(*c);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        for (r, n) in [
            (CompanionRequest::PetMenu(1), 3),
            (CompanionRequest::PetRename("Poe".into()), 26),
            (CompanionRequest::HomMenu(1), 5),
            (CompanionRequest::Attack { id: 1, target: 2 }, 11),
            (CompanionRequest::Return(1), 6),
            (CompanionRequest::MercMenu(2), 3),
            (CompanionRequest::Move { id: 1, x: 2, y: 3 }, 9),
        ] {
            let mut d = BytesMut::new();
            r.encode(&mut d);
            assert_eq!(d.len(), n, "{r:?}");
        }
    }
}

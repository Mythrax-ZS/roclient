//! The refine window (rAthena feature.refineui, PACKETVER 20250402).

use bytes::{Buf, BufMut, BytesMut};

use crate::codec::Encode;

/// One way to refine: the material, success chance (%) and zeny.
#[derive(Debug, Clone, PartialEq)]
pub struct RefineOption {
    pub material: u32,
    pub chance: u8,
    pub zeny: u32,
}

/// ZC_REFINING_MATERIAL_LIST (0x0aa2): (item index, Blacksmith Blessings it takes, options).
pub fn parse_refine_info(data: &[u8]) -> Option<(u16, u8, Vec<RefineOption>)> {
    let mut b = data.get(4..)?;
    if b.len() < 3 {
        return None;
    }
    let index = b.get_u16_le();
    let blessing = b.get_u8();
    let mut out = vec![];
    while b.len() >= 9 {
        out.push(RefineOption { material: b.get_u32_le(), chance: b.get_u8(), zeny: b.get_u32_le() });
    }
    Some((index, blessing, out))
}

/// ZC_ACK_ITEMREFINING (0x0188): (result: 0 ok, 1 broke, 2 went down, 3 failed; index; refine).
pub fn parse_refine_result(data: &[u8]) -> Option<(u16, u16, u16)> {
    let mut b = data.get(2..8)?;
    Some((b.get_u16_le(), b.get_u16_le(), b.get_u16_le()))
}

#[derive(Debug, Clone, PartialEq)]
pub enum RefineRequest {
    /// CZ_REFINING_SELECT_ITEM: show the options for this item.
    Select(u16),
    /// CZ_REQ_REFINING.
    Refine { index: u16, material: u32, blessing: bool },
    /// CZ_CLOSE_REFINING_UI.
    Close,
}

impl Encode for RefineRequest {
    fn encode(&self, d: &mut BytesMut) {
        match self {
            RefineRequest::Select(index) => {
                d.put_u16_le(0x0aa1);
                d.put_u16_le(*index);
            }
            RefineRequest::Refine { index, material, blessing } => {
                d.put_u16_le(0x0aa3);
                d.put_u16_le(*index);
                d.put_u32_le(*material);
                d.put_u8(*blessing as u8);
            }
            RefineRequest::Close => d.put_u16_le(0x0aa4),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_options() {
        let mut p = vec![0xa2, 0x0a, 0, 0, 5, 0, 0];
        p.extend(1010u32.to_le_bytes());
        p.push(100);
        p.extend(50u32.to_le_bytes());
        assert_eq!(parse_refine_info(&p), Some((5, 0, vec![RefineOption { material: 1010, chance: 100, zeny: 50 }])));
    }
}

/// One way to raise an item's grade: a material, how many, the zeny, and what a failure
/// does.
#[derive(Debug, Clone, PartialEq)]
pub struct GradeMaterial {
    pub item: u32,
    pub amount: u32,
    pub zeny: u32,
    /// A failure lowers the grade.
    pub downgrade: bool,
    /// A failure can destroy the item.
    pub breakable: bool,
}

/// ZC_GRADE_ENCHANT_MATERIAL_LIST (0x0b5a).
#[derive(Debug, Clone, PartialEq)]
pub struct GradeOptions {
    /// The item's inventory index; None when it can't be graded further.
    pub index: Option<u16>,
    /// Success chance in percent.
    pub chance: u32,
    /// The optional catalyst ("blessing"): item, amount per step, steps, percent per step.
    pub blessing: (u32, u32, u32, u32),
    pub materials: Vec<GradeMaterial>,
}

pub fn parse_grade_options(data: &[u8]) -> Option<GradeOptions> {
    let mut b = data.get(4..)?;
    if b.len() < 30 {
        return None;
    }
    let index = b.get_i16_le();
    let chance = b.get_i32_le().max(0) as u32;
    let blessing = (b.get_u32_le(), b.get_u32_le(), b.get_u32_le(), b.get_u32_le());
    b.advance(8); // protect item, amount
    let mut materials = vec![];
    while b.len() >= 17 {
        let (item, amount, zeny, downgrade) = (b.get_u32_le(), b.get_u32_le(), b.get_u32_le(), b.get_i32_le() != 0);
        materials.push(GradeMaterial { item, amount, zeny, downgrade, breakable: b.get_u8() != 0 });
    }
    Some(GradeOptions { index: (index >= 0).then_some(index as u16), chance, blessing, materials })
}

/// ZC_GRADE_ENCHANT_ACK (0x0b5d): (index, new grade, result: 0 up, 1 failed, 2 grade
/// lowered, 3 destroyed, 4 protected).
pub fn parse_grade_result(data: &[u8]) -> Option<(u16, u16, u32)> {
    let mut b = data.get(2..10)?;
    Some((b.get_u16_le(), b.get_u16_le(), b.get_u32_le()))
}

/// ZC_GRADE_ENCHANT_BROADCAST_RESULT (0x0b5e): (who, item, grade, success).
pub fn parse_grade_broadcast(data: &[u8]) -> Option<(String, u32, u16, bool)> {
    let mut b = data.get(2..33)?;
    let name = crate::packets::get_fixed_str(&mut b, 24);
    Some((name, b.get_u32_le(), b.get_u16_le(), b.get_u8() != 0))
}

#[derive(Debug, Clone, PartialEq)]
pub enum GradeRequest {
    /// CZ_GRADE_ENCHANT_SELECT_EQUIPMENT.
    Select(u16),
    /// CZ_GRADE_ENCHANT_REQUEST: material is the index in the option list; blessing the
    /// number of catalyst steps (0 for none).
    Upgrade { index: u16, material: u32, blessing: u32 },
    Close,
}

impl Encode for GradeRequest {
    fn encode(&self, d: &mut BytesMut) {
        match self {
            GradeRequest::Select(index) => {
                d.put_u16_le(0x0b59);
                d.put_u16_le(*index);
            }
            GradeRequest::Upgrade { index, material, blessing } => {
                d.put_u16_le(0x0b5b);
                d.put_u16_le(*index);
                d.put_u32_le(*material);
                d.put_u8((*blessing > 0) as u8);
                d.put_u32_le(*blessing);
                d.put_u8(0);
            }
            GradeRequest::Close => d.put_u16_le(0x0b5c),
        }
    }
}

/// Random enchant requests (the "Enchant" UI); `group` is the set the server opened.
#[derive(Debug, Clone, PartialEq)]
pub enum EnchantRequest {
    Random { group: u64, index: u16 },
    Perfect { group: u64, index: u16, item: u32 },
    Upgrade { group: u64, index: u16, slot: u16 },
    Reset { group: u64, index: u16 },
    Close,
}

impl Encode for EnchantRequest {
    fn encode(&self, d: &mut BytesMut) {
        match self {
            EnchantRequest::Random { group, index } | EnchantRequest::Reset { group, index } => {
                d.put_u16_le(if matches!(self, EnchantRequest::Random { .. }) { 0x0b9b } else { 0x0b9e });
                d.put_u64_le(*group);
                d.put_u16_le(*index);
            }
            EnchantRequest::Perfect { group, index, item } => {
                d.put_u16_le(0x0b9c);
                d.put_u64_le(*group);
                d.put_u16_le(*index);
                d.put_u32_le(*item);
            }
            EnchantRequest::Upgrade { group, index, slot } => {
                d.put_u16_le(0x0b9d);
                d.put_u64_le(*group);
                d.put_u16_le(*index);
                d.put_u16_le(*slot);
            }
            EnchantRequest::Close => d.put_u16_le(0x0ba0),
        }
    }
}

/// CZ_ITEM_REFORM (tool item, inventory index) / CZ_CLOSE_REFORM_UI.
#[derive(Debug, Clone, PartialEq)]
pub enum ReformRequest {
    Reform { tool: u32, index: u16 },
    Close,
}

impl Encode for ReformRequest {
    fn encode(&self, d: &mut BytesMut) {
        match self {
            ReformRequest::Reform { tool, index } => {
                d.put_u16_le(0x0b91);
                d.put_u32_le(*tool);
                d.put_u16_le(*index);
            }
            ReformRequest::Close => d.put_u16_le(0x0b90),
        }
    }
}

/// Laphine synthesis (combine several items into a reward box) and Laphine upgrade
/// (re-roll one item), opened by item scripts.
#[derive(Debug, Clone, PartialEq)]
pub enum LapineRequest {
    /// CZ_REQ_RANDOM_COMBINE_ITEM: the items handed in, as (inventory index, count).
    Synthesize { trigger: u32, items: Vec<(u16, u16)> },
    /// CZ_RANDOM_COMBINE_ITEM_UI_CLOSE.
    SynthesisClose,
    /// CZ_REQ_RANDOM_UPGRADE_ITEM.
    Upgrade { trigger: u32, index: u16 },
    /// CZ_RANDOM_UPGRADE_ITEM_UI_CLOSE.
    UpgradeClose,
}

impl Encode for LapineRequest {
    fn encode(&self, d: &mut BytesMut) {
        match self {
            LapineRequest::Synthesize { trigger, items } => {
                d.put_u16_le(0x0a4f);
                d.put_u16_le((8 + 4 * items.len()) as u16);
                d.put_u32_le(*trigger);
                for (index, count) in items {
                    d.put_u16_le(*index);
                    d.put_u16_le(*count);
                }
            }
            LapineRequest::SynthesisClose => d.put_u16_le(0x0a70),
            LapineRequest::Upgrade { trigger, index } => {
                d.put_u16_le(0x0ab6);
                d.put_u32_le(*trigger);
                d.put_u16_le(*index);
            }
            LapineRequest::UpgradeClose => d.put_u16_le(0x0ab5),
        }
    }
}

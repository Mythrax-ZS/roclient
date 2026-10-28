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

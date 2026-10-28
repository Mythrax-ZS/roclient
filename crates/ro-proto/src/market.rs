//! NPC market shops (limited stock, `marketshop`) and barter shops (trade items for items,
//! optionally plus zeny: npc/barters.yml), as rAthena sends them at PACKETVER 20250402.

use bytes::{Buf, BufMut, BytesMut};

use crate::codec::Encode;

#[derive(Debug, Clone, PartialEq)]
pub struct MarketItem {
    pub item: u32,
    pub kind: u8,
    pub price: u32,
    /// Stock left.
    pub stock: u32,
    pub weight: u16,
}

/// ZC_NPC_MARKET_OPEN (0x0b7a).
pub fn parse_market(data: &[u8]) -> Vec<MarketItem> {
    data.get(4..)
        .unwrap_or_default()
        .chunks_exact(19)
        .map(|mut b| {
            let item = b.get_u32_le();
            let kind = b.get_u8();
            let (price, stock, weight) = (b.get_u32_le(), b.get_u32_le(), b.get_u16_le());
            MarketItem { item, kind, price, stock, weight }
        })
        .collect()
}

/// ZC_NPC_MARKET_PURCHASE_RESULT (0x0b4e): (ok, items bought as (item, amount, price)).
pub fn parse_market_result(data: &[u8]) -> Option<(bool, Vec<(u32, u16, u32)>)> {
    let mut b = data.get(4..)?;
    if b.len() < 2 {
        return None;
    }
    // 0 is success at this version (-1 failure).
    let ok = b.get_u16_le() == 0;
    let list = b.chunks_exact(10).map(|mut s| (s.get_u32_le(), s.get_u16_le(), s.get_u32_le())).collect();
    Some((ok, list))
}

/// One thing a barter shop sells and what it costs.
#[derive(Debug, Clone, PartialEq)]
pub struct BarterItem {
    pub item: u32,
    /// Stock left (0 when unlimited in the expanded form; see `limited`).
    pub stock: u32,
    pub zeny: u32,
    pub refine: u32,
    /// The shop's own index, sent back when buying.
    pub index: u32,
    pub weight: u32,
    /// (item, amount, refine) to hand over.
    pub costs: Vec<(u32, u32, u16)>,
}

/// ZC_NPC_BARTER_MARKET_ITEMINFO (0x0b78): one required item each, no zeny.
pub fn parse_barter(data: &[u8]) -> Vec<BarterItem> {
    data.get(4..)
        .unwrap_or_default()
        .chunks_exact(31)
        .map(|mut b| {
            let item = b.get_u32_le();
            b.advance(1); // type
            let stock = b.get_u32_le();
            let (cost, amount, weight, index) = (b.get_u32_le(), b.get_u32_le(), b.get_u32_le(), b.get_u32_le());
            BarterItem { item, stock, zeny: 0, refine: 0, index, weight, costs: vec![(cost, amount, 0)] }
        })
        .collect()
}

/// ZC_NPC_EXPANDED_BARTER_MARKET_ITEMINFO (0x0b79): several required items plus zeny.
pub fn parse_barter_expanded(data: &[u8]) -> Vec<BarterItem> {
    let Some(mut b) = data.get(8..) else { return vec![] };
    let mut out = vec![];
    while b.len() >= 36 {
        let item = b.get_u32_le();
        b.advance(2); // type
        let stock = b.get_u32_le();
        let (weight, index, zeny) = (b.get_u32_le(), b.get_u32_le(), b.get_u32_le());
        b.advance(2 + 4); // view sprite, location
        let (count, refine) = (b.get_u32_le() as usize, b.get_u32_le());
        let mut costs = vec![];
        for _ in 0..count {
            if b.len() < 12 {
                break;
            }
            let cost = b.get_u32_le();
            let r = b.get_u16_le();
            let amount = b.get_u32_le();
            b.advance(2); // type
            costs.push((cost, amount, r));
        }
        out.push(BarterItem { item, stock, zeny, refine, index, weight, costs });
    }
    out
}

#[derive(Debug, Clone, PartialEq)]
pub enum MarketRequest {
    /// CZ_NPC_MARKET_PURCHASE: (item, amount).
    MarketBuy(Vec<(u32, u32)>),
    MarketClose,
    /// CZ_NPC_BARTER_MARKET_PURCHASE: (item, amount, shop index).
    BarterBuy(Vec<(u32, u32, u32)>),
    BarterClose,
    /// CZ_NPC_EXPANDED_BARTER_MARKET_PURCHASE: (item, amount, shop index).
    ExpandedBuy(Vec<(u32, u32, u32)>),
    ExpandedClose,
}

impl Encode for MarketRequest {
    fn encode(&self, d: &mut BytesMut) {
        match self {
            MarketRequest::MarketBuy(list) => {
                d.put_u16_le(0x09d6);
                d.put_u16_le((4 + list.len() * 8) as u16);
                for (item, n) in list {
                    d.put_u32_le(*item);
                    d.put_u32_le(*n);
                }
            }
            MarketRequest::MarketClose => d.put_u16_le(0x09d8),
            MarketRequest::BarterBuy(list) => {
                d.put_u16_le(0x0b0f);
                d.put_u16_le((4 + list.len() * 14) as u16);
                for (item, n, index) in list {
                    d.put_u32_le(*item);
                    d.put_u32_le(*n);
                    d.put_u16_le(0); // inventory index (unused by the server)
                    d.put_u32_le(*index);
                }
            }
            MarketRequest::BarterClose => d.put_u16_le(0x0b12),
            MarketRequest::ExpandedBuy(list) => {
                d.put_u16_le(0x0b57);
                d.put_u16_le((4 + list.len() * 12) as u16);
                for (item, n, index) in list {
                    d.put_u32_le(*item);
                    d.put_u32_le(*index);
                    d.put_u32_le(*n);
                }
            }
            MarketRequest::ExpandedClose => d.put_u16_le(0x0b58),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn market_open_and_buy() {
        let mut p = vec![0x7a, 0x0b, 23, 0];
        p.extend(501u32.to_le_bytes());
        p.push(0);
        p.extend(20u32.to_le_bytes());
        p.extend(5u32.to_le_bytes());
        p.extend(70u16.to_le_bytes());
        p.extend(0u32.to_le_bytes());
        let m = parse_market(&p);
        assert_eq!((m[0].item, m[0].price, m[0].stock), (501, 20, 5));
        let mut d = BytesMut::new();
        MarketRequest::ExpandedBuy(vec![(503, 1, 0)]).encode(&mut d);
        assert_eq!(d.len(), 16);
    }
}

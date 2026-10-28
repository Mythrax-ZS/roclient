//! Store search (the "Universal Catalog": search vending / buying stores on the map).

use bytes::{Buf, BufMut, BytesMut};

use crate::codec::Encode;
use crate::packets::get_fixed_str;
use crate::table::client_packet;

#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub store: u32,
    pub owner: u32,
    pub shop: String,
    pub item: u32,
    pub price: u32,
    pub amount: u16,
    pub refine: u8,
    pub cards: [u32; 4],
}

/// ZC_SEARCH_STORE_INFO_ACK (0x0b64): (first page, more pages, uses left, results).
pub fn parse_results(data: &[u8]) -> Option<(bool, bool, u8, Vec<Found>)> {
    let mut b = data.get(4..)?;
    if b.len() < 3 {
        return None;
    }
    let (first, next, uses) = (b.get_u8() != 0, b.get_u8() != 0, b.get_u8());
    let mut out = vec![];
    while b.len() >= 142 {
        let (store, owner) = (b.get_u32_le(), b.get_u32_le());
        let shop = get_fixed_str(&mut b, 80);
        let item = b.get_u32_le();
        b.advance(1);
        let (price, amount) = (b.get_u32_le(), b.get_u16_le());
        let cards = [b.get_u32_le(), b.get_u32_le(), b.get_u32_le(), b.get_u32_le()];
        b.advance(25);
        let refine = b.get_u8();
        b.advance(1);
        out.push(Found { store, owner, shop, item, price, amount, refine, cards });
    }
    Some((first, next, uses, out))
}

#[derive(Debug, Clone, PartialEq)]
pub enum SearchRequest {
    /// kind 0: selling (vending) shops, 1: buying stores.
    Search { kind: u8, min: u32, max: u32, items: Vec<u32>, cards: Vec<u32> },
    NextPage,
    Close,
    /// Show this shop on the map.
    Click { owner: u32, store: u32, item: u32 },
}

impl Encode for SearchRequest {
    fn encode(&self, d: &mut BytesMut) {
        match self {
            SearchRequest::Search { kind, min, max, items, cards } => {
                d.put_u16_le(client_packet("SearchStoreInfo").opcode);
                d.put_u16_le((15 + 4 * (items.len() + cards.len())) as u16);
                d.put_u8(*kind);
                d.put_u32_le(*max);
                d.put_u32_le(*min);
                d.put_u8(items.len() as u8);
                d.put_u8(cards.len() as u8);
                for i in items.iter().chain(cards) {
                    d.put_u32_le(*i);
                }
            }
            SearchRequest::NextPage => d.put_u16_le(client_packet("SearchStoreInfoNextPage").opcode),
            SearchRequest::Close => d.put_u16_le(client_packet("CloseSearchStoreInfo").opcode),
            SearchRequest::Click { owner, store, item } => {
                d.put_u16_le(client_packet("SearchStoreInfoListItemClick").opcode);
                d.put_u32_le(*owner);
                d.put_u32_le(*store);
                d.put_u32_le(*item);
            }
        }
    }
}

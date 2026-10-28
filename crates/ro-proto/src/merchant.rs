//! The pushcart and player shops (vending), as rAthena sends them at PACKETVER 20250402.
//! The cart's contents arrive like the inventory's (0x0b08/0x0b09/0x0b39, type 1).

use bytes::{Buf, BufMut, BytesMut};

use crate::codec::Encode;
use crate::items::ItemData;
use crate::packets::get_fixed_str;

fn cards(b: &mut &[u8]) -> [u32; 4] {
    [b.get_u32_le(), b.get_u32_le(), b.get_u32_le(), b.get_u32_le()]
}

/// ZC_NOTIFY_CARTITEM_COUNTINFO (0x0121): (items, max items, weight, max weight).
pub fn parse_cart_count(data: &[u8]) -> Option<(u16, u16, u32, u32)> {
    let mut b = data.get(2..14)?;
    Some((b.get_u16_le(), b.get_u16_le(), b.get_u32_le(), b.get_u32_le()))
}

/// ZC_ADD_ITEM_TO_CART (0x0b45): an item went into the cart.
pub fn parse_cart_add(data: &[u8]) -> Option<ItemData> {
    let mut b = data.get(2..58)?;
    let index = b.get_u16_le();
    let amount = b.get_u32_le() as u16;
    let id = b.get_u32_le();
    let kind = b.get_u8();
    let identified = b.get_u8() != 0;
    b.advance(1); // damaged
    let cards = cards(&mut b);
    b.advance(25); // options
    let refine = b.get_u8();
    Some(ItemData { index, id, kind, amount, location: 0, equipped: 0, refine, identified, cards, look: 0 })
}

/// ZC_DELETE_ITEM_FROM_CART (0x0125): (index, amount).
pub fn parse_cart_remove(data: &[u8]) -> Option<(u16, u32)> {
    let mut b = data.get(2..8)?;
    Some((b.get_u16_le(), b.get_u32_le()))
}

/// ZC_STORE_ENTRY (0x0131): a shop sign over a player, (account id, title).
pub fn parse_store_entry(data: &[u8]) -> Option<(u32, String)> {
    let mut b = data.get(2..)?;
    let aid = b.get_u32_le();
    let n = b.len().min(80);
    Some((aid, get_fixed_str(&mut b, n)))
}

/// An item for sale in a player's shop.
#[derive(Debug, Clone, PartialEq)]
pub struct VendItem {
    pub price: u32,
    pub amount: u16,
    /// The seller's cart index (what a purchase names).
    pub index: u16,
    pub id: u32,
    pub kind: u8,
    pub identified: bool,
    pub refine: u8,
    pub cards: [u32; 4],
}

/// ZC_PC_PURCHASE_ITEMLIST_FROMMC (0x0b3d): another player's shop, (seller, shop id, items).
pub fn parse_shop(data: &[u8]) -> Option<(u32, u32, Vec<VendItem>)> {
    let mut b = data.get(4..)?;
    let (aid, uid) = (b.get_u32_le(), b.get_u32_le());
    let mut items = vec![];
    while b.len() >= 64 {
        let price = b.get_u32_le();
        let amount = b.get_u16_le();
        let index = b.get_u16_le();
        let kind = b.get_u8();
        let id = b.get_u32_le();
        let identified = b.get_u8() != 0;
        b.advance(1);
        let cards = cards(&mut b);
        b.advance(25 + 4 + 2); // options, location, view
        let refine = b.get_u8();
        b.advance(1);
        items.push(VendItem { price, amount, index, id, kind, identified, refine, cards });
    }
    Some((aid, uid, items))
}

/// ZC_PC_PURCHASE_MYITEMLIST (0x0b40): our own shop as opened.
pub fn parse_my_shop(data: &[u8]) -> Vec<VendItem> {
    let Some(mut b) = data.get(8..) else { return vec![] };
    let mut items = vec![];
    while b.len() >= 58 {
        let price = b.get_u32_le();
        let index = b.get_u16_le();
        let amount = b.get_u16_le();
        let kind = b.get_u8();
        let id = b.get_u32_le();
        let identified = b.get_u8() != 0;
        b.advance(1);
        let cards = cards(&mut b);
        b.advance(25);
        let refine = b.get_u8();
        b.advance(1);
        items.push(VendItem { price, amount, index, id, kind, identified, refine, cards });
    }
    items
}

/// ZC_DELETEITEM_FROM_MCSTORE (0x09e5): something sold from our shop, (index, amount, zeny).
pub fn parse_sold(data: &[u8]) -> Option<(u16, u16, i32)> {
    let mut b = data.get(2..18)?;
    let (index, amount) = (b.get_u16_le(), b.get_u16_le());
    b.advance(8); // buyer, date
    Some((index, amount, b.get_i32_le()))
}

#[derive(Debug, Clone, PartialEq)]
pub enum MerchantRequest {
    /// CZ_MOVE_ITEM_FROM_BODY_TO_CART.
    CartPut { index: u16, amount: u32 },
    /// CZ_MOVE_ITEM_FROM_CART_TO_BODY.
    CartGet { index: u16, amount: u32 },
    /// CZ_REQ_CARTOFF: drop the cart.
    CartOff,
    /// CZ_REQ_OPENSTORE2: open a shop with (cart index, amount, price) entries.
    OpenShop { title: String, items: Vec<(u16, u16, u32)> },
    /// CZ_REQ_CLOSESTORE.
    CloseShop,
    /// CZ_REQ_BUY_FROMMC: look in someone's shop.
    Browse(u32),
    /// CZ_PC_PURCHASE_ITEMLIST_FROMMC2: buy (amount, index) entries.
    Buy { seller: u32, shop: u32, items: Vec<(u16, u16)> },
}

impl Encode for MerchantRequest {
    fn encode(&self, d: &mut BytesMut) {
        match self {
            MerchantRequest::CartPut { index, amount } | MerchantRequest::CartGet { index, amount } => {
                d.put_u16_le(if matches!(self, MerchantRequest::CartPut { .. }) { 0x0126 } else { 0x0127 });
                d.put_u16_le(*index);
                d.put_u32_le(*amount);
            }
            MerchantRequest::CartOff => d.put_u16_le(0x012a),
            MerchantRequest::OpenShop { title, items } => {
                d.put_u16_le(0x01b2);
                d.put_u16_le((85 + items.len() * 8) as u16);
                let (t, _, _) = encoding_rs::EUC_KR.encode(title);
                let mut name = [0u8; 80];
                let n = t.len().min(79);
                name[..n].copy_from_slice(&t[..n]);
                d.put_slice(&name);
                d.put_u8(1);
                for (index, amount, price) in items {
                    d.put_u16_le(*index);
                    d.put_u16_le(*amount);
                    d.put_u32_le(*price);
                }
            }
            MerchantRequest::CloseShop => d.put_u16_le(0x012e),
            MerchantRequest::Browse(aid) => {
                d.put_u16_le(0x0130);
                d.put_u32_le(*aid);
            }
            MerchantRequest::Buy { seller, shop, items } => {
                d.put_u16_le(0x0801);
                d.put_u16_le((12 + items.len() * 4) as u16);
                d.put_u32_le(*seller);
                d.put_u32_le(*shop);
                for (amount, index) in items {
                    d.put_u16_le(*amount);
                    d.put_u16_le(*index);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_open_shop() {
        let mut d = BytesMut::new();
        MerchantRequest::OpenShop { title: "Cheap".into(), items: vec![(2, 5, 1000)] }.encode(&mut d);
        assert_eq!(d.len(), 93);
        assert_eq!(u16::from_le_bytes([d[2], d[3]]), 93);
        assert_eq!(d[84], 1);
        assert_eq!(&d[85..87], &2u16.to_le_bytes());
    }
}

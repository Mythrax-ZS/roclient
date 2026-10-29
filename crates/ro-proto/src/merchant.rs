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
    Some(ItemData { index, id, kind, amount, location: 0, equipped: 0, refine, grade: 0, identified, cards, look: 0, favorite: false })
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

/// An item a buying store wants.
#[derive(Debug, Clone, PartialEq)]
pub struct WantedItem {
    pub price: u32,
    pub amount: u16,
    pub kind: u8,
    pub id: u32,
}

fn wanted(mut b: &[u8]) -> Vec<WantedItem> {
    let mut items = vec![];
    while b.len() >= 11 {
        items.push(WantedItem { price: b.get_u32_le(), amount: b.get_u16_le(), kind: b.get_u8(), id: b.get_u32_le() });
    }
    items
}

/// ZC_MYITEMLIST_BUYING_STORE (0x0813): our buying store as opened, (zeny limit, items).
pub fn parse_my_buying_store(data: &[u8]) -> Option<(u32, Vec<WantedItem>)> {
    let mut b = data.get(8..)?;
    let limit = b.get_u32_le();
    Some((limit, wanted(b)))
}

/// ZC_ACK_ITEMLIST_BUYING_STORE (0x0818): someone's buying store, (buyer, store id, zeny limit, items).
pub fn parse_buying_store(data: &[u8]) -> Option<(u32, u32, u32, Vec<WantedItem>)> {
    let mut b = data.get(4..)?;
    if b.len() < 12 {
        return None;
    }
    let (aid, store, limit) = (b.get_u32_le(), b.get_u32_le(), b.get_u32_le());
    Some((aid, store, limit, wanted(b)))
}

/// ZC_UPDATE_ITEM_FROM_BUYING_STORE2 (0x09e6): we bought something, (item id, amount bought, zeny paid, zeny limit left).
pub fn parse_buying_update(data: &[u8]) -> Option<(u32, u16, u32, u32)> {
    let mut b = data.get(2..16)?;
    Some((b.get_u32_le(), b.get_u16_le(), b.get_u32_le(), b.get_u32_le()))
}

/// ZC_ITEM_DELETE_BUYING_STORE (0x081c): we sold to a buying store, (index, amount, price each).
pub fn parse_sold_to_buyer(data: &[u8]) -> Option<(u16, u16, u32)> {
    let mut b = data.get(2..10)?;
    Some((b.get_u16_le(), b.get_u16_le(), b.get_u32_le()))
}

#[derive(Debug, Clone, PartialEq)]
pub enum BuyingStoreRequest {
    /// CZ_REQ_OPEN_BUYING_STORE: (item id, amount, price) entries.
    Open { limit: u32, title: String, items: Vec<(u32, u16, u32)> },
    /// CZ_REQ_CLOSE_BUYING_STORE.
    Close,
    /// CZ_REQ_CLICK_TO_BUYING_STORE.
    Browse(u32),
    /// CZ_REQ_TRADE_BUYING_STORE: sell (inventory index, item id, amount) entries.
    Sell { buyer: u32, store: u32, items: Vec<(u16, u32, u16)> },
}

impl Encode for BuyingStoreRequest {
    fn encode(&self, d: &mut BytesMut) {
        match self {
            BuyingStoreRequest::Open { limit, title, items } => {
                d.put_u16_le(0x0811);
                d.put_u16_le((89 + items.len() * 10) as u16);
                d.put_u32_le(*limit);
                d.put_u8(1);
                let (t, _, _) = encoding_rs::EUC_KR.encode(title);
                let mut name = [0u8; 80];
                let n = t.len().min(79);
                name[..n].copy_from_slice(&t[..n]);
                d.put_slice(&name);
                for (id, amount, price) in items {
                    d.put_u32_le(*id);
                    d.put_u16_le(*amount);
                    d.put_u32_le(*price);
                }
            }
            BuyingStoreRequest::Close => d.put_u16_le(0x0815),
            BuyingStoreRequest::Browse(aid) => {
                d.put_u16_le(0x0817);
                d.put_u32_le(*aid);
            }
            BuyingStoreRequest::Sell { buyer, store, items } => {
                d.put_u16_le(0x0819);
                d.put_u16_le((12 + items.len() * 8) as u16);
                d.put_u32_le(*buyer);
                d.put_u32_le(*store);
                for (index, id, amount) in items {
                    d.put_u16_le(*index);
                    d.put_u32_le(*id);
                    d.put_u16_le(*amount);
                }
            }
        }
    }
}

/// The cash shop (0x0b6d open, 0x08c9 list, 0x0848 buy).
#[derive(Debug, Clone, PartialEq)]
pub enum CashShopRequest {
    Open,
    List,
    Close,
    /// (item id, amount, tab) entries, paying `kafra` of the price in Kafra points.
    Buy { kafra: u32, items: Vec<(u32, u32, u16)> },
}

impl Encode for CashShopRequest {
    fn encode(&self, d: &mut BytesMut) {
        match self {
            CashShopRequest::Open => {
                d.put_u16_le(0x0b6d);
                d.put_u32_le(0);
            }
            CashShopRequest::List => d.put_u16_le(0x08c9),
            CashShopRequest::Close => d.put_u16_le(0x084a),
            CashShopRequest::Buy { kafra, items } => {
                d.put_u16_le(0x0848);
                d.put_u16_le((10 + items.len() * 10) as u16);
                d.put_u16_le(items.len() as u16);
                d.put_u32_le(*kafra);
                for (id, amount, tab) in items {
                    d.put_u32_le(*id);
                    d.put_u32_le(*amount);
                    d.put_u16_le(*tab);
                }
            }
        }
    }
}

/// ZC_SE_CASHSHOP_OPEN (0x0b6e): (cash points, kafra points).
pub fn parse_cash_open(data: &[u8]) -> Option<(u32, u32)> {
    let mut b = data.get(2..10)?;
    Some((b.get_u32_le(), b.get_u32_le()))
}

/// ZC_ACK_SCHEDULER_CASHITEM (0x08ca): a tab's (item id, price) list.
pub fn parse_cash_tab(data: &[u8]) -> Option<(u16, Vec<(u32, u32)>)> {
    let mut b = data.get(4..)?;
    if b.len() < 4 {
        return None;
    }
    let (count, tab) = (b.get_u16_le(), b.get_u16_le());
    let mut items = vec![];
    while b.len() >= 8 && items.len() < count as usize {
        items.push((b.get_u32_le(), b.get_u32_le()));
    }
    Some((tab, items))
}

/// ZC_SE_PC_BUY_CASHITEM_RESULT (0x0849): (result, cash points, kafra points).
pub fn parse_cash_result(data: &[u8]) -> Option<(u16, u32, u32)> {
    let mut b = data.get(6..16)?;
    Some((b.get_u16_le(), b.get_u32_le(), b.get_u32_le()))
}

/// The GM's bargain sale tool (/limitedsale): puts Sale-tab cash items on sale for a while,
/// with a stock. Every request carries our account id.
#[derive(Debug, Clone, PartialEq)]
pub enum SaleRequest {
    Open,
    Close,
    /// Look an item up by name.
    Search(String),
    /// `start` is a Unix time.
    Add { item: u32, amount: u32, start: u32, hours: u16 },
    Remove(u32),
}

pub struct SalePacket {
    pub aid: u32,
    pub req: SaleRequest,
}

impl Encode for SalePacket {
    fn encode(&self, d: &mut BytesMut) {
        match &self.req {
            SaleRequest::Open | SaleRequest::Close => {
                d.put_u16_le(if self.req == SaleRequest::Open { 0x09b4 } else { 0x09bc });
                d.put_u32_le(self.aid);
            }
            SaleRequest::Search(name) => {
                let name = &name.as_bytes()[..name.len().min(49)];
                d.put_u16_le(0x09ac);
                d.put_u16_le(9 + name.len() as u16);
                d.put_u32_le(self.aid);
                d.put_slice(name);
                d.put_u8(0);
            }
            SaleRequest::Add { item, amount, start, hours } => {
                d.put_u16_le(0x09ae);
                d.put_u32_le(self.aid);
                d.put_u32_le(*item);
                d.put_u32_le(*amount);
                d.put_u32_le(*start);
                d.put_u16_le(*hours);
            }
            SaleRequest::Remove(item) => {
                d.put_u16_le(0x09b0);
                d.put_u32_le(self.aid);
                d.put_u32_le(*item);
            }
        }
    }
}

/// What the server says about bargain sales.
#[derive(Debug, Clone, PartialEq)]
pub enum SaleNote {
    /// ZC_NOTIFY_BARGAIN_SALE_SELLING: an item went on sale for `secs` more seconds.
    Selling { item: u32, secs: u32 },
    /// ZC_NOTIFY_BARGAIN_SALE_CLOSE.
    Closed(u32),
    /// ZC_ACK_COUNT_BARGAIN_SALE_ITEM: stock left.
    Stock { item: u32, amount: u32 },
    /// ZC_OPEN_BARGAIN_SALE_TOOL / ZC_CLOSE_BARGAIN_SALE_TOOL.
    Tool(bool),
    /// ZC_ACK_CASH_BARGAIN_SALE_ITEM_INFO: the searched item (None: not in the Sale tab).
    Found(Option<(u32, u32)>),
    /// ZC_ACK_APPLY_BARGAIN_SALE_ITEM: 0 ok, 1 failed (not a Sale-tab item, bad time), 2 already on sale.
    Added(u16),
    /// ZC_ACK_REMOVE_BARGAIN_SALE_ITEM.
    Removed(bool),
}

pub fn parse_sale(opcode: u16, data: &[u8]) -> Option<SaleNote> {
    let l = |i: usize| data.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let w = |i: usize| data.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    Some(match opcode {
        0x09b2 => SaleNote::Selling { item: l(2)?, secs: l(6)? },
        0x09b3 => SaleNote::Closed(l(2)?),
        0x09c4 => SaleNote::Stock { item: l(2)?, amount: l(6)? },
        0x09b5 => SaleNote::Tool(true),
        0x09bd => SaleNote::Tool(false),
        0x09ad => SaleNote::Found((w(2)? == 0).then_some((l(4)?, l(8)?))),
        0x09af => SaleNote::Added(w(2)?),
        0x09b1 => SaleNote::Removed(w(2)? == 0),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_buying_store() {
        let mut d = BytesMut::new();
        BuyingStoreRequest::Open { limit: 5000, title: "Buying".into(), items: vec![(909, 10, 50)] }.encode(&mut d);
        assert_eq!(d.len(), 99);
        assert_eq!(d[8], 1);
        let mut d = BytesMut::new();
        BuyingStoreRequest::Sell { buyer: 1, store: 2, items: vec![(3, 909, 4)] }.encode(&mut d);
        assert_eq!(d.len(), 20);
        let mut d = BytesMut::new();
        CashShopRequest::Buy { kafra: 0, items: vec![(501, 2, 0)] }.encode(&mut d);
        assert_eq!(d.len(), 20);
    }

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

/// The zeny bank (feature.banking).
#[derive(Debug, Clone, PartialEq)]
pub enum BankRequest {
    Open(u32),
    Close(u32),
    Check(u32),
    Deposit { aid: u32, zeny: i32 },
    Withdraw { aid: u32, zeny: i32 },
}

impl Encode for BankRequest {
    fn encode(&self, d: &mut BytesMut) {
        let (op, aid, zeny) = match *self {
            BankRequest::Open(a) => (0x09b6, a, None),
            BankRequest::Close(a) => (0x09b8, a, None),
            BankRequest::Check(a) => (0x09ab, a, None),
            BankRequest::Deposit { aid, zeny } => (0x09a7, aid, Some(zeny)),
            BankRequest::Withdraw { aid, zeny } => (0x09a9, aid, Some(zeny)),
        };
        d.put_u16_le(op);
        d.put_u32_le(aid);
        if let Some(z) = zeny {
            d.put_i32_le(z);
        }
    }
}

/// ZC_BANKING_CHECK (0x09a6): the balance.
pub fn parse_bank_balance(data: &[u8]) -> Option<i64> {
    data.get(2..10).map(|mut b| b.get_i64_le())
}

/// ZC_ACK_BANKING_DEPOSIT / WITHDRAW (0x09a8 / 0x09aa): (reason, balance, zeny on hand).
pub fn parse_bank_ack(data: &[u8]) -> Option<(i16, i64, i32)> {
    let mut b = data.get(2..16)?;
    Some((b.get_i16_le(), b.get_i64_le(), b.get_i32_le()))
}

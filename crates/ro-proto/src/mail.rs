//! RODEX mail, as rAthena sends it at PACKETVER 20250402.

use bytes::{Buf, BufMut, BytesMut};

use crate::codec::Encode;
use crate::packets::get_fixed_str;

/// A mail in the inbox list.
#[derive(Debug, Clone, PartialEq)]
pub struct MailHeader {
    /// 0 character inbox, 1 account inbox, 2 returned.
    pub tab: u8,
    pub id: u64,
    pub read: bool,
    /// Bits: 1 text, 2 zeny, 4 items, 8 from an NPC/system.
    pub kind: u8,
    pub sender: String,
    /// Seconds until it is deleted or returned.
    pub expires: u32,
    pub title: String,
}

fn str_n(b: &mut &[u8], n: usize) -> String {
    let n = n.min(b.len());
    let (s, rest) = b.split_at(n);
    *b = rest;
    let s = s.split(|c| *c == 0).next().unwrap_or_default();
    encoding_rs::EUC_KR.decode(s).0.into_owned()
}

/// ZC_ACK_MAIL_LIST3 (0x0ac2).
pub fn parse_inbox(data: &[u8]) -> Vec<MailHeader> {
    let Some(mut b) = data.get(5..) else { return vec![] };
    let mut out = vec![];
    while b.len() >= 41 {
        let tab = b.get_u8();
        let id = b.get_u64_le();
        let read = b.get_u8() != 0;
        let kind = b.get_u8();
        let sender = get_fixed_str(&mut b, 24);
        let expires = b.get_u32_le();
        let len = b.get_u16_le() as usize;
        let title = str_n(&mut b, len);
        out.push(MailHeader { tab, id, read, kind, sender, expires, title });
    }
    out
}

/// An item attached to a mail.
#[derive(Debug, Clone, PartialEq)]
pub struct MailItem {
    pub id: u32,
    pub amount: u16,
    pub refine: u8,
    pub cards: [u32; 4],
}

/// ZC_ACK_READ_RODEX (0x0b63): (tab, mail id, text, zeny, items).
pub fn parse_mail(data: &[u8]) -> Option<(u8, u64, String, i64, Vec<MailItem>)> {
    let mut b = data.get(4..)?;
    if b.len() < 20 {
        return None;
    }
    let tab = b.get_u8();
    let id = b.get_u64_le();
    let text_len = b.get_u16_le() as usize;
    let zeny = b.get_i64_le();
    let count = b.get_u8() as usize;
    let text = str_n(&mut b, text_len);
    let mut items = vec![];
    for _ in 0..count {
        if b.len() < 60 {
            break;
        }
        let amount = b.get_u16_le();
        let item = b.get_u32_le();
        b.advance(2); // identified, damaged
        let cards = [b.get_u32_le(), b.get_u32_le(), b.get_u32_le(), b.get_u32_le()];
        b.advance(4 + 1 + 2 + 2 + 25); // location, type, view, bind, options
        let refine = b.get_u8();
        b.advance(1); // grade
        items.push(MailItem { id: item, amount, refine, cards });
    }
    Some((tab, id, text, zeny, items))
}

/// ZC_ACK_ZENY_FROM_MAIL / ITEM (0x09f2 / 0x09f4): (mail id, tab, result; 0 = ok).
pub fn parse_take_ack(data: &[u8]) -> Option<(u64, u8, u8)> {
    let mut b = data.get(2..12)?;
    Some((b.get_u64_le(), b.get_u8(), b.get_u8()))
}

/// ZC_ACK_DELETE_MAIL (0x09f6): (tab, mail id).
pub fn parse_delete_ack(data: &[u8]) -> Option<(u8, u64)> {
    let mut b = data.get(2..11)?;
    Some((b.get_u8(), b.get_u64_le()))
}

/// ZC_CHECKNAME (0x0a51): (char id; 0 = no such character, name).
pub fn parse_checkname(data: &[u8]) -> Option<(u32, String)> {
    let mut b = data.get(2..34)?;
    let id = b.get_u32_le();
    b.advance(4); // class, level
    Some((id, get_fixed_str(&mut b, 24)))
}

fn name24(d: &mut BytesMut, name: &str) {
    let (t, _, _) = encoding_rs::EUC_KR.encode(name);
    let mut buf = [0u8; 24];
    let n = t.len().min(23);
    buf[..n].copy_from_slice(&t[..n]);
    d.put_slice(&buf);
}

#[derive(Debug, Clone, PartialEq)]
pub enum MailRequest {
    /// CZ_OPEN_MAILBOX2 (refresh: CZ_REQ_REFRESH_MAIL_LIST2).
    Open { refresh: bool },
    Read { tab: u8, id: u64 },
    TakeZeny { tab: u8, id: u64 },
    TakeItems { tab: u8, id: u64 },
    Delete { tab: u8, id: u64 },
    /// CZ_REQ_OPEN_WRITE_MAIL: start writing (to an optional name).
    BeginWrite(String),
    CancelWrite,
    /// CZ_CHECKNAME2: look up a receiver.
    CheckName(String),
    /// CZ_REQ_WRITE_MAIL2.
    Send { to: String, to_id: u32, from: String, title: String, text: String, zeny: u64 },
}

impl Encode for MailRequest {
    fn encode(&self, d: &mut BytesMut) {
        match self {
            MailRequest::Open { refresh } => {
                d.put_u16_le(if *refresh { 0x0ac1 } else { 0x0ac0 });
                d.put_u64_le(0);
                d.put_slice(&[0; 16]);
            }
            MailRequest::Read { tab, id } => {
                d.put_u16_le(0x09ea);
                d.put_u8(*tab);
                d.put_u64_le(*id);
            }
            MailRequest::TakeZeny { tab, id } | MailRequest::TakeItems { tab, id } => {
                d.put_u16_le(if matches!(self, MailRequest::TakeZeny { .. }) { 0x09f1 } else { 0x09f3 });
                d.put_u64_le(*id);
                d.put_u8(*tab);
            }
            MailRequest::Delete { tab, id } => {
                d.put_u16_le(0x09f5);
                d.put_u8(*tab);
                d.put_u64_le(*id);
            }
            MailRequest::BeginWrite(name) => {
                d.put_u16_le(0x0a08);
                name24(d, name);
            }
            MailRequest::CancelWrite => d.put_u16_le(0x0a03),
            MailRequest::CheckName(name) => {
                d.put_u16_le(0x0b97);
                name24(d, name);
                d.put_u8(0);
            }
            MailRequest::Send { to, to_id, from, title, text, zeny } => {
                let enc = |s: &str| {
                    let mut v = encoding_rs::EUC_KR.encode(s).0.into_owned();
                    v.push(0);
                    v
                };
                let (t, x) = (enc(title), enc(text));
                d.put_u16_le(0x0a6e);
                d.put_u16_le((68 + t.len() + x.len()) as u16);
                name24(d, to);
                name24(d, from);
                d.put_u64_le(*zeny);
                d.put_u16_le(t.len() as u16);
                d.put_u16_le(x.len() as u16);
                d.put_u32_le(*to_id);
                d.put_slice(&t);
                d.put_slice(&x);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inbox_and_send() {
        let mut d = vec![0xc2, 0x0a, 0, 0, 1, 0];
        d.extend(7u64.to_le_bytes());
        d.extend([0, 9]);
        d.extend(b"Server\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0");
        d.extend(100u32.to_le_bytes());
        d.extend(4u16.to_le_bytes());
        d.extend(b"Hi!\0");
        let m = parse_inbox(&d);
        assert_eq!(m.len(), 1);
        assert_eq!((m[0].id, m[0].sender.as_str(), m[0].title.as_str(), m[0].kind), (7, "Server", "Hi!", 9));
        let mut b = BytesMut::new();
        MailRequest::Send { to: "Buddy".into(), to_id: 1, from: "Tester".into(), title: "a".into(), text: "bc".into(), zeny: 5 }.encode(&mut b);
        assert_eq!(b.len(), 68 + 2 + 3);
        assert_eq!(u16::from_le_bytes([b[2], b[3]]) as usize, b.len());
    }
}

//! Battlegrounds (the entry queue, the match: score, team positions and HP, battle chat)
//! and the guild castle list (War of Emperium: castles owned, investment, teleport).

use bytes::{Buf, BufMut, BytesMut};

use crate::codec::Encode;
use crate::packets::{get_fixed_str, put_fixed_str};

#[derive(Debug, Clone, PartialEq)]
pub enum BgEvent {
    /// ZC_ACK_ENTRY_QUEUE_APPLY: 1 accepted; else why not (see `apply_error`).
    Applied { result: u8, name: String },
    /// ZC_NOTIFY_ENTRY_QUEUE_APPLY: players queued so far.
    Queued { name: String, count: u32 },
    /// ZC_ACK_ENTRY_QUEUE_CANCEL.
    Cancelled { ok: bool, name: String },
    /// ZC_NOTIFY_LOBBY_ADMISSION: the match is ready; accept or decline.
    Ready { name: String, lobby: String },
    /// ZC_ACK_LOBBY_ADMISSION.
    Admission { ok: bool, name: String },
    /// ZC_ENTRY_QUEUE_INIT: we were moved into the match; the queue is over.
    QueueDone,
    /// ZC_BATTLEFIELD_NOTIFY_POINT: (Lion / Guillaume, Eagle / Croix).
    Score(u16, u16),
    /// ZC_BATTLEFIELD_NOTIFY_POSITION: a teammate on the minimap (None: left the map).
    Position { id: u32, name: String, class: u16, at: Option<(u16, u16)> },
    /// ZC_BATTLEFIELD_NOTIFY_HP2.
    Hp { id: u32, hp: u32, max: u32 },
    /// ZC_BATTLEFIELD_NOTIFY_CAMPINFO: which team a player is on.
    Camp { id: u32, team: u16 },
    /// ZC_BATTLEFIELD_CHAT: "Name : text".
    Chat(String),
}

pub fn apply_error(result: u8) -> &'static str {
    match result {
        2 => "Queuing has finished.",
        3 => "There is no battleground with that name.",
        4 => "Invalid application.",
        5 => "Too many players in your party or guild.",
        6 => "Your level doesn't meet the requirement.",
        7 => "You have already applied.",
        8 => "Reconnect, then apply.",
        9 => "Only the party or guild leader can apply.",
        10 => "Your job can't apply to this battleground.",
        _ => "The application failed.",
    }
}

fn s24(b: &mut &[u8]) -> String {
    get_fixed_str(b, 24)
}

pub fn parse(opcode: u16, data: &[u8]) -> Option<BgEvent> {
    let mut b = data.get(2..)?;
    Some(match opcode {
        0x08d8 if b.len() >= 25 => BgEvent::Applied { result: b.get_u8(), name: s24(&mut b) },
        0x08d9 if b.len() >= 28 => BgEvent::Queued { name: s24(&mut b), count: b.get_u32_le() },
        0x08db if b.len() >= 25 => BgEvent::Cancelled { ok: b.get_u8() != 0, name: s24(&mut b) },
        0x08df if b.len() >= 48 => BgEvent::Ready { name: s24(&mut b), lobby: s24(&mut b) },
        0x08e1 if b.len() >= 25 => BgEvent::Admission { ok: b.get_u8() != 0, name: s24(&mut b) },
        0x090e => BgEvent::QueueDone,
        0x02de if b.len() >= 4 => BgEvent::Score(b.get_u16_le(), b.get_u16_le()),
        0x02df if b.len() >= 34 => {
            let id = b.get_u32_le();
            let name = s24(&mut b);
            let (class, x, y) = (b.get_u16_le(), b.get_u16_le(), b.get_u16_le());
            BgEvent::Position { id, name, class, at: (x != 0xffff).then_some((x, y)) }
        }
        0x0a0e if b.len() >= 12 => BgEvent::Hp { id: b.get_u32_le(), hp: b.get_u32_le(), max: b.get_u32_le() },
        0x02dd if b.len() >= 30 => {
            let id = b.get_u32_le();
            b.advance(24);
            BgEvent::Camp { id, team: b.get_u16_le() }
        }
        0x02dc if b.len() >= 30 => {
            b.advance(2 + 4);
            b.advance(24); // the name is in the text too
            let end = b.iter().position(|c| *c == 0).unwrap_or(b.len());
            BgEvent::Chat(encoding_rs::EUC_KR.decode(&b[..end]).0.into_owned())
        }
        _ => return None,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub enum BgRequest {
    /// CZ_REQ_ENTRY_QUEUE_APPLY: kind 1 solo, 2 party, 4 guild.
    Apply { kind: u16, name: String },
    Cancel(String),
    /// CZ_REPLY_LOBBY_ADMISSION.
    Reply { accept: bool, name: String, lobby: String },
    /// CZ_REQ_ENTRY_QUEUE_RANKING: ask for the queue count again.
    Status(String),
    /// CZ_BATTLEFIELD_CHAT ("Name : text").
    Chat(String),
}

impl Encode for BgRequest {
    fn encode(&self, d: &mut BytesMut) {
        match self {
            BgRequest::Apply { kind, name } => {
                d.put_u16_le(0x08d7);
                d.put_u16_le(*kind);
                put_fixed_str(d, name, 24);
            }
            BgRequest::Cancel(name) => {
                d.put_u16_le(0x08da);
                put_fixed_str(d, name, 24);
            }
            BgRequest::Reply { accept, name, lobby } => {
                d.put_u16_le(0x08e0);
                d.put_u8(if *accept { 1 } else { 2 });
                put_fixed_str(d, name, 24);
                put_fixed_str(d, lobby, 24);
            }
            BgRequest::Status(name) => {
                d.put_u16_le(0x090a);
                put_fixed_str(d, name, 24);
            }
            BgRequest::Chat(text) => {
                let mut t = encoding_rs::EUC_KR.encode(text).0.into_owned();
                t.push(0);
                d.put_u16_le(0x02db);
                d.put_u16_le((4 + t.len()) as u16);
                d.put_slice(&t);
            }
        }
    }
}

/// ZC_GUILD_AGIT_INFO (0x0b27): the client ids of the castles our guild holds.
pub fn parse_castles(data: &[u8]) -> Vec<u8> {
    data.get(4..).unwrap_or_default().to_vec()
}

/// ZC_REQ_ACK_AGIT_INVESTMENT (0x0b2d): (castle, economy, defense).
pub fn parse_castle_info(data: &[u8]) -> Option<(u8, u32, u32)> {
    let mut b = data.get(2..11)?;
    Some((b.get_u8(), b.get_u32_le(), b.get_u32_le()))
}

#[derive(Debug, Clone, PartialEq)]
pub enum CastleRequest {
    /// CZ_REQ_AGIT_INVESTMENT.
    Info(u8),
    /// CZ_REQ_MOVE_GUILD_AGIT.
    Teleport(u8),
}

impl Encode for CastleRequest {
    fn encode(&self, d: &mut BytesMut) {
        let (op, id) = match self {
            CastleRequest::Info(id) => (0x0b2c, id),
            CastleRequest::Teleport(id) => (0x0b28, id),
        };
        d.put_u16_le(op);
        d.put_u8(*id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_and_score() {
        let mut d = BytesMut::new();
        BgRequest::Apply { kind: 1, name: "Tierra Gorge".into() }.encode(&mut d);
        assert_eq!(d.len(), 28);
        let mut d = BytesMut::new();
        BgRequest::Reply { accept: true, name: "a".into(), lobby: "b".into() }.encode(&mut d);
        assert_eq!(d.len(), 51);
        assert_eq!(parse(0x02de, &[0xde, 0x02, 3, 0, 1, 0]), Some(BgEvent::Score(3, 1)));
    }
}

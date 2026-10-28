//! Chat rooms (the signs players and NPC waiting rooms put over their heads).

use bytes::{Buf, BufMut, BytesMut};

use crate::codec::Encode;
use crate::packets::get_fixed_str;

#[derive(Debug, Clone, PartialEq)]
pub struct Room {
    pub owner: u32,
    pub id: u32,
    pub limit: u16,
    pub users: u16,
    /// 0 private (password), 1 public, 2 NPC waiting room, 3 not clickable.
    pub kind: u8,
    pub title: String,
}

/// ZC_ROOM_NEWENTRY (0x00d7) / ZC_CHANGE_CHATROOM (0x00df).
pub fn parse_room(data: &[u8]) -> Option<Room> {
    let mut b = data.get(4..)?;
    if b.len() < 13 {
        return None;
    }
    let (owner, id, limit, users, kind) = (b.get_u32_le(), b.get_u32_le(), b.get_u16_le(), b.get_u16_le(), b.get_u8());
    let n = b.len();
    let title = get_fixed_str(&mut b, n);
    Some(Room { owner, id, limit, users, kind, title })
}

/// ZC_ENTER_ROOM (0x00db): (chat id, members as (owner?, name)).
pub fn parse_enter(data: &[u8]) -> Option<(u32, Vec<(bool, String)>)> {
    let mut b = data.get(4..)?;
    if b.len() < 4 {
        return None;
    }
    let id = b.get_u32_le();
    let mut members = vec![];
    while b.len() >= 28 {
        let role = b.get_u32_le();
        members.push((role == 0, get_fixed_str(&mut b, 24)));
    }
    Some((id, members))
}

/// ZC_MEMBER_NEWENTRY (0x00dc) / ZC_MEMBER_EXIT (0x00dd): (users, name, kicked).
pub fn parse_member(opcode: u16, data: &[u8]) -> Option<(u16, String, bool)> {
    let mut b = data.get(2..28)?;
    let users = b.get_u16_le();
    let name = get_fixed_str(&mut b, 24);
    let kicked = opcode == 0x00dd && data.get(28).copied() == Some(1);
    Some((users, name, kicked))
}

/// ZC_ROLE_CHANGE (0x00e1): (owner?, name).
pub fn parse_role(data: &[u8]) -> Option<(bool, String)> {
    let mut b = data.get(2..30)?;
    let role = b.get_u32_le();
    Some((role == 0, get_fixed_str(&mut b, 24)))
}

fn fixed(d: &mut BytesMut, s: &str, n: usize) {
    let (t, _, _) = encoding_rs::EUC_KR.encode(s);
    let mut buf = vec![0u8; n];
    let k = t.len().min(n - 1);
    buf[..k].copy_from_slice(&t[..k]);
    d.put_slice(&buf);
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChatRoomRequest {
    /// CZ_CREATE_CHATROOM (or CZ_CHANGE_CHATROOM with `change`).
    Create { title: String, limit: u16, public: bool, password: String, change: bool },
    /// CZ_REQ_ENTER_ROOM.
    Join { id: u32, password: String },
    /// CZ_EXIT_ROOM.
    Leave,
    /// CZ_REQ_EXPEL_MEMBER.
    Kick(String),
    /// CZ_REQ_ROLE_CHANGE: hand the room to someone.
    GiveOwner(String),
}

impl Encode for ChatRoomRequest {
    fn encode(&self, d: &mut BytesMut) {
        match self {
            ChatRoomRequest::Create { title, limit, public, password, change } => {
                let (t, _, _) = encoding_rs::EUC_KR.encode(title);
                let t = &t[..t.len().min(36)];
                d.put_u16_le(if *change { 0x00de } else { 0x00d5 });
                d.put_u16_le((15 + t.len()) as u16);
                d.put_u16_le(*limit);
                d.put_u8(*public as u8);
                fixed(d, password, 8);
                d.put_slice(t);
            }
            ChatRoomRequest::Join { id, password } => {
                d.put_u16_le(0x00d9);
                d.put_u32_le(*id);
                fixed(d, password, 8);
            }
            ChatRoomRequest::Leave => d.put_u16_le(0x00e3),
            ChatRoomRequest::Kick(name) => {
                d.put_u16_le(0x00e2);
                fixed(d, name, 24);
            }
            ChatRoomRequest::GiveOwner(name) => {
                d.put_u16_le(0x00e0);
                d.put_u32_le(0);
                fixed(d, name, 24);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_entry() {
        let mut d = BytesMut::new();
        ChatRoomRequest::Create { title: "Hi".into(), limit: 5, public: true, password: String::new(), change: false }.encode(&mut d);
        assert_eq!(d.len(), 17);
        assert_eq!(u16::from_le_bytes([d[2], d[3]]), 17);
        let mut p = vec![0xd7, 0, 0, 0];
        p.extend(1u32.to_le_bytes());
        p.extend(2u32.to_le_bytes());
        p.extend(5u16.to_le_bytes());
        p.extend(1u16.to_le_bytes());
        p.push(1);
        p.extend(b"Hello");
        let r = parse_room(&p).unwrap();
        assert_eq!((r.owner, r.id, r.users, r.title.as_str()), (1, 2, 1, "Hello"));
    }
}

/// Whisper blocking: /ex and /in (one player), /exall and /inall, and the block list.
#[derive(Debug, Clone, PartialEq)]
pub enum IgnoreRequest {
    Player { name: String, block: bool },
    All { block: bool },
    List,
}

impl Encode for IgnoreRequest {
    fn encode(&self, d: &mut BytesMut) {
        match self {
            IgnoreRequest::Player { name, block } => {
                d.put_u16_le(0x00cf);
                fixed(d, name, 24);
                d.put_u8(!*block as u8);
            }
            IgnoreRequest::All { block } => {
                d.put_u16_le(0x00d0);
                d.put_u8(!*block as u8);
            }
            IgnoreRequest::List => d.put_u16_le(0x00d3),
        }
    }
}

/// ZC_WHISPER_LIST (0x00d4): the blocked names.
pub fn parse_ignore_list(data: &[u8]) -> Vec<String> {
    let Some(mut b) = data.get(4..) else { return vec![] };
    let mut out = vec![];
    while b.len() >= 24 {
        out.push(get_fixed_str(&mut b, 24));
    }
    out
}

/// A party booking (recruitment) ad.
#[derive(Debug, Clone, PartialEq)]
pub struct BookingAd {
    pub index: u32,
    pub leader: String,
    pub level: u16,
    pub map: u16,
    /// Wanted jobs (0xffff/empty: any).
    pub jobs: Vec<u16>,
}

/// ZC_PARTY_BOOKING_ACK_SEARCH (0x0805): (more results, ads).
pub fn parse_booking_list(data: &[u8]) -> (bool, Vec<BookingAd>) {
    let more = data.get(4).copied().unwrap_or(0) != 0;
    let Some(mut b) = data.get(5..) else { return (more, vec![]) };
    let mut out = vec![];
    while b.len() >= 48 {
        let index = b.get_u32_le();
        let leader = get_fixed_str(&mut b, 24);
        b.advance(4); // time
        let (level, map) = (b.get_u16_le(), b.get_u16_le());
        let jobs = (0..6).map(|_| b.get_u16_le()).filter(|j| *j != 0xffff).collect();
        out.push(BookingAd { index, leader, level, map, jobs });
    }
    (more, out)
}

#[derive(Debug, Clone, PartialEq)]
pub enum BookingRequest {
    /// CZ_PARTY_BOOKING_REQ_REGISTER: minimum level, map id (0 any), up to six wanted jobs.
    Register { level: u16, map: u16, jobs: Vec<u16> },
    /// CZ_PARTY_BOOKING_REQ_SEARCH (job 0xffff = any).
    Search { level: u16, map: u16, job: u16 },
    Delete,
}

impl Encode for BookingRequest {
    fn encode(&self, d: &mut BytesMut) {
        match self {
            BookingRequest::Register { level, map, jobs } => {
                d.put_u16_le(0x0802);
                d.put_u16_le(*level);
                d.put_u16_le(*map);
                // The server reads each job as a byte, two bytes apart.
                for i in 0..6 {
                    d.put_u16_le(jobs.get(i).copied().unwrap_or(0xff));
                }
            }
            BookingRequest::Search { level, map, job } => {
                d.put_u16_le(0x0804);
                d.put_u16_le(*level);
                d.put_u16_le(*map);
                d.put_u16_le(*job);
                d.put_u32_le(0);
                d.put_u16_le(10);
            }
            BookingRequest::Delete => d.put_u16_le(0x0806),
        }
    }
}

//! Packets needed for login -> char select -> map entry at PACKETVER 20250402.

use bytes::{Buf, BufMut, BytesMut};
use encoding_rs::EUC_KR;

use crate::{codec::Encode, table::client_packet};

pub fn put_fixed_str(dst: &mut BytesMut, s: &str, n: usize) {
    let (b, _, _) = EUC_KR.encode(s);
    let k = b.len().min(n - 1);
    dst.put_slice(&b[..k]);
    dst.put_bytes(0, n - k);
}

pub fn get_fixed_str(src: &mut &[u8], n: usize) -> String {
    let raw = &src[..n];
    let end = raw.iter().position(|&c| c == 0).unwrap_or(n);
    let (s, _, _) = EUC_KR.decode(&raw[..end]);
    let s = s.into_owned();
    src.advance(n);
    s
}

fn ipv4(ip: u32) -> std::net::Ipv4Addr {
    // rAthena stores the address in network order inside a little-endian u32.
    std::net::Ipv4Addr::from(ip.to_le_bytes())
}

// ---------------------------------------------------------------- client -> server

pub struct CaLogin<'a> {
    pub version: u32,
    pub username: &'a str,
    pub password: &'a str,
    pub clienttype: u8,
}

impl Encode for CaLogin<'_> {
    fn encode(&self, d: &mut BytesMut) {
        d.put_u16_le(0x0064);
        d.put_u32_le(self.version);
        put_fixed_str(d, self.username, 24);
        put_fixed_str(d, self.password, 24);
        d.put_u8(self.clienttype);
    }
}

pub struct ChEnter {
    pub account_id: u32,
    pub login_id1: u32,
    pub login_id2: u32,
    pub sex: u8,
}

impl Encode for ChEnter {
    fn encode(&self, d: &mut BytesMut) {
        d.put_u16_le(0x0065);
        d.put_u32_le(self.account_id);
        d.put_u32_le(self.login_id1);
        d.put_u32_le(self.login_id2);
        d.put_u16_le(0);
        d.put_u8(self.sex);
    }
}

pub struct ChCharlistReq;

impl Encode for ChCharlistReq {
    fn encode(&self, d: &mut BytesMut) {
        d.put_u16_le(0x09a1);
    }
}

pub struct ChSelectChar {
    pub slot: u8,
}

impl Encode for ChSelectChar {
    fn encode(&self, d: &mut BytesMut) {
        d.put_u16_le(0x0066);
        d.put_u8(self.slot);
    }
}

/// Char-server keepalive.
pub struct ChPing {
    pub account_id: u32,
}

impl Encode for ChPing {
    fn encode(&self, d: &mut BytesMut) {
        d.put_u16_le(0x0187);
        d.put_u32_le(self.account_id);
    }
}

/// A fixed-length map-server packet laid out from the generated packet db: opcode and
/// field offsets come from the handler's current registration, so shuffled
/// opcodes and padding are handled by the table rather than by hand.
fn put_db_packet(d: &mut BytesMut, handler: &str, fields: &[&[u8]]) {
    let p = client_packet(handler);
    assert!(p.len > 0, "{handler} is variable-length");
    assert_eq!(p.pos.len(), fields.len(), "{handler}: field count mismatch");
    let start = d.len();
    d.put_bytes(0, p.len as usize);
    let buf = &mut d[start..];
    buf[..2].copy_from_slice(&p.opcode.to_le_bytes());
    for (&off, f) in p.pos.iter().zip(fields) {
        buf[off as usize..off as usize + f.len()].copy_from_slice(f);
    }
}

/// CZ_ENTER
pub struct CzEnter {
    pub account_id: u32,
    pub char_id: u32,
    pub login_id1: u32,
    pub client_tick: u32,
    pub sex: u8,
}

impl Encode for CzEnter {
    fn encode(&self, d: &mut BytesMut) {
        put_db_packet(
            d,
            "WantToConnection",
            &[
                &self.account_id.to_le_bytes(),
                &self.char_id.to_le_bytes(),
                &self.login_id1.to_le_bytes(),
                &self.client_tick.to_le_bytes(),
                &[self.sex],
            ],
        );
    }
}

/// CZ_NOTIFY_ACTORINIT: map loaded, start sending world state.
pub struct CzLoadEndAck;

impl Encode for CzLoadEndAck {
    fn encode(&self, d: &mut BytesMut) {
        d.put_u16_le(client_packet("LoadEndAck").opcode);
    }
}

/// CZ_REQUEST_TIME: map-server keepalive.
pub struct CzRequestTime {
    pub client_tick: u32,
}

impl Encode for CzRequestTime {
    fn encode(&self, d: &mut BytesMut) {
        put_db_packet(d, "TickSend", &[&self.client_tick.to_le_bytes()]);
    }
}

/// CZ_REQUEST_MOVE
pub struct CzRequestMove {
    pub x: u16,
    pub y: u16,
    pub dir: u8,
}

impl Encode for CzRequestMove {
    fn encode(&self, d: &mut BytesMut) {
        put_db_packet(d, "WalkToXY", &[&pack_pos(self.x, self.y, self.dir)]);
    }
}

/// Packs x:10, y:10, dir:4 into 3 bytes (rAthena WBUFPOS).
pub fn pack_pos(x: u16, y: u16, dir: u8) -> [u8; 3] {
    [(x >> 2) as u8, (((x & 3) << 6) | (y >> 4)) as u8, (((y & 0xf) << 4) as u8) | (dir & 0xf)]
}

// ---------------------------------------------------------------- server -> client

#[derive(Debug, Clone)]
pub struct CharServerInfo {
    pub addr: std::net::SocketAddrV4,
    pub name: String,
    pub users: u16,
}

#[derive(Debug, Clone)]
pub struct AcceptLogin {
    pub login_id1: u32,
    pub account_id: u32,
    pub login_id2: u32,
    pub sex: u8,
    pub servers: Vec<CharServerInfo>,
}

/// 0x0ac4
pub fn parse_accept_login(data: &[u8]) -> AcceptLogin {
    let mut b = &data[4..];
    let login_id1 = b.get_u32_le();
    let account_id = b.get_u32_le();
    let login_id2 = b.get_u32_le();
    let _last_ip = b.get_u32_le();
    b.advance(26);
    let sex = b.get_u8();
    b.advance(17); // web auth token
    let mut servers = Vec::new();
    while b.len() >= 160 {
        let ip = b.get_u32_le();
        let port = b.get_u16_le();
        let name = get_fixed_str(&mut b, 20);
        let users = b.get_u16_le();
        b.advance(2 + 2 + 128);
        servers.push(CharServerInfo { addr: std::net::SocketAddrV4::new(ipv4(ip), port), name, users });
    }
    AcceptLogin { login_id1, account_id, login_id2, sex, servers }
}

/// 0x083e error code.
pub fn parse_refuse_login(data: &[u8]) -> u32 {
    (&data[2..]).get_u32_le()
}

/// CHARACTER_INFO, 175 bytes at this PACKETVER.
pub const CHARACTER_INFO_LEN: usize = 175;

#[derive(Debug, Clone)]
pub struct CharInfo {
    pub char_id: u32,
    pub base_exp: i64,
    pub zeny: i32,
    pub job_level: i32,
    pub hp: i64,
    pub max_hp: i64,
    pub sp: i64,
    pub max_sp: i64,
    pub job: i16,
    pub head: i16,
    pub base_level: i16,
    /// Milliseconds per cell.
    pub speed: i16,
    pub name: String,
    pub slot: u8,
    pub hair_color: u8,
    pub map: String,
    pub sex: u8,
}

pub fn parse_char_info(mut b: &[u8]) -> CharInfo {
    let char_id = b.get_u32_le();
    let base_exp = b.get_i64_le();
    let zeny = b.get_i32_le();
    let _job_exp = b.get_i64_le();
    let job_level = b.get_i32_le();
    b.advance(4 * 5 + 2); // body/health/effect state, virtue, honor, jobpoint
    let hp = b.get_i64_le();
    let max_hp = b.get_i64_le();
    let sp = b.get_i64_le();
    let max_sp = b.get_i64_le();
    let speed = b.get_i16_le();
    let job = b.get_i16_le();
    let head = b.get_i16_le();
    let _body = b.get_i16_le();
    let _weapon = b.get_i16_le();
    let base_level = b.get_i16_le();
    b.advance(2 * 7); // sppoint, accessories, shield, palettes
    let name = get_fixed_str(&mut b, 24);
    b.advance(6); // stats
    let slot = b.get_u8();
    let hair_color = b.get_u8();
    b.advance(2);
    let map = get_fixed_str(&mut b, 16);
    b.advance(4 * 4);
    let sex = b.get_u8();
    CharInfo { char_id, base_exp, zeny, job_level, hp, max_hp, sp, max_sp, job, head, base_level, speed, name, slot, hair_color, map, sex }
}

/// 0x006b: 3 + 20 bytes of header data, then characters.
pub fn parse_hc_accept_enter(data: &[u8]) -> Vec<CharInfo> {
    data[4 + 23..].chunks_exact(CHARACTER_INFO_LEN).map(parse_char_info).collect()
}

/// HC_ACK_CHARINFO_PER_PAGE
pub fn parse_charinfo_page(data: &[u8]) -> Vec<CharInfo> {
    data[4..].chunks_exact(CHARACTER_INFO_LEN).map(parse_char_info).collect()
}

#[derive(Debug, Clone)]
pub struct ZoneServer {
    pub char_id: u32,
    pub map: String,
    pub addr: std::net::SocketAddrV4,
}

/// 0x0ac5
pub fn parse_notify_zonesvr(data: &[u8]) -> ZoneServer {
    let mut b = &data[2..];
    let char_id = b.get_u32_le();
    let map = get_fixed_str(&mut b, 16);
    let ip = b.get_u32_le();
    let port = b.get_u16_le();
    ZoneServer { char_id, map, addr: std::net::SocketAddrV4::new(ipv4(ip), port) }
}

#[derive(Debug, Clone, Copy)]
pub struct AcceptEnter {
    pub start_time: u32,
    pub x: u16,
    pub y: u16,
    pub dir: u8,
}

/// 0x02eb. posDir packs x:10, y:10, dir:4.
pub fn parse_accept_enter(data: &[u8]) -> AcceptEnter {
    let mut b = &data[2..];
    let start_time = b.get_u32_le();
    let p = [b[0] as u16, b[1] as u16, b[2] as u16];
    AcceptEnter {
        start_time,
        x: (p[0] << 2) | (p[1] >> 6),
        y: ((p[1] & 0x3f) << 4) | (p[2] >> 4),
        dir: (p[2] & 0x0f) as u8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        let mut b = BytesMut::new();
        CaLogin { version: 0, username: "a", password: "b", clienttype: 0 }.encode(&mut b);
        assert_eq!(b.len(), 55);
        b.clear();
        ChEnter { account_id: 0, login_id1: 0, login_id2: 0, sex: 0 }.encode(&mut b);
        assert_eq!(b.len(), 17);
        b.clear();
        CzEnter { account_id: 1, char_id: 2, login_id1: 3, client_tick: 4, sex: 1 }.encode(&mut b);
        assert_eq!(b.len(), 23);
        assert_eq!(&b[..6], &[0x36, 0x04, 1, 0, 0, 0]);
        assert_eq!(b[22], 1);
        b.clear();
        CzRequestTime { client_tick: 7 }.encode(&mut b);
        assert_eq!(&b[..], &[0x60, 0x03, 7, 0, 0, 0]);
    }

    #[test]
    fn pos_dir() {
        let p = pack_pos(150, 200, 3);
        let mut data = vec![0xeb, 0x02, 0, 0, 0, 0];
        data.extend_from_slice(&p);
        data.extend_from_slice(&[5, 5, 0, 0]);
        let a = parse_accept_enter(&data);
        assert_eq!((a.x, a.y, a.dir), (150, 200, 3));
    }
}

// ---------------------------------------------------------------- map: movement

/// Unpacks rAthena's 6-byte MoveData (WBUFPOS2): source and destination cells.
pub fn unpack_move(p: &[u8]) -> ((u16, u16), (u16, u16)) {
    let p: [u16; 5] = [p[0] as u16, p[1] as u16, p[2] as u16, p[3] as u16, p[4] as u16];
    let x0 = (p[0] << 2) | (p[1] >> 6);
    let y0 = ((p[1] & 0x3f) << 4) | (p[2] >> 4);
    let x1 = ((p[2] & 0x0f) << 6) | (p[3] >> 2);
    let y1 = ((p[3] & 0x03) << 8) | p[4];
    ((x0, y0), (x1, y1))
}

/// ZC_NOTIFY_PLAYERMOVE: our own walk was accepted. Returns (start tick, from, to).
pub fn parse_player_move(data: &[u8]) -> (u32, (u16, u16), (u16, u16)) {
    let tick = (&data[2..]).get_u32_le();
    let (from, to) = unpack_move(&data[6..12]);
    (tick, from, to)
}

/// ZC_STOPMOVE: (account/unit id, x, y).
pub fn parse_stop_move(data: &[u8]) -> (u32, u16, u16) {
    let mut b = &data[2..];
    (b.get_u32_le(), b.get_u16_le(), b.get_u16_le())
}

/// ZC_NPCACK_MAPMOVE: warp to (map, x, y) on the same map server.
pub fn parse_map_move(data: &[u8]) -> (String, u16, u16) {
    let mut b = &data[2..];
    let map = get_fixed_str(&mut b, 16);
    (map, b.get_u16_le(), b.get_u16_le())
}

#[cfg(test)]
mod move_tests {
    use super::*;

    #[test]
    fn unpacks_wbufpos2() {
        // Mirror of rAthena WBUFPOS2 for (156,191) -> (160,185).
        let (x0, y0, x1, y1) = (156u16, 191u16, 160u16, 185u16);
        let p = [
            (x0 >> 2) as u8,
            ((x0 << 6) | ((y0 >> 4) & 0x3f)) as u8,
            ((y0 << 4) | ((x1 >> 6) & 0x0f)) as u8,
            ((x1 << 2) | ((y1 >> 8) & 0x03)) as u8,
            y1 as u8,
            0x88,
        ];
        assert_eq!(unpack_move(&p), ((156, 191), (160, 185)));
    }
}

// ---------------------------------------------------------------- map: other units

/// Where a unit is: standing at a cell, or walking between two.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnitPos {
    At { x: u16, y: u16, dir: u8 },
    Walking { from: (u16, u16), to: (u16, u16) },
}

/// A player, NPC, monster, pet, ... entering view.
#[derive(Debug, Clone)]
pub struct UnitInfo {
    /// 0 player, 1 NPC, 5 monster, 6 event NPC, 7 pet, 8 homunculus, ...
    pub object_type: u8,
    pub id: u32,
    pub speed: i16,
    /// Class: job id for players, sprite class for everything else.
    pub job: i16,
    pub hair: u16,
    pub hair_color: i16,
    pub sex: u8,
    pub name: String,
    pub pos: UnitPos,
}

/// ZC packets packet_idle_unit (0x09ff), packet_spawn_unit (0x09fe) and
/// packet_unit_walking (0x09fd) at PACKETVER 20250402 (see packets_struct.hpp).
pub fn parse_unit(opcode: u16, data: &[u8]) -> Option<UnitInfo> {
    let (pos_at, name_at, sex_at, walking) = match opcode {
        0x09ff => (63, 84, 62, false),
        0x09fe => (63, 83, 62, false),
        0x09fd => (67, 90, 66, true),
        _ => return None,
    };
    if data.len() < name_at {
        return None;
    }
    let u16_at = |o: usize| u16::from_le_bytes([data[o], data[o + 1]]);
    let i16_at = |o: usize| i16::from_le_bytes([data[o], data[o + 1]]);
    let u32_at = |o: usize| u32::from_le_bytes(data[o..o + 4].try_into().unwrap());
    // headpalette moves 4 bytes later in the walking packet (moveStartTime precedes it).
    let palette_at = if walking { 45 } else { 41 };
    let pos = if walking {
        let (from, to) = unpack_move(&data[pos_at..pos_at + 6]);
        UnitPos::Walking { from, to }
    } else {
        let a = parse_accept_enter(&[&[0u8; 6][..], &data[pos_at..pos_at + 3]].concat());
        UnitPos::At { x: a.x, y: a.y, dir: a.dir }
    };
    let mut name = &data[name_at.min(data.len())..];
    let name = if name.len() >= 24 { get_fixed_str(&mut name, 24) } else { String::new() };
    Some(UnitInfo {
        object_type: data[4],
        // AID: the unit id used by vanish/move packets (GID is a char id, 0 for NPCs).
        id: u32_at(5),
        speed: i16_at(13),
        job: i16_at(23),
        hair: u16_at(25),
        hair_color: i16_at(palette_at),
        sex: data[sex_at],
        name,
        pos,
    })
}

/// ZC_NOTIFY_VANISH: (unit id, reason: 0 out of sight, 1 died, 2 logged out, 3 teleported).
pub fn parse_vanish(data: &[u8]) -> (u32, u8) {
    ((&data[2..]).get_u32_le(), data[6])
}

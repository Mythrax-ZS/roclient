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
pub(crate) fn put_db_packet(d: &mut BytesMut, handler: &str, fields: &[&[u8]]) {
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
    pub look: Look,
    pub name: String,
    pub slot: u8,
    pub hair_color: u8,
    pub map: String,
    pub sex: u8,
    /// Seconds until a pending deletion can be confirmed; 0 when none is pending.
    pub delete_date: u32,
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
    let weapon = b.get_i16_le();
    let base_level = b.get_i16_le();
    let _sppoint = b.get_i16_le();
    let head_bottom = b.get_i16_le() as u16;
    let shield = b.get_i16_le() as u16 as u32;
    let head_top = b.get_i16_le() as u16;
    let head_mid = b.get_i16_le() as u16;
    b.advance(4); // palettes
    let look = Look { weapon: weapon as u16 as u32, shield, head_top, head_mid, head_bottom };
    let name = get_fixed_str(&mut b, 24);
    b.advance(6); // stats
    let slot = b.get_u8();
    let hair_color = b.get_u8();
    b.advance(2);
    let map = get_fixed_str(&mut b, 16);
    let delete_date = b.get_u32_le();
    b.advance(4 * 3);
    let sex = b.get_u8();
    CharInfo { char_id, base_exp, zeny, job_level, hp, max_hp, sp, max_sp, job, head, base_level, speed, look, name, slot, hair_color, map, sex, delete_date }
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
    pub look: Look,
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
        look: Look {
            weapon: u32_at(27),
            shield: u32_at(31),
            head_bottom: u16_at(35),
            head_top: u16_at(if walking { 41 } else { 37 }),
            head_mid: u16_at(if walking { 43 } else { 39 }),
        },
        sex: data[sex_at],
        name,
        pos,
    })
}

/// ZC_NOTIFY_VANISH: (unit id, reason: 0 out of sight, 1 died, 2 logged out, 3 teleported).
pub fn parse_vanish(data: &[u8]) -> (u32, u8) {
    ((&data[2..]).get_u32_le(), data[6])
}

/// ZC_NPCACK_SERVERMOVE: warp to (map, x, y) on another map server.
pub fn parse_server_move(data: &[u8]) -> (ZoneServer, u16, u16) {
    let mut b = &data[2..];
    let map = get_fixed_str(&mut b, 16);
    let (x, y) = (b.get_u16_le(), b.get_u16_le());
    let ip = b.get_u32_le();
    let port = b.get_u16_le();
    (ZoneServer { char_id: 0, map, addr: std::net::SocketAddrV4::new(ipv4(ip), port) }, x, y)
}

// ---------------------------------------------------------------- map: status and chat

/// rAthena status ids (`SP_*` in map.hpp) the HUD shows.
pub mod sp {
    pub const BASEEXP: u16 = 1;
    pub const JOBEXP: u16 = 2;
    pub const HP: u16 = 5;
    pub const MAXHP: u16 = 6;
    pub const SP: u16 = 7;
    pub const MAXSP: u16 = 8;
    pub const BASELEVEL: u16 = 11;
    pub const ZENY: u16 = 20;
    pub const NEXTBASEEXP: u16 = 22;
    pub const NEXTJOBEXP: u16 = 23;
    pub const WEIGHT: u16 = 24;
    pub const MAXWEIGHT: u16 = 25;
    pub const JOBLEVEL: u16 = 55;
}

/// ZC_PAR_CHANGE (0x00b0) / ZC_LONGPAR_CHANGE (0x00b1) / ZC_LONGLONGPAR_CHANGE (0x0acb):
/// (status id, value).
pub fn parse_par_change(opcode: u16, data: &[u8]) -> Option<(u16, i64)> {
    let mut b = &data[2..];
    let var = b.get_u16_le();
    let value = match opcode {
        0x00b0 => b.get_i32_le() as i64,
        0x00b1 => b.get_u32_le() as i64,
        0x0acb => b.get_i64_le(),
        _ => return None,
    };
    Some((var, value))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatKind {
    /// Someone nearby (or us) talking: "Name : text".
    Public,
    Npc,
    Whisper,
    Broadcast,
    System,
    Party,
    Guild,
}

/// Text after the fixed header of a variable packet, without trailing NULs.
fn tail_text(data: &[u8], from: usize) -> String {
    let raw = data.get(from..).unwrap_or_default();
    let end = raw.iter().position(|&c| c == 0).unwrap_or(raw.len());
    EUC_KR.decode(&raw[..end]).0.into_owned()
}

/// Chat-like packets: (kind, speaker unit id if any, text).
pub fn parse_chat(opcode: u16, data: &[u8]) -> Option<(ChatKind, Option<u32>, String)> {
    let id_at = |o: usize| u32::from_le_bytes(data[o..o + 4].try_into().unwrap());
    Some(match opcode {
        0x008d => (ChatKind::Public, Some(id_at(4)), tail_text(data, 8)), // ZC_NOTIFY_CHAT
        0x008e => (ChatKind::Public, None, tail_text(data, 4)),           // ZC_NOTIFY_PLAYERCHAT (ours)
        0x02c1 => (ChatKind::Npc, Some(id_at(4)), tail_text(data, 12)),   // ZC_NPC_CHAT
        0x09de => {
            // ZC_WHISPER: senderGID, sender[24], isAdmin, message
            let mut b = &data[8..];
            let sender = get_fixed_str(&mut b, 24);
            (ChatKind::Whisper, None, format!("(From {sender}) : {}", tail_text(data, 33)))
        }
        0x009a => {
            // ZC_BROADCAST: an optional 4-char colour tag ("blue", "ssss", "micc").
            let text = tail_text(data, 4);
            let text = match text.get(..4) {
                Some("blue" | "ssss" | "micc") => text[4..].to_string(),
                _ => text,
            };
            (ChatKind::Broadcast, None, text)
        }
        0x01c3 => (ChatKind::Broadcast, None, tail_text(data, 16)), // ZC_BROADCAST2
        _ => return None,
    })
}

/// CZ_REQUEST_CHAT (GlobalMessage): the client sends "Name : text".
pub struct CzChat<'a> {
    pub name: &'a str,
    pub text: &'a str,
}

impl Encode for CzChat<'_> {
    fn encode(&self, d: &mut BytesMut) {
        let line = format!("{} : {}", self.name, self.text);
        let (msg, _, _) = EUC_KR.encode(&line);
        d.put_u16_le(client_packet("GlobalMessage").opcode);
        d.put_u16_le((4 + msg.len() + 1) as u16);
        d.put_slice(&msg);
        d.put_u8(0);
    }
}

#[cfg(test)]
mod chat_tests {
    use super::*;

    #[test]
    fn chat_roundtrip_shapes() {
        let mut b = BytesMut::new();
        CzChat { name: "Tester", text: "hi" }.encode(&mut b);
        assert_eq!(u16::from_le_bytes([b[2], b[3]]) as usize, b.len());
        assert_eq!(&b[4..b.len() - 1], b"Tester : hi");

        let mut p = vec![0x8e, 0x00, 0, 0];
        p.extend_from_slice(b"Tester : hi\0");
        let n = p.len() as u16;
        p[2..4].copy_from_slice(&n.to_le_bytes());
        assert_eq!(parse_chat(0x008e, &p), Some((ChatKind::Public, None, "Tester : hi".into())));
    }
}

// ---------------------------------------------------------------- map: NPC dialogs

/// What an NPC script asks the client to show.
#[derive(Debug, Clone, PartialEq)]
pub enum NpcDialog {
    /// Append a line of text (may contain ^RRGGBB colour codes).
    Say { npc: u32, text: String },
    /// Show a Next button.
    Next { npc: u32 },
    /// Show a Close button.
    Close { npc: u32 },
    /// Choose one option; reply with its 1-based index (255 cancels).
    Menu { npc: u32, options: Vec<String> },
    InputNumber { npc: u32 },
    InputText { npc: u32 },
    /// Clear the text so far (`clear` in scripts).
    Clear { npc: u32 },
    /// Show (or with `kind` 255, hide) an illustration from texture\유저인터페이스\illust.
    Cutin { image: String, kind: u8 },
}

pub fn parse_npc_dialog(opcode: u16, data: &[u8]) -> Option<NpcDialog> {
    let npc_at = |o: usize| u32::from_le_bytes(data[o..o + 4].try_into().unwrap());
    Some(match opcode {
        0x00b4 => NpcDialog::Say { npc: npc_at(4), text: tail_text(data, 8) },
        0x00b5 => NpcDialog::Next { npc: npc_at(2) },
        0x00b6 => NpcDialog::Close { npc: npc_at(2) },
        0x00b7 => NpcDialog::Menu { npc: npc_at(4), options: tail_text(data, 8).split(':').map(str::to_string).collect() },
        0x0142 => NpcDialog::InputNumber { npc: npc_at(2) },
        0x01d4 => NpcDialog::InputText { npc: npc_at(2) },
        0x08d6 => NpcDialog::Clear { npc: npc_at(2) },
        0x01b3 => {
            let mut b = &data[2..];
            let image = get_fixed_str(&mut b, 64);
            NpcDialog::Cutin { image, kind: data[66] }
        }
        _ => return None,
    })
}

/// Replies to an NPC dialog.
#[derive(Debug, Clone, PartialEq)]
pub enum NpcReply {
    /// Talk to (click) an NPC.
    Contact,
    Next,
    Close,
    /// 1-based option, or 255 to cancel.
    Menu(u8),
    Number(i32),
    Text(String),
}

pub struct CzNpc<'a> {
    pub npc: u32,
    pub reply: &'a NpcReply,
}

impl Encode for CzNpc<'_> {
    fn encode(&self, d: &mut BytesMut) {
        let id = self.npc.to_le_bytes();
        match self.reply {
            NpcReply::Contact => {
                d.put_u16_le(0x0090); // CZ_CONTACTNPC
                d.put_u32_le(self.npc);
                d.put_u8(1);
            }
            NpcReply::Next => put_db_packet(d, "NpcNextClicked", &[&id]),
            NpcReply::Menu(choice) => put_db_packet(d, "NpcSelectMenu", &[&id, &[*choice]]),
            NpcReply::Close => {
                d.put_u16_le(0x0146); // CZ_CLOSE_DIALOG
                d.put_u32_le(self.npc);
            }
            NpcReply::Number(value) => {
                d.put_u16_le(0x0143); // CZ_INPUT_EDITDLG
                d.put_u32_le(self.npc);
                d.put_i32_le(*value);
            }
            NpcReply::Text(text) => {
                let (bytes, _, _) = EUC_KR.encode(text);
                d.put_u16_le(0x01d5); // CZ_INPUT_EDITDLGSTR
                d.put_u16_le((8 + bytes.len() + 1) as u16);
                d.put_u32_le(self.npc);
                d.put_slice(&bytes);
                d.put_u8(0);
            }
        }
    }
}

#[cfg(test)]
mod npc_tests {
    use super::*;

    #[test]
    fn menu_and_replies() {
        let mut p = vec![0xb7, 0x00, 0, 0, 7, 0, 0, 0];
        p.extend_from_slice(b"Save:Storage:Cancel\0");
        let n = p.len() as u16;
        p[2..4].copy_from_slice(&n.to_le_bytes());
        assert_eq!(
            parse_npc_dialog(0x00b7, &p),
            Some(NpcDialog::Menu { npc: 7, options: vec!["Save".into(), "Storage".into(), "Cancel".into()] })
        );
        let mut b = BytesMut::new();
        CzNpc { npc: 7, reply: &NpcReply::Contact }.encode(&mut b);
        assert_eq!(&b[..], &[0x90, 0x00, 7, 0, 0, 0, 1]);
        b.clear();
        CzNpc { npc: 7, reply: &NpcReply::Menu(2) }.encode(&mut b);
        assert_eq!(b.len(), client_packet("NpcSelectMenu").len as usize);
    }
}

// ---------------------------------------------------------------- map: combat

/// ZC_NOTIFY_ACT (0x08c8): someone acted on someone (attack, sit, stand, pick up...).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Act {
    pub src: u32,
    pub target: u32,
    /// Attacker's attack motion time and the target's flinch time, in ms.
    pub src_speed: i32,
    pub dmg_speed: i32,
    pub damage: i32,
    pub hits: i16,
    /// 0 hit, 1 pick up, 2 sit, 3 stand, 4 endure, 8 multi-hit, 9 multi-hit endure,
    /// 10 critical, 11 lucky dodge.
    pub kind: u8,
    pub left_damage: i32,
}

pub fn parse_act(data: &[u8]) -> Act {
    let mut b = &data[2..];
    Act {
        src: b.get_u32_le(),
        target: b.get_u32_le(),
        src_speed: {
            b.advance(4); // server tick
            b.get_i32_le()
        },
        dmg_speed: b.get_i32_le(),
        damage: b.get_i32_le(),
        hits: {
            b.advance(1); // isSPDamage
            b.get_i16_le()
        },
        kind: b.get_u8(),
        left_damage: b.get_i32_le(),
    }
}

/// ZC_ATTACK_FAILURE_FOR_DISTANCE (0x0139): (target, target cell, our cell, attack range).
pub fn parse_attack_out_of_range(data: &[u8]) -> (u32, (u16, u16), (u16, u16), i16) {
    let mut b = &data[2..];
    let target = b.get_u32_le();
    let t = (b.get_i16_le() as u16, b.get_i16_le() as u16);
    let me = (b.get_i16_le() as u16, b.get_i16_le() as u16);
    (target, t, me, b.get_i16_le())
}

/// ZC_NOTIFY_MONSTER_HP (0x0977): (monster id, hp, max hp).
pub fn parse_monster_hp(data: &[u8]) -> (u32, i32, i32) {
    let mut b = &data[2..];
    (b.get_u32_le(), b.get_i32_le(), b.get_i32_le())
}

/// CZ_REQUEST_ACT (ActionRequest): 0 attack once, 7 keep attacking, 2 sit, 3 stand.
pub struct CzRequestAct {
    pub target: u32,
    pub action: u8,
}

impl Encode for CzRequestAct {
    fn encode(&self, d: &mut BytesMut) {
        put_db_packet(d, "ActionRequest", &[&self.target.to_le_bytes(), &[self.action]]);
    }
}

#[cfg(test)]
mod combat_tests {
    use super::*;

    #[test]
    fn notify_act_layout() {
        let mut p = vec![0xc8, 0x08];
        p.extend_from_slice(&5u32.to_le_bytes()); // src
        p.extend_from_slice(&9u32.to_le_bytes()); // target
        p.extend_from_slice(&0u32.to_le_bytes()); // tick
        p.extend_from_slice(&400i32.to_le_bytes());
        p.extend_from_slice(&300i32.to_le_bytes());
        p.extend_from_slice(&17i32.to_le_bytes()); // damage
        p.push(0);
        p.extend_from_slice(&1i16.to_le_bytes());
        p.push(10); // critical
        p.extend_from_slice(&0i32.to_le_bytes());
        assert_eq!(p.len(), 34);
        let a = parse_act(&p);
        assert_eq!((a.src, a.target, a.src_speed, a.dmg_speed, a.damage, a.hits, a.kind), (5, 9, 400, 300, 17, 1, 10));
        let mut b = BytesMut::new();
        CzRequestAct { target: 9, action: 7 }.encode(&mut b);
        assert_eq!(b.len(), client_packet("ActionRequest").len as usize);
    }
}

/// Worn equipment views: weapon/shield sprite classes and headgear view ids.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Look {
    pub weapon: u32,
    pub shield: u32,
    pub head_top: u16,
    pub head_mid: u16,
    pub head_bottom: u16,
}

/// ZC_SPRITE_CHANGE (0x01d7): (unit, kind, value, value2). Kinds: 1 hair, 2 weapon
/// (value2 = shield), 3 lower headgear, 4 upper, 5 middle, 6 hair colour.
pub fn parse_sprite_change(data: &[u8]) -> (u32, u8, u32, u32) {
    let mut b = &data[2..];
    (b.get_u32_le(), b.get_u8(), b.get_u32_le(), b.get_u32_le())
}

/// CZ_RESTART (0x00b2): 0 return to the save point after dying, 1 back to character select.
pub struct CzRestart {
    pub kind: u8,
}

impl Encode for CzRestart {
    fn encode(&self, d: &mut BytesMut) {
        put_db_packet(d, "Restart", &[&[self.kind]]);
    }
}

/// Character-select screen requests at PACKETVER 20250402.
#[derive(Debug, Clone, PartialEq)]
pub enum CharRequest {
    /// CH_MAKE_CHAR (0x0a39). `sex`: 0 female, 1 male; `job` 0 Novice, 4218 Summoner.
    Make { name: String, slot: u8, hair_color: u16, hair_style: u16, job: u32, sex: u8 },
    /// CH_DELETE_CHAR3_RESERVED (0x0827): start the deletion timer.
    ReserveDelete(u32),
    /// CH_DELETE_CHAR3 (0x0829): delete after the timer, confirming with the birthdate (YYMMDD).
    Delete { char_id: u32, birthdate: String },
    /// CH_DELETE_CHAR3_CANCEL (0x082b).
    CancelDelete(u32),
}

impl Encode for CharRequest {
    fn encode(&self, d: &mut BytesMut) {
        match self {
            CharRequest::Make { name, slot, hair_color, hair_style, job, sex } => {
                d.put_u16_le(0x0a39);
                put_fixed_str(d, name, 24);
                d.put_u8(*slot);
                d.put_u16_le(*hair_color);
                d.put_u16_le(*hair_style);
                d.put_u32_le(*job);
                d.put_u8(*sex);
            }
            CharRequest::ReserveDelete(id) => {
                d.put_u16_le(0x0827);
                d.put_u32_le(*id);
            }
            CharRequest::Delete { char_id, birthdate } => {
                d.put_u16_le(0x0829);
                d.put_u32_le(*char_id);
                let mut b = [b'0'; 6];
                for (dst, src) in b.iter_mut().zip(birthdate.bytes().filter(u8::is_ascii_digit)) {
                    *dst = src;
                }
                d.put_slice(&b);
            }
            CharRequest::CancelDelete(id) => {
                d.put_u16_le(0x082b);
                d.put_u32_le(*id);
            }
        }
    }
}

/// HC_DELETE_CHAR3_RESERVED / HC_DELETE_CHAR3 / HC_DELETE_CHAR3_CANCEL: (char id, result
/// (1 ok), seconds until the deletion can be confirmed, 0 if not sent).
pub fn parse_delete_reply(data: &[u8]) -> (u32, i32, u32) {
    let mut b = &data[2..];
    let id = b.get_u32_le();
    let result = b.get_i32_le();
    (id, result, if b.remaining() >= 4 { b.get_u32_le() } else { 0 })
}

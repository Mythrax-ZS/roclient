//! Game Master requests the original client sends from its slash commands and right-click
//! menu, at PACKETVER 20250402 (layouts from rAthena's clif_packetdb.hpp and clif.cpp). The
//! server runs the matching atcommand and checks the caller's group permissions itself.

use bytes::{BufMut, BytesMut};
use encoding_rs::EUC_KR;

use crate::{codec::Encode, packets::put_fixed_str};

#[derive(Debug, Clone, PartialEq)]
pub enum GmRequest {
    /// CZ_BROADCAST (0x0099): the text as everyone sees it (@kami).
    Broadcast(String),
    /// CZ_LOCALBROADCAST (0x019c): to this map only (@lkami).
    LocalBroadcast(String),
    /// CZ_CHANGE_EFFECTSTATE (0x019d): toggle GM hiding (@hide).
    Hide,
    /// CZ_MOVETO_MAP (0x0140): @mapmove.
    MapMove { map: String, x: u16, y: u16 },
    /// CZ_RECALL (0x01bc): bring a character here by name (@recall).
    Recall(String),
    /// CZ_REMOVE_AID (0x01bb): go to a character by name (@jumpto).
    Shift(String),
    /// CZ_RECALL_SSO (0x0842): bring a character here by account id.
    RecallId(u32),
    /// CZ_REMOVE_AID_SSO (0x0843): go to a character by account id.
    GoTo(u32),
    /// CZ_ITEM_CREATE_EX (0x09ce): make an item (or summon a monster) by name.
    Create(String),
    /// CZ_DISCONNECT_CHARACTER (0x00cc): kick a player; for a monster or NPC, remove it.
    /// Answered by ZC_ACK_DISCONNECT_CHARACTER.
    Kick(u32),
    /// CZ_DISCONNECT_ALL_CHARACTER (0x00ce): @kickall.
    KickAll,
    /// CZ_REQ_CHAT_RESTRICTION (0x0212): mute a character for 60 minutes.
    Mute(String),
    /// CZ_GM_FULLSTRIP (0x07f5): take off all of a player's equipment.
    FullStrip(u32),
    /// CZ_RESET (0x0197) type 0: @resetstat on ourselves.
    ResetStats,
    /// CZ_CMD_RESETCOOLTIME (0x0a88): clear our skill cooldowns.
    ResetCooltime,
    /// CZ_REQ_STATUS_GM (0x0213): a character's stats, answered by ZC_ACK_STATUS_GM.
    Check(String),
    /// CZ_CHANGE_MAPTYPE (0x0198): set a map cell's type (0 walkable, 1 wall, 5 cliff ...).
    ChangeMapType { x: u16, y: u16, kind: u16 },
}

/// A broadcast's text as the server expects it: EUC-KR with its terminating NUL.
fn text(s: &str) -> Vec<u8> {
    let mut v = EUC_KR.encode(s).0.into_owned();
    v.push(0);
    v
}

impl Encode for GmRequest {
    fn encode(&self, d: &mut BytesMut) {
        match self {
            GmRequest::Broadcast(s) | GmRequest::LocalBroadcast(s) => {
                let body = text(s);
                d.put_u16_le(if matches!(self, GmRequest::Broadcast(_)) { 0x0099 } else { 0x019c });
                d.put_u16_le((4 + body.len()) as u16);
                d.put_slice(&body);
            }
            GmRequest::Hide => {
                d.put_u16_le(0x019d);
                d.put_u32_le(0);
            }
            GmRequest::MapMove { map, x, y } => {
                d.put_u16_le(0x0140);
                put_fixed_str(d, map, 16);
                d.put_u16_le(*x);
                d.put_u16_le(*y);
            }
            GmRequest::Recall(name) | GmRequest::Shift(name) | GmRequest::Mute(name) | GmRequest::Check(name) => {
                d.put_u16_le(match self {
                    GmRequest::Recall(_) => 0x01bc,
                    GmRequest::Shift(_) => 0x01bb,
                    GmRequest::Mute(_) => 0x0212,
                    _ => 0x0213,
                });
                // The server reads these as C strings: keep the last byte a NUL.
                put_fixed_str(d, &truncate(name, 23), 24);
            }
            GmRequest::RecallId(aid) | GmRequest::GoTo(aid) | GmRequest::Kick(aid) | GmRequest::FullStrip(aid) => {
                d.put_u16_le(match self {
                    GmRequest::RecallId(_) => 0x0842,
                    GmRequest::GoTo(_) => 0x0843,
                    GmRequest::Kick(_) => 0x00cc,
                    _ => 0x07f5,
                });
                d.put_u32_le(*aid);
            }
            GmRequest::Create(name) => {
                d.put_u16_le(0x09ce);
                put_fixed_str(d, &truncate(name, 99), 100);
            }
            GmRequest::KickAll => d.put_u16_le(0x00ce),
            GmRequest::ResetStats => {
                d.put_u16_le(0x0197);
                d.put_u16_le(0);
            }
            GmRequest::ResetCooltime => d.put_u16_le(0x0a88),
            GmRequest::ChangeMapType { x, y, kind } => {
                d.put_u16_le(0x0198);
                d.put_u16_le(*x);
                d.put_u16_le(*y);
                d.put_u16_le(*kind);
            }
        }
    }
}

/// At most `max` EUC-KR bytes of `s`, without splitting a character.
fn truncate(s: &str, max: usize) -> String {
    let mut out = String::new();
    for c in s.chars() {
        let mut next = out.clone();
        next.push(c);
        if EUC_KR.encode(&next).0.len() > max {
            break;
        }
        out = next;
    }
    out
}

/// ZC_ACK_STATUS_GM (0x0214): a character's stats for /check.
#[derive(Debug, Clone, PartialEq)]
pub struct GmStatus {
    /// str, agi, vit, int, dex, luk (each capped at 255) and the points to raise each.
    pub stats: [(u8, u8); 6],
    pub atk: (u16, u16),
    /// (max, min)
    pub matk: (u16, u16),
    pub def: (u16, u16),
    pub mdef: (u16, u16),
    pub hit: u16,
    pub flee: (u16, u16),
    pub crit: u16,
    pub aspd: u16,
}

pub fn parse_gm_status(data: &[u8]) -> Option<GmStatus> {
    if data.len() < 42 {
        return None;
    }
    let w = |i: usize| u16::from_le_bytes([data[i], data[i + 1]]);
    let mut stats = [(0, 0); 6];
    for (i, s) in stats.iter_mut().enumerate() {
        *s = (data[2 + i * 2], data[3 + i * 2]);
    }
    Some(GmStatus { stats, atk: (w(14), w(16)), matk: (w(18), w(20)), def: (w(22), w(24)), mdef: (w(26), w(28)), hit: w(30), flee: (w(32), w(34)), crit: w(36), aspd: w(38) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_packet;

    fn encoded(r: &GmRequest) -> BytesMut {
        let mut b = BytesMut::new();
        r.encode(&mut b);
        b
    }

    #[test]
    fn requests_match_the_server_packet_table() {
        for (r, handler) in [
            (GmRequest::Hide, "GMHide"),
            (GmRequest::MapMove { map: "prontera".into(), x: 150, y: 150 }, "MapMove"),
            (GmRequest::Recall("Hayate".into()), "GMRecall"),
            (GmRequest::Shift("Hayate".into()), "GMShift"),
            (GmRequest::RecallId(2000000), "GMRecall2"),
            (GmRequest::GoTo(2000000), "GMRemove2"),
            (GmRequest::Kick(2000000), "GMKick"),
            (GmRequest::KickAll, "GMKickAll"),
            (GmRequest::Mute("Hayate".into()), "GMRc"),
            (GmRequest::FullStrip(2000000), "GMFullStrip"),
            (GmRequest::ResetStats, "ResetChar"),
            (GmRequest::ResetCooltime, "gm_resetcooltime"),
            (GmRequest::ChangeMapType { x: 1, y: 2, kind: 1 }, "GMChangeMapType"),
        ] {
            let b = encoded(&r);
            let p = client_packet(handler);
            assert_eq!(b.len(), p.len as usize, "{handler}");
            assert!(crate::generated::CLIENT.iter().any(|c| c.handler == handler && c.opcode == u16::from_le_bytes([b[0], b[1]])), "{handler} opcode");
        }
        let b = encoded(&GmRequest::Create("Red_Potion".into()));
        assert_eq!((b.len(), &b[..2]), (102, &[0xce, 0x09][..]));
    }

    #[test]
    fn broadcasts_carry_the_text_with_its_nul() {
        let b = encoded(&GmRequest::Broadcast("Hello".into()));
        assert_eq!(&b[..], &[0x99, 0x00, 10, 0, b'H', b'e', b'l', b'l', b'o', 0]);
        let b = encoded(&GmRequest::LocalBroadcast("Hi".into()));
        assert_eq!(&b[..2], &[0x9c, 0x01]);
        assert_eq!(u16::from_le_bytes([b[2], b[3]]) as usize, b.len());
    }

    #[test]
    fn names_keep_a_terminating_nul() {
        let b = encoded(&GmRequest::Recall("A".repeat(40)));
        assert_eq!(b[2 + 23], 0);
    }

    #[test]
    fn gm_status_reads_every_field() {
        let mut p = vec![0x14, 0x02];
        p.extend([90, 9, 80, 8, 70, 7, 60, 6, 50, 5, 40, 4]);
        for v in [100u16, 20, 300, 200, 30, 3, 40, 4, 150, 160, 5, 33, 180, 0] {
            p.extend(v.to_le_bytes());
        }
        let s = parse_gm_status(&p).unwrap();
        assert_eq!(s.stats[0], (90, 9));
        assert_eq!((s.atk, s.matk, s.hit, s.flee, s.crit, s.aspd), ((100, 20), (300, 200), 150, (160, 5), 33, 180));
    }
}

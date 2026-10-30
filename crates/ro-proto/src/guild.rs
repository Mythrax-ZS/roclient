//! Guilds at PACKETVER 20250402: creating, inviting, leaving, and the guild window's info,
//! member list and notice (layouts from rAthena's packets_struct.hpp).

use bytes::{Buf, BufMut, BytesMut};

use crate::{
    codec::Encode,
    packets::{get_fixed_str, put_db_packet, put_fixed_str},
};

#[derive(Debug, Clone, PartialEq)]
pub enum GuildRequest {
    /// Found a guild (consumes an Emperium). `char_id` is ours.
    Create { char_id: u32, name: String },
    /// Ask the server for the guild window's data: 0 info, 1 member list.
    Info(u32),
    Invite { name: String },
    Answer { guild: u32, accept: bool },
    Leave { guild: u32, account_id: u32, char_id: u32, reason: String },
    Expel { guild: u32, account_id: u32, char_id: u32, reason: String },
    Notice { guild: u32, subject: String, text: String },
    /// Save ranks (titles, permissions, tax).
    Positions(Vec<Position>),
    /// Move a member to a rank.
    SetRank { account_id: u32, char_id: u32, position: i32 },
    /// CZ_REQ_DISORGANIZE_GUILD: disband; the key is the guild's name.
    Disband(String),
    /// CZ_REQ_GUILD_EMBLEM_IMG: ask for a guild's emblem.
    Emblem(u32),
    /// CZ_REGISTER_GUILD_EMBLEM_IMG: upload a zlib-compressed 24x24 BMP.
    SetEmblem(Vec<u8>),
    /// CZ_REQ_ADD_NEW_EMBLEM: the emblem was uploaded to the web server as this version.
    NewEmblem { guild: u32, version: u32 },
}

impl Encode for GuildRequest {
    fn encode(&self, d: &mut BytesMut) {
        match self {
            GuildRequest::Create { char_id, name } => {
                let mut n = BytesMut::new();
                put_fixed_str(&mut n, name, 24);
                put_db_packet(d, "CreateGuild", &[&char_id.to_le_bytes(), &n]);
            }
            GuildRequest::Info(kind) => put_db_packet(d, "GuildRequestInfo", &[&kind.to_le_bytes()]),
            // CZ_REQ_JOIN_GUILD2 (0x0916): invite by character name.
            GuildRequest::Invite { name } => {
                d.put_u16_le(0x0916);
                put_fixed_str(d, name, 24);
            }
            // CZ_JOIN_GUILD (0x016b): answer 1 accepts, 0 declines.
            GuildRequest::Answer { guild, accept } => {
                d.put_u16_le(0x016b);
                d.put_u32_le(*guild);
                d.put_u32_le(*accept as u32);
            }
            // CZ_REQ_LEAVE_GUILD (0x0159) / CZ_REQ_BAN_GUILD (0x015b).
            GuildRequest::Leave { guild, account_id, char_id, reason } | GuildRequest::Expel { guild, account_id, char_id, reason } => {
                d.put_u16_le(if matches!(self, GuildRequest::Leave { .. }) { 0x0159 } else { 0x015b });
                d.put_u32_le(*guild);
                d.put_u32_le(*account_id);
                d.put_u32_le(*char_id);
                put_fixed_str(d, reason, 40);
            }
            // CZ_REG_CHANGE_GUILD_POSITIONINFO (0x0161): 40 bytes per rank.
            GuildRequest::Positions(list) => {
                let mut body = BytesMut::new();
                for p in list {
                    body.put_i32_le(p.id);
                    body.put_u32_le(p.mode);
                    body.put_i32_le(p.id);
                    body.put_i32_le(p.tax);
                    put_fixed_str(&mut body, &p.name, 24);
                }
                d.put_u16_le(0x0161);
                d.put_u16_le((4 + body.len()) as u16);
                d.put_slice(&body);
            }
            // CZ_REQ_CHANGE_MEMBERPOS (0x0155).
            GuildRequest::Disband(key) => {
                d.put_u16_le(0x015d);
                put_fixed_str(d, key, 40);
            }
            GuildRequest::Emblem(guild) => {
                d.put_u16_le(0x0151);
                d.put_u32_le(*guild);
            }
            GuildRequest::NewEmblem { guild, version } => {
                d.put_u16_le(0x0b46);
                d.put_u32_le(*guild);
                d.put_u32_le(*version);
            }
            GuildRequest::SetEmblem(data) => {
                d.put_u16_le(0x0153);
                d.put_u16_le((4 + data.len()) as u16);
                d.put_slice(data);
            }
            GuildRequest::SetRank { account_id, char_id, position } => {
                d.put_u16_le(0x0155);
                d.put_u16_le(16);
                d.put_u32_le(*account_id);
                d.put_u32_le(*char_id);
                d.put_i32_le(*position);
            }
            GuildRequest::Notice { guild, subject, text } => {
                let (mut s, mut t) = (BytesMut::new(), BytesMut::new());
                put_fixed_str(&mut s, subject, 60);
                put_fixed_str(&mut t, text, 120);
                put_db_packet(d, "GuildChangeNotice", &[&guild.to_le_bytes(), &s, &t]);
            }
        }
    }
}

/// ZC_UPDATE_GDID (0x016c): we belong to `id` (0: no guild); `master` if we lead it.
pub fn parse_belong(data: &[u8]) -> (u32, bool, String) {
    let mut b = &data[2..];
    let id = b.get_u32_le();
    b.advance(4 + 4); // emblem, mode
    let master = b.get_u8() != 0;
    b.advance(4);
    (id, master, get_fixed_str(&mut b, 24))
}

/// The guild window's summary (ZC_GUILD_INFO, 0x0b7b).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GuildInfo {
    pub id: u32,
    pub level: i32,
    pub members: i32,
    pub max_members: i32,
    pub average_level: i32,
    pub exp: i32,
    pub max_exp: i32,
    pub name: String,
    pub castle: String,
    pub master: String,
    pub emblem: u32,
}

pub fn parse_guild_info(data: &[u8]) -> GuildInfo {
    let mut b = &data[2..];
    let id = b.get_i32_le() as u32;
    let level = b.get_i32_le();
    let members = b.get_i32_le();
    let max_members = b.get_i32_le();
    let average_level = b.get_i32_le();
    let exp = b.get_i32_le();
    let max_exp = b.get_i32_le();
    b.advance(4 * 3); // points, honor, virtue
    let emblem = b.get_u32_le();
    let name = get_fixed_str(&mut b, 24);
    let castle = get_fixed_str(&mut b, 16);
    b.advance(4 + 4); // zeny, master char id
    let master = get_fixed_str(&mut b, 24);
    GuildInfo { id, level, members, max_members, average_level, exp, max_exp, name, castle, master, emblem }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GuildMember {
    pub account_id: u32,
    pub char_id: u32,
    pub job: i16,
    pub level: i16,
    pub online: bool,
    pub position: i32,
    pub name: String,
}

/// ZC_MEMBERMGR_INFO (0x0b7d): the member list, 58 bytes each.
pub fn parse_members(data: &[u8]) -> Vec<GuildMember> {
    data.get(4..)
        .unwrap_or_default()
        .chunks_exact(58)
        .map(|mut b| {
            let account_id = b.get_u32_le();
            let char_id = b.get_u32_le();
            b.advance(2 * 3); // head, palette, sex
            let job = b.get_i16_le();
            let level = b.get_i16_le();
            b.advance(4); // contribution
            let online = b.get_i32_le() != 0;
            let position = b.get_i32_le();
            b.advance(4); // last login
            GuildMember { account_id, char_id, job, level, online, position, name: get_fixed_str(&mut b, 24) }
        })
        .collect()
}

/// ZC_GUILD_NOTICE (0x016f): (subject, text).
pub fn parse_notice(data: &[u8]) -> (String, String) {
    let mut b = &data[2..];
    let subject = get_fixed_str(&mut b, 60);
    (subject, get_fixed_str(&mut b, 120))
}

/// ZC_REQ_JOIN_GUILD (0x016a): (guild id, guild name) inviting us.
pub fn parse_invite(data: &[u8]) -> (u32, String) {
    let mut b = &data[2..];
    let id = b.get_u32_le();
    (id, get_fixed_str(&mut b, 24))
}

/// ZC_UPDATE_CHARSTAT (0x01f2): a member logged in or out. (account, char, online)
pub fn parse_member_state(data: &[u8]) -> (u32, u32, bool) {
    let mut b = &data[2..];
    (b.get_u32_le(), b.get_u32_le(), b.get_u32_le() != 0)
}

/// ZC_ACK_LEAVE_GUILD2 / ZC_ACK_BAN_GUILD3 (0x0a83 / 0x0a82): (char id, reason).
pub fn parse_member_gone(opcode: u16, data: &[u8]) -> (u32, String) {
    let mut b = &data[2..];
    if opcode == 0x0a82 {
        let reason = get_fixed_str(&mut b, 40);
        (b.get_u32_le(), reason)
    } else {
        let id = b.get_u32_le();
        (id, get_fixed_str(&mut b, 40))
    }
}

/// One guild storage log line.
#[derive(Debug, Clone, PartialEq)]
pub struct StorageLogEntry {
    pub item: u32,
    pub amount: u32,
    /// Put in (true) or taken out.
    pub put: bool,
    pub refine: u32,
    pub name: String,
    pub time: String,
}

/// ZC_ACK_GUILDSTORAGE_LOG (0x09da): result (1 entries follow, 2 empty, 3 failed) and the
/// entries, 93 bytes each.
pub fn parse_storage_log(data: &[u8]) -> Option<(u16, Vec<StorageLogEntry>)> {
    let mut b = data.get(4..)?;
    if b.len() < 4 {
        return None;
    }
    let (result, count) = (b.get_u16_le(), b.get_u16_le());
    let entries = b
        .chunks_exact(93)
        .take(count as usize)
        .map(|mut e| {
            let _id = e.get_u32_le();
            let (item, amount, put, refine) = (e.get_u32_le(), e.get_u32_le(), e.get_u8() == 1, e.get_u32_le());
            e.advance(8 + 1 + 2 + 16);
            let name = get_fixed_str(&mut e, 24);
            let time = get_fixed_str(&mut e, 24);
            StorageLogEntry { item, amount, put, refine, name, time }
        })
        .collect();
    Some((result, entries))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_packet;

    #[test]
    fn requests_have_server_sizes() {
        let mut b = BytesMut::new();
        GuildRequest::Create { char_id: 7, name: "Heroes".into() }.encode(&mut b);
        assert_eq!(b.len(), client_packet("CreateGuild").len as usize);
        let mut b = BytesMut::new();
        GuildRequest::Leave { guild: 1, account_id: 2, char_id: 3, reason: String::new() }.encode(&mut b);
        assert_eq!(b.len(), 54);
        let mut b = BytesMut::new();
        GuildRequest::Answer { guild: 1, accept: true }.encode(&mut b);
        assert_eq!(b.len(), 10);
        let mut b = BytesMut::new();
        GuildRequest::Invite { name: "Buddy".into() }.encode(&mut b);
        assert_eq!(b.len(), 26);
        let mut b = BytesMut::new();
        GuildRequest::NewEmblem { guild: 3, version: 2 }.encode(&mut b);
        assert_eq!(&b[..], &[0x46, 0x0b, 3, 0, 0, 0, 2, 0, 0, 0]);
    }
}

/// A guild rank: title, permissions (mode: 0x01 invite, 0x10 expel, 0x40 storage) and
/// the share of members' experience paid to the guild (tax, 0..50 %).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Position {
    pub id: i32,
    pub name: String,
    pub mode: u32,
    pub tax: i32,
}

/// ZC_POSITION_ID_NAME_INFO (0x0166): (rank id, title) pairs, 28 bytes each.
pub fn parse_position_names(data: &[u8]) -> Vec<(i32, String)> {
    data.get(4..)
        .unwrap_or_default()
        .chunks_exact(28)
        .map(|mut b| {
            let id = b.get_i32_le();
            (id, get_fixed_str(&mut b, 24))
        })
        .collect()
}

/// ZC_POSITION_INFO (0x0160): (rank id, mode, tax), 16 bytes each.
pub fn parse_position_info(data: &[u8]) -> Vec<(i32, u32, i32)> {
    data.get(4..)
        .unwrap_or_default()
        .chunks_exact(16)
        .map(|mut b| {
            let id = b.get_i32_le();
            let mode = b.get_u32_le();
            b.advance(4); // ranking
            (id, mode, b.get_i32_le())
        })
        .collect()
}

/// ZC_ACK_CHANGE_GUILD_POSITIONINFO (0x0174): ranks after a change, 40 bytes each.
pub fn parse_positions_changed(data: &[u8]) -> Vec<Position> {
    data.get(4..)
        .unwrap_or_default()
        .chunks_exact(40)
        .map(|mut b| {
            let id = b.get_i32_le();
            let mode = b.get_u32_le();
            b.advance(4);
            let tax = b.get_i32_le();
            Position { id, mode, tax, name: get_fixed_str(&mut b, 24) }
        })
        .collect()
}

/// ZC_ACK_REQ_CHANGE_MEMBERS (0x0156): (account, char, rank) for moved members.
pub fn parse_rank_changes(data: &[u8]) -> Vec<(u32, u32, i32)> {
    data.get(4..).unwrap_or_default().chunks_exact(12).map(|mut b| (b.get_u32_le(), b.get_u32_le(), b.get_i32_le())).collect()
}

/// A guild skill (GD_*): id 10000+, as in ZC_GUILD_SKILLINFO.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GuildSkill {
    pub id: u16,
    pub level: u16,
    pub name: String,
    pub upgradable: bool,
}

/// ZC_GUILD_SKILLINFO (0x0162): (skill points, skills), 37 bytes per skill.
pub fn parse_guild_skills(data: &[u8]) -> (i16, Vec<GuildSkill>) {
    let points = i16::from_le_bytes([data[4], data[5]]);
    let skills = data
        .get(6..)
        .unwrap_or_default()
        .chunks_exact(37)
        .map(|mut b| {
            let id = b.get_u16_le();
            b.advance(4); // target type
            let level = b.get_u16_le();
            b.advance(4); // sp, range
            let name = get_fixed_str(&mut b, 24);
            GuildSkill { id, level, name, upgradable: b.get_u8() != 0 }
        })
        .collect();
    (points, skills)
}

/// Guild relations: alliance requests and replies, enemies, breaking either off.
#[derive(Debug, Clone, PartialEq)]
pub enum RelationRequest {
    /// CZ_REQ_ALLY_GUILD: ask the other guild master (account id) for an alliance.
    Ally { target: u32, me: u32, char_id: u32 },
    /// CZ_ALLY_GUILD: answer an alliance request.
    Answer { inviter: u32, accept: bool },
    /// CZ_REQ_HOSTILE_GUILD: mark that player's guild as an enemy.
    Enemy(u32),
    /// CZ_REQ_DELETE_RELATED_GUILD: relation 0 ally, 1 enemy.
    Remove { guild: u32, relation: u32 },
}

impl Encode for RelationRequest {
    fn encode(&self, d: &mut BytesMut) {
        match self {
            RelationRequest::Ally { target, me, char_id } => {
                d.put_u16_le(0x0170);
                d.put_u32_le(*target);
                d.put_u32_le(*me);
                d.put_u32_le(*char_id);
            }
            RelationRequest::Answer { inviter, accept } => {
                d.put_u16_le(0x0172);
                d.put_u32_le(*inviter);
                d.put_u32_le(*accept as u32);
            }
            RelationRequest::Enemy(target) => {
                d.put_u16_le(0x0180);
                d.put_u32_le(*target);
            }
            RelationRequest::Remove { guild, relation } => {
                d.put_u16_le(0x0183);
                d.put_u32_le(*guild);
                d.put_u32_le(*relation);
            }
        }
    }
}

/// ZC_MYGUILD_BASIC_INFO (0x014c): (relation 0 ally / 1 enemy, guild id, name).
pub fn parse_relations(data: &[u8]) -> Vec<(u32, u32, String)> {
    data.get(4..)
        .unwrap_or_default()
        .chunks_exact(32)
        .map(|mut b| {
            let (rel, id) = (b.get_u32_le(), b.get_u32_le());
            (rel, id, get_fixed_str(&mut b, 24))
        })
        .collect()
}

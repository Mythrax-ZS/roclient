//! Connection state machines for login -> char -> map.

use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use anyhow::{bail, Context};
use futures::{SinkExt, StreamExt};
use ro_proto::{id, packets::*, Frame, RoCodec, ServerTable};
use tokio::net::TcpStream;
use tokio_util::codec::Framed;

pub use ro_proto::packets::{sp, Act, CharInfo, CharRequest, ChatKind, Look, NpcDialog, NpcReply, UnitInfo, UnitPos};
pub use ro_proto::items::{Cast, FloorItem, Hotkey, ItemData, ItemRequest, Skill, StatRequest, Stats};
use ro_proto::items::*;
pub use ro_proto::guild::{GuildInfo, GuildMember, GuildRequest, GuildSkill, Position};
use ro_proto::guild::*;
pub use ro_proto::social::{Friend, FriendRequest, PartyMember, PartyRequest, SellOffer, ShopItem, ShopRequest, StorageRequest, TradeRequest};
use ro_proto::social::*;

type Conn = Framed<TcpStream, RoCodec<ServerTable>>;

const TIMEOUT: Duration = Duration::from_secs(10);
const KEEPALIVE: Duration = Duration::from_secs(10);

async fn connect(addr: impl tokio::net::ToSocketAddrs + std::fmt::Display) -> anyhow::Result<Conn> {
    let stream = TcpStream::connect(&addr).await.with_context(|| format!("connect {addr}"))?;
    stream.set_nodelay(true)?;
    Ok(Framed::new(stream, RoCodec::new(ServerTable)))
}

async fn next(conn: &mut Conn) -> anyhow::Result<Frame> {
    match tokio::time::timeout(TIMEOUT, conn.next()).await {
        Err(_) => bail!("timed out waiting for server"),
        Ok(None) => bail!("server closed the connection"),
        Ok(Some(f)) => Ok(f?),
    }
}

fn ignored(stage: &str, f: &Frame) {
    if let Frame::Packet { opcode, data } = f {
        tracing::debug!(stage, opcode = format_args!("{opcode:#06x}"), len = data.len(), "ignored");
    }
}

pub async fn login(addr: &str, user: &str, pass: &str) -> anyhow::Result<AcceptLogin> {
    let mut conn = connect(addr).await?;
    conn.send(CaLogin { version: 55, username: user, password: pass, clienttype: 0 }).await?;
    loop {
        match next(&mut conn).await? {
            Frame::Packet { opcode: id::AC_ACCEPT_LOGIN, data } => return Ok(parse_accept_login(&data)),
            Frame::Packet { opcode: id::AC_REFUSE_LOGIN, data } => bail!("login refused, error {}", parse_refuse_login(&data)),
            Frame::Packet { opcode: id::SC_NOTIFY_BAN, data } => bail!("disconnected by login server, code {}", data[2]),
            f => ignored("login", &f),
        }
    }
}

pub struct CharSession {
    conn: Conn,
    pub characters: Vec<CharInfo>,
    /// Character slots on the account (shown on the select screen).
    pub slots: u8,
    account_id: u32,
}

pub async fn enter_char(server: &CharServerInfo, auth: &AcceptLogin) -> anyhow::Result<CharSession> {
    let mut conn = connect(server.addr).await?;
    conn.send(ChEnter {
        account_id: auth.account_id,
        login_id1: auth.login_id1,
        login_id2: auth.login_id2,
        sex: auth.sex,
    })
    .await?;
    // The char server answers with a bare account id before any real packet.
    conn.codec_mut().expect_raw(4);

    let mut characters = Vec::new();
    let mut slots = 9;
    loop {
        match next(&mut conn).await? {
            Frame::Raw(_) => {}
            Frame::Packet { opcode: id::HC_REFUSE_ENTER, data } => bail!("char server refused, error {}", data[2]),
            Frame::Packet { opcode: id::HC_ACCEPT_ENTER, data } => characters.extend(parse_hc_accept_enter(&data)),
            Frame::Packet { opcode: id::HC_CHARLIST_NOTIFY, data } => {
                // Pages of three slots.
                slots = (u32::from_le_bytes(data[2..6].try_into()?) * 3).clamp(3, 15) as u8;
                conn.send(ChCharlistReq).await?;
            }
            Frame::Packet { opcode: id::HC_ACK_CHARINFO_PER_PAGE, data } => {
                let page = parse_charinfo_page(&data);
                let done = page.len() != 3; // the client-side end-of-list rule rAthena follows
                characters.extend(page);
                if done {
                    break;
                }
            }
            Frame::Packet { opcode: id::HC_SECOND_PASSWD_LOGIN, data } => {
                let state = u16::from_le_bytes(data[10..12].try_into()?);
                if state != 0 {
                    bail!("pincode required (state {state}); set pincode_enabled: no in char_athena.conf for now");
                }
            }
            f => ignored("char", &f),
        }
    }
    characters.sort_by_key(|c| c.slot);
    characters.dedup_by_key(|c| c.char_id);
    Ok(CharSession { conn, characters, slots, account_id: auth.account_id })
}

impl CharSession {
    pub async fn select(mut self, slot: u8) -> anyhow::Result<ZoneServer> {
        self.conn.send(ChSelectChar { slot }).await?;
        loop {
            match next(&mut self.conn).await? {
                Frame::Packet { opcode: id::HC_NOTIFY_ZONESVR, data } => return Ok(parse_notify_zonesvr(&data)),
                Frame::Packet { opcode: id::HC_REFUSE_ENTER, data } => bail!("char select refused, error {}", data[2]),
                f => ignored("select", &f),
            }
        }
    }

    pub async fn ping(&mut self) -> anyhow::Result<()> {
        self.conn.send(ChPing { account_id: self.account_id }).await?;
        Ok(())
    }
}

pub struct MapSession {
    conn: Conn,
    pub spawn: AcceptEnter,
    started: Instant,
}

pub async fn enter_map(zone: &ZoneServer, auth: &AcceptLogin) -> anyhow::Result<MapSession> {
    let mut conn = connect(zone.addr).await?;
    let started = Instant::now();
    conn.send(CzEnter { account_id: auth.account_id, char_id: zone.char_id, login_id1: auth.login_id1, client_tick: 0, sex: auth.sex })
        .await?;
    loop {
        match next(&mut conn).await? {
            Frame::Packet { opcode: id::ZC_ACCEPT_ENTER, data } => {
                let spawn = parse_accept_enter(&data);
                conn.send(CzLoadEndAck).await?;
                return Ok(MapSession { conn, spawn, started });
            }
            Frame::Packet { opcode: id::ZC_REFUSE_ENTER, data } => bail!("map server refused, error {}", data[2]),
            Frame::Packet { opcode: id::SC_NOTIFY_BAN, data } => bail!("disconnected by map server, code {}", data[2]),
            f => ignored("map-enter", &f),
        }
    }
}

/// What the UI hears from a running session.
#[derive(Debug, Clone)]
pub enum Event {
    /// Entered a map with this character.
    Entered { map: String, x: u16, y: u16, dir: u8, account_id: u32, character: CharInfo },
    /// Our walk was accepted: walk from -> to.
    Walk { from: (u16, u16), to: (u16, u16) },
    /// The server placed a unit (us or another) at a cell, ending or correcting a walk.
    Stop { id: u32, x: u16, y: u16 },
    /// Warped within the same map server.
    MapMove { map: String, x: u16, y: u16 },
    /// A status value changed (see `sp`): HP, SP, EXP, level, zeny, weight...
    Stat { id: u16, value: i64 },
    /// A chat line; `from` is the speaking unit when known.
    Chat { kind: ChatKind, from: Option<u32>, text: String },
    /// Another unit came into view, or (Walking) started moving.
    Unit(UnitInfo),
    /// Someone attacked (or sat, stood, picked up...).
    Act(Act),
    /// Our attack target is out of range: it stands at `target_cell`, we can reach `range`.
    OutOfRange { target: u32, target_cell: (u16, u16), range: i16 },
    MonsterHp { id: u32, hp: i32, max_hp: i32 },
    /// An NPC script wants something shown.
    Npc(NpcDialog),
    /// A unit left view (reason: 0 out of sight, 1 died, 2 logged out, 3 teleported).
    Vanish { id: u32, reason: u8 },
    /// A unit's appearance changed (see `parse_sprite_change` kinds).
    LookChange { id: u32, kind: u8, value: u32, value2: u32 },
    StorageOpen,
    StorageItems(Vec<ItemData>),
    StorageAdded(ItemData),
    StorageRemoved { index: u16, amount: u16 },
    StorageCount { used: u16, capacity: u16 },
    StorageClosed,
    /// An NPC shop asks: buy or sell?
    ShopDeal(u32),
    ShopBuyList(Vec<ShopItem>),
    ShopSellList(Vec<SellOffer>),
    ShopResult { buy: bool, ok: bool },
    TradeRequested { name: String, id: u32, level: u16 },
    /// Answer to our trade request (3 accepted; see parse_trade_ack).
    TradeAnswer(u8),
    /// The partner put up an item (id 0: zeny).
    TradeItem { id: u32, amount: i32, identified: bool },
    /// Our own item was added (or refused).
    TradeAdded { index: u16, ok: bool },
    TradeLocked { partner: bool },
    TradeEnded { completed: bool },
    Party { name: String, members: Vec<PartyMember> },
    PartyMember { name: String, member: PartyMember },
    PartyLeft { account_id: u32, name: String },
    PartyHp { account_id: u32, hp: i32, max_hp: i32 },
    PartyInvite { party: u32, name: String },
    PartyCreated { ok: bool },
    Guild { id: u32, name: String },
    /// Which guild we belong to (0: none) and whether we lead it.
    GuildBelong { id: u32, master: bool, name: String },
    /// Founding result: 0 ok, 1 already in a guild, 2 name taken, 3 no Emperium.
    GuildCreated(u8),
    GuildInfo(GuildInfo),
    GuildMembers(Vec<GuildMember>),
    GuildNotice { subject: String, text: String },
    /// A guild invites us.
    GuildInvited { guild: u32, name: String },
    /// Our invite was answered (0 already in a guild, 1 refused, 2 joined, 3 guild full).
    GuildInviteResult(u8),
    GuildMemberState { account_id: u32, char_id: u32, online: bool },
    GuildMemberGone { char_id: u32, reason: String, expelled: bool },
    /// Rank titles (id, name), and rank settings (id, mode, tax).
    GuildRankNames(Vec<(i32, String)>),
    GuildRankInfo(Vec<(i32, u32, i32)>),
    GuildRanksChanged(Vec<Position>),
    /// Members moved to ranks: (account, char, rank).
    GuildRankMoves(Vec<(u32, u32, i32)>),
    GuildSkills { points: i16, skills: Vec<GuildSkill> },
    Friends(Vec<Friend>),
    FriendOnline { account_id: u32, char_id: u32, online: bool },
    FriendRequested { account_id: u32, char_id: u32, name: String },
    FriendAdded { ok: bool, friend: Friend },
    FriendRemoved { account_id: u32, char_id: u32 },
    /// The server is about to resend the whole inventory.
    InventoryStart,
    Items(Vec<ItemData>),
    ItemAdded(ItemData),
    ItemRemoved { index: u16, amount: u16 },
    /// An item was used (by `user`); `amount` is what remains.
    ItemUsed { index: u16, amount: u16, user: u32, ok: bool },
    Equipped { index: u16, slots: u32, look: u16, ok: bool },
    Unequipped { index: u16, slots: u32, ok: bool },
    FloorItem(FloorItem),
    FloorItemGone(u32),
    Stats(Stats),
    StatRaised { stat: u16, ok: bool, value: u8 },
    StatCost { stat: u16, cost: u8 },
    /// A base stat and its equipment/buff bonus (stat: SP_STR..SP_LUK, 13..=18).
    StatBonus { stat: u32, base: i32, plus: i32 },
    Skills(Vec<Skill>),
    SkillUpdate { id: u16, level: u16, sp: u16, range: u16, upgradable: bool },
    /// Shortcut bar (tab 0): visible row and all slots.
    Hotkeys { row: u8, keys: Vec<Hotkey> },
    /// Someone started casting.
    Casting(Cast),
    /// Our skill was refused (see `parse_skill_fail` causes).
    SkillFailed { skill: u16, cause: u8 },
    /// A non-damaging skill went off (heal, buff...).
    SkillUsed { skill: u16, src: u32, target: u32 },
    /// A damaging skill hit (animated like an attack).
    SkillHit { skill: u16, act: Act },
    /// A skill was placed on a cell.
    SkillAt { skill: u16, src: u32, cell: (u16, u16) },
    /// A unit's status option bits changed (hidden, cloaked, ...).
    UnitOption { id: u32, option: u32 },
    /// A lasting ground effect (Sanctuary, Fire Wall, traps...) appeared or ended.
    GroundEffect { id: u32, cell: (u16, u16), kind: u32 },
    GroundEffectGone(u32),
    /// A status icon turned on or off on a unit (EFST id; 160 is night).
    StatusIcon { id: u32, kind: u16, on: bool },
    /// A dead unit (maybe us) was brought back.
    Resurrected(u32),
    /// A misc effect on a unit (0 base level up, 1 job level up, ...).
    UnitEffect { id: u32, effect: u32 },
    /// Login screen: refused (message for the player).
    LoginFailed(String),
    /// More than one server: (name, users) to pick from.
    Servers(Vec<(String, u16)>),
    /// The character select screen: characters and how many slots the account has.
    Characters { chars: Vec<CharInfo>, slots: u8 },
    CharCreated(CharInfo),
    /// 0 name taken, 1 under age, 2 symbols not allowed, 3 not eligible slot, 11 premium only.
    CharCreateFailed(u8),
    /// Deletion replies: (char id, result: 1 ok, seconds until confirmable).
    CharDeleteStarted { char_id: u32, result: i32, seconds: u32 },
    CharDeleted { char_id: u32, result: i32 },
    CharDeleteCanceled { char_id: u32, result: i32 },
    Disconnected(String),
}

/// What the UI asks a session to do.
#[derive(Debug, Clone)]
pub enum Command {
    Item(ItemRequest),
    Shop(ShopRequest),
    Storage(StorageRequest),
    Trade(TradeRequest),
    Party(PartyRequest),
    Friend(FriendRequest),
    Guild(GuildRequest),
    Stat(StatRequest),
    Move { x: u16, y: u16 },
    /// Attack a unit (continuously when `keep`).
    Attack { target: u32, keep: bool },
    /// Sit down (true) or stand up.
    Sit(bool),
    /// After dying: 0 return to the save point, 1 character select.
    Restart(u8),
    /// Talk to an NPC or answer its dialog.
    Npc { npc: u32, reply: NpcReply },
    /// Say something in public chat.
    Chat(String),
    /// Login screen.
    Login { user: String, pass: String },
    SelectServer(usize),
    /// Character select screen: create / delete / cancel deletion.
    Char(CharRequest),
    SelectChar(u8),
}

#[derive(Clone)]
pub struct SessionConfig {
    pub login_addr: String,
    pub username: String,
    pub password: String,
    pub char_slot: u8,
    /// Log straight in with these credentials and slot (headless and scripted runs).
    /// Otherwise the session waits for Login / SelectChar commands from the UI.
    pub auto: bool,
}

/// Runs login -> char select -> map on its own thread and runtime. Events arrive on the
/// returned receiver; commands go in through the sender. Dropping the sender ends it.
pub fn spawn_session(cfg: SessionConfig) -> (tokio::sync::mpsc::UnboundedSender<Command>, std::sync::mpsc::Receiver<Event>) {
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
    let (ev_tx, ev_rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("net".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = ev_tx.send(Event::Disconnected(e.to_string()));
                    return;
                }
            };
            let result: anyhow::Result<()> = rt.block_on(async {
                let mut cmd_rx = cmd_rx;
                let (auth, character, mut zone) = if cfg.auto {
                    // Scripted / headless runs: straight in with the configured account and slot.
                    let auth = login(&cfg.login_addr, &cfg.username, &cfg.password).await?;
                    let server = auth.servers.first().context("no char servers offered")?.clone();
                    let chars = enter_char(&server, &auth).await?;
                    let character = chars
                        .characters
                        .iter()
                        .find(|c| c.slot == cfg.char_slot)
                        .cloned()
                        .with_context(|| format!("no character in slot {}", cfg.char_slot))?;
                    let zone = chars.select(cfg.char_slot).await?;
                    (auth, character, zone)
                } else {
                    match front_end(&cfg.login_addr, &mut cmd_rx, &ev_tx).await? {
                        Some(picked) => picked,
                        None => return Ok(()),
                    }
                };
                let char_id = zone.char_id;
                let mut first = true;
                loop {
                    let map = enter_map(&zone, &auth).await?;
                    let name = zone.map.trim_end_matches(".gat").to_string();
                    let event = if first {
                        Event::Entered {
                            map: name,
                            x: map.spawn.x,
                            y: map.spawn.y,
                            dir: map.spawn.dir,
                            account_id: auth.account_id,
                            character: character.clone(),
                        }
                    } else {
                        Event::MapMove { map: name, x: map.spawn.x, y: map.spawn.y }
                    };
                    first = false;
                    if ev_tx.send(event).is_err() {
                        return Ok(());
                    }
                    match map.run(&character.name, &mut cmd_rx, &ev_tx).await? {
                        // Warped to a map on another map server: re-enter there.
                        Some(next) => zone = ZoneServer { char_id, ..next },
                        None => return Ok(()),
                    }
                }
            });
            let reason = match result {
                Ok(()) => "closed".to_string(),
                Err(e) => format!("{e:#}"),
            };
            let _ = ev_tx.send(Event::Disconnected(reason));
        })
        .expect("spawn net thread");
    (cmd_tx, ev_rx)
}

impl MapSession {
    /// Pumps the map connection: keepalives, UI commands, and the packets the UI cares about.
    /// Returns the next zone server when the server moves us to another map server.
    pub async fn run(
        mut self,
        name: &str,
        commands: &mut tokio::sync::mpsc::UnboundedReceiver<Command>,
        events: &std::sync::mpsc::Sender<Event>,
    ) -> anyhow::Result<Option<ZoneServer>> {
        let mut keepalive = tokio::time::interval(KEEPALIVE);
        loop {
            tokio::select! {
                _ = keepalive.tick() => {
                    let tick = self.tick();
                    self.send(CzRequestTime { client_tick: tick }).await?;
                }
                cmd = commands.recv() => match cmd {
                    None => return Ok(None),
                    Some(Command::Move { x, y }) => self.send(CzRequestMove { x, y, dir: 0 }).await?,
                    Some(Command::Chat(text)) => self.send(CzChat { name, text: &text }).await?,
                    Some(Command::Attack { target, keep }) => self.send(CzRequestAct { target, action: if keep { 7 } else { 0 } }).await?,
                    Some(Command::Sit(sit)) => self.send(CzRequestAct { target: 0, action: if sit { 2 } else { 3 } }).await?,
                    Some(Command::Restart(kind)) => self.send(CzRestart { kind }).await?,
                    // Login and character-select screen commands mean nothing once in game.
                    Some(Command::Login { .. } | Command::SelectServer(_) | Command::Char(_) | Command::SelectChar(_)) => {}
                    Some(Command::Npc { npc, reply }) => self.send(CzNpc { npc, reply: &reply }).await?,
                    Some(Command::Item(r)) => self.send(r).await?,
                    Some(Command::Shop(r)) => self.send(r).await?,
                    Some(Command::Storage(r)) => self.send(r).await?,
                    Some(Command::Trade(r)) => self.send(r).await?,
                    Some(Command::Party(r)) => self.send(r).await?,
                    Some(Command::Friend(r)) => self.send(r).await?,
                    Some(Command::Guild(r)) => self.send(r).await?,
                    Some(Command::Stat(r)) => self.send(r).await?,
                },
                f = self.conn.next() => {
                    let Some(f) = f else { bail!("map server closed the connection") };
                    let Frame::Packet { opcode, data } = f? else { continue };
                    let event = match opcode {
                        id::ZC_NOTIFY_PLAYERMOVE => {
                            let (_, from, to) = parse_player_move(&data);
                            Some(Event::Walk { from, to })
                        }
                        id::ZC_STOPMOVE => {
                            let (id, x, y) = parse_stop_move(&data);
                            Some(Event::Stop { id, x, y })
                        }
                        id::ZC_NPCACK_MAPMOVE => {
                            let (map, x, y) = parse_map_move(&data);
                            // The server waits for the client to finish loading again.
                            self.send(CzLoadEndAck).await?;
                            Some(Event::MapMove { map: map.trim_end_matches(".gat").to_string(), x, y })
                        }
                        0x09fd | 0x09fe | 0x09ff => parse_unit(opcode, &data).map(Event::Unit),
                        0x00b4 | 0x00b5 | 0x00b6 | 0x00b7 | 0x0142 | 0x01d4 | 0x08d6 | 0x01b3 => parse_npc_dialog(opcode, &data).map(Event::Npc),
                        // Inventory (invType 0 only; carts and storage come later).
                        // Kafra storage: the same item lists with invType 2.
                        0x0b08 if data.get(4) == Some(&2) => Some(Event::StorageOpen),
                        0x0b09 | 0x0b39 if data.get(4) == Some(&2) => {
                            Some(Event::StorageItems(if opcode == 0x0b09 { parse_itemlist_normal(&data) } else { parse_itemlist_equip(&data) }))
                        }
                        0x0b44 => Some(Event::StorageAdded(parse_store_add(&data))),
                        0x00f6 => {
                            let (index, amount) = parse_store_delete(&data);
                            Some(Event::StorageRemoved { index, amount })
                        }
                        0x00f2 => {
                            let (used, capacity) = parse_store_count(&data);
                            Some(Event::StorageCount { used, capacity })
                        }
                        0x00f8 => Some(Event::StorageClosed),
                        0x00c4 => Some(Event::ShopDeal(parse_deal_type(&data))),
                        0x0b77 => Some(Event::ShopBuyList(parse_buy_list(&data))),
                        0x00c7 => Some(Event::ShopSellList(parse_sell_list(&data))),
                        0x00ca | 0x00cb => Some(Event::ShopResult { buy: opcode == 0x00ca, ok: data[2] == 0 }),
                        0x01f4 => {
                            let (name, id, level) = parse_trade_request(&data);
                            Some(Event::TradeRequested { name, id, level })
                        }
                        0x01f5 => Some(Event::TradeAnswer(parse_trade_ack(&data))),
                        0x0b42 => {
                            let (id, amount, identified) = parse_trade_item(&data);
                            Some(Event::TradeItem { id, amount, identified: identified != 0 })
                        }
                        0x00ea => Some(Event::TradeAdded { index: u16::from_le_bytes([data[2], data[3]]), ok: data[4] == 0 }),
                        0x00ec => Some(Event::TradeLocked { partner: data[2] != 0 }),
                        0x00ee => Some(Event::TradeEnded { completed: false }),
                        0x00f0 => Some(Event::TradeEnded { completed: data[2] == 0 }),
                        0x0ae5 => {
                            let (name, members) = parse_party_list(&data);
                            Some(Event::Party { name, members })
                        }
                        0x0ae4 => {
                            let (name, member) = parse_party_add(&data);
                            Some(Event::PartyMember { name, member })
                        }
                        0x0105 => {
                            let (account_id, name) = parse_party_remove(&data);
                            Some(Event::PartyLeft { account_id, name })
                        }
                        0x080e => {
                            let (account_id, hp, max_hp) = parse_party_hp(&data);
                            Some(Event::PartyHp { account_id, hp, max_hp })
                        }
                        0x02c6 => {
                            let (party, name) = parse_party_invite(&data);
                            Some(Event::PartyInvite { party, name })
                        }
                        0x00fa => Some(Event::PartyCreated { ok: data[2] == 0 }),
                        0x0109 => Some(Event::Chat { kind: ChatKind::Party, from: None, text: parse_party_chat(&data) }),
                        0x017f => Some(Event::Chat { kind: ChatKind::Guild, from: None, text: parse_guild_chat(&data) }),
                        // ZC_UPDATE_GDID: 0x02f7 at this PACKETVER (0x016c is the older form).
                        0x016c | 0x02f7 => {
                            let (id, master, name) = parse_belong(&data);
                            Some(Event::GuildBelong { id, master, name })
                        }
                        0x0167 => Some(Event::GuildCreated(data[2])),
                        0x0166 => Some(Event::GuildRankNames(parse_position_names(&data))),
                        0x0160 => Some(Event::GuildRankInfo(parse_position_info(&data))),
                        0x0174 => Some(Event::GuildRanksChanged(parse_positions_changed(&data))),
                        0x0156 => Some(Event::GuildRankMoves(parse_rank_changes(&data))),
                        0x0162 => {
                            let (points, skills) = parse_guild_skills(&data);
                            Some(Event::GuildSkills { points, skills })
                        }
                        0x0b7b => Some(Event::GuildInfo(parse_guild_info(&data))),
                        0x0b7d => Some(Event::GuildMembers(parse_members(&data))),
                        0x016f => {
                            let (subject, text) = parse_notice(&data);
                            Some(Event::GuildNotice { subject, text })
                        }
                        0x016a => {
                            let (guild, name) = parse_invite(&data);
                            Some(Event::GuildInvited { guild, name })
                        }
                        0x0169 => Some(Event::GuildInviteResult(data[2])),
                        0x01f2 => {
                            let (account_id, char_id, online) = parse_member_state(&data);
                            Some(Event::GuildMemberState { account_id, char_id, online })
                        }
                        0x0a82 | 0x0a83 => {
                            let (char_id, reason) = parse_member_gone(opcode, &data);
                            Some(Event::GuildMemberGone { char_id, reason, expelled: opcode == 0x0a82 })
                        }
                        0x0201 => Some(Event::Friends(parse_friends(&data))),
                        0x0206 => {
                            let (account_id, char_id, online) = parse_friend_state(&data);
                            Some(Event::FriendOnline { account_id, char_id, online })
                        }
                        0x0207 => {
                            let (account_id, char_id, name) = parse_friend_request(&data);
                            Some(Event::FriendRequested { account_id, char_id, name })
                        }
                        0x0209 => {
                            let (result, friend) = parse_friend_added(&data);
                            Some(Event::FriendAdded { ok: result == 0, friend })
                        }
                        0x020a => {
                            let (account_id, char_id) = parse_friend_removed(&data);
                            Some(Event::FriendRemoved { account_id, char_id })
                        }
                        0x0b08 if data.get(4) == Some(&0) => Some(Event::InventoryStart),
                        0x0b09 | 0x0b39 if data.get(4) == Some(&0) => {
                            Some(Event::Items(if opcode == 0x0b09 { parse_itemlist_normal(&data) } else { parse_itemlist_equip(&data) }))
                        }
                        0x0b41 => parse_pickup(&data).map(Event::ItemAdded),
                        0x07fa | 0x00af => parse_item_removed(opcode, &data).map(|(index, amount)| Event::ItemRemoved { index, amount }),
                        0x01c8 => {
                            let (index, amount, user, ok) = parse_use_item_ack(&data);
                            Some(Event::ItemUsed { index, amount, user, ok })
                        }
                        0x0999 => {
                            let (index, slots, look, ok) = parse_wear_ack(&data);
                            Some(Event::Equipped { index, slots, look, ok })
                        }
                        0x099a => {
                            let (index, slots, ok) = parse_takeoff_ack(&data);
                            Some(Event::Unequipped { index, slots, ok })
                        }
                        0x0add | 0x009d => parse_floor_item(opcode, &data).map(Event::FloorItem),
                        0x00a1 => Some(Event::FloorItemGone(parse_item_disappear(&data))),
                        0x01d7 => {
                            let (id, kind, value, value2) = parse_sprite_change(&data);
                            Some(Event::LookChange { id, kind, value, value2 })
                        }
                        0x00bd => Some(Event::Stats(parse_status(&data))),
                        0x00bc => {
                            let (stat, ok, value) = parse_status_change_ack(&data);
                            Some(Event::StatRaised { stat, ok, value })
                        }
                        0x00be => {
                            let (stat, cost) = parse_status_cost(&data);
                            Some(Event::StatCost { stat, cost })
                        }
                        0x010f => Some(Event::Skills(parse_skill_list(&data))),
                        0x010e => {
                            let (id, level, sp, range, upgradable) = parse_skill_update(&data);
                            Some(Event::SkillUpdate { id, level, sp, range, upgradable })
                        }
                        id::ZC_NOTIFY_ACT => Some(Event::Act(parse_act(&data))),
                        0x0b20 => match parse_hotkeys(&data) {
                            (0, row, keys) => Some(Event::Hotkeys { row, keys }),
                            _ => None,
                        },
                        0x0b1a => Some(Event::Casting(parse_cast(&data))),
                        0x0110 => {
                            let (skill, cause) = parse_skill_fail(&data);
                            Some(Event::SkillFailed { skill, cause })
                        }
                        0x01de => {
                            let (skill, act) = parse_skill_hit(&data);
                            Some(Event::SkillHit { skill, act })
                        }
                        0x0117 => {
                            let (skill, src, cell) = parse_ground_skill(&data);
                            Some(Event::SkillAt { skill, src, cell })
                        }
                        0x0229 => {
                            let (id, option) = parse_state_change(&data);
                            Some(Event::UnitOption { id, option })
                        }
                        0x0141 => {
                            let (stat, base, plus) = parse_couple_status(&data);
                            Some(Event::StatBonus { stat, base, plus })
                        }
                        0x09ca => {
                            let (id, _, cell, kind) = parse_skill_entry(&data);
                            Some(Event::GroundEffect { id, cell, kind })
                        }
                        0x0120 => Some(Event::GroundEffectGone(parse_skill_disappear(&data))),
                        0x0196 | 0x0983 => Some(Event::StatusIcon {
                            kind: u16::from_le_bytes([data[2], data[3]]),
                            id: u32::from_le_bytes([data[4], data[5], data[6], data[7]]),
                            on: data[8] != 0,
                        }),
                        0x0148 => Some(Event::Resurrected(u32::from_le_bytes([data[2], data[3], data[4], data[5]]))),
                        0x019b => {
                            let (id, effect) = parse_notify_effect(&data);
                            Some(Event::UnitEffect { id, effect })
                        }
                        0x09cb => {
                            let (skill, _, target, src) = parse_skill_used(&data);
                            Some(Event::SkillUsed { skill, src, target })
                        }
                        id::ZC_ATTACK_FAILURE_FOR_DISTANCE => {
                            let (target, target_cell, _, range) = parse_attack_out_of_range(&data);
                            Some(Event::OutOfRange { target, target_cell, range })
                        }
                        0x0977 => {
                            let (id, hp, max_hp) = parse_monster_hp(&data);
                            Some(Event::MonsterHp { id, hp, max_hp })
                        }
                        0x00b0 | 0x00b1 | 0x0acb => parse_par_change(opcode, &data).map(|(id, value)| Event::Stat { id, value }),
                        0x008d | 0x008e | 0x02c1 | 0x09de | 0x009a | 0x01c3 => {
                            parse_chat(opcode, &data).map(|(kind, from, text)| Event::Chat { kind, from, text })
                        }
                        id::ZC_NOTIFY_VANISH => {
                            let (id, reason) = parse_vanish(&data);
                            Some(Event::Vanish { id, reason })
                        }
                        id::ZC_NPCACK_SERVERMOVE => {
                            let (zone, x, y) = parse_server_move(&data);
                            tracing::info!(map = %zone.map, x, y, addr = %zone.addr, "moving to another map server");
                            return Ok(Some(zone));
                        }
                        id::SC_NOTIFY_BAN => bail!("disconnected by map server, code {}", data[2]),
                        _ => {
                            tracing::debug!(opcode = format_args!("{opcode:#06x}"), len = data.len(), "unhandled");
                            None
                        }
                    };
                    if let Some(e) = event {
                        if events.send(e).is_err() {
                            return Ok(None); // UI gone
                        }
                    }
                }
            }
        }
    }

    fn tick(&self) -> u32 {
        self.started.elapsed().as_millis() as u32
    }

    pub async fn send<P: ro_proto::Encode>(&mut self, p: P) -> anyhow::Result<()> {
        self.conn.send(p).await?;
        Ok(())
    }

    /// Stay connected for `dur`, answering keepalives, and return how often each opcode arrived.
    /// Fails on the first frame the generated table cannot size.
    pub async fn soak(&mut self, dur: Duration) -> anyhow::Result<BTreeMap<u16, usize>> {
        let deadline = tokio::time::Instant::now() + dur;
        let mut keepalive = tokio::time::interval(KEEPALIVE);
        let mut seen = BTreeMap::new();
        // Recent frames, to name the culprit when framing breaks.
        let mut recent: std::collections::VecDeque<(u16, usize)> = Default::default();
        loop {
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => return Ok(seen),
                _ = keepalive.tick() => {
                    let tick = self.tick();
                    self.send(CzRequestTime { client_tick: tick }).await?;
                }
                f = self.conn.next() => match f {
                    None => bail!("map server closed the connection"),
                    Some(Err(e)) => {
                        let recent: Vec<String> = recent.iter().map(|(op, len)| format!("{op:#06x}({len})")).collect();
                        bail!("{e}\nlast frames: {}", recent.join(" "));
                    }
                    Some(Ok(Frame::Packet { opcode, data })) => {
                        if recent.len() == 8 {
                            recent.pop_front();
                        }
                        recent.push_back((opcode, data.len()));
                        if opcode == id::SC_NOTIFY_BAN {
                            bail!("disconnected by map server, code {}", data[2]);
                        }
                        *seen.entry(opcode).or_insert(0) += 1;
                    }
                    Some(Ok(Frame::Raw(_))) => {}
                },
            }
        }
    }
}

/// Why the login server refused us, in the player's words (AC_REFUSE_LOGIN codes).
fn login_error(code: u32) -> String {
    match code {
        0 => "Unregistered ID.".into(),
        1 => "Incorrect password.".into(),
        2 => "This ID has expired.".into(),
        3 => "Rejected by the server.".into(),
        4 => "This account has been blocked.".into(),
        5 => "Your client is out of date.".into(),
        6 => "This account is temporarily banned.".into(),
        7 => "The server is full.".into(),
        8 => "This account is already logged in.".into(),
        99 => "This ID has been deleted.".into(),
        c => format!("Login refused (error {c})."),
    }
}

/// The login and character-select screens: waits for the UI's Login, SelectServer and
/// character commands, and returns once a character is picked (None if the UI closed).
async fn front_end(
    addr: &str,
    cmds: &mut tokio::sync::mpsc::UnboundedReceiver<Command>,
    events: &std::sync::mpsc::Sender<Event>,
) -> anyhow::Result<Option<(AcceptLogin, CharInfo, ZoneServer)>> {
    'login: loop {
        // Login screen.
        let auth = loop {
            let Some(cmd) = cmds.recv().await else { return Ok(None) };
            let Command::Login { user, pass } = cmd else { continue };
            match login_once(addr, &user, &pass).await {
                Ok(auth) => break auth,
                Err(e) => {
                    let _ = events.send(Event::LoginFailed(e));
                }
            }
        };
        let server = match auth.servers.len() {
            0 => {
                let _ = events.send(Event::LoginFailed("No servers are online.".into()));
                continue 'login;
            }
            1 => auth.servers[0].clone(),
            _ => {
                let _ = events.send(Event::Servers(auth.servers.iter().map(|s| (s.name.clone(), s.users)).collect()));
                loop {
                    match cmds.recv().await {
                        None => return Ok(None),
                        Some(Command::SelectServer(i)) if i < auth.servers.len() => break auth.servers[i].clone(),
                        Some(Command::Login { .. }) => continue 'login,
                        _ => {}
                    }
                }
            }
        };
        let mut chars = match enter_char(&server, &auth).await {
            Ok(c) => c,
            Err(e) => {
                let _ = events.send(Event::LoginFailed(format!("{e:#}")));
                continue 'login;
            }
        };
        let _ = events.send(Event::Characters { chars: chars.characters.clone(), slots: chars.slots });
        // Character select screen.
        let mut keepalive = tokio::time::interval(Duration::from_secs(10));
        loop {
            tokio::select! {
                _ = keepalive.tick() => chars.ping().await?,
                cmd = cmds.recv() => match cmd {
                    None => return Ok(None),
                    Some(Command::Char(req)) => chars.conn.send(req).await?,
                    Some(Command::SelectChar(slot)) => {
                        let Some(character) = chars.characters.iter().find(|c| c.slot == slot).cloned() else { continue };
                        let zone = chars.select(slot).await?;
                        return Ok(Some((auth, character, zone)));
                    }
                    _ => {}
                },
                frame = next(&mut chars.conn) => match frame? {
                    Frame::Packet { opcode: 0x0b6f, data } => {
                        let c = parse_char_info(&data[2..]);
                        chars.characters.push(c.clone());
                        let _ = events.send(Event::CharCreated(c));
                    }
                    Frame::Packet { opcode: 0x006e, data } => {
                        let _ = events.send(Event::CharCreateFailed(data[2]));
                    }
                    Frame::Packet { opcode: 0x0828, data } => {
                        let (char_id, result, seconds) = parse_delete_reply(&data);
                        let _ = events.send(Event::CharDeleteStarted { char_id, result, seconds });
                    }
                    Frame::Packet { opcode: 0x082a, data } => {
                        let (char_id, result, _) = parse_delete_reply(&data);
                        if result == 1 {
                            chars.characters.retain(|c| c.char_id != char_id);
                        }
                        let _ = events.send(Event::CharDeleted { char_id, result });
                    }
                    Frame::Packet { opcode: 0x082c, data } => {
                        let (char_id, result, _) = parse_delete_reply(&data);
                        let _ = events.send(Event::CharDeleteCanceled { char_id, result });
                    }
                    f => ignored("char select", &f),
                },
            }
        }
    }
}

/// One login attempt, with the refusal turned into a message.
async fn login_once(addr: &str, user: &str, pass: &str) -> Result<AcceptLogin, String> {
    let mut conn = connect(addr).await.map_err(|e| format!("Cannot reach the login server ({addr}): {e}"))?;
    conn.send(CaLogin { version: 55, username: user, password: pass, clienttype: 0 }).await.map_err(|e| e.to_string())?;
    loop {
        match next(&mut conn).await.map_err(|e| e.to_string())? {
            Frame::Packet { opcode: id::AC_ACCEPT_LOGIN, data } => return Ok(parse_accept_login(&data)),
            Frame::Packet { opcode: id::AC_REFUSE_LOGIN, data } => return Err(login_error(parse_refuse_login(&data))),
            Frame::Packet { opcode: id::SC_NOTIFY_BAN, data } => return Err(login_error(data[2] as u32)),
            f => ignored("login", &f),
        }
    }
}

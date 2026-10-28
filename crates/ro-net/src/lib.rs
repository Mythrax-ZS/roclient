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

pub use ro_proto::packets::{sp, Act, CharInfo, ChatKind, Look, NpcDialog, NpcReply, UnitInfo, UnitPos};
pub use ro_proto::items::{FloorItem, ItemData, ItemRequest, Skill, StatRequest, Stats};
use ro_proto::items::*;

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
    loop {
        match next(&mut conn).await? {
            Frame::Raw(_) => {}
            Frame::Packet { opcode: id::HC_REFUSE_ENTER, data } => bail!("char server refused, error {}", data[2]),
            Frame::Packet { opcode: id::HC_ACCEPT_ENTER, data } => characters.extend(parse_hc_accept_enter(&data)),
            Frame::Packet { opcode: id::HC_CHARLIST_NOTIFY, .. } => conn.send(ChCharlistReq).await?,
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
    Ok(CharSession { conn, characters, account_id: auth.account_id })
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
    Skills(Vec<Skill>),
    SkillUpdate { id: u16, level: u16, sp: u16, range: u16, upgradable: bool },
    Disconnected(String),
}

/// What the UI asks a session to do.
#[derive(Debug, Clone)]
pub enum Command {
    Item(ItemRequest),
    Stat(StatRequest),
    Move { x: u16, y: u16 },
    /// Attack a unit (continuously when `keep`).
    Attack { target: u32, keep: bool },
    /// Talk to an NPC or answer its dialog.
    Npc { npc: u32, reply: NpcReply },
    /// Say something in public chat.
    Chat(String),
}

pub struct SessionConfig {
    pub login_addr: String,
    pub username: String,
    pub password: String,
    pub char_slot: u8,
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
                let auth = login(&cfg.login_addr, &cfg.username, &cfg.password).await?;
                let server = auth.servers.first().context("no char servers offered")?.clone();
                let chars = enter_char(&server, &auth).await?;
                let character = chars
                    .characters
                    .iter()
                    .find(|c| c.slot == cfg.char_slot)
                    .cloned()
                    .with_context(|| format!("no character in slot {}", cfg.char_slot))?;
                let mut zone = chars.select(cfg.char_slot).await?;
                let char_id = zone.char_id;
                let mut cmd_rx = cmd_rx;
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
                    Some(Command::Npc { npc, reply }) => self.send(CzNpc { npc, reply: &reply }).await?,
                    Some(Command::Item(r)) => self.send(r).await?,
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

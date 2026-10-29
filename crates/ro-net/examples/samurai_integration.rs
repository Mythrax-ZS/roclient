//! Headless Samurai acceptance test against the disposable rAthena validation stack.
//! Run: cargo run -p ro-net --example samurai_integration
//! Requires the test fixture/account documented in rathena/build/samurai-test.
//! This deliberately refuses the normal login/char/map ports and any remote address.

use anyhow::{bail, ensure, Context, Result};
use ro_net::{Command, Event, ItemData, ItemRequest, Skill, StatRequest};
use std::{collections::{BTreeMap, BTreeSet}, net::SocketAddr, sync::mpsc, time::{Duration, Instant}};
use tokio::sync::mpsc::UnboundedSender;

const LOGIN: &str = "127.0.0.1:55120";
const USER: &str = "samurai_test";
const PASSWORD: &str = "samurai-test-only";

#[derive(Default)]
struct State {
    account: u32,
    items: BTreeMap<u16, ItemData>,
    skills: BTreeMap<u16, Skill>,
    stats: BTreeMap<u16, i64>,
    buffs: BTreeSet<u16>,
    dummy: Option<u32>,
    hits: Vec<(u16, i32, i32)>,
    attacks: Vec<(i32, i32)>,
    failed: Vec<(u16, u8)>,
    used: Vec<u16>,
    hp_updates: Vec<i64>,
}

impl State {
    fn accept(&mut self, e: Event) -> Result<()> {
        match e {
            Event::Disconnected(reason) => bail!("test session disconnected: {reason}"),
            Event::InventoryStart => self.items.clear(),
            Event::Items(items) => self.items.extend(items.into_iter().map(|i| (i.index, i))),
            Event::ItemAdded(item) => { self.items.insert(item.index, item); }
            Event::Equipped { index, slots, ok, .. } => {
                ensure!(ok, "equip refused for inventory index {index}, slots {slots:#x}");
                if let Some(item) = self.items.get_mut(&index) { item.equipped = slots; }
                println!("EQUIP index={index} slots={slots:#x}");
            }
            Event::Skills(skills) => self.skills = skills.into_iter().map(|s| (s.id, s)).collect(),
            Event::SkillAdded(skill) => { self.skills.insert(skill.id, skill); }
            Event::SkillUpdate { id, level, sp, range, upgradable } => {
                if let Some(skill) = self.skills.get_mut(&id) {
                    (skill.level, skill.sp, skill.range, skill.upgradable) = (level, sp, range, upgradable);
                }
            }
            Event::Stat { id, value } => {
                self.stats.insert(id, value);
                if id == ro_net::sp::HP { self.hp_updates.push(value); }
            }
            Event::Stats(s) => {
                self.stats.insert(49, s.hit as i64);
                self.stats.insert(53, (s.aspd.0 + s.aspd.1) as i64);
            }
            Event::StatusIcon { id, kind, on, remain_ms } if id == self.account && (2000..=2004).contains(&kind) => {
                if on { self.buffs.insert(kind); } else { self.buffs.remove(&kind); }
                println!("STATUS {kind} on={on} remaining={remain_ms:?}");
            }
            Event::SkillFailed { skill, cause, .. } => {
                self.failed.push((skill, cause));
                println!("SKILL_FAILED {skill} cause={cause}");
            }
            Event::SkillUsed { skill, src, .. } if src == self.account => {
                self.used.push(skill);
                println!("SKILL_USED {skill}");
            }
            Event::SkillHit { skill, act } if act.src == self.account => {
                self.hits.push((skill, act.damage, act.left_damage));
                println!("SKILL_HIT {skill} damage={} left={}", act.damage, act.left_damage);
            }
            Event::SkillHit { skill, act } if act.target == self.account => {
                println!("INCOMING_SKILL {skill} damage={} left={}", act.damage, act.left_damage);
            }
            Event::Act(act) if act.src == self.account && Some(act.target) == self.dummy => {
                self.attacks.push((act.damage, act.left_damage));
                println!("ATTACK damage={} left={}", act.damage, act.left_damage);
            }
            Event::Chat { text, .. } => {
                if let Some(rest) = text.strip_prefix("SAMURAI_DUMMY ") { self.dummy = rest.trim().parse().ok(); }
                println!("CHAT {text}");
            }
            _ => {}
        }
        Ok(())
    }

    fn stat(&self, id: u16) -> Result<i64> {
        self.stats.get(&id).copied().with_context(|| format!("missing stat {id}"))
    }
}

struct Test {
    tx: UnboundedSender<Command>,
    rx: mpsc::Receiver<Event>,
    state: State,
}

impl Test {
    async fn pump(&mut self, ms: u64) -> Result<()> {
        let until = Instant::now() + Duration::from_millis(ms);
        loop {
            while let Ok(e) = self.rx.try_recv() { self.state.accept(e)?; }
            if Instant::now() >= until { return Ok(()); }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
    fn send(&self, command: Command) -> Result<()> { self.tx.send(command).context("send test command") }
    async fn chat(&mut self, text: &str) -> Result<()> {
        self.send(Command::Chat(text.into()))?;
        self.pump(450).await
    }
    async fn cast(&mut self, skill: u16, target: u32) -> Result<()> {
        self.send(Command::Stat(StatRequest::UseSkill { skill, level: 5, target }))?;
        self.pump(650).await
    }
    async fn buff(&mut self, skill: u16, efst: u16) -> Result<()> {
        self.cast(skill, self.state.account).await?;
        ensure!(self.state.buffs.contains(&efst), "skill {skill} did not apply EFST {efst}; failures {:?}", self.state.failed);
        println!("PASS skill {skill} applies status {efst}");
        Ok(())
    }
}

fn test_address(address: SocketAddr, port: u16) -> Result<()> {
    ensure!(address.ip().is_loopback() && address.port() == port,
        "refusing non-test endpoint {address}; expected loopback:{port}");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("Samurai integration: disposable stack only ({LOGIN})");
    let auth = ro_net::login(LOGIN, USER, PASSWORD).await?;
    let server = auth.servers.first().context("test char server missing")?;
    test_address(server.addr.into(), 55121)?;
    let chars = ro_net::enter_char(server, &auth).await?;
    let character = chars.characters.iter().find(|c| c.slot == 0).context("test character in slot 0 missing")?.clone();
    ensure!(character.job == 4500 && character.sex == 1, "expected male Samurai, got job={} sex={}", character.job, character.sex);
    println!("PASS login retains male Samurai job 4500 ({})", character.name);
    let zone = chars.select(0).await?;
    test_address(zone.addr.into(), 55122)?;
    let map = ro_net::enter_map(&zone, &auth).await?;
    let (tx, mut commands) = tokio::sync::mpsc::unbounded_channel();
    let (events, rx) = mpsc::channel();
    let name = character.name.clone();
    let task = tokio::spawn(async move { map.run(&name, &mut commands, &events).await });
    let mut test = Test { tx, rx, state: State { account: auth.account_id, ..Default::default() } };
    test.pump(1200).await?;

    for skill in 7001..=7008 {
        for _ in 0..5 {
            if test.state.skills.get(&skill).is_some_and(|s| s.level == 5) { break; }
            test.send(Command::Stat(StatRequest::SkillUp(skill)))?;
            test.pump(220).await?;
        }
        let s = test.state.skills.get(&skill).with_context(|| format!("skill {skill} absent from skill packets"))?;
        ensure!(s.level == 5, "skill {skill} learned only to {}", s.level);
        ensure!(s.name.starts_with("SAM_") && s.target != 0, "invalid skill packet {s:?}");
    }
    println!("PASS all eight Samurai skills learned to level 5 through packets");

    let sword = test.state.items.values().find(|i| i.id == 900000).context("starter katana missing")?.index;
    let offhand = test.state.items.values().find(|i| i.id == 1201).or_else(|| test.state.items.values().find(|i| i.id == 900000 && i.index != sword))
        .context("test offhand dagger/katana missing")?.index;
    for (index, location) in [(sword, 2), (offhand, 32)] {
        if !test.state.items.get(&index).is_some_and(|i| i.equipped & location != 0) {
            test.send(Command::Item(ItemRequest::Equip { index, location }))?;
            test.pump(450).await?;
        }
        ensure!(test.state.items.get(&index).is_some_and(|i| i.equipped & location != 0), "hand {location} not equipped");
    }
    println!("PASS katana and offhand equip together");
    test.chat("@samuraidummy").await?;
    let dummy = test.state.dummy.context("fixture must reply SAMURAI_DUMMY <id>")?;
    test.pump(3000).await?; // The newly spawned monster must become ready to act.
    test.chat("@samuraireset").await?;
    test.chat("@heal").await?;
    let unguarded = test.state.stat(5)?;
    test.chat("@samuraihit").await?;
    test.pump(700).await?;
    let incoming = unguarded - test.state.stat(5)?;
    ensure!(incoming > 0, "fixture physical skill must deal damage without a guard");
    println!("BASELINE incoming Bash damage={incoming}");
    test.chat("@heal").await?;

    test.buff(7002, 2000).await?;
    test.pump(12200).await?;
    ensure!(!test.state.buffs.contains(&2000), "Kusanagi Guard failed to expire");
    println!("PASS Kusanagi Guard expires after 12 seconds");

    test.buff(7003, 2001).await?;
    let hp = test.state.stat(5)?;
    let sp = test.state.stat(7)?;
    let reflection_hits = test.state.hits.len();
    test.chat("@samuraihit").await?;
    test.pump(700).await?;
    ensure!(!test.state.buffs.contains(&2001), "Moonlit Reflection did not consume on physical skill");
    ensure!(test.state.stat(5)? >= hp, "Moonlit Reflection failed to block incoming physical skill");
    ensure!(test.state.stat(7)? > sp, "Moonlit Reflection did not restore SP");
    ensure!(test.state.hits[reflection_hits..].iter().any(|(id, right, left)| *id == 7003 && right + left > 0), "Moonlit Reflection did not return positive damage");
    println!("PASS Moonlit Reflection blocks physical skill, returns damage, consumes status and restores SP");

    let hit_before = test.state.stat(49)?;
    let aspd_before = test.state.stat(53)?;
    test.buff(7004, 2002).await?;
    ensure!(test.state.stat(49)? == hit_before + 50, "Blade Harmony HIT increase mismatch");
    ensure!(test.state.stat(53)? < aspd_before, "Blade Harmony ASPD did not improve");
    println!("PASS Blade Harmony adds 50 HIT and increases attack speed");

    test.chat("@heal").await?;
    let hp_before = test.state.stat(5)?;
    let maximum = test.state.stat(6)?;
    test.state.hp_updates.clear();
    test.buff(7005, 2003).await?;
    let paid = hp_before - test.state.hp_updates.iter().find(|&&hp| hp < hp_before).copied().context("Covenant HP payment missing")?;
    ensure!((paid - maximum / 10).abs() <= 2, "Blood Covenant HP cost {paid}, expected {}", maximum / 10);
    for n in 1..=3 {
        let before = test.state.attacks.len();
        test.send(Command::Attack { target: dummy, keep: false })?;
        test.pump(1000).await?;
        ensure!(test.state.attacks.len() > before, "normal attack {n} not observed");
        ensure!(test.state.buffs.contains(&2003) == (n < 3), "Blood Covenant charge count wrong after attack {n}");
    }
    println!("PASS Blood Covenant costs 10% max HP and consumes exactly three dual-wield attacks");

    test.chat("@heal").await?;
    let hp_before = test.state.stat(5)?;
    test.state.hp_updates.clear();
    test.buff(7006, 2004).await?;
    test.pump(700).await?;
    let hp_samples: Vec<_> = std::iter::once(hp_before).chain(test.state.hp_updates.iter().copied()).collect();
    ensure!(hp_samples.windows(2).any(|pair| pair[0] - pair[1] == maximum / 100),
        "Muramasa 1% drain missing: samples {hp_samples:?}, expected {}", maximum / 100);
    test.pump(19800).await?;
    ensure!(!test.state.buffs.contains(&2004), "Muramasa's Curse failed to expire");
    ensure!(!test.state.buffs.contains(&2002), "Blade Harmony failed to expire");
    ensure!(test.state.stat(49)? == hit_before && test.state.stat(53)? == aspd_before, "Blade Harmony stats did not return to baseline");
    println!("PASS Muramasa drains 1% max HP per second and expires; Harmony stats restore");

    test.chat("@heal").await?;
    for (skill, target) in [(7001, dummy), (7007, auth.account_id)] {
        let before = test.state.hits.len();
        test.cast(skill, target).await?;
        ensure!(test.state.hits[before..].iter().any(|(id, right, left)| *id == skill && right + left > 0), "skill {skill} did not damage dummy");
    }
    println!("PASS Heaven's Severance and Ashen Wind Slash deal damage");

    test.chat("@samurai30hp").await?;
    let before = test.state.failed.len();
    test.cast(7008, dummy).await?;
    ensure!(test.state.failed[before..].iter().any(|(id, _)| *id == 7008), "Wrath should fail at 30% HP");
    test.chat("@samurailowhp").await?;
    let before = test.state.hits.len();
    test.cast(7008, dummy).await?;
    ensure!(test.state.hits[before..].iter().any(|(id, right, left)| *id == 7008 && right + left > 0), "Wrath did not damage dummy below 30% HP");
    println!("PASS Wrath Unbound rejects 30% HP and deals damage below threshold");
    println!("SAMURAI_INTEGRATION_PASS");
    drop(test);
    task.await??;
    Ok(())
}

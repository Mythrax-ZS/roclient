//! roclient
//!
//!   ro-client --headless   run login -> char select -> map entry and exit
//!   ro-client              open the renderer window

mod audio;
mod sense;
mod pettalk;
mod fx;
mod gamepad;
mod guild;
mod hud;
mod iteminfo;
mod jobs;
mod mail;
mod msgstr;
mod merchant;
mod preview;
mod progress;
mod quests;
mod refine;
mod screens;
mod ai;
mod booking;
mod battle;
mod clan;
mod merge;
mod reputation;
mod gameopts;
mod ranking;
mod storesearch;
mod stylist;
mod lapine;
mod reform;
mod enchant;
mod market;
mod customdata;
mod navigation;
mod grade;
mod buffs;
mod chatrooms;
mod classic;
mod companions;
mod emotes;
mod settings;
mod skilltree;
mod status_icons;
mod stores;
mod splash;
mod social;
mod windows;

use std::{path::PathBuf, sync::Arc};

use anyhow::Context;
use serde::Deserialize;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowId},
};

#[derive(Deserialize)]
struct Config {
    #[serde(default = "default_login_addr")]
    login_addr: String,
    username: String,
    password: String,
    #[serde(default)]
    char_slot: u8,
    #[serde(default = "default_backend")]
    backend: String,
    /// --headless: how long to stay on the map after entering.
    #[serde(default = "default_soak")]
    soak_secs: u64,
    /// UI size multiplier on top of the display's DPI scale.
    #[serde(default = "default_ui_scale")]
    ui_scale: f32,
    /// Skip the login screen and log in with username / password / char_slot.
    #[serde(default)]
    auto_login: bool,
    /// MSAA sample count (1, 2, 4); falls back to what the GPU supports.
    #[serde(default = "default_msaa")]
    msaa: u32,
    /// Music and sound effect volume, 0..1 (0 mutes).
    #[serde(default = "default_bgm_volume")]
    bgm_volume: f32,
    #[serde(default = "default_sfx_volume")]
    sfx_volume: f32,
    /// Installed RO client folder; its `data/` is checked before the GRFs.
    client_dir: Option<PathBuf>,
    /// GRFs inside `client_dir`, highest priority first.
    #[serde(default = "default_grfs")]
    grfs: Vec<String>,
    /// Folders laid over the client data, first wins: an unpacked translation pack such as
    /// ROenglishRE's Translation/Renewal (its SystemEN stands in for System).
    #[serde(default)]
    overlays: Vec<PathBuf>,
    /// Map shown in the window until it follows the network session.
    #[serde(default = "default_map")]
    map: String,
}

fn default_grfs() -> Vec<String> {
    vec!["data.grf".into()]
}

fn default_map() -> String {
    "prontera".into()
}

fn open_vfs(cfg: &Config) -> anyhow::Result<Option<ro_vfs::Vfs>> {
    let Some(dir) = &cfg.client_dir else { return Ok(None) };
    let overlays: Vec<PathBuf> = cfg.overlays.iter().filter(|p| p.is_dir() || { tracing::warn!(path = %p.display(), "overlay folder not found, skipping"); false }).cloned().collect();
    let mut vfs = ro_vfs::Vfs { overlays, loose: Some(dir.clone()), grfs: vec![] };
    for name in &cfg.grfs {
        let path = dir.join(name);
        if path.is_file() {
            vfs.grfs.push(ro_vfs::Grf::open(&path)?);
        } else {
            tracing::warn!(path = %path.display(), "GRF not found, skipping");
        }
    }
    Ok(Some(vfs))
}

/// Character appearance, as the char server describes it.
#[derive(Clone)]
struct Look {
    /// Job sprite name (Korean, as in the GRF), e.g. 초보자 for Novice.
    job: String,
    male: bool,
    hair: u16,
    hair_color: u16,
    gear: ro_net::Look,
}

/// Loads body, head (with hair dye), worn headgear and weapon, and shadow sheets.
fn load_character(
    vfs: &ro_vfs::Vfs,
    r: &mut ro_render::Renderer,
    look: &Look,
    tables: &ro_game::looks::LookTables,
) -> anyhow::Result<ro_render::Actor> {
    let sex = if look.male { "남" } else { "여" };
    let mut sheet = |base: &str, palette: Option<String>| -> anyhow::Result<ro_render::SheetId> {
        let read = |p: String| vfs.read(&p)?.with_context(|| format!("{p} not found"));
        let spr = ro_formats::spr::Spr::parse(&read(format!("{base}.spr"))?)?;
        let act = ro_formats::act::Act::parse(&read(format!("{base}.act"))?)?;
        // A missing dye palette falls back to the sprite's own colors.
        let pal = palette.and_then(|p| vfs.read(&p).ok().flatten()).and_then(|d| ro_formats::spr::parse_palette(&d).ok());
        Ok(r.add_sheet(&spr, act, pal.as_ref()))
    };
    // Doram jobs have their own sprite and hair palette folders.
    let race = jobs::race_dir(&look.job);
    let pal_dir = if race == "도람족" { "palette\\도람족\\머리" } else { "palette\\머리" };
    let body = sheet(&format!("data\\sprite\\{race}\\몸통\\{sex}\\{}_{sex}", look.job), None)?;
    let head = sheet(
        &format!("data\\sprite\\{race}\\머리통\\{sex}\\{}_{sex}", look.hair),
        (look.hair_color > 0).then(|| format!("data\\{pal_dir}\\머리{}_{sex}_{}.pal", look.hair, look.hair_color)),
    )?;
    let shadow = sheet("data\\sprite\\shadow", None).inspect_err(|e| tracing::warn!("shadow sprite: {e:#}")).ok();
    // Equipment is optional: a missing sprite just isn't drawn.
    let g = look.gear;
    let mut headgear = vec![];
    for view in [g.head_bottom, g.head_mid, g.head_top] {
        if view == 0 {
            continue;
        }
        // A headgear on several slots (e.g. top+mid) is drawn once.
        if let Some(name) = tables.accessory(view as u32) {
            match sheet(&format!("data\\sprite\\악세사리\\{sex}\\{sex}{name}"), None) {
                Ok(id) if !headgear.contains(&id) => headgear.push(id),
                Ok(_) => {}
                Err(e) => tracing::debug!(view, "headgear sprite: {e:#}"),
            }
        }
    }
    // Weapon, then its slash trail (_검광). Two-handed katars are named twice
    // (어세신_남_카타르_카타르), as both hands hold one.
    let mut weapon = vec![];
    if let Some(name) = (g.weapon != 0).then(|| tables.weapon(g.weapon)).flatten() {
        let wj = jobs::weapon_dir(&look.job);
        let dir = format!("data\\sprite\\{race}\\{wj}\\{wj}_{sex}");
        let found = [format!("{dir}{name}"), format!("{dir}{name}{name}")].into_iter().find_map(|base| sheet(&base, None).ok().map(|id| (id, base)));
        tracing::debug!(%dir, %name, found = found.is_some(), "weapon sprite");
        match found {
            Some((id, base)) => {
                weapon.push(id);
                if let Ok(trail) = sheet(&format!("{base}_검광"), None) {
                    weapon.push(trail);
                }
            }
            None => tracing::debug!(weapon = g.weapon, %name, "no weapon sprite"),
        }
    }
    Ok(ro_render::Actor {
        pos: Default::default(),
        body,
        head: Some(head),
        headgear,
        weapon,
        shadow,
        action: 0,
        dir: ro_render::Dir::S,
        time_ms: 0.0,
        once: false,
        alpha: 1.0,
    })
}

struct LoadedMap {
    mesh: ro_render::SceneMesh,
    images: Vec<Option<ro_render::Rgba>>,
    lighting: ro_render::Lighting,
    /// Light sources (torches, lamps) in world space.
    lights: Vec<ro_render::PointLight>,
    water: Option<(ro_render::WaterMesh, Vec<Option<ro_render::Rgba>>)>,
    /// Ambient effect emitters (EF_* id, world position): torches, bubbles, fireflies.
    effects: Vec<(i32, glam::Vec3, f32)>,
}

fn load_map(vfs: &ro_vfs::Vfs, name: &str) -> anyhow::Result<LoadedMap> {
    let started = std::time::Instant::now();
    let read = |path: String| vfs.read(&path)?.with_context(|| format!("{path} not found"));
    let rsw = ro_formats::rsw::Rsw::parse(&read(format!("data\\{name}.rsw"))?)?;
    let gnd_name = if rsw.gnd.is_empty() { format!("{name}.gnd") } else { rsw.gnd.clone() };
    let gnd = ro_formats::gnd::Gnd::parse(&read(format!("data\\{gnd_name}"))?)?;

    // Each model file is parsed once and shared by all its placements.
    let mut models: std::collections::HashMap<String, Option<ro_formats::rsm::Rsm>> = Default::default();
    let mut builder = ro_render::SceneBuilder::new(&gnd);
    let mut failed = 0;
    for inst in &rsw.models {
        let rsm = models.entry(inst.file.to_ascii_lowercase()).or_insert_with(|| {
            let path = format!("data\\model\\{}", inst.file);
            match vfs.read(&path).map(|d| d.map(|d| ro_formats::rsm::Rsm::parse(&d))) {
                Ok(Some(Ok(m))) => Some(m),
                Ok(Some(Err(e))) => {
                    tracing::warn!(%path, "model unreadable: {e:#}");
                    None
                }
                Ok(None) => {
                    tracing::warn!(%path, "model missing");
                    None
                }
                Err(e) => {
                    tracing::warn!(%path, "model unreadable: {e:#}");
                    None
                }
            }
        });
        match rsm {
            Some(m) => builder.add_model(m, inst),
            None => failed += 1,
        }
    }
    let mesh = builder.finish();

    let load_textures = |names: &[String]| -> Vec<Option<ro_render::Rgba>> {
        names
            .iter()
            .map(|t| {
                let path = format!("data\\texture\\{t}");
                match vfs.read(&path).and_then(|d| d.map(|d| ro_formats::image::decode(&d, &path)).transpose()) {
                    Ok(Some(img)) => Some(img),
                    Ok(None) => {
                        tracing::warn!(%path, "texture missing");
                        None
                    }
                    Err(e) => {
                        tracing::warn!(%path, "texture unreadable: {e:#}");
                        None
                    }
                }
            })
            .collect()
    };
    let images = load_textures(&mesh.textures);
    let water = rsw.water.as_ref().map(|w| {
        let mesh = ro_render::WaterMesh::build(&gnd, w);
        let images = load_textures(&mesh.textures);
        (mesh, images)
    });
    tracing::info!(
        map = name,
        size = format_args!("{}x{}", gnd.width, gnd.height),
        models = rsw.models.len() - failed,
        failed,
        textures = mesh.textures.len(),
        triangles = mesh.vertices.len() / 3,
        ms = started.elapsed().as_millis() as u64,
        "loaded map"
    );
    // RSW light positions are centred on the map with y pointing down, like model placements.
    let lights = rsw
        .lights
        .iter()
        .map(|l| ro_render::PointLight {
            pos: glam::Vec3::new(l.pos[0] + gnd.width as f32 * gnd.zoom / 2.0, -l.pos[1], l.pos[2] + gnd.height as f32 * gnd.zoom / 2.0),
            color: l.color,
            range: l.range,
        })
        .collect();
    let effects = rsw.effects.iter().map(|e| (e.id, glam::Vec3::new(e.pos[0] + gnd.width as f32 * gnd.zoom / 2.0, -e.pos[1], e.pos[2] + gnd.height as f32 * gnd.zoom / 2.0), e.delay)).collect();
    Ok(LoadedMap { mesh, images, water, lighting: ro_render::Lighting::from_rsw(&rsw.light), lights, effects })
}

fn default_ui_scale() -> f32 {
    1.0
}

fn default_msaa() -> u32 {
    4
}

fn default_bgm_volume() -> f32 {
    0.5
}

fn default_sfx_volume() -> f32 {
    0.7
}

fn default_soak() -> u64 {
    30
}

fn default_backend() -> String {
    "vulkan".into()
}

fn default_login_addr() -> String {
    "127.0.0.1:6900".into()
}

/// Accepts `host`, `host:port`, `[v6]` or `[v6]:port`; appends the default login port when absent.
fn normalize_login_addr(s: &str) -> String {
    let s = s.trim();
    let has_port = match s.strip_prefix('[') {
        Some(rest) => rest.split_once(']').is_some_and(|(_, tail)| tail.starts_with(':')),
        None => s.matches(':').count() == 1,
    };
    if has_port {
        s.into()
    } else if s.contains(':') && !s.starts_with('[') {
        format!("[{s}]:6900") // bare IPv6
    } else {
        format!("{s}:6900")
    }
}

fn load_config() -> anyhow::Result<Config> {
    let path = std::env::var("ROCLIENT_CONFIG").unwrap_or_else(|_| "config/local.toml".into());
    let text = std::fs::read_to_string(&path).with_context(|| format!("read {path} (copy config/example.toml)"))?;
    Ok(toml::from_str(&text)?)
}

async fn headless(cfg: &Config) -> anyhow::Result<()> {
    let auth = ro_net::login(&cfg.login_addr, &cfg.username, &cfg.password).await?;
    tracing::info!(account_id = auth.account_id, servers = auth.servers.len(), "logged in");
    let server = auth.servers.first().context("no char servers offered")?;
    tracing::info!(name = %server.name, addr = %server.addr, users = server.users, "char server");

    let chars = ro_net::enter_char(server, &auth).await?;
    for c in &chars.characters {
        tracing::info!(slot = c.slot, name = %c.name, job = c.job, lv = c.base_level, map = %c.map, "character");
    }
    let zone = chars.select(cfg.char_slot).await?;
    tracing::info!(map = %zone.map, addr = %zone.addr, "zone server");

    let mut map = ro_net::enter_map(&zone, &auth).await?;
    tracing::info!(x = map.spawn.x, y = map.spawn.y, dir = map.spawn.dir, "entered map");

    let seen = map.soak(std::time::Duration::from_secs(cfg.soak_secs)).await?;
    let total: usize = seen.values().sum();
    tracing::info!(packets = total, kinds = seen.len(), secs = cfg.soak_secs, "stayed connected");
    for (op, n) in seen {
        tracing::info!("  {op:#06x} x{n}");
    }
    Ok(())
}

/// Sprite name (as in the GRF) for a job id. Covers Novice through the 2-1/2-2 classes.
/// Display name for a job id (Novice through the 2-1/2-2 classes).
fn job_title(job: i16) -> &'static str {
    jobs::title(job)
}

fn job_sprite(job: i16) -> &'static str {
    jobs::sprite(job)
}

const DIRS: [ro_render::Dir; 8] = {
    use ro_render::Dir::*;
    [S, SW, W, NW, N, NE, E, SE]
};

/// The map currently shown: its geometry (for picking/heights) and walkability.
struct World {
    name: String,
    ground: ro_render::SceneMesh,
    gat: Option<ro_formats::gat::Gat>,
    /// Indoor maps: the cells of the room we are in, plus its walls (one byte per cell).
    room: Option<Vec<u8>>,
}

/// Cells around a room's floor still drawn, so its walls and furniture show.
const ROOM_MARGIN: i32 = 5;

impl World {
    /// Interior maps pack many rooms (linked by warps) into one map.
    fn indoor(&self) -> bool {
        self.name.contains("_in")
    }

    /// Whether a cell is drawn (always, outdoors).
    fn shows_cell(&self, x: i32, y: i32) -> bool {
        match (&self.room, &self.gat) {
            (Some(room), Some(g)) => x >= 0 && y >= 0 && (x as u32) < g.width && (y as u32) < g.height && room[(y as u32 * g.width + x as u32) as usize] != 0,
            _ => true,
        }
    }

    /// The room containing `start`: walkable cells reachable from it, grown by ROOM_MARGIN.
    fn room_of(&self, start: (i32, i32)) -> Option<Vec<u8>> {
        let g = self.gat.as_ref()?;
        let (w, h) = (g.width as i32, g.height as i32);
        if !g.walkable(start.0, start.1) {
            return None;
        }
        let mut floor = vec![false; (w * h) as usize];
        let mut stack = vec![start];
        floor[(start.1 * w + start.0) as usize] = true;
        while let Some((x, y)) = stack.pop() {
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (nx, ny) = (x + dx, y + dy);
                if nx >= 0 && ny >= 0 && nx < w && ny < h && !floor[(ny * w + nx) as usize] && g.walkable(nx, ny) {
                    floor[(ny * w + nx) as usize] = true;
                    stack.push((nx, ny));
                }
            }
        }
        // Grow by the margin (separable: rows, then columns).
        let mut rows = vec![false; floor.len()];
        for y in 0..h {
            for x in 0..w {
                rows[(y * w + x) as usize] = (x - ROOM_MARGIN..=x + ROOM_MARGIN).any(|sx| sx >= 0 && sx < w && floor[(y * w + sx) as usize]);
            }
        }
        let mut out = vec![0u8; floor.len()];
        for y in 0..h {
            for x in 0..w {
                if (y - ROOM_MARGIN..=y + ROOM_MARGIN).any(|sy| sy >= 0 && sy < h && rows[(sy * w + x) as usize]) {
                    out[(y * w + x) as usize] = 255;
                }
            }
        }
        Some(out)
    }
}

/// Fractional cell (centre plus the server's twelfth-of-a-cell offset) of a floor item.
fn floor_cell(f: &ro_net::FloorItem) -> (f32, f32) {
    (f.x as f32 + f.sub.0 as f32 / 12.0, f.y as f32 + f.sub.1 as f32 / 12.0)
}

fn lapine_command(a: lapine::LapineAction) -> ro_net::Command {
    ro_net::Command::Lapine(match a {
        lapine::LapineAction::Synthesize { trigger, items } => ro_net::LapineRequest::Synthesize { trigger, items },
        lapine::LapineAction::Upgrade { trigger, index } => ro_net::LapineRequest::Upgrade { trigger, index },
        lapine::LapineAction::Close { upgrade: false } => ro_net::LapineRequest::SynthesisClose,
        lapine::LapineAction::Close { upgrade: true } => ro_net::LapineRequest::UpgradeClose,
    })
}

fn inv_item(d: ro_net::ItemData) -> ro_game::items::InvItem {
    ro_game::items::InvItem {
        index: d.index,
        id: d.id,
        kind: d.kind,
        amount: d.amount,
        location: d.location,
        equipped: d.equipped,
        refine: d.refine,
        grade: d.grade,
        switch: 0,
        identified: d.identified,
        cards: d.cards,
        look: d.look,
        favorite: d.favorite,
    }
}

/// Decodes an image from the VFS into an egui texture.
fn load_texture(ctx: &egui::Context, vfs: &ro_vfs::Vfs, path: &str) -> Option<egui::TextureHandle> {
    let data = vfs.read(path).ok().flatten()?;
    let img = ro_formats::image::decode(&data, path).ok()?;
    let color = egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.pixels);
    Some(ctx.load_texture(path, color, egui::TextureOptions::NEAREST))
}

/// Expanded-cell budget when planning a long (multi-hop) walk.
const LONG_WALK_NODES: usize = 200_000;
/// Steps per server walk request. rAthena refuses walks longer than `max_walk_path`
/// (conf/battle/client.conf, 17 by default); stay well under it, since the server's
/// position can trail the client's by a cell or two when the next hop is sent.
const WALK_HOP: usize = 12;

/// Controller: how far (cells) targets are picked up, and how far ahead the stick walks.
const PAD_REACH: i32 = 14;
const PAD_STEP: i32 = 4;

use windows::{HOTKEYS_PER_ROW, HOTKEY_ROWS};

/// F1..F9 -> slot within the visible shortcut row.
fn hotkey_slot(k: winit::keyboard::NamedKey) -> Option<usize> {
    use winit::keyboard::NamedKey::*;
    [F1, F2, F3, F4, F5, F6, F7, F8, F9].iter().position(|&f| f == k)
}

/// A clicked destination being reached through one or more server walks.
struct WalkGoal {
    cell: (u16, u16),
    sent_at: Option<std::time::Instant>,
    /// Hops sent without the previous one moving us; gives up after a few.
    tries: u32,
}

/// Anything that stands and walks on the map: the player, other players, NPCs, monsters.
struct Mob {
    actor: ro_render::Actor,
    walker: ro_game::Walker,
    /// Name shown under the actor (empty: none).
    name: String,
    /// Unit object type (0 player, 1/6 NPC, 5 monster, ...).
    kind: u8,
    /// Uses the player body/head sprites (whose action groups differ from monsters').
    human: bool,
    /// A timed pose (attack, hurt) overriding idle: (pose, remaining ms).
    pose: Option<(Pose, f32)>,
    sitting: bool,
    /// Playing the death animation; removed when it runs out (None for the player).
    dying: Option<f32>,
    hp: Option<(i32, i32)>,
    /// Player-sprite appearance (None for NPCs and monsters).
    look: Option<Look>,
    /// Players: (party, guild, guild rank) from ZC_ACK_REQNAMEALL, shown under the name.
    titles: Option<(String, String, String)>,
    /// Unit class (job / monster id).
    class: i16,
    /// Guild id (0: none), for the emblem beside the name.
    guild: u32,
    /// Drawn position (cells), eased toward the walker's.
    smooth: Option<(f32, f32)>,
    /// Player attack motion: 10, or 11 for weapons drawn only there (katars).
    attack_group: usize,
    /// Length of the current timed pose, and how fast its animation must play to fit.
    pose_len: f32,
    anim_rate: f32,
}

#[derive(Clone, Copy, PartialEq)]
enum Pose {
    Attack,
    Hurt,
    PickUp,
    Dead,
    /// Chanting a skill (players only; monsters stand still).
    Cast,
}

impl Mob {
    fn new(actor: ro_render::Actor, cell: (u16, u16), dir: usize, speed: i16) -> Self {
        let speed = if speed > 0 { speed as f32 } else { 150.0 };
        Self {
            actor,
            walker: ro_game::Walker::new((cell.0 as i32, cell.1 as i32), dir, speed),
            name: String::new(),
            kind: 0,
            human: true,
            pose: None,
            sitting: false,
            dying: None,
            hp: None,
            look: None,
            titles: None,
            class: 0,
            guild: 0,
            smooth: None,
            attack_group: 10,
            pose_len: 0.0,
            anim_rate: 1.0,
        }
    }

    /// Action group for a pose: player sprites and monster sprites order them differently.
    fn action_for(&self, pose: Pose) -> usize {
        match (self.human, pose) {
            (true, Pose::Attack) => self.attack_group,
            (true, Pose::Hurt) => 6,
            (true, Pose::Dead) => 8,
            (true, Pose::Cast) => 12,
            (false, Pose::Cast) => 0,
            (false, Pose::Attack) => 2,
            (false, Pose::Hurt) => 3,
            (false, Pose::Dead) => 4,
            (_, Pose::PickUp) => 3,
        }
    }

    /// Speeds a timed pose's animation up so all its frames show before the pose ends, as
    /// the original client does (attack motions follow attack speed; katar blades appear
    /// only in the later frames). Death keeps its own pace.
    fn fit_pose(&mut self, r: &ro_render::Renderer) {
        let Some((pose, _)) = self.pose else { return };
        if pose == Pose::Dead || self.pose_len <= 0.0 {
            return;
        }
        let index = self.action_for(pose) * 8 + self.actor.dir as usize;
        let full = r.action_ms(self.actor.body, index);
        self.anim_rate = (full / self.pose_len).clamp(1.0, 8.0);
    }

    fn set_pose(&mut self, pose: Pose, ms: f32) {
        self.pose = Some((pose, ms.max(200.0)));
        self.pose_len = ms.max(200.0);
        self.anim_rate = 1.0;
        self.actor.time_ms = 0.0;
    }

    /// Turns to face a cell.
    fn face(&mut self, cell: (i32, i32)) {
        let c = self.walker.cell;
        if c != cell {
            self.walker.dir = ro_game::dir_from_step(cell.0 - c.0, cell.1 - c.1);
        }
    }

    /// Name label colour, or None when the original client would not show one
    /// (monster names appear on hover only).
    fn label_color(&self) -> Option<egui::Color32> {
        if self.name.is_empty() {
            return None;
        }
        match self.kind {
            0 => Some(egui::Color32::WHITE),
            1 | 6 => Some(egui::Color32::from_rgb(160, 225, 255)),
            _ => None,
        }
    }

    /// Starts walking from -> to along the client-side path (as the server will).
    fn walk(&mut self, gat: Option<&ro_formats::gat::Gat>, from: (u16, u16), to: (u16, u16)) {
        let from = (from.0 as i32, from.1 as i32);
        let to = (to.0 as i32, to.1 as i32);
        match gat.and_then(|g| ro_game::path::find(g, from, to)) {
            // Continue smoothly when the new walk starts where we are (re-aimed walks).
            Some(path) => {
                if !self.walker.redirect(from, path.clone()) {
                    self.walker.stop_at(from);
                    self.walker.walk(path);
                }
            }
            None => self.walker.stop_at(to),
        }
    }

    /// Advances movement and returns the actor to draw. `idle_action` is the pose when
    /// standing; facing is drawn relative to the camera's rotation (`octant`).
    fn update(&mut self, dt: f32, world: &World, octant: i32, idle_action: usize) -> ro_render::Actor {
        let was_walking = self.walker.is_walking();
        let (wx, wy) = self.walker.update(dt);
        // Ease the drawn position toward the logical one (exponential smoothing, ~50 ms), so
        // corrections and re-aimed walks glide; big jumps (warps, knockback) snap.
        let (fx, fy) = match self.smooth {
            Some((sx, sy)) if (sx - wx).abs() + (sy - wy).abs() < 3.0 => {
                let k = 1.0 - (-dt / 50.0).exp();
                (sx + (wx - sx) * k, sy + (wy - sy) * k)
            }
            _ => (wx, wy),
        };
        self.smooth = Some((fx, fy));
        let walking = self.walker.is_walking();
        if walking != was_walking {
            self.actor.time_ms = 0.0; // restart the animation on start/stop
        }
        let (x, z) = ((fx + 0.5) * ro_render::CELL, (fy + 0.5) * ro_render::CELL);
        // Stand on the GAT surface: it includes stairs, bridges and other walkable models
        // that sit above the GND ground.
        let y = world
            .gat
            .as_ref()
            .and_then(|g| g.height_at(fx + 0.5, fy + 0.5))
            .map_or_else(|| world.ground.height_at(x, z), |h| -h);
        self.actor.pos = glam::Vec3::new(x, y, z);
        if let Some((_, left)) = self.pose.as_mut() {
            *left -= dt;
            if *left <= 0.0 && self.pose.is_some_and(|(p, _)| p != Pose::Dead) {
                self.pose = None;
                self.actor.time_ms = 0.0;
            }
        }
        if let Some(left) = self.dying.as_mut() {
            *left -= dt;
        }
        let (action, once) = match self.pose {
            Some((Pose::Dead, _)) => (self.action_for(Pose::Dead), true),
            Some((p, _)) if !walking => (self.action_for(p), true),
            _ if walking => (1, false),
            _ if self.sitting => (2, false),
            _ => (idle_action, false),
        };
        self.actor.action = action;
        self.actor.once = once;
        self.actor.dir = DIRS[(self.walker.dir as i32 + octant).rem_euclid(8) as usize];
        // Timed poses play fast enough to finish in the server's time (attack speed).
        self.actor.time_ms += if self.pose.is_some() { dt * self.anim_rate } else { dt };
        self.actor.clone()
    }
}

/// Class id -> sprite name, from the client's npcidentity + jobname tables.
fn load_class_names(vfs: &ro_vfs::Vfs) -> std::collections::HashMap<i16, String> {
    use ro_formats::lub::assignments;
    let dir = "data\\luafiles514\\lua files\\datainfo";
    let read = |name: &str| -> anyhow::Result<Vec<(ro_formats::lub::Value, ro_formats::lub::Value)>> {
        let path = format!("{dir}\\{name}.lub");
        assignments(&vfs.read(&path)?.with_context(|| format!("{path} not found"))?)
    };
    let (ids, names) = match (read("npcidentity"), read("jobname")) {
        (Ok(i), Ok(n)) => (i, n),
        (Err(e), _) | (_, Err(e)) => {
            tracing::warn!("class tables: {e:#}");
            return Default::default();
        }
    };
    let ids: std::collections::HashMap<String, i16> =
        ids.iter().filter_map(|(k, v)| Some((k.as_str()?.to_string(), v.as_number()? as i16))).collect();
    let map: std::collections::HashMap<i16, String> = names
        .iter()
        .filter_map(|(k, v)| Some((*ids.get(k.as_str()?)?, v.as_str()?.to_string())))
        .collect();
    tracing::info!(classes = map.len(), "loaded class sprite names");
    map
}

/// Player jobs use the human body/head sprites; everything else has one sprite per class.
fn is_player_class(job: i16) -> bool {
    job < 45 || (4001..4400).contains(&job)
}

struct App {
    backend: String,
    msaa: u32,
    /// How to (re)start the network session, and the login / character screens while they show.
    session: ro_net::SessionConfig,
    front: Option<screens::Front>,
    /// Exit requested from the login screen.
    quit: bool,
    /// The Esc game menu (Character Select, Options, Exit).
    esc_menu: bool,
    /// Start-up splash, title art and loading screens (made once the window exists).
    splash: Option<splash::Splash>,
    vfs: Option<ro_vfs::Vfs>,
    /// Offline preview map and cell (ignored once the server places us).
    map: String,
    at: (u16, u16),
    /// `--still`: ignore camera input (for reproducible captures).
    still: bool,
    /// `--capture file.png [--size WxH] [--capture-after ms]`: render offscreen, save, exit.
    /// Online, the delay counts from map entry.
    capture: Option<(PathBuf, u32, u32)>,
    capture_after: f32,
    /// `--walk x,y`: request a walk right after entering (scripted test).
    walk_on_enter: Option<(u16, u16)>,
    /// `--say text`: send a chat line right after entering (scripted test).
    say_on_enter: Option<String>,
    /// `--distance`: initial camera distance.
    distance: Option<f32>,
    /// `--yaw degrees`: initial camera rotation (0 looks north).
    yaw: Option<f32>,
    /// Offline preview appearance and pose (`--job`, `--sex`, `--action n`, `--dir 0..7`).
    look: Look,
    action: usize,
    dir: usize,
    net: Option<(tokio::sync::mpsc::UnboundedSender<ro_net::Command>, std::sync::mpsc::Receiver<ro_net::Event>)>,
    world: Option<World>,
    player: Option<Mob>,
    account_id: u32,
    char_id: u32,
    guild: guild::Guild,
    /// Connection problem shown in the title bar.
    status: Option<String>,
    walk_goal: Option<WalkGoal>,
    /// The NPC conversation in progress, and its illustration.
    dialog: Option<hud::Dialog>,
    cutin: Option<hud::Cutin>,
    minimap: Option<hud::Minimap>,
    settings: settings::Settings,
    audio: Option<audio::Audio>,
    /// NPC to talk to once we have walked into range.
    pending_npc: Option<u32>,
    /// A skill waiting until we have walked into range: (skill, level, target, tries).
    pending_skill: Option<(u16, u16, u32, u8)>,
    /// The monster we keep attacking, and our reach (cells) once the server tells us.
    attack: Option<(u32, i16)>,
    /// Walking toward the attack target after the server said it was out of reach.
    chasing: bool,
    /// Floating damage numbers: (world position, text, colour, age ms).
    floaters: Vec<(glam::Vec3, String, egui::Color32, f32)>,
    item_db: Option<ro_game::items::ItemDb>,
    skill_names: ro_game::skills::SkillNames,
    skill_tree: skilltree::SkillTree,
    inventory: ro_game::items::Inventory,
    /// Items on the ground, with their sprite.
    floor: std::collections::HashMap<u32, (ro_net::FloorItem, Option<ro_render::Actor>)>,
    stats: Option<ro_net::Stats>,
    skills: Vec<ro_net::Skill>,
    windows: windows::Windows,
    icons: std::collections::HashMap<u32, egui::TextureHandle>,
    icons_tried: std::collections::HashSet<u32>,
    skill_icons: std::collections::HashMap<u16, egui::TextureHandle>,
    /// Floor item to pick up once we have walked next to it.
    pending_pickup: Option<u32>,
    /// A targeted skill waiting for the player to click its target.
    aiming: Option<(u16, u16)>,
    /// Shortcut bar slots (tab 0, HOTKEY_ROWS rows) and the row on show.
    hotkeys: Vec<ro_net::Hotkey>,
    hotkey_row: u8,
    /// Casts in progress: caster -> (elapsed ms, total ms).
    casts: std::collections::HashMap<u32, (f32, f32)>,
    fx: fx::FxLib,
    /// The current map's ambient effect emitters (RSW torches, bubbles, fireflies).
    map_fx: Vec<fx::MapEmitter>,
    /// Sprite effects playing once: (actor, ms left).
    sprite_fx: Vec<(ro_render::Actor, f32)>,
    /// When each unit last showed an area burst (one per cast, not per monster hit).
    burst_at: std::collections::HashMap<u32, std::time::Instant>,
    /// Emotes showing over units: id -> (ET_* id, ms shown).
    emotes: std::collections::HashMap<u32, (u8, f32)>,
    /// Who last whispered us, for /r.
    last_whisper_from: Option<String>,
    /// Chat lines typed this frame, sent at the start of the next.
    queued_chat: Vec<String>,
    /// Our buffs and debuffs (the icon column).
    buffs: buffs::Buffs,
    /// The quest log and NPC quest marks.
    quests: quests::QuestLog,
    /// Cart, vending and shops.
    merchant: merchant::Merchant,
    /// Buying stores and the cash shop.
    stores: stores::Stores,
    /// Achievements, titles and attendance.
    progress: progress::Progress,
    /// Pet, homunculus and mercenary.
    companions: companions::Companions,
    /// RODEX mail.
    mailbox: mail::Mailbox,
    clan: clan::Clan,
    merge: merge::Merge,
    reputation: reputation::Reputation,
    game_opts: gameopts::GameOptions,
    ranking: ranking::Ranking,
    store_search: storesearch::StoreSearch,
    stylist: stylist::Stylist,
    reform: reform::Reform,
    lapine: lapine::Lapine,
    enchant: enchant::Enchant,
    market: market::Market,
    /// Test hook: a market purchase to make once a shop opens.
    market_buy: Option<(usize, u32)>,
    battle: battle::Battle,
    navi: navigation::Navigation,
    /// A walk the navigation window asked for (done outside the UI pass).
    navi_walk: Option<(u16, u16)>,
    grade: grade::Grade,
    /// Chat rooms.
    rooms: chatrooms::ChatRooms,
    /// Party booking.
    booking: booking::Booking,
    /// The last /ex or /in sent: (name, block).
    ignore_pending: Option<(String, bool)>,
    /// A companion window request, handled after the UI pass.
    companion_action: Option<companions::CompanionAction>,
    /// When the companion AI next checks distances.
    follow_at: std::time::Instant,
    /// Homunculus / mercenary classes by unit id, for their AI.
    unit_jobs: std::collections::HashMap<u32, i16>,
    /// Who each unit last attacked (monster aggro, for the AI scripts).
    aggro: std::collections::HashMap<u32, u32>,
    /// The companion AI scripts (AI\AI.lua, AI\AI_M.lua), loaded when first needed.
    homun_ai: Option<ai::Brain>,
    merc_ai: Option<ai::Brain>,
    client_dir: Option<std::path::PathBuf>,
    /// The Classic interface art, loaded once the GRF is open.
    skin: Option<classic::Skin>,
    /// The item information / card illustration window.
    item_info: iteminfo::ItemInfo,
    refine: refine::Refine,
    /// Whether others may view our equipment (as the server last said).
    show_equip: Option<bool>,
    /// Our character drawn for the equipment window, keyed by the look it shows.
    portrait: Option<(preview::Look, Option<egui::TextureHandle>)>,
    /// Skills cooling down: id -> ready again at.
    cooldowns: std::collections::HashMap<u16, std::time::Instant>,
    /// When a paused (`wait:`) test script carries on.
    script_resume: Option<std::time::Instant>,
    pad: gamepad::Pad,
    /// The unit the controller acts on (highlighted), the walk direction being held and
    /// time until the walk is re-aimed.
    pad_target: Option<u32>,
    pad_dir: Option<glam::Vec2>,
    pad_walk_ms: f32,
    /// We died and wait to return to the save point (or be resurrected).
    dead: bool,
    /// `--fx name`: loop one STR effect on the player (effect previews).
    fx_test: Option<&'static str>,
    alt: bool,
    queued_action: Option<windows::UiAction>,
    /// A pointer move the test script queued (UI points); egui keeps the position.
    fake_pointer: Option<egui::Pos2>,
    /// A click the test script queued there; pressed one frame, released the next.
    fake_click: Option<(egui::Pos2, bool)>,
    /// Test drags: pointer events played one frame at a time.
    fake_frames: std::collections::VecDeque<Vec<egui::Event>>,
    social: social::Social,
    /// The player we asked to trade, until they answer.
    trade_partner: Option<String>,
    /// Clicked player: (id, name, menu position in UI points).
    player_menu: Option<(u32, String, egui::Pos2)>,
    /// `--deal buy|sell` / `--sell item_id`: answer shops automatically (scripted tests).
    auto_deal: Option<bool>,
    auto_sell: Option<u32>,
    auto_buy: Option<u32>,
    /// `--accept`: accept every request; `--auto-trade item_id|0`: put one item up, lock, commit.
    auto_accept: bool,
    auto_trade: Option<u32>,
    trade_stage: u8,
    trade_offers: std::collections::HashMap<u16, i32>,
    /// Window commands sent next tick (the renderer is borrowed while the UI runs).
    queued_commands: Vec<ro_net::Command>,
    /// `--windows`: open all game windows (for captures).
    open_windows: bool,
    script: Vec<String>,
    /// Pick up the nearest floor item once one appears (scripted tests).
    auto_pickup: bool,
    /// `--talk name`: talk to the first NPC with this name once in view (scripted test).
    talk_to: Option<String>,
    /// Script minimap marks (viewpoint): (npc, mark id) -> (cell, colour, blinking until).
    viewpoints: std::collections::HashMap<(u32, u8), ((f32, f32), egui::Color32, Option<std::time::Instant>)>,
    /// Party and guild members elsewhere on the map: id -> (cell, guild?).
    member_pos: std::collections::HashMap<u32, ((f32, f32), bool)>,
    /// Spirit spheres per unit (ZC_SPIRITS).
    spirits: std::collections::HashMap<u32, (u16, u8)>,
    /// Our normal attack reach (ZC_ATTACK_RANGE).
    attack_range: i32,
    /// A taming item is waiting for a monster to be clicked.
    capturing: bool,
    /// ZC_MAPPROPERTY_R2 flags of the current map.
    map_flags: u32,
    /// Hat effect files by id (hateffectinfo), and the ones playing: (unit, effect) -> fx tag.
    hat_files: std::collections::HashMap<u16, &'static str>,
    hat_fx: std::collections::HashMap<(u32, u16), u32>,
    /// Guild emblems by guild id (None while being fetched).
    emblems: std::collections::HashMap<u32, Option<egui::TextureHandle>>,
    /// Test hook (--npc-answers): replies to NPC dialog steps, in order.
    npc_answers: std::collections::VecDeque<String>,
    /// `--attack`: attack the nearest monster once one is in view (scripted test).
    auto_attack: bool,
    /// Other units in view, by id.
    units: std::collections::HashMap<u32, Mob>,
    /// Warp portals in view (drawn as an effect, not a sprite), by unit id.
    warps: std::collections::HashMap<u32, (u16, u16)>,
    class_names: std::collections::HashMap<i16, String>,
    pet_talk: pettalk::PetTalk,
    sense: sense::Sense,
    look_tables: ro_game::looks::LookTables,
    /// Loaded actor templates by sprite key (None: not available, don't retry).
    sprites: std::collections::HashMap<String, Option<ro_render::Actor>>,
    /// When the current map became ready (for --capture-after).
    ready_at: Option<std::time::Instant>,
    last_tick: std::time::Instant,
    window: Option<Arc<Window>>,
    renderer: Option<ro_render::Renderer>,
    /// Right mouse button held: dragging rotates the camera.
    rotating: bool,
    cursor_sprite: Option<hud::CursorSprite>,
    /// Which cursor to show (hud::cursor), from what is under the mouse.
    cursor_action: usize,
    /// Left button held after clicking the ground: keep walking toward the cursor.
    mouse_walk: bool,
    mouse_walk_ms: f32,
    /// Left mouse button held (from window events).
    left_down: bool,
    cursor: Option<(f64, f64)>,
    fps: FpsCounter,
    egui_ctx: egui::Context,
    egui_state: Option<egui_winit::State>,
    hud: hud::Hud,
    /// UI size multiplier on top of the display's DPI scale (`ui_scale` in the config).
    ui_scale: f32,
}

impl App {
    /// Loads a map into the renderer and remembers its ground for picking.
    fn enter_map(&mut self, name: &str) {
        // A loading screen covers the map load (not behind the login screens or in captures).
        if self.front.is_none() && (self.capture.is_none() || std::env::args().any(|a| a == "--front-start")) {
            if let Some(s) = self.splash.as_mut() {
                s.begin_loading(&self.egui_ctx, self.vfs.as_ref());
            }
        }
        let (Some(r), Some(vfs)) = (self.renderer.as_mut(), self.vfs.as_ref()) else { return };
        match load_map(vfs, name) {
            Ok(map) => {
                r.set_scene(&map.mesh, map.images, map.lighting);
                tracing::info!(count = map.lights.len(), first = ?map.lights.first(), "map light sources");
                r.point_lights = map.lights;
                if let Some((mesh, images)) = map.water {
                    r.set_water(&mesh, images);
                }
                // Ambient map effects play from the frame loop.
                self.map_fx = map.effects.into_iter().enumerate().map(|(i, (id, pos, delay))| fx::MapEmitter::new(id, pos, delay, i)).collect();
                tracing::info!(emitters = self.map_fx.len(), drawn = self.map_fx.iter().filter(|e| e.visual.is_some()).count(), first = ?self.map_fx.first().map(|e| (e.pos, e.every_ms)), "map effects");
                let gat = vfs
                    .read(&format!("data\\{name}.gat"))
                    .ok()
                    .flatten()
                    .and_then(|d| ro_formats::gat::Gat::parse(&d).inspect_err(|e| tracing::warn!("{name}.gat: {e:#}")).ok());
                let path = format!("data\\texture\\유저인터페이스\\map\\{name}.bmp");
                self.minimap = match (load_texture(&self.egui_ctx, vfs, &path), gat.as_ref()) {
                    (Some(texture), Some(g)) => Some(hud::Minimap { texture, cells: (g.width as f32, g.height as f32) }),
                    _ => None,
                };
                if let Some(a) = self.audio.as_mut() {
                    a.enter_map(vfs, name);
                }
                r.set_room_mask(None);
                self.world = Some(World { name: name.to_string(), ground: map.mesh, gat, room: None });
                self.ready_at = Some(std::time::Instant::now());
            }
            Err(e) => tracing::error!("map {name}: {e:#}"),
        }
    }

    /// Actor template for a player look, cached so sheets upload once per look.
    fn player_actor(&mut self, look: &Look) -> Option<ro_render::Actor> {
        let key = format!("player:{}:{}:{}:{}:{:?}", look.job, look.male, look.hair, look.hair_color, look.gear);
        if let Some(a) = self.sprites.get(&key) {
            return a.clone();
        }
        let (r, vfs) = (self.renderer.as_mut()?, self.vfs.as_ref()?);
        // Weapons arrive as item ids; their sprite class is the item's ClassNum (iteminfo).
        let mut resolved = look.clone();
        if let Some(info) = self.item_db.as_ref().and_then(|d| d.get(look.gear.weapon)) {
            resolved.gear.weapon = info.view;
            tracing::debug!(item = look.gear.weapon, view = info.view, name = ?self.look_tables.weapon(info.view), "weapon view");
        }
        let actor = load_character(vfs, r, &resolved, &self.look_tables).inspect_err(|e| tracing::warn!("player sprites {key}: {e:#}")).ok();
        self.sprites.insert(key, actor.clone());
        actor
    }

    /// Actor template for an NPC / monster class: data\sprite\npc\ or data\sprite\몬스터\.
    fn class_actor(&mut self, job: i16) -> Option<ro_render::Actor> {
        // Newer NPC classes (10000+) this client has no name or sprite for still need a
        // body to be clicked (e.g. 10243, the Zonda teleporters): a stock townsman stands in.
        let name = match self.class_names.get(&job) {
            Some(n) => n.clone(),
            None if (10000..20000).contains(&(job as i32)) => {
                tracing::debug!(job, "unknown NPC class, using a stand-in sprite");
                "4_M_01".to_string()
            }
            None => return None,
        };
        let key = format!("class:{name}");
        if let Some(a) = self.sprites.get(&key) {
            return a.clone();
        }
        let (r, vfs) = (self.renderer.as_mut()?, self.vfs.as_ref()?);
        let read = |base: &str| -> Option<(ro_formats::spr::Spr, ro_formats::act::Act)> {
            let spr = vfs.read(&format!("{base}.spr")).ok().flatten()?;
            let act = vfs.read(&format!("{base}.act")).ok().flatten()?;
            Some((ro_formats::spr::Spr::parse(&spr).ok()?, ro_formats::act::Act::parse(&act).ok()?))
        };
        // Homunculi are in homun\; mercenaries name a human body (e.g. 여\활용병) and carry
        // a weapon from 인간족\용병\.
        // Monster classes look in 몬스터 first: some names (poring) are also NPC sprites.
        let folders: &[&str] = if (1000..4000).contains(&(job as i32)) || job as i32 >= 20000 || job < 0 { &["몬스터", "npc", "homun", "인간족\\몸통"] } else { &["npc", "몬스터", "homun", "인간족\\몸통"] };
        let found = folders.iter().find_map(|folder| read(&format!("data\\sprite\\{folder}\\{name}"))).or_else(|| {
            // Its sprite is missing from this client's data (e.g. 10243 4_m_drzonda01).
            (10000..20000).contains(&(job as i32)).then(|| tracing::debug!(job, %name, "missing NPC sprite, using a stand-in"))?;
            read("data\\sprite\\npc\\4_m_01")
        });
        let merc_weapon = name.rsplit('\\').next().and_then(|n| ["활", "창", "검"].iter().find_map(|w| read(&format!("data\\sprite\\인간족\\용병\\{n}_{w}"))));
        // Mercenary bodies are headless; the original adds a player head, its style set by
        // the mercenary's grade (classes 6017.. come in tens: archers, lancers, swordsmen).
        let merc_head = merc_weapon.as_ref().and(name.split_once('\\')).and_then(|(sex, _)| read(&format!("data\\sprite\\인간족\\머리통\\{sex}\\{}_{sex}", (job - 6017).rem_euclid(10) + 1)));
        let actor = match found {
            Some((spr, act)) => {
                let body = r.add_sheet(&spr, act, None);
                let weapon = merc_weapon.map(|(s, a)| vec![r.add_sheet(&s, a, None)]).unwrap_or_default();
                let head = merc_head.map(|(s, a)| r.add_sheet(&s, a, None));
                let shadow = self.sprites.get("shadow").cloned().flatten().and_then(|a| a.shadow);
                Some(ro_render::Actor { pos: Default::default(), body, head, headgear: vec![], weapon, shadow, action: 0, dir: ro_render::Dir::S, time_ms: 0.0, once: false, alpha: 1.0 })
            }
            None => {
                tracing::debug!(job, %name, "no sprite for class");
                None
            }
        };
        self.sprites.insert(key, actor.clone());
        actor
    }

    /// Creates (or re-creates) the local player at a cell.
    fn spawn_player(&mut self, look: &Look, cell: (u16, u16), dir: usize, speed: i16) {
        if let Some(actor) = self.player_actor(look) {
            // NPC/monster actors borrow the player's shadow sheet.
            self.sprites.insert("shadow".into(), Some(actor.clone()));
            let mut mob = Mob::new(actor, cell, dir, speed);
            mob.name = self.hud.status.name.clone();
            mob.look = Some(look.clone());
            self.player = Some(mob);
        }
    }

    fn market_test_buy(&mut self) {
        tracing::info!(pending = ?self.market_buy, "market test buy");
        if let Some(r) = self.market_buy.take().and_then(|(row, n)| self.market.test_buy(row, n)) {
            self.send(ro_net::Command::Market(r));
        }
    }

    /// Redraws the equipment window's portrait when our look has changed.
    fn refresh_portrait(&mut self) {
        let Some(look) = self.player.as_ref().and_then(|p| p.look.as_ref()) else { return };
        let mut headgear = vec![];
        for v in [look.gear.head_bottom, look.gear.head_mid, look.gear.head_top] {
            if let Some(n) = (v != 0).then(|| self.look_tables.accessory(v as u32).map(str::to_string)).flatten() {
                if !headgear.contains(&n) {
                    headgear.push(n);
                }
            }
        }
        let want = preview::Look { job: look.job.clone(), male: look.male, hair: look.hair.max(1), hair_color: look.hair_color, headgear };
        if self.portrait.as_ref().is_some_and(|p| p.0 == want) {
            return;
        }
        let tex = self.vfs.as_ref().and_then(|v| preview::compose(v, &want)).map(|img| self.egui_ctx.load_texture("portrait", img, egui::TextureOptions::NEAREST));
        self.portrait = Some((want, tex));
    }

    /// Applies a ZC_SPRITE_CHANGE to a player-sprite unit (us or another player).
    fn look_change(&mut self, id: u32, kind: u8, value: u32, value2: u32) {
        let Some(mut look) = self.mob_mut(id).and_then(|m| m.look.clone()) else { return };
        match kind {
            0 => {
                look.job = job_sprite(value as i16).to_string();
                if id == self.account_id {
                    self.hud.status.job = job_title(value as i16).to_string();
                    self.hud.status.job_id = value as i16;
                    self.load_skill_icons();
                }
            }
            1 => look.hair = value.max(1) as u16,
            2 => (look.gear.weapon, look.gear.shield) = (value, value2),
            3 => look.gear.head_bottom = value as u16,
            4 => look.gear.head_top = value as u16,
            5 => look.gear.head_mid = value as u16,
            6 => look.hair_color = value as u16,
            _ => return,
        }
        let Some(fresh) = self.player_actor(&look) else { return };
        if let Some(m) = self.mob_mut(id) {
            // Keep position, pose and timing; swap the layers.
            (m.actor.body, m.actor.head, m.actor.headgear, m.actor.weapon) = (fresh.body, fresh.head, fresh.headgear, fresh.weapon);
            m.look = Some(look);
        }
    }

    /// A unit came into view or started walking.
    fn unit(&mut self, info: ro_net::UnitInfo) {
        if info.id == self.account_id {
            return;
        }
        if (6001..=6099).contains(&info.job) {
            self.unit_jobs.insert(info.id, info.job);
        }
        let gat = self.world.as_ref().and_then(|w| w.gat.as_ref());
        if let Some(m) = self.units.get_mut(&info.id) {
            match info.pos {
                ro_net::UnitPos::Walking { from, to } => m.walk(gat, from, to),
                ro_net::UnitPos::At { x, y, dir } => {
                    m.walker.stop_at((x as i32, y as i32));
                    m.walker.dir = dir as usize;
                }
            }
            return;
        }
        // Warps are an effect; hidden NPCs are not drawn at all.
        match info.job {
            45 => {
                if let ro_net::UnitPos::At { x, y, .. } = info.pos {
                    self.warps.insert(info.id, (x, y));
                }
                return;
            }
            111 | 139 | 32767 => return,
            _ => {}
        }
        let look = is_player_class(info.job).then(|| Look {
            job: job_sprite(info.job).to_string(),
            male: info.sex == 1,
            hair: info.hair.max(1),
            hair_color: info.hair_color.max(0) as u16,
            gear: info.look,
        });
        let actor = match &look {
            Some(l) => self.player_actor(l),
            None => self.class_actor(info.job),
        };
        let Some(actor) = actor else { return };
        let (cell, dir) = match info.pos {
            ro_net::UnitPos::At { x, y, dir } => ((x, y), dir as usize),
            ro_net::UnitPos::Walking { from, .. } => (from, 0),
        };
        let mut mob = Mob::new(actor, cell, dir, info.speed);
        // NPC names may carry a "#unique-suffix" that the client never shows.
        mob.name = info.name.split('#').next().unwrap_or_default().trim().to_string();
        mob.kind = info.object_type;
        mob.class = info.job;
        mob.actor.alpha = option_alpha(info.option, false);
        mob.human = is_player_class(info.job);
        mob.look = look;
        if let ro_net::UnitPos::Walking { from, to } = info.pos {
            let gat = self.world.as_ref().and_then(|w| w.gat.as_ref());
            mob.walk(gat, from, to);
        }
        // Players: ask for their party and guild names, shown under the name.
        if info.object_type == 0 {
            self.send(ro_net::Command::ReqName(info.id));
        }
        mob.guild = info.guild;
        if info.guild != 0 && !self.emblems.contains_key(&info.guild) {
            self.emblems.insert(info.guild, None);
            self.send(ro_net::Command::Guild(ro_net::GuildRequest::Emblem(info.guild)));
        }
        tracing::debug!(id = info.id, job = info.job, name = %info.name, ?cell, "unit");
        self.units.insert(info.id, mob);
    }

    fn lapine_action(&mut self, a: lapine::LapineAction) {
        self.queued_commands.push(lapine_command(a));
    }

    /// Asks to walk to a cell: through the server online, locally offline.
    fn request_walk(&mut self, cell: (u16, u16)) {
        if self.net.is_none() {
            let (Some(p), Some(w)) = (self.player.as_mut(), self.world.as_ref()) else { return };
            if let Some(gat) = &w.gat {
                if let Some(path) = ro_game::path::find_long(gat, p.walker.cell, (cell.0 as i32, cell.1 as i32), LONG_WALK_NODES) {
                    p.walker.walk(path);
                }
            }
            return;
        }
        self.walk_goal = Some(WalkGoal { cell, sent_at: None, tries: 0 });
        self.next_hop();
    }

    /// Online walks longer than the server's MAX_WALKPATH go as a chain of hops along the
    /// full client-side path; the next hop is sent when the previous one finishes.
    fn next_hop(&mut self) {
        let (Some(goal), Some(p), Some(w), Some((tx, _))) = (self.walk_goal.as_mut(), self.player.as_ref(), self.world.as_ref(), self.net.as_ref())
        else {
            return;
        };
        let target = (goal.cell.0 as i32, goal.cell.1 as i32);
        if p.walker.cell == target || goal.tries >= 5 {
            self.walk_goal = None; // arrived, or the server keeps refusing
            return;
        }
        let path = w.gat.as_ref().and_then(|g| ro_game::path::find_long(g, p.walker.cell, target, LONG_WALK_NODES));
        let Some(path) = path.filter(|p| !p.is_empty()) else {
            tracing::debug!(from = ?p.walker.cell, ?target, "no path to walk target");
            self.walk_goal = None;
            return;
        };
        // A refused hop is retried shorter: rAthena's A* uses a small fixed node table and
        // fails on detours (around fountains, walls) even under max_walk_path.
        let hop_len = (WALK_HOP >> goal.tries.min(2)).max(1);
        let hop = path[path.len().min(hop_len) - 1];
        tracing::debug!(from = ?p.walker.cell, ?target, steps = path.len(), ?hop, "walk hop");
        let _ = tx.send(ro_net::Command::Move { x: hop.0 as u16, y: hop.1 as u16 });
        goal.sent_at = Some(std::time::Instant::now());
        goal.tries += 1;
    }

    fn handle(&mut self, event: ro_net::Event) {
        use ro_net::Event::*;
        // Login and character-select replies belong to those screens.
        if let Some(front) = self.front.as_mut() {
            if front.event(&event) {
                if let (Characters { chars, .. }, true) = (&event, std::env::args().any(|a| a == "--front-start")) {
                    if let Some(c) = chars.first() {
                        self.queued_commands.push(ro_net::Command::SelectChar(c.slot));
                    }
                }
                if let (Characters { chars, .. }, true) = (&event, std::env::args().any(|a| a == "--front-make")) {
                    if !chars.iter().any(|c| c.name == "Preview") {
                        let slot = (0..9u8).find(|s| !chars.iter().any(|c| c.slot == *s)).unwrap_or(1);
                        self.queued_commands.push(ro_net::Command::Char(ro_net::CharRequest::Make { name: "Preview".into(), slot, hair_color: 4, hair_style: 9, job: 0, sex: 0 }));
                    }
                }
                if matches!(event, Characters { .. }) && std::env::args().any(|a| a == "--front-create") {
                    front.open_create();
                }
                return;
            }
            match &event {
                Entered { .. } => {
                    // Remember the ID for next time (Remember ID).
                    self.settings.last_id = if front.remember { front.user().to_string() } else { String::new() };
                    self.settings.save();
                    self.front = None;
                }
                Disconnected(reason) => {
                    // Back to a fresh login screen with the reason.
                    let user = front.user().to_string();
                    let mut f = screens::Front::new(user, front.remember);
                    f.event(&LoginFailed(format!("Disconnected: {reason}")));
                    self.front = Some(f);
                    if let (Some(a), Some(v)) = (self.audio.as_mut(), self.vfs.as_ref()) {
                        a.title(v);
                    }
                    self.net = Some(ro_net::spawn_session(self.session.clone()));
                    return;
                }
                _ => {}
            }
        }
        match event {
            Entered { map, x, y, dir, character, account_id } => {
                tracing::info!(%map, x, y, name = %character.name, job = character.job, "entered");
                self.account_id = account_id;
                self.char_id = character.char_id;
                self.units.clear();
                self.floor.clear();
                self.warps.clear();
                self.enter_map(&map);
                let look = Look {
                    job: job_sprite(character.job).to_string(),
                    male: character.sex == 1,
                    hair: character.head.max(1) as u16,
                    hair_color: character.hair_color as u16,
                    gear: character.look,
                };
                let s = &mut self.hud.status;
                s.name = character.name.clone();
                s.job = job_title(character.job).to_string();
                s.job_id = character.job;
                s.base_level = character.base_level as i64;
                s.job_level = character.job_level as i64;
                (s.hp, s.max_hp, s.sp, s.max_sp) = (character.hp, character.max_hp, character.sp, character.max_sp);
                s.base_exp = character.base_exp;
                s.zeny = character.zeny as i64;
                self.spawn_player(&look, (x, y), dir as usize, character.speed);
                if let Some(text) = self.say_on_enter.take() {
                    // Several lines separated by "|" (e.g. a batch of GM commands).
                    for line in text.split('|') {
                        self.queued_chat.push(line.trim().to_string());
                    }
                }
                if let Some(cell) = self.walk_on_enter.take() {
                    self.request_walk(cell);
                }
            }
            Walk { from, to } => {
                let gat = self.world.as_ref().and_then(|w| w.gat.as_ref());
                if let Some(p) = self.player.as_mut() {
                    tracing::debug!(?from, ?to, "walk");
                    if let Some(g) = self.walk_goal.as_mut() {
                        g.tries = 0; // the server accepted this hop
                    }
                    p.walk(gat, from, to);
                }
            }
            Stop { id, x, y } => {
                let mob = if id == self.account_id { self.player.as_mut() } else { self.units.get_mut(&id) };
                if let Some(m) = mob {
                    m.walker.stop_at((x as i32, y as i32));
                }
            }
            Unit(info) => self.unit(info),
            Stat { id, value } => {
                self.hud.status.set(id, value);
                // Keep the Status window live: points and the derived values (SP_ATK1..SP_ASPD).
                if let Some(s) = self.stats.as_mut() {
                    let v = value.clamp(i16::MIN as i64, i16::MAX as i64) as i16;
                    match id {
                        9 => s.points = value.max(0) as u32,
                        41 => s.atk.0 = v,
                        42 => s.atk.1 = v,
                        43 => s.matk.1 = v,
                        44 => s.matk.0 = v,
                        45 => s.def.0 = v,
                        46 => s.def.1 = v,
                        47 => s.mdef.0 = v,
                        48 => s.mdef.1 = v,
                        49 => s.hit = v,
                        50 => s.flee.0 = v,
                        51 => s.flee.1 = v,
                        52 => s.crit = v,
                        53 => s.aspd = (v, 0),
                        _ => {}
                    }
                }
            }
            Chat { kind, text, .. } => {
                if let (ro_net::ChatKind::Whisper, Some(name)) = (kind, text.strip_prefix("(From ").and_then(|t| t.split_once(") : ")).map(|(n, _)| n.to_string())) {
                    self.last_whisper_from = Some(name);
                    self.play_sound("_chat_whisper.wav");
                }
                self.hud.push_chat(kind, text);
            }
            Menu(menu) => {
                // Icons for items that may not be in the inventory (poisons, arrows to make).
                let ids: Vec<u32> = match &menu {
                    ro_net::Menu::Items(ids) | ro_net::Menu::Make { items: ids, .. } => ids.clone(),
                    ro_net::Menu::Produce(list) => list.iter().flat_map(|(id, m)| std::iter::once(*id).chain(m.iter().copied())).collect(),
                    _ => vec![],
                };
                self.load_icons_for(&ids);
                self.windows.menu = Some(menu);
            }
            MakeResult { ok, item } => {
                let name = self.item_db.as_ref().map_or_else(|| format!("item {item}"), |d| d.name(item, true));
                let msg = if ok { format!("You made {name}.") } else { format!("Making {name} failed.") };
                self.hud.push_chat(ro_net::ChatKind::System, msg);
            }
            Quests(list) => self.quests.quests = list.into_iter().map(|q| (q.id, q)).collect(),
            QuestAdded(q) => {
                self.hud.push_chat(ro_net::ChatKind::System, format!("Quest accepted: {}", self.quests.title(q.id)));
                self.quests.quests.insert(q.id, q);
            }
            QuestRemoved(id) => {
                self.quests.quests.remove(&id);
            }
            QuestProgress(updates) => self.quests.progress(&updates),
            QuestActiveChanged { quest, active } => {
                if let Some(q) = self.quests.quests.get_mut(&quest) {
                    q.active = active;
                }
            }
            QuestMark { npc, mark, color } => {
                if mark == 9999 {
                    self.quests.marks.remove(&npc);
                } else {
                    self.quests.marks.insert(npc, (mark, color));
                }
            }
            InventoryExtra(n) => self.windows.inv_extra = n,
            ItemFavorite { index, favorite } => {
                if let Some(it) = self.inventory.items.get_mut(&index) {
                    it.favorite = favorite;
                }
            }
            ExpandAsk { result, item } => match result {
                0 => self.windows.expand_ask = Some(item),
                _ => self.hud.push_chat(ro_net::ChatKind::System, match result {
                    2 => "You can't expand your inventory right now.",
                    3 => "You need an inventory expansion item.",
                    4 => "Your inventory can't be expanded any further.",
                    _ => "The inventory can't be expanded.",
                }.into()),
            },
            CartOff => {
                self.merchant.cart.clear();
                self.merchant.cart_count = Default::default();
                self.merchant.cart_open = false;
            }
            PartyPickup { aid, item, refine } => {
                let who = self.units.get(&aid).map(|m| m.name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| "A party member".into());
                let name = self.item_db.as_ref().map_or_else(|| format!("Item #{item}"), |d| d.name(item, true));
                let refine = if refine > 0 { format!("+{refine} ") } else { String::new() };
                self.hud.push_chat(ro_net::ChatKind::Party, format!("{who} got {refine}{name}."));
            }
            CartStart => {
                tracing::debug!("cart list");
                self.merchant.cart.clear();
            }
            CartItems(items) => {
                for d in items {
                    let it = inv_item(d);
                    self.merchant.cart.insert(it.index, it);
                }
                let ids = self.merchant.item_ids();
                self.load_icons_for(&ids);
            }
            CartCount { items, max_items, weight, max_weight } => {
                tracing::debug!(items, max_items, weight, max_weight, "cart");
                self.merchant.cart_count = (items, max_items, weight, max_weight);
            }
            CartAdded(d) => {
                let it = inv_item(d);
                self.load_icons_for(&[it.id]);
                match self.merchant.cart.get_mut(&it.index) {
                    Some(e) if e.id == it.id => e.amount += it.amount,
                    _ => {
                        self.merchant.cart.insert(it.index, it);
                    }
                }
            }
            CartRemoved { index, amount } => {
                if let Some(e) = self.merchant.cart.get_mut(&index) {
                    e.amount = e.amount.saturating_sub(amount as u16);
                    if e.amount == 0 {
                        self.merchant.cart.remove(&index);
                    }
                }
            }
            ShopSign { aid, title } => {
                if title.is_empty() {
                    self.merchant.signs.remove(&aid);
                } else {
                    self.merchant.signs.insert(aid, title);
                }
            }
            Shop { seller, shop, items } => {
                let ids: Vec<u32> = items.iter().map(|i| i.id).collect();
                self.load_icons_for(&ids);
                let amounts = vec![0; items.len()];
                self.merchant.browsing = Some((seller, shop, items, amounts));
            }
            ShopSetup(slots) => {
                self.merchant.setup = Some((slots, format!("{}'s shop", self.hud.status.name), Default::default()));
            }
            MyShop(stock) => self.merchant.my_shop = Some(stock),
            Sold { index, amount, zeny } => {
                let what = self.merchant.my_shop.as_ref().and_then(|s| s.iter().find(|i| i.index == index)).map(|i| i.id);
                let name = what.and_then(|id| self.item_db.as_ref().map(|d| d.name(id, true))).unwrap_or_else(|| "an item".into());
                self.hud.push_chat(ro_net::ChatKind::System, format!("Sold {amount} x {name} for {zeny} z."));
                if let Some(stock) = self.merchant.my_shop.as_mut() {
                    if let Some(i) = stock.iter_mut().find(|i| i.index == index) {
                        i.amount = i.amount.saturating_sub(amount);
                    }
                    stock.retain(|i| i.amount > 0);
                }
            }
            BuySign { aid, title } => {
                if title.is_empty() {
                    self.stores.signs.remove(&aid);
                } else {
                    self.stores.signs.insert(aid, title);
                }
            }
            BuyStoreSetup(slots) => {
                let limit = self.hud.status.zeny.max(0).to_string();
                self.stores.setup = Some((slots, format!("{} is buying", self.hud.status.name), limit, Default::default()));
            }
            MyBuyStore { limit, items } => {
                self.stores.mine = Some((limit, items));
                let ids = self.stores.item_ids();
                self.load_icons_for(&ids);
            }
            BuyStore { buyer, store, limit, items } => {
                let amounts = vec![0; items.len()];
                self.stores.browsing = Some((buyer, store, limit, items, amounts));
                let ids = self.stores.item_ids();
                self.load_icons_for(&ids);
            }
            BoughtThroughStore { id, amount, zeny, limit } => {
                let name = self.item_db.as_ref().map_or_else(|| format!("Item #{id}"), |d| d.name(id, true));
                self.hud.push_chat(ro_net::ChatKind::System, format!("Your buying store bought {amount} x {name} for {zeny} z."));
                if let Some((l, items)) = self.stores.mine.as_mut() {
                    *l = limit;
                    if let Some(i) = items.iter_mut().find(|i| i.id == id) {
                        i.amount = i.amount.saturating_sub(amount);
                    }
                }
            }
            SoldToStore { index, amount, price } => {
                let name = self.inventory.items.get(&index).and_then(|i| self.item_db.as_ref().map(|d| d.name(i.id, true))).unwrap_or_else(|| "an item".into());
                self.hud.push_chat(ro_net::ChatKind::System, format!("Sold {amount} x {name} for {} z.", amount as u64 * price as u64));
                self.inventory.remove(index, amount);
            }
            BuyStoreNotice(msg) => {
                if msg.contains("closed") {
                    self.stores.mine = None;
                }
                self.hud.push_chat(ro_net::ChatKind::System, msg);
            }
            Inbox(mut list) => {
                for h in &mut list {
                    h.title = msgstr::resolve(&h.title, "(no title)");
                    h.sender = msgstr::resolve(&h.sender, "System");
                }
                self.mailbox.inbox = list;
            }
            Mail { id, text, zeny, items, .. } => {
                let text = msgstr::resolve(&text, "");
                if let Some(h) = self.mailbox.inbox.iter_mut().find(|h| h.id == id) {
                    h.read = true;
                    self.mailbox.reading = Some((h.clone(), text, zeny, items));
                }
                let ids = self.mailbox.item_ids();
                self.load_icons_for(&ids);
            }
            MailTaken { items, id, result } => {
                if result == 0 {
                    if let Some(r) = self.mailbox.reading.as_mut().filter(|r| r.0.id == id) {
                        if items {
                            r.3.clear();
                        } else {
                            r.2 = 0;
                        }
                    }
                    self.mailbox.status = if items { "Items taken.".into() } else { "Zeny taken.".into() };
                } else {
                    self.mailbox.status = "Couldn't take that (weight or inventory full?).".into();
                }
            }
            MailDeleted(id) => {
                self.mailbox.inbox.retain(|h| h.id != id);
                if self.mailbox.reading.as_ref().is_some_and(|r| r.0.id == id) {
                    self.mailbox.reading = None;
                }
            }
            MailUnread(new) => {
                if new && !self.mailbox.open {
                    self.mailbox.unread = true;
                    self.hud.push_chat(ro_net::ChatKind::System, "You have new mail (Alt+X).".into());
                }
            }
            MailReceiver(id, _) => {
                let me = self.hud.status.name.clone();
                if let Some(req) = self.mailbox.receiver_found(id, &me) {
                    self.send(ro_net::Command::Mail(req));
                }
            }
            ClanInfo(info) => self.clan.info = Some(info),
            ClanOnline(on, max) => self.clan.online = Some((on, max)),
            Navigate { map, x, y } => {
                self.navi.ensure_loaded(self.vfs.as_ref());
                let cell = (x > 0 && y > 0).then_some((x, y));
                let label = match cell { Some((x, y)) => format!("{map} ({x}, {y})"), None => map.clone() };
                self.navi.go(&map, cell, label);
            }
            Bg(e) => {
                let map = self.world.as_ref().map(|w| w.name.clone()).unwrap_or_default();
                if let Some(text) = self.battle.event(e, &map) {
                    self.hud.push_chat(ro_net::ChatKind::Battle, text);
                }
                if self.battle.auto_accept {
                    if let Some((name, lobby)) = self.battle.ready.take() {
                        self.send(ro_net::Command::Bg(ro_net::BgRequest::Reply { accept: true, name, lobby }));
                    }
                }
            }
            Castles(list) => self.battle.castles = list,
            CastleInfo { id, economy, defense } => self.battle.castle_info(id, economy, defense),
            CastleTeleport(r) => {
                let why = if r == 1 { "You don't have enough zeny to teleport to the castle." } else { "You can't teleport to the castle right now." };
                self.hud.push_chat(ro_net::ChatKind::System, why.into());
            }
            Market(items) => {
                self.market.open(market::Shop::Market(items));
                self.market_test_buy();
            }
            MarketResult { ok, bought } => {
                self.market.status = if ok { "Thank you for your purchase.".into() } else { "The purchase failed.".into() };
                if let Some(market::Shop::Market(items)) = self.market.shop.as_mut() {
                    for (item, n, _) in bought {
                        if let Some(it) = items.iter_mut().find(|i| i.item == item) {
                            it.stock = it.stock.saturating_sub(n as u32);
                        }
                    }
                }
            }
            Barter { items, expanded } => {
                self.market.open(market::Shop::Barter { items, expanded });
                self.market_test_buy();
            }
            ClanLeft => {
                self.clan.left();
                self.hud.push_chat(ro_net::ChatKind::System, "You left your clan.".into());
            }
            MailItemAdded { result, index, amount, id } => self.mailbox.item_added(result, index, amount, id),
            MailItemRemoved { ok, index, amount } => self.mailbox.item_removed(ok, index, amount),
            MailSent(result) => {
                self.mailbox.sent(result);
                if result == 0 {
                    self.send(ro_net::Command::Mail(ro_net::MailRequest::Open { refresh: true }));
                }
            }
            IgnoreResult { all, block, result } => {
                let who = if all { "everyone".to_string() } else { self.ignore_pending.take().map_or_else(|| "that player".into(), |(n, _)| n) };
                let msg = match (result, block) {
                    (0, true) => format!("Whispers from {who} are now blocked."),
                    (0, false) => format!("Whispers from {who} are allowed again."),
                    (2, _) => "Your block list is full.".into(),
                    (_, true) => format!("Whispers from {who} were already blocked."),
                    (_, false) => format!("{who} wasn't blocked."),
                };
                self.hud.push_chat(ro_net::ChatKind::System, msg);
            }
            IgnoreList(names) => {
                let msg = if names.is_empty() { "You aren't blocking anyone's whispers.".into() } else { format!("Blocked: {}", names.join(", ")) };
                self.hud.push_chat(ro_net::ChatKind::System, msg);
            }
            BookingResult { delete, result } => {
                self.booking.status = match (delete, result) {
                    (false, 0) => {
                        self.booking.posted = true;
                        "Your ad is posted.".into()
                    }
                    (false, 2) => {
                        self.booking.posted = true;
                        "You already have an ad posted.".into()
                    }
                    (false, _) => "Couldn't post the ad.".into(),
                    (true, 0) => {
                        self.booking.posted = false;
                        "Your ad was removed.".into()
                    }
                    (true, _) => "Couldn't remove the ad.".into(),
                };
                let level = self.hud.status.base_level.clamp(1, 999) as u16;
                let _ = level;
                    self.send(ro_net::Command::Booking(ro_net::BookingRequest::Search { level: 0, map: 0, job: 0xffff }));
            }
            BookingList(ads) => {
                let me = self.hud.status.name.clone();
                self.booking.posted = ads.iter().any(|a| a.leader == me) || self.booking.posted;
                self.booking.ads = ads;
            }
            RoomSign(room) => {
                if room.owner == self.account_id {
                    if let Some(inside) = self.rooms.inside.as_mut().filter(|r| r.0 == 0) {
                        inside.0 = room.id;
                    }
                }
                self.rooms.signs.insert(room.owner, room);
            }
            RoomGone(id) => self.rooms.signs.retain(|_, r| r.id != id),
            RoomCreated(code) => {
                if code == 0 {
                    // The creator gets no member list: the room is just us, as owner.
                    let me = self.hud.status.name.clone();
                    self.rooms.inside = Some((0, vec![(true, me)]));
                }
                let msg = match code {
                    0 => None,
                    1 => Some("Too many chat rooms here."),
                    _ => Some("You already have a chat room."),
                };
                if let Some(m) = msg {
                    self.hud.push_chat(ro_net::ChatKind::System, m.into());
                }
            }
            RoomRefused(code) => {
                let msg = match code {
                    0 => "The room is full.",
                    1 => "Wrong password.",
                    2 => "You were kicked from that room.",
                    4 => "You don't have enough zeny to join.",
                    5 => "Your level is too low for that room.",
                    6 => "Your level is too high for that room.",
                    7 => "Your job can't join that room.",
                    _ => "",
                };
                if !msg.is_empty() {
                    self.hud.push_chat(ro_net::ChatKind::System, msg.into());
                }
            }
            RoomEntered { id, members } => self.rooms.inside = Some((id, members)),
            RoomMember { joined, name, kicked } => {
                let me = self.hud.status.name.clone();
                if !joined && name == me {
                    self.rooms.inside = None;
                    if kicked {
                        self.hud.push_chat(ro_net::ChatKind::System, "You were kicked from the chat room.".into());
                    }
                } else if let Some((_, members)) = self.rooms.inside.as_mut() {
                    if joined {
                        members.push((false, name.clone()));
                        self.hud.push_chat(ro_net::ChatKind::System, format!("{name} joined the chat room."));
                    } else {
                        members.retain(|(_, n)| *n != name);
                        self.hud.push_chat(ro_net::ChatKind::System, format!("{name} left the chat room."));
                    }
                }
            }
            RoomOwner(name) => {
                if let Some((_, members)) = self.rooms.inside.as_mut() {
                    for m in members.iter_mut() {
                        m.0 = m.1 == name;
                    }
                }
            }
            Pet(info) => self.companions.pet = Some(info),
            CompanionState { homun, state, id, value } => {
                if homun {
                    if state == 0 {
                        self.companions.homun_id = id;
                    } else if let Some(h) = self.companions.homun.as_mut() {
                        match state {
                            1 => h.intimacy = value as u16,
                            2 => h.hunger = value as u16,
                            _ => {}
                        }
                    }
                } else if state == 0 {
                    self.companions.pet_id = id;
                    // The server sends the pet's details only when asked.
                    self.send(ro_net::Command::Companion(ro_net::CompanionRequest::PetMenu(0)));
                } else if let Some(p) = self.companions.pet.as_mut() {
                    match state {
                        1 => p.intimacy = value as u16,
                        2 => p.hunger = value as u16,
                        3 => p.accessory = value as u16,
                        _ => {}
                    }
                }
            }
            Homun(info) => self.companions.homun = Some(info),
            CompanionSkills { merc, skills } => {
                if merc {
                    self.companions.merc_skills = skills;
                } else {
                    self.companions.homun_skills = skills;
                }
            }
            Merc(info) => self.companions.merc = Some(info),
            MsgId(id @ (1266 | 1267)) => {
                // The mercenary is gone: killed, or its contract ran out.
                self.companions.merc = None;
                self.companions.merc_skills.clear();
                self.merc_ai = None;
                tracing::info!(id, "mercenary gone");
                let msg = if id == 1267 { "Your mercenary was killed." } else { "Your mercenary's contract has ended." };
                self.hud.push_chat(ro_net::ChatKind::System, msg.into());
            }
            MsgId(id) => match msgstr::get(id as usize, &[]) {
                Some(text) => self.hud.push_chat(ro_net::ChatKind::System, text),
                None => tracing::debug!(id, "server message"),
            },
            Elemental { id, hp, max_hp, sp, max_sp } => {
                let name = self.units.get(&id).map(|m| m.name.clone()).unwrap_or_default();
                self.companions.elem = Some(companions::Elemental { id, name, hp, max_hp, sp, max_sp });
            }
            ElemParam(k, v) => {
                if let Some(e) = self.companions.elem.as_mut() {
                    let u = v.max(0) as u32;
                    match k {
                        5 => e.hp = u,
                        6 => e.max_hp = u,
                        7 => e.sp = u,
                        8 => e.max_sp = u,
                        _ => {}
                    }
                    let id = e.id;
                    let (hp, max) = (e.hp as i32, e.max_hp as i32);
                    if let Some(m) = self.units.get_mut(&id) {
                        m.hp = Some((hp, max));
                    }
                }
            }
            HomunParam(k, v) => {
                if let Some(h) = self.companions.homun.as_mut() {
                    let u = v.clamp(0, u32::MAX as i64) as u32;
                    match k {
                        1 => h.exp = v,
                        5 => h.hp = u,
                        6 => h.max_hp = u,
                        7 => h.sp = u,
                        8 => h.max_sp = u,
                        _ => {}
                    }
                }
            }
            HomunSkill { id, level, sp, range, upgradable } => {
                if let Some(s) = self.companions.homun_skills.iter_mut().find(|s| s.id == id) {
                    s.level = level;
                    s.sp = sp;
                    s.range = range;
                    s.upgradable = upgradable;
                }
            }
            SummonHp { id, hp, max } => {
                if let Some(m) = self.units.get_mut(&id) {
                    let max = max.map(|x| x as i32).or(m.hp.map(|h| h.1)).unwrap_or(hp as i32);
                    m.hp = Some((hp as i32, max));
                }
            }
            MercParam(k, v) => {
                if let Some(m) = self.companions.merc.as_mut() {
                    match k {
                        5 => m.hp = v.max(0) as u32,
                        6 => m.max_hp = v.max(0) as u32,
                        7 => m.sp = v.max(0) as u32,
                        8 => m.max_sp = v.max(0) as u32,
                        _ => {}
                    }
                }
            }
            Achievements { rank, list } => {
                self.progress.set_all(rank, list);
                let ids = self.progress.item_ids();
                self.load_icons_for(&ids);
            }
            AchievementUpdate { rank, ach } => {
                if let Some(name) = self.progress.update(rank, ach) {
                    self.hud.push_chat(ro_net::ChatKind::System, format!("Achievement complete: {name}"));
                    self.play_sound("ui_achievement_complete.wav");
                }
            }
            AchievementReward { failed, id } => {
                if failed {
                    self.hud.push_chat(ro_net::ChatKind::System, "The achievement reward couldn't be given.".into());
                } else {
                    self.progress.rewarded(id);
                    self.hud.push_chat(ro_net::ChatKind::System, format!("Reward collected: {}", self.progress.name(id)));
                }
            }
            TitleChanged { failed, id } => {
                if !failed {
                    self.progress.title = id;
                    let msg = if id == 0 { "Title removed.".into() } else { format!("Title: {}", self.progress.title_name(id)) };
                    self.hud.push_chat(ro_net::ChatKind::System, msg);
                }
            }
            UiOpen { kind: 7, data } => {
                if let Some(vfs) = self.vfs.as_ref() {
                    self.progress.load_art(&self.egui_ctx, vfs);
                }
                self.progress.attendance = Some(data);
            }
            UiOpen { kind: 1, .. } => {
                let (hair, color) = self.player.as_ref().and_then(|p| p.look.as_ref()).map_or((1, 0), |l| (l.hair, l.hair_color));
                self.stylist.open(hair, color, 0);
            }
            UiOpen { kind: 8, .. } => {
                self.grade.open = true;
            }
            GradeOptions(o) => self.grade.options = Some(o),
            GradeResult { index, grade, result } => {
                if let Some(it) = self.inventory.items.get_mut(&index) {
                    it.grade = grade as u8;
                    // A grade raised starts again from +0.
                    if result == 0 {
                        it.refine = 0;
                    }
                }
                if let Some(r) = self.grade.result(grade, result) {
                    self.send(ro_net::Command::Grade(r));
                }
            }
            GradeBroadcast { name, item, grade, ok } => {
                let item = self.item_db.as_ref().map_or_else(|| format!("Item #{item}"), |d| d.name(item, true));
                let letter = grade::LETTERS.get(grade as usize).copied().unwrap_or("?");
                let text = if ok { format!("{name} raised {item} to grade {letter}!") } else { format!("{name} failed to raise the grade of {item}.") };
                self.hud.push_chat(ro_net::ChatKind::Broadcast, text);
            }
            UiOpen { .. } => {}
            Attendance(data) => {
                self.progress.attendance = Some(data);
                self.hud.push_chat(ro_net::ChatKind::System, format!("Attendance checked: day {}. Your reward was sent by mail.", data / 10));
            }
            CashShop { cash, kafra } => {
                self.stores.cash = Some((cash, kafra));
                if let Some(vfs) = self.vfs.as_ref() {
                    self.stores.load_cash_art(&self.egui_ctx, vfs);
                }
                // The server sends the item list once per login.
                if self.stores.cash_tabs.is_empty() {
                    self.send(ro_net::Command::CashShop(ro_net::CashShopRequest::List));
                }
            }
            CashTab { tab, items } => {
                self.stores.cash_tabs.insert(tab, items);
                let ids = self.stores.item_ids();
                self.load_icons_for(&ids);
            }
            CashResult { result, cash, kafra } => {
                if let Some(c) = self.stores.cash.as_mut() {
                    *c = (cash, kafra);
                }
                let msg = match result {
                    0 => "Purchase complete.",
                    2 => "You don't have enough points.",
                    3 => "That item isn't for sale.",
                    4 => "You are carrying too much weight.",
                    5 => "You can't carry that many items.",
                    6 => "You can't buy anything right now.",
                    _ => "The purchase failed.",
                };
                self.hud.push_chat(ro_net::ChatKind::System, msg.into());
            }
            ShopBuyResult(code) => {
                let msg = match code {
                    0 => "Purchase complete.",
                    1 => "You don't have enough zeny.",
                    2 => "You are carrying too much weight.",
                    4 => "The shop is out of that item.",
                    5 => "The shop has changed; look again.",
                    6 => "The seller cannot take more zeny.",
                    7 => "You cannot carry that many items.",
                    _ => "The purchase failed.",
                };
                self.hud.push_chat(ro_net::ChatKind::System, msg.into());
            }
            Bank { balance, reply } => {
                if let Some((b, _)) = self.merchant.bank.as_mut() {
                    *b = Some(balance);
                }
                if let Some((deposit, reason)) = reply.filter(|(_, r)| *r != 0) {
                    let why = match (deposit, reason) {
                        (true, 1) | (false, 1) => "The bank is busy; try again.",
                        (true, 2) => "You don't have that much zeny.",
                        (false, 2) => "The bank doesn't hold that much.",
                        (_, 3) => "That would exceed the limit.",
                        _ => "The bank couldn't do that.",
                    };
                    self.hud.push_chat(ro_net::ChatKind::System, why.into());
                }
            }
            ViewEquip { name, job, items } => {
                let items: Vec<ro_game::items::InvItem> = items.into_iter().map(inv_item).collect();
                let ids: Vec<u32> = items.iter().map(|i| i.id).collect();
                self.load_icons_for(&ids);
                self.item_info.viewing = Some((name, job_title(job).to_string(), items));
            }
            ShowEquip(on) => self.show_equip = Some(on),
            RefineOpen => {
                self.refine.open = true;
                self.refine.selected = None;
                self.windows.inventory = true;
            }
            RefineInfo { index, blessing, options } => {
                let ids: Vec<u32> = options.iter().map(|o| o.material).collect();
                self.load_icons_for(&ids);
                self.refine.selected = Some((index, blessing, options));
            }
            RefineResult { result, index, refine } => {
                let name = self.inventory.items.get(&index).and_then(|i| self.item_db.as_ref().map(|d| d.name(i.id, true))).unwrap_or_else(|| "The item".into());
                let (msg, sound) = match result {
                    0 => (format!("Refine succeeded: +{refine} {name}!"), "bs_refinesuccess.wav"),
                    1 => (format!("Refine failed: {name} broke."), "bs_refinefailed.wav"),
                    2 => (format!("Refine failed: {name} went down to +{refine}."), "bs_refinefailed.wav"),
                    _ => (format!("Refine failed: {name} stays +{refine}."), "bs_refinefailed.wav"),
                };
                if let Some(it) = self.inventory.items.get_mut(&index) {
                    it.refine = refine as u8;
                }
                self.play_sound(sound);
                self.hud.push_chat(ro_net::ChatKind::System, msg);
            }
            SkillCooldown { skill, ms } => {
                self.cooldowns.insert(skill, std::time::Instant::now() + std::time::Duration::from_millis(ms as u64));
            }
            Emotion { id, kind } => {
                self.emotes.insert(id, (kind, 0.0));
            }
            WhisperResult(code) => {
                let why = match code {
                    0 => return,
                    1 => "That character is not online.",
                    2 => "Your whisper was ignored.",
                    _ => "That character is ignoring all whispers.",
                };
                self.hud.push_chat(ro_net::ChatKind::System, why.into());
            }
            Npc(d) => self.npc_dialog(d),
            Vanish { id, reason } => {
                if self.companions.elem.as_ref().is_some_and(|e| e.id == id) && reason != 0 {
                    self.companions.elem = None;
                }
                self.warps.remove(&id);
                if self.attack.is_some_and(|(t, _)| t == id) {
                    self.attack = None;
                }
                if id == self.account_id {
                    if reason == 1 {
                        if let Some(p) = self.player.as_mut() {
                            p.set_pose(Pose::Dead, f32::MAX);
                        }
                        self.hud.push_chat(ro_net::ChatKind::System, "You have died.".into());
                        self.dead = true;
                        self.attack = None;
                        self.walk_goal = None;
                    }
                } else if reason == 1 {
                    // Died: play the death animation, then remove.
                    if let Some(m) = self.units.get_mut(&id) {
                        m.walker.stop_at(m.walker.cell);
                        m.set_pose(Pose::Dead, f32::MAX);
                        m.dying = Some(1500.0);
                    }
                } else {
                    self.units.remove(&id);
                }
            }
            Act(act) => {
                if act.target != 0 && act.src != act.target {
                    self.aggro.insert(act.src, act.target);
                }
                self.act(act)
            }
            OutOfRange { target, target_cell, range } => {
                // Walk toward the target; the attack is re-sent once in reach.
                self.chasing = true;
                self.attack = Some((target, range.max(1)));
                self.request_walk(target_cell);
            }
            LookChange { id, kind, value, value2 } => self.look_change(id, kind, value, value2),
            StorageOpen => {
                self.social.storage.items.clear();
                self.social.storage.open = true;
                // Talking to the Kafra ends the dialog the storage was opened from.
                self.dialog = None;
                self.cutin = None;
            }
            StorageItems(list) => {
                for it in list {
                    self.social.storage.items.insert(it.index, inv_item(it));
                }
                self.load_icons();
            }
            StorageAdded(it) => {
                let it = inv_item(it);
                match self.social.storage.items.get_mut(&it.index) {
                    Some(s) if s.id == it.id => s.amount += it.amount,
                    _ => {
                        self.social.storage.items.insert(it.index, it);
                    }
                }
                self.load_icons();
            }
            StorageRemoved { index, amount } => {
                if let Some(s) = self.social.storage.items.get_mut(&index) {
                    s.amount = s.amount.saturating_sub(amount);
                    if s.amount == 0 {
                        self.social.storage.items.remove(&index);
                    }
                }
            }
            StorageCount { used, capacity } => self.social.storage.count = (used, capacity),
            StorageClosed => self.social.storage.open = false,
            ShopDeal(npc) => {
                self.dialog = None;
                self.social.shop = Some(social::Shop::Choose { npc });
                if let Some(buy) = self.auto_deal.take() {
                    self.send(ro_net::Command::Shop(ro_net::ShopRequest::Deal { npc, buy }));
                }
            }
            ShopBuyList(items) => {
                if let Some(id) = self.auto_buy.take() {
                    tracing::info!(id, "buying");
                    self.send(ro_net::Command::Shop(ro_net::ShopRequest::Buy(vec![(id, 1)])));
                }
                self.social.shop = Some(social::Shop::Buy { items, cart: Default::default() });
                self.load_shop_icons();
            }
            ShopSellList(offers) => {
                if let Some(id) = self.auto_sell.take() {
                    let list: Vec<(u16, u16)> = offers
                        .iter()
                        .filter_map(|o| self.inventory.items.get(&o.index).filter(|i| i.id == id).map(|i| (i.index, i.amount)))
                        .collect();
                    tracing::info!(?list, "selling");
                    self.send(ro_net::Command::Shop(ro_net::ShopRequest::Sell(list)));
                }
                self.social.shop = Some(social::Shop::Sell { offers, cart: Default::default() });
            }
            ShopResult { buy, ok } => {
                let msg = match (buy, ok) {
                    (true, true) => "Purchase complete.",
                    (true, false) => "You could not buy that (not enough zeny, weight or space).",
                    (false, true) => "Sale complete.",
                    (false, false) => "You could not sell that.",
                };
                self.hud.push_chat(ro_net::ChatKind::System, msg.into());
            }
            TradeRequested { name, .. } => self.social.prompts.push(social::Prompt::Trade { name }),
            TradeAnswer(result) => match result {
                3 => {
                    let partner = self.trade_partner.take().unwrap_or_default();
                    self.social.trade = Some(social::Trade { partner, ..Default::default() });
                }
                r => {
                    let why = match r {
                        0 => "too far away",
                        1 => "that character is not available",
                        2 => "they are already trading",
                        4 => "they declined",
                        _ => "they are busy",
                    };
                    self.hud.push_chat(ro_net::ChatKind::System, format!("Trade failed: {why}."));
                }
            },
            TradeItem { id, amount, .. } => {
                if let Some(t) = self.social.trade.as_mut() {
                    if id == 0 {
                        t.zeny_theirs = amount;
                    } else {
                        t.theirs.push((id, amount));
                    }
                }
                self.load_icons_for(&[id]);
            }
            TradeAdded { index, ok } => {
                if let (true, Some(t)) = (ok, self.social.trade.as_mut()) {
                    if let Some(it) = self.inventory.items.get(&index) {
                        let amount = self.trade_offers.remove(&index).unwrap_or(it.amount as i32);
                        t.ours.push((it.id, amount));
                    }
                }
            }
            TradeLocked { partner } => {
                if let Some(t) = self.social.trade.as_mut() {
                    if partner {
                        t.they_locked = true;
                    } else {
                        t.we_locked = true;
                    }
                }
            }
            TradeEnded { completed } => {
                self.trade_stage = 0;
                tracing::info!(completed, "trade ended");
                self.social.trade = None;
                let msg = if completed { "Trade completed." } else { "Trade cancelled." };
                self.hud.push_chat(ro_net::ChatKind::System, msg.into());
            }
            Party { name, members } => {
                let hp = self.social.party.take().map(|p| p.hp).unwrap_or_default();
                self.social.party = Some(social::Party { name, members, hp });
            }
            PartyMember { name, member } => {
                let p = self.social.party.get_or_insert_with(|| social::Party { name: name.clone(), ..Default::default() });
                p.members.retain(|m| m.account_id != member.account_id);
                p.members.push(member);
            }
            PartyLeft { account_id, name } => {
                if account_id == self.account_id {
                    self.social.party = None;
                    self.hud.push_chat(ro_net::ChatKind::System, "You left the party.".into());
                } else if let Some(p) = self.social.party.as_mut() {
                    p.members.retain(|m| m.account_id != account_id);
                    self.hud.push_chat(ro_net::ChatKind::Party, format!("{name} left the party."));
                }
            }
            PartyHp { account_id, hp, max_hp } => {
                if let Some(p) = self.social.party.as_mut() {
                    p.hp.insert(account_id, (hp, max_hp));
                }
            }
            PartyInvite { party, name } => self.social.prompts.push(social::Prompt::Party { party, name }),
            PartyCreated { ok } => {
                let msg = if ok { "Party created." } else { "Could not create the party (name taken, or already in one)." };
                self.hud.push_chat(ro_net::ChatKind::System, msg.into());
            }
            Guild { id, name } => self.social.guild = (id != 0 && !name.is_empty()).then_some(name),
            GuildBelong { id, master, name } => {
                (self.guild.id, self.guild.master) = (id, master);
                if id == 0 {
                    (self.guild.info, self.guild.members) = (None, vec![]);
                } else {
                    self.guild.name = name.clone();
                    self.guild.needs_refresh = true;
                }
                self.social.guild = (id != 0 && !name.is_empty()).then_some(name);
            }
            GuildCreated(result) => {
                let text = match result {
                    0 => "Guild created.",
                    1 => "You are already in a guild.",
                    2 => "That guild name is already taken.",
                    3 => "You need an Emperium to found a guild.",
                    _ => "Could not create the guild.",
                };
                self.hud.push_chat(ro_net::ChatKind::System, text.into());
                if result == 0 {
                    self.guild.open = true;
                    self.guild.needs_refresh = true;
                }
            }
            GuildInfo(info) => {
                self.guild.name = info.name.clone();
                if let Some(p) = self.player.as_mut() {
                    p.guild = info.id;
                }
                // Fetch the emblem when it is new or changed.
                if info.emblem != 0 && self.guild.emblem.as_ref().is_none_or(|e| e.0 != info.emblem) {
                    self.send(ro_net::Command::Guild(ro_net::GuildRequest::Emblem(info.id)));
                }
                self.guild.info = Some(info);
            }
            GuildEmblem { guild, version, data } => {
                let ctx = self.egui_ctx.clone();
                self.emblems.insert(guild, guild::decode_emblem(&ctx, guild, version, &data));
                self.guild.set_emblem(&ctx, guild, version, &data);
            }
            GuildMembers(m) => self.guild.members = m,
            GuildRankNames(names) => {
                for (id, name) in names {
                    match self.guild.ranks.iter_mut().find(|r| r.id == id) {
                        Some(r) => r.name = name,
                        None => self.guild.ranks.push(ro_net::Position { id, name, ..Default::default() }),
                    }
                }
                self.guild.ranks.sort_by_key(|r| r.id);
            }
            GuildRankInfo(info) => {
                for (id, mode, tax) in info {
                    if let Some(r) = self.guild.ranks.iter_mut().find(|r| r.id == id) {
                        (r.mode, r.tax) = (mode, tax);
                    }
                }
            }
            GuildRanksChanged(list) => {
                for p in list {
                    match self.guild.ranks.iter_mut().find(|r| r.id == p.id) {
                        Some(r) => *r = p,
                        None => self.guild.ranks.push(p),
                    }
                }
                self.guild.ranks.sort_by_key(|r| r.id);
            }
            GuildRankMoves(moves) => {
                for (_, char_id, position) in moves {
                    if let Some(m) = self.guild.members.iter_mut().find(|m| m.char_id == char_id) {
                        m.position = position;
                    }
                }
            }
            GuildSkills { points, skills } => (self.guild.skill_points, self.guild.skills) = (points, skills),
            GuildNotice { subject, text } => self.guild.notice = (subject, text),
            GuildInvited { guild, name } => self.social.prompts.push(social::Prompt::Guild { guild, name }),
            GuildInviteResult(r) => {
                let text = match r {
                    0 => "That player is already in a guild.",
                    1 => "Your guild invitation was declined.",
                    2 => "A new member joined the guild.",
                    3 => "The guild is full.",
                    _ => "Guild invitation failed.",
                };
                self.hud.push_chat(ro_net::ChatKind::Guild, text.into());
                if r == 2 {
                    self.guild.needs_refresh = true;
                }
            }
            GuildMemberState { char_id, online, .. } => {
                if let Some(m) = self.guild.members.iter_mut().find(|m| m.char_id == char_id) {
                    m.online = online;
                }
            }
            GuildMemberGone { char_id, reason, expelled } => {
                let who = self.guild.members.iter().find(|m| m.char_id == char_id).map_or_else(|| "A member".to_string(), |m| m.name.clone());
                let verb = if expelled { "was expelled from" } else { "left" };
                let why = if reason.trim().is_empty() { String::new() } else { format!(" ({})", reason.trim()) };
                self.hud.push_chat(ro_net::ChatKind::Guild, format!("{who} {verb} the guild{why}."));
                self.guild.members.retain(|m| m.char_id != char_id);
            }
            Friends(list) => self.social.friends = list,
            FriendOnline { account_id, online, .. } => {
                if let Some(f) = self.social.friends.iter_mut().find(|f| f.account_id == account_id) {
                    f.online = online;
                }
            }
            FriendRequested { account_id, char_id, name } => self.social.prompts.push(social::Prompt::Friend { account_id, char_id, name }),
            FriendAdded { ok, friend } => {
                if ok {
                    self.hud.push_chat(ro_net::ChatKind::System, format!("{} is now your friend.", friend.name));
                    self.social.friends.retain(|f| f.account_id != friend.account_id);
                    self.social.friends.push(friend);
                }
            }
            FriendRemoved { account_id, .. } => self.social.friends.retain(|f| f.account_id != account_id),
            InventoryStart => self.inventory.items.clear(),
            Items(list) => {
                for it in list {
                    self.inventory.set(inv_item(it));
                }
                self.load_icons();
            }
            ItemAdded(it) => {
                tracing::debug!(id = it.id, amount = it.amount, "item added");
                let name = self.item_db.as_ref().map_or_else(|| format!("Item #{}", it.id), |d| d.name(it.id, it.identified));
                self.hud.push_chat(ro_net::ChatKind::System, format!("You got {name} ({}).", it.amount));
                self.inventory.add(inv_item(it));
                self.load_icons();
            }
            ItemRemoved { index, amount } => self.inventory.remove(index, amount),
            ItemUsed { index, amount, user, ok } => {
                if ok && user == self.account_id {
                    self.inventory.set_amount(index, amount);
                }
            }
            Equipped { index, slots, ok, .. } => {
                tracing::debug!(index, slots, ok, "equip");
                if ok {
                    self.inventory.set_equipped(index, slots);
                } else {
                    self.hud.push_chat(ro_net::ChatKind::System, "You cannot equip this item.".into());
                }
            }
            Unequipped { index, ok, .. } => {
                if ok {
                    self.inventory.set_equipped(index, 0);
                }
            }
            FloorItem(f) => {
                tracing::debug!(id = f.id, item = f.item, x = f.x, y = f.y, "floor item");
                let actor = self.item_actor(f.item, f.identified);
                self.floor.insert(f.id, (f, actor));
            }
            FloorItemGone(id) => {
                self.floor.remove(&id);
                if self.pending_pickup == Some(id) {
                    self.pending_pickup = None;
                }
            }
            CardTargets(equips) => {
                if let Some((_, list)) = self.windows.card_pick.as_mut() {
                    *list = equips;
                }
            }
            CardInserted { equip, card, ok } => {
                // The server removes the card itself; the equipment's slots are ours to update.
                let card_id = self.inventory.items.get(&card).map(|c| c.id);
                if let (true, Some(id), Some(e)) = (ok, card_id, self.inventory.items.get_mut(&equip)) {
                    if let Some(slot) = e.cards.iter_mut().find(|c| **c == 0) {
                        *slot = id;
                    }
                }
                let msg = if ok { "The card was inserted." } else { "The card could not be inserted." };
                self.hud.push_chat(ro_net::ChatKind::System, msg.to_string());
            }
            Stats(s) => self.stats = Some(s),
            StatRaised { stat, ok, value } => {
                if let (true, Some(s), 13..=18) = (ok, self.stats.as_mut(), stat) {
                    // The ack carries one byte; past 255 the full value follows in 0x0141.
                    if value < 255 {
                        s.base[(stat - 13) as usize] = value as u16;
                    }
                }
            }
            StatBonus { stat, base, plus } => {
                if let (Some(s), 13..=18) = (self.stats.as_mut(), stat) {
                    s.base[(stat - 13) as usize] = base.clamp(0, u16::MAX as i32) as u16;
                    s.plus[(stat - 13) as usize] = plus as i16;
                }
            }
            StatCost { stat, cost } => {
                if let (Some(s), 32..=37) = (self.stats.as_mut(), stat) {
                    s.cost[(stat - 32) as usize] = cost;
                }
            }
            Skills(list) => {
                self.skills = list;
                self.load_skill_icons();
            }
            SkillUpdate { id, level, sp, range, upgradable } => {
                if let Some(s) = self.skills.iter_mut().find(|s| s.id == id) {
                    (s.level, s.sp, s.range, s.upgradable) = (level, sp, range, upgradable);
                }
            }
            Hotkeys { row, keys } => {
                let n = self.hotkeys.len();
                self.hotkeys = keys;
                self.hotkeys.resize(n.max(self.hotkeys.len()), Default::default());
                self.hotkey_row = row % HOTKEY_ROWS;
            }
            Casting(c) => {
                let cell = (c.cell.0 as i32, c.cell.1 as i32);
                let target_cell = self.mob_mut(c.target).map(|m| m.walker.cell).or((c.target == 0).then_some(cell));
                if let Some(m) = self.mob_mut(c.src) {
                    if let Some(t) = target_cell {
                        m.face(t);
                    }
                    if c.ms > 0 {
                        m.set_pose(Pose::Cast, c.ms as f32);
                    }
                }
                if c.ms > 0 {
                    self.casts.insert(c.src, (0.0, c.ms as f32));
                    // The magic circle under the caster while chanting.
                    if let (Some(pos), Some(r), Some(vfs)) = (self.unit_pos(c.src), self.renderer.as_mut(), self.vfs.as_ref()) {
                        if let Some(tex) = self.fx.texture(r, vfs, "magic_target.tga") {
                            let shape = ro_render::Shape::Disc { radius: 1.6, spin: 0.25 };
                            r.procs.push(ro_render::ProcFx { tex, shape, pos, color: [1.0, 1.0, 1.0], time_ms: 0.0, duration_ms: c.ms as f32 });
                        }
                    }
                }
            }
            Fed { homun, ok, item } => {
                let name = self.item_db.as_ref().map_or_else(|| format!("Item #{item}"), |d| d.name(item as u32, true));
                let who = if homun { "homunculus" } else { "pet" };
                let text = if ok { format!("Your {who} ate {name}.") } else { format!("You don't have any {name} to feed your {who}.") };
                self.hud.push_chat(ro_net::ChatKind::System, text);
            }
            PetAct { id, value } => {
                if value < 10_000 {
                    self.handle(Emotion { id, kind: value as u8 });
                } else if let Some(name) = self.units.get(&id).map(|m| m.name.clone()) {
                    let names = &self.class_names;
                    if let Some(line) = self.pet_talk.line(value, |c| names.get(&c).cloned()) {
                        self.hud.push_chat(ro_net::ChatKind::Public, format!("{name} : {line}"));
                    }
                }
            }
            CastCancel(id) => {
                self.casts.remove(&id);
                if let Some(m) = self.mob_mut(id) {
                    if matches!(m.pose, Some((Pose::Cast, _))) {
                        m.pose = None;
                    }
                }
            }
            SkillFailed { skill, cause, amount, item } => {
                let name = self.skills.iter().find(|s| s.id == skill).map(|s| self.skill_names.display(&s.name)).unwrap_or_else(|| "Skill".into());
                let why = match cause {
                    1 => "not enough SP",
                    2 => "not enough HP",
                    3 => "missing required items",
                    4 => "still on cooldown",
                    5 => "not enough zeny",
                    6 => "wrong weapon equipped",
                    7 => "requires a Red Gemstone",
                    8 => "requires a Blue Gemstone",
                    9 => "carrying too much weight",
                    11 => "invalid target",
                    13 => "requires Holy Water",
                    15 | 22 => "already in effect",
                    16 => "requires another skill first",
                    23 => "conditions not met",
                    26 => "can't be used here",
                    28 => "too close to a wall",
                    32 => "no poison applied to the weapon",
                    33 | 50 => "requires a Mado Gear",
                    34 => "requires Kunai",
                    35 => "can only target players",
                    43 => "requires a Guillotine poison",
                    57 => "requires a cart",
                    71 => "requires an item",
                    72 | 95 => "requires a weapon",
                    73 => "must follow a combo",
                    74 | 96 => "not enough spirit spheres",
                    83 => "there are NPCs too close",
                    84 | 90 => "not enough bullets",
                    87 => "can't be used on this map",
                    89 => "a shop is already open",
                    91 => "requires arrows",
                    92 => "requires a party",
                    93 => "requires party leader",
                    97 => "requires Weapon Blocking",
                    98 => "too many traps",
                    100 => "not enough AP",
                    110 => "requires a shield",
                    _ => "failed",
                };
                self.casts.remove(&self.account_id);
                if let Some(p) = self.player.as_mut() {
                    p.pose = None;
                }
                let why = if cause == 71 && item != 0 {
                    let item_name = self.item_db.as_ref().map_or_else(|| format!("Item #{item}"), |d| d.name(item, true));
                    format!("requires {amount} {item_name}")
                } else {
                    why.to_string()
                };
                self.hud.push_chat(ro_net::ChatKind::System, format!("{name}: {why}."));
            }
            SkillUsed { skill, src, target } => {
                self.casts.remove(&src);
                let target_cell = self.mob_mut(target).map(|m| m.walker.cell);
                if let Some(m) = self.mob_mut(src) {
                    if let (Some(t), true) = (target_cell, src != target) {
                        m.face(t);
                    }
                    m.set_pose(Pose::Attack, 400.0);
                }
                if skill == 406 {
                    self.meteor_assault(src);
                }
                self.skill_fx(skill, self.unit_pos(target), false);
            }
            SkillHit { skill, act } => {
                let target = act.target;
                // Grimtooth: a line of spikes erupting from the caster to the target.
                if skill == 137 {
                    if let (Some(from), Some(to)) = (self.unit_pos(act.src), self.unit_pos(target)) {
                        self.spike_line(from, to);
                    }
                }
                // Soul Breaker: a ball of energy hurled at the target.
                if skill == 379 {
                    if let (Some(from), Some(to)) = (self.unit_pos(act.src), self.unit_pos(target)) {
                        let shape = ro_render::Shape::Slash { travel: to - from, size: (1.0, 2.0) };
                        self.proc_fx("purpleslash.tga", shape, from, [1.0, 1.0, 1.0], 500.0);
                    }
                }
                if skill == 406 {
                    self.meteor_assault(act.src);
                }
                self.act(act);
                self.skill_fx(skill, self.unit_pos(target), true);
            }
            SkillAt { skill, src, cell } => {
                self.casts.remove(&src);
                if let Some(m) = self.mob_mut(src) {
                    m.face((cell.0 as i32, cell.1 as i32));
                    m.set_pose(Pose::Attack, 400.0);
                }
                self.skill_fx(skill, self.cell_pos(cell), true);
            }
            UnitOption { id, option } => {
                let me = id == self.account_id;
                if let Some(m) = self.mob_mut(id) {
                    m.actor.alpha = option_alpha(option, me);
                }
            }
            GroundEffect { id, cell, kind } => {
                tracing::debug!(id, ?cell, kind = format_args!("{kind:#x}"), "ground effect");
                if let (Some(name), Some(pos)) = (fx::ground_str(kind), self.cell_pos(cell)) {
                    self.stop_fx(id);
                    self.play_fx_tagged(name, pos, id);
                } else {
                    tracing::debug!(kind = format_args!("{kind:#x}"), "no visual for ground effect");
                }
            }
            GroundEffectGone(id) => self.stop_fx(id),
            // EFST_SKE on ourselves: the server's night (on) or day (off).
            StatusIcon { id, kind: 160, on, .. } if id == self.account_id => {
                tracing::info!(night = on, "day/night");
                if let Some(r) = self.renderer.as_mut() {
                    r.night = on;
                }
            }
            StatusIcon { id, kind, on, remain_ms } if id == self.account_id => self.buffs.set(kind, on, remain_ms),
            StatusIcon { .. } => {}
            Resurrected(id) if id == self.account_id => self.revive(),
            Resurrected(id) => {
                if let Some(m) = self.units.get_mut(&id) {
                    m.pose = None;
                    m.dying = None;
                }
            }
            Sound { name, act } => {
                // 0 plays once, 1 loops (played once here), 2 stops.
                if act != 2 {
                    if let (Some(a), Some(v)) = (self.audio.as_ref(), self.vfs.as_ref()) {
                        a.play(v, &name);
                    }
                }
            }
            Bgm(name) => {
                if let (Some(a), Some(v)) = (self.audio.as_mut(), self.vfs.as_ref()) {
                    a.play_bgm(v, &name);
                }
            }
            Viewpoint { npc, kind, x, y, id, color } => {
                if kind == 2 {
                    self.viewpoints.remove(&(npc, id));
                } else {
                    let rgb = egui::Color32::from_rgb((color >> 16) as u8, (color >> 8) as u8, color as u8);
                    // Kind 0 marks blink for a while; 1 stays until removed or the map changes.
                    let until = (kind == 0).then(|| std::time::Instant::now() + std::time::Duration::from_secs(15));
                    self.viewpoints.insert((npc, id), ((x as f32, y as f32), rgb, until));
                }
            }
            NpcSprite { id, class } => {
                if let Some(m) = self.units.remove(&id) {
                    let (x, y) = (m.walker.cell.0 as u16, m.walker.cell.1 as u16);
                    let info = ro_net::UnitInfo {
                        object_type: m.kind,
                        id,
                        speed: m.walker.speed as i16,
                        option: 0,
                        job: class as i16,
                        hair: 0,
                        hair_color: 0,
                        look: Default::default(),
                        sex: 0,
                        name: m.name.clone(),
                        pos: ro_net::UnitPos::At { x, y, dir: m.walker.dir as u8 },
                        guild: m.guild,
                        emblem: 0,
                    };
                    self.unit(info);
                }
            }
            MsgValue { msg, value } => {
                let v = value.to_string();
                if let Some(text) = msgstr::get(msg as usize, &[&v]) {
                    self.hud.push_chat(ro_net::ChatKind::System, text);
                }
            }
            MsgColor { msg, .. } => {
                if let Some(text) = msgstr::get(msg as usize, &[]) {
                    self.hud.push_chat(ro_net::ChatKind::System, text);
                }
            }
            MsgSkill { skill, msg } => {
                if let Some(text) = msgstr::get(msg as usize, &[]) {
                    let name = self.skill_names.internal(skill).map(|n| self.skill_names.display(n)).unwrap_or_else(|| format!("Skill #{skill}"));
                    self.hud.push_chat(ro_net::ChatKind::System, format!("[{name}] {text}"));
                }
            }
            RemoveEffect { .. } => {}
            Notice(text) => self.hud.push_chat(ro_net::ChatKind::System, text),
            StyleResult(failed) => self.stylist.result(failed),
            MergeOpen(list) => self.merge.open = Some(list),
            Merged { reason, .. } => {
                let msg = match reason { 0 => "The items were merged.", 1 => "Those items can't be merged.", _ => "You have too many of that item to merge." };
                self.hud.push_chat(ro_net::ChatKind::System, msg.into());
            }
            GuildRelations(list) => self.guild.relations = list,
            GuildBans(list) => self.guild.bans = list,
            GuildBroken(r) => {
                let text = match r {
                    0 => {
                        self.guild.id = 0;
                        self.guild.master = false;
                        self.guild.name.clear();
                        self.guild.info = None;
                        self.guild.members.clear();
                        self.guild.bans.clear();
                        self.guild.relations.clear();
                        if let Some(p) = self.player.as_mut() {
                            p.guild = 0;
                            p.titles = None;
                        }
                        "The guild has been disbanded."
                    }
                    1 => "The guild name you entered does not match.",
                    _ => "You cannot disband the guild while it still has members.",
                };
                self.hud.push_chat(ro_net::ChatKind::Guild, text.into());
            }
            UnitGuild { id, guild } => {
                if let Some(m) = self.mob_mut(id) {
                    m.guild = guild;
                }
                if guild != 0 && !self.emblems.contains_key(&guild) {
                    self.emblems.insert(guild, None);
                    self.send(ro_net::Command::Guild(ro_net::GuildRequest::Emblem(guild)));
                }
            }
            AllyRequest { inviter, guild } => self.guild.ally_offer = Some((inviter, guild)),
            RelationNotice(text) => self.hud.push_chat(ro_net::ChatKind::Guild, text),
            RelationRemoved { guild, relation } => self.guild.relations.retain(|r| !(r.0 == relation && r.1 == guild)),
            StartCapture => {
                self.capturing = true;
                self.hud.push_chat(ro_net::ChatKind::System, "Click the monster you want to tame.".into());
            }
            Captured(ok) => {
                let msg = if ok { "You tamed it! Look for its egg in your inventory." } else { "The taming failed." };
                self.hud.push_chat(ro_net::ChatKind::System, msg.into());
            }
            Turned { id, dir } => {
                if let Some(m) = self.mob_mut(id) {
                    if !m.walker.is_walking() {
                        m.walker.dir = (dir % 8) as usize;
                    }
                }
            }
            MonsterInfo { class, level, size, hp, def, race, mdef, element, mods } => {
                let name = self.units.values().find(|m| m.class == class as i16).map(|m| m.name.clone()).or_else(|| self.class_names.get(&(class as i16)).cloned()).unwrap_or_else(|| format!("Monster #{class}"));
                self.sense.info = Some(sense::MonsterInfo { name, level, size, hp, def, race, mdef, element, mods });
            }
            CoupleName(name) => {
                let line = if name.is_empty() { "Your partner can't be called right now.".to_string() } else { format!("Calling your partner, {name}.") };
                self.hud.push_chat(ro_net::ChatKind::System, line);
            }
            BladeStop { src, target, active } => {
                // Both stand locked in place while it lasts.
                if active {
                    for id in [src, target] {
                        if let Some(m) = self.mob_mut(id) {
                            let cell = m.walker.cell;
                            m.walker.stop_at(cell);
                        }
                    }
                }
            }
            ComboDelay { id, ms } => tracing::debug!(id, ms, "combo window"),
            GuildTitle { id, position } => {
                if let Some(t) = self.mob_mut(id).and_then(|m| m.titles.as_mut()) {
                    t.2 = position;
                }
            }
            CellType { x, y, kind, map } => {
                if let Some(w) = self.world.as_mut().filter(|w| map.trim_end_matches(".gat") == w.name) {
                    if let Some(g) = w.gat.as_mut() {
                        g.set_cell(x as i32, y as i32, kind as u32);
                    }
                }
            }
            PartyMemberInfo { account_id, job, level } => {
                if let Some(m) = self.social.party.iter_mut().flat_map(|p| p.members.iter_mut()).find(|m| m.account_id == account_id) {
                    m.job = job;
                    m.level = level;
                }
            }
            Spirits { id, count, kind } => {
                if count == 0 {
                    self.spirits.remove(&id);
                } else {
                    self.spirits.insert(id, (count, kind));
                }
            }
            SkillAdded(mut s) => {
                tracing::debug!(id = s.id, level = s.level, "skill added");
                match self.skills.iter_mut().find(|k| k.id == s.id) {
                    Some(k) => {
                        if s.name.is_empty() {
                            s.name = std::mem::take(&mut k.name);
                        }
                        *k = s;
                    }
                    None => self.skills.push(s),
                }
                self.load_skill_icons();
            }
            SkillRemoved(id) => self.skills.retain(|k| k.id != id),
            SwitchList(list) => {
                for it in self.inventory.items.values_mut() {
                    it.switch = 0;
                }
                for (index, pos) in list {
                    if let Some(it) = self.inventory.items.get_mut(&index) {
                        it.switch = pos;
                    }
                }
            }
            SwitchChanged { index, location, ok } => {
                if ok {
                    if let Some(it) = self.inventory.items.get_mut(&index) {
                        it.switch = location;
                    }
                } else {
                    self.hud.push_chat(ro_net::ChatKind::System, "That item can't go in the switch set.".into());
                }
            }
            SwitchDone(ok) => {
                if !ok {
                    self.hud.push_chat(ro_net::ChatKind::System, "The equipment switch failed.".into());
                }
            }
            Reputation(list) => self.reputation.points.extend(list),
            ReputationOpen => {
                self.reputation.load(self.vfs.as_ref());
                self.reputation.open = true;
            }
            Config { kind, on } => self.game_opts.config(kind, on),
            DenyPartyInvites(d) => self.game_opts.deny_invites = d,
            PartyOptions { exp_share, pickup_share, item_share } => self.game_opts.party = Some((exp_share, pickup_share, item_share)),
            AttackRange(r) => self.attack_range = r.max(1) as i32,
            MapProperty { flags, .. } => {
                self.map_flags = flags;
            }
            OverweightPercent(p) => self.hud.status.overweight_pct = p,
            Ranking { kind, list, mine } => {
                self.ranking.open = true;
                (self.ranking.kind, self.ranking.list, self.ranking.mine) = (kind, list, mine);
                for cid in self.ranking.missing() {
                    self.send(ro_net::Command::ReqName(cid));
                }
            }
            CharName { cid, name } => {
                self.ranking.names.insert(cid, name);
            }
            SearchOpen(uses) => self.store_search.opened(uses),
            SearchResults { first, more, uses, found } => self.store_search.results(first, more, uses, found),
            SearchFailed(r) => self.store_search.failed(r),
            SearchAt(x, y) => {
                self.viewpoints.insert((0, 250), ((x as f32, y as f32), egui::Color32::from_rgb(255, 200, 60), Some(std::time::Instant::now() + std::time::Duration::from_secs(30))));
                self.hud.push_chat(ro_net::ChatKind::System, format!("The shop is at ({x}, {y}), marked on the minimap."));
            }
            BossInfo { kind, x, y, hours, minutes, name } => {
                let gold = egui::Color32::from_rgb(255, 60, 60);
                match kind {
                    0 => {
                        self.viewpoints.remove(&(0, 251));
                        self.hud.push_chat(ro_net::ChatKind::System, "You cannot find any trace of a Boss Monster in this area.".into());
                    }
                    1 | 2 => {
                        self.viewpoints.insert((0, 251), ((x as f32, y as f32), gold, None));
                        if kind == 2 {
                            self.hud.push_chat(ro_net::ChatKind::System, format!("{name} is present in this area."));
                        }
                    }
                    _ => {
                        self.viewpoints.remove(&(0, 251));
                        self.hud.push_chat(ro_net::ChatKind::System, format!("{name} will appear in approximately {hours} hours {minutes} minutes."));
                    }
                }
            }
            DressRoom => {
                let (hair, color) = self.player.as_ref().and_then(|p| p.look.as_ref()).map_or((1, 0), |l| (l.hair, l.hair_color));
                self.stylist.open(hair, color, 0);
                self.stylist.dressing = true;
            }
            ReformOpen(tool) => self.reform.open(self.vfs.as_ref(), tool),
            LapineOpen { upgrade, trigger } => self.lapine.open(self.vfs.as_ref(), upgrade, trigger),
            LapineResult { upgrade, result } => {
                if let Some(a) = self.lapine.result(upgrade, result) {
                    self.lapine_action(a);
                }
                let msg = self.lapine.status.clone();
                self.hud.push_chat(ro_net::ChatKind::System, msg);
            }
            ItemChanged { index, cards, refine, grade } => {
                if let Some(it) = self.inventory.items.get_mut(&index) {
                    (it.cards, it.refine, it.grade) = (cards, refine, grade);
                }
            }
            ReformResult { result, .. } => self.reform.result(result == 0),
            UiOpenV3 { kind: 10, data } => self.enchant.open(self.vfs.as_ref(), data),
            UiOpenV3 { .. } => {}
            EnchantResult { msg, item } => {
                if msg == 3857 && item != 0 {
                    if let Some((index, slot)) = self.enchant.filled_slot(&self.inventory.items) {
                        if let Some(it) = self.inventory.items.get_mut(&index) {
                            it.cards[slot] = item;
                        }
                    }
                }
                self.enchant.result(msg == 3857, item, self.item_db.as_ref());
            }
            HatEffect { id, on, effects } => {
                for e in effects {
                    let key = (id, e);
                    if on && !self.hat_fx.contains_key(&key) {
                        let Some(name) = self.hat_files.get(&e).copied() else { continue };
                        let tag = 0x7000_0000 + self.hat_fx.len() as u32 + 1 + (id & 0xffff) * 64;
                        if let Some(pos) = self.unit_pos(id) {
                            self.play_fx_tagged(name, pos, tag);
                            self.hat_fx.insert(key, tag);
                        }
                    } else if !on {
                        if let Some(tag) = self.hat_fx.remove(&key) {
                            self.stop_fx(tag);
                        }
                    }
                }
            }
            NameAll { id, party, guild, rank } => {
                let titles = (party, guild, rank);
                if id == self.account_id {
                    if let Some(p) = self.player.as_mut() {
                        p.titles = Some(titles);
                    }
                } else if let Some(m) = self.units.get_mut(&id) {
                    m.titles = Some(titles);
                }
            }
            AutorunSkill { id, target, level } => {
                // Self (and passive-flagged) skills go off at once; others wait for a target click.
                if target & 4 != 0 || target & (1 | 2 | 16 | 32) == 0 {
                    self.use_skill_on(id, level, self.account_id);
                } else {
                    self.aiming = Some((id, level));
                }
            }
            Identified(index) => {
                if let Some(it) = self.inventory.items.get_mut(&index) {
                    it.identified = true;
                    let name = self.item_db.as_ref().map_or_else(|| format!("Item #{}", it.id), |d| d.name(it.id, true));
                    self.hud.push_chat(ro_net::ChatKind::System, format!("Identified: {name}."));
                }
            }
            ArrowEquipped(index) => {
                for it in self.inventory.items.values_mut() {
                    it.equipped &= !0x8000;
                }
                if let Some(it) = self.inventory.items.get_mut(&index) {
                    it.equipped |= 0x8000;
                }
            }
            Mvp(id) => {
                if let Some(pos) = self.unit_pos(id) {
                    self.play_fx("mvp", pos);
                }
                let who = if id == self.account_id { self.hud.status.name.clone() } else { self.units.get(&id).map_or_else(|| "Someone".into(), |m| m.name.clone()) };
                self.hud.push_chat(ro_net::ChatKind::Broadcast, format!("{who} is the MVP!"));
            }
            MvpItem(item) => {
                let name = self.item_db.as_ref().map_or_else(|| format!("Item #{item}"), |d| d.name(item, true));
                self.hud.push_chat(ro_net::ChatKind::System, format!("MVP reward: {name}."));
            }
            MvpExp(exp) => self.hud.push_chat(ro_net::ChatKind::System, format!("MVP reward: {exp} experience.")),
            MvpDropped => self.hud.push_chat(ro_net::ChatKind::System, "You are the MVP, but the reward dropped to the floor because you are overweight.".into()),
            MemberPos { id, x, y, guild } => {
                if x == 0xffff || y == 0xffff {
                    self.member_pos.remove(&id);
                } else {
                    self.member_pos.insert(id, ((x as f32, y as f32), guild));
                }
            }
            EffectValue { id, effect, num } => {
                /// rAthena's EF_GREEN_NUMBER: the number is a heal.
                const EF_GREEN_NUMBER: u32 = 657;
                if effect == EF_GREEN_NUMBER {
                    if let Some(pos) = self.unit_pos(id) {
                        self.floaters.push((pos, num.to_string(), egui::Color32::from_rgb(110, 255, 110), 0.0));
                    }
                } else {
                    self.handle(ScriptEffect { id, effect });
                }
            }
            SpecialPopup(id) => {
                let popup = self.vfs.as_ref().and_then(|v| v.read(r"System\spopup.lub").ok().flatten()).and_then(|d| ro_formats::lub::eval(&d).ok()).and_then(|c| {
                    let t = c.globals.get("popuptbl")?.clone();
                    let e = &c.entries(&t).iter().find(|(k, _)| k.as_number() == Some(id as f64))?.1;
                    let ui = c.field(e, "uitype").and_then(|v| v.as_number()).unwrap_or(0.0) as u8;
                    let rgb = c.field(e, "textcolor").and_then(|v| v.as_number()).unwrap_or(16763904.0) as u32;
                    let lines: Vec<String> = c.field(e, "Description").map_or(vec![], |d| c.entries(d).iter().filter_map(|(_, v)| v.as_str().map(str::to_string)).collect());
                    Some((ui, egui::Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8), lines))
                });
                match popup {
                    Some((ui, color, lines)) => {
                        // uitype: 0 chat, 1 floating, 2 both.
                        if ui != 1 {
                            for l in &lines {
                                self.hud.push_chat(ro_net::ChatKind::System, l.clone());
                            }
                        }
                        if ui != 0 {
                            self.hud.banner = Some((lines, color, std::time::Instant::now()));
                        }
                    }
                    None => tracing::debug!(id, "special popup not in spopup.lub"),
                }
            }
            SkillMsg(id) => {
                let text = match id {
                    0x15 => "All abnormal status effects have been removed.",
                    0x16 => "You will be immune to abnormal status effects for the next minute.",
                    0x17 => "Your Max HP will stay increased for the next minute.",
                    0x18 => "Your Max SP will stay increased for the next minute.",
                    0x19 => "All of your stats will stay increased for the next minute.",
                    0x1c => "Your weapon will remain blessed with Holy power for the next minute.",
                    0x1d => "Your armor will remain blessed with Holy power for the next minute.",
                    0x1e => "Your Defense will stay increased for the next 10 seconds.",
                    0x1f => "Your Attack strength will stay increased for the next minute.",
                    0x20 => "Your Accuracy and Flee Rate will stay increased for the next minute.",
                    0x28 => "Full Strip failed because of coating.",
                    _ => "",
                };
                if !text.is_empty() {
                    self.hud.push_chat(ro_net::ChatKind::System, text.into());
                }
            }
            ScriptEffect { id, effect } => {
                if let Some(pos) = self.unit_pos(id) {
                    match (fx::script_effect(effect), fx::ef_skill(effect)) {
                        (Some(name), _) => self.play_fx(name, pos),
                        (None, Some(skill)) => {
                            tracing::debug!(effect, skill, "script effect via skill");
                            self.skill_fx(skill, Some(pos), false);
                        }
                        _ => tracing::debug!(effect, "no visual for script effect"),
                    }
                }
            }
            UnitEffect { id, effect } => {
                if let (Some(name), Some(pos)) = (fx::misc_effect(effect), self.unit_pos(id)) {
                    self.play_fx(name, pos);
                }
            }
            MonsterHp { id, hp, max_hp } => {
                if let Some(m) = self.units.get_mut(&id) {
                    m.hp = Some((hp, max_hp));
                }
            }
            MapMove { map, x, y } => {
                // Respawning at the save point arrives as a map move.
                self.viewpoints.clear();
                self.member_pos.clear();
                self.revive();
                self.units.clear();
                self.floor.clear();
                self.warps.clear();
                if self.world.as_ref().is_none_or(|w| w.name != map) {
                    self.enter_map(&map);
                }
                if let Some(p) = self.player.as_mut() {
                    p.walker.stop_at((x as i32, y as i32));
                }
            }
            // Back at character select: clear the world; the screens take over again.
            LeftGame => {
                self.units.clear();
                self.floor.clear();
                self.warps.clear();
                self.player = None;
                self.world = None;
                self.walk_goal = None;
                self.attack = None;
                self.dialog = None;
                self.esc_menu = false;
                let user = self.settings.last_id.clone();
                let mut front = screens::Front::new(user, true);
                front.screen = screens::Screen::Loading;
                self.front = Some(front);
                if let (Some(a), Some(v)) = (self.audio.as_mut(), self.vfs.as_ref()) {
                    a.title(v);
                }
            }
            // Login and character-select replies (handled by the screens above).
            LoginFailed(_) | Servers(_) | Characters { .. } | CharCreated(_) | CharCreateFailed(_) | CharDeleteStarted { .. } | CharDeleted { .. } | CharDeleteCanceled { .. } => {}
            Disconnected(reason) => {
                tracing::error!("disconnected: {reason}");
                self.status = Some(format!("disconnected: {reason}"));
            }
        }
    }

    fn send(&mut self, cmd: ro_net::Command) {
        // Remember how much of each item we offer in a trade (the ack only names the slot).
        if let ro_net::Command::Trade(ro_net::TradeRequest::Add { index, amount }) = &cmd {
            self.trade_offers.insert(*index, *amount);
        }
        if let Some((tx, _)) = self.net.as_ref() {
            let _ = tx.send(cmd);
        }
    }

    /// Turns a window action into a server request.
    fn ui_action(&mut self, a: windows::UiAction) {
        use ro_net::{Command, ItemRequest, StatRequest};
        use windows::UiAction::*;
        match a {
            UseItem(index) => self.send(Command::Item(ItemRequest::Use { index, account_id: self.account_id })),
            Equip { index, location } => self.send(Command::Item(ItemRequest::Equip { index, location })),
            Unequip(index) => self.send(Command::Item(ItemRequest::Unequip { index })),
            UnequipAll(location) => self.send(Command::Item(ItemRequest::UnequipAll { location })),
            Favorite { index, on } => self.send(Command::Item(ItemRequest::Favorite { index, on })),
            SwitchAdd { index, location } => self.send(Command::Switch(ro_net::SwitchRequest::Add { index, location })),
            SwitchRemove(index) => self.send(Command::Switch(ro_net::SwitchRequest::Remove(index))),
            SwitchAll => self.send(Command::Switch(ro_net::SwitchRequest::All)),
            Drop { index, amount } => self.send(Command::Item(ItemRequest::Drop { index, amount })),
            RaiseStat(i) => self.send(Command::Stat(StatRequest::Raise(13 + i as u16))),
            CardTargets(card) => {
                self.windows.card_pick = Some((card, vec![]));
                self.send(Command::Item(ItemRequest::CardTargets { card }));
            }
            InsertCard { card, equip } => self.send(Command::Item(ItemRequest::InsertCard { card, equip })),
            Menu(reply) => self.send(Command::Menu(reply)),
            ShowEquip(on) => self.send(Command::ShowEquip(on)),
            ExpandInventory => self.send(Command::Item(ItemRequest::ExpandAsk)),
            ExpandConfirm(yes) => {
                self.windows.expand_ask = None;
                self.send(Command::Item(ItemRequest::ExpandConfirm(yes)));
            }
            OpenBank => {
                if self.merchant.bank.is_none() {
                    self.toggle_bank();
                }
            }
            ItemInfo { id, refine, cards } => {
                if let Some(vfs) = self.vfs.as_ref() {
                    self.item_info.show_item(&self.egui_ctx, vfs, self.item_db.as_ref(), id, refine, cards);
                }
            }
            CartPut { index, amount } => self.send(Command::Merchant(ro_net::MerchantRequest::CartPut { index, amount: amount as u32 })),
            SkillUp(id) => self.send(Command::Stat(StatRequest::SkillUp(id))),
            UseSkill(id) => {
                let level = self.skills.iter().find(|s| s.id == id).map_or(0, |s| s.level);
                self.start_skill(id, level);
            }
            UseHotkey(slot) => {
                let Some(key) = self.hotkeys.get(slot).copied().filter(|k| k.id != 0) else { return };
                if key.skill {
                    self.start_skill(key.id as u16, key.count.max(1) as u16);
                } else if let Some(it) = self.inventory.items.values().find(|it| it.id == key.id && it.equipped == 0) {
                    let (index, equip, location) = (it.index, it.is_equipment(), it.location);
                    self.ui_action(if equip { Equip { index, location } } else { UseItem(index) });
                }
            }
            SetHotkey { index, key } => {
                if let Some(slot) = self.hotkeys.get_mut(index as usize) {
                    *slot = key;
                }
                self.send(Command::Stat(StatRequest::SetHotkey { index, key }));
            }
            HotkeyRow(row) => {
                self.hotkey_row = row;
                self.send(Command::Stat(StatRequest::HotkeyRow(row)));
            }
        }
    }

    /// Uses a skill at `level`: self skills go off at once, targeted ones wait for a click.
    fn start_skill(&mut self, id: u16, level: u16) {
        let Some(s) = self.skills.iter().find(|s| s.id == id) else { return };
        let level = level.clamp(1, s.level.max(1));
        if s.target & 4 != 0 {
            self.use_skill_on(id, level, self.account_id);
        } else if s.target & (1 | 2 | 16 | 32) != 0 {
            self.aiming = Some((id, level));
        }
    }

    fn use_skill_on(&mut self, skill: u16, level: u16, target: u32) {
        self.attack = None;
        // Out of reach: walk into range first, then cast (as the original client does).
        let range = self.skills.iter().find(|s| s.id == skill).map_or(1, |s| (s.range as i32).max(1));
        let cells = self.player.as_ref().map(|p| p.walker.cell).zip(self.units.get(&target).map(|m| m.walker.cell));
        if let (Some((a, b)), true) = (cells, target != self.account_id) {
            if (a.0 - b.0).abs().max((a.1 - b.1).abs()) > range {
                let tries = self.pending_skill.filter(|p| p.2 == target).map_or(0, |p| p.3 + 1);
                if tries > 3 {
                    self.pending_skill = None;
                    return;
                }
                let gat = self.world.as_ref().and_then(|w| w.gat.as_ref());
                let reach = range.min(3);
                let spot = (-reach..=reach)
                    .flat_map(|dy| (-reach..=reach).map(move |dx| (b.0 + dx, b.1 + dy)))
                    .filter(|&(x, y)| gat.is_some_and(|g| g.walkable(x, y)))
                    .min_by_key(|&(x, y)| (x - a.0).abs().max((y - a.1).abs()));
                if let Some((x, y)) = spot {
                    self.pending_skill = Some((skill, level, target, tries));
                    self.request_walk((x as u16, y as u16));
                }
                return;
            }
        }
        self.pending_skill = None;
        self.walk_goal = None;
        self.send(ro_net::Command::Stat(ro_net::StatRequest::UseSkill { skill, level, target }));
    }

    fn use_skill_at(&mut self, skill: u16, level: u16, cell: (u16, u16)) {
        self.walk_goal = None;
        self.attack = None;
        self.send(ro_net::Command::Stat(ro_net::StatRequest::UseSkillAt { skill, level, x: cell.0, y: cell.1 }));
    }

    /// Picks an item up, walking next to it first when it is out of reach.
    fn pick_up(&mut self, id: u32) {
        let (Some(p), Some((f, _))) = (self.player.as_ref(), self.floor.get(&id)) else { return };
        let (a, b) = (p.walker.cell, (f.x as i32, f.y as i32));
        self.attack = None;
        if (a.0 - b.0).abs().max((a.1 - b.1).abs()) <= 1 {
            self.pending_pickup = None;
            tracing::debug!(id, "pickup request");
            self.send(ro_net::Command::Item(ro_net::ItemRequest::PickUp { floor_id: id }));
        } else {
            self.pending_pickup = Some(id);
            self.request_walk((f.x, f.y));
        }
    }

    /// The floor item under the cursor.
    fn pick_floor_item(&self) -> Option<u32> {
        let (r, (cx, cy)) = (self.renderer.as_ref()?, self.cursor?);
        let w = self.world.as_ref()?;
        let size = r.size();
        self.floor
            .iter()
            .filter_map(|(&id, (f, _))| {
                let (fx, fy) = floor_cell(f);
                let (x, z) = (fx * ro_render::CELL, fy * ro_render::CELL);
                let p = r.project(glam::Vec3::new(x, w.ground.height_at(x, z), z), size)?;
                let d = (p.x - cx as f32).hypot(p.y - 8.0 - cy as f32);
                (d < 22.0).then_some((id, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    /// Ground sprite for an item (sprite\아이템\{resource}).
    fn item_actor(&mut self, item: u32, identified: bool) -> Option<ro_render::Actor> {
        let info = self.item_db.as_ref()?.get(item)?;
        let res = if identified || info.unidentified_resource.is_empty() { info.resource.clone() } else { info.unidentified_resource.clone() };
        let key = format!("item:{res}");
        if let Some(a) = self.sprites.get(&key) {
            return a.clone();
        }
        let (r, vfs) = (self.renderer.as_mut()?, self.vfs.as_ref()?);
        let base = format!("data\\sprite\\아이템\\{res}");
        let spr = vfs.read(&format!("{base}.spr")).ok().flatten().and_then(|d| ro_formats::spr::Spr::parse(&d).ok());
        let act = vfs.read(&format!("{base}.act")).ok().flatten().and_then(|d| ro_formats::act::Act::parse(&d).ok());
        let actor = match (spr, act) {
            (Some(spr), Some(act)) => {
                let body = r.add_sheet(&spr, act, None);
                Some(ro_render::Actor { pos: Default::default(), body, head: None, headgear: vec![], weapon: vec![], shadow: None, action: 0, dir: ro_render::Dir::S, time_ms: 0.0, once: false, alpha: 1.0 })
            }
            _ => None,
        };
        self.sprites.insert(key, actor.clone());
        actor
    }

    fn load_shop_icons(&mut self) {
        let ids: Vec<u32> = match &self.social.shop {
            Some(social::Shop::Buy { items, .. }) => items.iter().map(|i| i.id).collect(),
            _ => vec![],
        };
        self.load_icons_for(&ids);
    }

    /// Loads icons for specific item ids (shops, trades, storage).
    fn load_icons_for(&mut self, ids: &[u32]) {
        let (Some(db), Some(vfs)) = (self.item_db.as_ref(), self.vfs.as_ref()) else { return };
        for &id in ids {
            if self.icons.contains_key(&id) || !self.icons_tried.insert(id) {
                continue;
            }
            let Some(info) = db.get(id) else { continue };
            let path = format!("data\\texture\\유저인터페이스\\item\\{}.bmp", info.resource);
            if let Some(tex) = load_texture(&self.egui_ctx, vfs, &path) {
                self.icons.insert(id, tex);
            }
        }
    }

    /// Loads inventory icons not yet loaded into egui textures.
    fn load_icons(&mut self) {
        let (Some(db), Some(vfs)) = (self.item_db.as_ref(), self.vfs.as_ref()) else { return };
        let ids: Vec<(u32, bool)> = self.inventory.items.values().map(|i| (i.id, i.identified)).collect();
        for (id, identified) in ids {
            if self.icons.contains_key(&id) || !self.icons_tried.insert(id) {
                continue;
            }
            let Some(info) = db.get(id) else { continue };
            let res = if identified || info.unidentified_resource.is_empty() { &info.resource } else { &info.unidentified_resource };
            let path = format!("data\\texture\\유저인터페이스\\item\\{res}.bmp");
            if let Some(tex) = load_texture(&self.egui_ctx, vfs, &path) {
                self.icons.insert(id, tex);
            }
        }
    }

    fn load_skill_icons(&mut self) {
        let Some(vfs) = self.vfs.as_ref() else { return };
        // Learned skills, and the rest of the job's tree (drawn greyed out).
        let mut wanted: Vec<(u16, String)> = self.skills.iter().map(|s| (s.id, s.name.clone())).collect();
        for (_, grid) in self.skill_tree.tabs(self.hud.status.job_id) {
            wanted.extend(grid.iter().filter_map(|(_, n)| Some((self.skill_names.id_of(n)?, n.clone()))));
        }
        for (id, name) in wanted {
            if self.skill_icons.contains_key(&id) {
                continue;
            }
            let path = format!("data\\texture\\유저인터페이스\\item\\{}.bmp", name.to_ascii_lowercase());
            if let Some(tex) = load_texture(&self.egui_ctx, vfs, &path) {
                self.skill_icons.insert(id, tex);
            }
        }
    }

    /// The player or a unit by id.
    /// Left button held on the ground: re-aim the walk at the cursor every few frames, as
    /// the original client does when you hold the button and point.
    fn steer_to_cursor(&mut self, dt: f32) {
        if !self.mouse_walk {
            return;
        }
        if !self.left_down || self.dead || self.aiming.is_some() {
            self.mouse_walk = false;
            return;
        }
        self.mouse_walk_ms -= dt;
        if self.mouse_walk_ms > 0.0 {
            return;
        }
        self.mouse_walk_ms = 150.0;
        if self.egui_ctx.is_pointer_over_egui() {
            return;
        }
        if let Some(cell) = self.pick() {
            if self.walk_goal.as_ref().is_none_or(|g| g.cell != cell) {
                self.request_walk(cell);
            }
        }
    }

    /// Indoors: recompute which room we are in when we step outside the current one (map
    /// entry, warps within the map) and hand the renderer its mask.
    fn update_room(&mut self) {
        let (Some(w), Some(p)) = (self.world.as_mut(), self.player.as_ref()) else { return };
        if !w.indoor() || (w.room.is_some() && w.shows_cell(p.walker.cell.0, p.walker.cell.1)) {
            return;
        }
        let Some(room) = w.room_of(p.walker.cell) else { return };
        if let (Some(r), Some(g)) = (self.renderer.as_mut(), w.gat.as_ref()) {
            r.set_room_mask(Some((g.width, g.height, &room)));
        }
        w.room = Some(room);
    }

    /// The cursor for what is under the mouse, as the original client shows it.
    fn hover_cursor(&self) -> usize {
        use hud::cursor::*;
        if self.rotating {
            return ROTATE;
        }
        if self.aiming.is_some() || self.capturing {
            return TARGET;
        }
        if self.cursor.is_none() || self.egui_ctx.is_pointer_over_egui() {
            return ARROW;
        }
        match self.pick_unit().and_then(|id| self.units.get(&id)).map(|m| m.kind) {
            Some(5) => ATTACK,
            Some(0) if self.pick_unit().is_some_and(|id| self.is_enemy_player(id)) => ATTACK,
            Some(1 | 6) => TALK,
            _ if self.pick_floor_item().is_some() => PICK_UP,
            _ => ARROW,
        }
    }

    /// On a PvP / GvG map, a player outside our party (PvP) or guild (GvG).
    fn is_enemy_player(&self, id: u32) -> bool {
        let Some(m) = self.units.get(&id) else { return false };
        let pvp = self.map_flags & 1 != 0;
        let gvg = self.map_flags & 2 != 0;
        let in_party = self.social.party.iter().flat_map(|p| p.members.iter()).any(|pm| pm.name == m.name);
        let in_guild = self.guild.id != 0 && m.guild == self.guild.id;
        // DISABLE_LOCKON: players are only attacked while holding Shift.
        let lock_ok = self.map_flags & 16 == 0 || self.egui_ctx.input(|i| i.modifiers.shift);
        lock_ok && ((pvp && !in_party) || (gvg && !in_guild))
    }

    /// Back on our feet: respawned at the save point or resurrected.
    fn revive(&mut self) {
        if !std::mem::take(&mut self.dead) {
            return;
        }
        if let Some(p) = self.player.as_mut() {
            p.pose = None;
            p.actor.time_ms = 0.0;
        }
    }

    /// Where a unit stands (its feet), if we can see it.
    fn unit_pos(&self, id: u32) -> Option<glam::Vec3> {
        if id == self.account_id { self.player.as_ref() } else { self.units.get(&id) }.map(|m| m.actor.pos)
    }

    /// The ground point at the centre of a cell.
    fn cell_pos(&self, cell: (u16, u16)) -> Option<glam::Vec3> {
        let w = self.world.as_ref()?;
        let (fx, fy) = (cell.0 as f32 + 0.5, cell.1 as f32 + 0.5);
        let (x, z) = (fx * ro_render::CELL, fy * ro_render::CELL);
        let y = w.gat.as_ref().and_then(|g| g.height_at(fx, fy)).map_or_else(|| w.ground.height_at(x, z), |h| -h);
        Some(glam::Vec3::new(x, y, z))
    }

    /// Plays a skill's effect at `pos`, if it has one.
    /// Plays a skill's effect at `pos`: its known visual, else an STR named after the skill
    /// (AS_SONICBLOW -> sonicblow.str), else a generic hit (damage) or light (support).
    /// Weapon sound family of a player unit (None for monsters and NPCs).
    fn weapon_sound(&self, id: u32) -> Option<&'static str> {
        let m = if id == self.account_id { self.player.as_ref() } else { self.units.get(&id) }?;
        let look = m.look.as_ref()?;
        let view = self.item_db.as_ref().and_then(|d| d.get(look.gear.weapon)).map_or(look.gear.weapon, |i| i.view);
        Some(audio::weapon_kind(self.look_tables.weapon(view).as_deref().filter(|_| look.gear.weapon != 0)))
    }

    fn play_sound(&self, name: &str) {
        if let (Some(a), Some(vfs)) = (self.audio.as_ref(), self.vfs.as_ref()) {
            a.play(vfs, name);
        }
    }

    fn skill_fx(&mut self, skill: u16, pos: Option<glam::Vec3>, damaging: bool) {
        // The skill's own sound, when the client has one.
        if let (Some(a), Some(vfs), Some(name)) = (self.audio.as_ref(), self.vfs.as_ref(), self.skill_names.internal(skill)) {
            match a.skill_sound(name) {
                Some(file) => a.play(vfs, &file),
                None => tracing::debug!(skill, name, "no sound for skill"),
            }
        }
        let Some(pos) = pos else { return };
        let (Some(r), Some(vfs)) = (self.renderer.as_mut(), self.vfs.as_ref()) else { return };
        match fx::skill_visual(skill) {
            Some(fx::Visual::Str(name)) => {
                if let Some(asset) = self.fx.get(r, vfs, name) {
                    r.effects.push(ro_render::Fx { asset, pos, time_ms: 0.0, looping: false, tag: 0 });
                }
            }
            Some(fx::Visual::Column(tex, color)) => {
                if let Some(tex) = self.fx.texture(r, vfs, tex) {
                    let shape = ro_render::Shape::Column { radius: 0.9, height: 3.5 };
                    r.procs.push(ro_render::ProcFx { tex, shape, pos, color, time_ms: 0.0, duration_ms: 1200.0 });
                }
            }
            Some(fx::Visual::Sprite(name)) => {
                if let Some(body) = self.fx.sprite(r, vfs, name) {
                    let actor = ro_render::Actor {
                        pos,
                        body,
                        head: None,
                        headgear: vec![],
                        weapon: vec![],
                        shadow: None,
                        action: 0,
                        dir: ro_render::Dir::S,
                        time_ms: 0.0,
                        once: true,
                        alpha: 1.0,
                    };
                    let ms = r.action_ms(body, 0).max(200.0);
                    self.sprite_fx.push((actor, ms));
                }
            }
            Some(fx::Visual::Code) => {}
            None => {
                let by_name = self.skill_names.internal(skill).map(|n| {
                    let short = n.split_once('_').map_or(n, |(_, rest)| rest).to_ascii_lowercase();
                    [short.clone(), short.replace('_', "")]
                });
                let folder = by_name.as_ref().and_then(|n| self.fx.find(&n[0])).map(str::to_string);
                let asset = by_name.iter().flatten().find_map(|n| self.fx.get(r, vfs, n))
                    .or_else(|| folder.and_then(|p| self.fx.get(r, vfs, &p))).or_else(|| damaging.then(|| self.fx.get(r, vfs, "hit")).flatten());
                if let Some(asset) = asset {
                    r.effects.push(ro_render::Fx { asset, pos, time_ms: 0.0, looping: false, tag: 0 });
                } else if let Some(tex) = self.fx.texture(r, vfs, "alpha_down.tga") {
                    // Buffs and other support skills without their own effect: a soft light.
                    let shape = ro_render::Shape::Column { radius: 0.8, height: 3.0 };
                    r.procs.push(ro_render::ProcFx { tex, shape, pos, color: [0.45, 0.45, 0.5], time_ms: 0.0, duration_ms: 900.0 });
                }
                tracing::debug!(skill, name = ?self.skill_names.internal(skill), "no mapped effect; used fallback");
            }
        }
    }

    /// Earth spikes erupting in a wave from `from` toward `to` (Grimtooth).
    fn spike_line(&mut self, from: glam::Vec3, to: glam::Vec3) {
        let (Some(r), Some(vfs)) = (self.renderer.as_mut(), self.vfs.as_ref()) else { return };
        let Some(tex) = self.fx.texture(r, vfs, "dustblast_stick.bmp") else { return };
        let reach = to - from;
        let count = ((reach.length() / ro_render::CELL) as u32).clamp(3, 12);
        let shape = ro_render::Shape::Spikes { reach, count };
        r.procs.push(ro_render::ProcFx { tex, shape, pos: from, color: [0.85, 0.7, 0.5], time_ms: 0.0, duration_ms: 650.0 });
    }

    /// Alt+letter window shortcuts (also the Classic Basic Info buttons).
    fn alt_key(&mut self, key: &str) {
        match key {
            "e" => self.windows.toggle_inventory(),
            "q" => self.windows.toggle_equipment(),
            "a" => self.windows.toggle_stats(),
            "s" => self.windows.toggle_skills(),
            "z" => self.windows.party = !self.windows.party,
            "h" => self.windows.friends = !self.windows.friends,
            "g" => self.windows.guild = !self.windows.guild,
            "m" => {
                self.settings.minimap = !self.settings.minimap;
                self.settings.save();
            }
            "o" => self.windows.settings = !self.windows.settings,
            "u" => self.quests.open = !self.quests.open,
            "w" => self.merchant.cart_open = !self.merchant.cart_open,
            "b" => self.toggle_bank(),
            "c" => self.toggle_cash_shop(),
            "y" => self.progress.open = !self.progress.open,
            "clan" => self.clan.open = !self.clan.open,
            "repute" => {
                self.reputation.load(self.vfs.as_ref());
                self.reputation.open = !self.reputation.open;
            }
            "opts" => self.game_opts.open = !self.game_opts.open,
            "rank" => self.send(ro_net::Command::Ranking(self.ranking.kind)),
            "battle" => {
                self.battle.ensure_loaded(self.vfs.as_ref());
                self.battle.open = !self.battle.open;
            }
            "navi" => {
                self.navi.ensure_loaded(self.vfs.as_ref());
                self.navi.open = !self.navi.open;
            }
            "n" => {
                self.booking.open = !self.booking.open;
                if self.booking.open {
                    self.send(ro_net::Command::Booking(ro_net::BookingRequest::Search { level: 0, map: 0, job: 0xffff }));
                }
            }
            "k" => {
                let me = self.hud.status.name.clone();
                self.rooms.open_create(&me);
            }
            "x" => {
                self.mailbox.open = !self.mailbox.open;
                if self.mailbox.open {
                    self.mailbox.unread = false;
                    self.send(ro_net::Command::Mail(ro_net::MailRequest::Open { refresh: false }));
                }
            }
            "r" => {
                self.companions.open = !self.companions.open;
                if self.companions.open {
                    self.send(ro_net::Command::Companion(ro_net::CompanionRequest::PetMenu(0)));
                }
            }
            "j" => {
                tracing::info!(open = self.progress.attendance.is_some(), "attendance");
                if self.progress.attendance.take().is_none() {
                    self.queued_commands.push(ro_net::Command::Progress(ro_net::ProgressRequest::OpenAttendance));
                }
            }
            // Alt+1..Alt+0: the emote shortcuts.
            d if d.len() == 1 && d.as_bytes()[0].is_ascii_digit() => {
                if let Some((tx, _)) = self.net.as_ref() {
                    let _ = tx.send(ro_net::Command::Emotion(emotes::shortcut(d.as_bytes()[0] - b'0')));
                }
            }
            _ => {}
        }
    }

    /// The companion AI. Like the original, the homunculus runs AI\AI.lua and the
    /// mercenary AI\AI_M.lua (USER_AI\ when present there); without the scripts they just
    /// follow us and join our fights.
    fn companion_follow(&mut self, now: std::time::Instant) {
        if now < self.follow_at {
            return;
        }
        self.follow_at = now + std::time::Duration::from_millis(250);
        let homun = self.companions.homun_id;
        let merc = self.companions.merc.as_ref().map_or(0, |m| m.id);
        let mut scripted = false;
        for (id, file, homun_brain) in [(homun, "AI.lua", true), (merc, "AI_M.lua", false)] {
            if id == 0 || !self.units.contains_key(&id) {
                continue;
            }
            let slot = if homun_brain { &mut self.homun_ai } else { &mut self.merc_ai };
            if slot.is_none() {
                let Some(dir) = self.client_dir.as_deref() else { continue };
                let user = dir.join("AI").join("USER_AI").join(file).exists();
                match ai::load(dir, file, user) {
                    Ok(b) => {
                        tracing::info!(file, user, "companion AI loaded");
                        *slot = Some(b);
                    }
                    Err(e) => {
                        tracing::warn!("companion AI {file}: {e}");
                        self.client_dir = None;
                        continue;
                    }
                }
            }
            let snapshot = self.ai_snapshot();
            let Some(brain) = (if homun_brain { self.homun_ai.as_ref() } else { self.merc_ai.as_ref() }) else { continue };
            {
                let mut w = brain.world.borrow_mut();
                w.owner = snapshot.0;
                w.units = snapshot.1;
            }
            scripted = true;
            for order in brain.tick(id) {
                tracing::debug!(id, ?order, "AI order");
                let cmd = match order {
                    // The original's MoveToOwner asks the server to bring it beside us.
                    ai::AiOrder::Move { id, x, y } if self.player.as_ref().is_some_and(|p| p.walker.goal_or_cell() == (x, y)) => ro_net::CompanionRequest::Return(id),
                    ai::AiOrder::Move { id, x, y } => ro_net::CompanionRequest::Move { id, x: x as u16, y: y as u16 },
                    ai::AiOrder::Attack { id, target } => ro_net::CompanionRequest::Attack { id, target },
                    ai::AiOrder::Skill { level, skill, target, .. } => {
                        self.send(ro_net::Command::Stat(ro_net::StatRequest::UseSkill { skill, level, target }));
                        continue;
                    }
                    ai::AiOrder::SkillGround { level, skill, x, y, .. } => {
                        self.send(ro_net::Command::Stat(ro_net::StatRequest::UseSkillAt { skill, level, x: x as u16, y: y as u16 }));
                        continue;
                    }
                };
                self.send(ro_net::Command::Companion(cmd));
            }
        }
        if scripted {
            return;
        }
        self.follow_at = now + std::time::Duration::from_millis(400);
        let Some((me, facing)) = self.player.as_ref().map(|p| (p.walker.goal_or_cell(), p.walker.dir)) else { return };
        // Behind us relative to our facing (dir 0 = south ... clockwise), one on each side.
        const STEP: [(i32, i32); 8] = [(0, -1), (-1, -1), (-1, 0), (-1, 1), (0, 1), (1, 1), (1, 0), (1, -1)];
        let behind = |turn: usize| {
            let (x, y) = STEP[(facing + 4 + turn) % 8];
            (x * 2, y * 2)
        };
        let ids = [(self.companions.homun_id, behind(1)), (self.companions.merc.as_ref().map_or(0, |m| m.id), behind(7))];
        // While we fight, they attack our target too (the original AI's default).
        if let Some(target) = self.attack.map(|a| a.0).filter(|t| self.units.get(t).is_some_and(|m| m.dying.is_none())) {
            let present: Vec<u32> = ids.iter().map(|(id, _)| *id).filter(|id| *id != 0 && self.units.contains_key(id)).collect();
            for id in present {
                self.send(ro_net::Command::Companion(ro_net::CompanionRequest::Attack { id, target }));
            }
            self.follow_at = now + std::time::Duration::from_millis(1000);
            return;
        }
        for (id, (dx, dy)) in ids {
            let Some(unit) = self.units.get(&id).filter(|_| id != 0) else { continue };
            let at = unit.walker.goal_or_cell();
            if (at.0 - me.0).abs().max((at.1 - me.1).abs()) <= 3 {
                continue;
            }
            let gat = self.world.as_ref().and_then(|w| w.gat.as_ref());
            let spot = [(dx, dy), (dx, 0), (0, dy), (-dx, dy), (0, 0)]
                .iter()
                .map(|(ox, oy)| (me.0 + ox, me.1 + oy))
                .find(|&(x, y)| gat.is_some_and(|g| g.walkable(x, y)));
            if let Some((x, y)) = spot {
                self.send(ro_net::Command::Companion(ro_net::CompanionRequest::Move { id, x: x as u16, y: y as u16 }));
            }
        }
    }

    /// The units in sight as the AI scripts see them: (our account id, units).
    fn ai_snapshot(&self) -> (u32, std::collections::HashMap<u32, ai::AiUnit>) {
        let motion = |m: &Mob| {
            if m.dying.is_some() || matches!(m.pose, Some((Pose::Dead, _))) {
                3
            } else if matches!(m.pose, Some((Pose::Attack, _))) {
                2
            } else if m.walker.is_walking() {
                1
            } else {
                0
            }
        };
        let mut units = std::collections::HashMap::new();
        if let Some(p) = self.player.as_ref() {
            let s = &self.hud.status;
            units.insert(
                self.account_id,
                ai::AiUnit { cell: p.walker.cell, motion: motion(p), target: self.attack.map_or(0, |a| a.0), hp: (s.hp as i32, s.max_hp as i32), sp: (s.sp as i32, s.max_sp as i32), range: self.attack_range, ..Default::default() },
            );
        }
        for (id, m) in &self.units {
            let job = self.unit_jobs.get(id).copied().unwrap_or(0) as i32;
            let mut u = ai::AiUnit { cell: m.walker.cell, motion: motion(m), target: self.aggro.get(id).copied().unwrap_or(0), monster: m.kind == 5, range: 1, ..Default::default() };
            if let Some(hp) = m.hp {
                u.hp = hp;
            }
            if *id == self.companions.homun_id {
                u.kind = job - 6000;
                if let Some(h) = &self.companions.homun {
                    u.hp = (h.hp as i32, h.max_hp as i32);
                    u.sp = (h.sp as i32, h.max_sp as i32);
                }
            } else if self.companions.merc.as_ref().is_some_and(|mc| mc.id == *id) {
                u.kind = job - 6016;
                // Archer mercenaries (types 1-10) shoot from range.
                u.range = if (1..=10).contains(&u.kind) { 9 } else { 1 };
                if let Some(mc) = &self.companions.merc {
                    u.hp = (mc.hp as i32, mc.max_hp as i32);
                    u.sp = (mc.sp as i32, mc.max_sp as i32);
                }
            }
            units.insert(*id, u);
        }
        (self.account_id, units)
    }

    /// Opens the bank (asking for the balance) or closes it.
    fn toggle_bank(&mut self) {
        let aid = self.account_id;
        if self.merchant.bank.take().is_some() {
            self.queued_commands.push(ro_net::Command::Bank(ro_net::BankRequest::Close(aid)));
        } else {
            self.merchant.bank = Some((None, String::new()));
            self.queued_commands.push(ro_net::Command::Bank(ro_net::BankRequest::Open(aid)));
            self.queued_commands.push(ro_net::Command::Bank(ro_net::BankRequest::Check(aid)));
        }
    }

    /// Opens the cash shop (the server answers with our points) or closes it.
    fn toggle_cash_shop(&mut self) {
        let req = if self.stores.cash.take().is_some() { ro_net::CashShopRequest::Close } else { ro_net::CashShopRequest::Open };
        self.queued_commands.push(ro_net::Command::CashShop(req));
    }

    fn proc_fx(&mut self, tex: &'static str, shape: ro_render::Shape, pos: glam::Vec3, color: [f32; 3], duration_ms: f32) {
        let (Some(r), Some(vfs)) = (self.renderer.as_mut(), self.vfs.as_ref()) else { return };
        if let Some(tex) = self.fx.texture(r, vfs, tex) {
            r.procs.push(ro_render::ProcFx { tex, shape, pos, color, time_ms: 0.0, duration_ms });
        }
    }

    /// Meteor Assault: eight violet slashes flying outward from the caster.
    /// The cast and every monster hit all announce it, so it plays once per cast.
    fn meteor_assault(&mut self, src: u32) {
        use ro_render::{Shape, CELL};
        let now = std::time::Instant::now();
        if self.burst_at.get(&src).is_some_and(|t| now.duration_since(*t).as_millis() < 400) {
            return;
        }
        let Some(at) = self.unit_pos(src) else { return };
        self.burst_at.insert(src, now);
        for i in 0..8 {
            let a = i as f32 / 8.0 * std::f32::consts::TAU;
            let dir = glam::Vec3::new(a.cos(), 0.0, a.sin()) * CELL;
            let shape = Shape::Slash { travel: dir * 4.0, size: (1.0, 2.0) };
            self.proc_fx("purpleslash.tga", shape, at + dir, [1.0, 1.0, 1.0], 500.0);
        }
    }

    fn play_fx(&mut self, name: &'static str, pos: glam::Vec3) {
        self.play_fx_tagged(name, pos, 0);
    }

    /// Plays an STR effect; a non-zero `tag` loops it until `stop_fx(tag)`.
    fn play_fx_tagged(&mut self, name: &'static str, pos: glam::Vec3, tag: u32) {
        let (Some(r), Some(vfs)) = (self.renderer.as_mut(), self.vfs.as_ref()) else { return };
        if let Some(asset) = self.fx.get(r, vfs, name) {
            r.effects.push(ro_render::Fx { asset, pos, time_ms: 0.0, looping: tag != 0, tag });
        }
    }

    /// Sends a chat line, handling the client's own commands (whispers, emotes, /where...).
    fn submit_chat(&mut self, text: String) {
        let Some((tx, _)) = self.net.as_ref() else { return };
            // "/p text" goes to the party, "/g text" to the guild.
            let speaker = self.hud.status.name.clone();
            let mut note = None;
            // Whisper blocking, as the original: /ex name, /in name, /exall, /inall, /ex (list).
            let ignore = match text.trim() {
                "/exall" => Some(ro_net::IgnoreRequest::All { block: true }),
                "/inall" => Some(ro_net::IgnoreRequest::All { block: false }),
                "/ex" => Some(ro_net::IgnoreRequest::List),
                t => match t.split_once(' ') {
                    Some(("/ex", n)) => Some(ro_net::IgnoreRequest::Player { name: n.trim().trim_matches('"').to_string(), block: true }),
                    Some(("/in", n)) => Some(ro_net::IgnoreRequest::Player { name: n.trim().trim_matches('"').to_string(), block: false }),
                    _ => None,
                },
            };
            if let Some(k) = ["/blacksmith", "/alchemist", "/taekwon", "/pk"].iter().position(|c| text.trim() == *c) {
                let _ = tx.send(ro_net::Command::Ranking(k as u16));
                return;
            }
            // /navi map [x/y]: guide there, as the original client; /navi alone opens the window.
            if let Some(rest) = text.trim().strip_prefix("/navi") {
                self.navi.ensure_loaded(self.vfs.as_ref());
                let mut parts = rest.split(|c: char| c == ' ' || c == '/' || c == ',').filter(|s| !s.is_empty());
                match parts.next() {
                    Some(map) => {
                        let cell = match (parts.next().and_then(|x| x.parse().ok()), parts.next().and_then(|y| y.parse().ok())) {
                            (Some(x), Some(y)) => Some((x, y)),
                            _ => None,
                        };
                        let label = match cell { Some((x, y)) => format!("{map} ({x}, {y})"), None => map.to_string() };
                        self.navi.go(map, cell, label);
                    }
                    None => self.navi.open = true,
                }
                return;
            }
            if let Some(r) = ignore {
                if let ro_net::IgnoreRequest::Player { name, block } = &r {
                    self.ignore_pending = Some((name.clone(), *block));
                }
                let _ = tx.send(ro_net::Command::Ignore(r));
                return;
            }
            let cmd = match text.split_once(' ') {
                Some(("/p", rest)) => Some(ro_net::Command::Party(ro_net::PartyRequest::Chat { speaker, text: rest.to_string() })),
                Some(("/g", rest)) => Some(ro_net::Command::Party(ro_net::PartyRequest::GuildChat { speaker, text: rest.to_string() })),
                Some(("/cl", rest)) => Some(ro_net::Command::Party(ro_net::PartyRequest::ClanChat { speaker, text: rest.to_string() })),
                Some(("/bc", rest)) => Some(ro_net::Command::Bg(ro_net::BgRequest::Chat(format!("{speaker} : {rest}")))),
                // /guild "Name": found a guild, as in the original client.
                Some(("/guild", rest)) => Some(ro_net::Command::Guild(ro_net::GuildRequest::Create { char_id: self.char_id, name: rest.trim().trim_matches('"').to_string() })),
                // /r text: answer whoever whispered last.
                Some(("/r", rest)) => match self.last_whisper_from.clone() {
                    Some(to) => {
                        note = Some((ro_net::ChatKind::Whisper, format!("(To {to}) : {rest}")));
                        Some(ro_net::Command::Whisper { to, text: rest.to_string() })
                    }
                    None => {
                        note = Some((ro_net::ChatKind::System, "Nobody has whispered you yet.".into()));
                        None
                    }
                },
                _ if text.starts_with("/w ") || text.starts_with("/whisper ") => match emotes::whisper(&text) {
                    Some((to, msg)) => {
                        note = Some((ro_net::ChatKind::Whisper, format!("(To {to}) : {msg}")));
                        Some(ro_net::Command::Whisper { to, text: msg })
                    }
                    None => {
                        note = Some((ro_net::ChatKind::System, "Usage: /w Name message  (quote names with spaces)".into()));
                        None
                    }
                },
                _ => match emotes::command(&text) {
                    Some(e) => Some(ro_net::Command::Emotion(e)),
                    // Commands the client handles itself.
                    None if text == "/sit" || text == "/stand" => Some(ro_net::Command::Sit(text == "/sit")),
                    None if text == "/where" => {
                        let map = self.world.as_ref().map(|w| w.name.clone()).unwrap_or_default();
                        if let Some((x, y)) = self.player.as_ref().map(|p| p.walker.cell) {
                            note = Some((ro_net::ChatKind::System, format!("{map} {x}, {y}")));
                        }
                        None
                    }
                    // An unknown /command is not said aloud.
                    None if text.starts_with('/') => {
                        let name = text.split_whitespace().next().unwrap_or(&text);
                        note = Some((ro_net::ChatKind::System, format!("{name}: unknown command.")));
                        None
                    }
                    None => Some(ro_net::Command::Chat(text)),
                },
            };
            if let Some(cmd) = cmd {
                let _ = tx.send(cmd);
            }
            if let Some((kind, line)) = note {
                self.hud.push_chat(kind, line);
            }
    }

    /// Units the controller can target, nearest first: monsters and named NPCs in reach, or
    /// other players while aiming a support skill.
    fn pad_candidates(&self, players: bool) -> Vec<u32> {
        let Some(me) = self.player.as_ref().map(|p| p.walker.cell) else { return vec![] };
        let mut v: Vec<(i32, u32)> = self
            .units
            .iter()
            .filter(|(_, m)| m.dying.is_none() && m.actor.alpha > 0.01 && if players { m.kind == 0 } else { m.kind == 5 || (matches!(m.kind, 1 | 6) && !m.name.is_empty()) })
            .map(|(id, m)| ((m.walker.cell.0 - me.0).pow(2) + (m.walker.cell.1 - me.1).pow(2), *id))
            .filter(|(d, _)| *d <= PAD_REACH * PAD_REACH)
            .collect();
        v.sort();
        v.into_iter().map(|(_, id)| id).collect()
    }

    /// Controller play: left stick walks (camera-relative), right stick turns and tilts the
    /// camera, A acts on the highlighted target, LB/RB change target, X picks up, Y sits,
    /// B cancels, RT/LT + face buttons or D-pad fire the shortcut bar.
    fn pad_tick(&mut self, dt: f32) {
        use gilrs::Button::*;
        let f = self.pad.poll();
        if !self.pad.in_use() {
            return;
        }
        self.hud.pad_used = true;
        let secs = dt / 1000.0;

        // Camera: right stick turns and tilts; D-pad left/right turn 45 degrees, up/down zoom.
        if let (Some(r), false) = (self.renderer.as_mut(), self.still) {
            if f.right != glam::Vec2::ZERO {
                r.camera.rotate(-f.right.x * secs * 2.2, -f.right.y * secs * 1.2);
            }
            if f.just(RightThumb) {
                r.camera.yaw = 0.0;
            }
        }

        // Dead: A returns to the save point; nothing else acts.
        if self.dead {
            if f.just(South) {
                self.send(ro_net::Command::Restart(0));
            }
            return;
        }

        // NPC dialogs take the D-pad and face buttons while open.
        if self.dialog.is_some() {
            self.hud.pad_nav = if f.just(DPadUp) {
                Some(hud::PadNav::Up)
            } else if f.just(DPadDown) {
                Some(hud::PadNav::Down)
            } else if f.just(South) {
                Some(hud::PadNav::Confirm)
            } else if f.just(East) {
                Some(hud::PadNav::Cancel)
            } else {
                None
            };
            return;
        }

        // Shortcut bar: hold RT (this row) or LT (next row), then a face button or D-pad.
        if f.rt || f.lt {
            const KEYS: [gilrs::Button; 8] = [South, East, West, North, DPadUp, DPadRight, DPadDown, DPadLeft];
            if let Some(col) = KEYS.iter().position(|k| f.just(*k)) {
                let row = (self.hotkey_row + if f.lt && !f.rt { 1 } else { 0 }) % HOTKEY_ROWS;
                self.queued_action = Some(windows::UiAction::UseHotkey(row as usize * HOTKEYS_PER_ROW + col));
            }
        } else {
            // Target: keep the current one while it is in reach, else the nearest.
            let support_aim = self.aiming.and_then(|(s, _)| self.skills.iter().find(|k| k.id == s)).is_some_and(|k| k.target & 16 != 0 && k.target & 1 == 0);
            let list = self.pad_candidates(support_aim);
            if self.pad_target.is_none_or(|t| !list.contains(&t)) {
                self.pad_target = list.first().copied();
            }
            if f.just(RightTrigger) || f.just(LeftTrigger) {
                let at = self.pad_target.and_then(|t| list.iter().position(|&x| x == t)).unwrap_or(0);
                let n = list.len().max(1);
                let next = if f.just(RightTrigger) { (at + 1) % n } else { (at + n - 1) % n };
                self.pad_target = list.get(next).copied();
            }
            let target = self.pad_target.and_then(|t| self.units.get(&t).map(|m| (t, m.kind, m.walker.cell)));
            if let Some((skill, level)) = self.aiming {
                // Aiming a skill: A on the target (or its cell), Y on yourself, B cancels.
                let flags = self.skills.iter().find(|s| s.id == skill).map_or(0, |s| s.target);
                let ground = flags & 2 != 0;
                // Support skills (Heal, Detoxify, Blessing...) go on a targeted player, else on us.
                let support = flags & 16 != 0 && flags & 1 == 0;
                if f.just(South) && support {
                    self.aiming = None;
                    let ally = target.filter(|t| t.1 == 0).map_or(self.account_id, |t| t.0);
                    self.use_skill_on(skill, level, ally);
                } else if f.just(South) {
                    self.aiming = None;
                    match target {
                        Some((_, _, c)) if ground => self.use_skill_at(skill, level, (c.0 as u16, c.1 as u16)),
                        Some((id, _, _)) => self.use_skill_on(skill, level, id),
                        None if ground => {
                            if let Some(c) = self.player.as_ref().map(|p| p.walker.cell) {
                                self.use_skill_at(skill, level, (c.0 as u16, c.1 as u16));
                            }
                        }
                        None => {}
                    }
                } else if f.just(North) {
                    self.aiming = None;
                    self.use_skill_on(skill, level, self.account_id);
                } else if f.just(East) {
                    self.aiming = None;
                }
            } else {
                if f.just(South) {
                    match target {
                        Some((id, 5, _)) => self.attack_unit(id),
                        Some((id, _, _)) => {
                            self.walk_goal = None;
                            self.talk(id);
                        }
                        None => {}
                    }
                }
                if f.just(East) {
                    self.attack = None;
                    self.chasing = false;
                    self.pending_skill = None;
                    self.pending_pickup = None;
                }
                if f.just(West) {
                    // Nearest item on the ground.
                    let me = self.player.as_ref().map(|p| p.walker.cell);
                    let item = me.and_then(|me| {
                        self.floor
                            .iter()
                            .map(|(id, (fi, _))| ((fi.x as i32 - me.0).pow(2) + (fi.y as i32 - me.1).pow(2), *id))
                            .filter(|(d, _)| *d <= 100)
                            .min()
                            .map(|(_, id)| id)
                    });
                    if let Some(id) = item {
                        self.pick_up(id);
                    }
                }
                if f.just(North) {
                    let sitting = self.player.as_ref().is_some_and(|p| p.sitting);
                    self.send(ro_net::Command::Sit(!sitting));
                }
            }
            if let (Some(r), false) = (self.renderer.as_mut(), self.still) {
                if f.just(DPadLeft) {
                    r.camera.rotate(std::f32::consts::FRAC_PI_4, 0.0);
                }
                if f.just(DPadRight) {
                    r.camera.rotate(-std::f32::consts::FRAC_PI_4, 0.0);
                }
                if f.just(DPadUp) {
                    r.camera.zoom(1.0);
                }
                if f.just(DPadDown) {
                    r.camera.zoom(-1.0);
                }
            }
        }
        if f.just(Start) {
            self.windows.settings = !self.windows.settings;
        }
        if f.just(Select) {
            self.windows.toggle_inventory();
        }

        // Left stick walks toward a point a few cells ahead, re-aimed as the stick turns.
        let Some(yaw) = self.renderer.as_ref().map(|r| r.camera.yaw) else { return };
        self.pad_walk_ms -= dt;
        if f.left != glam::Vec2::ZERO {
            let (sy, cy) = yaw.sin_cos();
            let (fwd, right) = (glam::Vec2::new(-sy, cy), glam::Vec2::new(cy, sy));
            let dir = (right * f.left.x + fwd * f.left.y).normalize_or_zero();
            let turned = self.pad_dir.is_none_or(|d| d.dot(dir) < 0.85);
            if self.pad_walk_ms <= 0.0 || turned {
                self.attack = None;
                self.chasing = false;
                self.pending_npc = None;
                self.pending_skill = None;
                self.pending_pickup = None;
                if self.player.as_ref().is_some_and(|p| p.sitting) {
                    self.send(ro_net::Command::Sit(false));
                }
                let me = self.player.as_ref().map(|p| p.walker.cell);
                let gat = self.world.as_ref().and_then(|w| w.gat.as_ref());
                let goal = me.and_then(|me| {
                    (1..=PAD_STEP).rev().map(|k| (me.0 + (dir.x * k as f32).round() as i32, me.1 + (dir.y * k as f32).round() as i32)).find(|&(x, y)| (x, y) != me && gat.is_some_and(|g| g.walkable(x, y)))
                });
                if let Some((x, y)) = goal {
                    self.request_walk((x as u16, y as u16));
                }
                self.pad_dir = Some(dir);
                self.pad_walk_ms = 200.0;
            }
        } else if self.pad_dir.take().is_some() {
            // Let go: stop on the next cell instead of running on to the old goal.
            if let Some(p) = self.player.as_ref() {
                let c = p.walker.cell;
                self.request_walk((c.0 as u16, c.1 as u16));
            }
        }
    }

    fn stop_fx(&mut self, tag: u32) {
        if let Some(r) = self.renderer.as_mut() {
            r.effects.retain(|e| e.tag != tag);
        }
    }

    fn mob_mut(&mut self, id: u32) -> Option<&mut Mob> {
        if id == self.account_id { self.player.as_mut() } else { self.units.get_mut(&id) }
    }

    /// Someone acted on someone: animate both, and show what happened.
    fn act(&mut self, act: ro_net::Act) {
        let me = self.account_id;
        let cell_of = |app: &Self, id: u32| -> Option<(i32, i32)> {
            if id == me { app.player.as_ref().map(|p| p.walker.cell) } else { app.units.get(&id).map(|m| m.walker.cell) }
        };
        let (src_cell, target_cell) = (cell_of(self, act.src), cell_of(self, act.target));
        self.casts.remove(&act.src);
        match act.kind {
            2 | 3 => {
                if let Some(m) = self.mob_mut(act.src) {
                    m.sitting = act.kind == 2;
                }
                return;
            }
            1 => {
                tracing::debug!(src = act.src, "pick-up animation");
                // Picked something up: crouch (three frames, about 300 ms).
                if let Some(m) = self.mob_mut(act.src) {
                    m.set_pose(Pose::PickUp, 300.0);
                }
                return;
            }
            _ => {}
        }
        if let Some(m) = self.mob_mut(act.src) {
            if let Some(c) = target_cell {
                m.face(c);
            }
            m.set_pose(Pose::Attack, act.src_speed as f32);
        }
        let damage = act.damage + act.left_damage;
        // Players swing with their weapon's sound and land its hit sound; monsters' attacks
        // sound through their animation frames (see the unit update).
        if let Some(kind) = self.weapon_sound(act.src) {
            self.play_sound(&format!("_attack_{kind}.wav"));
            if damage > 0 {
                let hit = if kind == "fist" { format!("_hit_fist{}.wav", act.src % 4 + 1) } else { format!("_hit_{kind}.wav") };
                self.play_sound(&hit);
            }
        }
        if let Some(m) = self.mob_mut(act.target) {
            if damage > 0 && act.kind != 4 && act.kind != 9 {
                if let Some(c) = src_cell {
                    m.face(c);
                }
                m.set_pose(Pose::Hurt, act.dmg_speed as f32);
            }
            let pos = m.actor.pos;
            let (text, color) = match act.kind {
                11 => ("Lucky".to_string(), egui::Color32::from_rgb(140, 220, 255)),
                _ if damage <= 0 => ("Miss".to_string(), egui::Color32::from_rgb(200, 200, 200)),
                10 => (format!("{damage}!"), egui::Color32::from_rgb(255, 210, 60)),
                _ if act.target == me => (damage.to_string(), egui::Color32::from_rgb(255, 90, 90)),
                _ => (damage.to_string(), egui::Color32::WHITE),
            };
            self.floaters.push((pos, text, color, 0.0));
        }
    }

    /// Attacks a monster until it dies or we pick something else.
    fn attack_unit(&mut self, id: u32) {
        self.walk_goal = None;
        self.pending_npc = None;
        self.attack = Some((id, self.attack_range as i16));
        if let Some((tx, _)) = self.net.as_ref() {
            let _ = tx.send(ro_net::Command::Attack { target: id, keep: true });
        }
    }

    /// Applies an NPC script step to the dialog window.
    fn npc_dialog(&mut self, d: ro_net::NpcDialog) {
        use hud::DialogMode;
        use ro_net::NpcDialog::*;
        let title = |units: &std::collections::HashMap<u32, Mob>, npc: u32| units.get(&npc).map(|m| m.name.clone()).unwrap_or_default();
        let open = |dialog: &mut Option<hud::Dialog>, units: &std::collections::HashMap<u32, Mob>, npc: u32| {
            if dialog.as_ref().is_none_or(|d| d.npc != npc) {
                *dialog = Some(hud::Dialog { npc, title: title(units, npc), lines: vec![], mode: DialogMode::Reading });
            }
        };
        match d {
            Say { npc, text } => {
                open(&mut self.dialog, &self.units, npc);
                self.dialog.as_mut().unwrap().lines.push(text);
            }
            // A close with nothing on screen (touch scripts that only `close`): end it silently.
            Close { npc } if self.dialog.as_ref().is_none_or(|d| d.npc != npc || d.lines.is_empty()) => {
                self.dialog = None;
                if let Some((tx, _)) = self.net.as_ref() {
                    let _ = tx.send(ro_net::Command::Npc { npc, reply: ro_net::NpcReply::Close });
                }
            }
            Next { npc } | Close { npc } | Menu { npc, .. } | InputNumber { npc } | InputText { npc } => {
                open(&mut self.dialog, &self.units, npc);
                self.dialog.as_mut().unwrap().mode = match d {
                    Next { .. } => DialogMode::Next,
                    Close { .. } => DialogMode::Close,
                    Menu { options, .. } => DialogMode::Menu(options),
                    InputNumber { .. } => DialogMode::Number(String::new()),
                    _ => DialogMode::Text(String::new()),
                };
            }
            Clear { npc } => {
                if let Some(d) = self.dialog.as_mut().filter(|d| d.npc == npc) {
                    d.lines.clear();
                }
            }
            Cutin { image, kind } => {
                self.cutin = if kind == 255 { None } else { self.load_cutin(&image, kind) };
            }
            Align(a) => self.hud.dialog_layout.align = a,
            Size { width, height } => self.hud.dialog_layout.size = Some((width, height)),
            Pos { x, y } => self.hud.dialog_layout.pos = Some(hud::DialogPos::Pixels(x, y)),
            PosPercent { x, y } => self.hud.dialog_layout.pos = Some(hud::DialogPos::Percent(x, y)),
        }
    }

    /// Loads an NPC illustration into an egui texture.
    fn load_cutin(&self, image: &str, position: u8) -> Option<hud::Cutin> {
        let vfs = self.vfs.as_ref()?;
        let name = if image.contains('.') { image.to_string() } else { format!("{image}.bmp") };
        let path = format!("data\\texture\\유저인터페이스\\illust\\{name}");
        let data = vfs.read(&path).ok().flatten().or_else(|| {
            tracing::warn!(%path, "cutin not found");
            None
        })?;
        let img = ro_formats::image::decode(&data, &path).inspect_err(|e| tracing::warn!(%path, "cutin: {e:#}")).ok()?;
        let color = egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.pixels);
        let texture = self.egui_ctx.load_texture(format!("cutin:{name}"), color, egui::TextureOptions::LINEAR);
        Some(hud::Cutin { texture, position })
    }

    /// Talks to an NPC, walking next to it first when it is out of reach.
    fn talk(&mut self, npc: u32) {
        let (Some(p), Some(m)) = (self.player.as_ref(), self.units.get(&npc)) else { return };
        let (a, b) = (p.walker.cell, m.walker.cell);
        // rAthena lets you talk within a few cells (npc_checknear).
        if (a.0 - b.0).abs().max((a.1 - b.1).abs()) <= 3 {
            if let Some((tx, _)) = self.net.as_ref() {
                let _ = tx.send(ro_net::Command::Npc { npc, reply: ro_net::NpcReply::Contact });
            }
            return;
        }
        // Walk to the nearest walkable cell beside the NPC, then talk.
        let gat = self.world.as_ref().and_then(|w| w.gat.as_ref());
        let target = (-2..=2)
            .flat_map(|dy| (-2..=2).map(move |dx| (b.0 + dx, b.1 + dy)))
            .filter(|&(x, y)| gat.is_some_and(|g| g.walkable(x, y)))
            .min_by_key(|&(x, y)| (x - a.0).abs().max((y - a.1).abs()));
        if let Some((x, y)) = target {
            self.pending_npc = Some(npc);
            self.request_walk((x as u16, y as u16));
        }
    }

    /// The NPC or monster under the cursor, nearest to the camera first.
    fn pick_unit(&self) -> Option<u32> {
        let (r, (cx, cy)) = (self.renderer.as_ref()?, self.cursor?);
        let size = r.size();
        let eye = r.camera.eye();
        self.units
            .iter()
            .filter_map(|(&id, m)| {
                if m.actor.alpha <= 0.01 {
                    return None;
                }
                let feet = r.project(m.actor.pos, size)?;
                // Rough sprite box: about two cells tall, a little over one wide.
                let top = r.project(m.actor.pos + glam::Vec3::Y * 2.2 * ro_render::CELL, size)?;
                let h = (feet.y - top.y).max(8.0);
                let w = h * 0.55;
                let inside = (cx as f32 - feet.x).abs() <= w / 2.0 && (cy as f32) <= feet.y + h * 0.1 && (cy as f32) >= top.y;
                inside.then(|| (id, m.actor.pos.distance_squared(eye)))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    /// Ground cell under the cursor, if any.
    fn pick(&self) -> Option<(u16, u16)> {
        let (r, w, (cx, cy)) = (self.renderer.as_ref()?, self.world.as_ref()?, self.cursor?);
        let (origin, dir) = r.ray(cx as f32, cy as f32);
        // March along the ray until it passes below the ground.
        let mut t = 0.0;
        while t < 5000.0 {
            let p = origin + dir * t;
            if p.y <= w.ground.height_at(p.x, p.z) {
                let (x, y) = ((p.x / ro_render::CELL).floor(), (p.z / ro_render::CELL).floor());
                return (x >= 0.0 && y >= 0.0).then_some((x as u16, y as u16));
            }
            t += 1.0;
        }
        None
    }

    /// One frame: network events, movement, camera, render (or capture).
    fn tick(&mut self, el: &ActiveEventLoop) {
        let now = std::time::Instant::now();
        let dt = (now - self.last_tick).as_secs_f32() * 1000.0;
        self.last_tick = now;
        if self.front.is_none() {
            self.pad_tick(dt);
        } else if let (Some(r), false) = (self.renderer.as_mut(), self.still) {
            // Behind the login screens the backdrop slowly turns.
            r.camera.rotate(dt * 0.00004, 0.0);
        }
        self.cursor_action = self.hover_cursor();
        self.steer_to_cursor(dt);
        self.companion_follow(now);

        // `--do a,b,c`: scripted actions once the inventory has arrived (tests).
        // Scripted helpers for two-client tests: accept requests, run a trade to completion.
        if self.auto_accept {
            if let Some((inviter, _)) = self.guild.ally_offer.take() {
                self.send(ro_net::Command::Relation(ro_net::RelationRequest::Answer { inviter, accept: true }));
            }
            if let Some(c) = self.social.accept_first() {
                tracing::info!(?c, "auto-accept");
                self.send(c);
            }
        }
        if let (Some(item), Some(t)) = (self.auto_trade, self.social.trade.as_ref()) {
            match self.trade_stage {
                0 => {
                    if item != 0 {
                        if let Some(index) = self.inventory.items.values().find(|i| i.id == item).map(|i| i.index) {
                            self.send(ro_net::Command::Trade(ro_net::TradeRequest::Add { index, amount: 1 }));
                        }
                    }
                    self.send(ro_net::Command::Trade(ro_net::TradeRequest::Ok));
                    self.trade_stage = 1;
                }
                1 if t.we_locked && t.they_locked => {
                    tracing::info!(theirs = ?t.theirs, ours = ?t.ours, "trade: both locked, committing");
                    self.send(ro_net::Command::Trade(ro_net::TradeRequest::Commit));
                    self.trade_stage = 2;
                }
                _ => {}
            }
        }
        if !self.script.is_empty() && !self.inventory.items.is_empty() && self.stats.is_some() && self.ready_at.is_some_and(|t| t.elapsed().as_millis() > 1500) && self.script_resume.is_none_or(|t| std::time::Instant::now() >= t) {
            let mut steps = std::mem::take(&mut self.script).into_iter();
            while let Some(step) = steps.next() {
                // wait:ms pauses the script; the rest runs on a later frame.
                if let Some(ms) = step.strip_prefix("wait:").and_then(|m| m.parse::<u64>().ok()) {
                    self.script = steps.collect();
                    self.script_resume = Some(std::time::Instant::now() + std::time::Duration::from_millis(ms));
                    break;
                }
                let item = |id: u32| self.inventory.items.values().find(|i| i.id == id).map(|i| (i.index, i.location));
                let action = match step.split_once(':').or(Some((step.as_str(), ""))) {
                    Some(("equip", id)) => id.parse().ok().and_then(item).map(|(index, location)| windows::UiAction::Equip { index, location }),
                    Some(("use", id)) => id.parse().ok().and_then(item).map(|(index, _)| windows::UiAction::UseItem(index)),
                    Some((f @ ("fav" | "unfav"), id)) => id.parse().ok().and_then(item).map(|(index, _)| windows::UiAction::Favorite { index, on: f == "fav" }),
                    Some(("invtab", n)) => {
                        self.windows.inventory_tab(n.parse().unwrap_or(0));
                        None
                    }
                    Some(("card", id)) => id.parse().ok().and_then(item).map(|(index, _)| windows::UiAction::CardTargets(index)),
                    // refsel:item picks equipment in the refine window; refine:material refines it.
                    // gradesel:item picks equipment in the grade window; gradeup:material tries.
                    Some(("gradesel", id)) => {
                        if let Some(index) = id.parse().ok().and_then(item).map(|(index, _)| index) {
                            self.grade.pick(index);
                            self.queued_commands.push(ro_net::Command::Grade(ro_net::GradeRequest::Select(index)));
                        }
                        None
                    }
                    Some(("gradeup", m)) => {
                        if let Some(r) = self.grade.upgrade(m.parse().unwrap_or(0)) {
                            self.queued_commands.push(ro_net::Command::Grade(r));
                        }
                        None
                    }
                    // navi:map:x:y guides there; navigo walks to the next stop.
                    Some(("navi", spec)) => {
                        let f: Vec<&str> = spec.split(':').collect();
                        self.navi.ensure_loaded(self.vfs.as_ref());
                        let cell = match (f.get(1).and_then(|x| x.parse().ok()), f.get(2).and_then(|y| y.parse().ok())) {
                            (Some(x), Some(y)) => Some((x, y)),
                            _ => None,
                        };
                        self.navi.go(f[0], cell, f[0].to_string());
                        None
                    }
                    Some(("navigo", _)) => {
                        let map = self.world.as_ref().map(|w| w.name.clone()).unwrap_or_default();
                        if let Some(p) = self.player.as_ref() {
                            self.navi.update(&map, (p.walker.cell.0 as u16, p.walker.cell.1 as u16));
                        }
                        self.navi_walk = self.navi.waypoint(&map);
                        None
                    }
                    // bg:Name applies solo to a battleground; bgauto enters it when ready.
                    Some(("walk", xy)) => {
                        if let Some((x, y)) = xy.split_once('_').and_then(|(x, y)| Some((x.parse().ok()?, y.parse().ok()?))) {
                            self.request_walk((x, y));
                        }
                        None
                    }
                    Some(("lapine", _)) => {
                        let inv: Vec<&ro_game::items::InvItem> = self.inventory.items.values().collect();
                        if let Some(a) = self.lapine.test_go(&inv) {
                            self.lapine_action(a);
                        }
                        None
                    }
                    // cam:yaw_pitch in degrees (test hook for camera angles).
                    Some(("cam", yp)) => {
                        if let (Some((y, p)), Some(r)) = (yp.split_once('_').and_then(|(y, p)| Some((y.parse::<f32>().ok()?, p.parse::<f32>().ok()?))), self.renderer.as_mut()) {
                            r.camera.yaw = y.to_radians();
                            r.camera.pitch = p.to_radians().clamp(25f32.to_radians(), 75f32.to_radians());
                        }
                        None
                    }
                    Some(("comptab", n)) => {
                        self.companions.select_tab(n.parse().unwrap_or(0));
                        None
                    }
                    Some(("petmenu", n)) => {
                        if let Ok(n) = n.parse() {
                            self.queued_commands.push(ro_net::Command::Companion(ro_net::CompanionRequest::PetMenu(n)));
                        }
                        None
                    }
                    Some(("bg", name)) => {
                        self.battle.ensure_loaded(self.vfs.as_ref());
                        self.battle.open = true;
                        self.queued_commands.push(ro_net::Command::Bg(ro_net::BgRequest::Apply { kind: 1, name: name.to_string() }));
                        None
                    }
                    Some(("bgauto", _)) => {
                        self.battle.auto_accept = true;
                        None
                    }
                    // mbuy:row:n buys from an open market or barter shop.
                    Some(("mbuy", spec)) => {
                        let f: Vec<u32> = spec.split(':').filter_map(|v| v.parse().ok()).collect();
                        if let (Some(row), Some(n)) = (f.first(), f.get(1)) {
                            self.market_buy = Some((*row as usize, *n));
                            if self.market.shop.is_some() {
                                self.market_test_buy();
                            }
                        }
                        None
                    }
                    Some(("emblem", f)) => {
                        if let Some(c) = self.guild.upload_emblem(f) {
                            self.queued_commands.push(c);
                        }
                        None
                    }
                    Some(("enchant", id)) => {
                        if let Some(r) = id.parse().ok().and_then(item).and_then(|(index, _)| self.enchant.test_random(index)) {
                            self.queued_commands.push(ro_net::Command::Enchant(r));
                        }
                        None
                    }
                    Some(("reform", _)) => {
                        let inv: Vec<&ro_game::items::InvItem> = self.inventory.items.values().collect();
                        if let Some(reform::ReformAction::Reform { tool, index }) = self.reform.test_reform(&inv) {
                            self.queued_commands.push(ro_net::Command::Reform(ro_net::ReformRequest::Reform { tool, index }));
                        }
                        None
                    }
                    Some(("style", h)) => {
                        let o = self.stylist.test_order(h.parse().unwrap_or(1));
                        self.queued_commands.push(ro_net::Command::Style(ro_net::StyleRequest::Buy { hair_color: o.hair_color, hair: o.hair, clothes_color: o.clothes_color }));
                        None
                    }
                    Some(("ssearch", id)) => {
                        let r = self.store_search.test_search(id.parse().unwrap_or(0));
                        self.queued_commands.push(ro_net::Command::Search(r));
                        None
                    }
                    Some(("sclick", _)) => {
                        if let Some(f) = self.store_search.results.first() {
                            self.queued_commands.push(ro_net::Command::Search(ro_net::SearchRequest::Click { owner: f.owner, store: f.store, item: f.item }));
                        }
                        None
                    }
                    Some(("option", spec)) => {
                        if let Some((k, v)) = spec.split_once(':') {
                            self.queued_commands.push(ro_net::Command::Option(ro_net::OptionRequest::Config { kind: k.parse().unwrap_or(0), on: v == "1" }));
                        }
                        None
                    }
                    Some(("swadd", id)) => id.parse().ok().and_then(item).map(|(index, location)| windows::UiAction::SwitchAdd { index, location: location & 0x3ff }),
                    Some(("swap", _)) => Some(windows::UiAction::SwitchAll),
                    Some(("tame", _)) => {
                        let me = self.player.as_ref().map(|p| p.walker.cell).unwrap_or_default();
                        let target = self.units.iter().filter(|(_, m)| m.kind == 5 && m.dying.is_none()).min_by_key(|(_, m)| (m.walker.cell.0 - me.0).pow(2) + (m.walker.cell.1 - me.1).pow(2)).map(|(id, _)| *id);
                        if let (Some(id), true) = (target, std::mem::take(&mut self.capturing)) {
                            self.send(ro_net::Command::Companion(ro_net::CompanionRequest::Capture(id)));
                        }
                        None
                    }
                    Some(("ally", who)) => {
                        if let Some(id) = self.units.iter().find(|(_, m)| m.name == who).map(|(id, _)| *id) {
                            self.send(ro_net::Command::Relation(ro_net::RelationRequest::Ally { target: id, me: self.account_id, char_id: self.char_id }));
                        }
                        None
                    }
                    Some(("mergeall", _)) => {
                        if let Some(list) = self.merge.open.take() {
                            let first = list.first().and_then(|i| self.inventory.items.get(i)).map(|i| i.id);
                            let same: Vec<u16> = list.into_iter().filter(|i| self.inventory.items.get(i).map(|x| x.id) == first).collect();
                            self.queued_commands.push(ro_net::Command::Merge(same));
                        }
                        None
                    }
                    Some(("equiptab", t)) => {
                        self.windows.equip_tab = t.parse().unwrap_or(0);
                        None
                    }
                    Some(("viewequip", aid)) => {
                        aid.parse().ok().map(|aid| self.queued_commands.push(ro_net::Command::ViewEquip(aid)));
                        None
                    }
                    Some(("showequip", on)) => Some(windows::UiAction::ShowEquip(on == "1")),
                    Some(("bank", _)) => {
                        self.toggle_bank();
                        None
                    }
                    Some(("deposit", z)) => {
                        z.parse().ok().map(|zeny| self.queued_commands.push(ro_net::Command::Bank(ro_net::BankRequest::Deposit { aid: self.account_id, zeny })));
                        None
                    }
                    Some(("refsel", id)) => {
                        if let Some((index, _)) = id.parse().ok().and_then(item) {
                            self.refine.selected = Some((index, 0, vec![]));
                            self.queued_commands.push(ro_net::Command::Refine(ro_net::RefineRequest::Select(index)));
                        }
                        None
                    }
                    Some(("refine", mat)) => {
                        if let (Some((index, _, _)), Ok(material)) = (self.refine.selected.as_ref(), mat.parse()) {
                            self.queued_commands.push(ro_net::Command::Refine(ro_net::RefineRequest::Refine { index: *index, material, blessing: false }));
                        }
                        None
                    }
                    Some(("info", id)) => id.parse().ok().map(|id| windows::UiAction::ItemInfo { id, refine: 0, cards: [0; 4] }),
                    // Merchant tests: cartwin opens the cart; vend:price fills the shop
                    // set-up with everything in the cart; browse:aid looks in a shop; buyone
                    // buys one of each item in the shop being browsed.
                    Some(("cartwin", _)) => {
                        self.merchant.cart_open = true;
                        None
                    }
                    Some(("vend", price)) => {
                        let price: u32 = price.parse().unwrap_or(100);
                        if let Some((slots, title, _)) = self.merchant.setup.take() {
                            let items = self.merchant.cart.values().take(slots as usize).map(|it| (it.index, it.amount, price)).collect();
                            self.queued_commands.push(ro_net::Command::Merchant(ro_net::MerchantRequest::OpenShop { title, items }));
                        }
                        None
                    }
                    // Buying store / cash shop tests: buystore:id:amount:price opens a store
                    // from the set-up; buybrowse:aid looks in one; sellall sells everything it
                    // wants that we carry; cashshop toggles the shop; cashbuy:id buys one.
                    Some(("buystore", spec)) => {
                        let f: Vec<u32> = spec.split(':').filter_map(|v| v.parse().ok()).collect();
                        if let (Some((_, title, _, _)), [id, amount, price]) = (self.stores.setup.take(), f.as_slice()) {
                            let limit = self.hud.status.zeny.clamp(0, u32::MAX as i64) as u32;
                            self.queued_commands.push(ro_net::Command::BuyingStore(ro_net::BuyingStoreRequest::Open { limit, title, items: vec![(*id, *amount as u16, *price)] }));
                        }
                        None
                    }
                    Some(("buybrowse", aid)) => {
                        aid.parse().ok().map(|aid| self.queued_commands.push(ro_net::Command::BuyingStore(ro_net::BuyingStoreRequest::Browse(aid))));
                        None
                    }
                    Some(("sellall", _)) => {
                        if let Some((buyer, store, _, items, _)) = self.stores.browsing.take() {
                            let list = items
                                .iter()
                                .filter_map(|w| self.inventory.items.values().find(|i| i.id == w.id && i.equipped == 0).map(|i| (i.index, w.id, w.amount.min(i.amount))))
                                .collect();
                            self.queued_commands.push(ro_net::Command::BuyingStore(ro_net::BuyingStoreRequest::Sell { buyer, store, items: list }));
                        }
                        None
                    }
                    // mail:to:title:zeny writes and sends a mail.
                    Some(("mail", spec)) => {
                        let f: Vec<&str> = spec.split(':').collect();
                        // mail:to:title:zeny:itemid also attaches one of that item.
                        if let [to, title, zeny, rest @ ..] = f.as_slice() {
                            let [begin, check] = self.mailbox.compose(to, title, "Sent by the test script.", zeny);
                            self.send(ro_net::Command::Mail(begin));
                            if let Some(item) = rest.first().and_then(|s| s.parse::<u32>().ok()) {
                                if let Some(i) = self.inventory.items.values().find(|i| i.id == item && i.equipped == 0) {
                                    let index = i.index;
                                    self.send(ro_net::Command::Mail(ro_net::MailRequest::AddItem { index, amount: 1 }));
                                }
                            }
                            self.send(ro_net::Command::Mail(check));
                        }
                        None
                    }
                    // room:title creates a public chat room; joinroom joins the first one in sight.
                    Some(("room", title)) => {
                        self.send(ro_net::Command::ChatRoom(ro_net::ChatRoomRequest::Create { title: title.into(), limit: 10, public: true, password: String::new(), change: false }));
                        None
                    }
                    Some(("joinroom", _)) => {
                        if let Some(id) = self.rooms.signs.values().next().map(|r| r.id) {
                            self.send(ro_net::Command::ChatRoom(ro_net::ChatRoomRequest::Join { id, password: String::new() }));
                        }
                        None
                    }
                    // postad posts a party booking ad at our level minus ten.
                    Some(("postad", _)) => {
                        let level = (self.hud.status.base_level.clamp(1, 999) as u16).saturating_sub(10).max(1);
                        self.send(ro_net::Command::Booking(ro_net::BookingRequest::Register { level, map: 0, jobs: vec![] }));
                        None
                    }
                    // altkey:x presses Alt+x.
                    Some(("altkey", k)) => {
                        self.alt_key(k);
                        None
                    }
                    // hover:x:y holds the UI pointer there (UI points), for tooltips.
                    Some(("hover", at)) => {
                        let f: Vec<f32> = at.split(':').filter_map(|v| v.parse().ok()).collect();
                        self.fake_pointer = (f.len() == 2).then(|| egui::pos2(f[0], f[1]));
                        None
                    }
                    // click:x:y clicks the UI there.
                    // drag:x1:y1:x2:y2 presses at one point, moves over a few frames, releases at the other.
                    // idle:n holds the next test drag for n frames.
                    Some(("idle", n)) => {
                        for _ in 0..n.parse().unwrap_or(0) {
                            self.fake_frames.push_back(vec![]);
                        }
                        None
                    }
                    Some(("drag", at)) => {
                        let f: Vec<f32> = at.split(':').filter_map(|v| v.parse().ok()).collect();
                        if f.len() == 4 {
                            let (a, b) = (egui::pos2(f[0], f[1]), egui::pos2(f[2], f[3]));
                            let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
                            self.fake_frames.push_back(vec![egui::Event::PointerMoved(a)]);
                            self.fake_frames.push_back(vec![button(a, true)]);
                            for t in [0.1, 0.3, 0.6, 0.9, 1.0, 1.0] {
                                self.fake_frames.push_back(vec![egui::Event::PointerMoved(a + (b - a) * t)]);
                            }
                            self.fake_frames.push_back(vec![button(b, false)]);
                        }
                        None
                    }
                    Some(("click", at)) => {
                        let f: Vec<f32> = at.split(':').filter_map(|v| v.parse().ok()).collect();
                        self.fake_click = (f.len() == 2).then(|| (egui::pos2(f[0], f[1]), true));
                        None
                    }
                    Some(("cashshop", _)) => {
                        self.toggle_cash_shop();
                        None
                    }
                    Some(("cashbuy", id)) => {
                        let id: u32 = id.parse().unwrap_or(0);
                        if let Some(tab) = self.stores.cash_tabs.iter().find(|(_, items)| items.iter().any(|(i, _)| *i == id)).map(|(t, _)| *t) {
                            self.queued_commands.push(ro_net::Command::CashShop(ro_net::CashShopRequest::Buy { kafra: 0, items: vec![(id, 1, tab)] }));
                        }
                        None
                    }
                    Some(("expand", "")) => Some(windows::UiAction::ExpandInventory),
                    Some(("expand", "yes")) => Some(windows::UiAction::ExpandConfirm(true)),
                    Some(("offall", mask)) => u32::from_str_radix(mask, 16).ok().map(windows::UiAction::UnequipAll),
                    Some(("browse", aid)) => {
                        aid.parse().ok().map(|aid| self.queued_commands.push(ro_net::Command::Merchant(ro_net::MerchantRequest::Browse(aid))));
                        None
                    }
                    Some(("buyone", _)) => {
                        if let Some((seller, shop, items, _)) = self.merchant.browsing.take() {
                            let list = items.iter().map(|i| (1, i.index)).collect();
                            self.queued_commands.push(ro_net::Command::Merchant(ro_net::MerchantRequest::Buy { seller, shop, items: list }));
                        }
                        None
                    }
                    // pick:id answers the open menu with that item/skill id (or inventory index).
                    Some(("pick", id)) => {
                        let id: u32 = id.parse().unwrap_or(0);
                        self.windows.menu.take().and_then(|menu| {
                            use ro_net::{Menu, MenuReply};
                            let reply = match menu {
                                Menu::Items(ids) => ids.contains(&id).then_some(MenuReply::Item(id)),
                                Menu::Make { kind, items } => items.contains(&id).then_some(MenuReply::Make { kind, item: id }),
                                Menu::Produce(list) => list.iter().find(|(i, _)| *i == id).map(|(i, m)| MenuReply::Produce { item: *i, materials: *m }),
                                Menu::AutoSpell(s) => s.contains(&id).then_some(MenuReply::AutoSpell(id)),
                                Menu::Skill { flag, skills } => skills.contains(&(id as u16)).then_some(MenuReply::Skill { flag, skill: id as u16 }),
                                Menu::Identify(list) => {
                                    let by_item = self.inventory.items.values().find(|i| i.id == id && !i.identified && list.contains(&i.index)).map(|i| i.index);
                                    Some(MenuReply::Identify(by_item.unwrap_or(id as u16)))
                                }
                                Menu::Repair(list) => list.iter().find(|r| r.1 == id).map(|&(index, item, refine)| MenuReply::Repair { index, item, refine }),
                                Menu::Egg(_) => Some(MenuReply::Egg(self.inventory.items.values().find(|i| i.id == id).map_or(id as u16, |i| i.index))),
                                Menu::Warp { skill, maps } => maps.get(id as usize).map(|m| MenuReply::Warp { skill, map: m.clone() }),
                            };
                            reply.map(windows::UiAction::Menu)
                        })
                    }
                    Some(("raise", n)) => n.parse().ok().map(windows::UiAction::RaiseStat),
                    Some(("skillup", id)) => id.parse().ok().map(windows::UiAction::SkillUp),
                    // hotkey:slot=skill_id (at its learned level), key:slot presses it.
                    Some(("hotkey", spec)) => spec.split_once('=').and_then(|(slot, id)| {
                        let (index, id) = (slot.parse().ok()?, id.parse::<u16>().ok()?);
                        let level = self.skills.iter().find(|s| s.id == id)?.level as i16;
                        Some(windows::UiAction::SetHotkey { index, key: ro_net::Hotkey { skill: true, id: id as u32, count: level } })
                    }),
                    Some(("key", slot)) => slot.parse().ok().map(windows::UiAction::UseHotkey),
                    // selfcast:skill_id on ourselves.
                    Some(("selfcast", id)) => {
                        if let Ok(skill) = id.parse::<u16>() {
                            let level = self.skills.iter().find(|s| s.id == skill).map_or(1, |s| s.level.max(1));
                            self.use_skill_on(skill, level, self.account_id);
                        }
                        None
                    }
                    // cast:skill_id on the nearest monster.
                    Some(("cast", id)) => {
                        let me = self.player.as_ref().map_or((0, 0), |p| p.walker.cell);
                        let target = self
                            .units
                            .iter()
                            .filter(|(_, m)| m.kind == 5)
                            .min_by_key(|(_, m)| (m.walker.cell.0 - me.0).pow(2) + (m.walker.cell.1 - me.1).pow(2))
                            .map(|(id, _)| *id);
                        tracing::info!(?target, monsters = self.units.values().filter(|m| m.kind == 5).count(), "script cast {id}");
                        if let (Ok(skill), Some(target)) = (id.parse::<u16>(), target) {
                            let level = self.skills.iter().find(|s| s.id == skill).map_or(1, |s| s.level.max(1));
                            self.use_skill_on(skill, level, target);
                        }
                        None
                    }
                    Some(("party", name)) => {
                        self.send(ro_net::Command::Party(ro_net::PartyRequest::Create { name: name.into() }));
                        None
                    }
                    Some(("invite", name)) => {
                        self.send(ro_net::Command::Party(ro_net::PartyRequest::Invite { name: name.into() }));
                        None
                    }
                    Some(("guild", name)) => {
                        self.send(ro_net::Command::Guild(ro_net::GuildRequest::Create { char_id: self.char_id, name: name.into() }));
                        None
                    }
                    Some(("friend", name)) => {
                        self.send(ro_net::Command::Friend(ro_net::FriendRequest::Add { name: name.into() }));
                        None
                    }
                    Some(("trade", name)) => {
                        let id = self.units.iter().find(|(_, m)| m.name == name).map(|(id, _)| *id);
                        if let Some(target) = id {
                            self.trade_partner = Some(name.into());
                            self.send(ro_net::Command::Trade(ro_net::TradeRequest::Ask { target }));
                        }
                        None
                    }
                    Some(("drop", id)) => id.parse().ok().and_then(item).map(|(index, _)| windows::UiAction::Drop { index, amount: 1 }),
                    _ if step == "charselect" => {
                        self.send(ro_net::Command::Restart(1));
                        None
                    }
                    _ if step == "restart" => {
                        self.send(ro_net::Command::Restart(0));
                        None
                    }
                    _ if step == "pickup" => {
                        self.auto_pickup = true;
                        None
                    }
                    _ => None,
                };
                tracing::info!(%step, ?action, "script");
                if let Some(a) = action {
                    self.ui_action(a);
                }
            }
        }
        if self.auto_pickup {
            let me = self.player.as_ref().map(|p| p.walker.cell);
            let nearest = me.and_then(|a| self.floor.values().min_by_key(|(f, _)| (f.x as i32 - a.0).abs().max((f.y as i32 - a.1).abs())).map(|(f, _)| f.id));
            if let Some(id) = nearest {
                self.auto_pickup = false;
                tracing::info!(id, "picking up");
                self.pick_up(id);
            }
        }
        if std::mem::take(&mut self.open_windows) {
            let args: Vec<String> = std::env::args().collect();
            self.quests.open = args.iter().any(|a| a == "--quest-win");
            if args.iter().any(|a| a == "--skills-win") { self.windows.skills = true; self.windows.skill_list = args.iter().any(|a| a == "--skill-list"); self.windows.skill_tab = args.iter().skip_while(|a| *a != "--guild-tab").nth(1).and_then(|t| t.parse().ok()); } else { self.windows.guild = true; }
            self.guild.start_tab = std::env::args().skip_while(|a| a != "--guild-tab").nth(1).and_then(|t| t.parse().ok()).unwrap_or(0);
        }
        for text in std::mem::take(&mut self.queued_chat) {
            self.submit_chat(text);
        }
        for c in std::mem::take(&mut self.queued_commands) {
            self.send(c);
        }
        if let Some(a) = self.queued_action.take() {
            self.ui_action(a);
        }
        let events: Vec<_> = self.net.as_ref().map(|(_, rx)| rx.try_iter().collect()).unwrap_or_default();
        for e in events {
            self.handle(e);
        }

        // Chasing an out-of-reach target: attack again once close enough (or arrived).
        if self.chasing {
            if let (Some((target, range)), Some(p)) = (self.attack, self.player.as_ref()) {
                let near = self.units.get(&target).is_some_and(|m| {
                    let (a, b) = (p.walker.cell, m.walker.cell);
                    (a.0 - b.0).abs().max((a.1 - b.1).abs()) <= range as i32
                });
                if near || (self.walk_goal.is_none() && !p.walker.is_walking()) {
                    self.chasing = false;
                    self.attack_unit(target);
                }
            } else {
                self.chasing = false;
            }
        }
        self.units.retain(|_, m| m.dying.is_none_or(|t| t > 0.0));
        self.casts.retain(|_, (t, total)| {
            *t += dt;
            *t < *total
        });
        self.floaters.retain_mut(|f| {
            f.3 += dt;
            f.3 < 1200.0
        });

        if self.auto_attack {
            let me = self.player.as_ref().map(|p| p.walker.cell);
            let nearest = me.and_then(|a| {
                self.units
                    .iter()
                    .filter(|(_, m)| m.kind == 5 && m.dying.is_none())
                    .min_by_key(|(_, m)| (m.walker.cell.0 - a.0).abs().max((m.walker.cell.1 - a.1).abs()))
                    .map(|(id, _)| *id)
            });
            if let Some(id) = nearest {
                self.auto_attack = false;
                self.attack_unit(id);
            }
        }
        if let Some(name) = self.talk_to.clone() {
            if let Some(id) = self.units.iter().find(|(_, m)| m.name.eq_ignore_ascii_case(&name)).map(|(id, _)| *id) {
                self.talk_to = None;
                self.talk(id);
            }
        }

        // Pick up the item we were walking to once we have arrived.
        if self.pending_pickup.is_some() && self.walk_goal.is_none() && self.player.as_ref().is_some_and(|p| !p.walker.is_walking()) {
            if let Some(id) = self.pending_pickup.take() {
                tracing::debug!(id, "pickup request");
            self.send(ro_net::Command::Item(ro_net::ItemRequest::PickUp { floor_id: id }));
            }
        }

        // Talk to the NPC we were walking to once we have arrived.
        if self.pending_npc.is_some() && self.walk_goal.is_none() && self.player.as_ref().is_some_and(|p| !p.walker.is_walking()) {
            if let Some(npc) = self.pending_npc.take() {
                self.talk(npc);
            }
        }
        // `--fx skill:<id>`: replay a skill's effect on the player whenever the last one ended.
        if let Some(id) = self.fx_test.and_then(|n| n.strip_prefix("skill:")).and_then(|n| n.parse::<u16>().ok()) {
            let idle = self.renderer.as_ref().is_some_and(|r| r.effects.is_empty() && r.procs.is_empty()) && self.sprite_fx.is_empty();
            if idle {
                let pos = self.unit_pos(self.account_id).or_else(|| self.player.as_ref().map(|p| p.actor.pos));
                if let (137, Some(p)) = (id, pos) {
                    self.spike_line(p, p + glam::Vec3::new(6.0 * ro_render::CELL, 0.0, -3.0 * ro_render::CELL));
                }
                self.skill_fx(id, pos, true);
            }
        }
        // Cast the skill we walked into range for (re-walks if the target moved).
        if let (Some((skill, level, target, _)), None) = (self.pending_skill, self.walk_goal.as_ref()) {
            if self.player.as_ref().is_some_and(|p| !p.walker.is_walking()) {
                if self.units.contains_key(&target) {
                    self.use_skill_on(skill, level, target);
                } else {
                    self.pending_skill = None;
                }
            }
        }

        // Continue a long walk once the current hop has finished (or the server ignored it).
        let idle = self.player.as_ref().is_some_and(|p| !p.walker.is_walking());
        if idle && self.walk_goal.as_ref().is_some_and(|g| g.sent_at.is_none_or(|t| t.elapsed().as_millis() > 500)) {
            self.next_hop();
        }

        self.update_room();
        if let Some(a) = self.companion_action.take() {
            match a {
                // With a script running, commands go to it (as Alt+right-click does in the
                // original): ATTACK_OBJECT_CMD 3, FOLLOW_CMD 9.
                companions::CompanionAction::Send(r) => {
                    if r == ro_net::CompanionRequest::MercMenu(2) {
                        // The server says nothing on a dismissal.
                        self.companions.merc = None;
                        self.companions.merc_skills.clear();
                        self.merc_ai = None;
                    }
                    let msg = match r {
                        ro_net::CompanionRequest::Attack { id, target } => Some((id, vec![3, target as i64])),
                        ro_net::CompanionRequest::Return(id) => Some((id, vec![9])),
                        _ => None,
                    };
                    let brain = msg.as_ref().and_then(|(id, _)| if *id == self.companions.homun_id { self.homun_ai.as_ref() } else { self.merc_ai.as_ref() });
                    match (brain, msg) {
                        (Some(b), Some((_, m))) => b.world.borrow_mut().msg = m,
                        _ => self.send(ro_net::Command::Companion(r)),
                    }
                }
                companions::CompanionAction::Skill { id, level, on } => self.use_skill_on(id, level, on),
                companions::CompanionAction::SkillUp(id) => self.send(ro_net::Command::Stat(ro_net::StatRequest::SkillUp(id))),
            }
        }
        if let Some(key) = self.hud.menu_click.take().or_else(|| self.windows.menu_key.take()) {
            self.alt_key(key);
        }
        if self.windows.equipment || self.stylist.open {
            self.refresh_portrait();
        }
        if let (Some(w), Some(p)) = (self.world.as_ref(), self.player.as_ref()) {
            let (map, me) = (w.name.clone(), (p.walker.cell.0 as u16, p.walker.cell.1 as u16));
            self.navi.update(&map, me);
            self.battle.map_changed(&map);
        }
        // Hat effects follow their wearer; they end when the wearer leaves view.
        if !self.hat_fx.is_empty() {
            let placed: Vec<(u32, Option<glam::Vec3>)> = self.hat_fx.iter().map(|((unit, _), tag)| (*tag, self.unit_pos(*unit))).collect();
            let gone: std::collections::HashSet<u32> = placed.iter().filter(|(_, p)| p.is_none()).map(|(t, _)| *t).collect();
            self.hat_fx.retain(|_, tag| !gone.contains(tag));
            if let Some(r) = self.renderer.as_mut() {
                for (tag, pos) in placed {
                    match pos {
                        Some(p) => r.effects.iter_mut().filter(|e| e.tag == tag).for_each(|e| e.pos = p),
                        None => r.effects.retain(|e| e.tag != tag),
                    }
                }
            }
        }
        if let Some(cell) = self.navi_walk.take() {
            self.request_walk(cell);
        }
        let (Some(r), Some(window)) = (self.renderer.as_mut(), self.window.as_ref()) else { return };
        r.actors.clear();
        r.warps = self
            .warps
            .values()
            .filter_map(|&(x, y)| {
                let w = self.world.as_ref()?;
                if !w.shows_cell(x as i32, y as i32) {
                    return None;
                }
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                let (wx, wz) = (fx * ro_render::CELL, fy * ro_render::CELL);
                let h = w.gat.as_ref().and_then(|g| g.height_at(fx, fy)).map_or_else(|| w.ground.height_at(wx, wz), |h| -h);
                Some(glam::Vec3::new(wx, h, wz))
            })
            .collect();
        if let Some(w) = self.world.as_ref() {
            let octant = (r.camera.yaw / std::f32::consts::FRAC_PI_4).round() as i32;
            if let Some(p) = self.player.as_mut() {
                // Combat stance (weapon drawn) while attacking something.
                let idle = if self.attack.is_some() && self.action == 0 { 4 } else { self.action };
                p.attack_group = attack_group(r, &p.actor);
                p.fit_pose(r);
                let a = p.update(dt, w, octant, idle);
                r.camera.target = a.pos;
                r.actors.push(a);
            }
            for m in self.units.values_mut() {
                let (action, time) = (m.actor.action, m.actor.time_ms);
                m.attack_group = attack_group(r, &m.actor);
                m.fit_pose(r);
                let a = m.update(dt, w, octant, 0);
                // Attack, hurt and death frames carry their sounds in the ACT.
                if let (Some(_), Some(audio), Some(vfs)) = (m.pose, self.audio.as_ref(), self.vfs.as_ref()) {
                    let from = if a.action == action && time > 0.0 && a.time_ms >= time { time } else { -1.0 };
                    if let Some(s) = r.actor_sound(&a, from) {
                        audio.play(vfs, &s);
                    }
                }
                r.actors.push(a);
            }
            // Ambient map effects (RSW emitters, EF_* ids): torches loop in place; bubbles and
            // fireflies fire again on each emitter's own timer while the player is near.
            if let (Some(vfs), Some(p)) = (self.vfs.as_ref(), self.player.as_ref()) {
                let me = p.actor.pos;
                for e in &mut self.map_fx {
                    let near = e.pos.distance(me) < fx::MAP_FX_RANGE * ro_render::CELL;
                    match e.visual {
                        Some(fx::MapVisual::Sprite(name)) => {
                            e.clock_ms += dt;
                            if let (true, Some(body)) = (w.shows_cell((e.pos.x / ro_render::CELL) as i32, (e.pos.z / ro_render::CELL) as i32), self.fx.sprite(r, vfs, name)) {
                                r.actors.push(ro_render::Actor { pos: e.pos, body, head: None, headgear: vec![], weapon: vec![], shadow: None, action: 0, dir: ro_render::Dir::S, time_ms: e.clock_ms, once: false, alpha: 1.0 });
                            }
                        }
                        Some(visual) => {
                            e.clock_ms -= dt;
                            if e.clock_ms > 0.0 {
                                continue;
                            }
                            e.clock_ms = e.every_ms;
                            if !near {
                                continue;
                            }
                            e.fired += 1;
                            match visual {
                                fx::MapVisual::Str(names) => {
                                    if let Some(asset) = self.fx.get(r, vfs, names[(e.fired as usize * 7) % names.len()]) {
                                        r.effects.push(ro_render::Fx { asset, pos: e.pos, time_ms: 0.0, looping: false, tag: 0 });
                                    }
                                }
                                _ => {
                                    if let Some(tex) = self.fx.texture(r, vfs, "pok1.bmp") {
                                        let a = e.fired as f32 * 2.399;
                                        let drift = glam::Vec3::new(a.cos(), 0.25, a.sin()) * ro_render::CELL * 0.8;
                                        let shape = ro_render::Shape::Motes { drift, size: 1.6 };
                                        r.procs.push(ro_render::ProcFx { tex, shape, pos: e.pos, color: [0.85, 0.95, 0.8], time_ms: 0.0, duration_ms: e.every_ms.clamp(1500.0, 4000.0) });
                                    }
                                }
                            }
                        }
                        None => {}
                    }
                }
            }
            if let (Some(name), true, Some(vfs), Some(p)) = (self.fx_test.filter(|n| !n.starts_with("skill:")), r.effects.is_empty(), self.vfs.as_ref(), self.player.as_ref()) {
                if let Some(asset) = self.fx.get(r, vfs, name) {
                    r.effects.push(ro_render::Fx { asset, pos: p.actor.pos, time_ms: 0.0, looping: false, tag: 0 });
                }
            }
            // Skill effects run once, then go.
            for e in &mut r.effects {
                e.time_ms += dt;
            }
            // Ground effects loop until the server removes them.
            let durations: Vec<f32> = r.effects.iter().map(|e| r.fx_duration(e.asset)).collect();
            for (e, d) in r.effects.iter_mut().zip(&durations) {
                if e.looping && *d > 0.0 {
                    e.time_ms %= d;
                }
            }
            let mut alive = durations.into_iter();
            r.effects.retain(|e| alive.next().is_some_and(|d| e.looping || e.time_ms < d));
            for p in &mut r.procs {
                p.time_ms += dt;
            }
            r.procs.retain(|p| p.time_ms < p.duration_ms);
            for (a, left) in &mut self.sprite_fx {
                a.time_ms += dt;
                *left -= dt;
            }
            self.sprite_fx.retain(|(_, left)| *left > 0.0);
            r.actors.extend(self.sprite_fx.iter().map(|(a, _)| a.clone()));
            // Emotes over heads (sprite\이팩트\emotion, one action per ET_* id).
            if let (false, Some(vfs)) = (self.emotes.is_empty(), self.vfs.as_ref()) {
                if let Some(sheet) = self.fx.sprite(r, vfs, "emotion") {
                    for (id, (kind, t)) in &mut self.emotes {
                        *t += dt;
                        let m = if *id == self.account_id { self.player.as_ref() } else { self.units.get(id) };
                        let Some(m) = m.filter(|_| *t < r.action_ms(sheet, *kind as usize).max(1500.0)) else {
                            *t = f32::MAX;
                            continue;
                        };
                        r.actors.push(ro_render::Actor {
                            pos: m.actor.pos + glam::Vec3::Y * ro_render::CELL * 2.8,
                            body: sheet,
                            head: None,
                            headgear: vec![],
                            weapon: vec![],
                            shadow: None,
                            action: *kind as usize / 8,
                            dir: ro_render::Dir::from_index(*kind as usize),
                            time_ms: *t,
                            once: true,
                            alpha: 1.0,
                        });
                    }
                    self.emotes.retain(|_, (_, t)| *t < f32::MAX);
                }
            }
            // Items on the ground: their sprite at the cell plus sub-cell offset.
            for (f, actor) in self.floor.values() {
                if let Some(mut a) = actor.clone() {
                    let (fx, fy) = floor_cell(f);
                    let (x, z) = (fx * ro_render::CELL, fy * ro_render::CELL);
                    let y = w.gat.as_ref().and_then(|g| g.height_at(fx, fy)).map_or_else(|| w.ground.height_at(x, z), |h| -h);
                    a.pos = glam::Vec3::new(x, y, z);
                    r.actors.push(a);
                }
            }
        }

        r.post_settings = self.settings.visual.post();
        r.set_water_settings(self.settings.visual.water_settings());
        // HUD: names under actors, status, chat. Laid out for the capture size when capturing.
        let size = self.capture.as_ref().map_or_else(|| r.size(), |(_, w, h)| (*w, *h));
        let mut labels = vec![];
        for (uid, m) in self.player.iter().map(|p| (self.account_id, p)).chain(self.units.iter().map(|(i, m)| (*i, m))) {
            let shown = m.actor.alpha > 0.01 && self.world.as_ref().is_none_or(|w| w.shows_cell(m.walker.cell.0, m.walker.cell.1));
            if let (true, Some(color), Some(feet)) = (shown, m.label_color(), r.project(m.actor.pos, size)) {
                // As the original: "Name (Party)", then "Guild [Rank]" on a second line.
                let mut text = m.name.clone();
                if let Some((party, guild, rank)) = &m.titles {
                    if !party.is_empty() {
                        text = format!("{text} ({party})");
                    }
                    if !guild.is_empty() {
                        text = if rank.is_empty() { format!("{text}
{guild}") } else { format!("{text}
{guild} [{rank}]") };
                    }
                }
                let emblem = (m.guild != 0).then(|| self.emblems.get(&m.guild).cloned().flatten()).flatten().map(|t| t.id());
                let (orbs, orb_kind) = self.spirits.get(&uid).copied().unwrap_or((0, 0));
                labels.push(hud::Label { feet, text, color, emblem, orbs, orb_kind });
            }
        }
        let floaters: Vec<_> = self.floaters.iter().filter_map(|(pos, text, color, age)| Some((r.project(*pos, size)?, text.clone(), *color, *age))).collect();
        let bars: Vec<_> = self
            .units
            .values()
            .filter(|m| m.dying.is_none())
            .filter_map(|m| {
                let (hp, max) = m.hp?;
                Some((r.project(m.actor.pos, size)?, hp as f32 / max.max(1) as f32))
            })
            .collect();
        // The controller's target, marked over its head.
        let pad_marker = self.pad_target.filter(|_| self.pad.in_use()).and_then(|t| self.units.get(&t)).and_then(|m| r.project(m.actor.pos, size));
        // Cast bars over whoever is chanting.
        // Shop signs over vending players.
        let shop_signs: Vec<(u32, glam::Vec2)> = self
            .merchant
            .signs
            .keys()
            .filter_map(|id| { let m = if *id == self.account_id { self.player.as_ref() } else { self.units.get(id) }?; Some((*id, r.project(m.actor.pos + glam::Vec3::Y * ro_render::CELL * 3.2, size)?)) })
            .collect();
        let room_signs: Vec<(u32, glam::Vec2)> = self
            .rooms
            .signs
            .keys()
            .filter_map(|id| { let m = if *id == self.account_id { self.player.as_ref() } else { self.units.get(id) }?; Some((*id, r.project(m.actor.pos + glam::Vec3::Y * ro_render::CELL * 3.2, size)?)) })
            .collect();
        let buy_signs: Vec<(u32, glam::Vec2)> = self
            .stores
            .signs
            .keys()
            .filter_map(|id| { let m = if *id == self.account_id { self.player.as_ref() } else { self.units.get(id) }?; Some((*id, r.project(m.actor.pos + glam::Vec3::Y * ro_render::CELL * 3.2, size)?)) })
            .collect();
        // Quest marks over NPC heads (screen pixels).
        let quest_marks: Vec<(glam::Vec2, u16, u16)> = self
            .quests
            .marks
            .iter()
            .filter_map(|(id, &(mark, color))| Some((r.project(self.units.get(id)?.actor.pos + glam::Vec3::Y * ro_render::CELL * 3.0, size)?, mark, color)))
            .collect();
        let cast_bars: Vec<_> = self
            .casts
            .iter()
            .filter_map(|(id, &(t, total))| {
                let m = if *id == self.account_id { self.player.as_ref() } else { self.units.get(id) }?;
                Some((r.project(m.actor.pos, size)?, t / total))
            })
            .collect();
        if let Some(state) = self.egui_state.as_mut() {
            let mut input = state.take_egui_input(window);
            if let Some(p) = self.fake_pointer.take() {
                input.events.push(egui::Event::PointerMoved(p));
            }
            if let Some(events) = self.fake_frames.pop_front() {
                input.events.extend(events);
            }
            if let Some((p, pressed)) = self.fake_click.take() {
                input.events.push(egui::Event::PointerMoved(p));
                input.events.push(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() });
                if pressed {
                    self.fake_click = Some((p, false));
                }
            }
            let ppp = window.scale_factor() as f32 * self.ui_scale;
            input.screen_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size.0 as f32, size.1 as f32) / ppp));
            self.egui_ctx.set_pixels_per_point(ppp);
            let mut sent = None;
            let mut answer = None;
            let mut ui_action = None;
            if self.skin.is_none() && self.settings.classic_ui {
                if let Some(vfs) = self.vfs.as_ref() {
                    self.skin = Some(classic::Skin::load(&self.egui_ctx, vfs));
                }
            }
            self.hud.classic_bar = self.skin.as_ref().filter(|_| self.settings.classic_ui).and_then(|s| s.texture("dialog_bg")).cloned();
            classic::install(&self.egui_ctx, self.skin.as_ref().filter(|_| self.settings.classic_ui));
            let guild_name = self.guild.name.clone();
            let items: Vec<&ro_game::items::InvItem> = self.inventory.items.values().collect();
            let skill_names = |n: &str| self.skill_names.display(n);
            let view = windows::View {
                portrait: self.portrait.as_ref().and_then(|p| p.1.as_ref()),
                skin: self.skin.as_ref().filter(|_| self.settings.classic_ui),
                guild_name: Some(guild_name.as_str()),
                items: &items,
                db: self.item_db.as_ref(),
                icons: &self.icons,
                skill_icons: &self.skill_icons,
                stats: self.stats.as_ref(),
                skills: &self.skills,
                skill_names: &skill_names,
                skill_info: Some(&self.skill_names),
                skill_tree: Some(&self.skill_tree),
                job: self.hud.status.job_id,
                skill_points: self.hud.status.skill_points,
                weight: (self.hud.status.weight, self.hud.status.max_weight),
                zeny: self.hud.status.zeny,
                screen_width: size.0 as f32 / (window.scale_factor() as f32 * self.ui_scale),
                hotkeys: &self.hotkeys,
                hotkey_row: self.hotkey_row,
                cooldowns: &self.cooldowns,
                has_cart: self.merchant.has_cart(),
                show_equip: self.show_equip,
                pad_hints: self.pad.in_use(),
                playstation: self.pad.playstation,
            };
            let my_name = self.hud.status.name.clone();
            let social_ctx = social::Ctx {
                db: self.item_db.as_ref(),
                icons: &self.icons,
                inventory: &items,
                zeny: self.hud.status.zeny,
                my_name: &my_name,
            };
            let mut social_cmds = vec![];
            let mut front_action = None;
            let mut player_cmd = None;
            let party: std::collections::HashSet<&str> = self.social.party.iter().flat_map(|p| p.members.iter().map(|m| m.name.as_str())).collect();
            let mut dots: Vec<_> = self
                .units
                .values()
                .filter(|m| m.dying.is_none() && self.world.as_ref().is_none_or(|w| w.shows_cell(m.walker.cell.0, m.walker.cell.1)))
                .filter_map(|m| {
                    let color = match m.kind {
                        0 if party.contains(m.name.as_str()) => egui::Color32::from_rgb(255, 130, 200),
                        0 => egui::Color32::from_rgb(120, 180, 255),
                        5 => egui::Color32::from_rgb(230, 60, 50),
                        1 | 6 => egui::Color32::from_rgb(90, 220, 90),
                        _ => return None,
                    };
                    Some(((m.walker.cell.0 as f32, m.walker.cell.1 as f32), color))
                })
                .collect();
            dots.extend(self.member_pos.iter().filter(|(id, _)| !self.units.contains_key(id) && **id != self.account_id).map(|(_, (c, guild))| (*c, if *guild { egui::Color32::from_rgb(170, 255, 170) } else { egui::Color32::from_rgb(255, 130, 200) })));
            dots.extend(self.battle.team_dots().into_iter().map(|c| (c, egui::Color32::from_rgb(255, 160, 200))));
            let me = self.player.as_ref().map(|p| (p.walker.cell.0 as f32, p.walker.cell.1 as f32));
            let map_name = self.world.as_ref().map(|w| w.name.clone()).unwrap_or_default();
            let out = self.egui_ctx.run_ui(input, |ui| {
                // The game cursor on every screen (its own top layer, so draw order does not matter).
                if let (Some(c), Some((x, y))) = (self.cursor_sprite.as_ref(), self.cursor) {
                    let time = ui.ctx().input(|i| i.time);
                    let action = if self.front.is_some() { hud::cursor::ARROW } else { self.cursor_action };
                    c.draw(ui.ctx(), egui::pos2(x as f32 / ppp, y as f32 / ppp), action, time);
                }
                // Start-up splash (age rating), then the title art behind the login screens.
                if let (Some(s), true) = (self.splash.as_mut(), self.front.is_some()) {
                    if s.intro(ui.ctx()) {
                        return;
                    }
                    s.title(ui.ctx());
                }
                // Loading screen over a map being entered (fades once it is in).
                if let (Some(s), None) = (self.splash.as_mut(), self.front.as_ref()) {
                    if s.loading(ui.ctx(), self.world.is_some() && self.player.is_some()) {
                        return;
                    }
                }
                // Login and character screens replace the game UI until a character is picked.
                if let Some(front) = self.front.as_mut() {
                    let tables = &self.look_tables;
                    let accessory = |v: u16| tables.accessory(v as u32).map(str::to_string);
                    let fctx = screens::Ctx { vfs: self.vfs.as_ref(), job_title: &job_title, job_sprite: &job_sprite, accessory: &accessory };
                    front_action = front.show(ui.ctx(), &fctx);
                    return;
                }
                sent = self.hud.show(ui.ctx(), &labels, &floaters, &bars, &cast_bars);
                if let (true, Some(map), Some(me)) = (self.settings.minimap, self.minimap.as_ref(), me) {
                    {
                        let now = std::time::Instant::now();
                        self.viewpoints.retain(|_, v| v.2.is_none_or(|t| t > now));
                        let mut marks: Vec<_> = self.viewpoints.values().map(|v| (v.0, v.1)).collect();
                        marks.extend(self.navi.waypoint(&map_name).map(|(x, y)| ((x as f32, y as f32), egui::Color32::from_rgb(255, 80, 220))));
                        hud::show_minimap(ui.ctx(), map, &map_name, me, &dots, &marks);
                    }
                }
                if let Some(vfs) = self.vfs.as_ref() {
                    self.buffs.show(ui.ctx(), vfs, if self.settings.minimap && self.minimap.is_some() { 270.0 } else { 60.0 });
                }
                if let Some(vfs) = self.vfs.as_ref() {
                    let ppp = ui.ctx().pixels_per_point();
                    let painter = ui.ctx().layer_painter(egui::LayerId::background());
                    for (p, mark, color) in &quest_marks {
                        if let Some(tex) = self.quests.mark_icon(ui.ctx(), vfs, *mark, *color) {
                            let rect = egui::Rect::from_center_size(egui::pos2(p.x / ppp, p.y / ppp), tex.size_vec2() * 2.0);
                            painter.image(tex.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                        }
                    }
                }
                if let Some((id, refine, cards)) = self.item_info.show_viewing(ui.ctx(), self.item_db.as_ref(), &self.icons) {
                    if let Some(vfs) = self.vfs.as_ref() {
                        self.item_info.show_item(ui.ctx(), vfs, self.item_db.as_ref(), id, refine, cards);
                    }
                }
                self.item_info.show(ui.ctx(), self.item_db.as_ref());
                let inv: Vec<&ro_game::items::InvItem> = self.inventory.items.values().collect();
                if let Some(req) = self.refine.show(ui.ctx(), &inv, self.item_db.as_ref(), &self.icons, self.hud.status.zeny) {
                    self.queued_commands.push(ro_net::Command::Refine(req));
                }
                let (bank_move, bank_closed) = self.merchant.show_bank(ui.ctx(), self.hud.status.zeny);
                let aid = self.account_id;
                if let Some(z) = bank_move {
                    let req = if z > 0 { ro_net::BankRequest::Deposit { aid, zeny: z as i32 } } else { ro_net::BankRequest::Withdraw { aid, zeny: (-z) as i32 } };
                    self.queued_commands.push(ro_net::Command::Bank(req));
                }
                if bank_closed {
                    self.queued_commands.push(ro_net::Command::Bank(ro_net::BankRequest::Close(aid)));
                }
                let zeny = self.hud.status.zeny;
                if let Some(req) = self.merchant.show(ui.ctx(), self.item_db.as_ref(), &self.icons, zeny) {
                    self.queued_commands.push(ro_net::Command::Merchant(req));
                }
                if let Some(aid) = self.merchant.show_signs(ui.ctx(), &shop_signs) {
                    self.queued_commands.push(ro_net::Command::Merchant(ro_net::MerchantRequest::Browse(aid)));
                }
                if let Some(aid) = self.stores.show_signs(ui.ctx(), &buy_signs) {
                    self.queued_commands.push(ro_net::Command::BuyingStore(ro_net::BuyingStoreRequest::Browse(aid)));
                }
                let inv: Vec<&ro_game::items::InvItem> = self.inventory.items.values().collect();
                if let Some(req) = self.stores.show_buying(ui.ctx(), &inv, self.item_db.as_ref(), &self.icons, zeny) {
                    self.queued_commands.push(ro_net::Command::BuyingStore(req));
                }
                let shown = if self.settings.classic_ui { self.stores.show_cash_classic(ui.ctx(), self.item_db.as_ref(), &self.icons) } else { self.stores.show_cash_modern(ui.ctx(), self.item_db.as_ref(), &self.icons) };
                if let Some(req) = shown {
                    self.queued_commands.push(ro_net::Command::CashShop(req));
                }
                if std::mem::take(&mut self.windows.cash_toggle) {
                    let req = if self.stores.cash.take().is_some() { ro_net::CashShopRequest::Close } else { ro_net::CashShopRequest::Open };
                    self.queued_commands.push(ro_net::Command::CashShop(req));
                }
                self.windows.cash_shown = self.stores.cash.is_some();
                if std::mem::take(&mut self.windows.menu_toggled) {
                    self.settings.menu_collapsed = self.windows.menu_collapsed;
                    self.settings.save();
                } else {
                    self.windows.menu_collapsed = self.settings.menu_collapsed;
                }
                let target = self.attack.map(|a| a.0).or_else(|| { let me = self.player.as_ref()?.walker.cell; self.units.iter().filter(|(_, m)| m.kind == 5).filter(|(_, m)| (m.walker.cell.0 - me.0).abs().max((m.walker.cell.1 - me.1).abs()) <= 14).min_by_key(|(_, m)| (m.walker.cell.0 - me.0).pow(2) + (m.walker.cell.1 - me.1).pow(2)).map(|(id, _)| *id) });
                self.sense.show(ui.ctx());
                if let Some(a) = self.companions.show(ui.ctx(), target) {
                    self.companion_action = Some(a);
                }
                let mailable: Vec<(u16, u32, u16)> = {
                    let mut v: Vec<_> = self.inventory.items.values().filter(|i| i.equipped == 0).map(|i| (i.index, i.id, i.amount)).collect();
                    v.sort_unstable();
                    v
                };
                self.clan.show(ui.ctx());
                if let Some(v) = self.merge.show(ui.ctx(), &self.inventory.items, self.item_db.as_ref(), &self.icons) {
                    self.queued_commands.push(ro_net::Command::Merge(v));
                }
                if let Some((inviter, accept)) = self.guild.show_offer(ui.ctx()) {
                    self.queued_commands.push(ro_net::Command::Relation(ro_net::RelationRequest::Answer { inviter, accept }));
                }
                self.reputation.show(ui.ctx());
                {
                    let me = self.hud.status.name.clone();
                    let leader = self.social.party.as_ref().is_some_and(|p| p.members.iter().any(|m| m.leader && m.name == me));
                    for r in self.game_opts.show(ui.ctx(), leader) {
                        self.queued_commands.push(ro_net::Command::Option(r));
                    }
                }
                if let Some(k) = self.ranking.show(ui.ctx()) {
                    self.queued_commands.push(ro_net::Command::Ranking(k));
                }
                for r in self.store_search.show(ui.ctx(), self.item_db.as_ref(), &self.icons) {
                    self.queued_commands.push(ro_net::Command::Search(r));
                }
                {
                    let base = self.portrait.as_ref().map(|p| p.0.clone());
                    match self.stylist.show(ui.ctx(), self.vfs.as_ref(), base) {
                        Some(Some(o)) => self.queued_commands.push(ro_net::Command::Style(ro_net::StyleRequest::Buy { hair_color: o.hair_color, hair: o.hair, clothes_color: o.clothes_color })),
                        Some(None) => self.queued_commands.push(ro_net::Command::Style(ro_net::StyleRequest::Close)),
                        None => {}
                    }
                }
                {
                    let inv: Vec<&ro_game::items::InvItem> = self.inventory.items.values().collect();
                    if let Some(a) = self.lapine.show(ui.ctx(), &inv, self.item_db.as_ref(), &self.icons) {
                        self.queued_commands.push(lapine_command(a));
                    }
                }
                {
                    let inv: Vec<&ro_game::items::InvItem> = self.inventory.items.values().collect();
                    if let Some(a) = self.reform.show(ui.ctx(), &inv, self.item_db.as_ref(), &self.icons) {
                        self.queued_commands.push(ro_net::Command::Reform(match a {
                            reform::ReformAction::Reform { tool, index } => ro_net::ReformRequest::Reform { tool, index },
                            reform::ReformAction::Close => ro_net::ReformRequest::Close,
                        }));
                    }
                }
                {
                    let inv: Vec<&ro_game::items::InvItem> = self.inventory.items.values().collect();
                    let reqs = self.enchant.show(ui.ctx(), &inv, self.item_db.as_ref(), &self.icons, self.hud.status.zeny);
                    self.queued_commands.extend(reqs.into_iter().map(ro_net::Command::Enchant));
                }
                {
                    let inv = &self.inventory;
                    let have = |id: u32| inv.items.values().filter(|i| i.id == id && i.equipped == 0).map(|i| i.amount as u32).sum::<u32>();
                    let reqs = self.market.show(ui.ctx(), self.item_db.as_ref(), &self.icons, self.hud.status.zeny, &have);
                    self.queued_commands.extend(reqs.into_iter().map(ro_net::Command::Market));
                }
                let (bg_reqs, castle_reqs) = self.battle.show(ui.ctx(), self.hud.status.base_level.max(0) as u32);
                self.queued_commands.extend(bg_reqs.into_iter().map(ro_net::Command::Bg).chain(castle_reqs.into_iter().map(ro_net::Command::Castle)));
                if let Some(navigation::NaviAction::Walk(x, y)) = self.navi.show(ui.ctx(), &map_name) {
                    self.navi_walk = Some((x, y));
                }
                let inv: Vec<&ro_game::items::InvItem> = self.inventory.items.values().collect();
                for r in self.grade.show(ui.ctx(), &inv, self.item_db.as_ref(), &self.icons, self.hud.status.zeny) {
                    self.queued_commands.push(ro_net::Command::Grade(r));
                }
                for req in self.mailbox.show(ui.ctx(), self.item_db.as_ref(), &self.icons, self.hud.status.zeny, &mailable) {
                    self.queued_commands.push(ro_net::Command::Mail(req));
                }
                let me = self.hud.status.name.clone();
                for req in self.rooms.show(ui.ctx(), &me).into_iter().chain(self.rooms.show_signs(ui.ctx(), &room_signs)) {
                    self.queued_commands.push(ro_net::Command::ChatRoom(req));
                }
                let my_level = self.hud.status.base_level.clamp(1, 999) as u16;
                let job_name = |j: u16| jobs::title(j as i16).to_string();
                for a in self.booking.show(ui.ctx(), my_level, &job_name) {
                    match a {
                        booking::BookingAction::Send(r) => self.queued_commands.push(ro_net::Command::Booking(r)),
                        booking::BookingAction::Whisper(name) => self.hud.set_input(format!("/w \"{name}\" ")),
                    }
                }
                if let Some(req) = self.progress.show(ui.ctx(), self.item_db.as_ref(), &self.icons) {
                    self.queued_commands.push(ro_net::Command::Progress(req));
                }
                if let Some(req) = self.progress.show_attendance(ui.ctx()) {
                    self.queued_commands.push(ro_net::Command::Progress(req));
                }
                if let Some((quest, active)) = self.quests.show(ui.ctx()) {
                    self.queued_commands.push(ro_net::Command::QuestActive { quest, active });
                }
                if self.esc_menu {
                    match hud::esc_menu(ui.ctx()) {
                        Some(hud::EscChoice::CharSelect) => {
                            self.esc_menu = false;
                            self.queued_commands.push(ro_net::Command::Restart(1));
                        }
                        Some(hud::EscChoice::Options) => {
                            self.esc_menu = false;
                            self.windows.settings = true;
                        }
                        Some(hud::EscChoice::Exit) => self.quit = true,
                        Some(hud::EscChoice::Back) => self.esc_menu = false,
                        None => {}
                    }
                }
                if self.dead {
                    let hint = self.pad.in_use().then_some(if self.pad.playstation { "Cross" } else { "A" });
                    if hud::death_window(ui.ctx(), hint) {
                        self.queued_commands.push(ro_net::Command::Restart(0));
                    }
                }
                if let Some(feet) = pad_marker {
                    hud::target_marker(ui.ctx(), feet, ppp, self.aiming.is_some());
                }
                if self.windows.settings && self.settings.show(ui.ctx(), &mut self.windows.settings) {
                    self.ui_scale = self.settings.ui_scale;
                    if let Some(a) = self.audio.as_mut() {
                        a.set_bgm_volume(self.settings.bgm_volume);
                        a.sfx_volume = self.settings.sfx_volume;
                    }
                }
                answer = self.hud.show_dialog(ui.ctx(), &mut self.dialog, self.cutin.as_ref());
                ui_action = self.windows.show(ui.ctx(), &view);
                self.social.show_party = self.windows.party;
                self.social.show_friends = self.windows.friends;
                social_cmds = self.social.show(ui.ctx(), &social_ctx);
                // Guild window (menu button / Alt+G).
                if self.windows.guild != self.guild.open {
                    self.guild.toggle();
                }
                let guild_ctx = guild::Ctx {
                    account_id: self.account_id,
                    char_id: self.char_id,
                    has_emperium: items.iter().any(|i| i.id == guild::EMPERIUM),
                    job_name: &job_title,
                };
                social_cmds.extend(self.guild.show(ui.ctx(), &guild_ctx));
                self.windows.guild = self.guild.open;
                (self.windows.party, self.windows.friends) = (self.social.show_party, self.social.show_friends);
                // Menu for a clicked player.
                if let Some((id, name, pos)) = self.player_menu.clone() {
                    let mut keep = true;
                    egui::Area::new(egui::Id::unique("player menu")).fixed_pos(pos).show(ui.ctx(), |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.label(egui::RichText::new(&name).strong());
                            if ui.button("Trade").clicked() {
                                player_cmd = Some((ro_net::Command::Trade(ro_net::TradeRequest::Ask { target: id }), Some(name.clone())));
                                keep = false;
                            }
                            if ui.button("View equipment").clicked() {
                                player_cmd = Some((ro_net::Command::ViewEquip(id), None));
                                keep = false;
                            }
                            if ui.button("Invite to party").clicked() {
                                player_cmd = Some((ro_net::Command::Party(ro_net::PartyRequest::Invite { name: name.clone() }), None));
                                keep = false;
                            }
                            if ui.button("Add friend").clicked() {
                                player_cmd = Some((ro_net::Command::Friend(ro_net::FriendRequest::Add { name: name.clone() }), None));
                                keep = false;
                            }
                            if self.guild.id != 0 && ui.button("Invite to guild").clicked() {
                                player_cmd = Some((ro_net::Command::Guild(ro_net::GuildRequest::Invite { name: name.clone() }), None));
                                keep = false;
                            }
                            // Guild masters: offer an alliance, or mark the other guild an enemy.
                            let other_guild = self.units.get(&id).map_or(0, |m| m.guild);
                            if self.guild.master && other_guild != 0 && other_guild != self.guild.id {
                                if ui.button("Ask for an alliance").clicked() {
                                    player_cmd = Some((ro_net::Command::Relation(ro_net::RelationRequest::Ally { target: id, me: self.account_id, char_id: self.char_id }), None));
                                    keep = false;
                                }
                                if ui.button("Set guild as enemy").clicked() {
                                    player_cmd = Some((ro_net::Command::Relation(ro_net::RelationRequest::Enemy(id)), None));
                                    keep = false;
                                }
                            }
                            if ui.button("Close").clicked() {
                                keep = false;
                            }
                        });
                    });
                    if !keep {
                        self.player_menu = None;
                    }
                }
                if let Some((skill, _)) = self.aiming {
                    let name = self.skills.iter().find(|s| s.id == skill).map(|s| self.skill_names.display(&s.name)).unwrap_or_default();
                    egui::Area::new(egui::Id::unique("aiming")).anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 16.0)).show(ui.ctx(), |ui| {
                        ui.label(egui::RichText::new(format!("{name}: choose a target (Esc to cancel)")).size(15.0).color(egui::Color32::from_rgb(255, 220, 120)));
                    });
                }
            });
            drop(view);
            // Handled next tick: the renderer is borrowed here.
            if ui_action.is_some() {
                self.queued_action = ui_action;
            }
            if let Some((cmd, partner)) = player_cmd {
                self.trade_partner = partner.or(self.trade_partner.take());
                social_cmds.push(cmd);
            }
            self.queued_commands.extend(social_cmds);
            match front_action {
                Some(screens::Action::Send(cmd)) => self.queued_commands.push(cmd),
                Some(screens::Action::Logout) => {
                    // A fresh session back at the login screen.
                    let remember = self.front.as_ref().is_some_and(|f| f.remember);
                    let user = self.front.as_ref().map(|f| f.user().to_string()).unwrap_or_default();
                    self.front = Some(screens::Front::new(user, remember));
                    if let (Some(a), Some(v)) = (self.audio.as_mut(), self.vfs.as_ref()) {
                        a.title(v);
                    }
                    self.net = Some(ro_net::spawn_session(self.session.clone()));
                }
                Some(screens::Action::Quit) => self.quit = true,
                None => {}
            }
            if answer.is_none() {
                if let Some(d) = self.dialog.as_mut().filter(|d| !matches!(d.mode, hud::DialogMode::Reading)) {
                    if let Some(a) = self.npc_answers.pop_front() {
                        tracing::info!(answer = %a, "npc answer");
                        let reply = match a.split_once(':') {
                            Some(("menu", n)) => ro_net::NpcReply::Menu(n.parse().unwrap_or(1)),
                            Some(("num", n)) => ro_net::NpcReply::Number(n.parse().unwrap_or(0)),
                            Some(("text", t)) => ro_net::NpcReply::Text(t.to_string()),
                            _ if a == "close" => ro_net::NpcReply::Close,
                            _ => ro_net::NpcReply::Next,
                        };
                        let npc = d.npc;
                        match reply {
                            ro_net::NpcReply::Close => self.dialog = None,
                            ro_net::NpcReply::Next | ro_net::NpcReply::Menu(_) => {
                                d.mode = hud::DialogMode::Reading;
                                d.lines.clear();
                            }
                            _ => d.mode = hud::DialogMode::Reading,
                        }
                        answer = Some((npc, reply));
                    }
                }
            }
            if let (Some((npc, reply)), Some((tx, _))) = (answer, self.net.as_ref()) {
                if reply == ro_net::NpcReply::Close {
                    self.cutin = None;
                }
                let _ = tx.send(ro_net::Command::Npc { npc, reply });
            }
            // The game draws its own cursor; egui would otherwise show the system one again.
            let mut platform = out.platform_output;
            if self.cursor_sprite.is_some() {
                platform.cursor_icon = egui::CursorIcon::None;
            }
            state.handle_platform_output(window, platform);
            let primitives = self.egui_ctx.tessellate(out.shapes, out.pixels_per_point);
            r.set_ui(ro_render::UiFrame { primitives, textures: out.textures_delta, pixels_per_point: out.pixels_per_point });
            if let Some(text) = sent {
                self.queued_chat.push(text);
            }
        }

        if let Some((path, w, h)) = &self.capture {
            let ready = self.ready_at.is_some_and(|t| t.elapsed().as_secs_f32() * 1000.0 >= self.capture_after);
            if ready {
                match r.capture(*w, *h) {
                    Ok(img) => match image::save_buffer(path, &img.pixels, img.width, img.height, image::ColorType::Rgba8) {
                        Ok(()) => {
                            let at = self.player.as_ref().map(|p| p.walker.cell);
                            tracing::info!(path = %path.display(), ?at, "captured");
                        }
                        Err(e) => tracing::error!("save capture: {e:#}"),
                    },
                    Err(e) => tracing::error!("capture: {e:#}"),
                }
                self.capture = None;
                el.exit();
            }
            return;
        }

        if let Err(e) = r.frame() {
            tracing::error!("frame: {e:#}");
        }
        if let Some(fps) = self.fps.tick() {
            let (w, h) = r.size();
            let at = self.player.as_ref().map(|p| p.walker.cell).unwrap_or_default();
            let map = self.world.as_ref().map_or("-", |w| w.name.as_str());
            let title = match &self.status {
                Some(s) => format!("roclient — {s}"),
                None if self.world.is_none() && self.net.is_some() => "roclient — connecting…".to_string(),
                None => format!("roclient — {map} {},{} — {w}x{h} — {fps:.0} fps", at.0, at.1),
            };
            window.set_title(&title);
        }
    }
}

#[derive(Default)]
struct FpsCounter {
    frames: u32,
    since: Option<std::time::Instant>,
}

impl FpsCounter {
    /// Returns frames per second roughly once a second.
    fn tick(&mut self) -> Option<f64> {
        let now = std::time::Instant::now();
        let since = *self.since.get_or_insert(now);
        self.frames += 1;
        let dt = now - since;
        if dt.as_secs_f64() < 1.0 {
            return None;
        }
        let fps = self.frames as f64 / dt.as_secs_f64();
        self.frames = 0;
        self.since = Some(now);
        Some(fps)
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("roclient")
            .with_inner_size(winit::dpi::LogicalSize::new(1600.0, 900.0))
            .with_min_inner_size(winit::dpi::LogicalSize::new(640.0, 360.0))
            .with_visible(self.capture.is_none());
        let window = Arc::new(el.create_window(attrs).unwrap());
        // The game draws its own cursors (hud::CursorSprite).
        if self.cursor_sprite.is_some() && self.capture.is_none() {
            window.set_cursor_visible(false);
        }
        let backends = ro_render::parse_backends(&self.backend);
        match pollster::block_on(ro_render::Renderer::new(window.clone(), backends, self.msaa)) {
            Ok(mut r) => {
                r.camera.look_at_cell(self.at.0, self.at.1);
                if let Some(yaw) = self.yaw {
                    r.camera.yaw = yaw.to_radians();
                }
                if let Some(d) = self.distance {
                    r.camera.distance = d;
                }
                self.renderer = Some(r);
            }
            Err(e) => {
                tracing::error!("renderer init failed: {e:#}");
                el.exit();
                return;
            }
        }
        self.egui_state = Some(egui_winit::State::new(
            self.egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32 * self.ui_scale),
            None,
            None,
        ));
        if let (Some(r), Some(vfs)) = (self.renderer.as_mut(), self.vfs.as_ref()) {
            let path = r"data\texture\effect\ring_blue.tga";
            match vfs.read(path).ok().flatten().map(|d| ro_formats::image::decode(&d, path)) {
                Some(Ok(img)) => r.set_warp_texture(&img),
                _ => tracing::warn!(path, "warp portal texture unavailable"),
            }
        }
        self.window = Some(window);
        el.set_control_flow(winit::event_loop::ControlFlow::Poll);

        // Offline: show the preview map and character right away. Online waits for the server,
        // with the map as the login screen's backdrop.
        self.splash = Some(splash::Splash::new(&self.egui_ctx, self.vfs.as_ref()));
        // Captures of the login screens time from start-up (no map loads behind them).
        if self.front.is_some() {
            self.ready_at = Some(std::time::Instant::now());
        }
        if self.net.is_none() {
            let map = self.map.clone();
            self.enter_map(&map);
        }
        if self.net.is_none() {
            let look = self.look.clone();
            self.spawn_player(&look, self.at, self.dir, 150);
        } else if let (Some(r), Some(w)) = (self.renderer.as_mut(), self.world.as_ref()) {
            // Backdrop: look at the map centre from a little further out.
            let c = w.gat.as_ref().map_or((100, 100), |g| ((g.width / 2) as u16, (g.height / 2) as u16));
            r.camera.look_at_cell(c.0, c.1);
            r.camera.distance = 420.0;
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        self.tick(el);
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let (Some(r), Some(window)) = (self.renderer.as_mut(), self.window.as_ref()) else { return };
        // The UI sees input first; clicks and keys it uses don't reach the game.
        let ui_used = self.egui_state.as_mut().is_some_and(|s| s.on_window_event(window, &event).consumed);
        // Track the left button ourselves (hold-to-walk), whatever the UI did with the event.
        if let WindowEvent::MouseInput { state, button: MouseButton::Left, .. } = &event {
            self.left_down = *state == ElementState::Pressed;
        }
        if let WindowEvent::CursorMoved { position, .. } = &event {
            // Keep tracking the cursor even over UI so rotation deltas stay continuous.
            if ui_used {
                self.cursor = Some((position.x, position.y));
            }
        }
        // Login / character screens: the world takes no input.
        let front_blocks = self.front.is_some() && matches!(event, WindowEvent::MouseInput { .. } | WindowEvent::MouseWheel { .. } | WindowEvent::KeyboardInput { .. });
        if (ui_used || front_blocks) && !matches!(event, WindowEvent::Resized(_) | WindowEvent::CloseRequested | WindowEvent::ScaleFactorChanged { .. }) {
            return;
        }
        match event {
            WindowEvent::CloseRequested => el.exit(),
            _ if self.quit => el.exit(),
            WindowEvent::KeyboardInput { event: winit::event::KeyEvent { logical_key, state: ElementState::Pressed, .. }, .. }
                if logical_key == winit::keyboard::Key::Named(winit::keyboard::NamedKey::Enter) =>
            {
                self.hud.focus_chat();
            }
            WindowEvent::ModifiersChanged(m) => self.alt = m.state().alt_key(),
            WindowEvent::KeyboardInput { event: winit::event::KeyEvent { logical_key, state: ElementState::Pressed, repeat: false, .. }, .. } => {
                use winit::keyboard::{Key, NamedKey};
                match logical_key {
                    // Esc cancels aiming; otherwise it opens the game menu, as in the original.
                    Key::Named(NamedKey::Escape) => {
                        if self.aiming.take().is_none() && self.dialog.is_none() && !self.dead {
                            self.esc_menu = !self.esc_menu;
                        }
                    }
                    // Shortcut bar: F1-F9 use the visible row's slots, F12 shows the next row.
                    Key::Named(NamedKey::F12) => self.queued_action = Some(windows::UiAction::HotkeyRow((self.hotkey_row + 1) % HOTKEY_ROWS)),
                    Key::Named(k) if hotkey_slot(k).is_some() => {
                        let slot = self.hotkey_row as usize * HOTKEYS_PER_ROW + hotkey_slot(k).unwrap_or(0);
                        self.queued_action = Some(windows::UiAction::UseHotkey(slot));
                    }
                    Key::Character(c) if self.alt => self.alt_key(&c.to_ascii_lowercase()),
                    _ => {}
                }
            }
            WindowEvent::Resized(s) => {
                r.resize(s.width, s.height);
                window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => r.scale_factor = scale_factor,
            WindowEvent::MouseWheel { .. } | WindowEvent::MouseInput { .. } if self.still => {}
            WindowEvent::MouseWheel { delta, .. } => {
                let steps = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => (p.y / 60.0) as f32,
                };
                r.camera.zoom(steps);
            }
            WindowEvent::MouseInput { state, button: MouseButton::Right, .. } => {
                self.rotating = state == ElementState::Pressed;
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } => {
                let unit = self.pick_unit().and_then(|id| self.units.get(&id).map(|m| (id, m.kind)));
                match unit {
                    // NPCs: talk (walking over first if needed).
                    Some((id, 1 | 6)) => {
                        self.walk_goal = None;
                        self.attack = None;
                        self.talk(id);
                    }
                    // PvP / GvG maps: clicking an enemy player attacks (the original's attack cursor).
                    Some((id, 0)) if self.aiming.is_none() && self.is_enemy_player(id) => self.attack_unit(id),
                    // Other players: a menu (trade, party, friend).
                    Some((id, 0)) if self.aiming.is_none() => {
                        let name = self.units.get(&id).map(|m| m.name.clone()).unwrap_or_default();
                        let ppp = self.egui_ctx.pixels_per_point();
                        let pos = self.cursor.map_or(egui::Pos2::ZERO, |(x, y)| egui::pos2(x as f32 / ppp, y as f32 / ppp));
                        self.player_menu = Some((id, name, pos));
                    }
                    // Monsters: attack until it dies (or use the skill being aimed).
                    Some((id, 5)) if std::mem::take(&mut self.capturing) => self.send(ro_net::Command::Companion(ro_net::CompanionRequest::Capture(id))),
                    Some((id, 5)) => match self.aiming.take() {
                        Some((skill, level)) => self.use_skill_on(skill, level, id),
                        None => self.attack_unit(id),
                    },
                    // Aiming at another player (heal, blessing...).
                    Some((id, 0)) => {
                        if let Some((skill, level)) = self.aiming.take() {
                            self.use_skill_on(skill, level, id);
                        }
                    }
                    _ => {
                        self.attack = None;
                        self.chasing = false;
                        if let Some((skill, level)) = self.aiming.take() {
                            let ground = self.skills.iter().find(|s| s.id == skill).is_some_and(|s| s.target & 2 != 0);
                            let me = self.player.as_ref().map(|p| p.walker.cell);
                            match self.pick() {
                                Some(cell) if ground => self.use_skill_at(skill, level, cell),
                                // Clicking our own cell targets ourselves.
                                Some(cell) if Some((cell.0 as i32, cell.1 as i32)) == me => self.use_skill_on(skill, level, self.account_id),
                                _ => {}
                            }
                        } else if let Some(item) = self.pick_floor_item() {
                            self.pick_up(item);
                        } else if let Some(cell) = self.pick() {
                            self.pending_npc = None;
                            self.pending_pickup = None;
                            self.pending_skill = None;
                            // Holding the button keeps steering toward the cursor.
                            self.mouse_walk = true;
                            self.mouse_walk_ms = 150.0;
                            self.request_walk(cell);
                        }
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let (true, Some((lx, ly))) = (self.rotating, self.cursor) {
                    // Scale by DPI so rotation speed matches across monitors.
                    let s = 0.005 / r.scale_factor as f32;
                    r.camera.rotate((position.x - lx) as f32 * s, (position.y - ly) as f32 * s);
                }
                self.cursor = Some((position.x, position.y));
            }
            _ => {}
        }
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    let mut cfg = load_config()?;
    // `--map name` / `--at x,y` override the preview map and cell.
    let args: Vec<String> = std::env::args().collect();
    let arg = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    // `--login <addr>` / `--login-addr <addr>` beat the config (local or LAN server).
    cfg.login_addr = normalize_login_addr(&arg("--login").or_else(|| arg("--login-addr")).unwrap_or(cfg.login_addr));
    println!("Login server: {}", cfg.login_addr);
    tracing::info!(login_addr = %cfg.login_addr, "connecting to login server");

    if std::env::args().any(|a| a == "--headless") {
        let rt = tokio::runtime::Runtime::new()?;
        return rt.block_on(headless(&cfg));
    }

    let event_loop = EventLoop::new()?;
    let vfs = open_vfs(&cfg)?;
    let cell = |s: String| s.split_once(',').and_then(|(x, y)| Some((x.parse().ok()?, y.parse().ok()?)));
    // Online unless --offline (or no game data to render with).
    // Scripted and test runs (or `auto_login = true`) log straight in with the configured
    // account; otherwise the login and character screens drive the session.
    const SCRIPTED: [&str; 10] = ["--capture", "--do", "--say", "--attack", "--talk", "--walk", "--windows", "--deal", "--accept", "--auto-trade"];
    // `--front` keeps the screens for captures; `--front-login` also submits the configured login.
    let front_test = args.iter().any(|a| matches!(a.as_str(), "--front" | "--front-login" | "--front-create" | "--front-make" | "--front-start"));
    let auto = !front_test && (cfg.auto_login || args.iter().any(|a| SCRIPTED.contains(&a.as_str())));
    let (cfg_user, cfg_pass) = (cfg.username.clone(), cfg.password.clone());
    let session = ro_net::SessionConfig {
        login_addr: cfg.login_addr.clone(),
        username: cfg.username.clone(),
        password: cfg.password.clone(),
        char_slot: cfg.char_slot,
        auto,
    };
    let online = !args.iter().any(|a| a == "--offline") && vfs.is_some();
    let net = online.then(|| ro_net::spawn_session(session.clone()));
    let mut app = App {
        backend: cfg.backend,
        msaa: cfg.msaa,
        vfs,
        map: arg("--map").unwrap_or(cfg.map),
        still: args.iter().any(|a| a == "--still"),
        distance: arg("--distance").and_then(|d| d.parse().ok()),
        yaw: arg("--yaw").and_then(|d| d.parse().ok()),
        look: Look { job: arg("--job").unwrap_or_else(|| "초보자".into()), male: arg("--sex").is_none_or(|s| s != "f"), hair: 1, hair_color: 0, gear: ro_net::Look { weapon: arg("--weapon").and_then(|w| w.parse().ok()).unwrap_or(0), ..Default::default() } },
        action: arg("--action").and_then(|a| a.parse().ok()).unwrap_or(0),
        dir: arg("--dir").and_then(|d| d.parse::<usize>().ok()).unwrap_or(0) % 8,
        capture_after: arg("--capture-after").and_then(|d| d.parse().ok()).unwrap_or(0.0),
        walk_on_enter: arg("--walk").and_then(cell),
        say_on_enter: arg("--say"),
        net,
        session: session.clone(),
        quit: false,
        esc_menu: false,
        splash: None,
        front: (online && !auto).then(|| screens::Front::new(settings::Settings::load(Default::default()).last_id.clone(), true)),
        world: None,
        player: None,
        account_id: 0,
        char_id: 0,
        guild: Default::default(),
        status: None,
        walk_goal: None,
        dialog: None,
        cutin: None,
        minimap: None,
        settings: settings::Settings::load(settings::Settings {
            bgm_volume: cfg.bgm_volume,
            sfx_volume: cfg.sfx_volume,
            ui_scale: cfg.ui_scale,
            minimap: true,
            menu_collapsed: false,
            classic_ui: true,
            visual: Default::default(),
            last_id: String::new(),
        }),
        audio: None,
        pending_npc: None,
        pending_skill: None,
        attack: None,
        chasing: false,
        floaters: vec![],
        item_db: None,
        skill_names: Default::default(),
        skill_tree: Default::default(),
        inventory: Default::default(),
        floor: Default::default(),
        stats: None,
        skills: vec![],
        windows: Default::default(),
        icons: Default::default(),
        icons_tried: Default::default(),
        skill_icons: Default::default(),
        pending_pickup: None,
        aiming: None,
        hotkeys: vec![Default::default(); HOTKEY_ROWS as usize * HOTKEYS_PER_ROW],
        hotkey_row: 0,
        casts: Default::default(),
        fx: Default::default(),
        map_fx: vec![],
        sprite_fx: vec![],
        burst_at: Default::default(),
        emotes: Default::default(),
        last_whisper_from: None,
        queued_chat: vec![],
        buffs: Default::default(),
        quests: Default::default(),
        merchant: Default::default(),
        stores: Default::default(),
        progress: Default::default(),
        companions: Default::default(),
        mailbox: Default::default(),
        clan: Default::default(),
        merge: Default::default(),
        reputation: Default::default(),
        game_opts: Default::default(),
        ranking: Default::default(),
        store_search: Default::default(),
        stylist: Default::default(),
        reform: Default::default(),
        lapine: Default::default(),
        enchant: Default::default(),
        market: Default::default(),
        market_buy: None,
        battle: Default::default(),
        navi: Default::default(),
        navi_walk: None,
        grade: Default::default(),
        rooms: Default::default(),
        booking: Default::default(),
        ignore_pending: None,
        companion_action: None,
        follow_at: std::time::Instant::now(),
        unit_jobs: Default::default(),
        aggro: Default::default(),
        homun_ai: None,
        merc_ai: None,
        client_dir: cfg.client_dir.clone(),
        skin: None,
        item_info: Default::default(),
        refine: Default::default(),
        show_equip: None,
        portrait: None,
        cooldowns: Default::default(),
        script_resume: None,
        pad: gamepad::Pad::new(),
        pad_target: None,
        pad_dir: None,
        pad_walk_ms: 0.0,
        dead: false,
        fx_test: arg("--fx").map(|s| &*s.leak()),
        alt: false,
        queued_action: None,
        fake_pointer: None,
        fake_click: None,
        fake_frames: Default::default(),
        social: Default::default(),
        trade_partner: None,
        player_menu: None,
        auto_deal: arg("--deal").map(|d| d == "buy"),
        auto_sell: arg("--sell").and_then(|s| s.parse().ok()),
        auto_buy: arg("--buy").and_then(|s| s.parse().ok()),
        auto_accept: args.iter().any(|a| a == "--accept"),
        auto_trade: arg("--auto-trade").and_then(|s| s.parse().ok()),
        trade_stage: 0,
        trade_offers: Default::default(),
        queued_commands: vec![],
        open_windows: args.iter().any(|a| a == "--windows"),
        auto_pickup: false,
        script: arg("--do").map(|s| s.split(",").map(str::to_string).collect()).unwrap_or_default(),
        talk_to: arg("--talk"),
        viewpoints: Default::default(),
        member_pos: Default::default(),
        spirits: Default::default(),
        attack_range: 1,
        capturing: false,
        map_flags: 0,
        hat_files: Default::default(),
        hat_fx: Default::default(),
        emblems: Default::default(),
        npc_answers: arg("--npc-answers").map(|s| s.split(',').map(str::to_string).collect()).unwrap_or_default(),
        auto_attack: args.iter().any(|a| a == "--attack"),
        units: Default::default(),
        warps: Default::default(),
        class_names: Default::default(),
        pet_talk: Default::default(),
        sense: Default::default(),
        look_tables: Default::default(),
        sprites: Default::default(),
        ready_at: None,
        last_tick: std::time::Instant::now(),
        capture: arg("--capture").map(|p| {
            let (w, h) = arg("--size")
                .and_then(|s| s.split_once('x').and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?))))
                .unwrap_or((1920, 1080));
            (PathBuf::from(p), w, h)
        }),
        at: arg("--at").and_then(cell).unwrap_or((156, 191)),
        window: None,
        renderer: None,
        rotating: false,
        cursor_sprite: None,
        cursor_action: 0,
        mouse_walk: false,
        mouse_walk_ms: 0.0,
        left_down: false,
        cursor: None,
        fps: FpsCounter::default(),
        egui_ctx: egui::Context::default(),
        egui_state: None,
        hud: hud::Hud::default(),
        ui_scale: 1.0, // from settings below
    };
    app.class_names = app.vfs.as_ref().map(load_class_names).unwrap_or_default();
    app.pet_talk = app.vfs.as_ref().map(pettalk::PetTalk::load).unwrap_or_default();
    app.guild.client_dir = app.client_dir.clone();
    app.hat_files = app.vfs.as_ref().map(fx::hat_effects).unwrap_or_default();
    app.ui_scale = app.settings.ui_scale;
    if args.iter().any(|a| matches!(a.as_str(), "--front-login" | "--front-create" | "--front-make" | "--front-start")) {
        app.queued_commands.push(ro_net::Command::Login { user: cfg_user.clone(), pass: cfg_pass.clone() });
    }
    app.cursor_sprite = app.vfs.as_ref().and_then(|v| hud::CursorSprite::load(&app.egui_ctx, v));
    // Captures and headless runs stay silent.
    if app.capture.is_none() {
        app.audio = app.vfs.as_ref().and_then(|v| audio::Audio::new(v, app.settings.bgm_volume, app.settings.sfx_volume));
        // Title theme while the login screens show.
        if let (Some(a), Some(v), true) = (app.audio.as_mut(), app.vfs.as_ref(), app.front.is_some()) {
            a.title(v);
        }
    }
    if let Some(vfs) = app.vfs.as_ref() {
        // iteminfo.lub lives in the client's System folder; skill names in the GRF.
        let read = |p: &str| vfs.read(p).ok().flatten();
        app.fx.index(vfs);
        app.buffs = buffs::Buffs::load(vfs);
        app.quests = quests::QuestLog::load(vfs);
        app.progress = progress::Progress::load(vfs);
        msgstr::load(vfs);
        app.item_info = iteminfo::ItemInfo::load(vfs);
        match read(r"System\iteminfo.lub").map(|d| ro_game::items::ItemDb::from_lub(&d)) {
            Some(Ok(mut db)) => {
                for p in customdata::ITEMINFO_SOURCES {
                    if let Some(src) = read(p) {
                        db.extend(customdata::iteminfo(&src, p));
                    }
                }
                tracing::info!(items = db.len(), "loaded item info");
                app.item_db = Some(db);
            }
            Some(Err(e)) => tracing::warn!("iteminfo: {e:#}"),
            None => tracing::warn!("System/iteminfo.lub not found; items will show by id"),
        }
        let info = |n: &str| read(&format!(r"data\luafiles514\lua files\datainfo\{n}.lub"));
        if let (Some(a), Some(b), Some(c)) = (info("accessoryid"), info("accname"), info("weapontable")) {
            match ro_game::looks::LookTables::from_lubs(&a, &b, &c) {
                Ok(t) => app.look_tables = t,
                Err(e) => tracing::warn!("equipment sprite tables: {e:#}"),
            }
        }
        let skills = read(r"data\luafiles514\lua files\skillinfoz\skillinfolist.lub").map(|d| ro_game::skills::SkillNames::from_lub(&d));
        match skills {
            Some(Ok(s)) => app.skill_names = s,
            Some(Err(e)) => tracing::warn!("skillinfolist: {e:#}"),
            None => tracing::warn!("skillinfolist.lub not found"),
        }
        if let Some(d) = read(r"data\luafiles514\lua files\skillinfoz\skillid.lub") {
            if let Err(e) = app.skill_names.add_ids(&d) {
                tracing::warn!("skillid: {e:#}");
            }
        }
        let tree = read(r"data\luafiles514\lua files\skillinfoz\skilltreeview.lub");
        if let (Some(view), Some(inherit)) = (tree, read(r"data\luafiles514\lua files\skillinfoz\jobinheritlist.lub")) {
            match skilltree::SkillTree::from_lubs(&view, &inherit) {
                Ok(t) => app.skill_tree = t,
                Err(e) => tracing::warn!("skill tree: {e:#}"),
            }
        }
        if let Some(d) = read(r"data\luafiles514\lua files\skillinfoz\skilldescript.lub") {
            if let Err(e) = app.skill_names.add_descriptions(&d) {
                tracing::warn!("skilldescript: {e:#}");
            }
        }
    }
    // `--sound-check`: list which classic skills have a sound file, then exit.
    if args.iter().any(|a| a == "--sound-check") {
        let (Some(vfs), true) = (app.vfs.as_ref(), true) else { return Ok(()) };
        let Some(a) = audio::Audio::new(vfs, 0.0, 0.0) else { return Ok(()) };
        const CLASSES: [&str; 19] = ["SM_", "MG_", "AL_", "AC_", "MC_", "TF_", "KN_", "PR_", "WZ_", "BS_", "HT_", "AS_", "CR_", "MO_", "SA_", "RG_", "AM_", "BA_", "DC_"];
        let mut skills: Vec<(u16, &str)> = app.skill_names.all_ids().filter(|(_, n)| CLASSES.iter().any(|c| n.starts_with(c))).collect();
        skills.sort();
        let (mut hit, mut miss) = (vec![], vec![]);
        for (_, n) in &skills {
            match a.skill_sound(n) {
                Some(f) => hit.push(format!("{n} -> {f}")),
                None => miss.push(n.to_string()),
            }
        }
        println!("{} with sound, {} without", hit.len(), miss.len());
        println!("{}", hit.join("
"));
        println!("--- missing:
{}", miss.join(" "));
        return Ok(());
    }
    event_loop.run_app(&mut app)?;
    Ok(())
}

/// Which attack motion a player uses: the weapon sprite only has frames in its own one
/// (katars draw in group 11, most weapons in 10).
fn attack_group(r: &ro_render::Renderer, a: &ro_render::Actor) -> usize {
    match a.weapon.first() {
        Some(&w) if !r.action_visible(w, 10) && r.action_visible(w, 11) => 11,
        _ => 10,
    }
}

#[cfg(test)]
mod login_addr_tests {
    use super::normalize_login_addr as n;

    #[test]
    fn normalizes() {
        assert_eq!(n("127.0.0.1:6900"), "127.0.0.1:6900");
        assert_eq!(n(" 192.168.1.50 "), "192.168.1.50:6900");
        assert_eq!(n("192.168.1.50:6901"), "192.168.1.50:6901");
        assert_eq!(n("myhost.lan"), "myhost.lan:6900");
        assert_eq!(n("myhost.lan:7000"), "myhost.lan:7000");
        assert_eq!(n("[::1]"), "[::1]:6900");
        assert_eq!(n("[::1]:7000"), "[::1]:7000");
        assert_eq!(n("::1"), "[::1]:6900");
    }
}

/// How opaque a unit is under its OPTION_* flags. Hiding, cloaking and Chase Walk show
/// you faded; other units (and NPCs cloaked by scripts) vanish entirely.
fn option_alpha(option: u32, me: bool) -> f32 {
    match (option & (0x2 | 0x4 | 0x40 | 0x4000) != 0, me) {
        (false, _) => 1.0,
        (true, true) => 0.35,
        (true, false) => 0.0,
    }
}

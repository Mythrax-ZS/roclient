//! roclient
//!
//!   ro-client --headless   run login -> char select -> map entry and exit
//!   ro-client              open the renderer window

mod hud;
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
    /// MSAA sample count (1, 2, 4); falls back to what the GPU supports.
    #[serde(default = "default_msaa")]
    msaa: u32,
    /// Installed RO client folder; its `data/` is checked before the GRFs.
    client_dir: Option<PathBuf>,
    /// GRFs inside `client_dir`, highest priority first.
    #[serde(default = "default_grfs")]
    grfs: Vec<String>,
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
    let mut vfs = ro_vfs::Vfs { loose: Some(dir.clone()), grfs: vec![] };
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
    let body = sheet(&format!("data\\sprite\\인간족\\몸통\\{sex}\\{}_{sex}", look.job), None)?;
    let head = sheet(
        &format!("data\\sprite\\인간족\\머리통\\{sex}\\{}_{sex}", look.hair),
        (look.hair_color > 0).then(|| format!("data\\palette\\머리\\머리{}_{sex}_{}.pal", look.hair, look.hair_color)),
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
    let weapon = (g.weapon != 0)
        .then(|| tables.weapon(g.weapon))
        .flatten()
        .and_then(|name| sheet(&format!("data\\sprite\\인간족\\{}\\{}_{sex}{name}", look.job, look.job), None).inspect_err(|e| tracing::debug!(weapon = g.weapon, "weapon sprite: {e:#}")).ok());
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
    })
}

struct LoadedMap {
    mesh: ro_render::SceneMesh,
    images: Vec<Option<ro_render::Rgba>>,
    lighting: ro_render::Lighting,
    water: Option<(ro_render::WaterMesh, Vec<Option<ro_render::Rgba>>)>,
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
    Ok(LoadedMap { mesh, images, water, lighting: ro_render::Lighting::from_rsw(&rsw.light) })
}

fn default_ui_scale() -> f32 {
    1.0
}

fn default_msaa() -> u32 {
    4
}

fn default_soak() -> u64 {
    30
}

fn default_backend() -> String {
    "vulkan".into()
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
    const NAMES: [&str; 21] = [
        "Novice", "Swordman", "Mage", "Archer", "Acolyte", "Merchant", "Thief", "Knight", "Priest", "Wizard", "Blacksmith",
        "Hunter", "Assassin", "Knight", "Crusader", "Monk", "Sage", "Rogue", "Alchemist", "Bard", "Dancer",
    ];
    NAMES.get(job as usize).copied().unwrap_or("Adventurer")
}

fn job_sprite(job: i16) -> &'static str {
    match job {
        1 => "검사",
        2 => "마법사",
        3 => "궁수",
        4 => "성직자",
        5 => "상인",
        6 => "도둑",
        7 => "기사",
        8 => "프리스트",
        9 => "위저드",
        10 => "제철공",
        11 => "헌터",
        12 => "어세신",
        14 => "크루세이더",
        15 => "몽크",
        16 => "세이지",
        17 => "로그",
        18 => "연금술사",
        19 => "바드",
        20 => "무희",
        _ => "초보자",
    }
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
}

/// Fractional cell (centre plus the server's twelfth-of-a-cell offset) of a floor item.
fn floor_cell(f: &ro_net::FloorItem) -> (f32, f32) {
    (f.x as f32 + f.sub.0 as f32 / 12.0, f.y as f32 + f.sub.1 as f32 / 12.0)
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
        identified: d.identified,
        cards: d.cards,
        look: d.look,
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
}

#[derive(Clone, Copy, PartialEq)]
enum Pose {
    Attack,
    Hurt,
    Dead,
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
        }
    }

    /// Action group for a pose: player sprites and monster sprites order them differently.
    fn action_for(&self, pose: Pose) -> usize {
        match (self.human, pose) {
            (true, Pose::Attack) => 10,
            (true, Pose::Hurt) => 6,
            (true, Pose::Dead) => 8,
            (false, Pose::Attack) => 2,
            (false, Pose::Hurt) => 3,
            (false, Pose::Dead) => 4,
        }
    }

    fn set_pose(&mut self, pose: Pose, ms: f32) {
        self.pose = Some((pose, ms.max(200.0)));
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
        self.walker.stop_at(from);
        match gat.and_then(|g| ro_game::path::find(g, from, to)) {
            Some(path) => self.walker.walk(path),
            None => self.walker.stop_at(to),
        }
    }

    /// Advances movement and returns the actor to draw. `idle_action` is the pose when
    /// standing; facing is drawn relative to the camera's rotation (`octant`).
    fn update(&mut self, dt: f32, world: &World, octant: i32, idle_action: usize) -> ro_render::Actor {
        let was_walking = self.walker.is_walking();
        let (fx, fy) = self.walker.update(dt);
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
        self.actor.time_ms += dt;
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
    /// Connection problem shown in the title bar.
    status: Option<String>,
    walk_goal: Option<WalkGoal>,
    /// The NPC conversation in progress, and its illustration.
    dialog: Option<hud::Dialog>,
    cutin: Option<hud::Cutin>,
    /// NPC to talk to once we have walked into range.
    pending_npc: Option<u32>,
    /// The monster we keep attacking, and our reach (cells) once the server tells us.
    attack: Option<(u32, i16)>,
    /// Walking toward the attack target after the server said it was out of reach.
    chasing: bool,
    /// Floating damage numbers: (world position, text, colour, age ms).
    floaters: Vec<(glam::Vec3, String, egui::Color32, f32)>,
    item_db: Option<ro_game::items::ItemDb>,
    skill_names: ro_game::skills::SkillNames,
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
    aiming: Option<u16>,
    alt: bool,
    queued_action: Option<windows::UiAction>,
    /// `--windows`: open all game windows (for captures).
    open_windows: bool,
    script: Vec<String>,
    /// Pick up the nearest floor item once one appears (scripted tests).
    auto_pickup: bool,
    /// `--talk name`: talk to the first NPC with this name once in view (scripted test).
    talk_to: Option<String>,
    /// `--attack`: attack the nearest monster once one is in view (scripted test).
    auto_attack: bool,
    /// Other units in view, by id.
    units: std::collections::HashMap<u32, Mob>,
    /// Warp portals in view (drawn as an effect, not a sprite), by unit id.
    warps: std::collections::HashMap<u32, (u16, u16)>,
    class_names: std::collections::HashMap<i16, String>,
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
        let (Some(r), Some(vfs)) = (self.renderer.as_mut(), self.vfs.as_ref()) else { return };
        match load_map(vfs, name) {
            Ok(map) => {
                r.set_scene(&map.mesh, map.images, map.lighting);
                if let Some((mesh, images)) = map.water {
                    r.set_water(&mesh, images);
                }
                let gat = vfs
                    .read(&format!("data\\{name}.gat"))
                    .ok()
                    .flatten()
                    .and_then(|d| ro_formats::gat::Gat::parse(&d).inspect_err(|e| tracing::warn!("{name}.gat: {e:#}")).ok());
                self.world = Some(World { name: name.to_string(), ground: map.mesh, gat });
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
        }
        let actor = load_character(vfs, r, &resolved, &self.look_tables).inspect_err(|e| tracing::warn!("player sprites {key}: {e:#}")).ok();
        self.sprites.insert(key, actor.clone());
        actor
    }

    /// Actor template for an NPC / monster class: data\sprite\npc\ or data\sprite\몬스터\.
    fn class_actor(&mut self, job: i16) -> Option<ro_render::Actor> {
        let name = self.class_names.get(&job)?.clone();
        let key = format!("class:{name}");
        if let Some(a) = self.sprites.get(&key) {
            return a.clone();
        }
        let (r, vfs) = (self.renderer.as_mut()?, self.vfs.as_ref()?);
        let found = ["npc", "몬스터"].iter().find_map(|folder| {
            let base = format!("data\\sprite\\{folder}\\{name}");
            let spr = vfs.read(&format!("{base}.spr")).ok().flatten()?;
            let act = vfs.read(&format!("{base}.act")).ok().flatten()?;
            Some((ro_formats::spr::Spr::parse(&spr).ok()?, ro_formats::act::Act::parse(&act).ok()?))
        });
        let actor = match found {
            Some((spr, act)) => {
                let body = r.add_sheet(&spr, act, None);
                let shadow = self.sprites.get("shadow").cloned().flatten().and_then(|a| a.shadow);
                Some(ro_render::Actor { pos: Default::default(), body, head: None, headgear: vec![], weapon: None, shadow, action: 0, dir: ro_render::Dir::S, time_ms: 0.0, once: false })
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

    /// Applies a ZC_SPRITE_CHANGE to a player-sprite unit (us or another player).
    fn look_change(&mut self, id: u32, kind: u8, value: u32, value2: u32) {
        let Some(mut look) = self.mob_mut(id).and_then(|m| m.look.clone()) else { return };
        match kind {
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
        mob.human = is_player_class(info.job);
        mob.look = look;
        if let ro_net::UnitPos::Walking { from, to } = info.pos {
            let gat = self.world.as_ref().and_then(|w| w.gat.as_ref());
            mob.walk(gat, from, to);
        }
        tracing::debug!(id = info.id, job = info.job, name = %info.name, ?cell, "unit");
        self.units.insert(info.id, mob);
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
        match event {
            Entered { map, x, y, dir, character, account_id } => {
                tracing::info!(%map, x, y, name = %character.name, job = character.job, "entered");
                self.account_id = account_id;
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
                s.base_level = character.base_level as i64;
                s.job_level = character.job_level as i64;
                (s.hp, s.max_hp, s.sp, s.max_sp) = (character.hp, character.max_hp, character.sp, character.max_sp);
                s.base_exp = character.base_exp;
                s.zeny = character.zeny as i64;
                self.spawn_player(&look, (x, y), dir as usize, character.speed);
                if let (Some(text), Some((tx, _))) = (self.say_on_enter.take(), self.net.as_ref()) {
                    let _ = tx.send(ro_net::Command::Chat(text));
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
                if let (9, Some(s)) = (id, self.stats.as_mut()) {
                    s.points = value as u16; // SP_STATUSPOINT
                }
            }
            Chat { kind, text, .. } => self.hud.push_chat(kind, text),
            Npc(d) => self.npc_dialog(d),
            Vanish { id, reason } => {
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
            Act(act) => self.act(act),
            OutOfRange { target, target_cell, range } => {
                // Walk toward the target; the attack is re-sent once in reach.
                self.chasing = true;
                self.attack = Some((target, range.max(1)));
                self.request_walk(target_cell);
            }
            LookChange { id, kind, value, value2 } => self.look_change(id, kind, value, value2),
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
            Stats(s) => self.stats = Some(s),
            StatRaised { stat, ok, value } => {
                if let (true, Some(s), 13..=18) = (ok, self.stats.as_mut(), stat) {
                    s.base[(stat - 13) as usize] = value;
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
            MonsterHp { id, hp, max_hp } => {
                if let Some(m) = self.units.get_mut(&id) {
                    m.hp = Some((hp, max_hp));
                }
            }
            MapMove { map, x, y } => {
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
            Disconnected(reason) => {
                tracing::error!("disconnected: {reason}");
                self.status = Some(format!("disconnected: {reason}"));
            }
        }
    }

    fn send(&self, cmd: ro_net::Command) {
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
            Drop { index, amount } => self.send(Command::Item(ItemRequest::Drop { index, amount })),
            RaiseStat(i) => self.send(Command::Stat(StatRequest::Raise(13 + i as u16))),
            SkillUp(id) => self.send(Command::Stat(StatRequest::SkillUp(id))),
            UseSkill(id) => {
                let Some(s) = self.skills.iter().find(|s| s.id == id) else { return };
                // Self skills go off at once; targeted ones wait for a click.
                if s.target & 4 != 0 {
                    self.use_skill_on(id, self.account_id);
                } else if s.target & (1 | 2 | 16 | 32) != 0 {
                    self.aiming = Some(id);
                }
            }
        }
    }

    fn use_skill_on(&mut self, skill: u16, target: u32) {
        let level = self.skills.iter().find(|s| s.id == skill).map_or(1, |s| s.level.max(1));
        self.walk_goal = None;
        self.send(ro_net::Command::Stat(ro_net::StatRequest::UseSkill { skill, level, target }));
    }

    fn use_skill_at(&mut self, skill: u16, cell: (u16, u16)) {
        let level = self.skills.iter().find(|s| s.id == skill).map_or(1, |s| s.level.max(1));
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
                Some(ro_render::Actor { pos: Default::default(), body, head: None, headgear: vec![], weapon: None, shadow: None, action: 0, dir: ro_render::Dir::S, time_ms: 0.0, once: false })
            }
            _ => None,
        };
        self.sprites.insert(key, actor.clone());
        actor
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
        for s in &self.skills {
            if self.skill_icons.contains_key(&s.id) {
                continue;
            }
            let path = format!("data\\texture\\유저인터페이스\\item\\{}.bmp", s.name.to_ascii_lowercase());
            if let Some(tex) = load_texture(&self.egui_ctx, vfs, &path) {
                self.skill_icons.insert(s.id, tex);
            }
        }
    }

    /// The player or a unit by id.
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
        match act.kind {
            2 | 3 => {
                if let Some(m) = self.mob_mut(act.src) {
                    m.sitting = act.kind == 2;
                }
                return;
            }
            1 => return, // picked something up
            _ => {}
        }
        if let Some(m) = self.mob_mut(act.src) {
            if let Some(c) = target_cell {
                m.face(c);
            }
            m.set_pose(Pose::Attack, act.src_speed as f32);
        }
        let damage = act.damage + act.left_damage;
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
        self.attack = Some((id, 1));
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

        // `--do a,b,c`: scripted actions once the inventory has arrived (tests).
        if !self.script.is_empty() && !self.inventory.items.is_empty() && self.stats.is_some() {
            for step in std::mem::take(&mut self.script) {
                let item = |id: u32| self.inventory.items.values().find(|i| i.id == id).map(|i| (i.index, i.location));
                let action = match step.split_once(':') {
                    Some(("equip", id)) => id.parse().ok().and_then(item).map(|(index, location)| windows::UiAction::Equip { index, location }),
                    Some(("use", id)) => id.parse().ok().and_then(item).map(|(index, _)| windows::UiAction::UseItem(index)),
                    Some(("raise", n)) => n.parse().ok().map(windows::UiAction::RaiseStat),
                    Some(("skillup", id)) => id.parse().ok().map(windows::UiAction::SkillUp),
                    Some(("drop", id)) => id.parse().ok().and_then(item).map(|(index, _)| windows::UiAction::Drop { index, amount: 1 }),
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
            (self.windows.inventory, self.windows.equipment, self.windows.stats, self.windows.skills) = (true, true, true, true);
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

        // Continue a long walk once the current hop has finished (or the server ignored it).
        let idle = self.player.as_ref().is_some_and(|p| !p.walker.is_walking());
        if idle && self.walk_goal.as_ref().is_some_and(|g| g.sent_at.is_none_or(|t| t.elapsed().as_millis() > 500)) {
            self.next_hop();
        }

        let (Some(r), Some(window)) = (self.renderer.as_mut(), self.window.as_ref()) else { return };
        r.actors.clear();
        r.warps = self
            .warps
            .values()
            .filter_map(|&(x, y)| {
                let w = self.world.as_ref()?;
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
                let a = p.update(dt, w, octant, idle);
                r.camera.target = a.pos;
                r.actors.push(a);
            }
            for m in self.units.values_mut() {
                r.actors.push(m.update(dt, w, octant, 0));
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

        // HUD: names under actors, status, chat. Laid out for the capture size when capturing.
        let size = self.capture.as_ref().map_or_else(|| r.size(), |(_, w, h)| (*w, *h));
        let mut labels = vec![];
        for m in self.player.iter().chain(self.units.values()) {
            if let (Some(color), Some(feet)) = (m.label_color(), r.project(m.actor.pos, size)) {
                labels.push(hud::Label { feet, text: m.name.clone(), color });
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
        if let Some(state) = self.egui_state.as_mut() {
            let mut input = state.take_egui_input(window);
            let ppp = window.scale_factor() as f32 * self.ui_scale;
            input.screen_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size.0 as f32, size.1 as f32) / ppp));
            self.egui_ctx.set_pixels_per_point(ppp);
            let mut sent = None;
            let mut answer = None;
            let mut ui_action = None;
            let items: Vec<&ro_game::items::InvItem> = self.inventory.items.values().collect();
            let skill_names = |n: &str| self.skill_names.display(n);
            let view = windows::View {
                items: &items,
                db: self.item_db.as_ref(),
                icons: &self.icons,
                skill_icons: &self.skill_icons,
                stats: self.stats.as_ref(),
                skills: &self.skills,
                skill_names: &skill_names,
                skill_points: self.hud.status.skill_points,
                weight: (self.hud.status.weight, self.hud.status.max_weight),
                zeny: self.hud.status.zeny,
                screen_width: size.0 as f32 / (window.scale_factor() as f32 * self.ui_scale),
            };
            let out = self.egui_ctx.run_ui(input, |ui| {
                sent = self.hud.show(ui.ctx(), &labels, &floaters, &bars);
                answer = self.hud.show_dialog(ui.ctx(), &mut self.dialog, self.cutin.as_ref());
                ui_action = self.windows.show(ui.ctx(), &view);
                if let Some(skill) = self.aiming {
                    let name = self.skills.iter().find(|s| s.id == skill).map(|s| self.skill_names.display(&s.name)).unwrap_or_default();
                    egui::Area::new(egui::Id::unique("aiming")).anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 16.0)).show(ui.ctx(), |ui| {
                        ui.label(egui::RichText::new(format!("{name}: choose a target (Esc to cancel)")).size(15.0).color(egui::Color32::from_rgb(255, 220, 120)));
                    });
                }
            });
            drop(view);
            // Handled next tick: the renderer is borrowed here.
            self.queued_action = ui_action;
            if let (Some((npc, reply)), Some((tx, _))) = (answer, self.net.as_ref()) {
                if reply == ro_net::NpcReply::Close {
                    self.cutin = None;
                }
                let _ = tx.send(ro_net::Command::Npc { npc, reply });
            }
            state.handle_platform_output(window, out.platform_output);
            let primitives = self.egui_ctx.tessellate(out.shapes, out.pixels_per_point);
            r.set_ui(ro_render::UiFrame { primitives, textures: out.textures_delta, pixels_per_point: out.pixels_per_point });
            if let (Some(text), Some((tx, _))) = (sent, self.net.as_ref()) {
                let _ = tx.send(ro_net::Command::Chat(text));
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

        // Offline: show the preview map and character right away. Online waits for the server.
        if self.net.is_none() {
            let map = self.map.clone();
            self.enter_map(&map);
            let look = self.look.clone();
            self.spawn_player(&look, self.at, self.dir, 150);
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        self.tick(el);
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let (Some(r), Some(window)) = (self.renderer.as_mut(), self.window.as_ref()) else { return };
        // The UI sees input first; clicks and keys it uses don't reach the game.
        let ui_used = self.egui_state.as_mut().is_some_and(|s| s.on_window_event(window, &event).consumed);
        if let WindowEvent::CursorMoved { position, .. } = &event {
            // Keep tracking the cursor even over UI so rotation deltas stay continuous.
            if ui_used {
                self.cursor = Some((position.x, position.y));
            }
        }
        if ui_used && !matches!(event, WindowEvent::Resized(_) | WindowEvent::CloseRequested | WindowEvent::ScaleFactorChanged { .. }) {
            return;
        }
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::KeyboardInput { event: winit::event::KeyEvent { logical_key, state: ElementState::Pressed, .. }, .. }
                if logical_key == winit::keyboard::Key::Named(winit::keyboard::NamedKey::Enter) =>
            {
                self.hud.focus_chat();
            }
            WindowEvent::ModifiersChanged(m) => self.alt = m.state().alt_key(),
            WindowEvent::KeyboardInput { event: winit::event::KeyEvent { logical_key, state: ElementState::Pressed, repeat: false, .. }, .. } => {
                use winit::keyboard::{Key, NamedKey};
                match logical_key {
                    Key::Named(NamedKey::Escape) => self.aiming = None,
                    Key::Character(c) if self.alt => match c.to_ascii_lowercase().as_str() {
                        "e" => self.windows.toggle_inventory(),
                        "q" => self.windows.toggle_equipment(),
                        "a" => self.windows.toggle_stats(),
                        "s" => self.windows.toggle_skills(),
                        _ => {}
                    },
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
                    // Monsters: attack until it dies (or use the skill being aimed).
                    Some((id, 5)) => match self.aiming.take() {
                        Some(skill) => self.use_skill_on(skill, id),
                        None => self.attack_unit(id),
                    },
                    _ => {
                        self.attack = None;
                        self.chasing = false;
                        if let Some(skill) = self.aiming.take() {
                            if let Some(cell) = self.pick() {
                                self.use_skill_at(skill, cell);
                            }
                        } else if let Some(item) = self.pick_floor_item() {
                            self.pick_up(item);
                        } else if let Some(cell) = self.pick() {
                            self.pending_npc = None;
                            self.pending_pickup = None;
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
    let cfg = load_config()?;
    // `--map name` / `--at x,y` override the preview map and cell.
    let args: Vec<String> = std::env::args().collect();
    let arg = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();

    if std::env::args().any(|a| a == "--headless") {
        let rt = tokio::runtime::Runtime::new()?;
        return rt.block_on(headless(&cfg));
    }

    let event_loop = EventLoop::new()?;
    let vfs = open_vfs(&cfg)?;
    let cell = |s: String| s.split_once(',').and_then(|(x, y)| Some((x.parse().ok()?, y.parse().ok()?)));
    // Online unless --offline (or no game data to render with).
    let net = (!args.iter().any(|a| a == "--offline") && vfs.is_some()).then(|| {
        ro_net::spawn_session(ro_net::SessionConfig {
            login_addr: cfg.login_addr.clone(),
            username: cfg.username.clone(),
            password: cfg.password.clone(),
            char_slot: cfg.char_slot,
        })
    });
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
        world: None,
        player: None,
        account_id: 0,
        status: None,
        walk_goal: None,
        dialog: None,
        cutin: None,
        pending_npc: None,
        attack: None,
        chasing: false,
        floaters: vec![],
        item_db: None,
        skill_names: Default::default(),
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
        alt: false,
        queued_action: None,
        open_windows: args.iter().any(|a| a == "--windows"),
        auto_pickup: false,
        script: arg("--do").map(|s| s.split(",").map(str::to_string).collect()).unwrap_or_default(),
        talk_to: arg("--talk"),
        auto_attack: args.iter().any(|a| a == "--attack"),
        units: Default::default(),
        warps: Default::default(),
        class_names: Default::default(),
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
        cursor: None,
        fps: FpsCounter::default(),
        egui_ctx: egui::Context::default(),
        egui_state: None,
        hud: hud::Hud::default(),
        ui_scale: cfg.ui_scale,
    };
    app.class_names = app.vfs.as_ref().map(load_class_names).unwrap_or_default();
    if let Some(vfs) = app.vfs.as_ref() {
        // iteminfo.lub lives in the client's System folder; skill names in the GRF.
        let read = |p: &str| vfs.read(p).ok().flatten();
        match read(r"System\iteminfo.lub").map(|d| ro_game::items::ItemDb::from_lub(&d)) {
            Some(Ok(db)) => {
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
    }
    event_loop.run_app(&mut app)?;
    Ok(())
}

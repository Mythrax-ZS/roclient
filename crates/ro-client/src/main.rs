//! roclient
//!
//!   ro-client --headless   run login -> char select -> map entry and exit
//!   ro-client              open the renderer window

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

/// Reads a map's ground and textures from the VFS.
/// Character appearance, as the char server describes it.
#[derive(Clone)]
struct Look {
    /// Job sprite name (Korean, as in the GRF), e.g. 초보자 for Novice.
    job: String,
    male: bool,
    hair: u16,
    hair_color: u16,
}

/// Loads body, head (with hair dye) and shadow sheets for a player character.
fn load_character(vfs: &ro_vfs::Vfs, r: &mut ro_render::Renderer, look: &Look) -> anyhow::Result<ro_render::Actor> {
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
    Ok(ro_render::Actor {
        pos: Default::default(),
        body,
        head: Some(head),
        shadow,
        action: 0,
        dir: ro_render::Dir::S,
        time_ms: 0.0,
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

/// The local player.
struct Player {
    actor: ro_render::Actor,
    walker: ro_game::Walker,
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
    /// `--distance`: initial camera distance.
    distance: Option<f32>,
    /// Offline preview appearance and pose (`--job`, `--sex`, `--action n`, `--dir 0..7`).
    look: Look,
    action: usize,
    dir: usize,
    net: Option<(tokio::sync::mpsc::UnboundedSender<ro_net::Command>, std::sync::mpsc::Receiver<ro_net::Event>)>,
    world: Option<World>,
    player: Option<Player>,
    /// When the current map became ready (for --capture-after).
    ready_at: Option<std::time::Instant>,
    last_tick: std::time::Instant,
    window: Option<Arc<Window>>,
    renderer: Option<ro_render::Renderer>,
    /// Right mouse button held: dragging rotates the camera.
    rotating: bool,
    cursor: Option<(f64, f64)>,
    fps: FpsCounter,
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

    /// Creates (or re-creates) the local player at a cell.
    fn spawn_player(&mut self, look: &Look, cell: (u16, u16), dir: usize, speed: f32) {
        let (Some(r), Some(vfs)) = (self.renderer.as_mut(), self.vfs.as_ref()) else { return };
        match load_character(vfs, r, look) {
            Ok(actor) => {
                self.player = Some(Player { actor, walker: ro_game::Walker::new((cell.0 as i32, cell.1 as i32), dir, speed) });
            }
            Err(e) => tracing::error!("character sprites: {e:#}"),
        }
    }

    /// Asks to walk to a cell: through the server online, locally offline.
    fn request_walk(&mut self, cell: (u16, u16)) {
        match &self.net {
            Some((tx, _)) => {
                let _ = tx.send(ro_net::Command::Move { x: cell.0, y: cell.1 });
            }
            None => {
                let (Some(p), Some(w)) = (self.player.as_mut(), self.world.as_ref()) else { return };
                if let Some(gat) = &w.gat {
                    if let Some(path) = ro_game::path::find(gat, p.walker.cell, (cell.0 as i32, cell.1 as i32)) {
                        p.walker.walk(path);
                    }
                }
            }
        }
    }

    fn handle(&mut self, event: ro_net::Event) {
        use ro_net::Event::*;
        match event {
            Entered { map, x, y, dir, character, .. } => {
                tracing::info!(%map, x, y, name = %character.name, job = character.job, "entered");
                self.enter_map(&map);
                let look = Look {
                    job: job_sprite(character.job).to_string(),
                    male: character.sex == 1,
                    hair: character.head.max(1) as u16,
                    hair_color: character.hair_color as u16,
                };
                let speed = if character.speed > 0 { character.speed as f32 } else { 150.0 };
                self.spawn_player(&look, (x, y), dir as usize, speed);
                if let Some(cell) = self.walk_on_enter.take() {
                    self.request_walk(cell);
                }
            }
            Walk { from, to } => {
                let (Some(p), Some(w)) = (self.player.as_mut(), self.world.as_ref()) else { return };
                let from = (from.0 as i32, from.1 as i32);
                // Continue from where we are; the server's start cell is authoritative.
                p.walker.stop_at(from);
                let path = w.gat.as_ref().and_then(|g| ro_game::path::find(g, from, (to.0 as i32, to.1 as i32)));
                tracing::debug!(?from, ?to, steps = path.as_ref().map(Vec::len), "walk");
                match path {
                    Some(path) => p.walker.walk(path),
                    None => p.walker.stop_at((to.0 as i32, to.1 as i32)),
                }
            }
            Stop { x, y, .. } => {
                if let Some(p) = self.player.as_mut() {
                    p.walker.stop_at((x as i32, y as i32));
                }
            }
            MapMove { map, x, y } => {
                if self.world.as_ref().is_none_or(|w| w.name != map) {
                    self.enter_map(&map);
                }
                if let Some(p) = self.player.as_mut() {
                    p.walker.stop_at((x as i32, y as i32));
                }
            }
            Disconnected(reason) => tracing::error!("disconnected: {reason}"),
        }
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

        let events: Vec<_> = self.net.as_ref().map(|(_, rx)| rx.try_iter().collect()).unwrap_or_default();
        for e in events {
            self.handle(e);
        }

        let (Some(r), Some(window)) = (self.renderer.as_mut(), self.window.as_ref()) else { return };
        r.actors.clear();
        if let (Some(p), Some(w)) = (self.player.as_mut(), self.world.as_ref()) {
            let was_walking = p.walker.is_walking();
            let (fx, fy) = p.walker.update(dt);
            let walking = p.walker.is_walking();
            if walking != was_walking {
                p.actor.time_ms = 0.0; // restart the animation on start/stop
            }
            let (x, z) = ((fx + 0.5) * ro_render::CELL, (fy + 0.5) * ro_render::CELL);
            p.actor.pos = glam::Vec3::new(x, w.ground.height_at(x, z), z);
            p.actor.action = if walking { 1 } else { self.action };
            // Facing is drawn relative to the camera's rotation.
            let octant = (r.camera.yaw / std::f32::consts::FRAC_PI_4).round() as i32;
            p.actor.dir = DIRS[(p.walker.dir as i32 + octant).rem_euclid(8) as usize];
            p.actor.time_ms += dt;
            r.camera.target = p.actor.pos;
            r.actors.push(p.actor.clone());
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
            window.set_title(&format!("roclient — {map} {},{} — {w}x{h} — {fps:.0} fps", at.0, at.1));
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
        self.window = Some(window);
        el.set_control_flow(winit::event_loop::ControlFlow::Poll);

        // Offline: show the preview map and character right away. Online waits for the server.
        if self.net.is_none() {
            let map = self.map.clone();
            self.enter_map(&map);
            let look = self.look.clone();
            self.spawn_player(&look, self.at, self.dir, 150.0);
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        self.tick(el);
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let (Some(r), Some(window)) = (self.renderer.as_mut(), self.window.as_ref()) else { return };
        match event {
            WindowEvent::CloseRequested => el.exit(),
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
                if let Some(cell) = self.pick() {
                    self.request_walk(cell);
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
        look: Look { job: arg("--job").unwrap_or_else(|| "초보자".into()), male: arg("--sex").is_none_or(|s| s != "f"), hair: 1, hair_color: 0 },
        action: arg("--action").and_then(|a| a.parse().ok()).unwrap_or(0),
        dir: arg("--dir").and_then(|d| d.parse::<usize>().ok()).unwrap_or(0) % 8,
        capture_after: arg("--capture-after").and_then(|d| d.parse().ok()).unwrap_or(0.0),
        walk_on_enter: arg("--walk").and_then(cell),
        net,
        world: None,
        player: None,
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
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}

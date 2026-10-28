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
fn load_map(vfs: &ro_vfs::Vfs, name: &str) -> anyhow::Result<(ro_render::TerrainMesh, Vec<Option<ro_render::Rgba>>)> {
    let started = std::time::Instant::now();
    let data = vfs.read(&format!("data\\{name}.gnd"))?.with_context(|| format!("{name}.gnd not found"))?;
    let gnd = ro_formats::gnd::Gnd::parse(&data)?;
    let mesh = ro_render::TerrainMesh::build(&gnd);
    let images = gnd
        .textures
        .iter()
        .map(|t| {
            let path = format!("data\\texture\\{t}");
            match vfs.read(&path).and_then(|d| d.map(|d| ro_formats::image::decode(&d)).transpose()) {
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
        .collect();
    tracing::info!(
        map = name,
        size = format_args!("{}x{}", gnd.width, gnd.height),
        textures = gnd.textures.len(),
        triangles = mesh.vertices.len() / 3,
        ms = started.elapsed().as_millis() as u64,
        "loaded map"
    );
    Ok((mesh, images))
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

struct App {
    backend: String,
    msaa: u32,
    vfs: Option<ro_vfs::Vfs>,
    map: String,
    window: Option<Arc<Window>>,
    renderer: Option<ro_render::Renderer>,
    /// Right mouse button held: dragging rotates the camera.
    rotating: bool,
    last_cursor: Option<(f64, f64)>,
    fps: FpsCounter,
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
            .with_min_inner_size(winit::dpi::LogicalSize::new(640.0, 360.0));
        let window = Arc::new(el.create_window(attrs).unwrap());
        let backends = ro_render::parse_backends(&self.backend);
        match pollster::block_on(ro_render::Renderer::new(window.clone(), backends, self.msaa)) {
            Ok(mut r) => {
                // Prontera spawn until the world is wired to the network session.
                r.camera.look_at_cell(156, 191);
                if let Some(vfs) = &self.vfs {
                    match load_map(vfs, &self.map) {
                        Ok((mesh, images)) => {
                            r.camera.target.y = mesh.height_at(r.camera.target.x, r.camera.target.z);
                            r.set_terrain(&mesh, images);
                        }
                        Err(e) => tracing::error!("map {}: {e:#}", self.map),
                    }
                }
                self.renderer = Some(r);
            }
            Err(e) => {
                tracing::error!("renderer init failed: {e:#}");
                el.exit();
            }
        }
        window.request_redraw();
        self.window = Some(window);
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
            WindowEvent::CursorMoved { position, .. } => {
                if let (true, Some((lx, ly))) = (self.rotating, self.last_cursor) {
                    // Scale by DPI so rotation speed matches across monitors.
                    let s = 0.005 / r.scale_factor as f32;
                    r.camera.rotate((position.x - lx) as f32 * s, (position.y - ly) as f32 * s);
                }
                self.last_cursor = Some((position.x, position.y));
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = r.frame() {
                    tracing::error!("frame: {e:#}");
                }
                if let Some(fps) = self.fps.tick() {
                    let (w, h) = r.size();
                    window.set_title(&format!("roclient — {w}x{h} ({:.2}:1) — {fps:.0} fps", w as f32 / h as f32));
                }
                window.request_redraw();
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

    if std::env::args().any(|a| a == "--headless") {
        let rt = tokio::runtime::Runtime::new()?;
        return rt.block_on(headless(&cfg));
    }

    let event_loop = EventLoop::new()?;
    let vfs = open_vfs(&cfg)?;
    let mut app = App {
        backend: cfg.backend,
        msaa: cfg.msaa,
        vfs,
        map: cfg.map,
        window: None,
        renderer: None,
        rotating: false,
        last_cursor: None,
        fps: FpsCounter::default(),
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}

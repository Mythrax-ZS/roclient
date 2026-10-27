//! roclient
//!
//!   ro-client --headless   run login -> char select -> map entry and exit
//!   ro-client              open the renderer window

use std::sync::Arc;

use anyhow::Context;
use serde::Deserialize;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
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
    window: Option<Arc<Window>>,
    renderer: Option<ro_render::Renderer>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window = Arc::new(el.create_window(Window::default_attributes().with_title("roclient")).unwrap());
        let backends = ro_render::parse_backends(&self.backend);
        match pollster::block_on(ro_render::Renderer::new(window.clone(), backends)) {
            Ok(r) => self.renderer = Some(r),
            Err(e) => {
                tracing::error!("renderer init failed: {e:#}");
                el.exit();
            }
        }
        self.window = Some(window);
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(r) = self.renderer.as_mut() else { return };
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(s) => r.resize(s.width, s.height),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => r.scale_factor = scale_factor,
            WindowEvent::RedrawRequested => {
                if let Err(e) = r.frame() {
                    tracing::error!("frame: {e:#}");
                }
                self.window.as_ref().unwrap().request_redraw();
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
    let mut app = App { backend: cfg.backend, window: None, renderer: None };
    event_loop.run_app(&mut app)?;
    Ok(())
}

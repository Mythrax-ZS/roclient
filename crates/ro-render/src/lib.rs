//! wgpu renderer. Vulkan preferred; DX12 / GL selectable via config.

use std::sync::Arc;

use winit::window::Window;

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pub scale_factor: f64,
}

pub fn parse_backends(name: &str) -> wgpu::Backends {
    match name.to_ascii_lowercase().as_str() {
        "vulkan" => wgpu::Backends::VULKAN,
        "dx12" => wgpu::Backends::DX12,
        "gl" => wgpu::Backends::GL,
        "metal" => wgpu::Backends::METAL,
        _ => wgpu::Backends::all(),
    }
}

impl Renderer {
    pub async fn new(window: Arc<Window>, backends: wgpu::Backends) -> anyhow::Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends,
            ..wgpu::InstanceDescriptor::new_without_display_handle_from_env()
        });
        let surface = instance.create_surface(window.clone())?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await?;
        let info = adapter.get_info();
        tracing::info!(backend = ?info.backend, name = %info.name, "adapter");

        let (device, queue) = adapter.request_device(&wgpu::DeviceDescriptor::default()).await?;
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or_else(|| anyhow::anyhow!("surface not supported by adapter"))?;
        config.present_mode = wgpu::PresentMode::AutoVsync;
        config.desired_maximum_frame_latency = 2;
        surface.configure(&device, &config);
        Ok(Self { surface, device, queue, config, scale_factor: window.scale_factor() })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    /// The camera keeps a fixed vertical FOV, so any aspect ratio (21:9, 32:9) just widens the view.
    pub fn aspect(&self) -> f32 {
        self.config.width as f32 / self.config.height as f32
    }

    pub fn frame(&mut self) -> anyhow::Result<()> {
        let tex = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            other => {
                tracing::warn!(?other, "skipping frame");
                return Ok(());
            }
        };
        let view = tex.texture.create_view(&Default::default());
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let _pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.05, g: 0.06, b: 0.09, a: 1.0 }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            // TODO(M1): terrain, model, sprite, UI passes.
        }
        self.queue.submit([enc.finish()]);
        self.queue.present(tex);
        Ok(())
    }
}

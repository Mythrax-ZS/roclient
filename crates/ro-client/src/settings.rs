//! Settings the player changes in game, saved as settings.toml next to the config file
//! (which stays hand-edited). Values there override the config's defaults.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub bgm_volume: f32,
    pub sfx_volume: f32,
    pub ui_scale: f32,
    pub minimap: bool,
    /// The top-right menu bar is collapsed.
    pub menu_collapsed: bool,
    /// Classic: windows drawn with the original client's art where available; Modern: plain egui.
    pub classic_ui: bool,
    /// Login screen: the last ID used ("Remember ID"), empty when not remembered.
    pub last_id: String,
    /// The HD-2D look (lighting, shadows, post-processing).
    pub visual: Visual,
    /// Texture upscaling for ground, models, item icons and interface art. Character and
    /// effect sprites are never upscaled (see ro_render::upscale).
    pub upscale: ro_render::UpscaleSettings,
    /// The original's /bgm, /sound and /effect switches (volumes are kept while off).
    pub bgm_off: bool,
    pub sfx_off: bool,
    pub effects_off: bool,
    /// /aura2: no level aura at all; /aura: the simplified aura (its sparkles alone).
    pub aura_off: bool,
    pub aura_simple: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { bgm_volume: 0.5, sfx_volume: 0.7, ui_scale: 1.0, minimap: true, menu_collapsed: false, classic_ui: true, last_id: String::new(), visual: Visual::default(), upscale: ro_render::UpscaleSettings::default(), bgm_off: false, sfx_off: false, effects_off: false, aura_off: false, aura_simple: false }
    }
}

/// Artist-facing look controls, mirrored into the renderer's `PostSettings`.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq)]
#[serde(default)]
pub struct Visual {
    /// Off: the classic look (no bloom, blur, grading or sprite shadows).
    pub hd2d: bool,
    pub bloom: f32,
    pub depth_of_field: f32,
    pub vignette: f32,
    pub shadows: f32,
    pub sun_elevation: f32,
    pub sun_cycle: f32,
    /// Water quality preset: 0 Classic, 1 Low, 2 Medium, 3 High, 4 Ultra.
    pub water: u8,
    pub warmth: f32,
    pub saturation: f32,
    pub contrast: f32,
    pub sprite_light: f32,
    pub rim_light: f32,
    pub point_lights: f32,
    pub fog: f32,
    pub god_rays: f32,
    pub ambient_occlusion: f32,
    pub pixel_snap: bool,
    /// Strength of the underwater look on submerged maps (caustics, murk, shafts, motes).
    pub underwater: f32,
}

impl Default for Visual {
    fn default() -> Self {
        let p = ro_render::PostSettings::default();
        Self {
            hd2d: p.enabled,
            bloom: p.bloom_intensity,
            depth_of_field: p.dof_strength,
            vignette: p.vignette,
            shadows: p.shadow_strength,
            sun_elevation: p.sun_elevation,
            sun_cycle: p.sun_cycle_minutes,
            water: 3,
            warmth: p.warmth,
            saturation: p.saturation,
            contrast: p.contrast,
            sprite_light: p.sprite_light,
            rim_light: p.rim_light,
            point_lights: p.point_lights,
            fog: p.fog,
            god_rays: p.god_rays,
            ambient_occlusion: p.ambient_occlusion,
            pixel_snap: p.pixel_snap,
            underwater: 1.0,
        }
    }
}

impl Visual {
    /// The water preset in use; the classic look keeps classic water.
    pub fn water_settings(&self) -> ro_render::WaterSettings {
        let q = if self.hd2d { ro_render::WaterQuality::ALL[(self.water as usize).min(4)] } else { ro_render::WaterQuality::Classic };
        ro_render::WaterSettings::preset(q)
    }

    pub fn post(&self) -> ro_render::PostSettings {
        ro_render::PostSettings {
            enabled: self.hd2d,
            bloom_intensity: self.bloom,
            dof_strength: self.depth_of_field,
            vignette: self.vignette,
            shadow_strength: self.shadows,
            sun_elevation: self.sun_elevation,
            sun_cycle_minutes: self.sun_cycle,
            warmth: self.warmth,
            saturation: self.saturation,
            contrast: self.contrast,
            sprite_light: self.sprite_light,
            rim_light: self.rim_light,
            point_lights: self.point_lights,
            fog: self.fog,
            god_rays: self.god_rays,
            ambient_occlusion: self.ambient_occlusion,
            pixel_snap: self.pixel_snap,
            ..Default::default()
        }
    }
}

fn path() -> PathBuf {
    let config = std::env::var("ROCLIENT_CONFIG").unwrap_or_else(|_| "config/local.toml".into());
    PathBuf::from(config).with_file_name("settings.toml")
}

impl Settings {
    /// The saved settings, or `defaults` if none were saved yet.
    pub fn load(defaults: Settings) -> Self {
        std::fs::read_to_string(path())
            .ok()
            .and_then(|t| toml::from_str(&t).inspect_err(|e| tracing::warn!("settings.toml: {e}")).ok())
            .unwrap_or(defaults)
    }

    pub fn save(&self) {
        let text = toml::to_string(self).expect("settings serialize");
        if let Err(e) = std::fs::write(path(), text) {
            tracing::warn!("save settings: {e}");
        }
    }

    /// The settings window. Returns true when something changed (already saved).
    pub fn show(&mut self, ctx: &egui::Context, open: &mut bool) -> bool {
        let before = self.clone();
        crate::classic::show(ctx, egui::Window::new("Settings").open(open).resizable(false).collapsible(false), "Settings", true, |ui| {
            egui::Grid::new("settings").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
                ui.label("Music");
                ui.add(egui::Slider::new(&mut self.bgm_volume, 0.0..=1.0).show_value(false));
                ui.end_row();
                ui.label("Effects");
                ui.add(egui::Slider::new(&mut self.sfx_volume, 0.0..=1.0).show_value(false));
                ui.end_row();
                ui.label("UI scale");
                // Applied on release, so the slider doesn't move under the pointer while dragging.
                let mut scale = self.ui_scale;
                let r = ui.add(egui::Slider::new(&mut scale, 0.75..=2.0).step_by(0.05));
                if r.drag_stopped() || (r.changed() && !r.dragged()) {
                    self.ui_scale = scale;
                }
                ui.end_row();
                ui.label("Interface");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.classic_ui, true, "Classic");
                    ui.selectable_value(&mut self.classic_ui, false, "Modern");
                });
                ui.end_row();
                ui.label("Minimap");
                ui.checkbox(&mut self.minimap, "Show (Alt+M)");
                ui.end_row();
                ui.label(egui::RichText::new("Visual").strong());
                ui.checkbox(&mut self.visual.hd2d, "HD-2D lighting and effects");
                ui.end_row();
                let v = &mut self.visual;
                ui.add_enabled_ui(v.hd2d, |ui| ui.label("Bloom"));
                ui.add_enabled(v.hd2d, egui::Slider::new(&mut v.bloom, 0.0..=1.0));
                ui.end_row();
                ui.add_enabled_ui(v.hd2d, |ui| ui.label("Depth of field"));
                ui.add_enabled(v.hd2d, egui::Slider::new(&mut v.depth_of_field, 0.0..=1.0));
                ui.end_row();
                ui.add_enabled_ui(v.hd2d, |ui| ui.label("Shadows"));
                ui.add_enabled(v.hd2d, egui::Slider::new(&mut v.shadows, 0.0..=1.0));
                ui.end_row();
                ui.add_enabled_ui(v.hd2d, |ui| ui.label("Sun height"));
                ui.add_enabled(v.hd2d, egui::Slider::new(&mut v.sun_elevation, 15.0..=85.0).suffix("°"));
                ui.end_row();
                ui.add_enabled_ui(v.hd2d, |ui| ui.label("Sun cycle")).response.on_hover_text("Minutes for the sun to cross the sky; 0 keeps it still");
                ui.add_enabled(v.hd2d, egui::Slider::new(&mut v.sun_cycle, 0.0..=180.0).suffix(" min"));
                ui.end_row();
                ui.add_enabled_ui(v.hd2d, |ui| ui.label("Water"));
                ui.add_enabled_ui(v.hd2d, |ui| {
                    egui::ComboBox::from_id_salt("water quality").selected_text(ro_render::WaterQuality::ALL[(v.water as usize).min(4)].name()).show_ui(ui, |ui| {
                        for (i, q) in ro_render::WaterQuality::ALL.iter().enumerate() {
                            ui.selectable_value(&mut v.water, i as u8, q.name());
                        }
                    });
                });
                ui.end_row();
                ui.add_enabled_ui(v.hd2d, |ui| ui.label("Vignette"));
                ui.add_enabled(v.hd2d, egui::Slider::new(&mut v.vignette, 0.0..=1.0));
                ui.end_row();
                ui.add_enabled_ui(v.hd2d, |ui| ui.label("Warmth"));
                ui.add_enabled(v.hd2d, egui::Slider::new(&mut v.warmth, -1.0..=1.0));
                ui.end_row();
                ui.add_enabled_ui(v.hd2d, |ui| ui.label("Saturation"));
                ui.add_enabled(v.hd2d, egui::Slider::new(&mut v.saturation, 0.5..=1.5));
                ui.end_row();
                ui.add_enabled_ui(v.hd2d, |ui| ui.label("Contrast"));
                ui.add_enabled(v.hd2d, egui::Slider::new(&mut v.contrast, 0.7..=1.4));
                ui.end_row();
                for (label, value, max) in [
                    ("Sprite lighting", &mut v.sprite_light, 1.0),
                    ("Rim light", &mut v.rim_light, 1.0),
                    ("Light sources", &mut v.point_lights, 2.0),
                    ("Fog", &mut v.fog, 1.0),
                    ("God rays", &mut v.god_rays, 1.0),
                    ("Ambient occlusion", &mut v.ambient_occlusion, 1.0),
                    ("Underwater", &mut v.underwater, 1.0),
                ] {
                    ui.add_enabled_ui(v.hd2d, |ui| ui.label(label));
                    ui.add_enabled(v.hd2d, egui::Slider::new(value, 0.0..=max));
                    ui.end_row();
                }
                ui.add_enabled_ui(v.hd2d, |ui| ui.label("Pixel snapping"));
                ui.add_enabled(v.hd2d, egui::Checkbox::new(&mut v.pixel_snap, "Crisp sprites"));
                ui.end_row();
                if ui.add_enabled(v.hd2d, egui::Button::new("Reset look")).clicked() {
                    *v = Visual { hd2d: true, ..Visual::default() };
                }
                ui.end_row();
                ui.label(egui::RichText::new("Textures").strong());
                ui.end_row();
                let u = &mut self.upscale;
                ui.label("Upscaling").on_hover_text("Ground, models, item icons and interface art (never character sprites). Load-time upscaling applies from the next map or window opened.");
                use ro_render::UpscaleQuality as Q;
                let names = |q: Q| match q {
                    Q::Off => "Off",
                    Q::Fast => "Fast (filtering only)",
                    Q::Balanced => "Balanced (bicubic)",
                    Q::High => "High (Lanczos)",
                };
                egui::ComboBox::from_id_salt("upscale quality").selected_text(names(u.quality)).show_ui(ui, |ui| {
                    for q in [Q::Off, Q::Fast, Q::Balanced, Q::High] {
                        ui.selectable_value(&mut u.quality, q, names(q));
                    }
                });
                ui.end_row();
                let scaling = matches!(u.quality, Q::Balanced | Q::High);
                ui.add_enabled_ui(scaling, |ui| ui.label("Upscale factor"));
                ui.add_enabled_ui(scaling, |ui| {
                    egui::ComboBox::from_id_salt("upscale factor").selected_text(format!("{}x", u.factor)).show_ui(ui, |ui| {
                        for f in ro_render::UpscaleSettings::FACTORS {
                            ui.selectable_value(&mut u.factor, f, format!("{f}x"));
                        }
                    });
                });
                ui.end_row();
            });
        });
        let changed = *self != before;
        if changed {
            self.save();
        }
        changed
    }
}

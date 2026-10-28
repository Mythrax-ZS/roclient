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
}

impl Default for Settings {
    fn default() -> Self {
        Self { bgm_volume: 0.5, sfx_volume: 0.7, ui_scale: 1.0, minimap: true }
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
        egui::Window::new("Settings").open(open).resizable(false).collapsible(false).show(ctx, |ui| {
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
                ui.label("Minimap");
                ui.checkbox(&mut self.minimap, "Show (Alt+M)");
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

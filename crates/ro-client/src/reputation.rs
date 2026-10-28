//! Reputation (ZC_REPUTE_INFO): standing with each faction, grouped by region, from the
//! client's contentdata\repute\*.bson (names and point limits).

use std::collections::HashMap;

use egui::{Color32, RichText};
use ro_formats::bson::{self, Bson};

pub struct Faction {
    pub name: String,
    pub max_neg: i64,
    pub max_pos: i64,
    pub hidden: bool,
}

#[derive(Default)]
pub struct Reputation {
    pub open: bool,
    factions: HashMap<u64, Faction>,
    groups: Vec<(String, Vec<u64>)>,
    pub points: HashMap<u64, i64>,
    loaded: bool,
}

/// "GrayWolfVillage" -> "Gray Wolf Village".
fn spaced(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && c.is_uppercase() && !out.ends_with(' ') {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

impl Reputation {
    pub fn load(&mut self, vfs: Option<&ro_vfs::Vfs>) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        let Some(vfs) = vfs else { return };
        let read = |n: &str| vfs.read(&format!(r"data\contentdata\repute\{n}.bson")).ok().flatten().and_then(|d| bson::parse(&d).ok());
        if let Some(info) = read("reputeinfodata").as_ref().and_then(|d| d.get("reputeInfo").cloned()) {
            if let Bson::Doc(list) = info {
                for (id, f) in list {
                    let Ok(id) = id.parse::<u64>() else { continue };
                    self.factions.insert(id, Faction {
                        name: spaced(f.get("Name").and_then(Bson::as_str).unwrap_or("?")),
                        max_neg: f.get("MaxPoint_Negative").and_then(Bson::as_i64).unwrap_or(0),
                        max_pos: f.get("MaxPoint_Positive").and_then(Bson::as_i64).unwrap_or(0),
                        hidden: f.get("Invisible").and_then(Bson::as_str) == Some("VISIBLE_FALSE"),
                    });
                }
            }
        }
        if let Some(Bson::Doc(groups)) = read("reputegroupdata").as_ref().and_then(|d| d.get("ReputeGroup").cloned()) {
            for (_, g) in groups {
                let name = g.get("Name").and_then(Bson::as_str).unwrap_or("?").to_string();
                let ids = g.get("ReputeList").map(|l| l.items().iter().filter_map(|v| v.as_i64()).map(|v| v as u64).collect()).unwrap_or_default();
                self.groups.push((name, ids));
            }
        }
        tracing::info!(factions = self.factions.len(), groups = self.groups.len(), "reputation data");
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        let mut open = true;
        crate::classic::show(ctx, egui::Window::new("Reputation").open(&mut open).resizable(false).default_width(340.0), "Reputation", true, |ui| {
            if self.groups.is_empty() {
                ui.label("No reputation data in the client files.");
            }
            for (group, ids) in &self.groups {
                ui.label(RichText::new(group).strong());
                for id in ids {
                    let Some(f) = self.factions.get(id) else { continue };
                    let pts = self.points.get(id).copied().unwrap_or(0);
                    if f.hidden && pts == 0 {
                        continue;
                    }
                    ui.horizontal(|ui| {
                        ui.label(&f.name);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let (max, color) = if pts < 0 { (f.max_neg, Color32::from_rgb(220, 90, 80)) } else { (f.max_pos, Color32::from_rgb(90, 190, 110)) };
                            let frac = if max > 0 { (pts.abs() as f32 / max as f32).min(1.0) } else { 0.0 };
                            ui.add(egui::ProgressBar::new(frac).desired_width(140.0).fill(color).text(format!("{pts}")));
                        });
                    });
                }
                ui.add_space(4.0);
            }
        });
        self.open = open;
    }
}

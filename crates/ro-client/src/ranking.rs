//! Ranking windows (/blacksmith, /alchemist, /taekwon, /pk, or the menu): the top ten of a
//! fame list, names looked up by character id.

use std::collections::HashMap;

use egui::{Color32, RichText};

pub const KINDS: [&str; 4] = ["Blacksmith", "Alchemist", "Taekwon", "PK"];

#[derive(Default)]
pub struct Ranking {
    pub open: bool,
    pub kind: u16,
    pub list: Vec<(u32, u32)>,
    pub mine: u32,
    pub names: HashMap<u32, String>,
}

impl Ranking {
    /// Names still to look up.
    pub fn missing(&self) -> Vec<u32> {
        self.list.iter().map(|(c, _)| *c).filter(|c| !self.names.contains_key(c)).collect()
    }

    /// Returns a ranking type to request.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<u16> {
        if !self.open {
            return None;
        }
        let mut ask = None;
        let mut open = true;
        crate::classic::show(ctx, egui::Window::new("Ranking").open(&mut open).resizable(false).default_width(280.0), "Ranking", true, |ui| {
            ui.horizontal(|ui| {
                for (i, k) in KINDS.iter().enumerate() {
                    if ui.selectable_label(self.kind == i as u16, *k).clicked() {
                        ask = Some(i as u16);
                    }
                }
            });
            ui.separator();
            if self.list.is_empty() {
                ui.label(RichText::new("No one is ranked yet.").color(crate::classic::ink(Color32::GRAY)));
            }
            egui::Grid::new("ranks").num_columns(3).spacing([14.0, 4.0]).show(ui, |ui| {
                for (i, (cid, pts)) in self.list.iter().enumerate() {
                    ui.label(format!("{}.", i + 1));
                    ui.label(self.names.get(cid).cloned().unwrap_or_else(|| "...".into()));
                    ui.label(format!("{pts}"));
                    ui.end_row();
                }
            });
            ui.separator();
            ui.label(format!("Your points: {}", self.mine));
        });
        self.open = open;
        ask
    }
}

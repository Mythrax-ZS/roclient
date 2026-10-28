//! The clan window: our clan's name, master, home map, members online and its allied and
//! rival clans. Clans are joined and left through NPCs; "/cl text" talks in clan chat.

use egui::{Color32, RichText};
use ro_net::ClanInfo;

#[derive(Default)]
pub struct Clan {
    pub open: bool,
    pub info: Option<ClanInfo>,
    /// Members online and the clan's maximum.
    pub online: Option<(u16, u16)>,
}

impl Clan {
    pub fn left(&mut self) {
        self.info = None;
        self.online = None;
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        let mut open = true;
        crate::classic::show(ctx, egui::Window::new("Clan").open(&mut open).resizable(false).default_width(260.0), "Clan", true, |ui| {
            let Some(c) = &self.info else {
                ui.label("You are not in a clan.");
                ui.label(RichText::new("Clan helpers in the cities can tell you how to join one.").size(11.0).color(crate::classic::ink(Color32::GRAY)));
                return;
            };
            ui.label(RichText::new(&c.name).strong().size(16.0));
            egui::Grid::new("clan_info").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                ui.label("Master");
                ui.label(&c.master);
                ui.end_row();
                ui.label("Home");
                ui.label(c.map.trim_end_matches(".gat"));
                ui.end_row();
                if let Some((on, max)) = self.online {
                    ui.label("Online");
                    ui.label(format!("{on} / {max}"));
                    ui.end_row();
                }
            });
            for (title, names, color) in [("Allies", &c.allies, Color32::from_rgb(120, 200, 140)), ("Rivals", &c.antagonists, Color32::from_rgb(230, 120, 110))] {
                ui.separator();
                ui.label(RichText::new(title).strong().color(crate::classic::ink(color)));
                if names.is_empty() {
                    ui.label(RichText::new("None").color(crate::classic::ink(Color32::GRAY)));
                }
                for n in names {
                    ui.label(n);
                }
            }
            ui.separator();
            ui.label(RichText::new("Type /cl message to talk to your clan.").size(11.0).color(crate::classic::ink(Color32::GRAY)));
        });
        self.open = open;
    }
}

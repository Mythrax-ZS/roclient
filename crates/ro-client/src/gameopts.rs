//! Game Options: settings the server keeps for the character (the original's /call,
//! /noshift-like toggles, auto-feeding, party invites) and the party's sharing rules.

use egui::{Color32, RichText};
use ro_net::OptionRequest;

#[derive(Default)]
pub struct GameOptions {
    pub open: bool,
    pub deny_invites: bool,
    pub pet_autofeed: bool,
    pub homun_autofeed: bool,
    pub hide_costumes: bool,
    pub call: bool,
    /// Party sharing, once the server has said: (exp, pickup, item).
    pub party: Option<(bool, bool, bool)>,
    /// GM accounts (config `gm`): whether our GM look is on, shown as a toggle; and the
    /// change asked for (sent as @gmlook on / off).
    pub gm_look: Option<bool>,
    pub gm_look_asked: Option<bool>,
}

impl GameOptions {
    pub fn config(&mut self, kind: u32, on: bool) {
        match kind {
            1 => self.call = on,
            2 => self.pet_autofeed = on,
            3 => self.homun_autofeed = on,
            5 => self.hide_costumes = on,
            _ => {}
        }
    }

    /// `leader`: whether we may change the party's rules.
    pub fn show(&mut self, ctx: &egui::Context, leader: bool) -> Vec<OptionRequest> {
        let mut out = vec![];
        if !self.open {
            return out;
        }
        let mut open = true;
        crate::classic::show(ctx, egui::Window::new("Game Options").open(&mut open).resizable(false), "Game Options", true, |ui| {
            let mut toggle = |ui: &mut egui::Ui, v: &mut bool, label: &str, req: &dyn Fn(bool) -> OptionRequest| {
                if ui.checkbox(v, label).changed() {
                    out.push(req(*v));
                }
            };
            toggle(ui, &mut self.deny_invites, "Refuse party invitations", &|v| OptionRequest::DenyPartyInvites(v));
            toggle(ui, &mut self.pet_autofeed, "Feed my pet automatically", &|v| OptionRequest::Config { kind: 2, on: v });
            toggle(ui, &mut self.homun_autofeed, "Feed my homunculus automatically", &|v| OptionRequest::Config { kind: 3, on: v });
            toggle(ui, &mut self.hide_costumes, "Hide other players' costumes", &|v| OptionRequest::Config { kind: 5, on: v });
            toggle(ui, &mut self.call, "Allow being summoned by my spouse (/call)", &|v| OptionRequest::Config { kind: 1, on: v });
            if let Some(mut on) = self.gm_look {
                ui.separator();
                ui.label(RichText::new("Game Master").strong());
                if ui.checkbox(&mut on, "GM look (GM sprite and yellow name, seen by everyone)").changed() {
                    self.gm_look_asked = Some(on);
                }
            }
            if let Some((mut exp, mut pickup, mut item)) = self.party {
                ui.separator();
                ui.label(RichText::new("Party").strong());
                let mut changed = false;
                ui.add_enabled_ui(leader, |ui| {
                    changed |= ui.checkbox(&mut exp, "Share experience evenly").changed();
                    changed |= ui.checkbox(&mut pickup, "Share items picked up").changed();
                    changed |= ui.checkbox(&mut item, "Distribute items in turn").changed();
                });
                if !leader {
                    ui.label(RichText::new("Only the party leader can change these.").size(11.0).color(crate::classic::ink(Color32::GRAY)));
                }
                if changed {
                    out.push(OptionRequest::PartyOptions { exp_share: exp, pickup_share: pickup, item_share: item });
                }
            }
        });
        self.open = open;
        out
    }
}

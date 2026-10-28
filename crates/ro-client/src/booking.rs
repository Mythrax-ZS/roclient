//! Party booking (Alt+N): search the recruitment board, whisper a leader to ask in, and
//! post or remove our own ad.

use egui::{Color32, RichText};
use ro_net::{BookingAd, BookingRequest};

#[derive(Default)]
pub struct Booking {
    pub open: bool,
    pub ads: Vec<BookingAd>,
    /// Search filter (0: any level) and the level our ad asks for.
    level: u16,
    ad_level: u16,
    /// Our ad is posted.
    pub posted: bool,
    pub status: String,
}

/// What the window asks for: a request, or a whisper to an ad's leader.
pub enum BookingAction {
    Send(BookingRequest),
    Whisper(String),
}

impl Booking {
    pub fn show(&mut self, ctx: &egui::Context, my_level: u16, job_name: &dyn Fn(u16) -> String) -> Vec<BookingAction> {
        let mut out = vec![];
        if !self.open {
            return out;
        }
        if self.ad_level == 0 {
            self.ad_level = my_level.saturating_sub(10).max(1);
        }
        let mut open = true;
        crate::classic::show(ctx, egui::Window::new("Party Booking").open(&mut open).resizable(false).default_width(440.0).default_pos(egui::pos2(400.0, 150.0)), "Party Booking", true, |ui| {
            ui.horizontal(|ui| {
                ui.label("Level");
                ui.add(egui::DragValue::new(&mut self.level).range(0..=999)).on_hover_text("0 lists every level; otherwise ads within 15 levels below");
                if ui.button("Search").clicked() {
                    out.push(BookingAction::Send(BookingRequest::Search { level: self.level, map: 0, job: 0xffff }));
                }
                ui.separator();
                if self.posted {
                    if ui.button("Remove my ad").clicked() {
                        out.push(BookingAction::Send(BookingRequest::Delete));
                    }
                } else if ui.button("Post ad").on_hover_text("Recruit for your party at this level").clicked() {
                    out.push(BookingAction::Send(BookingRequest::Register { level: if self.level > 0 { self.level } else { self.ad_level }, map: 0, jobs: vec![] }));
                }
            });
            if !self.status.is_empty() {
                ui.label(RichText::new(&self.status).color(crate::classic::ink(Color32::from_rgb(255, 210, 110))));
            }
            ui.separator();
            if self.ads.is_empty() {
                ui.label(RichText::new("No parties are recruiting. Search to look again.").color(crate::classic::ink(Color32::GRAY)));
            }
            egui::Grid::new("booking_ads").num_columns(4).spacing([12.0, 4.0]).show(ui, |ui| {
                for ad in &self.ads {
                    ui.label(RichText::new(&ad.leader).strong());
                    ui.label(format!("Lv {}+", ad.level));
                    let jobs = if ad.jobs.is_empty() { "Any job".into() } else { ad.jobs.iter().map(|j| job_name(*j)).collect::<Vec<_>>().join(", ") };
                    ui.label(RichText::new(jobs).size(11.5));
                    if ui.small_button("Whisper").clicked() {
                        out.push(BookingAction::Whisper(ad.leader.clone()));
                    }
                    ui.end_row();
                }
            });
        });
        if !open {
            self.open = false;
        }
        out
    }
}

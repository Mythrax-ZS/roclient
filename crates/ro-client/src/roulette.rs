//! The roulette window (/roulette, or the menu): seven rows of prizes, the top row with nine
//! slots and each row below one fewer. A spin costs one bronze point (or ten silver, starting
//! on row 3, or ten gold, starting on row 5); a win climbs a row, and the prize can be claimed
//! or risked on the next spin. The bonus item pays double. Prizes come from rAthena's
//! roulette database (ZC_ACK_ROULEITTE_INFO).

use std::collections::HashMap;

use egui::{Color32, RichText, Vec2};

const ROWS: usize = 7;
const COLUMNS: usize = 9;

#[derive(Default)]
pub struct Roulette {
    pub open: bool,
    /// (row, column) -> (item, amount).
    prizes: HashMap<(u16, u16), (u32, u16)>,
    /// Gold, silver, bronze.
    points: [u32; 3],
    bonus: u32,
    /// The prize won and not yet claimed.
    won: Option<(u16, u16)>,
    /// The row the next spin plays.
    row: u16,
    note: String,
}

pub enum Action {
    Spin,
    Claim,
    Close,
}

impl Roulette {
    pub fn items(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self.prizes.values().map(|p| p.0).collect();
        v.push(self.bonus);
        v
    }

    pub fn apply(&mut self, note: ro_net::RouletteNote, name: impl Fn(u32) -> String) {
        use ro_net::RouletteNote::*;
        match note {
            Opened { ok, step, idx, bonus, points } => {
                self.open = ok;
                self.points = points;
                self.bonus = bonus;
                self.won = (idx >= 0).then_some((step.max(0) as u16, idx as u16));
                self.row = self.won.map_or(0, |(r, _)| r + 1);
                self.note.clear();
            }
            Prizes(list) => self.prizes = list.into_iter().map(|(r, c, i, n)| ((r, c), (i, n))).collect(),
            Spun { result, step, idx, bonus, points } => {
                self.points = points;
                self.bonus = bonus;
                match result {
                    0 => {
                        self.won = Some((step, idx));
                        self.row = step + 1;
                        let (item, n) = self.prizes.get(&(step, idx)).copied().unwrap_or_default();
                        self.note = format!("You won {} x{n}. Claim it, or spin again for a better prize.", name(item));
                    }
                    3 => {
                        self.won = None;
                        self.row = 0;
                        self.note = "No luck this time.".into();
                    }
                    2 => self.note = "Not enough points.".into(),
                    _ => self.note = "The roulette failed.".into(),
                }
            }
            Claimed { result, bonus } => {
                self.bonus = bonus;
                self.note = match result {
                    0 => "Prize received.".into(),
                    2 => "You have too many items to receive the prize.".into(),
                    3 => "You are carrying too much to receive the prize.".into(),
                    _ => "The prize could not be received.".into(),
                };
                if result == 0 {
                    self.won = None;
                    self.row = 0;
                }
            }
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, icons: &HashMap<u32, egui::TextureHandle>, name: impl Fn(u32) -> String) -> Option<Action> {
        if !self.open {
            return None;
        }
        let mut act = None;
        let mut open = true;
        crate::classic::show(ctx, egui::Window::new("Roulette").open(&mut open).resizable(false).collapsible(false), "Roulette", true, |ui| {
            ui.horizontal(|ui| {
                for (label, n, c) in [("Gold", self.points[0], Color32::from_rgb(230, 190, 60)), ("Silver", self.points[1], Color32::from_gray(190)), ("Bronze", self.points[2], Color32::from_rgb(190, 120, 70))] {
                    ui.label(RichText::new(format!("{label} {n}")).color(c).strong());
                    ui.add_space(8.0);
                }
            });
            if self.bonus != 0 {
                ui.label(RichText::new(format!("Bonus: {} (double reward)", name(self.bonus))).color(Color32::from_rgb(240, 200, 90)));
            }
            ui.add_space(4.0);
            for row in 0..ROWS as u16 {
                ui.horizontal(|ui| {
                    // Rows narrow toward the bottom: centre them.
                    ui.add_space(row as f32 * 19.0);
                    for col in 0..(COLUMNS as u16 - row) {
                        let (size, _) = ui.allocate_exact_size(Vec2::splat(34.0), egui::Sense::hover());
                        let won = self.won == Some((row, col));
                        let current = self.won.is_none() && row == self.row || self.won.is_some_and(|(r, _)| r + 1 == row);
                        let p = ui.painter();
                        let fill = if won { Color32::from_rgb(120, 90, 20) } else if current { Color32::from_rgb(45, 60, 90) } else { Color32::from_gray(30) };
                        p.rect_filled(size, 3.0, fill);
                        p.rect_stroke(size, 3.0, egui::Stroke::new(if won { 2.0 } else { 1.0 }, if won { Color32::GOLD } else { Color32::from_gray(80) }), egui::StrokeKind::Inside);
                        if let Some(&(item, n)) = self.prizes.get(&(row, col)) {
                            if let Some(t) = icons.get(&item) {
                                p.image(t.id(), egui::Rect::from_center_size(size.center(), Vec2::splat(24.0)), egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                            }
                            if n > 1 {
                                p.text(size.right_bottom() - Vec2::new(2.0, 1.0), egui::Align2::RIGHT_BOTTOM, n.to_string(), egui::FontId::proportional(10.0), Color32::WHITE);
                            }
                            ui.interact(size, ui.scope_id().with((row, col)), egui::Sense::hover()).on_hover_text(format!("{} x{n}", name(item)));
                        }
                    }
                });
            }
            ui.add_space(4.0);
            if !self.note.is_empty() {
                ui.label(&self.note);
            }
            ui.horizontal(|ui| {
                if ui.button("Spin").clicked() {
                    act = Some(Action::Spin);
                }
                if ui.add_enabled(self.won.is_some(), egui::Button::new("Claim")).clicked() {
                    act = Some(Action::Claim);
                }
            });
        });
        if !open {
            self.open = false;
            act = Some(Action::Close);
        }
        act
    }
}

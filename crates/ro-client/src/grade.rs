//! Grade enhancement (the grade enchant NPC's window): pick a piece of equipment, see the
//! chance and the ways to raise its grade (material, zeny, risk), optionally add catalyst
//! steps for a better chance, and try.

use std::collections::HashMap;

use egui::{Color32, RichText, Vec2};
use ro_game::items::{InvItem, ItemDb};
use ro_net::{GradeOptions, GradeRequest};

pub const LETTERS: [&str; 5] = ["None", "D", "C", "B", "A"];

#[derive(Default)]
pub struct Grade {
    pub open: bool,
    /// The item picked, and the server's options for it.
    selected: Option<u16>,
    pub options: Option<GradeOptions>,
    material: usize,
    blessing: u32,
    pub status: String,
}

fn ink(c: Color32) -> Color32 {
    crate::classic::ink(c)
}

impl Grade {
    /// Test hooks: pick an item, and try with a material.
    pub fn pick(&mut self, index: u16) {
        self.selected = Some(index);
        self.options = None;
    }

    pub fn upgrade(&mut self, material: u32) -> Option<GradeRequest> {
        let index = self.selected?;
        self.status = "Upgrading...".into();
        Some(GradeRequest::Upgrade { index, material, blessing: 0 })
    }

    /// After a try: say how it went, and ask for the item's options again.
    pub fn result(&mut self, grade: u16, result: u32) -> Option<GradeRequest> {
        let letter = LETTERS.get(grade as usize).copied().unwrap_or("?");
        self.status = match result {
            0 => format!("Success! The grade is now {letter}."),
            1 => "The upgrade failed.".into(),
            2 => format!("The upgrade failed and the grade dropped to {letter}."),
            3 => "The upgrade failed and the item was destroyed.".into(),
            4 => "The upgrade failed, but the item was protected.".into(),
            _ => format!("Result {result}."),
        };
        self.options = None;
        if result == 3 {
            self.selected = None;
            return None;
        }
        self.selected.map(GradeRequest::Select)
    }

    pub fn show(&mut self, ctx: &egui::Context, items: &[&InvItem], db: Option<&ItemDb>, icons: &HashMap<u32, egui::TextureHandle>, zeny: i64) -> Vec<GradeRequest> {
        let mut out = vec![];
        if !self.open {
            return out;
        }
        let name = |id: u32| db.map_or_else(|| format!("Item #{id}"), |d| d.name(id, true));
        let mut open = true;
        crate::classic::show(ctx, egui::Window::new("Grade Enhancement").open(&mut open).fixed_size(Vec2::new(560.0, 340.0)).default_pos(egui::pos2(380.0, 200.0)), "Grade Enhancement", true, |ui| {
            ui.horizontal_top(|ui| {
                // Equipment that could be graded: refined weapons and armor.
                ui.vertical(|ui| {
                    ui.set_width(230.0);
                    ui.label(RichText::new("Choose equipment").strong());
                    egui::ScrollArea::vertical().id_salt("grade_items").max_height(300.0).auto_shrink([false, false]).show(ui, |ui| {
                        let mut gear: Vec<_> = items.iter().filter(|i| i.is_equipment() && i.location & 0x3ff != 0).collect();
                        gear.sort_by_key(|i| (std::cmp::Reverse(i.refine), i.index));
                        for it in gear {
                            ui.horizontal(|ui| {
                                if let Some(t) = icons.get(&it.id) {
                                    ui.image((t.id(), Vec2::splat(20.0)));
                                }
                                let grade = if it.grade > 0 { format!(" [{}]", LETTERS[it.grade.min(4) as usize]) } else { String::new() };
                                let label = format!("+{} {}{grade}", it.refine, name(it.id));
                                if ui.selectable_label(self.selected == Some(it.index), label).clicked() {
                                    self.selected = Some(it.index);
                                    self.options = None;
                                    self.material = 0;
                                    self.blessing = 0;
                                    self.status.clear();
                                    out.push(GradeRequest::Select(it.index));
                                }
                            });
                        }
                    });
                });
                ui.separator();
                ui.vertical(|ui| {
                    let Some(index) = self.selected else {
                        ui.label(RichText::new("Pick an item on the left.").color(ink(Color32::GRAY)));
                        ui.label(RichText::new("Grades need +9 or better weapons (level 5) and armor (level 2).").size(11.0).color(ink(Color32::GRAY)));
                        return;
                    };
                    let Some(o) = &self.options else {
                        ui.label(RichText::new("Asking...").color(ink(Color32::GRAY)));
                        if !self.status.is_empty() {
                            ui.label(&self.status);
                        }
                        return;
                    };
                    if o.index.is_none() || o.materials.is_empty() {
                        ui.label("This item can't be graded any further.");
                        return;
                    }
                    let (b_item, b_per, b_max, b_bonus) = o.blessing;
                    let chance = (o.chance + self.blessing * b_bonus).min(100);
                    ui.label(RichText::new(format!("Success chance: {chance}%")).strong().size(15.0));
                    ui.add_space(4.0);
                    for (i, m) in o.materials.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut self.material, i, "");
                            if let Some(t) = icons.get(&m.item) {
                                ui.image((t.id(), Vec2::splat(20.0)));
                            }
                            let have: u32 = items.iter().filter(|it| it.id == m.item).map(|it| it.amount as u32).sum();
                            let color = if have >= m.amount { ink(Color32::WHITE) } else { Color32::from_rgb(230, 110, 100) };
                            ui.label(RichText::new(format!("{} x{} ({have})", name(m.item), m.amount)).color(color));
                            ui.label(format!("{} z", m.zeny));
                        });
                        let mut risk = vec![];
                        if m.downgrade {
                            risk.push("grade can drop");
                        }
                        if m.breakable {
                            risk.push("item can break");
                        }
                        if !risk.is_empty() {
                            ui.label(RichText::new(format!("      On failure: {}", risk.join(", "))).size(11.0).color(ink(Color32::from_rgb(230, 160, 90))));
                        }
                    }
                    if b_item != 0 && b_max > 0 {
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label(format!("{} (x{b_per} per step, +{b_bonus}% each)", name(b_item)));
                            ui.add(egui::DragValue::new(&mut self.blessing).range(0..=b_max));
                        });
                    }
                    ui.separator();
                    let m = &o.materials[self.material.min(o.materials.len() - 1)];
                    let ok = (m.zeny as i64) <= zeny && items.iter().filter(|it| it.id == m.item).map(|it| it.amount as u32).sum::<u32>() >= m.amount;
                    if ui.add_enabled(ok, egui::Button::new("Upgrade")).clicked() {
                        out.push(GradeRequest::Upgrade { index, material: self.material as u32, blessing: self.blessing });
                        self.status = "Upgrading...".into();
                    }
                    if !self.status.is_empty() {
                        ui.label(RichText::new(&self.status).color(ink(Color32::from_rgb(255, 210, 110))));
                    }
                });
            });
        });
        if !open {
            self.open = false;
            self.selected = None;
            self.options = None;
            out.push(GradeRequest::Close);
        }
        out
    }
}

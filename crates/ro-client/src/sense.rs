//! The Sense / Monster Information window (ZC_MONSTER_INFO): a monster's level, HP,
//! size, race, element, defences and the damage it takes from each element.

use egui::{Color32, RichText};

pub struct MonsterInfo {
    pub name: String,
    pub level: u16,
    pub size: u16,
    pub hp: u32,
    pub def: i16,
    pub race: u16,
    pub mdef: i16,
    pub element: u16,
    /// Damage taken (%) from water, earth, fire, wind, poison, holy, shadow, ghost, undead.
    pub mods: [u8; 9],
}

const SIZES: [&str; 3] = ["Small", "Medium", "Large"];
const RACES: [&str; 10] = ["Formless", "Undead", "Brute", "Plant", "Insect", "Fish", "Demon", "Demi-Human", "Angel", "Dragon"];
const ELEMENTS: [&str; 10] = ["Neutral", "Water", "Earth", "Fire", "Wind", "Poison", "Holy", "Shadow", "Ghost", "Undead"];

#[derive(Default)]
pub struct Sense {
    pub info: Option<MonsterInfo>,
}

impl Sense {
    pub fn show(&mut self, ctx: &egui::Context) {
        let Some(m) = &self.info else { return };
        let mut open = true;
        crate::classic::show(ctx, egui::Window::new("Monster Information").open(&mut open).resizable(false).collapsible(false), "Monster Information", true, |ui| {
            ui.label(RichText::new(&m.name).strong().size(15.0));
            let gray = crate::classic::ink(Color32::GRAY);
            egui::Grid::new("sense").num_columns(4).spacing([14.0, 4.0]).show(ui, |ui| {
                let cell = |ui: &mut egui::Ui, k: &str, v: String| {
                    ui.label(RichText::new(k).color(gray));
                    ui.label(v);
                };
                cell(ui, "Level", m.level.to_string());
                cell(ui, "HP", m.hp.to_string());
                ui.end_row();
                cell(ui, "Size", SIZES.get(m.size as usize).unwrap_or(&"?").to_string());
                cell(ui, "Race", RACES.get(m.race as usize).unwrap_or(&"?").to_string());
                ui.end_row();
                cell(ui, "Def", m.def.to_string());
                cell(ui, "Mdef", m.mdef.to_string());
                ui.end_row();
                cell(ui, "Element", ELEMENTS.get(m.element as usize % 10).unwrap_or(&"?").to_string());
                ui.end_row();
            });
            ui.separator();
            egui::Grid::new("sense_elements").num_columns(6).spacing([10.0, 3.0]).show(ui, |ui| {
                for (i, pct) in m.mods.iter().enumerate() {
                    ui.label(RichText::new(ELEMENTS[i + 1]).color(gray));
                    let c = if *pct > 100 { Color32::from_rgb(220, 90, 70) } else if *pct < 100 { Color32::from_rgb(90, 140, 220) } else { crate::classic::ink(Color32::WHITE) };
                    ui.label(RichText::new(format!("{pct}%")).color(c));
                    if i % 3 == 2 {
                        ui.end_row();
                    }
                }
            });
        });
        if !open {
            self.info = None;
        }
    }
}

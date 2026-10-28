//! Item merging (script `mergeitem`): stacks of the same item that the server split
//! (different unique ids) can be joined back into one.

use std::collections::{BTreeMap, HashMap};

use egui::{RichText, Vec2};
use ro_game::items::{InvItem, ItemDb};

#[derive(Default)]
pub struct Merge {
    /// Mergeable inventory indices, while the window is open.
    pub open: Option<Vec<u16>>,
}

impl Merge {
    /// Returns the indices to merge once one is chosen.
    pub fn show(&mut self, ctx: &egui::Context, inv: &BTreeMap<u16, InvItem>, db: Option<&ItemDb>, icons: &HashMap<u32, egui::TextureHandle>) -> Option<Vec<u16>> {
        let list = self.open.as_ref()?;
        let mut by_item: BTreeMap<u32, Vec<u16>> = BTreeMap::new();
        for i in list {
            if let Some(it) = inv.get(i) {
                by_item.entry(it.id).or_default().push(*i);
            }
        }
        let mut out = None;
        let mut open = true;
        crate::classic::show(ctx, egui::Window::new("Merge Items").open(&mut open).resizable(false), "Merge Items", true, |ui| {
            if by_item.values().all(|v| v.len() < 2) {
                ui.label("Nothing to merge.");
            }
            for (id, idx) in by_item.iter().filter(|(_, v)| v.len() > 1) {
                ui.horizontal(|ui| {
                    if let Some(t) = icons.get(id) {
                        ui.image((t.id(), Vec2::splat(22.0)));
                    }
                    let total: u32 = idx.iter().filter_map(|i| inv.get(i)).map(|i| i.amount as u32).sum();
                    let name = db.map_or_else(|| format!("Item #{id}"), |d| d.name(*id, true));
                    ui.label(RichText::new(format!("{name}: {} stacks, {total} in all", idx.len())));
                    if ui.button("Merge").clicked() {
                        out = Some(idx.clone());
                    }
                });
            }
        });
        if !open || out.is_some() {
            self.open = None;
        }
        out
    }
}

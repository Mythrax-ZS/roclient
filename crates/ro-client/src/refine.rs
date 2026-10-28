//! The refine window (opened by refiner NPCs or @refineui): pick a piece of equipment from
//! the inventory, then a material; the server rolls the result.

use std::collections::HashMap;

use egui::{Color32, RichText, Vec2};
use ro_game::items::{InvItem, ItemDb};
use ro_net::{RefineOption, RefineRequest};

#[derive(Default)]
pub struct Refine {
    pub open: bool,
    /// The selected item (inventory index), its Blacksmith Blessing cost and options.
    pub selected: Option<(u16, u8, Vec<RefineOption>)>,
    use_blessing: bool,
}

/// Blacksmith Blessing.
const BLESSING: u32 = 6635;

impl Refine {
    pub fn show(&mut self, ctx: &egui::Context, items: &[&InvItem], db: Option<&ItemDb>, icons: &HashMap<u32, egui::TextureHandle>, zeny: i64) -> Option<RefineRequest> {
        if !self.open {
            return None;
        }
        let mut out = None;
        let mut open = true;
        let name = |id: u32| db.map_or_else(|| format!("Item #{id}"), |d| d.name(id, true));
        let have = |id: u32| items.iter().filter(|i| i.id == id).map(|i| i.amount as u32).sum::<u32>();
        let icon = |ui: &mut egui::Ui, id: u32| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(26.0), egui::Sense::hover());
            if let Some(t) = icons.get(&id) {
                ui.painter().image(t.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            }
        };
        egui::Window::new("Refine").open(&mut open).collapsible(false).default_width(520.0).default_pos(egui::pos2(380.0, 120.0)).show(ctx, |ui| {
            ui.columns(2, |cols| {
                // Equipment that can be refined (not worn, identified).
                let ui = &mut cols[0];
                ui.label(RichText::new("Equipment").strong());
                egui::ScrollArea::vertical().id_salt("refine_items").max_height(300.0).show(ui, |ui| {
                    for it in items.iter().filter(|i| i.is_equipment() && i.equipped == 0 && i.identified) {
                        let label = if it.refine > 0 { format!("+{} {}", it.refine, name(it.id)) } else { name(it.id) };
                        ui.horizontal(|ui| {
                            icon(ui, it.id);
                            let on = self.selected.as_ref().is_some_and(|s| s.0 == it.index);
                            if ui.selectable_label(on, label).clicked() {
                                self.selected = Some((it.index, 0, vec![]));
                                out = Some(RefineRequest::Select(it.index));
                            }
                        });
                    }
                });
                let ui = &mut cols[1];
                let Some((index, blessing, options)) = &self.selected else {
                    ui.label(RichText::new("Choose a piece of equipment.").color(Color32::GRAY));
                    return;
                };
                let Some(item) = items.iter().find(|i| i.index == *index) else {
                    ui.label(RichText::new("That item is gone.").color(Color32::LIGHT_RED));
                    return;
                };
                ui.label(RichText::new(format!("+{} {}", item.refine, name(item.id))).strong().size(15.0));
                if options.is_empty() {
                    ui.label(RichText::new("This item can't be refined further.").color(Color32::GRAY));
                    return;
                }
                if *blessing > 0 {
                    let owned = have(BLESSING);
                    ui.add_enabled(owned >= *blessing as u32, egui::Checkbox::new(&mut self.use_blessing, format!("Use {blessing} Blacksmith Blessing (have {owned}) — no break or downgrade")));
                }
                ui.separator();
                for o in options {
                    let owned = have(o.material);
                    ui.horizontal(|ui| {
                        icon(ui, o.material);
                        ui.vertical(|ui| {
                            ui.label(format!("{}  (have {owned})", name(o.material)));
                            ui.label(RichText::new(format!("{}% success   {} z", o.chance, o.zeny)).color(if o.chance >= 50 { Color32::LIGHT_GREEN } else { Color32::from_rgb(255, 170, 90) }));
                        });
                        let ok = owned > 0 && zeny >= o.zeny as i64;
                        if ui.add_enabled(ok, egui::Button::new("Refine")).clicked() {
                            out = Some(RefineRequest::Refine { index: *index, material: o.material, blessing: self.use_blessing && *blessing > 0 });
                        }
                    });
                }
            });
        });
        if !open {
            self.open = false;
            self.selected = None;
            out = Some(RefineRequest::Close);
        }
        out
    }
}

//! The item information window (right-click an item → Info, as in the original): the
//! item's collection picture and description, or for cards their full illustration
//! (data\num2cardillustnametable.txt → texture\유저인터페이스\cardbmp\).

use crate::uitex::UiSize;
use std::collections::HashMap;

use egui::{Color32, RichText, Vec2};
use ro_game::items::ItemDb;

#[derive(Default)]
pub struct ItemInfo {
    /// Card item id -> illustration file name (no extension).
    card_art: HashMap<u32, String>,
    /// The item shown: (id, refine, cards, picture).
    open: Option<(u32, u8, [u32; 4], Option<egui::TextureHandle>)>,
    /// Another player's equipment being viewed: (name, job title, items).
    pub viewing: Option<(String, String, Vec<ro_game::items::InvItem>)>,
}

impl ItemInfo {
    pub fn load(vfs: &ro_vfs::Vfs) -> Self {
        let mut s = Self::default();
        if let Some(d) = vfs.read(r"data\num2cardillustnametable.txt").ok().flatten() {
            let (text, _, _) = encoding_rs::EUC_KR.decode(&d);
            for line in text.lines() {
                let mut f = line.split('#');
                if let (Some(Ok(id)), Some(name)) = (f.next().map(|i| i.trim().parse::<u32>()), f.next()) {
                    if !name.is_empty() {
                        s.card_art.insert(id, name.to_string());
                    }
                }
            }
        }
        tracing::info!(cards = s.card_art.len(), "card illustrations");
        s
    }


    /// Opens the window for an item, loading its picture.
    pub fn show_item(&mut self, ctx: &egui::Context, vfs: &ro_vfs::Vfs, db: Option<&ItemDb>, id: u32, refine: u8, cards: [u32; 4]) {
        let ui_dir = r"data\texture\유저인터페이스";
        let pic = match self.card_art.get(&id) {
            Some(art) => crate::load_texture(ctx, vfs, &format!(r"{ui_dir}\cardbmp\{art}.bmp")),
            None => db
                .and_then(|d| d.get(id))
                .and_then(|i| crate::load_texture(ctx, vfs, &format!(r"{ui_dir}\collection\{}.bmp", i.resource))),
        };
        self.open = Some((id, refine, cards, pic));
    }

    pub fn show(&mut self, ctx: &egui::Context, db: Option<&ItemDb>) {
        let Some((id, refine, cards, pic)) = &self.open else { return };
        let name = db.map_or_else(|| format!("Item #{id}"), |d| d.name(*id, true));
        let name = if *refine > 0 { format!("+{refine} {name}") } else { name };
        let card = self.card_art.contains_key(id);
        let mut open = true;
        crate::classic::show(ctx, egui::Window::new(RichText::new(&name).strong()).id(egui::Id::unique("iteminfo")).open(&mut open).collapsible(false).resizable(false), &name, true, |ui| {
            if card {
                // Cards: the illustration alone, large, as the original's card window.
                if let Some(t) = pic {
                    ui.image((t.id(), t.ui_size() * 1.25));
                }
                return;
            }
            ui.horizontal_top(|ui| {
                if let Some(t) = pic {
                    ui.image((t.id(), t.ui_size().min(Vec2::splat(150.0))));
                }
                ui.vertical(|ui| {
                    ui.set_max_width(320.0);
                    if let Some(info) = db.and_then(|d| d.get(*id)) {
                        for line in &info.description {
                            ui.label(crate::hud::npc_text(line, 12.5));
                        }
                    }
                    let inserted: Vec<u32> = cards.iter().copied().filter(|&c| c != 0 && !matches!(cards[0], 0x00ff | 0x00fe | 0xff00)).collect();
                    if !inserted.is_empty() {
                        ui.separator();
                        for c in inserted {
                            ui.label(RichText::new(db.map_or_else(|| format!("Card #{c}"), |d| d.name(c, true))).color(crate::classic::ink(Color32::from_rgb(140, 200, 255))));
                        }
                    }
                });
            });
        });
        if !open {
            self.open = None;
        }
    }
}

impl ItemInfo {
    /// Another player's equipment. Returns an item to show in the info window.
    pub fn show_viewing(&mut self, ctx: &egui::Context, db: Option<&ItemDb>, icons: &HashMap<u32, egui::TextureHandle>) -> Option<(u32, u8, [u32; 4])> {
        let (name, job, items) = self.viewing.as_ref()?;
        let mut info = None;
        let mut open = true;
        crate::classic::show(ctx, egui::Window::new(format!("{name}'s equipment")).id(egui::Id::unique("view_equip")).open(&mut open).collapsible(false).default_width(320.0), &format!("{name}'s equipment"), true, |ui| {
            ui.label(RichText::new(job).color(crate::classic::ink(Color32::GRAY)));
            ui.separator();
            if items.is_empty() {
                ui.label(RichText::new("Nothing worn.").color(crate::classic::ink(Color32::GRAY)));
            }
            for it in items {
                let resp = ui
                    .horizontal(|ui| {
                        let (rect, resp) = ui.allocate_exact_size(Vec2::splat(28.0), egui::Sense::click());
                        if let Some(t) = icons.get(&it.id) {
                            ui.painter().image(t.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                        }
                        let base = db.map_or_else(|| format!("Item #{}", it.id), |d| d.name(it.id, true));
                        let label = if it.refine > 0 { format!("+{} {base}", it.refine) } else { base };
                        resp | ui.add(egui::Label::new(label).sense(egui::Sense::click()))
                    })
                    .inner;
                if resp.on_hover_text("Click for details").clicked() {
                    info = Some((it.id, it.refine, it.cards));
                }
            }
        });
        if !open {
            self.viewing = None;
        }
        info
    }
}

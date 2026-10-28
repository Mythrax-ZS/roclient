//! Store search (the Universal Catalog item, or `searchstores` in a script): find vending
//! shops (or buying stores) on the map selling an item, by name; click a result to have
//! the server mark the shop on the minimap.

use std::collections::HashMap;

use egui::{Color32, RichText, Vec2};
use ro_game::items::ItemDb;
use ro_net::{Found, SearchRequest};

#[derive(Default)]
pub struct StoreSearch {
    pub open: bool,
    uses: u8,
    query: String,
    buying: bool,
    min: String,
    max: String,
    pub results: Vec<Found>,
    more: bool,
    pub status: String,
}

impl StoreSearch {
    pub fn opened(&mut self, uses: u8) {
        self.open = true;
        self.uses = uses;
        self.results.clear();
        self.status.clear();
    }

    pub fn results(&mut self, first: bool, more: bool, uses: u8, found: Vec<Found>) {
        if first {
            self.results.clear();
        }
        self.results.extend(found);
        self.more = more;
        self.uses = uses;
        self.status = format!("{} found.", self.results.len());
    }

    pub fn failed(&mut self, reason: u8) {
        self.status = match reason {
            0 => "No matching stores were found.",
            1 => "There are too many results; search for something more specific.",
            2 => "You cannot search anymore.",
            3 => "You cannot search yet.",
            _ => "No items were found.",
        }
        .into();
    }

    /// Item ids whose name contains the query.
    fn matches(&self, db: Option<&ItemDb>) -> Vec<u32> {
        let q = self.query.trim().to_lowercase();
        if q.len() < 2 {
            return vec![];
        }
        db.map(|d| d.search(&q)).unwrap_or_default().into_iter().take(10).collect()
    }

    /// Test hook: search for an item id.
    pub fn test_search(&self, item: u32) -> SearchRequest {
        SearchRequest::Search { kind: 0, min: 0, max: 0, items: vec![item], cards: vec![] }
    }

    pub fn show(&mut self, ctx: &egui::Context, db: Option<&ItemDb>, icons: &HashMap<u32, egui::TextureHandle>) -> Vec<SearchRequest> {
        let mut out = vec![];
        if !self.open {
            return out;
        }
        let name = |id: u32| db.map_or_else(|| format!("Item #{id}"), |d| d.name(id, true));
        let mut open = true;
        crate::classic::show(ctx, egui::Window::new("Store Search").open(&mut open).resizable(false).default_width(460.0), "Store Search", true, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.buying, false, "Selling");
                ui.selectable_value(&mut self.buying, true, "Buying");
                ui.label(RichText::new(format!("{} searches left", self.uses)).size(11.0).color(crate::classic::ink(Color32::GRAY)));
            });
            ui.horizontal(|ui| {
                ui.label("Item");
                ui.add(egui::TextEdit::singleline(&mut self.query).desired_width(200.0));
                ui.label("Price");
                ui.add(egui::TextEdit::singleline(&mut self.min).hint_text("min").desired_width(60.0));
                ui.add(egui::TextEdit::singleline(&mut self.max).hint_text("max").desired_width(60.0));
            });
            let ids = self.matches(db);
            if !ids.is_empty() {
                ui.label(RichText::new(format!("Matches: {}", ids.iter().map(|i| name(*i)).collect::<Vec<_>>().join(", "))).size(11.0).color(crate::classic::ink(Color32::GRAY)));
            }
            if ui.add_enabled(!ids.is_empty() && self.uses > 0, egui::Button::new("Search")).clicked() {
                let p = |s: &str| s.trim().parse().unwrap_or(0);
                out.push(SearchRequest::Search { kind: self.buying as u8, min: p(&self.min), max: p(&self.max), items: ids, cards: vec![] });
            }
            ui.separator();
            egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                for f in &self.results {
                    ui.horizontal(|ui| {
                        if let Some(t) = icons.get(&f.item) {
                            ui.image((t.id(), Vec2::splat(22.0)));
                        }
                        let refine = if f.refine > 0 { format!("+{} ", f.refine) } else { String::new() };
                        let r = ui.selectable_label(false, format!("{refine}{} x{} — {} z", name(f.item), f.amount, f.price));
                        if r.on_hover_text(format!("Shop: {}", f.shop)).clicked() {
                            out.push(SearchRequest::Click { owner: f.owner, store: f.store, item: f.item });
                        }
                    });
                }
            });
            if self.more && ui.button("More results").clicked() {
                out.push(SearchRequest::NextPage);
            }
            if !self.status.is_empty() {
                ui.label(RichText::new(&self.status).color(crate::classic::ink(Color32::from_rgb(255, 210, 110))));
            }
        });
        if !open {
            self.open = false;
            out.push(SearchRequest::Close);
        }
        out
    }
}

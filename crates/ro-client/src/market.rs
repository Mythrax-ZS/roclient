//! NPC market shops (fixed stock, bought with zeny) and barter shops (items for items,
//! sometimes plus zeny). Pick an amount per row and buy; the server checks the cost.

use std::collections::HashMap;

use egui::{Color32, RichText, Vec2};
use ro_game::items::ItemDb;
use ro_net::{BarterItem, MarketItem, MarketRequest};

pub enum Shop {
    Market(Vec<MarketItem>),
    Barter { items: Vec<BarterItem>, expanded: bool },
}

#[derive(Default)]
pub struct Market {
    pub shop: Option<Shop>,
    /// Amount typed per row.
    amounts: HashMap<usize, u32>,
    pub status: String,
}

impl Market {
    pub fn open(&mut self, shop: Shop) {
        self.shop = Some(shop);
        self.amounts.clear();
        self.status.clear();
    }

    /// `have`: how many of an item we carry.
    pub fn show(&mut self, ctx: &egui::Context, db: Option<&ItemDb>, icons: &HashMap<u32, egui::TextureHandle>, zeny: i64, have: &dyn Fn(u32) -> u32) -> Vec<MarketRequest> {
        let mut out = vec![];
        let Some(shop) = &self.shop else { return out };
        let name = |id: u32| db.map_or_else(|| format!("Item #{id}"), |d| d.name(id, true));
        let icon = |ui: &mut egui::Ui, id: u32| {
            match icons.get(&id) {
                Some(t) => ui.image((t.id(), Vec2::splat(24.0))),
                None => ui.allocate_exact_size(Vec2::splat(24.0), egui::Sense::hover()).1,
            };
        };
        let title = if matches!(shop, Shop::Market(_)) { "Market" } else { "Trade" };
        let mut open = true;
        crate::classic::show(ctx, egui::Window::new(title).id(egui::Id::unique("npc_market")).open(&mut open).resizable(false).default_width(420.0), title, true, |ui| {
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| match shop {
                Shop::Market(items) => {
                    for (i, it) in items.iter().enumerate() {
                        ui.horizontal(|ui| {
                            icon(ui, it.item);
                            ui.vertical(|ui| {
                                ui.label(RichText::new(name(it.item)).strong());
                                ui.label(RichText::new(format!("{} z   stock {}", it.price, it.stock)).size(11.0).color(crate::classic::ink(Color32::GRAY)));
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let n = self.amounts.entry(i).or_insert(1);
                                let ok = *n > 0 && *n <= it.stock && (it.price as i64 * *n as i64) <= zeny;
                                if ui.add_enabled(ok, egui::Button::new("Buy")).clicked() {
                                    out.push(MarketRequest::MarketBuy(vec![(it.item, *n)]));
                                }
                                ui.add(egui::DragValue::new(n).range(1..=it.stock.max(1)));
                            });
                        });
                        ui.separator();
                    }
                }
                Shop::Barter { items, expanded } => {
                    for (i, it) in items.iter().enumerate() {
                        ui.horizontal(|ui| {
                            icon(ui, it.item);
                            ui.vertical(|ui| {
                                let refine = if it.refine > 0 { format!("+{} ", it.refine) } else { String::new() };
                                ui.label(RichText::new(format!("{refine}{}", name(it.item))).strong());
                                for (cost, amount, _) in &it.costs {
                                    let enough = have(*cost) >= *amount;
                                    let color = if enough { crate::classic::ink(Color32::GRAY) } else { Color32::from_rgb(230, 110, 100) };
                                    ui.label(RichText::new(format!("{} x{amount}  (have {})", name(*cost), have(*cost))).size(11.0).color(color));
                                }
                                if it.zeny > 0 {
                                    ui.label(RichText::new(format!("{} z", it.zeny)).size(11.0).color(crate::classic::ink(Color32::GRAY)));
                                }
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let n = self.amounts.entry(i).or_insert(1);
                                let ok = *n > 0 && it.costs.iter().all(|(c, a, _)| have(*c) >= a * *n) && (it.zeny as i64 * *n as i64) <= zeny;
                                if ui.add_enabled(ok, egui::Button::new("Trade")).clicked() {
                                    let list = vec![(it.item, *n, it.index)];
                                    out.push(if *expanded { MarketRequest::ExpandedBuy(list) } else { MarketRequest::BarterBuy(list) });
                                }
                                ui.add(egui::DragValue::new(n).range(1..=999));
                            });
                        });
                        ui.separator();
                    }
                }
            });
            if !self.status.is_empty() {
                ui.label(RichText::new(&self.status).color(crate::classic::ink(Color32::from_rgb(255, 210, 110))));
            }
        });
        if !open {
            out.push(match shop {
                Shop::Market(_) => MarketRequest::MarketClose,
                Shop::Barter { expanded: true, .. } => MarketRequest::ExpandedClose,
                Shop::Barter { .. } => MarketRequest::BarterClose,
            });
            self.shop = None;
        }
        out
    }
}

impl Market {
    /// Test hook: buy `n` of row `row`.
    pub fn test_buy(&self, row: usize, n: u32) -> Option<MarketRequest> {
        Some(match self.shop.as_ref()? {
            Shop::Market(items) => MarketRequest::MarketBuy(vec![(items.get(row)?.item, n)]),
            Shop::Barter { items, expanded } => {
                let it = items.get(row)?;
                let list = vec![(it.item, n, it.index)];
                if *expanded { MarketRequest::ExpandedBuy(list) } else { MarketRequest::BarterBuy(list) }
            }
        })
    }
}

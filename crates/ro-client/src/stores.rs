//! Buying stores (a player posts what they want to buy; others sell to them) and the cash
//! shop (items for cash / Kafra points, by tab).

use std::collections::{BTreeMap, HashMap};

use egui::{Color32, RichText, Vec2};
use ro_game::items::{InvItem, ItemDb};
use ro_net::{BuyingStoreRequest, CashShopRequest, WantedItem};

#[derive(Default)]
pub struct Stores {
    /// Buying store titles over players: account id -> title.
    pub signs: HashMap<u32, String>,
    /// Setting up our buying store: (slots, title, zeny limit typed, item id -> (amount, price)).
    pub setup: Option<(u8, String, String, BTreeMap<u32, (u16, u32)>)>,
    /// Our open buying store: (zeny limit left, items).
    pub mine: Option<(u32, Vec<WantedItem>)>,
    /// A buying store being sold to: (buyer, store id, zeny limit, items, amounts to sell).
    pub browsing: Option<(u32, u32, u32, Vec<WantedItem>, Vec<u16>)>,
    /// The cash shop while open: (cash points, kafra points).
    pub cash: Option<(u32, u32)>,
    /// Cash shop tabs as the server listed them (sent once per login).
    pub cash_tabs: BTreeMap<u16, Vec<(u32, u32)>>,
    cash_tab: u16,
    /// Cash shop basket: (item id, tab) -> amount.
    basket: BTreeMap<(u32, u16), u32>,
    use_kafra: u32,
}

const CASH_TABS: [&str; 9] = ["New", "Hot", "Limited", "Rental", "Permanent", "Scrolls", "Consumables", "Other", "Sale"];

fn icon(ui: &mut egui::Ui, icons: &HashMap<u32, egui::TextureHandle>, id: u32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(26.0), egui::Sense::hover());
    if let Some(t) = icons.get(&id) {
        ui.painter().image(t.id(), egui::Rect::from_center_size(rect.center(), t.size_vec2().min(Vec2::splat(24.0))), egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
    }
}

fn name(db: Option<&ItemDb>, id: u32) -> String {
    db.map_or_else(|| format!("Item #{id}"), |d| d.name(id, true))
}

fn gold(text: String) -> RichText {
    RichText::new(text).color(Color32::from_rgb(255, 220, 120))
}

impl Stores {
    /// Item ids whose icons the windows need.
    pub fn item_ids(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self.cash_tabs.values().flatten().map(|(id, _)| *id).collect();
        if let Some((_, _, _, items, _)) = &self.browsing {
            ids.extend(items.iter().map(|i| i.id));
        }
        if let Some((_, items)) = &self.mine {
            ids.extend(items.iter().map(|i| i.id));
        }
        ids
    }

    /// The buying store windows: set-up, our open store, and someone else's.
    pub fn show_buying(&mut self, ctx: &egui::Context, inventory: &[&InvItem], db: Option<&ItemDb>, icons: &HashMap<u32, egui::TextureHandle>, zeny: i64) -> Option<BuyingStoreRequest> {
        let mut out = None;
        if let Some((slots, title, limit, chosen)) = self.setup.as_mut() {
            let mut open = true;
            let mut submit = false;
            egui::Window::new("Open Buying Store").open(&mut open).collapsible(false).default_width(440.0).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Store name");
                    ui.text_edit_singleline(title);
                });
                ui.horizontal(|ui| {
                    ui.label("Zeny limit");
                    ui.add(egui::TextEdit::singleline(limit).desired_width(110.0));
                    ui.label(RichText::new(format!("(you have {zeny} z)")).color(Color32::GRAY));
                });
                ui.label(RichText::new(format!("Up to {slots} kinds of item. You must already carry one of each.")).color(Color32::GRAY));
                ui.separator();
                let mut seen = std::collections::HashSet::new();
                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    for it in inventory.iter().filter(|i| !i.is_equipment() && i.identified && seen.insert(i.id)) {
                        ui.horizontal(|ui| {
                            let mut on = chosen.contains_key(&it.id);
                            icon(ui, icons, it.id);
                            if ui.checkbox(&mut on, name(db, it.id)).changed() {
                                if on && chosen.len() < *slots as usize {
                                    chosen.insert(it.id, (10, 100));
                                } else {
                                    chosen.remove(&it.id);
                                }
                            }
                            if let Some((amount, price)) = chosen.get_mut(&it.id) {
                                ui.add(egui::DragValue::new(amount).range(1..=9999).prefix("x"));
                                ui.add(egui::DragValue::new(price).range(1..=1_000_000_000).suffix(" z").speed(10));
                            }
                        });
                    }
                });
                ui.separator();
                let cost: u64 = chosen.values().map(|(a, p)| *a as u64 * *p as u64).sum();
                let lim = limit.trim().replace(',', "").parse::<u32>().ok().filter(|l| *l > 0 && *l as i64 <= zeny);
                ui.label(format!("Buying everything would cost {cost} z."));
                ui.add_enabled_ui(!chosen.is_empty() && lim.is_some() && !title.trim().is_empty(), |ui| {
                    submit = ui.button("Open buying store").clicked();
                });
            });
            if submit {
                let limit = limit.trim().replace(',', "").parse().unwrap_or(0);
                out = Some(BuyingStoreRequest::Open { limit, title: title.trim().to_string(), items: chosen.iter().map(|(id, (a, p))| (*id, *a, *p)).collect() });
                self.setup = None;
            } else if !open {
                self.setup = None;
            }
        }

        if let Some((limit, items)) = &self.mine {
            let mut close = false;
            egui::Window::new("My Buying Store").collapsible(false).default_width(300.0).show(ctx, |ui| {
                ui.label(gold(format!("Zeny limit left: {limit} z")));
                ui.separator();
                for it in items {
                    ui.horizontal(|ui| {
                        icon(ui, icons, it.id);
                        ui.label(format!("{}  want {}  at {} z", name(db, it.id), it.amount, it.price));
                    });
                }
                ui.separator();
                close = ui.button("Close buying store").clicked();
            });
            if close {
                out = Some(BuyingStoreRequest::Close);
                self.mine = None;
            }
        }

        if let Some((buyer, store, limit, items, amounts)) = self.browsing.as_mut() {
            let mut open = true;
            let mut sell = false;
            let have = |id: u32| inventory.iter().filter(|i| i.id == id && i.equipped == 0).map(|i| i.amount).sum::<u16>();
            let title = self.signs.get(buyer).cloned().unwrap_or_else(|| "Buying Store".into());
            egui::Window::new(title).id(egui::Id::unique("buying_store_browse")).open(&mut open).collapsible(false).default_width(440.0).show(ctx, |ui| {
                ui.label(gold(format!("This buyer can spend up to {limit} z.")));
                ui.separator();
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    for (it, n) in items.iter().zip(amounts.iter_mut()) {
                        let owned = have(it.id);
                        ui.horizontal(|ui| {
                            icon(ui, icons, it.id);
                            ui.label(format!("{}  (wants {}, you have {owned})", name(db, it.id), it.amount));
                            ui.label(gold(format!("{} z", it.price)));
                            ui.add_enabled(owned > 0 && it.amount > 0, egui::DragValue::new(n).range(0..=owned.min(it.amount)).prefix("sell "));
                        });
                    }
                });
                let total: u64 = items.iter().zip(amounts.iter()).map(|(i, a)| i.price as u64 * *a as u64).sum();
                ui.separator();
                ui.label(format!("You get: {total} z"));
                ui.add_enabled_ui(total > 0 && total <= *limit as u64, |ui| {
                    sell = ui.button("Sell").clicked();
                });
            });
            if sell {
                // Each wanted item comes from the first unworn stack that has it.
                let list = items
                    .iter()
                    .zip(amounts.iter())
                    .filter(|(_, a)| **a > 0)
                    .filter_map(|(w, a)| inventory.iter().find(|i| i.id == w.id && i.equipped == 0).map(|i| (i.index, w.id, (*a).min(i.amount))))
                    .collect();
                out = Some(BuyingStoreRequest::Sell { buyer: *buyer, store: *store, items: list });
                self.browsing = None;
            } else if !open {
                self.browsing = None;
            }
        }
        out
    }

    /// Buying store signs over players; clicking one opens the store.
    pub fn show_signs(&self, ctx: &egui::Context, signs: &[(u32, glam::Vec2)]) -> Option<u32> {
        let ppp = ctx.pixels_per_point();
        let mut clicked = None;
        for (aid, p) in signs {
            let Some(title) = self.signs.get(aid) else { continue };
            egui::Area::new(egui::Id::unique(("buy_sign", *aid))).fixed_pos(egui::pos2(p.x / ppp, p.y / ppp)).pivot(egui::Align2::CENTER_BOTTOM).show(ctx, |ui| {
                let b = egui::Button::new(RichText::new(format!("Buying: {title}")).color(Color32::BLACK).size(12.0)).fill(Color32::from_rgb(200, 230, 255));
                if ui.add(b).on_hover_text("Click to sell to this store").clicked() {
                    clicked = Some(*aid);
                }
            });
        }
        clicked
    }

    /// The cash shop window.
    pub fn show_cash(&mut self, ctx: &egui::Context, db: Option<&ItemDb>, icons: &HashMap<u32, egui::TextureHandle>) -> Option<CashShopRequest> {
        let (cash, kafra) = self.cash?;
        let mut out = None;
        let mut open = true;
        if !self.cash_tabs.contains_key(&self.cash_tab) {
            if let Some(first) = self.cash_tabs.keys().next() {
                self.cash_tab = *first;
            }
        }
        egui::Window::new("Cash Shop").open(&mut open).collapsible(false).default_width(560.0).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(gold(format!("Cash points: {cash}")));
                ui.separator();
                ui.label(gold(format!("Kafra points: {kafra}")));
            });
            ui.horizontal_wrapped(|ui| {
                for tab in self.cash_tabs.keys() {
                    let label = CASH_TABS.get(*tab as usize).copied().unwrap_or("Tab");
                    if ui.selectable_label(self.cash_tab == *tab, label).clicked() {
                        self.cash_tab = *tab;
                    }
                }
            });
            ui.separator();
            if self.cash_tabs.is_empty() {
                ui.label(RichText::new("The shop has nothing for sale.").color(Color32::GRAY));
                return;
            }
            ui.columns(2, |cols| {
                let ui = &mut cols[0];
                egui::ScrollArea::vertical().id_salt("cash_items").max_height(320.0).show(ui, |ui| {
                    for (id, price) in self.cash_tabs.get(&self.cash_tab).into_iter().flatten() {
                        ui.horizontal(|ui| {
                            icon(ui, icons, *id);
                            ui.vertical(|ui| {
                                ui.label(name(db, *id));
                                ui.label(gold(format!("{price} points")));
                            });
                            if ui.small_button("Add").clicked() {
                                *self.basket.entry((*id, self.cash_tab)).or_insert(0) += 1;
                            }
                        });
                    }
                });
                let ui = &mut cols[1];
                ui.label(RichText::new("Basket").strong());
                let price_of = |id: u32, tab: u16| self.cash_tabs.get(&tab).and_then(|t| t.iter().find(|(i, _)| *i == id)).map_or(0, |(_, p)| *p);
                let mut remove = None;
                egui::ScrollArea::vertical().id_salt("cash_basket").max_height(240.0).show(ui, |ui| {
                    for ((id, tab), n) in self.basket.iter_mut() {
                        ui.horizontal(|ui| {
                            icon(ui, icons, *id);
                            ui.label(name(db, *id));
                            ui.add(egui::DragValue::new(n).range(1..=99).prefix("x"));
                            if ui.small_button("✖").clicked() {
                                remove = Some((*id, *tab));
                            }
                        });
                    }
                });
                if let Some(k) = remove {
                    self.basket.remove(&k);
                }
                let total: u64 = self.basket.iter().map(|((id, tab), n)| price_of(*id, *tab) as u64 * *n as u64).sum();
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Pay with Kafra points");
                    ui.add(egui::DragValue::new(&mut self.use_kafra).range(0..=kafra.min(total as u32)));
                });
                ui.label(format!("Total: {total} points"));
                let ok = total > 0 && total <= cash as u64 + self.use_kafra as u64;
                if ui.add_enabled(ok, egui::Button::new("Buy")).clicked() {
                    out = Some(CashShopRequest::Buy { kafra: self.use_kafra, items: self.basket.iter().map(|((id, tab), n)| (*id, *n, *tab)).collect() });
                    self.basket.clear();
                    self.use_kafra = 0;
                }
            });
        });
        if !open {
            self.cash = None;
            out = Some(CashShopRequest::Close);
        }
        out
    }
}

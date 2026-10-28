//! The pushcart window (Alt+W), setting up a shop after casting Vending, browsing other
//! players' shops, and the shop signs over their heads.

use std::collections::{BTreeMap, HashMap};

use egui::{Color32, RichText, Vec2};
use ro_game::items::{InvItem, ItemDb};
use ro_net::{MerchantRequest, VendItem};

#[derive(Default)]
pub struct Merchant {
    pub cart: BTreeMap<u16, InvItem>,
    /// (items, max items, weight, max weight); max items is 0 without a cart.
    pub cart_count: (u16, u16, u32, u32),
    pub cart_open: bool,
    /// Shop titles over players: account id -> title.
    pub signs: HashMap<u32, String>,
    /// A shop being browsed: (seller, shop id, items, amounts to buy).
    pub browsing: Option<(u32, u32, Vec<VendItem>, Vec<u16>)>,
    /// Setting up our shop after Vending: (slots, title, cart index -> (amount, price)).
    pub setup: Option<(u16, String, BTreeMap<u16, (u16, u32)>)>,
    /// Our open shop's stock.
    pub my_shop: Option<Vec<VendItem>>,
    /// The bank window: (balance once known, amount typed).
    pub bank: Option<(Option<i64>, String)>,
}

fn name(db: Option<&ItemDb>, id: u32, refine: u8) -> String {
    let base = db.map_or_else(|| format!("Item #{id}"), |d| d.name(id, true));
    if refine > 0 { format!("+{refine} {base}") } else { base }
}

fn icon(ui: &mut egui::Ui, icons: &HashMap<u32, egui::TextureHandle>, id: u32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(26.0), egui::Sense::hover());
    if let Some(t) = icons.get(&id) {
        ui.painter().image(t.id(), egui::Rect::from_center_size(rect.center(), t.size_vec2().min(Vec2::splat(24.0))), egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
    }
}

impl Merchant {
    pub fn has_cart(&self) -> bool {
        self.cart_count.1 > 0
    }

    /// Item ids whose icons the windows need.
    pub fn item_ids(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self.cart.values().map(|i| i.id).collect();
        if let Some((_, _, items, _)) = &self.browsing {
            ids.extend(items.iter().map(|i| i.id));
        }
        ids
    }

    pub fn show(&mut self, ctx: &egui::Context, db: Option<&ItemDb>, icons: &HashMap<u32, egui::TextureHandle>, zeny: i64) -> Option<MerchantRequest> {
        let mut out = None;
        if self.cart_open && self.has_cart() {
            let mut open = true;
            let (n, max, w, max_w) = self.cart_count;
            egui::Window::new("Cart").open(&mut open).default_width(300.0).show(ctx, |ui| {
                ui.label(RichText::new(format!("{n} / {max} items    Weight {} / {}", w / 10, max_w / 10)).color(Color32::GRAY));
                ui.separator();
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    for it in self.cart.values() {
                        let resp = ui
                            .horizontal(|ui| {
                                icon(ui, icons, it.id);
                                ui.add(egui::Label::new(format!("{}  x{}", name(db, it.id, it.refine), it.amount)).sense(egui::Sense::click()))
                            })
                            .inner;
                        if resp.double_clicked() {
                            out = Some(MerchantRequest::CartGet { index: it.index, amount: it.amount as u32 });
                        }
                        resp.on_hover_text("Double-click to move to your inventory").context_menu(|ui| {
                            if ui.button("Move to inventory").clicked() {
                                out = Some(MerchantRequest::CartGet { index: it.index, amount: it.amount as u32 });
                                ui.close();
                            }
                        });
                    }
                });
                ui.separator();
                if ui.button("Remove cart").clicked() {
                    out = Some(MerchantRequest::CartOff);
                }
            });
            self.cart_open = open;
        }

        // Vending: pick cart items, amounts and prices, then open the shop.
        if let Some((slots, title, chosen)) = self.setup.as_mut() {
            let mut open = true;
            let mut submit = false;
            egui::Window::new("Open Shop").open(&mut open).collapsible(false).default_width(420.0).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Shop name");
                    ui.text_edit_singleline(title);
                });
                ui.label(RichText::new(format!("Up to {slots} items, from your cart.")).color(Color32::GRAY));
                ui.separator();
                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    for it in self.cart.values() {
                        ui.horizontal(|ui| {
                            let mut on = chosen.contains_key(&it.index);
                            icon(ui, icons, it.id);
                            if ui.checkbox(&mut on, name(db, it.id, it.refine)).changed() {
                                if on && chosen.len() < *slots as usize {
                                    chosen.insert(it.index, (it.amount, 1000));
                                } else {
                                    chosen.remove(&it.index);
                                }
                            }
                            if let Some((amount, price)) = chosen.get_mut(&it.index) {
                                ui.add(egui::DragValue::new(amount).range(1..=it.amount).prefix("x"));
                                ui.add(egui::DragValue::new(price).range(1..=1_000_000_000).suffix(" z").speed(100));
                            }
                        });
                    }
                });
                ui.separator();
                ui.add_enabled_ui(!chosen.is_empty() && !title.trim().is_empty(), |ui| {
                    submit = ui.button("Open shop").clicked();
                });
            });
            if submit {
                out = Some(MerchantRequest::OpenShop { title: title.trim().to_string(), items: chosen.iter().map(|(i, (a, p))| (*i, *a, *p)).collect() });
                self.setup = None;
            } else if !open {
                self.setup = None;
            }
        }

        if let Some(stock) = &self.my_shop {
            let mut close = false;
            egui::Window::new("My Shop").collapsible(false).default_width(300.0).show(ctx, |ui| {
                for it in stock {
                    ui.horizontal(|ui| {
                        icon(ui, icons, it.id);
                        ui.label(format!("{}  x{}  {} z", name(db, it.id, it.refine), it.amount, it.price));
                    });
                }
                ui.separator();
                close = ui.button("Close shop").clicked();
            });
            if close {
                out = Some(MerchantRequest::CloseShop);
                self.my_shop = None;
            }
        }

        // Someone else's shop: choose amounts, see the total, buy.
        if let Some((seller, shop, items, amounts)) = self.browsing.as_mut() {
            let mut open = true;
            let mut buy = false;
            let title = self.signs.get(seller).cloned().unwrap_or_else(|| "Shop".into());
            egui::Window::new(title).open(&mut open).collapsible(false).default_width(420.0).show(ctx, |ui| {
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    for (it, want) in items.iter().zip(amounts.iter_mut()) {
                        ui.horizontal(|ui| {
                            icon(ui, icons, it.id);
                            ui.label(format!("{}  ({} left)", name(db, it.id, it.refine), it.amount));
                            ui.label(RichText::new(format!("{} z", it.price)).color(Color32::from_rgb(255, 220, 120)));
                            ui.add(egui::DragValue::new(want).range(0..=it.amount).prefix("buy "));
                        });
                    }
                });
                let total: u64 = items.iter().zip(amounts.iter()).map(|(i, a)| i.price as u64 * *a as u64).sum();
                ui.separator();
                ui.label(format!("Total: {total} z   (you have {zeny} z)"));
                ui.add_enabled_ui(total > 0 && total <= zeny.max(0) as u64, |ui| {
                    buy = ui.button("Buy").clicked();
                });
            });
            if buy {
                let list = items.iter().zip(amounts.iter()).filter(|(_, a)| **a > 0).map(|(i, a)| (*a, i.index)).collect();
                out = Some(MerchantRequest::Buy { seller: *seller, shop: *shop, items: list });
                self.browsing = None;
            } else if !open {
                self.browsing = None;
            }
        }
        out
    }

    /// Shop signs over players; clicking one opens the shop.
    pub fn show_signs(&self, ctx: &egui::Context, signs: &[(u32, glam::Vec2)]) -> Option<u32> {
        let ppp = ctx.pixels_per_point();
        let mut clicked = None;
        for (aid, p) in signs {
            let Some(title) = self.signs.get(aid) else { continue };
            egui::Area::new(egui::Id::unique(("shop_sign", *aid))).fixed_pos(egui::pos2(p.x / ppp, p.y / ppp)).pivot(egui::Align2::CENTER_BOTTOM).show(ctx, |ui| {
                let b = egui::Button::new(RichText::new(title).color(Color32::BLACK).size(12.0)).fill(Color32::from_rgb(255, 240, 200));
                if ui.add(b).on_hover_text("Click to browse this shop").clicked() {
                    clicked = Some(*aid);
                }
            });
        }
        clicked
    }
}

impl Merchant {
    /// The bank window. Returns deposit (+) or withdrawal (-) amounts, and whether it closed.
    pub fn show_bank(&mut self, ctx: &egui::Context, zeny: i64) -> (Option<i64>, bool) {
        let Some((balance, amount)) = self.bank.as_mut() else { return (None, false) };
        let mut out = None;
        let mut open = true;
        egui::Window::new("Bank").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            egui::Grid::new("bank").num_columns(2).show(ui, |ui| {
                ui.label("In the bank");
                ui.label(RichText::new(balance.map_or("…".into(), |b| format!("{b} z"))).color(Color32::from_rgb(255, 220, 120)));
                ui.end_row();
                ui.label("On hand");
                ui.label(format!("{zeny} z"));
                ui.end_row();
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Amount");
                ui.add(egui::TextEdit::singleline(amount).desired_width(120.0));
                if ui.small_button("All on hand").clicked() {
                    *amount = zeny.to_string();
                }
            });
            let value = amount.trim().replace(',', "").parse::<i64>().ok().filter(|v| *v > 0 && *v <= i32::MAX as i64);
            ui.horizontal(|ui| {
                if ui.add_enabled(value.is_some_and(|v| v <= zeny), egui::Button::new("Deposit")).clicked() {
                    out = value;
                }
                if ui.add_enabled(value.is_some_and(|v| balance.is_none_or(|b| v <= b)), egui::Button::new("Withdraw")).clicked() {
                    out = value.map(|v| -v);
                }
            });
        });
        if out.is_some() {
            self.bank.as_mut().unwrap().1.clear();
        }
        if !open {
            self.bank = None;
        }
        (out, !open)
    }
}

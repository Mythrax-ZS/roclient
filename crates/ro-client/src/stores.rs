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
    cash_page: usize,
    cash_search: String,
    /// The cash shop window's images by file name.
    cash_art: HashMap<String, egui::TextureHandle>,
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
    RichText::new(text).color(crate::classic::ink(Color32::from_rgb(255, 220, 120)))
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
            crate::classic::show(ctx, egui::Window::new("Open Buying Store").open(&mut open).collapsible(false).default_width(440.0), "Open Buying Store", true, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Store name");
                    ui.text_edit_singleline(title);
                });
                ui.horizontal(|ui| {
                    ui.label("Zeny limit");
                    ui.add(egui::TextEdit::singleline(limit).desired_width(110.0));
                    ui.label(RichText::new(format!("(you have {zeny} z)")).color(crate::classic::ink(Color32::GRAY)));
                });
                ui.label(RichText::new(format!("Up to {slots} kinds of item. You must already carry one of each.")).color(crate::classic::ink(Color32::GRAY)));
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
            crate::classic::show(ctx, egui::Window::new("My Buying Store").collapsible(false).default_width(300.0), "My Buying Store", false, |ui| {
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
            crate::classic::show(ctx, egui::Window::new(title.as_str()).id(egui::Id::unique("buying_store_browse")).open(&mut open).collapsible(false).default_width(440.0), &title, true, |ui| {
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
                let b = egui::Button::new(RichText::new(format!("Buying: {title}")).color(crate::classic::ink(Color32::BLACK)).size(12.0)).fill(Color32::from_rgb(200, 230, 255));
                if ui.add(b).on_hover_text("Click to sell to this store").clicked() {
                    clicked = Some(*aid);
                }
            });
        }
        clicked
    }
}

/// The cash shop window's art (texture\유저인터페이스\cashshop).
const CASH_ART: [&str; 19] = [
    "img_shop_bg", "img_shop_itembg", "img_shop_cart_bg", "img_ad1", "btn_close", "btn_add_normal", "btn_add_press", "btn_add_disable", "btn_buy_normal",
    "btn_buy_press", "btn_plus_normal", "btn_minus_normal", "bt_arrowl_off", "bt_arrowl_on", "bt_arrowr_off", "bt_arrowr_on", "btn_searchbar_normal", "btn_searchbar_press",
    "btn_searchbar_over",
];

/// Item cards on a page (3 x 3), and where each sits in the frame.
const PER_PAGE: usize = 9;
const CELL_X: [f32; 3] = [10.0, 185.0, 360.0];
const CELL_Y: [f32; 3] = [108.0, 235.0, 362.0];
/// Basket rows in the cart panel.
const CART_ROWS: usize = 6;

impl Stores {
    /// Loads the cash shop art (once).
    pub fn load_cash_art(&mut self, ctx: &egui::Context, vfs: &ro_vfs::Vfs) {
        if !self.cash_art.is_empty() {
            return;
        }
        let dir = r"data\texture\유저인터페이스\cashshop";
        let tabs = (0..9).flat_map(|i| ["on", "off", "disable"].map(|s| format!("img_shop_tap{i}_{s}")));
        for name in CASH_ART.iter().map(|s| s.to_string()).chain(tabs) {
            if let Some(t) = crate::load_texture(ctx, vfs, &format!(r"{dir}\{name}.bmp")) {
                self.cash_art.insert(name, t);
            }
        }
        tracing::info!(images = self.cash_art.len(), "cash shop art");
    }

    /// The cash shop window in the classic style, drawn with the original's art.
    pub fn show_cash_classic(&mut self, ctx: &egui::Context, db: Option<&ItemDb>, icons: &HashMap<u32, egui::TextureHandle>) -> Option<CashShopRequest> {
        let (cash, kafra) = self.cash?;
        let mut out = None;
        let mut close = false;
        if !self.cash_tabs.contains_key(&self.cash_tab) {
            if let Some(first) = self.cash_tabs.keys().next() {
                self.cash_tab = *first;
            }
        }
        let art = &self.cash_art;
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        let dark = Color32::from_rgb(40, 40, 60);
        let font = egui::FontId::proportional;
        egui::Area::new(egui::Id::unique("cash_shop")).default_pos(egui::pos2(200.0, 80.0)).movable(true).show(ctx, |ui| {
            let (frame, _) = ui.allocate_exact_size(Vec2::new(723.0, 540.0), egui::Sense::drag());
            let o = frame.min.to_vec2();
            let at = |x: f32, y: f32, w: f32, h: f32| egui::Rect::from_min_size(egui::pos2(x, y) + o, Vec2::new(w, h));
            let p = ui.painter().clone();
            let draw = |name: &str, x: f32, y: f32| {
                if let Some(t) = art.get(name) {
                    p.image(t.id(), at(x, y, t.size_vec2().x, t.size_vec2().y), uv, Color32::WHITE);
                }
            };
            // An image button with an optional label; true when clicked.
            let button = |ui: &mut egui::Ui, normal: &str, pressed: &str, disabled: &str, x: f32, y: f32, label: &str, enabled: bool| {
                let Some(t) = art.get(normal) else { return false };
                let r = at(x, y, t.size_vec2().x, t.size_vec2().y);
                let resp = ui.interact(r, ui.scope_id().with(("cash_btn", normal, x as i32, y as i32)), egui::Sense::click());
                let shown = if !enabled { disabled } else if resp.is_pointer_button_down_on() { pressed } else { normal };
                draw(if art.contains_key(shown) { shown } else { normal }, x, y);
                if !enabled && !art.contains_key(disabled) {
                    p.rect_filled(r, 3.0, Color32::from_black_alpha(60));
                }
                if !label.is_empty() {
                    p.text(r.center(), egui::Align2::CENTER_CENTER, label, font(12.5), if enabled { dark } else { Color32::GRAY });
                }
                enabled && resp.clicked()
            };

            if art.contains_key("img_shop_bg") {
                draw("img_shop_bg", 0.0, 0.0);
            } else {
                p.rect_filled(frame, 4.0, Color32::from_rgb(235, 238, 248));
            }
            p.text(at(0.0, 0.0, 723.0, 16.0).center(), egui::Align2::CENTER_CENTER, "Cash Shop", font(11.5), Color32::WHITE);
            draw("btn_close", 710.0, 4.0);
            if ui.interact(at(704.0, 0.0, 19.0, 16.0), ui.scope_id().with("cash_close"), egui::Sense::click()).clicked() {
                close = true;
            }
            draw("img_ad1", 2.0, 18.0);

            // Points: cash in the dark bar, Kafra points in the white box.
            p.text(egui::pos2(626.0, 55.0) + o, egui::Align2::RIGHT_CENTER, cash.to_string(), font(12.5), Color32::from_rgb(255, 220, 120));
            p.text(egui::pos2(713.0, 55.0) + o, egui::Align2::RIGHT_CENTER, kafra.to_string(), font(12.5), dark);
            ui.interact(at(543.0, 46.0, 175.0, 19.0), ui.scope_id().with("cash_points"), egui::Sense::hover()).on_hover_text("Cash points  |  Kafra points");

            // Tabs: the original's lettered tab images, greyed when the server sent nothing for one.
            for i in 0..9u16 {
                let x = 4.0 + i as f32 * 59.0;
                let has = self.cash_tabs.contains_key(&i);
                let state = if !has { "disable" } else if i == self.cash_tab { "on" } else { "off" };
                draw(&format!("img_shop_tap{i}_{state}"), x, 72.0);
                let resp = ui.interact(at(x, 72.0, 56.0, 31.0), ui.scope_id().with(("cash_tab", i)), egui::Sense::click()).on_hover_text(CASH_TABS[i as usize]);
                if has && resp.clicked() && self.cash_tab != i {
                    self.cash_tab = i;
                    self.cash_page = 0;
                }
            }

            // The item cards for this tab and page (filtered by the search box).
            let needle = self.cash_search.trim().to_lowercase();
            let list: Vec<(u32, u32)> = self
                .cash_tabs
                .get(&self.cash_tab)
                .into_iter()
                .flatten()
                .filter(|(id, _)| needle.is_empty() || name(db, *id).to_lowercase().contains(&needle))
                .copied()
                .collect();
            let pages = list.len().div_ceil(PER_PAGE).max(1);
            self.cash_page = self.cash_page.min(pages - 1);
            for (n, (id, price)) in list.iter().skip(self.cash_page * PER_PAGE).take(PER_PAGE).enumerate() {
                let (x, y) = (CELL_X[n % 3] - 2.0, CELL_Y[n / 3] - 2.0);
                draw("img_shop_itembg", x, y);
                let mut title = name(db, *id);
                if title.chars().count() > 22 {
                    title = title.chars().take(21).collect::<String>() + "…";
                }
                p.text(egui::pos2(x + 86.0, y + 11.0) + o, egui::Align2::CENTER_CENTER, title, font(12.0), dark);
                let icon_rect = at(x + 16.0, y + 30.0, 48.0, 48.0);
                if let Some(t) = icons.get(id) {
                    p.image(t.id(), icon_rect, uv, Color32::WHITE);
                }
                if let Some(info) = db.and_then(|d| d.get(*id)) {
                    ui.interact(icon_rect, ui.scope_id().with(("cash_icon", *id)), egui::Sense::hover()).on_hover_ui(|ui| {
                        ui.set_max_width(300.0);
                        for line in &info.description {
                            ui.label(crate::hud::npc_text(line, 12.5));
                        }
                    });
                }
                p.text(egui::pos2(x + 158.0, y + 54.0) + o, egui::Align2::RIGHT_CENTER, price.to_string(), font(12.5), Color32::WHITE);
                let room = self.basket.len() < CART_ROWS || self.basket.contains_key(&(*id, self.cash_tab));
                if button(ui, "btn_add_normal", "btn_add_press", "btn_add_disable", x + 46.0, y + 94.0, "Add to cart", room) {
                    *self.basket.entry((*id, self.cash_tab)).or_insert(0) += 1;
                }
            }
            if list.is_empty() {
                let msg = if self.cash_tabs.is_empty() { "The shop has nothing for sale." } else { "Nothing matches." };
                p.text(egui::pos2(268.0, 290.0) + o, egui::Align2::CENTER_CENTER, msg, font(14.0), Color32::GRAY);
            }

            // Pages.
            if pages > 1 {
                let (back, next) = (self.cash_page > 0, self.cash_page + 1 < pages);
                if button(ui, "bt_arrowl_off", "bt_arrowl_on", "bt_arrowl_off", 230.0, 492.0, "", back) {
                    self.cash_page -= 1;
                }
                p.text(egui::pos2(268.0, 498.0) + o, egui::Align2::CENTER_CENTER, format!("{} / {pages}", self.cash_page + 1), font(12.0), dark);
                if button(ui, "bt_arrowr_off", "bt_arrowr_on", "bt_arrowr_off", 295.0, 492.0, "", next) {
                    self.cash_page += 1;
                }
            }

            // Search bar along the bottom.
            let search = egui::TextEdit::singleline(&mut self.cash_search).frame(egui::Frame::NONE).hint_text("Search items").text_color(dark).font(font(12.5));
            if ui.put(at(96.0, 517.0, 292.0, 16.0), search).changed() {
                self.cash_page = 0;
            }
            if button(ui, "btn_searchbar_normal", "btn_searchbar_press", "btn_searchbar_normal", 391.0, 516.0, "", true) {
                self.cash_page = 0;
            }

            // The cart: one row per item with an amount spinner; right-click removes.
            draw("img_shop_cart_bg", 537.0, 72.0);
            let price_of = |id: u32, tab: u16| self.cash_tabs.get(&tab).and_then(|t| t.iter().find(|(i, _)| *i == id)).map_or(0, |(_, p)| *p);
            let mut remove = None;
            for (row, ((id, tab), n)) in self.basket.iter_mut().take(CART_ROWS).enumerate() {
                let (x, y) = (542.0, 77.0 + row as f32 * 57.5);
                if let Some(t) = icons.get(id) {
                    p.image(t.id(), at(x + 4.0, y + 12.0, 24.0, 24.0), uv, Color32::WHITE);
                }
                let mut title = name(db, *id);
                if title.chars().count() > 18 {
                    title = title.chars().take(17).collect::<String>() + "…";
                }
                p.text(egui::pos2(x + 34.0, y + 12.0) + o, egui::Align2::LEFT_CENTER, title, font(11.5), dark);
                p.text(egui::pos2(x + 34.0, y + 32.0) + o, egui::Align2::LEFT_CENTER, format!("{} pts", price_of(*id, *tab) as u64 * *n as u64), font(11.5), Color32::from_rgb(60, 80, 160));
                p.text(egui::pos2(x + 136.0, y + 32.0) + o, egui::Align2::RIGHT_CENTER, format!("x{n}"), font(12.0), dark);
                if button(ui, "btn_plus_normal", "btn_plus_normal", "btn_plus_normal", x + 140.0, y + 23.0, "", *n < 99) {
                    *n += 1;
                }
                if button(ui, "btn_minus_normal", "btn_minus_normal", "btn_minus_normal", x + 140.0, y + 33.0, "", true) {
                    if *n > 1 {
                        *n -= 1;
                    } else {
                        remove = Some((*id, *tab));
                    }
                }
                let resp = ui.interact(at(x, y, 134.0, 50.0), ui.scope_id().with(("cash_row", *id, *tab)), egui::Sense::click());
                if resp.on_hover_text("Right-click to remove").secondary_clicked() {
                    remove = Some((*id, *tab));
                }
            }
            if let Some(k) = remove {
                self.basket.remove(&k);
            }
            let total: u64 = self.basket.iter().map(|((id, tab), n)| price_of(*id, *tab) as u64 * *n as u64).sum();

            // Kafra points to spend, the total, what's left, Buy.
            let max_kafra = kafra.min(total as u32);
            self.use_kafra = self.use_kafra.min(max_kafra);
            p.text(egui::pos2(547.0, 438.0) + o, egui::Align2::LEFT_CENTER, "Use Kafra points", font(11.5), dark);
            ui.put(at(655.0, 430.0, 56.0, 16.0), egui::DragValue::new(&mut self.use_kafra).range(0..=max_kafra));
            p.text(egui::pos2(547.0, 455.0) + o, egui::Align2::LEFT_CENTER, "Total", font(12.5), dark);
            p.text(egui::pos2(708.0, 455.0) + o, egui::Align2::RIGHT_CENTER, total.to_string(), font(12.5), dark);
            let left = cash as i64 + self.use_kafra as i64 - total as i64;
            let (text, color) = if left < 0 { ("not enough".to_string(), Color32::from_rgb(200, 60, 60)) } else { ((left.min(cash as i64)).to_string(), Color32::GRAY) };
            p.text(egui::pos2(547.0, 472.0) + o, egui::Align2::LEFT_CENTER, "Cash after", font(11.5), Color32::GRAY);
            p.text(egui::pos2(708.0, 472.0) + o, egui::Align2::RIGHT_CENTER, text, font(11.5), color);
            if button(ui, "btn_buy_normal", "btn_buy_press", "btn_buy_normal", 553.0, 487.0, "Buy", total > 0 && left >= 0) {
                out = Some(CashShopRequest::Buy { kafra: self.use_kafra, items: self.basket.iter().map(|((id, tab), n)| (*id, *n, *tab)).collect() });
                self.basket.clear();
                self.use_kafra = 0;
            }
            let clear = at(553.0, 510.0, 152.0, 18.0);
            if ui.interact(clear, ui.scope_id().with("cash_clear"), egui::Sense::click()).clicked() {
                self.basket.clear();
            }
            p.text(clear.center(), egui::Align2::CENTER_CENTER, "Empty cart", font(12.0), dark);
        });
        if close {
            self.cash = None;
            out = Some(CashShopRequest::Close);
        }
        out
    }
}

impl Stores {
    /// The cash shop window in the modern (plain egui) style.
    pub fn show_cash_modern(&mut self, ctx: &egui::Context, db: Option<&ItemDb>, icons: &HashMap<u32, egui::TextureHandle>) -> Option<CashShopRequest> {
        let (cash, kafra) = self.cash?;
        let mut out = None;
        let mut open = true;
        if !self.cash_tabs.contains_key(&self.cash_tab) {
            if let Some(first) = self.cash_tabs.keys().next() {
                self.cash_tab = *first;
            }
        }
        crate::classic::show(ctx, egui::Window::new("Cash Shop").open(&mut open).collapsible(false).default_width(560.0), "Cash Shop", true, |ui| {
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
                ui.label(RichText::new("The shop has nothing for sale.").color(crate::classic::ink(Color32::GRAY)));
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

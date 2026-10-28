//! Shops, Kafra storage, trading, party, friends and guild: state from the server and the
//! windows that act on it. `show` returns the commands the player chose.

use std::collections::{BTreeMap, HashMap};

use egui::{Color32, RichText, Vec2};
use ro_game::items::{InvItem, ItemDb};
use ro_net::{Command, Friend, FriendRequest, PartyMember, PartyRequest, SellOffer, ShopItem, ShopRequest, StorageRequest, TradeRequest};

/// An NPC shop in progress.
pub enum Shop {
    /// The NPC asked: buy or sell?
    Choose { npc: u32 },
    Buy { items: Vec<ShopItem>, cart: BTreeMap<u32, u16> },
    Sell { offers: Vec<SellOffer>, cart: BTreeMap<u16, u16> },
}

#[derive(Default)]
pub struct Storage {
    pub open: bool,
    pub items: BTreeMap<u16, InvItem>,
    pub count: (u16, u16),
}

/// A trade in progress (or being requested).
#[derive(Default)]
pub struct Trade {
    pub partner: String,
    pub ours: Vec<(u32, i32)>,
    pub theirs: Vec<(u32, i32)>,
    pub zeny_ours: i32,
    pub zeny_theirs: i32,
    pub we_locked: bool,
    pub they_locked: bool,
    pub zeny_input: String,
}

#[derive(Default)]
pub struct Party {
    pub name: String,
    pub members: Vec<PartyMember>,
    pub hp: HashMap<u32, (i32, i32)>,
}

/// A yes/no question from another player.
pub enum Prompt {
    Trade { name: String },
    Party { party: u32, name: String },
    Friend { account_id: u32, char_id: u32, name: String },
}

#[derive(Default)]
pub struct Social {
    pub shop: Option<Shop>,
    pub storage: Storage,
    pub trade: Option<Trade>,
    pub party: Option<Party>,
    pub friends: Vec<Friend>,
    pub guild: Option<String>,
    pub prompts: Vec<Prompt>,
    pub show_party: bool,
    pub show_friends: bool,
    party_name: String,
    invite_name: String,
    friend_name: String,
}

/// What the windows need from the rest of the client.
pub struct Ctx<'a> {
    pub db: Option<&'a ItemDb>,
    pub icons: &'a HashMap<u32, egui::TextureHandle>,
    pub inventory: &'a [&'a InvItem],
    pub zeny: i64,
    pub my_name: &'a str,
}

fn name(db: Option<&ItemDb>, id: u32) -> String {
    db.map_or_else(|| format!("Item #{id}"), |d| d.name(id, true))
}

fn icon(ui: &mut egui::Ui, icons: &HashMap<u32, egui::TextureHandle>, id: u32) {
    match icons.get(&id) {
        Some(t) => {
            ui.image((t.id(), Vec2::splat(24.0)));
        }
        None => {
            ui.allocate_exact_size(Vec2::splat(24.0), egui::Sense::hover());
        }
    }
}

impl Social {
    /// Accepts the oldest pending request (scripted tests).
    pub fn accept_first(&mut self) -> Option<Command> {
        if self.prompts.is_empty() {
            return None;
        }
        Some(match self.prompts.remove(0) {
            Prompt::Trade { .. } => Command::Trade(TradeRequest::Answer { accept: true }),
            Prompt::Party { party, .. } => Command::Party(PartyRequest::Answer { party, accept: true }),
            Prompt::Friend { account_id, char_id, .. } => Command::Friend(FriendRequest::Answer { account_id, char_id, accept: true }),
        })
    }

    pub fn show(&mut self, ctx: &egui::Context, c: &Ctx) -> Vec<Command> {
        let mut out = vec![];
        self.prompts(ctx, &mut out);
        self.shop_window(ctx, c, &mut out);
        self.storage_window(ctx, c, &mut out);
        self.trade_window(ctx, c, &mut out);
        self.party_window(ctx, c, &mut out);
        self.friends_window(ctx, &mut out);
        out
    }

    fn prompts(&mut self, ctx: &egui::Context, out: &mut Vec<Command>) {
        let mut answered = None;
        if let Some((i, p)) = self.prompts.iter().enumerate().next() {
            let text = match p {
                Prompt::Trade { name } => format!("{name} wants to trade with you."),
                Prompt::Party { name, .. } => format!("You are invited to the party \"{name}\"."),
                Prompt::Friend { name, .. } => format!("{name} wants to be your friend."),
            };
            egui::Window::new("Request").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_TOP, Vec2::new(0.0, 120.0)).show(ctx, |ui| {
                ui.label(text);
                ui.horizontal(|ui| {
                    let yes = ui.button("Accept").clicked();
                    let no = ui.button("Decline").clicked();
                    if yes || no {
                        answered = Some(i);
                        out.push(match p {
                            Prompt::Trade { .. } => Command::Trade(TradeRequest::Answer { accept: yes }),
                            Prompt::Party { party, .. } => Command::Party(PartyRequest::Answer { party: *party, accept: yes }),
                            Prompt::Friend { account_id, char_id, .. } => {
                                Command::Friend(FriendRequest::Answer { account_id: *account_id, char_id: *char_id, accept: yes })
                            }
                        });
                    }
                });
            });
        }
        if let Some(i) = answered {
            self.prompts.remove(i);
        }
    }

    fn shop_window(&mut self, ctx: &egui::Context, c: &Ctx, out: &mut Vec<Command>) {
        let Some(shop) = self.shop.as_mut() else { return };
        let mut close = false;
        let mut open = true;
        let title = match shop {
            Shop::Choose { .. } => "Shop",
            Shop::Buy { .. } => "Buy",
            Shop::Sell { .. } => "Sell",
        };
        egui::Window::new(title).open(&mut open).resizable(false).anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| match shop {
            Shop::Choose { npc } => {
                ui.horizontal(|ui| {
                    if ui.button("Buy").clicked() {
                        out.push(Command::Shop(ShopRequest::Deal { npc: *npc, buy: true }));
                    }
                    if ui.button("Sell").clicked() {
                        out.push(Command::Shop(ShopRequest::Deal { npc: *npc, buy: false }));
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            }
            Shop::Buy { items, cart } => {
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    egui::Grid::new("buy").num_columns(4).spacing(Vec2::new(8.0, 4.0)).show(ui, |ui| {
                        for it in items.iter() {
                            icon(ui, c.icons, it.id);
                            ui.label(name(c.db, it.id));
                            ui.label(RichText::new(format!("{} z", it.discount_price)).color(Color32::from_rgb(240, 210, 120)));
                            let n = cart.entry(it.id).or_insert(0);
                            ui.add(egui::DragValue::new(n).range(0..=999));
                            ui.end_row();
                        }
                    });
                });
                cart.retain(|_, n| *n > 0);
                let total: u64 = items.iter().map(|i| i.discount_price as u64 * *cart.get(&i.id).unwrap_or(&0) as u64).sum();
                ui.separator();
                ui.horizontal(|ui| {
                    let affordable = total as i64 <= c.zeny;
                    ui.label(RichText::new(format!("Total {total} z  (you have {} z)", c.zeny)).color(if affordable { Color32::LIGHT_GRAY } else { Color32::from_rgb(255, 120, 100) }));
                    if ui.add_enabled(!cart.is_empty() && affordable, egui::Button::new("Buy")).clicked() {
                        out.push(Command::Shop(ShopRequest::Buy(cart.iter().map(|(id, n)| (*id, *n)).collect())));
                        close = true;
                    }
                });
            }
            Shop::Sell { offers, cart } => {
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    egui::Grid::new("sell").num_columns(4).spacing(Vec2::new(8.0, 4.0)).show(ui, |ui| {
                        for o in offers.iter() {
                            let Some(it) = c.inventory.iter().find(|i| i.index == o.index) else { continue };
                            if it.equipped != 0 {
                                continue;
                            }
                            icon(ui, c.icons, it.id);
                            ui.label(format!("{} x{}", name(c.db, it.id), it.amount));
                            ui.label(RichText::new(format!("{} z", o.overcharge_price)).color(Color32::from_rgb(240, 210, 120)));
                            let n = cart.entry(o.index).or_insert(0);
                            ui.add(egui::DragValue::new(n).range(0..=it.amount));
                            ui.end_row();
                        }
                    });
                });
                cart.retain(|_, n| *n > 0);
                let total: u64 = offers.iter().map(|o| o.overcharge_price as u64 * *cart.get(&o.index).unwrap_or(&0) as u64).sum();
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(format!("Total {total} z"));
                    if ui.add_enabled(!cart.is_empty(), egui::Button::new("Sell")).clicked() {
                        out.push(Command::Shop(ShopRequest::Sell(cart.iter().map(|(i, n)| (*i, *n)).collect())));
                        close = true;
                    }
                });
            }
        });
        if close || !open {
            self.shop = None;
        }
    }

    fn storage_window(&mut self, ctx: &egui::Context, c: &Ctx, out: &mut Vec<Command>) {
        if !self.storage.open {
            return;
        }
        let mut open = true;
        egui::Window::new(format!("Storage ({}/{})", self.storage.count.0, self.storage.count.1))
            .id(egui::Id::unique("storage"))
            .open(&mut open)
            .resizable(false)
            .default_pos(egui::pos2(60.0, 280.0))
            .show(ctx, |ui| {
                ui.columns(2, |cols| {
                    cols[0].label(RichText::new("Storage").strong());
                    egui::ScrollArea::vertical().id_salt("st").max_height(300.0).show(&mut cols[0], |ui| {
                        for it in self.storage.items.values() {
                            ui.horizontal(|ui| {
                                icon(ui, c.icons, it.id);
                                let r = ui.add(egui::Label::new(format!("{} x{}", name(c.db, it.id), it.amount)).sense(egui::Sense::click()));
                                if r.on_hover_text("Double-click to take").double_clicked() {
                                    out.push(Command::Storage(StorageRequest::Take { index: it.index, amount: it.amount as u32 }));
                                }
                            });
                        }
                    });
                    cols[1].label(RichText::new("Inventory").strong());
                    egui::ScrollArea::vertical().id_salt("inv").max_height(300.0).show(&mut cols[1], |ui| {
                        for it in c.inventory.iter().filter(|i| i.equipped == 0) {
                            ui.horizontal(|ui| {
                                icon(ui, c.icons, it.id);
                                let r = ui.add(egui::Label::new(format!("{} x{}", name(c.db, it.id), it.amount)).sense(egui::Sense::click()));
                                if r.on_hover_text("Double-click to store").double_clicked() {
                                    out.push(Command::Storage(StorageRequest::Put { index: it.index, amount: it.amount as u32 }));
                                }
                            });
                        }
                    });
                });
            });
        if !open {
            out.push(Command::Storage(StorageRequest::Close));
            self.storage.open = false;
        }
    }

    fn trade_window(&mut self, ctx: &egui::Context, c: &Ctx, out: &mut Vec<Command>) {
        let Some(t) = self.trade.as_mut() else { return };
        let mut open = true;
        egui::Window::new(format!("Trade with {}", t.partner)).id(egui::Id::unique("trade")).open(&mut open).resizable(false).show(ctx, |ui| {
            ui.columns(2, |cols| {
                for (col, title, items, zeny, locked) in [
                    (0, t.partner.as_str(), &t.theirs, t.zeny_theirs, t.they_locked),
                    (1, c.my_name, &t.ours, t.zeny_ours, t.we_locked),
                ] {
                    let ui = &mut cols[col];
                    ui.label(RichText::new(title).strong().color(if locked { Color32::from_rgb(120, 220, 120) } else { Color32::WHITE }));
                    for (id, n) in items {
                        ui.horizontal(|ui| {
                            icon(ui, c.icons, *id);
                            ui.label(format!("{} x{n}", name(c.db, *id)));
                        });
                    }
                    ui.label(format!("{zeny} z"));
                }
            });
            ui.separator();
            if !t.we_locked {
                ui.label(RichText::new("Add from inventory (double-click):").size(11.0).color(Color32::GRAY));
                egui::ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
                    for it in c.inventory.iter().filter(|i| i.equipped == 0) {
                        let r = ui.add(egui::Label::new(format!("{} x{}", name(c.db, it.id), it.amount)).sense(egui::Sense::click()));
                        if r.double_clicked() {
                            out.push(Command::Trade(TradeRequest::Add { index: it.index, amount: it.amount as i32 }));
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut t.zeny_input).hint_text("zeny").desired_width(100.0));
                    if ui.button("Add zeny").clicked() {
                        if let Ok(z) = t.zeny_input.trim().parse::<i32>() {
                            out.push(Command::Trade(TradeRequest::Add { index: 0, amount: z }));
                            t.zeny_ours = z;
                        }
                    }
                });
            }
            ui.horizontal(|ui| {
                if !t.we_locked && ui.button("OK (lock)").clicked() {
                    out.push(Command::Trade(TradeRequest::Ok));
                }
                if t.we_locked && t.they_locked && ui.button("Trade").clicked() {
                    out.push(Command::Trade(TradeRequest::Commit));
                }
                if ui.button("Cancel").clicked() {
                    out.push(Command::Trade(TradeRequest::Cancel));
                }
            });
        });
        if !open {
            out.push(Command::Trade(TradeRequest::Cancel));
        }
    }

    fn party_window(&mut self, ctx: &egui::Context, c: &Ctx, out: &mut Vec<Command>) {
        if !self.show_party {
            return;
        }
        let mut open = true;
        egui::Window::new("Party").open(&mut open).resizable(false).default_pos(egui::pos2(60.0, 260.0)).show(ctx, |ui| {
            match &self.party {
                None => {
                    ui.label("You are not in a party.");
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.party_name).hint_text("party name").desired_width(140.0));
                        if ui.add_enabled(!self.party_name.trim().is_empty(), egui::Button::new("Create")).clicked() {
                            out.push(Command::Party(PartyRequest::Create { name: self.party_name.trim().to_string() }));
                        }
                    });
                }
                Some(p) => {
                    ui.label(RichText::new(&p.name).strong());
                    for m in &p.members {
                        ui.horizontal(|ui| {
                            let color = if m.online { Color32::WHITE } else { Color32::GRAY };
                            let crown = if m.leader { "[Leader] " } else { "" };
                            ui.label(RichText::new(format!("{crown}{} (Lv {})", m.name, m.level)).color(color));
                            ui.label(RichText::new(&m.map).size(11.0).color(Color32::GRAY));
                        });
                        if let Some(&(hp, max)) = p.hp.get(&m.account_id) {
                            ui.add(egui::ProgressBar::new(hp as f32 / max.max(1) as f32).desired_height(6.0).fill(Color32::from_rgb(70, 190, 90)));
                        }
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.invite_name).hint_text("character name").desired_width(140.0));
                        if ui.add_enabled(!self.invite_name.trim().is_empty(), egui::Button::new("Invite")).clicked() {
                            out.push(Command::Party(PartyRequest::Invite { name: self.invite_name.trim().to_string() }));
                            self.invite_name.clear();
                        }
                    });
                    if ui.button("Leave party").clicked() {
                        out.push(Command::Party(PartyRequest::Leave));
                    }
                }
            }
            if let Some(g) = &self.guild {
                ui.separator();
                ui.label(format!("Guild: {g}"));
            }
            ui.label(RichText::new("Chat: /p message for party, /g message for guild").size(11.0).color(Color32::GRAY));
            let _ = c;
        });
        self.show_party = open;
    }

    fn friends_window(&mut self, ctx: &egui::Context, out: &mut Vec<Command>) {
        if !self.show_friends {
            return;
        }
        let mut open = true;
        egui::Window::new("Friends").open(&mut open).resizable(false).default_pos(egui::pos2(60.0, 460.0)).show(ctx, |ui| {
            if self.friends.is_empty() {
                ui.label(RichText::new("No friends yet").color(Color32::GRAY));
            }
            let mut remove = None;
            for f in &self.friends {
                ui.horizontal(|ui| {
                    let (dot, color) = if f.online { ("online", Color32::from_rgb(90, 220, 90)) } else { ("offline", Color32::GRAY) };
                    ui.label(RichText::new(dot).color(color));
                    ui.label(&f.name);
                    if ui.small_button("Remove").clicked() {
                        remove = Some((f.account_id, f.char_id));
                    }
                });
            }
            if let Some((account_id, char_id)) = remove {
                out.push(Command::Friend(FriendRequest::Remove { account_id, char_id }));
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.friend_name).hint_text("character name").desired_width(140.0));
                if ui.add_enabled(!self.friend_name.trim().is_empty(), egui::Button::new("Add")).clicked() {
                    out.push(Command::Friend(FriendRequest::Add { name: self.friend_name.trim().to_string() }));
                    self.friend_name.clear();
                }
            });
        });
        self.show_friends = open;
    }
}

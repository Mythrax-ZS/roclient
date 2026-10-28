//! Game windows: inventory, equipment, character stats and skills. They read the player's
//! state and return the actions the player chose; main.rs turns those into requests.

use std::collections::HashMap;

use egui::{Color32, RichText, Vec2};
use ro_game::items::{InvItem, ItemDb};
use ro_net::{Hotkey, Skill, Stats};

/// What the player asked for through a window.
#[derive(Debug, Clone, PartialEq)]
pub enum UiAction {
    UseItem(u16),
    Equip { index: u16, location: u32 },
    Unequip(u16),
    Drop { index: u16, amount: u16 },
    /// Raise a base stat; 0..6 = str, agi, vit, int, dex, luk.
    RaiseStat(usize),
    /// Ask which equipment can take this card (inventory index).
    CardTargets(u16),
    InsertCard { card: u16, equip: u16 },
    /// Answer the open skill/item menu.
    Menu(ro_net::MenuReply),
    SkillUp(u16),
    UseSkill(u16),
    /// Activate shortcut slot n (row * HOTKEYS_PER_ROW + column).
    UseHotkey(usize),
    SetHotkey { index: u16, key: Hotkey },
    HotkeyRow(u8),
}

/// The shortcut bar: rows of F1..F9 slots, saved on the server (38 slots per tab).
pub const HOTKEY_ROWS: u8 = 4;
pub const HOTKEYS_PER_ROW: usize = 9;

#[derive(Default)]
pub struct Windows {
    pub inventory: bool,
    pub equipment: bool,
    pub stats: bool,
    pub skills: bool,
    /// Show skills as a plain list instead of the tree.
    pub skill_list: bool,
    /// The skill tree tab shown; None opens on the current job's tab.
    pub skill_tab: Option<usize>,
    /// A card being inserted and the equipment the server offered for it.
    pub card_pick: Option<(u16, Vec<u16>)>,
    /// A pick-one menu from a skill or item (poisons, warp points, eggs...).
    pub menu: Option<ro_net::Menu>,
    /// How long a stat "+" has been held, for repeating.
    stat_hold: f32,
    pub party: bool,
    pub friends: bool,
    pub guild: bool,
    pub settings: bool,
    tab: InvTab,
}

#[derive(Default, PartialEq, Clone, Copy)]
enum InvTab {
    #[default]
    Items,
    Equip,
    Etc,
}

/// Everything the windows draw.
pub struct View<'a> {
    pub items: &'a [&'a InvItem],
    pub db: Option<&'a ItemDb>,
    pub icons: &'a HashMap<u32, egui::TextureHandle>,
    pub skill_icons: &'a HashMap<u16, egui::TextureHandle>,
    pub stats: Option<&'a Stats>,
    pub skills: &'a [Skill],
    pub skill_names: &'a dyn Fn(&str) -> String,
    pub skill_info: Option<&'a ro_game::skills::SkillNames>,
    pub skill_tree: Option<&'a crate::skilltree::SkillTree>,
    /// The player's job id, choosing the skill tree tabs.
    pub job: i16,
    pub skill_points: i64,
    pub weight: (i64, i64),
    pub zeny: i64,
    /// Screen width in UI points, for the default window layout.
    pub screen_width: f32,
    pub hotkeys: &'a [Hotkey],
    pub hotkey_row: u8,
    /// Skills cooling down: id -> when they are ready again.
    pub cooldowns: &'a HashMap<u16, std::time::Instant>,
    /// A controller is in use: label slots with its buttons.
    pub pad_hints: bool,
    pub playstation: bool,
}

/// Draws a skill or item icon centred in `rect`.
fn paint_icon(painter: &egui::Painter, rect: egui::Rect, tex: Option<&egui::TextureHandle>, tint: Color32) {
    if let Some(tex) = tex {
        let s = tex.size_vec2().min(rect.size() - Vec2::splat(6.0));
        painter.image(tex.id(), egui::Rect::from_center_size(rect.center(), s), egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), tint);
    }
}

fn outlined(painter: &egui::Painter, pos: egui::Pos2, align: egui::Align2, text: &str, size: f32, color: Color32) {
    let font = egui::FontId::proportional(size);
    painter.text(pos + Vec2::new(1.0, 1.0), align, text, font.clone(), Color32::BLACK);
    painter.text(pos, align, text, font, color);
}

/// The shortcut bar along the bottom: drag skills and items in, click or press F1..F9 to
/// use, right-click to clear, scroll over a skill to change its level, F12 for the next row.
fn hotkey_bar(ctx: &egui::Context, v: &View) -> Option<UiAction> {
    const SIZE: f32 = 40.0;
    let mut action = None;
    egui::Area::new(egui::Id::unique("hotkeys")).anchor(egui::Align2::CENTER_BOTTOM, Vec2::new(0.0, -12.0)).show(ctx, |ui| {
        egui::Frame::new().fill(Color32::from_rgba_unmultiplied(16, 20, 28, 210)).corner_radius(8).inner_margin(6).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let base = v.hotkey_row as usize * HOTKEYS_PER_ROW;
                for col in 0..HOTKEYS_PER_ROW {
                    let index = base + col;
                    let key = v.hotkeys.get(index).copied().unwrap_or_default();
                    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(SIZE), egui::Sense::click_and_drag());
                    let painter = ui.painter_at(rect);
                    let hovering_drop = resp.dnd_hover_payload::<Hotkey>().is_some();
                    let bg = if hovering_drop { Color32::from_rgba_unmultiplied(255, 220, 120, 60) } else if resp.hovered() { Color32::from_white_alpha(30) } else { Color32::from_white_alpha(12) };
                    painter.rect_filled(rect, 4.0, bg);
                    let skill = key.skill.then(|| v.skills.iter().find(|s| s.id as u32 == key.id)).flatten();
                    if key.id != 0 {
                        if key.skill {
                            paint_icon(&painter, rect, v.skill_icons.get(&(key.id as u16)), Color32::WHITE);
                            if skill.is_some_and(|s| s.level > 1) {
                                outlined(&painter, rect.right_bottom() - Vec2::new(3.0, 1.0), egui::Align2::RIGHT_BOTTOM, &format!("{}", key.count), 11.0, Color32::from_rgb(255, 220, 120));
                            }
                            // Cooldown: a dark sweep with the seconds left, as in the original.
                            if let Some(left) = v.cooldowns.get(&(key.id as u16)).map(|e| e.saturating_duration_since(std::time::Instant::now()).as_secs_f32()).filter(|s| *s > 0.0) {
                                painter.rect_filled(rect.shrink(2.0), 4.0, Color32::from_black_alpha(150));
                                let text = if left >= 60.0 { format!("{}m", (left / 60.0).ceil()) } else { format!("{left:.1}") };
                                outlined(&painter, rect.center(), egui::Align2::CENTER_CENTER, &text, 12.0, Color32::WHITE);
                                ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                            }
                        } else {
                            let count: u32 = v.items.iter().filter(|it| it.id == key.id && it.equipped == 0).map(|it| it.amount as u32).sum();
                            let tint = if count == 0 { Color32::from_white_alpha(70) } else { Color32::WHITE };
                            paint_icon(&painter, rect, v.icons.get(&key.id), tint);
                            outlined(&painter, rect.right_bottom() - Vec2::new(3.0, 1.0), egui::Align2::RIGHT_BOTTOM, &count.to_string(), 11.0, Color32::WHITE);
                        }
                    }
                    // Controller: hold RT for this row (LT for the next), then the button shown.
                    const XBOX: [&str; HOTKEYS_PER_ROW] = ["A", "B", "X", "Y", "Up", "Rt", "Dn", "Lt", ""];
                    const PS: [&str; HOTKEYS_PER_ROW] = ["Cr", "Ci", "Sq", "Tr", "Up", "Rt", "Dn", "Lt", ""];
                    let key_label = if v.pad_hints { (if v.playstation { PS } else { XBOX })[col].to_string() } else { format!("F{}", col + 1) };
                    outlined(&painter, rect.left_top() + Vec2::new(3.0, 1.0), egui::Align2::LEFT_TOP, &key_label, 10.0, Color32::from_gray(200));

                    if let Some(dropped) = resp.dnd_release_payload::<Hotkey>() {
                        action = Some(UiAction::SetHotkey { index: index as u16, key: *dropped });
                    } else if key.id != 0 {
                        resp.dnd_set_drag_payload(key);
                        if resp.clicked() {
                            action = Some(UiAction::UseHotkey(index));
                        } else if resp.secondary_clicked() {
                            action = Some(UiAction::SetHotkey { index: index as u16, key: Hotkey::default() });
                        } else if let (true, Some(s)) = (resp.hovered(), skill) {
                            let scroll: f32 = ui.input(|i| i.events.iter().map(|e| if let egui::Event::MouseWheel { delta, .. } = e { delta.y } else { 0.0 }).sum());
                            if scroll != 0.0 {
                                let count = (key.count + scroll.signum() as i16).clamp(1, s.level.max(1) as i16);
                                if count != key.count {
                                    action = Some(UiAction::SetHotkey { index: index as u16, key: Hotkey { count, ..key } });
                                }
                            }
                        }
                        let tip = match skill {
                            Some(s) => format!("{} Lv {}", (v.skill_names)(&s.name), key.count),
                            None if key.skill => format!("Skill {}", key.id),
                            None => v.db.map_or_else(|| format!("Item {}", key.id), |db| db.name(key.id, true)),
                        };
                        resp.on_hover_text(tip);
                    }
                }
                ui.separator();
                let row = ui.add(egui::Button::new(RichText::new(format!("{}", v.hotkey_row + 1)).size(13.0)).min_size(Vec2::new(22.0, SIZE)));
                if v.pad_hints {
                    let hint = if v.playstation { "hold R2\nL2: next" } else { "hold RT\nLT: next" };
                    ui.label(RichText::new(hint).size(10.0).color(Color32::from_gray(170)));
                }
                if row.on_hover_text("Next row (F12)").clicked() {
                    action = Some(UiAction::HotkeyRow((v.hotkey_row + 1) % HOTKEY_ROWS));
                }
            });
        });
    });
    // Show what is being dragged under the pointer.
    if let (Some(key), Some(pos)) = (egui::DragAndDrop::payload::<Hotkey>(ctx), ctx.pointer_interact_pos()) {
        let tex = if key.skill { v.skill_icons.get(&(key.id as u16)) } else { v.icons.get(&key.id) };
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("drag icon")));
        paint_icon(&painter, egui::Rect::from_center_size(pos, Vec2::splat(SIZE)), tex, Color32::from_white_alpha(200));
    }
    action
}

/// Equipment slots in display order: (bits, label).
const SLOTS: [(u32, &str); 10] = [
    (0x100, "Upper Headgear"),
    (0x200, "Middle Headgear"),
    (0x001, "Lower Headgear"),
    (0x010, "Armor"),
    (0x002, "Weapon"),
    (0x020, "Shield"),
    (0x004, "Garment"),
    (0x040, "Shoes"),
    (0x008, "Accessory"),
    (0x080, "Accessory"),
];

const STAT_NAMES: [&str; 6] = ["STR", "AGI", "VIT", "INT", "DEX", "LUK"];

fn item_name(db: Option<&ItemDb>, it: &InvItem) -> String {
    let base = db.map_or_else(|| format!("Item #{}", it.id), |d| d.name(it.id, it.identified));
    let base = if it.refine > 0 { format!("+{} {base}", it.refine) } else { base };
    // Slotted equipment shows its slot count, as in the original ("Knife [3]").
    match db.and_then(|d| d.get(it.id)).map(|i| i.slots) {
        Some(n) if n > 0 && it.identified => format!("{base} [{n}]"),
        _ => base,
    }
}

/// Cards compounded into an item (forged, brewed and pet-egg items reuse the slots for other data).
fn inserted_cards(it: &InvItem) -> impl Iterator<Item = u32> + '_ {
    let special = matches!(it.cards[0], 0x00ff | 0x00fe | 0xff00);
    it.cards.iter().copied().filter(move |&c| !special && c != 0)
}

/// Item tooltip: name, inserted cards and the client's description (^RRGGBB codes stripped).
fn item_tooltip(ui: &mut egui::Ui, db: Option<&ItemDb>, it: &InvItem) {
    ui.label(RichText::new(item_name(db, it)).strong());
    let slots = db.and_then(|d| d.get(it.id)).map_or(0, |i| i.slots) as usize;
    if slots > 0 && it.is_equipment() {
        let cards: Vec<u32> = inserted_cards(it).collect();
        for i in 0..slots.max(cards.len()) {
            let text = match cards.get(i) {
                Some(&c) => db.map_or_else(|| format!("Card #{c}"), |d| d.name(c, true)),
                None => "(empty slot)".to_string(),
            };
            ui.label(RichText::new(text).color(if i < cards.len() { Color32::from_rgb(140, 200, 255) } else { Color32::GRAY }));
        }
        ui.separator();
    }
    if let Some(info) = db.and_then(|d| d.get(it.id)) {
        for line in &info.description {
            ui.label(strip_colors(line));
        }
    }
    if it.kind == ro_game::items::kind::CARD {
        ui.separator();
        ui.label(RichText::new("Double-click to insert into equipment").color(Color32::GRAY).size(11.5));
    }
}

fn strip_colors(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('^') {
        out.push_str(&rest[..i]);
        let code = rest.get(i + 1..i + 7);
        if code.is_some_and(|c| c.chars().all(|ch| ch.is_ascii_hexdigit())) {
            rest = &rest[i + 7..];
        } else {
            out.push('^');
            rest = &rest[i + 1..];
        }
    }
    out.push_str(rest);
    out
}

/// A square slot with the item icon (or a placeholder) and its amount.
fn item_slot(ui: &mut egui::Ui, icons: &HashMap<u32, egui::TextureHandle>, it: &InvItem, size: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    let hovered = resp.hovered();
    painter.rect_filled(rect, 4.0, if hovered { Color32::from_white_alpha(30) } else { Color32::from_white_alpha(12) });
    if let Some(tex) = icons.get(&it.id) {
        let s = tex.size_vec2().min(Vec2::splat(size - 6.0));
        painter.image(tex.id(), egui::Rect::from_center_size(rect.center(), s), egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
    }
    if it.amount > 1 {
        let font = egui::FontId::proportional(11.0);
        let pos = rect.right_bottom() - Vec2::new(3.0, 1.0);
        painter.text(pos + Vec2::new(1.0, 1.0), egui::Align2::RIGHT_BOTTOM, it.amount.to_string(), font.clone(), Color32::BLACK);
        painter.text(pos, egui::Align2::RIGHT_BOTTOM, it.amount.to_string(), font, Color32::WHITE);
    }
    if it.equipped != 0 {
        painter.rect_stroke(rect, 4.0, egui::Stroke::new(1.5, Color32::from_rgb(120, 200, 255)), egui::StrokeKind::Inside);
    }
    resp
}

impl Windows {
    pub fn toggle_inventory(&mut self) {
        self.inventory = !self.inventory;
    }
    pub fn toggle_equipment(&mut self) {
        self.equipment = !self.equipment;
    }
    pub fn toggle_stats(&mut self) {
        self.stats = !self.stats;
    }
    pub fn toggle_skills(&mut self) {
        self.skills = !self.skills;
    }

    /// Small always-visible buttons for the windows (top right).
    fn menu(&mut self, ctx: &egui::Context) {
        egui::Area::new(egui::Id::unique("menu")).anchor(egui::Align2::RIGHT_TOP, Vec2::new(-12.0, 12.0)).show(ctx, |ui| {
            egui::Frame::new()
                .fill(Color32::from_rgba_unmultiplied(16, 20, 28, 210))
                .corner_radius(8)
                .inner_margin(6)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for (label, key, open) in [
                            ("Inventory", "Alt+E", &mut self.inventory),
                            ("Equipment", "Alt+Q", &mut self.equipment),
                            ("Status", "Alt+A", &mut self.stats),
                            ("Skills", "Alt+S", &mut self.skills),
                            ("Party", "Alt+Z", &mut self.party),
                            ("Friends", "Alt+H", &mut self.friends),
                            ("Guild", "Alt+G", &mut self.guild),
                            ("Settings", "Alt+O", &mut self.settings),
                        ] {
                            if ui.selectable_label(*open, label).on_hover_text(key).clicked() {
                                *open = !*open;
                            }
                        }
                    });
                });
        });
    }

    pub fn show(&mut self, ctx: &egui::Context, v: &View) -> Option<UiAction> {
        self.menu(ctx);
        let mut action = hotkey_bar(ctx, v);

        // Default layout across the top, sized to the screen (windows remember moves after).
        let w = v.screen_width;
        let col = |i: f32| egui::pos2((w * 0.22 + i * 330.0).min(w - 340.0), 110.0);

        if self.inventory {
            let mut open = true;
            egui::Window::new("Inventory").open(&mut open).default_pos(col(2.0)).resizable(false).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.tab, InvTab::Items, "Items");
                    ui.selectable_value(&mut self.tab, InvTab::Equip, "Equip");
                    ui.selectable_value(&mut self.tab, InvTab::Etc, "Etc");
                });
                ui.separator();
                let shown: Vec<&&InvItem> = v
                    .items
                    .iter()
                    .filter(|it| it.equipped == 0)
                    .filter(|it| match self.tab {
                        InvTab::Items => it.is_usable(),
                        InvTab::Equip => it.is_equipment(),
                        InvTab::Etc => !it.is_usable() && !it.is_equipment(),
                    })
                    .collect();
                egui::Grid::new("inv").spacing(Vec2::splat(4.0)).show(ui, |ui| {
                    for (i, it) in shown.iter().enumerate() {
                        let resp = item_slot(ui, v.icons, it, 38.0).on_hover_ui(|ui| item_tooltip(ui, v.db, it));
                        // Usable items and equipment can go on the shortcut bar.
                        if it.is_usable() || it.is_equipment() {
                            resp.dnd_set_drag_payload(Hotkey { skill: false, id: it.id, count: 0 });
                        }
                        if resp.double_clicked() {
                            action = Some(if it.kind == ro_game::items::kind::CARD {
                                UiAction::CardTargets(it.index)
                            } else if it.is_equipment() {
                                UiAction::Equip { index: it.index, location: it.location }
                            } else {
                                UiAction::UseItem(it.index)
                            });
                        }
                        resp.context_menu(|ui| {
                            if it.is_equipment() && ui.button("Equip").clicked() {
                                action = Some(UiAction::Equip { index: it.index, location: it.location });
                                ui.close();
                            }
                            if it.is_usable() && ui.button("Use").clicked() {
                                action = Some(UiAction::UseItem(it.index));
                                ui.close();
                            }
                            if ui.button(if it.amount > 1 { "Drop all" } else { "Drop" }).clicked() {
                                action = Some(UiAction::Drop { index: it.index, amount: it.amount });
                                ui.close();
                            }
                        });
                        if i % 8 == 7 {
                            ui.end_row();
                        }
                    }
                });
                if shown.is_empty() {
                    ui.label(RichText::new("Empty").color(Color32::GRAY));
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(format!("{} z", v.zeny));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(format!("Weight {} / {}", v.weight.0 / 10, v.weight.1 / 10));
                    });
                });
            });
            self.inventory = open;
        }

        if self.equipment {
            let mut open = true;
            egui::Window::new("Equipment").open(&mut open).default_pos(col(1.0)).resizable(false).show(ctx, |ui| {
                let mut used_acc = vec![];
                egui::Grid::new("equip").num_columns(2).spacing(Vec2::new(8.0, 4.0)).show(ui, |ui| {
                    for (bits, label) in SLOTS {
                        let worn = v.items.iter().find(|it| it.equipped & bits != 0 && !used_acc.contains(&it.index));
                        if let Some(it) = worn {
                            if bits & 0x88 != 0 {
                                used_acc.push(it.index);
                            }
                            let resp = item_slot(ui, v.icons, it, 32.0).on_hover_ui(|ui| item_tooltip(ui, v.db, it));
                            if resp.double_clicked() {
                                action = Some(UiAction::Unequip(it.index));
                            }
                            ui.label(item_name(v.db, it));
                        } else {
                            ui.allocate_exact_size(Vec2::splat(32.0), egui::Sense::hover());
                            ui.label(RichText::new(label).color(Color32::GRAY));
                        }
                        ui.end_row();
                    }
                });
                ui.label(RichText::new("Double-click to take off").size(11.0).color(Color32::GRAY));
            });
            self.equipment = open;
        }

        let mut held = false;
        if self.stats {
            let mut open = true;
            egui::Window::new("Status").open(&mut open).default_pos(col(0.0)).resizable(false).show(ctx, |ui| {
                let Some(s) = v.stats else {
                    ui.label("Waiting for the server…");
                    return;
                };
                ui.columns(2, |cols| {
                    egui::Grid::new("base").num_columns(3).show(&mut cols[0], |ui| {
                        for (i, name) in STAT_NAMES.iter().enumerate() {
                            ui.label(RichText::new(*name).strong());
                            ui.label(if s.plus[i] != 0 { format!("{} + {}", s.base[i], s.plus[i]) } else { s.base[i].to_string() });
                            // The cost arrives as one byte (capped at 255), so past that the server decides.
                            let can = s.points >= s.cost[i] as u32;
                            let plus = ui.add_enabled(can, egui::Button::new("+")).on_hover_text(format!("Costs {} — hold to keep raising", s.cost[i]));
                            if plus.clicked() {
                                action = Some(UiAction::RaiseStat(i));
                            }
                            // Holding the button repeats: after 0.4 s, one point every 50 ms.
                            if can && plus.is_pointer_button_down_on() {
                                held = true;
                                let dt = ui.input(|inp| inp.stable_dt);
                                let before = self.stat_hold;
                                self.stat_hold += dt;
                                if self.stat_hold > 0.4 && ((self.stat_hold - 0.4) / 0.05) as u32 != ((before - 0.4).max(0.0) / 0.05) as u32 {
                                    action = Some(UiAction::RaiseStat(i));
                                }
                                ui.ctx().request_repaint();
                            }
                            ui.end_row();
                        }
                    });
                    egui::Grid::new("derived").num_columns(2).show(&mut cols[1], |ui| {
                        let rows = [
                            ("Atk", format!("{} + {}", s.atk.0, s.atk.1)),
                            ("Matk", format!("{} + {}", s.matk.0, s.matk.1)),
                            ("Def", format!("{} + {}", s.def.0, s.def.1)),
                            ("Mdef", format!("{} + {}", s.mdef.0, s.mdef.1)),
                            ("Hit", s.hit.to_string()),
                            ("Flee", format!("{} + {}", s.flee.0, s.flee.1)),
                            ("Critical", s.crit.to_string()),
                            // The server sends the attack delay (amotion); players read ASPD.
                            ("Aspd", ((2000 - (s.aspd.0 + s.aspd.1) as i32) / 10).to_string()),
                        ];
                        for (k, val) in rows {
                            ui.label(RichText::new(k).color(Color32::GRAY));
                            ui.label(val);
                            ui.end_row();
                        }
                    });
                });
                ui.separator();
                ui.label(format!("Status points: {}", s.points));
            });
            self.stats = open;
            if !held {
                self.stat_hold = 0.0;
            }
        }

        // Skill and item menus: one choice answers the server.
        if let Some(menu) = self.menu.clone() {
            use ro_net::{Menu, MenuReply};
            let title = match &menu {
                Menu::Items(_) => "Select an Item",
                Menu::Produce(_) | Menu::Make { .. } => "Create",
                Menu::Warp { .. } => "Warp Portal",
                Menu::AutoSpell(_) => "Auto Spell",
                Menu::Identify(_) => "Item Appraisal",
                Menu::Skill { .. } => "Select a Skill",
                Menu::Egg(_) => "Hatch an Egg",
            };
            let skill_name = |id: u16| v.skill_info.and_then(|i| i.internal(id)).map_or_else(|| format!("Skill {id}"), |n| (v.skill_names)(n));
            let item_name_of = |id: u32| v.db.map_or_else(|| format!("Item #{id}"), |d| d.name(id, true));
            let inv = |index: u16| v.items.iter().find(|it| it.index == index).copied();
            let mut chosen = None;
            let mut open = true;
            egui::Window::new(title).open(&mut open).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
                egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
                    // An item row: icon (when loaded) and a button with its name.
                    let item_row = |ui: &mut egui::Ui, id: u32, label: String| {
                        ui.horizontal(|ui| {
                            let (rect, _) = ui.allocate_exact_size(Vec2::splat(26.0), egui::Sense::hover());
                            paint_icon(ui.painter(), rect, v.icons.get(&id), Color32::WHITE);
                            ui.button(label).clicked()
                        })
                        .inner
                    };
                    match &menu {
                        Menu::Items(ids) | Menu::Make { items: ids, .. } => {
                            for &id in ids {
                                if item_row(ui, id, item_name_of(id)) {
                                    chosen = Some(match &menu {
                                        Menu::Make { kind, .. } => MenuReply::Make { kind: *kind, item: id },
                                        _ => MenuReply::Item(id),
                                    });
                                }
                            }
                        }
                        Menu::Produce(list) => {
                            for (id, materials) in list {
                                let extra: Vec<String> = materials.iter().filter(|&&m| m != 0).map(|&m| item_name_of(m)).collect();
                                let label = if extra.is_empty() { item_name_of(*id) } else { format!("{}  ({})", item_name_of(*id), extra.join(", ")) };
                                if item_row(ui, *id, label) {
                                    chosen = Some(MenuReply::Produce { item: *id, materials: *materials });
                                }
                            }
                        }
                        Menu::Identify(list) | Menu::Egg(list) => {
                            for &index in list {
                                let Some(it) = inv(index) else { continue };
                                if item_row(ui, it.id, item_name(v.db, it)) {
                                    chosen = Some(if matches!(menu, Menu::Egg(_)) { MenuReply::Egg(index) } else { MenuReply::Identify(index) });
                                }
                            }
                        }
                        Menu::Warp { skill, maps } => {
                            for map in maps {
                                if ui.button(map.trim_end_matches(".gat")).clicked() {
                                    chosen = Some(MenuReply::Warp { skill: *skill, map: map.clone() });
                                }
                            }
                        }
                        Menu::AutoSpell(skills) => {
                            for &id in skills {
                                if ui.button(skill_name(id as u16)).clicked() {
                                    chosen = Some(MenuReply::AutoSpell(id));
                                }
                            }
                        }
                        Menu::Skill { flag, skills } => {
                            for &id in skills {
                                if ui.button(skill_name(id)).clicked() {
                                    chosen = Some(MenuReply::Skill { flag: *flag, skill: id });
                                }
                            }
                        }
                    }
                });
            });
            if let Some(reply) = chosen {
                action = Some(UiAction::Menu(reply));
                self.menu = None;
            } else if !open {
                // Closing the portal menu tells the server to drop the cast.
                if let Menu::Warp { skill, .. } = menu {
                    action = Some(UiAction::Menu(MenuReply::Warp { skill, map: "cancel".into() }));
                }
                self.menu = None;
            }
        }

        // Inserting a card: pick the equipment the server says can take it.
        if let Some((card, equips)) = self.card_pick.clone() {
            let mut open = true;
            egui::Window::new("Insert Card").open(&mut open).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
                let card_item = v.items.iter().find(|it| it.index == card);
                if let Some(c) = card_item {
                    ui.label(RichText::new(item_name(v.db, c)).strong());
                }
                ui.separator();
                if equips.is_empty() {
                    ui.label(RichText::new("No equipment can take this card.").color(Color32::GRAY));
                }
                for e in &equips {
                    let Some(it) = v.items.iter().find(|it| it.index == *e) else { continue };
                    ui.horizontal(|ui| {
                        item_slot(ui, v.icons, it, 32.0).on_hover_ui(|ui| item_tooltip(ui, v.db, it));
                        if ui.button(item_name(v.db, it)).clicked() {
                            action = Some(UiAction::InsertCard { card, equip: *e });
                            self.card_pick = None;
                        }
                    });
                }
            });
            if !open {
                self.card_pick = None;
            }
        }

        if self.skills {
            let mut open = true;
            egui::Window::new("Skills").open(&mut open).default_pos(egui::pos2(col(0.0).x, 420.0)).resizable(false).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!("Skill points: {}", v.skill_points));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.selectable_value(&mut self.skill_list, true, "List");
                        ui.selectable_value(&mut self.skill_list, false, "Tree");
                    });
                });
                ui.separator();
                let tabs = v.skill_tree.map(|t| t.tabs(v.job)).unwrap_or_default();
                if self.skill_list || tabs.is_empty() {
                    if v.skills.is_empty() {
                        ui.label(RichText::new("No skills").color(Color32::GRAY));
                    }
                    // Long skill lists (3rd and 4th jobs) scroll instead of running off the screen.
                    egui::ScrollArea::vertical().max_height(420.0).auto_shrink([false, true]).show(ui, |ui| {
                    egui::Grid::new("skills").num_columns(4).spacing(Vec2::new(8.0, 4.0)).show(ui, |ui| {
                        for s in v.skills {
                            let name = (v.skill_names)(&s.name);
                            let icon = skill_icon(ui, v, s.id, Some(s), 24.0);
                            let label = ui.add(egui::Label::new(RichText::new(&name)).sense(egui::Sense::click()));
                            if s.level > 0 && s.target != 0 && (icon.double_clicked() || label.double_clicked()) {
                                action = Some(UiAction::UseSkill(s.id));
                            }
                            icon.on_hover_ui(|ui| skill_tooltip(ui, v, &s.name, Some(s)));
                            label.on_hover_ui(|ui| skill_tooltip(ui, v, &s.name, Some(s)));
                            ui.label(format!("Lv {}", s.level));
                            if ui.add_enabled(s.upgradable && v.skill_points > 0, egui::Button::new("+")).clicked() {
                                action = Some(UiAction::SkillUp(s.id));
                            }
                            ui.end_row();
                        }
                    });
                    });
                } else {
                    // One tab per job tier, as in the original skill tree window.
                    let mut tab = self.skill_tab.unwrap_or(usize::MAX).min(tabs.len() - 1);
                    ui.horizontal(|ui| {
                        for (i, (job, _)) in tabs.iter().enumerate() {
                            ui.selectable_value(&mut tab, i, crate::jobs::title(*job));
                        }
                    });
                    ui.add_space(4.0);
                    self.skill_tab = Some(tab);
                    let grid = tabs[tab].1;
                    let rows = grid.iter().map(|(slot, _)| slot / crate::skilltree::COLUMNS + 1).max().unwrap_or(1);
                    const CELL: f32 = 58.0;
                    let cols = crate::skilltree::COLUMNS;
                    let (area, _) = ui.allocate_exact_size(Vec2::new(cols as f32 * CELL, rows as f32 * CELL), egui::Sense::hover());
                    for (slot, internal) in grid {
                        let at = area.min + Vec2::new((slot % cols) as f32 * CELL, (slot / cols) as f32 * CELL);
                        let cell = egui::Rect::from_min_size(at, Vec2::splat(CELL)).shrink(2.0);
                        let skill = v.skills.iter().find(|s| &s.name == internal);
                        let id = skill.map(|s| s.id).or_else(|| v.skill_info.and_then(|i| i.id_of(internal)));
                        let lv = skill.map_or(0, |s| s.level);
                        ui.painter().rect_filled(cell, 4.0, if lv > 0 { Color32::from_gray(48) } else { Color32::from_gray(30) });
                        let icon_rect = egui::Rect::from_center_size(cell.center_top() + Vec2::new(0.0, 19.0), Vec2::splat(32.0));
                        let icon = ui.scope_builder(egui::UiBuilder::new().max_rect(icon_rect), |ui| skill_icon(ui, v, id.unwrap_or(0), skill, 32.0)).inner;
                        if let Some(s) = skill.filter(|s| s.level > 0 && s.target != 0) {
                            if icon.double_clicked() {
                                action = Some(UiAction::UseSkill(s.id));
                            }
                        }
                        icon.on_hover_ui(|ui| skill_tooltip(ui, v, internal, skill));
                        let color = if lv > 0 { Color32::WHITE } else { Color32::GRAY };
                        ui.painter().text(cell.left_bottom() + Vec2::new(5.0, -3.0), egui::Align2::LEFT_BOTTOM, format!("{lv}"), egui::FontId::proportional(11.0), color);
                        if let Some(s) = skill.filter(|s| s.upgradable && v.skill_points > 0) {
                            let plus = egui::Rect::from_min_size(cell.right_bottom() - Vec2::new(18.0, 16.0), Vec2::new(16.0, 14.0));
                            if ui.put(plus, egui::Button::new(RichText::new("+").size(10.0))).clicked() {
                                action = Some(UiAction::SkillUp(s.id));
                            }
                        }
                    }
                }
            });
            self.skills = open;
        }
        action
    }
}

/// A skill icon that can be dragged to the shortcut bar; greyed out until learned.
fn skill_icon(ui: &mut egui::Ui, v: &View, id: u16, skill: Option<&Skill>, size: f32) -> egui::Response {
    let learned = skill.is_some_and(|s| s.level > 0);
    let resp = match v.skill_icons.get(&id) {
        Some(t) => ui.add(egui::Image::new((t.id(), Vec2::splat(size))).tint(if learned { Color32::WHITE } else { Color32::from_gray(90) }).sense(egui::Sense::click_and_drag())),
        None => ui.allocate_exact_size(Vec2::splat(size), egui::Sense::click()).1,
    };
    if let Some(s) = skill.filter(|s| s.level > 0 && s.target != 0) {
        resp.dnd_set_drag_payload(Hotkey { skill: true, id: s.id as u32, count: s.level as i16 });
    }
    resp
}

/// Hovering a skill shows its name, numbers and description.
fn skill_tooltip(ui: &mut egui::Ui, v: &View, internal: &str, skill: Option<&Skill>) {
    ui.set_max_width(360.0);
    ui.label(RichText::new((v.skill_names)(internal)).strong());
    match skill {
        Some(s) => ui.label(RichText::new(format!("Lv {}   SP {}   Range {}", s.level, s.sp, s.range)).color(Color32::GRAY)),
        None => ui.label(RichText::new("Not yet available").color(Color32::GRAY)),
    };
    if let Some(lines) = v.skill_info.and_then(|i| i.description(internal)) {
        ui.separator();
        for line in lines {
            ui.label(crate::hud::npc_text(line, 12.5));
        }
    }
    if skill.is_some_and(|s| s.level > 0 && s.target != 0) {
        ui.separator();
        ui.label(RichText::new("Double-click to use, or drag to the shortcut bar").color(Color32::GRAY).size(11.5));
    }
}

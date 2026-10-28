//! Game windows: inventory, equipment, character stats and skills. They read the player's
//! state and return the actions the player chose; main.rs turns those into requests.

use std::collections::HashMap;

use egui::{Color32, RichText, Vec2};
use ro_game::items::{InvItem, ItemDb};
use ro_net::{Skill, Stats};

/// What the player asked for through a window.
#[derive(Debug, Clone, PartialEq)]
pub enum UiAction {
    UseItem(u16),
    Equip { index: u16, location: u32 },
    Unequip(u16),
    Drop { index: u16, amount: u16 },
    /// Raise a base stat; 0..6 = str, agi, vit, int, dex, luk.
    RaiseStat(usize),
    SkillUp(u16),
    UseSkill(u16),
}

#[derive(Default)]
pub struct Windows {
    pub inventory: bool,
    pub equipment: bool,
    pub stats: bool,
    pub skills: bool,
    pub party: bool,
    pub friends: bool,
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
    pub skill_points: i64,
    pub weight: (i64, i64),
    pub zeny: i64,
    /// Screen width in UI points, for the default window layout.
    pub screen_width: f32,
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
    if it.refine > 0 { format!("+{} {base}", it.refine) } else { base }
}

/// Item tooltip: name and the client's description (with ^RRGGBB codes stripped).
fn item_tooltip(ui: &mut egui::Ui, db: Option<&ItemDb>, it: &InvItem) {
    ui.label(RichText::new(item_name(db, it)).strong());
    if let Some(info) = db.and_then(|d| d.get(it.id)) {
        for line in &info.description {
            ui.label(strip_colors(line));
        }
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
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::click());
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
        let mut action = None;
        self.menu(ctx);

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
                        if resp.double_clicked() {
                            action = Some(if it.is_equipment() {
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
                            ui.label(s.base[i].to_string());
                            let can = s.points >= s.cost[i] as u16 && s.base[i] < 130;
                            if ui.add_enabled(can, egui::Button::new("+")).on_hover_text(format!("Costs {}", s.cost[i])).clicked() {
                                action = Some(UiAction::RaiseStat(i));
                            }
                            ui.end_row();
                        }
                    });
                    egui::Grid::new("derived").num_columns(2).show(&mut cols[1], |ui| {
                        let rows = [
                            ("Atk", format!("{} + {}", s.atk.0, s.atk.1)),
                            ("Matk", format!("{} ~ {}", s.matk.0, s.matk.1)),
                            ("Def", format!("{} + {}", s.def.0, s.def.1)),
                            ("Mdef", format!("{} + {}", s.mdef.0, s.mdef.1)),
                            ("Hit", s.hit.to_string()),
                            ("Flee", format!("{} + {}", s.flee.0, s.flee.1)),
                            ("Critical", s.crit.to_string()),
                            ("Aspd", (s.aspd.0 + s.aspd.1).to_string()),
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
        }

        if self.skills {
            let mut open = true;
            egui::Window::new("Skills").open(&mut open).default_pos(egui::pos2(col(0.0).x, 420.0)).resizable(false).show(ctx, |ui| {
                ui.label(format!("Skill points: {}", v.skill_points));
                ui.separator();
                if v.skills.is_empty() {
                    ui.label(RichText::new("No skills").color(Color32::GRAY));
                }
                egui::Grid::new("skills").num_columns(4).spacing(Vec2::new(8.0, 4.0)).show(ui, |ui| {
                    for s in v.skills {
                        match v.skill_icons.get(&s.id) {
                            Some(t) => {
                                ui.image((t.id(), Vec2::splat(24.0)));
                            }
                            None => {
                                ui.allocate_exact_size(Vec2::splat(24.0), egui::Sense::hover());
                            }
                        }
                        let name = (v.skill_names)(&s.name);
                        let usable = s.level > 0 && s.target != 0;
                        let label = ui.add(egui::Label::new(RichText::new(&name)).sense(egui::Sense::click()));
                        if usable && label.on_hover_text("Double-click to use").double_clicked() {
                            action = Some(UiAction::UseSkill(s.id));
                        }
                        ui.label(format!("Lv {}", s.level));
                        if ui.add_enabled(s.upgradable && v.skill_points > 0, egui::Button::new("+")).clicked() {
                            action = Some(UiAction::SkillUp(s.id));
                        }
                        ui.end_row();
                    }
                });
            });
            self.skills = open;
        }
        action
    }
}

//! The companion window (Alt+R): our pet (feed, perform, return to egg, accessory,
//! rename), homunculus (stats, feed, skills, attack/return, rename) and mercenary
//! (stats, skills, attack/return, dismiss).

use egui::{Color32, RichText};
use ro_net::{CompanionRequest, CompanionSkill, HomunInfo, MercInfo, PetInfo};

/// What the window asks the app to do.
pub enum CompanionAction {
    Send(CompanionRequest),
    /// Use a companion skill (on the companion itself).
    Skill { id: u16, level: u16, on: u32 },
    /// Raise a homunculus skill.
    SkillUp(u16),
}

#[derive(Clone, Copy, PartialEq, Default)]
enum Tab {
    #[default]
    Pet,
    Homun,
    Merc,
    Elem,
}

/// Our summoned elemental (ZC_EL_INIT / ZC_EL_PAR_CHANGE).
#[derive(Default, Clone)]
pub struct Elemental {
    pub id: u32,
    pub name: String,
    pub hp: u32,
    pub max_hp: u32,
    pub sp: u32,
    pub max_sp: u32,
}

#[derive(Default)]
pub struct Companions {
    pub open: bool,
    tab: Tab,
    pub pet: Option<PetInfo>,
    pub pet_id: u32,
    pub homun: Option<HomunInfo>,
    pub homun_id: u32,
    pub homun_skills: Vec<CompanionSkill>,
    pub merc: Option<MercInfo>,
    pub merc_skills: Vec<CompanionSkill>,
    pub elem: Option<Elemental>,
    rename: String,
    /// Evolutions open to our pet: (evolved egg, its name, materials as (name, held, needed)).
    pub evolutions: Vec<(u32, String, Vec<(String, u32, u32)>)>,
}

/// The pet's hunger in words, as the original shows it.
fn hunger_word(h: u16) -> &'static str {
    match h {
        0..=10 => "Very Hungry",
        11..=25 => "Hungry",
        26..=75 => "Neutral",
        76..=90 => "Satisfied",
        _ => "Stuffed",
    }
}

/// Intimacy (0-1000) in words.
fn intimacy_word(i: u16) -> &'static str {
    match i {
        0..=99 => "Awkward",
        100..=249 => "Shy",
        250..=749 => "Neutral",
        750..=909 => "Cordial",
        _ => "Loyal",
    }
}

fn bar(ui: &mut egui::Ui, label: &str, v: u32, max: u32, color: Color32) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).strong());
        let frac = if max > 0 { v as f32 / max as f32 } else { 0.0 };
        ui.add(egui::ProgressBar::new(frac.clamp(0.0, 1.0)).desired_width(200.0).fill(color).text(format!("{v} / {max}")));
    });
}

fn stats(ui: &mut egui::Ui, id: &str, rows: &[(&str, String)]) {
    egui::Grid::new(id).num_columns(4).spacing([14.0, 4.0]).show(ui, |ui| {
        for pair in rows.chunks(2) {
            for (k, v) in pair {
                ui.label(RichText::new(*k).color(crate::classic::ink(Color32::GRAY)));
                ui.label(v);
            }
            ui.end_row();
        }
    });
}

impl Companions {
    /// Test hook: 0 pet, 1 homunculus, 2 mercenary, 3 elemental.
    pub fn select_tab(&mut self, n: u8) {
        self.tab = [Tab::Pet, Tab::Homun, Tab::Merc, Tab::Elem][(n as usize).min(3)];
        self.open = true;
    }

    /// `target`: our current attack target, for "Attack".
    pub fn show(&mut self, ctx: &egui::Context, target: Option<u32>) -> Option<CompanionAction> {
        if !self.open {
            return None;
        }
        let mut out = None;
        let mut open = true;
        crate::classic::show(ctx, egui::Window::new("Companions").open(&mut open).resizable(false).default_width(360.0).default_pos(egui::pos2(420.0, 160.0)), "Companions", true, |ui| {
            ui.horizontal(|ui| {
                for (t, label, has) in [(Tab::Pet, "Pet", self.pet.is_some()), (Tab::Homun, "Homunculus", self.homun.is_some()), (Tab::Merc, "Mercenary", self.merc.is_some()), (Tab::Elem, "Elemental", self.elem.is_some())] {
                    let text = if has { RichText::new(label) } else { RichText::new(label).color(crate::classic::ink(Color32::GRAY)) };
                    if ui.selectable_label(self.tab == t, text).clicked() {
                        self.tab = t;
                        self.rename.clear();
                    }
                }
            });
            ui.separator();
            match self.tab {
                Tab::Pet => match &self.pet {
                    None => {
                        ui.label(RichText::new("No pet out. Use a Pet Incubator on an egg to hatch one.").color(crate::classic::ink(Color32::GRAY)));
                    }
                    Some(p) => {
                        ui.label(RichText::new(format!("{}  (Lv {})", p.name, p.level)).strong().size(15.0));
                        bar(ui, "Hunger  ", p.hunger as u32, 100, Color32::from_rgb(230, 160, 70));
                        ui.label(format!("Hunger: {}", hunger_word(p.hunger)));
                        bar(ui, "Intimacy", p.intimacy as u32, 1000, Color32::from_rgb(230, 110, 150));
                        ui.label(format!("Intimacy: {}", intimacy_word(p.intimacy)));
                        if p.accessory != 0 {
                            ui.label(format!("Wearing accessory #{}", p.accessory));
                        }
                        ui.add_space(4.0);
                        ui.horizontal_wrapped(|ui| {
                            for (label, cmd) in [("Feed", 1), ("Performance", 2), ("Return to egg", 3)] {
                                if ui.button(label).clicked() {
                                    out = Some(CompanionAction::Send(CompanionRequest::PetMenu(cmd)));
                                }
                            }
                            if p.accessory != 0 && ui.button("Remove accessory").clicked() {
                                out = Some(CompanionAction::Send(CompanionRequest::PetMenu(4)));
                            }
                        });
                        if !p.renamed {
                            ui.horizontal(|ui| {
                                ui.add(egui::TextEdit::singleline(&mut self.rename).hint_text("new name").desired_width(140.0));
                                if ui.add_enabled(!self.rename.trim().is_empty(), egui::Button::new("Rename")).clicked() {
                                    out = Some(CompanionAction::Send(CompanionRequest::PetRename(self.rename.trim().to_string())));
                                    self.rename.clear();
                                }
                            });
                        }
                        // Evolution: open once the pet is Loyal, for the listed materials.
                        if !self.evolutions.is_empty() {
                            ui.separator();
                            ui.label(RichText::new("Pet Evolution").strong());
                            let loyal = p.intimacy >= 910;
                            if !loyal {
                                ui.label(RichText::new("Pet can only be evolved when intimacy is Loyal.").color(crate::classic::ink(Color32::GRAY)));
                            }
                            for (egg, name, mats) in &self.evolutions {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(name).color(crate::classic::ink(Color32::from_rgb(255, 220, 120))));
                                    let ready = loyal && p.accessory == 0 && mats.iter().all(|(_, have, need)| have >= need);
                                    if ui.add_enabled(ready, egui::Button::new("Evolve")).clicked() {
                                        out = Some(CompanionAction::Send(CompanionRequest::PetEvolve(*egg)));
                                    }
                                });
                                for (m, have, need) in mats {
                                    let color = if have >= need { Color32::from_gray(200) } else { Color32::from_rgb(230, 110, 110) };
                                    ui.label(RichText::new(format!("    {m}  {have} / {need}")).color(crate::classic::ink(color)));
                                }
                            }
                        }
                    }
                },
                Tab::Homun => match &self.homun {
                    None => {
                        ui.label(RichText::new("No homunculus. Alchemists create one with an Embryo.").color(crate::classic::ink(Color32::GRAY)));
                    }
                    Some(h) => {
                        let state = if !h.alive { "  (dead)" } else if h.resting { "  (resting)" } else { "" };
                        ui.label(RichText::new(format!("{}  (Lv {}){state}", h.name, h.level)).strong().size(15.0));
                        bar(ui, "HP ", h.hp, h.max_hp, Color32::from_rgb(70, 190, 90));
                        bar(ui, "SP ", h.sp, h.max_sp, Color32::from_rgb(70, 120, 220));
                        let exp = if h.exp_next > 0 { format!("{:.1}%", h.exp as f64 * 100.0 / h.exp_next as f64) } else { "max".into() };
                        stats(
                            ui,
                            "hom_stats",
                            &[
                                ("Atk", h.atk.to_string()),
                                ("Matk", h.matk.to_string()),
                                ("Hit", h.hit.to_string()),
                                ("Critical", h.crit.to_string()),
                                ("Def", h.def.to_string()),
                                ("Mdef", h.mdef.to_string()),
                                ("Flee", h.flee.to_string()),
                                ("Aspd", ((2000 - h.aspd as i32) / 10).to_string()),
                                ("Hunger", format!("{} ({})", h.hunger, hunger_word(h.hunger))),
                                ("Intimacy", h.intimacy.to_string()),
                                ("Exp", exp),
                                ("Skill points", h.skill_points.to_string()),
                            ],
                        );
                        ui.add_space(4.0);
                        let id = self.homun_id;
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("Feed").clicked() {
                                out = Some(CompanionAction::Send(CompanionRequest::HomMenu(1)));
                            }
                            if ui.add_enabled(target.is_some() && id != 0, egui::Button::new("Attack target")).clicked() {
                                out = target.map(|t| CompanionAction::Send(CompanionRequest::Attack { id, target: t }));
                            }
                            if ui.add_enabled(id != 0, egui::Button::new("Return to me")).clicked() {
                                out = Some(CompanionAction::Send(CompanionRequest::Return(id)));
                            }
                        });
                        if !h.renamed {
                            ui.horizontal(|ui| {
                                ui.add(egui::TextEdit::singleline(&mut self.rename).hint_text("new name").desired_width(140.0));
                                if ui.add_enabled(!self.rename.trim().is_empty(), egui::Button::new("Rename")).clicked() {
                                    out = Some(CompanionAction::Send(CompanionRequest::HomRename(self.rename.trim().to_string())));
                                    self.rename.clear();
                                }
                            });
                        }
                        if let Some(a) = skills(ui, &self.homun_skills, id, h.skill_points > 0) {
                            out = Some(a);
                        }
                    }
                },
                Tab::Elem => match &self.elem {
                    None => {
                        ui.label(RichText::new("No elemental. Sorcerers summon one with their Summon skills.").color(crate::classic::ink(Color32::GRAY)));
                    }
                    Some(e) => {
                        ui.label(RichText::new(if e.name.is_empty() { "Elemental" } else { &e.name }).strong().size(15.0));
                        bar(ui, "HP ", e.hp, e.max_hp, Color32::from_rgb(70, 190, 90));
                        bar(ui, "SP ", e.sp, e.max_sp, Color32::from_rgb(70, 120, 220));
                    }
                },
                Tab::Merc => match &self.merc {
                    None => {
                        ui.label(RichText::new("No mercenary. Hire one with a mercenary scroll.").color(crate::classic::ink(Color32::GRAY)));
                    }
                    Some(m) => {
                        ui.label(RichText::new(format!("{}  (Lv {})", m.name, m.level)).strong().size(15.0));
                        bar(ui, "HP ", m.hp, m.max_hp, Color32::from_rgb(70, 190, 90));
                        bar(ui, "SP ", m.sp, m.max_sp, Color32::from_rgb(70, 120, 220));
                        stats(
                            ui,
                            "merc_stats",
                            &[
                                ("Atk", m.atk.to_string()),
                                ("Matk", m.matk.to_string()),
                                ("Hit", m.hit.to_string()),
                                ("Critical", m.crit.to_string()),
                                ("Def", m.def.to_string()),
                                ("Mdef", m.mdef.to_string()),
                                ("Flee", m.flee.to_string()),
                                ("Aspd", ((2000 - m.aspd as i32) / 10).to_string()),
                                ("Loyalty", m.faith.to_string()),
                                ("Kills", m.kills.to_string()),
                            ],
                        );
                        ui.add_space(4.0);
                        let id = m.id;
                        ui.horizontal_wrapped(|ui| {
                            if ui.add_enabled(target.is_some(), egui::Button::new("Attack target")).clicked() {
                                out = target.map(|t| CompanionAction::Send(CompanionRequest::Attack { id, target: t }));
                            }
                            if ui.button("Return to me").clicked() {
                                out = Some(CompanionAction::Send(CompanionRequest::Return(id)));
                            }
                            if ui.button("Dismiss").clicked() {
                                out = Some(CompanionAction::Send(CompanionRequest::MercMenu(2)));
                            }
                        });
                        if let Some(a) = skills(ui, &self.merc_skills, id, false) {
                            out = Some(a);
                        }
                    }
                },
            }
        });
        if !open {
            self.open = false;
        }
        out
    }
}

/// The companion's skill list: double-click (or Use) to cast; + raises it.
fn skills(ui: &mut egui::Ui, list: &[CompanionSkill], owner: u32, points: bool) -> Option<CompanionAction> {
    if list.is_empty() {
        return None;
    }
    let mut out = None;
    ui.separator();
    ui.label(RichText::new("Skills").strong());
    egui::Grid::new(("comp_skills", owner)).num_columns(4).spacing([10.0, 3.0]).show(ui, |ui| {
        for s in list {
            ui.label(s.name.replace('_', " "));
            ui.label(format!("Lv {}", s.level));
            if ui.add_enabled(s.level > 0, egui::Button::new("Use")).on_hover_text(format!("SP {}", s.sp)).clicked() {
                out = Some(CompanionAction::Skill { id: s.id, level: s.level, on: owner });
            }
            if s.upgradable && points {
                if ui.small_button("+").clicked() {
                    out = Some(CompanionAction::SkillUp(s.id));
                }
            } else {
                ui.label("");
            }
            ui.end_row();
        }
    });
    out
}

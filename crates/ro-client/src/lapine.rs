//! Laphine synthesis and upgrade (ZC_RANDOM_COMBINE_ITEM_UI_OPEN / ZC_RANDOM_UPGRADE_ITEM_UI_OPEN):
//! an item script opens the window for its trigger item. The server only names the trigger;
//! what it accepts comes from the official datainfo\lapineddukddakbox.lub and
//! lapineupgradebox.lub, as in the original client.

use std::collections::HashMap;

use egui::{Color32, RichText, Vec2};
use ro_formats::lub::{self, Value};
use ro_game::items::{InvItem, ItemDb};

/// A synthesis recipe: hand in `need` stacks out of `sources` (item, amount each).
#[derive(Debug, Clone, Default)]
pub struct Synthesis {
    pub need: usize,
    pub refine: (u8, u8),
    pub sources: Vec<(u32, u16)>,
    pub hint: String,
}

/// An upgrade: one of `targets` is re-rolled.
#[derive(Debug, Clone, Default)]
pub struct Upgrade {
    pub refine: (u8, u8),
    pub min_options: u32,
    pub no_cards: bool,
    pub targets: Vec<u32>,
}

#[derive(Default)]
pub struct Tables {
    pub synthesis: HashMap<u32, Synthesis>,
    pub upgrade: HashMap<u32, Upgrade>,
}

impl Tables {
    pub fn load(vfs: &ro_vfs::Vfs) -> Self {
        const DIR: &str = r"data\luafiles514\lua files\datainfo";
        // Every layer's copy, first wins per trigger: kRO's data carries recipes iRO's lacks.
        let eval = |p: &str| vfs.read_all(&format!(r"{DIR}\{p}.lub")).unwrap_or_default().into_iter().filter_map(|d| lub::eval(&d).ok()).collect::<Vec<_>>();
        let mut t = Self::default();
        let num = |c: &lub::Chunk, e: &Value, k: &str| c.field(e, k).and_then(Value::as_number).unwrap_or(0.0);
        for c in eval("lapineddukddakbox") {
            for (_, e) in c.globals.get("tblLapineDdukddakBox").and_then(|g| c.field(g, "sources")).map(|s| c.entries(s)).unwrap_or_default() {
                let sources = c.field(e, "SourceItems").map(|s| c.entries(s).iter().filter_map(|(_, i)| Some((c.field_index(i, 3)?.as_number()? as u32, c.field_index(i, 2)?.as_number()? as u16))).collect()).unwrap_or_default();
                let hint = c.field(e, "NeedSource_String").and_then(Value::as_str).unwrap_or_default().to_string();
                let recipe = Synthesis { need: num(&c, e, "NeedCount") as usize, refine: (num(&c, e, "NeedRefineMin") as u8, num(&c, e, "NeedRefineMax") as u8), sources, hint };
                t.synthesis.entry(num(&c, e, "ItemID") as u32).or_insert(recipe);
            }
        }
        for c in eval("lapineupgradebox") {
            for (_, e) in c.globals.get("tblLapineUpgradeBox").and_then(|g| c.field(g, "targets")).map(|s| c.entries(s)).unwrap_or_default() {
                let targets = c.field(e, "TargetItems").map(|s| c.entries(s).iter().filter_map(|(_, i)| Some(c.field_index(i, 2)?.as_number()? as u32)).collect()).unwrap_or_default();
                let no_cards = matches!(c.field(e, "NotSocketEnchantItem"), Some(Value::Bool(true)));
                let up = Upgrade { refine: (num(&c, e, "NeedRefineMin") as u8, num(&c, e, "NeedRefineMax") as u8), min_options: num(&c, e, "NeedOptionNumMin") as u32, no_cards, targets };
                t.upgrade.entry(num(&c, e, "ItemID") as u32).or_insert(up);
            }
        }
        tracing::info!(synthesis = t.synthesis.len(), upgrade = t.upgrade.len(), "Laphine tables");
        t
    }
}

pub enum LapineAction {
    Synthesize { trigger: u32, items: Vec<(u16, u16)> },
    Upgrade { trigger: u32, index: u16 },
    Close { upgrade: bool },
}

#[derive(Default)]
pub struct Lapine {
    tables: Option<Tables>,
    /// The open window: (upgrade?, trigger item).
    pub open: Option<(bool, u32)>,
    /// Picked inventory indices.
    picked: Vec<u16>,
    pub status: String,
}

impl Lapine {
    pub fn open(&mut self, vfs: Option<&ro_vfs::Vfs>, upgrade: bool, trigger: u32) {
        if self.tables.is_none() {
            self.tables = Some(vfs.map(Tables::load).unwrap_or_default());
        }
        self.open = Some((upgrade, trigger));
        self.picked.clear();
        self.status.clear();
    }

    /// The server's answer; on success the window closes (the server keeps it open until told).
    pub fn result(&mut self, upgrade: bool, result: u16) -> Option<LapineAction> {
        self.picked.clear();
        if result == 0 {
            self.status = if upgrade { "Upgrade complete." } else { "Synthesis complete." }.into();
            self.open = None;
            return Some(LapineAction::Close { upgrade });
        }
        self.status = match (upgrade, result) {
            (false, 5) => "Not enough of a required item.",
            (false, _) => "Those items can't be used for this.",
            (true, _) => "The upgrade failed: that item can't be upgraded with this.",
        }
        .into();
        None
    }

    fn synth_ok(r: &Synthesis, it: &InvItem) -> Option<u16> {
        let (_, n) = r.sources.iter().find(|(id, _)| *id == it.id)?;
        (it.equipped == 0 && it.amount >= *n && (r.refine.0..=r.refine.1).contains(&it.refine)).then_some(*n)
    }

    fn upgrade_ok(u: &Upgrade, it: &InvItem) -> bool {
        u.targets.contains(&it.id) && it.equipped == 0 && it.identified && (u.refine.0..=u.refine.1).contains(&it.refine) && !(u.no_cards && it.cards.iter().any(|&c| c != 0))
    }

    /// Test hook: hand in / upgrade whatever qualifies.
    pub fn test_go(&self, items: &[&InvItem]) -> Option<LapineAction> {
        let (upgrade, trigger) = self.open?;
        let t = self.tables.as_ref()?;
        if upgrade {
            let u = t.upgrade.get(&trigger)?;
            let it = items.iter().find(|i| Self::upgrade_ok(u, i))?;
            return Some(LapineAction::Upgrade { trigger, index: it.index });
        }
        let r = t.synthesis.get(&trigger)?;
        let picks: Vec<(u16, u16)> = items.iter().filter_map(|i| Some((i.index, Self::synth_ok(r, i)?))).take(r.need).collect();
        (picks.len() == r.need).then_some(LapineAction::Synthesize { trigger, items: picks })
    }

    pub fn show(&mut self, ctx: &egui::Context, items: &[&InvItem], db: Option<&ItemDb>, icons: &HashMap<u32, egui::TextureHandle>) -> Option<LapineAction> {
        let (upgrade, trigger) = self.open?;
        let mut out = None;
        let name = |it: &InvItem| {
            let n = db.map_or_else(|| format!("Item #{}", it.id), |d| d.name(it.id, it.identified));
            if it.refine > 0 { format!("+{} {n}", it.refine) } else { n }
        };
        let title = if upgrade { "Laphine Upgrade" } else { "Laphine Synthesis" };
        let mut open = true;
        let tables = self.tables.as_ref();
        let picked = &mut self.picked;
        let status = &mut self.status;
        crate::classic::show(ctx, egui::Window::new(title).open(&mut open).resizable(false).default_width(380.0), title, true, |ui| {
            ui.label(RichText::new(db.map_or_else(|| format!("Item #{trigger}"), |d| d.name(trigger, true))).strong());
            let row = |ui: &mut egui::Ui, it: &InvItem, on: bool, text: String| -> bool {
                ui.horizontal(|ui| {
                    if let Some(t) = icons.get(&it.id) {
                        ui.image((t.id(), Vec2::splat(22.0)));
                    }
                    ui.selectable_label(on, text).clicked()
                })
                .inner
            };
            if upgrade {
                let Some(u) = tables.and_then(|t| t.upgrade.get(&trigger)) else {
                    ui.label("This client has no Laphine data for that item.");
                    return;
                };
                let options = if u.min_options > 0 { format!(", at least {} random options", u.min_options) } else { String::new() };
                ui.label(RichText::new(format!("Choose the item to upgrade (refine +{} to +{}{}{options}).", u.refine.0, u.refine.1, if u.no_cards { ", no cards" } else { "" })).color(crate::classic::ink(Color32::GRAY)));
                let mine: Vec<&&InvItem> = items.iter().filter(|i| Self::upgrade_ok(u, i)).collect();
                if mine.is_empty() {
                    ui.label(format!("You carry nothing this can upgrade ({} kinds).", u.targets.len()));
                }
                for it in mine {
                    if row(ui, it, picked.first() == Some(&it.index), name(it)) {
                        *picked = vec![it.index];
                    }
                }
                if ui.add_enabled(picked.len() == 1, egui::Button::new("Upgrade")).clicked() {
                    out = Some(LapineAction::Upgrade { trigger, index: picked[0] });
                    *status = "Upgrading...".into();
                }
            } else {
                let Some(r) = tables.and_then(|t| t.synthesis.get(&trigger)) else {
                    ui.label("This client has no Laphine data for that item.");
                    return;
                };
                if !r.hint.is_empty() {
                    ui.label(RichText::new(&r.hint).color(crate::classic::ink(Color32::GRAY)));
                }
                ui.label(format!("Pick {} ({} / {} chosen).", r.need, picked.len(), r.need));
                let mine: Vec<(&&InvItem, u16)> = items.iter().filter_map(|i| Some((i, Self::synth_ok(r, i)?))).collect();
                if mine.is_empty() {
                    ui.label(format!("You carry none of the {} accepted items.", r.sources.len()));
                }
                egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                    for (it, n) in &mine {
                        let on = picked.contains(&it.index);
                        let text = if *n > 1 { format!("{} x{n}", name(it)) } else { name(it) };
                        if row(ui, it, on, text) {
                            if on {
                                picked.retain(|i| *i != it.index);
                            } else if picked.len() < r.need {
                                picked.push(it.index);
                            }
                        }
                    }
                });
                if ui.add_enabled(picked.len() == r.need, egui::Button::new("Synthesize")).clicked() {
                    let items = picked.iter().filter_map(|i| mine.iter().find(|(it, _)| it.index == *i).map(|(_, n)| (*i, *n))).collect();
                    out = Some(LapineAction::Synthesize { trigger, items });
                    *status = "Synthesizing...".into();
                }
            }
            if !status.is_empty() {
                ui.label(RichText::new(status.as_str()).color(crate::classic::ink(Color32::from_rgb(255, 210, 110))));
            }
        });
        if !open {
            self.open = None;
            out = Some(LapineAction::Close { upgrade });
        }
        out
    }
}

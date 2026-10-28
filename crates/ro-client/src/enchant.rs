//! Random enchanting (the server's enchantgradeui-style "Enchant" UI, ZC_UI_OPEN_V3 type
//! 10). What each enchant set allows comes from the official client's tables:
//! enchant\enchantlist_f.lub + enchantlist.lub (run in Lua, after widening the 32-bit
//! bytecode), with item AegisNames resolved through itemdbnametbl.lub.

use std::collections::HashMap;

use egui::{Color32, RichText, Vec2};
use mlua::{Lua, Table, Value};
use ro_game::items::{InvItem, ItemDb};
use ro_net::EnchantRequest;

const DIR: &str = r"data\luafiles514\lua files";

/// Items and zeny one action costs.
#[derive(Debug, Clone, Default)]
pub struct Cost {
    pub zeny: u32,
    pub items: Vec<(u32, u32)>,
}

#[derive(Debug, Clone, Default)]
pub struct Slot {
    pub cost: Cost,
    /// Success chance in 0.001% units (100000 = 100%).
    pub rate: u32,
    /// Enchants a perfect enchant can choose, with their costs.
    pub perfect: Vec<(u32, Cost)>,
}

#[derive(Debug, Clone, Default)]
pub struct Set {
    pub caution: String,
    pub targets: Vec<u32>,
    pub min_refine: u8,
    pub min_grade: u8,
    /// Card slots in the order they are filled (the card index: slot 3 is the fourth card).
    pub order: Vec<u8>,
    pub slots: HashMap<u8, Slot>,
    pub reset: Option<(Cost, u32)>,
}

const PRELUDE: &str = r#"
MAX_SLOT_NUM = 4; MAX_MATERIAL_NUM = 10; MAX_REFINE_LEVEL = 20; MAX_GRADE_LEVEL = 4; IS_CLIENT = true
function MessageBox(...) end
for _, n in ipairs({"C_AddPerfectEnchant","C_AddPerfectUpgradeEnchant","C_AddRandomUpgradeEnchant","C_AddTargetItem","C_AddUpgradeEnchant","C_ApproveRandomOption","C_SetCaution","C_SetCondition","C_SetEnchant","C_SetGradeBonus","C_SetRandomUpgradeRequire","C_SetRequire","C_SetReset","C_SetSlotOrder","C_SetSuccessRate"}) do
  _G[n] = function() return true end
end
function C_GetSlotCount() return 4 end
"#;

fn read(vfs: &ro_vfs::Vfs, name: &str) -> Option<Vec<u8>> {
    let d = vfs.read(&format!(r"{DIR}\{name}.lub")).ok().flatten()?;
    ro_formats::lub::widen_size_t(&d).ok()
}

fn text(v: Value) -> String {
    match v {
        Value::String(s) => encoding_rs::EUC_KR.decode(&s.as_bytes()).0.into_owned(),
        _ => String::new(),
    }
}

/// Loads every enchant set by its index (the id the server opens the window with).
pub fn load(vfs: &ro_vfs::Vfs) -> HashMap<u64, Set> {
    let mut out = HashMap::new();
    let names: HashMap<String, u32> = vfs
        .read(&format!(r"{DIR}\itemdbnametbl.lub"))
        .ok()
        .flatten()
        .and_then(|d| ro_formats::lub::eval(&d).ok())
        .map(|c| {
            let t = c.globals.get("ItemDBNameTbl").cloned();
            t.map(|t| c.entries(&t).iter().filter_map(|(k, v)| Some((k.as_str()?.to_string(), v.as_number()? as u32))).collect()).unwrap_or_default()
        })
        .unwrap_or_default();
    let (Some(f), Some(list)) = (read(vfs, r"enchant\enchantlist_f"), read(vfs, r"enchant\enchantlist")) else { return out };
    let lua = unsafe { Lua::unsafe_new() };
    let run = lua.load(PRELUDE).exec().and_then(|_| lua.load(&f[..]).exec()).and_then(|_| lua.load(&list[..]).exec());
    if let Err(e) = run {
        tracing::warn!("enchant tables: {e}");
        return out;
    }
    let id = |name: &str| names.get(name).copied();
    let cost = |t: &Table| -> Cost {
        let zeny = t.get::<f64>("Zeny").unwrap_or(0.0) as u32;
        let mut items = vec![];
        if let Ok(m) = t.get::<Table>("MatTbl") {
            for (k, n) in m.pairs::<String, f64>().filter_map(Result::ok) {
                if let Some(i) = id(&k) {
                    items.push((i, n as u32));
                }
            }
        }
        Cost { zeny, items }
    };
    let Ok(sets) = lua.globals().get::<Table>("Table") else { return out };
    for (index, s) in sets.pairs::<i64, Table>().filter_map(Result::ok) {
        let mut set = Set { caution: text(s.get("CautionMsg").unwrap_or(Value::Nil)), ..Default::default() };
        if let Ok(t) = s.get::<Table>("TargetItemTbl") {
            set.targets = t.sequence_values::<String>().filter_map(Result::ok).filter_map(|n| id(&n)).collect();
        }
        if let Ok(c) = s.get::<Table>("Condition") {
            set.min_refine = c.get::<f64>("MinRefine").unwrap_or(0.0) as u8;
            set.min_grade = c.get::<f64>("MinGrade").unwrap_or(0.0) as u8;
        }
        if let Ok(o) = s.get::<Table>("SlotOrder") {
            set.order = o.sequence_values::<f64>().filter_map(Result::ok).map(|n| n as u8).collect();
        }
        if let Ok(slots) = s.get::<Table>("Slot") {
            for (n, t) in slots.pairs::<i64, Table>().filter_map(Result::ok) {
                let mut slot = Slot { rate: t.get::<f64>("SuccessRate").unwrap_or(0.0) as u32, ..Default::default() };
                if let Ok(r) = t.get::<Table>("RequireTbl") {
                    slot.cost = cost(&r);
                }
                if let Ok(p) = t.get::<Table>("PerfectECTbl") {
                    for (k, c) in p.pairs::<String, Table>().filter_map(Result::ok) {
                        if let Some(i) = id(&k) {
                            slot.perfect.push((i, cost(&c)));
                        }
                    }
                    slot.perfect.sort_by_key(|p| p.0);
                }
                set.slots.insert(n as u8, slot);
            }
        }
        if let Ok(r) = s.get::<Table>("Reset") {
            if r.get::<bool>("bReset").unwrap_or(false) {
                set.reset = Some((cost(&r), r.get::<f64>("Rate").unwrap_or(0.0) as u32));
            }
        }
        out.insert(index as u64, set);
    }
    tracing::info!(sets = out.len(), "enchant sets");
    out
}

#[derive(Default)]
pub struct Enchant {
    sets: Option<HashMap<u64, Set>>,
    /// The open window's enchant set.
    pub open: Option<u64>,
    selected: Option<u16>,
    pub status: String,
}

impl Enchant {
    pub fn open(&mut self, vfs: Option<&ro_vfs::Vfs>, group: u64) {
        if self.sets.is_none() {
            self.sets = Some(vfs.map(load).unwrap_or_default());
        }
        self.open = Some(group);
        self.selected = None;
        self.status.clear();
    }

    /// The next card slot this item would be enchanted in.
    fn next_slot(set: &Set, it: &InvItem) -> Option<u8> {
        set.order.iter().copied().find(|s| it.cards.get(*s as usize).is_some_and(|c| *c == 0))
    }

    pub fn result(&mut self, ok: bool, item: u32, db: Option<&ItemDb>) {
        let name = db.map_or_else(|| format!("Item #{item}"), |d| d.name(item, true));
        self.status = if ok { format!("Enchanted: {name}") } else { "The enchant failed.".into() };
    }

    pub fn show(&mut self, ctx: &egui::Context, items: &[&InvItem], db: Option<&ItemDb>, icons: &HashMap<u32, egui::TextureHandle>, zeny: i64) -> Vec<EnchantRequest> {
        let mut out = vec![];
        let Some(group) = self.open else { return out };
        let name = |id: u32| db.map_or_else(|| format!("Item #{id}"), |d| d.name(id, true));
        let have = |id: u32| items.iter().filter(|i| i.id == id && i.equipped == 0).map(|i| i.amount as u32).sum::<u32>();
        let afford = |c: &Cost| c.zeny as i64 <= zeny && c.items.iter().all(|(i, n)| have(*i) >= *n);
        let mut open = true;
        crate::classic::show(ctx, egui::Window::new("Enchant").open(&mut open).resizable(false).default_width(460.0), "Enchant", true, |ui| {
            let Some(set) = self.sets.as_ref().and_then(|s| s.get(&group)) else {
                ui.label(format!("This client has no data for enchant set {group}."));
                return;
            };
            if !set.caution.is_empty() {
                ui.label(RichText::new(&set.caution).size(11.0).color(crate::classic::ink(Color32::from_rgb(230, 190, 120))));
                ui.separator();
            }
            let candidates: Vec<&&InvItem> = items.iter().filter(|i| set.targets.contains(&i.id) && i.refine >= set.min_refine && i.grade >= set.min_grade).collect();
            if candidates.is_empty() {
                ui.label(format!("Bring one of: {}", set.targets.iter().map(|t| name(*t)).collect::<Vec<_>>().join(", ")));
                if set.min_refine > 0 {
                    ui.label(format!("Refined to +{} or more.", set.min_refine));
                }
            }
            for it in &candidates {
                ui.horizontal(|ui| {
                    if let Some(t) = icons.get(&it.id) {
                        ui.image((t.id(), Vec2::splat(22.0)));
                    }
                    if ui.selectable_label(self.selected == Some(it.index), format!("+{} {}", it.refine, name(it.id))).clicked() {
                        self.selected = Some(it.index);
                    }
                });
            }
            let Some(it) = self.selected.and_then(|i| candidates.iter().find(|c| c.index == i)) else { return };
            ui.separator();
            for (i, c) in it.cards.iter().enumerate().filter(|(_, c)| **c != 0) {
                ui.label(RichText::new(format!("Slot {i}: {}", name(*c))).size(11.5));
            }
            match Self::next_slot(set, it).and_then(|s| set.slots.get(&s).map(|sl| (s, sl))) {
                Some((s, slot)) => {
                    ui.label(RichText::new(format!("Next: slot {s}, {}% success", slot.rate as f32 / 1000.0)).strong());
                    let cost_line = |c: &Cost| {
                        let mut parts: Vec<String> = c.items.iter().map(|(i, n)| format!("{} x{n} ({})", name(*i), have(*i))).collect();
                        if c.zeny > 0 {
                            parts.push(format!("{} z", c.zeny));
                        }
                        parts.join(", ")
                    };
                    ui.label(format!("Cost: {}", cost_line(&slot.cost)));
                    if ui.add_enabled(afford(&slot.cost), egui::Button::new("Enchant")).clicked() {
                        out.push(EnchantRequest::Random { group, index: it.index });
                        self.status = "Enchanting...".into();
                    }
                    if !slot.perfect.is_empty() {
                        egui::CollapsingHeader::new(format!("Choose the enchant ({} options)", slot.perfect.len())).show(ui, |ui| {
                            egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                                for (e, c) in &slot.perfect {
                                    ui.horizontal(|ui| {
                                        if ui.add_enabled(afford(c), egui::Button::new(name(*e)).small()).clicked() {
                                            out.push(EnchantRequest::Perfect { group, index: it.index, item: *e });
                                        }
                                        ui.label(RichText::new(cost_line(c)).size(11.0));
                                    });
                                }
                            });
                        });
                    }
                }
                None => {
                    ui.label("Every enchant slot of this item is filled.");
                }
            }
            if let Some((c, rate)) = &set.reset {
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.add_enabled(afford(c) && it.cards.iter().any(|c| *c != 0), egui::Button::new("Reset enchants")).clicked() {
                        out.push(EnchantRequest::Reset { group, index: it.index });
                    }
                    ui.label(RichText::new(format!("{}% — costs {} z", *rate as f32 / 1000.0, c.zeny)).size(11.0));
                });
            }
            if !self.status.is_empty() {
                ui.label(RichText::new(&self.status).color(crate::classic::ink(Color32::from_rgb(255, 210, 110))));
            }
        });
        if !open {
            self.open = None;
            out.push(EnchantRequest::Close);
        }
        out
    }
}

impl Enchant {
    /// Test hook: a random enchant on the inventory item `index`.
    pub fn test_random(&mut self, index: u16) -> Option<EnchantRequest> {
        let group = self.open?;
        self.selected = Some(index);
        Some(EnchantRequest::Random { group, index })
    }
}

impl Enchant {
    /// After a successful random enchant: (inventory index, card slot 0-based) it went in.
    pub fn filled_slot(&self, items: &std::collections::BTreeMap<u16, InvItem>) -> Option<(u16, usize)> {
        let set = self.sets.as_ref()?.get(&self.open?)?;
        let it = items.get(&self.selected?)?;
        Self::next_slot(set, it).map(|s| (it.index, s as usize))
    }
}

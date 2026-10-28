//! Item reform (ZC_OPEN_REFORM_UI): a reform tool turns one item into another. What each
//! tool can reform, the materials and the rules come from the official
//! itemreform\itemreformsystem.lub; AegisNames resolve through itemdbnametbl.lub.

use std::collections::HashMap;

use egui::{Color32, RichText, Vec2};
use ro_formats::lub::{self, Value};
use ro_game::items::{InvItem, ItemDb};

#[derive(Debug, Clone, Default)]
pub struct Recipe {
    pub base: u32,
    pub result: u32,
    pub materials: Vec<(u32, u32)>,
    pub refine: (u8, u8),
    pub info: Vec<String>,
}

#[derive(Default)]
pub struct Tables {
    /// Tool item id -> recipes it can do.
    pub by_tool: HashMap<u32, Vec<Recipe>>,
}

impl Tables {
    pub fn load(vfs: &ro_vfs::Vfs) -> Self {
        const DIR: &str = r"data\luafiles514\lua files";
        let eval = |p: &str| vfs.read(&format!(r"{DIR}\{p}.lub")).ok().flatten().and_then(|d| lub::eval(&d).ok());
        let (Some(names), Some(c)) = (eval("itemdbnametbl"), eval(r"itemreform\itemreformsystem")) else { return Self::default() };
        let ids: HashMap<&str, u32> = names.globals.get("ItemDBNameTbl").map(|t| names.entries(t).iter().filter_map(|(k, v)| Some((k.as_str()?, v.as_number()? as u32))).collect()).unwrap_or_default();
        let id = |v: Option<&Value>| v.and_then(Value::as_str).and_then(|n| ids.get(n).copied());
        let mut recipes: HashMap<u32, Recipe> = HashMap::new();
        if let Some(info) = c.globals.get("ReformInfo") {
            for (k, t) in c.entries(info) {
                let Some(k) = k.as_number() else { continue };
                let (Some(base), Some(result)) = (id(c.field(t, "BaseItem")), id(c.field(t, "ResultItem"))) else { continue };
                let materials = c.field(t, "Material").map(|m| c.entries(m).iter().filter_map(|(n, a)| Some((*ids.get(n.as_str()?)?, a.as_number()? as u32))).collect()).unwrap_or_default();
                let num = |f: &str| c.field(t, f).and_then(Value::as_number).unwrap_or(0.0) as u8;
                let info = c.field(t, "InformationString").map(|s| c.entries(s).iter().filter_map(|(_, l)| l.as_str().map(str::to_string)).collect()).unwrap_or_default();
                recipes.insert(k as u32, Recipe { base, result, materials, refine: (num("NeedRefineMin"), num("NeedRefineMax")), info });
            }
        }
        let mut by_tool = HashMap::new();
        if let Some(list) = c.globals.get("ReformItemList") {
            for (tool, keys) in c.entries(list) {
                let Some(tool) = tool.as_str().and_then(|n| ids.get(n).copied()) else { continue };
                let rs: Vec<Recipe> = c.entries(keys).iter().filter_map(|(_, k)| recipes.get(&(k.as_number()? as u32)).cloned()).collect();
                by_tool.insert(tool, rs);
            }
        }
        tracing::info!(tools = by_tool.len(), recipes = recipes.len(), "item reform tables");
        Self { by_tool }
    }
}

#[derive(Default)]
pub struct Reform {
    tables: Option<Tables>,
    /// The tool the window was opened with.
    pub tool: Option<u32>,
    selected: Option<u16>,
    pub status: String,
}

pub enum ReformAction {
    Reform { tool: u32, index: u16 },
    Close,
}

impl Reform {
    pub fn open(&mut self, vfs: Option<&ro_vfs::Vfs>, tool: u32) {
        if self.tables.is_none() {
            self.tables = Some(vfs.map(Tables::load).unwrap_or_default());
        }
        self.tool = Some(tool);
        self.selected = None;
        self.status.clear();
    }

    pub fn result(&mut self, ok: bool) {
        if ok {
            self.status = "Reform complete.".into();
            self.tool = None;
        } else {
            self.status = "The reform failed (materials or refine level).".into();
        }
    }

    /// Test hook: reform the first item the tool accepts.
    pub fn test_reform(&self, items: &[&InvItem]) -> Option<ReformAction> {
        let tool = self.tool?;
        let rs = self.tables.as_ref()?.by_tool.get(&tool)?;
        let it = items.iter().find(|i| rs.iter().any(|r| r.base == i.id && (r.refine.0..=r.refine.1).contains(&i.refine)))?;
        Some(ReformAction::Reform { tool, index: it.index })
    }

    pub fn show(&mut self, ctx: &egui::Context, items: &[&InvItem], db: Option<&ItemDb>, icons: &HashMap<u32, egui::TextureHandle>) -> Option<ReformAction> {
        let tool = self.tool?;
        let mut out = None;
        let name = |id: u32| db.map_or_else(|| format!("Item #{id}"), |d| d.name(id, true));
        let have = |id: u32| items.iter().filter(|i| i.id == id && i.equipped == 0).map(|i| i.amount as u32).sum::<u32>();
        let mut open = true;
        crate::classic::show(ctx, egui::Window::new("Item Reform").open(&mut open).resizable(false).default_width(420.0), "Item Reform", true, |ui| {
            ui.label(RichText::new(name(tool)).strong());
            let Some(rs) = self.tables.as_ref().and_then(|t| t.by_tool.get(&tool)) else {
                ui.label("This client has no reform data for that item.");
                return;
            };
            let mine: Vec<(&&InvItem, &Recipe)> = items.iter().filter_map(|i| rs.iter().find(|r| r.base == i.id).map(|r| (i, r))).collect();
            if mine.is_empty() {
                ui.label(format!("You carry nothing this can reform ({} kinds).", rs.len()));
            }
            for (it, r) in &mine {
                ui.horizontal(|ui| {
                    if let Some(t) = icons.get(&it.id) {
                        ui.image((t.id(), Vec2::splat(22.0)));
                    }
                    let label = format!("+{} {}  →  {}", it.refine, name(it.id), name(r.result));
                    if ui.selectable_label(self.selected == Some(it.index), label).clicked() {
                        self.selected = Some(it.index);
                    }
                });
            }
            if let Some((it, r)) = self.selected.and_then(|s| mine.iter().find(|(i, _)| i.index == s)) {
                ui.separator();
                for l in &r.info {
                    ui.label(crate::hud::npc_text(&l.replace("<B>", "").replace("</B>", ""), 12.0));
                }
                let refine_ok = (r.refine.0..=r.refine.1).contains(&it.refine);
                ui.label(RichText::new(format!("Refine +{} to +{}", r.refine.0, r.refine.1)).color(if refine_ok { crate::classic::ink(Color32::GRAY) } else { Color32::from_rgb(230, 110, 100) }));
                for (m, n) in &r.materials {
                    let ok = have(*m) >= *n;
                    ui.label(RichText::new(format!("{} x{n} (have {})", name(*m), have(*m))).color(if ok { crate::classic::ink(Color32::GRAY) } else { Color32::from_rgb(230, 110, 100) }));
                }
                let ready = refine_ok && r.materials.iter().all(|(m, n)| have(*m) >= *n);
                if ui.add_enabled(ready, egui::Button::new("Reform")).clicked() {
                    out = Some(ReformAction::Reform { tool, index: it.index });
                    self.status = "Reforming...".into();
                }
            }
            if !self.status.is_empty() {
                ui.label(RichText::new(&self.status).color(crate::classic::ink(Color32::from_rgb(255, 210, 110))));
            }
        });
        if !open {
            self.tool = None;
            out = Some(ReformAction::Close);
        }
        out
    }
}

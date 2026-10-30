//! Status icons (buffs and debuffs) on the local character: the column of icons beside the
//! minimap, with the time left under each and the client's description on hover.
//! Names and descriptions come from efstids.lub / stateiconinfo.lub; icons from
//! data\texture\effect\ (see `status_icons` for the hard-coded classic names).

use std::collections::{BTreeMap, HashMap};
use std::time::Instant;

use egui::{Color32, RichText, Vec2};

#[derive(Default)]
pub struct Buffs {
    /// EFST id -> name ("EFST_BLESSING").
    names: HashMap<u16, String>,
    /// EFST name -> description lines (first is the title).
    descriptions: HashMap<String, Vec<String>>,
    /// Active: id -> when it runs out (None: until removed).
    pub active: BTreeMap<u16, Option<Instant>>,
    icons: HashMap<u16, Option<egui::TextureHandle>>,
}

impl Buffs {
    pub fn load(vfs: &ro_vfs::Vfs) -> Self {
        let mut b = Self::default();
        let read = |p: &str| vfs.read(p).ok().flatten().and_then(|d| ro_formats::lub::eval(&d).ok());
        if let Some(c) = read(r"data\luafiles514\lua files\stateicon\efstids.lub") {
            for (_, table) in c.globals.iter().filter(|(n, _)| *n == "EFST_IDs") {
                for (k, v) in c.entries(table) {
                    if let (Some(name), Some(id)) = (k.as_str(), v.as_number()) {
                        b.names.insert(id as u16, name.to_string());
                    }
                }
            }
        }
        // The English table the iRO client reads, then the older Korean copy for the few
        // statuses it lacks.
        for path in [r"data\luafiles514\lua files\stateicon\stateiconinfo.lub", r"data\lua514\data\luafiles514\lua files\stateicon\stateiconinfo.lub"] {
            let Some(c) = read(path) else { continue };
            for (_, table) in c.globals.iter().filter(|(n, _)| *n == "StateIconList") {
                for (k, v) in c.entries(table) {
                    let Some(name) = k.as_str() else { continue };
                    if b.descriptions.contains_key(name) {
                        continue;
                    }
                    let lines: Vec<String> = c
                        .field(v, "descript")
                        .map(|d| c.entries(d).iter().filter_map(|(_, line)| c.entries(line).first()?.1.as_str().map(str::to_string)).collect())
                        .unwrap_or_default();
                    if !lines.is_empty() {
                        b.descriptions.insert(name.to_string(), lines);
                    }
                }
            }
        }
        for &(id, _) in crate::samurai::BUFFS {
            if let Some(skill) = crate::samurai::buff_skill(id) {
                let name = format!("EFST_{}", skill.internal);
                b.descriptions.insert(name.clone(), vec![skill.name.into(), skill.description.into()]);
                b.names.insert(id, name);
            }
        }
        tracing::info!(names = b.names.len(), described = b.descriptions.len(), "status icon tables");
        b
    }

    /// A status icon turned on or off for us.
    /// Whether a status is on, by its EFST name ("EFST_LIGHT_OF_REGENE").
    pub fn has(&self, name: &str) -> bool {
        self.active.keys().any(|id| self.names.get(id).is_some_and(|n| n == name))
    }

    pub fn set(&mut self, id: u16, on: bool, remain_ms: Option<u32>) {
        if on {
            self.active.insert(id, remain_ms.map(|ms| Instant::now() + std::time::Duration::from_millis(ms as u64)));
        } else {
            self.active.remove(&id);
        }
    }

    fn icon(&mut self, ctx: &egui::Context, vfs: &ro_vfs::Vfs, id: u16) -> Option<egui::TextureHandle> {
        let names = &self.names;
        self.icons
            .entry(id)
            .or_insert_with(|| {
                let classic = crate::status_icons::ICONS.iter().find(|(i, _)| *i == id).map(|(_, f)| f.to_string());
                // Samurai buffs are EFST_SAM_<skill>: the bundled efst_sam_<skill>.tga.
                let modern = names.get(&id).map(|n| format!("{}.tga", n.to_ascii_lowercase()));
                let found = classic.into_iter().chain(modern).find_map(|f| crate::load_texture(ctx, vfs, &format!(r"data\texture\effect\{f}")));
                // Without one, a Samurai buff shows its skill's icon (bundled, else the stock stand-in).
                found.or_else(|| {
                    let skill = crate::samurai::buff_skill(id)?;
                    [skill.internal.to_ascii_lowercase(), skill.icon.to_string()]
                        .iter()
                        .find_map(|name| crate::load_texture(ctx, vfs, &format!(r"data\texture\유저인터페이스\item\{name}.bmp")))
                })
            })
            .clone()
    }

    /// The icon column, down the right side under the minimap.
    pub fn show(&mut self, ctx: &egui::Context, vfs: &ro_vfs::Vfs, top: f32) {
        let now = Instant::now();
        self.active.retain(|_, end| end.is_none_or(|e| e > now));
        let ids: Vec<(u16, Option<Instant>)> = self.active.iter().map(|(k, v)| (*k, *v)).collect();
        let shown: Vec<_> = ids.into_iter().filter_map(|(id, end)| Some((id, end, self.icon(ctx, vfs, id)?))).collect();
        if shown.is_empty() {
            return;
        }
        egui::Area::new(egui::Id::unique("buffs")).anchor(egui::Align2::RIGHT_TOP, Vec2::new(-12.0, top)).interactable(true).show(ctx, |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(4.0, 2.0);
            for row in shown.chunks(2) {
                ui.horizontal(|ui| {
                    for (id, end, tex) in row {
                        ui.vertical(|ui| {
                            let resp = ui.add(egui::Image::new((tex.id(), Vec2::splat(28.0))).sense(egui::Sense::hover()));
                            let left = end.map(|e| e.saturating_duration_since(now).as_secs());
                            let label = match left {
                                Some(s) if s >= 3600 => format!("{}h", s / 3600),
                                Some(s) if s >= 60 => format!("{}m", s / 60),
                                Some(s) => format!("{s}s"),
                                None => String::new(),
                            };
                            ui.label(RichText::new(label).size(10.0).color(Color32::WHITE));
                            let name = self.names.get(id).cloned().unwrap_or_else(|| format!("Status {id}"));
                            let lines = self.descriptions.get(&name).cloned();
                            resp.on_hover_ui(|ui| {
                                ui.set_max_width(320.0);
                                match &lines {
                                    Some(lines) => {
                                        ui.label(RichText::new(&lines[0]).strong());
                                        for l in lines.iter().skip(1).filter(|l| !l.contains("%s")) {
                                            ui.label(l);
                                        }
                                    }
                                    None => {
                                        ui.label(RichText::new(name.trim_start_matches("EFST_").replace('_', " ")).strong());
                                    }
                                }
                                if let Some(s) = left {
                                    ui.label(RichText::new(format!("{}:{:02} remaining", s / 60, s % 60)).color(Color32::LIGHT_BLUE));
                                }
                            });
                        });
                    }
                });
            }
        });
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }
}

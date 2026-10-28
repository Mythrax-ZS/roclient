//! The quest log (Alt+U) and the quest marks over NPCs. Titles and descriptions come from
//! System\OngoingQuestInfoList_True.lub, marks from texture\유저인터페이스\minimap\quest_*.bmp.

use std::collections::{BTreeMap, HashMap};

use egui::{Color32, RichText, Vec2};
use ro_net::Quest;

#[derive(Default)]
pub struct QuestLog {
    pub quests: BTreeMap<u32, Quest>,
    /// Quest id -> (title, summary, description lines).
    info: HashMap<u32, (String, String, Vec<String>)>,
    /// NPC id -> (mark, colour).
    pub marks: HashMap<u32, (u16, u16)>,
    mark_icons: HashMap<(u16, u16), Option<egui::TextureHandle>>,
    pub open: bool,
    selected: Option<u32>,
    show_inactive: bool,
}

impl QuestLog {
    pub fn load(vfs: &ro_vfs::Vfs) -> Self {
        let mut q = Self::default();
        let Some(c) = vfs.read(r"System\OngoingQuestInfoList_True.lub").ok().flatten().and_then(|d| ro_formats::lub::eval(&d).ok()) else {
            tracing::warn!("OngoingQuestInfoList_True.lub not found; quests show by id");
            return q;
        };
        for (_, table) in c.globals.iter().filter(|(n, _)| *n == "QuestInfoList") {
            for (id, entry) in c.entries(table) {
                let Some(id) = id.as_number() else { continue };
                let s = |k: &str| c.field(entry, k).and_then(|v| v.as_str()).unwrap_or_default().to_string();
                let desc = c.field(entry, "Description").map(|d| c.entries(d).iter().filter_map(|(_, l)| l.as_str().map(str::to_string)).collect()).unwrap_or_default();
                q.info.insert(id as u32, (s("Title"), s("Summary"), desc));
            }
        }
        tracing::info!(quests = q.info.len(), "quest descriptions");
        q
    }

    pub fn title(&self, id: u32) -> String {
        self.info.get(&id).map(|i| i.0.clone()).filter(|t| !t.is_empty()).unwrap_or_else(|| format!("Quest {id}"))
    }

    /// Hunting progress from the server.
    pub fn progress(&mut self, updates: &[(u32, u32, u16, u16)]) {
        for &(quest, hunt, total, count) in updates {
            if let Some(o) = self.quests.get_mut(&quest).and_then(|q| q.objectives.iter_mut().find(|o| o.hunt_id == hunt)) {
                o.count = count;
                o.total = total;
            }
        }
    }

    pub fn mark_icon(&mut self, ctx: &egui::Context, vfs: &ro_vfs::Vfs, mark: u16, color: u16) -> Option<egui::TextureHandle> {
        self.mark_icons
            .entry((mark, color))
            .or_insert_with(|| crate::load_texture(ctx, vfs, &format!(r"data\texture\유저인터페이스\minimap\quest_{mark}_{}.bmp", color.max(1))))
            .clone()
    }

    /// The log window. Returns a quest to show or hide in the active list.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<(u32, bool)> {
        if !self.open {
            return None;
        }
        let mut toggle = None;
        let mut open = true;
        egui::Window::new("Quests").open(&mut open).default_size(Vec2::new(520.0, 360.0)).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.show_inactive, false, "Active");
                ui.selectable_value(&mut self.show_inactive, true, "Inactive");
            });
            ui.separator();
            let list: Vec<(u32, bool)> = self.quests.values().filter(|q| q.active != self.show_inactive).map(|q| (q.id, q.active)).collect();
            if list.is_empty() {
                ui.label(RichText::new("No quests here.").color(Color32::GRAY));
                return;
            }
            ui.columns(2, |cols| {
                egui::ScrollArea::vertical().id_salt("quest_list").max_height(300.0).show(&mut cols[0], |ui| {
                    for (id, _) in &list {
                        if ui.selectable_label(self.selected == Some(*id), self.title(*id)).clicked() {
                            self.selected = Some(*id);
                        }
                    }
                });
                let ui = &mut cols[1];
                let Some(q) = self.selected.and_then(|id| self.quests.get(&id)) else {
                    ui.label(RichText::new("Select a quest.").color(Color32::GRAY));
                    return;
                };
                ui.label(RichText::new(self.title(q.id)).strong().size(15.0));
                if let Some((_, summary, desc)) = self.info.get(&q.id) {
                    if !summary.is_empty() {
                        ui.label(crate::hud::npc_text(summary, 12.5));
                    }
                    egui::ScrollArea::vertical().id_salt("quest_desc").max_height(160.0).show(ui, |ui| {
                        for line in desc {
                            ui.label(crate::hud::npc_text(line, 12.5));
                        }
                    });
                }
                if !q.objectives.is_empty() {
                    ui.separator();
                    for o in &q.objectives {
                        let done = o.total > 0 && o.count >= o.total;
                        let text = if o.total > 0 { format!("{}  {} / {}", o.name, o.count, o.total) } else { format!("{}  {}", o.name, o.count) };
                        ui.label(RichText::new(text).color(if done { Color32::LIGHT_GREEN } else { Color32::WHITE }));
                    }
                }
                if q.end > 0 {
                    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
                    let left = q.end as i64 - now;
                    ui.label(RichText::new(if left > 0 { format!("Time left: {}h {}m", left / 3600, left % 3600 / 60) } else { "Expired".into() }).color(Color32::LIGHT_BLUE));
                }
                ui.separator();
                let label = if q.active { "Move to Inactive" } else { "Move to Active" };
                if ui.button(label).clicked() {
                    toggle = Some((q.id, !q.active));
                }
            });
        });
        self.open = open;
        toggle
    }
}

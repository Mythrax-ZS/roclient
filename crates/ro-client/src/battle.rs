//! Battlegrounds and War of Emperium: the entry queue (the battleground list comes from the
//! client's entryqueue\entryqueuelist), the "your match is ready" prompt, the score and
//! team panel during a match, and our guild's castles (names from agit\agitconfig) with
//! their investment and the castle teleport.

use std::collections::HashMap;

use egui::{Align2, Color32, RichText, Vec2};
use ro_formats::lub::{self, Value};
use ro_net::{BgEvent, BgRequest, CastleRequest};

pub struct Entry {
    /// The name the server knows it by.
    pub name: String,
    pub display: String,
    pub solo: bool,
    pub party: bool,
    pub guild: bool,
    pub levels: Option<(u32, u32)>,
    pub reward: String,
}

#[derive(Default)]
pub struct Teammate {
    pub name: String,
    pub at: Option<(u16, u16)>,
    pub hp: Option<(u32, u32)>,
}

#[derive(Default)]
pub struct Battle {
    pub open: bool,
    tab: u8,
    loaded: bool,
    entries: Vec<Entry>,
    /// Castle client id -> (name, map).
    castle_names: HashMap<u8, (String, String)>,
    /// The queue we are in, and how many have joined it.
    pub queued: Option<(String, u32)>,
    /// A match waiting for our answer: (battleground, lobby).
    pub ready: Option<(String, String)>,
    pub status: String,
    /// During a match: the score, the map it is on, and our team.
    pub score: Option<(u16, u16)>,
    score_map: String,
    pub team: HashMap<u32, Teammate>,
    pub castles: Vec<u8>,
    /// Test hook: answer "ready" with Enter at once.
    pub auto_accept: bool,
    castle_info: HashMap<u8, (u32, u32)>,
}

fn load_chunk(vfs: &ro_vfs::Vfs, path: &str) -> Option<lub::Chunk> {
    lub::eval(&vfs.read(path).ok()??).ok()
}

fn num(c: &lub::Chunk, t: &Value, k: &str) -> f64 {
    c.field(t, k).and_then(Value::as_number).unwrap_or(0.0)
}

fn text(c: &lub::Chunk, t: &Value, k: &str) -> String {
    c.field(t, k).and_then(Value::as_str).unwrap_or_default().to_string()
}

impl Battle {
    pub fn ensure_loaded(&mut self, vfs: Option<&ro_vfs::Vfs>) {
        let Some(vfs) = vfs.filter(|_| !self.loaded) else { return };
        self.loaded = true;
        if let Some(c) = load_chunk(vfs, r"data\luafiles514\lua files\entryqueue\entryqueuelist.lub") {
            if let Some(list) = c.globals.get("EntryQueueList").or_else(|| c.globals.get("ReadEntryQueueList")) {
                for (_, e) in c.entries(list) {
                    let levels = (num(&c, e, "EnterCondType") == 2.0).then(|| (num(&c, e, "EnterCondValue1") as u32, num(&c, e, "EnterCondValue2") as u32));
                    self.entries.push(Entry {
                        name: text(&c, e, "BattleFieldName"),
                        display: text(&c, e, "DisplayBattleFieldName"),
                        solo: num(&c, e, "PrivateApply") != 0.0,
                        party: num(&c, e, "PartyApply") != 0.0,
                        guild: num(&c, e, "GuildApply") != 0.0,
                        levels,
                        reward: text(&c, e, "RewardWin"),
                    });
                }
            }
        }
        if let Some(c) = load_chunk(vfs, r"data\luafiles514\lua files\agit\agitconfig.lub") {
            if let Some(list) = c.globals.get("tbl_AgitInfo") {
                for (k, e) in c.entries(list) {
                    let map = k.as_str().unwrap_or_default().trim_end_matches(".gat").to_string();
                    self.castle_names.insert(num(&c, e, "Number") as u8, (text(&c, e, "Name"), map));
                }
            }
        }
        tracing::info!(battlegrounds = self.entries.len(), castles = self.castle_names.len(), "battle tables");
    }

    /// A server update; returns a chat line to show, if any.
    pub fn event(&mut self, e: BgEvent, map: &str) -> Option<String> {
        match e {
            BgEvent::Applied { result: 1, name } => {
                self.status = format!("Applied for {name}.");
                self.queued = Some((name, 0));
            }
            BgEvent::Applied { result, .. } => self.status = ro_net::bg_apply_error(result).into(),
            BgEvent::Queued { name, count } => self.queued = Some((name, count)),
            BgEvent::Cancelled { ok, name } => {
                if ok {
                    self.queued = None;
                    self.status = format!("Left the queue for {name}.");
                }
            }
            BgEvent::Ready { name, lobby } => {
                self.ready = Some((name.clone(), lobby));
                return Some(format!("The battleground {name} is ready to enter."));
            }
            BgEvent::Admission { ok, name } => {
                if !ok {
                    self.status = format!("Entry to {name} was declined.");
                    self.queued = None;
                }
            }
            BgEvent::QueueDone => {
                self.queued = None;
                self.ready = None;
                self.status.clear();
            }
            BgEvent::Score(a, b) => {
                self.score = Some((a, b));
                self.score_map = map.to_string();
            }
            BgEvent::Position { id, name, at, .. } => {
                let t = self.team.entry(id).or_default();
                if !name.is_empty() {
                    t.name = name;
                }
                t.at = at;
            }
            BgEvent::Hp { id, hp, max } => self.team.entry(id).or_default().hp = Some((hp, max)),
            BgEvent::Camp { .. } => {}
            BgEvent::Chat(text) => return Some(text),
        }
        None
    }

    /// Leaving the match's map ends the match display.
    pub fn map_changed(&mut self, map: &str) {
        if self.score.is_some() && self.score_map != map {
            self.score = None;
            self.team.clear();
        }
    }

    pub fn castle_info(&mut self, id: u8, economy: u32, defense: u32) {
        self.castle_info.insert(id, (economy, defense));
    }

    pub fn castle_name(&self, id: u8) -> String {
        self.castle_names.get(&id).map_or_else(|| format!("Castle #{id}"), |(n, m)| format!("{n} ({m})"))
    }

    /// Teammates for the minimap.
    pub fn team_dots(&self) -> Vec<(f32, f32)> {
        self.team.values().filter_map(|t| t.at).map(|(x, y)| (x as f32, y as f32)).collect()
    }

    pub fn show(&mut self, ctx: &egui::Context, level: u32) -> (Vec<BgRequest>, Vec<CastleRequest>) {
        let mut bg = vec![];
        let mut castle = vec![];
        // The match is ready: accept or decline, wherever we are.
        if let Some((name, lobby)) = self.ready.clone() {
            egui::Window::new("bg_ready").title_bar(false).resizable(false).anchor(Align2::CENTER_CENTER, Vec2::new(0.0, -120.0)).show(ctx, |ui| {
                ui.label(RichText::new(format!("{name} is ready.")).strong().size(15.0));
                ui.label("Enter the battleground now?");
                ui.horizontal(|ui| {
                    if ui.button("Enter").clicked() {
                        bg.push(BgRequest::Reply { accept: true, name: name.clone(), lobby: lobby.clone() });
                        self.ready = None;
                    }
                    if ui.button("Decline").clicked() {
                        bg.push(BgRequest::Reply { accept: false, name: name.clone(), lobby: lobby.clone() });
                        self.ready = None;
                    }
                });
            });
        }
        // The score over the top of the screen during a match.
        if let Some((lion, eagle)) = self.score {
            egui::Area::new(egui::Id::unique("bg_score")).anchor(Align2::CENTER_TOP, Vec2::new(0.0, 64.0)).interactable(false).show(ctx, |ui| {
                egui::Frame::NONE.fill(Color32::from_black_alpha(170)).corner_radius(6).inner_margin(egui::Margin::symmetric(14, 6)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Lion").color(Color32::from_rgb(120, 170, 255)).size(15.0));
                        ui.label(RichText::new(format!("{lion}  :  {eagle}")).color(Color32::WHITE).strong().size(18.0));
                        ui.label(RichText::new("Eagle").color(Color32::from_rgb(255, 120, 110)).size(15.0));
                    });
                });
            });
        }
        if !self.team.is_empty() {
            egui::Area::new(egui::Id::unique("bg_team")).anchor(Align2::RIGHT_TOP, Vec2::new(-12.0, 280.0)).interactable(false).show(ctx, |ui| {
                egui::Frame::NONE.fill(Color32::from_black_alpha(150)).corner_radius(4).inner_margin(6).show(ui, |ui| {
                    ui.set_width(150.0);
                    ui.label(RichText::new("Team").strong().color(Color32::WHITE));
                    let mut team: Vec<_> = self.team.values().filter(|t| !t.name.is_empty()).collect();
                    team.sort_by(|a, b| a.name.cmp(&b.name));
                    for t in team {
                        ui.label(RichText::new(&t.name).size(11.0).color(Color32::WHITE));
                        if let Some((hp, max)) = t.hp {
                            ui.add(egui::ProgressBar::new(hp as f32 / max.max(1) as f32).desired_height(5.0).fill(Color32::from_rgb(90, 200, 90)));
                        }
                    }
                });
            });
        }
        if !self.open {
            return (bg, castle);
        }
        let mut open = true;
        crate::classic::show(ctx, egui::Window::new("Battle").open(&mut open).default_width(420.0).resizable(false), "Battle", true, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, 0, "Battlegrounds");
                ui.selectable_value(&mut self.tab, 1, "Castles");
            });
            ui.separator();
            if self.tab == 1 {
                if self.castles.is_empty() {
                    ui.label("Your guild holds no castles.");
                }
                for &id in &self.castles {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(self.castle_name(id)).strong());
                        if let Some((e, d)) = self.castle_info.get(&id) {
                            ui.label(format!("Economy {e}  Defense {d}"));
                        } else if ui.small_button("Info").clicked() {
                            castle.push(CastleRequest::Info(id));
                        }
                        if ui.small_button("Teleport").clicked() {
                            castle.push(CastleRequest::Teleport(id));
                        }
                    });
                }
            } else {
                if let Some((name, count)) = &self.queued {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("In the queue for {name}: {count} waiting")).strong());
                        if ui.small_button("Refresh").clicked() {
                            bg.push(BgRequest::Status(name.clone()));
                        }
                        if ui.small_button("Leave queue").clicked() {
                            bg.push(BgRequest::Cancel(name.clone()));
                        }
                    });
                    ui.separator();
                }
                if self.entries.is_empty() {
                    ui.label("No battleground list in the client files.");
                }
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    for e in &self.entries {
                        let fits = e.levels.is_none_or(|(lo, hi)| (lo..=hi).contains(&level));
                        ui.horizontal(|ui| {
                            let title = RichText::new(&e.display).strong();
                            ui.label(if fits { title } else { title.color(crate::classic::ink(Color32::GRAY)) });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let free = self.queued.is_none();
                                for (on, label, kind) in [(e.guild, "Guild", 4), (e.party, "Party", 2), (e.solo, "Solo", 1)] {
                                    if on && ui.add_enabled(free, egui::Button::new(label).small()).clicked() {
                                        bg.push(BgRequest::Apply { kind, name: e.name.clone() });
                                    }
                                }
                            });
                        });
                        if !e.reward.is_empty() {
                            ui.label(RichText::new(format!("Win: {}", e.reward)).size(11.0).color(crate::classic::ink(Color32::GRAY)));
                        }
                        ui.add_space(2.0);
                    }
                });
            }
            if !self.status.is_empty() {
                ui.separator();
                ui.label(RichText::new(&self.status).color(crate::classic::ink(Color32::from_rgb(255, 210, 110))));
            }
        });
        self.open = open;
        (bg, castle)
    }
}

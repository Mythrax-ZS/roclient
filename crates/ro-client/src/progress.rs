//! Progress screens: achievements (System\achievement_list.lub for names, objectives and
//! rewards; the server sends progress), titles earned through them (titletable.lub), and
//! attendance (check_attendance art, the server's day counter).

use std::collections::{BTreeMap, HashMap};

use egui::{Color32, RichText, Vec2};
use ro_net::{AchRank, Achievement, ProgressRequest};

/// An achievement as the client data describes it.
#[derive(Default, Clone)]
struct Info {
    group: String,
    title: String,
    summary: String,
    details: String,
    /// Objectives: (text, count needed).
    objectives: Vec<(String, u32)>,
    reward_item: u32,
    reward_title: u32,
    score: u32,
}

#[derive(Default)]
pub struct Progress {
    info: HashMap<u32, Info>,
    titles: HashMap<u32, String>,
    pub open: bool,
    rank: AchRank,
    list: BTreeMap<u32, Achievement>,
    group: Option<String>,
    show_titles: bool,
    /// The title we wear (0: none).
    pub title: u32,
    /// Attendance: Some(counter) while the window is open (days * 10 + claimed today).
    pub attendance: Option<i32>,
    art: HashMap<&'static str, egui::TextureHandle>,
}

/// Category names for the groups achievement_list.lub uses.
fn group_name(g: &str) -> &str {
    match g {
        "ADVENTURE" => "Adventure",
        "BATTLE" => "Battle",
        "CHATTING" => "Social",
        "EAT" | "GOAL_ACHIEVE" => "Achieve",
        "GOAL_LEVEL" | "GOAL_STATUS" | "JOB_CHANGE" => "Growth",
        "GOAL_REFINE" | "ENCHANT" => "Refine",
        "HUNTING" | "MONSTER_KILL" => "Hunting",
        "MEMORIAL" | "TAMING" => "Memorial",
        _ => "Feat",
    }
}

const DAYS: i32 = 20;

impl Progress {
    pub fn load(vfs: &ro_vfs::Vfs) -> Self {
        let mut p = Self::default();
        if let Some(c) = vfs.read(r"System\achievement_list.lub").ok().flatten().and_then(|d| ro_formats::lub::eval(&d).ok()) {
            if let Some(tbl) = c.globals.get("achievement_tbl") {
                for (id, e) in c.entries(tbl) {
                    let Some(id) = id.as_number() else { continue };
                    let s = |t: &ro_formats::lub::Value, k: &str| c.field(t, k).and_then(|v| v.as_str()).unwrap_or_default().to_string();
                    let n = |t: &ro_formats::lub::Value, k: &str| c.field(t, k).and_then(|v| v.as_number()).unwrap_or(0.0) as u32;
                    let content = c.field(e, "content").cloned().unwrap_or(ro_formats::lub::Value::Nil);
                    let reward = c.field(e, "reward").cloned().unwrap_or(ro_formats::lub::Value::Nil);
                    let objectives = c
                        .field(e, "resource")
                        .map(|r| c.entries(r).iter().map(|(_, o)| (s(o, "text"), n(o, "count"))).collect())
                        .unwrap_or_default();
                    p.info.insert(
                        id as u32,
                        Info {
                            group: s(e, "group"),
                            title: s(e, "title"),
                            summary: s(&content, "summary"),
                            details: s(&content, "details"),
                            objectives,
                            reward_item: n(&reward, "item"),
                            reward_title: n(&reward, "title"),
                            score: n(e, "score"),
                        },
                    );
                }
            }
        }
        let titles = vfs.read(r"data\luafiles514\lua files\datainfo\titletable.lub").ok().flatten().and_then(|d| ro_formats::lub::eval(&d).ok());
        if let Some(c) = titles {
            if let Some(t) = c.globals.get("title_tbl") {
                for (id, name) in c.entries(t) {
                    if let (Some(id), Some(name)) = (id.as_number(), name.as_str()) {
                        p.titles.insert(id as u32, name.to_string());
                    }
                }
            }
        }
        tracing::info!(achievements = p.info.len(), titles = p.titles.len(), "achievement data");
        p
    }

    pub fn load_art(&mut self, ctx: &egui::Context, vfs: &ro_vfs::Vfs) {
        if !self.art.is_empty() {
            return;
        }
        let dir = r"data\texture\유저인터페이스\check_attendance";
        for (key, file) in [("bg", "attendance_bg.bmp"), ("slot", "bt_slot_off.tga"), ("done", "bt_slot_complete.tga"), ("ok", "bt_ok_normal.bmp"), ("ok_press", "bt_ok_press.bmp")] {
            if let Some(t) = crate::load_texture(ctx, vfs, &format!(r"{dir}\{file}")) {
                self.art.insert(key, t);
            }
        }
    }

    pub fn set_all(&mut self, rank: AchRank, list: Vec<Achievement>) {
        self.rank = rank;
        self.list = list.into_iter().map(|a| (a.id, a)).collect();
    }

    /// Returns the achievement's name when it was just completed.
    pub fn update(&mut self, rank: AchRank, ach: Achievement) -> Option<String> {
        self.rank = rank;
        let newly = ach.completed && !self.list.get(&ach.id).is_some_and(|a| a.completed);
        let name = self.name(ach.id);
        self.list.insert(ach.id, ach);
        newly.then_some(name)
    }

    pub fn rewarded(&mut self, id: u32) {
        if let Some(a) = self.list.get_mut(&id) {
            a.rewarded = true;
        }
    }

    pub fn name(&self, id: u32) -> String {
        self.info.get(&id).map(|i| i.title.clone()).filter(|t| !t.is_empty()).unwrap_or_else(|| format!("Achievement {id}"))
    }

    pub fn title_name(&self, id: u32) -> String {
        self.titles.get(&id).cloned().unwrap_or_else(|| format!("Title {id}"))
    }

    /// Titles earned: rewards of completed achievements.
    fn earned_titles(&self) -> Vec<u32> {
        let mut t: Vec<u32> = self.list.values().filter(|a| a.completed).filter_map(|a| self.info.get(&a.id)).map(|i| i.reward_title).filter(|t| *t != 0).collect();
        t.sort_unstable();
        t.dedup();
        t
    }

    pub fn item_ids(&self) -> Vec<u32> {
        self.info.values().map(|i| i.reward_item).filter(|i| *i != 0).collect()
    }

    /// The achievement window.
    pub fn show(&mut self, ctx: &egui::Context, db: Option<&ro_game::items::ItemDb>, icons: &HashMap<u32, egui::TextureHandle>) -> Option<ProgressRequest> {
        if !self.open {
            return None;
        }
        let mut out = None;
        let mut open = true;
        let ink = crate::classic::ink;
        crate::classic::show(ctx, egui::Window::new("Achievements").open(&mut open).fixed_size(Vec2::new(640.0, 440.0)).default_pos(egui::pos2(380.0, 90.0)), "Achievements", true, |ui| {
            let (points, rank, cur, next) = self.rank;
            let done = self.list.values().filter(|a| a.completed).count();
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Rank {rank}")).strong().size(15.0));
                ui.label(format!("{points} points"));
                let frac = if next > 0 { cur as f32 / next as f32 } else { 1.0 };
                ui.add(egui::ProgressBar::new(frac.clamp(0.0, 1.0)).desired_width(180.0).text(format!("{cur} / {next}")));
                ui.label(RichText::new(format!("{done} / {} completed", self.info.len())).color(ink(Color32::GRAY)));
            });
            ui.separator();
            ui.horizontal_top(|ui| {
                // Categories down the left, then titles.
                ui.vertical(|ui| {
                    ui.set_width(110.0);
                    let mut groups: Vec<&str> = self.info.values().map(|i| group_name(&i.group)).collect();
                    groups.sort_unstable();
                    groups.dedup();
                    if ui.selectable_label(self.group.is_none() && !self.show_titles, "All").clicked() {
                        self.group = None;
                        self.show_titles = false;
                    }
                    for g in groups {
                        if ui.selectable_label(!self.show_titles && self.group.as_deref() == Some(g), g).clicked() {
                            self.group = Some(g.to_string());
                            self.show_titles = false;
                        }
                    }
                    ui.separator();
                    if ui.selectable_label(self.show_titles, "Titles").clicked() {
                        self.show_titles = true;
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical().id_salt("ach_list").max_height(360.0).auto_shrink([false, false]).show(ui, |ui| ui.vertical(|ui| {
                    if self.show_titles {
                        let earned = self.earned_titles();
                        ui.label(RichText::new(format!("Wearing: {}", if self.title == 0 { "nothing".into() } else { self.title_name(self.title) })).strong());
                        ui.add_space(4.0);
                        if earned.is_empty() {
                            ui.label(RichText::new("Titles come from completing achievements.").color(ink(Color32::GRAY)));
                        }
                        for t in earned {
                            ui.horizontal(|ui| {
                                ui.label(self.title_name(t));
                                let on = self.title == t;
                                if ui.add_enabled(!on, egui::Button::new(if on { "Worn" } else { "Wear" })).clicked() {
                                    out = Some(ProgressRequest::Title(t));
                                }
                            });
                        }
                        if self.title != 0 && ui.button("Remove title").clicked() {
                            out = Some(ProgressRequest::Title(0));
                        }
                        return;
                    }
                    // Unclaimed rewards first, then in progress, then the rest.
                    let mut ids: Vec<(&u32, &Info)> = self.info.iter().filter(|(_, i)| self.group.as_deref().is_none_or(|g| group_name(&i.group) == g)).collect();
                    let state = |id: &u32| match self.list.get(id) {
                        Some(a) if a.completed && !a.rewarded => 0,
                        Some(a) if !a.completed => 1,
                        Some(_) => 3,
                        None => 2,
                    };
                    ids.sort_by_key(|(id, _)| (state(id), **id));
                    for (id, info) in ids.into_iter().take(150) {
                        let a = self.list.get(id);
                        let completed = a.is_some_and(|a| a.completed);
                        egui::Frame::new().fill(if completed { Color32::from_rgba_unmultiplied(120, 170, 255, 30) } else { Color32::TRANSPARENT }).inner_margin(4).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.set_width(330.0);
                                    ui.label(RichText::new(&info.title).strong());
                                    ui.label(RichText::new(&info.summary).size(11.5).color(ink(Color32::GRAY))).on_hover_text(&info.details);
                                    for (i, (text, need)) in info.objectives.iter().enumerate() {
                                        let have = a.map_or(0, |a| a.objectives.get(i).copied().unwrap_or(0));
                                        if *need > 1 {
                                            ui.add(egui::ProgressBar::new((have as f32 / *need as f32).min(1.0)).desired_width(300.0).text(format!("{} {}/{}", text, have.min(*need), need)));
                                        } else if !text.is_empty() {
                                            ui.label(RichText::new(format!("{} {text}", if completed || have > 0 { "[x]" } else { "[ ]" })).size(11.5));
                                        }
                                    }
                                });
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(format!("{} pts", info.score)).color(ink(Color32::from_rgb(255, 210, 110))));
                                    if info.reward_item != 0 {
                                        ui.horizontal(|ui| {
                                            if let Some(t) = icons.get(&info.reward_item) {
                                                ui.image((t.id(), Vec2::splat(20.0)));
                                            }
                                            ui.label(RichText::new(db.map_or_else(|| format!("Item #{}", info.reward_item), |d| d.name(info.reward_item, true))).size(11.0));
                                        });
                                    }
                                    if info.reward_title != 0 {
                                        ui.label(RichText::new(format!("Title: {}", self.title_name(info.reward_title))).size(11.0).color(ink(Color32::from_rgb(160, 200, 255))));
                                    }
                                    match a {
                                        Some(a) if a.completed && !a.rewarded && (info.reward_item != 0 || info.reward_title != 0) => {
                                            if ui.button("Claim reward").clicked() {
                                                out = Some(ProgressRequest::Reward(*id));
                                            }
                                        }
                                        Some(a) if a.completed => {
                                            ui.label(RichText::new("Complete").color(ink(Color32::from_rgb(120, 220, 140))));
                                        }
                                        _ => {}
                                    }
                                });
                            });
                        });
                        ui.separator();
                    }
                }));
            });
        });
        if !open {
            self.open = false;
        }
        out
    }

    /// The attendance window: twenty day slots, stamped up to the days attended.
    pub fn show_attendance(&mut self, ctx: &egui::Context) -> Option<ProgressRequest> {
        let counter = self.attendance?;
        let (days, today) = (counter / 10, counter % 10 == 1);
        let mut out = None;
        let mut open = true;
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        crate::classic::show(ctx, egui::Window::new("Attendance").open(&mut open).resizable(false).collapsible(false), "Attendance", true, |ui| {
            let bg = self.art.get("bg").map_or(Vec2::new(560.0, 300.0), |t| t.size_vec2());
            let (r, _) = ui.allocate_exact_size(bg, egui::Sense::hover());
            let p = ui.painter();
            if let Some(t) = self.art.get("bg") {
                p.image(t.id(), r, uv, Color32::WHITE);
            }
            // The art's 5 x 4 grid of day cells: stamped up to the days attended; the next
            // claimable day outlined.
            for d in 0..DAYS {
                let s = egui::Rect::from_min_size(r.min + Vec2::new(25.0 + (d % 5) as f32 * 62.0, 94.5 + (d / 5) as f32 * 64.0), Vec2::new(58.5, 61.5));
                p.text(s.left_top() + Vec2::new(4.0, 2.0), egui::Align2::LEFT_TOP, format!("{}", d + 1), egui::FontId::proportional(10.5), Color32::from_rgb(120, 130, 160));
                if d < days {
                    if let Some(t) = self.art.get("done") {
                        p.image(t.id(), s, uv, Color32::WHITE);
                    } else {
                        p.text(s.center(), egui::Align2::CENTER_CENTER, "OK", egui::FontId::proportional(16.0), Color32::from_rgb(70, 110, 200));
                    }
                } else if d == days && !today {
                    p.rect_stroke(s.shrink(1.0), 4.0, egui::Stroke::new(2.0, Color32::from_rgb(240, 150, 170)), egui::StrokeKind::Inside);
                }
            }
            ui.horizontal(|ui| {
                ui.label(format!("Days attended: {days} / {DAYS}"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if today { "Claimed today" } else { "Claim today's reward" };
                    if ui.add_enabled(!today && days < DAYS, egui::Button::new(label)).clicked() {
                        out = Some(ProgressRequest::ClaimAttendance);
                    }
                });
            });
            ui.label(RichText::new("Rewards arrive by mail (RODEX).").size(11.0).color(crate::classic::ink(Color32::GRAY)));
        });
        if !open {
            self.attendance = None;
        }
        out
    }
}

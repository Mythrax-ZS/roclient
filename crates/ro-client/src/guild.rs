//! The Guild window (Alt+G), like the original client's: found a guild (an Emperium is
//! consumed), then Info / Members / Notice tabs. Invites arrive as a Request prompt
//! (social.rs); players are invited from their click menu or the Members tab.

use egui::{Color32, RichText, Vec2};
use ro_net::{Command, GuildInfo, GuildMember, GuildRequest};

/// Emperium: consumed to found a guild.
pub const EMPERIUM: u32 = 714;

#[derive(Default, PartialEq, Clone, Copy)]
enum Tab {
    #[default]
    Info,
    Members,
    Notice,
}

#[derive(Default)]
pub struct Guild {
    pub open: bool,
    /// Our guild (0: none), whether we lead it, and its name.
    pub id: u32,
    pub master: bool,
    pub name: String,
    pub info: Option<GuildInfo>,
    pub members: Vec<GuildMember>,
    pub notice: (String, String),
    pub tab_members: bool,
    tab: Tab,
    create_name: String,
    invite_name: String,
    edit_notice: Option<(String, String)>,
    /// Ask for info and members next time the window shows.
    pub needs_refresh: bool,
}

/// What the window needs from the rest of the client.
pub struct Ctx<'a> {
    pub account_id: u32,
    pub char_id: u32,
    pub has_emperium: bool,
    pub job_name: &'a dyn Fn(i16) -> &'static str,
}

impl Guild {
    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.needs_refresh |= self.open;
    }

    pub fn show(&mut self, ctx: &egui::Context, c: &Ctx) -> Vec<Command> {
        let mut out = vec![];
        if !self.open {
            return out;
        }
        if std::mem::take(&mut self.needs_refresh) && self.id != 0 {
            out.push(Command::Guild(GuildRequest::Info(0)));
            out.push(Command::Guild(GuildRequest::Info(1)));
        }
        let mut open = true;
        let title = if self.id == 0 { "Guild".to_string() } else { format!("Guild — {}", self.name) };
        egui::Window::new(title).id(egui::Id::unique("guild window")).open(&mut open).resizable(false).default_pos(egui::pos2(520.0, 200.0)).show(ctx, |ui| {
            ui.set_min_width(380.0);
            if self.id == 0 {
                self.create(ui, c, &mut out);
                return;
            }
            if std::mem::take(&mut self.tab_members) {
                self.tab = Tab::Members;
            }
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Info, "Info");
                ui.selectable_value(&mut self.tab, Tab::Members, "Members");
                ui.selectable_value(&mut self.tab, Tab::Notice, "Notice");
            });
            ui.separator();
            match self.tab {
                Tab::Info => self.info(ui),
                Tab::Members => self.members(ui, c, &mut out),
                Tab::Notice => self.notice(ui, &mut out),
            }
        });
        self.open = open;
        out
    }

    fn create(&mut self, ui: &mut egui::Ui, c: &Ctx, out: &mut Vec<Command>) {
        ui.label("You are not in a guild.");
        ui.add_space(6.0);
        ui.label(RichText::new("Found a guild (uses one Emperium):").color(Color32::GRAY));
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.create_name).hint_text("guild name").desired_width(200.0).char_limit(23));
            let ok = c.has_emperium && !self.create_name.trim().is_empty();
            if ui.add_enabled(ok, egui::Button::new("Create")).clicked() {
                out.push(Command::Guild(GuildRequest::Create { char_id: c.char_id, name: self.create_name.trim().to_string() }));
            }
        });
        if !c.has_emperium {
            ui.label(RichText::new("You need an Emperium in your inventory.").color(Color32::from_rgb(230, 150, 90)).size(12.0));
        }
        ui.label(RichText::new("Or type /guild \"Name\" in chat.").color(Color32::GRAY).size(11.5));
    }

    fn info(&self, ui: &mut egui::Ui) {
        let Some(i) = &self.info else {
            ui.label(RichText::new("Loading...").color(Color32::GRAY));
            return;
        };
        egui::Grid::new("guild info").num_columns(2).spacing(Vec2::new(16.0, 6.0)).show(ui, |ui| {
            let row = |ui: &mut egui::Ui, k: &str, v: String| {
                ui.label(RichText::new(k).color(Color32::GRAY));
                ui.label(v);
                ui.end_row();
            };
            row(ui, "Guild", i.name.clone());
            row(ui, "Level", i.level.to_string());
            row(ui, "Master", i.master.clone());
            row(ui, "Members", format!("{} / {}", i.members, i.max_members));
            row(ui, "Average level", i.average_level.to_string());
            row(ui, "Castle", if i.castle.is_empty() { "None".into() } else { i.castle.clone() });
            ui.label(RichText::new("Experience").color(Color32::GRAY));
            let frac = if i.max_exp > 0 { i.exp as f32 / i.max_exp as f32 } else { 0.0 };
            ui.add(egui::ProgressBar::new(frac.clamp(0.0, 1.0)).text(format!("{} / {}", i.exp, i.max_exp)).desired_width(220.0));
            ui.end_row();
        });
    }

    fn members(&mut self, ui: &mut egui::Ui, c: &Ctx, out: &mut Vec<Command>) {
        let online = self.members.iter().filter(|m| m.online).count();
        ui.label(RichText::new(format!("{} members, {online} online", self.members.len())).color(Color32::GRAY));
        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
            egui::Grid::new("guild members").num_columns(4).spacing(Vec2::new(12.0, 4.0)).striped(true).show(ui, |ui| {
                let mut sorted: Vec<&GuildMember> = self.members.iter().collect();
                sorted.sort_by_key(|m| (!m.online, m.position, m.name.clone()));
                for m in sorted {
                    let color = if m.online { Color32::from_rgb(120, 220, 120) } else { Color32::GRAY };
                    ui.label(RichText::new(if m.online { "online" } else { "offline" }).color(color).size(11.5));
                    ui.label(RichText::new(&m.name).color(if m.online { Color32::WHITE } else { Color32::GRAY }));
                    ui.label(format!("{} {}", (c.job_name)(m.job), m.level));
                    if self.master && m.char_id != c.char_id {
                        if ui.small_button("Expel").clicked() {
                            out.push(Command::Guild(GuildRequest::Expel { guild: self.id, account_id: m.account_id, char_id: m.char_id, reason: String::new() }));
                        }
                    } else {
                        ui.label("");
                    }
                    ui.end_row();
                }
            });
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.invite_name).hint_text("character name").desired_width(160.0));
            if ui.add_enabled(!self.invite_name.trim().is_empty(), egui::Button::new("Invite")).clicked() {
                out.push(Command::Guild(GuildRequest::Invite { name: self.invite_name.trim().to_string() }));
                self.invite_name.clear();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Leave guild").clicked() {
                    out.push(Command::Guild(GuildRequest::Leave { guild: self.id, account_id: c.account_id, char_id: c.char_id, reason: String::new() }));
                }
            });
        });
        ui.label(RichText::new("Guild chat: /g message").color(Color32::GRAY).size(11.5));
    }

    fn notice(&mut self, ui: &mut egui::Ui, out: &mut Vec<Command>) {
        match self.edit_notice.as_mut() {
            Some((subject, text)) => {
                ui.add(egui::TextEdit::singleline(subject).hint_text("subject").desired_width(f32::INFINITY).char_limit(59));
                ui.add(egui::TextEdit::multiline(text).hint_text("notice").desired_width(f32::INFINITY).desired_rows(4).char_limit(119));
                let (save, cancel) = ui.horizontal(|ui| (ui.button("Save").clicked(), ui.button("Cancel").clicked())).inner;
                if save {
                    out.push(Command::Guild(GuildRequest::Notice { guild: self.id, subject: subject.clone(), text: text.clone() }));
                    self.notice = (subject.clone(), text.clone());
                }
                if save || cancel {
                    self.edit_notice = None;
                }
            }
            None => {
                let (subject, text) = &self.notice;
                if subject.is_empty() && text.is_empty() {
                    ui.label(RichText::new("No notice.").color(Color32::GRAY));
                } else {
                    ui.label(RichText::new(subject).strong());
                    ui.label(text);
                }
                if self.master && ui.button("Edit notice").clicked() {
                    self.edit_notice = Some(self.notice.clone());
                }
            }
        }
    }
}

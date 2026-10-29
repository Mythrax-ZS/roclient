//! The Guild window (Alt+G), like the original client's: found a guild (an Emperium is
//! consumed), then Info / Members / Notice tabs. Invites arrive as a Request prompt
//! (social.rs); players are invited from their click menu or the Members tab.

use egui::{Color32, RichText, Vec2};
use ro_net::{Command, GuildInfo, GuildMember, GuildRequest, GuildSkill, Position, StatRequest};

/// Emperium: consumed to found a guild.
pub const EMPERIUM: u32 = 714;

#[derive(Default, PartialEq, Clone, Copy)]
enum Tab {
    #[default]
    Info,
    Members,
    Positions,
    Skills,
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
    /// Ranks (titles, permissions, tax) and guild skills.
    pub ranks: Vec<Position>,
    pub skills: Vec<GuildSkill>,
    pub skill_points: i16,
    edit_ranks: Option<Vec<Position>>,
    /// Test hook: open on this tab (1 members, 2 positions, 3 skills).
    pub start_tab: u8,
    tab: Tab,
    create_name: String,
    invite_name: String,
    edit_notice: Option<(String, String)>,
    /// Ask for info and members next time the window shows.
    pub needs_refresh: bool,
    /// Our emblem: (version, texture).
    pub emblem: Option<(u32, egui::TextureHandle)>,
    /// The client folder, where Emblem*.bmp files are picked from.
    pub client_dir: Option<std::path::PathBuf>,
    emblem_files: Option<Vec<String>>,
    pub emblem_status: String,
    /// Allied (0) and enemy (1) guilds: (relation, guild id, name).
    pub relations: Vec<(u32, u32, String)>,
    /// An alliance offer waiting for an answer: (inviter, their guild).
    pub ally_offer: Option<(u32, String)>,
    /// Expelled characters (name, reason).
    pub bans: Vec<(String, String)>,
    /// The master typing the guild name to confirm disbanding.
    disband_key: Option<String>,
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
            out.push(Command::Guild(GuildRequest::Info(2)));
            out.push(Command::Guild(GuildRequest::Info(3)));
            out.push(Command::Guild(GuildRequest::Info(4)));
        }
        let mut open = true;
        let title = if self.id == 0 { "Guild".to_string() } else { format!("Guild — {}", self.name) };
        crate::classic::show(ctx, egui::Window::new(title.as_str()).id(egui::Id::unique("guild window")).open(&mut open).resizable(false).default_pos(egui::pos2(520.0, 200.0)), &title, true, |ui| {
            ui.set_min_width(380.0);
            if self.id == 0 {
                self.create(ui, c, &mut out);
                return;
            }
            match std::mem::take(&mut self.start_tab) {
                1 => self.tab = Tab::Members,
                2 => self.tab = Tab::Positions,
                3 => self.tab = Tab::Skills,
                _ => {}
            }
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Info, "Info");
                ui.selectable_value(&mut self.tab, Tab::Members, "Members");
                ui.selectable_value(&mut self.tab, Tab::Positions, "Positions");
                ui.selectable_value(&mut self.tab, Tab::Skills, "Skills");
                ui.selectable_value(&mut self.tab, Tab::Notice, "Notice");
            });
            ui.separator();
            match self.tab {
                Tab::Info => self.info(ui, &mut out),
                Tab::Members => self.members(ui, c, &mut out),
                Tab::Positions => self.positions(ui, &mut out),
                Tab::Skills => self.skills(ui, &mut out),
                Tab::Notice => self.notice(ui, &mut out),
            }
        });
        self.open = open;
        out
    }

    fn create(&mut self, ui: &mut egui::Ui, c: &Ctx, out: &mut Vec<Command>) {
        ui.label("You are not in a guild.");
        ui.add_space(6.0);
        ui.label(RichText::new("Found a guild (uses one Emperium):").color(crate::classic::ink(Color32::GRAY)));
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.create_name).hint_text("guild name").desired_width(200.0).char_limit(23));
            let ok = c.has_emperium && !self.create_name.trim().is_empty();
            if ui.add_enabled(ok, egui::Button::new("Create")).clicked() {
                out.push(Command::Guild(GuildRequest::Create { char_id: c.char_id, name: self.create_name.trim().to_string() }));
            }
        });
        if !c.has_emperium {
            ui.label(RichText::new("You need an Emperium in your inventory.").color(crate::classic::ink(Color32::from_rgb(230, 150, 90))).size(12.0));
        }
        ui.label(RichText::new("Or type /guild \"Name\" in chat.").color(crate::classic::ink(Color32::GRAY)).size(11.5));
    }

    fn info(&mut self, ui: &mut egui::Ui, out: &mut Vec<Command>) {
        let Some(i) = self.info.clone() else {
            ui.label(RichText::new("Loading...").color(crate::classic::ink(Color32::GRAY)));
            return;
        };
        ui.horizontal(|ui| {
            match &self.emblem {
                Some((_, t)) => ui.image((t.id(), Vec2::splat(48.0))),
                None => ui.allocate_exact_size(Vec2::splat(48.0), egui::Sense::hover()).1,
            };
            ui.label(RichText::new(&i.name).strong().size(17.0));
        });
        if self.master {
            self.emblem_picker(ui, out);
        }
        ui.separator();
        let i = &i;
        egui::Grid::new("guild info").num_columns(2).spacing(Vec2::new(16.0, 6.0)).show(ui, |ui| {
            let row = |ui: &mut egui::Ui, k: &str, v: String| {
                ui.label(RichText::new(k).color(crate::classic::ink(Color32::GRAY)));
                ui.label(v);
                ui.end_row();
            };
            row(ui, "Guild", i.name.clone());
            row(ui, "Level", i.level.to_string());
            row(ui, "Master", i.master.clone());
            row(ui, "Members", format!("{} / {}", i.members, i.max_members));
            row(ui, "Average level", i.average_level.to_string());
            row(ui, "Castle", if i.castle.is_empty() { "None".into() } else { i.castle.clone() });
            for (rel, title) in [(0u32, "Allies"), (1, "Enemies")] {
                let names: Vec<&str> = self.relations.iter().filter(|r| r.0 == rel).map(|r| r.2.as_str()).collect();
                row(ui, title, if names.is_empty() { "None".into() } else { names.join(", ") });
            }
            ui.label(RichText::new("Experience").color(crate::classic::ink(Color32::GRAY)));
            let frac = if i.max_exp > 0 { i.exp as f32 / i.max_exp as f32 } else { 0.0 };
            ui.add(egui::ProgressBar::new(frac.clamp(0.0, 1.0)).text(format!("{} / {}", i.exp, i.max_exp)).desired_width(220.0));
            ui.end_row();
        });
        if !self.bans.is_empty() {
            ui.separator();
            ui.label(RichText::new("Expelled").strong());
            for (name, why) in &self.bans {
                ui.label(if why.trim().is_empty() { name.clone() } else { format!("{name} — {}", why.trim()) });
            }
        }
        if self.master {
            ui.separator();
            match &mut self.disband_key {
                None => {
                    if ui.button("Disband guild").clicked() {
                        self.disband_key = Some(String::new());
                    }
                }
                Some(key) => {
                    ui.label("Type the guild name to disband it (all other members must have left):");
                    ui.text_edit_singleline(key);
                    let (go, cancel) = ui.horizontal(|ui| (ui.add_enabled(*key == i.name, egui::Button::new("Disband")).clicked(), ui.button("Cancel").clicked())).inner;
                    if go {
                        out.push(Command::Guild(GuildRequest::Disband(key.clone())));
                    }
                    if go || cancel {
                        self.disband_key = None;
                    }
                }
            }
        }
    }

    fn members(&mut self, ui: &mut egui::Ui, c: &Ctx, out: &mut Vec<Command>) {
        let online = self.members.iter().filter(|m| m.online).count();
        ui.label(RichText::new(format!("{} members, {online} online", self.members.len())).color(crate::classic::ink(Color32::GRAY)));
        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
            egui::Grid::new("guild members").num_columns(5).spacing(Vec2::new(12.0, 4.0)).striped(true).show(ui, |ui| {
                let mut sorted: Vec<&GuildMember> = self.members.iter().collect();
                sorted.sort_by_key(|m| (!m.online, m.position, m.name.clone()));
                for m in sorted {
                    let color = if m.online { Color32::from_rgb(120, 220, 120) } else { Color32::GRAY };
                    ui.label(RichText::new(if m.online { "online" } else { "offline" }).color(crate::classic::ink(color)).size(11.5));
                    ui.label(RichText::new(&m.name).color(crate::classic::ink(if m.online { Color32::WHITE } else { Color32::GRAY })));
                    ui.label(format!("{} {}", (c.job_name)(m.job), m.level));
                    // Rank: the master assigns it (rank 0 is the master's own and fixed).
                    let rank = self.rank_name(m.position);
                    if self.master && m.position != 0 && !self.ranks.is_empty() {
                        egui::ComboBox::from_id_salt(("rank", m.char_id)).selected_text(rank).width(110.0).show_ui(ui, |ui| {
                            for r in self.ranks.iter().filter(|r| r.id != 0) {
                                if ui.selectable_label(r.id == m.position, &r.name).clicked() && r.id != m.position {
                                    out.push(Command::Guild(GuildRequest::SetRank { account_id: m.account_id, char_id: m.char_id, position: r.id }));
                                }
                            }
                        });
                    } else {
                        ui.label(RichText::new(rank).color(crate::classic::ink(Color32::from_rgb(220, 200, 140))));
                    }
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
        ui.label(RichText::new("Guild chat: /g message").color(crate::classic::ink(Color32::GRAY)).size(11.5));
    }

    fn rank_name(&self, id: i32) -> String {
        self.ranks.iter().find(|r| r.id == id).map_or_else(|| format!("Rank {}", id + 1), |r| r.name.clone())
    }

    /// Ranks: title, invite/expel permissions and experience tax. The master edits them.
    fn positions(&mut self, ui: &mut egui::Ui, out: &mut Vec<Command>) {
        if self.ranks.is_empty() {
            ui.label(RichText::new("Loading...").color(crate::classic::ink(Color32::GRAY)));
            return;
        }
        let editing = self.edit_ranks.is_some();
        let list = self.edit_ranks.as_mut().unwrap_or(&mut self.ranks);
        egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
            egui::Grid::new("guild ranks").num_columns(5).spacing(Vec2::new(10.0, 4.0)).striped(true).show(ui, |ui| {
                for h in ["#", "Title", "Invite", "Expel", "Tax %"] {
                    ui.label(RichText::new(h).color(crate::classic::ink(Color32::GRAY)));
                }
                ui.end_row();
                for p in list.iter_mut() {
                    ui.label((p.id + 1).to_string());
                    let fixed = p.id == 0; // the master's rank keeps its rights
                    if editing {
                        ui.add(egui::TextEdit::singleline(&mut p.name).desired_width(130.0).char_limit(23));
                        let mut invite = p.mode & 0x01 != 0;
                        let mut expel = p.mode & 0x10 != 0;
                        ui.add_enabled(!fixed, egui::Checkbox::without_text(&mut invite));
                        ui.add_enabled(!fixed, egui::Checkbox::without_text(&mut expel));
                        p.mode = (p.mode & !0x11) | if invite { 0x01 } else { 0 } | if expel { 0x10 } else { 0 };
                        ui.add(egui::DragValue::new(&mut p.tax).range(0..=50).suffix("%"));
                    } else {
                        ui.label(&p.name);
                        ui.label(if p.mode & 0x01 != 0 { "yes" } else { "-" });
                        ui.label(if p.mode & 0x10 != 0 { "yes" } else { "-" });
                        ui.label(format!("{}%", p.tax));
                    }
                    ui.end_row();
                }
            });
        });
        ui.separator();
        if !self.master {
            ui.label(RichText::new("Only the guild master can change ranks.").color(crate::classic::ink(Color32::GRAY)).size(11.5));
        } else if editing {
            let (save, cancel) = ui.horizontal(|ui| (ui.button("Save").clicked(), ui.button("Cancel").clicked())).inner;
            if save {
                let ranks = self.edit_ranks.take().unwrap_or_default();
                out.push(Command::Guild(GuildRequest::Positions(ranks.clone())));
                self.ranks = ranks;
            } else if cancel {
                self.edit_ranks = None;
            }
        } else if ui.button("Edit ranks").clicked() {
            self.edit_ranks = Some(self.ranks.clone());
        }
        ui.label(RichText::new("Tax: share of members' experience given to the guild (how guilds level).").color(crate::classic::ink(Color32::GRAY)).size(11.5));
    }

    /// Guild skills, raised with guild skill points (one per guild level).
    fn skills(&mut self, ui: &mut egui::Ui, out: &mut Vec<Command>) {
        ui.label(format!("Skill points: {}", self.skill_points));
        ui.separator();
        if self.skills.is_empty() {
            ui.label(RichText::new("Loading...").color(crate::classic::ink(Color32::GRAY)));
            return;
        }
        egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
            egui::Grid::new("guild skills").num_columns(3).spacing(Vec2::new(12.0, 4.0)).striped(true).show(ui, |ui| {
                for s in &self.skills {
                    ui.label(guild_skill_name(&s.name));
                    ui.label(format!("Lv {}", s.level));
                    let can = self.master && s.upgradable && self.skill_points > 0;
                    if ui.add_enabled(can, egui::Button::new("+")).clicked() {
                        out.push(Command::Stat(StatRequest::SkillUp(s.id)));
                    }
                    ui.end_row();
                }
            });
        });
        if !self.master {
            ui.label(RichText::new("Only the guild master can raise guild skills.").color(crate::classic::ink(Color32::GRAY)).size(11.5));
        }
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
                    ui.label(RichText::new("No notice.").color(crate::classic::ink(Color32::GRAY)));
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

/// Readable name for a guild skill (the server sends GD_* internal names).
fn guild_skill_name(internal: &str) -> String {
    let known = match internal {
        "GD_APPROVAL" => "Official Guild Approval",
        "GD_KAFRACONTRACT" => "Kafra Contract",
        "GD_GUARDRESEARCH" => "Guardian Research",
        "GD_GUARDUP" => "Strengthen Guardians",
        "GD_EXTENSION" => "Guild Extension",
        "GD_GLORYGUILD" => "Glory of Guild",
        "GD_LEADERSHIP" => "Great Leadership",
        "GD_GLORYWOUNDS" => "Glorious Wounds",
        "GD_SOULCOLD" => "Cold Heart",
        "GD_HAWKEYES" => "Sharp Gaze",
        "GD_BATTLEORDER" => "Battle Orders",
        "GD_REGENERATION" => "Regeneration",
        "GD_RESTORE" => "Restore",
        "GD_EMERGENCYCALL" => "Emergency Call",
        "GD_DEVELOPMENT" => "Permanent Development",
        "GD_ITEMEMERGENCYCALL" => "Emergency Call (Item)",
        "GD_GUILD_STORAGE" => "Guild Storage Expansion",
        "GD_CHARGESHOUT_FLAG" => "Charge Shout Flag",
        "GD_CHARGESHOUT_BEATING" => "Charge Shout Beating",
        "GD_EMERGENCY_MOVE" => "Emergency Move",
        _ => "",
    };
    if known.is_empty() { internal.trim_start_matches("GD_").replace('_', " ") } else { known.to_string() }
}

impl Guild {
    /// The leader's emblem change: 24x24 BMPs from the client's Emblem folder, as the
    /// original client lists them.
    fn emblem_picker(&mut self, ui: &mut egui::Ui, out: &mut Vec<Command>) {
        let Some(dir) = self.client_dir.as_ref().map(|d| d.join("Emblem")) else { return };
        let files = self.emblem_files.get_or_insert_with(|| {
            let mut v: Vec<String> = std::fs::read_dir(&dir)
                .map(|r| r.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.to_lowercase().ends_with(".bmp")).collect())
                .unwrap_or_default();
            v.sort();
            v
        }).clone();
        let mut rescan = false;
        let mut pick = None;
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("emblem_pick").selected_text("Change emblem...").show_ui(ui, |ui| {
                if files.is_empty() {
                    ui.label(format!("Put 24x24 .bmp files in {}", dir.display()));
                }
                for f in files.iter() {
                    if ui.selectable_label(false, f).clicked() {
                        pick = Some(f.clone());
                    }
                }
            });
            rescan = ui.small_button("Rescan").clicked();
        });
        if rescan {
            self.emblem_files = None;
        }
        if let Some(f) = pick {
            out.extend(self.upload_emblem(&f));
        }
        if !self.emblem_status.is_empty() {
            ui.label(RichText::new(&self.emblem_status).size(11.0).color(crate::classic::ink(Color32::GRAY)));
        }
    }

    /// Reads Emblem<f> and makes the upload request (checks it is a 24x24 BMP).
    pub fn upload_emblem(&mut self, f: &str) -> Option<Command> {
        let dir = self.client_dir.as_ref()?.join("Emblem");
        {
            match std::fs::read(dir.join(f)) {
                Ok(bmp) if bmp.len() > 26 && &bmp[..2] == b"BM" && u32::from_le_bytes([bmp[18], bmp[19], bmp[20], bmp[21]]) == 24 && i32::from_le_bytes([bmp[22], bmp[23], bmp[24], bmp[25]]).unsigned_abs() == 24 => {
                    use std::io::Write;
                    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
                    let _ = z.write_all(&bmp);
                    let data = z.finish().ok()?;
                    self.emblem_status = format!("Uploading {f}...");
                    Some(Command::Guild(GuildRequest::SetEmblem(data)))
                }
                Ok(_) => {
                    self.emblem_status = "The emblem must be a 24x24 BMP.".into();
                    None
                }
                Err(e) => {
                    self.emblem_status = format!("Can't read {f}: {e}");
                    None
                }
            }
        }
    }

    /// An emblem arrived (zlib-compressed BMP or GIF).
    pub fn set_emblem(&mut self, ctx: &egui::Context, guild: u32, version: u32, data: &[u8]) {
        if guild != self.id {
            return;
        }
        use std::io::Read;
        let mut raw = vec![];
        if flate2::read::ZlibDecoder::new(data).read_to_end(&mut raw).is_err() {
            return;
        }
        let Ok(img) = ro_formats::image::decode(&raw, "emblem.bmp") else { return };
        // Magenta is transparent, as in other client art.
        let color = egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.pixels);
        self.emblem = Some((version, ctx.load_texture(format!("emblem {guild} {version}"), color, egui::TextureOptions::NEAREST)));
        self.emblem_status.clear();
    }
}

/// Decodes an emblem (zlib-compressed BMP) into a texture.
pub fn decode_emblem(ctx: &egui::Context, guild: u32, version: u32, data: &[u8]) -> Option<egui::TextureHandle> {
    use std::io::Read;
    let mut raw = vec![];
    flate2::read::ZlibDecoder::new(data).read_to_end(&mut raw).ok()?;
    let img = ro_formats::image::decode(&raw, "emblem.bmp").ok()?;
    let color = egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.pixels);
    Some(ctx.load_texture(format!("emblem {guild} {version}"), color, egui::TextureOptions::NEAREST))
}

impl Guild {
    /// The alliance offer prompt: Some(accept?) once answered.
    pub fn show_offer(&mut self, ctx: &egui::Context) -> Option<(u32, bool)> {
        let (inviter, guild) = self.ally_offer.clone()?;
        let mut out = None;
        egui::Window::new("Alliance").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, Vec2::new(0.0, -140.0)).show(ctx, |ui| {
            ui.label(format!("The guild {guild} asks for an alliance."));
            ui.horizontal(|ui| {
                if ui.button("Accept").clicked() {
                    out = Some((inviter, true));
                }
                if ui.button("Decline").clicked() {
                    out = Some((inviter, false));
                }
            });
        });
        if out.is_some() {
            self.ally_offer = None;
        }
        out
    }
}

//! The Adventurer's Agency: the party recruitment board of 2020+ clients. Ads live on
//! rAthena's web server (multipart POSTs to /party/*, authenticated by the token the login
//! server hands out); asking to join and the leader's answer go through the map server.

use std::sync::mpsc;

use egui::{Color32, RichText};
use serde::Deserialize;

use crate::msgstr;

/// Purposes, as the web server numbers them (0 is "All" in searches).
const PURPOSES: [(u16, usize, &str); 6] = [(1, 3360, "Quest"), (2, 3361, "Field"), (3, 3362, "Dungeon"), (4, 3363, "MD"), (5, 3364, "Paradise"), (6, 3365, "Other")];
const PAGE: usize = 8;

fn msg(id: usize, fallback: &str) -> String {
    msgstr::get(id, &[]).unwrap_or_else(|| fallback.into())
}

fn purpose_name(t: u16) -> String {
    PURPOSES.iter().find(|p| p.0 == t).map_or_else(|| msg(3377, "All"), |p| msg(p.1, p.2))
}

/// One ad, as /party/list returns it.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Ad {
    #[serde(rename = "AID")]
    pub aid: u32,
    #[serde(rename = "GID")]
    pub gid: u32,
    pub char_name: String,
    #[serde(rename = "MinLV")]
    pub min_lv: u16,
    #[serde(rename = "MaxLV")]
    pub max_lv: u16,
    #[serde(rename = "Type")]
    pub purpose: u16,
    pub tanker: u8,
    pub dealer: u8,
    pub healer: u8,
    pub assist: u8,
    pub memo: String,
}

impl Ad {
    fn roles(&self) -> String {
        [(self.tanker, 3420, "Tank"), (self.dealer, 3444, "Dealer"), (self.healer, 3491, "Healer"), (self.assist, 3508, "Support")]
            .iter()
            .filter(|r| r.0 != 0)
            .map(|r| msg(r.1, r.2))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[derive(Deserialize)]
struct ListReply {
    #[serde(default)]
    data: Vec<Ad>,
}

/// Who we are to the web server.
#[derive(Clone, Default)]
pub struct Login {
    pub url: String,
    pub aid: u32,
    pub token: String,
    pub world: String,
}

enum Job {
    List,
    Add(Ad),
    Delete,
}

enum Reply {
    Listed(Vec<Ad>),
    Added,
    Deleted,
    /// (msgstringtable id, fallback) to show.
    Failed(usize, &'static str),
}

fn post(login: &Login, path: &str, fields: &[(&str, String)]) -> Result<String, String> {
    const BOUNDARY: &str = "----roclientAgencyBoundary";
    let mut body = String::new();
    let base = [("AID", login.aid.to_string()), ("AuthToken", login.token.clone()), ("WorldName", login.world.clone())];
    for (k, v) in base.iter().map(|(k, v)| (*k, v)).chain(fields.iter().map(|(k, v)| (*k, v))) {
        body.push_str(&format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{v}\r\n"));
    }
    body.push_str(&format!("--{BOUNDARY}--\r\n"));
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(5))).build().into();
    let mut resp = agent
        .post(&format!("{}{path}", login.url))
        .header("Content-Type", &format!("multipart/form-data; boundary={BOUNDARY}"))
        .send(body.as_bytes())
        .map_err(|e| e.to_string())?;
    resp.body_mut().read_to_string().map_err(|e| e.to_string())
}

fn run(login: &Login, job: Job) -> Reply {
    let result = match &job {
        Job::List => post(login, "/party/list", &[]).and_then(|t| serde_json::from_str::<ListReply>(&t).map(|r| Reply::Listed(r.data)).map_err(|e| format!("{e}: {t}"))),
        Job::Add(ad) => {
            let b = |v: u8| v.to_string();
            let fields = [
                ("GID", ad.gid.to_string()),
                ("CharName", ad.char_name.clone()),
                ("Type", ad.purpose.to_string()),
                ("MinLV", ad.min_lv.to_string()),
                ("MaxLV", ad.max_lv.to_string()),
                ("Tanker", b(ad.tanker)),
                ("Dealer", b(ad.dealer)),
                ("Healer", b(ad.healer)),
                ("Assist", b(ad.assist)),
                ("Memo", ad.memo.clone()),
            ];
            post(login, "/party/add", &fields).map(|_| Reply::Added)
        }
        Job::Delete => post(login, "/party/del", &[]).map(|_| Reply::Deleted),
    };
    result.unwrap_or_else(|e| {
        tracing::warn!(error = %e, "adventurer's agency request failed");
        match job {
            Job::Add(_) => Reply::Failed(3417, "Failed to register for the Adventurer's Agency."),
            _ => Reply::Failed(3337, "Could not reach the Adventurer's Agency."),
        }
    })
}

/// What the rest of the client knows about us.
pub struct Me<'a> {
    pub char_id: u32,
    pub name: &'a str,
    pub level: u16,
    /// None: not in a party; Some(true): we lead it.
    pub leader: Option<bool>,
}

pub enum AgencyAction {
    Join { char_id: u32, account_id: u32 },
    Whisper(String),
    Notice(String),
}

pub struct Agency {
    pub open: bool,
    login: Option<Login>,
    jobs: Option<mpsc::Sender<(Login, Job)>>,
    replies: Option<mpsc::Receiver<Reply>>,
    ads: Vec<Ad>,
    busy: bool,
    page: usize,
    /// Search: purpose (0 any), roles, text, and only ads our level fits.
    purpose: u16,
    roles: [bool; 4],
    text: String,
    fits: bool,
    /// The registration form, when open.
    form: Option<Ad>,
    status: String,
}

impl Default for Agency {
    fn default() -> Self {
        Agency {
            open: false,
            login: None,
            jobs: None,
            replies: None,
            ads: vec![],
            busy: false,
            page: 0,
            purpose: 0,
            roles: [false; 4],
            text: String::new(),
            fits: false,
            form: None,
            status: String::new(),
        }
    }
}

impl Agency {
    pub fn set_login(&mut self, login: Login) {
        self.login = Some(login);
        self.ads.clear();
    }

    fn submit(&mut self, job: Job) {
        let Some(login) = self.login.clone() else {
            self.status = msg(3337, "Could not reach the Adventurer's Agency.");
            return;
        };
        if self.jobs.is_none() {
            let (jtx, jrx) = mpsc::channel::<(Login, Job)>();
            let (rtx, rrx) = mpsc::channel();
            let spawned = std::thread::Builder::new().name("agency".into()).spawn(move || {
                for (login, job) in jrx {
                    if rtx.send(run(&login, job)).is_err() {
                        break;
                    }
                }
            });
            if spawned.is_err() {
                return;
            }
            self.jobs = Some(jtx);
            self.replies = Some(rrx);
        }
        if let Some(tx) = &self.jobs {
            self.busy = tx.send((login, job)).is_ok();
        }
    }

    /// Registers (or updates) our party's ad without the form (scripted tests).
    pub fn register(&mut self, ad: Ad) {
        self.submit(Job::Add(ad));
    }

    pub fn find(&self, leader: &str) -> Option<&Ad> {
        self.ads.iter().find(|a| a.char_name == leader)
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
        if self.open {
            self.submit(Job::List);
        }
    }

    /// Takes finished web requests; returns lines for the chat log.
    pub fn poll(&mut self) -> Vec<String> {
        let mut out = vec![];
        let mut relist = false;
        while let Some(r) = self.replies.as_ref().and_then(|r| r.try_recv().ok()) {
            self.busy = false;
            match r {
                Reply::Listed(ads) => {
                    self.ads = ads;
                    self.page = self.page.min(self.ads.len().saturating_sub(1) / PAGE);
                }
                Reply::Added => {
                    out.push(msg(3418, "Your party has been registered in the Adventurer's Agency."));
                    self.form = None;
                    relist = true;
                }
                Reply::Deleted => {
                    out.push(msg(3459, "Party recruitment has been canceled."));
                    relist = true;
                }
                Reply::Failed(id, fallback) => {
                    self.status = msg(id, fallback);
                    out.push(self.status.clone());
                }
            }
        }
        if relist {
            self.submit(Job::List);
        }
        out
    }

    fn visible(&self, me: &Me) -> Vec<&Ad> {
        let text = self.text.trim().to_lowercase();
        self.ads
            .iter()
            .filter(|a| self.purpose == 0 || a.purpose == self.purpose)
            .filter(|a| {
                let want = [a.tanker, a.dealer, a.healer, a.assist];
                !self.roles.iter().any(|r| *r) || self.roles.iter().zip(want).any(|(r, w)| *r && w != 0)
            })
            .filter(|a| text.is_empty() || a.memo.to_lowercase().contains(&text) || a.char_name.to_lowercase().contains(&text))
            .filter(|a| !self.fits || (a.min_lv..=a.max_lv).contains(&me.level))
            .collect()
    }

    pub fn show(&mut self, ctx: &egui::Context, me: &Me) -> Vec<AgencyAction> {
        let mut out = vec![];
        if !self.open {
            return out;
        }
        let title = msg(3504, "Adventurer's Agency");
        let mut open = true;
        let mine = self.ads.iter().find(|a| a.gid == me.char_id).cloned();
        crate::classic::show(ctx, egui::Window::new(&title).id(egui::Id::unique("agency")).open(&mut open).resizable(false).default_width(520.0).default_pos(egui::pos2(380.0, 140.0)), &title, true, |ui| {
            ui.horizontal(|ui| {
                ui.label(msg(3369, "Purpose"));
                egui::ComboBox::from_id_salt("agency_purpose").selected_text(purpose_name(self.purpose)).show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.purpose, 0, msg(3377, "All"));
                    for p in PURPOSES {
                        ui.selectable_value(&mut self.purpose, p.0, msg(p.1, p.2));
                    }
                });
                for (i, (id, en)) in [(3420, "Tank"), (3444, "Dealer"), (3491, "Healer"), (3508, "Support")].into_iter().enumerate() {
                    ui.checkbox(&mut self.roles[i], msg(id, en));
                }
            });
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.text).hint_text(msg(3419, "Enter search details")).desired_width(200.0));
                ui.checkbox(&mut self.fits, format!("Lv {}", me.level)).on_hover_text("Only parties your level can join");
                if ui.add_enabled(!self.busy, egui::Button::new(msg(3366, "Search"))).clicked() {
                    self.page = 0;
                    self.submit(Job::List);
                }
                if ui.button(msg(3367, "Reset")).clicked() {
                    (self.purpose, self.roles, self.fits, self.page) = (0, [false; 4], false, 0);
                    self.text.clear();
                }
            });
            if !self.status.is_empty() {
                ui.label(RichText::new(&self.status).color(crate::classic::ink(Color32::from_rgb(255, 210, 110))));
            }
            ui.separator();
            let shown = self.visible(me);
            let pages = shown.len().div_ceil(PAGE).max(1);
            let page = self.page.min(pages - 1);
            if shown.is_empty() {
                ui.label(RichText::new(msg(3443, "No party can be found.")).color(crate::classic::ink(Color32::GRAY)));
            }
            let mut join = None;
            for ad in shown.iter().skip(page * PAGE).take(PAGE) {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&ad.char_name).strong());
                        ui.label(RichText::new(format!("[{}]", purpose_name(ad.purpose))).color(crate::classic::ink(Color32::from_rgb(120, 190, 255))));
                        ui.label(format!("Lv {} ~ {}", ad.min_lv, ad.max_lv));
                        let roles = ad.roles();
                        if !roles.is_empty() {
                            ui.label(RichText::new(roles).size(11.5).color(crate::classic::ink(Color32::from_rgb(150, 220, 150))));
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ad.gid != me.char_id {
                                if ui.small_button(msg(3374, "PM")).clicked() {
                                    out.push(AgencyAction::Whisper(ad.char_name.clone()));
                                }
                                if ui.small_button(msg(3373, "Join")).clicked() {
                                    join = Some((*ad).clone());
                                }
                            }
                        });
                    });
                    if !ad.memo.is_empty() {
                        ui.label(RichText::new(&ad.memo).size(11.5).color(crate::classic::ink(Color32::LIGHT_GRAY)));
                    }
                });
            }
            if let Some(ad) = join {
                out.push(match me.leader {
                    Some(true) => AgencyAction::Notice(msg(3515, "You can not request to join, if you are a party leader.")),
                    Some(false) => AgencyAction::Notice(msgstr::get(3500, &[me.name]).unwrap_or_else(|| format!("[{}] is already in the party.", me.name))),
                    None if !(ad.min_lv..=ad.max_lv).contains(&me.level) => AgencyAction::Notice(msg(3416, "You don't meet the required level to join the party.")),
                    None => {
                        self.status = msg(3487, "Sent a request to join the party.");
                        AgencyAction::Join { char_id: ad.gid, account_id: ad.aid }
                    }
                });
            }
            ui.horizontal(|ui| {
                if ui.add_enabled(page > 0, egui::Button::new("<")).clicked() {
                    self.page = page - 1;
                }
                ui.label(format!("{} / {pages}", page + 1));
                if ui.add_enabled(page + 1 < pages, egui::Button::new(">")).clicked() {
                    self.page = page + 1;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(ad) = &mine {
                        if ui.add_enabled(!self.busy, egui::Button::new(msg(3375, "Cancel Recruitment"))).clicked() {
                            self.submit(Job::Delete);
                        }
                        if ui.button(msg(3376, "Edit")).clicked() {
                            self.form = Some(ad.clone());
                        }
                    } else if ui.button(msg(3356, "Register in the Adventurer's Agency")).clicked() {
                        if me.leader == Some(true) {
                            self.form = Some(Ad {
                                gid: me.char_id,
                                char_name: me.name.into(),
                                purpose: 1,
                                min_lv: me.level.saturating_sub(10).max(1),
                                max_lv: me.level.saturating_add(10),
                                ..Default::default()
                            });
                        } else {
                            out.push(AgencyAction::Notice(msg(3506, "If you are not a party leader, you can not register party at the Adventurer's Agency.")));
                        }
                    }
                });
            });
        });
        if !open {
            self.open = false;
            self.form = None;
        }
        self.form_window(ctx);
        out
    }

    fn form_window(&mut self, ctx: &egui::Context) {
        let Some(mut ad) = self.form.take() else { return };
        let title = msg(3368, "Registration for Party");
        let mut open = true;
        let mut submit = false;
        crate::classic::show(ctx, egui::Window::new(&title).id(egui::Id::unique("agency_form")).open(&mut open).resizable(false).collapsible(false).default_pos(egui::pos2(460.0, 220.0)), &title, false, |ui| {
            egui::Grid::new("agency_form_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label(msg(3369, "Purpose"));
                egui::ComboBox::from_id_salt("agency_form_purpose").selected_text(purpose_name(ad.purpose)).show_ui(ui, |ui| {
                    for p in PURPOSES {
                        ui.selectable_value(&mut ad.purpose, p.0, msg(p.1, p.2));
                    }
                });
                ui.end_row();
                ui.label("Level");
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut ad.min_lv).range(1..=ad.max_lv.max(1)));
                    ui.label("~");
                    ui.add(egui::DragValue::new(&mut ad.max_lv).range(ad.min_lv..=999));
                });
                ui.end_row();
                ui.label("");
                ui.horizontal(|ui| {
                    for (v, id, en) in [(&mut ad.tanker, 3420, "Tank"), (&mut ad.dealer, 3444, "Dealer"), (&mut ad.healer, 3491, "Healer"), (&mut ad.assist, 3508, "Support")] {
                        let mut on = *v != 0;
                        ui.checkbox(&mut on, msg(id, en));
                        *v = on as u8;
                    }
                });
                ui.end_row();
                ui.label(msg(3359, "Note"));
                ui.add(egui::TextEdit::multiline(&mut ad.memo).desired_rows(3).desired_width(260.0).char_limit(250));
                ui.end_row();
            });
            ui.label(RichText::new(msg(3370, "* Please respect the rules of the server.")).size(11.0).color(crate::classic::ink(Color32::GRAY)));
            submit = ui.add_enabled(!self.busy, egui::Button::new(msg(3371, "Register"))).clicked();
        });
        if submit {
            // The web server writes the memo into its JSON unescaped.
            ad.memo = ad.memo.chars().filter(|c| !matches!(c, '"' | '\\') && !c.is_control()).collect();
            self.submit(Job::Add(ad.clone()));
        }
        if open {
            self.form = Some(ad);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_web_list() {
        let t = r#"{"Type":1,"totalPage":1,"data":[{"AID":2000000,"GID":150004,"CharName":"Hayate","WorldName":"rAthena","Tanker":1,"Healer":0,"Dealer":1,"Assist":0,"MinLV":90,"MaxLV":110,"Memo":"ET run","Type":3}]}"#;
        let r: ListReply = serde_json::from_str(t).unwrap();
        assert_eq!(r.data.len(), 1);
        let a = &r.data[0];
        assert_eq!((a.aid, a.gid, a.min_lv, a.max_lv, a.purpose, a.tanker, a.dealer), (2000000, 150004, 90, 110, 3, 1, 1));
        assert_eq!(a.char_name, "Hayate");
        let empty: ListReply = serde_json::from_str(r#"{"Type":1,"totalPage":0}"#).unwrap();
        assert!(empty.data.is_empty());
    }
}

//! The mailbox (RODEX, Alt+X): the inbox, reading a mail and taking its zeny and items,
//! deleting, and writing a mail (receiver looked up first, then sent).

use std::collections::HashMap;

use egui::{Color32, RichText, Vec2};
use ro_game::items::ItemDb;
use ro_net::{MailHeader, MailItem, MailRequest};

#[derive(Default)]
pub struct Mailbox {
    pub open: bool,
    pub inbox: Vec<MailHeader>,
    /// The mail being read: (header, text, zeny, items).
    pub reading: Option<(MailHeader, String, i64, Vec<MailItem>)>,
    /// New mail waiting.
    pub unread: bool,
    writing: bool,
    to: String,
    title: String,
    text: String,
    zeny: String,
    /// A send waiting on the receiver lookup.
    pub pending_send: bool,
    pub status: String,
}

fn ink(c: Color32) -> Color32 {
    crate::classic::ink(c)
}

impl Mailbox {
    pub fn item_ids(&self) -> Vec<u32> {
        self.reading.as_ref().map(|r| r.3.iter().map(|i| i.id).collect()).unwrap_or_default()
    }

    /// The receiver was looked up: the mail to send, if the lookup found them.
    pub fn receiver_found(&mut self, id: u32, me: &str) -> Option<MailRequest> {
        if !std::mem::take(&mut self.pending_send) {
            return None;
        }
        if id == 0 {
            self.status = format!("There is no character named {}.", self.to.trim());
            return None;
        }
        let zeny = self.zeny.trim().parse().unwrap_or(0);
        Some(MailRequest::Send { to: self.to.trim().into(), to_id: id, from: me.into(), title: self.title.trim().into(), text: self.text.clone(), zeny })
    }

    /// Fills the write form and starts sending (test scripts).
    pub fn compose(&mut self, to: &str, title: &str, text: &str, zeny: &str) -> [MailRequest; 2] {
        self.writing = true;
        self.to = to.into();
        self.title = title.into();
        self.text = text.into();
        self.zeny = zeny.into();
        self.pending_send = true;
        [MailRequest::BeginWrite(to.into()), MailRequest::CheckName(to.into())]
    }

    pub fn sent(&mut self, result: u8) {
        if result == 0 {
            self.status = "Mail sent.".into();
            self.writing = false;
            self.title.clear();
            self.text.clear();
            self.zeny.clear();
        } else {
            self.status = "The mail could not be sent.".into();
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, db: Option<&ItemDb>, icons: &HashMap<u32, egui::TextureHandle>, zeny: i64) -> Vec<MailRequest> {
        let mut out = vec![];
        if !self.open {
            return out;
        }
        let mut open = true;
        crate::classic::show(ctx, egui::Window::new("Mail").open(&mut open).fixed_size(Vec2::new(640.0, 380.0)).default_pos(egui::pos2(360.0, 140.0)), "Mail", true, |ui| {
            ui.horizontal(|ui| {
                if ui.selectable_label(!self.writing, "Inbox").clicked() {
                    if self.writing {
                        out.push(MailRequest::CancelWrite);
                    }
                    self.writing = false;
                }
                if ui.selectable_label(self.writing, "Write").clicked() && !self.writing {
                    self.writing = true;
                    out.push(MailRequest::BeginWrite(String::new()));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Refresh").clicked() {
                        out.push(MailRequest::Open { refresh: true });
                    }
                    if !self.status.is_empty() {
                        ui.label(RichText::new(&self.status).color(ink(Color32::from_rgb(255, 210, 110))));
                    }
                });
            });
            ui.separator();
            if self.writing {
                egui::Grid::new("mail_write").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                    ui.label("To");
                    ui.add(egui::TextEdit::singleline(&mut self.to).desired_width(220.0));
                    ui.end_row();
                    ui.label("Title");
                    ui.add(egui::TextEdit::singleline(&mut self.title).desired_width(480.0).char_limit(39));
                    ui.end_row();
                    ui.label("Message");
                    ui.add(egui::TextEdit::multiline(&mut self.text).desired_width(480.0).desired_rows(8).char_limit(499));
                    ui.end_row();
                    ui.label("Zeny");
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.zeny).desired_width(120.0));
                        ui.label(RichText::new(format!("(you have {zeny} z)")).color(ink(Color32::GRAY)));
                    });
                    ui.end_row();
                });
                let z = self.zeny.trim();
                let zeny_ok = z.is_empty() || z.parse::<i64>().is_ok_and(|v| v >= 0 && v <= zeny);
                let ok = !self.to.trim().is_empty() && !self.title.trim().is_empty() && zeny_ok && !self.pending_send;
                if ui.add_enabled(ok, egui::Button::new("Send")).clicked() {
                    self.pending_send = true;
                    self.status = "Sending...".into();
                    out.push(MailRequest::CheckName(self.to.trim().into()));
                }
                return;
            }
            ui.horizontal_top(|ui| {
                // The inbox list.
                ui.vertical(|ui| {
                    ui.set_width(250.0);
                    egui::ScrollArea::vertical().id_salt("mail_inbox").max_height(320.0).auto_shrink([false, false]).show(ui, |ui| {
                        if self.inbox.is_empty() {
                            ui.label(RichText::new("No mail.").color(ink(Color32::GRAY)));
                        }
                        for m in &self.inbox {
                            let selected = self.reading.as_ref().is_some_and(|r| r.0.id == m.id);
                            let mut title = RichText::new(&m.title);
                            if !m.read {
                                title = title.strong();
                            }
                            let resp = ui.selectable_label(selected, title);
                            let extra = match (m.kind & 2 != 0, m.kind & 4 != 0) {
                                (true, true) => "  zeny + items",
                                (true, false) => "  zeny",
                                (false, true) => "  items",
                                _ => "",
                            };
                            ui.label(RichText::new(format!("from {}{extra}", m.sender)).size(11.0).color(ink(Color32::GRAY)));
                            if resp.clicked() {
                                out.push(MailRequest::Read { tab: m.tab, id: m.id });
                            }
                            ui.add_space(2.0);
                        }
                    });
                });
                ui.separator();
                // The open mail.
                ui.vertical(|ui| {
                    let Some((h, text, zeny, items)) = &self.reading else {
                        ui.label(RichText::new("Choose a mail.").color(ink(Color32::GRAY)));
                        return;
                    };
                    ui.label(RichText::new(&h.title).strong().size(15.0));
                    ui.label(RichText::new(format!("From {}", h.sender)).color(ink(Color32::GRAY)));
                    ui.separator();
                    egui::ScrollArea::vertical().id_salt("mail_text").max_height(150.0).show(ui, |ui| {
                        ui.label(text);
                    });
                    ui.separator();
                    if *zeny > 0 {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("{zeny} z")).color(ink(Color32::from_rgb(255, 210, 110))));
                            if ui.button("Take zeny").clicked() {
                                out.push(MailRequest::TakeZeny { tab: h.tab, id: h.id });
                            }
                        });
                    }
                    if !items.is_empty() {
                        for it in items {
                            ui.horizontal(|ui| {
                                if let Some(t) = icons.get(&it.id) {
                                    ui.image((t.id(), Vec2::splat(24.0)));
                                }
                                let name = db.map_or_else(|| format!("Item #{}", it.id), |d| d.name(it.id, true));
                                let name = if it.refine > 0 { format!("+{} {name}", it.refine) } else { name };
                                ui.label(format!("{name}  x{}", it.amount));
                            });
                        }
                        if ui.button("Take items").clicked() {
                            out.push(MailRequest::TakeItems { tab: h.tab, id: h.id });
                        }
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("Delete").clicked() {
                            out.push(MailRequest::Delete { tab: h.tab, id: h.id });
                        }
                        if h.kind & 8 == 0 && ui.button("Reply").clicked() {
                            self.to = h.sender.clone();
                            self.title = format!("Re: {}", h.title).chars().take(39).collect();
                            self.writing = true;
                            out.push(MailRequest::BeginWrite(h.sender.clone()));
                        }
                    });
                });
            });
        });
        if !open {
            self.open = false;
            if self.writing {
                self.writing = false;
                out.push(MailRequest::CancelWrite);
            }
        }
        out
    }
}

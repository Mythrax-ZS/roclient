//! Chat rooms: the signs over their owners (click to join, asking for a password when
//! private), the room window while we are in one (members, kick, hand over, leave) and
//! the create form.

use std::collections::HashMap;

use egui::{Color32, RichText};
use ro_net::{ChatRoomRequest, Room};

#[derive(Default)]
pub struct ChatRooms {
    /// Rooms by owner unit id.
    pub signs: HashMap<u32, Room>,
    /// The room we are in: (room id, members as (owner?, name)).
    pub inside: Option<(u32, Vec<(bool, String)>)>,
    /// The create form: (title, limit, public, password).
    pub creating: Option<(String, u16, bool, String)>,
    /// Joining a private room: (room id, password typed).
    asking: Option<(u32, String)>,
}

impl ChatRooms {
    pub fn room(&self, id: u32) -> Option<&Room> {
        self.signs.values().find(|r| r.id == id)
    }

    pub fn open_create(&mut self, me: &str) {
        self.creating.get_or_insert_with(|| (format!("{me}'s room"), 10, true, String::new()));
    }

    /// Signs over the owners; returns a join request when one is clicked.
    pub fn show_signs(&mut self, ctx: &egui::Context, at: &[(u32, glam::Vec2)]) -> Option<ChatRoomRequest> {
        let ppp = ctx.pixels_per_point();
        let mut out = None;
        for (owner, p) in at {
            let Some(r) = self.signs.get(owner) else { continue };
            if self.inside.as_ref().is_some_and(|(id, _)| *id == r.id) {
                continue;
            }
            egui::Area::new(egui::Id::unique(("room_sign", *owner))).fixed_pos(egui::pos2(p.x / ppp, p.y / ppp)).pivot(egui::Align2::CENTER_BOTTOM).show(ctx, |ui| {
                let lock = if r.kind == 0 { "[locked] " } else { "" };
                let text = RichText::new(format!("{lock}{} ({}/{})", r.title, r.users, r.limit)).color(Color32::BLACK).size(12.0);
                let b = egui::Button::new(text).fill(Color32::from_rgb(200, 245, 200));
                if r.kind != 3 && ui.add(b).on_hover_text("Click to join").clicked() {
                    if r.kind == 0 {
                        self.asking = Some((r.id, String::new()));
                    } else {
                        out = Some(ChatRoomRequest::Join { id: r.id, password: String::new() });
                    }
                }
            });
        }
        out
    }

    pub fn show(&mut self, ctx: &egui::Context, me: &str) -> Vec<ChatRoomRequest> {
        let mut out = vec![];
        if let Some((id, pass)) = self.asking.as_mut() {
            let mut open = true;
            let mut go = false;
            crate::classic::show(ctx, egui::Window::new("Room password").open(&mut open).collapsible(false).resizable(false), "Room password", true, |ui| {
                ui.add(egui::TextEdit::singleline(pass).password(true).char_limit(8).desired_width(140.0));
                go = ui.button("Join").clicked();
            });
            if go {
                out.push(ChatRoomRequest::Join { id: *id, password: pass.clone() });
            }
            if go || !open {
                self.asking = None;
            }
        }
        if let Some((title, limit, public, pass)) = self.creating.as_mut() {
            let mut open = true;
            let mut create = false;
            crate::classic::show(ctx, egui::Window::new("Create Chat Room").open(&mut open).collapsible(false).resizable(false), "Create Chat Room", true, |ui| {
                egui::Grid::new("room_new").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                    ui.label("Title");
                    ui.add(egui::TextEdit::singleline(title).char_limit(36).desired_width(220.0));
                    ui.end_row();
                    ui.label("People");
                    ui.add(egui::DragValue::new(limit).range(2..=20));
                    ui.end_row();
                    ui.label("Access");
                    ui.horizontal(|ui| {
                        ui.selectable_value(public, true, "Public");
                        ui.selectable_value(public, false, "Private");
                    });
                    ui.end_row();
                    if !*public {
                        ui.label("Password");
                        ui.add(egui::TextEdit::singleline(pass).password(true).char_limit(8).desired_width(120.0));
                        ui.end_row();
                    }
                });
                create = ui.add_enabled(!title.trim().is_empty(), egui::Button::new("Create")).clicked();
            });
            if create {
                out.push(ChatRoomRequest::Create { title: title.trim().into(), limit: *limit, public: *public, password: pass.clone(), change: false });
            }
            if create || !open {
                self.creating = None;
            }
        }
        if let Some((id, members)) = &self.inside {
            let title = self.room(*id).map_or_else(|| "Chat Room".to_string(), |r| r.title.clone());
            let owner = members.iter().any(|(o, n)| *o && n == me);
            let mut open = true;
            crate::classic::show(ctx, egui::Window::new(title.as_str()).id(egui::Id::unique("chat_room")).open(&mut open).resizable(false).default_width(260.0), &title, true, |ui| {
                ui.label(RichText::new("Talk in the chat box; only the room hears you.").size(11.0).color(crate::classic::ink(Color32::GRAY)));
                ui.separator();
                for (is_owner, name) in members {
                    ui.horizontal(|ui| {
                        ui.label(if *is_owner { RichText::new(format!("{name}  (owner)")).strong() } else { RichText::new(name) });
                        if owner && name != me {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("Kick").clicked() {
                                    out.push(ChatRoomRequest::Kick(name.clone()));
                                }
                                if ui.small_button("Make owner").clicked() {
                                    out.push(ChatRoomRequest::GiveOwner(name.clone()));
                                }
                            });
                        }
                    });
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button("Leave").clicked() {
                        out.push(ChatRoomRequest::Leave);
                    }
                });
            });
            if !open {
                out.push(ChatRoomRequest::Leave);
            }
        }
        out
    }
}

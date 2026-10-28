//! In-game HUD: character status, chat, and names over actors. Drawn with egui on top of
//! the world; layout scales with the window's DPI and the configured UI scale.

use std::collections::VecDeque;

use egui::{Align2, Color32, CornerRadius, FontId, Frame, Margin, RichText, Stroke, Vec2};
use ro_net::{sp, ChatKind};

const CHAT_LINES: usize = 200;

/// The local character's numbers, as the server reports them.
#[derive(Default, Clone)]
pub struct Status {
    pub name: String,
    pub job: String,
    pub base_level: i64,
    pub job_level: i64,
    pub hp: i64,
    pub max_hp: i64,
    pub sp: i64,
    pub max_sp: i64,
    pub base_exp: i64,
    pub next_base_exp: i64,
    pub job_exp: i64,
    pub next_job_exp: i64,
    pub zeny: i64,
    pub skill_points: i64,
    pub weight: i64,
    pub max_weight: i64,
}

impl Status {
    pub fn set(&mut self, id: u16, value: i64) {
        match id {
            sp::HP => self.hp = value,
            sp::MAXHP => self.max_hp = value,
            sp::SP => self.sp = value,
            sp::MAXSP => self.max_sp = value,
            sp::BASELEVEL => self.base_level = value,
            sp::JOBLEVEL => self.job_level = value,
            sp::BASEEXP => self.base_exp = value,
            sp::NEXTBASEEXP => self.next_base_exp = value,
            sp::JOBEXP => self.job_exp = value,
            sp::NEXTJOBEXP => self.next_job_exp = value,
            sp::ZENY => self.zeny = value,
            12 => self.skill_points = value, // SP_SKILLPOINT
            sp::WEIGHT => self.weight = value,
            sp::MAXWEIGHT => self.max_weight = value,
            _ => {}
        }
    }
}

/// A name label for an actor on screen.
pub struct Label {
    /// Window pixels (physical) of the actor's feet.
    pub feet: glam::Vec2,
    pub text: String,
    pub color: Color32,
}

#[derive(Default)]
pub struct Hud {
    pub status: Status,
    chat: VecDeque<(ChatKind, String)>,
    input: String,
    /// Enter focuses the chat box; Enter again sends, Escape leaves.
    want_focus: bool,
    /// Lines sent, oldest first; Up/Down in the chat box walk through them.
    history: Vec<String>,
    /// Position while browsing history (None: editing a fresh line).
    recall: Option<usize>,
}

fn chat_color(kind: ChatKind) -> Color32 {
    match kind {
        ChatKind::Public => Color32::from_rgb(235, 235, 235),
        ChatKind::Npc => Color32::from_rgb(150, 220, 255),
        ChatKind::Whisper => Color32::from_rgb(255, 230, 120),
        ChatKind::Broadcast => Color32::from_rgb(255, 255, 120),
        ChatKind::System => Color32::from_rgb(170, 255, 170),
        ChatKind::Party => Color32::from_rgb(255, 200, 220),
        ChatKind::Guild => Color32::from_rgb(180, 255, 200),
    }
}

fn panel() -> Frame {
    Frame::new()
        .fill(Color32::from_rgba_unmultiplied(16, 20, 28, 210))
        .stroke(Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 30)))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(10))
}

/// A labelled bar like "HP 120 / 200".
fn bar(ui: &mut egui::Ui, label: &str, value: i64, max: i64, color: Color32, show_numbers: bool) {
    let frac = if max > 0 { (value as f32 / max as f32).clamp(0.0, 1.0) } else { 0.0 };
    let text = if show_numbers { format!("{label}  {value} / {max}") } else { format!("{label}  {:.1}%", frac * 100.0) };
    ui.add(
        egui::ProgressBar::new(frac)
            .fill(color)
            .desired_height(16.0)
            .corner_radius(CornerRadius::same(4))
            .text(RichText::new(text).size(11.0).color(Color32::WHITE)),
    );
}

impl Hud {
    pub fn push_chat(&mut self, kind: ChatKind, text: String) {
        self.chat.push_back((kind, text));
        while self.chat.len() > CHAT_LINES {
            self.chat.pop_front();
        }
    }

    /// True while the chat box has keyboard focus (game hotkeys should yield).
    pub fn focus_chat(&mut self) {
        self.want_focus = true;
    }

    /// Draws the HUD; returns a chat line the player submitted.
    /// `floaters`: (screen pos, text, colour, age ms) damage numbers; `bars`: (feet, hp fraction).
    /// `casts`: (feet, progress 0..1) cast bars drawn over the caster's head.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        labels: &[Label],
        floaters: &[(glam::Vec2, String, Color32, f32)],
        bars: &[(glam::Vec2, f32)],
        casts: &[(glam::Vec2, f32)],
    ) -> Option<String> {
        // Names under actors, outlined for readability on any background.
        let painter = ctx.layer_painter(egui::LayerId::background());
        let ppp = ctx.pixels_per_point();
        for l in labels {
            let pos = egui::pos2(l.feet.x / ppp, l.feet.y / ppp + 6.0);
            let font = FontId::proportional(12.0);
            for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
                painter.text(pos + Vec2::new(dx, dy), Align2::CENTER_TOP, &l.text, font.clone(), Color32::BLACK);
            }
            painter.text(pos, Align2::CENTER_TOP, &l.text, font, l.color);
        }

        // HP bars under monsters that have been hit.
        for &(feet, frac) in bars {
            let c = egui::pos2(feet.x / ppp, feet.y / ppp + 4.0);
            let rect = egui::Rect::from_center_size(c, Vec2::new(40.0, 5.0));
            painter.rect_filled(rect.expand(1.0), 1.0, Color32::BLACK);
            let mut fill = rect;
            fill.set_width(rect.width() * frac.clamp(0.0, 1.0));
            painter.rect_filled(fill, 1.0, Color32::from_rgb(220, 60, 60));
        }
        for &(feet, frac) in casts {
            let c = egui::pos2(feet.x / ppp, feet.y / ppp - 100.0);
            let rect = egui::Rect::from_center_size(c, Vec2::new(60.0, 6.0));
            painter.rect_filled(rect.expand(1.0), 2.0, Color32::BLACK);
            let mut fill = rect;
            fill.set_width(rect.width() * frac.clamp(0.0, 1.0));
            painter.rect_filled(fill, 2.0, Color32::from_rgb(90, 200, 90));
        }
        // Damage numbers rise and fade over their lifetime.
        for (pos, text, color, age) in floaters {
            let t = age / 1200.0;
            let p = egui::pos2(pos.x / ppp, pos.y / ppp - 60.0 - t * 40.0);
            let alpha = ((1.0 - t) * 255.0) as u8;
            let font = FontId::proportional(20.0);
            for (dx, dy) in [(-1.5, 0.0), (1.5, 0.0), (0.0, -1.5), (0.0, 1.5)] {
                painter.text(p + Vec2::new(dx, dy), Align2::CENTER_BOTTOM, text, font.clone(), Color32::from_black_alpha(alpha));
            }
            painter.text(p, Align2::CENTER_BOTTOM, text, font, Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha));
        }

        let s = &self.status;
        egui::Window::new("status")
            .title_bar(false)
            .resizable(false)
            .anchor(Align2::LEFT_TOP, Vec2::new(12.0, 12.0))
            .frame(panel())
            .show(ctx, |ui| {
                ui.set_width(250.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&s.name).strong().size(15.0).color(Color32::WHITE));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(format!("Lv {} / {}", s.base_level, s.job_level)).color(Color32::LIGHT_GRAY));
                    });
                });
                ui.label(RichText::new(&s.job).size(11.0).color(Color32::GRAY));
                ui.add_space(4.0);
                bar(ui, "HP", s.hp, s.max_hp, Color32::from_rgb(70, 190, 90), true);
                bar(ui, "SP", s.sp, s.max_sp, Color32::from_rgb(70, 120, 220), true);
                ui.add_space(2.0);
                bar(ui, "Base", s.base_exp, s.next_base_exp, Color32::from_rgb(200, 160, 60), false);
                bar(ui, "Job", s.job_exp, s.next_job_exp, Color32::from_rgb(160, 110, 200), false);
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{} z", s.zeny)).size(11.0).color(Color32::from_rgb(240, 210, 120)));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let heavy = s.max_weight > 0 && s.weight * 2 >= s.max_weight;
                        let c = if heavy { Color32::from_rgb(255, 140, 90) } else { Color32::LIGHT_GRAY };
                        ui.label(RichText::new(format!("Weight {} / {}", s.weight / 10, s.max_weight / 10)).size(11.0).color(c));
                    });
                });
            });

        let mut sent = None;
        egui::Window::new("chat")
            .title_bar(false)
            .resizable(true)
            .default_size(Vec2::new(460.0, 190.0))
            .anchor(Align2::LEFT_BOTTOM, Vec2::new(12.0, -12.0))
            .frame(panel())
            .show(ctx, |ui| {
                let input_h = 30.0;
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .auto_shrink([false, false])
                    .max_height(ui.available_height() - input_h)
                    .show(ui, |ui| {
                        for (kind, text) in &self.chat {
                            ui.label(RichText::new(text).size(12.5).color(chat_color(*kind)));
                        }
                    });
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut self.input)
                        .hint_text("Press Enter to chat")
                        .desired_width(f32::INFINITY),
                );
                if self.want_focus {
                    edit.request_focus();
                    self.want_focus = false;
                }
                if edit.has_focus() && !self.history.is_empty() {
                    let (up, down) = ui.input(|i| (i.key_pressed(egui::Key::ArrowUp), i.key_pressed(egui::Key::ArrowDown)));
                    let last = self.history.len() - 1;
                    let recall = match (up, down, self.recall) {
                        (true, _, None) => Some(Some(last)),
                        (true, _, Some(i)) => Some(Some(i.saturating_sub(1))),
                        (_, true, Some(i)) if i < last => Some(Some(i + 1)),
                        (_, true, Some(_)) => Some(None),
                        _ => None,
                    };
                    if let Some(r) = recall {
                        self.recall = r;
                        self.input = r.map(|i| self.history[i].clone()).unwrap_or_default();
                        // Caret to the end of the recalled line.
                        if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), edit.id) {
                            let end = egui::text::CCursor::new(self.input.chars().count());
                            state.cursor.set_char_range(Some(egui::text::CCursorRange::one(end)));
                            state.store(ui.ctx(), edit.id);
                        }
                    }
                }
                if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    let text = std::mem::take(&mut self.input);
                    self.recall = None;
                    if !text.trim().is_empty() {
                        if self.history.last() != Some(&text) {
                            self.history.push(text.clone());
                            if self.history.len() > 100 {
                                self.history.remove(0);
                            }
                        }
                        sent = Some(text);
                    }
                }
            });
        sent
    }
}

// ---------------------------------------------------------------- NPC dialog

/// What the dialog window currently offers.
#[derive(Clone, PartialEq)]
pub enum DialogMode {
    /// Text is still arriving.
    Reading,
    Next,
    Close,
    Menu(Vec<String>),
    Number(String),
    Text(String),
}

pub struct Dialog {
    pub npc: u32,
    pub title: String,
    pub lines: Vec<String>,
    pub mode: DialogMode,
}

/// An NPC illustration ("cutin") and where the script wants it.
pub struct Cutin {
    pub texture: egui::TextureHandle,
    /// 0 bottom-left, 1 bottom-centre, 2 bottom-right, 3 middle-right, 4 middle-centre.
    pub position: u8,
}

/// NPC text with ^RRGGBB colour codes. ^000000 means "default colour".
fn npc_text(text: &str, size: f32) -> egui::text::LayoutJob {
    let default = Color32::from_rgb(230, 230, 230);
    let mut job = egui::text::LayoutJob::default();
    let mut color = default;
    let mut rest = text;
    while let Some(i) = rest.find('^') {
        let code = rest.get(i + 1..i + 7).filter(|c| c.chars().all(|ch| ch.is_ascii_hexdigit()));
        let Some(code) = code else {
            job.append(&rest[..i + 1], 0.0, egui::TextFormat::simple(FontId::proportional(size), color));
            rest = &rest[i + 1..];
            continue;
        };
        job.append(&rest[..i], 0.0, egui::TextFormat::simple(FontId::proportional(size), color));
        let v = u32::from_str_radix(code, 16).unwrap_or(0);
        color = if v == 0 { default } else { Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8) };
        rest = &rest[i + 7..];
    }
    job.append(rest, 0.0, egui::TextFormat::simple(FontId::proportional(size), color));
    job
}

impl Hud {
    /// Shows the NPC dialog and cutin; returns the player's answer.
    pub fn show_dialog(&mut self, ctx: &egui::Context, dialog: &mut Option<Dialog>, cutin: Option<&Cutin>) -> Option<(u32, ro_net::NpcReply)> {
        use ro_net::NpcReply;

        if let Some(c) = cutin {
            let size = c.texture.size_vec2();
            let (align, offset) = match c.position {
                0 => (Align2::LEFT_BOTTOM, Vec2::new(20.0, -220.0)),
                1 => (Align2::CENTER_BOTTOM, Vec2::new(0.0, -220.0)),
                3 => (Align2::RIGHT_CENTER, Vec2::new(-20.0, 0.0)),
                4 => (Align2::CENTER_CENTER, Vec2::ZERO),
                _ => (Align2::RIGHT_BOTTOM, Vec2::new(-20.0, -20.0)),
            };
            egui::Area::new(egui::Id::unique("cutin")).anchor(align, offset).interactable(false).show(ctx, |ui| {
                ui.image((c.texture.id(), size));
            });
        }

        let d = dialog.as_mut()?;
        let mut reply = None;
        egui::Window::new("npc dialog")
            .title_bar(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::new(0.0, 60.0))
            .frame(panel())
            .show(ctx, |ui| {
                ui.set_width(440.0);
                if !d.title.is_empty() {
                    ui.label(RichText::new(&d.title).strong().size(14.0).color(Color32::from_rgb(160, 225, 255)));
                    ui.add_space(4.0);
                }
                egui::ScrollArea::vertical().max_height(220.0).stick_to_bottom(true).show(ui, |ui| {
                    for line in &d.lines {
                        ui.label(npc_text(line, 14.0));
                    }
                });
                ui.add_space(8.0);
                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                match &mut d.mode {
                    DialogMode::Reading => {}
                    DialogMode::Next => {
                        ui.allocate_ui_with_layout(Vec2::new(ui.available_width(), 28.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Next").clicked() || enter {
                                reply = Some(NpcReply::Next);
                            }
                        });
                    }
                    DialogMode::Close => {
                        ui.allocate_ui_with_layout(Vec2::new(ui.available_width(), 28.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Close").clicked() || enter {
                                reply = Some(NpcReply::Close);
                            }
                        });
                    }
                    DialogMode::Menu(options) => {
                        // Empty options are hidden but keep their index.
                        for (i, opt) in options.iter().enumerate() {
                            if opt.is_empty() {
                                continue;
                            }
                            let button = egui::Button::new(npc_text(opt, 13.5)).min_size(Vec2::new(ui.available_width(), 24.0));
                            if ui.add(button).clicked() {
                                reply = Some(NpcReply::Menu((i + 1).min(254) as u8));
                            }
                        }
                        if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            reply = Some(NpcReply::Menu(255));
                        }
                    }
                    DialogMode::Number(value) => {
                        ui.horizontal(|ui| {
                            let edit = ui.add(egui::TextEdit::singleline(value).desired_width(200.0));
                            edit.request_focus();
                            if ui.button("OK").clicked() || (edit.lost_focus() && enter) {
                                reply = Some(NpcReply::Number(value.trim().parse().unwrap_or(0)));
                            }
                        });
                    }
                    DialogMode::Text(value) => {
                        ui.horizontal(|ui| {
                            let edit = ui.add(egui::TextEdit::singleline(value).desired_width(300.0));
                            edit.request_focus();
                            if ui.button("OK").clicked() || (edit.lost_focus() && enter) {
                                reply = Some(NpcReply::Text(value.clone()));
                            }
                        });
                    }
                }
            });

        let reply = reply?;
        let npc = d.npc;
        match reply {
            // Close ends the conversation; other answers wait for the script's next step.
            NpcReply::Close => *dialog = None,
            NpcReply::Menu(255) => *dialog = None,
            NpcReply::Next | NpcReply::Menu(_) => {
                d.mode = DialogMode::Reading;
                d.lines.clear();
            }
            _ => d.mode = DialogMode::Reading,
        }
        Some((npc, reply))
    }
}

/// The map's thumbnail and the cell extents it covers.
pub struct Minimap {
    pub texture: egui::TextureHandle,
    pub cells: (f32, f32),
}

/// Minimap under the menu bar: `dots` are cells with a colour, `me` is drawn last and larger.
pub fn show_minimap(ctx: &egui::Context, map: &Minimap, name: &str, me: (f32, f32), dots: &[((f32, f32), Color32)]) {
    const SIZE: f32 = 160.0;
    egui::Area::new(egui::Id::unique("minimap")).anchor(Align2::RIGHT_TOP, Vec2::new(-12.0, 52.0)).interactable(false).show(ctx, |ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(SIZE), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        painter.image(map.texture.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        // Cell y grows north; the image's y grows down.
        let at = |c: (f32, f32)| rect.min + Vec2::new(c.0 / map.cells.0 * SIZE, (1.0 - c.1 / map.cells.1) * SIZE);
        for &(c, color) in dots {
            painter.circle_filled(at(c), 2.0, color);
        }
        painter.circle(at(me), 3.5, Color32::WHITE, egui::Stroke::new(1.0, Color32::BLACK));
        // Map name and coordinates over the image's bottom edge, outlined for contrast.
        let text = format!("{name}  {}, {}", me.0 as i32, me.1 as i32);
        let pos = rect.center_bottom() - Vec2::new(0.0, 3.0);
        let font = FontId::proportional(12.0);
        for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
            painter.text(pos + Vec2::new(dx, dy), Align2::CENTER_BOTTOM, &text, font.clone(), Color32::BLACK);
        }
        painter.text(pos, Align2::CENTER_BOTTOM, &text, font, Color32::WHITE);
    });
}

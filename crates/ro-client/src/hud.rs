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
}

fn chat_color(kind: ChatKind) -> Color32 {
    match kind {
        ChatKind::Public => Color32::from_rgb(235, 235, 235),
        ChatKind::Npc => Color32::from_rgb(150, 220, 255),
        ChatKind::Whisper => Color32::from_rgb(255, 230, 120),
        ChatKind::Broadcast => Color32::from_rgb(255, 255, 120),
        ChatKind::System => Color32::from_rgb(170, 255, 170),
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
    pub fn show(&mut self, ctx: &egui::Context, labels: &[Label]) -> Option<String> {
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
                if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    let text = std::mem::take(&mut self.input);
                    if !text.trim().is_empty() {
                        sent = Some(text);
                    }
                }
            });
        sent
    }
}

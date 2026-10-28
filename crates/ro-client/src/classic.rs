//! The original client's window art (texture\유저인터페이스\basic_interface) and a window
//! frame built from it: the blue title bar with its close button, a white body, and the
//! striped button bar along the bottom. Windows in the Classic interface draw with these.

use std::collections::HashMap;

use egui::{Color32, Rect, Vec2};

const DIR: &str = r"data\texture\유저인터페이스\basic_interface";

/// The images loaded (file names without .bmp).
const NAMES: &[&str] = &[
    "titlebar_left", "titlebar_mid", "titlebar_right", "btnbar_left", "btnbar_mid", "btnbar_right", "itemwin_mid", "tab_itm_01", "tab_itm_02", "tab_itm_03", "sys_close_off",
    "sys_close_on", "equipwin_bg", "statwin_bg", "arw_right", "arw_right_on",
];

/// Dark text on the white window bodies.
pub const TEXT: Color32 = Color32::from_rgb(40, 40, 52);
pub const TITLE_H: f32 = 17.0;
pub const BAR_H: f32 = 21.0;

pub struct Skin {
    tex: HashMap<&'static str, egui::TextureHandle>,
}

const UV: Rect = Rect { min: egui::pos2(0.0, 0.0), max: egui::pos2(1.0, 1.0) };

impl Skin {
    pub fn load(ctx: &egui::Context, vfs: &ro_vfs::Vfs) -> Self {
        let tex: HashMap<_, _> = NAMES.iter().filter_map(|n| Some((*n, crate::load_texture(ctx, vfs, &format!(r"{DIR}\{n}.bmp"))?))).collect();
        tracing::info!(images = tex.len(), "classic interface art");
        Self { tex }
    }

    /// Draws an image at its own size.
    pub fn draw(&self, p: &egui::Painter, name: &str, at: egui::Pos2) -> Option<Rect> {
        let t = self.tex.get(name)?;
        let r = Rect::from_min_size(at, t.size_vec2());
        p.image(t.id(), r, UV, Color32::WHITE);
        Some(r)
    }

    /// A three-piece bar (left cap, stretched middle, right cap) across `r`.
    fn bar(&self, p: &egui::Painter, r: Rect, left: &str, mid: &str, right: &str) {
        let (Some(l), Some(m), Some(rt)) = (self.tex.get(left), self.tex.get(mid), self.tex.get(right)) else { return };
        let (lw, rw) = (l.size_vec2().x, rt.size_vec2().x);
        p.image(l.id(), Rect::from_min_size(r.min, Vec2::new(lw, r.height())), UV, Color32::WHITE);
        p.image(m.id(), Rect::from_min_max(egui::pos2(r.min.x + lw, r.min.y), egui::pos2(r.max.x - rw, r.max.y)), UV, Color32::WHITE);
        p.image(rt.id(), Rect::from_min_max(egui::pos2(r.max.x - rw, r.min.y), r.max), UV, Color32::WHITE);
    }

    /// A classic window: title bar (drag to move, ✕ closes), white body holding `body`,
    /// and the button bar holding `footer`. Clears `open` when closed.
    pub fn window(&self, ctx: &egui::Context, title: &str, pos: egui::Pos2, width: f32, open: &mut bool, body: impl FnOnce(&mut egui::Ui), footer: impl FnOnce(&mut egui::Ui)) {
        let area = egui::Area::new(egui::Id::unique(("classic", title))).default_pos(pos).movable(true).show(ctx, |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            let p = ui.painter().clone();
            let (top, _) = ui.allocate_exact_size(Vec2::new(width, TITLE_H), egui::Sense::hover());
            self.bar(&p, top, "titlebar_left", "titlebar_mid", "titlebar_right");
            p.text(top.left_center() + Vec2::new(17.0, 0.0), egui::Align2::LEFT_CENTER, title, egui::FontId::proportional(12.0), Color32::from_rgb(30, 30, 60));
            let close = Rect::from_min_size(egui::pos2(top.max.x - 15.0, top.min.y + 3.0), Vec2::splat(11.0));
            let resp = ui.interact(close, egui::Id::unique(("classic_close", title)), egui::Sense::click());
            self.draw(&p, if resp.hovered() { "sys_close_on" } else { "sys_close_off" }, close.min);
            if resp.clicked() {
                *open = false;
            }
            // The body: a white panel drawn under whatever the window puts in it.
            let bg = p.add(egui::Shape::Noop);
            let inner = ui
                .allocate_ui_with_layout(Vec2::new(width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.set_width(width);
                    ui.visuals_mut().override_text_color = Some(TEXT);
                    body(ui);
                })
                .response
                .rect;
            let body_rect = Rect::from_min_max(egui::pos2(top.min.x, top.max.y), egui::pos2(top.max.x, inner.max.y.max(top.max.y)));
            p.set(bg, egui::Shape::rect_filled(body_rect, 0.0, Color32::WHITE));
            let (bottom, _) = ui.allocate_exact_size(Vec2::new(width, BAR_H), egui::Sense::hover());
            self.bar(&p, bottom, "btnbar_left", "btnbar_mid", "btnbar_right");
            ui.scope_builder(egui::UiBuilder::new().max_rect(bottom.shrink2(Vec2::new(8.0, 2.0))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
                ui.visuals_mut().override_text_color = Some(TEXT);
                footer(ui);
            });
            p.rect_stroke(Rect::from_min_max(top.min, bottom.max), 0.0, egui::Stroke::new(1.0, Color32::from_rgb(150, 160, 190)), egui::StrokeKind::Outside);
        });
        let _ = area;
    }

    /// A small classic text button: a pale blue bevel like the originals.
    pub fn button(&self, ui: &mut egui::Ui, label: &str) -> egui::Response {
        let galley = ui.painter().layout_no_wrap(label.to_string(), egui::FontId::proportional(11.5), TEXT);
        let size = Vec2::new(galley.size().x + 16.0, 18.0);
        let (r, resp) = ui.allocate_exact_size(size, egui::Sense::click());
        let fill = if resp.is_pointer_button_down_on() {
            Color32::from_rgb(190, 204, 240)
        } else if resp.hovered() {
            Color32::from_rgb(226, 234, 252)
        } else {
            Color32::from_rgb(244, 246, 252)
        };
        ui.painter().rect(r, 3.0, fill, egui::Stroke::new(1.0, Color32::from_rgb(140, 152, 190)), egui::StrokeKind::Inside);
        ui.painter().galley(r.center() - galley.size() / 2.0, galley, TEXT);
        resp
    }
}

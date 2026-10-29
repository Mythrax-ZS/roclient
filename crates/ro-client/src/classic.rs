//! The original client's window art (texture\유저인터페이스\basic_interface) and a window
//! frame built from it: the blue title bar with its close button, a white body, and the
//! striped button bar along the bottom. Windows in the Classic interface draw with these.

use crate::uitex::UiSize;
use std::collections::HashMap;

use egui::{Color32, Rect, Vec2};

const DIR: &str = r"data\texture\유저인터페이스\basic_interface";

/// The images loaded (file names without .bmp).
const NAMES: &[&str] = &[
    "titlebar_left", "titlebar_mid", "titlebar_right", "btnbar_left", "btnbar_mid", "btnbar_right", "itemwin_mid", "tab_itm_01", "tab_itm_02", "tab_itm_03", "sys_close_off",
    "sys_close_on", "equipwin_bg", "statwin_bg", "arw_right", "arw_right_on", "dialog_bg", "basewin_bg", "gze_bg", "gzeblue_left", "gzeblue_mid", "gzeblue_right", "gzered_left", "gzered_mid", "gzered_right", "btn_status_off", "btn_status_on", "btn_equip_off", "btn_equip_on", "btn_items_off", "btn_items_on", "btn_skill_off", "btn_skill_on", "btn_comm_off", "btn_comm_on", "btn_friend_off", "btn_friend_on", "btn_option_off", "btn_option_on", "btn_map_off", "btn_map_on", "tab_l", "tab_m", "tab_r", "tab_a_l", "tab_a_m", "tab_a_r",
];

/// Dark text on the white window bodies.
pub const TEXT: Color32 = Color32::from_rgb(40, 40, 52);
pub const TITLE_H: f32 = 17.0;
pub const BAR_H: f32 = 21.0;

#[derive(Clone)]
pub struct Skin {
    tex: HashMap<&'static str, egui::TextureHandle>,
}

const UV: Rect = Rect { min: egui::pos2(0.0, 0.0), max: egui::pos2(1.0, 1.0) };

static CLASSIC: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// A text color for window bodies: unchanged in Modern; in Classic, light colors (meant for
/// dark panels) are darkened so they stay readable on the white bodies, keeping their hue.
pub fn ink(c: Color32) -> Color32 {
    if !CLASSIC.load(std::sync::atomic::Ordering::Relaxed) {
        return c;
    }
    let [r, g, b, a] = c.to_array();
    let lum = (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) / 255.0;
    if lum < 0.5 {
        return c;
    }
    let spread = r.max(g).max(b) - r.min(g).min(b);
    if spread < 40 {
        // Whites and greys: dark text, or a mid grey for the dimmer ones.
        return if lum > 0.85 { TEXT } else { Color32::from_rgb(110, 114, 128) };
    }
    let k = 0.5;
    Color32::from_rgba_unmultiplied((r as f32 * k) as u8, (g as f32 * k) as u8, (b as f32 * k) as u8, a)
}

fn skin_id() -> egui::Id {
    egui::Id::unique("classic_skin")
}

/// Makes the Classic art available to every window (or removes it for Modern), and sets
/// the matching look: light panels like the original's, or egui's dark one.
pub fn install(ctx: &egui::Context, skin: Option<&Skin>) {
    let had = ctx.data(|d| d.get_temp::<Skin>(skin_id()).is_some());
    CLASSIC.store(skin.is_some(), std::sync::atomic::Ordering::Relaxed);
    match skin {
        Some(s) => ctx.data_mut(|d| { d.insert_temp(skin_id(), s.clone()); }),
        None => ctx.data_mut(|d| d.remove::<Skin>(skin_id())),
    }
    if had != skin.is_some() {
        ctx.set_visuals(if skin.is_some() { visuals() } else { egui::Visuals::dark() });
    }
}

/// The Classic art, when that interface is on.
pub fn skin(ctx: &egui::Context) -> Option<Skin> {
    ctx.data(|d| d.get_temp::<Skin>(skin_id()))
}

/// The original's palette for egui widgets: white bodies, dark text, pale blue controls.
pub fn visuals() -> egui::Visuals {
    use egui::Color32 as C;
    let mut v = egui::Visuals::light();
    v.window_fill = C::WHITE;
    v.panel_fill = C::WHITE;
    v.faint_bg_color = C::from_rgb(236, 240, 250);
    v.extreme_bg_color = C::from_rgb(246, 247, 252);
    v.window_stroke = egui::Stroke::new(1.0, C::from_rgb(150, 160, 190));
    v.window_corner_radius = egui::CornerRadius::ZERO;
    v.window_shadow = egui::Shadow { offset: [2, 2], blur: 6, spread: 0, color: C::from_black_alpha(60) };
    v.hyperlink_color = C::from_rgb(52, 84, 180);
    v.selection.bg_fill = C::from_rgb(150, 172, 226);
    v.selection.stroke = egui::Stroke::new(1.0, TEXT);
    let w = &mut v.widgets;
    w.noninteractive.bg_stroke = egui::Stroke::new(1.0, C::from_rgb(206, 212, 230));
    w.noninteractive.fg_stroke = egui::Stroke::new(1.0, TEXT);
    for (s, fill) in [(&mut w.inactive, C::from_rgb(244, 246, 252)), (&mut w.hovered, C::from_rgb(226, 234, 252)), (&mut w.active, C::from_rgb(190, 204, 240))] {
        s.bg_fill = fill;
        s.weak_bg_fill = fill;
        s.bg_stroke = egui::Stroke::new(1.0, C::from_rgb(140, 152, 190));
        s.fg_stroke = egui::Stroke::new(1.0, TEXT);
        s.corner_radius = egui::CornerRadius::same(3);
    }
    w.open.weak_bg_fill = C::from_rgb(226, 234, 252);
    v
}

/// Shows an egui window; in Classic, dressed with the original title bar (blue gradient,
/// title, round close button) painted over egui's own.
pub fn show<R>(ctx: &egui::Context, window: egui::Window<'_>, title: &str, closable: bool, body: impl FnOnce(&mut egui::Ui) -> R) -> Option<egui::InnerResponse<Option<R>>> {
    let window = if CASCADE.contains(&title) { window.default_pos(cascade(title)) } else { window };
    let out = window.show(ctx, body)?;
    if let Some(skin) = skin(ctx) {
        let style = ctx.global_style();
        let h = ctx.fonts_mut(|f| f.row_height(&egui::TextStyle::Heading.resolve(&style))) + style.spacing.window_margin.sum().y;
        let r = out.response.rect;
        let top = Rect::from_min_size(r.min, Vec2::new(r.width(), h.min(r.height())));
        let p = ctx.layer_painter(out.response.layer_id);
        skin.bar(&p, top, "titlebar_left", "titlebar_mid", "titlebar_right");
        p.text(top.left_center() + Vec2::new(12.0, 0.0), egui::Align2::LEFT_CENTER, title, egui::FontId::proportional(13.0), Color32::from_rgb(30, 30, 60));
        let close = Rect::from_center_size(egui::pos2(top.max.x - style.spacing.window_margin.right as f32 - 7.0, top.center().y), Vec2::splat(11.0));
        let hovered = ctx.pointer_hover_pos().is_some_and(|p| top.contains(p) && p.x > close.min.x - 6.0);
        if closable {
            skin.draw(&p, if hovered { "sys_close_on" } else { "sys_close_off" }, close.min);
        }
    }
    Some(out)
}

impl Skin {
    pub fn load(ctx: &egui::Context, vfs: &ro_vfs::Vfs) -> Self {
        let tex: HashMap<_, _> = NAMES.iter().filter_map(|n| Some((*n, crate::load_texture(ctx, vfs, &format!(r"{DIR}\{n}.bmp"))?))).collect();
        tracing::info!(images = tex.len(), "classic interface art");
        Self { tex }
    }

    pub fn texture(&self, name: &str) -> Option<&egui::TextureHandle> {
        self.tex.get(name)
    }

    /// Draws an image at its own size.
    pub fn draw(&self, p: &egui::Painter, name: &str, at: egui::Pos2) -> Option<Rect> {
        let t = self.tex.get(name)?;
        let r = Rect::from_min_size(at, t.ui_size());
        p.image(t.id(), r, UV, Color32::WHITE);
        Some(r)
    }

    /// A three-piece bar (left cap, stretched middle, right cap) across `r`.
    pub fn bar(&self, p: &egui::Painter, r: Rect, left: &str, mid: &str, right: &str) {
        let (Some(l), Some(m), Some(rt)) = (self.tex.get(left), self.tex.get(mid), self.tex.get(right)) else { return };
        let (lw, rw) = (l.ui_size().x, rt.ui_size().x);
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

    /// A classic tab (tab_l/m/r, or tab_a_* when active) around a label.
    pub fn tab(&self, ui: &mut egui::Ui, label: &str, active: bool) -> egui::Response {
        let galley = ui.painter().layout_no_wrap(label.to_string(), egui::FontId::proportional(11.5), TEXT);
        let (r, resp) = ui.allocate_exact_size(Vec2::new(galley.size().x + 14.0, 21.0), egui::Sense::click());
        let (l, m, rt) = if active { ("tab_a_l", "tab_a_m", "tab_a_r") } else { ("tab_l", "tab_m", "tab_r") };
        self.bar(ui.painter(), r, l, m, rt);
        let color = if active || resp.hovered() { TEXT } else { Color32::from_rgb(110, 116, 140) };
        ui.painter().galley(r.center() - galley.size() / 2.0, galley, color);
        resp
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

/// Windows with no place of their own: they open spread out instead of all on one spot.
const CASCADE: [&str; 21] = [
    "Battle", "Room password", "Create Chat Room", "Clan", "Enchant", "Game Options", "Merge Items", "Navigation", "Attendance", "Quests", "Ranking",
    "Item Reform", "Reputation", "Settings", "Store Search", "Stylist", "Dressing Room", "Market", "Trade", "Bank", "Cash Shop",
];

/// A starting position from the title, on a grid over the middle of the screen.
fn cascade(title: &str) -> egui::Pos2 {
    let h = title.bytes().fold(7u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
    egui::pos2(380.0 + (h % 5) as f32 * 110.0, 110.0 + ((h / 5) % 4) as f32 * 70.0)
}

//! Controller button glyphs, drawn as vector shapes in the look of each family: Xbox
//! (coloured A B X Y), PlayStation (cross, circle, square, triangle) and Nintendo (B A Y X,
//! by position). Every place the UI names a controller button draws it with these.

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Sense, Stroke, Vec2};
use serde::{Deserialize, Serialize};

/// Which family's buttons to draw.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Style {
    Xbox,
    PlayStation,
    Nintendo,
}

/// The button icons setting: follow the controller, or one family always.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Choice {
    #[default]
    Auto,
    Xbox,
    PlayStation,
    Nintendo,
}

impl Choice {
    pub fn resolve(self, detected: Style) -> Style {
        match self {
            Choice::Auto => detected,
            Choice::Xbox => Style::Xbox,
            Choice::PlayStation => Style::PlayStation,
            Choice::Nintendo => Style::Nintendo,
        }
    }
}

impl Style {
    pub fn name(self) -> &'static str {
        match self {
            Style::Xbox => "Xbox",
            Style::PlayStation => "PlayStation",
            Style::Nintendo => "Nintendo",
        }
    }
}

/// The family a controller belongs to, from the name the system gives it.
pub fn detect(name: &str) -> Option<Style> {
    let n = name.to_ascii_lowercase();
    // An Xbox pad calls itself "Xbox Wireless Controller"; a PS4 pad is just "Wireless Controller".
    if n.contains("xbox") || n.contains("xinput") {
        None
    } else if ["ps3", "ps4", "ps5", "dualsense", "dualshock", "sony", "wireless controller"].iter().any(|k| n.contains(k)) {
        Some(Style::PlayStation)
    } else if ["nintendo", "switch", "pro controller", "joy-con", "joycon"].iter().any(|k| n.contains(k)) {
        Some(Style::Nintendo)
    } else {
        None
    }
}

/// A button, by position (South is the bottom face button on every family).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Btn {
    South,
    East,
    West,
    North,
    Up,
    Right,
    Down,
    Left,
    /// The whole D-pad.
    DPad,
    LB,
    RB,
    LT,
    RT,
    /// The right stick's click.
    RS,
    /// The sticks themselves (moving them).
    LStick,
    RStick,
    Start,
    Select,
    /// A PlayStation pad's touchpad.
    Touchpad,
}

/// The slot each button fires on a shortcut row while a trigger is held (column order).
pub const SLOT_BUTTONS: [Btn; 9] = [Btn::South, Btn::East, Btn::West, Btn::North, Btn::Up, Btn::Right, Btn::Down, Btn::Left, Btn::RS];

/// The button's name in words, as that family prints it.
pub fn name(b: Btn, s: Style) -> &'static str {
    use Btn::*;
    use Style::*;
    match (b, s) {
        (South, Xbox) => "A",
        (East, Xbox) => "B",
        (West, Xbox) => "X",
        (North, Xbox) => "Y",
        (South, PlayStation) => "Cross",
        (East, PlayStation) => "Circle",
        (West, PlayStation) => "Square",
        (North, PlayStation) => "Triangle",
        (South, Nintendo) => "B",
        (East, Nintendo) => "A",
        (West, Nintendo) => "Y",
        (North, Nintendo) => "X",
        (Up, _) => "D-pad up",
        (Right, _) => "D-pad right",
        (Down, _) => "D-pad down",
        (Left, _) => "D-pad left",
        (DPad, _) => "D-pad",
        (LB, Xbox) => "LB",
        (RB, Xbox) => "RB",
        (LT, Xbox) => "LT",
        (RT, Xbox) => "RT",
        (LB, PlayStation) => "L1",
        (RB, PlayStation) => "R1",
        (LT, PlayStation) => "L2",
        (RT, PlayStation) => "R2",
        (LB, Nintendo) => "L",
        (RB, Nintendo) => "R",
        (LT, Nintendo) => "ZL",
        (RT, Nintendo) => "ZR",
        (RS, PlayStation) => "R3",
        (RS, _) => "RS",
        (LStick, _) => "Left stick",
        (RStick, _) => "Right stick",
        (Start, Xbox) => "Menu",
        (Select, Xbox) => "View",
        (Start, PlayStation) => "Options",
        (Select, PlayStation) => "Create",
        (Start, Nintendo) => "+",
        (Select, Nintendo) => "-",
        (Touchpad, _) => "Touchpad",
    }
}

/// Words for a combination, "RT + A".
pub fn combo_name(bs: &[Btn], s: Style) -> String {
    bs.iter().map(|b| name(*b, s)).collect::<Vec<_>>().join(" + ")
}

/// Width of a glyph `size` points tall.
pub fn width(b: Btn, size: f32) -> f32 {
    match b {
        Btn::LB | Btn::RB => size * 1.55,
        Btn::LT | Btn::RT => size * 1.4,
        Btn::Start | Btn::Select => size * 1.3,
        Btn::Touchpad => size * 1.8,
        _ => size,
    }
}

const BASE: Color32 = Color32::from_rgb(34, 37, 46);
const RIM: Color32 = Color32::from_rgb(112, 118, 136);
const INK: Color32 = Color32::from_rgb(236, 238, 244);
const DIM: Color32 = Color32::from_rgb(92, 98, 116);

/// Draws `b` filling `rect` (its height is the glyph size).
pub fn paint(p: &Painter, rect: Rect, b: Btn, s: Style) {
    let size = rect.height();
    let c = rect.center();
    let r = size / 2.0;
    let rim = Stroke::new((size * 0.06).max(1.0), RIM);
    let font = |k: f32| FontId::proportional(size * k);
    match b {
        Btn::South | Btn::East | Btn::West | Btn::North => {
            p.circle(c, r - 0.5, BASE, rim);
            match s {
                Style::Xbox => {
                    let color = match b {
                        Btn::South => Color32::from_rgb(110, 196, 76),
                        Btn::East => Color32::from_rgb(232, 76, 66),
                        Btn::West => Color32::from_rgb(70, 140, 232),
                        _ => Color32::from_rgb(244, 196, 52),
                    };
                    p.text(c + Vec2::new(0.0, size * 0.02), Align2::CENTER_CENTER, name(b, s), font(0.62), color);
                }
                Style::Nintendo => {
                    p.text(c + Vec2::new(0.0, size * 0.02), Align2::CENTER_CENTER, name(b, s), font(0.62), INK);
                }
                Style::PlayStation => {
                    let w = Stroke::new((size * 0.09).max(1.2), match b {
                        Btn::South => Color32::from_rgb(124, 162, 236),
                        Btn::East => Color32::from_rgb(236, 92, 96),
                        Btn::West => Color32::from_rgb(226, 140, 200),
                        _ => Color32::from_rgb(70, 200, 168),
                    });
                    let k = r * 0.46;
                    match b {
                        Btn::South => {
                            p.line_segment([c + Vec2::new(-k, -k), c + Vec2::new(k, k)], w);
                            p.line_segment([c + Vec2::new(-k, k), c + Vec2::new(k, -k)], w);
                        }
                        Btn::East => {
                            p.circle_stroke(c, k * 1.05, w);
                        }
                        Btn::West => {
                            p.rect_stroke(Rect::from_center_size(c, Vec2::splat(k * 1.8)), 0.0, w, egui::StrokeKind::Middle);
                        }
                        _ => {
                            let h = k * 1.15;
                            let pts = [c + Vec2::new(0.0, -h), c + Vec2::new(h * 1.05, h * 0.75), c + Vec2::new(-h * 1.05, h * 0.75)];
                            p.add(egui::Shape::closed_line(pts.to_vec(), w));
                        }
                    }
                }
            }
        }
        Btn::Up | Btn::Right | Btn::Down | Btn::Left | Btn::DPad => {
            let arm = (size * 0.38).max(4.0);
            let full = size - 1.0;
            let cross = if b == Btn::DPad { Color32::from_rgb(200, 204, 216) } else { Color32::from_rgb(128, 135, 156) };
            p.rect_filled(Rect::from_center_size(c, Vec2::new(full, arm)), 1.5, cross);
            p.rect_filled(Rect::from_center_size(c, Vec2::new(arm, full)), 1.5, cross);
            // The direction meant, lit from the centre out.
            for (d, dx, dy) in [(Btn::Up, 0.0, -1.0), (Btn::Right, 1.0, 0.0), (Btn::Down, 0.0, 1.0), (Btn::Left, -1.0, 0.0f32)] {
                if b == d {
                    let len = full / 2.0;
                    let at = c + Vec2::new(dx, dy) * (len / 2.0);
                    let sz = if dx != 0.0 { Vec2::new(len, arm) } else { Vec2::new(arm, len) };
                    p.rect_filled(Rect::from_center_size(at, sz), 1.5, INK);
                }
            }
        }
        Btn::LB | Btn::RB => {
            let rect = rect.shrink2(Vec2::new(0.5, size * 0.14));
            let round = egui::CornerRadius { nw: (size * 0.36) as u8, ne: (size * 0.36) as u8, sw: (size * 0.12) as u8, se: (size * 0.12) as u8 };
            p.rect(rect, round, BASE, rim, egui::StrokeKind::Inside);
            p.text(rect.center(), Align2::CENTER_CENTER, name(b, s), font(0.48), INK);
        }
        Btn::LT | Btn::RT => {
            let rect = rect.shrink(0.5);
            let round = egui::CornerRadius { nw: (size * 0.42) as u8, ne: (size * 0.42) as u8, sw: (size * 0.14) as u8, se: (size * 0.14) as u8 };
            p.rect(rect, round, BASE, rim, egui::StrokeKind::Inside);
            p.text(rect.center() + Vec2::new(0.0, size * 0.04), Align2::CENTER_CENTER, name(b, s), font(0.5), INK);
        }
        Btn::RS | Btn::LStick | Btn::RStick => {
            p.circle(c, r - 0.5, BASE, rim);
            p.circle_stroke(c, r * 0.62, Stroke::new(1.0, DIM));
            let text = match b {
                Btn::LStick => "L",
                Btn::RStick => "R",
                _ => name(b, s),
            };
            p.text(c, Align2::CENTER_CENTER, text, font(if text.len() > 1 { 0.44 } else { 0.56 }), INK);
            if matches!(b, Btn::LStick | Btn::RStick) {
                // Arrows round the rim: it is moved, not pressed.
                let a = r * 0.82;
                let t = size * 0.09;
                for (dx, dy) in [(0.0, -1.0), (1.0, 0.0), (0.0, 1.0), (-1.0, 0.0f32)] {
                    let tip = c + Vec2::new(dx, dy) * a;
                    let side = Vec2::new(-dy, dx) * t;
                    let back = Vec2::new(dx, dy) * -t;
                    p.add(egui::Shape::convex_polygon(vec![tip, tip + back + side, tip + back - side], INK, Stroke::NONE));
                }
            }
        }
        Btn::Touchpad => {
            // The pad seen from above: wide, its lower corners rounded more.
            let pad = rect.shrink2(Vec2::new(0.5, size * 0.12));
            let round = egui::CornerRadius { nw: (size * 0.12) as u8, ne: (size * 0.12) as u8, sw: (size * 0.3) as u8, se: (size * 0.3) as u8 };
            p.rect(pad, round, BASE, rim, egui::StrokeKind::Inside);
            p.circle_filled(pad.center() + Vec2::new(size * 0.18, -size * 0.04), size * 0.09, INK);
        }
        Btn::Start | Btn::Select => match s {
            Style::Nintendo => {
                p.circle(c, r * 0.8, BASE, rim);
                let k = r * 0.42;
                let w = Stroke::new((size * 0.09).max(1.2), INK);
                p.line_segment([c - Vec2::new(k, 0.0), c + Vec2::new(k, 0.0)], w);
                if b == Btn::Start {
                    p.line_segment([c - Vec2::new(0.0, k), c + Vec2::new(0.0, k)], w);
                }
            }
            _ => {
                let pill = Rect::from_center_size(c, Vec2::new(rect.width() - 1.0, size * 0.66));
                p.rect(pill, (size * 0.33) as u8, BASE, rim, egui::StrokeKind::Inside);
                let w = Stroke::new((size * 0.07).max(1.0), INK);
                let k = size * 0.2;
                if b == Btn::Start {
                    // Menu / Options: three lines.
                    for dy in [-k * 0.7, 0.0, k * 0.7] {
                        p.line_segment([c + Vec2::new(-k, dy), c + Vec2::new(k, dy)], w);
                    }
                } else if s == Style::Xbox {
                    // View: two overlapping windows.
                    let a = Rect::from_center_size(c + Vec2::new(-k * 0.3, -k * 0.3), Vec2::splat(k * 1.3));
                    let b2 = Rect::from_center_size(c + Vec2::new(k * 0.3, k * 0.3), Vec2::splat(k * 1.3));
                    p.rect_stroke(a, 1.0, w, egui::StrokeKind::Middle);
                    p.rect_filled(b2, 1.0, BASE);
                    p.rect_stroke(b2, 1.0, w, egui::StrokeKind::Middle);
                } else {
                    // Create: a spark of three short strokes.
                    for (dx, dy) in [(0.0, -1.0), (0.87, 0.5), (-0.87, 0.5f32)] {
                        p.line_segment([c + Vec2::new(dx, dy) * k * 0.35, c + Vec2::new(dx, dy) * k * 1.05], w);
                    }
                }
            }
        },
    }
}

/// One glyph as a widget; hovering it names the button.
pub fn show(ui: &mut egui::Ui, b: Btn, s: Style, size: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width(b, size), size), Sense::hover());
    paint(ui.painter(), rect, b, s);
    resp.on_hover_text(name(b, s))
}

/// Glyphs joined by "+", as one control: "hold RT + A".
pub fn combo(ui: &mut egui::Ui, bs: &[Btn], s: Style, size: f32) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        for (i, b) in bs.iter().enumerate() {
            if i > 0 {
                ui.label(egui::RichText::new("+").size(size * 0.7));
            }
            show(ui, *b, s, size);
        }
    });
}

/// Paints a glyph `size` tall with its top-left at `at`; returns its width.
pub fn paint_at(p: &Painter, at: Pos2, b: Btn, s: Style, size: f32) -> f32 {
    let w = width(b, size);
    paint(p, Rect::from_min_size(at, Vec2::new(w, size)), b, s);
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn families() {
        assert_eq!(detect("DualSense Wireless Controller"), Some(Style::PlayStation));
        assert_eq!(detect("Nintendo Switch Pro Controller"), Some(Style::Nintendo));
        assert_eq!(detect("Xbox Wireless Controller"), None);
        assert_eq!(detect("Wireless Controller"), Some(Style::PlayStation));
        assert_eq!(name(Btn::South, Style::Nintendo), "B");
        assert_eq!(name(Btn::RT, Style::PlayStation), "R2");
        assert_eq!(Choice::Auto.resolve(Style::Nintendo), Style::Nintendo);
        assert_eq!(Choice::Xbox.resolve(Style::Nintendo), Style::Xbox);
    }
}

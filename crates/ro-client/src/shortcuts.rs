//! The Shortcut Information window (the original's menu entry of that name, msgstringtable
//! 3880): every keyboard, mouse and gamepad shortcut and what it does. It describes what
//! main.rs does (`key_pressed`, `alt_key`, the mouse events and `gamepad_frame`), so a new
//! shortcut there belongs here too.

use egui::{Color32, RichText};

const WINDOWS: &[(&str, &str)] = &[
    ("Alt+E", "Inventory"),
    ("Alt+Q", "Equipment"),
    ("Alt+A", "Status"),
    ("Alt+S", "Skills"),
    ("Alt+Z", "Party"),
    ("Alt+H", "Friends"),
    ("Alt+G", "Guild"),
    ("Alt+U", "Quests"),
    ("Alt+W", "Cart"),
    ("Alt+R", "Companions (pet, homunculus, mercenary)"),
    ("Alt+X", "Mail"),
    ("Alt+B", "Bank"),
    ("Alt+C", "Cash Shop"),
    ("Alt+Y", "Achievements"),
    ("Alt+J", "Attendance"),
    ("Alt+N", "Party Booking"),
    ("Alt+K", "Create a chat room"),
    ("Alt+L", "Emotion List"),
    ("Alt+M", "Macros (what Alt+1 to Alt+0 say)"),
    ("Alt+O", "Settings"),
];

const BAR: &[(&str, &str)] = &[
    ("F1 - F9", "Use the slots of the shortcut row on show"),
    ("F12", "Show one to four shortcut rows, or hide the bar"),
    ("Drag the top edge", "Show more or fewer shortcut rows"),
];

const CHAT: &[(&str, &str)] = &[
    ("Enter", "Type in the chat (with battle mode off, any letter starts typing too)"),
    ("Up / Down", "While typing: the lines you sent before"),
    ("Alt+1 - Alt+0", "Say a macro (emotes until changed with Alt+M)"),
];

const GAME: &[(&str, &str)] = &[
    ("Esc", "Game menu; cancels a skill being aimed"),
    ("Insert", "Sit down or stand up"),
    ("Ctrl+Tab", "Minimap on or off"),
    ("F11", "Full screen on or off"),
    ("Print Screen", "Screenshot, saved in the ScreenShot folder"),
    ("Scroll Lock", "Screenshot"),
];

const MOUSE: &[(&str, &str)] = &[
    ("Click the ground", "Walk there; hold the button to keep walking toward the cursor"),
    ("Click a monster", "Attack it until it falls"),
    ("Click an NPC", "Talk to it"),
    ("Click a player", "Their menu (trade, party, friend); on PvP and GvG maps, attack"),
    ("Click an item", "Walk to it and pick it up"),
    ("Hover", "Names of monsters and items on the ground"),
    ("Right-drag", "Turn and tilt the camera"),
    ("Wheel", "Zoom"),
];

/// Face buttons by position: bottom, right, left, top.
fn faces(playstation: bool) -> [&'static str; 4] {
    if playstation { ["Cross", "Circle", "Square", "Triangle"] } else { ["A", "B", "X", "Y"] }
}

fn pad(playstation: bool) -> Vec<(String, String)> {
    let [a, b, x, y] = faces(playstation);
    let (lt, rt, back, rs) = if playstation { ("L2", "R2", "Share", "R3") } else { ("LT", "RT", "Back", "Right stick click") };
    [
        ("Left stick".into(), "Walk".into()),
        ("Right stick".into(), "Turn and tilt the camera".into()),
        (rs.into(), "Reset the camera's turn".into()),
        ("D-pad left / right".into(), "Turn the camera 45 degrees".into()),
        ("D-pad up / down".into(), "Zoom".into()),
        (format!("{lt} / {rt}"), "Previous or next target".into()),
        (a.into(), "Attack the target, or talk to it".into()),
        (b.into(), "Stop attacking".into()),
        (x.into(), "Pick up the nearest item".into()),
        (y.into(), "Sit down or stand up".into()),
        (format!("Hold {rt} + {a}/{b}/{x}/{y}/D-pad"), "Use the slots of the shortcut row on show".into()),
        (format!("Hold {lt} + {a}/{b}/{x}/{y}/D-pad"), "Use the slots of the next row".into()),
        (format!("Aiming a skill: {a} / {y} / {b}"), "Use it on the target / on yourself / cancel".into()),
        (format!("NPC talk: D-pad, {a}, {b}"), "Choose, confirm, cancel".into()),
        (format!("Dead: {a}"), "Return to your save point".into()),
        ("Start".into(), "Settings".into()),
        (back.into(), "Inventory".into()),
    ]
    .into()
}

/// What the window needs to know about the current settings.
pub struct Ctx<'a> {
    pub battle_mode: bool,
    /// The keys of the three rows after the one on show (settings.battle_keys).
    pub battle_keys: &'a [String],
    pub battle_space_chat: bool,
    pub playstation: bool,
}

#[derive(Default)]
pub struct ShortcutWindow {
    pub open: bool,
    tab: usize,
}

impl ShortcutWindow {
    pub fn show(&mut self, ctx: &egui::Context, s: Ctx<'_>) {
        if !self.open {
            return;
        }
        let title = crate::msgstr::get(3880, &[]).unwrap_or_else(|| "Shortcut Information".into());
        let max_height = (ctx.content_rect().height() - 140.0).clamp(220.0, 620.0);
        let mut open = self.open;
        let tab = &mut self.tab;
        crate::classic::show(ctx, egui::Window::new(&title).id(egui::Id::unique("shortcut information")).open(&mut open).resizable(false).collapsible(false).default_width(470.0).default_pos(egui::pos2(ctx.content_rect().center().x - 235.0, 70.0)), &title, true, |ui| {
            ui.horizontal(|ui| {
                for (i, name) in ["Keyboard", "Mouse", "Gamepad"].iter().enumerate() {
                    ui.selectable_value(tab, i, *name);
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().max_height(max_height).auto_shrink([false, true]).show(ui, |ui| {
                ui.set_width(450.0);
                egui::Grid::new(("shortcuts", *tab)).num_columns(2).spacing([14.0, 6.0]).min_col_width(120.0).show(ui, |ui| match *tab {
                    0 => keyboard(ui, &s),
                    1 => rows(ui, MOUSE.iter().map(|(k, d)| (k.to_string(), d.to_string()))),
                    _ => rows(ui, pad(s.playstation).into_iter()),
                });
            });
        });
        self.open = open;
    }
}

fn keyboard(ui: &mut egui::Ui, s: &Ctx<'_>) {
    let list = |ui: &mut egui::Ui, l: &[(&str, &str)]| rows(ui, l.iter().map(|(k, d)| (k.to_string(), d.to_string())));
    section(ui, "Windows");
    list(ui, WINDOWS);
    section(ui, "Shortcut bar");
    list(ui, BAR);
    section(ui, &format!("Battle mode (/bm): {}", if s.battle_mode { "on" } else { "off" }));
    let keys: Vec<(String, String)> = s
        .battle_keys
        .iter()
        .enumerate()
        .filter(|(_, k)| !k.is_empty())
        .map(|(i, k)| {
            let k = k.to_uppercase();
            let span = match (k.chars().next(), k.chars().last()) {
                (Some(a), Some(b)) if a != b => format!("{a} - {b}"),
                _ => k.clone(),
            };
            (span, format!("Use the slots of the {} row after the one on show", ["first", "second", "third"][i.min(2)]))
        })
        .collect();
    rows(ui, keys.into_iter());
    if s.battle_space_chat {
        rows(ui, std::iter::once(("Space".to_string(), "Type in the chat".to_string())));
    }
    ui.label("");
    ui.label(RichText::new("With battle mode on, letters use skills instead of typing. Settings > Controls changes the keys.").size(11.0).weak());
    ui.end_row();
    section(ui, "Chat");
    list(ui, CHAT);
    section(ui, "Game");
    list(ui, GAME);
}

fn rows(ui: &mut egui::Ui, items: impl Iterator<Item = (String, String)>) {
    for (key, what) in items {
        key_chip(ui, &key);
        ui.label(what);
        ui.end_row();
    }
}

/// A key drawn as a small keycap.
fn key_chip(ui: &mut egui::Ui, key: &str) {
    let dark = ui.visuals().dark_mode;
    let (fill, stroke, text) = if dark {
        (Color32::from_rgb(44, 48, 60), Color32::from_rgb(90, 98, 120), Color32::from_rgb(230, 232, 240))
    } else {
        (Color32::from_rgb(246, 246, 250), Color32::from_rgb(170, 174, 190), Color32::from_rgb(30, 34, 48))
    };
    egui::Frame::new().fill(fill).stroke((1.0, stroke)).corner_radius(4.0).inner_margin(egui::Margin::symmetric(6, 2)).show(ui, |ui| {
        ui.label(RichText::new(key).monospace().size(12.0).color(text));
    });
}

fn section(ui: &mut egui::Ui, title: &str) {
    let color = if ui.visuals().dark_mode { Color32::from_rgb(140, 175, 255) } else { Color32::from_rgb(50, 70, 150) };
    ui.vertical(|ui| {
        ui.add_space(4.0);
        ui.label(RichText::new(title).strong().size(13.5).color(color));
    });
    ui.end_row();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pad_names_follow_the_controller() {
        assert!(pad(true).iter().any(|(k, _)| k == "Cross"));
        assert!(pad(false).iter().any(|(k, _)| k == "A"));
    }
}

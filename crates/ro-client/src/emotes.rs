//! Emote chat commands, as the original client accepts them ("/!", "/thx", ...). The value
//! is the ET_* id the server broadcasts (and the action in sprite\이팩트\emotion).

const COMMANDS: &[(&str, u8)] = &[
    ("/!", 0),
    ("/?", 1),
    ("/ho", 2),
    ("/lv", 3),
    ("/swt", 4),
    ("/ic", 5),
    ("/an", 6),
    ("/ag", 7),
    ("/$", 8),
    ("/...", 9),
    ("/lv2", 14),
    ("/thx", 15),
    ("/wah", 16),
    ("/sry", 17),
    ("/heh", 18),
    ("/swt2", 19),
    ("/hmm", 20),
    ("/no1", 21),
    ("/??", 22),
    ("/omg", 23),
    ("/oh", 24),
    ("/x", 25),
    ("/hlp", 26),
    ("/go", 27),
    ("/sob", 28),
    ("/gg", 29),
    ("/kis", 30),
    ("/kis2", 31),
    ("/pif", 32),
    ("/ok", 33),
    ("/bzz", 36),
    ("/rice", 37),
    ("/awsm", 38),
    ("/meh", 39),
    ("/shy", 40),
    ("/pat", 41),
    ("/mp", 42),
    ("/slur", 43),
    ("/com", 44),
    ("/yawn", 45),
    ("/grat", 46),
    ("/hp", 47),
    ("/fsh", 52),
    ("/spin", 53),
    ("/sigh", 54),
    ("/dum", 55),
    ("/crwd", 56),
    ("/desp", 57),
    ("/love", 65),
    ("/oops", 76),
    ("/spit", 77),
    ("/panic", 79),
    ("/whisp", 80),
];

/// The action in sprite\이팩트\emotion.act that draws emote `et` (ET_* id). The sprite's
/// order differs from the ids in its first rows, and it has no picture for
/// ET_CHAT_PROHIBIT (34), so the later ones sit one place earlier.
pub fn action(et: u8) -> Option<usize> {
    let et = et as usize;
    Some(match et {
        4 => 5,
        5..=9 => et + 1,
        10 => 12,
        12 => 13,
        13 => 14,
        14 => 4,
        34 => return None,
        35.. => et - 1,
        _ => et,
    })
}

/// The emote for a chat command, if it is one. "/dice" rolls one of the six dice faces.
pub fn command(text: &str) -> Option<u8> {
    let t = text.trim().to_ascii_lowercase();
    if t == "/dice" {
        return Some(58 + (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.subsec_nanos() % 6) as u8);
    }
    COMMANDS.iter().find(|(c, _)| *c == t).map(|(_, e)| *e)
}

/// What Alt+1..Alt+0 say before the player changes them in the macro window (Alt+M): the
/// first ten emotes, as in the original.
pub const DEFAULT_MACROS: [&str; 10] = ["/!", "/?", "/ho", "/lv", "/swt", "/ic", "/an", "/ag", "/$", "/..."];

/// The macro slot for Alt+digit: Alt+1 is the first, Alt+0 the tenth.
pub fn macro_slot(digit: u8) -> usize {
    ((digit + 9) % 10) as usize
}

/// Splits a whisper command: `/w Name text`, `/w "Name with spaces" text`.
pub fn whisper(text: &str) -> Option<(String, String)> {
    let rest = text.strip_prefix("/w ").or_else(|| text.strip_prefix("/whisper "))?.trim_start();
    let (name, msg) = match rest.strip_prefix('"') {
        Some(q) => q.split_once('"')?,
        None => rest.split_once(' ')?,
    };
    let msg = msg.trim();
    (!name.is_empty() && !msg.is_empty()).then(|| (name.to_string(), msg.to_string()))
}

/// One emote drawn as a still picture (its fullest frame), for the Emotion List.
struct Icon {
    tex: egui::TextureHandle,
    size: egui::Vec2,
}

/// The Emotion List (Alt+L, /emo) and the macro window (Alt+M), as in the original: the
/// list shows every emote with its command, and clicking one shows it; the macro window
/// holds what Alt+1..Alt+0 say.
#[derive(Default)]
pub struct EmoteWindow {
    pub open: bool,
    pub macros_open: bool,
    /// Emote id -> picture; None until loaded (empty if the sprite is missing).
    icons: Option<std::collections::HashMap<u8, Icon>>,
}

impl EmoteWindow {
    fn load(&mut self, ctx: &egui::Context, vfs: Option<&ro_vfs::Vfs>) {
        if self.icons.is_some() {
            return;
        }
        let base = "data\\sprite\\이팩트\\emotion";
        let read = |ext: &str| vfs.and_then(|v| v.read(&format!("{base}.{ext}")).ok().flatten());
        let spr = read("spr").and_then(|d| ro_formats::spr::Spr::parse(&d).ok());
        let act = read("act").and_then(|d| ro_formats::act::Act::parse(&d).ok());
        let mut icons = std::collections::HashMap::new();
        if let (Some(spr), Some(act)) = (spr, act) {
            let (indexed, rgba) = spr.images(None);
            for &(_, id) in COMMANDS.iter().chain([("/dice", 58)].iter()) {
                let Some(action) = action(id).and_then(|a| act.actions.get(a)) else { continue };
                if let Some((image, size)) = still(action, &indexed, &rgba) {
                    let tex = ctx.load_texture(format!("emote{id}"), image, egui::TextureOptions::NEAREST);
                    icons.insert(id, Icon { tex, size });
                }
            }
        } else {
            tracing::warn!("emotion sprite missing; the Emotion List shows commands only");
        }
        self.icons = Some(icons);
    }

    /// Draws the open windows; returns a chat line to say (an emote command).
    /// `macros_changed` is set when the macros should be saved.
    pub fn show(&mut self, ctx: &egui::Context, vfs: Option<&ro_vfs::Vfs>, macros: &mut Vec<String>, macros_changed: &mut bool) -> Option<String> {
        let mut say = None;
        if self.open {
            self.load(ctx, vfs);
            let title = crate::msgstr::get(542, &[]).unwrap_or_else(|| "Emotion List".into());
            let mut open = true;
            let icons = self.icons.as_ref();
            let mut macros_button = false;
            crate::classic::show(ctx, egui::Window::new(&title).id(egui::Id::unique("emotion list")).open(&mut open).default_width(460.0).resizable(false), &title, true, |ui| {
                const CELL: egui::Vec2 = egui::vec2(52.0, 56.0);
                egui::ScrollArea::vertical().max_height(500.0).show(ui, |ui| {
                    egui::Grid::new("emotes").spacing(egui::vec2(4.0, 4.0)).show(ui, |ui| {
                        for (i, &(cmd, id)) in COMMANDS.iter().chain([("/dice", 58)].iter()).enumerate() {
                            let (rect, resp) = ui.allocate_exact_size(CELL, egui::Sense::click());
                            let p = ui.painter_at(rect);
                            if resp.hovered() {
                                p.rect_filled(rect, 4.0, egui::Color32::from_rgba_unmultiplied(120, 150, 230, 50));
                            }
                            let pic = egui::Rect::from_min_max(rect.min + egui::vec2(3.0, 2.0), rect.max - egui::vec2(3.0, 15.0));
                            if let Some(icon) = icons.and_then(|m| m.get(&id)) {
                                // Shrunk to fit, never enlarged (pixel art).
                                let k = (pic.width() / icon.size.x).min(pic.height() / icon.size.y).min(1.0);
                                let r = egui::Rect::from_center_size(pic.center(), icon.size * k);
                                p.image(icon.tex.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                            }
                            p.text(egui::pos2(rect.center().x, rect.max.y - 1.0), egui::Align2::CENTER_BOTTOM, cmd, egui::FontId::proportional(11.5), ui.visuals().text_color());
                            if resp.on_hover_text(format!("Type {cmd} in chat")).clicked() {
                                say = Some(cmd.to_string());
                            }
                            if i % 8 == 7 {
                                ui.end_row();
                            }
                        }
                    });
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Alt+1 ~ Alt+0 say your macros").size(11.5));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        macros_button = ui.button("Macros (Alt+M)").clicked();
                    });
                });
            });
            self.open = open;
            if macros_button {
                self.macros_open = !self.macros_open;
            }
        }
        if self.macros_open {
            let title = crate::msgstr::get(574, &[]).unwrap_or_else(|| "Shortcut List".into());
            let mut open = true;
            crate::classic::show(ctx, egui::Window::new(&title).id(egui::Id::unique("macro list")).open(&mut open).default_width(300.0).resizable(false), &title, true, |ui| {
                macros.resize(10, String::new());
                egui::Grid::new("macros").num_columns(2).spacing(egui::vec2(8.0, 4.0)).show(ui, |ui| {
                    for (i, m) in macros.iter_mut().enumerate() {
                        ui.label(format!("Alt+{}", (i + 1) % 10));
                        if ui.add(egui::TextEdit::singleline(m).desired_width(220.0).char_limit(70)).lost_focus() {
                            *macros_changed = true;
                        }
                        ui.end_row();
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button("Defaults").clicked() {
                        *macros = DEFAULT_MACROS.iter().map(|s| s.to_string()).collect();
                        *macros_changed = true;
                    }
                    ui.label(egui::RichText::new("Emotes, phrases or / commands").size(11.5));
                });
            });
            if !open {
                self.macros_open = false;
                *macros_changed = true;
            }
        }
        say
    }
}

/// The fullest frame of an emote (the most visible pixels; emotes fade and pop in and out),
/// composited into one image: (image, size in pixels).
fn still(action: &ro_formats::act::Action, indexed: &[ro_formats::image::Rgba], rgba: &[ro_formats::image::Rgba]) -> Option<(egui::ColorImage, egui::Vec2)> {
    let best = action
        .frames
        .iter()
        .filter_map(|f| composite(f, indexed, rgba))
        .max_by(|a, b| a.2.total_cmp(&b.2))?;
    let (bytes, [w, h], _) = best;
    Some((egui::ColorImage::from_rgba_premultiplied([w, h], &bytes), egui::vec2(w as f32, h as f32)))
}

/// One ACT frame drawn into an image: (premultiplied RGBA, size, total alpha).
fn composite(frame: &ro_formats::act::Frame, indexed: &[ro_formats::image::Rgba], rgba: &[ro_formats::image::Rgba]) -> Option<(Vec<u8>, [usize; 2], f32)> {
    let image = |l: &ro_formats::act::Layer| usize::try_from(l.sprite).ok().and_then(|i| if l.kind == 1 { rgba.get(i) } else { indexed.get(i) });
    // Each drawn layer's rectangle around the frame origin.
    let layers: Vec<_> = frame
        .layers
        .iter()
        .filter_map(|l| {
            let img = image(l)?;
            let size = egui::vec2(img.width as f32 * l.scale[0].abs(), img.height as f32 * l.scale[1].abs());
            (l.color[3] > 0 && size.x >= 1.0 && size.y >= 1.0).then(|| (egui::Rect::from_center_size(egui::pos2(l.x as f32, l.y as f32), size), l, img))
        })
        .collect();
    let b = layers.iter().map(|(r, ..)| *r).reduce(|a, b| a.union(b))?;
    let (w, h) = (b.width().ceil().max(1.0) as usize, b.height().ceil().max(1.0) as usize);
    let mut out = vec![[0f32; 4]; w * h];
    for (r, l, img) in layers {
        let tint = l.color.map(|c| c as f32 / 255.0);
        for y in 0..h {
            for x in 0..w {
                let (px, py) = (b.min.x + x as f32 + 0.5, b.min.y + y as f32 + 0.5);
                if !r.contains(egui::pos2(px, py)) {
                    continue;
                }
                let mut u = (((px - r.min.x) / r.width() * img.width as f32) as u32).min(img.width - 1);
                let v = (((py - r.min.y) / r.height() * img.height as f32) as u32).min(img.height - 1);
                if l.mirror {
                    u = img.width - 1 - u;
                }
                let at = (v * img.width + u) as usize * 4;
                let s = &img.pixels[at..at + 4];
                // Premultiplied "over", tinted by the layer colour.
                let a = s[3] as f32 / 255.0 * tint[3];
                let src = [s[0] as f32 / 255.0 * tint[0] * tint[3], s[1] as f32 / 255.0 * tint[1] * tint[3], s[2] as f32 / 255.0 * tint[2] * tint[3], a];
                let d = &mut out[y * w + x];
                for c in 0..4 {
                    d[c] = src[c] + d[c] * (1.0 - a);
                }
            }
        }
    }
    let total = out.iter().map(|p| p[3]).sum();
    let bytes = out.iter().flat_map(|p| p.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8)).collect();
    Some((bytes, [w, h], total))
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses() {
        assert_eq!(super::command("/thx"), Some(15));
        assert_eq!(super::command("hello"), None);
        assert_eq!(super::whisper("/w Bob hi there"), Some(("Bob".into(), "hi there".into())));
        assert_eq!(super::whisper("/w \"Big Bob\" hi"), Some(("Big Bob".into(), "hi".into())));
        assert_eq!(super::macro_slot(1), 0);
        assert_eq!(super::macro_slot(0), 9);
        assert!(super::DEFAULT_MACROS.iter().all(|m| super::command(m).is_some()));
        // /swt is the sprite's sixth action, /lv2 its fifth; /bzz (36) one place early.
        assert_eq!(super::action(4), Some(5));
        assert_eq!(super::action(14), Some(4));
        assert_eq!(super::action(36), Some(35));
    }
}

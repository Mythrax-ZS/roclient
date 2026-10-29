//! Books: `data\book\<item id>.txt`, opened by the server (`readbook` scripts, ZC_READ_BOOK)
//! or by using an item listed in `data\bookitemnametable.txt`, which the original client
//! reads locally instead of sending a use request.
//!
//! A book file's first line is `%RRGGBB`, the page colour; the text uses ^RRGGBB colour
//! codes like NPC dialogs. Pages are fixed runs of lines, turned with Prev / Next.

use std::collections::HashSet;

/// Source lines per page.
const LINES_PER_PAGE: usize = 18;

pub struct Book {
    title: String,
    paper: egui::Color32,
    pages: Vec<String>,
    page: usize,
}

#[derive(Default)]
pub struct Books {
    /// Item ids that open as books when used.
    items: HashSet<u32>,
    pub open: Option<Book>,
}

impl Books {
    pub fn load(vfs: &ro_vfs::Vfs) -> Self {
        let items = vfs
            .read("data\\bookitemnametable.txt")
            .ok()
            .flatten()
            .map(|d| encoding_rs::EUC_KR.decode(&d).0.lines().filter_map(|l| l.split('#').next()?.trim().parse().ok()).collect())
            .unwrap_or_default();
        Self { items, open: None }
    }

    pub fn is_book(&self, item: u32) -> bool {
        self.items.contains(&item)
    }

    /// Opens book `id` at `page` (1-based); false if the client has no text for it.
    pub fn open(&mut self, vfs: &ro_vfs::Vfs, id: u32, page: u32, title: String) -> bool {
        let Some(data) = vfs.read(&format!("data\\book\\{id}.txt")).ok().flatten() else {
            tracing::warn!(id, "book text missing");
            return false;
        };
        let text = encoding_rs::EUC_KR.decode(&data).0.replace('\r', "");
        let mut lines: Vec<&str> = text.lines().collect();
        let mut paper = egui::Color32::from_rgb(245, 245, 220);
        if let Some(c) = lines.first().and_then(|l| l.strip_prefix('%')).and_then(|h| u32::from_str_radix(h.trim(), 16).ok()) {
            paper = egui::Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8);
            lines.remove(0);
        }
        let pages: Vec<String> = lines.chunks(LINES_PER_PAGE).map(|c| c.join("\n")).collect();
        let page = (page.max(1) as usize - 1).min(pages.len().saturating_sub(1));
        self.open = Some(Book { title, paper, pages, page });
        true
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        let Some(b) = self.open.as_mut() else { return };
        let mut open = true;
        let frame = egui::Frame::new().fill(b.paper).stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(90))).corner_radius(4.0).inner_margin(10.0);
        egui::Window::new(&b.title).id(egui::Id::unique("book")).open(&mut open).frame(frame).resizable(false).collapsible(false).default_width(420.0).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.set_min_height(360.0);
            let text = b.pages.get(b.page).map_or("", String::as_str);
            // Dark ink on the page colour (the default ^000000 is black here).
            let mut job = crate::hud::npc_text(text, 13.0);
            for s in &mut job.sections {
                if s.format.color == egui::Color32::from_rgb(230, 230, 230) {
                    s.format.color = egui::Color32::BLACK;
                }
            }
            job.wrap.max_width = 410.0;
            ui.label(job);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(b.page > 0, egui::Button::new("Prev")).clicked() {
                    b.page -= 1;
                }
                ui.label(egui::RichText::new(format!("{} / {}", b.page + 1, b.pages.len().max(1))).color(egui::Color32::BLACK));
                if ui.add_enabled(b.page + 1 < b.pages.len(), egui::Button::new("Next")).clicked() {
                    b.page += 1;
                }
            });
        });
        if !open {
            self.open = None;
        }
    }
}

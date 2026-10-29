//! Bargain sales: Sale-tab cash items a GM puts on sale for a few hours with a limited stock
//! (the cash shop shows the time and stock left), and the GM's tool for it (/limitedsale).

use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use egui::{Color32, RichText};
use ro_net::{SaleNote, SaleRequest};

#[derive(Default)]
pub struct Sales {
    /// Items on sale: item id -> (sale end, stock left when known).
    pub on_sale: HashMap<u32, (Instant, Option<u32>)>,
    tool: Option<Tool>,
}

#[derive(Default)]
struct Tool {
    search: String,
    /// The item found: (id, price).
    found: Option<(u32, u32)>,
    amount: u32,
    hours: u16,
    /// Minutes from now until the sale starts.
    delay: u32,
    note: String,
}

impl Sales {
    /// Applies a server note; returns a line for the chat log.
    pub fn apply(&mut self, note: SaleNote, name: impl Fn(u32) -> String) -> Option<String> {
        match note {
            SaleNote::Selling { item, secs } => {
                // Announced once; later notes (on opening the shop) only refresh the time.
                let known = self.on_sale.get(&item).copied();
                self.on_sale.insert(item, (Instant::now() + Duration::from_secs(secs as u64), known.and_then(|s| s.1)));
                if known.is_none() {
                    return Some(format!("[Bargain Sale] {} is on sale in the Cash Shop for {}.", name(item), left(secs as u64)));
                }
            }
            SaleNote::Closed(item) => {
                self.on_sale.remove(&item);
                return Some(format!("[Bargain Sale] The sale of {} has ended.", name(item)));
            }
            SaleNote::Stock { item, amount } => {
                if let Some(s) = self.on_sale.get_mut(&item) {
                    s.1 = Some(amount);
                }
            }
            SaleNote::Tool(open) => self.tool = open.then(|| Tool { amount: 10, hours: 1, ..Default::default() }),
            SaleNote::Found(found) => {
                if let Some(t) = self.tool.as_mut() {
                    t.found = found;
                    t.note = if found.is_some() { String::new() } else { "That item is not in the Sale tab.".into() };
                }
            }
            SaleNote::Added(result) => {
                if let Some(t) = self.tool.as_mut() {
                    t.note = match result {
                        0 => "The item has been put on sale.",
                        2 => "That item is already on sale.",
                        _ => "The item could not be put on sale.",
                    }
                    .into();
                }
            }
            SaleNote::Removed(ok) => {
                if let Some(t) = self.tool.as_mut() {
                    t.note = if ok { "The sale has been cancelled." } else { "That item is not on sale." }.into();
                }
            }
        }
        None
    }

    /// The sale line for a cash shop card: time and stock left.
    pub fn badge(&self, item: u32) -> Option<String> {
        let (end, stock) = self.on_sale.get(&item)?;
        let secs = end.saturating_duration_since(Instant::now()).as_secs();
        Some(match stock {
            Some(n) => format!("Sale: {} · {n} left", left(secs)),
            None => format!("Sale: {}", left(secs)),
        })
    }

    /// The GM's bargain sale tool.
    pub fn show_tool(&mut self, ctx: &egui::Context, icons: &HashMap<u32, egui::TextureHandle>, name: impl Fn(u32) -> String) -> Option<SaleRequest> {
        let badge = self.tool.as_ref().and_then(|t| t.found).and_then(|(item, _)| self.badge(item));
        let t = self.tool.as_mut()?;
        let mut out = None;
        let mut open = true;
        crate::classic::show(ctx, egui::Window::new("Bargain Sale").open(&mut open).collapsible(false).resizable(false).default_width(320.0), "Bargain Sale", true, |ui| {
            ui.horizontal(|ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut t.search).hint_text("Item name").desired_width(200.0));
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (ui.button("Search").clicked() || enter) && !t.search.trim().is_empty() {
                    out = Some(SaleRequest::Search(t.search.trim().to_string()));
                }
            });
            if let Some((item, price)) = t.found {
                ui.separator();
                ui.horizontal(|ui| {
                    if let Some(tex) = icons.get(&item) {
                        ui.image((tex.id(), egui::vec2(24.0, 24.0)));
                    }
                    ui.label(RichText::new(name(item)).strong());
                    ui.label(format!("{price} cash"));
                });
                if let Some(badge) = &badge {
                    ui.label(RichText::new(badge).color(crate::classic::ink(Color32::from_rgb(255, 120, 120))));
                }
                egui::Grid::new("sale_tool").num_columns(2).show(ui, |ui| {
                    ui.label("Stock");
                    ui.add(egui::DragValue::new(&mut t.amount).range(1..=99999));
                    ui.end_row();
                    ui.label("Starts in (minutes)");
                    ui.add(egui::DragValue::new(&mut t.delay).range(0..=10080));
                    ui.end_row();
                    ui.label("Hours on sale");
                    ui.add(egui::DragValue::new(&mut t.hours).range(1..=168));
                    ui.end_row();
                });
                ui.horizontal(|ui| {
                    if ui.button("Put on sale").clicked() {
                        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) as u32;
                        out = Some(SaleRequest::Add { item, amount: t.amount, start: now + t.delay * 60, hours: t.hours });
                    }
                    if ui.button("Cancel sale").clicked() {
                        out = Some(SaleRequest::Remove(item));
                    }
                });
            }
            if !t.note.is_empty() {
                ui.label(RichText::new(&t.note).color(crate::classic::ink(Color32::from_rgb(255, 210, 120))));
            }
        });
        if !open {
            self.tool = None;
            out = Some(SaleRequest::Close);
        }
        out
    }
}

/// "2h 05m" / "4m 10s" for a time left.
fn left(secs: u64) -> String {
    match secs {
        3600.. => format!("{}h {:02}m", secs / 3600, secs / 60 % 60),
        _ => format!("{}m {:02}s", secs / 60, secs % 60),
    }
}

//! Navigation (/navi, or the server's navigateto): search the client's navigation tables
//! for a map or NPC, plan the cheapest chain of warps there, and guide the walk — the
//! steps in a window, the next stop on the minimap, and a button that walks to it.
//!
//! Tables (data\luafiles514\lua files\navigation, the _us copies when present):
//!   Navi_Map  { map, name, ?, width, height }
//!   Navi_Npc  { map, id, kind (101 NPC, 102 ...), sprite, name, ?, x, y }
//!   Navi_Link { map, id, kind (200 warp, 201 NPC service, 204 ...), sprite, name, ?, x, y,
//!               to map, to x, to y }

use std::collections::{BinaryHeap, HashMap};

use egui::{Color32, RichText};
use ro_formats::lub::{self, Value};

#[derive(Debug, Clone)]
pub struct Npc {
    pub map: String,
    pub name: String,
    pub x: u16,
    pub y: u16,
}

#[derive(Debug, Clone)]
struct Link {
    map: String,
    /// Walk-through warp (200) or something to talk to (an NPC that moves you).
    warp: bool,
    name: String,
    x: u16,
    y: u16,
    to: String,
    to_x: u16,
    to_y: u16,
}

#[derive(Default)]
pub struct Tables {
    /// Map id -> display name.
    pub maps: Vec<(String, String)>,
    pub npcs: Vec<Npc>,
    links: Vec<Link>,
    /// Links by the map they start on.
    from: HashMap<String, Vec<usize>>,
}

const DIR: &str = r"data\luafiles514\lua files\navigation";

fn load_rows(vfs: &ro_vfs::Vfs, file: &str, global: &str) -> Vec<Vec<Value>> {
    let data = [format!(r"{DIR}\{file}_us.lub"), format!(r"{DIR}\{file}.lub")].iter().find_map(|p| vfs.read(p).ok().flatten());
    let Some(chunk) = data.and_then(|d| lub::eval(&d).ok()) else { return vec![] };
    let Some(t) = chunk.globals.get(global) else { return vec![] };
    chunk.entries(t).iter().map(|(_, row)| chunk.entries(row).iter().map(|(_, v)| v.clone()).collect()).collect()
}

fn s(row: &[Value], i: usize) -> String {
    row.get(i).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn n(row: &[Value], i: usize) -> u16 {
    row.get(i).and_then(Value::as_number).unwrap_or(0.0) as u16
}

impl Tables {
    pub fn load(vfs: &ro_vfs::Vfs) -> Self {
        let maps: Vec<(String, String)> = load_rows(vfs, "navi_map", "Navi_Map").iter().map(|r| (s(r, 0), s(r, 1))).collect();
        let npcs: Vec<Npc> = load_rows(vfs, "navi_npc", "Navi_Npc").iter().filter(|r| !s(r, 4).is_empty()).map(|r| Npc { map: s(r, 0), name: s(r, 4), x: n(r, 6), y: n(r, 7) }).collect();
        let links: Vec<Link> = load_rows(vfs, "navi_link", "Navi_Link")
            .iter()
            .filter(|r| !s(r, 8).is_empty())
            .map(|r| Link { map: s(r, 0), warp: n(r, 2) == 200, name: s(r, 4), x: n(r, 6), y: n(r, 7), to: s(r, 8), to_x: n(r, 9), to_y: n(r, 10) })
            .collect();
        let mut from: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, l) in links.iter().enumerate() {
            from.entry(l.map.clone()).or_default().push(i);
        }
        tracing::info!(maps = maps.len(), npcs = npcs.len(), links = links.len(), "navigation tables");
        Self { maps, npcs, links, from }
    }

    pub fn map_name(&self, map: &str) -> Option<&str> {
        self.maps.iter().find(|(id, _)| id == map).map(|(_, n)| n.as_str())
    }

    /// The cheapest way from `start` to `goal` (None: any cell of the goal map): cells
    /// walked, plus a toll per map change (more for NPC services, which cost zeny/time).
    pub fn route(&self, start: (&str, u16, u16), goal: (&str, Option<(u16, u16)>)) -> Option<Vec<Step>> {
        let dist = |a: (u16, u16), b: (u16, u16)| (a.0 as i32 - b.0 as i32).abs().max((a.1 as i32 - b.1 as i32).abs()) as u32;
        // Node 0 is the start; node i+1 is arriving through link i.
        let at = |node: usize| -> (&str, (u16, u16)) {
            if node == 0 { (start.0, (start.1, start.2)) } else { let l = &self.links[node - 1]; (l.to.as_str(), (l.to_x, l.to_y)) }
        };
        let mut best: HashMap<usize, (u32, usize)> = HashMap::new();
        let mut heap = BinaryHeap::new();
        heap.push(std::cmp::Reverse((0u32, 0usize)));
        best.insert(0, (0, usize::MAX));
        let mut done: Option<(u32, usize)> = None;
        while let Some(std::cmp::Reverse((cost, node))) = heap.pop() {
            if best.get(&node).is_some_and(|b| b.0 < cost) {
                continue;
            }
            if done.is_some_and(|d| d.0 <= cost) {
                break;
            }
            let (map, pos) = at(node);
            if map == goal.0 {
                let total = cost + goal.1.map_or(0, |g| dist(pos, g));
                if done.is_none_or(|d| total < d.0) {
                    done = Some((total, node));
                }
            }
            for &li in self.from.get(map).into_iter().flatten() {
                let l = &self.links[li];
                let c = cost + dist(pos, (l.x, l.y)) + if l.warp { 10 } else { 120 };
                let next = li + 1;
                if best.get(&next).is_none_or(|b| c < b.0) {
                    best.insert(next, (c, node));
                    heap.push(std::cmp::Reverse((c, next)));
                }
            }
        }
        let (_, mut node) = done?;
        let mut links = vec![];
        while node != 0 {
            links.push(node - 1);
            node = best[&node].1;
        }
        links.reverse();
        let mut steps: Vec<Step> = links
            .iter()
            .map(|&i| {
                let l = &self.links[i];
                let to = self.map_name(&l.to).unwrap_or(&l.to).to_string();
                let what = if l.warp { format!("Take the warp to {to}") } else { format!("Talk to {} to go to {to}", l.name) };
                Step { map: l.map.clone(), x: l.x, y: l.y, what }
            })
            .collect();
        if let Some((x, y)) = goal.1 {
            steps.push(Step { map: goal.0.to_string(), x, y, what: "Arrive".into() });
        }
        Some(steps)
    }
}

#[derive(Debug, Clone)]
pub struct Step {
    pub map: String,
    pub x: u16,
    pub y: u16,
    pub what: String,
}

#[derive(Default)]
pub struct Navigation {
    pub open: bool,
    tables: Option<Tables>,
    query: String,
    /// Where we are going: (map, cell, label).
    target: Option<(String, Option<(u16, u16)>, String)>,
    pub steps: Vec<Step>,
    /// The map the route was planned from (replanned on arrival elsewhere).
    planned_on: String,
    pub status: String,
}

pub enum NaviAction {
    /// Walk to this cell on the current map.
    Walk(u16, u16),
}

impl Navigation {
    /// A map's display name from the navigation tables (once loaded).
    pub fn map_name(&self, map: &str) -> Option<&str> {
        self.tables.as_ref()?.map_name(map)
    }

    pub fn ensure_loaded(&mut self, vfs: Option<&ro_vfs::Vfs>) {
        if self.tables.is_none() {
            if let Some(v) = vfs {
                self.tables = Some(Tables::load(v));
            }
        }
    }

    /// Starts guiding to a map (and cell); `label` names the goal.
    pub fn go(&mut self, map: &str, cell: Option<(u16, u16)>, label: String) {
        self.target = Some((map.to_string(), cell, label));
        self.planned_on.clear();
        self.open = true;
    }

    pub fn stop(&mut self) {
        self.target = None;
        self.steps.clear();
        self.status.clear();
    }

    /// The stop to head for on this map, for the minimap.
    pub fn waypoint(&self, map: &str) -> Option<(u16, u16)> {
        self.steps.iter().find(|s| s.map == map).map(|s| (s.x, s.y))
    }

    /// Replans when we change maps; clears the route on arrival.
    pub fn update(&mut self, map: &str, me: (u16, u16)) {
        let Some((goal, cell, _)) = &self.target else { return };
        if map == goal && cell.is_none_or(|c| (c.0 as i32 - me.0 as i32).abs().max((c.1 as i32 - me.1 as i32).abs()) <= 3) {
            self.status = "You have arrived.".into();
            self.target = None;
            self.steps.clear();
            return;
        }
        if self.planned_on == map {
            return;
        }
        self.planned_on = map.to_string();
        let Some(t) = &self.tables else { return };
        match t.route((map, me.0, me.1), (goal, *cell)) {
            Some(steps) => {
                self.status.clear();
                self.steps = steps;
            }
            None => {
                self.steps.clear();
                self.status = "No known route from here.".into();
            }
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, map: &str) -> Option<NaviAction> {
        if !self.open {
            return None;
        }
        let mut out = None;
        let mut open = true;
        let mut pick = None;
        let mut stop = false;
        crate::classic::show(ctx, egui::Window::new("Navigation").open(&mut open).default_width(340.0).resizable(false), "Navigation", true, |ui| {
            let Some(t) = &self.tables else {
                ui.label("No navigation data in the client files.");
                return;
            };
            ui.horizontal(|ui| {
                ui.label("Search");
                ui.add(egui::TextEdit::singleline(&mut self.query).hint_text("NPC or map name").desired_width(240.0));
            });
            let q = self.query.trim().to_lowercase();
            if q.len() >= 2 {
                egui::ScrollArea::vertical().id_salt("navi_results").max_height(180.0).show(ui, |ui| {
                    let mut shown = 0;
                    for (id, name) in t.maps.iter().filter(|(id, name)| id.to_lowercase().contains(&q) || name.to_lowercase().contains(&q)).take(15) {
                        if ui.selectable_label(false, format!("{name}  ({id})")).clicked() {
                            pick = Some((id.clone(), None, name.clone()));
                        }
                        shown += 1;
                    }
                    for npc in t.npcs.iter().filter(|n| n.name.to_lowercase().contains(&q)).take(40) {
                        let where_ = t.map_name(&npc.map).unwrap_or(&npc.map);
                        if ui.selectable_label(false, format!("{}  — {where_} ({}, {})", npc.name, npc.x, npc.y)).clicked() {
                            pick = Some((npc.map.clone(), Some((npc.x, npc.y)), npc.name.clone()));
                        }
                        shown += 1;
                    }
                    if shown == 0 {
                        ui.label(RichText::new("Nothing found.").color(crate::classic::ink(Color32::GRAY)));
                    }
                });
            }
            if let Some((_, _, label)) = &self.target {
                ui.separator();
                ui.label(RichText::new(format!("Going to {label}")).strong());
                for (i, s) in self.steps.iter().enumerate() {
                    let here = s.map == map;
                    let place = t.map_name(&s.map).unwrap_or(&s.map);
                    let text = RichText::new(format!("{}. {place} ({}, {}): {}", i + 1, s.x, s.y, s.what));
                    ui.label(if here { text.strong().color(crate::classic::ink(Color32::from_rgb(255, 220, 120))) } else { text });
                }
                ui.horizontal(|ui| {
                    if let Some(s) = self.steps.iter().find(|s| s.map == map) {
                        if ui.button("Walk there").clicked() {
                            out = Some(NaviAction::Walk(s.x, s.y));
                        }
                    }
                    if ui.button("Stop").clicked() {
                        stop = true;
                    }
                });
            }
            if !self.status.is_empty() {
                ui.label(RichText::new(&self.status).color(crate::classic::ink(Color32::from_rgb(255, 210, 110))));
            }
        });
        if let Some((m, c, l)) = pick {
            self.go(&m, c, l);
        }
        if stop {
            self.stop();
        }
        self.open = open;
        out
    }
}

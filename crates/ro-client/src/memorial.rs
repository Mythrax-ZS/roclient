//! The Memorial Dungeon window: the instance booked for us (or our party), its place in the
//! queue, then the time left to enter and the time left inside, with a Destroy button
//! (CZ_MEMORIALDUNGEON_COMMAND), like the original's small status window.

#[derive(Default)]
pub struct Memorial {
    pub name: String,
    /// Standby priority while queued.
    place: Option<u16>,
    /// Unix times: the dungeon closes / entry closes (0 unknown).
    live: u32,
    enter: u32,
    pub active: bool,
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

fn clock(until: u32) -> Option<String> {
    let left = until as i64 - now();
    (until > 0 && left > 0).then(|| format!("{:02}:{:02}:{:02}", left / 3600, left / 60 % 60, left % 60))
}

impl Memorial {
    /// Applies a server note; returns a chat line for the ones the original prints.
    pub fn apply(&mut self, note: ro_net::InstanceNote) -> Option<String> {
        use ro_net::InstanceNote::*;
        match note {
            Queued { name, place } => {
                *self = Self { name, place: Some(place), active: true, ..Default::default() };
                None
            }
            Place(p) => {
                self.place = Some(p);
                None
            }
            Status { name, live, enter } => {
                *self = Self { name, place: None, live, enter, active: true };
                None
            }
            Notice { kind, limit } => match kind {
                0 => {
                    // A new limit (entered: the time inside starts counting).
                    self.live = limit;
                    self.enter = 0;
                    None
                }
                1 | 2 => {
                    self.active = false;
                    Some("No one entered the Memorial Dungeon within its duration; the dungeon has disappeared.".into())
                }
                3 => {
                    self.active = false;
                    Some(format!("Memorial Dungeon {} has been destroyed.", self.name))
                }
                _ => {
                    self.active = false;
                    Some(format!("Failed to create Memorial Dungeon {}. Please try again.", self.name))
                }
            },
        }
    }

    /// Draws the window; returns true when Destroy was pressed.
    pub fn show(&mut self, ctx: &egui::Context) -> bool {
        if !self.active {
            return false;
        }
        let mut destroy = false;
        let mut open = true;
        crate::classic::show(ctx, egui::Window::new("Memorial Dungeon").open(&mut open).resizable(false).collapsible(true).default_pos([20.0, 260.0]), "Memorial Dungeon", true, |ui| {
            ui.label(egui::RichText::new(&self.name).strong());
            match (self.place, clock(self.enter), clock(self.live)) {
                (Some(p), _, _) => {
                    ui.label(format!("{} in Standby", self.name));
                    ui.label(format!("Your Standby Priority: {p}"));
                }
                (None, Some(enter), _) => {
                    ui.label(format!("{} Available", self.name));
                    ui.label(format!("Enter within {enter}"));
                }
                (None, None, Some(live)) => {
                    ui.label(format!("{} in Progress", self.name));
                    ui.label(format!("Dungeon Mission Time Limit: {live}"));
                }
                _ => {
                    ui.label(format!("{} in Progress", self.name));
                }
            }
            ui.add_space(4.0);
            if ui.button("Destroy").clicked() {
                destroy = true;
            }
        });
        if !open {
            // Closing only hides it; the dungeon stays until it ends.
            self.active = false;
        }
        // Count down every second.
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
        destroy
    }
}

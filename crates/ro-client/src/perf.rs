//! The performance overlay (F10, or Settings > Game > Interface): the frame rate, frame times
//! and their graph, where a frame's time goes, the map server's round trip, and what is drawn.

use std::collections::VecDeque;
use std::time::Instant;

/// Frame times kept for the graph.
const HISTORY: usize = 240;

pub struct Perf {
    /// The time between frames (ms), newest last.
    times: VecDeque<f32>,
    /// The second being counted: when it began, its frames and its slowest frame.
    second: Option<(Instant, u32, f32)>,
    /// The last whole second: frames per second, average and slowest frame (ms).
    pub fps: f32,
    pub avg: f32,
    pub worst: f32,
    /// Smoothed: building the frame (game, world and UI) and drawing it, which includes
    /// waiting for the display (ms).
    pub cpu: f32,
    pub render: f32,
}

impl Default for Perf {
    fn default() -> Self {
        Self { times: VecDeque::with_capacity(HISTORY), second: None, fps: 0.0, avg: 0.0, worst: 0.0, cpu: 0.0, render: 0.0 }
    }
}

impl Perf {
    /// A frame began, `dt` ms after the last.
    pub fn frame(&mut self, dt: f32) {
        // A pause (minimised, suspended) is no frame time.
        if dt > 1000.0 {
            self.second = None;
            return;
        }
        if self.times.len() == HISTORY {
            self.times.pop_front();
        }
        self.times.push_back(dt);
        let now = Instant::now();
        let (since, frames, worst) = self.second.get_or_insert((now, 0, 0.0));
        *frames += 1;
        *worst = worst.max(dt);
        let span = (now - *since).as_secs_f32();
        if span >= 1.0 {
            self.fps = *frames as f32 / span;
            self.avg = span * 1000.0 / *frames as f32;
            self.worst = *worst;
            self.second = Some((now, 0, 0.0));
        }
    }

    /// How long this frame took to build and to draw (ms).
    pub fn split(&mut self, cpu: f32, render: f32) {
        let ease = |old: f32, new: f32| if old == 0.0 { new } else { old + (new - old) * 0.1 };
        self.cpu = ease(self.cpu, cpu);
        self.render = ease(self.render, render);
    }

    /// The overlay: these lines over the frame-time graph, at the top of the screen.
    pub fn show(&self, ctx: &egui::Context, lines: &[String]) {
        egui::Area::new(egui::Id::unique("perf overlay"))
            .order(egui::Order::Foreground)
            // Under the menu bar.
            .anchor(egui::Align2::CENTER_TOP, [0.0, 50.0])
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::new().fill(egui::Color32::from_black_alpha(170)).corner_radius(4).inner_margin(6).show(ui, |ui| {
                    let font = egui::FontId::monospace(11.0);
                    for line in lines {
                        ui.label(egui::RichText::new(line).font(font.clone()).color(egui::Color32::from_gray(230)));
                    }
                    self.graph(ui);
                });
            });
    }

    /// The frame times as bars: green within 60 fps, yellow within 30, red beyond; the line
    /// marks 60 fps.
    fn graph(&self, ui: &mut egui::Ui) {
        const TOP: f32 = 50.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(HISTORY as f32, 36.0), egui::Sense::hover());
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, egui::Color32::from_black_alpha(120));
        let y = |ms: f32| rect.bottom() - (ms / TOP).min(1.0) * rect.height();
        let start = HISTORY - self.times.len();
        for (i, &ms) in self.times.iter().enumerate() {
            let x = rect.left() + (start + i) as f32 + 0.5;
            let color = match ms {
                ms if ms <= 17.5 => egui::Color32::from_rgb(90, 200, 110),
                ms if ms <= 34.0 => egui::Color32::from_rgb(230, 200, 70),
                _ => egui::Color32::from_rgb(230, 80, 70),
            };
            p.line_segment([egui::pos2(x, rect.bottom()), egui::pos2(x, y(ms))], egui::Stroke::new(1.0, color));
        }
        p.hline(rect.x_range(), y(1000.0 / 60.0), egui::Stroke::new(1.0, egui::Color32::from_white_alpha(60)));
    }
}

#[cfg(test)]
mod tests {
    use super::Perf;

    #[test]
    fn keeps_a_bounded_history_and_skips_pauses() {
        let mut p = Perf::default();
        for _ in 0..1000 {
            p.frame(16.0);
        }
        assert_eq!(p.times.len(), super::HISTORY);
        p.frame(5000.0);
        assert_eq!(p.times.len(), super::HISTORY);
        assert_eq!(*p.times.back().unwrap(), 16.0);
    }
}

//! The macro detector (anti-bot captcha) window: a GM or the server puts us to the test, the
//! image arrives zlib-compressed in chunks, and we type what it shows before the time runs out,
//! with a few tries. Failing can get the account punished (conf/battle/client.conf).

use std::time::{Duration, Instant};

#[derive(Default)]
pub struct Captcha {
    image: Option<egui::TextureHandle>,
    /// Waiting for an answer: tries left and the deadline.
    ask: Option<(u8, Instant)>,
    answer: String,
    focus: bool,
}

impl Captcha {
    /// Decodes the zlib-compressed BMP; true when it worked (the server is then told).
    pub fn set_image(&mut self, ctx: &egui::Context, zipped: &[u8]) -> bool {
        use std::io::Read;
        let mut bmp = vec![];
        if flate2::read::ZlibDecoder::new(zipped).read_to_end(&mut bmp).is_err() {
            tracing::warn!(len = zipped.len(), "captcha image does not decompress");
            return false;
        }
        let Ok(img) = ro_formats::image::decode(&bmp, "captcha.bmp") else {
            tracing::warn!("captcha image does not decode");
            return false;
        };
        let color = egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.pixels);
        self.image = Some(ctx.load_texture("captcha", color, egui::TextureOptions::NEAREST));
        true
    }

    pub fn ask(&mut self, tries: u8, timeout_ms: u32) {
        self.ask = Some((tries, Instant::now() + Duration::from_millis(timeout_ms as u64)));
        self.answer.clear();
        self.focus = true;
    }

    /// The server closed the test: 0 timed out, 1 wrong, 2 correct. Returns the message.
    pub fn done(&mut self, status: u32) -> &'static str {
        *self = Self::default();
        match status {
            2 => "You answered the captcha correctly.",
            1 => "The captcha answer was wrong.",
            _ => "The captcha timed out.",
        }
    }

    /// Draws the window; returns an answer to send.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<String> {
        let (tries, deadline) = self.ask?;
        let mut send = None;
        crate::classic::show(ctx, egui::Window::new("Macro Detector").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]), "Macro Detector", false, |ui| {
            ui.label("Please enter the characters shown below.");
            if let Some(t) = &self.image {
                ui.image((t.id(), egui::vec2(220.0, 90.0)));
            }
            let left = deadline.saturating_duration_since(Instant::now()).as_secs();
            ui.label(format!("Tries left: {tries}    Time left: {left} s"));
            let r = ui.add(egui::TextEdit::singleline(&mut self.answer).char_limit(15).desired_width(220.0));
            if self.focus {
                r.request_focus();
                self.focus = false;
            }
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button("OK").clicked() || enter) && !self.answer.is_empty() {
                send = Some(std::mem::take(&mut self.answer));
            }
        });
        ctx.request_repaint_after(Duration::from_millis(500));
        send
    }
}

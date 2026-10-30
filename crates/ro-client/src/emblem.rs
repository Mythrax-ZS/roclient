//! Guild emblems on rAthena's web server, as 2019+ clients keep them. The guild master uploads
//! a BMP, GIF or PNG (with alpha) of up to 256x256 pixels to /emblem/upload and announces the
//! new version to the map server (CZ_REQ_ADD_NEW_EMBLEM); everyone downloads it from
//! /emblem/download. Older 24x24 BMP emblems still come over the game connection
//! (ZC_GUILD_EMBLEM_IMG), so a guild missing from the web server is asked for there.

use std::sync::mpsc;

use crate::agency::Login;

/// The largest emblem the web server accepts, per side.
pub const MAX_SIDE: u32 = 256;
/// The largest emblem file the web server accepts (a 256x256 32-bit BMP fits).
pub const MAX_BYTES: usize = 307_200;
/// Emblems up to this size are pixel art and keep their hard edges.
const PIXEL_ART: u32 = 24;

pub enum Done {
    Downloaded { guild: u32, version: u32, data: Vec<u8> },
    /// Not on the web server, or it could not be reached: ask the map server.
    Missing { guild: u32 },
    Uploaded { guild: u32, version: u32 },
    UploadFailed(String),
}

/// The file type the web server expects in ImgType, from the file's signature.
pub fn kind(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(b"BM") {
        Some("BMP")
    } else if data.starts_with(b"GIF8") {
        Some("GIF")
    } else if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("PNG")
    } else {
        None
    }
}

/// Checks a file the guild master picked: its type, file size and dimensions.
pub fn check(data: &[u8]) -> Result<&'static str, String> {
    let kind = kind(data).ok_or("The emblem must be a BMP, GIF or PNG image.")?;
    if data.len() > MAX_BYTES {
        return Err(format!("The emblem file is too large ({} KB, at most {} KB).", data.len() / 1024, MAX_BYTES / 1024));
    }
    let img = decode(data).ok_or("The emblem image could not be read.")?;
    if img.width > MAX_SIDE || img.height > MAX_SIDE {
        return Err(format!("The emblem is {}x{}; it can be at most {MAX_SIDE}x{MAX_SIDE} pixels.", img.width, img.height));
    }
    Ok(kind)
}

/// Decodes an emblem file. Magenta is transparent, as in other client art; PNGs keep their alpha.
pub fn decode(data: &[u8]) -> Option<ro_formats::image::Rgba> {
    let img = ro_formats::image::decode(data, "emblem").ok()?;
    (img.width > 0 && img.height > 0).then_some(img)
}

/// A texture to draw the emblem at about `size` pixels. Small emblems are pixel art and are
/// drawn as they are; larger ones are centred on a square, shrunk with Lanczos filtering over
/// premultiplied alpha (so transparent edges do not darken) and drawn smoothly.
pub fn texture(ctx: &egui::Context, name: String, img: &ro_formats::image::Rgba, size: u32) -> egui::TextureHandle {
    if img.width.max(img.height) <= PIXEL_ART {
        let color = egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.pixels);
        return ctx.load_texture(name, color, egui::TextureOptions::NEAREST);
    }
    let side = img.width.max(img.height);
    let mut square = image::RgbaImage::new(side, side);
    let (ox, oy) = ((side - img.width) / 2, (side - img.height) / 2);
    for (i, p) in img.pixels.chunks_exact(4).enumerate() {
        let a = p[3] as u32;
        let pre = |c: u8| ((c as u32 * a + 127) / 255) as u8;
        let (x, y) = (i as u32 % img.width, i as u32 / img.width);
        square.put_pixel(ox + x, oy + y, image::Rgba([pre(p[0]), pre(p[1]), pre(p[2]), p[3]]));
    }
    let out = if side > size { image::imageops::resize(&square, size, size, image::imageops::FilterType::Lanczos3) } else { square };
    let color = egui::ColorImage::from_rgba_premultiplied([out.width() as usize, out.height() as usize], out.as_raw());
    ctx.load_texture(name, color, egui::TextureOptions::LINEAR)
}

/// Requests to the web server, run on worker threads.
pub struct Web {
    login: Option<Login>,
    tx: mpsc::Sender<Done>,
    rx: mpsc::Receiver<Done>,
}

impl Default for Web {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self { login: None, tx, rx }
    }
}

impl Web {
    pub fn set_login(&mut self, login: Login) {
        self.login = Some(login);
    }

    /// Fetches a guild's emblem; false when there is no web login to fetch it with.
    pub fn download(&self, guild: u32, version: u32) -> bool {
        let Some(login) = self.login.clone() else { return false };
        let tx = self.tx.clone();
        spawn(move || {
            let fields = [("GDID", guild.to_string().into_bytes()), ("Version", version.to_string().into_bytes())];
            let done = match post(&login, "/emblem/download", &fields) {
                Ok(data) if !data.is_empty() => Done::Downloaded { guild, version, data },
                Ok(_) => Done::Missing { guild },
                Err(e) => {
                    tracing::debug!(guild, error = %e, "web emblem download");
                    Done::Missing { guild }
                }
            };
            let _ = tx.send(done);
        });
        true
    }

    /// Uploads a checked emblem file; false when there is no web login to upload with.
    pub fn upload(&self, guild: u32, data: Vec<u8>, kind: &'static str) -> bool {
        let Some(login) = self.login.clone() else { return false };
        let tx = self.tx.clone();
        spawn(move || {
            let fields = [("GDID", guild.to_string().into_bytes()), ("ImgType", kind.as_bytes().to_vec()), ("Img", data)];
            #[derive(serde::Deserialize)]
            struct Reply {
                version: u32,
            }
            let done = match post(&login, "/emblem/upload", &fields).and_then(|r| serde_json::from_slice::<Reply>(&r).map_err(|e| e.to_string())) {
                Ok(r) => Done::Uploaded { guild, version: r.version },
                Err(e) => {
                    tracing::warn!(guild, error = %e, "web emblem upload");
                    Done::UploadFailed(format!("The web server refused the emblem ({e})."))
                }
            };
            let _ = tx.send(done);
        });
        true
    }

    pub fn poll(&self) -> Vec<Done> {
        self.rx.try_iter().collect()
    }
}

fn spawn(f: impl FnOnce() + Send + 'static) {
    if let Err(e) = std::thread::Builder::new().name("emblem".into()).spawn(f) {
        tracing::warn!(error = %e, "emblem worker");
    }
}

/// A multipart POST with the web login fields; the response body on success.
fn post(login: &Login, path: &str, fields: &[(&str, Vec<u8>)]) -> Result<Vec<u8>, String> {
    const BOUNDARY: &str = "----roclientEmblemBoundary";
    let mut body = Vec::new();
    let base = [("AID", login.aid.to_string().into_bytes()), ("AuthToken", login.token.clone().into_bytes()), ("WorldName", login.world.clone().into_bytes())];
    for (k, v) in base.iter().map(|(k, v)| (*k, v)).chain(fields.iter().map(|(k, v)| (*k, v))) {
        // The image goes up as a file part, as the original client sends it.
        let file = if k == "Img" { "; filename=\"emblem\"\r\nContent-Type: application/octet-stream" } else { "" };
        body.extend_from_slice(format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{k}\"{file}\r\n\r\n").as_bytes());
        body.extend_from_slice(v);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(10))).build().into();
    let mut resp = agent
        .post(&format!("{}{path}", login.url))
        .header("Content-Type", &format!("multipart/form-data; boundary={BOUNDARY}"))
        .send(&body[..])
        .map_err(|e| e.to_string())?;
    resp.body_mut().with_config().limit(MAX_BYTES as u64 + 1024).read_to_vec().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut img = image::RgbaImage::new(w, h);
        for (x, _, p) in img.enumerate_pixels_mut() {
            *p = image::Rgba([200, 30, 30, if x < w / 2 { 0 } else { 255 }]);
        }
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn png_with_alpha_up_to_256_is_accepted() {
        assert_eq!(check(&png(256, 256)), Ok("PNG"));
        assert_eq!(check(&png(64, 48)), Ok("PNG"));
        let img = decode(&png(64, 48)).unwrap();
        assert_eq!((img.width, img.height), (64, 48));
        assert_eq!(img.pixels[3], 0, "the transparent half keeps its alpha");
        assert_eq!(img.pixels[(40 * 4) + 3], 255);
    }

    #[test]
    fn oversized_or_unknown_emblems_are_refused() {
        assert!(check(&png(257, 16)).unwrap_err().contains("257x16"));
        assert!(check(b"not an image").is_err());
        assert_eq!(kind(b"GIF89a"), Some("GIF"));
        assert_eq!(kind(b"BM...."), Some("BMP"));
    }
}

//! Map background music (mp3nametable.txt → BGM folder) and one-shot sound effects (data\wav).

use std::collections::HashMap;
use std::io::Cursor;

use rodio::Source;

pub struct Audio {
    sink: rodio::MixerDeviceSink,
    bgm: rodio::Player,
    /// Map name → BGM path, as listed in data\mp3nametable.txt.
    table: HashMap<String, String>,
    playing: String,
    pub bgm_volume: f32,
    pub sfx_volume: f32,
}

impl Audio {
    /// None when there is no output device (the client runs silently).
    pub fn new(vfs: &ro_vfs::Vfs, bgm_volume: f32, sfx_volume: f32) -> Option<Self> {
        let mut sink = rodio::DeviceSinkBuilder::open_default_sink().inspect_err(|e| tracing::warn!("no audio output: {e}")).ok()?;
        sink.log_on_drop(false);
        let bgm = rodio::Player::connect_new(sink.mixer());
        bgm.set_volume(bgm_volume);
        let table = vfs.read("data\\mp3nametable.txt").ok().flatten().map(|d| parse_table(&d)).unwrap_or_default();
        Some(Self { sink, bgm, table, playing: String::new(), bgm_volume, sfx_volume })
    }

    /// Switches to the map's track; keeps playing if it is already the current one.
    pub fn enter_map(&mut self, vfs: &ro_vfs::Vfs, map: &str) {
        let Some(path) = self.table.get(map).cloned() else { return };
        if path == self.playing {
            return;
        }
        self.bgm.stop();
        self.playing = path.clone();
        let file = path.rsplit(['\\', '/']).next().unwrap_or(&path);
        let data = [path.clone(), format!("BGM\\{file}")].into_iter().find_map(|p| vfs.read(&p).ok().flatten());
        let Some(data) = data else {
            tracing::warn!(%path, "BGM not found");
            return;
        };
        match rodio::Decoder::try_from(Cursor::new(data)) {
            Ok(src) => self.bgm.append(src.repeat_infinite()),
            Err(e) => tracing::warn!(%path, "BGM: {e}"),
        }
    }

    pub fn set_bgm_volume(&mut self, v: f32) {
        self.bgm_volume = v;
        self.bgm.set_volume(v);
    }

    /// Plays data\wav\<name> once over whatever else is playing.
    pub fn play(&self, vfs: &ro_vfs::Vfs, name: &str) {
        if self.sfx_volume <= 0.0 || name.is_empty() {
            return;
        }
        let Some(data) = vfs.read(&format!("data\\wav\\{name}")).ok().flatten() else { return };
        if let Ok(src) = rodio::Decoder::try_from(Cursor::new(data)) {
            self.sink.mixer().add(src.amplify(self.sfx_volume));
        }
    }
}

/// Lines look like `prontera.rsw#bgm\\08.mp3#`; `//` comments the line out.
fn parse_table(data: &[u8]) -> HashMap<String, String> {
    let (text, _, _) = encoding_rs::EUC_KR.decode(data);
    text.lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .filter_map(|l| {
            let mut parts = l.split('#');
            let map = parts.next()?.trim().strip_suffix(".rsw")?.to_ascii_lowercase();
            let path = parts.next()?.trim().replace("\\\\", "\\");
            Some((map, path))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_bgm_table() {
        let t = super::parse_table(br"prontera.rsw#bgm\\08.mp3#
//prontera.rsw#bgm\\55.mp3#
Prt_Fild08.rsw#bgm\\12.mp3#
");
        assert_eq!(t["prontera"], r"bgm\08.mp3");
        assert_eq!(t["prt_fild08"], r"bgm\12.mp3");
        assert_eq!(t.len(), 2);
    }
}

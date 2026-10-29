//! Virtual file system: loose data folder overlaid on a list of GRFs.

mod des;
pub mod grf;

pub use grf::Grf;

use std::path::PathBuf;

/// Normalized lookup key: backslashes, lowercase (ASCII only; Korean is case-less).
pub fn normalize(path: &str) -> String {
    path.replace('/', "\\").to_ascii_lowercase()
}

/// Lookup order: overlay folders (translation packs), the loose folder, then GRFs in the
/// order given.
#[derive(Default)]
pub struct Vfs {
    /// Folders laid over everything else, first wins. Translation packs keep their System
    /// files in SystemEN\, which a `System\...` lookup tries first.
    pub overlays: Vec<PathBuf>,
    pub loose: Option<PathBuf>,
    pub grfs: Vec<Grf>,
}

impl Vfs {
    /// The highest-priority copy of a file.
    pub fn read(&self, path: &str) -> anyhow::Result<Option<Vec<u8>>> {
        let mut found = None;
        self.layers(path, &mut |d| {
            found = Some(d);
            false
        })?;
        Ok(found)
    }

    /// Every layer's copy of a file, highest priority first: for tables that are merged
    /// (a translation pack's text over the base data's newer entries).
    pub fn read_all(&self, path: &str) -> anyhow::Result<Vec<Vec<u8>>> {
        let mut all = vec![];
        self.layers(path, &mut |d| {
            all.push(d);
            true
        })?;
        Ok(all)
    }

    /// Feeds each layer's copy to `each` in priority order until it returns false.
    fn layers(&self, path: &str, each: &mut dyn FnMut(Vec<u8>) -> bool) -> anyhow::Result<()> {
        let rel = path.replace('\\', "/");
        for root in &self.overlays {
            let en = rel.get(..7).filter(|p| p.eq_ignore_ascii_case("System/")).map(|_| root.join(format!("SystemEN/{}", &rel[7..])));
            if let Some(p) = en.into_iter().chain(std::iter::once(root.join(&rel))).find(|p| p.is_file()) {
                if !each(std::fs::read(p)?) {
                    return Ok(());
                }
            }
        }
        if let Some(root) = &self.loose {
            let p = root.join(&rel);
            if p.is_file() && !each(std::fs::read(p)?) {
                return Ok(());
            }
        }
        for g in &self.grfs {
            if let Some(d) = g.read(path)? {
                if !each(d) {
                    return Ok(());
                }
            }
        }
        Ok(())
    }
}

impl Vfs {
    /// Every file name in the GRFs (normalized, backslashes).
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.grfs.iter().flat_map(|g| g.names())
    }
}

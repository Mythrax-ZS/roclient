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
    pub fn read(&self, path: &str) -> anyhow::Result<Option<Vec<u8>>> {
        let rel = path.replace('\\', "/");
        for root in &self.overlays {
            let en = rel.get(..7).filter(|p| p.eq_ignore_ascii_case("System/")).map(|_| root.join(format!("SystemEN/{}", &rel[7..])));
            for p in en.into_iter().chain(std::iter::once(root.join(&rel))) {
                if p.is_file() {
                    return Ok(Some(std::fs::read(p)?));
                }
            }
        }
        if let Some(root) = &self.loose {
            let p = root.join(path.replace('\\', "/"));
            if p.is_file() {
                return Ok(Some(std::fs::read(p)?));
            }
        }
        for g in &self.grfs {
            if let Some(d) = g.read(path)? {
                return Ok(Some(d));
            }
        }
        Ok(None)
    }
}

impl Vfs {
    /// Every file name in the GRFs (normalized, backslashes).
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.grfs.iter().flat_map(|g| g.names())
    }
}

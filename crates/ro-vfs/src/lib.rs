//! Virtual file system: loose data folder overlaid on a list of GRFs.

mod grf;

pub use grf::Grf;

use std::path::PathBuf;

/// Normalized lookup key: backslashes, lowercase (ASCII only; Korean is case-less).
pub fn normalize(path: &str) -> String {
    path.replace('/', "\\").to_ascii_lowercase()
}

/// Lookup order: loose folder first, then GRFs in the order given.
#[derive(Default)]
pub struct Vfs {
    pub loose: Option<PathBuf>,
    pub grfs: Vec<Grf>,
}

impl Vfs {
    pub fn read(&self, path: &str) -> anyhow::Result<Option<Vec<u8>>> {
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

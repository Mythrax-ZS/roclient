//! Virtual file system: loose data folder overlaid on a list of GRFs.

mod des;
pub mod grf;

pub use grf::Grf;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

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
    /// Every file in each overlay folder, then the loose folder, by lowercased `/` path:
    /// the game's paths ignore case, Linux file names do not. Built on first use.
    folders: OnceLock<Vec<HashMap<String, PathBuf>>>,
}

impl Vfs {
    pub fn new(overlays: Vec<PathBuf>, loose: Option<PathBuf>, grfs: Vec<Grf>) -> Self {
        Self { overlays, loose, grfs, folders: OnceLock::new() }
    }

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
        let rel = path.replace('\\', "/").to_ascii_lowercase();
        let folders = self.folders.get_or_init(|| self.overlays.iter().chain(&self.loose).map(|root| index(root)).collect());
        for (i, files) in folders.iter().enumerate() {
            let en = rel.strip_prefix("system/").filter(|_| i < self.overlays.len()).and_then(|r| files.get(&format!("systemen/{r}")));
            if let Some(p) = en.or_else(|| files.get(&rel)) {
                if !each(std::fs::read(p)?) {
                    return Ok(());
                }
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

/// Every file under `root`, keyed by its lowercased path relative to it with `/` separators.
fn index(root: &Path) -> HashMap<String, PathBuf> {
    let mut files = HashMap::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let path = e.path();
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                dirs.push(path);
            } else if let Some(rel) = path.strip_prefix(root).ok().and_then(Path::to_str) {
                files.insert(rel.replace('\\', "/").to_ascii_lowercase(), path.clone());
            }
        }
    }
    files
}

impl Vfs {
    /// Every file name in the GRFs (normalized, backslashes).
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.grfs.iter().flat_map(|g| g.names())
    }
}

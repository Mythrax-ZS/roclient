//! GAT: per-cell walkability and heights (the server's view of the map).

use anyhow::{ensure, Result};

use crate::reader::Reader;

#[derive(Debug)]
pub struct Gat {
    pub width: u32,
    pub height: u32,
    /// Cell type: 0 walkable, 1 blocked, 3 walkable water, 5 cliff (blocked, shootable).
    pub cells: Vec<u32>,
}

impl Gat {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        ensure!(r.bytes(4)? == b"GRAT", "not a GAT file");
        r.bytes(2)?; // version
        let width = r.u32()?;
        let height = r.u32()?;
        let mut cells = Vec::with_capacity((width * height) as usize);
        for _ in 0..width * height {
            r.bytes(16)?; // corner heights
            cells.push(r.u32()?);
        }
        Ok(Self { width, height, cells })
    }

    pub fn walkable(&self, x: i32, y: i32) -> bool {
        if x < 0 || y < 0 || x as u32 >= self.width || y as u32 >= self.height {
            return false;
        }
        matches!(self.cells[(y as u32 * self.width + x as u32) as usize], 0 | 3)
    }
}

//! GAT: per-cell walkability and heights (the server's view of the map).

use anyhow::{ensure, Result};

use crate::reader::Reader;

#[derive(Debug)]
pub struct Gat {
    pub width: u32,
    pub height: u32,
    /// Cell type: 0 walkable, 1 blocked, 3 walkable water, 5 cliff (blocked, shootable); see
    /// `walkable` for the rest.
    pub cells: Vec<u32>,
    /// Corner heights per cell, RO space (y down), corners (x,y), (x+1,y), (x,y+1), (x+1,y+1).
    /// Unlike the GND these include walkable model surfaces such as stairs and bridges.
    pub heights: Vec<[f32; 4]>,
}

impl Gat {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        ensure!(r.bytes(4)? == b"GRAT", "not a GAT file");
        r.bytes(2)?; // version
        let width = r.u32()?;
        let height = r.u32()?;
        let n = (width * height) as usize;
        let mut cells = Vec::with_capacity(n);
        let mut heights = Vec::with_capacity(n);
        for _ in 0..n {
            heights.push([r.f32()?, r.f32()?, r.f32()?, r.f32()?]);
            cells.push(r.u32()?);
        }
        Ok(Self { width, height, cells, heights })
    }

    /// Changes a cell's type (the server's setcell / setwall).
    pub fn set_cell(&mut self, x: i32, y: i32, kind: u32) {
        if x >= 0 && y >= 0 && (x as u32) < self.width && (y as u32) < self.height {
            self.cells[(y as u32 * self.width + x as u32) as usize] = kind;
        }
    }

    /// As the server reads cells (rAthena's map cache keeps the low byte; map_gat2cell walks
    /// 0, 2, 3, 4 and 6). Newer maps flag cells in the high bits (0x80000000) or use type 6.
    pub fn walkable(&self, x: i32, y: i32) -> bool {
        if x < 0 || y < 0 || x as u32 >= self.width || y as u32 >= self.height {
            return false;
        }
        matches!(self.cells[(y as u32 * self.width + x as u32) as usize] as u8, 0 | 2 | 3 | 4 | 6)
    }

    /// Surface height (RO space, y down) at a fractional cell position; cell (x, y) spans
    /// [x, x+1) x [y, y+1). None outside the map.
    pub fn height_at(&self, fx: f32, fy: f32) -> Option<f32> {
        let (cx, cy) = (fx.floor(), fy.floor());
        if cx < 0.0 || cy < 0.0 || cx as u32 >= self.width || cy as u32 >= self.height {
            return None;
        }
        let h = self.heights[(cy as u32 * self.width + cx as u32) as usize];
        let (tx, ty) = (fx - cx, fy - cy);
        let near = h[0] + (h[1] - h[0]) * tx;
        let far = h[2] + (h[3] - h[2]) * tx;
        Some(near + (far - near) * ty)
    }
}

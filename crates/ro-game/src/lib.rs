//! Client-side game state: walking along server-confirmed paths.

pub mod items;
pub mod looks;
pub mod path;
pub mod skills;

/// RO facing, counter-clockwise from south (ACT action order).
pub fn dir_from_step(dx: i32, dy: i32) -> usize {
    match (dx.signum(), dy.signum()) {
        (0, -1) => 0,  // S
        (-1, -1) => 1, // SW
        (-1, 0) => 2,  // W
        (-1, 1) => 3,  // NW
        (0, 1) => 4,   // N
        (1, 1) => 5,   // NE
        (1, 0) => 6,   // E
        (1, -1) => 7,  // SE
        _ => 0,
    }
}

/// An actor walking cell to cell. Positions are fractional cells.
#[derive(Debug, Clone)]
pub struct Walker {
    pub cell: (i32, i32),
    pub dir: usize,
    /// Milliseconds per straight cell (the char's speed stat; 150 by default).
    pub speed: f32,
    path: Vec<(i32, i32)>,
    /// Time spent along the current path.
    elapsed: f32,
}

impl Walker {
    pub fn new(cell: (i32, i32), dir: usize, speed: f32) -> Self {
        Self { cell, dir, speed, path: vec![], elapsed: 0.0 }
    }

    pub fn is_walking(&self) -> bool {
        !self.path.is_empty()
    }

    /// Where the walk ends, or the current cell when standing.
    pub fn goal_or_cell(&self) -> (i32, i32) {
        self.path.last().copied().unwrap_or(self.cell)
    }

    /// Starts walking along `path` (cells after the current one).
    pub fn walk(&mut self, path: Vec<(i32, i32)>) {
        self.path = path;
        self.elapsed = 0.0;
    }

    /// Follows a new path that starts at `from` without losing the current step: if `from`
    /// is the cell we are leaving or the one we are heading to, the step in progress is
    /// kept and the new path continues from it (no snap back when re-aiming a walk).
    /// Returns false when `from` is elsewhere (the caller should place us there).
    pub fn redirect(&mut self, from: (i32, i32), path: Vec<(i32, i32)>) -> bool {
        match self.path.first().copied() {
            Some(next) if next == from => {
                self.path = std::iter::once(next).chain(path).collect();
                true
            }
            Some(next) if self.cell == from && path.first() == Some(&next) => {
                self.path = path;
                true
            }
            _ if self.cell == from => {
                self.walk(path);
                true
            }
            _ => false,
        }
    }

    /// Stops at a cell (server correction).
    pub fn stop_at(&mut self, cell: (i32, i32)) {
        self.cell = cell;
        self.path.clear();
    }

    fn step_cost(&self, from: (i32, i32), to: (i32, i32)) -> f32 {
        if from.0 != to.0 && from.1 != to.1 { self.speed * 1.4 } else { self.speed }
    }

    /// Advances by `dt_ms`; returns the fractional position.
    pub fn update(&mut self, dt_ms: f32) -> (f32, f32) {
        self.elapsed += dt_ms;
        loop {
            let Some(&next) = self.path.first() else {
                return (self.cell.0 as f32, self.cell.1 as f32);
            };
            let cost = self.step_cost(self.cell, next);
            self.dir = dir_from_step(next.0 - self.cell.0, next.1 - self.cell.1);
            if self.elapsed < cost {
                let t = self.elapsed / cost;
                return (
                    self.cell.0 as f32 + (next.0 - self.cell.0) as f32 * t,
                    self.cell.1 as f32 + (next.1 - self.cell.1) as f32 * t,
                );
            }
            self.elapsed -= cost;
            self.cell = next;
            self.path.remove(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walks_at_speed() {
        let mut w = Walker::new((0, 0), 0, 100.0);
        w.walk(vec![(1, 0), (2, 0)]);
        assert_eq!(w.update(50.0), (0.5, 0.0));
        assert_eq!(w.dir, 6);
        assert_eq!(w.update(100.0), (1.5, 0.0));
        assert_eq!(w.update(100.0), (2.0, 0.0));
        assert!(!w.is_walking());
    }
}

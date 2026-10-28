//! Walk paths, matching rAthena's `path_search` (A*, 10/14 costs, Manhattan heuristic,
//! no diagonal corner cutting) so the client walks the same cells the server does.

use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap},
};

/// rAthena MAX_WALKPATH: longer requests are refused by the server.
pub const MAX_WALKPATH: usize = 32;
const MOVE_COST: i32 = 10;
const MOVE_DIAGONAL_COST: i32 = 14;

pub trait Walkable {
    fn walkable(&self, x: i32, y: i32) -> bool;
}

impl Walkable for ro_formats::gat::Gat {
    fn walkable(&self, x: i32, y: i32) -> bool {
        ro_formats::gat::Gat::walkable(self, x, y)
    }
}

fn heuristic(x0: i32, y0: i32, x1: i32, y1: i32) -> i32 {
    MOVE_COST * ((x1 - x0).abs() + (y1 - y0).abs())
}

/// Cells from start (exclusive) to goal (inclusive) for a single server walk: at most
/// MAX_WALKPATH steps, or None.
pub fn find(map: &impl Walkable, from: (i32, i32), to: (i32, i32)) -> Option<Vec<(i32, i32)>> {
    search(map, from, to, MAX_WALKPATH * MAX_WALKPATH).filter(|p| p.len() <= MAX_WALKPATH)
}

/// Like [`find`] but without the server's length cap, for planning long walks as a chain
/// of server-sized hops. Bounded by `max_nodes` expanded cells.
pub fn find_long(map: &impl Walkable, from: (i32, i32), to: (i32, i32), max_nodes: usize) -> Option<Vec<(i32, i32)>> {
    search(map, from, to, max_nodes)
}

fn search(map: &impl Walkable, from: (i32, i32), to: (i32, i32), max_nodes: usize) -> Option<Vec<(i32, i32)>> {
    if !map.walkable(to.0, to.1) {
        return None;
    }
    if from == to {
        return Some(vec![]);
    }
    // (f, tie-break counter) min-heap; the counter keeps expansion order stable.
    let mut open = BinaryHeap::new();
    let mut g: HashMap<(i32, i32), i32> = HashMap::new();
    let mut parent: HashMap<(i32, i32), (i32, i32)> = HashMap::new();
    let mut closed = std::collections::HashSet::new();
    let mut seq = 0u32;
    g.insert(from, 0);
    open.push(Reverse((heuristic(from.0, from.1, to.0, to.1), seq, from)));

    while let Some(Reverse((_, _, cur))) = open.pop() {
        if !closed.insert(cur) {
            continue;
        }
        if cur == to {
            break;
        }
        // Bound the search the way the server's fixed-size node table does.
        if closed.len() > max_nodes {
            return None;
        }
        let (x, y) = cur;
        let gc = g[&cur];
        let n = map.walkable(x, y + 1);
        let s = map.walkable(x, y - 1);
        let e = map.walkable(x + 1, y);
        let w = map.walkable(x - 1, y);
        // Same neighbour order as rAthena.
        let candidates = [
            (s && e && map.walkable(x + 1, y - 1), (x + 1, y - 1), MOVE_DIAGONAL_COST),
            (e, (x + 1, y), MOVE_COST),
            (n && e && map.walkable(x + 1, y + 1), (x + 1, y + 1), MOVE_DIAGONAL_COST),
            (n, (x, y + 1), MOVE_COST),
            (n && w && map.walkable(x - 1, y + 1), (x - 1, y + 1), MOVE_DIAGONAL_COST),
            (w, (x - 1, y), MOVE_COST),
            (s && w && map.walkable(x - 1, y - 1), (x - 1, y - 1), MOVE_DIAGONAL_COST),
            (s, (x, y - 1), MOVE_COST),
        ];
        for (ok, next, cost) in candidates {
            if !ok || closed.contains(&next) {
                continue;
            }
            let ng = gc + cost;
            if g.get(&next).is_none_or(|&old| ng < old) {
                g.insert(next, ng);
                parent.insert(next, cur);
                seq += 1;
                open.push(Reverse((ng + heuristic(next.0, next.1, to.0, to.1), seq, next)));
            }
        }
    }

    if !parent.contains_key(&to) {
        return None;
    }
    let mut path = vec![to];
    let mut c = to;
    while let Some(&p) = parent.get(&c) {
        if p == from {
            break;
        }
        path.push(p);
        c = p;
    }
    path.reverse();
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Grid(Vec<&'static str>);
    impl Walkable for Grid {
        /// Row 0 of the literal is the top (highest y).
        fn walkable(&self, x: i32, y: i32) -> bool {
            let h = self.0.len() as i32;
            if x < 0 || y < 0 || y >= h {
                return false;
            }
            self.0[(h - 1 - y) as usize].as_bytes().get(x as usize) == Some(&b'.')
        }
    }

    #[test]
    fn straight_and_diagonal() {
        let g = Grid(vec![".....", ".....", "....."]);
        assert_eq!(find(&g, (0, 0), (2, 2)).unwrap(), vec![(1, 1), (2, 2)]);
        assert_eq!(find(&g, (0, 1), (3, 1)).unwrap(), vec![(1, 1), (2, 1), (3, 1)]);
    }

    #[test]
    fn walks_around_corners() {
        // Wall at (1,1): going (0,0) -> (2,2) may not cut its corners.
        let g = Grid(vec!["...", ".#.", "..."]);
        let p = find(&g, (0, 0), (2, 2)).unwrap();
        assert_eq!(p.len(), 4);
        assert!(!p.contains(&(1, 1)));
    }

    #[test]
    fn unreachable() {
        let g = Grid(vec!["..#..", "..#..", "..#.."]);
        assert!(find(&g, (0, 0), (4, 0)).is_none());
    }
}

#[cfg(test)]
mod long_tests {
    use super::*;

    struct Open(i32);
    impl Walkable for Open {
        fn walkable(&self, x: i32, y: i32) -> bool {
            (0..self.0).contains(&x) && (0..self.0).contains(&y)
        }
    }

    #[test]
    fn long_paths_exceed_the_server_cap() {
        let map = Open(100);
        assert!(find(&map, (0, 0), (60, 0)).is_none());
        assert_eq!(find_long(&map, (0, 0), (60, 0), 100_000).unwrap().len(), 60);
    }
}

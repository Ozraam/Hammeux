//! Recherche de chemin (A*, 8 directions) sur la carte.

use std::{cmp::Reverse, collections::BinaryHeap};

use crate::map::Map;

/// Tampons réutilisés d'une recherche à l'autre (marqués par génération, jamais remis à zéro).
#[derive(Debug, Default)]
pub struct PathFinder {
    g: Vec<f32>,
    came: Vec<u32>,
    seen: Vec<u32>,
    generation: u32,
}

const DIRS: [(i64, i64, f32); 8] = [
    (1, 0, 1.0),
    (-1, 0, 1.0),
    (0, 1, 1.0),
    (0, -1, 1.0),
    (1, 1, 1.414),
    (-1, 1, 1.414),
    (1, -1, 1.414),
    (-1, -1, 1.414),
];

impl PathFinder {
    /// Chemin de `start` (exclu) à `goal` (inclus). `blocked(i)` exclut des cases (bâtiments) ;
    /// la case d'arrivée est toujours acceptée. `None` si inaccessible ou trop loin pour `budget`.
    pub fn find(
        &mut self,
        map: &Map,
        start: u32,
        goal: u32,
        blocked: impl Fn(usize) -> bool,
        budget: usize,
    ) -> Option<Vec<u32>> {
        if start == goal {
            return Some(Vec::new());
        }
        let n = map.len();
        if self.g.len() != n {
            self.g = vec![0.0; n];
            self.came = vec![0; n];
            self.seen = vec![0; n];
            self.generation = 0;
        }
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            self.seen.fill(0);
            self.generation = 1;
        }
        let gen_ = self.generation;
        let (gx, gy) = map.xy(goal as usize);
        let h = |i: u32| {
            let (x, y) = map.xy(i as usize);
            let dx = (x as f32 - gx as f32).abs();
            let dy = (y as f32 - gy as f32).abs();
            // Distance octogonale × coût minimal (route) : heuristique admissible.
            (dx.max(dy) + 0.414 * dx.min(dy)) * 0.6
        };
        let key = |f: f32, i: u32| Reverse(((f.to_bits() as u64) << 32) | i as u64);
        let mut heap = BinaryHeap::new();
        self.g[start as usize] = 0.0;
        self.seen[start as usize] = gen_;
        self.came[start as usize] = start;
        heap.push(key(h(start), start));
        let mut expanded = 0;
        while let Some(Reverse(k)) = heap.pop() {
            let i = (k & 0xFFFF_FFFF) as u32;
            if i == goal {
                let mut path = vec![goal];
                let mut c = goal;
                while self.came[c as usize] != start {
                    c = self.came[c as usize];
                    path.push(c);
                }
                path.reverse();
                return Some(path);
            }
            let f = f32::from_bits((k >> 32) as u32);
            let gi = self.g[i as usize];
            if f > gi + h(i) + 1e-3 {
                continue; // entrée périmée
            }
            expanded += 1;
            if expanded > budget {
                return None;
            }
            let (x, y) = map.xy(i as usize);
            for (dx, dy, len) in DIRS {
                let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                if !map.inside(nx, ny) {
                    continue;
                }
                let j = (ny as u32 * map.width + nx as u32) as usize;
                if j as u32 != goal && blocked(j) {
                    continue;
                }
                let cost = match map.walk_cost(j) {
                    Some(c) => c,
                    // Arrivée sur l'eau (puits, ponton) : acceptée.
                    None if j as u32 == goal => 1.0,
                    None => continue,
                };
                // Pas de coin coupé entre deux obstacles en diagonale.
                if dx != 0 && dy != 0 {
                    let a = (y * map.width + nx as u32) as usize;
                    let b = (ny as u32 * map.width + x) as usize;
                    if map.walk_cost(a).is_none() || map.walk_cost(b).is_none() {
                        continue;
                    }
                }
                let ng = gi + cost * len;
                if self.seen[j] != gen_ || ng < self.g[j] {
                    self.seen[j] = gen_;
                    self.g[j] = ng;
                    self.came[j] = i;
                    heap.push(key(ng + h(j as u32), j as u32));
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Biome, Map};

    #[test]
    fn walks_around_water() {
        // 10 × 5, mur d'eau en x = 5 sauf en y = 4.
        let mut m = Map::new(10, 5);
        for i in 0..m.len() {
            m.biome[i] = Biome::Meadow as u8;
        }
        for y in 0..4 {
            let i = m.idx(5, y);
            m.biome[i] = Biome::Lake as u8;
        }
        let mut pf = PathFinder::default();
        let path = pf
            .find(&m, m.idx(0, 0) as u32, m.idx(9, 0) as u32, |_| false, 10_000)
            .unwrap();
        assert_eq!(*path.last().unwrap(), m.idx(9, 0) as u32);
        assert!(path.iter().all(|&i| m.walk_cost(i as usize).is_some()));
        assert!(
            path.iter().any(|&i| m.xy(i as usize).1 == 4),
            "passe par le gué en bas"
        );
    }

    #[test]
    fn unreachable() {
        let mut m = Map::new(6, 3);
        for i in 0..m.len() {
            m.biome[i] = if m.xy(i).0 == 3 {
                Biome::Lake as u8
            } else {
                Biome::Meadow as u8
            };
        }
        let mut pf = PathFinder::default();
        assert!(
            pf.find(&m, m.idx(0, 1) as u32, m.idx(5, 1) as u32, |_| false, 10_000)
                .is_none()
        );
    }
}

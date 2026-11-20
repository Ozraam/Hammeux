//! Génération d'un monde à partir d'une graine.
//!
//! Étapes : relief (bruit + masque de continent) → mer et lacs → écoulement de l'eau et
//! rivières → humidité et température → milieux naturels → fertilité et ressources →
//! emplacement et nom des villages. Même graine + mêmes paramètres = même monde.

// Les couches de la carte sont indexées en parallèle : les boucles sur `0..n` sont plus lisibles.
#![allow(clippy::needless_range_loop)]

mod names;

use std::collections::{BinaryHeap, VecDeque};

use hameaux_core::rng::Rng;
use hameaux_core::{Biome, Map, Resource, Site, World, map::SEA_LEVEL};
use noise::{Fbm, MultiFractal, NoiseFn, Perlin, RidgedMulti};
use rayon::prelude::*;

#[derive(Debug, Clone)]
pub struct Params {
    pub width: u32,
    pub height: u32,
    /// Nombre de villages visé.
    pub villages: usize,
    /// Part de la carte sous le niveau de la mer.
    pub water: f32,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            width: 1024,
            height: 1024,
            villages: 12,
            water: 0.38,
        }
    }
}

pub fn generate(seed: u64, p: &Params) -> World {
    generate_at(seed, p, 0)
}

/// Comme [`generate`], mais les villages sont fondés au tick donné (reprise d'une sauvegarde).
pub fn generate_at(seed: u64, p: &Params, tick: u64) -> World {
    let (w, h) = (p.width, p.height);
    let mut map = Map::new(w, h);
    let n = map.len();

    // 1. Relief brut, puis répartition par rang : on maîtrise la part de mer et la rareté
    //    des sommets quel que soit le bruit.
    let raw = relief(seed, w, h);
    let rank = ranks(&raw);
    let wf = p.water;
    let height: Vec<f32> = rank.iter().map(|&r| r as f32 / n as f32).collect();
    for i in 0..n {
        map.elevation[i] = quantize(height[i], wf);
    }
    map.shade = hillshade(w, h, &height, wf);

    // 2. Mer = eau reliée au bord de la carte. Le reste sous le niveau de la mer = lacs.
    let ocean = ocean_mask(&map);

    // 3. Écoulement : chaque case de terre trouve sa pente jusqu'à la mer (remplissage par
    //    priorité, les cuvettes deviennent des lacs), puis on cumule la pluie vers l'aval.
    let moisture_noise = field(w, h, {
        let f = Fbm::<Perlin>::new(Rng::derive(seed, 2)).set_octaves(5);
        move |x, y| f.get([x * 3.0, y * 3.0]) * 0.5 + 0.5
    });
    let flow = drainage(&map, &height, &ocean, &moisture_noise);
    // Seules les cuvettes profondes deviennent des lacs ; les autres sont traversées par la rivière.
    let lake_depth = 0.015;
    let mut water = vec![false; n];
    for i in 0..n {
        let lake = !ocean[i] && flow.filled[i] - height[i] > lake_depth;
        if ocean[i] {
            water[i] = true;
            map.biome[i] = if map.elevation[i] < SEA_LEVEL - 18 {
                Biome::DeepSea
            } else {
                Biome::Sea
            } as u8;
        } else if lake {
            water[i] = true;
            map.biome[i] = Biome::Lake as u8;
            map.elevation[i] = quantize(flow.filled[i], wf).max(SEA_LEVEL);
        } else if map.elevation[i] < SEA_LEVEL {
            // Creux intérieur peu profond : une plaine basse, pas une mer.
            map.elevation[i] = SEA_LEVEL;
        }
        if water[i] {
            map.shade[i] = 128;
        }
    }
    let threshold = n as f32 * 0.0012;
    for i in 0..n {
        if water[i] {
            continue;
        }
        let a = flow.accumulation[i];
        let size = if a > threshold * 16.0 {
            3
        } else if a > threshold * 4.0 {
            2
        } else if a > threshold {
            1
        } else {
            0
        };
        map.flags[i] |= size;
    }

    // 4. Humidité (bruit + proximité de l'eau douce ou salée) et température (nord froid,
    //    altitude froide).
    let fresh: Vec<bool> = (0..n)
        .map(|i| map.biome_at(i) == Biome::Lake || map.river_at(i) > 0)
        .collect();
    let near_water = distance(&map, |i| water[i] || fresh[i], 10);
    let near_ocean = distance(&map, |i| ocean[i], 10);
    let temp_noise = field(w, h, {
        let f = Fbm::<Perlin>::new(Rng::derive(seed, 3)).set_octaves(3);
        move |x, y| f.get([x * 2.0, y * 2.0])
    });
    let mut moisture = vec![0f32; n];
    let mut temperature = vec![0f32; n];
    for y in 0..h {
        for x in 0..w {
            let i = map.idx(x, y);
            let wet = 1.0 - near_water[i] as f32 / 10.0;
            moisture[i] = (moisture_noise[i] as f32 + wet * 0.12).clamp(0.0, 1.0);
            let alt = map.elevation[i].saturating_sub(SEA_LEVEL) as f32 / (255 - SEA_LEVEL) as f32;
            temperature[i] = 1.0 - 0.45 * (y as f32 / h as f32) - alt * 0.75 + temp_noise[i] as f32 * 0.08;
        }
    }

    // 5. Milieux naturels.
    for i in 0..n {
        if water[i] {
            continue;
        }
        let e = map.elevation[i];
        let (m, t) = (moisture[i], temperature[i]);
        let b = if near_ocean[i] <= 2 && e < SEA_LEVEL + 4 {
            Biome::Beach
        } else if e > 244 || t < 0.1 {
            Biome::Snow
        } else if e > 205 {
            Biome::Mountain
        } else if e > 165 {
            if m > 0.62 { Biome::Conifers } else { Biome::Hills }
        } else if t < 0.36 {
            if m > 0.45 { Biome::Conifers } else { Biome::Heath }
        } else if m > 0.78 && e < SEA_LEVEL + 14 && near_water[i] <= 4 {
            Biome::Marsh
        } else if m > 0.6 {
            Biome::Forest
        } else if m > 0.42 {
            Biome::Meadow
        } else if m > 0.24 {
            Biome::Grassland
        } else {
            Biome::Heath
        };
        map.biome[i] = b as u8;
    }

    // 6. Fertilité et ressources.
    let patches = |salt: u64, freq: f64| {
        let f = Fbm::<Perlin>::new(Rng::derive(seed, salt)).set_octaves(3);
        field(w, h, move |x, y| f.get([x * freq, y * freq]) * 0.5 + 0.5)
    };
    let (stone, iron, clay, fish, game, berries) = (
        patches(10, 24.0),
        patches(11, 30.0),
        patches(12, 28.0),
        patches(13, 20.0),
        patches(14, 22.0),
        patches(15, 40.0),
    );
    for i in 0..n {
        let b = map.biome_at(i);
        let base = match b {
            Biome::Meadow => 1.0,
            Biome::Grassland => 0.7,
            Biome::Forest => 0.55,
            Biome::Marsh => 0.35,
            Biome::Heath | Biome::Hills | Biome::Conifers => 0.3,
            Biome::Beach => 0.1,
            _ => 0.0,
        };
        let river_bonus = if near_water[i] <= 3 && !water[i] {
            0.15
        } else {
            0.0
        };
        map.fertility[i] = ((base * (0.6 + 0.4 * moisture[i]) + river_bonus).min(1.0) * 255.0) as u8;

        let r = match b {
            Biome::Mountain | Biome::Hills if iron[i] > 0.74 => Resource::Iron,
            Biome::Mountain | Biome::Hills if stone[i] > 0.64 => Resource::Stone,
            Biome::Sea | Biome::Lake if near_water_land(&map, i, &water) && fish[i] > 0.6 => Resource::Fish,
            Biome::Forest | Biome::Conifers if game[i] > 0.66 => Resource::Game,
            Biome::Forest | Biome::Meadow | Biome::Grassland if berries[i] > 0.82 => Resource::Berries,
            _ if !water[i] && near_water[i] <= 2 && moisture[i] > 0.5 && clay[i] > 0.72 => Resource::Clay,
            _ => Resource::None,
        };
        map.resource[i] = r as u8;
    }

    // 7. Villages.
    let sites = place_villages(seed, &map, &near_ocean, p.villages);
    World::from_map_at(seed, map, &sites, tick)
}

/// Bruit de relief : continent déformé, chaînes de montagnes par endroits, mer sur les bords.
fn relief(seed: u64, w: u32, h: u32) -> Vec<f64> {
    let base = Fbm::<Perlin>::new(Rng::derive(seed, 0))
        .set_octaves(7)
        .set_persistence(0.5);
    let warp = Fbm::<Perlin>::new(Rng::derive(seed, 1)).set_octaves(3);
    let ridge = RidgedMulti::<Perlin>::new(Rng::derive(seed, 4)).set_octaves(5);
    let mountains = Fbm::<Perlin>::new(Rng::derive(seed, 5)).set_octaves(2);
    let detail = Fbm::<Perlin>::new(Rng::derive(seed, 6)).set_octaves(3);
    field(w, h, move |x, y| {
        let wx = x + 0.22 * warp.get([x * 2.0, y * 2.0]);
        let wy = y + 0.22 * warp.get([x * 2.0 + 5.2, y * 2.0 + 1.3]);
        let b = base.get([wx * 3.0, wy * 3.0]);
        // Distance au centre, déformée pour que la côte ne soit pas un cercle.
        let d = ((x - 0.5).powi(2) + (y - 0.5).powi(2)).sqrt() * 2.0
            + 0.22 * warp.get([x * 1.3 + 9.1, y * 1.3 + 3.7]);
        let edge = smoothstep(0.5, 1.0, d);
        // Dôme continental : l'intérieur est plus haut, pas de mer intérieure géante.
        let dome = (1.0 - d).clamp(0.0, 1.0) * 0.45;
        let mm = smoothstep(0.05, 0.5, mountains.get([x * 1.6, y * 1.6]));
        let r = ridge.get([wx * 4.0, wy * 4.0]) * 0.5 + 0.5;
        // Petit grain : casse les pentes trop régulières (rivières moins rectilignes).
        let grain = detail.get([x * 48.0, y * 48.0]) * 0.025;
        // Bande de mer garantie le long des bords : la carte ne finit jamais sur de la terre.
        let to_edge = x.min(1.0 - x).min(y).min(1.0 - y);
        let border = smoothstep(0.04, 0.01, to_edge);
        b * 0.5 + dome + mm * r * 0.5 - edge * 1.6 + grain - border * 2.5
    })
}

/// Calcule `f(x, y)` sur toute la carte (coordonnées 0–1), une ligne par fil.
fn field(w: u32, h: u32, f: impl Fn(f64, f64) -> f64 + Sync) -> Vec<f64> {
    let mut out = vec![0.0; (w * h) as usize];
    let scale = w.max(h) as f64;
    out.par_chunks_mut(w as usize).enumerate().for_each(|(y, row)| {
        for (x, v) in row.iter_mut().enumerate() {
            *v = f(x as f64 / scale, y as f64 / scale);
        }
    });
    out
}

fn smoothstep(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Rang de chaque valeur (0 = la plus basse).
fn ranks(v: &[f64]) -> Vec<u32> {
    let mut order: Vec<u32> = (0..v.len() as u32).collect();
    order.par_sort_unstable_by(|&a, &b| v[a as usize].total_cmp(&v[b as usize]).then(a.cmp(&b)));
    let mut rank = vec![0u32; v.len()];
    for (r, &i) in order.iter().enumerate() {
        rank[i as usize] = r as u32;
    }
    rank
}

/// Hauteur relative (0–1, par rang) → altitude continue sur l'échelle 0–255.
/// La terre est surtout basse, les sommets rares.
fn altitude(h: f32, water: f32) -> f32 {
    if h < water {
        h / water * (SEA_LEVEL - 1) as f32
    } else {
        let q = ((h - water) / (1.0 - water)).clamp(0.0, 1.0);
        SEA_LEVEL as f32 + q.powf(3.0) * (255 - SEA_LEVEL) as f32
    }
}

fn quantize(h: f32, water: f32) -> u8 {
    altitude(h, water) as u8
}

/// Ombrage du relief (éclairé du nord-ouest), calculé sur l'altitude continue pour éviter
/// les marches de l'altitude entière. La mer est plate.
fn hillshade(w: u32, h: u32, height: &[f32], water: f32) -> Vec<u8> {
    let (wi, hi) = (w as i64, h as i64);
    let alt = |x: i64, y: i64| {
        let i = (y.clamp(0, hi - 1) * wi + x.clamp(0, wi - 1)) as usize;
        altitude(height[i], water).max(SEA_LEVEL as f32)
    };
    let mut out = vec![128u8; (w * h) as usize];
    out.par_chunks_mut(w as usize).enumerate().for_each(|(y, row)| {
        let y = y as i64;
        for (x, v) in row.iter_mut().enumerate() {
            let x = x as i64;
            let dx = alt(x + 1, y) - alt(x - 1, y);
            let dy = alt(x, y + 1) - alt(x, y - 1);
            let light = (-(dx + dy) * 0.05).clamp(-1.0, 1.0);
            *v = (128.0 + light * 100.0) as u8;
        }
    });
    out
}

fn neighbors(map: &Map, i: usize) -> impl Iterator<Item = usize> + '_ {
    let (w, h) = (map.width as i64, map.height as i64);
    let (x, y) = ((i as i64) % w, (i as i64) / w);
    [
        (-1, 0),
        (1, 0),
        (0, -1),
        (0, 1),
        (-1, -1),
        (1, -1),
        (-1, 1),
        (1, 1),
    ]
    .into_iter()
    .filter_map(move |(dx, dy)| {
        let (nx, ny) = (x + dx, y + dy);
        (nx >= 0 && ny >= 0 && nx < w && ny < h).then(|| (ny * w + nx) as usize)
    })
}

fn on_border(map: &Map, i: usize) -> bool {
    let (x, y) = (i as u32 % map.width, i as u32 / map.width);
    x == 0 || y == 0 || x == map.width - 1 || y == map.height - 1
}

fn ocean_mask(map: &Map) -> Vec<bool> {
    let mut ocean = vec![false; map.len()];
    let mut queue: VecDeque<usize> = VecDeque::new();
    for i in 0..map.len() {
        if on_border(map, i) && map.elevation[i] < SEA_LEVEL {
            ocean[i] = true;
            queue.push_back(i);
        }
    }
    while let Some(i) = queue.pop_front() {
        for j in neighbors(map, i) {
            if !ocean[j] && map.elevation[j] < SEA_LEVEL {
                ocean[j] = true;
                queue.push_back(j);
            }
        }
    }
    ocean
}

struct Drainage {
    /// Hauteur après remplissage des cuvettes (≥ hauteur d'origine).
    filled: Vec<f32>,
    /// Pluie cumulée venue de l'amont (en « cases de pluie »).
    accumulation: Vec<f32>,
}

/// Remplissage par priorité (Barnes et al., 2014) depuis la mer et les bords, puis cumul de
/// la pluie de l'amont vers l'aval.
fn drainage(map: &Map, height: &[f32], ocean: &[bool], rain: &[f64]) -> Drainage {
    let n = map.len();
    let eps = 0.1 / n as f32;
    let mut filled = height.to_vec();
    let mut downstream = vec![u32::MAX; n];
    let mut seen = vec![false; n];
    let mut order = Vec::with_capacity(n);
    // Clé de tas : bits du flottant positif (l'ordre est conservé) puis index.
    let key = |h: f32, i: usize| std::cmp::Reverse(((h.max(0.0).to_bits() as u64) << 32) | i as u64);
    let mut heap = BinaryHeap::new();
    for i in 0..n {
        if ocean[i] || on_border(map, i) {
            seen[i] = true;
            heap.push(key(filled[i], i));
        }
    }
    while let Some(std::cmp::Reverse(k)) = heap.pop() {
        let i = (k & 0xFFFF_FFFF) as usize;
        order.push(i as u32);
        for j in neighbors(map, i) {
            if !seen[j] {
                seen[j] = true;
                filled[j] = filled[j].max(filled[i] + eps);
                downstream[j] = i as u32;
                heap.push(key(filled[j], j));
            }
        }
    }
    let mut accumulation: Vec<f32> = (0..n)
        .map(|i| if ocean[i] { 0.0 } else { 0.5 + rain[i] as f32 })
        .collect();
    for &i in order.iter().rev() {
        let d = downstream[i as usize];
        if d != u32::MAX && !ocean[i as usize] {
            accumulation[d as usize] += accumulation[i as usize];
        }
    }
    Drainage { filled, accumulation }
}

/// Distance (en cases, 8 voisins) à la case la plus proche qui vérifie `source`, plafonnée.
fn distance(map: &Map, source: impl Fn(usize) -> bool, cap: u8) -> Vec<u8> {
    let mut dist = vec![cap; map.len()];
    let mut queue = VecDeque::new();
    for (i, d) in dist.iter_mut().enumerate() {
        if source(i) {
            *d = 0;
            queue.push_back(i);
        }
    }
    while let Some(i) = queue.pop_front() {
        let next = dist[i] + 1;
        if next >= cap {
            continue;
        }
        for j in neighbors(map, i) {
            if dist[j] > next {
                dist[j] = next;
                queue.push_back(j);
            }
        }
    }
    dist
}

fn near_water_land(map: &Map, i: usize, water: &[bool]) -> bool {
    neighbors(map, i).any(|j| !water[j])
}

fn place_villages(seed: u64, map: &Map, near_ocean: &[u8], wanted: usize) -> Vec<Site> {
    let (w, h) = (map.width as usize, map.height as usize);
    // Table des sommes de fertilité pour noter un voisinage en temps constant.
    let mut sat = vec![0u64; (w + 1) * (h + 1)];
    for y in 0..h {
        for x in 0..w {
            sat[(y + 1) * (w + 1) + x + 1] =
                map.fertility[y * w + x] as u64 + sat[y * (w + 1) + x + 1] + sat[(y + 1) * (w + 1) + x]
                    - sat[y * (w + 1) + x];
        }
    }
    let area = |x0: usize, y0: usize, x1: usize, y1: usize| {
        sat[y1 * (w + 1) + x1] + sat[y0 * (w + 1) + x0] - sat[y0 * (w + 1) + x1] - sat[y1 * (w + 1) + x0]
    };
    let fresh = distance(map, |i| map.biome_at(i) == Biome::Lake || map.river_at(i) > 0, 20);
    let mut rng = Rng::new(seed ^ 0x0056_494C_4C41_4745);
    let radius = (w.min(h) / 100).max(4);
    let step = (w.min(h) / 300).max(2);

    let mut candidates = Vec::new();
    for y in (radius..h - radius).step_by(step) {
        for x in (radius..w - radius).step_by(step) {
            let i = y * w + x;
            let b = map.biome_at(i);
            let ok = matches!(
                b,
                Biome::Meadow | Biome::Grassland | Biome::Forest | Biome::Heath | Biome::Hills
            );
            if !ok || map.river_at(i) > 0 {
                continue;
            }
            let fert = area(x - radius, y - radius, x + radius + 1, y + radius + 1) as f32
                / ((2 * radius + 1).pow(2) as f32 * 255.0);
            let water_bonus = match fresh[i] {
                1..=4 => 0.4,
                5..=8 => 0.15,
                _ => 0.0,
            } + if near_ocean[i] <= 3 { 0.12 } else { 0.0 };
            let height_penalty = map.elevation[i].saturating_sub(SEA_LEVEL + 40) as f32 / 200.0;
            let jitter = (rng.below(1000) as f32 / 1000.0) * 0.05;
            candidates.push((fert + water_bonus - height_penalty + jitter, x as u32, y as u32));
        }
    }
    candidates.sort_by(|a, b| b.0.total_cmp(&a.0));

    let mut chosen: Vec<(u32, u32)> = Vec::new();
    let mut min_dist = ((w * h) as f32 / wanted.max(1) as f32).sqrt() * 0.6;
    while chosen.len() < wanted && min_dist > radius as f32 * 2.0 {
        for &(_, x, y) in &candidates {
            if chosen.len() >= wanted {
                break;
            }
            let far = chosen.iter().all(|&(cx, cy)| {
                ((cx as f32 - x as f32).powi(2) + (cy as f32 - y as f32).powi(2)).sqrt() >= min_dist
            });
            if far {
                chosen.push((x, y));
            }
        }
        min_dist *= 0.8;
    }

    let mut namer = names::Namer::new(&mut rng);
    chosen
        .into_iter()
        .enumerate()
        .map(|(id, (x, y))| {
            let surroundings = names::Surroundings::look(map, x, y, near_ocean);
            Site {
                name: namer.next(&mut rng, &surroundings),
                x,
                y,
                color: names::COLORS[id % names::COLORS.len()],
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small() -> Params {
        Params {
            width: 256,
            height: 256,
            villages: 6,
            water: 0.3,
        }
    }

    #[test]
    fn same_seed_same_world() {
        assert_eq!(generate(1, &small()), generate(1, &small()));
    }

    #[test]
    fn different_seeds_differ() {
        assert_ne!(generate(1, &small()).map, generate(2, &small()).map);
    }

    #[test]
    fn worlds_are_plausible() {
        for seed in [1, 7, 42, 1234] {
            let world = generate(seed, &small());
            let m = &world.map;
            let water = (0..m.len()).filter(|&i| m.biome_at(i).is_water()).count() as f32 / m.len() as f32;
            assert!((0.2..0.55).contains(&water), "graine {seed} : {water:.2} d'eau");
            let rivers = (0..m.len()).filter(|&i| m.river_at(i) > 0).count();
            assert!(rivers > 50, "graine {seed} : {rivers} cases de rivière");
            assert!(
                world.villages.len() >= 4,
                "graine {seed} : {} villages",
                world.villages.len()
            );
            let mut names: Vec<_> = world.villages.iter().map(|v| v.name.clone()).collect();
            names.sort();
            names.dedup();
            assert_eq!(names.len(), world.villages.len(), "noms en double");
            for v in &world.villages {
                assert!(
                    !m.biome_at(m.idx(v.x, v.y)).is_water(),
                    "{} est dans l'eau",
                    v.name
                );
            }
            // Pas de terre contre le bord (4 cases sur 256 = 16 sur 1024).
            let margin = 4;
            let coast_on_edge = (0..m.len()).any(|i| {
                let (x, y) = (i as u32 % m.width, i as u32 / m.width);
                let near = x < margin || y < margin || x >= m.width - margin || y >= m.height - margin;
                near && !m.biome_at(i).is_water()
            });
            assert!(!coast_on_edge, "graine {seed} : de la terre touche le bord");
        }
    }

    /// Un an de vie : les villages tiennent, grandissent un peu, et l'histoire est reproductible.
    #[test]
    fn villages_live_a_year() {
        use hameaux_core::{clock::TICKS_PER_DAY, settlement::HouseState};
        let p = Params {
            width: 256,
            height: 256,
            villages: 4,
            water: 0.38,
        };
        let mut a = generate(5, &p);
        let start = a.alive().count();
        for _ in 0..28 * TICKS_PER_DAY {
            a.step();
        }
        let end = a.alive().count();
        assert!(end >= start, "{start} → {end} habitants");
        for v in &a.villages {
            assert!(a.population(v.id) > 0, "{} est vide", v.name);
            assert!(v.houses.iter().any(|h| h.state == HouseState::Inhabited));
        }
        assert!(a.chronicle.events.iter().any(|e| e.text.starts_with("Naissance")));
        // Même graine, même histoire.
        let mut b = generate(5, &p);
        for _ in 0..28 * TICKS_PER_DAY {
            b.step();
        }
        assert_eq!(a.people.len(), b.people.len());
        assert_eq!(a.villages, b.villages);
        assert_eq!(a.chronicle, b.chronicle);
    }
}

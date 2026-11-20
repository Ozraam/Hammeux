//! Fondation des villages, choix des parcelles, habitants, routes.

use crate::{
    map::Biome,
    names,
    people::{Brain, Job, Person, Status},
    settlement::{Field, FieldState, House, HouseState, Stock, Village},
    tuning::*,
    world::{Caches, Site, World, occ},
};

impl World {
    pub fn found_village(&mut self, site: &Site) {
        let Some((gx, gy)) = self.civic_spot(site.x, site.y) else {
            return;
        };
        let id = self.villages.len() as u32;
        let well = (gx + 3, gy + 1);
        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let i = self.map.idx(gx + dx, gy + dy);
            self.clear_forest(i);
            self.set_occ(i, occ::CIVIC);
        }
        let wi = self.map.idx(well.0, well.1);
        self.clear_forest(wi);
        self.set_occ(wi, occ::CIVIC);
        let has_stone = self.stone_nearby(gx, gy);
        self.villages.push(Village {
            id,
            name: site.name.clone(),
            x: gx,
            y: gy,
            well,
            color: site.color,
            founded: self.tick,
            houses: Vec::new(),
            fields: Vec::new(),
            stock: Stock {
                food: 0.0,
                wood: 40,
                stone: if has_stone { 20 } else { 0 },
            },
            hungry_days: 0,
            has_stone,
        });
        let vi = self.villages.len() - 1;
        let families = 4 + self.rng.below(2) as usize;
        for _ in 0..families {
            if let Some(h) = self.new_house(vi, HouseState::Inhabited) {
                self.settle_family(vi, h);
            }
        }
        self.new_house(vi, HouseState::Building);
        let pop = self.population(id);
        let fields = (pop as f32 * FIELDS_PER_PERSON).ceil() as usize + 1;
        for _ in 0..fields {
            self.new_field(vi);
        }
        self.villages[vi].stock.food = pop as f32 * 35.0;
        self.event(
            Some(id),
            vec![],
            format!("Fondation de {} ({pop} habitants)", site.name),
        );
    }

    /// Grenier 2 × 2, sa porte, et le puits à droite : tout doit être constructible et libre.
    fn civic_spot(&self, sx: u32, sy: u32) -> Option<(u32, u32)> {
        let m = &self.map;
        let ok = |x: i64, y: i64| {
            m.inside(x, y)
                && m.buildable(m.idx(x as u32, y as u32))
                && self.occ(m.idx(x as u32, y as u32)) == occ::NONE
        };
        for r in 0..16i64 {
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs().max(dy.abs()) != r {
                        continue;
                    }
                    let (x, y) = (sx as i64 + dx, sy as i64 + dy);
                    let cells = [(0, 0), (1, 0), (0, 1), (1, 1), (0, 2), (3, 1), (2, 1), (2, 2)];
                    if cells.iter().all(|&(cx, cy)| ok(x + cx, y + cy)) {
                        return Some((x as u32, y as u32));
                    }
                }
            }
        }
        None
    }

    fn stone_nearby(&self, cx: u32, cy: u32) -> bool {
        let m = &self.map;
        let r = RESOURCE_RADIUS as i64;
        for dy in (-r..=r).step_by(2) {
            for dx in (-r..=r).step_by(2) {
                let (x, y) = (cx as i64 + dx, cy as i64 + dy);
                if !m.inside(x, y) {
                    continue;
                }
                if is_quarry(m, m.idx(x as u32, y as u32)) {
                    return true;
                }
            }
        }
        false
    }

    pub(crate) fn clear_forest(&mut self, i: usize) {
        if matches!(self.map.biome_at(i), Biome::Forest | Biome::Conifers) {
            self.set_biome(i, Biome::Meadow);
        }
    }

    /// Meilleure case libre pour une maison : près du grenier, avec de la place autour.
    fn house_plot(&mut self, vi: usize) -> Option<(u32, u32)> {
        let (cx, cy) = (self.villages[vi].x as i64, self.villages[vi].y as i64 + 1);
        let r = VILLAGE_RADIUS as i64;
        let mut best: Option<(f32, u32, u32)> = None;
        for dy in -r..=r {
            for dx in -r..=r {
                let (x, y) = (cx + dx, cy + dy);
                let d = ((dx * dx + dy * dy) as f32).sqrt();
                if d < 2.5 || d > r as f32 || !self.map.inside(x, y) {
                    continue;
                }
                let i = self.map.idx(x as u32, y as u32);
                if !self.map.buildable(i) || self.occ(i) != occ::NONE || self.map.road_at(i) {
                    continue;
                }
                // Au moins deux voisins libres et praticables : on peut tourner autour.
                let free = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                    .iter()
                    .filter(|&&(ox, oy)| {
                        let (nx, ny) = (x + ox, y + oy);
                        self.map.inside(nx, ny) && {
                            let j = self.map.idx(nx as u32, ny as u32);
                            self.occ(j) == occ::NONE && self.map.walk_cost(j).is_some()
                        }
                    })
                    .count();
                if free < 2 {
                    continue;
                }
                let score = d + self.rng.f32() * 4.0;
                if best.is_none_or(|b| score < b.0) {
                    best = Some((score, x as u32, y as u32));
                }
            }
        }
        best.map(|(_, x, y)| (x, y))
    }

    /// Meilleur emplacement 3 × 3 pour un champ : fertile et pas trop loin.
    fn field_plot(&mut self, vi: usize) -> Option<(u32, u32, f32)> {
        let (cx, cy) = (self.villages[vi].x as i64, self.villages[vi].y as i64 + 1);
        let r = VILLAGE_RADIUS as i64;
        let s = FIELD_SIZE as i64;
        let mut best: Option<(f32, u32, u32, f32)> = None;
        for dy in -r..=r {
            for dx in -r..=r {
                let (x, y) = (cx + dx, cy + dy);
                let d = (((dx + 1) * (dx + 1) + (dy + 1) * (dy + 1)) as f32).sqrt();
                if d < 4.0 || d > r as f32 {
                    continue;
                }
                let mut fert = 0.0;
                let mut ok = true;
                'scan: for fy in -1..=s {
                    for fx in -1..=s {
                        let (tx, ty) = (x + fx, y + fy);
                        if !self.map.inside(tx, ty) {
                            ok = false;
                            break 'scan;
                        }
                        let i = self.map.idx(tx as u32, ty as u32);
                        let inside = fx >= 0 && fy >= 0 && fx < s && fy < s;
                        let o = self.occ(i);
                        if inside {
                            if !self.map.buildable(i) || o != occ::NONE || self.map.road_at(i) {
                                ok = false;
                                break 'scan;
                            }
                            fert += self.map.fertility[i] as f32 / 255.0;
                        } else if o == occ::HOUSE || o == occ::CIVIC {
                            ok = false;
                            break 'scan;
                        }
                    }
                }
                if !ok {
                    continue;
                }
                let fert = fert / (s * s) as f32;
                let score = d / r as f32 - fert * 1.5 + self.rng.f32() * 0.3;
                if best.is_none_or(|b| score < b.0) {
                    best = Some((score, x as u32, y as u32, fert));
                }
            }
        }
        best.map(|(_, x, y, f)| (x, y, f))
    }

    pub(crate) fn new_house(&mut self, vi: usize, state: HouseState) -> Option<u32> {
        if self.villages[vi].houses.len() >= MAX_HOUSES {
            return None;
        }
        let (x, y) = self.house_plot(vi)?;
        let id = self.next_building;
        self.next_building += 1;
        let i = self.map.idx(x, y);
        self.clear_forest(i);
        self.set_occ(i, occ::HOUSE);
        let has_stone = self.villages[vi].has_stone;
        let (wood, stone) = if has_stone {
            (HOUSE_WOOD, HOUSE_STONE)
        } else {
            (HOUSE_WOOD + 2 * HOUSE_STONE, 0)
        };
        let built = state != HouseState::Building;
        let roof = self.rng.below(6) as u8;
        self.villages[vi].houses.push(House {
            id,
            x,
            y,
            state,
            wood_needed: if built { 0 } else { wood },
            stone_needed: if built { 0 } else { stone },
            wood_in: if built { wood } else { 0 },
            stone_in: if built { stone } else { 0 },
            residents: Vec::new(),
            family: String::new(),
            reserved_for: None,
            since: self.tick,
            roof,
        });
        let door = self.villages[vi].door();
        self.carve_road(self.map.idx(x, y) as u32, self.map.idx(door.0, door.1) as u32);
        Some(id)
    }

    pub(crate) fn new_field(&mut self, vi: usize) -> Option<u32> {
        let (x, y, fertility) = self.field_plot(vi)?;
        let id = self.next_building;
        self.next_building += 1;
        for dy in 0..FIELD_SIZE {
            for dx in 0..FIELD_SIZE {
                let i = self.map.idx(x + dx, y + dy);
                self.clear_forest(i);
                self.set_occ(i, occ::FIELD);
            }
        }
        self.villages[vi].fields.push(Field {
            id,
            x,
            y,
            state: FieldState::Fallow,
            work: 0.0,
            growth: 0.0,
            fertility,
        });
        Some(id)
    }

    /// Trace un chemin (cases marquées « route ») entre deux cases, en évitant les bâtiments et les champs.
    pub(crate) fn carve_road(&mut self, from: u32, to: u32) {
        let path = {
            let Caches { occ: grid, pf, .. } = &mut self.caches.0;
            pf.find(&self.map, from, to, |j| grid[j] != occ::NONE, PATH_BUDGET)
        };
        let Some(path) = path else { return };
        for i in std::iter::once(from).chain(path) {
            let i = i as usize;
            if self.occ(i) == occ::NONE {
                self.clear_forest(i);
                self.set_road(i);
            }
        }
    }

    /// Une famille fondatrice : un couple, de 0 à 3 enfants.
    fn settle_family(&mut self, vi: usize, house: u32) {
        let last = self.rng.pick(names::LAST).unwrap().to_string();
        let mother_age = 24.0 + self.rng.f32() * 26.0;
        let a = self.new_person(vi, &last, true, mother_age, Some(house), &[]);
        let father_last = self.rng.pick(names::LAST).unwrap().to_string();
        let father_age = mother_age + self.rng.f32() * 8.0 - 3.0;
        let b = self.new_person(vi, &father_last, false, father_age, Some(house), &[]);
        self.people[b as usize].last = last.clone();
        self.couple(a, b);
        let kids = self.rng.below(4);
        for k in 0..kids {
            let age = if k == 0 && kids == 3 {
                ADULT_AGE + self.rng.f32() * 3.0
            } else {
                self.rng.f32() * (ADULT_AGE - 1.0)
            };
            let age = age.min(mother_age - 18.0).max(0.0);
            let female = self.rng.chance(0.5);
            self.new_person(vi, &last, female, age, Some(house), &[a, b]);
        }
        if let Some(h) = self.villages[vi].house_mut(house) {
            h.family = last;
        }
    }

    /// Crée un habitant d'un âge donné (en années), chez lui s'il a une maison.
    pub(crate) fn new_person(
        &mut self,
        vi: usize,
        last: &str,
        female: bool,
        age: f32,
        home: Option<u32>,
        parents: &[u32],
    ) -> u32 {
        let village = self.villages[vi].id;
        let used: Vec<&str> = self
            .people
            .iter()
            .filter(|p| p.alive() && p.village == village)
            .map(|p| p.first.as_str())
            .collect();
        let pool: Vec<&str> = (if female { names::FEMALE } else { names::MALE })
            .iter()
            .copied()
            .filter(|n| !used.contains(n))
            .collect();
        let first = match self.rng.pick(&pool) {
            Some(n) => n.to_string(),
            None => self
                .rng
                .pick(if female { names::FEMALE } else { names::MALE })
                .unwrap()
                .to_string(),
        };
        let id = self.people.len() as u32;
        let (x, y) = match home.and_then(|h| self.villages[vi].house(h)) {
            Some(h) => (h.x as f32 + 0.5, h.y as f32 + 0.5),
            None => (self.villages[vi].x as f32 + 0.5, self.villages[vi].y as f32 + 2.5),
        };
        let looks = [
            self.rng.below(8) as u8,
            self.rng.below(4) as u8,
            self.rng.below(5) as u8,
        ];
        let death_age = DEATH_AGE_MIN + self.rng.f32() * (DEATH_AGE_MAX - DEATH_AGE_MIN);
        let born = self.tick as i64 - (age * YEAR as f32) as i64;
        self.people.push(Person {
            id,
            first,
            last: last.to_string(),
            female,
            born,
            death_age,
            status: Status::Alive,
            village,
            home,
            job: None,
            partner: None,
            parents: parents.to_vec(),
            children: Vec::new(),
            friends: Default::default(),
            master: None,
            apprentice_until: 0,
            x,
            y,
            looks,
            brain: Brain::default(),
        });
        for &p in parents {
            self.people[p as usize].children.push(id);
        }
        if let Some(h) = home.and_then(|h| self.villages[vi].house_mut(h)) {
            h.residents.push(id);
        }
        if age >= ADULT_AGE {
            let job = self.pick_job(id);
            self.people[id as usize].job = Some(job);
        }
        id
    }

    pub(crate) fn couple(&mut self, a: u32, b: u32) {
        self.people[a as usize].partner = Some(b);
        self.people[b as usize].partner = Some(a);
    }

    /// Métier : souvent celui d'un parent, sinon celui qui manque le plus au village.
    pub(crate) fn pick_job(&mut self, id: u32) -> Job {
        let p = &self.people[id as usize];
        let parent_jobs: Vec<Job> = p
            .parents
            .iter()
            .filter_map(|&q| self.people[q as usize].job)
            .collect();
        if !parent_jobs.is_empty() && self.rng.chance(0.6) {
            return *self.rng.pick(&parent_jobs).unwrap();
        }
        let village = p.village;
        let has_stone = self
            .village_index(village)
            .is_some_and(|vi| self.villages[vi].has_stone);
        let adults: Vec<Job> = self
            .alive()
            .filter(|q| q.village == village && q.id != id)
            .filter_map(|q| q.job)
            .collect();
        let n = adults.len() as f32 + 1.0;
        let want = |j: Job| -> f32 {
            match j {
                Job::Builder => 1.0 + (n / 15.0).floor(),
                Job::Farmer => (n * 0.4).ceil(),
                Job::Woodcutter => (n * 0.25).ceil(),
                Job::Quarrier => {
                    if has_stone {
                        (n * 0.15).ceil()
                    } else {
                        0.0
                    }
                }
            }
        };
        Job::ALL
            .into_iter()
            .max_by(|&a, &b| {
                let ga = want(a) - adults.iter().filter(|&&j| j == a).count() as f32;
                let gb = want(b) - adults.iter().filter(|&&j| j == b).count() as f32;
                ga.total_cmp(&gb)
            })
            .unwrap()
    }
}

/// Case où l'on peut tailler de la pierre.
pub(crate) fn is_quarry(m: &crate::map::Map, i: usize) -> bool {
    m.resource[i] == crate::map::Resource::Stone as u8
        || matches!(m.biome_at(i), Biome::Hills | Biome::Mountain)
}

//! Ce que fait chaque habitant : choisir une tâche, la découper en étapes, avancer.

use crate::{
    clock::Season,
    map::Biome,
    people::{Act, Carry, Job, Status, Step},
    settlement::{FieldState, HouseState},
    tuning::*,
    world::{Caches, World, occ},
};

use super::{DayPart, day_part, is_quarry, rest_day};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Resource {
    Wood,
    Stone,
}

impl World {
    pub(crate) fn update_person(&mut self, i: usize) {
        if self.people[i].brain.hidden {
            return;
        }
        if self.people[i].brain.path_pos < self.people[i].brain.path.len() {
            self.walk(i);
            return;
        }
        {
            let b = &mut self.people[i].brain;
            b.path.clear();
            b.path_pos = 0;
            if b.wait > 0 {
                b.wait -= 1;
                if b.wait == 0 {
                    b.act = Act::Idle;
                    b.with = None;
                }
                return;
            }
        }
        if self.people[i].brain.plan.is_empty() {
            self.assign(i);
        }
        if let Some(step) = self.people[i].brain.plan.pop_front() {
            self.exec(i, step);
        }
    }

    fn walk(&mut self, i: usize) {
        let tick = self.tick;
        let width = self.map.width;
        let p = &mut self.people[i];
        let mut budget = WALK
            * if p.child(tick) {
                0.85
            } else if p.elder(tick) {
                0.65
            } else {
                1.0
            };
        while budget > 0.0 && p.brain.path_pos < p.brain.path.len() {
            let c = p.brain.path[p.brain.path_pos];
            let (tx, ty) = ((c % width) as f32 + 0.5, (c / width) as f32 + 0.5);
            let (dx, dy) = (tx - p.x, ty - p.y);
            let d = (dx * dx + dy * dy).sqrt();
            if d <= budget {
                p.x = tx;
                p.y = ty;
                p.brain.path_pos += 1;
                budget -= d;
            } else {
                p.x += dx / d * budget;
                p.y += dy / d * budget;
                budget = 0.0;
            }
        }
    }

    fn exec(&mut self, i: usize, step: Step) {
        let Some(vi) = self.village_index(self.people[i].village) else {
            return;
        };
        match step {
            Step::Go(goal) => {
                let start = self.people[i].tile(self.map.width);
                let path = {
                    let Caches { occ: grid, pf, .. } = &mut self.caches.0;
                    // Les bâtiments bloquent le passage (sauf celui où l'on va).
                    pf.find(
                        &self.map,
                        start,
                        goal,
                        |j| grid[j] == occ::HOUSE || grid[j] == occ::CIVIC,
                        PATH_BUDGET,
                    )
                };
                let night = day_part(self.clock().hour) == DayPart::Night;
                let b = &mut self.people[i].brain;
                b.path = path.unwrap_or_else(|| vec![goal]);
                b.path_pos = 0;
                b.act = if b.leaving {
                    Act::Leaving
                } else if b.carry.is_some() {
                    Act::Hauling
                } else if night {
                    Act::GoingHome
                } else if b.act == Act::Arriving {
                    Act::Arriving
                } else {
                    Act::Walking
                };
            }
            Step::Wait { ticks, act, with } => {
                let b = &mut self.people[i].brain;
                b.wait = ticks.max(1);
                b.act = act;
                b.with = with;
            }
            Step::Take(carry) => {
                let st = &mut self.villages[vi].stock;
                let got = match carry {
                    Carry::Wood if st.wood > 0 => {
                        let n = st.wood.min(2);
                        st.wood -= n;
                        n
                    }
                    Carry::Stone if st.stone > 0 => {
                        let n = st.stone.min(2);
                        st.stone -= n;
                        n
                    }
                    _ => 0,
                };
                if got > 0 {
                    let b = &mut self.people[i].brain;
                    b.carry = Some(carry);
                    b.carry_amount = got as f32;
                }
            }
            Step::Deposit => {
                let b = &mut self.people[i].brain;
                let (carry, amount) = (b.carry.take(), b.carry_amount);
                b.carry_amount = 0.0;
                let st = &mut self.villages[vi].stock;
                match carry {
                    Some(Carry::Wood) => st.wood = (st.wood + amount as u32).min(MAX_WOOD),
                    Some(Carry::Stone) => st.stone = (st.stone + amount as u32).min(MAX_STONE),
                    Some(Carry::Food) => st.food = (st.food + amount).min(MAX_FOOD),
                    _ => {}
                }
            }
            Step::Build(hid) => {
                let b = &mut self.people[i].brain;
                let (carry, amount) = (b.carry.take(), b.carry_amount as u32);
                b.carry_amount = 0.0;
                let Some(h) = self.villages[vi].house_mut(hid) else {
                    return;
                };
                if h.state != HouseState::Building {
                    return;
                }
                match carry {
                    Some(Carry::Wood) => {
                        let n = amount.min(h.wood_needed);
                        h.wood_needed -= n;
                        h.wood_in += n;
                    }
                    Some(Carry::Stone) => {
                        let n = amount.min(h.stone_needed);
                        h.stone_needed -= n;
                        h.stone_in += n;
                    }
                    _ => return,
                }
                if h.wood_needed == 0 && h.stone_needed == 0 {
                    self.complete_house(vi, hid);
                }
            }
            Step::FieldWork(fid) => self.field_work(i, vi, fid),
            Step::Chop(tile) => {
                let b = &mut self.people[i].brain;
                b.carry = Some(Carry::Wood);
                b.carry_amount = LOAD_WOOD as f32;
                let t = tile as usize;
                if matches!(self.map.biome_at(t), Biome::Forest | Biome::Conifers) && self.rng.chance(0.12) {
                    self.set_biome(t, Biome::Meadow);
                }
            }
            Step::Quarry(_) => {
                let b = &mut self.people[i].brain;
                b.carry = Some(Carry::Stone);
                b.carry_amount = LOAD_STONE as f32;
            }
            Step::Hide => {
                if let Some((x, y)) = self.home_tile(i) {
                    let p = &mut self.people[i];
                    p.x = x as f32 + 0.5;
                    p.y = y as f32 + 0.5;
                    p.brain.hidden = true;
                    p.brain.carry = None;
                    p.brain.act = Act::Sleeping;
                }
            }
            Step::Leave => {
                let tick = self.tick;
                let p = &mut self.people[i];
                p.status = Status::Left { tick };
                p.brain = Default::default();
                let (id, home) = (p.id, p.home.take());
                if let Some(h) = home.and_then(|h| self.villages[vi].house_mut(h)) {
                    h.residents.retain(|&r| r != id);
                }
            }
        }
    }

    fn field_work(&mut self, i: usize, vi: usize, fid: u32) {
        let season = self.clock().season;
        let Some(f) = self.villages[vi].field_mut(fid) else {
            return;
        };
        match f.state {
            FieldState::Fallow if season != Season::Winter => {
                f.work += FIELD_WORK_PER_SESSION;
                if f.work >= 1.0 {
                    f.state = FieldState::Ploughed;
                    f.work = 0.0;
                }
            }
            FieldState::Ploughed if season != Season::Winter => {
                f.work += FIELD_WORK_PER_SESSION;
                if f.work >= 1.0 {
                    f.state = FieldState::Sown;
                    f.work = 0.0;
                    f.growth = 0.0;
                }
            }
            FieldState::Ripe => {
                let part = FIELD_WORK_PER_SESSION.min(1.0 - f.work);
                let amount = FIELD_YIELD * (0.35 + f.fertility) * part;
                f.work += part;
                if f.work >= 0.999 {
                    f.state = FieldState::Fallow;
                    f.work = 0.0;
                    f.growth = 0.0;
                }
                let b = &mut self.people[i].brain;
                b.carry = Some(Carry::Food);
                b.carry_amount = amount;
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------------ choix d'une tâche

    fn plan(&mut self, i: usize, steps: impl IntoIterator<Item = Step>) {
        self.people[i].brain.plan.extend(steps);
    }

    fn wait(ticks: u64, act: Act) -> Step {
        Step::Wait {
            ticks,
            act,
            with: None,
        }
    }

    fn assign(&mut self, i: usize) {
        let tick = self.tick;
        if self.people[i].brain.leaving {
            self.plan(i, [Step::Leave]);
            return;
        }
        let part = day_part(self.clock().hour);
        let home = self.home_tile(i).map(|(x, y)| self.map.idx(x, y) as u32);
        match part {
            DayPart::Night => match home {
                Some(h) => self.plan(i, [Step::Go(h), Step::Hide]),
                None => self.plan(i, [Self::wait(30 * MINUTE, Act::Idle)]),
            },
            DayPart::Morning => self.near_home(i, 20 * MINUTE, Act::Idle),
            DayPart::Evening => self.leisure(i, true),
            DayPart::Work => {
                let p = &self.people[i];
                if p.child(tick) {
                    self.play(i);
                } else if p.elder(tick) || rest_day(tick) {
                    self.leisure(i, false);
                } else {
                    self.work(i);
                }
            }
        }
    }

    /// Une case libre et praticable près de chez soi (ou du grenier).
    fn spot_near(&mut self, cx: u32, cy: u32, r: i64) -> u32 {
        for _ in 0..12 {
            let x = cx as i64 + self.rng.range(-r, r);
            let y = cy as i64 + self.rng.range(-r, r);
            if !self.map.inside(x, y) {
                continue;
            }
            let i = self.map.idx(x as u32, y as u32);
            if self.map.walk_cost(i).is_some() && matches!(self.occ(i), occ::NONE | occ::FIELD) {
                return i as u32;
            }
        }
        self.map.idx(cx, cy) as u32
    }

    fn village_of(&self, i: usize) -> usize {
        self.village_index(self.people[i].village).unwrap_or(0)
    }

    fn near_home(&mut self, i: usize, ticks: u64, act: Act) {
        let (x, y) = self
            .home_tile(i)
            .unwrap_or_else(|| self.villages[self.village_of(i)].door());
        let spot = self.spot_near(x, y, 2);
        self.plan(i, [Step::Go(spot), Self::wait(ticks, act)]);
    }

    fn play(&mut self, i: usize) {
        let tick = self.tick;
        // Les plus grands suivent parfois un parent au travail.
        let p = &self.people[i];
        let parent = p.parents.iter().copied().find(|&q| {
            let q = &self.people[q as usize];
            q.alive() && !q.brain.hidden && (q.x - p.x).abs() + (q.y - p.y).abs() < 14.0
        });
        if p.age(tick) > 6.0
            && let Some(q) = parent
            && self.rng.chance(0.3)
        {
            let t = self.people[q as usize].tile(self.map.width);
            self.plan(
                i,
                [
                    Step::Go(t),
                    Step::Wait {
                        ticks: 30 * MINUTE,
                        act: Act::Playing,
                        with: Some(q),
                    },
                ],
            );
            return;
        }
        self.near_home(i, 30 * MINUTE, Act::Playing);
    }

    /// Temps libre : discuter au puits avec un ami, se promener, ou se reposer devant chez soi.
    fn leisure(&mut self, i: usize, evening: bool) {
        if self.rng.chance(if evening { 0.35 } else { 0.25 }) && self.chat(i) {
            return;
        }
        if !evening && self.rng.chance(0.5) {
            let v = &self.villages[self.village_of(i)];
            let (x, y) = (v.x, v.y);
            let spot = self.spot_near(x, y, 10);
            self.plan(i, [Step::Go(spot), Self::wait(40 * MINUTE, Act::Strolling)]);
            return;
        }
        self.near_home(i, 30 * MINUTE, Act::Idle);
    }

    /// Va discuter au puits avec un ami (qui vient aussi s'il est disponible).
    fn chat(&mut self, i: usize) -> bool {
        let tick = self.tick;
        let me = self.people[i].id;
        let friends: Vec<u32> = self.people[i]
            .friends
            .iter()
            .filter(|&(_, &s)| s >= 15.0)
            .map(|(&id, _)| id)
            .filter(|&f| {
                let q = &self.people[f as usize];
                q.alive()
                    && !q.brain.hidden
                    && !q.brain.leaving
                    && !q.child(tick)
                    && q.village == self.people[i].village
            })
            .collect();
        let Some(&f) = self.rng.pick(&friends) else {
            return false;
        };
        let (wx, wy) = self.villages[self.village_of(i)].well;
        let a = self.spot_near(wx, wy, 1);
        let b = self.spot_near(wx, wy, 1);
        self.plan(
            i,
            [
                Step::Go(a),
                Step::Wait {
                    ticks: CHAT,
                    act: Act::Chatting,
                    with: Some(f),
                },
            ],
        );
        let fi = f as usize;
        let fb = &mut self.people[fi].brain;
        if fb.carry.is_none() && fb.wait == 0 {
            fb.plan.clear();
            fb.path.clear();
            fb.path_pos = 0;
            fb.plan.extend([
                Step::Go(b),
                Step::Wait {
                    ticks: CHAT,
                    act: Act::Chatting,
                    with: Some(me),
                },
            ]);
        }
        true
    }

    fn work(&mut self, i: usize) {
        let tick = self.tick;
        let p = &self.people[i];
        // Apprenti : suit son maître et l'imite.
        if tick < p.apprentice_until
            && let Some(m) = p.master
        {
            let q = &self.people[m as usize];
            if q.alive() && !q.brain.hidden && !q.brain.leaving {
                let (t, act) = (q.tile(self.map.width), q.brain.act);
                let act = if act == Act::Idle || act == Act::Walking {
                    Act::Learning
                } else {
                    act
                };
                self.plan(
                    i,
                    [
                        Step::Go(t),
                        Step::Wait {
                            ticks: HOUR,
                            act,
                            with: Some(m),
                        },
                    ],
                );
                return;
            }
        }
        if self.rng.chance(0.06) && self.chat(i) {
            return;
        }
        let vi = self.village_of(i);
        let st = self.villages[vi].stock.clone();
        let has_stone = self.villages[vi].has_stone;
        match self.people[i].job {
            Some(Job::Builder) => {
                if !self.build(i, vi) {
                    let site_needs_stone = self.villages[vi]
                        .houses
                        .iter()
                        .any(|h| h.state == HouseState::Building && h.stone_needed > 0);
                    let r = if site_needs_stone && has_stone && st.stone == 0 {
                        Resource::Stone
                    } else {
                        Resource::Wood
                    };
                    self.gather(i, vi, r);
                }
            }
            Some(Job::Woodcutter) if st.wood < MAX_WOOD => self.gather(i, vi, Resource::Wood),
            Some(Job::Quarrier) if has_stone && st.stone < MAX_STONE => self.gather(i, vi, Resource::Stone),
            Some(Job::Farmer) | Some(Job::Woodcutter) | Some(Job::Quarrier) => {
                if !self.farm(i, vi) {
                    if st.wood < MAX_WOOD {
                        self.gather(i, vi, Resource::Wood)
                    } else {
                        self.leisure(i, false)
                    }
                }
            }
            None => self.gather(i, vi, Resource::Wood),
        }
    }

    /// Une ressource proche du village (bois ou pierre) et la case d'où la travailler.
    fn find_resource(&mut self, vi: usize, r: Resource) -> Option<(u32, u32)> {
        // D'abord près du village, puis plus loin si rien n'est trouvé.
        self.find_resource_within(vi, r, RESOURCE_RADIUS as i64)
            .or_else(|| self.find_resource_within(vi, r, RESOURCE_RADIUS as i64 * 2))
            .or_else(|| self.find_resource_within(vi, r, RESOURCE_RADIUS as i64 * 3))
    }

    fn find_resource_within(&mut self, vi: usize, r: Resource, rad: i64) -> Option<(u32, u32)> {
        let (cx, cy) = (self.villages[vi].x as i64, self.villages[vi].y as i64);
        let mut best: Option<(f32, u32, u32)> = None;
        for _ in 0..90 {
            let (x, y) = (cx + self.rng.range(-rad, rad), cy + self.rng.range(-rad, rad));
            if !self.map.inside(x, y) {
                continue;
            }
            let t = self.map.idx(x as u32, y as u32);
            let ok = match r {
                Resource::Wood => matches!(self.map.biome_at(t), Biome::Forest | Biome::Conifers),
                Resource::Stone => is_quarry(&self.map, t),
            };
            if !ok {
                continue;
            }
            let stand = [(0, 1), (1, 0), (-1, 0), (0, -1), (0, 0)]
                .into_iter()
                .find_map(|(dx, dy)| {
                    let (sx, sy) = (x + dx, y + dy);
                    if !self.map.inside(sx, sy) {
                        return None;
                    }
                    let s = self.map.idx(sx as u32, sy as u32);
                    (self.map.walk_cost(s).is_some() && matches!(self.occ(s), occ::NONE | occ::FIELD))
                        .then_some(s)
                });
            let Some(stand) = stand else { continue };
            let d = (((x - cx).pow(2) + (y - cy).pow(2)) as f32).sqrt() + self.rng.f32() * 6.0;
            if best.is_none_or(|b| d < b.0) {
                best = Some((d, t as u32, stand as u32));
            }
        }
        best.map(|(_, t, s)| (t, s))
    }

    fn gather(&mut self, i: usize, vi: usize, r: Resource) {
        let Some((res, stand)) = self.find_resource(vi, r) else {
            self.plan(i, [Self::wait(20 * MINUTE, Act::Idle)]);
            return;
        };
        let (act, take) = match r {
            Resource::Wood => (Act::Chopping, Step::Chop(res)),
            Resource::Stone => (Act::Quarrying, Step::Quarry(res)),
        };
        let door = self.villages[vi].door();
        let door = self.map.idx(door.0, door.1) as u32;
        let session = WORK_SESSION - self.rng.below(HOUR / 2);
        self.plan(
            i,
            [
                Step::Go(stand),
                Self::wait(session, act),
                take,
                Step::Go(door),
                Step::Deposit,
            ],
        );
    }

    fn build(&mut self, i: usize, vi: usize) -> bool {
        let v = &self.villages[vi];
        let site = v
            .houses
            .iter()
            .filter(|h| h.state == HouseState::Building)
            .find(|h| (h.stone_needed > 0 && v.stock.stone > 0) || (h.wood_needed > 0 && v.stock.wood > 0));
        let Some(h) = site else { return false };
        let mat = if h.stone_needed > 0 && v.stock.stone > 0 {
            Carry::Stone
        } else {
            Carry::Wood
        };
        let (hid, tile) = (h.id, self.map.idx(h.x, h.y) as u32);
        let door = v.door();
        let door = self.map.idx(door.0, door.1) as u32;
        self.plan(
            i,
            [
                Step::Go(door),
                Step::Take(mat),
                Step::Go(tile),
                Step::Wait {
                    ticks: BUILD_SESSION,
                    act: Act::Building,
                    with: Some(hid),
                },
                Step::Build(hid),
            ],
        );
        true
    }

    /// Travail aux champs selon la saison. Faux s'il n'y a rien à faire.
    fn farm(&mut self, i: usize, vi: usize) -> bool {
        let season = self.clock().season;
        let fields: Vec<(u32, FieldState, u32, u32)> = self.villages[vi]
            .fields
            .iter()
            .map(|f| (f.id, f.state, f.x, f.y))
            .collect();
        let pick = |want: &[FieldState]| -> Vec<(u32, FieldState, u32, u32)> {
            fields.iter().copied().filter(|f| want.contains(&f.1)).collect()
        };
        let todo = match season {
            Season::Winter => Vec::new(),
            Season::Spring => {
                let ripe = pick(&[FieldState::Ripe]);
                if ripe.is_empty() {
                    pick(&[FieldState::Fallow, FieldState::Ploughed])
                } else {
                    ripe
                }
            }
            Season::Summer => {
                let ripe = pick(&[FieldState::Ripe]);
                if !ripe.is_empty() {
                    ripe
                } else if self.clock().day <= 3 {
                    pick(&[FieldState::Fallow, FieldState::Ploughed])
                } else if self.rng.chance(0.3) {
                    pick(&[FieldState::Sown])
                } else {
                    Vec::new()
                }
            }
            Season::Autumn => pick(&[FieldState::Ripe]),
        };
        let Some(&(fid, state, fx, fy)) = self.rng.pick(&todo) else {
            return false;
        };
        let act = match state {
            FieldState::Fallow => Act::Ploughing,
            FieldState::Ploughed => Act::Sowing,
            FieldState::Sown => Act::Tending,
            FieldState::Ripe => Act::Harvesting,
        };
        let tile = self.map.idx(
            fx + self.rng.below(FIELD_SIZE as u64) as u32,
            fy + self.rng.below(FIELD_SIZE as u64) as u32,
        );
        self.plan(
            i,
            [
                Step::Go(tile as u32),
                Step::Wait {
                    ticks: WORK_SESSION,
                    act,
                    with: Some(fid),
                },
                Step::FieldWork(fid),
            ],
        );
        if state == FieldState::Ripe {
            let door = self.villages[vi].door();
            let door = self.map.idx(door.0, door.1) as u32;
            self.plan(i, [Step::Go(door), Step::Deposit]);
        }
        true
    }
}

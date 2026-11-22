//! Ce que fait chaque habitant : choisir une tâche, la découper en étapes, avancer.

use crate::{
    clock::Season,
    economy::{Good, Workshop},
    map::{Biome, Resource as MapResource},
    people::{Act, Carry, Intent, Job, Status, Step},
    settlement::{FieldState, HouseState},
    tuning::*,
    world::{Caches, World, occ},
};

use super::{DayPart, day_part, market::resource_at, rest_day};

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
        if !self.plan_still_valid(i) {
            self.abandon(i);
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
        // Les charrettes vont plus vite sur les routes.
        let road = p.brain.traveling && self.map.road_at(p.tile(width) as usize);
        let mut budget = WALK
            * if road { 1.4 } else { 1.0 }
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
            Step::Take(good) => {
                let st = &mut self.villages[vi].stock;
                let pool = match good {
                    Good::Wood => &mut st.wood,
                    _ => &mut st.stone,
                };
                let got = (*pool).min(2);
                *pool -= got;
                if got > 0 {
                    let b = &mut self.people[i].brain;
                    b.carry = Some(Carry::Good(good));
                    b.carry_amount = got as f32;
                }
            }
            Step::Deposit => self.deposit(i, vi),
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
                    Some(Carry::Good(Good::Wood)) => {
                        let n = amount.min(h.wood_needed);
                        h.wood_needed -= n;
                        h.wood_in += n;
                    }
                    Some(Carry::Good(Good::Stone)) => {
                        let n = amount.min(h.stone_needed);
                        h.stone_needed -= n;
                        h.stone_in += n;
                    }
                    _ => return,
                }
                let done = h.wood_needed == 0 && h.stone_needed == 0;
                self.pay_builder(i, vi);
                if done {
                    self.complete_house(vi, hid);
                }
            }
            Step::FieldWork(fid) => self.field_work(i, vi, fid),
            Step::Gather(tile, good) => {
                let base = match good {
                    Good::Wood => LOAD_WOOD as f32,
                    Good::Stone => LOAD_STONE as f32,
                    Good::Fish => {
                        if self.map.resource[tile as usize] == MapResource::Fish as u8 {
                            1.4
                        } else {
                            0.7
                        }
                    }
                    Good::Meat => {
                        if self.map.resource[tile as usize] == MapResource::Game as u8 {
                            1.1
                        } else {
                            0.5
                        }
                    }
                    Good::Ore => 1.5,
                    _ => 1.0,
                };
                let amount = base * self.work_bonus(i);
                let b = &mut self.people[i].brain;
                b.carry = Some(Carry::Good(good));
                b.carry_amount = amount;
                let t = tile as usize;
                if good == Good::Wood
                    && matches!(self.map.biome_at(t), Biome::Forest | Biome::Conifers)
                    && self.rng.chance(0.12)
                {
                    self.set_biome(t, Biome::Meadow);
                }
            }
            Step::Craft(wid) => self.craft(i, wid),
            Step::Talk(j) => self.talk(i, j as usize),
            Step::HideHere => {
                let b = &mut self.people[i].brain;
                b.hidden = true;
                b.carry = None;
                b.act = Act::Sleeping;
            }
            Step::Journey { to, back } => self.journey(i, to, back),
            Step::Arrive => self.trade_arrive(i),
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
                let amount = amount * self.work_bonus(i);
                let b = &mut self.people[i].brain;
                b.carry = Some(Carry::Good(Good::Grain));
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
            self.people[i].brain.intent = Intent::Leave;
            self.plan(i, [Step::Leave]);
            return;
        }
        if self.trader_assign(i) {
            return;
        }
        let part = day_part(self.clock().hour);
        let home = self.home_tile(i).map(|(x, y)| self.map.idx(x, y) as u32);
        match part {
            DayPart::Night => {
                self.people[i].brain.intent = Intent::Sleep;
                match home {
                    Some(h) => self.plan(i, [Step::Go(h), Step::Hide]),
                    None => self.plan(i, [Self::wait(30 * MINUTE, Act::Idle)]),
                }
            }
            DayPart::Morning => {
                self.people[i].brain.intent = Intent::Rest;
                self.near_home(i, 20 * MINUTE, Act::Idle);
            }
            DayPart::Evening | DayPart::Work if self.people[i].child(tick) => {
                self.people[i].brain.intent = Intent::Play;
                self.play(i);
            }
            DayPart::Evening | DayPart::Work => {
                let intent = self.decide(i, part == DayPart::Work);
                self.carry_out(i, intent);
            }
        }
    }

    // ------------------------------------------------------------------ intentions (utilité)

    /// Note chaque chose que l'habitant pourrait faire selon ses besoins, sa personnalité,
    /// son humeur, l'heure, ses relations et ses objectifs ; garde la meilleure (avec un peu
    /// de hasard).
    fn decide(&mut self, i: usize, work_time: bool) -> Intent {
        let tick = self.tick;
        let vi = self.village_of(i);
        let p = &self.people[i];
        let (n, t, mood) = (p.mind.needs, p.mind.traits, p.mind.mood);
        let elder = p.elder(tick);
        let evening = if work_time { 0.0 } else { 1.0 };
        let mut options: Vec<(f32, Intent)> = Vec::with_capacity(6);

        // Travailler.
        if work_time
            && !elder
            && let Some(job) = p.job
        {
            let hh = self.home_household(i);
            let poverty = hh.map_or(0.3, |h| {
                (if h.coins < 5.0 { 0.25 } else { 0.0 }) + (if h.rations() < 21.0 { 0.25 } else { 0.0 })
            });
            let goal =
                if p.mind.has_goal(crate::mind::Goal::Master) || p.mind.has_goal(crate::mind::Goal::Wealth) {
                    0.15
                } else {
                    0.0
                };
            let mut w = 0.85 + 0.3 * t.rigor + 0.2 * t.ambition + poverty + goal
                - 1.5 * (0.4 - n.sleep).max(0.0)
                - 0.3 * mood.sadness;
            if rest_day(tick) {
                // Le septième jour, seuls les pressés d'argent travaillent.
                w = if poverty > 0.2 || p.mind.has_goal(crate::mind::Goal::Wealth) {
                    w * 0.45
                } else {
                    0.0
                };
            }
            // Son métier ne rapporte presque rien et le foyer a de quoi voir venir.
            if job != Job::Builder
                && self.job_value(vi, job) < 0.6
                && hh.is_some_and(|h| h.coins > 10.0 && h.rations() > 14.0 * 3.0)
            {
                w *= 0.35;
            }
            let learning = (tick < p.apprentice_until)
                .then_some(p.master)
                .flatten()
                .filter(|&m| self.available(m as usize));
            options.push((w, learning.map_or(Intent::Work, Intent::Learn)));
        }

        // Discuter avec un ami.
        if let Some(f) = self.pick_friend(i) {
            let c = (1.0 - n.company) * (0.5 + 0.9 * t.sociability) + 0.15 * mood.joy + 0.3 * evening + 0.05;
            options.push((c, Intent::Chat(f)));
        }

        // Faire la cour.
        if self.single(i)
            && let Some((q, attraction)) = self.pick_crush(i)
        {
            let p = &self.people[i];
            let goal = if p.mind.has_goal(crate::mind::Goal::Marry) {
                0.25
            } else {
                0.0
            };
            let c = 0.1 + attraction / 100.0 * 0.9 + goal + 0.2 * (1.0 - n.company) + 0.25 * evening;
            options.push((c, Intent::Court(q)));
        }

        // Rendre visite à la famille qui vit ailleurs.
        if let Some(q) = self.pick_relative(i) {
            let v = (1.0 - n.meaning) * 0.8 + 0.25 * t.attachment + 0.2 * evening;
            options.push((v, Intent::Visit(q)));
        }

        // Se promener, se reposer.
        let free = if elder || rest_day(tick) { 0.2 } else { 0.0 };
        options.push((
            (1.0 - n.leisure) * (0.45 + 0.6 * t.openness) + 0.05 + free,
            Intent::Stroll,
        ));
        let rest = 0.1 + (1.0 - n.sleep) * 0.7 + if elder { 0.3 } else { 0.0 } + 0.3 * evening;
        options.push((rest, Intent::Rest));

        let mut best = (f32::MIN, Intent::Rest);
        for (score, intent) in options {
            let s = score + self.rng.f32() * (0.15 + 0.15 * t.openness);
            if s > best.0 {
                best = (s, intent);
            }
        }
        best.1
    }

    /// Découpe une intention en étapes (le plan). Si elle n'est pas réalisable, on se rabat
    /// sur une promenade ou du repos.
    fn carry_out(&mut self, i: usize, intent: Intent) {
        self.people[i].brain.intent = intent;
        let done = match intent {
            Intent::Work => self.work(i),
            Intent::Learn(m) => self.learn(i, m),
            Intent::Chat(f) | Intent::Court(f) => self.meet(i, f, true),
            Intent::Visit(q) => self.meet(i, q, false),
            Intent::Stroll => {
                self.stroll(i);
                true
            }
            _ => false,
        };
        if !done {
            self.people[i].brain.intent = Intent::Rest;
            self.near_home(i, 30 * MINUTE, Act::Idle);
        }
    }

    /// Quelqu'un qu'on peut aller voir maintenant (dehors, au village, pas en partance).
    fn available(&self, q: usize) -> bool {
        let p = &self.people[q];
        p.alive() && !p.brain.hidden && !p.brain.leaving && !p.brain.traveling && !self.away(q)
    }

    /// Un ami (affection d'au moins 15, pas un rival) disponible, tiré au sort selon l'affection.
    fn pick_friend(&mut self, i: usize) -> Option<u32> {
        let tick = self.tick;
        let village = self.people[i].village;
        let friends: Vec<(u32, f32)> = self.people[i]
            .mind
            .relations
            .iter()
            .filter(|(_, r)| r.affection >= 15.0 && r.rivalry < r.affection)
            .map(|(&id, r)| (id, r.affection))
            .filter(|&(f, _)| {
                let q = &self.people[f as usize];
                self.available(f as usize) && !q.child(tick) && q.village == village
            })
            .collect();
        let total: f32 = friends.iter().map(|f| f.1).sum();
        let mut roll = self.rng.f32() * total;
        for (f, a) in friends {
            roll -= a;
            if roll <= 0.0 {
                return Some(f);
            }
        }
        None
    }

    /// Celui ou celle qui plaît le plus (et qui est libre), ou, pour qui cherche l'amour,
    /// un célibataire qu'on connaît un peu.
    fn pick_crush(&mut self, i: usize) -> Option<(u32, f32)> {
        let best = self.people[i]
            .mind
            .relations
            .iter()
            .filter(|(_, r)| r.attraction >= 12.0)
            .filter(|&(&q, _)| self.available(q as usize) && self.could_pair(i, q as usize))
            .max_by(|a, b| a.1.attraction.total_cmp(&b.1.attraction))
            .map(|(&q, r)| (q, r.attraction));
        if best.is_some() || !self.people[i].mind.has_goal(crate::mind::Goal::Marry) {
            return best;
        }
        let female = self.people[i].female;
        let options: Vec<u32> = self.people[i]
            .mind
            .relations
            .iter()
            .filter(|(_, r)| r.affection >= 5.0 && r.rivalry < 20.0)
            .map(|(&q, _)| q)
            .filter(|&q| {
                self.available(q as usize)
                    && self.could_pair(i, q as usize)
                    && self.people[q as usize].female != female
            })
            .collect();
        self.rng.pick(&options).map(|&q| (q, 0.0))
    }

    /// Un parent, un enfant, un frère ou une sœur qui vit sous un autre toit.
    fn pick_relative(&mut self, i: usize) -> Option<u32> {
        let tick = self.tick;
        let (home, village) = (self.people[i].home, self.people[i].village);
        let family: Vec<u32> = self
            .family_of(i)
            .into_iter()
            .filter(|&q| {
                let p = &self.people[q as usize];
                self.available(q as usize) && p.village == village && p.home != home && !p.child(tick)
            })
            .collect();
        self.rng.pick(&family).copied()
    }

    /// L'apprenti suit son maître et l'imite.
    fn learn(&mut self, i: usize, m: u32) -> bool {
        let q = &self.people[m as usize];
        if !self.available(m as usize) {
            return false;
        }
        let (t, act) = (q.tile(self.map.width), q.brain.act);
        let act = if act == Act::Idle || act == Act::Walking {
            Act::Learning
        } else {
            act
        };
        // L'apprenti progresse vite tant que le maître en sait plus que lui.
        if let Some(job) = self.people[i].job {
            let gap = (q.mind.skill(job) - self.people[i].mind.skill(job)).max(0.0);
            self.improve(i, job, 0.06 * gap);
        }
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
        true
    }

    fn stroll(&mut self, i: usize) {
        let v = &self.villages[self.village_of(i)];
        let (x, y) = (v.x, v.y);
        let spot = self.spot_near(x, y, 10);
        self.plan(i, [Step::Go(spot), Self::wait(40 * MINUTE, Act::Strolling)]);
    }

    /// Va retrouver quelqu'un (au puits, ou là où il se trouve pour une visite), qui
    /// interrompt ce qu'il fait s'il le peut ; la conversation a lieu à la fin.
    fn meet(&mut self, i: usize, f: u32, at_well: bool) -> bool {
        if !self.available(f as usize) {
            return false;
        }
        let me = self.people[i].id;
        let (cx, cy) = if at_well {
            self.villages[self.village_of(i)].well
        } else {
            let q = &self.people[f as usize];
            (q.x as u32, q.y as u32)
        };
        let a = self.spot_near(cx, cy, 1);
        let b = self.spot_near(cx, cy, 1);
        let ticks = if at_well { CHAT } else { 2 * CHAT };
        self.plan(
            i,
            [
                Step::Go(a),
                Step::Wait {
                    ticks,
                    act: Act::Chatting,
                    with: Some(f),
                },
                Step::Talk(f),
            ],
        );
        let fi = f as usize;
        let busy = self
            .trade
            .traders
            .get(&f)
            .is_some_and(|t| t.leg != crate::trade::Leg::Idle);
        let fb = &mut self.people[fi].brain;
        if fb.carry.is_none() && fb.wait == 0 && !busy {
            fb.plan.clear();
            fb.path.clear();
            fb.path_pos = 0;
            fb.intent = Intent::Chat(me);
            fb.plan.extend([
                Step::Go(b),
                Step::Wait {
                    ticks: ticks + 6,
                    act: Act::Chatting,
                    with: Some(me),
                },
            ]);
        }
        true
    }

    /// Temps libre (repli quand rien d'autre n'est possible) : un ami, une promenade, ou
    /// devant chez soi.
    fn leisure(&mut self, i: usize) {
        if let Some(f) = self.pick_friend(i)
            && self.rng.chance(0.3)
            && self.meet(i, f, true)
        {
            self.people[i].brain.intent = Intent::Chat(f);
            return;
        }
        if self.rng.chance(0.5) {
            self.people[i].brain.intent = Intent::Stroll;
            self.stroll(i);
            return;
        }
        self.people[i].brain.intent = Intent::Rest;
        self.near_home(i, 30 * MINUTE, Act::Idle);
    }

    // ------------------------------------------------------------------ plans (HTN)

    /// Le plan tient-il toujours ? (la personne visée est partie, le champ a été moissonné
    /// par un autre, le chantier est fini, l'atelier n'existe plus…)
    fn plan_still_valid(&self, i: usize) -> bool {
        let b = &self.people[i].brain;
        if b.plan.is_empty() || b.leaving {
            return true;
        }
        if let Some(q) = b.intent.target() {
            let p = &self.people[q as usize];
            if !p.alive() || p.brain.leaving || p.village != self.people[i].village {
                return false;
            }
        }
        let Some(Step::Wait {
            act, with: Some(w), ..
        }) = b.plan.front()
        else {
            return true;
        };
        let vi = self.village_of(i);
        let v = &self.villages[vi];
        match act {
            Act::Ploughing | Act::Sowing | Act::Tending | Act::Harvesting => {
                let want = match act {
                    Act::Ploughing => FieldState::Fallow,
                    Act::Sowing => FieldState::Ploughed,
                    Act::Tending => FieldState::Sown,
                    _ => FieldState::Ripe,
                };
                v.fields.iter().any(|f| f.id == *w && f.state == want)
            }
            Act::Building => v.house(*w).is_some_and(|h| h.state == HouseState::Building),
            Act::Milling | Act::Baking | Act::Forging => self.economy.workshops.contains_key(w),
            _ => true,
        }
    }

    /// Abandonne le plan en cours ; ce qu'on porte est rapporté au village.
    fn abandon(&mut self, i: usize) {
        let vi = self.village_of(i);
        let b = &mut self.people[i].brain;
        b.plan.clear();
        b.path.clear();
        b.path_pos = 0;
        b.wait = 0;
        b.intent = Intent::None;
        if matches!(b.carry, Some(Carry::Good(_))) {
            let door = self.villages[vi].door();
            let door = self.map.idx(door.0, door.1) as u32;
            self.plan(i, [Step::Go(door), Step::Deposit]);
        }
    }

    /// Une case libre et praticable près de chez soi (ou du grenier).
    pub(super) fn spot_near(&mut self, cx: u32, cy: u32, r: i64) -> u32 {
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

    /// Une séance de travail dans son métier. Faux s'il n'y a vraiment rien à faire.
    fn work(&mut self, i: usize) -> bool {
        let vi = self.village_of(i);
        let has_stone = self.villages[vi].has_stone;
        let stock_of = |w: &World, g: Good| w.home_household(i).map_or(0.0, |h| h.inv[g as usize]);
        // Au-delà de ces réserves, on n'en produit plus (personne ne les achète).
        let glut = |w: &World, g: Good| stock_of(w, g) > 60.0;
        let job = self.people[i].job;
        let done = match job {
            Some(Job::Builder) => {
                self.build(i, vi) || {
                    let st = &self.villages[vi].stock;
                    let need_stone = self.villages[vi]
                        .houses
                        .iter()
                        .any(|h| h.state == HouseState::Building && h.stone_needed > 0);
                    let g = if need_stone && has_stone && st.stone == 0 {
                        Good::Stone
                    } else {
                        Good::Wood
                    };
                    self.gather(i, vi, g)
                }
            }
            Some(Job::Woodcutter) if !glut(self, Good::Wood) => self.gather(i, vi, Good::Wood),
            Some(Job::Quarrier) if has_stone && !glut(self, Good::Stone) => self.gather(i, vi, Good::Stone),
            Some(Job::Fisher) if !glut(self, Good::Fish) => self.gather(i, vi, Good::Fish),
            Some(Job::Hunter) if !glut(self, Good::Meat) => self.gather(i, vi, Good::Meat),
            Some(Job::Miner) if !glut(self, Good::Ore) => self.gather(i, vi, Good::Ore),
            Some(Job::Miller) => self.workshop_work(i, vi, Workshop::Mill),
            Some(Job::Baker) => self.workshop_work(i, vi, Workshop::Bakery),
            Some(Job::Smith) => self.workshop_work(i, vi, Workshop::Smithy),
            Some(Job::Farmer) => self.farm(i, vi),
            Some(Job::Merchant) => self.merchant_work(i, vi),
            _ => false,
        };
        if !done {
            // Rien à faire dans son métier : on aide aux champs, sinon on coupe du bois.
            let ok = self.farm(i, vi) || (!glut(self, Good::Wood) && self.gather(i, vi, Good::Wood));
            if !ok {
                self.leisure(i);
            }
        }
        true
    }

    /// Une ressource proche du village (bois ou pierre) et la case d'où la travailler.
    fn find_resource(&mut self, vi: usize, r: Good) -> Option<(u32, u32)> {
        // D'abord près du village, puis plus loin si rien n'est trouvé.
        self.find_resource_within(vi, r, RESOURCE_RADIUS as i64)
            .or_else(|| self.find_resource_within(vi, r, RESOURCE_RADIUS as i64 * 2))
            .or_else(|| self.find_resource_within(vi, r, RESOURCE_RADIUS as i64 * 3))
    }

    fn find_resource_within(&mut self, vi: usize, r: Good, rad: i64) -> Option<(u32, u32)> {
        let (cx, cy) = (self.villages[vi].x as i64, self.villages[vi].y as i64);
        let mut best: Option<(f32, u32, u32)> = None;
        for _ in 0..90 {
            let (x, y) = (cx + self.rng.range(-rad, rad), cy + self.rng.range(-rad, rad));
            if !self.map.inside(x, y) {
                continue;
            }
            let t = self.map.idx(x as u32, y as u32);
            if !resource_at(&self.map, t, r) {
                continue;
            }
            // On pêche depuis la rive ; ailleurs on se tient sur la case ou à côté.
            let around: &[(i64, i64)] = if r == Good::Fish {
                &[(0, 1), (1, 0), (-1, 0), (0, -1)]
            } else {
                &[(0, 1), (1, 0), (-1, 0), (0, -1), (0, 0)]
            };
            let stand = around.iter().copied().into_iter().find_map(|(dx, dy)| {
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

    /// Va chercher une ressource et la rapporte au marché. Faux si rien n'est à portée.
    fn gather(&mut self, i: usize, vi: usize, r: Good) -> bool {
        let Some((res, stand)) = self.find_resource(vi, r) else {
            return false;
        };
        let act = match r {
            Good::Wood => Act::Chopping,
            Good::Stone => Act::Quarrying,
            Good::Fish => Act::Fishing,
            Good::Meat => Act::Hunting,
            _ => Act::Mining,
        };
        let door = self.villages[vi].door();
        let door = self.map.idx(door.0, door.1) as u32;
        let session = WORK_SESSION - self.rng.below(HOUR / 2);
        self.plan(
            i,
            [
                Step::Go(stand),
                Self::wait(session, act),
                Step::Gather(res, r),
                Step::Go(door),
                Step::Deposit,
            ],
        );
        true
    }

    /// Travail à l'atelier de son métier (moulin, four, forge). Faux s'il n'existe pas encore.
    fn workshop_work(&mut self, i: usize, vi: usize, kind: Workshop) -> bool {
        let v = &self.villages[vi];
        let shop = v
            .houses
            .iter()
            .find(|h| h.state == HouseState::Inhabited && self.economy.workshops.get(&h.id) == Some(&kind));
        let Some(h) = shop else { return false };
        let (wid, tile) = (h.id, self.map.idx(h.x, h.y) as u32);
        // Personne n'achète ce qu'on a déjà produit : inutile d'en faire plus.
        let output = match kind {
            Workshop::Mill => Good::Flour,
            Workshop::Bakery => Good::Bread,
            Workshop::Smithy => Good::Tools,
        };
        let limit = if kind == Workshop::Smithy { 6.0 } else { 60.0 };
        if self
            .home_household(i)
            .is_some_and(|h| h.inv[output as usize] > limit)
        {
            return false;
        }
        let act = match kind {
            Workshop::Mill => Act::Milling,
            Workshop::Bakery => Act::Baking,
            Workshop::Smithy => Act::Forging,
        };
        self.plan(
            i,
            [
                Step::Go(tile),
                Step::Wait {
                    ticks: WORK_SESSION,
                    act,
                    with: Some(wid),
                },
                Step::Craft(wid),
            ],
        );
        true
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
            Good::Stone
        } else {
            Good::Wood
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
            // Une seule récolte par an : on laboure et sème au printemps.
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

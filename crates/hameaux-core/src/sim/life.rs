//! Vie des familles et des villages : repas, âges, naissances, couples, maisons, départs.

use crate::{
    clock::Season,
    mind::{Emotion, MemoryKind},
    names,
    people::{Act, Status, Step},
    settlement::HouseState,
    tuning::*,
    world::{World, occ},
};

use super::{DayPart, day_part};

impl World {
    /// Une fois par jour (minuit) : âges, décès, naissances, couples.
    pub(super) fn daily(&mut self) {
        let tick = self.tick;
        for i in 0..self.people.len() {
            let p = &self.people[i];
            if !p.alive() || p.brain.leaving {
                continue;
            }
            let age = p.age(tick);
            if age >= p.death_age {
                self.die_of(i, None);
            } else if age >= ADULT_AGE && p.job.is_none() {
                self.come_of_age(i);
            }
        }
        self.births();
        self.pairing();
    }

    /// Décès (de vieillesse, ou de la maladie nommée : « de la fièvre »).
    pub(crate) fn die_of(&mut self, i: usize, cause: Option<&str>) {
        let tick = self.tick;
        let chief = self
            .society
            .office(self.people[i].village, self.people[i].id)
            .is_some_and(|o| o == "chef");
        let p = &mut self.people[i];
        let (id, village, age) = (p.id, p.village, p.age(tick) as u32);
        p.status = Status::Dead { tick };
        p.brain = Default::default();
        let home = p.home.take();
        let mut text = match cause {
            Some(c) => format!("{} meurt {c} à {age} ans", p.name()),
            None => format!("Décès {} à {age} ans", de(&p.name())),
        };
        if chief {
            text += ", le village perd son chef";
        }
        self.mourn(i);
        if let Some(vi) = self.village_index(village)
            && let Some(h) = home.and_then(|h| self.villages[vi].house_mut(h))
        {
            h.residents.retain(|&r| r != id);
        }
        self.event(Some(village), vec![id], text);
    }

    fn come_of_age(&mut self, i: usize) {
        let tick = self.tick;
        let id = self.people[i].id;
        let job = self.pick_job(id);
        let village = self.people[i].village;
        // Un maître du même métier, de préférence un parent.
        let parents = self.people[i].parents.clone();
        // Seul un maître prend des apprentis (deux à la fois au plus).
        let pupils = |m: u32| {
            self.alive()
                .filter(|p| p.master == Some(m) && tick < p.apprentice_until)
                .count()
        };
        let master = self
            .alive()
            .filter(|q| {
                q.village == village
                    && q.id != id
                    && q.job == Some(job)
                    && !q.elder(tick)
                    && q.mind.skill(job) >= crate::mind::MASTERY
            })
            .filter(|q| pupils(q.id) < super::mind::APPRENTICES_PER_MASTER)
            .min_by_key(|q| if parents.contains(&q.id) { 0 } else { 1 })
            .map(|q| q.id);
        let p = &mut self.people[i];
        p.job = Some(job);
        p.master = master;
        p.apprentice_until = if master.is_some() {
            tick + APPRENTICESHIP
        } else {
            0
        };
        // Les enfants de la partie apprennent un peu en regardant leurs parents.
        let watched = parents.iter().any(|&q| self.people[q as usize].job == Some(job));
        let p = &mut self.people[i];
        let s = p.mind.skill_mut(job);
        *s = s.max(if watched { 0.2 } else { 0.08 });
        let mut text = format!("{} devient {}", p.first, job.label(p.female));
        let mut who = vec![id];
        self.pick_goals(i);
        if let Some(m) = master {
            self.feel(i, MemoryKind::Apprenticed, Some(m), 0.45, Emotion::Joy, 0.3);
            let r = self.people[i].mind.rel_mut(m);
            r.respect = r.respect.max(40.0);
            r.trust = r.trust.max(30.0);
            text += &format!(" auprès {}", de(&self.people[m as usize].first));
            who.push(m);
        }
        self.event(Some(village), who, text);
    }

    fn births(&mut self) {
        let tick = self.tick;
        for i in 0..self.people.len() {
            let p = &self.people[i];
            if !p.alive() || !p.female || p.brain.leaving || p.child(tick) || p.age(tick) > FERTILE_MAX_AGE {
                continue;
            }
            let Some(q) = p.partner else { continue };
            let q = &self.people[q as usize];
            if !q.alive() || q.female || q.home != p.home {
                continue;
            }
            let Some(vi) = self.village_index(p.village) else {
                continue;
            };
            let Some(h) = p.home.and_then(|h| self.villages[vi].house(h)) else {
                continue;
            };
            let v = &self.villages[vi];
            // Un foyer qui a de quoi manger quelques jours.
            let food = self.economy.households.get(&h.id).map_or(0.0, |hh| hh.rations());
            if h.state != HouseState::Inhabited
                || h.residents.len() >= HOUSE_CAPACITY
                || v.hungry_days > 0
                || food < h.residents.len() as f32 * 3.0
            {
                continue;
            }
            if !self.rng.chance(BIRTH_PER_DAY) {
                continue;
            }
            let (mother, father, last, home) = (p.id, q.id, p.last.clone(), h.id);
            let female = self.rng.chance(0.5);
            let child = self.new_person(vi, &last, female, 0.0, Some(home), &[mother, father]);
            self.send_home(child as usize);
            for parent in [mother, father] {
                self.feel(
                    parent as usize,
                    MemoryKind::ChildBorn,
                    Some(child),
                    0.9,
                    Emotion::Joy,
                    0.7,
                );
                let m = &mut self.people[parent as usize].mind;
                m.needs.meaning = (m.needs.meaning + 0.3).min(1.0);
            }
            let text = format!(
                "Naissance {} chez {} et {}",
                de(&self.people[child as usize].first),
                self.people[mother as usize].first,
                self.people[father as usize].first
            );
            let village = self.villages[vi].id;
            self.event(Some(village), vec![child, mother, father], text);
        }
    }

    pub(super) fn related(&self, a: u32, b: u32) -> bool {
        let (pa, pb) = (&self.people[a as usize], &self.people[b as usize]);
        pa.parents.contains(&b)
            || pb.parents.contains(&a)
            || pa.parents.iter().any(|x| pb.parents.contains(x))
    }

    pub(super) fn single(&self, i: usize) -> bool {
        let tick = self.tick;
        let p = &self.people[i];
        let age = p.age(tick);
        p.alive()
            && !p.brain.leaving
            && (18.0..=55.0).contains(&age)
            && p.partner.is_none_or(|q| !self.people[q as usize].alive())
    }

    /// Les célibataires d'un même village se trouvent : d'abord ceux qui se plaisent (la cour
    /// faite au puits fait monter l'attirance), sinon, plus rarement, des unions arrangées.
    /// Ils s'installent ensemble.
    fn pairing(&mut self) {
        for i in 0..self.people.len() {
            if !self.single(i) {
                continue;
            }
            let me = self.people[i].id;
            // Le béguin réciproque le plus fort.
            let mut best: Option<(f32, u32)> = None;
            for (&q, r) in &self.people[i].mind.relations {
                let j = q as usize;
                if r.attraction < 25.0 || !self.could_pair(i, j) {
                    continue;
                }
                let back = self.people[j].mind.rel(me).attraction;
                if back < 20.0 {
                    continue;
                }
                let score = r.attraction + back + 0.3 * (r.affection + self.people[j].mind.affection(me));
                if best.is_none_or(|b| score > b.0) {
                    best = Some((score, q));
                }
            }
            let j = match best {
                // Plus l'attirance est forte, plus la demande vient vite.
                Some((score, q)) if self.rng.chance((score / 400.0).min(0.5)) => q,
                Some(_) => continue,
                None => {
                    if !self.rng.chance(PAIRING_PER_DAY * 0.6) {
                        continue;
                    }
                    // Parfois, l'âme sœur vit dans un village que les marchands fréquentent.
                    if self.rng.chance(0.25)
                        && let Some(j) = self.distant_match(i)
                    {
                        self.couple(i as u32, j);
                        if self.join_partner_village(i as u32, j) {
                            self.heartbreak(i as u32, j);
                            for (x, y) in [(i as u32, j), (j, i as u32)] {
                                self.feel(
                                    x as usize,
                                    MemoryKind::PartnerFound,
                                    Some(y),
                                    0.9,
                                    Emotion::Joy,
                                    0.6,
                                );
                            }
                        } else {
                            // Personne n'a de place pour les accueillir : ce ne sera pas pour cette fois.
                            self.people[i].partner = None;
                            self.people[j as usize].partner = None;
                        }
                        continue;
                    }
                    let p = &self.people[i];
                    let want_female = if self.rng.chance(0.9) {
                        Some(!p.female)
                    } else {
                        None
                    };
                    let candidates: Vec<u32> = (0..self.people.len())
                        .filter(|&j| j != i && self.could_pair(i, j))
                        .filter(|&j| want_female.is_none_or(|f| self.people[j].female == f))
                        // On n'épouse pas son rival.
                        .filter(|&j| self.people[i].mind.rel(j as u32).rivalry < 30.0)
                        .map(|j| j as u32)
                        .collect();
                    let Some(&j) = self.rng.pick(&candidates) else {
                        continue;
                    };
                    j
                }
            };
            let a = i as u32;
            self.couple(a, j);
            self.heartbreak(a, j);
            for (x, y) in [(a, j), (j, a)] {
                self.feel(
                    x as usize,
                    MemoryKind::PartnerFound,
                    Some(y),
                    0.9,
                    Emotion::Joy,
                    0.6,
                );
            }
            let village = self.people[i].village;
            let (na, nb) = (
                self.people[i].first.clone(),
                self.people[j as usize].first.clone(),
            );
            // L'un a sa propre maison (sans ses parents) : l'autre l'y rejoint. Sinon ils
            // attendront la leur (voir le planificateur).
            let own = |w: &World, x: u32| -> Option<u32> {
                let p = &w.people[x as usize];
                let vi = w.village_index(p.village)?;
                let h = w.villages[vi].house(p.home?)?;
                let with_parents = h.residents.iter().any(|r| p.parents.contains(r));
                (!with_parents && h.residents.len() < HOUSE_CAPACITY).then_some(h.id)
            };
            if let Some(h) = own(self, a) {
                self.move_in(j, h);
                self.event(Some(village), vec![a, j], format!("{nb} emménage chez {na}"));
            } else if let Some(h) = own(self, j) {
                self.move_in(a, h);
                self.event(Some(village), vec![a, j], format!("{na} emménage chez {nb}"));
            } else {
                self.event(Some(village), vec![a, j], format!("{na} et {nb} se fiancent"));
            }
        }
    }

    pub(crate) fn move_in(&mut self, person: u32, house: u32) {
        let village = self.people[person as usize].village;
        let Some(vi) = self.village_index(village) else {
            return;
        };
        if let Some(old) = self.people[person as usize].home
            && let Some(h) = self.villages[vi].house_mut(old)
        {
            h.residents.retain(|&r| r != person);
        }
        let Some(h) = self.villages[vi].house_mut(house) else {
            return;
        };
        h.residents.push(person);
        let tile = (h.x, h.y);
        let p = &mut self.people[person as usize];
        p.home = Some(house);
        p.brain.plan.clear();
        p.brain.path.clear();
        p.brain.path_pos = 0;
        p.brain.wait = 0;
        if !p.brain.hidden {
            let t = tile.1 * self.map.width + tile.0;
            p.brain.plan.push_back(Step::Go(t));
        }
    }

    /// Adulte qui vit encore chez ses parents et veut sa maison (avec son conjoint).
    fn is_mover(&self, i: usize) -> bool {
        let tick = self.tick;
        let p = &self.people[i];
        if !p.alive() || p.brain.leaving || p.age(tick) < MOVE_OUT_AGE || p.elder(tick) {
            return false;
        }
        let Some(vi) = self.village_index(p.village) else {
            return false;
        };
        let Some(h) = p.home.and_then(|h| self.villages[vi].house(h)) else {
            return true;
        };
        h.residents.iter().any(|r| p.parents.contains(r))
    }

    /// Toutes les 10 minutes : chantiers, champs, nouveaux venus, déclin des maisons vides.
    pub(super) fn planner(&mut self, vi: usize) {
        let tick = self.tick;
        let id = self.villages[vi].id;
        let pop = self.population(id);
        let season = self.clock().season;

        if pop == 0 {
            self.pioneers(vi);
            return;
        }

        // Une maison vide encore debout : un candidat au départ de chez ses parents la reprend.
        let reserved: Vec<u32> = self.villages[vi]
            .houses
            .iter()
            .filter_map(|h| h.reserved_for)
            .collect();
        let movers: Vec<usize> = (0..self.people.len())
            .filter(|&i| self.people[i].village == id && self.is_mover(i))
            .filter(|&i| {
                let p = &self.people[i];
                !reserved.contains(&p.id) && p.partner.is_none_or(|q| !reserved.contains(&q))
            })
            .collect();
        if let Some(&m) = movers.first() {
            let empty = self.villages[vi]
                .houses
                .iter()
                .find(|h| h.state == HouseState::Abandoned && !self.economy.workshops.contains_key(&h.id))
                .map(|h| h.id);
            if let Some(h) = empty {
                self.villages[vi].house_mut(h).unwrap().state = HouseState::Inhabited;
                self.settle_mover(vi, m as u32, h, false);
            } else {
                let sites = self.villages[vi]
                    .houses
                    .iter()
                    .filter(|h| h.state == HouseState::Building)
                    .count();
                if sites < 1 + pop / 25
                    && let Some(h) = self.new_house(vi, HouseState::Building)
                {
                    self.villages[vi].house_mut(h).unwrap().reserved_for = Some(m as u32);
                    let name = self.people[m].first.clone();
                    self.event(
                        Some(id),
                        vec![m as u32],
                        format!("Nouveau chantier : la maison {}", de(&name)),
                    );
                }
            }
        }

        // Nouveaux venus attirés par un grenier bien rempli (ou un village trop petit).
        let homes: Vec<u32> = self.villages[vi].houses.iter().map(|h| h.id).collect();
        let food: f32 = homes
            .iter()
            .filter_map(|h| self.economy.households.get(h))
            .map(|h| h.rations())
            .sum::<f32>()
            + self.villages[vi].stock.food;
        let sites = self.villages[vi]
            .houses
            .iter()
            .filter(|h| h.state == HouseState::Building)
            .count();
        let attractive = pop < 8 || food > pop as f32 * 25.0;
        if attractive
            && sites == 0
            && self.rng.chance(IMMIGRATION_PER_DAY / 144.0)
            && self.new_house(vi, HouseState::Building).is_some()
        {
            self.event(
                Some(id),
                vec![],
                "Nouveau chantier : une maison pour de nouveaux venus".into(),
            );
        }

        if self.rng.chance(0.05) {
            self.plan_workshops(vi);
        }

        // Champs : assez pour nourrir tout le monde.
        let target = (pop as f32 * FIELDS_PER_PERSON).ceil() as usize + 1;
        // … tant que le grain se vend (s'il ne vaut plus rien, inutile de défricher).
        let grain_sells = self.economy.markets.get(vi).is_none_or(|m| {
            m.price[crate::economy::Good::Grain as usize] >= crate::economy::Good::Grain.base_price() * 0.6
        });
        if season != Season::Winter
            && self.villages[vi].fields.len() < target
            && (grain_sells || self.villages[vi].fields.len() < 3)
            && self.rng.chance(0.3)
        {
            self.new_field(vi);
        }

        // Maisons vides : abandon, ruine, puis la nature reprend la parcelle.
        let mut gone = Vec::new();
        let mut abandoned = Vec::new();
        let workshops = &self.economy.workshops;
        for h in &mut self.villages[vi].houses {
            match h.state {
                _ if workshops.contains_key(&h.id) => {}
                HouseState::Inhabited if h.residents.is_empty() && h.reserved_for.is_none() => {
                    h.state = HouseState::Abandoned;
                    h.since = tick;
                    abandoned.push(h.family.clone());
                }
                HouseState::Abandoned if tick - h.since > ABANDONED_TO_RUIN => {
                    h.state = HouseState::Ruin;
                    h.since = tick;
                }
                HouseState::Ruin if tick - h.since > RUIN_TO_OVERGROWN => {
                    h.state = HouseState::Overgrown;
                    h.since = tick;
                }
                HouseState::Overgrown if tick - h.since > OVERGROWN_TO_GONE => gone.push((h.id, h.x, h.y)),
                _ => {}
            }
        }
        for family in abandoned {
            self.event(Some(id), vec![], format!("La maison des {family} est abandonnée"));
        }
        for (hid, x, y) in gone {
            self.villages[vi].houses.retain(|h| h.id != hid);
            let i = self.map.idx(x, y);
            self.set_occ(i, occ::NONE);
        }
    }

    /// Un chantier se termine : son futur habitant s'y installe, ou une famille arrive.
    pub(crate) fn complete_house(&mut self, vi: usize, hid: u32) {
        let tick = self.tick;
        let id = self.villages[vi].id;
        let h = self.villages[vi].house_mut(hid).unwrap();
        h.state = HouseState::Inhabited;
        h.since = tick;
        let mover = h.reserved_for.take();
        if let Some(kind) = self.economy.workshops.get(&hid) {
            let name = self.villages[vi].name.clone();
            let what = kind.the();
            let what = what[..1].to_uppercase() + &what[1..];
            let done = if *kind == crate::economy::Workshop::Smithy {
                "terminée"
            } else {
                "terminé"
            };
            self.event(Some(id), vec![], format!("{what} de {name} est {done}"));
            return;
        }
        match mover.filter(|&m| self.people[m as usize].alive() && !self.people[m as usize].brain.leaving) {
            Some(m) => self.settle_mover(vi, m, hid, true),
            None => {
                let last = self.rng.pick(names::LAST).unwrap().to_string();
                self.villages[vi].house_mut(hid).unwrap().family = last.clone();
                let age = 20.0 + self.rng.f32() * 15.0;
                let a = self.arrive(vi, &last, true, age, hid);
                let age_b = age + self.rng.f32() * 6.0 - 2.0;
                let b = self.arrive(vi, &last, false, age_b, hid);
                self.couple(a, b);
                if self.rng.chance(0.4) {
                    let kid_age = self.rng.f32() * 12.0;
                    let female = self.rng.chance(0.5);
                    let c = self.arrive(vi, &last, female, kid_age, hid);
                    self.people[c as usize].parents = vec![a, b];
                    self.people[a as usize].children.push(c);
                    self.people[b as usize].children.push(c);
                }
                self.open_household(hid, 25.0, 40.0);
                let text = format!(
                    "Une famille arrive : {} et {} {last}",
                    self.people[a as usize].first, self.people[b as usize].first
                );
                self.event(Some(id), vec![a, b], text);
            }
        }
    }

    /// Installe un habitant (et son conjoint) dans sa nouvelle maison.
    fn settle_mover(&mut self, vi: usize, m: u32, hid: u32, built: bool) {
        let id = self.villages[vi].id;
        let last = self.people[m as usize].last.clone();
        self.villages[vi].house_mut(hid).unwrap().family = last;
        let partner = self.people[m as usize]
            .partner
            .filter(|&q| self.people[q as usize].alive());
        // Les parents aident à démarrer : un peu d'argent et de quoi manger.
        let parent_home = self.people[m as usize].home;
        let (mut coins, mut grain) = (0.0, 0.0);
        if let Some(ph) = parent_home
            && ph != hid
            && let Some(p) = self.economy.households.get_mut(&ph)
        {
            coins = (p.coins * 0.2).min(15.0);
            grain = (p.inv[crate::economy::Good::Grain as usize] * 0.3).min(30.0);
            p.coins -= coins;
            p.inv[crate::economy::Good::Grain as usize] -= grain;
        }
        self.open_household(hid, coins, grain);
        self.move_in(m, hid);
        self.feel(m as usize, MemoryKind::NewHome, None, 0.6, Emotion::Joy, 0.4);
        let name = self.people[m as usize].first.clone();
        let place = if built {
            "leur nouvelle maison"
        } else {
            "une maison libre"
        };
        match partner {
            Some(q) => {
                self.move_in(q, hid);
                self.feel(q as usize, MemoryKind::NewHome, None, 0.6, Emotion::Joy, 0.4);
                let other = self.people[q as usize].first.clone();
                self.event(
                    Some(id),
                    vec![m, q],
                    format!("{name} et {other} s'installent dans {place}"),
                );
            }
            None => {
                let place = if built {
                    "sa nouvelle maison"
                } else {
                    "une maison libre"
                };
                self.event(Some(id), vec![m], format!("{name} s'installe dans {place}"));
            }
        }
    }

    /// Un nouveau venu arrive à pied depuis les environs.
    pub(super) fn arrive(&mut self, vi: usize, last: &str, female: bool, age: f32, home: u32) -> u32 {
        let p = self.new_person(vi, last, female, age, Some(home), &[]);
        let (x, y) = self.far_tile(vi, 22);
        let h = self.villages[vi].house(home).unwrap();
        let target = h.y * self.map.width + h.x;
        let person = &mut self.people[p as usize];
        person.x = x as f32 + 0.5;
        person.y = y as f32 + 0.5;
        person.brain.act = Act::Arriving;
        person.brain.plan.push_back(Step::Go(target));
        p
    }

    /// Une case praticable à environ `dist` cases du village (pour arriver ou partir).
    pub(crate) fn far_tile(&mut self, vi: usize, dist: i64) -> (u32, u32) {
        let (cx, cy) = (self.villages[vi].x as i64, self.villages[vi].y as i64);
        for _ in 0..30 {
            let a = self.rng.f32() * std::f32::consts::TAU;
            let (x, y) = (
                cx + (a.cos() * dist as f32) as i64,
                cy + (a.sin() * dist as f32) as i64,
            );
            if self.map.inside(x, y) && self.map.walk_cost(self.map.idx(x as u32, y as u32)).is_some() {
                return (x as u32, y as u32);
            }
        }
        (cx as u32, cy as u32 + 2)
    }

    /// Un habitant quitte le village (il marche vers le lointain, puis disparaît).
    pub(crate) fn leave(&mut self, i: usize) {
        if self.people[i].brain.leaving {
            return;
        }
        let Some(vi) = self.village_index(self.people[i].village) else {
            return;
        };
        self.miss(i);
        let (x, y) = self.far_tile(vi, 30);
        let night = day_part(self.clock().hour) == DayPart::Night;
        let width = self.map.width;
        let p = &mut self.people[i];
        p.brain = Default::default();
        p.brain.leaving = true;
        p.brain.intent = crate::people::Intent::Leave;
        p.brain.carry = Some(crate::people::Carry::Bundle);
        p.brain.plan.extend([Step::Go(y * width + x), Step::Leave]);
        if night {
            p.brain.act = Act::Leaving;
        }
    }

    /// Disette : une famille affamée s'en va, et parfois ses meilleurs amis avec elle.
    pub(super) fn famine_leave_house(&mut self, vi: usize, hid: u32) {
        // Un village voisin qui mange à sa faim les accueille peut-être.
        if self.rng.chance(0.7) && self.migrate_household(vi, hid) {
            return;
        }
        let id = self.villages[vi].id;
        let Some(h) = self.villages[vi].house(hid) else {
            return;
        };
        let (family, residents) = (h.family.clone(), h.residents.clone());
        self.event(
            Some(id),
            residents.clone(),
            format!("Faute de nourriture, la famille {family} quitte le village"),
        );
        for &r in &residents {
            self.leave(r as usize);
        }
        for &r in &residents {
            let best = self.people[r as usize]
                .mind
                .relations
                .iter()
                .filter(|&(&f, r)| {
                    r.affection >= 20.0
                        && self.people[f as usize].alive()
                        && !self.people[f as usize].brain.leaving
                })
                .max_by(|a, b| a.1.affection.total_cmp(&b.1.affection))
                .map(|(&f, _)| f);
            // Les plus attachés à leur village hésitent davantage à suivre.
            if let Some(f) = best
                && self
                    .rng
                    .chance(0.5 * (1.1 - self.people[f as usize].mind.traits.attachment))
                && let Some(fh) = self.people[f as usize].home
                && let Some(house) = self.villages[vi].house(fh)
            {
                let others = house.residents.clone();
                let name = self.people[f as usize].first.clone();
                self.event(
                    Some(id),
                    others.clone(),
                    format!("{name} et les siens les suivent"),
                );
                for o in others {
                    self.leave(o as usize);
                }
            }
        }
    }

    /// Village vide : une famille de pionniers reprend une maison (ou en construit une d'office).
    fn pioneers(&mut self, vi: usize) {
        if !self.rng.chance(1.0 / 144.0) {
            return; // environ une fois par jour
        }
        let id = self.villages[vi].id;
        let standing = self.villages[vi]
            .houses
            .iter()
            .find(|h| {
                matches!(
                    h.state,
                    HouseState::Abandoned | HouseState::Inhabited | HouseState::Building
                ) && !self.economy.workshops.contains_key(&h.id)
            })
            .map(|h| h.id);
        let hid = match standing {
            Some(h) => h,
            None => match self.new_house(vi, HouseState::Inhabited) {
                Some(h) => h,
                None => return,
            },
        };
        let h = self.villages[vi].house_mut(hid).unwrap();
        h.state = HouseState::Inhabited;
        h.wood_needed = 0;
        h.stone_needed = 0;
        h.reserved_for = None;
        let last = self.rng.pick(names::LAST).unwrap().to_string();
        h.family = last.clone();
        let a = self.arrive(vi, &last, true, 24.0, hid);
        let b = self.arrive(vi, &last, false, 26.0, hid);
        self.couple(a, b);
        self.open_household(hid, 25.0, 60.0);
        let text = format!(
            "Des pionniers s'installent : {} et {} {last}",
            self.people[a as usize].first, self.people[b as usize].first
        );
        self.event(Some(id), vec![a, b], text);
    }
}

/// « de Belair », « des Trois-Puits », « du Val » : complément de nom d'un lieu.
pub fn of_place(name: &str) -> String {
    if let Some(rest) = name.strip_prefix("Les ") {
        format!("des {rest}")
    } else if let Some(rest) = name.strip_prefix("Le ") {
        format!("du {rest}")
    } else {
        de(name)
    }
}

/// « à Belair », « aux Trois-Puits », « au Val ».
pub fn at_place(name: &str) -> String {
    if let Some(rest) = name.strip_prefix("Les ") {
        format!("aux {rest}")
    } else if let Some(rest) = name.strip_prefix("Le ") {
        format!("au {rest}")
    } else {
        format!("à {name}")
    }
}

/// « de » ou « d' » devant un nom (élision devant une voyelle ou un h muet).
pub fn de(name: &str) -> String {
    let first = name
        .chars()
        .next()
        .unwrap_or('x')
        .to_lowercase()
        .next()
        .unwrap_or('x');
    if "aeiouyhéèêëàâîïôöûüæœ".contains(first) {
        format!("d'{name}")
    } else {
        format!("de {name}")
    }
}

#[cfg(test)]
mod tests {
    use super::de;

    #[test]
    fn elision() {
        assert_eq!(de("Inès"), "d'Inès");
        assert_eq!(de("Hortense"), "d'Hortense");
        assert_eq!(de("Élise"), "d'Élise");
        assert_eq!(de("Paul"), "de Paul");
        assert_eq!(super::of_place("Les Trois-Puits"), "des Trois-Puits");
        assert_eq!(super::at_place("Le Val"), "au Val");
        assert_eq!(super::of_place("L'Aubépine"), "de L'Aubépine");
    }
}

//! Vie des familles et des villages : repas, âges, naissances, couples, maisons, départs.

use crate::{
    clock::Season,
    names,
    people::{Act, Status, Step},
    settlement::HouseState,
    tuning::*,
    world::{World, occ},
};

use super::{DayPart, day_part};

impl World {
    /// Repas de midi : chacun mange sa ration de la journée.
    pub(super) fn meals(&mut self) {
        let tick = self.tick;
        for vi in 0..self.villages.len() {
            let id = self.villages[vi].id;
            let need: f32 = self
                .alive()
                .filter(|p| p.village == id && !p.brain.leaving)
                .map(|p| if p.child(tick) { RATION_CHILD } else { RATION_ADULT })
                .sum();
            if need == 0.0 {
                continue;
            }
            let st = &mut self.villages[vi].stock;
            if st.food >= need {
                st.food -= need;
                self.villages[vi].hungry_days = 0;
            } else {
                st.food = 0.0;
                self.villages[vi].hungry_days += 1;
                let days = self.villages[vi].hungry_days;
                if days == 1 {
                    let name = self.villages[vi].name.clone();
                    self.event(Some(id), vec![], format!("Le grenier de {name} est vide"));
                }
                if days >= FAMINE_DAYS {
                    self.villages[vi].hungry_days = 0;
                    self.famine_leave(vi);
                }
            }
        }
    }

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
                self.die(i);
            } else if age >= ADULT_AGE && p.job.is_none() {
                self.come_of_age(i);
            }
        }
        self.births();
        self.pairing();
    }

    fn die(&mut self, i: usize) {
        let tick = self.tick;
        let p = &mut self.people[i];
        let (id, village, age) = (p.id, p.village, p.age(tick) as u32);
        p.status = Status::Dead { tick };
        p.brain = Default::default();
        let home = p.home.take();
        let text = format!("Décès {} à {age} ans", de(&p.name()));
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
        let master = self
            .alive()
            .filter(|q| q.village == village && q.id != id && q.job == Some(job) && !q.elder(tick))
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
        let mut text = format!("{} devient {}", p.first, job.label(p.female));
        let mut who = vec![id];
        if let Some(m) = master {
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
            let pop = self.population(v.id) as f32;
            if h.state != HouseState::Inhabited
                || h.residents.len() >= HOUSE_CAPACITY
                || v.hungry_days > 0
                || v.stock.food < pop * 5.0
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

    fn related(&self, a: u32, b: u32) -> bool {
        let (pa, pb) = (&self.people[a as usize], &self.people[b as usize]);
        pa.parents.contains(&b)
            || pb.parents.contains(&a)
            || pa.parents.iter().any(|x| pb.parents.contains(x))
    }

    fn single(&self, i: usize) -> bool {
        let tick = self.tick;
        let p = &self.people[i];
        let age = p.age(tick);
        p.alive()
            && !p.brain.leaving
            && (18.0..=55.0).contains(&age)
            && p.partner.is_none_or(|q| !self.people[q as usize].alive())
    }

    /// Les célibataires d'un même village se trouvent ; ils s'installent ensemble.
    fn pairing(&mut self) {
        let tick = self.tick;
        for i in 0..self.people.len() {
            if !self.single(i) || !self.rng.chance(PAIRING_PER_DAY) {
                continue;
            }
            let p = &self.people[i];
            let want_female = if self.rng.chance(0.9) {
                Some(!p.female)
            } else {
                None
            };
            let candidates: Vec<u32> = (0..self.people.len())
                .filter(|&j| j != i && self.single(j))
                .filter(|&j| {
                    let q = &self.people[j];
                    q.village == p.village
                        && want_female.is_none_or(|f| q.female == f)
                        && (q.age(tick) - p.age(tick)).abs() <= 12.0
                        && !self.related(p.id, q.id)
                })
                .map(|j| j as u32)
                .collect();
            let Some(&j) = self.rng.pick(&candidates) else {
                continue;
            };
            let a = i as u32;
            self.couple(a, j);
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
                .find(|h| h.state == HouseState::Abandoned)
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
        let food = self.villages[vi].stock.food;
        let sites = self.villages[vi]
            .houses
            .iter()
            .filter(|h| h.state == HouseState::Building)
            .count();
        let attractive = pop < 8 || food > pop as f32 * 40.0;
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

        // Champs : assez pour nourrir tout le monde.
        let target = (pop as f32 * FIELDS_PER_PERSON).ceil() as usize + 1;
        if season != Season::Winter && self.villages[vi].fields.len() < target && self.rng.chance(0.3) {
            self.new_field(vi);
        }

        // Maisons vides : abandon, ruine, puis la nature reprend la parcelle.
        let mut gone = Vec::new();
        let mut abandoned = Vec::new();
        for h in &mut self.villages[vi].houses {
            match h.state {
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
        self.move_in(m, hid);
        let name = self.people[m as usize].first.clone();
        let place = if built {
            "leur nouvelle maison"
        } else {
            "une maison libre"
        };
        match partner {
            Some(q) => {
                self.move_in(q, hid);
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
    fn arrive(&mut self, vi: usize, last: &str, female: bool, age: f32, home: u32) -> u32 {
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
    fn far_tile(&mut self, vi: usize, dist: i64) -> (u32, u32) {
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
        let (x, y) = self.far_tile(vi, 30);
        let night = day_part(self.clock().hour) == DayPart::Night;
        let width = self.map.width;
        let p = &mut self.people[i];
        p.brain = Default::default();
        p.brain.leaving = true;
        p.brain.carry = Some(crate::people::Carry::Bundle);
        p.brain.plan.extend([Step::Go(y * width + x), Step::Leave]);
        if night {
            p.brain.act = Act::Leaving;
        }
    }

    /// Disette : une famille s'en va, et parfois ses meilleurs amis avec elle.
    fn famine_leave(&mut self, vi: usize) {
        let id = self.villages[vi].id;
        let homes: Vec<u32> = self.villages[vi]
            .houses
            .iter()
            .filter(|h| h.state == HouseState::Inhabited && !h.residents.is_empty())
            .map(|h| h.id)
            .collect();
        let Some(&hid) = self.rng.pick(&homes) else { return };
        let h = self.villages[vi].house(hid).unwrap();
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
                .friends
                .iter()
                .filter(|&(&f, &s)| {
                    s >= 20.0 && self.people[f as usize].alive() && !self.people[f as usize].brain.leaving
                })
                .max_by(|a, b| a.1.total_cmp(b.1))
                .map(|(&f, _)| f);
            if let Some(f) = best
                && self.rng.chance(0.35)
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
                )
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
        let st = &mut self.villages[vi].stock;
        st.food = st.food.max(60.0);
        let text = format!(
            "Des pionniers s'installent : {} et {} {last}",
            self.people[a as usize].first, self.people[b as usize].first
        );
        self.event(Some(id), vec![a, b], text);
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
    }
}

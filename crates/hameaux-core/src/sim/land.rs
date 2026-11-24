//! Terres au quotidien (phase 9) : les mauvaises herbes poussent dans les champs semés, les
//! friches se reposent, le gibier et le poisson se reconstituent.

use std::collections::BTreeMap;

use crate::{land::FALLOW_REST, people::Job, settlement::FieldState, world::World};

/// Champs au plus par fermier (au-delà, ils restent au village).
const FIELDS_PER_FARMER: usize = 4;

impl World {
    /// Une fois par jour, à minuit.
    pub(super) fn daily_land(&mut self) {
        let clock = self.clock();
        let growth = clock.season != crate::clock::Season::Winter;
        if clock.season == crate::clock::Season::Spring && clock.day == 1 {
            self.new_year();
        }
        if super::tick_day(self.tick).is_multiple_of(7) {
            self.levy();
        }
        for vi in 0..self.villages.len() {
            for f in &self.villages[vi].fields {
                let soil = self.land.soil_mut(f.id);
                match f.state {
                    FieldState::Sown if growth => {
                        soil.weeds = (soil.weeds + crate::land::WEEDS_PER_DAY).min(1.0);
                    }
                    FieldState::Fallow | FieldState::Ploughed if soil.richness < 1.0 => {
                        soil.richness = (soil.richness + FALLOW_REST).min(1.0);
                    }
                    _ => {}
                }
            }
            self.land.wild_mut(vi).regrow();
            self.assign_fields(vi);
        }
    }

    /// Chaque champ est cultivé par un foyer de fermiers ; quand le foyer n'a plus de fermier
    /// (mort, départ, changement de métier), son champ revient au village, qui le confie au
    /// foyer de fermiers qui en a le moins.
    fn assign_fields(&mut self, vi: usize) {
        let tick = self.tick;
        let id = self.villages[vi].id;
        let mut farmers: BTreeMap<u32, usize> = BTreeMap::new();
        for p in self.alive().filter(|p| {
            p.village == id
                && p.job == Some(Job::Farmer)
                && !p.child(tick)
                && !p.elder(tick)
                && !p.brain.leaving
        }) {
            if let Some(h) = p.home {
                *farmers.entry(h).or_default() += 1;
            }
        }
        let mut held: BTreeMap<u32, usize> = farmers.keys().map(|&h| (h, 0)).collect();
        let mut free = Vec::new();
        for f in &self.villages[vi].fields {
            let soil = self.land.soil_mut(f.id);
            match soil.owner.and_then(|o| held.get_mut(&o)) {
                Some(n) => *n += 1,
                None => {
                    soil.owner = None;
                    free.push(f.id);
                }
            }
        }
        for fid in free {
            let best = held
                .iter()
                .filter(|&(h, &n)| n < FIELDS_PER_FARMER * farmers[h])
                .min_by(|a, b| {
                    let load = |(h, n): (&u32, &usize)| *n as f32 / farmers[h] as f32;
                    load(*a).total_cmp(&load(*b))
                })
                .map(|(&h, _)| h);
            let Some(h) = best else { break };
            *held.get_mut(&h).unwrap() += 1;
            self.land.soil_mut(fid).owner = Some(h);
        }
    }
}

impl World {
    /// Au premier jour du printemps, la taille repart de zéro et l'année s'ouvre : ce que
    /// promet la récolte dépend désormais du temps qu'il fera (voir `sim/weather.rs`), et
    /// chaque village le saura à la fin du printemps.
    fn new_year(&mut self) {
        for vi in 0..self.villages.len() {
            *self.land.year_mut(vi) = crate::land::Year::default();
        }
    }

    /// Chaque semaine, la taille : les foyers aisés versent à la caisse une part de ce qu'ils
    /// ont au-delà d'un seuil (selon la taxe fixée par le chef). C'est elle qui paie les
    /// journaliers, les bâtisseurs et le grenier commun ; quand la caisse dépasse sa réserve,
    /// le village fait en plus l'aumône aux foyers les plus pauvres.
    fn levy(&mut self) {
        use crate::land::{LEVY_FLOOR, LEVY_PER_HEAD, LEVY_RATE, RESERVE, RESERVE_PER_HEAD};
        for vi in 0..self.villages.len() {
            self.economy.ensure_village(vi);
            let homes = self.homes(vi);
            let pop: usize = homes.iter().map(|(_, m)| m.len()).sum();
            let reserve = RESERVE + RESERVE_PER_HEAD * pop as f32;
            let rate = (self.society.tax(self.villages[vi].id) * LEVY_RATE).min(0.5);
            let mut total = 0.0;
            for (hid, members) in &homes {
                let h = self.economy.household(*hid);
                let excess = h.coins - LEVY_FLOOR - LEVY_PER_HEAD * members.len() as f32;
                if excess > 0.0 {
                    let part = excess * rate;
                    h.coins -= part;
                    total += part;
                }
            }
            self.economy.treasury[vi] += total;
            self.land.year_mut(vi).levied += total;
            let treasury = self.economy.treasury[vi];
            if treasury < reserve {
                continue;
            }
            // L'aumône : la moitié de ce qui dépasse la réserve, aux foyers qui n'ont presque
            // plus rien, selon leurs bouches.
            let mut poor: Vec<(u32, usize)> = homes
                .iter()
                .filter(|(h, m)| {
                    self.economy
                        .households
                        .get(h)
                        .is_some_and(|hh| hh.coins < 2.0 * m.len() as f32)
                })
                .map(|(h, m)| (*h, m.len()))
                .collect();
            if poor.is_empty() {
                continue;
            }
            poor.sort();
            let mouths: usize = poor.iter().map(|p| p.1).sum();
            let purse = ((treasury - reserve) * 0.5).min(mouths as f32 * 4.0);
            for (h, n) in &poor {
                self.economy.household(*h).coins += purse * *n as f32 / mouths as f32;
            }
            self.economy.treasury[vi] -= purse;
            let year = self.land.year_mut(vi);
            let first = year.alms == 0.0;
            year.alms += purse;
            if first && purse >= 5.0 {
                let (id, name) = (self.villages[vi].id, self.villages[vi].name.clone());
                let n = poor.len();
                let s = if n > 1 { "s" } else { "" };
                self.event(
                    Some(id),
                    vec![],
                    format!(
                        "La caisse de {name} fait l'aumône à {n} foyer{s} dans le besoin ({purse:.0} sous)"
                    ),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{economy::Good, people::Carry, people::Job, save::tests::sample};

    /// Chaque champ revient à un foyer de fermiers ; celui qui aide à moissonner le champ d'un
    /// autre garde sa part, le reste va au foyer du champ.
    #[test]
    fn fields_have_owners_and_reapers_keep_a_share() {
        let mut w = sample();
        let tick = w.tick;
        let adults: Vec<usize> = (0..w.people.len())
            .filter(|&i| w.people[i].alive() && !w.people[i].child(tick) && !w.people[i].elder(tick))
            .collect();
        let farmer = adults[0];
        let home = w.people[farmer].home.unwrap();
        let helper = *adults.iter().find(|&&i| w.people[i].home != Some(home)).unwrap();
        for &i in &adults {
            w.people[i].job = Some(if i == farmer { Job::Farmer } else { Job::Woodcutter });
        }
        w.assign_fields(0);
        let fid = w.villages[0].fields[0].id;
        assert_eq!(w.land.soil(fid).owner, Some(home));

        let helper_home = w.people[helper].home.unwrap();
        let before = (
            w.economy.household(home).inv[Good::Grain as usize],
            w.economy.household(helper_home).inv[Good::Grain as usize],
        );
        let b = &mut w.people[helper].brain;
        b.carry = Some(Carry::Good(Good::Grain));
        b.carry_amount = 40.0;
        w.bring_harvest(helper, 0, fid);
        let mine = w.economy.household(helper_home).inv[Good::Grain as usize] - before.1;
        let theirs = w.economy.household(home).inv[Good::Grain as usize] - before.0;
        assert!((mine - 40.0 * crate::land::REAPER_SHARE).abs() < 1e-3, "{mine}");
        assert!(theirs > 20.0 && theirs < 30.0, "{theirs}");
    }
}

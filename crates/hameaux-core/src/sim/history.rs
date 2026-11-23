//! Relevés quotidiens des villages et annales (phase 7).

use std::sync::Arc;

use crate::{
    economy::{GOODS, Good},
    history::{Kind, Sample},
    people::Status,
    settlement::HouseState,
    world::World,
};

use super::tick_day;

impl World {
    /// Monde repris d'une sauvegarde sans histoire : un premier relevé tout de suite, et les
    /// événements marquants de la chronique entrent dans les annales.
    pub(crate) fn bootstrap_history(&mut self) {
        let h = Arc::make_mut(&mut self.history);
        h.last_tick = self.tick;
        self.record_history();
    }

    /// À minuit : un relevé par village, et les événements marquants du jour aux annales.
    pub(super) fn record_history(&mut self) {
        let since = self.history.last_tick;
        let seen = self.history.annals_seen;
        let tick = self.tick;
        let day = tick_day(tick) as u32;
        let samples: Vec<(u32, Sample)> = (0..self.villages.len())
            .map(|vi| (self.villages[vi].id, self.sample(vi, since, seen, day)))
            .collect();
        let fresh: Vec<_> = self.chronicle.since(seen).cloned().collect();
        let next = self.chronicle.next_seq;
        let h = Arc::make_mut(&mut self.history);
        for (id, s) in samples {
            h.villages.entry(id).or_default().record(s);
        }
        for e in &fresh {
            h.note(e);
        }
        h.annals_seen = next;
        h.last_tick = tick;
    }

    fn sample(&self, vi: usize, since: u64, seen: u64, day: u32) -> Sample {
        let tick = self.tick;
        let v = &self.villages[vi];
        let id = v.id;
        let (mut population, mut children, mut elders) = (0u16, 0u16, 0u16);
        let (mut births, mut deaths, mut departures, mut sick) = (0u16, 0u16, 0u16, 0u16);
        let mut need = 0.0f32;
        for p in self.people.iter().filter(|p| p.village == id) {
            match p.status {
                Status::Alive if !p.brain.leaving => {
                    population += 1;
                    if p.child(tick) {
                        children += 1;
                        need += 0.6;
                    } else {
                        need += 1.0;
                    }
                    if p.elder(tick) {
                        elders += 1;
                    }
                    if self.society.sick.contains_key(&p.id) {
                        sick += 1;
                    }
                }
                Status::Dead { tick: t } if t > since => deaths += 1,
                Status::Left { tick: t } if t > since => departures += 1,
                _ => {}
            }
            if p.born > since as i64 && p.born <= tick as i64 && p.parents.len() == 2 {
                births += 1;
            }
        }
        let arrivals = self
            .chronicle
            .since(seen)
            .filter(|e| e.village == Some(id) && Kind::of(&e.text) == Kind::Family)
            .filter(|e| e.text.contains(" arrive"))
            .count() as u16;
        let mut stock = [0.0f32; GOODS];
        let mut coins = 0.0;
        let mut rations = v.stock.food;
        let mut houses = 0u16;
        for hs in &v.houses {
            if hs.state == HouseState::Inhabited && !hs.residents.is_empty() {
                houses += 1;
            }
            if let Some(h) = self.economy.households.get(&hs.id) {
                for (s, q) in stock.iter_mut().zip(h.inv) {
                    *s += q;
                }
                coins += h.coins;
                rations += h.rations();
            }
        }
        stock[Good::Grain as usize] += v.stock.food / Good::Grain.ration();
        stock[Good::Wood as usize] += v.stock.wood as f32;
        stock[Good::Stone as usize] += v.stock.stone as f32;
        let market = self.economy.markets.get(vi);
        Sample {
            day,
            population,
            children,
            elders,
            births,
            deaths,
            departures,
            arrivals,
            houses,
            sick,
            food_days: if need > 0.0 { rations / need } else { 0.0 },
            treasury: self.economy.treasury.get(vi).copied().unwrap_or(0.0),
            coins,
            tax: self.society.tax(id),
            stock,
            price: market.map_or([0.0; GOODS], |m| m.price),
            volume: market.map_or([0.0; GOODS], |m| m.volume),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::save::tests::sample;

    #[test]
    fn a_sample_every_midnight() {
        let mut w = sample();
        let id = w.villages[0].id;
        for _ in 0..3 * crate::tuning::DAY {
            w.step();
        }
        let h = &w.history.villages[&id];
        assert_eq!(h.days.len(), 3);
        let last = h.days.back().unwrap();
        assert_eq!(last.population as usize, w.population(id));
        assert!(last.food_days > 0.0);
        assert!(last.price.iter().all(|&p| p > 0.0));
        assert!(
            h.days
                .iter()
                .zip(h.days.iter().skip(1))
                .all(|(a, b)| a.day + 1 == b.day)
        );
    }
}

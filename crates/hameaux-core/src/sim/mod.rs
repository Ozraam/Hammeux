//! Avancement du monde, tick par tick.
//!
//! Ordre d'un tick : changements d'heure (lever, travail, soir, nuit), repas et vie des
//! familles une fois par jour, planification des villages toutes les 10 minutes,
//! amitiés chaque minute, pousse des champs, puis chaque habitant agit.

mod found;
mod god;
mod history;
mod land;
mod life;
mod market;
mod mind;
mod society;
mod tasks;
mod trade;

pub use mind::{INTENT_LABELS, LEVEL_LABELS, MindStats, skill_level};

pub(crate) use found::is_quarry;
pub use life::{at_place, de, of_place};

use crate::{clock::Season, settlement::FieldState, tuning::*, world::World};

/// Moments de la journée.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayPart {
    Night,
    Morning,
    Work,
    Evening,
}

pub fn day_part(hour: u64) -> DayPart {
    match hour {
        6 => DayPart::Morning,
        7..=17 => DayPart::Work,
        18..=20 => DayPart::Evening,
        _ => DayPart::Night,
    }
}

/// Le septième jour de chaque semaine, on ne travaille pas.
pub fn rest_day(tick: u64) -> bool {
    tick_day(tick) % 7 == 6
}

/// Numéro du jour (depuis le début du monde) d'un tick.
pub fn tick_day(tick: u64) -> u64 {
    (tick * crate::clock::TICK_SECONDS + crate::clock::START_HOUR * 3600) / crate::clock::SECONDS_PER_DAY
}

impl World {
    /// Avance le monde d'un tick (10 s de jeu).
    pub fn step(&mut self) {
        self.ensure_caches();
        self.tick += 1;
        let clock = self.clock();
        if self.tick.is_multiple_of(HOUR) {
            self.god_hourly();
            match clock.hour {
                6 => self.wake_up(),
                7 | 18 | 21 => self.reset_plans(),
                11 => self.market_day(),
                12 => self.meals(),
                0 => {
                    if clock.season == Season::Winter && clock.day == 1 {
                        self.winter_rot();
                    }
                    self.update_incomes();
                    self.reconsider_jobs();
                    self.daily();
                    self.daily_minds();
                    self.daily_trade();
                    self.daily_society();
                    self.daily_god();
                    self.daily_land();
                    self.record_history();
                }
                _ => {}
            }
        }
        if self.tick.is_multiple_of(10 * MINUTE) {
            for vi in 0..self.villages.len() {
                self.planner(vi);
            }
            self.minds_tick();
        }
        if self.tick.is_multiple_of(MINUTE) {
            self.social();
        }
        self.grow_fields(clock.season);
        for i in 0..self.people.len() {
            if self.people[i].alive() {
                self.update_person(i);
            }
        }
    }

    pub(crate) fn event(&mut self, village: Option<u32>, people: Vec<u32>, text: String) {
        self.chronicle.push(self.tick, village, people, text);
    }

    /// Après chargement : plans à refaire ; la nuit, chacun dort chez soi.
    pub(crate) fn after_load(&mut self) {
        let night = day_part(self.clock().hour) == DayPart::Night;
        for i in 0..self.people.len() {
            if !self.people[i].alive() {
                continue;
            }
            self.people[i].brain = Default::default();
            // Un marchand en voyage reprend sa route (ou dort à l'auberge) : voir `trader_assign`.
            if night && !self.away(i) {
                self.send_home(i);
            }
        }
    }

    fn home_tile(&self, i: usize) -> Option<(u32, u32)> {
        let p = &self.people[i];
        let vi = self.village_index(p.village)?;
        let h = self.villages[vi].house(p.home?)?;
        Some((h.x, h.y))
    }

    /// Place l'habitant chez lui, endormi.
    fn send_home(&mut self, i: usize) {
        if let Some((x, y)) = self.home_tile(i) {
            let p = &mut self.people[i];
            p.x = x as f32 + 0.5;
            p.y = y as f32 + 0.5;
            p.brain.hidden = true;
            p.brain.act = crate::people::Act::Sleeping;
        }
    }

    fn wake_up(&mut self) {
        for i in 0..self.people.len() {
            let p = &mut self.people[i];
            if p.alive() && p.brain.hidden {
                p.brain = Default::default();
            }
        }
    }

    fn reset_plans(&mut self) {
        for p in self
            .people
            .iter_mut()
            .filter(|p| p.alive() && !p.brain.leaving && !p.brain.hidden && !p.brain.traveling)
        {
            p.brain.plan.clear();
            p.brain.path.clear();
            p.brain.path_pos = 0;
            p.brain.wait = 0;
            p.brain.carry = None;
        }
    }

    /// Début de l'hiver : ce qui n'a pas été moissonné pourrit sur pied.
    fn winter_rot(&mut self) {
        for vi in 0..self.villages.len() {
            let mut lost = 0;
            for f in &mut self.villages[vi].fields {
                if f.state == FieldState::Ripe {
                    f.state = FieldState::Fallow;
                    f.work = 0.0;
                    f.growth = 0.0;
                    lost += 1;
                }
            }
            if lost > 0 {
                let (id, name) = (self.villages[vi].id, self.villages[vi].name.clone());
                let s = if lost > 1 { "s" } else { "" };
                self.event(
                    Some(id),
                    vec![],
                    format!("À {name}, {lost} champ{s} non moissonné{s} pourri{s} sur pied"),
                );
            }
        }
    }

    fn grow_fields(&mut self, season: Season) {
        let rate = match season {
            Season::Spring => 1.0,
            Season::Summer => 1.25,
            Season::Autumn => 0.8,
            Season::Winter => 0.0,
        } / (FIELD_GROW_DAYS * DAY as f32);
        if rate <= 0.0 {
            return;
        }
        for vi in 0..self.villages.len() {
            // Pluie et sécheresse du mode dieu.
            let rate = rate * self.growth_factor(self.villages[vi].id);
            for f in &mut self.villages[vi].fields {
                if f.state == FieldState::Sown && rate > 0.0 {
                    f.growth += rate;
                    if f.growth >= 1.0 {
                        f.state = FieldState::Ripe;
                        f.growth = 1.0;
                        f.work = 0.0;
                    }
                }
            }
        }
    }

    /// Amitiés : ceux qui se croisent ou travaillent ensemble se rapprochent (les sociables et
    /// les gentils plus vite), et se sentent moins seuls.
    fn social(&mut self) {
        for v in 0..self.villages.len() {
            let id = self.villages[v].id;
            let here: Vec<usize> = (0..self.people.len())
                .filter(|&i| {
                    let p = &self.people[i];
                    p.alive() && p.village == id && !p.brain.hidden && !p.brain.leaving
                })
                .collect();
            for a in 0..here.len() {
                for b in a + 1..here.len() {
                    let (i, j) = (here[a], here[b]);
                    let (pi, pj) = (&self.people[i], &self.people[j]);
                    if (pi.x - pj.x).abs() + (pi.y - pj.y).abs() > 1.5 {
                        continue;
                    }
                    let (ii, jj) = (pi.id, pj.id);
                    let (ti, tj) = (pi.mind.traits, pj.mind.traits);
                    for (k, other, gain) in [
                        (i, jj, 0.02 + 0.04 * ti.sociability + 0.03 * tj.kindness),
                        (j, ii, 0.02 + 0.04 * tj.sociability + 0.03 * ti.kindness),
                    ] {
                        let m = &mut self.people[k].mind;
                        let r = m.rel_mut(other);
                        r.affection = (r.affection + gain * (1.0 - r.affection / 120.0)).min(100.0);
                        m.needs.company = (m.needs.company + 0.001).min(1.0);
                    }
                }
            }
        }
    }
}

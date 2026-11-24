//! Société : élections du chef et du conseil, taxes, justice, fêtes et foires, maladies,
//! découverte et diffusion des savoirs.

use std::collections::BTreeMap;

use crate::{
    clock::Season,
    economy::Good,
    mind::{Emotion, MemoryKind, Traits},
    people::{Act, Job},
    society::{Crime, DISEASES, Disease, Known, Offence, Sickness, Tech},
    tuning::*,
    world::World,
};

use super::{
    life::{at_place, of_place},
    rest_day, tick_day,
};

/// Durée du mandat d'un chef.
const TERM: u64 = 2 * YEAR;
/// Population à partir de laquelle un village tient une foire chaque été.
const FAIR_POP: usize = 30;
/// Jours entre l'annonce de la foire et son ouverture.
const FAIR_NOTICE: u64 = 5;
/// Taxe au plus bas et au plus haut.
const TAX_MIN: f32 = 0.05;
const TAX_MAX: f32 = 0.20;

/// Taxe que fixe un chef selon son caractère : les ambitieux et les regardants taxent plus,
/// les gentils moins.
pub(crate) fn chief_tax(t: Traits) -> f32 {
    (0.06 + 0.08 * t.ambition + 0.05 * (1.0 - t.generosity) - 0.03 * t.kindness).clamp(TAX_MIN, 0.18)
}

fn pct(t: f32) -> String {
    format!("{:.0} %", t * 100.0)
}

impl World {
    /// Reprise d'une sauvegarde sans société : chaque village élira son chef au prochain
    /// minuit.
    pub(crate) fn bootstrap_society(&mut self) {
        for v in &self.villages {
            self.society.polities.entry(v.id).or_default();
        }
    }

    /// Une fois par jour (minuit) : élections, jugements, fêtes et foires, maladies, inventions.
    pub(super) fn daily_society(&mut self) {
        let rep = self.reputations();
        let today = tick_day(self.tick);
        for vi in 0..self.villages.len() {
            let id = self.villages[vi].id;
            self.society.polity_mut(id);
            self.politics(vi, &rep);
            self.greed(vi);
            self.judge(vi);
            self.festivities(vi, today);
        }
        self.diseases();
        self.inventions();
    }

    // ------------------------------------------------------------------ chef et conseil

    /// Adultes d'un village (marchands en voyage compris).
    fn adults_of(&self, village: u32) -> Vec<usize> {
        let tick = self.tick;
        (0..self.people.len())
            .filter(|&i| {
                let p = &self.people[i];
                p.alive() && p.village == village && !p.brain.leaving && !p.child(tick)
            })
            .collect()
    }

    fn still_here(&self, village: u32, who: u32) -> bool {
        let p = &self.people[who as usize];
        p.alive() && p.village == village && !p.brain.leaving
    }

    /// Le chef meurt, part ou perd la confiance du village : on vote. Sinon, chaque saison, il
    /// revoit la taxe selon l'état de la caisse.
    fn politics(&mut self, vi: usize, rep: &BTreeMap<u32, f32>) {
        let tick = self.tick;
        let id = self.villages[vi].id;
        let adults = self.adults_of(id);
        let (chief, next) = {
            let p = self.society.polity(id).unwrap();
            (p.chief, p.next_election)
        };
        let council: Vec<u32> = self.society.polity(id).unwrap().council.clone();
        let council: Vec<u32> = council.into_iter().filter(|&c| self.still_here(id, c)).collect();
        self.society.polity_mut(id).council = council;
        let chief_ok = chief.is_some_and(|c| self.still_here(id, c));
        if adults.len() < 3 {
            if !chief_ok {
                self.society.polity_mut(id).chief = None;
            }
            return;
        }
        let mut due = !chief_ok || tick >= next;
        // Un chef mal vu est destitué par le conseil (ou par l'assemblée des anciens).
        if chief_ok && !due {
            let c = chief.unwrap();
            if rep.get(&c).copied().unwrap_or(0.0) < -6.0 && self.rng.chance(0.2) {
                let name = self.villages[vi].name.clone();
                let first = self.people[c as usize].name();
                self.event(
                    Some(id),
                    vec![c],
                    format!(
                        "Le village {}, mécontent, destitue son chef {first}",
                        of_place(&name)
                    ),
                );
                self.feel(c as usize, MemoryKind::Judged, None, -0.6, Emotion::Anger, 0.5);
                let n = &mut self.people[c as usize].mind.needs;
                n.esteem = (n.esteem - 0.3).max(0.0);
                self.society.stats.deposed += 1;
                self.society.polity_mut(id).chief = None;
                due = true;
            }
        }
        if due {
            self.election(vi, &adults, rep);
        } else if self.clock().day == 1 && self.clock().hour == 0 {
            self.review_tax(vi);
        }
    }

    /// Élection : les mieux considérés (et les ambitieux) se présentent ; chacun vote pour
    /// celui qu'il estime le plus (sa famille d'abord). Les suivants forment le conseil.
    fn election(&mut self, vi: usize, adults: &[usize], rep: &BTreeMap<u32, f32>) {
        let tick = self.tick;
        let id = self.villages[vi].id;
        let mut field: Vec<(f32, u32)> = adults
            .iter()
            .map(|&i| &self.people[i])
            .filter(|p| p.age(tick) >= 25.0 && self.society.convictions.get(&p.id).copied().unwrap_or(0) < 2)
            .map(|p| {
                let t = p.mind.traits;
                let r = rep.get(&p.id).copied().unwrap_or(0.0);
                (r + 25.0 * t.ambition + 10.0 * t.sociability, p.id)
            })
            .collect();
        field.sort_by(|a, b| b.0.total_cmp(&a.0));
        field.truncate(4);
        if field.is_empty() {
            return;
        }
        let candidates: Vec<u32> = field.iter().map(|c| c.1).collect();
        let mut votes: BTreeMap<u32, u32> = candidates.iter().map(|&c| (c, 0)).collect();
        for &v in adults {
            let family = self.family_of(v);
            let me = self.people[v].id;
            let mut best: Option<(f32, u32)> = None;
            for &c in &candidates {
                let s = if c == me {
                    60.0
                } else {
                    self.people[v].mind.rel(c).opinion() + if family.contains(&c) { 25.0 } else { 0.0 }
                } + self.rng.f32() * 6.0;
                if best.is_none_or(|b| s > b.0) {
                    best = Some((s, c));
                }
            }
            if let Some((_, c)) = best {
                *votes.get_mut(&c).unwrap() += 1;
            }
        }
        let mut ranked: Vec<(u32, u32)> = candidates.iter().map(|&c| (votes[&c], c)).collect();
        // À égalité, le plus estimé l'emporte (ordre des candidats).
        ranked.sort_by_key(|&(n, _)| std::cmp::Reverse(n));
        let (won, chief) = ranked[0];
        let seats = (adults.len() / 10).min(3);
        let council: Vec<u32> = ranked.iter().skip(1).take(seats).map(|&(_, c)| c).collect();
        let old = self.society.polity(id).and_then(|p| p.chief);
        let old_tax = self.society.tax(id);
        let tax = chief_tax(self.people[chief as usize].mind.traits);
        let first_ever = self.society.polity(id).is_some_and(|p| p.chiefs == 0);
        {
            let p = self.society.polity_mut(id);
            if old != Some(chief) {
                p.elected = tick;
                p.chiefs += 1;
            }
            p.chief = Some(chief);
            p.council = council;
            p.next_election = tick + TERM;
            p.tax = tax;
        }
        self.society.stats.elections += 1;
        let name = self.villages[vi].name.clone();
        let c = &self.people[chief as usize];
        let (who, e) = (c.name(), c.e());
        let total = adults.len();
        let of = of_place(&name);
        let female = self.people[chief as usize].female;
        let (title, first) = if female {
            ("cheffe", "la première cheffe")
        } else {
            ("chef", "le premier chef")
        };
        let mut text = if old == Some(chief) {
            format!("{who} est réélu{e} {title} {of} ({won} voix sur {total})")
        } else if first_ever {
            format!("{who} devient {first} {of} ({won} voix sur {total})")
        } else {
            format!("{who} est élu{e} {title} {of} ({won} voix sur {total})")
        };
        if (tax - old_tax).abs() >= 0.01 {
            text += &format!(" et fixe la taxe à {}", pct(tax));
        }
        let mut people = vec![chief];
        if let Some(o) = old.filter(|&o| o != chief) {
            people.push(o);
            if self.still_here(id, o) {
                self.people[o as usize].mind.mood.add(Emotion::Sadness, 0.2);
                let n = &mut self.people[o as usize].mind.needs;
                n.esteem = (n.esteem - 0.15).max(0.0);
            }
        }
        self.event(Some(id), people, text);
        if old != Some(chief) {
            self.feel(chief as usize, MemoryKind::Elected, None, 0.85, Emotion::Joy, 0.5);
            let n = &mut self.people[chief as usize].mind.needs;
            n.esteem = (n.esteem + 0.3).min(1.0);
            n.meaning = (n.meaning + 0.2).min(1.0);
        }
    }

    /// Au début de chaque saison, le chef revoit la taxe : il la monte quand la caisse ne peut
    /// plus remplir le grenier commun (ce que les regardants n'aiment pas), la baisse quand elle
    /// déborde, et sinon la ramène peu à peu vers ce qu'il juge juste.
    fn review_tax(&mut self, vi: usize) {
        let id = self.villages[vi].id;
        let Some(chief) = self.society.polity(id).and_then(|p| p.chief) else {
            return;
        };
        let pop = self.population(id).max(1) as f32;
        let treasury = self.economy.treasury.get(vi).copied().unwrap_or(0.0);
        let granary = self.villages[vi].stock.food;
        let tax = self.society.tax(id);
        let fair = chief_tax(self.people[chief as usize].mind.traits);
        let new = if treasury < pop && granary < 2.0 * pop && tax < TAX_MAX - 0.001 {
            (tax + 0.02).min(TAX_MAX)
        } else if (treasury > 25.0 * pop || (treasury > 6.0 * pop && tax > fair + 0.005))
            && tax > TAX_MIN + 0.001
        {
            (tax - 0.02).max(TAX_MIN)
        } else {
            return;
        };
        self.society.polity_mut(id).tax = new;
        let (who, name) = (
            self.people[chief as usize].first.clone(),
            self.villages[vi].name.clone(),
        );
        let verb = if new < tax { "baisse" } else { "augmente" };
        self.event(
            Some(id),
            vec![chief],
            format!("{who} {verb} la taxe {} à {}", of_place(&name), pct(new)),
        );
        for i in self.adults_of(id) {
            if self.people[i].id == chief {
                continue;
            }
            let g = self.people[i].mind.traits.generosity;
            let r = self.people[i].mind.rel_mut(chief);
            if new < tax {
                r.affection += 2.0;
                r.respect += 1.0;
            } else {
                r.affection -= 3.0 * (1.2 - g);
                r.respect -= 1.0;
            }
            r.clamp();
        }
    }

    // ------------------------------------------------------------------ justice

    /// Une plainte est déposée : le chef jugera.
    pub(crate) fn complain(&mut self, crime: Crime, offender: u32, victim: u32, value: f32) {
        let tick = self.tick;
        let village = self.people[offender as usize].village;
        match crime {
            Crime::Theft => self.society.stats.thefts += 1,
            Crime::Brawl => self.society.stats.brawls += 1,
        }
        self.society.polity_mut(village).offences.push(Offence {
            crime,
            offender,
            victim,
            tick,
            value,
        });
    }

    /// Un affamé que personne n'a aidé vole des provisions chez un voisin mieux pourvu.
    /// Renvoie les rations volées (déjà dans le foyer du voleur).
    pub(super) fn steal_food(&mut self, vi: usize, hid: u32, members: &[usize], missing: f32) -> f32 {
        let tick = self.tick;
        let Some(&thief) = members
            .iter()
            .filter(|&&m| !self.people[m].child(tick))
            .min_by(|&&a, &&b| {
                self.people[a]
                    .mind
                    .traits
                    .kindness
                    .total_cmp(&self.people[b].mind.traits.kindness)
            })
        else {
            return 0.0;
        };
        let t = self.people[thief].mind.traits;
        if !self.rng.chance(0.35 * (1.0 - t.kindness) * (1.0 - 0.5 * t.rigor)) {
            return 0.0;
        }
        let homes: Vec<u32> = self.villages[vi]
            .houses
            .iter()
            .filter(|h| h.id != hid && !h.residents.is_empty())
            .filter(|h| {
                self.economy
                    .households
                    .get(&h.id)
                    .is_some_and(|hh| hh.rations() >= 20.0)
            })
            .map(|h| h.id)
            .collect();
        let Some(&target) = self.rng.pick(&homes) else {
            return 0.0;
        };
        let price = self.economy.markets.get(vi).map(|m| m.price).unwrap_or_default();
        let want = missing * 2.0;
        let (mut took, mut value) = (0.0, 0.0);
        let mut moved = [0.0f32; crate::economy::GOODS];
        let h = self.economy.household(target);
        let spare = h.rations() * 0.3;
        for &g in &Good::FOODS {
            let gi = g as usize;
            let units = h.inv[gi].min((want.min(spare) - took) / g.ration());
            if units <= 0.0 {
                continue;
            }
            h.inv[gi] -= units;
            moved[gi] = units;
            took += units * g.ration();
            value += units * price[gi];
        }
        if took < 0.5 {
            return 0.0;
        }
        let mine = self.economy.household(hid);
        for (gi, u) in moved.iter().enumerate() {
            mine.inv[gi] += u;
        }
        self.theft_found(vi, thief, target, value, "des provisions");
        took
    }

    /// Les pauvres sans scrupules volent parfois de l'argent chez les riches.
    fn greed(&mut self, vi: usize) {
        let tick = self.tick;
        let id = self.villages[vi].id;
        for i in self.adults_of(id) {
            let p = &self.people[i];
            let t = p.mind.traits;
            if p.elder(tick) || self.away(i) || self.home_household(i).is_none_or(|h| h.coins >= 4.0) {
                continue;
            }
            if !self
                .rng
                .chance(0.004 * (1.0 - t.kindness) * (1.0 - t.generosity) * (0.5 + t.ambition))
            {
                continue;
            }
            let home = p.home;
            let rich: Vec<u32> = self.villages[vi]
                .houses
                .iter()
                .filter(|h| Some(h.id) != home && !h.residents.is_empty())
                .filter(|h| {
                    self.economy
                        .households
                        .get(&h.id)
                        .is_some_and(|hh| hh.coins >= 40.0)
                })
                .map(|h| h.id)
                .collect();
            let Some(&target) = self.rng.pick(&rich) else {
                continue;
            };
            let h = self.economy.household(target);
            let amount = (h.coins * 0.2).min(12.0);
            h.coins -= amount;
            if let Some(mine) = home {
                self.economy.household(mine).coins += amount;
            }
            self.theft_found(vi, i, target, amount, &format!("{amount:.0} sous"));
        }
    }

    /// Un vol est (ou n'est pas) découvert.
    fn theft_found(&mut self, vi: usize, thief: usize, target: u32, value: f32, what: &str) {
        let tick = self.tick;
        let id = self.villages[vi].id;
        let Some(h) = self.villages[vi].house(target) else {
            return;
        };
        let family = h.family.clone();
        let victim = h
            .residents
            .iter()
            .copied()
            .find(|&r| !self.people[r as usize].child(tick))
            .or(h.residents.first().copied());
        let Some(victim) = victim else { return };
        let watch = self.people[victim as usize].mind.traits.rigor;
        if !self.rng.chance(0.5 + 0.3 * watch) {
            self.event(Some(id), vec![], format!("Chez les {family}, {what} ont disparu"));
            return;
        }
        let tid = self.people[thief].id;
        self.feel(
            victim as usize,
            MemoryKind::Robbed,
            Some(tid),
            -0.55,
            Emotion::Anger,
            0.35,
        );
        let r = self.people[victim as usize].mind.rel_mut(tid);
        r.rivalry += 20.0;
        r.trust -= 30.0;
        r.affection -= 15.0;
        r.clamp();
        let name = self.people[thief].first.clone();
        self.event(
            Some(id),
            vec![tid, victim],
            format!(
                "{name} est pris{} à voler {what} chez les {family}",
                self.people[thief].e()
            ),
        );
        self.complain(Crime::Theft, tid, victim, value);
    }

    /// Une dispute qui tourne mal (deux colères, une vieille rivalité) finit en bagarre.
    pub(super) fn maybe_brawl(&mut self, i: usize, j: usize) {
        let tick = self.tick;
        let (pi, pj) = (&self.people[i], &self.people[j]);
        if pi.child(tick) || pj.child(tick) {
            return;
        }
        let rivalry = pi.mind.rel(pj.id).rivalry.max(pj.mind.rel(pi.id).rivalry);
        let anger = pi.mind.mood.anger + pj.mind.mood.anger;
        if rivalry < 45.0 || anger < 0.9 || !self.rng.chance(0.2 + 0.3 * (1.0 - pi.mind.traits.kindness)) {
            return;
        }
        let (idi, idj) = (pi.id, pj.id);
        for (a, b) in [(i, idj), (j, idi)] {
            self.feel(a, MemoryKind::Fight, Some(b), -0.5, Emotion::Anger, 0.3);
            let n = &mut self.people[a].mind.needs;
            n.safety = (n.safety - 0.25).max(0.0);
        }
        let text = format!(
            "{} et {} en viennent aux mains",
            self.people[i].first, self.people[j].first
        );
        let village = self.people[i].village;
        self.event(Some(village), vec![idi, idj], text);
        // Celui qui a commencé passera devant le chef.
        self.complain(Crime::Brawl, idi, idj, 0.0);
    }

    /// Le chef (ou un conseiller, s'il est partie prenante) juge les plaintes du jour : pardon,
    /// amende ou bannissement, selon sa rigueur, sa bonté, la récidive… et ce qu'il pense de
    /// chacun.
    fn judge(&mut self, vi: usize) {
        let tick = self.tick;
        let id = self.villages[vi].id;
        let offences = std::mem::take(&mut self.society.polity_mut(id).offences);
        if offences.is_empty() {
            return;
        }
        let (chief, council) = {
            let p = self.society.polity(id).unwrap();
            (p.chief, p.council.clone())
        };
        let mut keep = Vec::new();
        for o in offences {
            if !self.still_here(id, o.offender) {
                continue;
            }
            let judge = chief
                .into_iter()
                .chain(council.iter().copied())
                .find(|&j| j != o.offender && j != o.victim && self.still_here(id, j));
            let Some(judge) = judge else {
                if tick < o.tick + 3 * DAY {
                    keep.push(o);
                } else {
                    self.society.stats.unpunished += 1;
                    let v = &mut self.people[o.victim as usize].mind;
                    v.mood.add(Emotion::Anger, 0.2);
                    v.needs.safety = (v.needs.safety - 0.1).max(0.0);
                }
                continue;
            };
            self.verdict(vi, &o, judge);
        }
        self.society.polity_mut(id).offences.extend(keep);
    }

    fn verdict(&mut self, vi: usize, o: &Offence, judge: u32) {
        let id = self.villages[vi].id;
        let (off, vic, ju) = (o.offender as usize, o.victim as usize, judge as usize);
        let t = self.people[ju].mind.traits;
        let repeat = self.society.convictions.get(&o.offender).copied().unwrap_or(0) as f32;
        let severity = match o.crime {
            Crime::Theft => 1.0 + o.value / 15.0,
            Crime::Brawl => 1.0,
        };
        let likes_off = self.people[ju].mind.rel(o.offender).opinion() / 100.0;
        let likes_vic = self.people[ju].mind.rel(o.victim).opinion() / 100.0;
        // Un affamé qui vole pour manger attendrit les juges bons.
        let hungry = o.crime == Crime::Theft
            && self.home_household(off).is_some_and(|h| h.hungry_days > 0)
            && o.value < 15.0;
        let harsh = 0.3 + 0.4 * t.rigor - 0.3 * t.kindness + 0.25 * repeat + 0.15 * (severity - 1.0)
            - 0.35 * likes_off
            + 0.2 * likes_vic
            - if hungry { 0.2 } else { 0.0 }
            + 0.1 * (self.rng.f32() - 0.5);
        let what = match o.crime {
            Crime::Theft => format!("un vol chez {}", self.people[vic].first),
            Crime::Brawl => format!("une bagarre avec {}", self.people[vic].first),
        };
        let (jn, on, e) = (
            self.people[ju].first.clone(),
            self.people[off].first.clone(),
            self.people[off].e(),
        );
        let village_name = self.villages[vi].name.clone();
        let who = vec![o.offender, judge, o.victim];
        if harsh < 0.2 {
            // Pardon : le coupable en sait gré au juge, la victime beaucoup moins.
            self.society.stats.pardons += 1;
            self.event(Some(id), who, format!("{jn} pardonne à {on} {what}"));
            let r = self.people[off].mind.rel_mut(judge);
            r.affection += 10.0;
            r.respect += 5.0;
            r.clamp();
            self.people[off].mind.mood.add(Emotion::Joy, 0.2);
            let r = self.people[vic].mind.rel_mut(judge);
            r.respect -= 8.0;
            r.affection -= 4.0;
            r.clamp();
            self.people[vic].mind.mood.add(Emotion::Anger, 0.2);
            return;
        }
        *self.society.convictions.entry(o.offender).or_default() += 1;
        if repeat >= 2.0 && harsh > 0.75 {
            self.society.stats.banished += 1;
            self.event(
                Some(id),
                who,
                format!("{jn} bannit {on} de {village_name} après {what} (récidive)"),
            );
            self.feel(off, MemoryKind::Banished, Some(judge), -0.95, Emotion::Anger, 0.6);
            self.leave(off);
        } else {
            // Amende : la victime est dédommagée, le reste va à la caisse (ce qui manque devient
            // une dette du foyer).
            let fine = (match o.crime {
                Crime::Theft => o.value * 1.5 + 2.0,
                Crime::Brawl => 3.0,
            } + 6.0 * harsh)
                .round();
            let mut paid = 0.0;
            if let Some(h) = self.people[off].home {
                let hh = self.economy.household(h);
                paid = hh.coins.min(fine);
                hh.coins -= paid;
                hh.debt += fine - paid;
            }
            let restitution = if o.crime == Crime::Theft {
                paid.min(o.value)
            } else {
                0.0
            };
            if let Some(h) = self.people[vic].home {
                self.economy.household(h).coins += restitution;
            }
            self.economy.ensure_village(vi);
            self.economy.treasury[vi] += paid - restitution;
            self.society.stats.fines += 1;
            self.society.stats.fined += fine;
            self.event(
                Some(id),
                who,
                format!("{jn} condamne {on} à {fine:.0} sous d'amende pour {what}"),
            );
            self.feel(off, MemoryKind::Judged, Some(judge), -0.5, Emotion::Anger, 0.4);
            let n = &mut self.people[off].mind.needs;
            n.esteem = (n.esteem - 0.2).max(0.0);
        }
        // Le coupable en veut au juge ; la victime lui fait confiance ; le village s'en souvient.
        let r = self.people[off].mind.rel_mut(judge);
        r.rivalry += 12.0;
        r.affection -= 5.0;
        r.clamp();
        let r = self.people[vic].mind.rel_mut(judge);
        r.trust += 6.0;
        r.respect += 6.0;
        r.clamp();
        let _ = e;
        for i in self.adults_of(id) {
            if i == off || !self.people[i].mind.relations.contains_key(&o.offender) {
                continue;
            }
            let r = self.people[i].mind.rel_mut(o.offender);
            r.respect -= 4.0;
            r.trust -= 3.0;
            r.clamp();
        }
    }

    // ------------------------------------------------------------------ fêtes et foires

    /// Fête des moissons le jour de repos de l'automne ; grande foire l'été dans les villages
    /// assez peuplés et reliés par une route.
    fn festivities(&mut self, vi: usize, today: u64) {
        let id = self.villages[vi].id;
        let c = self.clock();
        let pop = self.population(id);
        self.society.polity_mut(id).feast_day = None;
        if c.season == Season::Autumn && rest_day(self.tick) && pop >= 4 {
            self.hold_feast(vi, today);
        }
        // Foire : en été, un jour qui dépend du village (pour que les foires voisines ne
        // tombent pas le même jour).
        let in_year = today % crate::clock::DAYS_PER_YEAR;
        let fair_in_year = crate::clock::DAYS_PER_SEASON + 2 + (id as u64 % 3);
        let fair = self.society.polity(id).and_then(|p| p.fair_day);
        if let Some(d) = fair
            && today > d + 1
        {
            self.society.polity_mut(id).fair_day = None;
        }
        let roads = self
            .trade
            .routes
            .iter()
            .any(|(&(a, b), r)| r.road && (a == id || b == id));
        if fair.is_none() && in_year + FAIR_NOTICE == fair_in_year && pop >= FAIR_POP && roads {
            self.society.polity_mut(id).fair_day = Some(today + FAIR_NOTICE);
            // La nouvelle court les routes : les villages reliés savent où aller vendre.
            self.economy.ensure_village(vi);
            let news = crate::trade::PriceNews::of(self.tick, &self.economy.markets[vi]);
            let linked: Vec<u32> = self
                .trade
                .routes
                .iter()
                .filter(|(_, r)| r.road)
                .filter_map(|(&(a, b), _)| {
                    if a == id {
                        Some(b)
                    } else if b == id {
                        Some(a)
                    } else {
                        None
                    }
                })
                .collect();
            for v in linked {
                self.trade.hear(v, id, &news);
            }
            let name = self.villages[vi].name.clone();
            self.event(
                Some(id),
                vec![],
                format!(
                    "On annonce la grande foire {}, dans {FAIR_NOTICE} jours",
                    of_place(&name)
                ),
            );
        }
    }

    fn hold_feast(&mut self, vi: usize, today: u64) {
        let tick = self.tick;
        let id = self.villages[vi].id;
        let pop = self.population(id) as f32;
        self.economy.ensure_village(vi);
        let treasury = self.economy.treasury[vi];
        let name = self.villages[vi].name.clone();
        let chief = self.society.polity(id).and_then(|p| p.chief);
        if treasury < pop * 0.3 && self.villages[vi].stock.food < pop * 0.3 {
            self.event(
                Some(id),
                vec![],
                format!(
                    "Pas de fête des moissons {} cette année : la caisse est vide",
                    at_place(&name)
                ),
            );
            if let Some(c) = chief {
                for i in self.adults_of(id) {
                    if self.people[i].id != c {
                        let r = self.people[i].mind.rel_mut(c);
                        r.affection -= 3.0;
                        r.clamp();
                    }
                }
            }
            return;
        }
        // Le village offre à boire et à manger : il paie les foyers qui fournissent le
        // banquet et puise dans le grenier commun.
        let spend = (treasury * 0.4).min(pop * 0.6);
        let food = (self.villages[vi].stock.food * 0.3).min(pop * 0.5);
        self.villages[vi].stock.food -= food;
        self.economy.treasury[vi] -= spend;
        let suppliers: Vec<u32> = self
            .alive()
            .filter(|p| {
                p.village == id
                    && matches!(
                        p.job,
                        Some(Job::Baker | Job::Farmer | Job::Hunter | Job::Fisher | Job::Miller)
                    )
            })
            .filter_map(|p| p.home)
            .collect();
        if !suppliers.is_empty() {
            let each = spend / suppliers.len() as f32;
            for h in suppliers {
                self.economy.household(h).coins += each;
            }
        }
        let p = self.society.polity_mut(id);
        p.feast_day = Some(today);
        p.last_feast = tick;
        self.society.stats.feasts += 1;
        let host = chief
            .map(|c| format!(", offerte par {}", self.people[c as usize].first))
            .unwrap_or_default();
        self.event(
            Some(id),
            chief.into_iter().collect(),
            format!(
                "Fête des moissons {}{host} ({spend:.0} sous de banquet)",
                at_place(&name)
            ),
        );
    }

    /// Le village est-il en fête aujourd'hui (fête des moissons, premier jour de sa foire) ?
    pub fn festive(&self, village: u32) -> bool {
        let today = tick_day(self.tick);
        self.society
            .polity(village)
            .is_some_and(|p| p.feast_day == Some(today) || p.fair_day == Some(today))
    }

    /// La foire de ce village est-elle ouverte (deux marchés) ?
    pub fn fair_open(&self, village: u32) -> bool {
        let today = tick_day(self.tick);
        self.society
            .polity(village)
            .and_then(|p| p.fair_day)
            .is_some_and(|d| today == d || today == d + 1)
    }

    /// Un marchand qui partirait aujourd'hui arriverait-il pour la foire de ce village ?
    pub(crate) fn fair_soon(&self, village: u32) -> bool {
        let today = tick_day(self.tick);
        self.society
            .polity(village)
            .and_then(|p| p.fair_day)
            .is_some_and(|d| today + 2 >= d && today <= d)
    }

    /// La foire de ce village ouvre dans un jour ou deux : les marchands déjà là gardent leur
    /// cargaison pour elle.
    pub(crate) fn fair_ahead(&self, village: u32) -> bool {
        let today = tick_day(self.tick);
        self.society
            .polity(village)
            .and_then(|p| p.fair_day)
            .is_some_and(|d| today < d)
    }

    /// Jour de foire, après le marché : les marchands venus de partout échangent nouvelles
    /// et savoirs.
    pub(super) fn fair_market(&mut self, vi: usize, visitors: &[u32]) {
        let id = self.villages[vi].id;
        let today = tick_day(self.tick);
        let opening = self.society.polity(id).and_then(|p| p.fair_day) == Some(today);
        let mut from: Vec<u32> = visitors
            .iter()
            .map(|&p| self.people[p as usize].village)
            .collect();
        from.sort();
        from.dedup();
        for &a in &from {
            self.trade.share(a, id);
            self.trade.share(id, a);
            for &b in &from {
                if a != b {
                    self.trade.share(a, b);
                }
            }
        }
        for &a in &from {
            let via = visitors
                .iter()
                .copied()
                .find(|&p| self.people[p as usize].village == a);
            self.teach(a, id, 0.2, via);
            if let Some(av) = self.village_index(a) {
                self.teach(id, self.villages[av].id, 0.2, via);
            }
        }
        if opening {
            self.society.stats.fairs += 1;
            self.society.polity_mut(id).last_fair = self.tick;
            let name = self.villages[vi].name.clone();
            let text = if from.is_empty() {
                format!("Grande foire {}, mais aucun marchand n'est venu", at_place(&name))
            } else {
                let names: Vec<String> = from.iter().map(|&v| of_place(&self.village_name(v))).collect();
                format!(
                    "Grande foire {} : des marchands sont venus {}",
                    at_place(&name),
                    names.join(", ")
                )
            };
            self.event(Some(id), visitors.to_vec(), text);
        }
    }

    /// À la fête : on mange, on boit, on danse, et l'on parle avec ceux qui sont là.
    pub(super) fn mingle(&mut self, i: usize) {
        let tick = self.tick;
        let (x, y, village, me) = {
            let p = &self.people[i];
            (p.x, p.y, p.village, p.id)
        };
        let near = (0..self.people.len())
            .filter(|&j| j != i)
            .filter(|&j| {
                let q = &self.people[j];
                q.alive() && q.village == village && !q.brain.hidden && q.brain.act == Act::Feasting
            })
            .map(|j| (((self.people[j].x - x).abs() + (self.people[j].y - y).abs()), j))
            .filter(|&(d, _)| d <= 3.5)
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, j)) = near {
            self.talk(i, j);
        }
        let m = &mut self.people[i].mind;
        m.remember(tick, MemoryKind::Feast, None, 0.3);
        m.mood.add(Emotion::Joy, 0.1);
        if let Some(c) = self.society.polity(village).and_then(|p| p.chief)
            && c != me
        {
            let r = self.people[i].mind.rel_mut(c);
            r.respect += 0.5;
            r.affection += 0.3;
            r.clamp();
        }
    }

    // ------------------------------------------------------------------ maladies

    pub fn sick(&self, person: u32) -> Option<&Sickness> {
        self.society.sick.get(&person)
    }

    /// Où se trouve un habitant (le village d'un marchand en voyage est sa destination).
    fn location(&self, i: usize) -> u32 {
        let p = &self.people[i];
        match self.trade.traders.get(&p.id) {
            Some(t) if t.leg.away() => t.dest,
            _ => p.village,
        }
    }

    /// Fragilité face à la maladie : enfants, aînés, affamés et gelés tombent plus vite.
    fn frailty(&self, i: usize) -> f32 {
        let tick = self.tick;
        let p = &self.people[i];
        let n = p.mind.needs;
        let age = if p.child(tick) {
            1.4
        } else if p.elder(tick) {
            2.0 + (p.age(tick) - ELDER_AGE) * 0.08
        } else {
            1.0
        };
        age * (1.0 + 1.2 * (1.0 - n.food) + 0.5 * (1.0 - n.warmth))
    }

    pub(crate) fn infect(&mut self, i: usize, d: Disease, place: u32) {
        let tick = self.tick;
        let id = self.people[i].id;
        self.society.sick.insert(
            id,
            Sickness {
                disease: d,
                since: tick,
                days_left: d.days(),
                caught_in: place,
            },
        );
        self.society.stats.cases[d as usize] += 1;
        let m = &mut self.people[i].mind;
        m.mood.add(Emotion::Fear, 0.25);
        m.needs.safety = (m.needs.safety - 0.2).max(0.0);
        let village = self.people[i].village;
        for f in self.family_of(i) {
            let q = &mut self.people[f as usize];
            if q.alive() && q.village == village {
                q.mind.mood.add(Emotion::Fear, 0.1);
            }
        }
    }

    /// Une fois par jour : les malades guérissent ou meurent ; la maladie passe à ceux qui
    /// vivent sous le même toit ou dans le même village (marchands de passage compris, qui la
    /// rapportent chez eux) ; parfois elle apparaît d'elle-même ; épidémies déclarées et finies.
    // Les habitants sont indexés partout par leur numéro : les boucles par index restent lisibles.
    #[allow(clippy::needless_range_loop)]
    fn diseases(&mut self) {
        let tick = self.tick;
        let season = self.clock().season;
        // 1. Les malades.
        let sick: Vec<(u32, Sickness)> = self.society.sick.iter().map(|(&k, &s)| (k, s)).collect();
        for (id, s) in sick {
            let i = id as usize;
            if !self.people[i].alive() {
                self.society.sick.remove(&id);
                continue;
            }
            let herbs = self.society.knows(self.location(i), Tech::Herbs);
            let death = s.disease.lethality() * self.frailty(i) * if herbs { 0.5 } else { 1.0 };
            if self.rng.chance(death) {
                self.society.sick.remove(&id);
                self.society.stats.deaths[s.disease as usize] += 1;
                let village = self.people[i].village;
                if let Some(p) = self.society.polities.get_mut(&village) {
                    p.epidemic_dead += 1;
                }
                self.die_of(i, Some(s.disease.the()));
                continue;
            }
            let step = if herbs && s.days_left > 1 && self.rng.chance(0.5) {
                2
            } else {
                1
            };
            let left = s.days_left.saturating_sub(step);
            if left == 0 {
                self.society.sick.remove(&id);
                let r = self.society.recovered.entry(id).or_insert([0; DISEASES]);
                r[s.disease as usize] = tick + 1;
                if s.disease != Disease::Fever {
                    self.feel(i, MemoryKind::Sick, None, 0.4, Emotion::Joy, 0.25);
                }
            } else if let Some(x) = self.society.sick.get_mut(&id) {
                x.days_left = left;
            }
        }

        // 2. La contagion.
        let mut here: BTreeMap<u32, (u32, [u32; DISEASES])> = BTreeMap::new();
        let mut houses: BTreeMap<u32, [u32; DISEASES]> = BTreeMap::new();
        let mut where_: Vec<u32> = vec![u32::MAX; self.people.len()];
        for i in 0..self.people.len() {
            let p = &self.people[i];
            if !p.alive() || p.brain.leaving {
                continue;
            }
            let loc = self.location(i);
            where_[i] = loc;
            let e = here.entry(loc).or_default();
            e.0 += 1;
            if let Some(s) = self.society.sick.get(&p.id) {
                e.1[s.disease as usize] += 1;
                if loc == p.village
                    && let Some(h) = p.home
                {
                    houses.entry(h).or_default()[s.disease as usize] += 1;
                }
            }
        }
        for i in 0..self.people.len() {
            let loc = where_[i];
            if loc == u32::MAX || self.society.sick.contains_key(&self.people[i].id) {
                continue;
            }
            let Some(&(n, counts)) = here.get(&loc) else {
                continue;
            };
            let p = &self.people[i];
            let at_home = (loc == p.village)
                .then_some(p.home)
                .flatten()
                .and_then(|h| houses.get(&h))
                .copied()
                .unwrap_or_default();
            let frail = self.frailty(i);
            for d in Disease::ALL {
                let (s, h) = (counts[d as usize], at_home[d as usize]);
                if s == 0 || self.society.immune(p.id, d, tick) {
                    continue;
                }
                let season_k = match (d, season) {
                    (Disease::Fever, Season::Winter) => 1.4,
                    (Disease::Flux, Season::Summer) => 1.4,
                    _ => 1.0,
                };
                let risk = d.contagion()
                    * season_k
                    * (0.3 * s as f32 / n.max(1) as f32 + 0.12 * h as f32)
                    * (0.6 + 0.4 * frail);
                if self.rng.chance(risk) {
                    self.infect(i, d, loc);
                    break;
                }
            }
        }

        // 3. La maladie apparaît d'elle-même, rarement (plus souvent l'hiver et dans la disette).
        for vi in 0..self.villages.len() {
            let v = &self.villages[vi];
            let id = v.id;
            let k = (if v.hungry_days > 0 { 2.5 } else { 1.0 })
                * (if season == Season::Winter { 1.5 } else { 1.0 })
                * self.chill(vi);
            if !self.rng.chance(k / (4.0 * DAYS_PER_YEAR_F)) {
                continue;
            }
            let roll = self.rng.f32();
            let d = if roll < 0.08 {
                Disease::Pox
            } else if roll < if season == Season::Summer { 0.55 } else { 0.3 } {
                Disease::Flux
            } else {
                Disease::Fever
            };
            let candidates: Vec<usize> = (0..self.people.len())
                .filter(|&i| {
                    let p = &self.people[i];
                    p.alive()
                        && p.village == id
                        && !p.brain.leaving
                        && !self.society.sick.contains_key(&p.id)
                        && !self.society.immune(p.id, d, tick)
                })
                .collect();
            if let Some(&i) = self.rng.pick(&candidates) {
                self.infect(i, d, id);
            }
        }

        // 4. Épidémies.
        for vi in 0..self.villages.len() {
            let id = self.villages[vi].id;
            let mut counts = [0u32; DISEASES];
            for (&p, s) in &self.society.sick {
                if self.people[p as usize].village == id {
                    counts[s.disease as usize] += 1;
                }
            }
            let total: u32 = counts.iter().sum();
            let (worst, n) = Disease::ALL
                .iter()
                .map(|&d| (d, counts[d as usize]))
                .max_by_key(|x| x.1)
                .unwrap();
            let name = self.villages[vi].name.clone();
            let current = self.society.polity(id).and_then(|p| p.epidemic);
            match current {
                None if n >= 3 => {
                    let p = self.society.polity_mut(id);
                    p.epidemic = Some(worst);
                    p.epidemic_dead = 0;
                    self.society.stats.epidemics += 1;
                    self.event(
                        Some(id),
                        vec![],
                        format!("Épidémie de {} {} ({n} malades)", worst.label(), at_place(&name)),
                    );
                    for i in self.adults_of(id) {
                        let m = &mut self.people[i].mind;
                        m.mood.add(Emotion::Fear, 0.15 + 0.15 * m.traits.nervousness);
                        m.needs.safety = (m.needs.safety - 0.15).max(0.0);
                    }
                }
                Some(d) if total == 0 => {
                    let dead = self.society.polity(id).map_or(0, |p| p.epidemic_dead);
                    let toll = match dead {
                        0 => "sans faire de victime".to_string(),
                        1 => "après un mort".to_string(),
                        k => format!("après {k} morts"),
                    };
                    self.society.polity_mut(id).epidemic = None;
                    self.event(
                        Some(id),
                        vec![],
                        format!("Fin de l'épidémie de {} {}, {toll}", d.label(), at_place(&name)),
                    );
                }
                _ => {}
            }
        }
    }

    // ------------------------------------------------------------------ savoirs

    /// Une technique est découverte par un habitant habile et curieux (ou, pendant une
    /// épidémie, par quelqu'un qui cherche des remèdes).
    fn inventions(&mut self) {
        let tick = self.tick;
        for i in 0..self.people.len() {
            let p = &self.people[i];
            if !p.alive() || p.child(tick) || p.elder(tick) || p.brain.leaving {
                continue;
            }
            let Some(job) = p.job else { continue };
            let Some(tech) = Tech::ALL.into_iter().find(|t| t.job() == Some(job)) else {
                continue;
            };
            let skill = p.mind.skill(job);
            if skill < 0.45 || self.society.knows(p.village, tech) {
                continue;
            }
            let chance = 0.0003 * (0.3 + p.mind.traits.openness) * (skill - 0.3) * 2.5;
            if self.rng.chance(chance)
                && let Some(vi) = self.village_index(p.village)
            {
                self.discover(vi, tech, i);
            }
        }
        for vi in 0..self.villages.len() {
            let id = self.villages[vi].id;
            if self.society.polity(id).is_none_or(|p| p.epidemic.is_none())
                || self.society.knows(id, Tech::Herbs)
                || !self.rng.chance(0.04)
            {
                continue;
            }
            let healer = self
                .adults_of(id)
                .into_iter()
                .filter(|&i| !self.society.sick.contains_key(&self.people[i].id))
                .max_by(|&a, &b| {
                    let s = |i: usize| {
                        let t = self.people[i].mind.traits;
                        t.openness + 0.5 * t.kindness
                    };
                    s(a).total_cmp(&s(b))
                });
            if let Some(h) = healer {
                self.discover(vi, Tech::Herbs, h);
            }
        }
    }

    pub(crate) fn discover(&mut self, vi: usize, tech: Tech, by: usize) {
        let tick = self.tick;
        let id = self.villages[vi].id;
        let pid = self.people[by].id;
        self.society.polity_mut(id).techs.push(Known {
            tech,
            tick,
            by: Some(pid),
            from: None,
        });
        self.society.stats.inventions += 1;
        let text = format!(
            "{} découvre {} {} : {}",
            self.people[by].name(),
            tech.label(),
            at_place(&self.villages[vi].name),
            tech.effect()
        );
        self.event(Some(id), vec![pid], text);
        self.feel(by, MemoryKind::Invented, None, 0.9, Emotion::Joy, 0.6);
        let n = &mut self.people[by].mind.needs;
        n.esteem = (n.esteem + 0.4).min(1.0);
        n.meaning = (n.meaning + 0.3).min(1.0);
        for i in self.adults_of(id) {
            if i != by {
                let r = self.people[i].mind.rel_mut(pid);
                r.respect += 8.0;
                r.clamp();
            }
        }
    }

    /// Le village `to` apprend (avec cette chance pour chacune) les techniques que connaît
    /// `from` : par un marchand, un nouveau venu, une foire.
    pub(crate) fn teach(&mut self, from: u32, to: u32, chance: f32, via: Option<u32>) {
        if from == to {
            return;
        }
        let known: Vec<Tech> = self
            .society
            .polity(from)
            .map(|p| p.techs.iter().map(|k| k.tech).collect())
            .unwrap_or_default();
        for tech in known {
            if self.society.knows(to, tech) || !self.rng.chance(chance) {
                continue;
            }
            let tick = self.tick;
            self.society.polity_mut(to).techs.push(Known {
                tech,
                tick,
                by: None,
                from: Some(from),
            });
            self.society.stats.spread += 1;
            let (src, dst) = (self.village_name(from), self.village_name(to));
            let (text, people) = match via {
                Some(p) => (
                    format!(
                        "{} apporte {} {} {}",
                        self.people[p as usize].first,
                        tech.label(),
                        of_place(&src),
                        at_place(&dst)
                    ),
                    vec![p],
                ),
                None => (
                    format!("{dst} apprend {} {}", tech.label(), of_place(&src)),
                    vec![],
                ),
            };
            self.event(Some(to), people, text);
        }
    }

    /// Un village fondé sait tout ce que savait son village d'origine.
    pub(crate) fn inherit_techs(&mut self, from: u32, to: u32) {
        let tick = self.tick;
        let known: Vec<Tech> = self
            .society
            .polity(from)
            .map(|p| p.techs.iter().map(|k| k.tech).collect())
            .unwrap_or_default();
        let p = self.society.polity_mut(to);
        for tech in known {
            if !p.knows(tech) {
                p.techs.push(Known {
                    tech,
                    tick,
                    by: None,
                    from: Some(from),
                });
            }
        }
    }

    /// Rendement d'une technique pour le village de cet habitant (1 si inconnue).
    pub(crate) fn tech_bonus(&self, i: usize, tech: Tech, k: f32) -> f32 {
        if self.society.knows(self.people[i].village, tech) {
            k
        } else {
            1.0
        }
    }
}

const DAYS_PER_YEAR_F: f32 = crate::clock::DAYS_PER_YEAR as f32;

#[cfg(test)]
mod tests {
    use crate::{
        economy::Good,
        mind::MemoryKind,
        people::Act,
        save::tests::sample,
        society::{Crime, Disease, Tech},
        tuning::{DAY, HOUR, YEAR},
    };

    fn adults(w: &crate::World) -> Vec<usize> {
        let tick = w.tick;
        (0..w.people.len())
            .filter(|&i| w.people[i].alive() && !w.people[i].child(tick))
            .collect()
    }

    /// Le premier minuit, le village se choisit un chef parmi ses adultes ; il est réélu ou
    /// remplacé au bout de son mandat, et un nouveau chef est élu à sa mort.
    #[test]
    fn villages_elect_chiefs() {
        let mut w = sample();
        let id = w.villages[0].id;
        w.daily_society();
        let p = w.society.polity(id).unwrap();
        let chief = p.chief.expect("pas de chef");
        assert!(adults(&w).contains(&(chief as usize)));
        assert!((0.05..=0.2).contains(&p.tax));
        assert!(w.chronicle.events.iter().any(|e| e.text.contains("premi")));
        // Fin du mandat : nouvelle élection.
        w.tick += 2 * YEAR;
        w.daily_society();
        assert_eq!(w.society.stats.elections, 2);
        // Le chef meurt : on en élit un autre.
        let chief = w.society.polity(id).unwrap().chief.unwrap();
        w.die_of(chief as usize, None);
        w.daily_society();
        let new = w.society.polity(id).unwrap().chief.unwrap();
        assert_ne!(new, chief);
        assert!(w.people[new as usize].alive());
    }

    /// Un vol découvert est jugé par le chef : la victime est dédommagée, le coupable s'en
    /// souvient (ou est pardonné par un juge très bon qui l'apprécie).
    #[test]
    fn theft_is_judged() {
        let mut w = sample();
        let id = w.villages[0].id;
        w.daily_society();
        let chief = w.society.polity(id).unwrap().chief.unwrap();
        let others: Vec<usize> = adults(&w)
            .into_iter()
            .filter(|&i| i as u32 != chief && w.people[i].home.is_some())
            .collect();
        let (thief, victim) = (
            others[0],
            *others
                .iter()
                .find(|&&v| w.people[v].home != w.people[others[0]].home)
                .unwrap(),
        );
        let (th, vh) = (w.people[thief].home.unwrap(), w.people[victim].home.unwrap());
        w.economy.household(th).coins = 50.0;
        w.economy.household(vh).coins = 0.0;
        // Un juge rigoureux, sans tendresse pour le voleur.
        let t = &mut w.people[chief as usize].mind.traits;
        t.rigor = 1.0;
        t.kindness = 0.0;
        w.complain(Crime::Theft, thief as u32, victim as u32, 10.0);
        w.judge(0);
        assert!(
            w.economy.household(vh).coins >= 10.0 - 1e-3,
            "pas de dédommagement"
        );
        assert!(w.economy.household(th).coins < 50.0 - 10.0);
        assert!(
            w.people[thief]
                .mind
                .memories
                .iter()
                .any(|m| m.kind == MemoryKind::Judged && m.who == Some(chief))
        );
        assert_eq!(w.society.convictions.get(&(thief as u32)), Some(&1));
        assert_eq!(w.society.stats.fines, 1);
    }

    /// La fièvre passe d'un malade à sa famille ; les guéris sont protégés un temps.
    #[test]
    fn fever_spreads_and_leaves_immunity() {
        let mut w = sample();
        let id = w.villages[0].id;
        let hid = w.villages[0]
            .houses
            .iter()
            .max_by_key(|h| h.residents.len())
            .unwrap()
            .id;
        let family = w.villages[0].house(hid).unwrap().residents.clone();
        assert!(family.len() >= 2);
        for &r in &family {
            w.infect(r as usize, Disease::Fever, id);
        }
        let cases = w.society.stats.cases[Disease::Fever as usize];
        // La contagion est affaire de hasard : quelques tirages, il en faut un où elle passe.
        let start = w.clone();
        let mut total = cases;
        for seed in 0..8 {
            w = start.clone();
            w.rng = crate::rng::Rng::new(seed);
            for _ in 0..12 {
                w.tick += DAY;
                w.diseases();
            }
            total = w.society.stats.cases[Disease::Fever as usize];
            if total > cases {
                break;
            }
        }
        assert!(total > cases, "la fièvre ne s'est pas répandue ({total})");
        let tick = w.tick;
        for &r in &family {
            let p = &w.people[r as usize];
            if p.alive() {
                assert!(
                    w.society.immune(r, Disease::Fever, tick),
                    "{} pas protégé",
                    p.name()
                );
            }
        }
        assert!(
            w.chronicle
                .events
                .iter()
                .any(|e| e.text.starts_with("Épidémie de fièvre"))
        );
    }

    /// Un malade reste chez lui au lieu de travailler.
    #[test]
    fn the_sick_stay_in_bed() {
        let mut w = sample();
        let i = adults(&w)[0];
        let id = w.villages[0].id;
        w.infect(i, Disease::Flux, id);
        w.people[i].brain = Default::default();
        // Au milieu de la journée de travail.
        while super::super::day_part(w.clock().hour) != super::super::DayPart::Work || w.clock().hour < 9 {
            w.step();
        }
        for _ in 0..HOUR {
            w.step();
            if w.people[i].brain.act == Act::Ill {
                return;
            }
        }
        panic!("{:?}", w.people[i].brain.act);
    }

    /// Une technique connue d'un village passe à l'autre ; l'assolement fait de plus belles
    /// moissons.
    #[test]
    fn knowledge_spreads_and_helps() {
        let mut w = sample();
        let id = w.villages[0].id;
        let farmer = adults(&w)[0];
        w.discover(0, Tech::Rotation, farmer);
        assert!(w.society.knows(id, Tech::Rotation));
        assert!(
            w.people[farmer]
                .mind
                .memories
                .iter()
                .any(|m| m.kind == MemoryKind::Invented)
        );
        assert_eq!(w.tech_bonus(farmer, Tech::Rotation, 1.25), 1.25);
        w.teach(id, 99, 1.0, None);
        assert!(w.society.knows(99, Tech::Rotation));
        assert!(!w.society.knows(99, Tech::Nets));
    }

    /// Le jour de la fête des moissons, le village se retrouve autour du puits ; le banquet
    /// est payé par la caisse.
    #[test]
    fn harvest_feast() {
        let mut w = sample();
        let id = w.villages[0].id;
        w.economy.ensure_village(0);
        w.economy.treasury[0] = 200.0;
        // Juste avant minuit, la veille du jour de repos de l'automne.
        let per_day = crate::clock::SECONDS_PER_DAY / crate::clock::TICK_SECONDS;
        let start = crate::clock::START_HOUR * 3600 / crate::clock::TICK_SECONDS;
        let mut day = super::super::tick_day(w.tick) + 1;
        while day % crate::clock::DAYS_PER_YEAR != 2 * crate::clock::DAYS_PER_SEASON + 6 {
            day += 1;
        }
        w.tick = day * per_day - start - 1;
        let before = w.economy.treasury[0];
        w.step();
        assert!(w.festive(id), "pas de fête");
        assert!(w.economy.treasury[0] < before);
        let mut feasting = 0;
        for _ in 0..11 * HOUR {
            w.step();
            feasting = feasting.max(w.alive().filter(|p| p.brain.act == Act::Feasting).count());
        }
        assert!(feasting >= 4, "{feasting} à la fête");
        assert!(
            w.alive()
                .any(|p| p.mind.memories.iter().any(|m| m.kind == MemoryKind::Feast))
        );
        let _ = (Good::Grain, DAY);
    }
}

//! Vie intérieure des habitants : besoins, humeur, souvenirs, conversations, commérages,
//! entraide, compétences et objectifs de vie.

use std::collections::BTreeMap;

use crate::{
    clock::Season,
    economy::{Good, JOBS},
    mind::{Emotion, Goal, LifeGoal, MASTERY, MemoryKind, Mind, Traits},
    people::{Act, Intent, Job},
    tuning::*,
    world::World,
};

/// Progrès d'une séance de travail (plus pour les rigoureux), avant ralentissement.
const PRACTICE: f32 = 0.0016;
/// Apprentis qu'un maître forme à la fois.
pub(crate) const APPRENTICES_PER_MASTER: usize = 2;

/// Argent d'un foyer à partir duquel l'objectif « mettre de l'argent de côté » est atteint.
const WEALTHY: f32 = 60.0;
/// Réputation (opinion moyenne du village, -100 à 100) d'un habitant « respecté ».
const RESPECTED: f32 = 30.0;

impl World {
    // ------------------------------------------------------------------ toutes les 10 minutes

    /// Les besoins bougent selon ce que fait chacun ; les émotions retombent.
    pub(super) fn minds_tick(&mut self) {
        let tick = self.tick;
        for p in self.people.iter_mut().filter(|p| p.alive()) {
            let child = p.child(tick);
            let (act, hidden, with) = (p.brain.act, p.brain.hidden, p.brain.with);
            let m = &mut p.mind;
            let t = m.traits;
            let n = &mut m.needs;
            if hidden && act == Act::Sleeping {
                n.sleep += 0.022;
            } else {
                n.sleep -= if child { 0.006 } else { 0.0075 };
                n.food -= 0.002;
            }
            match act {
                Act::Chatting | Act::Playing if with.is_some() => {
                    n.company += 0.12;
                    n.leisure += 0.06;
                }
                Act::Playing | Act::Strolling => n.leisure += 0.08,
                Act::Learning => n.company += 0.03,
                Act::Idle if !hidden => {
                    n.leisure += 0.02;
                    // Le soir devant chez soi, en famille.
                    n.meaning += 0.008;
                }
                Act::Sleeping => {}
                _ => {
                    n.company -= 0.004 + 0.006 * t.sociability;
                    n.leisure -= 0.004 + 0.003 * t.openness;
                }
            }
            n.esteem -= 0.0005;
            n.meaning -= 0.001;
            n.safety += 0.003;
            n.clamp();
            let n = *n;
            m.mood.decay(0.985);
            if n.wellbeing() < 0.4 {
                m.mood.add(Emotion::Sadness, 0.01);
            }
            if n.sleep < 0.15 {
                m.mood.add(Emotion::Anger, 0.01);
            }
            if n.safety < 0.4 {
                m.mood.add(Emotion::Fear, 0.008 * (0.5 + t.nervousness));
            }
            if n.wellbeing() > 0.75 {
                m.mood.add(Emotion::Joy, 0.006);
            }
        }
    }

    // ------------------------------------------------------------------ une fois par jour

    /// Souvenirs qui s'effacent, relations oubliées, réputation, objectifs de vie.
    pub(super) fn daily_minds(&mut self) {
        let tick = self.tick;
        // Réputation : opinion moyenne de ceux qui connaissent quelqu'un.
        let rep = self.reputations();
        let season = self.clock().season;
        for i in 0..self.people.len() {
            if !self.people[i].alive() {
                continue;
            }
            let family = self.family_of(i);
            let kin: Vec<u32> = family
                .iter()
                .copied()
                .filter(|&id| self.people[id as usize].alive())
                .collect();
            let keep = |id: u32| kin.contains(&id);
            let gone: Vec<u32> = self.people[i]
                .mind
                .relations
                .keys()
                .copied()
                .filter(|&id| !self.people[id as usize].alive() && !family.contains(&id))
                .collect();
            let mut mind = std::mem::take(&mut self.people[i].mind);
            // Les morts et les partis s'oublient peu à peu (sauf la famille).
            for id in gone {
                if let Some(r) = mind.relations.get_mut(&id) {
                    r.affection *= 0.9;
                    r.rivalry *= 0.8;
                    r.attraction *= 0.7;
                }
            }
            // Les sentiments sans entretien retombent un peu (sauf pour la famille).
            for (id, r) in mind.relations.iter_mut() {
                r.attraction *= 0.985;
                r.rivalry *= 0.98;
                if !kin.contains(id) {
                    r.affection *= 0.97;
                }
            }
            mind.fade_memories();
            mind.prune_relations(keep);
            // Reconnaissance : la réputation et le talent nourrissent l'estime de soi.
            let r = rep.get(&(i as u32)).copied().unwrap_or(0.0);
            let skill = self.people[i].job.map_or(0.0, |j| mind.skill(j));
            mind.needs.esteem = (mind.needs.esteem + 0.04 * (r / 50.0) + 0.03 * skill).clamp(0.0, 1.0);
            if season == Season::Winter {
                mind.mood.add(Emotion::Sadness, 0.03 * mind.traits.nervousness);
            }
            self.people[i].mind = mind;
            if !self.people[i].child(tick) {
                self.check_goals(i, r);
            }
        }
    }

    /// Opinion moyenne que le village a de chacun (seulement ceux qu'on connaît).
    pub fn reputations(&self) -> BTreeMap<u32, f32> {
        let mut sum: BTreeMap<u32, (f32, u32)> = BTreeMap::new();
        for p in self.alive() {
            for (&id, r) in &p.mind.relations {
                let e = sum.entry(id).or_default();
                e.0 += r.opinion();
                e.1 += 1;
            }
        }
        sum.into_iter().map(|(id, (s, n))| (id, s / n as f32)).collect()
    }

    /// Réputation d'un seul habitant (pour une fiche).
    pub fn reputation(&self, id: u32) -> Option<f32> {
        let (mut s, mut n) = (0.0, 0);
        for p in self.alive() {
            if let Some(r) = p.mind.relations.get(&id) {
                s += r.opinion();
                n += 1;
            }
        }
        (n > 0).then(|| s / n as f32)
    }

    /// Conjoint, parents, enfants, frères et sœurs.
    pub(crate) fn family_of(&self, i: usize) -> Vec<u32> {
        let p = &self.people[i];
        let mut f: Vec<u32> = p.parents.clone();
        f.extend(&p.children);
        f.extend(p.partner);
        for &q in &p.parents {
            f.extend(self.people[q as usize].children.iter().filter(|&&c| c != p.id));
        }
        f.sort();
        f.dedup();
        f
    }

    // ------------------------------------------------------------------ objectifs de vie

    /// Un ou deux objectifs selon la personnalité (à l'âge adulte).
    pub(crate) fn pick_goals(&mut self, i: usize) {
        let t = self.people[i].mind.traits;
        let single = self.people[i]
            .partner
            .is_none_or(|q| !self.people[q as usize].alive());
        let mut options = vec![
            (Goal::Children, 0.3 + 0.4 * t.kindness),
            (Goal::Master, 0.2 + 0.6 * t.ambition + 0.3 * t.rigor),
            (Goal::Wealth, 0.1 + 0.5 * t.ambition + 0.4 * (1.0 - t.generosity)),
            (Goal::Respect, 0.1 + 0.4 * t.ambition + 0.4 * t.sociability),
        ];
        if single {
            options.push((Goal::Marry, 0.5 + 0.6 * t.sociability));
        }
        for o in &mut options {
            o.1 += self.rng.f32() * 0.5;
        }
        options.sort_by(|a, b| b.1.total_cmp(&a.1));
        let n = if self.rng.chance(0.5) { 2 } else { 1 };
        let goals = &mut self.people[i].mind.goals;
        for (g, _) in options.into_iter().take(n) {
            if !goals.iter().any(|l| l.goal == g) {
                goals.push(LifeGoal {
                    goal: g,
                    reached: None,
                });
            }
        }
    }

    fn check_goals(&mut self, i: usize, reputation: f32) {
        let tick = self.tick;
        if self.people[i].mind.goals.is_empty() {
            self.pick_goals(i);
        }
        let p = &self.people[i];
        let married = p.partner.is_some_and(|q| self.people[q as usize].alive());
        let has_kids = !p.children.is_empty();
        let master = p.job.is_some_and(|j| p.mind.skill(j) >= MASTERY);
        let rich = self.home_household(i).is_some_and(|h| h.coins >= WEALTHY);
        let mut reached = Vec::new();
        for (k, l) in p.mind.goals.iter().enumerate() {
            if l.reached.is_some() {
                continue;
            }
            let ok = match l.goal {
                Goal::Marry => married,
                Goal::Children => has_kids,
                Goal::Master => master,
                Goal::Wealth => rich,
                Goal::Respect => reputation >= RESPECTED,
            };
            if ok {
                reached.push((k, l.goal));
            }
        }
        for (k, goal) in reached {
            let p = &mut self.people[i];
            p.mind.goals[k].reached = Some(tick);
            p.mind.remember(tick, MemoryKind::GoalReached, None, 0.85);
            p.mind.mood.add(Emotion::Joy, 0.5);
            p.mind.needs.meaning = (p.mind.needs.meaning + 0.4).min(1.0);
            // Mariage, enfants et maîtrise ont déjà leur événement dans la chronique.
            if matches!(goal, Goal::Wealth | Goal::Respect) {
                let text = format!("{} a réalisé son rêve : {}", p.first, goal.label());
                let (id, village) = (p.id, p.village);
                self.event(Some(village), vec![id], text);
            }
        }
        // Marié et sans enfant : l'envie d'en avoir peut venir.
        let p = &self.people[i];
        if married
            && !has_kids
            && !p.mind.goals.iter().any(|l| l.goal == Goal::Children)
            && p.age(tick) < FERTILE_MAX_AGE
            && self.rng.chance(0.02 + 0.03 * p.mind.traits.kindness)
        {
            self.people[i].mind.goals.push(LifeGoal {
                goal: Goal::Children,
                reached: None,
            });
        }
    }

    // ------------------------------------------------------------------ conversations

    /// Fin d'une conversation entre `i` et `j` (si `j` est bien venu) : le sujet dépend des
    /// caractères, des humeurs et de ce qu'ils pensent l'un de l'autre.
    pub(super) fn talk(&mut self, i: usize, j: usize) {
        let tick = self.tick;
        let gossip_subject = self.gossip_subject(i, self.people[j].id);
        let (pi, pj) = (&self.people[i], &self.people[j]);
        if !pj.alive()
            || pj.brain.hidden
            || pj.brain.leaving
            || (pi.x - pj.x).abs() + (pi.y - pj.y).abs() > 4.0
        {
            // On a attendu pour rien.
            self.people[i].mind.mood.add(Emotion::Sadness, 0.05);
            return;
        }
        let (ti, tj) = (pi.mind.traits, pj.mind.traits);
        let (ri, rj) = (pi.mind.rel(pj.id), pj.mind.rel(pi.id));
        let (idi, idj) = (pi.id, pj.id);
        let courting = matches!(pi.brain.intent, Intent::Court(q) if q == idj);
        let can_flirt = self.could_pair(i, j);
        let advice = pj
            .job
            .is_some_and(|job| pi.mind.skill(job) > pj.mind.skill(job) + 0.25 && !pi.child(tick));

        let w_small = 1.0;
        let w_gossip = if gossip_subject.is_some() {
            0.4 + 0.8 * ti.sociability
        } else {
            0.0
        };
        let w_flirt = match (can_flirt, courting) {
            (false, _) => 0.0,
            (true, true) => 2.5,
            (true, false) => 0.2 + ri.attraction / 50.0,
        };
        let w_dispute = 0.005
            + 0.25 * pi.mind.mood.anger
            + 0.1 * pj.mind.mood.anger
            + (ri.rivalry + rj.rivalry) / 300.0
            + 0.025 * (1.0 - ti.kindness)
            + 0.01 * ti.nervousness;
        let w_advice = if advice { 0.5 } else { 0.0 };
        let total = w_small + w_gossip + w_flirt + w_dispute + w_advice;
        let mut roll = self.rng.f32() * total;
        let mut pick = |w: f32| {
            roll -= w;
            roll < 0.0
        };
        let topic = if pick(w_dispute) {
            Topic::Dispute
        } else if pick(w_flirt) {
            Topic::Flirt
        } else if pick(w_gossip) {
            Topic::Gossip
        } else if pick(w_advice) {
            Topic::Advice
        } else {
            Topic::Small
        };

        // Toute conversation (sauf une dispute) rapproche un peu.
        if topic != Topic::Dispute {
            let warm_i = 1.5 + 2.0 * tj.kindness + 1.0 * ti.sociability;
            let warm_j = 1.5 + 2.0 * ti.kindness + 1.0 * tj.sociability;
            self.bond(i, idj, warm_i, 1.0);
            self.bond(j, idi, warm_j, 1.0);
            for k in [i, j] {
                let m = &mut self.people[k].mind;
                m.needs.company = (m.needs.company + 0.15).min(1.0);
                m.mood.add(Emotion::Joy, 0.04);
            }
        }
        match topic {
            Topic::Small => {
                if self.rng.chance(0.05) && ri.affection > 40.0 {
                    self.people[i]
                        .mind
                        .remember(tick, MemoryKind::GoodTalk, Some(idj), 0.3);
                    self.people[j]
                        .mind
                        .remember(tick, MemoryKind::GoodTalk, Some(idi), 0.3);
                }
            }
            Topic::Gossip => {
                let (k, opinion) = gossip_subject.unwrap();
                self.gossip(i, j, k, opinion);
            }
            Topic::Flirt => self.flirt(i, j),
            Topic::Dispute => self.quarrel(i, j),
            Topic::Advice => {
                let job = self.people[j].job.unwrap();
                let gap = self.people[i].mind.skill(job) - self.people[j].mind.skill(job);
                self.improve(j, job, 0.04 * gap);
                let r = self.people[j].mind.rel_mut(idi);
                r.respect += 5.0;
                r.trust += 2.0;
                r.clamp();
                self.people[j]
                    .mind
                    .remember(tick, MemoryKind::Advice, Some(idi), 0.3);
                let m = &mut self.people[i].mind;
                m.needs.esteem = (m.needs.esteem + 0.05).min(1.0);
            }
        }
    }

    /// Rapproche `i` de `other` (affection et confiance).
    fn bond(&mut self, i: usize, other: u32, affection: f32, trust: f32) {
        let r = self.people[i].mind.rel_mut(other);
        // Plus on est proche, plus il faut d'efforts pour l'être davantage.
        r.affection += affection * (1.0 - r.affection / 120.0);
        r.trust += trust;
        r.rivalry -= 0.5;
        r.clamp();
    }

    /// Quelqu'un dont `i` pense beaucoup de bien ou de mal (pour un commérage).
    fn gossip_subject(&mut self, i: usize, listener: u32) -> Option<(u32, f32)> {
        let p = &self.people[i];
        let options: Vec<(u32, f32)> = p
            .mind
            .relations
            .iter()
            .filter(|&(&k, _)| k != listener && self.people[k as usize].alive())
            .map(|(&k, r)| (k, r.opinion()))
            .filter(|&(_, o)| o.abs() > 20.0)
            .collect();
        self.rng.pick(&options).copied()
    }

    /// `i` raconte à `j` ce qu'il pense de `k` ; `j` le croit d'autant plus qu'il a
    /// confiance en `i`. Les anxieux en rajoutent.
    fn gossip(&mut self, i: usize, j: usize, k: u32, opinion: f32) {
        if j as u32 == k {
            return;
        }
        let ti = self.people[i].mind.traits;
        let trust = self.people[j].mind.rel(self.people[i].id).trust;
        let mut said = opinion;
        if self.rng.chance(0.2 + 0.4 * ti.nervousness) {
            // Déformation : on exagère, parfois dans le mauvais sens.
            said *= 1.5;
        }
        let weight = 0.04 + 0.08 * trust / 100.0;
        let r = self.people[j].mind.rel_mut(k);
        if said >= 0.0 {
            r.affection += said * weight * 0.6;
            r.respect += said * weight;
        } else {
            r.affection += said * weight * 0.6;
            r.trust += said * weight;
            r.rivalry += -said * weight * 0.3;
        }
        r.clamp();
    }

    /// Deux célibataires qui peuvent se plaire (âge, pas de parenté, même village).
    pub(crate) fn could_pair(&self, i: usize, j: usize) -> bool {
        let tick = self.tick;
        let (p, q) = (&self.people[i], &self.people[j]);
        self.single(i)
            && self.single(j)
            && p.village == q.village
            && (p.age(tick) - q.age(tick)).abs() <= 12.0
            && !self.related(p.id, q.id)
    }

    fn flirt(&mut self, i: usize, j: usize) {
        let (ti, tj) = (self.people[i].mind.traits, self.people[j].mind.traits);
        let (idi, idj) = (self.people[i].id, self.people[j].id);
        // Affinités : des caractères proches s'accordent, la bonne humeur aide.
        let likeness =
            1.0 - ((ti.openness - tj.openness).abs() + (ti.sociability - tj.sociability).abs()) / 2.0;
        // Préférence (90 % des habitants préfèrent l'autre sexe).
        let pref = |w: &World, a: usize, b: usize| {
            let (pa, pb) = (&w.people[a], &w.people[b]);
            let straight = (pa.id.wrapping_mul(2654435761) >> 7) % 10 != 0;
            if straight == (pa.female != pb.female) {
                1.0
            } else {
                0.1
            }
        };
        let (pi_, pj_) = (pref(self, i, j), pref(self, j, i));
        let joy_j = self.people[j].mind.mood.joy;
        let gain_j = (3.0 + 7.0 * likeness + 3.0 * joy_j + self.rng.f32() * 3.0) * pj_;
        let gain_i = (4.0 + 6.0 * likeness + self.rng.f32() * 3.0) * pi_;
        let before = self.people[i].mind.rel(idj).attraction;
        for (a, b, g) in [(i, idj, gain_i), (j, idi, gain_j)] {
            let r = self.people[a].mind.rel_mut(b);
            r.attraction += g;
            r.affection += g * 0.3;
            r.clamp();
            let m = &mut self.people[a].mind;
            m.mood.add(Emotion::Joy, 0.05 * g / 5.0);
        }
        let after = self.people[i].mind.rel(idj).attraction;
        if before < 50.0 && after >= 50.0 {
            let p = &self.people[i];
            let text = format!("{} a le cœur qui bat pour {}", p.first, self.people[j].first);
            let village = p.village;
            self.event(Some(village), vec![idi, idj], text);
        }
    }

    fn quarrel(&mut self, i: usize, j: usize) {
        let tick = self.tick;
        let (idi, idj) = (self.people[i].id, self.people[j].id);
        let before = self.people[i]
            .mind
            .rel(idj)
            .rivalry
            .max(self.people[j].mind.rel(idi).rivalry);
        for (a, b) in [(i, idj), (j, idi)] {
            let kind = self.people[a].mind.traits.kindness;
            let r = self.people[a].mind.rel_mut(b);
            r.affection -= 4.0 + 4.0 * (1.0 - kind);
            r.trust -= 4.0;
            r.rivalry += 6.0 + 6.0 * (1.0 - kind);
            r.attraction *= 0.8;
            r.clamp();
            let m = &mut self.people[a].mind;
            m.mood.add(Emotion::Anger, 0.3);
            m.needs.company = (m.needs.company + 0.05).min(1.0);
            m.remember(tick, MemoryKind::Quarrel, Some(b), -0.35);
        }
        let after = self.people[i]
            .mind
            .rel(idj)
            .rivalry
            .max(self.people[j].mind.rel(idi).rivalry);
        if before < 40.0 && after >= 40.0 {
            let (a, b) = (&self.people[i], &self.people[j]);
            let text = format!("{} et {} sont brouillés", a.first, b.first);
            let village = a.village;
            self.event(Some(village), vec![idi, idj], text);
        }
    }

    // ------------------------------------------------------------------ grands moments

    /// Un événement marquant pour `i` (souvenir + émotion).
    pub(crate) fn feel(
        &mut self,
        i: usize,
        kind: MemoryKind,
        who: Option<u32>,
        weight: f32,
        e: Emotion,
        amount: f32,
    ) {
        let tick = self.tick;
        let m = &mut self.people[i].mind;
        m.remember(tick, kind, who, weight);
        m.mood.add(e, amount);
    }

    /// Un décès : la famille et les amis proches en gardent le souvenir.
    pub(crate) fn mourn(&mut self, dead: usize) {
        let id = self.people[dead].id;
        let family = self.family_of(dead);
        let partner = self.people[dead].partner;
        for k in 0..self.people.len() {
            if k == dead || !self.people[k].alive() {
                continue;
            }
            let aff = self.people[k].mind.affection(id);
            let (w, s) = if partner == Some(k as u32) {
                (-1.0, 0.9)
            } else if family.contains(&(k as u32)) {
                (-0.85, 0.6)
            } else if aff >= 40.0 {
                (-0.5, 0.35)
            } else {
                continue;
            };
            self.feel(k, MemoryKind::Bereaved, Some(id), w, Emotion::Sadness, s);
            let n = &mut self.people[k].mind.needs;
            n.meaning = (n.meaning - 0.15).max(0.0);
            n.safety = (n.safety - 0.1).max(0.0);
        }
    }

    /// Un habitant s'en va : ses amis restés au village le regrettent.
    pub(crate) fn miss(&mut self, gone: usize) {
        let id = self.people[gone].id;
        for k in 0..self.people.len() {
            let q = &self.people[k];
            if k == gone || !q.alive() || q.brain.leaving || q.village != self.people[gone].village {
                continue;
            }
            if q.mind.affection(id) >= 35.0 {
                self.feel(k, MemoryKind::FriendLeft, Some(id), -0.4, Emotion::Sadness, 0.3);
            }
        }
    }

    /// Ceux qui aimaient en secret l'un des deux nouveaux amoureux ont le cœur brisé.
    pub(crate) fn heartbreak(&mut self, a: u32, b: u32) {
        for k in 0..self.people.len() {
            let q = &self.people[k];
            if !q.alive() || q.id == a || q.id == b {
                continue;
            }
            for who in [a, b] {
                if q.mind.rel(who).attraction >= 40.0 {
                    self.feel(k, MemoryKind::Heartbreak, Some(who), -0.5, Emotion::Sadness, 0.45);
                    let r = self.people[k].mind.rel_mut(who);
                    r.attraction *= 0.3;
                    break;
                }
            }
        }
    }

    // ------------------------------------------------------------------ compétences

    /// Rendement d'une séance de travail : outil (×1,4, qui s'use), compétence (0,75 à 1,25)
    /// et humeur. La pratique fait progresser.
    pub(super) fn work_bonus(&mut self, i: usize) -> f32 {
        let tool = self.tool_bonus(i);
        let Some(job) = self.people[i].job else {
            return tool;
        };
        let rigor = self.people[i].mind.traits.rigor;
        self.improve(i, job, PRACTICE + 0.0026 * rigor);
        let p = &mut self.people[i];
        let skill = p.mind.skill(job);
        let mood = 1.0 + 0.08 * p.mind.mood.valence() - if p.mind.needs.sleep < 0.15 { 0.1 } else { 0.0 };
        p.mind.needs.esteem = (p.mind.needs.esteem + 0.02).min(1.0);
        tool * (0.75 + 0.5 * skill) * mood
    }

    /// Fait progresser `i` dans son métier ; passer maître est un grand moment.
    pub(super) fn improve(&mut self, i: usize, job: Job, rate: f32) {
        let tick = self.tick;
        let p = &mut self.people[i];
        if !p.mind.practice(job, rate) {
            return;
        }
        p.mind.remember(tick, MemoryKind::BecameMaster, None, 0.9);
        p.mind.mood.add(Emotion::Joy, 0.5);
        p.mind.needs.esteem = (p.mind.needs.esteem + 0.3).min(1.0);
        let text = format!("{} est reconnu{} maître {}", p.first, p.e(), job.label(p.female));
        let (id, village) = (p.id, p.village);
        self.event(Some(village), vec![id], text);
    }

    /// Qualité de ce que vend un foyer (0–1) : le meilleur de ses membres dans le métier qui
    /// produit ce bien. Un foyer qui revend ce qu'il n'a pas fait compte pour un compagnon.
    pub(super) fn quality(&self, members: &[usize], g: crate::economy::Good) -> f32 {
        let tick = self.tick;
        let job = g.producer();
        members
            .iter()
            .map(|&m| &self.people[m])
            .filter(|p| p.job == Some(job) && !p.child(tick))
            .map(|p| p.mind.skill(job))
            .fold(None, |a: Option<f32>, s| Some(a.map_or(s, |a| a.max(s))))
            .unwrap_or(0.35)
    }

    /// Compétences d'un habitant qui arrive adulte (fondation, nouveaux venus) : l'expérience
    /// vient avec l'âge.
    pub(crate) fn seasoned_skills(&mut self, i: usize) {
        let tick = self.tick;
        let p = &self.people[i];
        let Some(job) = p.job else { return };
        // Quelques aînés arrivent déjà maîtres, rarement.
        let years = (p.age(tick) - ADULT_AGE).max(0.0);
        let base = (0.15 + years * 0.015).min(0.65);
        let s = (base * (0.8 + 0.4 * self.rng.f32())).min(0.85);
        self.people[i].mind.skills[job as usize] = s;
    }

    // ------------------------------------------------------------------ entraide

    /// Un foyer qui a faim : un ami généreux qui a des réserves partage les siennes.
    /// Renvoie les rations données.
    pub(super) fn neighbourly_help(&mut self, vi: usize, hid: u32, members: &[usize], missing: f32) -> f32 {
        let tick = self.tick;
        let village = self.villages[vi].id;
        // Le meilleur ami (le plus généreux) de chaque membre, avec un foyer bien pourvu.
        let mut best: Option<(f32, usize)> = None;
        for &m in members {
            for (&f, r) in &self.people[m].mind.relations {
                let q = &self.people[f as usize];
                if r.affection < 20.0 || !q.alive() || q.village != village || q.child(tick) {
                    continue;
                }
                let Some(fh) = q.home.filter(|&h| h != hid) else {
                    continue;
                };
                let Some(h) = self.economy.households.get(&fh) else {
                    continue;
                };
                let t = q.mind.traits;
                let their = q.mind.rel(self.people[m].id).affection;
                let will = (0.3 * t.generosity + 0.3 * t.kindness) * (0.5 + their / 100.0);
                if h.rations() < 20.0 || will < 0.2 {
                    continue;
                }
                if best.is_none_or(|b| will > b.0) {
                    best = Some((will, f as usize));
                }
            }
        }
        let Some((_, giver)) = best else { return 0.0 };
        if !self.rng.chance(0.7) {
            return 0.0;
        }
        let Some(gh) = self.people[giver].home else {
            return 0.0;
        };
        // On donne de quoi tenir trois jours, sans se mettre soi-même en danger.
        let mut want = missing * 3.0;
        let mut given = 0.0;
        let h = self.economy.household(gh);
        let spare = (h.rations() - 14.0).max(0.0);
        want = want.min(spare);
        let mut moved = [0.0; crate::economy::GOODS];
        for g in Good::FOODS.iter().rev() {
            let gi = *g as usize;
            let units = h.inv[gi].min((want - given) / g.ration());
            if units <= 0.0 {
                continue;
            }
            h.inv[gi] -= units;
            moved[gi] = units;
            given += units * g.ration();
        }
        if given < 0.5 {
            return 0.0;
        }
        let to = self.economy.household(hid);
        for (gi, u) in moved.iter().enumerate() {
            to.inv[gi] += u;
        }
        let gid = self.people[giver].id;
        let receiver = members[0];
        let rid = self.people[receiver].id;
        for &m in members {
            if self.people[m].child(tick) {
                continue;
            }
            self.feel(m, MemoryKind::Helped, Some(gid), 0.8, Emotion::Joy, 0.2);
            let r = self.people[m].mind.rel_mut(gid);
            r.affection += 15.0;
            r.trust += 20.0;
            r.respect += 10.0;
            r.clamp();
        }
        self.feel(giver, MemoryKind::GaveHelp, Some(rid), 0.5, Emotion::Joy, 0.2);
        let gm = &mut self.people[giver].mind;
        gm.needs.esteem = (gm.needs.esteem + 0.15).min(1.0);
        gm.needs.meaning = (gm.needs.meaning + 0.15).min(1.0);
        let family = self.villages[vi]
            .house(hid)
            .map(|h| h.family.clone())
            .unwrap_or_default();
        let text = format!(
            "{} partage ses provisions avec la famille {family}",
            self.people[giver].first
        );
        self.event(Some(village), vec![gid, rid], text);
        given
    }

    // ------------------------------------------------------------------ reprise

    /// Donne un esprit aux habitants d'une sauvegarde qui n'en avait pas (phase 3) :
    /// personnalité, compétences selon l'âge, objectifs, et les amitiés d'alors.
    pub(crate) fn bootstrap_minds(&mut self, friends: Vec<BTreeMap<u32, f32>>) {
        for (i, old) in friends.iter().enumerate() {
            let traits = Traits::random(&mut self.rng);
            let mut mind = Mind::new(traits);
            for (&f, &s) in old {
                let r = mind.rel_mut(f);
                r.affection = s;
                r.trust = s * 0.6;
            }
            self.people[i].mind = mind;
        }
        for i in 0..self.people.len() {
            self.family_ties(i);
            if self.people[i].alive() && !self.people[i].child(self.tick) {
                self.seasoned_skills(i);
                self.pick_goals(i);
            }
        }
    }

    /// Liens de départ avec la famille proche.
    pub(crate) fn family_ties(&mut self, i: usize) {
        let me = self.people[i].id;
        let partner = self.people[i].partner;
        for f in self.family_of(i) {
            let close = if Some(f) == partner { 65.0 } else { 50.0 };
            for (a, b) in [(i, f), (f as usize, me)] {
                let r = self.people[a].mind.rel_mut(b);
                r.affection = r.affection.max(close);
                r.trust = r.trust.max(close);
                if Some(f) == partner {
                    r.attraction = r.attraction.max(55.0);
                }
            }
        }
    }

    /// Un nouveau-né : personnalité en partie héritée.
    pub(crate) fn born_mind(&mut self, i: usize) {
        let parents: Vec<Traits> = self.people[i]
            .parents
            .iter()
            .map(|&q| self.people[q as usize].mind.traits)
            .collect();
        let traits = Traits::inherit(&parents, &mut self.rng);
        self.people[i].mind = Mind::new(traits);
        self.family_ties(i);
    }

    /// Statistiques pour les rapports : humeur moyenne, bien-être moyen, part de chaque
    /// intention parmi les adultes éveillés.
    pub fn mind_stats(&self) -> MindStats {
        let tick = self.tick;
        let mut s = MindStats::default();
        let (mut n, mut workers) = (0.0, 0.0f32);
        for p in self.alive().filter(|p| !p.child(tick)) {
            n += 1.0;
            s.mood += p.mind.mood.valence();
            s.wellbeing += p.mind.needs.wellbeing();
            for (k, v) in p.mind.needs.to_array().iter().enumerate() {
                s.needs[k] += v;
            }
            if let Some(j) = p.job {
                s.skill += p.mind.skill(j);
            }
            s.relations += p.mind.relations.len() as f32;
            s.memories += p.mind.memories.len() as f32;
            if !p.elder(tick)
                && let Some(job) = p.job
            {
                workers += 1.0;
                s.levels[skill_level(p.mind.skill(job))] += 1.0;
                s.intents[intent_index(p.brain.intent)] += 1.0;
                match p.brain.intent {
                    Intent::Work | Intent::Learn(_) => s.working += 1.0,
                    Intent::Chat(_) | Intent::Visit(_) | Intent::Court(_) => s.social += 1.0,
                    _ => {}
                }
            }
        }
        let n: f32 = f32::max(n, 1.0);
        s.mood /= n;
        s.wellbeing /= n;
        s.skill /= n;
        s.relations /= n;
        s.memories /= n;
        let workers = workers.max(1.0);
        s.working /= workers;
        s.social /= workers;
        for v in s.intents.iter_mut().chain(s.levels.iter_mut()) {
            *v /= workers;
        }
        for v in &mut s.needs {
            *v /= n;
        }
        s
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MindStats {
    pub mood: f32,
    pub wellbeing: f32,
    pub needs: [f32; 8],
    pub skill: f32,
    pub relations: f32,
    pub memories: f32,
    /// Part des adultes en âge de travailler qui travaillent (au moment de l'appel).
    pub working: f32,
    /// … qui sont allés voir quelqu'un.
    pub social: f32,
    /// … pour chaque intention (voir [`INTENT_LABELS`]).
    pub intents: [f32; 11],
    /// … à chaque niveau dans leur métier (voir [`LEVEL_LABELS`]).
    pub levels: [f32; 4],
}

pub const LEVEL_LABELS: [&str; 4] = ["débutant", "compagnon", "habile", "maître"];

/// Niveau dans un métier (index de [`LEVEL_LABELS`]).
pub fn skill_level(s: f32) -> usize {
    match s {
        s if s >= MASTERY => 3,
        s if s >= 0.55 => 2,
        s if s >= 0.3 => 1,
        _ => 0,
    }
}

pub const INTENT_LABELS: [&str; 11] = [
    "aucune",
    "dormir",
    "travailler",
    "apprendre",
    "discuter",
    "courtiser",
    "visiter",
    "promenade",
    "repos",
    "jouer",
    "partir",
];

fn intent_index(i: Intent) -> usize {
    match i {
        Intent::None => 0,
        Intent::Sleep => 1,
        Intent::Work => 2,
        Intent::Learn(_) => 3,
        Intent::Chat(_) => 4,
        Intent::Court(_) => 5,
        Intent::Visit(_) => 6,
        Intent::Stroll => 7,
        Intent::Rest => 8,
        Intent::Play => 9,
        Intent::Leave => 10,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Topic {
    Small,
    Gossip,
    Flirt,
    Dispute,
    Advice,
}

const _: () = assert!(JOBS == Job::ALL.len());

#[cfg(test)]
mod tests {
    use crate::{economy::Good, mind::MemoryKind, people::Intent, save::tests::sample, tuning::DAY};

    /// Un foyer affamé, grenier commun vide : un ami généreux partage ses provisions.
    #[test]
    fn a_friend_shares_food() {
        let mut w = sample();
        let tick = w.tick;
        let homes: Vec<(u32, Vec<u32>)> = w.villages[0]
            .houses
            .iter()
            .filter(|h| !h.residents.is_empty())
            .map(|h| (h.id, h.residents.clone()))
            .collect();
        let (ha, a) = (homes[0].0, homes[0].1[0]);
        let (hb, b) = (
            homes[1].0,
            *homes[1]
                .1
                .iter()
                .find(|&&r| !w.people[r as usize].child(tick))
                .unwrap(),
        );
        w.economy.household(ha).inv = [0.0; crate::economy::GOODS];
        w.economy.household(hb).inv[Good::Grain as usize] = 500.0;
        w.villages[0].stock.food = 0.0;
        w.people[b as usize].mind.traits.generosity = 1.0;
        w.people[b as usize].mind.traits.kindness = 1.0;
        for (x, y) in [(a, b), (b, a)] {
            w.people[x as usize].mind.rel_mut(y).affection = 70.0;
        }
        // Le partage n'a lieu que 7 fois sur 10 : on laisse quelques repas passer.
        for _ in 0..5 {
            w.meals();
            if w.people[a as usize]
                .mind
                .memories
                .iter()
                .any(|m| m.kind == MemoryKind::Helped)
            {
                break;
            }
            w.economy.household(ha).inv = [0.0; crate::economy::GOODS];
        }
        let m = &w.people[a as usize].mind;
        assert!(
            m.memories
                .iter()
                .any(|m| m.kind == MemoryKind::Helped && m.who == Some(b))
        );
        assert!(m.rel(b).trust >= 20.0);
        assert!(
            w.people[b as usize]
                .mind
                .memories
                .iter()
                .any(|m| m.kind == MemoryKind::GaveHelp)
        );
    }

    /// Après deux semaines, les habitants se connaissent, se souviennent, ont des projets, et
    /// la même graine redonne exactement la même histoire.
    #[test]
    fn minds_come_alive_deterministically() {
        let run = || {
            let mut w = sample();
            let mut intents = std::collections::BTreeSet::new();
            for _ in 0..14 * DAY {
                w.step();
                for p in w.alive() {
                    intents.insert(format!("{:?}", std::mem::discriminant(&p.brain.intent)));
                }
            }
            (w, intents)
        };
        let (w, intents) = run();
        let tick = w.tick;
        let adults: Vec<_> = w.alive().filter(|p| !p.child(tick)).collect();
        assert!(adults.iter().all(|p| !p.mind.goals.is_empty()));
        assert!(
            adults
                .iter()
                .any(|p| p.mind.relations.values().any(|r| r.affection >= 15.0))
        );
        assert!(w.alive().any(|p| !p.mind.memories.is_empty()));
        assert!(w.reputations().values().any(|&r| r > 0.0));
        // Travail, discussions et repos alternent.
        for i in [Intent::Work, Intent::Chat(0), Intent::Sleep] {
            assert!(
                intents.contains(&format!("{:?}", std::mem::discriminant(&i))),
                "{i:?} jamais choisi"
            );
        }
        let (again, _) = run();
        assert_eq!(
            w.people.iter().map(|p| &p.mind).collect::<Vec<_>>(),
            again.people.iter().map(|p| &p.mind).collect::<Vec<_>>()
        );
    }
}

//! Mode dieu (phase 8) : pouvoirs, foi, croyance des habitants, cultes, prêtres et temples.
//!
//! Chaque commande est appliquée entre deux ticks (au tick courant), coûte de la foi et
//! laisse une trace dans la chronique (« Le Ciel… »), dans le journal des actes et dans la
//! mémoire de ceux qui en sont témoins.

use crate::{
    clock::Season,
    economy::Workshop,
    god::{BELIEVER, Deed, GodCommand, KEEP_DEEDS, Power, Spell, SpellKind, belief_label},
    map::{Biome, Resource},
    mind::{Emotion, Goal, LifeGoal, MemoryKind},
    people::Act,
    settlement::{FieldState, HouseState},
    society::{DISEASES, Disease, Tech},
    trade::pair,
    tuning::*,
    world::{World, occ},
};

use super::{
    found::is_quarry,
    life::{at_place, de},
    tick_day,
    trade::MAX_VILLAGES,
};

/// Rayon (cases) d'où l'on voit un prodige sur la carte (forêt, gisement).
const SEEN_FROM: f32 = 45.0;

impl World {
    /// Applique un pouvoir du mode dieu. Refusé (sans rien changer ni rien coûter) si la
    /// cible n'existe pas, si le pouvoir n'a pas de sens ici ou s'il manque de la foi.
    pub fn god_command(&mut self, cmd: &GodCommand) -> Result<Deed, String> {
        self.ensure_caches();
        let power = cmd.power();
        if self.god.faith + 1e-3 < power.cost {
            self.god.stats.refused += 1;
            return Err(format!(
                "Pas assez de foi : {} en demande {:.0}, il n'en reste que {:.0}",
                power.label,
                power.cost,
                self.god.faith.floor()
            ));
        }
        let (village, people, text) = match self.apply_power(cmd, power) {
            Ok(r) => r,
            Err(e) => {
                self.god.stats.refused += 1;
                return Err(e);
            }
        };
        self.god.faith = (self.god.faith - power.cost).max(0.0);
        let g = &mut self.god;
        g.stats.commands += 1;
        g.stats.spent += power.cost;
        *g.stats.by_power.entry(power.key.to_string()).or_default() += 1;
        let deed = Deed {
            tick: self.tick,
            command: cmd.clone(),
            cost: power.cost,
            text: text.clone(),
        };
        g.deeds.push_back(deed.clone());
        while g.deeds.len() > KEEP_DEEDS {
            g.deeds.pop_front();
        }
        self.event(village, people, text);
        Ok(deed)
    }

    fn vi_of(&self, village: u32) -> Result<usize, String> {
        self.village_index(village)
            .ok_or_else(|| format!("Aucun village n°{village}"))
    }

    /// Un habitant vivant (et toujours là).
    fn living(&self, person: u32) -> Result<usize, String> {
        match self.people.get(person as usize) {
            Some(p) if p.alive() && !p.brain.leaving => Ok(person as usize),
            Some(p) => Err(format!("{} n'est plus de ce monde", p.name())),
            None => Err(format!("Personne ne porte le n°{person}")),
        }
    }

    fn residents(&self, village: u32) -> Vec<usize> {
        (0..self.people.len())
            .filter(|&i| {
                let p = &self.people[i];
                p.alive() && p.village == village && !p.brain.leaving
            })
            .collect()
    }

    fn spell(&mut self, village: u32, kind: SpellKind, ticks: u64) {
        let until = self.tick + ticks;
        self.god
            .spells
            .retain(|s| !(s.village == village && s.kind == kind));
        self.god.spells.push(Spell { village, kind, until });
    }

    /// Village le plus proche d'une case, s'il est assez près pour avoir vu le prodige.
    fn witness_village(&self, x: u32, y: u32) -> Option<usize> {
        self.villages
            .iter()
            .enumerate()
            .map(|(vi, v)| (vi, (v.x as f32 - x as f32).hypot(v.y as f32 - y as f32)))
            .filter(|&(_, d)| d <= SEEN_FROM)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(vi, _)| vi)
    }

    /// `(village de la chronique, habitants cités, texte)`.
    #[allow(clippy::type_complexity)]
    fn apply_power(
        &mut self,
        cmd: &GodCommand,
        power: &Power,
    ) -> Result<(Option<u32>, Vec<u32>, String), String> {
        let tick = self.tick;
        let season = self.clock().season;
        match *cmd {
            // ---------------------------------------------------------------- nature
            GodCommand::Rain { village } => {
                let vi = self.vi_of(village)?;
                let dry = self.god.active(village, SpellKind::Drought, tick);
                self.god
                    .spells
                    .retain(|s| !(s.village == village && s.kind == SpellKind::Drought));
                self.spell(village, SpellKind::Rain, 2 * DAY);
                self.witness(vi, power, 0.10);
                let name = &self.villages[vi].name;
                let text = match (dry, season) {
                    (true, _) => format!("Le Ciel fait pleuvoir sur {name} : la sécheresse prend fin"),
                    (false, Season::Winter) => format!("Le Ciel fait tomber la neige sur {name}"),
                    _ => format!("Le Ciel fait pleuvoir sur {name} : les champs reverdissent"),
                };
                Ok((Some(village), vec![], text))
            }
            GodCommand::Drought { village } => {
                let vi = self.vi_of(village)?;
                self.god
                    .spells
                    .retain(|s| !(s.village == village && s.kind == SpellKind::Rain));
                self.spell(village, SpellKind::Drought, 3 * DAY);
                self.witness(vi, power, 0.08);
                let text = format!("Le Ciel envoie la sécheresse sur {}", self.villages[vi].name);
                Ok((Some(village), vec![], text))
            }
            GodCommand::Storm { village } => {
                let vi = self.vi_of(village)?;
                let (mut flattened, mut damaged) = (0, false);
                for k in 0..self.villages[vi].fields.len() {
                    let roll = self.rng.f32();
                    let f = &mut self.villages[vi].fields[k];
                    match f.state {
                        FieldState::Ripe if roll < 0.35 => {
                            f.state = FieldState::Fallow;
                            f.work = 0.0;
                            f.growth = 0.0;
                            flattened += 1;
                        }
                        FieldState::Sown => f.growth *= 0.7,
                        _ => {}
                    }
                }
                let sites: Vec<u32> = self.villages[vi]
                    .houses
                    .iter()
                    .filter(|h| h.state == HouseState::Building && h.wood_in + h.stone_in > 0)
                    .map(|h| h.id)
                    .collect();
                if let Some(&hid) = self.rng.pick(&sites) {
                    let h = self.villages[vi].house_mut(hid).unwrap();
                    let (w, s) = (h.wood_in / 2, h.stone_in / 2);
                    h.wood_in -= w;
                    h.stone_in -= s;
                    h.wood_needed += w;
                    h.stone_needed += s;
                    damaged = true;
                }
                // Chacun court s'abriter.
                let day = (7..21).contains(&self.clock().hour);
                for i in self.residents(village) {
                    let b = &self.people[i].brain;
                    if day && !b.hidden && !b.traveling && !self.society.sick.contains_key(&self.people[i].id)
                    {
                        self.near_home(i, HOUR, Act::Idle);
                    }
                }
                self.spell(village, SpellKind::Storm, 6 * HOUR);
                self.witness(vi, power, 0.10);
                let mut text = format!("Le Ciel déchaîne l'orage sur {}", self.villages[vi].name);
                let s = if flattened > 1 { "s" } else { "" };
                match (flattened, damaged) {
                    (0, false) => {}
                    (0, true) => text += " : un chantier est abîmé",
                    (n, false) => text += &format!(" : {n} champ{s} couché{s}"),
                    (n, true) => text += &format!(" : {n} champ{s} couché{s}, un chantier abîmé"),
                }
                Ok((Some(village), vec![], text))
            }
            GodCommand::Bounty { village } => {
                let vi = self.vi_of(village)?;
                let mut ripe = 0;
                for f in &mut self.villages[vi].fields {
                    if f.state == FieldState::Sown {
                        f.state = FieldState::Ripe;
                        f.growth = 1.0;
                        f.work = 0.0;
                        ripe += 1;
                    }
                }
                let name = self.villages[vi].name.clone();
                let text = if ripe > 0 {
                    let s = if ripe > 1 { "s" } else { "" };
                    format!("Le Ciel fait mûrir d'un coup {ripe} champ{s} {}", de(&name))
                } else {
                    let food = (self.population(village) as f32 * 4.0).max(10.0);
                    let st = &mut self.villages[vi].stock;
                    st.food = (st.food + food).min(MAX_FOOD);
                    format!("Une manne tombe du Ciel sur {name} : {food:.0} rations au grenier")
                };
                self.spell(village, SpellKind::Bounty, DAY);
                self.witness(vi, power, 0.15);
                Ok((Some(village), vec![], text))
            }
            GodCommand::Locusts { village } => {
                let vi = self.vi_of(village)?;
                let mut eaten = 0;
                for k in 0..self.villages[vi].fields.len() {
                    let roll = self.rng.f32();
                    let f = &mut self.villages[vi].fields[k];
                    if matches!(f.state, FieldState::Sown | FieldState::Ripe) && roll < 0.75 {
                        f.state = FieldState::Fallow;
                        f.work = 0.0;
                        f.growth = 0.0;
                        eaten += 1;
                    }
                }
                let st = &mut self.villages[vi].stock;
                let lost = st.food * 0.2;
                st.food -= lost;
                self.spell(village, SpellKind::Locusts, 12 * HOUR);
                self.witness(vi, power, 0.12);
                let name = &self.villages[vi].name;
                let s = if eaten > 1 { "s" } else { "" };
                let text = if eaten > 0 {
                    format!("Le Ciel envoie une nuée de sauterelles sur {name} : {eaten} champ{s} dévoré{s}")
                } else {
                    format!(
                        "Le Ciel envoie une nuée de sauterelles sur {name} : {lost:.0} rations perdues au grenier"
                    )
                };
                Ok((Some(village), vec![], text))
            }
            GodCommand::Abundance { village } => {
                let vi = self.vi_of(village)?;
                self.spell(village, SpellKind::Abundance, 5 * DAY);
                self.witness(vi, power, 0.08);
                let text = format!(
                    "Le Ciel peuple de gibier et de poisson les environs {}",
                    de(&self.villages[vi].name)
                );
                Ok((Some(village), vec![], text))
            }
            GodCommand::ClearSky { village } => {
                let vi = self.vi_of(village)?;
                let storm = self.god.active(village, SpellKind::Storm, tick);
                self.god
                    .spells
                    .retain(|s| !(s.village == village && s.kind == SpellKind::Storm));
                let gone = self.clear_sky(vi);
                if gone == 0 && !storm {
                    return Err(format!(
                        "Le ciel est déjà dégagé au-dessus {}",
                        de(&self.villages[vi].name)
                    ));
                }
                self.witness(vi, power, 0.06);
                let text = format!(
                    "Le Ciel dissipe les nuages au-dessus {}",
                    de(&self.villages[vi].name)
                );
                Ok((Some(village), vec![], text))
            }
            GodCommand::Cold { village } => {
                let vi = self.vi_of(village)?;
                self.divine_front(vi, crate::weather::FrontKind::Cold);
                self.witness(vi, power, 0.08);
                let name = &self.villages[vi].name;
                let text = if season == Season::Winter {
                    format!("Le Ciel envoie un froid mortel sur {name}")
                } else {
                    format!("Le Ciel envoie une vague de froid sur {name}")
                };
                Ok((Some(village), vec![], text))
            }
            GodCommand::Flood { village } => {
                let vi = self.vi_of(village)?;
                let what = self.flood(vi, true)?;
                self.witness(vi, power, 0.12);
                let text = what.replacen("Crue : la rivière déborde", "Le Ciel fait déborder la rivière", 1);
                Ok((Some(village), vec![], text))
            }
            GodCommand::Wildfire { x, y } => {
                if !self.map.inside(x as i64, y as i64) {
                    return Err("Cette case est hors de la carte".into());
                }
                let tile = self
                    .forest_around(x, y)
                    .ok_or("Pas de forêt ici : il faut des arbres pour un incendie")?;
                let (tx, ty) = self.map.xy(tile as usize);
                let text = self
                    .start_fire(tile, true, "le Ciel")
                    .ok_or("Le feu ne prend pas : un incendie brûle déjà près d'ici")?;
                if let Some(vi) = self.witness_village(tx, ty) {
                    self.witness(vi, power, 0.1);
                }
                let fire = self.weather.fires.last().map(|f| f.village).unwrap_or(None);
                let text = text
                    .replacen("Incendie de forêt", "Le Ciel embrase la forêt", 1)
                    .replace(" : le Ciel a mis le feu aux bois", "");
                Ok((fire, vec![], text))
            }
            GodCommand::Forest { x, y } => {
                if !self.map.inside(x as i64, y as i64) {
                    return Err("Cette case est hors de la carte".into());
                }
                let mut grown = 0;
                let r = 6i64;
                for dy in -r..=r {
                    for dx in -r..=r {
                        let (tx, ty) = (x as i64 + dx, y as i64 + dy);
                        let d = ((dx * dx + dy * dy) as f32).sqrt();
                        if d > r as f32 + 0.3 || !self.map.inside(tx, ty) {
                            continue;
                        }
                        let i = self.map.idx(tx as u32, ty as u32);
                        let free = matches!(
                            self.map.biome_at(i),
                            Biome::Grassland | Biome::Meadow | Biome::Heath
                        ) && self.occ(i) == occ::NONE
                            && !self.map.road_at(i)
                            && self.map.river_at(i) == 0;
                        if free && self.rng.chance(0.95 - 0.08 * d) {
                            self.set_biome(i, Biome::Forest);
                            grown += 1;
                        }
                    }
                }
                if grown == 0 {
                    return Err("Rien ne peut pousser ici : il faut des prés libres".into());
                }
                let near = self.witness_village(x, y);
                if let Some(vi) = near {
                    self.witness(vi, power, 0.05);
                }
                let text = match near {
                    Some(vi) => format!(
                        "Le Ciel fait pousser une forêt près {} ({grown} case{})",
                        de(&self.villages[vi].name),
                        if grown > 1 { "s" } else { "" }
                    ),
                    None => format!(
                        "Le Ciel fait pousser une forêt loin de tout village ({grown} case{})",
                        if grown > 1 { "s" } else { "" }
                    ),
                };
                Ok((near.map(|vi| self.villages[vi].id), vec![], text))
            }
            GodCommand::Deposit { x, y, resource } => {
                if !self.map.inside(x as i64, y as i64) {
                    return Err("Cette case est hors de la carte".into());
                }
                let res = Resource::ALL
                    .into_iter()
                    .find(|r| *r as u8 == resource && *r != Resource::None)
                    .ok_or_else(|| format!("Ressource inconnue ({resource})"))?;
                let wet = |w: &World, i: usize| w.map.biome_at(i).is_water() || w.map.river_at(i) > 0;
                let mut placed = 0;
                for dy in -1i64..=1 {
                    for dx in -1i64..=1 {
                        let (tx, ty) = (x as i64 + dx, y as i64 + dy);
                        if !self.map.inside(tx, ty) {
                            continue;
                        }
                        let i = self.map.idx(tx as u32, ty as u32);
                        let ok = if res == Resource::Fish {
                            wet(self, i)
                        } else {
                            !self.map.biome_at(i).is_water() && self.occ(i) == occ::NONE
                        };
                        if ok && self.map.resource[i] == Resource::None as u8 {
                            self.edit_tile(i, |m, i| m.resource[i] = res as u8);
                            placed += 1;
                        }
                    }
                }
                if placed == 0 {
                    return Err(if res == Resource::Fish {
                        "Il faut de l'eau pour du poisson".into()
                    } else {
                        "Pas de place ici : il faut une terre libre".into()
                    });
                }
                // Les villages à portée en profitent.
                for vi in 0..self.villages.len() {
                    let v = &self.villages[vi];
                    let d = (v.x as f32 - x as f32).hypot(v.y as f32 - y as f32);
                    if d <= RESOURCE_RADIUS as f32 * 3.5 {
                        self.compute_access(vi);
                    }
                    if d <= RESOURCE_RADIUS as f32 * 1.5 && !self.villages[vi].has_stone {
                        let (vx, vy) = (self.villages[vi].x, self.villages[vi].y);
                        if is_quarry(&self.map, self.map.idx(x, y)) || self.stone_nearby(vx, vy) {
                            self.villages[vi].has_stone = self.stone_nearby(vx, vy);
                        }
                    }
                }
                let near = self.witness_village(x, y);
                if let Some(vi) = near {
                    self.witness(vi, power, 0.05);
                }
                let what = match res {
                    Resource::Stone => "un gisement de pierre",
                    Resource::Iron => "un gisement de fer",
                    Resource::Clay => "un gisement d'argile",
                    Resource::Fish => "des eaux poissonneuses",
                    Resource::Game => "un bois giboyeux",
                    _ => "des buissons de baies",
                };
                let place = near.map_or("loin de tout village".to_string(), |vi| {
                    format!("près {}", de(&self.villages[vi].name))
                });
                Ok((
                    near.map(|vi| self.villages[vi].id),
                    vec![],
                    format!("Le Ciel révèle {what} {place}"),
                ))
            }

            // ---------------------------------------------------------------- fléaux
            GodCommand::Cure { village } => {
                let vi = self.vi_of(village)?;
                let sick: Vec<u32> = self
                    .society
                    .sick
                    .keys()
                    .copied()
                    .filter(|&id| self.people[id as usize].village == village)
                    .collect();
                if sick.is_empty() {
                    return Err(format!(
                        "Personne n'est malade {}",
                        at_place(&self.villages[vi].name)
                    ));
                }
                for &id in &sick {
                    self.cure(id as usize);
                }
                if let Some(p) = self.society.polities.get_mut(&village) {
                    p.epidemic = None;
                }
                self.witness(vi, power, 0.15);
                let n = sick.len();
                let s = if n > 1 { "s" } else { "" };
                let text = format!(
                    "Le Ciel guérit {n} malade{s} {}",
                    at_place(&self.villages[vi].name)
                );
                Ok((Some(village), vec![], text))
            }
            GodCommand::Plague { village, disease } => {
                let vi = self.vi_of(village)?;
                let d = *Disease::ALL
                    .get(disease as usize)
                    .ok_or_else(|| format!("Maladie inconnue ({disease}, de 0 à {})", DISEASES - 1))?;
                let mut healthy: Vec<usize> = self
                    .residents(village)
                    .into_iter()
                    .filter(|&i| {
                        let id = self.people[i].id;
                        !self.society.sick.contains_key(&id) && !self.society.immune(id, d, tick)
                    })
                    .collect();
                if healthy.is_empty() {
                    return Err(format!(
                        "Personne ne peut attraper {} {}",
                        d.the(),
                        at_place(&self.villages[vi].name)
                    ));
                }
                let n = 3.min(healthy.len());
                let mut hit = Vec::new();
                for _ in 0..n {
                    let k = self.rng.below(healthy.len() as u64) as usize;
                    let i = healthy.swap_remove(k);
                    self.infect(i, d, village);
                    hit.push(self.people[i].id);
                }
                self.witness(vi, power, 0.10);
                let s = if n > 1 { "s" } else { "" };
                let text = format!(
                    "Le Ciel frappe {} : {} se déclare chez {n} habitant{s}",
                    self.villages[vi].name,
                    d.the()
                );
                Ok((Some(village), hit, text))
            }

            // ---------------------------------------------------------------- villageois
            GodCommand::Heal { person } => {
                let i = self.living(person)?;
                let Some(s) = self.society.sick.get(&person).copied() else {
                    return Err(format!("{} n'est pas malade", self.people[i].name()));
                };
                self.cure(i);
                self.touched(i, true, 0.35, MemoryKind::Miracle);
                let the = s.disease.the();
                let of = match the.strip_prefix("le ") {
                    Some(rest) => format!("du {rest}"),
                    None => format!("de {the}"),
                };
                let text = format!("Le Ciel guérit {} {of}", self.people[i].name());
                Ok((Some(self.people[i].village), vec![person], text))
            }
            GodCommand::Bless { person } => {
                let i = self.living(person)?;
                let job = self.people[i].job;
                let m = &mut self.people[i].mind;
                if let Some(j) = job {
                    let s = m.skill_mut(j);
                    *s = (*s + 0.12).min(1.0);
                }
                m.needs.esteem = (m.needs.esteem + 0.3).min(1.0);
                m.needs.meaning = (m.needs.meaning + 0.3).min(1.0);
                self.touched(i, true, 0.4, MemoryKind::Blessed);
                // On regarde l'élu du Ciel avec respect.
                let village = self.people[i].village;
                for k in self.residents(village) {
                    if k != i {
                        let r = self.people[k].mind.rel_mut(person);
                        r.respect += 4.0;
                        r.clamp();
                    }
                }
                let text = format!("Le Ciel bénit {}", self.people[i].name());
                Ok((Some(village), vec![person], text))
            }
            GodCommand::Curse { person } => {
                let i = self.living(person)?;
                let job = self.people[i].job;
                let m = &mut self.people[i].mind;
                if let Some(j) = job {
                    let s = m.skill_mut(j);
                    *s = (*s - 0.12).max(0.0);
                }
                m.needs.safety = (m.needs.safety - 0.5).max(0.0);
                m.mood.add(Emotion::Sadness, 0.4);
                self.touched(i, false, 0.4, MemoryKind::Cursed);
                let village = self.people[i].village;
                let fever = !self.society.sick.contains_key(&person)
                    && !self.society.immune(person, Disease::Fever, tick)
                    && self.rng.chance(0.35);
                if fever {
                    self.infect(i, Disease::Fever, village);
                }
                // On évite le maudit, porte-malheur.
                for k in self.residents(village) {
                    if k != i {
                        let r = self.people[k].mind.rel_mut(person);
                        r.affection -= 4.0;
                        r.clamp();
                    }
                }
                let mut text = format!("Le Ciel maudit {}", self.people[i].name());
                if fever {
                    text += ", que la fièvre prend aussitôt";
                }
                Ok((Some(village), vec![person], text))
            }
            GodCommand::Inspire { person } => {
                let i = self.living(person)?;
                if self.people[i].child(tick) {
                    return Err(format!(
                        "{} est trop jeune pour une telle idée",
                        self.people[i].first
                    ));
                }
                let village = self.people[i].village;
                let vi = self.vi_of(village)?;
                let job = self.people[i].job;
                let epidemic = self.society.polity(village).is_some_and(|p| p.epidemic.is_some());
                let tech = Tech::ALL.into_iter().find(|t| {
                    !self.society.knows(village, *t)
                        && match t.job() {
                            Some(j) => job == Some(j),
                            None => epidemic,
                        }
                });
                let name = self.people[i].name();
                self.touched(i, true, 0.35, MemoryKind::Miracle);
                let text = if let Some(t) = tech {
                    self.discover(vi, t, i);
                    format!("Le Ciel souffle une idée à {name} : {}", t.label())
                } else {
                    // Pas de technique à trouver : un grand rêve, et du cœur à l'ouvrage.
                    let master = job.is_some_and(|j| self.people[i].mind.skill(j) >= crate::mind::MASTERY);
                    let goal = if master { Goal::Respect } else { Goal::Master };
                    let m = &mut self.people[i].mind;
                    if !m.has_goal(goal) {
                        m.goals.push(LifeGoal { goal, reached: None });
                    }
                    if let Some(j) = job {
                        let s = m.skill_mut(j);
                        *s = (*s + 0.06).min(1.0);
                    }
                    m.needs.meaning = 1.0;
                    format!("Le Ciel inspire {name}, qui rêve désormais de {}", goal.label())
                };
                Ok((Some(village), vec![person], text))
            }
            GodCommand::Matchmake { a, b } => {
                let (i, j) = (self.living(a)?, self.living(b)?);
                if i == j {
                    return Err("Il faut deux personnes différentes".into());
                }
                for k in [i, j] {
                    if !self.single(k) {
                        let p = &self.people[k];
                        return Err(format!(
                            "{} n'a pas le cœur libre (il faut un adulte de 18 à 55 ans sans conjoint)",
                            p.name()
                        ));
                    }
                }
                if self.related(a, b) {
                    return Err("Ils sont de la même famille".into());
                }
                for (k, other) in [(i, b), (j, a)] {
                    let r = self.people[k].mind.rel_mut(other);
                    r.attraction = r.attraction.max(85.0);
                    r.affection += 25.0;
                    r.rivalry = 0.0;
                    r.clamp();
                    self.people[k].mind.mood.add(Emotion::Joy, 0.4);
                    self.touched(k, true, 0.15, MemoryKind::Miracle);
                }
                let text = format!(
                    "Le Ciel fait se rencontrer {} et {}",
                    self.people[i].name(),
                    self.people[j].name()
                );
                Ok((Some(self.people[i].village), vec![a, b], text))
            }

            // ---------------------------------------------------------------- société
            GodCommand::Sign => {
                for i in 0..self.people.len() {
                    let p = &self.people[i];
                    if !p.alive() || p.brain.leaving {
                        continue;
                    }
                    let t = p.mind.traits;
                    let id = p.id;
                    let b = self.god.belief(id);
                    let gain = 0.07 * (0.3 + 0.7 * t.piety);
                    self.god.belief.insert(id, b + gain * (1.0 - b));
                    let m = &mut self.people[i].mind;
                    if t.piety >= 0.5 {
                        m.remember(tick, MemoryKind::Omen, None, 0.5);
                        m.mood.add(Emotion::Joy, 0.2);
                    } else {
                        m.remember(tick, MemoryKind::Omen, None, -0.3);
                        m.mood.add(Emotion::Fear, 0.15 + 0.2 * t.nervousness);
                    }
                }
                for c in self.god.cults.values_mut() {
                    c.love += 0.05;
                    c.fear += 0.05;
                }
                self.god.sign = tick;
                Ok((
                    None,
                    vec![],
                    "Le Ciel fait paraître un signe : une lueur immense, vue de tous les villages".into(),
                ))
            }
            GodCommand::Feast { village } => {
                let vi = self.vi_of(village)?;
                let pop = self.population(village);
                if pop < 2 {
                    return Err("Il faut au moins deux habitants pour faire la fête".into());
                }
                let st = &mut self.villages[vi].stock;
                st.food = (st.food + pop as f32 * 0.6).min(MAX_FOOD);
                let name = self.villages[vi].name.clone();
                let text = if (6..17).contains(&self.clock().hour) {
                    self.society.polity_mut(village).feast_day = Some(tick_day(tick));
                    format!(
                        "Le Ciel offre une fête {} : on danse autour du puits",
                        at_place(&name)
                    )
                } else {
                    if !self.god.feasts.contains(&village) {
                        self.god.feasts.push(village);
                    }
                    format!("Le Ciel promet une fête {} pour demain", at_place(&name))
                };
                self.witness(vi, power, 0.06);
                Ok((Some(village), vec![], text))
            }
            GodCommand::Strangers { village } => {
                let vi = self.vi_of(village)?;
                let empty = self.villages[vi]
                    .houses
                    .iter()
                    .find(|h| h.state == HouseState::Abandoned && !self.economy.workshops.contains_key(&h.id))
                    .map(|h| h.id);
                let hid = match empty {
                    Some(h) => h,
                    None => self.new_house(vi, HouseState::Inhabited).ok_or_else(|| {
                        format!(
                            "Plus de place pour une maison {}",
                            at_place(&self.villages[vi].name)
                        )
                    })?,
                };
                let last = self.rng.pick(crate::names::LAST).unwrap().to_string();
                let h = self.villages[vi].house_mut(hid).unwrap();
                h.state = HouseState::Inhabited;
                h.since = tick;
                h.wood_needed = 0;
                h.stone_needed = 0;
                h.reserved_for = None;
                h.family = last.clone();
                let (age_a, age_b) = (20.0 + self.rng.f32() * 14.0, 21.0 + self.rng.f32() * 14.0);
                let a = self.arrive(vi, &last, true, age_a, hid);
                let b = self.arrive(vi, &last, false, age_b, hid);
                self.couple(a, b);
                let mut family = vec![a, b];
                for _ in 0..self.rng.below(3) {
                    let female = self.rng.chance(0.5);
                    let age = 1.0 + self.rng.f32() * 12.0;
                    family.push(self.arrive(vi, &last, female, age, hid));
                }
                self.open_household(hid, 30.0, 60.0);
                // Des pèlerins : ils croient, et le disent.
                for &p in &family {
                    let i = p as usize;
                    let piety = self.people[i].mind.traits.piety;
                    self.people[i].mind.traits.piety = piety.max(0.6);
                    self.god.belief.insert(p, 0.75);
                }
                self.witness(vi, power, 0.04);
                let text = format!(
                    "Envoyée par le Ciel, une famille de pèlerins arrive : {} et {} {last}",
                    self.people[a as usize].first, self.people[b as usize].first
                );
                Ok((Some(village), vec![a, b], text))
            }
            GodCommand::Rename { village, ref name } => {
                let vi = self.vi_of(village)?;
                let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
                let n = name.chars().count();
                if !(2..=32).contains(&n) {
                    return Err("Un nom de village a de 2 à 32 lettres".into());
                }
                if !name
                    .chars()
                    .all(|c| c.is_alphabetic() || matches!(c, ' ' | '-' | '\'' | '’'))
                    || !name.chars().next().is_some_and(char::is_alphabetic)
                {
                    return Err("Un nom de village ne contient que des lettres, des espaces, des traits d'union et des apostrophes".into());
                }
                if self
                    .villages
                    .iter()
                    .any(|v| v.id != village && v.name.to_lowercase() == name.to_lowercase())
                {
                    return Err(format!("Un autre village s'appelle déjà {name}"));
                }
                let old = std::mem::replace(&mut self.villages[vi].name, name.clone());
                if old == name {
                    return Err(format!("Le village s'appelle déjà {name}"));
                }
                self.witness(vi, power, 0.04);
                Ok((
                    Some(village),
                    vec![],
                    format!("Le Ciel renomme {old} : le village s'appelle désormais {name}"),
                ))
            }

            // ---------------------------------------------------------------- création
            GodCommand::Road { from, to } => {
                let (a, b) = (self.vi_of(from)?, self.vi_of(to)?);
                if a == b {
                    return Err("Il faut deux villages différents".into());
                }
                let (na, nb) = (self.villages[a].name.clone(), self.villages[b].name.clone());
                if self.trade.route(from, to).is_some_and(|r| r.road) {
                    return Err(format!("{na} et {nb} sont déjà reliés par une route"));
                }
                if !self.ensure_route(a, b) {
                    return Err(format!("Aucun passage praticable entre {na} et {nb}"));
                }
                let r = self.trade.routes.get_mut(&pair(from, to)).unwrap();
                r.road = true;
                let path = r.path.clone();
                for t in path {
                    let t = t as usize;
                    if self.occ(t) == occ::NONE {
                        self.clear_forest(t);
                        self.set_road(t);
                    }
                }
                self.witness(a, power, 0.05);
                self.witness(b, power, 0.05);
                Ok((
                    Some(from),
                    vec![],
                    format!("Le Ciel trace une route entre {na} et {nb}"),
                ))
            }
            GodCommand::Found { x, y } => {
                if self.villages.len() >= MAX_VILLAGES {
                    return Err(format!("Le monde compte déjà {MAX_VILLAGES} villages"));
                }
                let (xi, yi) = (x as i64, y as i64);
                if !self.map.inside(xi - 8, yi - 8) || !self.map.inside(xi + 8, yi + 8) {
                    return Err("Trop près du bord du monde".into());
                }
                let i = self.map.idx(x, y);
                if !matches!(
                    self.map.biome_at(i),
                    Biome::Meadow | Biome::Grassland | Biome::Forest | Biome::Heath | Biome::Hills
                ) || self.map.river_at(i) > 0
                    || self.occ(i) != occ::NONE
                {
                    return Err("Il faut une terre ferme et libre (pré, lande, forêt ou colline)".into());
                }
                if let Some(v) = self
                    .villages
                    .iter()
                    .find(|v| (v.x as f32 - x as f32).hypot(v.y as f32 - y as f32) < 40.0)
                {
                    return Err(format!("Trop près de {} (40 cases au moins)", v.name));
                }
                if self.civic_spot(x, y).is_none() {
                    return Err("Pas assez de place ici pour un grenier".into());
                }
                let before = self.villages.len();
                let site = self.new_site(x, y);
                self.found_village(&site);
                if self.villages.len() == before {
                    return Err("Les familles n'ont pas pu s'installer ici".into());
                }
                let vi = self.villages.len() - 1;
                let id = self.villages[vi].id;
                // Guidés par le Ciel, les fondateurs y croient.
                for k in self.residents(id) {
                    let pid = self.people[k].id;
                    self.god.belief.insert(pid, 0.6);
                }
                self.economy.ensure_village(vi);
                self.compute_access(vi);
                self.witness(vi, power, 0.05);
                Ok((
                    Some(id),
                    vec![],
                    format!(
                        "Le Ciel guide des familles jusqu'à une terre nouvelle : elles fondent {}",
                        site.name
                    ),
                ))
            }
        }
    }

    /// Guérison miraculeuse : la maladie part, une protection reste.
    fn cure(&mut self, i: usize) {
        let id = self.people[i].id;
        if let Some(s) = self.society.sick.remove(&id) {
            let r = self.society.recovered.entry(id).or_insert([0; DISEASES]);
            r[s.disease as usize] = self.tick + 1;
            let b = &mut self.people[i].brain;
            if b.act == Act::Ill {
                b.plan.clear();
                b.wait = 0;
                b.act = Act::Idle;
            }
        }
    }

    /// Les habitants d'un village voient un prodige : ils y croient (selon leur piété), s'en
    /// souviennent, se réjouissent ou prennent peur ; le village en garde gratitude ou crainte.
    fn witness(&mut self, vi: usize, power: &Power, strength: f32) {
        let tick = self.tick;
        let id = self.villages[vi].id;
        let benign = power.benign;
        for i in self.residents(id) {
            let p = &self.people[i];
            let (pid, t) = (p.id, p.mind.traits);
            let b = self.god.belief(pid);
            let gain = if benign {
                strength * (0.3 + 0.7 * t.piety)
            } else {
                strength * (0.15 + 0.5 * t.piety + 0.35 * t.nervousness)
            };
            self.god.belief.insert(pid, (b + gain * (1.0 - b)).min(1.0));
            let m = &mut self.people[i].mind;
            if benign {
                m.remember(tick, MemoryKind::Miracle, None, 0.3 + strength * 2.0);
                m.mood.add(Emotion::Joy, 0.1 + strength);
                m.needs.meaning = (m.needs.meaning + strength).min(1.0);
            } else {
                m.remember(tick, MemoryKind::Scourge, None, -(0.3 + strength * 2.5));
                m.mood.add(Emotion::Fear, 0.1 + strength * (1.0 + t.nervousness));
                m.needs.safety = (m.needs.safety - strength * 2.0).max(0.0);
            }
        }
        let c = self.god.cults.entry(id).or_default();
        let k = strength / 0.1;
        if benign {
            c.love += 0.25 * k;
            c.miracles += 1;
        } else {
            c.fear += 0.3 * k;
            c.scourges += 1;
        }
    }

    /// Un habitant touché en personne (béni, guéri, maudit) : il y croit bien plus, et sa
    /// famille un peu.
    fn touched(&mut self, i: usize, benign: bool, strength: f32, kind: MemoryKind) {
        let tick = self.tick;
        let (pid, village) = (self.people[i].id, self.people[i].village);
        let t = self.people[i].mind.traits;
        let b = self.god.belief(pid);
        let gain = strength * (0.5 + 0.5 * t.piety);
        self.god.belief.insert(pid, (b + gain * (1.0 - b)).min(1.0));
        let m = &mut self.people[i].mind;
        if benign {
            m.remember(tick, kind, None, 0.9);
            m.mood.add(Emotion::Joy, 0.5);
        } else {
            m.remember(tick, kind, None, -0.9);
            m.mood.add(Emotion::Fear, 0.5);
        }
        for f in self.family_of(i) {
            let q = &self.people[f as usize];
            if q.alive() && q.village == village {
                let b = self.god.belief(f);
                self.god.belief.insert(f, b + 0.25 * strength * (1.0 - b));
            }
        }
        let c = self.god.cults.entry(village).or_default();
        if benign {
            c.love += strength * 0.3;
        } else {
            c.fear += strength * 0.4;
        }
    }

    // ------------------------------------------------------------------ chaque heure, chaque jour

    /// La foi se recharge ; les effets terminés s'en vont.
    pub(super) fn god_hourly(&mut self) {
        let max = self.god.max_faith();
        let g = &mut self.god;
        if g.faith < max {
            g.faith = (g.faith + g.regen / 24.0).min(max);
        }
        let tick = self.tick;
        g.spells.retain(|s| s.until > tick);
    }

    /// Multiplicateur de pousse des champs d'un village (pluie, sécheresse).
    pub(crate) fn growth_factor(&self, village: u32) -> f32 {
        if self.god.spells.is_empty() {
            return 1.0;
        }
        if self.god.active(village, SpellKind::Drought, self.tick) {
            0.0
        } else if self.god.active(village, SpellKind::Rain, self.tick) {
            1.8
        } else {
            1.0
        }
    }

    /// Multiplicateur de la chasse et de la pêche (abondance).
    pub(crate) fn abundance(&self, village: u32) -> f32 {
        if !self.god.spells.is_empty() && self.god.active(village, SpellKind::Abundance, self.tick) {
            1.6
        } else {
            1.0
        }
    }

    /// Croyance d'un habitant (par index).
    pub(crate) fn belief_of(&self, i: usize) -> f32 {
        self.god.belief(self.people[i].id)
    }

    /// Où l'on prie : devant le temple s'il est bâti, sinon près du puits.
    pub(crate) fn prayer_spot(&mut self, vi: usize) -> u32 {
        let id = self.villages[vi].id;
        let temple = self
            .god
            .cult(id)
            .and_then(|c| c.temple)
            .and_then(|t| self.villages[vi].house(t))
            .filter(|h| h.state == HouseState::Inhabited)
            .map(|h| (h.x, h.y + 1));
        let (x, y) = temple.unwrap_or(self.villages[vi].well);
        self.spot_near(x, y, 2)
    }

    /// À minuit : fêtes promises, croyance qui s'use ou se transmet, cultes qui naissent ou
    /// s'éteignent, prêtres, temples, foi rendue chaque jour.
    pub(super) fn daily_god(&mut self) {
        let tick = self.tick;
        let today = tick_day(tick);
        for v in std::mem::take(&mut self.god.feasts) {
            if self.village_index(v).is_some() {
                self.society.polity_mut(v).feast_day = Some(today);
            }
        }
        // Sécheresse : les champs semés se dessèchent.
        for vi in 0..self.villages.len() {
            let id = self.villages[vi].id;
            if self.god.active(id, SpellKind::Drought, tick) {
                for k in 0..self.villages[vi].fields.len() {
                    let roll = self.rng.f32();
                    let f = &mut self.villages[vi].fields[k];
                    if f.state == FieldState::Sown {
                        f.growth = (f.growth - 0.08).max(0.0);
                        if roll < 0.15 {
                            f.state = FieldState::Fallow;
                            f.growth = 0.0;
                            f.work = 0.0;
                        }
                    }
                }
            }
        }

        // Croyance : elle s'use sans prodige ni prière (moins chez les pieux), les petits
        // reçoivent celle de leurs parents, les morts sont oubliés.
        for i in 0..self.people.len() {
            let p = &self.people[i];
            let id = p.id;
            if !p.alive() {
                self.god.belief.remove(&id);
                continue;
            }
            let piety = p.mind.traits.piety;
            let b = match self.god.belief.get(&id) {
                Some(&b) => b - 0.004 * (1.3 - piety),
                None if p.age(tick) < 1.0 && !p.parents.is_empty() => {
                    let parents: f32 = p.parents.iter().map(|&q| self.god.belief(q)).sum();
                    parents / p.parents.len() as f32 * (0.5 + 0.5 * piety)
                }
                None => continue,
            };
            if b <= 0.01 {
                self.god.belief.remove(&id);
            } else {
                self.god.belief.insert(id, b.min(1.0));
            }
        }

        let mut regen = crate::god::FAITH_PER_DAY;
        for vi in 0..self.villages.len() {
            let id = self.villages[vi].id;
            let residents = self.residents(id);
            let (mut sum, mut believers) = (0.0f32, 0u32);
            for &i in &residents {
                let b = self.belief_of(i);
                sum += b;
                if b >= BELIEVER {
                    believers += 1;
                    regen += 0.06 * b;
                }
            }
            let devotion = if residents.is_empty() {
                0.0
            } else {
                sum / residents.len() as f32
            };
            if believers == 0 && !self.god.cults.contains_key(&id) {
                continue;
            }
            let c = self.god.cults.entry(id).or_default();
            c.believers = believers;
            c.devotion = devotion;
            c.love *= 0.985;
            c.fear *= 0.985;
            let temple_built = c
                .temple
                .and_then(|t| self.villages[vi].house(t))
                .is_some_and(|h| h.state == HouseState::Inhabited);
            if temple_built {
                regen += 2.0;
            }
            self.cult_life(vi, &residents);
        }
        self.god.regen = regen;
    }

    /// Naissance et fin d'un culte, choix du prêtre, chantier du temple.
    fn cult_life(&mut self, vi: usize, residents: &[usize]) {
        let tick = self.tick;
        let id = self.villages[vi].id;
        let name = self.villages[vi].name.clone();
        let Some(c) = self.god.cults.get(&id).cloned() else {
            return;
        };
        match c.founded {
            None if c.believers >= 3 && c.devotion >= 0.18 => {
                let cult = if c.love >= 1.5 * c.fear {
                    "le culte du Ciel clément"
                } else if c.fear >= 1.5 * c.love {
                    "le culte du Ciel courroucé"
                } else {
                    "le culte du Ciel"
                };
                let x = self.god.cults.get_mut(&id).unwrap();
                x.founded = Some(tick);
                x.name = cult.to_string();
                self.god.stats.cults += 1;
                self.event(
                    Some(id),
                    vec![],
                    format!(
                        "{}, on se met à prier le Ciel : naissance du {}",
                        cap(&at_place(&name)),
                        &cult[3..]
                    ),
                );
            }
            Some(_) if c.believers == 0 && c.devotion < 0.05 => {
                let x = self.god.cults.get_mut(&id).unwrap();
                x.founded = None;
                x.priest = None;
                self.god.stats.cults_lost += 1;
                self.event(
                    Some(id),
                    vec![],
                    format!("{}, plus personne ne prie le Ciel", cap(&at_place(&name))),
                );
                return;
            }
            Some(_) => {}
            None => return,
        }

        // Prêtre : le plus fervent des adultes, s'il croit assez.
        let priest_ok = c.priest.is_some_and(|p| {
            let p = &self.people[p as usize];
            p.alive() && p.village == id && !p.brain.leaving
        });
        if !priest_ok {
            let chief = self.society.polity(id).and_then(|p| p.chief);
            let best = residents
                .iter()
                .copied()
                .filter(|&i| {
                    let p = &self.people[i];
                    p.age(tick) >= 20.0 && Some(p.id) != chief && self.belief_of(i) >= 0.4
                })
                .max_by(|&a, &b| {
                    let s = |i: usize| self.belief_of(i) * (0.5 + self.people[i].mind.traits.piety);
                    s(a).total_cmp(&s(b))
                });
            let x = self.god.cults.get_mut(&id).unwrap();
            x.priest = best.map(|i| self.people[i].id);
            if let Some(i) = best {
                self.god.stats.priests += 1;
                let p = &mut self.people[i];
                let pid = p.id;
                p.mind.needs.meaning = 1.0;
                p.mind.needs.esteem = (p.mind.needs.esteem + 0.3).min(1.0);
                let title = if p.female { "prêtresse" } else { "prêtre" };
                let text = format!("{} devient {title} du Ciel {}", p.name(), at_place(&name));
                self.god.belief.insert(pid, self.god.belief(pid).max(0.7));
                self.event(Some(id), vec![pid], text);
            }
        }
        if let Some(p) = self.god.cults[&id].priest {
            let b = self.god.belief(p);
            self.god.belief.insert(p, b.max(0.6));
        }

        // Temple : quand assez de monde prie, le village en bâtit un.
        let temple = c.temple.filter(|&t| self.villages[vi].house(t).is_some());
        if temple.is_none() && c.temple.is_some() {
            self.god.cults.get_mut(&id).unwrap().temple = None;
        }
        let sites = self.villages[vi]
            .houses
            .iter()
            .filter(|h| h.state == HouseState::Building)
            .count();
        if temple.is_none()
            && c.believers >= 8
            && c.devotion >= 0.3
            && sites <= 1
            && let Some(hid) = self.new_house(vi, HouseState::Building)
        {
            self.economy.workshops.insert(hid, Workshop::Temple);
            self.god.cults.get_mut(&id).unwrap().temple = Some(hid);
            self.god.stats.temples += 1;
            self.event(
                Some(id),
                vec![],
                format!(
                    "Nouveau chantier {} : un temple pour prier le Ciel",
                    at_place(&name)
                ),
            );
        }
    }

    /// Ce que pense un habitant du Ciel (pour sa fiche).
    pub fn belief_text(&self, i: usize) -> &'static str {
        belief_label(self.belief_of(i), self.people[i].female)
    }
}

fn cap(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        god::{GodCommand, SpellKind},
        save::tests::sample,
        settlement::FieldState,
        society::Disease,
        tuning::DAY,
    };

    #[test]
    fn powers_cost_faith_and_are_recorded() {
        let mut w = sample();
        let id = w.villages[0].id;
        let faith = w.god.faith;
        let deed = w.god_command(&GodCommand::Rain { village: id }).unwrap();
        assert!(deed.text.contains("Le Ciel"), "{}", deed.text);
        assert_eq!(w.god.faith, faith - 10.0);
        assert_eq!(w.god.deeds.len(), 1);
        assert!(w.god.active(id, SpellKind::Rain, w.tick));
        assert!(w.chronicle.events.back().unwrap().text.starts_with("Le Ciel"));
        // Cible inconnue : refusé, rien n'est dépensé.
        assert!(w.god_command(&GodCommand::Rain { village: 999 }).is_err());
        assert_eq!(w.god.faith, faith - 10.0);
        // Trop cher.
        w.god.faith = 5.0;
        let e = w.god_command(&GodCommand::Bounty { village: id }).unwrap_err();
        assert!(e.contains("foi"), "{e}");
    }

    #[test]
    fn miracles_make_believers_and_a_cult() {
        let mut w = sample();
        let id = w.villages[0].id;
        w.god.faith = 1000.0;
        for _ in 0..6 {
            w.god_command(&GodCommand::Bounty { village: id }).unwrap();
            w.god_command(&GodCommand::Rain { village: id }).unwrap();
        }
        let believers = w.alive().filter(|p| w.god.belief(p.id) >= 0.25).count();
        assert!(believers >= 3, "{believers} croyants");
        for _ in 0..DAY {
            w.step();
        }
        let c = w.god.cult(id).unwrap();
        assert!(c.founded.is_some(), "pas de culte : {c:?}");
        assert!(c.priest.is_some(), "pas de prêtre");
        assert_eq!(c.attitude(), "te vénère");
        assert!(w.god.regen > crate::god::FAITH_PER_DAY);
    }

    #[test]
    fn rain_and_drought_change_growth() {
        let mut w = sample();
        let id = w.villages[0].id;
        w.god.faith = 1000.0;
        assert_eq!(w.growth_factor(id), 1.0);
        w.god_command(&GodCommand::Drought { village: id }).unwrap();
        assert_eq!(w.growth_factor(id), 0.0);
        w.god_command(&GodCommand::Rain { village: id }).unwrap();
        assert!(w.growth_factor(id) > 1.0, "la pluie met fin à la sécheresse");
        for _ in 0..3 * DAY {
            w.step();
        }
        assert_eq!(w.growth_factor(id), 1.0);
        assert!(w.god.spells.is_empty());
    }

    #[test]
    fn bounty_ripens_sown_fields() {
        let mut w = sample();
        let vi = 0;
        let id = w.villages[vi].id;
        for f in &mut w.villages[vi].fields {
            f.state = FieldState::Sown;
            f.growth = 0.1;
        }
        w.god.faith = 100.0;
        w.god_command(&GodCommand::Bounty { village: id }).unwrap();
        assert!(w.villages[vi].fields.iter().all(|f| f.state == FieldState::Ripe));
    }

    #[test]
    fn plague_and_cure() {
        let mut w = sample();
        let id = w.villages[0].id;
        w.god.faith = 1000.0;
        w.god_command(&GodCommand::Plague {
            village: id,
            disease: Disease::Flux as u8,
        })
        .unwrap();
        assert!(!w.society.sick.is_empty());
        let sick = *w.society.sick.keys().next().unwrap();
        w.god_command(&GodCommand::Cure { village: id }).unwrap();
        assert!(w.society.sick.is_empty());
        assert!(w.society.immune(sick, Disease::Flux, w.tick));
        assert!(
            w.god_command(&GodCommand::Cure { village: id }).is_err(),
            "plus personne à guérir"
        );
        assert!(w.god_command(&GodCommand::Heal { person: sick }).is_err());
    }

    #[test]
    fn people_powers() {
        let mut w = sample();
        w.god.faith = 1000.0;
        let tick = w.tick;
        let adults: Vec<u32> = w
            .alive()
            .filter(|p| !p.child(tick) && p.job.is_some())
            .map(|p| p.id)
            .collect();
        let a = adults[0];
        let job = w.people[a as usize].job.unwrap();
        let before = w.people[a as usize].mind.skill(job);
        w.god_command(&GodCommand::Bless { person: a }).unwrap();
        assert!(w.people[a as usize].mind.skill(job) > before);
        assert!(w.god.belief(a) > 0.1);
        assert!(
            w.people[a as usize]
                .mind
                .memories
                .iter()
                .any(|m| m.kind == crate::mind::MemoryKind::Blessed)
        );
        w.god_command(&GodCommand::Curse { person: a }).unwrap();
        assert!(w.people[a as usize].mind.mood.fear > 0.2);
        w.god_command(&GodCommand::Inspire { person: a }).unwrap();
        assert!(w.god_command(&GodCommand::Matchmake { a, b: a }).is_err());
    }

    #[test]
    fn rename_feast_strangers_sign() {
        let mut w = sample();
        w.god.faith = 1000.0;
        let id = w.villages[0].id;
        let pop = w.population(id);
        w.god_command(&GodCommand::Strangers { village: id }).unwrap();
        assert!(w.population(id) >= pop + 2);
        assert!(
            w.god_command(&GodCommand::Rename {
                village: id,
                name: "x".into()
            })
            .is_err()
        );
        assert!(
            w.god_command(&GodCommand::Rename {
                village: id,
                name: "Bourg<script>".into()
            })
            .is_err()
        );
        w.god_command(&GodCommand::Rename {
            village: id,
            name: "  Sainte-Lumière ".into(),
        })
        .unwrap();
        assert_eq!(w.villages[0].name, "Sainte-Lumière");
        w.god_command(&GodCommand::Feast { village: id }).unwrap();
        assert!(w.festive(id) || w.god.feasts.contains(&id));
        w.god_command(&GodCommand::Sign).unwrap();
        assert_eq!(w.god.sign, w.tick);
    }

    #[test]
    fn forest_and_deposit_change_the_map() {
        let mut w = sample();
        w.god.faith = 1000.0;
        let rev = w.map_rev;
        w.god_command(&GodCommand::Forest { x: 70, y: 70 }).unwrap();
        assert!(w.map_rev > rev);
        assert!(w.god_command(&GodCommand::Forest { x: 500, y: 5 }).is_err());
        w.god_command(&GodCommand::Deposit {
            x: 66,
            y: 12,
            resource: crate::map::Resource::Iron as u8,
        })
        .unwrap();
        assert_eq!(
            w.map.resource[w.map.idx(66, 12)],
            crate::map::Resource::Iron as u8
        );
        assert!(
            w.god_command(&GodCommand::Deposit {
                x: 66,
                y: 12,
                resource: 9
            })
            .is_err()
        );
    }

    #[test]
    fn same_commands_same_history() {
        let run = || {
            let mut w = sample();
            w.god.faith = 1000.0;
            let id = w.villages[0].id;
            for k in 0..3 * DAY {
                if k == 100 {
                    w.god_command(&GodCommand::Storm { village: id }).unwrap();
                }
                if k == DAY {
                    w.god_command(&GodCommand::Strangers { village: id }).unwrap();
                }
                w.step();
            }
            w
        };
        assert_eq!(run(), run());
    }
}

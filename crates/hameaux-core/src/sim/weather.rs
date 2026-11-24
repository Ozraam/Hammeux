//! Météo et risques au fil des heures (phase 10) : les fronts avancent, chaque village reçoit
//! pluie, neige, froid ou chaleur ; les sols se mouillent et sèchent ; orages, gel tardif,
//! crues et incendies de forêt frappent ; les marchands affrontent la neige, la boue, les
//! accidents et les brigands.

use std::collections::BTreeSet;

use crate::{
    clock::Season,
    map::Biome,
    mind::{Emotion, MemoryKind},
    people::Act,
    settlement::{FieldState, HouseState},
    trade::{Leg, pair},
    tuning::*,
    weather::{
        FLOOD_RADIUS, FLOOD_WATER, Fire, Front, FrontKind, HARSH_WINTER, REGROW_YEARS, Trip, base_temp,
    },
    world::{World, occ},
};

use super::life::{at_place, de};

/// Cases autour d'un village où peut naître un incendie de forêt.
const FIRE_RING: (i64, i64) = (10, 40);
/// Cases brûlées au plus par un incendie.
const FIRE_MAX: u32 = 250;
/// Distance (cases) à tout village au-delà de laquelle les brigands rôdent.
const WILDS: f32 = 45.0;

impl World {
    // ------------------------------------------------------------------ temps qu'il fait

    /// Pluie (0–1), température (°C) et vent d'orage (0–1) sur une case.
    pub fn weather_at(&self, x: f32, y: f32) -> (f32, f32, f32) {
        let clock = self.clock();
        let tick = self.tick;
        let mut temp = base_temp(clock.season) + self.weather.anomaly;
        // Plus frais la nuit, plus chaud vers 15 h.
        let h = clock.hour as f32 + clock.minute as f32 / 60.0;
        temp += 4.0 * ((h - 15.0) / 24.0 * std::f32::consts::TAU).cos();
        // Plus froid en altitude.
        if self.map.inside(x as i64, y as i64) {
            let e = self.map.elevation[self.map.idx(x as u32, y as u32)] as f32;
            temp -= ((e - 110.0) / 145.0).max(0.0) * 12.0;
        }
        let (mut rain, mut storm, mut dry) = (0.0f32, 0.0f32, 0.0f32);
        for f in &self.weather.fronts {
            let s = f.at(x, y, tick);
            if s <= 0.0 {
                continue;
            }
            match f.kind {
                FrontKind::Rain => {
                    rain = rain.max(s);
                    temp -= 1.5 * s;
                }
                FrontKind::Storm => {
                    rain = rain.max(s);
                    storm = storm.max(s);
                    temp -= 2.0 * s;
                }
                FrontKind::Dry => {
                    dry = dry.max(s);
                    temp += match clock.season {
                        Season::Summer => 5.0,
                        Season::Winter => -2.5,
                        _ => 2.0,
                    } * s;
                }
                FrontKind::Cold => temp -= 10.0 * s,
            }
        }
        rain *= 1.0 - dry;
        storm *= 1.0 - dry;
        (rain, temp, storm)
    }

    /// Un nouveau front, venu de l'ouest (ou posé sur une case par le Ciel).
    fn spawn_front(&mut self, kind: FrontKind, at: Option<(f32, f32)>) {
        let (w, h) = (self.map.width as f32, self.map.height as f32);
        let size = w.max(h);
        let r = &mut self.rng;
        let radius = size
            * match kind {
                FrontKind::Storm => 0.05 + 0.06 * r.f32(),
                FrontKind::Dry => 0.16 + 0.12 * r.f32(),
                FrontKind::Cold => 0.2 + 0.12 * r.f32(),
                FrontKind::Rain => 0.09 + 0.1 * r.f32(),
            };
        let radius = radius.max(12.0);
        let speed = (size / 100.0).max(1.0)
            * match kind {
                FrontKind::Storm => 1.3 + 0.6 * r.f32(),
                FrontKind::Dry => 0.4 + 0.4 * r.f32(),
                _ => 0.7 + 0.7 * r.f32(),
            };
        let vy = speed * (r.f32() - 0.5) * 0.6;
        let (x, y, hours) = match at {
            Some((x, y)) => (x, y, 36.0 + 24.0 * r.f32()),
            None => {
                let y = r.f32() * h;
                let cross = (w + 2.0 * radius) / speed;
                (-radius * 0.7, y, cross * (0.55 + 0.55 * r.f32()))
            }
        };
        let strength = 0.5 + 0.5 * r.f32();
        let (vx, vy) = if at.is_some() {
            (speed * 0.25, vy * 0.25)
        } else {
            (speed, vy)
        };
        let id = self.weather.next_id;
        self.weather.next_id += 1;
        self.weather.stats.fronts += 1;
        self.weather.fronts.push(Front {
            id,
            kind,
            x,
            y,
            vx,
            vy,
            radius,
            strength: if at.is_some() { 1.0 } else { strength },
            born: self.tick,
            until: self.tick + (hours * HOUR as f32) as u64,
            hit: Vec::new(),
        });
    }

    /// Chaque heure : les fronts avancent (de nouveaux naissent à l'ouest), chaque village
    /// reçoit son temps, les feux s'étendent, les marchands affrontent la route.
    pub(super) fn weather_hourly(&mut self) {
        if self.map.width == 0 {
            return;
        }
        let tick = self.tick;
        let (w, h) = (self.map.width as f32, self.map.height as f32);
        for f in &mut self.weather.fronts {
            f.x += f.vx;
            f.y += f.vy;
        }
        self.weather
            .fronts
            .retain(|f| f.until > tick && f.x - f.radius < w && f.y + f.radius > 0.0 && f.y - f.radius < h);
        let season = self.clock().season;
        let warm = self.weather.anomaly;
        let chance = match season {
            Season::Spring => 0.08,
            Season::Summer => 0.075,
            Season::Autumn => 0.085,
            Season::Winter => 0.07,
        };
        if self.rng.chance(chance) {
            let roll = self.rng.f32();
            // Un printemps sec ou un été brûlant amènent surtout du temps sec ; un printemps
            // pluvieux ou un été pourri, de la pluie.
            let dry_bias = match season {
                Season::Spring | Season::Summer => (warm / 10.0).clamp(-0.2, 0.3),
                _ => 0.0,
            };
            let kind = match season {
                Season::Spring => match roll {
                    r if r < 0.66 - dry_bias => FrontKind::Rain,
                    r if r < 0.76 - dry_bias => FrontKind::Storm,
                    r if r < 0.94 => FrontKind::Dry,
                    _ => FrontKind::Cold,
                },
                Season::Summer => match roll {
                    r if r < 0.5 - dry_bias => FrontKind::Rain,
                    r if r < 0.8 - dry_bias => FrontKind::Storm,
                    _ => FrontKind::Dry,
                },
                Season::Autumn => match roll {
                    r if r < 0.72 => FrontKind::Rain,
                    r if r < 0.77 => FrontKind::Storm,
                    r if r < 0.93 => FrontKind::Dry,
                    _ => FrontKind::Cold,
                },
                Season::Winter => match roll {
                    r if r < 0.55 => FrontKind::Rain,
                    r if r < 0.85 => FrontKind::Cold,
                    _ => FrontKind::Dry,
                },
            };
            self.spawn_front(kind, None);
        }
        for vi in 0..self.villages.len() {
            self.village_weather(vi);
        }
        self.fires_hourly();
        self.road_hourly();
    }

    /// Le temps d'un village, heure par heure : pluie ou neige, sol qui se mouille ou sèche,
    /// rivière qui monte ; orage, gel tardif et crue.
    fn village_weather(&mut self, vi: usize) {
        let tick = self.tick;
        let clock = self.clock();
        let (cx, cy) = (self.villages[vi].x as f32 + 1.0, self.villages[vi].y as f32 + 1.0);
        let (rain, temp, storm) = self.weather_at(cx, cy);
        if self.weather.local(vi).river == 255 {
            let river = self.river_near(vi);
            self.weather.local_mut(vi).river = river;
        }
        // Pluie ou sécheresse du Ciel : le sol s'en ressent aussi.
        let id = self.villages[vi].id;
        let (blessed, cursed) = if self.god.spells.is_empty() {
            (false, false)
        } else {
            (
                self.god.active(id, crate::god::SpellKind::Rain, tick),
                self.god.active(id, crate::god::SpellKind::Drought, tick),
            )
        };
        let l = self.weather.local_mut(vi);
        l.rain = if cursed {
            0.0
        } else if blessed {
            rain.max(0.5)
        } else {
            rain
        };
        l.temp = temp;
        l.storm = storm;
        l.temp_sum += temp;
        l.temp_n += 1;
        l.rain_today += l.rain;
        let rain = l.rain;
        let mut melt = 0.0;
        if temp < 0.5 {
            l.snow = (l.snow + rain * 0.06).min(1.0);
        } else {
            l.wet += rain * 0.06;
            melt = (temp * 0.006).min(l.snow);
            l.snow -= melt;
            l.wet += melt * 0.5;
        }
        if l.snow < 0.05 {
            // Un sol humide sèche plus vite qu'un sol déjà sec.
            let evap =
                (0.0008 + 0.00025 * temp.max(0.0)) * (0.2 + l.wet) / 0.75 * (1.0 - 0.8 * rain.min(1.0));
            l.wet -= evap * if cursed { 3.0 } else { 1.0 };
        }
        l.wet = l.wet.clamp(0.0, 1.0);
        l.flood_water = l.flood_water * (1.0 - 1.0 / 48.0) + rain + melt * 2.0;
        l.crop = l.crop_factor();
        let l = *l;

        // Orage (une fois par passage d'une ligne d'orages).
        if l.storm > 0.45 {
            let front = self
                .weather
                .fronts
                .iter()
                .filter(|f| f.kind == FrontKind::Storm && !f.hit.contains(&id))
                .find(|f| f.at(cx, cy, tick) > 0.3)
                .map(|f| f.id);
            if let Some(fid) = front {
                if let Some(f) = self.weather.fronts.iter_mut().find(|f| f.id == fid) {
                    f.hit.push(id);
                }
                self.natural_storm(vi, l.storm);
            }
        }
        // Gel tardif : au printemps, une nuit sous zéro grille les jeunes pousses.
        if clock.season == Season::Spring && l.temp < -0.5 && !l.frost_told {
            self.late_frost(vi);
        }
        // Crue : trop de pluie (ou de neige fondue) en quelques jours.
        let threshold = FLOOD_WATER
            * match l.river {
                3 => 0.85,
                2 => 1.0,
                1 => 1.35,
                _ => f32::MAX,
            };
        if l.flood_water > threshold
            && tick.saturating_sub(l.last_flood) > 21 * DAY
            && l.last_flood <= tick
            && let Ok(text) = self.flood(vi, false)
        {
            self.event(Some(id), vec![], text);
        }
    }

    /// Plus grande rivière qui passe près des champs du village (0 : aucune).
    fn river_near(&self, vi: usize) -> u8 {
        let v = &self.villages[vi];
        let mut best = 0;
        for dy in -FLOOD_RADIUS..=FLOOD_RADIUS {
            for dx in -FLOOD_RADIUS..=FLOOD_RADIUS {
                let (x, y) = (v.x as i64 + dx, v.y as i64 + dy);
                if self.map.inside(x, y) {
                    best = best.max(self.map.river_at(self.map.idx(x as u32, y as u32)));
                }
            }
        }
        best
    }

    /// Habitants présents au village (ni partis, ni en voyage).
    fn at_home_people(&self, village: u32) -> Vec<usize> {
        (0..self.people.len())
            .filter(|&i| {
                let p = &self.people[i];
                p.alive() && p.village == village && !p.brain.leaving && !p.brain.traveling
            })
            .collect()
    }

    /// Un malheur frappe le village : chacun s'en souvient, a peur et se sent moins en
    /// sécurité (les nerveux plus que les autres).
    pub(crate) fn endure(&mut self, vi: usize, kind: MemoryKind, strength: f32) {
        let tick = self.tick;
        for i in self.at_home_people(self.villages[vi].id) {
            let t = self.people[i].mind.traits;
            let m = &mut self.people[i].mind;
            m.remember(tick, kind, None, -(0.2 + strength));
            m.mood.add(Emotion::Fear, 0.1 + strength * (0.5 + t.nervousness));
            m.needs.safety = (m.needs.safety - strength).max(0.0);
        }
    }

    /// Chacun court s'abriter (de jour).
    fn shelter(&mut self, village: u32) {
        if !(7..21).contains(&self.clock().hour) {
            return;
        }
        for i in self.at_home_people(village) {
            let b = &self.people[i].brain;
            if !b.hidden && !self.society.sick.contains_key(&self.people[i].id) {
                self.near_home(i, HOUR, Act::Idle);
            }
        }
    }

    /// Un orage naturel : on s'abrite, des champs mûrs sont couchés, parfois un chantier
    /// abîmé ; l'été, sur un sol sec, la foudre peut mettre le feu à la forêt.
    fn natural_storm(&mut self, vi: usize, force: f32) {
        let id = self.villages[vi].id;
        self.weather.stats.storms += 1;
        self.shelter(id);
        let mut flattened = 0;
        for k in 0..self.villages[vi].fields.len() {
            let roll = self.rng.f32();
            let f = &mut self.villages[vi].fields[k];
            if f.state == FieldState::Ripe && roll < 0.2 * force {
                f.state = FieldState::Fallow;
                f.work = 0.0;
                f.growth = 0.0;
                flattened += 1;
            }
        }
        self.weather.stats.fields_lost += flattened;
        let mut damaged = false;
        if self.rng.chance(0.25 * force) {
            let sites: Vec<u32> = self.villages[vi]
                .houses
                .iter()
                .filter(|h| h.state == HouseState::Building && h.wood_in > 1)
                .map(|h| h.id)
                .collect();
            if let Some(&hid) = self.rng.pick(&sites) {
                let h = self.villages[vi].house_mut(hid).unwrap();
                let w = h.wood_in / 3;
                h.wood_in -= w;
                h.wood_needed += w;
                damaged = w > 0;
            }
        }
        let l = self.weather.local(vi);
        let lightning = self.clock().season == Season::Summer && l.wet < 0.3 && self.rng.chance(0.3);
        let name = self.villages[vi].name.clone();
        let s = if flattened > 1 { "s" } else { "" };
        let text = match (flattened, damaged) {
            (0, false) => None,
            (0, true) => Some(format!(
                "Un violent orage s'abat sur {name} : un chantier est abîmé"
            )),
            (n, false) => Some(format!(
                "Un violent orage s'abat sur {name} : {n} champ{s} couché{s}"
            )),
            (n, true) => Some(format!(
                "Un violent orage s'abat sur {name} : {n} champ{s} couché{s}, un chantier abîmé"
            )),
        };
        if let Some(text) = text {
            self.endure(vi, MemoryKind::Storm, 0.1);
            self.event(Some(id), vec![], text);
        }
        if lightning && let Some(tile) = self.forest_near(vi) {
            self.start_fire(tile, false, "la foudre");
        }
    }

    /// Gel tardif : les jeunes pousses gèlent (on resème ce qui est perdu).
    fn late_frost(&mut self, vi: usize) {
        let mut hurt = 0;
        let mut lost = 0;
        for k in 0..self.villages[vi].fields.len() {
            let roll = self.rng.f32();
            let f = &mut self.villages[vi].fields[k];
            if f.state == FieldState::Sown && f.growth < 0.5 {
                hurt += 1;
                f.growth *= 0.5;
                if roll < 0.25 {
                    f.state = FieldState::Ploughed;
                    f.growth = 0.0;
                    f.work = 0.0;
                    lost += 1;
                }
            }
        }
        self.weather.local_mut(vi).frost_told = true;
        if hurt == 0 {
            return;
        }
        self.weather.stats.frosts += 1;
        self.weather.stats.fields_lost += lost;
        let (id, name) = (self.villages[vi].id, self.villages[vi].name.clone());
        let text = if lost > 0 {
            let s = if lost > 1 { "s" } else { "" };
            format!(
                "Gel tardif {} : les jeunes pousses ont gelé, {lost} champ{s} à ressemer",
                at_place(&name)
            )
        } else {
            format!("Gel tardif {} : les jeunes pousses ont souffert", at_place(&name))
        };
        self.event(Some(id), vec![], text);
    }

    /// La rivière déborde : champs noyés près de l'eau, grenier mouillé s'il est près de la
    /// rive, chemins boueux pendant deux jours. Erreur s'il n'y a pas de rivière.
    pub(crate) fn flood(&mut self, vi: usize, divine: bool) -> Result<String, String> {
        let tick = self.tick;
        if self.weather.local(vi).river == 255 {
            let river = self.river_near(vi);
            self.weather.local_mut(vi).river = river;
        }
        if self.weather.local(vi).river == 0 {
            return Err(format!(
                "Aucune rivière ne passe près {}",
                de(&self.villages[vi].name)
            ));
        }
        let near_water = |w: &World, x: u32, y: u32, r: i64| {
            (-r..=r).any(|dy| {
                (-r..=r).any(|dx| {
                    let (tx, ty) = (x as i64 + dx, y as i64 + dy);
                    w.map.inside(tx, ty) && w.map.river_at(w.map.idx(tx as u32, ty as u32)) > 0
                })
            })
        };
        let wet_fields: Vec<usize> = (0..self.villages[vi].fields.len())
            .filter(|&k| {
                let f = &self.villages[vi].fields[k];
                near_water(self, f.x + 1, f.y + 1, 4)
            })
            .collect();
        let mut drowned = 0;
        for k in wet_fields {
            let roll = self.rng.f32();
            let f = &mut self.villages[vi].fields[k];
            match f.state {
                FieldState::Sown | FieldState::Ripe if roll < 0.6 => {
                    f.state = FieldState::Fallow;
                    f.growth = 0.0;
                    f.work = 0.0;
                    drowned += 1;
                }
                FieldState::Sown => f.growth *= 0.6,
                FieldState::Ploughed => f.work = 0.0,
                _ => {}
            }
        }
        self.weather.stats.fields_lost += drowned;
        let (gx, gy) = (self.villages[vi].x, self.villages[vi].y);
        let mut spoiled = 0.0;
        if near_water(self, gx, gy, 6) {
            let st = &mut self.villages[vi].stock;
            spoiled = st.food * 0.15;
            st.food -= spoiled;
        }
        let l = self.weather.local_mut(vi);
        l.flooded_until = tick + 2 * DAY;
        l.last_flood = tick;
        l.flood_water *= 0.4;
        l.wet = 1.0;
        self.weather.stats.floods += 1;
        if !divine {
            self.endure(vi, MemoryKind::Flood, 0.25);
        }
        let name = self.villages[vi].name.clone();
        let s = if drowned > 1 { "s" } else { "" };
        let what = match (drowned, spoiled >= 1.0) {
            (0, false) => "sans grands dégâts".to_string(),
            (0, true) => format!("l'eau monte jusqu'au grenier ({spoiled:.0} rations gâtées)"),
            (n, false) => format!("{n} champ{s} noyé{s}"),
            (n, true) => format!("{n} champ{s} noyé{s}, {spoiled:.0} rations gâtées au grenier"),
        };
        Ok(format!("Crue : la rivière déborde {} ({what})", at_place(&name))
            .replace(" (sans grands dégâts)", ", sans grands dégâts"))
    }

    // ------------------------------------------------------------------ incendies

    /// Une case de forêt à quelque distance du village, s'il y en a.
    fn forest_near(&mut self, vi: usize) -> Option<u32> {
        let (vx, vy) = (self.villages[vi].x as i64, self.villages[vi].y as i64);
        for _ in 0..80 {
            let d = self.rng.range(FIRE_RING.0, FIRE_RING.1) as f32;
            let a = self.rng.f32() * std::f32::consts::TAU;
            let (x, y) = (vx + (a.cos() * d) as i64, vy + (a.sin() * d) as i64);
            if !self.map.inside(x, y) {
                continue;
            }
            let i = self.map.idx(x as u32, y as u32);
            if matches!(self.map.biome_at(i), Biome::Forest | Biome::Conifers) {
                return Some(i as u32);
            }
        }
        None
    }

    /// Village le plus proche d'une case (et sa distance).
    fn nearest_village(&self, x: f32, y: f32) -> Option<(usize, f32)> {
        self.villages
            .iter()
            .enumerate()
            .map(|(vi, v)| (vi, (v.x as f32 - x).hypot(v.y as f32 - y)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// Met le feu à une case de forêt. `cause` : « la foudre », « une étincelle », « le Ciel ».
    pub(crate) fn start_fire(&mut self, tile: u32, divine: bool, cause: &str) -> Option<String> {
        let i = tile as usize;
        if i >= self.map.len()
            || !matches!(self.map.biome_at(i), Biome::Forest | Biome::Conifers)
            || self.weather.burning(tile)
        {
            return None;
        }
        let (x, y) = self.map.xy(i);
        let near = self
            .nearest_village(x as f32, y as f32)
            .filter(|&(_, d)| d < 80.0);
        if let Some((vi, _)) = near
            && self
                .weather
                .fires
                .iter()
                .any(|f| f.village == Some(self.villages[vi].id))
        {
            return None;
        }
        let id = self.weather.next_id;
        self.weather.next_id += 1;
        let out = self.tick + (3 + self.rng.below(3)) * HOUR;
        self.weather.fires.push(Fire {
            id,
            village: near.map(|(vi, _)| self.villages[vi].id),
            started: self.tick,
            burning: vec![(tile, out)],
            burnt: 0,
            divine,
        });
        self.weather.stats.fires += 1;
        let text = match near {
            Some((vi, _)) => {
                let name = self.villages[vi].name.clone();
                if !divine {
                    self.endure(vi, MemoryKind::Wildfire, 0.2);
                }
                format!(
                    "Incendie de forêt près {} : {cause} a mis le feu aux bois",
                    de(&name)
                )
            }
            None => format!("Incendie de forêt loin de tout village : {cause} a mis le feu aux bois"),
        };
        if !divine {
            let village = near.map(|(vi, _)| self.villages[vi].id);
            self.event(village, vec![], text.clone());
        }
        Some(text)
    }

    /// Chaque heure, le feu gagne les arbres voisins (moins vite sous la pluie, sur un sol
    /// humide ou aux abords du village, que les habitants défendent) ; une case brûlée
    /// devient lande, et repoussera.
    fn fires_hourly(&mut self) {
        if self.weather.fires.is_empty() {
            return;
        }
        let tick = self.tick;
        let width = self.map.width as i64;
        let mut fires = std::mem::take(&mut self.weather.fires);
        let mut ended = Vec::new();
        for fire in &mut fires {
            let alight: BTreeSet<u32> = fire.burning.iter().map(|&(t, _)| t).collect();
            let mut next = Vec::new();
            let mut lit = alight.clone();
            // Pluie et humidité autour du feu.
            let (fx, fy) = self.map.xy(fire.burning[0].0 as usize);
            let (rain, _, _) = self.weather_at(fx as f32, fy as f32);
            let wet = self
                .nearest_village(fx as f32, fy as f32)
                .filter(|&(_, d)| d < 120.0)
                .map_or(0.4, |(vi, _)| self.weather.local(vi).wet);
            let spread = 0.22 * (1.0 - 1.5 * wet).clamp(0.05, 1.0) * (1.0 - rain).max(0.0);
            for &(t, out) in &fire.burning {
                if out <= tick || (rain > 0.3 && self.rng.chance(0.5 * rain)) {
                    let i = t as usize;
                    let old = self.map.biome[i];
                    if out <= tick {
                        self.set_biome(i, Biome::Heath);
                        self.weather.burnt.push((t, tick, old));
                        self.weather.stats.burnt_tiles += 1;
                        fire.burnt += 1;
                    }
                    continue;
                }
                next.push((t, out));
                if fire.burnt + lit.len() as u32 >= FIRE_MAX {
                    continue;
                }
                for (dx, dy) in [(1i64, 0i64), (-1, 0), (0, 1), (0, -1)] {
                    let (x, y) = ((t as i64 % width) + dx, (t as i64 / width) + dy);
                    if !self.map.inside(x, y) {
                        continue;
                    }
                    let j = self.map.idx(x as u32, y as u32);
                    if lit.contains(&(j as u32))
                        || !matches!(self.map.biome_at(j), Biome::Forest | Biome::Conifers)
                    {
                        continue;
                    }
                    // Aux abords des maisons et des champs, les habitants font la chaîne.
                    let guarded = self.near_occupied(x, y, 2);
                    let p = if guarded { spread * 0.15 } else { spread };
                    if self.rng.chance(p) {
                        lit.insert(j as u32);
                        next.push((j as u32, tick + (3 + self.rng.below(3)) * HOUR));
                    }
                }
            }
            fire.burning = next;
            if fire.burning.is_empty() {
                ended.push(fire.clone());
            }
        }
        fires.retain(|f| !f.burning.is_empty());
        self.weather.fires = fires;
        for f in ended {
            let near = f.village.and_then(|v| self.village_index(v));
            if let Some(vi) = near {
                // Moins de bois et de gibier alentour.
                let wild = self.land.wild_mut(vi);
                wild.game =
                    (wild.game * (1.0 - (f.burnt as f32 / 600.0).min(0.5))).max(crate::land::WILD_MIN);
                self.compute_access(vi);
            }
            if f.burnt < 3 {
                continue;
            }
            let s = if f.burnt > 1 { "s" } else { "" };
            let place = near.map_or("loin de tout village".to_string(), |vi| {
                format!("près {}", de(&self.villages[vi].name))
            });
            let text = format!(
                "L'incendie {place} est éteint : {} case{s} de forêt brûlée{s}",
                f.burnt
            );
            self.event(f.village, vec![], text);
        }
    }

    /// Une maison, un champ ou un grenier à moins de `r` cases ?
    fn near_occupied(&self, x: i64, y: i64, r: i64) -> bool {
        (-r..=r).any(|dy| {
            (-r..=r).any(|dx| {
                self.map.inside(x + dx, y + dy)
                    && self.occ(self.map.idx((x + dx) as u32, (y + dy) as u32)) != occ::NONE
            })
        })
    }

    // ------------------------------------------------------------------ chaque jour

    /// À minuit : bilan de la journée de chaque village (pluie, sécheresse), écart de saison
    /// (hiver rude, été brûlant), promesse de la récolte, froid qui tue, feux qui prennent,
    /// forêts qui repoussent, dangers de la route qui s'oublient.
    pub(super) fn daily_weather(&mut self) {
        if self.map.width == 0 {
            return;
        }
        let tick = self.tick;
        let clock = self.clock();
        if self.weather.anomaly_season != Some(clock.season) {
            self.new_season(clock.season, clock.day == 1 && tick > HOUR);
        }
        let growing = matches!(clock.season, Season::Spring | Season::Summer);
        for vi in 0..self.villages.len() {
            let l = self.weather.local_mut(vi);
            if l.temp_n > 0 {
                l.temp_mean = l.temp_sum / l.temp_n as f32;
            }
            l.temp_sum = 0.0;
            l.temp_n = 0;
            let rainy = l.rain_today > 1.0;
            if rainy {
                l.dry_days = 0;
            } else {
                l.dry_days += 1;
            }
            let snowy = rainy && l.temp_mean < 1.0;
            l.rain_today = 0.0;
            if clock.season == Season::Spring && clock.day == 1 {
                l.frost_told = false;
            }
            let l = *l;
            let st = &mut self.weather.stats;
            if snowy {
                st.snowy_days += 1;
            } else if rainy {
                st.rainy_days += 1;
            }
            if growing && l.wet < 0.2 {
                st.drought_days += 1;
            }
            // Ce que promet la récolte : le temps du printemps et de l'été.
            if growing {
                let q = 0.5 + 0.55 * l.crop.min(1.1);
                let y = self.land.year_mut(vi);
                y.harvest = (y.harvest + 0.08 * (q - y.harvest)).clamp(0.6, 1.2);
            }
            // Un sol sec et brûlant : un feu peut prendre dans les bois.
            if matches!(clock.season, Season::Summer | Season::Autumn)
                && l.dry_days >= 7
                && l.wet < 0.15
                && l.temp_mean > 14.0
                && self.rng.chance(0.008 * (l.dry_days - 6).min(6) as f32)
                && let Some(tile) = self.forest_near(vi)
            {
                self.start_fire(tile, false, "une étincelle");
            }
            // Hiver rude : le gibier se terre.
            if self.weather.harsh() {
                let w = self.land.wild_mut(vi);
                w.game = (w.game * 0.98).max(crate::land::WILD_MIN);
            }
        }
        // Fin du printemps : on sait ce que promet l'année.
        if clock.season == Season::Summer && clock.day == 1 {
            self.announce_harvest();
        }
        if clock.season == Season::Winter && self.weather.harsh() {
            self.cold_deaths();
        }
        // La forêt repousse sur les brûlis.
        let old = tick.saturating_sub(REGROW_YEARS * YEAR);
        if self.weather.burnt.first().is_some_and(|b| b.1 <= old) {
            let (due, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut self.weather.burnt)
                .into_iter()
                .partition(|b| b.1 <= old);
            self.weather.burnt = keep;
            for (t, _, biome) in due {
                let i = t as usize;
                if self.map.biome_at(i) == Biome::Heath && self.occ(i) == occ::NONE && !self.map.road_at(i) {
                    self.set_biome(i, Biome::from_u8(biome));
                    self.weather.stats.regrown_tiles += 1;
                }
            }
        }
        for d in self.weather.danger.values_mut() {
            *d *= 0.95;
        }
        self.weather.danger.retain(|_, d| *d > 0.05);
    }

    /// Chaque saison a son écart : la plupart sont ordinaires, certains hivers sont rudes,
    /// certains étés brûlants ou pourris.
    fn new_season(&mut self, season: Season, tell: bool) {
        let roll = self.rng.f32();
        let jitter = (self.rng.f32() - 0.5) * 4.0;
        let (anomaly, text) = match season {
            Season::Winter if roll < 0.22 => {
                self.weather.stats.harsh_winters += 1;
                (
                    -HARSH_WINTER - 2.0 * self.rng.f32(),
                    Some("L'hiver arrive, et il sera rude : le froid mord dans tout le pays"),
                )
            }
            Season::Summer if roll < 0.15 => (
                4.0 + 2.0 * self.rng.f32(),
                Some("L'été s'annonce brûlant : les puits baissent, les bois sont secs"),
            ),
            Season::Summer if roll < 0.3 => (-2.5, Some("L'été s'annonce pourri : il pleut sans cesse")),
            // Le printemps : sec et chaud, ou pluvieux et frais (on le saura à la récolte).
            Season::Spring if roll < 0.18 => (3.0, None),
            Season::Spring if roll < 0.36 => (-2.0, None),
            _ => (jitter, None),
        };
        self.weather.anomaly = anomaly;
        self.weather.anomaly_season = Some(season);
        if tell && let Some(t) = text {
            if self.weather.harsh() {
                for vi in 0..self.villages.len() {
                    self.endure(vi, MemoryKind::HarshWinter, 0.05);
                }
            }
            self.event(None, vec![], t.to_string());
        }
    }

    /// À la fin du printemps, chaque village sait ce que promet sa récolte.
    fn announce_harvest(&mut self) {
        for vi in 0..self.villages.len() {
            let harvest = self.land.year(vi).harvest;
            let l = self.weather.local(vi);
            let (id, name) = (self.villages[vi].id, self.villages[vi].name.clone());
            let why = if l.wet < 0.3 {
                "sec"
            } else if l.wet > 0.88 {
                "noyé de pluie"
            } else if l.temp_mean < 8.0 {
                "froid"
            } else {
                "capricieux"
            };
            let text = if harvest < 0.85 {
                format!("À {name}, le printemps a été {why} : la récolte sera maigre")
            } else if harvest > 1.06 {
                format!("À {name}, le printemps a été doux et arrosé : la récolte s'annonce belle")
            } else {
                continue;
            };
            self.event(Some(id), vec![], text);
        }
    }

    /// Hiver rude : les vieux et les tout-petits d'un foyer sans feu peuvent mourir de froid.
    fn cold_deaths(&mut self) {
        let tick = self.tick;
        let frail: Vec<usize> = (0..self.people.len())
            .filter(|&i| {
                let p = &self.people[i];
                p.alive()
                    && !p.brain.leaving
                    && (p.elder(tick) || p.age(tick) < 2.0)
                    && p.mind.needs.warmth < 0.15
            })
            .collect();
        for i in frail {
            if self.rng.chance(0.05) {
                self.weather.stats.cold_deaths += 1;
                self.die_of(i, Some("de froid"));
            }
        }
    }

    // ------------------------------------------------------------------ pour les autres systèmes

    /// Bois de chauffe par personne et par jour, selon le froid de la veille.
    pub fn heating(&self, village: u32) -> f32 {
        let t = self
            .village_index(village)
            .map_or(base_temp(self.clock().season), |vi| {
                self.weather.local(vi).temp_mean
            });
        0.08 + 0.022 * (15.0 - t).max(0.0)
    }

    /// Ralentissement de la marche (neige, boue, crue ; pour un marchand, le temps qu'il fait
    /// sur sa route).
    pub(crate) fn walk_slow(&self, i: usize) -> f32 {
        let p = &self.people[i];
        if p.brain.traveling {
            return self.weather.trips.get(&p.id).map_or(1.0, |t| t.slow);
        }
        if self.weather.local.is_empty() {
            return 1.0;
        }
        self.village_index(p.village)
            .map_or(1.0, |vi| self.weather.local(vi).slow(self.tick))
    }

    /// Pousse des champs selon le temps (humidité du sol, froid, chaleur).
    pub(crate) fn weather_growth(&self, vi: usize) -> f32 {
        self.weather.local.get(vi).map_or(1.0, |l| l.crop)
    }

    /// Le froid favorise la fièvre.
    pub(crate) fn chill(&self, vi: usize) -> f32 {
        let t = self.weather.local(vi).temp_mean;
        1.0 + ((4.0 - t) / 8.0).clamp(0.0, 1.0)
    }

    // ------------------------------------------------------------------ la route

    /// Chaque heure, pour chaque marchand sur la route : neige et boue ralentissent, l'orage
    /// arrête, la charrette peut verser, les brigands rôdent loin des villages.
    fn road_hourly(&mut self) {
        let travellers: Vec<(u32, u32)> = self
            .trade
            .traders
            .iter()
            .filter(|(_, t)| matches!(t.leg, Leg::Outbound | Leg::Returning))
            .map(|(&id, t)| (id, t.dest))
            .collect();
        let live: BTreeSet<u32> = travellers.iter().map(|t| t.0).collect();
        self.weather.trips.retain(|id, _| live.contains(id));
        for (id, dest) in travellers {
            let i = id as usize;
            let p = &self.people[i];
            if !p.alive() || !p.brain.traveling {
                continue;
            }
            let (x, y) = (p.x, p.y);
            let home = p.village;
            let (rain, temp, storm) = self.weather_at(x, y);
            let near = self.nearest_village(x, y);
            let snow = match near {
                Some((vi, d)) if d < 90.0 => self.weather.local(vi).snow,
                _ if temp < 0.5 && rain > 0.1 => 0.5,
                _ => 0.0,
            };
            let road =
                self.map.inside(x as i64, y as i64) && self.map.road_at(self.map.idx(x as u32, y as u32));
            let mut slow = 1.0f32;
            if snow > 0.25 {
                slow *= 1.0 - 0.45 * ((snow - 0.25) / 0.75).min(1.0);
            }
            if rain > 0.4 && temp > 0.5 {
                slow *= if road { 0.9 } else { 0.7 };
            }
            if storm > 0.45 {
                slow = 0.0;
            }
            let trip = self.weather.trips.entry(id).or_insert(Trip {
                slow: 1.0,
                ..Default::default()
            });
            trip.slow = slow;
            if slow < 0.75 {
                trip.delay += 1;
                self.weather.stats.delays += 1;
            }
            let trip = *trip;
            // La charrette verse dans la boue, glisse sur la glace, perd une roue.
            let bad = rain + 2.0 * storm + snow;
            if !trip.mishap && bad > 0.2 && self.rng.chance(0.03 * bad * if road { 0.4 } else { 1.0 }) {
                self.mishap(i, dest, home);
            }
            // Les brigands : loin de tout village, plus rares sur les routes fréquentées.
            let far = near.is_none_or(|(_, d)| d > WILDS);
            if !trip.robbed && far && self.rng.chance(0.035 * if road { 0.35 } else { 1.0 }) {
                self.bandits(i, dest, home);
            }
        }
    }

    fn cargo_loss(&mut self, id: u32, part: f32) -> f32 {
        let Some(t) = self.trade.traders.get_mut(&id) else {
            return 0.0;
        };
        let mut lost = 0.0;
        for q in t.cargo.iter_mut() {
            let l = *q * part;
            *q -= l;
            lost += l;
        }
        lost
    }

    /// Accident de la route : une partie de la cargaison perdue, quelques heures de retard.
    fn mishap(&mut self, i: usize, dest: u32, home: u32) {
        let id = self.people[i].id;
        let part = 0.2 + 0.3 * self.rng.f32();
        let lost = self.cargo_loss(id, part);
        let trip = self.weather.trips.entry(id).or_default();
        trip.mishap = true;
        trip.delay += 2;
        *self.weather.danger.entry(pair(home, dest)).or_default() += 0.3;
        self.weather.stats.mishaps += 1;
        self.weather.stats.lost_goods += lost;
        self.people[i].mind.mood.add(Emotion::Anger, 0.3);
        if lost >= 4.0 {
            let back = self
                .trade
                .traders
                .get(&id)
                .is_some_and(|t| t.leg == Leg::Returning);
            let place = self.village_name(if back { home } else { dest });
            let first = self.people[i].first.clone();
            let text = format!(
                "La charrette de {first} verse en route vers {place} : {lost:.0} unités de marchandises perdues"
            );
            self.event(Some(home), vec![id], text);
        }
    }

    /// Des brigands détroussent le marchand : une bonne part de sa bourse et de sa cargaison.
    fn bandits(&mut self, i: usize, dest: u32, home: u32) {
        let id = self.people[i].id;
        let take = 0.4 + 0.4 * self.rng.f32();
        let coins = self.trade.traders.get_mut(&id).map_or(0.0, |t| {
            let c = t.purse.max(0.0) * take;
            t.purse -= c;
            c
        });
        let part = 0.2 + 0.3 * self.rng.f32();
        let goods = self.cargo_loss(id, part);
        self.weather.trips.entry(id).or_default().robbed = true;
        *self.weather.danger.entry(pair(home, dest)).or_default() += 1.0;
        let st = &mut self.weather.stats;
        st.robberies += 1;
        st.stolen_coins += coins;
        st.stolen_goods += goods;
        let tick = self.tick;
        let m = &mut self.people[i].mind;
        m.remember(tick, MemoryKind::Waylaid, None, -0.8);
        m.mood.add(Emotion::Fear, 0.6);
        m.needs.safety = (m.needs.safety - 0.4).max(0.0);
        let back = self
            .trade
            .traders
            .get(&id)
            .is_some_and(|t| t.leg == Leg::Returning);
        let place = self.village_name(if back { home } else { dest });
        let name = self.people[i].name();
        let loot = match (coins >= 1.0, goods >= 1.0) {
            (true, true) => format!("{coins:.0} sous et {goods:.0} unités de marchandises"),
            (true, false) => format!("{coins:.0} sous"),
            (false, true) => format!("{goods:.0} unités de marchandises"),
            (false, false) => "presque rien".to_string(),
        };
        self.event(
            Some(home),
            vec![id],
            format!("Des brigands détroussent {name} sur la route de {place} : {loot}"),
        );
    }

    /// Ce que craint un marchand sur un trajet (en sous) : les brigands et accidents qu'on y
    /// a connus, plus pour les prudents.
    pub(crate) fn road_fear(&self, i: usize, home: u32, dest: u32) -> f32 {
        let risk = self.people[i].mind.traits.risk;
        self.weather.danger(home, dest) * 4.0 * (1.5 - risk)
    }

    /// Le temps d'un village pour le Ciel (refroidir, éclaircir) : un front posé sur lui.
    pub(crate) fn divine_front(&mut self, vi: usize, kind: FrontKind) {
        let (x, y) = (self.villages[vi].x as f32 + 1.0, self.villages[vi].y as f32 + 1.0);
        self.spawn_front(kind, Some((x, y)));
    }

    /// Éclaircie : les fronts qui couvrent le village se dissipent.
    pub(crate) fn clear_sky(&mut self, vi: usize) -> usize {
        let (x, y) = (self.villages[vi].x as f32 + 1.0, self.villages[vi].y as f32 + 1.0);
        let before = self.weather.fronts.len();
        self.weather
            .fronts
            .retain(|f| !(f.kind != FrontKind::Dry && (f.x - x).hypot(f.y - y) < f.radius));
        let gone = before - self.weather.fronts.len();
        let l = self.weather.local_mut(vi);
        l.rain = 0.0;
        l.storm = 0.0;
        gone
    }

    /// Une forêt où mettre le feu, autour d'une case.
    pub(crate) fn forest_around(&self, x: u32, y: u32) -> Option<u32> {
        for r in 0..=3i64 {
            for dy in -r..=r {
                for dx in -r..=r {
                    let (tx, ty) = (x as i64 + dx, y as i64 + dy);
                    if !self.map.inside(tx, ty) {
                        continue;
                    }
                    let i = self.map.idx(tx as u32, ty as u32);
                    if matches!(self.map.biome_at(i), Biome::Forest | Biome::Conifers) {
                        return Some(i as u32);
                    }
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        clock::Season,
        map::{Biome, Map},
        settlement::FieldState,
        tuning::*,
        weather::FrontKind,
        world::{Site, World},
    };

    fn meadow(size: u32) -> World {
        let mut map = Map::new(size, size);
        map.biome.fill(Biome::Meadow as u8);
        map.elevation.fill(100);
        map.fertility.fill(200);
        let site = Site {
            name: "Clairval".into(),
            x: size / 2,
            y: size / 2,
            color: 0xff8800,
        };
        World::from_map(42, map, &[site])
    }

    /// Les fronts naissent à l'ouest, traversent la carte, et il pleut au village quand l'un
    /// d'eux passe ; le sol garde la pluie.
    #[test]
    fn fronts_cross_the_map_and_bring_rain() {
        let mut w = meadow(200);
        let (mut rained, mut fronts, mut soaked) = (0, 0, 0);
        let mut wet = w.weather.local(0).wet;
        for _ in 0..(10 * DAY) {
            w.step();
            if w.tick.is_multiple_of(HOUR) {
                let l = w.weather.local(0);
                if l.rain > 0.1 {
                    rained += 1;
                }
                if l.wet > wet + 0.005 {
                    soaked += 1;
                }
                wet = l.wet;
                fronts = fronts.max(w.weather.fronts.len());
            }
        }
        assert!(w.weather.stats.fronts >= 5, "{}", w.weather.stats.fronts);
        assert!(fronts >= 1);
        assert!(rained >= 5, "{rained}");
        assert!(soaked >= 5, "{soaked}");
        // Les fronts avancent vers l'est.
        assert!(w.weather.fronts.iter().all(|f| f.vx > 0.0));
    }

    /// La pluie fait pousser, la sécheresse arrête la pousse.
    #[test]
    fn dry_soil_slows_crops() {
        let mut w = meadow(120);
        w.step();
        w.weather.local_mut(0).wet = 0.05;
        w.weather.local_mut(0).temp_mean = 18.0;
        w.weather.local_mut(0).crop = w.weather.local(0).crop_factor();
        assert!(w.weather_growth(0) <= 0.31);
        w.weather.local_mut(0).wet = 0.6;
        w.weather.local_mut(0).crop = w.weather.local(0).crop_factor();
        assert!(w.weather_growth(0) > 1.0);
    }

    /// Trop de pluie près d'une rivière : les champs proches de l'eau sont noyés.
    #[test]
    fn rivers_flood_after_heavy_rain() {
        let mut w = meadow(120);
        let v = w.villages[0].clone();
        // Une rivière qui longe les champs.
        for f in &v.fields {
            for y in 0..120 {
                let i = w.map.idx(f.x + 3, y);
                w.edit_tile(i, |m, i| m.flags[i] |= 2);
            }
        }
        for k in 0..w.villages[0].fields.len() {
            w.villages[0].fields[k].state = FieldState::Sown;
        }
        w.step();
        w.weather.local_mut(0).river = 255;
        w.weather.local_mut(0).flood_water = 100.0;
        w.tick = 22 * DAY;
        w.village_weather(0);
        assert_eq!(w.weather.stats.floods, 1);
        assert!(w.weather.stats.fields_lost >= 1);
        assert!(w.weather.local(0).flooded_until > w.tick);
        // Pas de seconde crue tout de suite.
        w.weather.local_mut(0).flood_water = 100.0;
        w.village_weather(0);
        assert_eq!(w.weather.stats.floods, 1);
    }

    /// Un feu dans une forêt sèche s'étend puis s'éteint ; la forêt brûlée devient lande, et
    /// repousse au bout de deux ans.
    #[test]
    fn wildfire_spreads_then_regrows() {
        let mut w = meadow(160);
        for y in 0..60 {
            for x in 0..60 {
                let i = w.map.idx(x, y);
                w.set_biome(i, Biome::Forest);
            }
        }
        w.step();
        w.weather.local_mut(0).wet = 0.0;
        let tile = w.map.idx(30, 30) as u32;
        assert!(w.start_fire(tile, true, "le Ciel").is_some());
        for _ in 0..(3 * DAY) {
            w.step();
            w.weather.fronts.clear();
            w.weather.local_mut(0).wet = 0.0;
        }
        assert!(w.weather.fires.is_empty(), "{:?}", w.weather.fires.len());
        let burnt = w.weather.stats.burnt_tiles;
        assert!(burnt >= 10, "{burnt}");
        assert_eq!(w.map.biome_at(tile as usize), Biome::Heath);
        w.tick += 2 * YEAR;
        w.daily_weather();
        assert_eq!(w.map.biome_at(tile as usize), Biome::Forest);
    }

    /// Un hiver rude fait brûler plus de bois.
    #[test]
    fn harsh_winter_needs_more_wood() {
        let mut w = meadow(120);
        w.step();
        let id = w.villages[0].id;
        w.weather.local_mut(0).temp_mean = 1.0;
        let normal = w.heating(id);
        w.weather.local_mut(0).temp_mean = -8.0;
        assert!(w.heating(id) > normal * 1.3);
        w.weather.anomaly_season = Some(Season::Winter);
        w.weather.anomaly = -8.0;
        assert!(w.weather.harsh());
    }

    /// Le Ciel peut poser un front sur un village (vague de froid) ou dissiper les nuages.
    #[test]
    fn divine_fronts() {
        let mut w = meadow(120);
        w.step();
        w.weather.fronts.clear();
        w.divine_front(0, FrontKind::Cold);
        let (x, y) = (w.villages[0].x as f32 + 1.0, w.villages[0].y as f32 + 1.0);
        for _ in 0..(8 * HOUR) {
            w.step();
        }
        let cold = w.weather_at(x, y).1;
        w.weather.fronts.clear();
        assert!(cold < w.weather_at(x, y).1 - 4.0, "{cold}");
        w.divine_front(0, FrontKind::Rain);
        assert!(w.clear_sky(0) >= 1);
    }
}

//! Pages du site (phase 7) : tableaux de bord, arbre généalogique, carte du commerce,
//! chronique complète et résumé « depuis ta dernière visite ».

use std::collections::{BTreeMap, BTreeSet};

use hameaux_core::{
    Clock, World,
    clock::{DAYS_PER_SEASON, DAYS_PER_YEAR},
    economy::{GOODS, Good, Inv},
    events::Event,
    history::{Kind, Sample, VillageHistory},
    people::{Person, Status},
    sim::tick_day,
    trade::Leg,
    tuning::YEAR,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{EventView, GoodAmount, VillageRef};

const SEASONS: [&str; 4] = ["printemps", "été", "automne", "hiver"];

/// « an 3, été j4 » pour un jour du monde.
pub fn day_label(day: u32) -> String {
    let d = day as u64;
    let in_year = d % DAYS_PER_YEAR;
    format!(
        "an {}, {} j{}",
        d / DAYS_PER_YEAR + 1,
        SEASONS[(in_year / DAYS_PER_SEASON) as usize],
        in_year % DAYS_PER_SEASON + 1
    )
}

// ------------------------------------------------------------------ tableaux de bord

/// Une courbe par village (population de chacun, pour le tableau de bord du monde).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SeriesView {
    pub id: u32,
    pub name: String,
    pub color: u32,
    pub values: Vec<f32>,
}

/// Relevés d'un village (ou du monde entier), en colonnes : l'élément `k` de chaque tableau
/// correspond au jour `days[k]`. Réponse de `GET /api/village/{id}/history` et
/// `GET /api/world/history` (`?all=true` : un relevé par semaine depuis le début).
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct HistoryView {
    pub name: String,
    pub color: u32,
    pub days: Vec<u32>,
    /// « an 3, été j4 ».
    pub labels: Vec<String>,
    pub population: Vec<u32>,
    pub children: Vec<u32>,
    pub elders: Vec<u32>,
    /// Depuis le relevé précédent.
    pub births: Vec<u32>,
    pub deaths: Vec<u32>,
    pub departures: Vec<u32>,
    /// Familles arrivées depuis le relevé précédent.
    pub arrivals: Vec<u32>,
    pub houses: Vec<u32>,
    pub sick: Vec<u32>,
    pub food_days: Vec<f32>,
    pub treasury: Vec<f32>,
    pub coins: Vec<f32>,
    pub tax: Vec<f32>,
    /// Par bien (index de `MapInfo::goods`), puis par relevé.
    pub stock: Vec<Vec<f32>>,
    pub price: Vec<Vec<f32>>,
    pub volume: Vec<Vec<f32>>,
    /// Monde : population de chaque village.
    pub villages: Vec<SeriesView>,
}

impl HistoryView {
    fn push(&mut self, s: &Sample) {
        self.days.push(s.day);
        self.labels.push(day_label(s.day));
        self.population.push(s.population as u32);
        self.children.push(s.children as u32);
        self.elders.push(s.elders as u32);
        self.births.push(s.births as u32);
        self.deaths.push(s.deaths as u32);
        self.departures.push(s.departures as u32);
        self.arrivals.push(s.arrivals as u32);
        self.houses.push(s.houses as u32);
        self.sick.push(s.sick as u32);
        self.food_days.push(s.food_days);
        self.treasury.push(s.treasury);
        self.coins.push(s.coins);
        self.tax.push(s.tax);
        for g in 0..GOODS {
            self.stock[g].push(s.stock[g]);
            self.price[g].push(s.price[g]);
            self.volume[g].push(s.volume[g]);
        }
    }

    fn empty(name: String, color: u32) -> Self {
        HistoryView {
            name,
            color,
            stock: vec![Vec::new(); GOODS],
            price: vec![Vec::new(); GOODS],
            volume: vec![Vec::new(); GOODS],
            ..Default::default()
        }
    }

    pub fn village(world: &World, id: u32, all: bool) -> Option<Self> {
        let v = &world.villages[world.village_index(id)?];
        let mut out = Self::empty(v.name.clone(), v.color);
        if let Some(h) = world.history.villages.get(&id) {
            for s in samples(h, all) {
                out.push(s);
            }
        }
        Some(out)
    }

    /// Le monde entier : sommes (et moyennes pondérées par la population) des villages.
    pub fn world(world: &World, all: bool) -> Self {
        let mut out = Self::empty("Le monde".into(), 0xffffff);
        let series: Vec<(&hameaux_core::settlement::Village, Vec<&Sample>)> = world
            .villages
            .iter()
            .filter_map(|v| {
                let h = world.history.villages.get(&v.id)?;
                Some((v, samples(h, all).collect()))
            })
            .collect();
        let days: BTreeSet<u32> = series.iter().flat_map(|(_, s)| s.iter().map(|s| s.day)).collect();
        let mut per_village: Vec<Vec<f32>> = vec![Vec::new(); series.len()];
        for &day in &days {
            let mut sum = Sample {
                day,
                ..Default::default()
            };
            let (mut weight, mut food, mut tax) = (0.0f32, 0.0f32, 0.0f32);
            let mut price: Inv = [0.0; GOODS];
            for (k, (_, list)) in series.iter().enumerate() {
                // Le dernier relevé de ce village à cette date (s'il existait déjà).
                let at = list.partition_point(|s| s.day <= day);
                let Some(s) = at.checked_sub(1).map(|i| list[i]) else {
                    per_village[k].push(0.0);
                    continue;
                };
                per_village[k].push(s.population as f32);
                if s.day == day {
                    sum.births += s.births;
                    sum.deaths += s.deaths;
                    sum.departures += s.departures;
                    sum.arrivals += s.arrivals;
                }
                sum.population += s.population;
                sum.children += s.children;
                sum.elders += s.elders;
                sum.houses += s.houses;
                sum.sick += s.sick;
                sum.treasury += s.treasury;
                sum.coins += s.coins;
                let w = s.population as f32;
                weight += w;
                food += s.food_days * w;
                tax += s.tax * w;
                for g in 0..GOODS {
                    sum.stock[g] += s.stock[g];
                    sum.volume[g] += s.volume[g];
                    price[g] += s.price[g] * w;
                }
            }
            if weight > 0.0 {
                sum.food_days = food / weight;
                sum.tax = tax / weight;
                for g in 0..GOODS {
                    sum.price[g] = price[g] / weight;
                }
            }
            out.push(&sum);
        }
        out.villages = series
            .iter()
            .zip(per_village)
            .map(|((v, _), values)| SeriesView {
                id: v.id,
                name: v.name.clone(),
                color: v.color,
                values,
            })
            .collect();
        out
    }
}

fn samples(h: &VillageHistory, all: bool) -> Box<dyn Iterator<Item = &Sample> + '_> {
    if all {
        // Les semaines, puis les derniers jours depuis la dernière semaine.
        let last = h.weeks.last().map(|w| w.day);
        Box::new(
            h.weeks
                .iter()
                .chain(h.days.iter().filter(move |d| last.is_none_or(|l| d.day > l))),
        )
    } else {
        Box::new(h.days.iter())
    }
}

// ------------------------------------------------------------------ arbre généalogique

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TreePerson {
    pub id: u32,
    pub name: String,
    pub female: bool,
    pub alive: bool,
    /// « an 3 – an 71 », « an 12 – », « ≈ an −30 – an 40 ».
    pub years: String,
    /// Métier ou « enfant ».
    pub role: String,
    pub village: String,
    pub parents: Vec<u32>,
    pub partner: Option<u32>,
    pub children: Vec<u32>,
    /// 0 : la personne de l'arbre ; négatif : ancêtres ; positif : descendants.
    pub generation: i32,
}

/// Réponse de `GET /api/tree/{id}` : ancêtres (3 générations), frères et sœurs, conjoints et
/// descendants (3 générations) d'un habitant.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FamilyTree {
    pub root: u32,
    pub people: Vec<TreePerson>,
}

const TREE_DEPTH: i32 = 3;
const TREE_MAX: usize = 160;

impl FamilyTree {
    pub fn of(world: &World, root: u32) -> Option<Self> {
        world.people.get(root as usize)?;
        let mut depth: BTreeMap<u32, i32> = BTreeMap::new();
        depth.insert(root, 0);
        // Ancêtres.
        let mut wave = vec![root];
        for g in 1..=TREE_DEPTH {
            let mut next = Vec::new();
            for id in wave {
                for &p in &world.people[id as usize].parents {
                    if depth.len() < TREE_MAX && !depth.contains_key(&p) {
                        depth.insert(p, -g);
                        next.push(p);
                    }
                }
            }
            wave = next;
        }
        // Frères et sœurs.
        for &p in &world.people[root as usize].parents {
            for &c in &world.people[p as usize].children {
                if depth.len() < TREE_MAX {
                    depth.entry(c).or_insert(0);
                }
            }
        }
        // Descendants, avec leurs conjoints.
        let mut wave = vec![root];
        for g in 0..=TREE_DEPTH {
            let mut next = Vec::new();
            for id in wave {
                let p = &world.people[id as usize];
                if let Some(q) = p.partner
                    && depth.len() < TREE_MAX
                {
                    depth.entry(q).or_insert(g);
                }
                if g == TREE_DEPTH {
                    continue;
                }
                for &c in &p.children {
                    if depth.len() < TREE_MAX && !depth.contains_key(&c) {
                        depth.insert(c, g + 1);
                        next.push(c);
                    }
                }
            }
            wave = next;
        }
        let tick = world.tick;
        let people = depth
            .iter()
            .map(|(&id, &generation)| {
                let p = &world.people[id as usize];
                TreePerson {
                    id,
                    name: p.name(),
                    female: p.female,
                    alive: p.alive(),
                    years: years(p),
                    role: if p.alive() && p.child(tick) {
                        "enfant".into()
                    } else {
                        p.job.map(|j| j.label(p.female).to_string()).unwrap_or_default()
                    },
                    village: world.village_name(p.village),
                    parents: p.parents.clone(),
                    partner: p.partner,
                    children: p.children.clone(),
                    generation,
                }
            })
            .collect();
        Some(FamilyTree { root, people })
    }
}

fn year_of(tick: i64) -> String {
    if tick < 0 {
        format!("≈ an −{}", (-tick) as u64 / YEAR)
    } else {
        format!("an {}", Clock::at(tick as u64).year)
    }
}

fn years(p: &Person) -> String {
    let born = year_of(p.born);
    match p.status {
        Status::Alive => format!("{born} –"),
        Status::Dead { tick } => format!("{born} – {}", year_of(tick as i64)),
        Status::Left { tick } => format!("{born} – parti{} an {}", p.e(), Clock::at(tick).year),
    }
}

// ------------------------------------------------------------------ carte du commerce

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TradeVillage {
    pub id: u32,
    pub name: String,
    pub x: u32,
    pub y: u32,
    pub color: u32,
    pub population: u32,
    pub merchants: u32,
    /// Village d'où sont venus ses fondateurs.
    pub founded_from: Option<u32>,
}

/// Un trajet parcouru entre deux villages.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RouteView {
    pub a: u32,
    pub b: u32,
    pub trips: u32,
    pub road: bool,
    /// Cases du trajet (une sur quelques-unes), `[x, y]`.
    pub path: Vec<[u32; 2]>,
}

/// Ce que les marchands de `from` ont vendu à `to`, depuis le début des relevés.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FlowView {
    pub from: u32,
    pub to: u32,
    /// Par bien (index de `MapInfo::goods`).
    pub goods: Vec<f32>,
    pub units: f32,
    pub coins: f32,
}

/// Une charrette en voyage.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CartView {
    pub id: u32,
    pub name: String,
    pub home: u32,
    pub dest: u32,
    pub x: f32,
    pub y: f32,
    /// « en route vers », « au marché de », « rentre de ».
    pub leg: String,
    pub cargo: Vec<GoodAmount>,
}

/// Réponse de `GET /api/trade`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TradeMap {
    pub width: u32,
    pub height: u32,
    pub villages: Vec<TradeVillage>,
    pub routes: Vec<RouteView>,
    pub flows: Vec<FlowView>,
    pub carts: Vec<CartView>,
    /// Unités vendues par des marchands depuis le début, par bien.
    pub sold: Vec<f32>,
}

impl TradeMap {
    pub fn of(world: &World) -> Self {
        let w = world.map.width;
        let merchants = |id: u32| {
            world
                .trade
                .traders
                .keys()
                .filter(|&&p| world.people[p as usize].village == id && world.people[p as usize].alive())
                .count() as u32
        };
        let villages = world
            .villages
            .iter()
            .map(|v| TradeVillage {
                id: v.id,
                name: v.name.clone(),
                x: v.x,
                y: v.y,
                color: v.color,
                population: world.population(v.id) as u32,
                merchants: merchants(v.id),
                founded_from: world.trade.founded_from.get(&v.id).copied(),
            })
            .collect();
        let routes = world
            .trade
            .routes
            .iter()
            .filter(|(_, r)| r.reachable() && (r.trips > 0 || r.road))
            .map(|(&(a, b), r)| {
                let n = r.path.len();
                let path = r
                    .path
                    .iter()
                    .enumerate()
                    .filter(|&(k, _)| k % 6 == 0 || k + 1 == n)
                    .map(|(_, &i)| [i % w, i / w])
                    .collect();
                RouteView {
                    a,
                    b,
                    trips: r.trips,
                    road: r.road,
                    path,
                }
            })
            .collect();
        let flows = world
            .history
            .flows
            .iter()
            .map(|(&(from, to), inv)| FlowView {
                from,
                to,
                goods: inv.to_vec(),
                units: inv.iter().sum(),
                coins: world.history.flow_coins.get(&(from, to)).copied().unwrap_or(0.0),
            })
            .filter(|f| f.units >= 1.0)
            .collect();
        let carts = world
            .trade
            .traders
            .iter()
            .filter(|(_, t)| t.leg.away())
            .filter_map(|(&id, t)| {
                let p = world.people.get(id as usize).filter(|p| p.alive())?;
                let leg = match t.leg {
                    Leg::Outbound => "en route vers",
                    Leg::Selling => "au marché de",
                    _ => "rentre de",
                };
                Some(CartView {
                    id,
                    name: p.name(),
                    home: p.village,
                    dest: t.dest,
                    x: p.x,
                    y: p.y,
                    leg: leg.into(),
                    cargo: Good::ALL
                        .iter()
                        .filter(|&&g| t.cargo[g as usize] >= 1.0)
                        .map(|&g| GoodAmount {
                            good: g as u8,
                            label: g.label().into(),
                            amount: t.cargo[g as usize],
                        })
                        .collect(),
                })
            })
            .collect();
        TradeMap {
            width: world.map.width,
            height: world.map.height,
            villages,
            routes,
            flows,
            carts,
            sold: world.trade.sold.to_vec(),
        }
    }
}

// ------------------------------------------------------------------ chronique complète

/// Paramètres de `GET /api/chronicle`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ChronicleQuery {
    /// Événements de numéro strictement inférieur (page précédente).
    pub before: Option<u64>,
    pub village: Option<u32>,
    pub person: Option<u32>,
    /// Sorte d'événement (index de `MapInfo::event_kinds`).
    pub kind: Option<u8>,
    /// Texte cherché (sans tenir compte des majuscules).
    pub q: Option<String>,
    /// Chercher dans les annales (événements marquants, gardés bien plus longtemps).
    pub annals: Option<bool>,
    pub limit: Option<usize>,
}

/// Réponse de `GET /api/chronicle` : du plus récent au plus ancien.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct EventPage {
    pub events: Vec<EventView>,
    /// Il reste des événements plus anciens qui correspondent.
    pub more: bool,
    /// Date du plus ancien événement encore connu de la source (chronique ou annales).
    pub oldest: String,
}

impl EventPage {
    pub fn of(world: &World, q: &ChronicleQuery) -> Self {
        let limit = q.limit.unwrap_or(50).clamp(1, 200);
        let needle = q.q.as_deref().map(str::to_lowercase).filter(|s| !s.is_empty());
        let annals = q.annals.unwrap_or(false);
        let source: Box<dyn DoubleEndedIterator<Item = &Event>> = if annals {
            Box::new(world.history.annals.iter())
        } else {
            Box::new(world.chronicle.events.iter())
        };
        let oldest = if annals {
            world.history.annals.front()
        } else {
            world.chronicle.events.front()
        }
        .map(|e| EventView::of(world, e).date)
        .unwrap_or_default();
        let mut matching = source
            .rev()
            .filter(|e| q.before.is_none_or(|b| e.seq < b))
            .filter(|e| q.village.is_none_or(|v| e.village == Some(v)))
            .filter(|e| q.person.is_none_or(|p| e.people.contains(&p)))
            .filter(|e| q.kind.is_none_or(|k| Kind::of(&e.text) as u8 == k))
            .filter(|e| {
                needle
                    .as_ref()
                    .is_none_or(|n| e.text.to_lowercase().contains(n.as_str()))
            });
        let events: Vec<EventView> = matching
            .by_ref()
            .take(limit)
            .map(|e| EventView::of(world, e))
            .collect();
        let more = matching.next().is_some();
        EventPage { events, more, oldest }
    }
}

// ------------------------------------------------------------------ depuis ta dernière visite

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct VillageDelta {
    pub village: VillageRef,
    pub color: u32,
    /// Population à la dernière visite (0 s'il n'existait pas encore).
    pub then: u32,
    pub now: u32,
    /// Fondé depuis.
    pub new: bool,
    pub births: u32,
    pub deaths: u32,
}

/// Nombre d'événements d'une sorte.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct KindCount {
    pub kind: u8,
    pub label: String,
    pub count: u32,
}

/// Réponse de `GET /api/summary?since=TICK` : ce qui s'est passé depuis une date.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Summary {
    #[ts(type = "number")]
    pub since: u64,
    pub from: String,
    pub to: String,
    /// Temps de jeu écoulé (« 3 jours et 5 heures », « 2 ans et 4 jours »).
    pub elapsed: String,
    pub births: u32,
    pub deaths: u32,
    pub departures: u32,
    pub population_then: u32,
    pub population_now: u32,
    pub villages: Vec<VillageDelta>,
    /// Événements de la chronique par sorte (si elle remonte jusque-là).
    pub kinds: Vec<KindCount>,
    /// Les événements marquants, du plus récent au plus ancien.
    pub highlights: Vec<EventView>,
    /// La chronique ne remonte pas jusqu'à la dernière visite : les décomptes par sorte sont
    /// partiels (les naissances, décès et populations restent exacts).
    pub partial: bool,
}

const HIGHLIGHTS: usize = 40;

impl Summary {
    pub fn since(world: &World, since: u64) -> Self {
        let tick = world.tick;
        let since = since.min(tick);
        let day = tick_day(since) as u32;
        let (mut births, mut deaths, mut departures) = (0u32, 0u32, 0u32);
        let mut by_village: BTreeMap<u32, (u32, u32)> = BTreeMap::new();
        for p in &world.people {
            if p.born > since as i64 && p.parents.len() == 2 {
                births += 1;
                by_village.entry(p.village).or_default().0 += 1;
            }
            match p.status {
                Status::Dead { tick: t } if t > since => {
                    deaths += 1;
                    by_village.entry(p.village).or_default().1 += 1;
                }
                Status::Left { tick: t } if t > since => departures += 1,
                _ => {}
            }
        }
        let villages: Vec<VillageDelta> = world
            .villages
            .iter()
            .map(|v| {
                let new = v.founded > since;
                let then = if new {
                    0
                } else {
                    world
                        .history
                        .villages
                        .get(&v.id)
                        .and_then(|h| h.at(day))
                        .map_or(0, |s| s.population as u32)
                };
                let (b, d) = by_village.get(&v.id).copied().unwrap_or_default();
                VillageDelta {
                    village: VillageRef {
                        id: v.id,
                        name: v.name.clone(),
                    },
                    color: v.color,
                    then,
                    now: world.population(v.id) as u32,
                    new,
                    births: b,
                    deaths: d,
                }
            })
            .collect();
        let partial = world.chronicle.events.front().is_none_or(|e| e.tick > since)
            && world.chronicle.next_seq > world.chronicle.events.len() as u64;
        let mut counts: BTreeMap<Kind, u32> = BTreeMap::new();
        for e in world.chronicle.events.iter().filter(|e| e.tick > since) {
            *counts.entry(Kind::of(&e.text)).or_default() += 1;
        }
        // Annales, plus ce que la chronique a de marquant depuis le dernier relevé.
        let mut highlights: Vec<&Event> = world
            .history
            .annals
            .iter()
            .filter(|e| e.tick > since)
            .chain(
                world
                    .chronicle
                    .since(world.history.annals_seen)
                    .filter(|e| e.tick > since && Kind::of(&e.text).notable(&e.text)),
            )
            .collect();
        highlights.reverse();
        highlights.truncate(HIGHLIGHTS);
        Summary {
            since,
            from: Clock::at(since).to_string(),
            to: Clock::at(tick).to_string(),
            elapsed: elapsed(tick - since),
            births,
            deaths,
            departures,
            population_then: villages.iter().map(|v| v.then).sum(),
            population_now: villages.iter().map(|v| v.now).sum(),
            villages,
            kinds: counts
                .into_iter()
                .map(|(k, count)| KindCount {
                    kind: k as u8,
                    label: k.label().into(),
                    count,
                })
                .collect(),
            highlights: highlights.into_iter().map(|e| EventView::of(world, e)).collect(),
            partial,
        }
    }
}

/// « 2 ans et 4 jours », « 3 jours et 5 heures », « 40 minutes ».
pub fn elapsed(ticks: u64) -> String {
    use hameaux_core::tuning::{DAY, HOUR, MINUTE};
    let plural = |n: u64, word: &str| {
        if n > 1 {
            format!("{n} {word}s")
        } else {
            format!("{n} {word}")
        }
    };
    let (years, days) = (ticks / YEAR, ticks % YEAR / DAY);
    let (hours, minutes) = (ticks % DAY / HOUR, ticks % HOUR / MINUTE);
    let parts: Vec<String> = if years > 0 {
        vec![plural(years, "an"), plural(days, "jour")]
    } else if days > 0 {
        vec![plural(days, "jour"), plural(hours, "heure")]
    } else if hours > 0 {
        vec![plural(hours, "heure"), plural(minutes, "minute")]
    } else {
        vec![plural(minutes, "minute")]
    };
    let parts: Vec<String> = parts.into_iter().filter(|p| !p.starts_with("0 ")).collect();
    match parts.len() {
        0 => "un instant".into(),
        1 => parts[0].clone(),
        _ => parts.join(" et "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        assert_eq!(day_label(0), "an 1, printemps j1");
        assert_eq!(day_label(30), "an 2, printemps j3");
        assert_eq!(day_label(27), "an 1, hiver j7");
        let day = hameaux_core::tuning::DAY;
        assert_eq!(
            elapsed(3 * day + 5 * hameaux_core::tuning::HOUR),
            "3 jours et 5 heures"
        );
        assert_eq!(elapsed(YEAR), "1 an");
        assert_eq!(elapsed(0), "un instant");
    }

    /// Un petit monde qui a vécu trois semaines.
    fn lived() -> World {
        let p = hameaux_worldgen::Params {
            width: 384,
            height: 384,
            villages: 4,
            ..Default::default()
        };
        let mut w = hameaux_worldgen::generate(7, &p);
        for _ in 0..21 * hameaux_core::tuning::DAY {
            w.step();
        }
        w
    }

    #[test]
    fn pages_of_a_living_world() {
        let w = lived();
        let id = w.villages[0].id;

        // Tableaux de bord : un relevé par jour, des colonnes de même longueur.
        let h = HistoryView::village(&w, id, false).unwrap();
        assert_eq!(h.days.len(), 21);
        assert_eq!(h.labels.len(), 21);
        assert_eq!(h.price.len(), GOODS);
        assert!(h.price.iter().all(|p| p.len() == 21));
        assert_eq!(*h.population.last().unwrap() as usize, w.population(id));
        let all = HistoryView::village(&w, id, true).unwrap();
        assert!(all.days.windows(2).all(|d| d[0] < d[1]), "{:?}", all.days);
        assert_eq!(*all.days.last().unwrap(), *h.days.last().unwrap());
        let world = HistoryView::world(&w, false);
        let total: usize = w.villages.iter().map(|v| w.population(v.id)).sum();
        assert_eq!(*world.population.last().unwrap() as usize, total);
        assert_eq!(world.villages.len(), w.villages.len());
        assert!(HistoryView::village(&w, 999, false).is_none());

        // Arbre : un enfant né ici, avec ses parents au-dessus de lui.
        let child = w
            .people
            .iter()
            .find(|p| p.parents.len() == 2 && p.born > 0)
            .expect("une naissance");
        let t = FamilyTree::of(&w, child.id).unwrap();
        let generation = |id: u32| t.people.iter().find(|p| p.id == id).map(|p| p.generation);
        assert_eq!(generation(child.id), Some(0));
        for &p in &child.parents {
            assert_eq!(generation(p), Some(-1));
        }
        let parent = FamilyTree::of(&w, child.parents[0]).unwrap();
        assert_eq!(
            parent
                .people
                .iter()
                .find(|p| p.id == child.id)
                .unwrap()
                .generation,
            1
        );

        // Chronique : pages successives sans trou ni doublon, filtres.
        let q = ChronicleQuery {
            limit: Some(10),
            ..Default::default()
        };
        let first = EventPage::of(&w, &q);
        assert_eq!(first.events.len(), 10);
        assert!(first.more);
        let next = EventPage::of(
            &w,
            &ChronicleQuery {
                before: Some(first.events.last().unwrap().seq),
                ..q.clone()
            },
        );
        assert_eq!(next.events[0].seq + 1, first.events.last().unwrap().seq);
        let births = EventPage::of(
            &w,
            &ChronicleQuery {
                kind: Some(Kind::Birth as u8),
                limit: Some(200),
                ..Default::default()
            },
        );
        assert!(!births.events.is_empty());
        assert!(births.events.iter().all(|e| e.text.starts_with("Naissance")));
        let found = EventPage::of(
            &w,
            &ChronicleQuery {
                q: Some("NAISSANCE".into()),
                village: Some(births.events[0].village.unwrap()),
                ..Default::default()
            },
        );
        assert!(found.events.iter().all(|e| e.village == births.events[0].village));

        // Depuis la dernière visite : les naissances comptées correspondent à la chronique.
        let since = 7 * hameaux_core::tuning::DAY;
        let s = Summary::since(&w, since);
        let born = w
            .people
            .iter()
            .filter(|p| p.born > since as i64 && p.parents.len() == 2)
            .count();
        assert_eq!(s.births as usize, born);
        assert_eq!(s.population_now as usize, total);
        assert!(s.population_then > 0);
        assert!(!s.partial);

        // Commerce : la carte se construit (même sans voyage encore).
        let t = TradeMap::of(&w);
        assert_eq!(t.villages.len(), w.villages.len());
        assert_eq!(t.sold.len(), GOODS);
    }
}

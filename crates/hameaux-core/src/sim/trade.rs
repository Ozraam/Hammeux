//! Entre villages : marchands, routes, nouvelles des prix, mariages lointains, migrations
//! et fondation de nouveaux villages.

use crate::{
    economy::{GOODS, Good},
    map::Biome,
    mind::{Emotion, MemoryKind},
    names,
    people::{Act, Carry, Intent, Job, Step},
    settlement::HouseState,
    trade::{CART, Leg, NO_GOODS, PriceNews, ROAD_TRIPS, Route, TRADE_RANGE, Trader, load, pair},
    tuning::*,
    world::{Caches, Site, World, occ},
};

use super::{DayPart, day_part, life::de};

/// Ce qu'un marchand compte charger au départ (bien, quantité).
type Cargo = Option<(Good, f32)>;

/// Taxe du marché (la même que dans `market.rs`).
const TAX: f32 = 0.10;
/// Bénéfice attendu (sous) en dessous duquel un marchand ne part pas.
const MIN_PROFIT: f32 = 4.0;
/// Population à partir de laquelle un village essaime.
const COLONY_POP: usize = 60;
/// Villages au plus sur la carte.
const MAX_VILLAGES: usize = 30;

impl World {
    /// Reprise d'une sauvegarde sans commerce : revenus complétés pour les nouveaux métiers.
    pub(crate) fn bootstrap_trade(&mut self) {
        for vi in 0..self.villages.len() {
            self.economy.ensure_village(vi);
        }
    }

    // ------------------------------------------------------------------ géographie

    /// Distance à vol d'oiseau entre deux villages (cases).
    pub(crate) fn village_distance(&self, a: usize, b: usize) -> f32 {
        let (va, vb) = (&self.villages[a], &self.villages[b]);
        ((va.x as f32 - vb.x as f32).powi(2) + (va.y as f32 - vb.y as f32).powi(2)).sqrt()
    }

    /// Villages à portée de commerce, du plus proche au plus lointain (5 au plus).
    pub(crate) fn neighbours(&self, vi: usize) -> Vec<usize> {
        let mut near: Vec<(f32, usize)> = (0..self.villages.len())
            .filter(|&j| j != vi)
            .map(|j| (self.village_distance(vi, j), j))
            .filter(|&(d, _)| d <= TRADE_RANGE)
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        near.into_iter().take(5).map(|(_, j)| j).collect()
    }

    /// Calcule (une fois) le trajet à pied entre deux villages. Vrai s'il existe.
    pub(crate) fn ensure_route(&mut self, a: usize, b: usize) -> bool {
        let (ia, ib) = (self.villages[a].id, self.villages[b].id);
        if let Some(r) = self.trade.route(ia, ib) {
            return r.reachable();
        }
        let (lo, hi) = if ia < ib { (a, b) } else { (b, a) };
        let door = |w: &World, v: usize| {
            let (x, y) = w.villages[v].door();
            w.map.idx(x, y) as u32
        };
        let (from, to) = (door(self, lo), door(self, hi));
        let budget = self.map.len();
        let path = {
            let Caches { occ: grid, pf, .. } = &mut self.caches.0;
            pf.find(
                &self.map,
                from,
                to,
                |j| grid[j] == occ::HOUSE || grid[j] == occ::CIVIC,
                budget,
            )
        };
        let path = path
            .map(|p| std::iter::once(from).chain(p).collect())
            .unwrap_or_default();
        let route = Route {
            path,
            trips: 0,
            road: false,
        };
        let ok = route.reachable();
        self.trade.routes.insert(pair(ia, ib), route);
        ok
    }

    /// Un voyage de plus sur un trajet ; assez de passages en font une route.
    fn travelled(&mut self, a: u32, b: u32) {
        let Some(r) = self.trade.routes.get_mut(&pair(a, b)) else {
            return;
        };
        r.trips += 1;
        if r.road || r.trips < ROAD_TRIPS {
            return;
        }
        r.road = true;
        let path = r.path.clone();
        for t in path {
            let t = t as usize;
            if self.occ(t) == occ::NONE {
                self.clear_forest(t);
                self.set_road(t);
            }
        }
        let (na, nb) = (self.village_name(a), self.village_name(b));
        self.event(Some(a), vec![], format!("Une route relie désormais {na} et {nb}"));
    }

    pub fn village_name(&self, id: u32) -> String {
        self.village_index(id)
            .map(|vi| self.villages[vi].name.clone())
            .unwrap_or_default()
    }

    // ------------------------------------------------------------------ marchands

    /// Ce que le métier de marchand pourrait rapporter par jour dans ce village (meilleur
    /// voyage connu, un aller-retour prenant environ deux jours).
    pub(crate) fn trade_potential(&self, vi: usize) -> f32 {
        let id = self.villages[vi].id;
        if self.economy.markets.get(vi).is_none() {
            return 0.0;
        }
        let known = self.trade.news.get(&id);
        let mut best: f32 = 0.0;
        for j in self.neighbours(vi) {
            let other = self.villages[j].id;
            if self.trade.route(id, other).is_some_and(|r| !r.reachable()) {
                continue;
            }
            let Some(n) = known.and_then(|k| k.get(&other)) else {
                // Un village dont on ne sait rien : il y a peut-être quelque chose à y faire.
                best = best.max(2.5);
                continue;
            };
            best = best.max(self.plan_trip(vi, j, n, 50.0).0 / 2.0);
        }
        best
    }

    /// Le meilleur aller-retour vers un village dont on a ces nouvelles, avec ce capital :
    /// bénéfice espéré, et ce qu'il faut charger au départ. On compte aussi ce qu'on
    /// rapportera (acheté là-bas, revendu chez soi). Les vieilles nouvelles comptent moins,
    /// et l'on n'emporte pas plus que ce que l'autre marché peut prendre.
    fn plan_trip(
        &self,
        vi: usize,
        dest: usize,
        news: &PriceNews,
        capital: f32,
    ) -> (f32, Option<(Good, f32)>) {
        let home = &self.economy.markets[vi];
        // Ce que l'autre village pourrait prendre si le prix baissait : un peu par habitant.
        let room = self.population(self.villages[dest].id) as f32 * 0.5;
        let age = (self.tick.saturating_sub(news.tick)) as f32 / DAY as f32;
        let trust = 1.0 / (1.0 + age / 5.0);
        let there = |g: usize| trust * news.price[g] + (1.0 - trust) * home.price[g];
        // Aller : acheter ici, vendre là-bas.
        let mut out: (f32, Cargo) = (0.0, None);
        for g in Good::ALL {
            let k = g as usize;
            let cost = home.price[k] * 1.05;
            let margin = there(k) * (1.0 - TAX) - cost;
            if margin < 0.15 * cost || margin < 0.05 {
                continue;
            }
            let q = CART
                .min(capital / cost)
                .min(home.supply[k] * 0.6)
                .min(news.demand[k] * trust + room);
            if q >= 1.0 && q * margin > out.0 {
                out = (q * margin, Some((g, q)));
            }
        }
        // Retour : acheter là-bas, revendre ici.
        let mut back: f32 = 0.0;
        for k in 0..GOODS {
            let cost = there(k) * 1.05;
            let margin = home.price[k] * (1.0 - TAX) - cost;
            if margin < 0.15 * cost || margin < 0.05 {
                continue;
            }
            let q = CART
                .min((capital + out.0) / cost)
                .min(news.supply[k] * trust * 0.6)
                .min(home.demand[k] + 2.0);
            back = back.max(q * margin);
        }
        (out.0 + back, out.1)
    }

    /// Le matin, un marchand sans voyage prévu choisit où aller et quoi emporter (vrai), ou
    /// renonce pour aujourd'hui (faux : il travaillera aux champs).
    pub(super) fn merchant_work(&mut self, i: usize, vi: usize) -> bool {
        if self.clock().hour >= 11 {
            return false;
        }
        let id = self.people[i].id;
        let home_id = self.villages[vi].id;
        if self.society.sick.contains_key(&id) {
            return false;
        }
        let Some((coins, debt)) = self.home_household(i).map(|h| (h.coins, h.debt)) else {
            return false;
        };
        let own = (coins * 0.7).min(80.0);
        // Peu d'argent : la caisse du village avance de quoi charger la charrette
        // (remboursé sur les ventes du foyer, comme pour les artisans).
        self.economy.ensure_village(vi);
        let treasury = self.economy.treasury[vi];
        let loan = if own < 10.0 {
            (20.0 - own).min(treasury * 0.3).min(40.0 - debt).max(0.0)
        } else {
            0.0
        };
        let capital = own + loan;
        let price = self.economy.markets[vi].price;
        let mut best: Option<(f32, usize, Cargo)> = None;
        let mut unknown: Option<usize> = None;
        for j in self.neighbours(vi) {
            if !self.ensure_route(vi, j) {
                continue;
            }
            let other = self.villages[j].id;
            let news = self.trade.news.get(&home_id).and_then(|k| k.get(&other)).cloned();
            match news {
                None => {
                    unknown.get_or_insert(j);
                }
                Some(n) => {
                    // Des nouvelles de plus de deux semaines : autant aller voir.
                    if self.tick.saturating_sub(n.tick) > 14 * DAY {
                        unknown.get_or_insert(j);
                    }
                    let (mut profit, want) = self.plan_trip(vi, j, &n, capital);
                    // Une foire s'y ouvre : on y vendra bien, on y apprendra beaucoup.
                    if self.fair_soon(other) {
                        profit = profit.max(MIN_PROFIT) + 12.0;
                    }
                    if profit >= MIN_PROFIT && best.as_ref().is_none_or(|b| profit > b.0) {
                        best = Some((profit, j, want));
                    }
                }
            }
        }
        // Sans argent, on peut encore aller voir un village inconnu (et en rapporter des nouvelles).
        let (dest, want) = match (best, unknown) {
            (Some((_, j, want)), _) if capital >= 3.0 => (j, want),
            (_, Some(j)) => (j, None),
            _ => return false,
        };
        let loan = if want.is_some() { loan } else { 0.0 };
        let capital = own.min(if want.is_some() { own } else { own.min(20.0) }) + loan;
        let dest_id = self.villages[dest].id;
        // Seul un premier voyage vers ce village fait parler de lui.
        if want.is_none() && self.trade.route(home_id, dest_id).is_some_and(|r| r.trips == 0) {
            let p = &self.people[i];
            let text = format!(
                "{} part voir ce qui se vend à {}",
                p.first, self.villages[dest].name
            );
            self.event(Some(home_id), vec![id], text);
        }
        if let Some(h) = self.people[i].home {
            let hh = self.economy.household(h);
            hh.coins -= capital - loan;
            hh.debt += loan;
            self.economy.treasury[vi] -= loan;
        }
        let t = self.trade.traders.entry(id).or_default();
        *t = Trader {
            leg: Leg::Buying,
            dest: dest_id,
            cargo: NO_GOODS,
            purse: capital,
            capital,
            want,
            home_price: price,
            home_demand: self.economy.markets[vi].demand,
            arrived: false,
            dealt: 0,
            trips: t.trips,
            waited: 0,
            profit: t.profit,
        };
        self.wait_at_market(i, vi);
        true
    }

    fn wait_at_market(&mut self, i: usize, vi: usize) {
        let (x, y) = self.villages[vi].door();
        let spot = self.spot_near(x, y, 2);
        self.people[i].brain.intent = Intent::Work;
        self.people[i].brain.plan.extend([
            Step::Go(spot),
            Step::Wait {
                ticks: 30 * MINUTE,
                act: Act::Trading,
                with: None,
            },
        ]);
    }

    /// Marchand en voyage : son plan suit l'étape où il en est, de jour comme de nuit.
    /// Faux s'il n'a rien de prévu (il vit alors comme tout le monde).
    pub(super) fn trader_assign(&mut self, i: usize) -> bool {
        let id = self.people[i].id;
        let Some(t) = self.trade.traders.get(&id).cloned() else {
            return false;
        };
        let night = day_part(self.clock().hour) == DayPart::Night;
        let home = self.people[i].village;
        let (Some(hv), Some(dv)) = (self.village_index(home), self.village_index(t.dest)) else {
            self.trade.traders.remove(&id);
            return false;
        };
        match t.leg {
            Leg::Idle => false,
            Leg::Buying if t.dealt == 0 => {
                if night {
                    return false;
                }
                self.wait_at_market(i, hv);
                true
            }
            Leg::Buying => {
                // Même sans rien à vendre là-bas, on part : on rapportera de quoi vendre ici.
                let tr = self.trade.traders.get_mut(&id).unwrap();
                tr.leg = Leg::Outbound;
                tr.dealt = 0;
                let (x, y) = self.villages[hv].door();
                let door = self.map.idx(x, y) as u32;
                let p = &mut self.people[i];
                p.brain.intent = Intent::Work;
                p.brain.plan.extend([
                    Step::Go(door),
                    Step::Journey {
                        to: t.dest,
                        back: false,
                    },
                    Step::Arrive,
                ]);
                true
            }
            Leg::Outbound | Leg::Returning => {
                let back = t.leg == Leg::Returning;
                let p = &mut self.people[i];
                p.brain.intent = Intent::Work;
                p.brain
                    .plan
                    .extend([Step::Journey { to: t.dest, back }, Step::Arrive]);
                true
            }
            Leg::Selling if t.dealt == 0 || night => {
                // À l'auberge la nuit, au marché le jour.
                let (x, y) = self.villages[dv].door();
                let spot = self.spot_near(x, y, 2);
                let p = &mut self.people[i];
                p.brain.intent = Intent::Work;
                if night {
                    p.brain.plan.extend([Step::Go(spot), Step::HideHere]);
                } else {
                    p.brain.plan.extend([
                        Step::Go(spot),
                        Step::Wait {
                            ticks: 30 * MINUTE,
                            act: Act::Trading,
                            with: None,
                        },
                    ]);
                }
                true
            }
            Leg::Selling => {
                let tr = self.trade.traders.get_mut(&id).unwrap();
                // Presque rien vendu : on attend un marché de plus (deux au plus), le prix
                // baissera avec ce qu'on propose.
                let unsold = Good::ALL.iter().any(|&g| {
                    t.want
                        .is_some_and(|(w, q)| w == g && tr.cargo[g as usize] > 0.6 * q)
                });
                if unsold && tr.waited < 2 {
                    tr.waited += 1;
                    tr.dealt = 0;
                    return self.trader_assign(i);
                }
                tr.leg = Leg::Returning;
                tr.dealt = 0;
                let p = &mut self.people[i];
                p.brain.intent = Intent::Work;
                p.brain.plan.extend([
                    Step::Journey {
                        to: t.dest,
                        back: true,
                    },
                    Step::Arrive,
                ]);
                true
            }
        }
    }

    /// Met le marchand sur la route (depuis le point du trajet le plus proche).
    pub(super) fn journey(&mut self, i: usize, dest: u32, back: bool) {
        let home = self.people[i].village;
        let (from, to) = if back { (dest, home) } else { (home, dest) };
        let Some(path) = self.trade.path(from, to) else {
            // Plus de trajet (cela ne devrait pas arriver) : le voyage s'arrête là.
            self.end_trip(i, true);
            return;
        };
        let here = self.people[i].tile(self.map.width);
        let (hx, hy) = self.map.xy(here as usize);
        let start = path
            .iter()
            .enumerate()
            .min_by_key(|&(_, &t)| {
                let (x, y) = self.map.xy(t as usize);
                (x as i64 - hx as i64).abs() + (y as i64 - hy as i64).abs()
            })
            .map_or(0, |(k, _)| k);
        let b = &mut self.people[i].brain;
        b.path = path[start + 1..].to_vec();
        b.path_pos = 0;
        b.traveling = true;
        b.act = Act::Travelling;
        b.carry = Some(Carry::Cart);
        b.hidden = false;
    }

    /// Arrivée au bout d'un trajet : on vend à destination, ou l'on rentre chez soi.
    pub(super) fn trade_arrive(&mut self, i: usize) {
        let id = self.people[i].id;
        let home = self.people[i].village;
        self.people[i].brain.traveling = false;
        self.people[i].brain.carry = None;
        let Some(t) = self.trade.traders.get(&id).cloned() else {
            return;
        };
        self.travelled(home, t.dest);
        match t.leg {
            Leg::Outbound => {
                // Il raconte les prix de chez lui et tout ce qu'il sait.
                let news = PriceNews {
                    tick: self.tick,
                    price: t.home_price,
                    supply: NO_GOODS,
                    demand: t.home_demand,
                };
                self.trade.hear(t.dest, home, &news);
                self.trade.share(home, t.dest);
                // … et parfois un savoir-faire de chez lui.
                self.teach(home, t.dest, 0.04, Some(id));
                let tr = self.trade.traders.get_mut(&id).unwrap();
                tr.leg = Leg::Selling;
                tr.arrived = true;
                tr.dealt = 0;
            }
            Leg::Returning => {
                if let Some(dv) = self.village_index(t.dest) {
                    let news = PriceNews::of(self.tick, &self.economy.markets[dv]);
                    self.trade.hear(home, t.dest, &news);
                    self.trade.share(t.dest, home);
                }
                // Il rapporte ce qu'il a vu faire là-bas.
                self.teach(t.dest, home, 0.07, Some(id));
                self.end_trip(i, true);
            }
            _ => {}
        }
    }

    /// Fin d'un voyage (ou renoncement) : l'argent et la cargaison vont au foyer.
    fn end_trip(&mut self, i: usize, travelled: bool) {
        let id = self.people[i].id;
        let Some(t) = self.trade.traders.get(&id).cloned() else {
            return;
        };
        let Some(vi) = self.village_index(self.people[i].village) else {
            return;
        };
        let price = self.economy.markets[vi].price;
        let worth: f32 = (0..GOODS).map(|g| t.cargo[g] * price[g] * (1.0 - TAX)).sum();
        let profit = t.purse + worth - t.capital;
        if let Some(h) = self.people[i].home {
            let hh = self.economy.household(h);
            hh.coins += t.purse;
            for g in 0..GOODS {
                hh.inv[g] += t.cargo[g];
            }
        }
        let tr = self.trade.traders.get_mut(&id).unwrap();
        tr.leg = Leg::Idle;
        tr.cargo = NO_GOODS;
        tr.purse = 0.0;
        tr.capital = 0.0;
        tr.want = None;
        tr.arrived = false;
        tr.dealt = 0;
        if !travelled {
            return;
        }
        tr.trips += 1;
        tr.profit += profit;
        self.economy.ensure_village(vi);
        self.economy.today[vi][Job::Merchant as usize] += profit;
        let m = &mut self.people[i].mind;
        if profit > 10.0 {
            m.mood.add(Emotion::Joy, 0.3);
            m.needs.esteem = (m.needs.esteem + 0.1).min(1.0);
        } else if profit < -5.0 {
            m.mood.add(Emotion::Sadness, 0.3);
        }
        if profit.abs() >= 25.0 {
            let p = &self.people[i];
            let place = self.village_name(t.dest);
            let text = if profit > 0.0 {
                format!(
                    "{} revient de {place} avec {:.0} sous de bénéfice",
                    p.first, profit
                )
            } else {
                format!(
                    "{} revient de {place} en ayant perdu {:.0} sous",
                    p.first, -profit
                )
            };
            let village = p.village;
            self.event(Some(village), vec![id], text);
        }
    }

    /// Marchands de passage qui vendent à ce marché (identifiant, cargaison).
    pub(super) fn market_sellers(&self, vi: usize) -> Vec<u32> {
        let id = self.villages[vi].id;
        self.trade
            .traders
            .iter()
            .filter(|(p, t)| {
                t.leg == Leg::Selling && t.arrived && t.dest == id && self.people[**p as usize].alive()
            })
            // Venus pour la foire : ils attendent qu'elle ouvre.
            .filter(|_| !self.fair_ahead(id))
            .map(|(&p, _)| p)
            .collect()
    }

    /// Marchands qui achètent à ce marché : ceux du village qui chargent pour partir, et
    /// ceux de passage qui rachètent pour le retour.
    pub(super) fn market_buyers(&self, vi: usize) -> Vec<u32> {
        let id = self.villages[vi].id;
        self.trade
            .traders
            .iter()
            .filter(|(p, t)| {
                let person = &self.people[**p as usize];
                person.alive()
                    && ((t.leg == Leg::Buying && person.village == id && t.dealt == 0)
                        || (t.leg == Leg::Selling
                            && t.arrived
                            && t.dest == id
                            && t.dealt == 0
                            && !self.fair_ahead(id)))
            })
            .map(|(&p, _)| p)
            .collect()
    }

    /// Ce que le marchand `p` achète, compte tenu de ce qui reste (`left`) et des prix.
    /// Renvoie les quantités par bien.
    pub(super) fn trader_purchase(
        &mut self,
        p: u32,
        price: &[f32; GOODS],
        left: &[f32; GOODS],
    ) -> [f32; GOODS] {
        let mut bought = [0.0f32; GOODS];
        let Some(t) = self.trade.traders.get_mut(&p) else {
            return bought;
        };
        let mut buy = |g: usize, q: f32, t: &mut Trader| {
            let u = q
                .min(left[g] - bought[g])
                .min(t.purse / price[g].max(0.01))
                .max(0.0);
            t.purse -= u * price[g];
            t.cargo[g] += u;
            bought[g] += u;
        };
        if t.leg == Leg::Buying {
            if let Some((g, q)) = t.want {
                buy(g as usize, q, t);
            }
        } else {
            // Pour le retour : ce qui se vend le mieux chez soi par rapport à ici.
            let mut goods: Vec<(f32, usize)> = (0..GOODS)
                .map(|g| (t.home_price[g] * (1.0 - TAX) - price[g] * 1.05, g))
                .filter(|&(m, g)| m > 0.15 * price[g] && m > 0.05)
                .collect();
            goods.sort_by(|a, b| b.0.total_cmp(&a.0));
            for (_, g) in goods {
                let room = CART - load(&t.cargo);
                if room < 1.0 {
                    break;
                }
                // Pas plus que ce que son village achète d'habitude.
                buy(g, room.min(t.home_demand[g] + 2.0), t);
            }
        }
        t.dealt = self.tick;
        bought
    }

    /// Le marchand vend : `sold` unités pour `money` sous (après taxe).
    pub(super) fn trader_sold(&mut self, p: u32, g: usize, sold: f32, money: f32) {
        self.trade.sold[g] += sold;
        if let Some(t) = self.trade.traders.get_mut(&p) {
            t.cargo[g] = (t.cargo[g] - sold).max(0.0);
            t.purse += money;
            t.dealt = self.tick;
        }
    }

    /// Est-il en voyage, loin de son village ?
    pub fn away(&self, i: usize) -> bool {
        self.trade
            .traders
            .get(&self.people[i].id)
            .is_some_and(|t| t.leg.away())
    }

    // ------------------------------------------------------------------ une fois par jour

    /// Marchands disparus ou reconvertis, essaimage des villages trop peuplés.
    pub(super) fn daily_trade(&mut self) {
        let gone: Vec<u32> = self
            .trade
            .traders
            .iter()
            .filter(|(p, t)| {
                let q = &self.people[**p as usize];
                !q.alive() || (q.job != Some(Job::Merchant) && t.leg == Leg::Idle)
            })
            .map(|(&p, _)| p)
            .collect();
        for p in gone {
            self.trade.traders.remove(&p);
        }
        for vi in 0..self.villages.len() {
            self.maybe_colony(vi);
        }
    }

    // ------------------------------------------------------------------ déménagements

    /// Un habitant part vivre dans un autre village, dans la maison `house`.
    pub(crate) fn relocate(&mut self, i: usize, to: usize, house: u32) {
        let tick = self.tick;
        let id = self.people[i].id;
        if let Some(t) = self.trade.traders.remove(&id)
            && let Some(h) = self.people[i].home
        {
            self.economy.household(h).coins += t.purse;
        }
        self.miss(i);
        if let Some(from) = self.village_index(self.people[i].village)
            && let Some(old) = self.people[i].home
            && let Some(h) = self.villages[from].house_mut(old)
        {
            h.residents.retain(|&r| r != id);
        }
        let to_id = self.villages[to].id;
        // Un adulte qui s'installe ailleurs y apporte ce qu'on savait faire chez lui.
        let from_id = self.people[i].village;
        if !self.people[i].child(tick) {
            self.teach(from_id, to_id, 0.25, Some(id));
        }
        let (x, y) = self.far_tile(to, 20);
        let target = {
            let h = self.villages[to].house_mut(house).unwrap();
            h.residents.push(id);
            h.y * self.map.width + h.x
        };
        let p = &mut self.people[i];
        p.village = to_id;
        p.home = Some(house);
        p.master = None;
        p.apprentice_until = 0;
        p.x = x as f32 + 0.5;
        p.y = y as f32 + 0.5;
        p.brain = Default::default();
        p.brain.act = Act::Arriving;
        p.brain.plan.push_back(Step::Go(target));
        // Son métier n'existe peut-être pas là-bas (pas d'atelier, pas de mer…).
        if !p.child(tick)
            && let Some(job) = p.job
            && !self.feasible_jobs(to, Some(id)).contains(&job)
        {
            let new = self.pick_job(id);
            self.people[i].job = Some(new);
        }
        self.feel(i, MemoryKind::NewHome, None, 0.5, Emotion::Joy, 0.2);
    }

    /// Mariage avec quelqu'un d'un autre village que les marchands relient au sien.
    /// Renvoie le partenaire trouvé.
    pub(super) fn distant_match(&mut self, i: usize) -> Option<u32> {
        let tick = self.tick;
        let p = &self.people[i];
        let known: Vec<u32> = self
            .trade
            .news
            .get(&p.village)
            .map(|k| k.keys().copied().collect())
            .unwrap_or_default();
        let candidates: Vec<u32> = (0..self.people.len())
            .filter(|&j| {
                let q = &self.people[j];
                j != i
                    && known.contains(&q.village)
                    && self.single(j)
                    && q.female != p.female
                    && (q.age(tick) - p.age(tick)).abs() <= 10.0
                    && !self.related(p.id, q.id)
                    && !self.away(j)
            })
            .map(|j| j as u32)
            .collect();
        self.rng.pick(&candidates).copied()
    }

    /// Le moins attaché des deux rejoint l'autre dans son village. Faux si aucun foyer n'a
    /// de place (l'union attendra).
    pub(super) fn join_partner_village(&mut self, a: u32, b: u32) -> bool {
        let (pa, pb) = (&self.people[a as usize], &self.people[b as usize]);
        let (mover, stay) = if pa.mind.traits.attachment <= pb.mind.traits.attachment {
            (a, b)
        } else {
            (b, a)
        };
        let s = &self.people[stay as usize];
        let Some(to) = self.village_index(s.village) else {
            return false;
        };
        let Some(house) = s.home.filter(|&h| {
            self.villages[to]
                .house(h)
                .is_some_and(|h| h.residents.len() < HOUSE_CAPACITY && h.state == HouseState::Inhabited)
        }) else {
            return false;
        };
        let from = self.village_name(self.people[mover as usize].village);
        self.relocate(mover as usize, to, house);
        let (m, s) = (&self.people[mover as usize], &self.people[stay as usize]);
        let text = format!(
            "{} {} épouse {} et s'installe à {}",
            m.first,
            de(&from),
            s.first,
            self.villages[to].name
        );
        let village = s.village;
        self.event(Some(village), vec![mover, stay], text);
        true
    }

    /// Disette : plutôt que de partir au hasard, la famille va vers un village voisin qui
    /// mange à sa faim (si elle en connaît un). Vrai si elle a trouvé où aller.
    pub(super) fn migrate_household(&mut self, vi: usize, hid: u32) -> bool {
        let id = self.villages[vi].id;
        let known: Vec<u32> = self
            .trade
            .news
            .get(&id)
            .map(|k| k.keys().copied().collect())
            .unwrap_or_default();
        let mut best: Option<(f32, usize)> = None;
        for j in self.neighbours(vi) {
            let v = &self.villages[j];
            // On ne va que là où l'on sait aller : un village dont les marchands parlent,
            // ou le plus proche.
            if !known.contains(&v.id) && best.is_some() {
                continue;
            }
            let pop = self.population(v.id).max(1) as f32;
            let food: f32 = v
                .houses
                .iter()
                .filter_map(|h| self.economy.households.get(&h.id))
                .map(|h| h.rations())
                .sum::<f32>()
                + v.stock.food;
            let days = food / pop;
            if v.hungry_days > 0 || days < 12.0 || v.houses.len() >= MAX_HOUSES {
                continue;
            }
            let score = days - self.village_distance(vi, j) / 20.0;
            if best.is_none_or(|b| score > b.0) {
                best = Some((score, j));
            }
        }
        let Some((_, to)) = best else { return false };
        let Some(house) = self.new_house(to, HouseState::Inhabited) else {
            return false;
        };
        let Some(h) = self.villages[vi].house(hid) else {
            return false;
        };
        let (family, residents) = (h.family.clone(), h.residents.clone());
        self.villages[to].house_mut(house).unwrap().family = family.clone();
        // Ils emportent leur argent et ce qui leur reste.
        let old = self.economy.households.remove(&hid).unwrap_or_default();
        let new = self.economy.household(house);
        new.coins = old.coins;
        new.inv = old.inv;
        for &r in &residents {
            self.relocate(r as usize, to, house);
        }
        let (from, dest) = (self.villages[vi].name.clone(), self.villages[to].name.clone());
        self.event(
            Some(id),
            residents.clone(),
            format!("Chassée par la faim, la famille {family} quitte {from} pour {dest}"),
        );
        let to_id = self.villages[to].id;
        self.event(
            Some(to_id),
            residents,
            format!("La famille {family} arrive {} pour s'installer", de(&from)),
        );
        true
    }

    // ------------------------------------------------------------------ nouveaux villages

    /// Un village trop peuplé essaime : quelques jeunes couples partent fonder un village.
    fn maybe_colony(&mut self, vi: usize) {
        let tick = self.tick;
        let id = self.villages[vi].id;
        if self.villages.len() >= MAX_VILLAGES
            || self.population(id) < COLONY_POP
            || self
                .trade
                .last_colony
                .get(&id)
                .is_some_and(|&t| tick < t + 3 * YEAR)
            || !self.rng.chance(1.0 / 14.0)
        {
            return;
        }
        self.trade.last_colony.insert(id, tick);
        // Les couples les plus aventureux (et les moins attachés au village).
        let mut couples: Vec<(f32, u32, u32)> = Vec::new();
        for p in self.alive().filter(|p| p.village == id && !p.female) {
            let Some(q) = p.partner else { continue };
            let q = &self.people[q as usize];
            let (ap, aq) = (p.age(tick), q.age(tick));
            if !q.alive() || q.home != p.home || !(18.0..=40.0).contains(&ap) || !(18.0..=40.0).contains(&aq)
            {
                continue;
            }
            if self.away(p.id as usize) || self.away(q.id as usize) || p.brain.leaving {
                continue;
            }
            let t = |x: &crate::people::Person| {
                x.mind.traits.risk + (1.0 - x.mind.traits.attachment) + 0.5 * x.mind.traits.ambition
            };
            couples.push((t(p) + t(q), p.id, q.id));
        }
        for c in &mut couples {
            c.0 += self.rng.f32();
        }
        couples.sort_by(|a, b| b.0.total_cmp(&a.0));
        couples.truncate(3);
        if couples.len() < 2 {
            return;
        }
        let Some(site) = self.colony_site(vi) else {
            return;
        };
        let Some(new) = self.start_village(&site) else {
            return;
        };
        let new_id = self.villages[new].id;
        self.trade.founded_from.insert(new_id, id);
        self.inherit_techs(id, new_id);
        // Le chemin des fondateurs devient tout de suite une route.
        if self.ensure_route(vi, new)
            && let Some(r) = self.trade.routes.get_mut(&pair(id, new_id))
        {
            r.trips = ROAD_TRIPS - 1;
            self.travelled(id, new_id);
        }
        // Chacun connaît les prix de l'autre.
        let news = PriceNews::of(tick, &self.economy.markets[vi]);
        self.trade.hear(new_id, id, &news);
        let mut founders = Vec::new();
        for (_, a, b) in couples {
            let Some(old) = self.people[a as usize].home else {
                continue;
            };
            let Some(house) = self.new_house(new, HouseState::Inhabited) else {
                continue;
            };
            let last = self.people[a as usize].last.clone();
            self.villages[new].house_mut(house).unwrap().family = last;
            // Le couple, leurs enfants qui vivent avec eux, et une part des réserves.
            let kids: Vec<u32> = self.villages[vi]
                .house(old)
                .map(|h| {
                    h.residents
                        .iter()
                        .copied()
                        .filter(|&r| {
                            let c = &self.people[r as usize];
                            c.child(tick) && (c.parents.contains(&a) || c.parents.contains(&b))
                        })
                        .collect()
                })
                .unwrap_or_default();
            let leaving_all = self.villages[vi]
                .house(old)
                .is_some_and(|h| h.residents.len() == 2 + kids.len());
            let share = if leaving_all { 1.0 } else { 0.5 };
            let (coins, inv) = {
                let h = self.economy.household(old);
                let c = h.coins * share;
                h.coins -= c;
                let inv: [f32; GOODS] = std::array::from_fn(|g| h.inv[g] * share);
                for (stock, taken) in h.inv.iter_mut().zip(inv) {
                    *stock -= taken;
                }
                (c, inv)
            };
            let hh = self.economy.household(house);
            hh.coins = coins;
            hh.inv = inv;
            hh.inv[Good::Grain as usize] += 40.0;
            for who in [a, b].into_iter().chain(kids) {
                self.relocate(who as usize, new, house);
                founders.push(who);
            }
        }
        self.first_fields(new);
        let (from, name) = (self.villages[vi].name.clone(), site.name.clone());
        let n = founders.len();
        self.event(
            Some(id),
            founders.clone(),
            format!("Des familles {} partent fonder un village", de(&from)),
        );
        self.event(
            Some(new_id),
            founders,
            format!(
                "Fondation de {name} par des familles {} ({n} habitants)",
                de(&from)
            ),
        );
    }

    /// Un bon endroit pour un nouveau village : terres fertiles, eau douce, ni trop près ni
    /// trop loin du village d'origine, loin des autres.
    fn colony_site(&mut self, vi: usize) -> Option<Site> {
        let (cx, cy) = (self.villages[vi].x as f32, self.villages[vi].y as f32);
        let m = self.map.clone();
        let mut best: Option<(f32, u32, u32)> = None;
        for _ in 0..400 {
            let a = self.rng.f32() * std::f32::consts::TAU;
            let d = 60.0 + self.rng.f32() * 100.0;
            let (x, y) = ((cx + a.cos() * d) as i64, (cy + a.sin() * d) as i64);
            if !m.inside(x - 8, y - 8) || !m.inside(x + 8, y + 8) {
                continue;
            }
            let i = m.idx(x as u32, y as u32);
            if !matches!(
                m.biome_at(i),
                Biome::Meadow | Biome::Grassland | Biome::Forest | Biome::Heath | Biome::Hills
            ) || m.river_at(i) > 0
                || self.occ(i) != occ::NONE
            {
                continue;
            }
            let far = self
                .villages
                .iter()
                .all(|v| ((v.x as f32 - x as f32).powi(2) + (v.y as f32 - y as f32).powi(2)).sqrt() >= 55.0);
            if !far {
                continue;
            }
            let (mut fert, mut water, mut n) = (0.0, 0.0f32, 0.0);
            for dy in (-6..=6).step_by(2) {
                for dx in (-6..=6).step_by(2) {
                    let j = m.idx((x + dx) as u32, (y + dy) as u32);
                    fert += m.fertility[j] as f32 / 255.0;
                    n += 1.0;
                    if m.river_at(j) > 0 || m.biome_at(j) == Biome::Lake {
                        water = 0.3;
                    }
                }
            }
            let score = fert / n + water - d / 600.0 + self.rng.f32() * 0.05;
            if best.is_none_or(|b| score > b.0) && self.civic_spot(x as u32, y as u32).is_some() {
                best = Some((score, x as u32, y as u32));
            }
        }
        let (_, x, y) = best?;
        let name = self.colony_name(x, y);
        let color = names::VILLAGE_COLORS[self.villages.len() % names::VILLAGE_COLORS.len()];
        // Couleur un peu décalée pour ne pas confondre avec le village de même couleur.
        let color = if self.villages.len() >= names::VILLAGE_COLORS.len() {
            let shift = |c: u32, s: u32| ((((c >> s) & 0xff) as f32 * 0.75 + 40.0) as u32).min(255) << s;
            shift(color, 16) | shift(color, 8) | shift(color, 0)
        } else {
            color
        };
        Some(Site { name, x, y, color })
    }

    /// Un nom libre (de la liste des noms de villages), avec « -sur-Mer » ou « -les-Bois »
    /// selon les environs.
    fn colony_name(&mut self, x: u32, y: u32) -> String {
        let used: Vec<&str> = self.villages.iter().map(|v| v.name.as_str()).collect();
        let free: Vec<&str> = names::VILLAGES
            .iter()
            .copied()
            .filter(|b| !used.iter().any(|u| u.starts_with(b)))
            .collect();
        let base = match self.rng.pick(&free) {
            Some(b) => b.to_string(),
            None => format!("Neuville {}", self.villages.len() + 1),
        };
        if base.starts_with("Les ") || base.starts_with("La ") || base.starts_with("L'") {
            return base;
        }
        let m = &self.map;
        let (mut sea, mut forest, mut total) = (false, 0, 0);
        for dy in -8i64..=8 {
            for dx in -8i64..=8 {
                let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                if !m.inside(nx, ny) {
                    continue;
                }
                total += 1;
                match m.biome_at(m.idx(nx as u32, ny as u32)) {
                    Biome::Sea => sea = true,
                    Biome::Forest | Biome::Conifers => forest += 1,
                    _ => {}
                }
            }
        }
        if sea {
            format!("{base}-sur-Mer")
        } else if forest * 10 > total * 4 {
            format!("{base}-les-Bois")
        } else {
            base
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        economy::Good,
        map::{Biome, Map},
        people::Job,
        settlement::HouseState,
        trade::{Leg, PriceNews},
        tuning::DAY,
        world::{Site, World},
    };

    /// Une grande prairie avec deux villages à 120 cases l'un de l'autre.
    fn two_villages() -> World {
        let mut map = Map::new(300, 200);
        for i in 0..map.len() {
            map.biome[i] = Biome::Meadow as u8;
            map.elevation[i] = 100;
            map.fertility[i] = 200;
        }
        let site = |name: &str, x| Site {
            name: name.into(),
            x,
            y: 100,
            color: 0xff8800,
        };
        World::from_map(7, map, &[site("Amont", 60), site("Aval", 180)])
    }

    /// Un marchand d'Amont sait que le poisson est cher à Aval : il charge, voyage, vend,
    /// et chacun apprend les prix de l'autre. À force de passages, une route se trace.
    #[test]
    fn a_merchant_makes_a_profitable_trip() {
        let mut w = two_villages();
        w.economy.ensure_village(0);
        w.economy.ensure_village(1);
        let tick = w.tick;
        let m = w.alive().find(|p| p.village == 0 && !p.child(tick)).unwrap().id;
        w.people[m as usize].job = Some(Job::Merchant);
        let home = w.people[m as usize].home.unwrap();
        w.economy.household(home).coins = 100.0;
        let fish = Good::Fish as usize;
        // Beaucoup de poisson à vendre à Amont (le foyer d'un voisin), cher et demandé à Aval.
        let other = w.villages[0]
            .houses
            .iter()
            .map(|h| h.id)
            .find(|&h| h != home)
            .unwrap();
        // (Un foyer vend d'abord ce qu'il aime le moins : sans grain, il vend son poisson.)
        w.economy.household(other).inv = [0.0; crate::economy::GOODS];
        w.economy.household(other).inv[fish] = 200.0;
        w.economy.markets[0].price[fish] = 0.3;
        w.economy.markets[0].supply[fish] = 150.0;
        let mut dear = w.economy.markets[1].clone();
        dear.price[fish] = 1.8;
        dear.demand[fish] = 40.0;
        w.economy.markets[1] = dear.clone();
        w.trade.hear(0, 1, &PriceNews::of(tick, &dear));
        let mut left = false;
        for _ in 0..6 * DAY {
            w.step();
            if w.trade.traders.get(&m).is_some_and(|t| t.leg.away()) {
                left = true;
            }
            if left
                && w.trade
                    .traders
                    .get(&m)
                    .is_some_and(|t| t.leg == Leg::Idle && t.trips > 0)
            {
                break;
            }
        }
        let t = &w.trade.traders[&m];
        assert!(left, "le marchand n'est jamais parti");
        assert!(t.trips >= 1, "{t:?}");
        assert!(w.trade.sold[fish] > 5.0, "poisson vendu : {}", w.trade.sold[fish]);
        assert!(t.profit > 0.0, "bénéfice {}", t.profit);
        assert!(
            w.trade.news.get(&1).is_some_and(|k| k.contains_key(&0)),
            "Aval connaît Amont"
        );
        let r = w.trade.route(0, 1).unwrap();
        assert!(r.reachable() && r.trips >= 2);
    }

    /// Un village trop peuplé essaime : de jeunes couples partent fonder un village relié
    /// au leur par une route.
    #[test]
    fn a_crowded_village_founds_a_colony() {
        let mut w = two_villages();
        let tick = w.tick;
        for k in 0..30 {
            let Some(h) = w.new_house(0, HouseState::Inhabited) else {
                break;
            };
            let a = w.new_person(0, "Colon", true, 22.0 + k as f32 % 10.0, Some(h), &[]);
            let b = w.new_person(0, "Colon", false, 24.0 + k as f32 % 10.0, Some(h), &[]);
            w.couple(a, b);
            w.open_household(h, 30.0, 100.0);
        }
        assert!(
            w.population(0) >= super::COLONY_POP,
            "{} habitants",
            w.population(0)
        );
        for _ in 0..500 {
            w.trade.last_colony.clear();
            w.maybe_colony(0);
            if w.villages.len() > 2 {
                break;
            }
        }
        assert_eq!(w.villages.len(), 3, "pas de fondation");
        let new = &w.villages[2];
        let pop = w.population(new.id);
        assert!(pop >= 4, "{pop} fondateurs");
        assert_eq!(w.trade.founded_from.get(&new.id), Some(&0));
        assert!(w.trade.route(0, new.id).is_some_and(|r| r.road));
        assert!(
            w.alive()
                .filter(|p| p.village == new.id)
                .all(|p| p.home.is_some())
        );
        let _ = tick;
    }

    /// Une famille affamée part vers le village voisin qui mange à sa faim.
    #[test]
    fn a_hungry_family_moves_to_a_fed_village() {
        let mut w = two_villages();
        let hid = w.villages[0]
            .houses
            .iter()
            .find(|h| !h.residents.is_empty())
            .unwrap()
            .id;
        let residents = w.villages[0].house(hid).unwrap().residents.clone();
        for h in w.villages[1].houses.iter().map(|h| h.id).collect::<Vec<_>>() {
            w.economy.household(h).inv[Good::Grain as usize] = 500.0;
        }
        assert!(w.migrate_household(0, hid));
        for r in residents {
            let p = &w.people[r as usize];
            assert!(p.alive() && p.village == 1, "{} est resté", p.name());
            assert!(p.home.is_some_and(|h| w.villages[1].house(h).is_some()));
        }
    }
}

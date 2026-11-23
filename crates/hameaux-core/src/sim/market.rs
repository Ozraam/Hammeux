//! Économie au quotidien : dépôt de la production, ateliers, marché de 11 h, repas de midi,
//! revenus des métiers et changements de métier.

// Les stocks sont des tableaux indexés par bien : les boucles par index restent plus lisibles.
#![allow(clippy::needless_range_loop)]
use crate::{
    clock::Season,
    economy::{GOODS, Good, Household, Inv, JOBS, Workshop},
    map::{Biome, Map, Resource as MapResource},
    people::{Carry, Job},
    settlement::HouseState,
    tuning::*,
    world::World,
};

/// Jours de nourriture qu'un foyer veut avoir d'avance.
const FOOD_DAYS: f32 = 7.0;
/// Salaire d'un bâtisseur par voyage de matériaux, payé par la caisse du village.
const BUILDER_WAGE: f32 = 1.0;

/// La case `t` porte-t-elle la ressource qui donne ce bien ?
pub(crate) fn resource_at(m: &Map, t: usize, g: Good) -> bool {
    let b = m.biome_at(t);
    match g {
        Good::Wood => matches!(b, Biome::Forest | Biome::Conifers),
        Good::Stone => super::is_quarry(m, t),
        Good::Fish => matches!(b, Biome::Sea | Biome::Lake) || m.resource[t] == MapResource::Fish as u8,
        Good::Meat => {
            m.resource[t] == MapResource::Game as u8 || matches!(b, Biome::Forest | Biome::Conifers)
        }
        Good::Ore => m.resource[t] == MapResource::Iron as u8,
        _ => false,
    }
}

impl World {
    pub fn home_household(&self, i: usize) -> Option<&Household> {
        self.economy.households.get(&self.people[i].home?)
    }

    fn home_household_mut(&mut self, i: usize) -> Option<&mut Household> {
        let h = self.people[i].home?;
        Some(self.economy.household(h))
    }

    /// Rendement ×1,4 avec un outil, qui s'use un peu à chaque séance : un débutant l'abîme
    /// plus vite qu'un maître, qui sait l'entretenir (usure de ×1,2 à ×0,6).
    pub(super) fn tool_bonus(&mut self, i: usize) -> f32 {
        let skill = self.people[i].job.map_or(0.3, |j| self.people[i].mind.skill(j));
        let wear = 0.025 * (1.3 - 0.75 * skill);
        match self.home_household_mut(i) {
            Some(h) if h.inv[Good::Tools as usize] >= 0.05 => {
                h.inv[Good::Tools as usize] -= wear;
                1.4
            }
            _ => 1.0,
        }
    }

    /// Ce qu'on rapporte va aux réserves de son foyer (un dixième des récoltes au grenier
    /// commun). Les bâtisseurs, eux, alimentent le stock des chantiers.
    pub(super) fn deposit(&mut self, i: usize, vi: usize) {
        let b = &mut self.people[i].brain;
        let (carry, amount) = (b.carry.take(), b.carry_amount);
        b.carry_amount = 0.0;
        let Some(Carry::Good(g)) = carry else { return };
        let public = self.people[i].job == Some(Job::Builder) || self.people[i].home.is_none();
        let st = &mut self.villages[vi].stock;
        if public {
            match g {
                Good::Wood => st.wood = (st.wood + amount as u32).min(MAX_WOOD),
                Good::Stone => st.stone = (st.stone + amount as u32).min(MAX_STONE),
                _ => st.food = (st.food + amount * g.ration()).min(MAX_FOOD),
            }
            return;
        }
        let mut keep = amount;
        if g == Good::Grain {
            // La taxe du village (fixée par le chef) se prélève aussi en grain.
            let tax = self.society.tax(self.villages[vi].id);
            let st = &mut self.villages[vi].stock;
            st.food = (st.food + amount * tax * g.ration()).min(MAX_FOOD);
            keep = amount * (1.0 - tax);
        }
        if let Some(h) = self.home_household_mut(i) {
            h.inv[g as usize] += keep;
        }
    }

    pub(super) fn pay_builder(&mut self, i: usize, vi: usize) {
        self.economy.ensure_village(vi);
        let t = &mut self.economy.treasury[vi];
        let pay = BUILDER_WAGE.min(*t);
        *t -= pay;
        if let Some(h) = self.home_household_mut(i) {
            h.coins += pay;
        }
        self.economy.today[vi][Job::Builder as usize] += pay;
    }

    /// Une séance d'atelier : transforme les matières du foyer de l'artisan.
    pub(super) fn craft(&mut self, i: usize, wid: u32) {
        let Some(&kind) = self.economy.workshops.get(&wid) else {
            return;
        };
        let bonus = self.work_bonus(i);
        let sourdough = self.tech_bonus(i, crate::society::Tech::Sourdough, 1.15);
        let tempering = self.tech_bonus(i, crate::society::Tech::Tempering, 1.4);
        let Some(h) = self.home_household_mut(i) else {
            return;
        };
        let inv = &mut h.inv;
        let (grain, flour, bread, wood, ore, tools) = (
            Good::Grain as usize,
            Good::Flour as usize,
            Good::Bread as usize,
            Good::Wood as usize,
            Good::Ore as usize,
            Good::Tools as usize,
        );
        match kind {
            Workshop::Mill => {
                let n = (8.0 * bonus).min(inv[grain]);
                inv[grain] -= n;
                inv[flour] += n;
            }
            Workshop::Bakery => {
                let n = (6.0 * bonus).min(inv[flour]).min(inv[wood] * 6.0);
                inv[flour] -= n;
                inv[wood] -= n / 6.0;
                inv[bread] += n * 1.1 * sourdough;
            }
            Workshop::Smithy => {
                let k = (0.8 * bonus).min(inv[ore] / 2.0).min(inv[wood] / 2.0);
                inv[ore] -= 2.0 * k;
                inv[wood] -= 2.0 * k;
                inv[tools] += k * tempering;
            }
        }
    }

    // ------------------------------------------------------------------ foyers

    /// Les maisons d'habitation (pas les ateliers) d'un village, avec leurs habitants vivants.
    fn homes(&self, vi: usize) -> Vec<(u32, Vec<usize>)> {
        self.villages[vi]
            .houses
            .iter()
            .filter(|h| h.state == HouseState::Inhabited && !self.economy.workshops.contains_key(&h.id))
            .map(|h| {
                let members: Vec<usize> = h
                    .residents
                    .iter()
                    .map(|&r| r as usize)
                    .filter(|&r| self.people[r].alive() && !self.people[r].brain.leaving)
                    .collect();
                (h.id, members)
            })
            .filter(|(_, m)| !m.is_empty())
            .collect()
    }

    /// Rations consommées chaque jour par un groupe d'habitants.
    fn need(&self, members: &[usize]) -> f32 {
        let tick = self.tick;
        members
            .iter()
            .map(|&m| {
                if self.people[m].child(tick) {
                    RATION_CHILD
                } else {
                    RATION_ADULT
                }
            })
            .sum()
    }

    fn firewood(&self, members: &[usize]) -> f32 {
        let per = if self.clock().season == Season::Winter {
            0.35
        } else {
            0.1
        };
        members.len() as f32 * per
    }

    /// Ce qu'un foyer veut garder de chaque bien (réserves de vie et matières de travail).
    fn targets(&self, members: &[usize]) -> (Inv, f32) {
        let tick = self.tick;
        let mut t = [0.0; GOODS];
        let mut workers = 0.0;
        t[Good::Wood as usize] = self.firewood(members) * FOOD_DAYS;
        for &m in members {
            let p = &self.people[m];
            if p.child(tick) || p.elder(tick) {
                continue;
            }
            workers += 1.0;
            match p.job {
                Some(Job::Miller) => t[Good::Grain as usize] += 16.0,
                Some(Job::Baker) => {
                    t[Good::Flour as usize] += 12.0;
                    t[Good::Wood as usize] += 3.0;
                }
                Some(Job::Smith) => {
                    t[Good::Ore as usize] += 6.0;
                    t[Good::Wood as usize] += 6.0;
                }
                _ => {}
            }
        }
        t[Good::Tools as usize] = workers;
        (t, self.need(members) * FOOD_DAYS)
    }

    // ------------------------------------------------------------------ marché

    /// Marché de 11 h dans chaque village.
    pub(super) fn market_day(&mut self) {
        for vi in 0..self.villages.len() {
            self.economy.ensure_village(vi);
            self.market(vi);
        }
    }

    fn market(&mut self, vi: usize) {
        let homes = self.homes(vi);
        let n = homes.len();
        let price = self.economy.markets[vi].price;
        let village = self.villages[vi].id;
        let tax = self.society.tax(village);
        // Jour de foire : on achète plus volontiers, et les marchands venus d'ailleurs ne
        // paient pas la taxe.
        let fair = self.fair_open(village);
        // Offres de vente et besoins de chaque foyer.
        let mut sell = vec![[0.0f32; GOODS]; n];
        let mut buy = vec![[0.0f32; GOODS]; n];
        let mut food_need = vec![0.0f32; n];
        for (k, (hid, members)) in homes.iter().enumerate() {
            let (target, reserve) = self.targets(members);
            let h = self.economy.households.get(hid).cloned().unwrap_or_default();
            for g in Good::ALL {
                let gi = g as usize;
                if g.is_food() {
                    continue;
                }
                if g == Good::Tools {
                    // On rachète un outil quand il est presque usé ; le forgeron vend ce qu'il a en trop.
                    if h.inv[gi] < 0.3 * target[gi] {
                        buy[k][gi] = (target[gi] - h.inv[gi]).max(0.0);
                    } else {
                        sell[k][gi] = (h.inv[gi] - target[gi] - 0.5).max(0.0).floor();
                    }
                    continue;
                }
                let surplus = h.inv[gi] - target[gi];
                if surplus > 0.5 {
                    sell[k][gi] = surplus;
                } else if surplus < 0.0 {
                    buy[k][gi] = -surplus;
                }
            }
            // Matières de travail alimentaires (grain du meunier, farine du boulanger).
            for g in [Good::Grain, Good::Flour] {
                let gi = g as usize;
                if target[gi] > h.inv[gi] {
                    buy[k][gi] = target[gi] - h.inv[gi];
                }
            }
            // On aime varier : de la viande ou du poisson deux jours par semaine. On en prend
            // moins quand c'est cher (plus du tout à deux fois et demie le prix habituel).
            // C'est ce qui fait venir le poisson de la côte dans les villages de l'intérieur.
            let protein = h.inv[Good::Meat as usize] + h.inv[Good::Fish as usize];
            let want = members.len() as f32 * if fair { 3.0 } else { 2.0 } - protein;
            if want > 0.5 {
                for g in [Good::Meat, Good::Fish] {
                    let base = g.base_price();
                    let keen = ((2.5 * base - price[g as usize]) / base).clamp(0.0, 1.0);
                    buy[k][g as usize] += want / 2.0 * keen;
                }
            }
            // Nourriture : au-delà d'une semaine de réserve, on vend, en commençant par ce qu'on aime le moins.
            let rations: f32 = Good::FOODS
                .iter()
                .map(|&g| (h.inv[g as usize] - target[g as usize]).max(0.0) * g.ration())
                .sum();
            let mut surplus = rations - reserve;
            if surplus > 0.0 {
                for &g in Good::FOODS.iter().rev() {
                    let gi = g as usize;
                    let avail = (h.inv[gi] - target[gi]).max(0.0);
                    let units = avail.min(surplus / g.ration());
                    if units > 0.1 {
                        sell[k][gi] += units;
                        surplus -= units * g.ration();
                    }
                    if surplus <= 0.0 {
                        break;
                    }
                }
            } else {
                food_need[k] = -surplus;
            }
        }

        // Le village achète du bois et de la pierre pour ses chantiers.
        let sites = self.villages[vi]
            .houses
            .iter()
            .filter(|h| h.state == HouseState::Building)
            .count();
        let st = &self.villages[vi].stock;
        let mut public_buy = [0.0f32; GOODS];
        public_buy[Good::Wood as usize] = (if sites > 0 { 30.0 } else { 10.0 } - st.wood as f32).max(0.0);
        if self.villages[vi].has_stone {
            public_buy[Good::Stone as usize] =
                (if sites > 0 { 15.0 } else { 5.0 } - st.stone as f32).max(0.0);
        }

        let mut coins: Vec<f32> = homes
            .iter()
            .map(|(hid, _)| self.economy.households.get(hid).map_or(0.0, |h| h.coins))
            .collect();
        let mut treasury = self.economy.treasury[vi];
        // Avance aux artisans à court d'argent pour acheter leurs matières (grain, farine, minerai).
        let mut debt: Vec<f32> = homes
            .iter()
            .map(|(hid, _)| self.economy.households.get(hid).map_or(0.0, |h| h.debt))
            .collect();
        for k in 0..n {
            let cost: f32 = [Good::Grain, Good::Flour, Good::Ore]
                .iter()
                .map(|&g| buy[k][g as usize] * price[g as usize])
                .sum();
            let lack = cost - coins[k];
            if lack > 0.0 && debt[k] < 40.0 {
                let loan = lack.min(40.0 - debt[k]).min(treasury * 0.5).max(0.0);
                coins[k] += loan;
                debt[k] += loan;
                treasury -= loan;
            }
        }
        let mut got = vec![[0.0f32; GOODS]; n];
        let mut public_got = [0.0f32; GOODS];
        let mut supply = [0.0f32; GOODS];
        let mut demand = [0.0f32; GOODS];
        let mut traded = [0.0f32; GOODS];
        // Les marchands de passage vendent toute leur cargaison.
        let visitors = self.market_sellers(vi);
        let mut offers = sell.clone();
        for p in &visitors {
            offers.push(self.trade.traders[p].cargo);
        }
        for row in &offers {
            for g in 0..GOODS {
                supply[g] += row[g];
            }
        }
        let mut left = supply;

        // Biens non alimentaires et matières : partage au prorata de la demande.
        for g in Good::ALL {
            let gi = g as usize;
            let p = price[gi];
            let wants: Vec<f32> = (0..n).map(|k| buy[k][gi].min(coins[k] / p)).collect();
            let public = public_buy[gi].min(treasury / p);
            let total: f32 = wants.iter().sum::<f32>() + public;
            demand[gi] += buy.iter().map(|b| b[gi]).sum::<f32>() + public_buy[gi];
            if total <= 0.0 || left[gi] <= 0.0 {
                continue;
            }
            let q = total.min(left[gi]);
            let share = q / total;
            for k in 0..n {
                let a = wants[k] * share;
                got[k][gi] += a;
                coins[k] -= a * p;
            }
            public_got[gi] = public * share;
            treasury -= public_got[gi] * p;
            left[gi] -= q;
            traded[gi] += q;
        }

        // Nourriture : chacun achète ce qui lui coûte le moins par ration, à goût égal (on paie
        // volontiers le pain un peu plus cher que le grain), tant qu'il en reste.
        let cost = |g: Good| price[g as usize] / g.ration() / g.taste();
        let start = if n > 0 {
            self.rng.below(n as u64) as usize
        } else {
            0
        };
        for step in 0..n {
            let k = (start + step) % n;
            let mut need = food_need[k];
            while need > 0.01 && coins[k] > 0.01 {
                let cheapest = Good::FOODS
                    .iter()
                    .copied()
                    .filter(|&g| left[g as usize] > 0.01)
                    .min_by(|a, b| (cost(*a)).total_cmp(&cost(*b)));
                let Some(g) = cheapest else {
                    // Plus rien à vendre : la demande insatisfaite fait monter le prix du moins cher.
                    let g = Good::FOODS
                        .iter()
                        .copied()
                        .min_by(|a, b| (cost(*a)).total_cmp(&cost(*b)))
                        .unwrap();
                    demand[g as usize] += need / g.ration();
                    break;
                };
                let gi = g as usize;
                let units = (need / g.ration()).min(left[gi]).min(coins[k] / price[gi]);
                got[k][gi] += units;
                coins[k] -= units * price[gi];
                left[gi] -= units;
                traded[gi] += units;
                demand[gi] += units;
                need -= units * g.ration();
            }
        }

        // Le village remplit son grenier commun (une semaine de vivres pour tous) avec sa caisse :
        // c'est lui qui nourrit les foyers qui n'ont plus rien.
        let pop = homes.iter().map(|(_, m)| m.len()).sum::<usize>() as f32;
        let mut relief = (pop * 3.0 - self.villages[vi].stock.food).max(0.0);
        let mut relief_got = 0.0;
        while relief > 0.01 && treasury > 0.01 {
            let cheapest = Good::FOODS
                .iter()
                .copied()
                .filter(|&g| left[g as usize] > 0.01)
                .min_by(|a, b| (cost(*a)).total_cmp(&cost(*b)));
            let Some(g) = cheapest else { break };
            let gi = g as usize;
            let units = (relief / g.ration()).min(left[gi]).min(treasury / price[gi]);
            treasury -= units * price[gi];
            left[gi] -= units;
            traded[gi] += units;
            demand[gi] += units;
            relief -= units * g.ration();
            relief_got += units * g.ration();
        }
        self.villages[vi].stock.food = (self.villages[vi].stock.food + relief_got).min(MAX_FOOD);

        // Les marchands chargent leur charrette avec ce qui reste.
        for p in self.market_buyers(vi) {
            let bought = self.trader_purchase(p, &price, &left);
            for g in 0..GOODS {
                left[g] -= bought[g];
                traded[g] += bought[g];
                demand[g] += bought[g];
            }
        }

        // Paiement des vendeurs, taxe au village. Les acheteurs préfèrent le travail des
        // meilleurs : quand tout ne se vend pas, ce qui vient d'un maître part d'abord, et à
        // prix moyen égal, il se paie plus cher que celui d'un débutant (de ×0,8 à ×1,2).
        let mut revenue = [0.0f32; GOODS];
        for g in 0..GOODS {
            if supply[g] <= 0.0 {
                continue;
            }
            let worth: Vec<f32> = homes
                .iter()
                .map(|(_, members)| 0.8 + 0.4 * self.quality(members, Good::ALL[g]))
                .chain(visitors.iter().map(|_| 1.0))
                .collect();
            let sold = share_by_quality(&offers, g, traded[g], &worth);
            let paid: f32 = (0..offers.len())
                .map(|k| sold[k] * worth[k])
                .sum::<f32>()
                .max(1e-6);
            for (v, &p) in visitors.iter().enumerate() {
                let k = n + v;
                let money = traded[g] * price[g] * sold[k] * worth[k] / paid;
                // (Les recettes des marchands ne comptent pas comme revenu d'un métier d'ici.)
                let tax = if fair { 0.0 } else { tax };
                treasury += money * tax;
                self.trader_sold(p, vi, g, sold[k], money, money * (1.0 - tax));
            }
            for k in 0..n {
                let units = sold[k];
                let money = traded[g] * price[g] * units * worth[k] / paid;
                got[k][g] -= units;
                let net = money * (1.0 - tax);
                // Remboursement de l'avance sur la moitié des ventes.
                let repay = debt[k].min(net * 0.5);
                debt[k] -= repay;
                coins[k] += net - repay;
                treasury += money * tax + repay;
                revenue[g] += money;
            }
        }

        // Écritures dans les foyers et le village.
        for (k, (hid, _)) in homes.iter().enumerate() {
            let h = self.economy.household(*hid);
            for g in 0..GOODS {
                h.inv[g] = (h.inv[g] + got[k][g]).max(0.0);
            }
            h.coins = coins[k].max(0.0);
            h.debt = debt[k].max(0.0);
        }
        let st = &mut self.villages[vi].stock;
        st.wood = (st.wood + public_got[Good::Wood as usize].round() as u32).min(MAX_WOOD);
        st.stone = (st.stone + public_got[Good::Stone as usize].round() as u32).min(MAX_STONE);
        self.economy.treasury[vi] = treasury;

        // Ce que chaque métier a gagné aujourd'hui.
        for g in Good::ALL {
            self.economy.today[vi][g.producer() as usize] += revenue[g as usize];
        }

        // Les prix suivent l'offre et la demande.
        let m = &mut self.economy.markets[vi];
        for g in 0..GOODS {
            let (d, s) = (demand[g], supply[g]);
            let base = Good::ALL[g].base_price();
            // Rien ne s'échange : le prix revient doucement vers le prix habituel.
            let change = if d + s <= 0.0 {
                0.03 * (base / m.price[g] - 1.0).clamp(-1.0, 1.0)
            } else {
                0.12 * (d - s) / (d + s + 1.0)
            };
            m.price[g] = (m.price[g] * (1.0 + change.clamp(-0.12, 0.12))).clamp(base * 0.3, base * 5.0);
        }
        m.supply = supply;
        m.demand = demand;
        m.volume = traded;
        m.history.push_back(m.price);
        while m.history.len() > crate::economy::PRICE_HISTORY {
            m.history.pop_front();
        }
        if fair {
            self.fair_market(vi, &visitors);
        }
    }
}

// ------------------------------------------------------------------ qualité

/// Répartit `traded` unités vendues entre les vendeurs, en priorité selon la qualité
/// (`worth`), sans dépasser ce que chacun a mis en vente.
fn share_by_quality(sell: &[Inv], g: usize, traded: f32, worth: &[f32]) -> Vec<f32> {
    let n = sell.len();
    let mut sold = vec![0.0f32; n];
    let mut left = traded;
    // Quelques passes suffisent : chacune sert d'abord les mieux notés, au prorata.
    for _ in 0..6 {
        let weight: f32 = (0..n)
            .filter(|&k| sell[k][g] - sold[k] > 1e-4)
            .map(|k| (sell[k][g] - sold[k]) * worth[k] * worth[k])
            .sum();
        if left <= 1e-4 || weight <= 0.0 {
            break;
        }
        let mut given = 0.0;
        for k in 0..n {
            let rest = sell[k][g] - sold[k];
            if rest <= 1e-4 {
                continue;
            }
            let a = (left * rest * worth[k] * worth[k] / weight).min(rest);
            sold[k] += a;
            given += a;
        }
        left -= given;
    }
    sold
}

impl World {
    // ------------------------------------------------------------------ repas

    /// Repas de midi : chaque foyer mange ses réserves (le meilleur d'abord), puis le grenier
    /// commun aide ; un foyer qui a faim trop longtemps s'en va. Le bois de chauffe brûle,
    /// la nourriture se gâte un peu.
    pub(super) fn meals(&mut self) {
        for vi in 0..self.villages.len() {
            self.economy.ensure_village(vi);
            let homes = self.homes(vi);
            let mut any_hungry = false;
            let mut leaving = Vec::new();
            for (hid, members) in &homes {
                let full = self.need(members);
                let mut need = full;
                let wood = self.firewood(members);
                let h = self.economy.household(*hid);
                for g in Good::FOODS {
                    let gi = g as usize;
                    let units = h.inv[gi].min(need / g.ration());
                    h.inv[gi] -= units;
                    need -= units * g.ration();
                }
                let cold = h.inv[Good::Wood as usize] < wood * 0.5;
                h.inv[Good::Wood as usize] = (h.inv[Good::Wood as usize] - wood).max(0.0);
                for g in Good::ALL {
                    h.inv[g as usize] *= 1.0 - g.spoil();
                }
                let st = &mut self.villages[vi].stock;
                let help = st.food.min(need);
                st.food -= help;
                need -= help;
                // Plus rien au grenier : un ami peut partager ses provisions.
                // … sinon, un affamé sans scrupules vole chez un voisin.
                if need > 0.25 * full
                    && (self.neighbourly_help(vi, *hid, members, need) > 0.0
                        || self.steal_food(vi, *hid, members, need) > 0.0)
                {
                    let h = self.economy.household(*hid);
                    for g in Good::FOODS {
                        let gi = g as usize;
                        let units = h.inv[gi].min(need / g.ration());
                        h.inv[gi] -= units;
                        need -= units * g.ration();
                    }
                }
                self.fed(members, 1.0 - need / full.max(0.01), cold);
                let h = self.economy.household(*hid);
                if need > 0.25 * full {
                    h.hungry_days += 1;
                    any_hungry = true;
                    if h.hungry_days >= FAMINE_DAYS {
                        h.hungry_days = 0;
                        leaving.push(*hid);
                    }
                } else {
                    h.hungry_days = 0;
                }
            }
            let v = &mut self.villages[vi];
            if any_hungry {
                v.hungry_days += 1;
                if v.hungry_days == 1 {
                    let (id, name) = (v.id, v.name.clone());
                    self.event(Some(id), vec![], format!("La faim s'installe à {name}"));
                }
            } else {
                v.hungry_days = 0;
            }
            for hid in leaving {
                self.famine_leave_house(vi, hid);
            }
        }
    }

    /// Après le repas : faim (et souvenir des jours de disette), froid l'hiver sans bois.
    fn fed(&mut self, members: &[usize], share: f32, cold: bool) {
        let winter = self.clock().season == Season::Winter;
        for &m in members {
            let mind = &mut self.people[m].mind;
            mind.needs.food = share.clamp(0.0, 1.0);
            if share < 0.75 {
                mind.needs.safety = (mind.needs.safety - 0.2).max(0.0);
                mind.mood.add(crate::mind::Emotion::Fear, 0.25);
            }
            mind.needs.warmth = if cold && winter {
                (mind.needs.warmth - 0.35).max(0.0)
            } else {
                (mind.needs.warmth + 0.5).min(1.0)
            };
            if mind.needs.warmth < 0.3 {
                mind.mood.add(crate::mind::Emotion::Sadness, 0.15);
            }
        }
        if share < 0.75 {
            let tick = self.tick;
            for &m in members {
                self.people[m]
                    .mind
                    .remember(tick, crate::mind::MemoryKind::Hunger, None, -0.5);
            }
        }
    }

    // ------------------------------------------------------------------ métiers

    /// Une fois par jour : revenus moyens par métier (moyenne glissante sur ~10 jours).
    pub(super) fn update_incomes(&mut self) {
        let tick = self.tick;
        for vi in 0..self.villages.len() {
            self.economy.ensure_village(vi);
            let id = self.villages[vi].id;
            let mut workers = [0usize; JOBS];
            for p in self
                .alive()
                .filter(|p| p.village == id && !p.child(tick) && !p.elder(tick))
            {
                if let Some(j) = p.job {
                    workers[j as usize] += 1;
                }
            }
            for j in 0..JOBS {
                if workers[j] == 0 {
                    // Personne ne l'exerce : on garde l'estimation, en la rapprochant du prix du marché.
                    continue;
                }
                let per = self.economy.today[vi][j] / workers[j] as f32;
                let e = &mut self.economy.income[vi][j];
                *e = *e * 0.9 + per * 0.1;
            }
            self.economy.today[vi] = vec![0.0; JOBS];
        }
    }

    /// Ce qu'un métier rapporterait par jour aux prix actuels du marché (production typique
    /// d'une journée, moins le coût des matières), mêlé au revenu réellement observé.
    pub fn job_value(&self, vi: usize, job: Job) -> f32 {
        let Some(m) = self.economy.markets.get(vi) else {
            return 1.0;
        };
        let p = |g: Good| m.price[g as usize];
        let potential = match job {
            Job::Farmer => {
                // Le travail aux champs ne dure qu'une partie de l'année.
                let season = if self.clock().season == Season::Winter {
                    0.3
                } else {
                    1.0
                };
                12.0 * p(Good::Grain) * season
            }
            Job::Woodcutter => 8.0 * p(Good::Wood),
            Job::Quarrier => 5.0 * p(Good::Stone),
            Job::Fisher => 3.5 * p(Good::Fish),
            Job::Hunter => 2.5 * p(Good::Meat),
            Job::Miner => 4.0 * p(Good::Ore),
            Job::Miller => 20.0 * (p(Good::Flour) - p(Good::Grain)).max(0.0),
            Job::Baker => 15.0 * (1.1 * p(Good::Bread) - p(Good::Flour) - p(Good::Wood) / 6.0).max(0.0),
            Job::Smith => 2.0 * (p(Good::Tools) - 2.0 * p(Good::Ore) - 2.0 * p(Good::Wood)).max(0.0),
            Job::Merchant => self.trade_potential(vi),
            Job::Builder => {
                // Payé par voyage : vaut d'autant plus que des chantiers attendent sans bâtisseur.
                let tick = self.tick;
                let v = &self.villages[vi];
                let sites = v
                    .houses
                    .iter()
                    .filter(|h| h.state == HouseState::Building)
                    .count() as f32;
                let builders = self
                    .alive()
                    .filter(|p| p.village == v.id && p.job == Some(Job::Builder) && !p.elder(tick))
                    .count() as f32;
                if sites == 0.0 {
                    0.5
                } else {
                    2.0 + 4.0 * sites / (builders + 1.0)
                }
            }
        };
        let observed = self.economy.income.get(vi).map_or(potential, |r| r[job as usize]);
        0.5 * potential + 0.5 * observed
    }

    /// Métiers possibles dans un village (ateliers construits, ressources à portée, places libres).
    pub(crate) fn feasible_jobs(&self, vi: usize, except: Option<u32>) -> Vec<Job> {
        let tick = self.tick;
        let v = &self.villages[vi];
        let access = self.economy.access.get(vi).copied().unwrap_or([true; GOODS]);
        let count = |j: Job| {
            self.alive()
                .filter(|p| p.village == v.id && Some(p.id) != except && p.job == Some(j) && !p.elder(tick))
                .count()
        };
        let shops = |k: Workshop| {
            v.houses
                .iter()
                .filter(|h| h.state == HouseState::Inhabited && self.economy.workshops.get(&h.id) == Some(&k))
                .count()
                * k.capacity()
        };
        let pop = self.population(v.id);
        Job::ALL
            .into_iter()
            .filter(|&j| match j {
                Job::Farmer | Job::Woodcutter => true,
                Job::Builder => count(j) < 1 + pop / 15,
                Job::Quarrier => v.has_stone,
                Job::Fisher => access[Good::Fish as usize],
                Job::Hunter => access[Good::Meat as usize],
                Job::Miner => access[Good::Ore as usize] && count(j) < 3,
                Job::Miller => count(j) < shops(Workshop::Mill),
                Job::Baker => count(j) < shops(Workshop::Bakery),
                Job::Smith => count(j) < shops(Workshop::Smithy),
                // Un marchand dès quinze habitants, un de plus par trentaine, s'il y a des
                // voisins à portée.
                Job::Merchant => pop >= 15 && count(j) < 1 + pop / 30 && !self.neighbours(vi).is_empty(),
            })
            .collect()
    }

    /// Une fois par jour, quelques adultes changent de métier si un autre rapporte bien plus.
    pub(super) fn reconsider_jobs(&mut self) {
        let tick = self.tick;
        for i in 0..self.people.len() {
            let p = &self.people[i];
            if !p.alive() || p.brain.leaving || p.child(tick) || p.elder(tick) || tick < p.apprentice_until {
                continue;
            }
            let Some(job) = p.job else { continue };
            // Un marchand ne change pas de métier en plein voyage.
            if self
                .trade
                .traders
                .get(&p.id)
                .is_some_and(|t| t.leg != crate::trade::Leg::Idle)
            {
                continue;
            }
            if !self.rng.chance(1.0 / 56.0) {
                continue;
            }
            if self.economy.switched.get(&p.id).is_some_and(|&t| tick < t + YEAR) {
                continue;
            }
            let Some(vi) = self.village_index(p.village) else {
                continue;
            };
            let options = self.feasible_jobs(vi, Some(p.id));
            let value = |w: &World, j: Job| w.job_value(vi, j);
            let best = options
                .iter()
                .copied()
                .max_by(|a, b| value(self, *a).total_cmp(&value(self, *b)))
                .unwrap_or(job);
            if best != job && value(self, best) > value(self, job) * 1.5 + 0.3 {
                let id = self.people[i].id;
                self.economy.switched.insert(id, tick);
                let p = &mut self.people[i];
                p.job = Some(best);
                let text = format!(
                    "{} se fait {} (son métier ne nourrissait plus)",
                    p.first,
                    best.label(p.female)
                );
                let (id, village) = (p.id, p.village);
                self.event(Some(village), vec![id], text);
            }
        }
    }

    // ------------------------------------------------------------------ fondation, reprise

    /// Ressources à portée d'un village (calculé une fois).
    pub(crate) fn compute_access(&mut self, vi: usize) {
        self.economy.ensure_village(vi);
        while self.economy.access.len() <= vi {
            self.economy.access.push([false; GOODS]);
        }
        let (cx, cy) = (self.villages[vi].x as i64, self.villages[vi].y as i64);
        let mut found = [false; GOODS];
        found[Good::Grain as usize] = true;
        for (g, r) in [
            (Good::Wood, 2),
            (Good::Stone, 1),
            (Good::Fish, 2),
            (Good::Meat, 2),
            (Good::Ore, 3),
        ] {
            let rad = RESOURCE_RADIUS as i64 * r;
            'scan: for dy in (-rad..=rad).step_by(2) {
                for dx in (-rad..=rad).step_by(2) {
                    let (x, y) = (cx + dx, cy + dy);
                    if self.map.inside(x, y) && resource_at(&self.map, self.map.idx(x as u32, y as u32), g) {
                        found[g as usize] = true;
                        break 'scan;
                    }
                }
            }
        }
        self.economy.access[vi] = found;
    }

    /// Crée le foyer d'une maison (s'il n'existe pas) avec un peu d'argent et de grain.
    pub(crate) fn open_household(&mut self, house: u32, coins: f32, grain: f32) {
        let h = self.economy.household(house);
        h.coins += coins;
        h.inv[Good::Grain as usize] += grain;
    }

    /// Reprise d'un monde sans économie (sauvegarde de la phase 2) : un foyer par maison, qui
    /// reçoit sa part du grenier, des marchés aux prix de départ.
    pub(crate) fn bootstrap_economy(&mut self) {
        for vi in 0..self.villages.len() {
            self.compute_access(vi);
            let homes = self.homes(vi);
            let total: usize = homes.iter().map(|(_, m)| m.len()).sum();
            let food = self.villages[vi].stock.food;
            for (hid, members) in &homes {
                let share = members.len() as f32 / total.max(1) as f32;
                self.open_household(*hid, 30.0, food * 0.8 * share / Good::Grain.ration());
            }
            self.villages[vi].stock.food = food * 0.2;
        }
    }

    /// Ateliers : un moulin, puis un four, puis une forge, quand le village grandit.
    pub(super) fn plan_workshops(&mut self, vi: usize) {
        let v = &self.villages[vi];
        let pop = self.population(v.id);
        let have = |k: Workshop| {
            v.houses
                .iter()
                .any(|h| self.economy.workshops.get(&h.id) == Some(&k))
        };
        let built = |k: Workshop| {
            v.houses
                .iter()
                .any(|h| h.state == HouseState::Inhabited && self.economy.workshops.get(&h.id) == Some(&k))
        };
        let sites = v
            .houses
            .iter()
            .filter(|h| h.state == HouseState::Building)
            .count();
        if sites > 1 {
            return;
        }
        let ore = self.economy.access.get(vi).is_some_and(|a| a[Good::Ore as usize]);
        let want = if !have(Workshop::Mill) && pop >= 12 {
            Some(Workshop::Mill)
        } else if built(Workshop::Mill) && !have(Workshop::Bakery) && pop >= 16 {
            Some(Workshop::Bakery)
        } else if !have(Workshop::Smithy) && pop >= 20 && ore {
            Some(Workshop::Smithy)
        } else {
            None
        };
        let Some(kind) = want else { return };
        if let Some(id) = self.new_house(vi, HouseState::Building) {
            self.economy.workshops.insert(id, kind);
            let (vid, label) = (self.villages[vi].id, kind.a());
            self.event(Some(vid), vec![], format!("Nouveau chantier : {label}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::share_by_quality;
    use crate::economy::GOODS;

    /// Quand tout ne se vend pas, le maître vend d'abord ; personne ne vend plus qu'il n'a.
    #[test]
    fn masters_sell_first() {
        let mut sell = vec![[0.0f32; GOODS]; 2];
        sell[0][2] = 10.0; // débutant
        sell[1][2] = 10.0; // maître
        let worth = [0.84, 1.16];
        let sold = share_by_quality(&sell, 2, 10.0, &worth);
        assert!((sold[0] + sold[1] - 10.0).abs() < 1e-3);
        assert!(sold[1] > sold[0] * 1.5, "{sold:?}");
        // Tout se vend : chacun écoule tout, quelle que soit la qualité.
        let sold = share_by_quality(&sell, 2, 20.0, &worth);
        assert!(
            (sold[0] - 10.0).abs() < 1e-3 && (sold[1] - 10.0).abs() < 1e-3,
            "{sold:?}"
        );
    }
}

//! Entre villages (phase 5) : marchands, routes, nouvelles des prix.
//!
//! Tout est sauvegardé avec le monde. Le comportement des marchands, les migrations et la
//! fondation de villages sont dans `sim/trade.rs`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::economy::{GOODS, Good, Inv};

/// Ce qu'une charrette emporte (unités, tous biens confondus).
pub const CART: f32 = 40.0;
/// Voyages sur un trajet avant qu'il devienne une vraie route.
pub const ROAD_TRIPS: u32 = 6;
/// Distance à vol d'oiseau (cases) au-delà de laquelle un marchand ne s'aventure pas.
pub const TRADE_RANGE: f32 = 420.0;

/// Ce qu'un village sait des prix d'un autre (rapporté par un marchand, avec retard).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriceNews {
    /// Tick où ces prix avaient cours.
    pub tick: u64,
    pub price: Inv,
    /// Ce qui était offert et demandé à ce marché.
    pub supply: Inv,
    pub demand: Inv,
}

impl PriceNews {
    pub fn of(tick: u64, m: &crate::economy::Market) -> Self {
        PriceNews {
            tick,
            price: m.price,
            supply: m.supply,
            demand: m.demand,
        }
    }
}

/// Trajet entre deux villages : chemin de la porte du premier (plus petit identifiant) à
/// celle du second, et nombre de voyages faits dessus.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Route {
    /// Cases du trajet, départ et arrivée compris. Vide : pas de passage à pied.
    pub path: Vec<u32>,
    pub trips: u32,
    /// Le trajet est devenu une route (cases marquées sur la carte).
    pub road: bool,
}

impl Route {
    pub fn reachable(&self) -> bool {
        !self.path.is_empty()
    }
}

/// Où en est un marchand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Leg {
    /// Au village, sans voyage prévu.
    #[default]
    Idle,
    /// Attend le marché de son village pour charger la charrette.
    Buying,
    /// En route vers `dest`.
    Outbound,
    /// Arrivé à `dest`, attend son marché pour vendre (et racheter pour le retour).
    Selling,
    /// Sur le chemin du retour.
    Returning,
}

impl Leg {
    /// Hors de son village (on ne peut pas venir lui parler).
    pub fn away(self) -> bool {
        matches!(self, Leg::Outbound | Leg::Selling | Leg::Returning)
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Trader {
    pub leg: Leg,
    /// Village visé (identifiant).
    pub dest: u32,
    pub cargo: Inv,
    /// Argent emporté (capital du foyer, puis recettes).
    pub purse: f32,
    /// Capital emporté au départ (pour calculer le bénéfice).
    pub capital: f32,
    /// Ce qu'il compte acheter au marché de son village (bien, quantité).
    pub want: Option<(Good, f32)>,
    /// Prix et demande de son village au départ (pour choisir quoi ramener).
    pub home_price: Inv,
    pub home_demand: Inv,
    /// Arrivé à destination (sinon encore en chemin).
    pub arrived: bool,
    /// Dernier marché où il a acheté ou vendu (tick) : il repart après.
    pub dealt: u64,
    pub trips: u32,
    /// Marchés déjà attendus à destination sans tout vendre.
    pub waited: u8,
    /// Bénéfice total de sa vie de marchand.
    pub profit: f32,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Trade {
    /// Marchands, par identifiant d'habitant.
    pub traders: BTreeMap<u32, Trader>,
    /// Trajets entre villages, par paire d'identifiants (le plus petit d'abord).
    pub routes: BTreeMap<(u32, u32), Route>,
    /// Ce que chaque village sait des prix des autres : village → (autre village → nouvelles).
    pub news: BTreeMap<u32, BTreeMap<u32, PriceNews>>,
    /// Village d'origine de chaque village fondé en cours de partie.
    pub founded_from: BTreeMap<u32, u32>,
    /// Dernière fondation partie de chaque village (tick).
    pub last_colony: BTreeMap<u32, u64>,
    /// Unités vendues par des marchands, depuis le début, par bien.
    pub sold: Inv,
}

pub fn pair(a: u32, b: u32) -> (u32, u32) {
    (a.min(b), a.max(b))
}

impl Trade {
    pub fn route(&self, a: u32, b: u32) -> Option<&Route> {
        self.routes.get(&pair(a, b))
    }

    /// Chemin de la porte de `from` à celle de `to` (départ compris).
    pub fn path(&self, from: u32, to: u32) -> Option<Vec<u32>> {
        let r = self.route(from, to)?;
        if !r.reachable() {
            return None;
        }
        let mut p = r.path.clone();
        if from > to {
            p.reverse();
        }
        Some(p)
    }

    /// Apprend des nouvelles plus fraîches que celles qu'on a.
    pub fn hear(&mut self, village: u32, about: u32, news: &PriceNews) {
        if village == about {
            return;
        }
        let known = self.news.entry(village).or_default();
        if known.get(&about).is_none_or(|n| n.tick < news.tick) {
            known.insert(about, news.clone());
        }
    }

    /// Tout ce que `from` sait passe à `to` (ce qu'un voyageur raconte en arrivant).
    pub fn share(&mut self, from: u32, to: u32) {
        let known: Vec<(u32, PriceNews)> = self
            .news
            .get(&from)
            .map(|m| m.iter().map(|(&k, n)| (k, n.clone())).collect())
            .unwrap_or_default();
        for (about, n) in known {
            self.hear(to, about, &n);
        }
    }
}

/// Unités transportées (tous biens confondus).
pub fn load(cargo: &Inv) -> f32 {
    cargo.iter().sum()
}

pub const NO_GOODS: Inv = [0.0; GOODS];

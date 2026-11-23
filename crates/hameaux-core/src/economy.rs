//! Économie : biens, foyers, marchés, ateliers, trésor des villages.
//!
//! Tout ce qui est propre à l'économie vit dans [`Economy`] (un champ du monde), pour que les
//! types des phases précédentes restent identiques et que les anciennes sauvegardes se lisent.

use std::collections::{BTreeMap, VecDeque};

use serde::{Deserialize, Serialize};

use crate::people::Job;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum Good {
    Grain = 0,
    Flour = 1,
    Bread = 2,
    Fish = 3,
    Meat = 4,
    Wood = 5,
    Stone = 6,
    Ore = 7,
    Tools = 8,
}

pub const GOODS: usize = 9;
/// Un stock de chaque bien (quantités).
pub type Inv = [f32; GOODS];

impl Good {
    pub const ALL: [Good; GOODS] = [
        Good::Grain,
        Good::Flour,
        Good::Bread,
        Good::Fish,
        Good::Meat,
        Good::Wood,
        Good::Stone,
        Good::Ore,
        Good::Tools,
    ];
    /// Nourriture, de la préférée à la moins appréciée (ordre où l'on mange).
    pub const FOODS: [Good; 5] = [Good::Bread, Good::Meat, Good::Fish, Good::Flour, Good::Grain];

    pub fn label(self) -> &'static str {
        match self {
            Good::Grain => "grain",
            Good::Flour => "farine",
            Good::Bread => "pain",
            Good::Fish => "poisson",
            Good::Meat => "viande",
            Good::Wood => "bois",
            Good::Stone => "pierre",
            Good::Ore => "minerai",
            Good::Tools => "outils",
        }
    }

    /// « du grain », « de la farine »…
    pub fn partitive(self) -> &'static str {
        match self {
            Good::Grain => "du grain",
            Good::Flour => "de la farine",
            Good::Bread => "du pain",
            Good::Fish => "du poisson",
            Good::Meat => "de la viande",
            Good::Wood => "du bois",
            Good::Stone => "des pierres",
            Good::Ore => "du minerai",
            Good::Tools => "des outils",
        }
    }

    /// Rations par unité (0 si ce n'est pas de la nourriture).
    pub fn ration(self) -> f32 {
        match self {
            Good::Bread | Good::Meat => 1.0,
            Good::Fish => 0.8,
            Good::Flour => 0.9,
            Good::Grain => 0.8,
            _ => 0.0,
        }
    }

    /// Goût pour ce bien : on accepte de payer la ration plus cher pour ce qu'on aime.
    pub fn taste(self) -> f32 {
        match self {
            Good::Bread => 1.6,
            Good::Meat => 1.5,
            Good::Fish => 1.3,
            Good::Flour => 1.0,
            Good::Grain => 0.8,
            _ => 1.0,
        }
    }

    pub fn is_food(self) -> bool {
        self.ration() > 0.0
    }

    /// Part perdue chaque jour (pain rassis, poisson qui se gâte…).
    pub fn spoil(self) -> f32 {
        match self {
            Good::Bread => 0.03,
            Good::Fish | Good::Meat => 0.04,
            Good::Flour => 0.006,
            Good::Grain => 0.005,
            _ => 0.0,
        }
    }

    /// Prix de départ d'un marché (en sous).
    pub fn base_price(self) -> f32 {
        match self {
            Good::Grain => 0.5,
            Good::Flour => 0.9,
            Good::Bread => 1.2,
            Good::Fish => 0.9,
            Good::Meat => 1.3,
            Good::Wood => 0.6,
            Good::Stone => 0.8,
            Good::Ore => 1.5,
            Good::Tools => 8.0,
        }
    }

    /// Métier qui produit ce bien (pour savoir ce que rapporte chaque métier).
    pub fn producer(self) -> Job {
        match self {
            Good::Grain => Job::Farmer,
            Good::Flour => Job::Miller,
            Good::Bread => Job::Baker,
            Good::Fish => Job::Fisher,
            Good::Meat => Job::Hunter,
            Good::Wood => Job::Woodcutter,
            Good::Stone => Job::Quarrier,
            Good::Ore => Job::Miner,
            Good::Tools => Job::Smith,
        }
    }
}

/// Atelier (bâtiment de travail, sans habitants).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Workshop {
    Mill,
    Bakery,
    Smithy,
}

impl Workshop {
    pub fn job(self) -> Job {
        match self {
            Workshop::Mill => Job::Miller,
            Workshop::Bakery => Job::Baker,
            Workshop::Smithy => Job::Smith,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Workshop::Mill => "moulin",
            Workshop::Bakery => "four à pain",
            Workshop::Smithy => "forge",
        }
    }

    /// « le moulin », « la forge »…
    pub fn the(self) -> &'static str {
        match self {
            Workshop::Mill => "le moulin",
            Workshop::Bakery => "le four à pain",
            Workshop::Smithy => "la forge",
        }
    }

    /// « un moulin », « une forge »…
    pub fn a(self) -> &'static str {
        match self {
            Workshop::Mill => "un moulin",
            Workshop::Bakery => "un four à pain",
            Workshop::Smithy => "une forge",
        }
    }

    /// Ouvriers au plus par atelier.
    pub fn capacity(self) -> usize {
        match self {
            Workshop::Mill => 2,
            Workshop::Bakery => 3,
            Workshop::Smithy => 2,
        }
    }
}

/// Un foyer = les habitants d'une maison : ses réserves et son argent.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Household {
    pub inv: Inv,
    pub coins: f32,
    pub hungry_days: u32,
    /// Avance de la caisse du village pour acheter des matières (remboursée sur les ventes).
    pub debt: f32,
}

impl Household {
    pub fn rations(&self) -> f32 {
        Good::FOODS
            .iter()
            .map(|&g| self.inv[g as usize] * g.ration())
            .sum()
    }
}

/// Jours de prix gardés pour les courbes.
pub const PRICE_HISTORY: usize = 56;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Market {
    pub price: Inv,
    /// Quantités échangées au dernier marché.
    pub volume: Inv,
    /// Offre et demande au dernier marché.
    pub supply: Inv,
    pub demand: Inv,
    /// Prix des derniers jours (le plus récent à la fin).
    pub history: VecDeque<Inv>,
}

impl Default for Market {
    fn default() -> Self {
        let mut price = [0.0; GOODS];
        for g in Good::ALL {
            price[g as usize] = g.base_price();
        }
        Market {
            price,
            volume: [0.0; GOODS],
            supply: [0.0; GOODS],
            demand: [0.0; GOODS],
            history: VecDeque::new(),
        }
    }
}

/// Revenus récents d'un métier dans un village (moyenne glissante, sous par travailleur et par jour).
pub const JOBS: usize = Job::ALL.len();

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Economy {
    /// Foyers, par identifiant de maison.
    pub households: BTreeMap<u32, Household>,
    /// Un marché par village (même ordre que `World::villages`).
    pub markets: Vec<Market>,
    /// Caisse de chaque village (même ordre).
    pub treasury: Vec<f32>,
    /// Ateliers, par identifiant de bâtiment (le bâtiment est une `House` sans habitants).
    pub workshops: BTreeMap<u32, Workshop>,
    /// Revenu moyen par travailleur et par jour, par village et par métier (`Job as usize`).
    pub income: Vec<Vec<f32>>,
    /// Gains du jour en cours, par village et par métier.
    pub today: Vec<Vec<f32>>,
    /// Ressources à portée de chaque village.
    pub access: Vec<[bool; GOODS]>,
    /// Dernier changement de métier de chaque habitant (tick) : on garde un métier au moins un an.
    pub switched: BTreeMap<u32, u64>,
}

/// Revenu supposé d'un métier tant qu'on ne l'a pas observé.
pub fn default_income(job: Job) -> f32 {
    match job {
        Job::Farmer => 2.0,
        Job::Woodcutter => 1.5,
        Job::Quarrier => 1.4,
        Job::Builder => 1.6,
        Job::Miller => 2.0,
        Job::Baker => 2.4,
        Job::Smith => 2.6,
        Job::Fisher => 1.8,
        Job::Hunter => 1.8,
        Job::Miner => 1.6,
        Job::Merchant => 2.5,
    }
}

impl Economy {
    pub fn market_for(&mut self, vi: usize) -> &mut Market {
        while self.markets.len() <= vi {
            self.markets.push(Market::default());
        }
        &mut self.markets[vi]
    }

    pub fn ensure_village(&mut self, vi: usize) {
        self.market_for(vi);
        while self.treasury.len() <= vi {
            self.treasury.push(100.0);
        }
        while self.income.len() <= vi {
            self.income.push(Vec::new());
        }
        while self.today.len() <= vi {
            self.today.push(Vec::new());
        }
        // Un métier nouveau (sauvegarde plus ancienne) commence au revenu supposé.
        for row in self.income.iter_mut() {
            while row.len() < JOBS {
                row.push(default_income(Job::ALL[row.len()]));
            }
        }
        for row in self.today.iter_mut() {
            row.resize(JOBS.max(row.len()), 0.0);
        }
    }

    pub fn household(&mut self, house: u32) -> &mut Household {
        self.households.entry(house).or_default()
    }
}

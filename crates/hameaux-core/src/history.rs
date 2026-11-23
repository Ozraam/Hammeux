//! Mémoire longue du monde (phase 7) : relevés quotidiens de chaque village pour les tableaux
//! de bord, flux du commerce entre villages et annales (les événements marquants, gardés bien
//! plus longtemps que la chronique).
//!
//! Le tout est partagé (`Arc`) avec l'instantané publié : il ne change qu'une fois par jour
//! (et aux marchés), ce qui évite de le recopier à chaque publication.

use std::collections::{BTreeMap, VecDeque};

use serde::{Deserialize, Serialize};

use crate::{
    economy::{GOODS, Inv},
    events::Event,
};

/// Relevés quotidiens gardés (4 ans de jeu) ; au-delà, un relevé par semaine.
pub const KEEP_DAYS: usize = 112;
/// Relevés hebdomadaires gardés (≈ 70 ans de jeu).
pub const KEEP_WEEKS: usize = 1_000;
/// Événements marquants gardés dans les annales.
pub const KEEP_ANNALS: usize = 5_000;

/// Un relevé de village, pris à minuit.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Sample {
    /// Jour du monde (voir `sim::tick_day`).
    pub day: u32,
    pub population: u16,
    pub children: u16,
    pub elders: u16,
    /// Depuis le relevé précédent.
    pub births: u16,
    pub deaths: u16,
    /// Habitants partis pour de bon (disette, bannissement…).
    pub departures: u16,
    /// Familles arrivées (nouveaux venus, migrants).
    pub arrivals: u16,
    pub houses: u16,
    pub sick: u16,
    /// Jours de vivres d'avance (foyers et grenier commun).
    pub food_days: f32,
    pub treasury: f32,
    /// Argent de tous les foyers.
    pub coins: f32,
    pub tax: f32,
    /// Réserves des foyers et du village (le grenier commun compte comme du grain).
    pub stock: Inv,
    pub price: Inv,
    pub volume: Inv,
}

impl Sample {
    /// Ajoute les naissances, décès, départs et arrivées d'un autre relevé (plus ancien).
    fn add_counts(&mut self, o: &Sample) {
        self.births = self.births.saturating_add(o.births);
        self.deaths = self.deaths.saturating_add(o.deaths);
        self.departures = self.departures.saturating_add(o.departures);
        self.arrivals = self.arrivals.saturating_add(o.arrivals);
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct VillageHistory {
    /// Derniers jours (le plus récent à la fin).
    pub days: VecDeque<Sample>,
    /// Un relevé par semaine depuis la fondation (ou le début des relevés).
    pub weeks: Vec<Sample>,
}

impl VillageHistory {
    pub fn record(&mut self, s: Sample) {
        if s.day.is_multiple_of(7) || self.weeks.is_empty() {
            // Le relevé de la semaine compte tout ce qui s'est passé depuis le précédent.
            let mut w = s.clone();
            let after = self.weeks.last().map(|l| l.day);
            for d in self.days.iter().filter(|d| after.is_some_and(|a| d.day > a)) {
                w.add_counts(d);
            }
            self.weeks.push(w);
            if self.weeks.len() > KEEP_WEEKS {
                // Dans la moitié la plus ancienne, on fond les relevés deux à deux.
                let half = (self.weeks.len() / 2) & !1;
                let old: Vec<Sample> = self.weeks.drain(..half).collect();
                let merged: Vec<Sample> = old
                    .chunks(2)
                    .map(|pair| {
                        let mut keep = pair[pair.len() - 1].clone();
                        if pair.len() == 2 {
                            keep.add_counts(&pair[0]);
                        }
                        keep
                    })
                    .collect();
                self.weeks.splice(0..0, merged);
            }
        }
        self.days.push_back(s);
        while self.days.len() > KEEP_DAYS {
            self.days.pop_front();
        }
    }

    /// Dernier relevé pris au plus tard le jour donné.
    pub fn at(&self, day: u32) -> Option<&Sample> {
        let recent = self.days.iter().rev().find(|s| s.day <= day);
        recent.or_else(|| self.weeks.iter().rev().find(|s| s.day <= day))
    }
}

/// Sorte d'événement, reconnue à son texte (pour filtrer la chronique et choisir ce qui
/// mérite d'entrer dans les annales).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Kind {
    Birth,
    Death,
    Love,
    Family,
    Work,
    Building,
    Trade,
    Government,
    Justice,
    Health,
    Feast,
    Knowledge,
    Village,
    Feelings,
    Other,
}

impl Kind {
    pub const ALL: [Kind; 15] = [
        Kind::Birth,
        Kind::Death,
        Kind::Love,
        Kind::Family,
        Kind::Work,
        Kind::Building,
        Kind::Trade,
        Kind::Government,
        Kind::Justice,
        Kind::Health,
        Kind::Feast,
        Kind::Knowledge,
        Kind::Village,
        Kind::Feelings,
        Kind::Other,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Kind::Birth => "naissances",
            Kind::Death => "décès",
            Kind::Love => "amours",
            Kind::Family => "arrivées et départs",
            Kind::Work => "métiers",
            Kind::Building => "constructions",
            Kind::Trade => "commerce",
            Kind::Government => "chefs et taxes",
            Kind::Justice => "justice",
            Kind::Health => "santé",
            Kind::Feast => "fêtes et foires",
            Kind::Knowledge => "savoirs",
            Kind::Village => "villages",
            Kind::Feelings => "vie intime",
            Kind::Other => "divers",
        }
    }

    /// Reconnaît la sorte d'un événement à sa formulation (voir les `event(…)` du moteur).
    pub fn of(text: &str) -> Kind {
        let has = |s: &str| text.contains(s);
        let starts = |s: &str| text.starts_with(s);
        if starts("Naissance") {
            Kind::Birth
        } else if starts("Décès") || has(" meurt ") {
            Kind::Death
        } else if starts("Fondation de")
            || has("partent fonder un village")
            || starts("La faim s'installe")
            || has("non moissonné")
        {
            Kind::Village
        } else if has("Épidémie") || has("épidémie") {
            Kind::Health
        } else if has(" chef") || has("cheffe") || has("la taxe") || has(" destitue ") {
            Kind::Government
        } else if has(" condamne ")
            || has(" pardonne ")
            || has(" bannit ")
            || has(" à voler ")
            || has("ont disparu")
            || has("en viennent aux mains")
        {
            Kind::Justice
        } else if has("se fiancent") || has(" épouse ") || has(" emménage chez ") {
            Kind::Love
        } else if has("Fête des moissons") || has("fête des moissons") || has("foire") {
            Kind::Feast
        } else if has(" découvre ") || has(" apporte ") {
            Kind::Knowledge
        } else if starts("Une route") || has(" revient de ") || has("part voir ce qui se vend") {
            Kind::Trade
        } else if has("Une famille arrive")
            || has(" arrive ")
            || has(" quitte ")
            || has("les siens les suivent")
        {
            Kind::Family
        } else if starts("Nouveau chantier")
            || has(" est terminé")
            || has(" est abandonnée")
            || has(" s'installe dans ")
            || has(" s'installent dans ")
        {
            Kind::Building
        } else if has(" se fait ") || has(" devient ") || has(" maître ") {
            Kind::Work
        } else if has("son rêve") || has("le cœur qui bat") || has("sont brouillés") {
            Kind::Feelings
        } else {
            Kind::Other
        }
    }

    /// Mérite les annales (l'histoire longue du monde).
    pub fn notable(self, text: &str) -> bool {
        match self {
            Kind::Village | Kind::Health | Kind::Government | Kind::Knowledge => true,
            Kind::Trade => text.starts_with("Une route"),
            Kind::Feast => text.contains("Grande foire") || text.contains("Pas de fête"),
            Kind::Justice => text.contains(" bannit "),
            Kind::Death => text.contains(" meurt ") || text.contains("perd son chef"),
            Kind::Work => text.contains(" maître "),
            Kind::Building => text.contains(" est terminé"),
            Kind::Family => text.starts_with("Chassée") || text.starts_with("Faute de nourriture"),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct History {
    /// Relevés par village (identifiant).
    pub villages: BTreeMap<u32, VillageHistory>,
    /// Ce que les marchands d'un village ont vendu dans un autre : (d'où, où) → unités.
    pub flows: BTreeMap<(u32, u32), Inv>,
    /// Recettes de ces ventes (sous, avant taxe).
    pub flow_coins: BTreeMap<(u32, u32), f32>,
    /// Événements marquants (le plus récent à la fin).
    pub annals: VecDeque<Event>,
    /// Prochain numéro de chronique à examiner pour les annales.
    pub annals_seen: u64,
    /// Dernier relevé (tick), pour compter naissances et décès depuis.
    pub last_tick: u64,
}

impl History {
    pub fn add_flow(&mut self, from: u32, to: u32, good: usize, units: f32, coins: f32) {
        if good < GOODS {
            self.flows.entry((from, to)).or_insert([0.0; GOODS])[good] += units;
            *self.flow_coins.entry((from, to)).or_default() += coins;
        }
    }

    pub fn note(&mut self, e: &Event) {
        if Kind::of(&e.text).notable(&e.text) {
            self.annals.push_back(e.clone());
            while self.annals.len() > KEEP_ANNALS {
                self.annals.pop_front();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_from_texts() {
        let cases = [
            ("Naissance de Léa chez Marie et Paul", Kind::Birth),
            ("Décès de Jean Martin à 71 ans", Kind::Death),
            ("Jean Martin meurt de la fièvre à 3 ans", Kind::Death),
            (
                "Nina Mercier devient la première cheffe de Belair (5 voix sur 13)",
                Kind::Government,
            ),
            ("Paul est réélu chef de Belair (9 voix sur 20)", Kind::Government),
            ("Nina monte la taxe de Belair à 12 %", Kind::Government),
            (
                "Gabrielle condamne Ninon à 23 sous d'amende pour un vol chez Estelle",
                Kind::Justice,
            ),
            (
                "Ninon est prise à voler 12 sous chez les Carpentier",
                Kind::Justice,
            ),
            ("Chez les Martin, des provisions ont disparu", Kind::Justice),
            ("Paul et Jean en viennent aux mains", Kind::Justice),
            ("Léa et Paul se fiancent", Kind::Love),
            (
                "Léa Martin de Belair épouse Paul et s'installe à Rivedoux",
                Kind::Love,
            ),
            (
                "Fête des moissons à Belair offerte par Nina (40 sous de banquet)",
                Kind::Feast,
            ),
            ("On annonce la grande foire de Belair, dans 5 jours", Kind::Feast),
            (
                "Jeanne découvre l'assolement à Belair : les champs rendent plus",
                Kind::Knowledge,
            ),
            ("Une route relie désormais Belair et Rivedoux", Kind::Trade),
            ("Camille revient de Belair avec 28 sous de bénéfice", Kind::Trade),
            ("Une famille arrive : Perrine et Aurèle Tessier", Kind::Family),
            (
                "Chassée par la faim, la famille Roux quitte Belair pour Rivedoux",
                Kind::Family,
            ),
            (
                "Nouveau chantier : une maison pour de nouveaux venus",
                Kind::Building,
            ),
            ("Le moulin de Belair est terminé", Kind::Building),
            (
                "Marius se fait pêcheur (son métier ne nourrissait plus)",
                Kind::Work,
            ),
            ("Lucie est reconnue maître boulangère", Kind::Work),
            ("Épidémie de fièvre à Belair (3 malades)", Kind::Health),
            (
                "Fin de l'épidémie de fièvre à Belair, après 2 morts",
                Kind::Health,
            ),
            ("Fondation de Clairval (9 habitants)", Kind::Village),
            ("Léa a réalisé son rêve : trouver l'amour", Kind::Feelings),
            ("Léa et Paul sont brouillés", Kind::Feelings),
        ];
        for (text, kind) in cases {
            assert_eq!(Kind::of(text), kind, "{text}");
        }
    }

    #[test]
    fn weekly_samples_thin_out() {
        let mut h = VillageHistory::default();
        let n = KEEP_WEEKS as u32 * 7 + 70;
        for day in 0..n {
            h.record(Sample {
                day,
                births: 1,
                ..Default::default()
            });
        }
        // Aucune naissance perdue en route (sauf les jours après le dernier relevé hebdomadaire).
        let counted: u32 = h.weeks.iter().map(|w| w.births as u32).sum();
        assert_eq!(counted, h.weeks.last().unwrap().day + 1);
        assert_eq!(h.days.len(), KEEP_DAYS);
        assert!(h.weeks.len() <= KEEP_WEEKS);
        assert!(h.weeks.windows(2).all(|w| w[0].day < w[1].day));
        assert!(h.at(30).unwrap().day <= 30);
        assert_eq!(h.at(n).unwrap().day, n - 1);
    }
}

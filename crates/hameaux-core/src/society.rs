//! Société (phase 6) : chef et conseil, taxes, justice, fêtes et foires, maladies, savoirs.
//!
//! Tout est sauvegardé avec le monde (dernier champ de [`crate::World`]). Les systèmes qui
//! font vivre ces données sont dans `sim/society.rs`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{people::Job, tuning::YEAR};

// ------------------------------------------------------------------ maladies

/// Maladies. L'ordre des variantes est celui des sauvegardes : ajouter à la fin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Disease {
    /// Fièvre : fréquente, assez contagieuse, rarement mortelle.
    Fever,
    /// Flux de ventre (dysenterie) : surtout l'été, dangereuse pour les faibles.
    Flux,
    /// Picote (variole) : rare, très contagieuse et meurtrière ; on n'y est pris qu'une fois.
    Pox,
}

pub const DISEASES: usize = 3;

impl Disease {
    pub const ALL: [Disease; DISEASES] = [Disease::Fever, Disease::Flux, Disease::Pox];

    pub fn label(self) -> &'static str {
        match self {
            Disease::Fever => "fièvre",
            Disease::Flux => "flux de ventre",
            Disease::Pox => "picote",
        }
    }

    /// « la fièvre », « le flux de ventre »…
    pub fn the(self) -> &'static str {
        match self {
            Disease::Fever => "la fièvre",
            Disease::Flux => "le flux de ventre",
            Disease::Pox => "la picote",
        }
    }

    /// Chance par jour d'attraper la maladie si tout le village était malade.
    pub fn contagion(self) -> f32 {
        match self {
            Disease::Fever => 0.7,
            Disease::Flux => 0.5,
            Disease::Pox => 1.1,
        }
    }

    /// Chance de mourir par jour de maladie, pour un adulte bien portant.
    pub fn lethality(self) -> f32 {
        match self {
            Disease::Fever => 0.004,
            Disease::Flux => 0.012,
            Disease::Pox => 0.035,
        }
    }

    /// Jours de maladie.
    pub fn days(self) -> u8 {
        match self {
            Disease::Fever => 3,
            Disease::Flux => 4,
            Disease::Pox => 6,
        }
    }

    /// Durée de la protection après guérison (ticks).
    pub fn immunity(self) -> u64 {
        match self {
            Disease::Fever => YEAR,
            Disease::Flux => 2 * YEAR,
            Disease::Pox => u64::MAX / 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Sickness {
    pub disease: Disease,
    /// Tick où la maladie s'est déclarée.
    pub since: u64,
    /// Jours de maladie restants.
    pub days_left: u8,
    /// Village où on l'a attrapée.
    pub caught_in: u32,
}

// ------------------------------------------------------------------ savoirs

/// Techniques qui apparaissent quelque part et se répandent. Ordre des sauvegardes : ajouter
/// à la fin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Tech {
    /// Assolement : les champs rendent un quart de plus.
    Rotation,
    /// Filets : la pêche rapporte 40 % de plus.
    Nets,
    /// Collets : la chasse rapporte 30 % de plus.
    Snares,
    /// Trempe du fer : la forge produit 40 % d'outils de plus.
    Tempering,
    /// Levain : plus de pain pour la même farine.
    Sourdough,
    /// Simples (herbes qui soignent) : on meurt deux fois moins des maladies, on guérit plus vite.
    Herbs,
}

impl Tech {
    pub const ALL: [Tech; 6] = [
        Tech::Rotation,
        Tech::Nets,
        Tech::Snares,
        Tech::Tempering,
        Tech::Sourdough,
        Tech::Herbs,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tech::Rotation => "l'assolement",
            Tech::Nets => "les filets de pêche",
            Tech::Snares => "les collets",
            Tech::Tempering => "la trempe du fer",
            Tech::Sourdough => "le levain",
            Tech::Herbs => "les simples",
        }
    }

    /// Ce que la technique change (pour les fiches).
    pub fn effect(self) -> &'static str {
        match self {
            Tech::Rotation => "les champs rendent un quart de plus",
            Tech::Nets => "la pêche rapporte davantage",
            Tech::Snares => "la chasse rapporte davantage",
            Tech::Tempering => "la forge fait plus d'outils",
            Tech::Sourdough => "plus de pain pour la même farine",
            Tech::Herbs => "les malades guérissent mieux",
        }
    }

    /// Métier où l'on peut la découvrir (aucun : les simples, trouvées pendant une épidémie).
    pub fn job(self) -> Option<Job> {
        match self {
            Tech::Rotation => Some(Job::Farmer),
            Tech::Nets => Some(Job::Fisher),
            Tech::Snares => Some(Job::Hunter),
            Tech::Tempering => Some(Job::Smith),
            Tech::Sourdough => Some(Job::Baker),
            Tech::Herbs => None,
        }
    }
}

/// Comment un village a appris une technique.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Known {
    pub tech: Tech,
    pub tick: u64,
    /// Inventeur (si elle est née ici).
    pub by: Option<u32>,
    /// Village d'où elle est venue (sinon).
    pub from: Option<u32>,
}

// ------------------------------------------------------------------ justice

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Crime {
    /// Vol de provisions (ou d'argent).
    Theft,
    /// Bagarre.
    Brawl,
}

/// Une plainte en attente de jugement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Offence {
    pub crime: Crime,
    pub offender: u32,
    pub victim: u32,
    pub tick: u64,
    /// Valeur de ce qui a été volé (sous).
    pub value: f32,
}

// ------------------------------------------------------------------ vie du village

/// Gouvernement et vie collective d'un village.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Polity {
    pub chief: Option<u32>,
    pub council: Vec<u32>,
    /// Tick de l'élection du chef.
    pub elected: u64,
    /// Prochaine élection (tick).
    pub next_election: u64,
    /// Taxe prélevée sur les ventes du marché (et sur les récoltes, pour le grenier commun).
    pub tax: f32,
    pub offences: Vec<Offence>,
    pub techs: Vec<Known>,
    /// Épidémie en cours et morts qu'elle a faits.
    pub epidemic: Option<Disease>,
    pub epidemic_dead: u32,
    /// Dernière fête des moissons et dernière foire (tick).
    pub last_feast: u64,
    pub last_fair: u64,
    /// Jour (numéro depuis le début du monde) de la fête en cours, s'il y en a une.
    pub feast_day: Option<u64>,
    /// Jour d'ouverture de la foire annoncée ou en cours.
    pub fair_day: Option<u64>,
    /// Combien de chefs le village a eus.
    pub chiefs: u32,
}

/// Taxe tant qu'aucun chef ne l'a fixée.
pub const DEFAULT_TAX: f32 = 0.10;

impl Default for Polity {
    fn default() -> Self {
        Polity {
            chief: None,
            council: Vec::new(),
            elected: 0,
            next_election: 0,
            tax: DEFAULT_TAX,
            offences: Vec::new(),
            techs: Vec::new(),
            epidemic: None,
            epidemic_dead: 0,
            last_feast: 0,
            last_fair: 0,
            feast_day: None,
            fair_day: None,
            chiefs: 0,
        }
    }
}

impl Polity {
    pub fn knows(&self, t: Tech) -> bool {
        self.techs.iter().any(|k| k.tech == t)
    }
}

/// Compteurs depuis le début du monde (pour les rapports).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SocietyStats {
    pub elections: u32,
    pub deposed: u32,
    pub thefts: u32,
    pub brawls: u32,
    pub pardons: u32,
    pub fines: u32,
    pub fined: f32,
    pub banished: u32,
    pub unpunished: u32,
    pub feasts: u32,
    pub fairs: u32,
    pub cases: [u32; DISEASES],
    pub deaths: [u32; DISEASES],
    pub epidemics: u32,
    pub inventions: u32,
    pub spread: u32,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Society {
    /// Par identifiant de village.
    pub polities: BTreeMap<u32, Polity>,
    /// Malades, par identifiant d'habitant.
    pub sick: BTreeMap<u32, Sickness>,
    /// Dernière guérison de chaque maladie (tick + 1 ; 0 = jamais), par habitant.
    pub recovered: BTreeMap<u32, [u64; DISEASES]>,
    /// Condamnations de chaque habitant.
    pub convictions: BTreeMap<u32, u32>,
    pub stats: SocietyStats,
}

impl Society {
    pub fn polity(&self, village: u32) -> Option<&Polity> {
        self.polities.get(&village)
    }

    pub fn polity_mut(&mut self, village: u32) -> &mut Polity {
        self.polities.entry(village).or_default()
    }

    pub fn tax(&self, village: u32) -> f32 {
        self.polities.get(&village).map_or(DEFAULT_TAX, |p| p.tax)
    }

    pub fn knows(&self, village: u32, t: Tech) -> bool {
        self.polities.get(&village).is_some_and(|p| p.knows(t))
    }

    /// Protégé de cette maladie (guéri depuis peu, ou pour toujours de la picote) ?
    pub fn immune(&self, person: u32, d: Disease, tick: u64) -> bool {
        self.recovered
            .get(&person)
            .is_some_and(|r| r[d as usize] > 0 && tick < (r[d as usize] - 1).saturating_add(d.immunity()))
    }

    /// Titre d'un habitant dans son village (« chef », « conseiller »).
    pub fn office(&self, village: u32, person: u32) -> Option<&'static str> {
        let p = self.polities.get(&village)?;
        if p.chief == Some(person) {
            Some("chef")
        } else if p.council.contains(&person) {
            Some("conseil")
        } else {
            None
        }
    }
}

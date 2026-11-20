//! Habitants.

use std::collections::{BTreeMap, VecDeque};

use serde::{Deserialize, Serialize};

use crate::{
    economy::Good,
    tuning::{ADULT_AGE, ELDER_AGE, YEAR},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Job {
    Farmer,
    Woodcutter,
    Quarrier,
    Builder,
    // Phase 3 (ajoutés à la fin : l'ordre est celui des sauvegardes).
    Miller,
    Baker,
    Smith,
    Fisher,
    Hunter,
    Miner,
}

impl Job {
    pub const ALL: [Job; 10] = [
        Job::Farmer,
        Job::Woodcutter,
        Job::Quarrier,
        Job::Builder,
        Job::Miller,
        Job::Baker,
        Job::Smith,
        Job::Fisher,
        Job::Hunter,
        Job::Miner,
    ];

    pub fn label(self, female: bool) -> &'static str {
        match (self, female) {
            (Job::Farmer, false) => "fermier",
            (Job::Farmer, true) => "fermière",
            (Job::Woodcutter, false) => "bûcheron",
            (Job::Woodcutter, true) => "bûcheronne",
            (Job::Quarrier, false) => "tailleur de pierre",
            (Job::Quarrier, true) => "tailleuse de pierre",
            (Job::Builder, false) => "bâtisseur",
            (Job::Builder, true) => "bâtisseuse",
            (Job::Miller, false) => "meunier",
            (Job::Miller, true) => "meunière",
            (Job::Baker, false) => "boulanger",
            (Job::Baker, true) => "boulangère",
            (Job::Smith, false) => "forgeron",
            (Job::Smith, true) => "forgeronne",
            (Job::Fisher, false) => "pêcheur",
            (Job::Fisher, true) => "pêcheuse",
            (Job::Hunter, false) => "chasseur",
            (Job::Hunter, true) => "chasseuse",
            (Job::Miner, false) => "mineur",
            (Job::Miner, true) => "mineuse",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Alive,
    Dead { tick: u64 },
    Left { tick: u64 },
}

/// Ce qu'un habitant porte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Carry {
    Good(Good),
    Bundle,
}

/// Ce que fait un habitant (pour l'affichage).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Act {
    #[default]
    Idle,
    Walking,
    GoingHome,
    Sleeping,
    Chopping,
    Quarrying,
    Ploughing,
    Sowing,
    Tending,
    Harvesting,
    Hauling,
    Building,
    Playing,
    Chatting,
    Learning,
    Strolling,
    Arriving,
    Leaving,
    Fishing,
    Hunting,
    Mining,
    Milling,
    Baking,
    Forging,
}

impl Act {
    pub fn label(self) -> &'static str {
        match self {
            Act::Idle => "se repose",
            Act::Walking => "marche",
            Act::GoingHome => "rentre chez soi",
            Act::Sleeping => "dort",
            Act::Chopping => "coupe du bois",
            Act::Quarrying => "taille de la pierre",
            Act::Ploughing => "laboure un champ",
            Act::Sowing => "sème",
            Act::Tending => "désherbe un champ",
            Act::Harvesting => "moissonne",
            Act::Hauling => "porte un chargement au grenier",
            Act::Building => "construit une maison",
            Act::Playing => "joue",
            Act::Chatting => "discute",
            Act::Learning => "apprend le métier",
            Act::Strolling => "se promène",
            Act::Arriving => "arrive au village",
            Act::Leaving => "quitte le village",
            Act::Fishing => "pêche",
            Act::Hunting => "chasse",
            Act::Mining => "extrait du minerai",
            Act::Milling => "moud du grain",
            Act::Baking => "cuit du pain",
            Act::Forging => "forge des outils",
        }
    }
}

impl Carry {
    pub fn label(self) -> &'static str {
        match self {
            Carry::Good(g) => g.partitive(),
            Carry::Bundle => "son baluchon",
        }
    }
}

/// Une étape d'un plan. Les plans ne sont pas sauvegardés : on replanifie au chargement.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Go(u32),
    Wait {
        ticks: u64,
        act: Act,
        with: Option<u32>,
    },
    /// Prendre des matériaux au grenier commun pour un chantier.
    Take(Good),
    /// Déposer au marché, dans les réserves de son foyer, ce qu'on porte.
    Deposit,
    Build(u32),
    FieldWork(u32),
    /// Récolter une ressource naturelle (bois, pierre, poisson, gibier, minerai).
    Gather(u32, Good),
    /// Transformer dans un atelier (moulin, four, forge).
    Craft(u32),
    /// Entrer chez soi (dormir).
    Hide,
    /// Quitter le village pour de bon.
    Leave,
}

/// État de travail d'un habitant (non sauvegardé, sauf ce qui est utile à l'affichage).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Brain {
    pub plan: VecDeque<Step>,
    pub path: Vec<u32>,
    pub path_pos: usize,
    pub wait: u64,
    pub act: Act,
    /// Personne ou bâtiment lié à l'activité (ami avec qui l'on parle, maison en chantier…).
    pub with: Option<u32>,
    pub carry: Option<Carry>,
    pub carry_amount: f32,
    pub hidden: bool,
    pub leaving: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Person {
    pub id: u32,
    pub first: String,
    pub last: String,
    pub female: bool,
    pub born: i64,
    pub death_age: f32,
    pub status: Status,
    pub village: u32,
    pub home: Option<u32>,
    pub job: Option<Job>,
    /// Conjoint (même décédé : on garde le lien pour l'histoire de la famille).
    pub partner: Option<u32>,
    pub parents: Vec<u32>,
    pub children: Vec<u32>,
    /// Amitiés : identifiant → score 0–100.
    pub friends: BTreeMap<u32, f32>,
    pub master: Option<u32>,
    pub apprentice_until: u64,
    pub x: f32,
    pub y: f32,
    /// Apparence : couleur de vêtement, de peau, de cheveux (indices).
    pub looks: [u8; 3],
    #[serde(skip)]
    pub brain: Brain,
}

impl Person {
    /// Âge en années de jeu au tick donné.
    pub fn age(&self, tick: u64) -> f32 {
        (tick as i64 - self.born) as f32 / YEAR as f32
    }

    pub fn alive(&self) -> bool {
        self.status == Status::Alive
    }

    pub fn child(&self, tick: u64) -> bool {
        self.age(tick) < ADULT_AGE
    }

    pub fn elder(&self, tick: u64) -> bool {
        self.age(tick) >= ELDER_AGE
    }

    pub fn name(&self) -> String {
        format!("{} {}", self.first, self.last)
    }

    pub fn tile(&self, width: u32) -> u32 {
        self.y as u32 * width + self.x as u32
    }

    /// Accord du participe passé / adjectif : « né » / « née ».
    pub fn e(&self) -> &'static str {
        if self.female { "e" } else { "" }
    }
}

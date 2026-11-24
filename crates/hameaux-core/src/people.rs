//! Habitants.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::{
    economy::Good,
    mind::Mind,
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
    // Phase 5.
    Merchant,
}

impl Job {
    pub const ALL: [Job; 11] = [
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
        Job::Merchant,
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
            (Job::Merchant, false) => "marchand",
            (Job::Merchant, true) => "marchande",
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
    /// Charrette de marchand.
    Cart,
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
    Travelling,
    Trading,
    // Phase 6 (ajoutés à la fin : l'index est envoyé au navigateur).
    Feasting,
    Ill,
    // Phase 8.
    Praying,
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
            Act::Travelling => "voyage",
            Act::Trading => "attend le marché",
            Act::Feasting => "fait la fête",
            Act::Praying => "prie le Ciel",
            Act::Ill => "est malade, alité",
        }
    }
}

impl Carry {
    pub fn label(self) -> &'static str {
        match self {
            Carry::Good(g) => g.partitive(),
            Carry::Bundle => "son baluchon",
            Carry::Cart => "sa charrette",
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
    /// Conversation avec quelqu'un (à la fin d'une discussion).
    Talk(u32),
    /// À la fête : parler avec celui qui est le plus près.
    Mingle,
    /// Entrer chez soi (dormir).
    Hide,
    /// Dormir là où l'on est (à l'auberge d'un autre village).
    HideHere,
    /// Voyage vers un autre village par la route (`back` : retour au village).
    Journey {
        to: u32,
        back: bool,
    },
    /// Arrivée au bout d'un voyage.
    Arrive,
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
    /// En voyage entre deux villages (les changements d'heure ne l'interrompent pas).
    pub traveling: bool,
    /// Intention en cours (ce que le plan cherche à accomplir).
    pub intent: Intent,
}

/// Intention choisie par l'IA à utilité ; elle se découpe ensuite en étapes ([`Step`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Intent {
    #[default]
    None,
    Sleep,
    Work,
    Learn(u32),
    Chat(u32),
    Court(u32),
    Visit(u32),
    Stroll,
    Rest,
    Play,
    Leave,
    /// Aller à la fête (ou à la foire) du village.
    Feast,
    /// Prier (le jour de repos, les croyants).
    Pray,
}

impl Intent {
    /// Personne visée par l'intention.
    pub fn target(self) -> Option<u32> {
        match self {
            Intent::Learn(p) | Intent::Chat(p) | Intent::Court(p) | Intent::Visit(p) => Some(p),
            _ => None,
        }
    }

    /// « veut discuter avec », « travaille »… (le nom de la personne visée suit).
    pub fn label(self) -> &'static str {
        match self {
            Intent::None => "",
            Intent::Sleep => "va dormir",
            Intent::Work => "travaille",
            Intent::Learn(_) => "apprend auprès de",
            Intent::Chat(_) => "veut discuter avec",
            Intent::Court(_) => "fait la cour à",
            Intent::Visit(_) => "rend visite à",
            Intent::Stroll => "prend l'air",
            Intent::Rest => "se repose",
            Intent::Play => "joue",
            Intent::Leave => "s'en va",
            Intent::Feast => "va à la fête",
            Intent::Pray => "va prier",
        }
    }
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
    pub master: Option<u32>,
    pub apprentice_until: u64,
    pub x: f32,
    pub y: f32,
    /// Apparence : couleur de vêtement, de peau, de cheveux (indices).
    pub looks: [u8; 3],
    /// Personnalité, besoins, humeur, souvenirs, relations, compétences, objectifs (phase 4).
    pub mind: Mind,
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

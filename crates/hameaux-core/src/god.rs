//! Mode dieu (phase 8) : commandes datées, jauge de foi, effets en cours, religion.
//!
//! Le joueur agit sur le monde par des [`GodCommand`] : chacune coûte de la foi, qui se
//! recharge avec le temps et avec les prières des croyants. Les miracles marquent les
//! habitants (croyance, souvenirs, crainte ou gratitude) ; quand assez d'habitants d'un
//! village croient, un culte y naît, avec son prêtre et bientôt son temple.
//!
//! Tout est sauvegardé avec le monde (dernier champ de [`crate::World`]). Les effets sont
//! dans `sim/god.rs`.

use std::collections::{BTreeMap, VecDeque};

use serde::{Deserialize, Serialize};

/// Un pouvoir appliqué au monde. Le JSON (`{"power": "rain", "village": 3}`) est celui de
/// l'API (`POST /api/god`) et des scripts rejoués par `hameaux-cli run --god`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "power", rename_all = "snake_case")]
pub enum GodCommand {
    // Nature.
    /// Pluie : les champs poussent plus vite pendant deux jours, la sécheresse cesse.
    Rain {
        village: u32,
    },
    /// Sécheresse : les champs ne poussent plus pendant trois jours et se dessèchent.
    Drought {
        village: u32,
    },
    /// Orage : tout le monde rentre, des champs sont couchés, un chantier abîmé.
    Storm {
        village: u32,
    },
    /// Récolte miraculeuse : les champs semés mûrissent d'un coup (l'hiver, une manne).
    Bounty {
        village: u32,
    },
    /// Sauterelles : les champs sont dévorés, le grenier entamé.
    Locusts {
        village: u32,
    },
    /// Abondance de gibier et de poisson pendant cinq jours.
    Abundance {
        village: u32,
    },
    /// Faire pousser une forêt autour d'une case.
    Forest {
        x: u32,
        y: u32,
    },
    /// Révéler un gisement (pierre, fer, argile) ou poser une ressource (poisson, gibier, baies).
    Deposit {
        x: u32,
        y: u32,
        resource: u8,
    },
    // Bénédictions et fléaux.
    /// Guérir tous les malades d'un village.
    Cure {
        village: u32,
    },
    /// Déclencher une épidémie (0 fièvre, 1 flux de ventre, 2 picote).
    Plague {
        village: u32,
        disease: u8,
    },
    // Villageois.
    Heal {
        person: u32,
    },
    /// Bénir : chance et talent dans son métier.
    Bless {
        person: u32,
    },
    /// Maudire : le malheur s'abat sur lui.
    Curse {
        person: u32,
    },
    /// Souffle d'inspiration : une technique de son métier, sinon un grand rêve.
    Inspire {
        person: u32,
    },
    /// Faire se rencontrer deux personnes (elles tombent amoureuses).
    Matchmake {
        a: u32,
        b: u32,
    },
    // Société.
    /// Un signe dans le ciel, vu de tout le monde.
    Sign,
    /// Provoquer une fête (aujourd'hui, ou demain s'il est tard).
    Feast {
        village: u32,
    },
    /// Envoyer une famille d'étrangers.
    Strangers {
        village: u32,
    },
    /// Renommer un village.
    Rename {
        village: u32,
        name: String,
    },
    // Création.
    /// Tracer une route entre deux villages.
    Road {
        from: u32,
        to: u32,
    },
    /// Fonder un village avec quelques familles venues d'ailleurs.
    Found {
        x: u32,
        y: u32,
    },
}

/// Ce que vise un pouvoir (pour l'interface).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    World,
    Village,
    Person,
    TwoPeople,
    TwoVillages,
    Tile,
}

impl Target {
    pub fn key(self) -> &'static str {
        match self {
            Target::World => "world",
            Target::Village => "village",
            Target::Person => "person",
            Target::TwoPeople => "two_people",
            Target::TwoVillages => "two_villages",
            Target::Tile => "tile",
        }
    }
}

/// Fiche d'un pouvoir : identifiant (celui du JSON), nom, effet, coût, catégorie, cible.
#[derive(Debug, Clone, Copy)]
pub struct Power {
    pub key: &'static str,
    pub label: &'static str,
    pub help: &'static str,
    pub cost: f32,
    pub category: &'static str,
    pub target: Target,
    /// Bienfait (gratitude) ou fléau (crainte) : ce qu'en pensent les témoins.
    pub benign: bool,
}

const NATURE: &str = "Nature";
const PLAGUES: &str = "Bénédictions et fléaux";
const PEOPLE: &str = "Villageois";
const SOCIETY: &str = "Société";
const CREATION: &str = "Création";

pub const POWERS: [Power; 21] = [
    Power {
        key: "rain",
        label: "Pluie",
        help: "Les champs poussent vite pendant deux jours ; la sécheresse cesse.",
        cost: 10.0,
        category: NATURE,
        target: Target::Village,
        benign: true,
    },
    Power {
        key: "drought",
        label: "Sécheresse",
        help: "Trois jours sans pousse : les champs semés se dessèchent.",
        cost: 15.0,
        category: NATURE,
        target: Target::Village,
        benign: false,
    },
    Power {
        key: "storm",
        label: "Orage",
        help: "Chacun court s'abriter ; des champs sont couchés, un chantier abîmé.",
        cost: 20.0,
        category: NATURE,
        target: Target::Village,
        benign: false,
    },
    Power {
        key: "bounty",
        label: "Récolte miraculeuse",
        help: "Les champs semés mûrissent d'un coup (l'hiver : une manne au grenier).",
        cost: 45.0,
        category: NATURE,
        target: Target::Village,
        benign: true,
    },
    Power {
        key: "locusts",
        label: "Sauterelles",
        help: "Les champs sont dévorés et le grenier entamé.",
        cost: 35.0,
        category: NATURE,
        target: Target::Village,
        benign: false,
    },
    Power {
        key: "abundance",
        label: "Gibier et poisson",
        help: "Chasse et pêche rapportent bien plus pendant cinq jours.",
        cost: 25.0,
        category: NATURE,
        target: Target::Village,
        benign: true,
    },
    Power {
        key: "forest",
        label: "Faire pousser une forêt",
        help: "Les prés libres autour de la case se couvrent d'arbres.",
        cost: 15.0,
        category: NATURE,
        target: Target::Tile,
        benign: true,
    },
    Power {
        key: "deposit",
        label: "Révéler un gisement",
        help: "Pierre, fer ou argile sous la case (ou poisson, gibier, baies).",
        cost: 30.0,
        category: NATURE,
        target: Target::Tile,
        benign: true,
    },
    Power {
        key: "cure",
        label: "Guérir l'épidémie",
        help: "Tous les malades du village se relèvent.",
        cost: 35.0,
        category: PLAGUES,
        target: Target::Village,
        benign: true,
    },
    Power {
        key: "plague",
        label: "Épidémie",
        help: "Quelques habitants tombent malades ; la maladie fera le reste.",
        cost: 30.0,
        category: PLAGUES,
        target: Target::Village,
        benign: false,
    },
    Power {
        key: "heal",
        label: "Guérir",
        help: "Un malade se relève, guéri.",
        cost: 8.0,
        category: PEOPLE,
        target: Target::Person,
        benign: true,
    },
    Power {
        key: "bless",
        label: "Bénir",
        help: "Joie, chance et talent dans son métier.",
        cost: 12.0,
        category: PEOPLE,
        target: Target::Person,
        benign: true,
    },
    Power {
        key: "curse",
        label: "Maudire",
        help: "Peur, malheur, talent qui se perd ; parfois la fièvre.",
        cost: 12.0,
        category: PEOPLE,
        target: Target::Person,
        benign: false,
    },
    Power {
        key: "inspire",
        label: "Souffle d'inspiration",
        help: "Une technique de son métier pour tout le village, sinon un grand rêve.",
        cost: 30.0,
        category: PEOPLE,
        target: Target::Person,
        benign: true,
    },
    Power {
        key: "matchmake",
        label: "Faire se rencontrer",
        help: "Deux cœurs libres se trouvent.",
        cost: 10.0,
        category: PEOPLE,
        target: Target::TwoPeople,
        benign: true,
    },
    Power {
        key: "sign",
        label: "Signe dans le ciel",
        help: "Une lueur vue de tous : les pieux s'émerveillent, les autres s'inquiètent.",
        cost: 20.0,
        category: SOCIETY,
        target: Target::World,
        benign: true,
    },
    Power {
        key: "feast",
        label: "Fête",
        help: "Le village fait la fête (demain s'il est déjà tard).",
        cost: 20.0,
        category: SOCIETY,
        target: Target::Village,
        benign: true,
    },
    Power {
        key: "strangers",
        label: "Envoyer des étrangers",
        help: "Une famille de pèlerins s'installe au village.",
        cost: 25.0,
        category: SOCIETY,
        target: Target::Village,
        benign: true,
    },
    Power {
        key: "rename",
        label: "Renommer",
        help: "Le village prend le nom que tu lui donnes.",
        cost: 5.0,
        category: SOCIETY,
        target: Target::Village,
        benign: true,
    },
    Power {
        key: "road",
        label: "Tracer une route",
        help: "Une route relie deux villages.",
        cost: 25.0,
        category: CREATION,
        target: Target::TwoVillages,
        benign: true,
    },
    Power {
        key: "found",
        label: "Fonder un village",
        help: "Quelques familles s'installent sur la case choisie.",
        cost: 80.0,
        category: CREATION,
        target: Target::Tile,
        benign: true,
    },
];

impl GodCommand {
    pub fn key(&self) -> &'static str {
        match self {
            GodCommand::Rain { .. } => "rain",
            GodCommand::Drought { .. } => "drought",
            GodCommand::Storm { .. } => "storm",
            GodCommand::Bounty { .. } => "bounty",
            GodCommand::Locusts { .. } => "locusts",
            GodCommand::Abundance { .. } => "abundance",
            GodCommand::Forest { .. } => "forest",
            GodCommand::Deposit { .. } => "deposit",
            GodCommand::Cure { .. } => "cure",
            GodCommand::Plague { .. } => "plague",
            GodCommand::Heal { .. } => "heal",
            GodCommand::Bless { .. } => "bless",
            GodCommand::Curse { .. } => "curse",
            GodCommand::Inspire { .. } => "inspire",
            GodCommand::Matchmake { .. } => "matchmake",
            GodCommand::Sign => "sign",
            GodCommand::Feast { .. } => "feast",
            GodCommand::Strangers { .. } => "strangers",
            GodCommand::Rename { .. } => "rename",
            GodCommand::Road { .. } => "road",
            GodCommand::Found { .. } => "found",
        }
    }

    pub fn power(&self) -> &'static Power {
        let k = self.key();
        POWERS
            .iter()
            .find(|p| p.key == k)
            .expect("chaque commande a sa fiche")
    }
}

// ------------------------------------------------------------------ effets en cours

/// Effet qui dure sur un village. L'ordre est celui des sauvegardes et de l'index envoyé au
/// navigateur : ajouter à la fin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpellKind {
    Rain,
    Drought,
    Storm,
    Abundance,
    Locusts,
    Bounty,
    Blessing,
}

impl SpellKind {
    pub fn label(self) -> &'static str {
        match self {
            SpellKind::Rain => "pluie bienfaisante",
            SpellKind::Drought => "sécheresse",
            SpellKind::Storm => "orage",
            SpellKind::Abundance => "gibier et poisson en abondance",
            SpellKind::Locusts => "nuée de sauterelles",
            SpellKind::Bounty => "récolte miraculeuse",
            SpellKind::Blessing => "lumière du Ciel",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Spell {
    pub village: u32,
    pub kind: SpellKind,
    pub until: u64,
}

// ------------------------------------------------------------------ religion

/// Ce qu'un village pense du Ciel.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Cult {
    /// Tick de la naissance du culte (`None` : on ne prie pas encore ici).
    pub founded: Option<u64>,
    /// « le culte du Ciel clément ».
    pub name: String,
    pub priest: Option<u32>,
    /// Maison (identifiant) du temple, en chantier ou bâti.
    pub temple: Option<u32>,
    /// Gratitude pour les bienfaits et crainte des fléaux (0 à ~1, s'estompent).
    pub love: f32,
    pub fear: f32,
    pub miracles: u32,
    pub scourges: u32,
    /// Croyants (croyance ≥ [`BELIEVER`]) et croyance moyenne des habitants, au dernier minuit.
    pub believers: u32,
    pub devotion: f32,
}

/// À partir de cette croyance, un habitant prie.
pub const BELIEVER: f32 = 0.25;

impl Cult {
    /// « te vénère », « te craint », « t'en veut »…
    pub fn attitude(&self) -> &'static str {
        let (l, f) = (self.love, self.fear);
        if l < 0.08 && f < 0.08 {
            if self.devotion >= BELIEVER {
                "te prie"
            } else {
                "t'ignore"
            }
        } else if l >= 1.5 * f {
            if self.devotion >= 0.35 {
                "te vénère"
            } else {
                "t'est reconnaissant"
            }
        } else if f >= 1.5 * l {
            if self.devotion >= BELIEVER {
                "te craint"
            } else {
                "t'en veut"
            }
        } else {
            "te craint et t'implore"
        }
    }
}

// ------------------------------------------------------------------ journal

/// Une commande appliquée : de quoi la relire et la rejouer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Deed {
    pub tick: u64,
    #[serde(with = "command_repr")]
    pub command: GodCommand,
    pub cost: f32,
    /// Ce qui s'est passé (« Le Ciel fait pleuvoir sur Belair »).
    pub text: String,
}

/// Représentation d'une commande : l'objet JSON habituel (`{"power": "rain", …}`) pour l'API
/// et les scripts, mais un texte JSON dans les sauvegardes binaires. Postcard ne sait pas
/// relire une énumération étiquetée par un champ ; et sous forme de texte, un pouvoir ajouté
/// plus tard ne casse pas les anciennes sauvegardes.
mod command_repr {
    use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _, ser::Error as _};

    use super::GodCommand;

    pub fn serialize<S: Serializer>(c: &GodCommand, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            c.serialize(s)
        } else {
            let text = serde_json::to_string(c).map_err(S::Error::custom)?;
            s.serialize_str(&text)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<GodCommand, D::Error> {
        if d.is_human_readable() {
            GodCommand::deserialize(d)
        } else {
            let text = String::deserialize(d)?;
            serde_json::from_str(&text).map_err(D::Error::custom)
        }
    }
}

/// Une commande à appliquer à un tick donné (scripts de `hameaux-cli run --god`). Le journal
/// des actes (`GET /api/god/deeds`) a la même forme : on peut le rejouer tel quel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scheduled {
    pub tick: u64,
    pub command: GodCommand,
}

/// Compteurs depuis le début du monde.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct GodStats {
    pub commands: u32,
    pub refused: u32,
    pub spent: f32,
    pub by_power: BTreeMap<String, u32>,
    pub cults: u32,
    pub cults_lost: u32,
    pub priests: u32,
    pub temples: u32,
    /// Prières (dizaines de minutes passées à prier).
    pub prayers: u64,
}

/// Foi au départ et plafond sans culte.
pub const FAITH_START: f32 = 50.0;
pub const FAITH_MAX: f32 = 100.0;
/// Foi rendue chaque jour sans aucun croyant.
pub const FAITH_PER_DAY: f32 = 15.0;
/// Actes gardés dans le journal.
pub const KEEP_DEEDS: usize = 500;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct God {
    pub faith: f32,
    /// Foi rendue par jour (calculée à minuit : base + croyants + temples).
    pub regen: f32,
    pub spells: Vec<Spell>,
    /// Le plus récent à la fin.
    pub deeds: VecDeque<Deed>,
    /// Par identifiant de village.
    pub cults: BTreeMap<u32, Cult>,
    /// Croyance de chaque habitant (0–1), par identifiant ; absent = 0.
    pub belief: BTreeMap<u32, f32>,
    /// Fêtes promises pour le lendemain.
    pub feasts: Vec<u32>,
    /// Tick du dernier signe dans le ciel (le navigateur le montre).
    pub sign: u64,
    pub stats: GodStats,
}

impl Default for God {
    fn default() -> Self {
        God {
            faith: FAITH_START,
            regen: FAITH_PER_DAY,
            spells: Vec::new(),
            deeds: VecDeque::new(),
            cults: BTreeMap::new(),
            belief: BTreeMap::new(),
            feasts: Vec::new(),
            sign: 0,
            stats: GodStats::default(),
        }
    }
}

impl God {
    pub fn belief(&self, person: u32) -> f32 {
        self.belief.get(&person).copied().unwrap_or(0.0)
    }

    /// Plafond de la foi : plus de cultes, plus de réserve.
    pub fn max_faith(&self) -> f32 {
        let cults = self.cults.values().filter(|c| c.founded.is_some()).count() as f32;
        (FAITH_MAX + 20.0 * cults).min(250.0)
    }

    pub fn active(&self, village: u32, kind: SpellKind, tick: u64) -> bool {
        self.spells
            .iter()
            .any(|s| s.village == village && s.kind == kind && s.until > tick)
    }

    pub fn cult(&self, village: u32) -> Option<&Cult> {
        self.cults.get(&village)
    }

    /// Titre religieux (« prêtre du Ciel »).
    pub fn office(&self, village: u32, person: u32) -> Option<&'static str> {
        self.cults
            .get(&village)
            .is_some_and(|c| c.priest == Some(person))
            .then_some("prêtre du Ciel")
    }
}

/// Mot pour la croyance d'un habitant.
pub fn belief_label(b: f32, female: bool) -> &'static str {
    match (b, female) {
        (b, false) if b >= 0.7 => "croyant fervent",
        (b, true) if b >= 0.7 => "croyante fervente",
        (b, false) if b >= 0.45 => "croyant",
        (b, true) if b >= 0.45 => "croyante",
        (b, _) if b >= BELIEVER => "prie parfois",
        (b, _) if b >= 0.08 => "a entendu parler du Ciel",
        _ => "ne croit en rien",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_read_as_json() {
        let c: GodCommand = serde_json::from_str(r#"{"power":"rain","village":3}"#).unwrap();
        assert_eq!(c, GodCommand::Rain { village: 3 });
        assert_eq!(c.power().label, "Pluie");
        let s: Vec<Scheduled> = serde_json::from_str(
            r#"[{"tick":10,"command":{"power":"sign"},"cost":20,"text":"…"},
                {"tick":12,"command":{"power":"matchmake","a":1,"b":2}}]"#,
        )
        .unwrap();
        assert_eq!(s[0].command, GodCommand::Sign);
        assert_eq!(s[1].command, GodCommand::Matchmake { a: 1, b: 2 });
    }

    #[test]
    fn every_power_has_a_unique_key() {
        for (i, p) in POWERS.iter().enumerate() {
            assert!(POWERS[i + 1..].iter().all(|q| q.key != p.key), "{}", p.key);
        }
    }
}

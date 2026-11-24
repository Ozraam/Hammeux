//! Cerveaux des habitants (phase 4) : personnalité, besoins, humeur, souvenirs, relations,
//! compétences et objectifs de vie.
//!
//! Tout ce qui est ici est sauvegardé avec l'habitant. Les systèmes qui font vivre ces
//! données (besoins qui baissent, conversations, commérages…) sont dans `sim/mind.rs`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{economy::JOBS, people::Job, rng::Rng};

// ------------------------------------------------------------------ personnalité

/// Cinq traits de personnalité et cinq valeurs, entre 0 et 1 (0,5 = moyen).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Traits {
    pub openness: f32,
    pub rigor: f32,
    pub sociability: f32,
    pub kindness: f32,
    pub nervousness: f32,
    pub ambition: f32,
    pub generosity: f32,
    pub piety: f32,
    pub risk: f32,
    pub attachment: f32,
}

impl Default for Traits {
    fn default() -> Self {
        Traits::from_array([0.5; 10])
    }
}

pub const TRAIT_LABELS: [(&str, &str); 10] = [
    ("curieux", "routinier"),
    ("rigoureux", "désordonné"),
    ("sociable", "solitaire"),
    ("gentil", "bourru"),
    ("anxieux", "serein"),
    ("ambitieux", "sans ambition"),
    ("généreux", "regardant"),
    ("pieux", "sceptique"),
    ("audacieux", "prudent"),
    ("attaché à son village", "bourlingueur"),
];

impl Traits {
    pub fn to_array(self) -> [f32; 10] {
        [
            self.openness,
            self.rigor,
            self.sociability,
            self.kindness,
            self.nervousness,
            self.ambition,
            self.generosity,
            self.piety,
            self.risk,
            self.attachment,
        ]
    }

    pub fn from_array(a: [f32; 10]) -> Self {
        Traits {
            openness: a[0],
            rigor: a[1],
            sociability: a[2],
            kindness: a[3],
            nervousness: a[4],
            ambition: a[5],
            generosity: a[6],
            piety: a[7],
            risk: a[8],
            attachment: a[9],
        }
    }

    /// Tirage : moyenne de deux dés (les extrêmes sont rares).
    pub fn random(rng: &mut Rng) -> Self {
        let mut a = [0.0; 10];
        for v in &mut a {
            *v = (rng.f32() + rng.f32()) / 2.0;
        }
        Traits::from_array(a)
    }

    /// Enfant : moitié héritée (moyenne des parents), moitié tirée au sort.
    pub fn inherit(parents: &[Traits], rng: &mut Rng) -> Self {
        let mut t = Traits::random(rng).to_array();
        if !parents.is_empty() {
            for (k, v) in t.iter_mut().enumerate() {
                let mean = parents.iter().map(|p| p.to_array()[k]).sum::<f32>() / parents.len() as f32;
                *v = (0.5 * *v + 0.5 * mean).clamp(0.0, 1.0);
            }
        }
        Traits::from_array(t)
    }

    /// Les deux ou trois traits les plus marqués, en mots (accordés au féminin si besoin).
    pub fn describe(self, female: bool) -> Vec<String> {
        let mut marked: Vec<(f32, String)> = self
            .to_array()
            .iter()
            .zip(TRAIT_LABELS)
            .filter_map(|(&v, (hi, lo))| {
                let d = (v - 0.5).abs();
                (d > 0.12).then(|| (d, feminine(if v > 0.5 { hi } else { lo }, female)))
            })
            .collect();
        marked.sort_by(|a, b| b.0.total_cmp(&a.0));
        marked.into_iter().take(3).map(|(_, s)| s).collect()
    }
}

/// Accord au féminin des adjectifs de [`TRAIT_LABELS`] et des humeurs.
pub fn feminine(word: &str, female: bool) -> String {
    if !female {
        return word.to_string();
    }
    let (head, tail) = match word.split_once(' ') {
        Some((h, t)) => (h, format!(" {t}")),
        None => (word, String::new()),
    };
    let f = match head {
        "gentil" => "gentille".to_string(),
        "inquiet" => "inquiète".to_string(),
        "routinier" => "routinière".to_string(),
        "sans" => head.to_string(),
        h if h.ends_with("eux") => format!("{}euse", &h[..h.len() - 3]),
        h if h.ends_with("eur") => format!("{}euse", &h[..h.len() - 3]),
        h if h.ends_with('e') => h.to_string(),
        h => format!("{h}e"),
    };
    f + &tail
}

// ------------------------------------------------------------------ besoins

/// Besoins, de 0 (manque total) à 1 (comblé). Ils baissent avec le temps et remontent selon
/// ce qu'on fait (dormir, manger, discuter, se promener, travailler, être en famille).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Needs {
    pub food: f32,
    pub sleep: f32,
    pub warmth: f32,
    pub safety: f32,
    pub company: f32,
    pub leisure: f32,
    pub esteem: f32,
    pub meaning: f32,
}

impl Default for Needs {
    fn default() -> Self {
        Needs {
            food: 1.0,
            sleep: 0.9,
            warmth: 1.0,
            safety: 0.9,
            company: 0.7,
            leisure: 0.7,
            esteem: 0.6,
            meaning: 0.7,
        }
    }
}

pub const NEED_LABELS: [&str; 8] = [
    "faim",
    "sommeil",
    "chaleur",
    "sécurité",
    "compagnie",
    "loisir",
    "reconnaissance",
    "sens",
];

impl Needs {
    pub fn to_array(self) -> [f32; 8] {
        [
            self.food,
            self.sleep,
            self.warmth,
            self.safety,
            self.company,
            self.leisure,
            self.esteem,
            self.meaning,
        ]
    }

    /// Bien-être général (0–1) : les besoins vitaux comptent double.
    pub fn wellbeing(self) -> f32 {
        let a = self.to_array();
        let w = [2.0, 1.5, 1.5, 1.0, 1.0, 0.7, 0.7, 0.7];
        a.iter().zip(w).map(|(v, w)| v * w).sum::<f32>() / w.iter().sum::<f32>()
    }

    pub fn clamp(&mut self) {
        for v in [
            &mut self.food,
            &mut self.sleep,
            &mut self.warmth,
            &mut self.safety,
            &mut self.company,
            &mut self.leisure,
            &mut self.esteem,
            &mut self.meaning,
        ] {
            *v = v.clamp(0.0, 1.0);
        }
    }
}

// ------------------------------------------------------------------ humeur

/// Émotions, de 0 à 1. Elles retombent toutes seules (demi-vie d'environ une demi-journée).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Mood {
    pub joy: f32,
    pub anger: f32,
    pub fear: f32,
    pub sadness: f32,
}

impl Mood {
    /// Humeur générale, de -1 (au plus mal) à 1 (radieux).
    pub fn valence(self) -> f32 {
        (self.joy - self.sadness - 0.6 * self.anger - 0.6 * self.fear).clamp(-1.0, 1.0)
    }

    pub fn add(&mut self, e: Emotion, amount: f32) {
        let v = match e {
            Emotion::Joy => &mut self.joy,
            Emotion::Anger => &mut self.anger,
            Emotion::Fear => &mut self.fear,
            Emotion::Sadness => &mut self.sadness,
        };
        *v = (*v + amount).clamp(0.0, 1.0);
    }

    pub fn decay(&mut self, k: f32) {
        self.joy *= k;
        self.anger *= k;
        self.fear *= k;
        self.sadness *= k;
    }

    /// L'émotion dominante (si elle est assez forte).
    pub fn dominant(self) -> Option<Emotion> {
        let all = [
            (self.joy, Emotion::Joy),
            (self.anger, Emotion::Anger),
            (self.fear, Emotion::Fear),
            (self.sadness, Emotion::Sadness),
        ];
        all.into_iter()
            .filter(|e| e.0 > 0.25)
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .map(|e| e.1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Emotion {
    Joy,
    Anger,
    Fear,
    Sadness,
}

// ------------------------------------------------------------------ souvenirs

/// Ce dont on se souvient. L'ordre des variantes est celui des sauvegardes : ajouter à la fin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryKind {
    ChildBorn,
    PartnerFound,
    Married,
    Bereaved,
    Hunger,
    /// Quelqu'un nous a aidés (nourriture).
    Helped,
    /// On a aidé quelqu'un.
    GaveHelp,
    Quarrel,
    GoodTalk,
    Advice,
    BecameMaster,
    NewHome,
    FriendLeft,
    GoalReached,
    Heartbreak,
    Apprenticed,
    // Phase 6.
    Elected,
    Robbed,
    Fight,
    Judged,
    Feast,
    Sick,
    Invented,
    Banished,
    // Phase 8.
    /// Témoin d'un bienfait du Ciel.
    Miracle,
    /// Témoin d'un fléau du Ciel.
    Scourge,
    Blessed,
    Cursed,
    /// Un signe dans le ciel.
    Omen,
    // Phase 10.
    Storm,
    Flood,
    Wildfire,
    HarshWinter,
    /// Détroussé par des brigands.
    Waylaid,
}

/// Nombre maximal de souvenirs gardés (les plus faibles s'effacent d'abord).
pub const MEMORIES: usize = 20;
/// Au-delà de cette intensité, un souvenir ne s'efface plus.
pub const STRONG_MEMORY: f32 = 0.8;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Memory {
    pub tick: u64,
    pub kind: MemoryKind,
    /// La personne concernée, s'il y en a une.
    pub who: Option<u32>,
    /// Charge émotionnelle : positive (bon souvenir) ou négative ; sa valeur absolue s'efface.
    pub weight: f32,
}

impl Memory {
    /// Phrase à la première personne (« Paul m'a aidé quand j'avais faim »), `who` déjà résolu.
    pub fn text(&self, who: &str, female: bool) -> String {
        let e = if female { "e" } else { "" };
        match self.kind {
            MemoryKind::ChildBorn => format!("La naissance de {who}"),
            MemoryKind::PartnerFound => format!("Sa rencontre avec {who}"),
            MemoryKind::Married => format!("Son union avec {who}"),
            MemoryKind::Bereaved => format!("La mort de {who}"),
            MemoryKind::Hunger => "Les jours de faim".into(),
            MemoryKind::Helped => format!("{who} l'a aidé{e} quand son foyer avait faim"),
            MemoryKind::GaveHelp => format!("A aidé {who} à passer une disette"),
            MemoryKind::Quarrel => format!("Une dispute avec {who}"),
            MemoryKind::GoodTalk => format!("Une longue conversation avec {who}"),
            MemoryKind::Advice => format!("Un bon conseil de {who}"),
            MemoryKind::BecameMaster => "Le jour où son talent a été reconnu".into(),
            MemoryKind::NewHome => "L'installation dans sa maison".into(),
            MemoryKind::FriendLeft => format!("Le départ de {who}"),
            MemoryKind::GoalReached => "Un rêve réalisé".into(),
            MemoryKind::Heartbreak => format!("Son amour déçu pour {who}"),
            MemoryKind::Apprenticed => format!("Son apprentissage auprès de {who}"),
            MemoryKind::Elected => "Le jour où le village l'a choisi{e} pour chef".into(),
            MemoryKind::Robbed => format!("{who} l'a volé{e}"),
            MemoryKind::Fight => format!("Une bagarre avec {who}"),
            MemoryKind::Judged => format!("Le jugement rendu par {who}"),
            MemoryKind::Feast => "Une belle fête au village".into(),
            MemoryKind::Sick => "Sa guérison après une maladie".into(),
            MemoryKind::Invented => "Sa découverte".into(),
            MemoryKind::Banished => "Son bannissement".into(),
            MemoryKind::Miracle => "Un prodige du Ciel".into(),
            MemoryKind::Scourge => "La colère du Ciel".into(),
            MemoryKind::Blessed => format!("Le jour où le Ciel l'a béni{e}"),
            MemoryKind::Cursed => format!("Le jour où le Ciel l'a maudit{e}"),
            MemoryKind::Omen => "Un signe dans le ciel".into(),
            MemoryKind::Storm => "Le grand orage".into(),
            MemoryKind::Flood => "La crue de la rivière".into(),
            MemoryKind::Wildfire => "L'incendie de la forêt".into(),
            MemoryKind::HarshWinter => "L'hiver rude".into(),
            MemoryKind::Waylaid => "Le jour où des brigands l'ont détroussé{e}".into(),
        }
    }
}

// ------------------------------------------------------------------ relations

/// Ce qu'un habitant pense d'un autre. Affection, confiance, respect et attirance vont de 0
/// à 100 ; la rivalité aussi (0 = aucune).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Relation {
    pub affection: f32,
    pub trust: f32,
    pub respect: f32,
    pub attraction: f32,
    pub rivalry: f32,
}

impl Relation {
    /// Opinion générale (-100 à 100).
    pub fn opinion(&self) -> f32 {
        (0.5 * self.affection + 0.3 * self.respect + 0.2 * self.trust - self.rivalry).clamp(-100.0, 100.0)
    }

    pub fn clamp(&mut self) {
        for v in [
            &mut self.affection,
            &mut self.trust,
            &mut self.respect,
            &mut self.attraction,
            &mut self.rivalry,
        ] {
            *v = v.clamp(0.0, 100.0);
        }
    }

    /// Une relation qui ne vaut plus la peine d'être gardée en mémoire.
    pub fn faint(&self) -> bool {
        self.affection < 3.0 && self.rivalry < 3.0 && self.attraction < 3.0 && self.respect < 5.0
    }

    /// Mot pour la relation (« ami », « rival », « amoureux »…).
    pub fn label(&self, female: bool) -> String {
        let s = if self.rivalry >= 40.0 && self.rivalry > self.affection {
            "rival"
        } else if self.attraction >= 50.0 {
            "amoureux"
        } else if self.affection >= 70.0 {
            "très proche"
        } else if self.affection >= 40.0 {
            "proche"
        } else if self.rivalry >= 15.0 && self.rivalry > self.affection {
            "en froid"
        } else if self.affection >= 15.0 {
            "ami"
        } else {
            "connaissance"
        };
        match (s, female) {
            ("rival", true) => "rivale".into(),
            ("amoureux", true) => "amoureuse".into(),
            ("ami", true) => "amie".into(),
            _ => s.into(),
        }
    }
}

/// Relations gardées au plus par habitant (les plus faibles sont oubliées).
pub const RELATIONS: usize = 40;

// ------------------------------------------------------------------ objectifs

/// Objectifs de vie : ils orientent les intentions sur des années.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Goal {
    Marry,
    Children,
    Master,
    Wealth,
    Respect,
}

impl Goal {
    pub fn label(self) -> &'static str {
        match self {
            Goal::Marry => "trouver l'amour",
            Goal::Children => "avoir des enfants",
            Goal::Master => "devenir maître de son métier",
            Goal::Wealth => "mettre de l'argent de côté",
            Goal::Respect => "être respecté du village",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LifeGoal {
    pub goal: Goal,
    /// Tick où il a été atteint.
    pub reached: Option<u64>,
}

// ------------------------------------------------------------------ l'esprit complet

/// Compétence à partir de laquelle on est reconnu maître de son métier.
pub const MASTERY: f32 = 0.8;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Mind {
    pub traits: Traits,
    pub needs: Needs,
    pub mood: Mood,
    pub memories: Vec<Memory>,
    pub relations: BTreeMap<u32, Relation>,
    /// Compétence par métier (0–1), indexée par `Job as usize` (absente = 0).
    pub skills: Vec<f32>,
    pub goals: Vec<LifeGoal>,
}

impl Mind {
    pub fn new(traits: Traits) -> Self {
        Mind {
            traits,
            ..Default::default()
        }
    }

    pub fn rel(&self, id: u32) -> Relation {
        self.relations.get(&id).copied().unwrap_or_default()
    }

    pub fn rel_mut(&mut self, id: u32) -> &mut Relation {
        self.relations.entry(id).or_default()
    }

    pub fn affection(&self, id: u32) -> f32 {
        self.relations.get(&id).map_or(0.0, |r| r.affection)
    }

    pub fn skill(&self, job: Job) -> f32 {
        self.skills.get(job as usize).copied().unwrap_or(0.0)
    }

    pub fn skill_mut(&mut self, job: Job) -> &mut f32 {
        if self.skills.len() < JOBS {
            self.skills.resize(JOBS, 0.0);
        }
        &mut self.skills[job as usize]
    }

    /// Progrès dans un métier : rapide jusqu'à 0,5, puis de plus en plus lent (la maîtrise
    /// demande des années). Vrai si l'on vient de passer maître.
    pub fn practice(&mut self, job: Job, rate: f32) -> bool {
        let s = self.skill_mut(job);
        let before = *s;
        let slow = if *s > 0.5 { 2.0 * (1.0 - *s) } else { 1.0 };
        *s = (*s + rate.max(0.0) * (1.0 - *s) * slow).min(1.0);
        before < MASTERY && *s >= MASTERY
    }

    pub fn has_goal(&self, g: Goal) -> bool {
        self.goals.iter().any(|l| l.goal == g && l.reached.is_none())
    }

    /// Ajoute un souvenir ; s'il y en a trop, le plus faible (non fort) est oublié.
    pub fn remember(&mut self, tick: u64, kind: MemoryKind, who: Option<u32>, weight: f32) {
        // Même souvenir récent avec la même personne : on le renforce au lieu de le dupliquer.
        if let Some(m) = self
            .memories
            .iter_mut()
            .find(|m| m.kind == kind && m.who == who && tick.saturating_sub(m.tick) < 3 * crate::tuning::DAY)
        {
            m.weight = (m.weight + weight * 0.5).clamp(-1.0, 1.0);
            return;
        }
        self.memories.push(Memory {
            tick,
            kind,
            who,
            weight: weight.clamp(-1.0, 1.0),
        });
        if self.memories.len() > MEMORIES
            && let Some(k) = self
                .memories
                .iter()
                .enumerate()
                .min_by(|a, b| a.1.weight.abs().total_cmp(&b.1.weight.abs()))
                .map(|(k, _)| k)
        {
            self.memories.remove(k);
        }
    }

    /// Une fois par jour : les souvenirs ordinaires s'estompent, puis s'oublient.
    pub fn fade_memories(&mut self) {
        for m in &mut self.memories {
            if m.weight.abs() < STRONG_MEMORY {
                m.weight *= 0.93;
            }
        }
        self.memories.retain(|m| m.weight.abs() >= 0.05);
    }

    /// Une fois par jour : on oublie les relations sans importance et on en garde au plus
    /// [`RELATIONS`].
    pub fn prune_relations(&mut self, keep: impl Fn(u32) -> bool) {
        self.relations.retain(|&id, r| keep(id) || !r.faint());
        if self.relations.len() > RELATIONS {
            let mut all: Vec<(u32, f32)> = self
                .relations
                .iter()
                .map(|(&id, r)| {
                    (
                        id,
                        r.affection + r.rivalry + r.attraction + if keep(id) { 1000.0 } else { 0.0 },
                    )
                })
                .collect();
            all.sort_by(|a, b| b.1.total_cmp(&a.1));
            for (id, _) in all.into_iter().skip(RELATIONS) {
                self.relations.remove(&id);
            }
        }
    }

    /// Libellé de l'humeur (« joyeux », « triste »…), au masculin.
    pub fn mood_label(&self) -> &'static str {
        match self.mood.dominant() {
            Some(Emotion::Anger) => "fâché",
            Some(Emotion::Fear) => "inquiet",
            Some(Emotion::Sadness) if self.mood.sadness > 0.55 => "triste",
            Some(Emotion::Sadness) => "morose",
            Some(Emotion::Joy) if self.mood.joy > 0.6 => "joyeux",
            _ => {
                let w = self.needs.wellbeing() + 0.3 * self.mood.valence();
                if self.needs.sleep < 0.2 {
                    "épuisé"
                } else if w > 0.8 {
                    "heureux"
                } else if w > 0.55 {
                    "content"
                } else {
                    "morose"
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memories_are_capped_and_strong_ones_stay() {
        let mut m = Mind::default();
        m.remember(0, MemoryKind::Bereaved, Some(1), -0.95);
        for k in 0..40u64 {
            m.remember(
                k * crate::tuning::DAY * 2,
                MemoryKind::GoodTalk,
                Some(k as u32 + 10),
                0.2,
            );
        }
        assert_eq!(m.memories.len(), MEMORIES);
        for _ in 0..200 {
            m.fade_memories();
        }
        assert_eq!(m.memories.len(), 1);
        assert_eq!(m.memories[0].kind, MemoryKind::Bereaved);
    }

    #[test]
    fn mastery_takes_much_longer_than_competence() {
        let mut m = Mind::default();
        let job = Job::Baker;
        let mut to_half = None;
        let mut sessions = 0;
        while !m.practice(job, 0.005) {
            sessions += 1;
            if to_half.is_none() && m.skill(job) >= 0.5 {
                to_half = Some(sessions);
            }
            assert!(sessions < 10_000);
        }
        // Plus de deux fois plus de séances de 0,5 à 0,8 que de 0 à 0,5.
        assert!(
            sessions - to_half.unwrap() > 2 * to_half.unwrap(),
            "{to_half:?} puis {sessions}"
        );
    }

    #[test]
    fn inheritance_stays_in_range() {
        let mut rng = Rng::new(3);
        let a = Traits::from_array([1.0; 10]);
        let b = Traits::from_array([0.9; 10]);
        for _ in 0..100 {
            let c = Traits::inherit(&[a, b], &mut rng);
            assert!(c.to_array().iter().all(|&v| (0.45..=1.0).contains(&v)));
        }
    }

    #[test]
    fn feminine_agreement() {
        assert_eq!(feminine("curieux", true), "curieuse");
        assert_eq!(feminine("attaché à son village", true), "attachée à son village");
        assert_eq!(feminine("sans ambition", true), "sans ambition");
        assert_eq!(feminine("bourru", true), "bourrue");
        assert_eq!(feminine("rigoureux", true), "rigoureuse");
        assert_eq!(feminine("bourlingueur", true), "bourlingueuse");
        assert_eq!(feminine("solitaire", true), "solitaire");
        assert_eq!(feminine("fâché", true), "fâchée");
        for (hi, lo) in TRAIT_LABELS {
            assert!(!feminine(hi, true).is_empty() && !feminine(lo, true).is_empty());
        }
    }
}

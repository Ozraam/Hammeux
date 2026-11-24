//! Météo et risques (phase 10) : fronts qui traversent la carte, temps qu'il fait dans chaque
//! village, humidité du sol, neige, hivers rudes, crues, incendies de forêt, dangers de la
//! route.
//!
//! Le temps n'est plus tiré au sort village par village : des fronts (pluie, orage, temps sec,
//! vague de froid) naissent à l'ouest et traversent la carte vers l'est ; chaque village reçoit
//! ce que lui apportent les fronts qui passent sur lui, et le sol garde la mémoire de la pluie
//! (humidité, neige, crues). Tout est rangé à part, à la fin du monde (voir `save.rs`). Les
//! effets sont dans `sim/weather.rs`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::clock::Season;

/// Sorte de front. L'ordre est celui des sauvegardes et de l'index envoyé au navigateur.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrontKind {
    /// Nuages et pluie (neige quand il gèle).
    Rain,
    /// Orage : pluie drue, vent, éclairs.
    Storm,
    /// Hautes pressions : ciel clair, sol qui sèche, chaleur l'été.
    Dry,
    /// Vague de froid : le thermomètre chute, il neige s'il pleut.
    Cold,
}

impl FrontKind {
    pub fn label(self) -> &'static str {
        match self {
            FrontKind::Rain => "front pluvieux",
            FrontKind::Storm => "ligne d'orages",
            FrontKind::Dry => "temps sec",
            FrontKind::Cold => "vague de froid",
        }
    }

    /// Apporte-t-il de la pluie (ou de la neige) ?
    pub fn wet(self) -> bool {
        matches!(self, FrontKind::Rain | FrontKind::Storm)
    }
}

/// Un front : une tache de temps qui avance sur la carte (coordonnées en cases).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Front {
    pub id: u32,
    pub kind: FrontKind,
    pub x: f32,
    pub y: f32,
    /// Cases par heure.
    pub vx: f32,
    pub vy: f32,
    pub radius: f32,
    /// Force au cœur du front (0–1).
    pub strength: f32,
    pub born: u64,
    /// Tick où il se dissipe.
    pub until: u64,
    /// Villages déjà frappés (orages : une seule fois par passage).
    pub hit: Vec<u32>,
}

impl Front {
    /// Force du front sur une case (0 hors de son rayon, plus fort au cœur, qui faiblit à la
    /// naissance et à la fin).
    pub fn at(&self, x: f32, y: f32, tick: u64) -> f32 {
        let d = (self.x - x).hypot(self.y - y);
        if d >= self.radius {
            return 0.0;
        }
        let core = 1.0 - (d / self.radius).powi(2);
        let life = (self.until.saturating_sub(self.born)).max(1) as f32;
        let age = tick.saturating_sub(self.born) as f32 / life;
        let fade = (age / 0.1).min(1.0).min((1.0 - age) / 0.15).clamp(0.0, 1.0);
        self.strength * core * fade
    }
}

/// Ce qu'il fait dans un village (mis à jour chaque heure).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Local {
    /// Pluie (ou neige) qui tombe en ce moment, 0–1.
    pub rain: f32,
    /// Température (°C).
    pub temp: f32,
    /// Vent d'orage en ce moment (0–1).
    pub storm: f32,
    /// Humidité du sol, 0 (craquelé) – 1 (détrempé).
    pub wet: f32,
    /// Neige au sol, 0–1.
    pub snow: f32,
    /// Pluie des trois derniers jours (somme qui s'oublie lentement) : au-delà d'un seuil, la
    /// rivière déborde.
    pub flood_water: f32,
    /// Jours de suite sans pluie.
    pub dry_days: u32,
    /// Température moyenne de la veille (pour le bois de chauffe).
    pub temp_mean: f32,
    /// Somme et nombre des relevés de la journée en cours.
    pub temp_sum: f32,
    pub temp_n: u32,
    /// Pluie tombée aujourd'hui.
    pub rain_today: f32,
    /// Fin de la crue en cours (tick) ; 0 : pas de crue.
    pub flooded_until: u64,
    /// Dernière crue (tick), pour qu'elles ne se suivent pas de trop près.
    pub last_flood: u64,
    /// Plus grande rivière à portée des champs (0 aucune, 1 ruisseau, 2 rivière, 3 fleuve) ;
    /// 255 : pas encore regardé.
    pub river: u8,
    /// Ce que le temps fait à la pousse des champs en ce moment (1 : normal).
    pub crop: f32,
    /// Gel tardif déjà annoncé ce printemps.
    pub frost_told: bool,
}

impl Default for Local {
    fn default() -> Self {
        Local {
            rain: 0.0,
            temp: 10.0,
            storm: 0.0,
            wet: 0.55,
            snow: 0.0,
            flood_water: 0.0,
            dry_days: 0,
            temp_mean: 10.0,
            temp_sum: 0.0,
            temp_n: 0,
            rain_today: 0.0,
            flooded_until: 0,
            last_flood: 0,
            river: 255,
            crop: 1.0,
            frost_told: false,
        }
    }
}

impl Local {
    /// Le ciel en un mot (index envoyé au navigateur : voir [`SKY_LABELS`]).
    pub fn sky(&self) -> u8 {
        if self.storm > 0.25 {
            3
        } else if self.rain > 0.08 && self.temp < 1.0 {
            4
        } else if self.rain > 0.35 {
            2
        } else if self.rain > 0.08 {
            1
        } else {
            0
        }
    }

    /// Ralentissement de la marche : neige épaisse, boue après la pluie, crue.
    pub fn slow(&self, tick: u64) -> f32 {
        let mut k = 1.0;
        if self.snow > 0.25 {
            k *= 1.0 - 0.45 * ((self.snow - 0.25) / 0.75).min(1.0);
        }
        if self.wet > 0.85 && self.temp > 0.0 {
            k *= 0.85;
        }
        if self.flooded_until > tick {
            k *= 0.8;
        }
        k
    }

    /// Ce que le temps fait à la pousse : sécheresse, sol détrempé, froid, canicule.
    pub fn crop_factor(&self) -> f32 {
        let w = self.wet;
        let water = if w < 0.08 {
            0.3
        } else if w < 0.28 {
            0.3 + 0.8 * (w - 0.08) / 0.2
        } else if w <= 0.9 {
            1.1
        } else {
            0.85
        };
        let t = self.temp_mean;
        let warmth = if t < 2.0 {
            0.4
        } else if t < 7.0 {
            0.4 + 0.6 * (t - 2.0) / 5.0
        } else if t <= 29.0 {
            1.0
        } else {
            0.8
        };
        water * warmth
    }
}

/// Libellés du ciel (index de [`Local::sky`]).
pub const SKY_LABELS: [&str; 5] = ["beau temps", "quelques averses", "pluie", "orage", "neige"];

/// Un incendie de forêt en cours.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fire {
    pub id: u32,
    /// Village le plus proche (pour la chronique), s'il y en a un.
    pub village: Option<u32>,
    pub started: u64,
    /// Cases en feu et l'heure où chacune s'éteindra.
    pub burning: Vec<(u32, u64)>,
    /// Cases déjà brûlées par ce feu.
    pub burnt: u32,
    /// Allumé par le Ciel.
    pub divine: bool,
}

/// Ce qui arrive à un marchand pendant un voyage (oublié à son retour).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Trip {
    /// Ralentissement en ce moment (neige, boue) : 1 sans gêne.
    pub slow: f32,
    pub robbed: bool,
    pub mishap: bool,
    /// Heures perdues (abri pendant l'orage, chemin enneigé).
    pub delay: u32,
}

/// Compteurs depuis le début du monde.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct WeatherStats {
    pub fronts: u32,
    /// Jours de pluie et de neige (par village).
    pub rainy_days: u32,
    pub snowy_days: u32,
    /// Journées (par village) où les champs souffrent de la sécheresse.
    pub drought_days: u32,
    pub storms: u32,
    pub frosts: u32,
    pub harsh_winters: u32,
    pub floods: u32,
    pub fires: u32,
    pub burnt_tiles: u32,
    pub regrown_tiles: u32,
    pub cold_deaths: u32,
    pub robberies: u32,
    pub stolen_coins: f32,
    pub stolen_goods: f32,
    pub mishaps: u32,
    pub lost_goods: f32,
    /// Heures de retard des marchands.
    pub delays: u32,
    /// Champs perdus (crues, orages, gel).
    pub fields_lost: u32,
}

/// Hiver rude : ce que la saison retire au thermomètre.
pub const HARSH_WINTER: f32 = 7.0;
/// Rayon (cases) autour du grenier où une rivière menace les champs.
pub const FLOOD_RADIUS: i64 = 14;
/// Pluie des trois derniers jours au-delà de laquelle une rivière déborde.
pub const FLOOD_WATER: f32 = 19.0;
/// Années avant qu'une forêt brûlée ait repoussé.
pub const REGROW_YEARS: u64 = 2;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Weather {
    pub fronts: Vec<Front>,
    pub next_id: u32,
    /// Temps de chaque village (même ordre que `World::villages`).
    pub local: Vec<Local>,
    /// Écart de température de la saison en cours (hiver rude, été caniculaire), partout.
    pub anomaly: f32,
    /// Saison à laquelle l'écart a été tiré.
    pub anomaly_season: Option<Season>,
    pub fires: Vec<Fire>,
    /// Cases de forêt brûlées, avec le tick du feu et leur ancien milieu (elles repoussent).
    pub burnt: Vec<(u32, u64, u8)>,
    /// Voyages en cours, par marchand.
    pub trips: BTreeMap<u32, Trip>,
    /// Danger connu de chaque trajet (brigands, accidents), qui s'oublie avec le temps.
    pub danger: BTreeMap<(u32, u32), f32>,
    pub stats: WeatherStats,
}

impl Default for Weather {
    fn default() -> Self {
        Weather {
            fronts: Vec::new(),
            next_id: 1,
            local: Vec::new(),
            anomaly: 0.0,
            anomaly_season: None,
            fires: Vec::new(),
            burnt: Vec::new(),
            trips: BTreeMap::new(),
            danger: BTreeMap::new(),
            stats: WeatherStats::default(),
        }
    }
}

impl Weather {
    pub fn local(&self, vi: usize) -> Local {
        self.local.get(vi).copied().unwrap_or_default()
    }

    pub fn local_mut(&mut self, vi: usize) -> &mut Local {
        while self.local.len() <= vi {
            self.local.push(Local::default());
        }
        &mut self.local[vi]
    }

    /// L'hiver en cours est-il rude ?
    pub fn harsh(&self) -> bool {
        self.anomaly_season == Some(Season::Winter) && self.anomaly <= -HARSH_WINTER + 1.5
    }

    /// Danger connu d'un trajet (0 : sûr).
    pub fn danger(&self, a: u32, b: u32) -> f32 {
        self.danger.get(&crate::trade::pair(a, b)).copied().unwrap_or(0.0)
    }

    /// Une case brûle-t-elle ?
    pub fn burning(&self, tile: u32) -> bool {
        self.fires
            .iter()
            .any(|f| f.burning.iter().any(|&(t, _)| t == tile))
    }
}

/// Température de saison (°C) sans front ni relief.
pub fn base_temp(season: Season) -> f32 {
    match season {
        Season::Spring => 11.0,
        Season::Summer => 20.0,
        Season::Autumn => 11.0,
        Season::Winter => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crops_like_moist_warm_soil() {
        let mut l = Local {
            wet: 0.6,
            temp_mean: 15.0,
            ..Default::default()
        };
        assert!(l.crop_factor() > 1.0);
        l.wet = 0.05;
        assert!(l.crop_factor() <= 0.31);
        l.wet = 0.6;
        l.temp_mean = 1.0;
        assert!(l.crop_factor() < 0.45);
    }

    #[test]
    fn fronts_fade_in_and_out() {
        let f = Front {
            id: 1,
            kind: FrontKind::Rain,
            x: 0.0,
            y: 0.0,
            vx: 1.0,
            vy: 0.0,
            radius: 10.0,
            strength: 1.0,
            born: 0,
            until: 1000,
            hit: vec![],
        };
        assert_eq!(f.at(20.0, 0.0, 500), 0.0);
        assert!(f.at(0.0, 0.0, 500) > 0.99);
        assert!(f.at(0.0, 0.0, 10) < 0.2);
        assert!(f.at(5.0, 0.0, 500) < 0.8);
    }
}

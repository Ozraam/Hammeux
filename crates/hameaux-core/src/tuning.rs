//! Réglages de la simulation des villages. Durées en ticks (1 tick = 10 s de jeu).
//!
//! Échelle : une case ≈ 25 m. Une maison occupe une case, un champ 3 × 3 cases.
//! Une année de jeu dure 28 jours (≈ 11 h réelles à la vitesse normale).

use crate::clock::{DAYS_PER_YEAR, TICK_SECONDS};

pub const MINUTE: u64 = 60 / TICK_SECONDS;
pub const HOUR: u64 = 60 * MINUTE;
pub const DAY: u64 = 24 * HOUR;
pub const YEAR: u64 = DAY * DAYS_PER_YEAR;

/// Âges (en années de jeu).
pub const ADULT_AGE: f32 = 16.0;
pub const MOVE_OUT_AGE: f32 = 19.0;
pub const ELDER_AGE: f32 = 60.0;
pub const DEATH_AGE_MIN: f32 = 62.0;
pub const DEATH_AGE_MAX: f32 = 88.0;
pub const FERTILE_MAX_AGE: f32 = 42.0;

/// Marche : cases par tick (≈ 1,25 m/s).
pub const WALK: f32 = 0.5;
/// Au-delà de ce nombre de cases explorées, la recherche de chemin abandonne (trajet en ligne droite).
pub const PATH_BUDGET: usize = 40_000;

/// Rayon (cases) où un village construit ses maisons et ses champs.
pub const VILLAGE_RADIUS: i32 = 24;
/// Rayon (cases) où l'on va chercher du bois et de la pierre.
pub const RESOURCE_RADIUS: i32 = 34;
pub const MAX_HOUSES: usize = 60;
pub const HOUSE_CAPACITY: usize = 6;
pub const FIELD_SIZE: u32 = 3;

/// Matériaux d'une maison.
pub const HOUSE_WOOD: u32 = 8;
pub const HOUSE_STONE: u32 = 4;
/// Ce qu'on rapporte en un voyage.
pub const LOAD_WOOD: u32 = 3;
pub const LOAD_STONE: u32 = 2;
pub const MAX_WOOD: u32 = 300;
pub const MAX_STONE: u32 = 200;
pub const MAX_FOOD: f32 = 5_000.0;

/// Nourriture par jour (1 ration = un adulte nourri une journée).
pub const RATION_ADULT: f32 = 1.0;
pub const RATION_CHILD: f32 = 0.6;
/// Récolte d'un champ (rations) pour une fertilité de 1.
pub const FIELD_YIELD: f32 = 100.0;
/// Jours de pousse entre semis et récolte (croissance saisonnière comprise).
pub const FIELD_GROW_DAYS: f32 = 9.0;
/// Part d'un champ travaillée (labour, semis, moisson) par séance de 2 h.
pub const FIELD_WORK_PER_SESSION: f32 = 0.34;
/// Champs visés par habitant.
pub const FIELDS_PER_PERSON: f32 = 0.23;

/// Durées des tâches.
pub const WORK_SESSION: u64 = 2 * HOUR;
pub const BUILD_SESSION: u64 = HOUR;
pub const CHAT: u64 = 20 * MINUTE;
pub const APPRENTICESHIP: u64 = 7 * DAY;

/// Probabilités quotidiennes.
pub const BIRTH_PER_DAY: f32 = 1.0 / (5.0 * DAYS_PER_YEAR as f32);
pub const PAIRING_PER_DAY: f32 = 0.08;
pub const IMMIGRATION_PER_DAY: f32 = 0.03;
/// Jours de disette avant qu'une famille parte.
pub const FAMINE_DAYS: u32 = 4;

/// Déclin d'une maison vide.
pub const ABANDONED_TO_RUIN: u64 = 2 * DAY;
pub const RUIN_TO_OVERGROWN: u64 = 3 * DAY;
pub const OVERGROWN_TO_GONE: u64 = 4 * DAY;

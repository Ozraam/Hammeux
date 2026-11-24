//! Terres (phase 9) : l'état du sol de chaque champ, le gibier et le poisson autour de chaque
//! village.
//!
//! La terre n'est plus inépuisable : chaque moisson appauvrit le champ, que la fumure (à
//! l'automne et l'hiver) remonte ; les mauvaises herbes envahissent les champs semés qu'on ne
//! sarcle pas ; le gibier et le poisson diminuent quand on en prend trop et se reconstituent
//! lentement. Tout ceci est rangé à part (à la fin du monde, voir `save.rs`) pour que les
//! types des phases précédentes restent identiques.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Richesse du sol au-dessous de laquelle un champ ne descend pas (même épuisé, il rend un peu).
pub const SOIL_MIN: f32 = 0.35;
/// Richesse qu'une fumure soignée peut atteindre (au-delà de la terre neuve).
pub const SOIL_MAX: f32 = 1.1;
/// Ce qu'une moisson complète retire au sol (l'assolement en retire moins).
pub const HARVEST_DRAIN: f32 = 0.16;
pub const HARVEST_DRAIN_ROTATION: f32 = 0.1;
/// Fumure : gain d'une séance, d'autant plus faible que le sol est déjà riche.
pub const MANURE_GAIN: f32 = 0.035;
/// Mauvaises herbes gagnées chaque jour par un champ semé, et ôtées par une séance de sarclage.
pub const WEEDS_PER_DAY: f32 = 0.15;
pub const WEEDING: f32 = 0.3;
/// Ce qu'un foyer de fermiers paie à un journalier pour une séance aux champs (sarclage, fumure,
/// labour) ; il ne loue des bras que s'il a de quoi payer.
pub const FARMHAND_WAGE: f32 = 0.7;
pub const FARMHAND_MIN_COINS: f32 = 12.0;
/// Grain qu'il faut pour semer un champ (pris aux réserves du foyer qui le cultive, ou au
/// grenier commun) ; sans semence, le champ est semé clair et rend moitié moins.
pub const SEED: f32 = 14.0;
/// Bois qu'une maison demande chaque jour pour son entretien (toit, portes, clôtures).
pub const HOUSE_UPKEEP: f32 = 0.25;
/// Part de la moisson que garde celui qui aide à moissonner le champ d'un autre.
pub const REAPER_SHARE: f32 = 0.25;
/// Une friche se repose seule, très lentement.
pub const FALLOW_REST: f32 = 0.004;

/// Gibier ou poisson qu'un village peut prendre avant que la ressource tombe à zéro (unités) ;
/// elle se reconstitue en quelques semaines si on la laisse en paix.
pub const GAME_STOCK: f32 = 220.0;
pub const FISH_STOCK: f32 = 320.0;
pub const WILD_REGROWTH: f32 = 0.09;
/// Le gibier et le poisson ne disparaissent jamais tout à fait.
pub const WILD_MIN: f32 = 0.12;

/// État du sol d'un champ.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Soil {
    /// Richesse : 1 pour une terre neuve, de `SOIL_MIN` à `SOIL_MAX`.
    pub richness: f32,
    /// Mauvaises herbes, 0–1 (elles étouffent jusqu'à 60 % de la récolte).
    pub weeds: f32,
    /// Foyer (maison) qui cultive ce champ ; personne : la récolte va au grenier commun.
    pub owner: Option<u32>,
    /// Semé clair, faute de semence : la récolte en souffre.
    pub thin: bool,
}

impl Default for Soil {
    fn default() -> Self {
        Soil {
            richness: 1.0,
            weeds: 0.0,
            owner: None,
            thin: false,
        }
    }
}

impl Soil {
    /// Part de la récolte qu'on peut espérer de ce champ (sol et herbes).
    pub fn yield_factor(&self) -> f32 {
        self.richness * (1.0 - 0.6 * self.weeds) * if self.thin { 0.5 } else { 1.0 }
    }

    /// Une séance de fumure.
    pub fn manure(&mut self) {
        self.richness = (self.richness + MANURE_GAIN * (SOIL_MAX + 0.1 - self.richness)).min(SOIL_MAX);
    }

    /// Faut-il encore fumer ce champ ?
    pub fn wants_manure(&self) -> bool {
        self.richness < SOIL_MAX - 0.05
    }
}

/// Gibier et poisson autour d'un village, 0–1 (1 : abondant, jamais chassé).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Wild {
    pub game: f32,
    pub fish: f32,
}

impl Default for Wild {
    fn default() -> Self {
        Wild { game: 1.0, fish: 1.0 }
    }
}

impl Wild {
    /// Ce qu'on prend fait baisser la ressource.
    pub fn take(&mut self, fish: bool, amount: f32) {
        let (level, stock) = if fish {
            (&mut self.fish, FISH_STOCK)
        } else {
            (&mut self.game, GAME_STOCK)
        };
        *level = (*level - amount / stock).max(WILD_MIN);
    }

    /// Une journée de repos : la ressource se reconstitue (vite à mi-hauteur, lentement près du
    /// plein ou quand il n'en reste presque rien).
    pub fn regrow(&mut self) {
        for level in [&mut self.game, &mut self.fish] {
            *level = (*level + WILD_REGROWTH * *level * (1.0 - *level) + 0.004).min(1.0);
        }
    }
}

/// Ce qui change d'une année à l'autre dans un village.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Year {
    /// Promesse des récoltes de l'année (printemps humide ou sec, gel tardif…), autour de 1.
    pub harvest: f32,
    /// Taille levée sur les foyers aisés depuis le printemps (sous).
    pub levied: f32,
    /// Aumônes distribuées aux foyers pauvres depuis le printemps (sous).
    pub alms: f32,
}

impl Default for Year {
    fn default() -> Self {
        Year {
            harvest: 1.0,
            levied: 0.0,
            alms: 0.0,
        }
    }
}

/// Part de l'argent d'un foyer au-delà du seuil que la taille prend chaque semaine, rapportée
/// à la taxe du marché (une taxe de 10 % prend 10 % de l'excédent par semaine).
pub const LEVY_RATE: f32 = 1.6;
/// Seuil de la taille : un foyer garde au moins ceci, plus un peu par bouche.
pub const LEVY_FLOOR: f32 = 30.0;
pub const LEVY_PER_HEAD: f32 = 6.0;
/// Réserve de la caisse (plus tant par habitant) : au-delà, la taille n'est plus levée et le
/// village fait l'aumône aux foyers les plus pauvres.
pub const RESERVE: f32 = 30.0;
pub const RESERVE_PER_HEAD: f32 = 5.0;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Land {
    /// Sol de chaque champ, par identifiant de champ (absent : terre neuve).
    pub soil: BTreeMap<u32, Soil>,
    /// Gibier et poisson, par village (même ordre que `World::villages`).
    pub wild: Vec<Wild>,
    /// Année en cours de chaque village (même ordre).
    pub years: Vec<Year>,
}

impl Land {
    pub fn soil(&self, field: u32) -> Soil {
        self.soil.get(&field).copied().unwrap_or_default()
    }

    pub fn soil_mut(&mut self, field: u32) -> &mut Soil {
        self.soil.entry(field).or_default()
    }

    pub fn wild(&self, vi: usize) -> Wild {
        self.wild.get(vi).copied().unwrap_or_default()
    }

    pub fn year(&self, vi: usize) -> Year {
        self.years.get(vi).copied().unwrap_or_default()
    }

    pub fn year_mut(&mut self, vi: usize) -> &mut Year {
        while self.years.len() <= vi {
            self.years.push(Year::default());
        }
        &mut self.years[vi]
    }

    pub fn wild_mut(&mut self, vi: usize) -> &mut Wild {
        while self.wild.len() <= vi {
            self.wild.push(Wild::default());
        }
        &mut self.wild[vi]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Chasser sans relâche vide les bois ; quelques semaines de repos les repeuplent.
    #[test]
    fn game_runs_out_and_comes_back() {
        let mut w = Wild::default();
        for _ in 0..20 {
            w.take(false, 15.0);
        }
        assert!(w.game < 0.2, "{}", w.game);
        assert_eq!(w.fish, 1.0);
        for _ in 0..40 {
            w.regrow();
        }
        assert!(w.game > 0.7, "{}", w.game);
    }

    /// Une moisson appauvrit le champ, la fumure le remonte (sans dépasser le maximum).
    #[test]
    fn manure_restores_soil() {
        let mut s = Soil {
            richness: 0.6,
            ..Default::default()
        };
        for _ in 0..60 {
            s.manure();
        }
        assert!(
            s.richness > SOIL_MAX - 0.05 && s.richness <= SOIL_MAX,
            "{}",
            s.richness
        );
        s.weeds = 1.0;
        assert!(s.yield_factor() < 0.5 * s.richness);
    }
}

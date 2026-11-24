//! Mode dieu (phase 8) : jauge de foi, pouvoirs, journal des actes, religion des villages.

use hameaux_core::{
    World,
    god::{Cult, Deed, POWERS},
    settlement::HouseState,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{PersonRef, VillageRef};

/// La foi du joueur (envoyée avec chaque `Tick`).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct GodGauge {
    pub faith: f32,
    pub max: f32,
    /// Foi rendue par jour de jeu.
    pub regen: f32,
    /// Tick du dernier signe dans le ciel (0 : jamais).
    #[ts(type = "number")]
    pub sign: u64,
    /// Nombre d'actes (le navigateur recharge le journal quand il change).
    pub deeds: u32,
}

impl GodGauge {
    pub fn of(world: &World) -> Self {
        let g = &world.god;
        GodGauge {
            faith: g.faith,
            max: g.max_faith(),
            regen: g.regen,
            sign: g.sign,
            deeds: g.stats.commands,
        }
    }
}

/// Un pouvoir, tel que le propose la palette.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PowerView {
    /// Valeur de `power` dans la commande JSON.
    pub key: String,
    pub label: String,
    pub help: String,
    pub cost: f32,
    pub category: String,
    /// `world`, `village`, `person`, `two_people`, `two_villages` ou `tile`.
    pub target: String,
    pub benign: bool,
}

/// Un acte du joueur.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DeedView {
    #[ts(type = "number")]
    pub tick: u64,
    pub date: String,
    pub power: String,
    pub cost: f32,
    pub text: String,
    pub benign: bool,
}

impl DeedView {
    pub fn of(d: &Deed) -> Self {
        let p = d.command.power();
        DeedView {
            tick: d.tick,
            date: {
                let c = hameaux_core::Clock::at(d.tick);
                format!(
                    "An {}, {} j{} {:02}:{:02}",
                    c.year,
                    c.season.label(),
                    c.day,
                    c.hour,
                    c.minute
                )
            },
            power: p.label.to_string(),
            cost: d.cost,
            text: d.text.clone(),
            benign: p.benign,
        }
    }
}

/// La religion d'un village.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CultView {
    pub village: VillageRef,
    /// « le culte du Ciel clément » (vide : on n'y prie pas encore).
    pub name: String,
    /// « fondé au printemps de l'an 3 ».
    pub founded: Option<String>,
    /// « te vénère », « te craint »…
    pub attitude: String,
    pub believers: u32,
    pub population: u32,
    /// Croyance moyenne des habitants (0–1).
    pub devotion: f32,
    pub love: f32,
    pub fear: f32,
    pub miracles: u32,
    pub scourges: u32,
    pub priest: Option<PersonRef>,
    /// « en chantier (40 %) », « bâti », ou rien.
    pub temple: Option<String>,
    /// Effets du Ciel en cours (« pluie bienfaisante, encore 1 j 6 h »).
    pub spells: Vec<String>,
}

impl CultView {
    pub fn of(world: &World, village: u32) -> Option<Self> {
        let vi = world.village_index(village)?;
        let v = &world.villages[vi];
        let empty = Cult::default();
        let c = world.god.cult(village).unwrap_or(&empty);
        let temple = c.temple.and_then(|t| v.house(t)).map(|h| match h.state {
            HouseState::Building => format!("en chantier ({:.0} %)", h.progress() * 100.0),
            HouseState::Inhabited => "bâti".to_string(),
            _ => "à l'abandon".to_string(),
        });
        Some(CultView {
            village: VillageRef {
                id: v.id,
                name: v.name.clone(),
            },
            name: if c.founded.is_some() {
                c.name.clone()
            } else {
                String::new()
            },
            founded: c.founded.map(|t| {
                let k = hameaux_core::Clock::at(t);
                let when = match k.season {
                    hameaux_core::Season::Spring => "au printemps".to_string(),
                    s => format!("en {}", s.label().to_lowercase()),
                };
                format!("né {when} de l'an {}", k.year)
            }),
            attitude: c.attitude().to_string(),
            believers: c.believers,
            population: world.population(village) as u32,
            devotion: c.devotion,
            love: c.love,
            fear: c.fear,
            miracles: c.miracles,
            scourges: c.scourges,
            priest: c.priest.map(|p| {
                let q = &world.people[p as usize];
                crate::person_ref(world, p, if q.female { "prêtresse" } else { "prêtre" })
            }),
            temple,
            spells: spells(world, village),
        })
    }
}

/// Effets en cours sur un village, en mots.
pub fn spells(world: &World, village: u32) -> Vec<String> {
    world
        .god
        .spells
        .iter()
        .filter(|s| s.village == village && s.until > world.tick)
        .map(|s| {
            let left = s.until - world.tick;
            let (d, h) = (
                left / hameaux_core::tuning::DAY,
                (left % hameaux_core::tuning::DAY) / hameaux_core::tuning::HOUR,
            );
            let left = match (d, h) {
                (0, 0) => "presque fini".to_string(),
                (0, h) => format!("encore {h} h"),
                (d, 0) => format!("encore {d} j"),
                (d, h) => format!("encore {d} j {h} h"),
            };
            format!("{}, {left}", s.kind.label())
        })
        .collect()
}

/// Réponse de `GET /api/god` : tout ce qu'il faut à la palette et à la page du Ciel.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct GodView {
    pub gauge: GodGauge,
    pub powers: Vec<PowerView>,
    /// Les derniers actes, du plus récent au plus ancien.
    pub deeds: Vec<DeedView>,
    /// Les villages où l'on croit (ou qui ont vu passer le Ciel), les plus fervents d'abord.
    pub cults: Vec<CultView>,
    pub believers: u32,
    pub population: u32,
    pub prayers: u32,
    pub temples: u32,
    /// Maladies (index de la commande `plague`) et ressources (`deposit`).
    pub diseases: Vec<crate::Labeled>,
    pub resources: Vec<crate::Labeled>,
}

impl GodView {
    pub fn of(world: &World) -> Self {
        let mut cults: Vec<CultView> = world
            .god
            .cults
            .keys()
            .filter_map(|&v| CultView::of(world, v))
            .collect();
        cults.sort_by(|a, b| b.devotion.total_cmp(&a.devotion));
        let population = world.alive().filter(|p| !p.brain.leaving).count() as u32;
        let believers = world
            .alive()
            .filter(|p| world.god.belief(p.id) >= hameaux_core::god::BELIEVER)
            .count() as u32;
        let temples = world
            .god
            .cults
            .iter()
            .filter(|(v, c)| {
                c.temple
                    .and_then(|t| {
                        world
                            .village_index(**v)
                            .and_then(|vi| world.villages[vi].house(t))
                    })
                    .is_some_and(|h| h.state == HouseState::Inhabited)
            })
            .count() as u32;
        GodView {
            gauge: GodGauge::of(world),
            powers: POWERS
                .iter()
                .map(|p| PowerView {
                    key: p.key.into(),
                    label: p.label.into(),
                    help: p.help.into(),
                    cost: p.cost,
                    category: p.category.into(),
                    target: p.target.key().into(),
                    benign: p.benign,
                })
                .collect(),
            deeds: world.god.deeds.iter().rev().take(60).map(DeedView::of).collect(),
            cults,
            believers,
            population,
            prayers: world.god.stats.prayers.min(u32::MAX as u64) as u32,
            temples,
            diseases: hameaux_core::society::Disease::ALL
                .iter()
                .map(|d| crate::Labeled {
                    id: *d as u8,
                    label: d.label().into(),
                })
                .collect(),
            resources: hameaux_core::Resource::ALL
                .iter()
                .filter(|r| **r != hameaux_core::Resource::None)
                .map(|r| crate::Labeled {
                    id: *r as u8,
                    label: r.label().into(),
                })
                .collect(),
        }
    }
}

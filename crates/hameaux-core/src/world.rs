//! État complet du monde.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::{
    clock::Clock,
    events::Chronicle,
    map::{Biome, CHUNK, Map, flags},
    path::PathFinder,
    people::Person,
    rng::Rng,
    settlement::Village,
};

/// Emplacement d'un village choisi par le générateur, avant sa fondation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Site {
    pub name: String,
    pub x: u32,
    pub y: u32,
    pub color: u32,
}

/// Occupation d'une case (reconstruite au chargement, jamais sauvegardée).
pub mod occ {
    pub const NONE: u8 = 0;
    pub const HOUSE: u8 = 1;
    pub const FIELD: u8 = 2;
    pub const CIVIC: u8 = 3;
}

#[derive(Debug, Default)]
pub struct Caches {
    pub occ: Vec<u8>,
    pub pf: PathFinder,
    pub ready: bool,
}

/// Cache attaché au monde : ni comparé, ni copié, ni sauvegardé.
#[derive(Debug, Default)]
pub struct Scratch<T>(pub T);

impl<T: Default> Clone for Scratch<T> {
    fn clone(&self) -> Self {
        Scratch(T::default())
    }
}

impl<T> PartialEq for Scratch<T> {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct World {
    /// Graine du monde : même graine + mêmes commandes = même histoire.
    pub seed: u64,
    /// Nombre de ticks écoulés depuis la création.
    pub tick: u64,
    /// Partagée : l'instantané publié ne recopie pas la carte. Modifier via [`World::edit_tile`].
    pub map: Arc<Map>,
    pub villages: Vec<Village>,
    /// Tous les habitants, vivants ou non (l'identifiant est l'index).
    pub people: Vec<Person>,
    pub chronicle: Chronicle,
    pub rng: Rng,
    pub next_building: u32,
    /// Révision de chaque tronçon de carte (pour que le navigateur recharge ce qui a changé).
    pub chunk_rev: Vec<u64>,
    pub map_rev: u64,
    #[serde(skip)]
    pub caches: Scratch<Caches>,
}

impl World {
    /// Monde vide (sans carte) : utile aux tests.
    pub fn empty(seed: u64) -> Self {
        Self::from_map(seed, Map::new(0, 0), &[])
    }

    /// Monde neuf sur une carte générée : fonde un village sur chaque site.
    pub fn from_map(seed: u64, map: Map, sites: &[Site]) -> Self {
        Self::from_map_at(seed, map, sites, 0)
    }

    /// Comme [`World::from_map`], mais fondé au tick donné (reprise d'une ancienne sauvegarde).
    pub fn from_map_at(seed: u64, map: Map, sites: &[Site], tick: u64) -> Self {
        let chunks = (map.chunks_x() * map.chunks_y()) as usize;
        let mut w = World {
            seed,
            tick,
            map: Arc::new(map),
            villages: Vec::new(),
            people: Vec::new(),
            chronicle: Chronicle::default(),
            rng: Rng::new(seed ^ 0x6A09_E667_F3BC_C908),
            next_building: 1,
            chunk_rev: vec![0; chunks],
            map_rev: 0,
            caches: Scratch::default(),
        };
        w.ensure_caches();
        for site in sites {
            w.found_village(site);
        }
        w
    }

    pub fn clock(&self) -> Clock {
        Clock::at(self.tick)
    }

    /// Prépare les caches (occupation des cases, chercheur de chemin) et remet les habitants
    /// dans un état cohérent après un chargement.
    pub fn ensure_caches(&mut self) {
        if self.caches.0.ready {
            return;
        }
        let mut grid = vec![occ::NONE; self.map.len()];
        for v in &self.villages {
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                grid[self.map.idx(v.x + dx, v.y + dy)] = occ::CIVIC;
            }
            grid[self.map.idx(v.well.0, v.well.1)] = occ::CIVIC;
            for h in &v.houses {
                grid[self.map.idx(h.x, h.y)] = occ::HOUSE;
            }
            for f in &v.fields {
                for dy in 0..crate::tuning::FIELD_SIZE {
                    for dx in 0..crate::tuning::FIELD_SIZE {
                        grid[self.map.idx(f.x + dx, f.y + dy)] = occ::FIELD;
                    }
                }
            }
        }
        self.caches.0.occ = grid;
        self.caches.0.ready = true;
        self.after_load();
    }

    pub fn occ(&self, i: usize) -> u8 {
        self.caches.0.occ.get(i).copied().unwrap_or(occ::NONE)
    }

    pub(crate) fn set_occ(&mut self, i: usize, v: u8) {
        if let Some(c) = self.caches.0.occ.get_mut(i) {
            *c = v;
        }
    }

    /// Modifie une case de la carte et note le tronçon comme changé.
    pub fn edit_tile(&mut self, i: usize, f: impl FnOnce(&mut Map, usize)) {
        f(Arc::make_mut(&mut self.map), i);
        let (x, y) = self.map.xy(i);
        let c = ((y / CHUNK) * self.map.chunks_x() + x / CHUNK) as usize;
        self.map_rev += 1;
        if let Some(r) = self.chunk_rev.get_mut(c) {
            *r = self.map_rev;
        }
    }

    pub fn set_biome(&mut self, i: usize, b: Biome) {
        if self.map.biome[i] != b as u8 {
            self.edit_tile(i, |m, i| m.biome[i] = b as u8);
        }
    }

    pub fn set_road(&mut self, i: usize) {
        if !self.map.road_at(i) {
            self.edit_tile(i, |m, i| m.flags[i] |= flags::ROAD);
        }
    }

    pub fn village_index(&self, id: u32) -> Option<usize> {
        self.villages.iter().position(|v| v.id == id)
    }

    pub fn alive(&self) -> impl Iterator<Item = &Person> {
        self.people.iter().filter(|p| p.alive())
    }

    pub fn population(&self, village: u32) -> usize {
        self.alive()
            .filter(|p| p.village == village && !p.brain.leaving)
            .count()
    }
}

//! Messages échangés entre le serveur et le navigateur (JSON).
//!
//! Les types TypeScript correspondants sont générés dans `web/src/protocol/` par
//! `cargo test -p hameaux-protocol` : ne pas les modifier à la main.

use hameaux_core::{Biome, Clock, Map, Resource, World, map::CHUNK, settlement::Village};

mod views;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
pub use views::*;

/// Date du monde, prête à afficher.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ClockView {
    #[ts(type = "number")]
    pub tick: u64,
    #[ts(type = "number")]
    pub year: u64,
    /// 0 printemps, 1 été, 2 automne, 3 hiver.
    pub season: u8,
    pub season_label: String,
    #[ts(type = "number")]
    pub day: u64,
    #[ts(type = "number")]
    pub hour: u64,
    #[ts(type = "number")]
    pub minute: u64,
    /// « An 1, Printemps jour 3, 14:20 ».
    pub label: String,
}

impl ClockView {
    pub fn of(world: &World) -> Self {
        let c: Clock = world.clock();
        ClockView {
            tick: world.tick,
            year: c.year,
            season: c.season as u8,
            season_label: c.season.label().to_string(),
            day: c.day,
            hour: c.hour,
            minute: c.minute,
            label: c.to_string(),
        }
    }
}

/// État de la boucle de simulation.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RunState {
    /// Secondes de jeu par seconde réelle (60 = un jour de jeu en 24 min).
    pub speed: f64,
    pub paused: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "type")]
#[ts(export)]
pub enum ServerMsg {
    /// Premier message après la connexion.
    Hello {
        server_version: String,
        #[ts(type = "number")]
        seed: u64,
        run: RunState,
        clock: ClockView,
        #[ts(type = "number")]
        map_rev: u64,
        #[ts(type = "number")]
        events_seq: u64,
    },
    /// Le monde a avancé (envoyé au plus quelques fois par seconde). `map_rev` change quand
    /// des cases de la carte changent (voir `/api/map/changes`), `events_seq` quand la
    /// chronique s'allonge (voir `/api/events`).
    Tick {
        clock: ClockView,
        run: RunState,
        #[ts(type = "number")]
        map_rev: u64,
        #[ts(type = "number")]
        events_seq: u64,
    },
    /// Ce qui se trouve dans la zone regardée (après un `View` avec `detail`).
    Scene(Scene),
    /// Résumé de chaque village (toutes les quelques secondes).
    Villages { villages: Vec<VillageView> },
}

/// Messages du navigateur.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "type")]
#[ts(export)]
pub enum ClientMsg {
    /// Zone de la carte affichée (cases). `detail` : envoyer habitants et bâtiments.
    View {
        x0: i32,
        y0: i32,
        x1: i32,
        y1: i32,
        detail: bool,
    },
}

/// Réponse de `GET /health`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Health {
    pub status: String,
    #[ts(type = "number")]
    pub tick: u64,
    pub run: RunState,
}

/// Un village vu de la carte.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct VillageView {
    pub id: u32,
    pub name: String,
    /// Grenier (centre du village).
    pub x: u32,
    pub y: u32,
    /// Couleur RVB (`0xRRGGBB`).
    pub color: u32,
    pub population: u32,
}

impl VillageView {
    pub fn of(world: &World, v: &Village) -> Self {
        VillageView {
            id: v.id,
            name: v.name.clone(),
            x: v.x,
            y: v.y,
            color: v.color,
            population: world.population(v.id) as u32,
        }
    }
}

/// Une valeur d'énumération et son libellé (milieux, ressources).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Labeled {
    pub id: u8,
    pub label: String,
}

/// Réponse de `GET /api/map` : tout ce qu'il faut pour demander et lire les tronçons.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MapInfo {
    pub width: u32,
    pub height: u32,
    /// Côté d'un tronçon (cases). Tronçons : `GET /api/map/chunk/{cx}/{cy}`.
    pub chunk_size: u32,
    pub chunks_x: u32,
    pub chunks_y: u32,
    pub sea_level: u8,
    /// Ordre des couches dans un tronçon (voir [`encode_chunk`]).
    pub chunk_layers: Vec<String>,
    pub biomes: Vec<Labeled>,
    pub resources: Vec<Labeled>,
    pub villages: Vec<VillageView>,
}

/// Couches envoyées pour chaque tronçon, dans cet ordre.
pub const CHUNK_LAYERS: [&str; 6] = ["elevation", "biome", "flags", "resource", "fertility", "shade"];

impl MapInfo {
    pub fn of(world: &World) -> Self {
        let m = &world.map;
        MapInfo {
            width: m.width,
            height: m.height,
            chunk_size: CHUNK,
            chunks_x: m.chunks_x(),
            chunks_y: m.chunks_y(),
            sea_level: hameaux_core::map::SEA_LEVEL,
            chunk_layers: CHUNK_LAYERS.iter().map(|s| s.to_string()).collect(),
            biomes: Biome::ALL
                .iter()
                .map(|&b| Labeled {
                    id: b as u8,
                    label: b.label().into(),
                })
                .collect(),
            resources: Resource::ALL
                .iter()
                .map(|&r| Labeled {
                    id: r as u8,
                    label: r.label().into(),
                })
                .collect(),
            villages: world.villages.iter().map(|v| VillageView::of(world, v)).collect(),
        }
    }
}

/// Tronçon `(cx, cy)` en binaire : les couches de [`CHUNK_LAYERS`] l'une après l'autre,
/// chacune de `CHUNK × CHUNK` octets ligne par ligne. Les cases hors carte valent 0.
/// `None` si le tronçon est hors carte.
pub fn encode_chunk(map: &Map, cx: u32, cy: u32) -> Option<Vec<u8>> {
    if cx >= map.chunks_x() || cy >= map.chunks_y() {
        return None;
    }
    let side = CHUNK as usize;
    let mut out = vec![0u8; side * side * CHUNK_LAYERS.len()];
    let layers: [&[u8]; 6] = [
        &map.elevation,
        &map.biome,
        &map.flags,
        &map.resource,
        &map.fertility,
        &map.shade,
    ];
    for (l, layer) in layers.iter().enumerate() {
        let base = l * side * side;
        for dy in 0..CHUNK {
            let y = cy * CHUNK + dy;
            if y >= map.height {
                break;
            }
            let x0 = cx * CHUNK;
            let x1 = (x0 + CHUNK).min(map.width);
            let src = &layer[map.idx(x0, y)..map.idx(x0, y) + (x1 - x0) as usize];
            let dst = base + dy as usize * side;
            out[dst..dst + src.len()].copy_from_slice(src);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_layout() {
        // Carte de 100 × 70 : 2 × 2 tronçons, le dernier partiel.
        let mut map = Map::new(100, 70);
        let i = map.idx(65, 66);
        map.biome[i] = 7;
        map.shade[i] = 200;
        let c = encode_chunk(&map, 1, 1).unwrap();
        let side = CHUNK as usize;
        let at = |layer: usize, x: usize, y: usize| c[layer * side * side + y * side + x];
        assert_eq!(c.len(), side * side * 6);
        assert_eq!(at(1, 1, 2), 7);
        assert_eq!(at(5, 1, 2), 200);
        assert_eq!(at(5, 40, 2), 0, "hors carte");
        assert!(encode_chunk(&map, 2, 0).is_none());
    }
}

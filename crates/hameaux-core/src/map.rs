//! Carte du monde : une grille de cases découpée en tronçons de [`CHUNK`] × [`CHUNK`].
//!
//! Chaque couche est un tableau d'octets ligne par ligne (`y * width + x`), compact à
//! sauvegarder et à envoyer au navigateur.

use serde::{Deserialize, Serialize};

/// Côté d'un tronçon, en cases.
pub const CHUNK: u32 = 64;
/// Altitude du niveau de la mer (sur 0–255). En dessous : mer.
pub const SEA_LEVEL: u8 = 80;

/// Bits de [`Map::flags`].
pub mod flags {
    /// Taille de la rivière sur 2 bits : 0 aucune, 1 ruisseau, 2 rivière, 3 fleuve.
    pub const RIVER_MASK: u8 = 0b11;
    /// Chemin tracé par les habitants (sur une rivière : un pont).
    pub const ROAD: u8 = 0b100;
}

/// Milieu naturel d'une case. L'ordre (et donc la valeur) est partagé avec le client web.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum Biome {
    DeepSea = 0,
    Sea = 1,
    Lake = 2,
    Beach = 3,
    Grassland = 4,
    Meadow = 5,
    Forest = 6,
    Conifers = 7,
    Marsh = 8,
    Heath = 9,
    Hills = 10,
    Mountain = 11,
    Snow = 12,
}

impl Biome {
    pub const ALL: [Biome; 13] = [
        Biome::DeepSea,
        Biome::Sea,
        Biome::Lake,
        Biome::Beach,
        Biome::Grassland,
        Biome::Meadow,
        Biome::Forest,
        Biome::Conifers,
        Biome::Marsh,
        Biome::Heath,
        Biome::Hills,
        Biome::Mountain,
        Biome::Snow,
    ];

    pub fn from_u8(v: u8) -> Biome {
        Biome::ALL.get(v as usize).copied().unwrap_or(Biome::Grassland)
    }

    pub fn is_water(self) -> bool {
        matches!(self, Biome::DeepSea | Biome::Sea | Biome::Lake)
    }

    pub fn label(self) -> &'static str {
        match self {
            Biome::DeepSea => "Mer profonde",
            Biome::Sea => "Mer",
            Biome::Lake => "Lac",
            Biome::Beach => "Plage",
            Biome::Grassland => "Prairie sèche",
            Biome::Meadow => "Prairie",
            Biome::Forest => "Forêt",
            Biome::Conifers => "Forêt de conifères",
            Biome::Marsh => "Marais",
            Biome::Heath => "Lande",
            Biome::Hills => "Collines",
            Biome::Mountain => "Montagne",
            Biome::Snow => "Neiges éternelles",
        }
    }
}

/// Ressource notable d'une case (une au plus).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum Resource {
    None = 0,
    Stone = 1,
    Iron = 2,
    Clay = 3,
    Fish = 4,
    Game = 5,
    Berries = 6,
}

impl Resource {
    pub const ALL: [Resource; 7] = [
        Resource::None,
        Resource::Stone,
        Resource::Iron,
        Resource::Clay,
        Resource::Fish,
        Resource::Game,
        Resource::Berries,
    ];

    pub fn from_u8(v: u8) -> Resource {
        Resource::ALL.get(v as usize).copied().unwrap_or(Resource::None)
    }

    pub fn label(self) -> &'static str {
        match self {
            Resource::None => "",
            Resource::Stone => "Pierre",
            Resource::Iron => "Minerai de fer",
            Resource::Clay => "Argile",
            Resource::Fish => "Poissons",
            Resource::Game => "Gibier",
            Resource::Berries => "Baies",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Map {
    pub width: u32,
    pub height: u32,
    /// Altitude 0–255 ([`SEA_LEVEL`] = rivage).
    pub elevation: Vec<u8>,
    /// [`Biome`] en `u8`.
    pub biome: Vec<u8>,
    /// Voir [`flags`].
    pub flags: Vec<u8>,
    /// [`Resource`] en `u8`.
    pub resource: Vec<u8>,
    /// Qualité des terres pour l'agriculture, 0–255.
    pub fertility: Vec<u8>,
    /// Ombrage du relief pour l'affichage (128 = plat, plus clair face au nord-ouest).
    pub shade: Vec<u8>,
}

impl Map {
    pub fn new(width: u32, height: u32) -> Self {
        let n = (width * height) as usize;
        Map {
            width,
            height,
            elevation: vec![0; n],
            biome: vec![Biome::DeepSea as u8; n],
            flags: vec![0; n],
            resource: vec![0; n],
            fertility: vec![0; n],
            shade: vec![128; n],
        }
    }

    #[inline]
    pub fn idx(&self, x: u32, y: u32) -> usize {
        (y * self.width + x) as usize
    }

    pub fn len(&self) -> usize {
        (self.width * self.height) as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn biome_at(&self, i: usize) -> Biome {
        Biome::from_u8(self.biome[i])
    }

    pub fn river_at(&self, i: usize) -> u8 {
        self.flags[i] & flags::RIVER_MASK
    }

    pub fn chunks_x(&self) -> u32 {
        self.width.div_ceil(CHUNK)
    }

    pub fn chunks_y(&self) -> u32 {
        self.height.div_ceil(CHUNK)
    }

    pub fn road_at(&self, i: usize) -> bool {
        self.flags[i] & flags::ROAD != 0
    }

    pub fn xy(&self, i: usize) -> (u32, u32) {
        (i as u32 % self.width, i as u32 / self.width)
    }

    pub fn inside(&self, x: i64, y: i64) -> bool {
        x >= 0 && y >= 0 && x < self.width as i64 && y < self.height as i64
    }

    /// Peut-on construire (maison, champ) sur cette case ?
    pub fn buildable(&self, i: usize) -> bool {
        let b = self.biome_at(i);
        !b.is_water()
            && self.river_at(i) == 0
            && !matches!(b, Biome::Mountain | Biome::Snow | Biome::Marsh | Biome::Beach)
    }

    /// Coût de passage d'une case (`None` = infranchissable).
    pub fn walk_cost(&self, i: usize) -> Option<f32> {
        if self.road_at(i) {
            return Some(0.6);
        }
        let b = self.biome_at(i);
        if b.is_water() {
            return None;
        }
        let base = match b {
            Biome::Forest | Biome::Conifers | Biome::Hills => 1.6,
            Biome::Marsh => 2.5,
            Biome::Mountain => 4.0,
            Biome::Snow => 7.0,
            _ => 1.0,
        };
        Some(if self.river_at(i) > 0 { base + 3.0 } else { base })
    }
}

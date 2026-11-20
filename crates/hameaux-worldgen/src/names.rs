//! Noms et couleurs des villages.

use hameaux_core::{Biome, Map, map::SEA_LEVEL};

use hameaux_core::rng::Rng;

/// Couleurs de village (RVB), bien distinctes entre elles et sur la carte.
pub const COLORS: [u32; 12] = [
    0xe4572e, 0x2e86ab, 0xf3a712, 0x8e44ad, 0xc0392b, 0x16a085, 0xd35400, 0x2c3e50, 0xe84393, 0x6c5ce7,
    0x00a8a8, 0xb8860b,
];

const BASES: &[&str] = &[
    "Clairval",
    "Beaupré",
    "Montfleury",
    "Roquebrune",
    "Vauclair",
    "Belcastel",
    "Fontclaire",
    "Hautrive",
    "Brumeval",
    "Chênedor",
    "Pré-Lilas",
    "Valjoli",
    "Boisrond",
    "Saint-Aubin",
    "Malemort",
    "Bellerive",
    "Rocheblanche",
    "Combefroide",
    "Ormesson",
    "Les Saules",
    "La Grange",
    "Fontenelle",
    "Mirebeau",
    "Aiguebelle",
    "Champfleuri",
    "Corbières",
    "Vieux-Moulin",
    "Pierrefeu",
    "Bourg-Menthe",
    "Sainte-Colombe",
    "Les Trois-Puits",
    "Herbeval",
    "Chantemerle",
    "Rivedoux",
    "Montaigu",
    "Lavalette",
    "Noirmoutier",
    "Glanville",
    "Ronceval",
    "Belair",
    "Froidefond",
    "Sauvebonne",
    "Cressonnière",
    "L'Aubépine",
    "Mortevieille",
    "Pommeraie",
];

/// Ce qui entoure un village : sert à ajouter « -sur-Mer », « -les-Bois »…
pub struct Surroundings {
    coast: bool,
    forest: bool,
    high: bool,
    marsh: bool,
}

impl Surroundings {
    pub fn look(map: &Map, x: u32, y: u32, near_ocean: &[u8]) -> Self {
        let r = 8i64;
        let (mut total, mut forest, mut marsh) = (0, 0, 0);
        for dy in -r..=r {
            for dx in -r..=r {
                let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                if nx < 0 || ny < 0 || nx >= map.width as i64 || ny >= map.height as i64 {
                    continue;
                }
                total += 1;
                match map.biome_at(map.idx(nx as u32, ny as u32)) {
                    Biome::Forest | Biome::Conifers => forest += 1,
                    Biome::Marsh => marsh += 1,
                    _ => {}
                }
            }
        }
        let i = map.idx(x, y);
        Surroundings {
            coast: near_ocean[i] <= 6,
            forest: forest * 10 > total * 4,
            high: map.elevation[i] > SEA_LEVEL + 60,
            marsh: marsh * 10 > total,
        }
    }
}

pub struct Namer {
    pool: Vec<&'static str>,
    used: Vec<String>,
}

impl Namer {
    pub fn new(rng: &mut Rng) -> Self {
        let mut pool = BASES.to_vec();
        // Mélange de Fisher-Yates.
        for i in (1..pool.len()).rev() {
            pool.swap(i, rng.below(i as u64 + 1) as usize);
        }
        Namer {
            pool,
            used: Vec::new(),
        }
    }

    pub fn next(&mut self, rng: &mut Rng, s: &Surroundings) -> String {
        let base = self
            .pool
            .pop()
            .map(str::to_string)
            .unwrap_or_else(|| format!("Hameau {}", self.used.len() + 1));
        let suffix = if s.coast {
            "-sur-Mer"
        } else if s.marsh && rng.below(2) == 0 {
            "-les-Marais"
        } else if s.forest && rng.below(2) == 0 {
            "-les-Bois"
        } else if s.high && rng.below(2) == 0 {
            "-le-Haut"
        } else {
            ""
        };
        // Pas de « Les Saules-sur-Mer » : les noms à article restent tels quels.
        let name = if base.starts_with("Les ") || base.starts_with("La ") || base.starts_with("L'") {
            base
        } else {
            format!("{base}{suffix}")
        };
        self.used.push(name.clone());
        name
    }
}

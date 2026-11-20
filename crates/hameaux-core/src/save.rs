//! Format des sauvegardes : `HAMX` + version (u32 LE) + zstd(postcard(World)).
//!
//! Chaque changement incompatible du monde incrémente [`SAVE_VERSION`]. Les anciennes
//! versions restent lisibles : [`decode`] renvoie ce qu'il sait en tirer et le serveur
//! termine la migration (par exemple en régénérant la carte depuis la graine).

use serde::Deserialize;

use crate::{
    map::Map,
    world::{Site, World},
};

pub const MAGIC: &[u8; 4] = b"HAMX";
/// 1 : graine + tick. 2 : + carte et emplacements des villages. 3 : + habitants et bâtiments.
pub const SAVE_VERSION: u32 = 3;

#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error("ce n'est pas une sauvegarde Hameaux")]
    BadMagic,
    #[error("version de sauvegarde {0} inconnue (ce serveur lit jusqu'à la version {SAVE_VERSION})")]
    UnknownVersion(u32),
    #[error("décompression : {0}")]
    Io(#[from] std::io::Error),
    #[error("décodage : {0}")]
    Decode(#[from] postcard::Error),
}

/// Résultat de la lecture d'une sauvegarde.
#[derive(Debug)]
pub enum Loaded {
    Current(World),
    /// Version 1 (phase 0) : pas de carte. Le serveur régénère le monde depuis la graine
    /// et reprend au même tick.
    V1 {
        seed: u64,
        tick: u64,
    },
    /// Version 2 (phase 1) : carte et emplacements, pas encore d'habitants. Le serveur fonde
    /// les villages sur la même carte et reprend au même tick.
    V2 {
        seed: u64,
        tick: u64,
        map: Map,
        sites: Vec<Site>,
    },
}

#[derive(Deserialize)]
struct WorldV1 {
    seed: u64,
    tick: u64,
}

#[derive(Deserialize)]
struct VillageV2 {
    #[allow(dead_code)]
    id: u32,
    name: String,
    x: u32,
    y: u32,
    color: u32,
}

#[derive(Deserialize)]
struct WorldV2 {
    seed: u64,
    tick: u64,
    map: Map,
    villages: Vec<VillageV2>,
}

pub fn encode(world: &World) -> Result<Vec<u8>, SaveError> {
    let raw = postcard::to_stdvec(world)?;
    let mut out = Vec::with_capacity(raw.len() / 4 + 8);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&SAVE_VERSION.to_le_bytes());
    out.extend_from_slice(&zstd::encode_all(&raw[..], 3)?);
    Ok(out)
}

pub fn decode(bytes: &[u8]) -> Result<Loaded, SaveError> {
    if bytes.len() < 8 || &bytes[..4] != MAGIC {
        return Err(SaveError::BadMagic);
    }
    let version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    if version == 0 || version > SAVE_VERSION {
        return Err(SaveError::UnknownVersion(version));
    }
    let raw = zstd::decode_all(&bytes[8..])?;
    match version {
        1 => {
            let w: WorldV1 = postcard::from_bytes(&raw)?;
            Ok(Loaded::V1 {
                seed: w.seed,
                tick: w.tick,
            })
        }
        2 => {
            let w: WorldV2 = postcard::from_bytes(&raw)?;
            let sites = w
                .villages
                .into_iter()
                .map(|v| Site {
                    name: v.name,
                    x: v.x,
                    y: v.y,
                    color: v.color,
                })
                .collect();
            Ok(Loaded::V2 {
                seed: w.seed,
                tick: w.tick,
                map: w.map,
                sites,
            })
        }
        _ => {
            let mut w: World = postcard::from_bytes(&raw)?;
            w.ensure_caches();
            Ok(Loaded::Current(w))
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::map::{Biome, Map};

    /// Petit monde de prairie avec un village, avancé de 1 000 ticks.
    pub fn sample() -> World {
        let mut map = Map::new(80, 80);
        for i in 0..map.len() {
            map.biome[i] = Biome::Meadow as u8;
            map.elevation[i] = 100;
            map.fertility[i] = 200;
        }
        let site = Site {
            name: "Clairval".into(),
            x: 40,
            y: 40,
            color: 0xff8800,
        };
        let mut w = World::from_map(42, map, &[site]);
        for _ in 0..1000 {
            w.step();
        }
        w
    }

    #[test]
    fn round_trip() {
        let w = sample();
        match decode(&encode(&w).unwrap()).unwrap() {
            Loaded::Current(back) => {
                assert_eq!(back.tick, w.tick);
                assert_eq!(back.villages, w.villages);
                assert_eq!(back.people.len(), w.people.len());
                assert_eq!(back.people[0].name(), w.people[0].name());
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn reads_v1() {
        let raw = postcard::to_stdvec(&(7u64, 1234u64)).unwrap();
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&zstd::encode_all(&raw[..], 3).unwrap());
        assert!(matches!(
            decode(&bytes).unwrap(),
            Loaded::V1 { seed: 7, tick: 1234 }
        ));
    }

    #[test]
    fn reads_v2() {
        // Monde v2 : graine, tick, carte, villages (id, nom, x, y, couleur).
        let map = Map::new(4, 4);
        let raw = postcard::to_stdvec(&(
            9u64,
            50u64,
            &map,
            vec![(0u32, "Vaux".to_string(), 1u32, 2u32, 0xffu32)],
        ))
        .unwrap();
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&zstd::encode_all(&raw[..], 3).unwrap());
        match decode(&bytes).unwrap() {
            Loaded::V2 {
                seed: 9,
                tick: 50,
                map: m,
                sites,
            } => {
                assert_eq!(m, map);
                assert_eq!(sites[0].name, "Vaux");
                assert_eq!((sites[0].x, sites[0].y), (1, 2));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn rejects_garbage_and_future_versions() {
        assert!(matches!(decode(b"hello world"), Err(SaveError::BadMagic)));
        let mut bytes = encode(&sample()).unwrap();
        bytes[4..8].copy_from_slice(&99u32.to_le_bytes());
        assert!(matches!(decode(&bytes), Err(SaveError::UnknownVersion(99))));
    }
}

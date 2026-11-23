//! Format des sauvegardes : `HAMX` + version (u32 LE) + zstd(postcard(World)).
//!
//! Chaque changement incompatible du monde incrémente [`SAVE_VERSION`]. Les anciennes
//! versions restent lisibles : [`decode`] renvoie ce qu'il sait en tirer et le serveur
//! termine la migration (par exemple en régénérant la carte depuis la graine).

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::{
    map::Map,
    world::{Site, World},
};

pub const MAGIC: &[u8; 4] = b"HAMX";
/// 1 : graine + tick. 2 : + carte et emplacements des villages. 3 : + habitants et bâtiments.
/// 4 : + économie (foyers, marchés, ateliers), ajoutée à la fin du monde.
/// 5 : + esprit de chaque habitant (personnalité, besoins, souvenirs, relations…), qui
/// remplace ses amitiés.
/// 6 : + commerce (marchands, routes, nouvelles), ajouté à la fin ; tableaux par métier de
/// longueur libre (un nouveau métier ne casse plus les sauvegardes).
/// 7 : + société (chefs, justice, fêtes, maladies, savoirs), ajoutée à la fin.
/// 8 : + histoire (relevés quotidiens des villages, flux du commerce, annales), à la fin.
pub const SAVE_VERSION: u32 = 8;

/// Métiers connus jusqu'à la version 5.
const JOBS_V5: usize = 10;

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
#[allow(clippy::large_enum_variant)]
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

/// Habitant des phases 2 et 3 : des amitiés (score 0–100) au lieu d'un esprit.
#[derive(Deserialize)]
#[cfg_attr(test, derive(serde::Serialize))]
struct PersonV4 {
    id: u32,
    first: String,
    last: String,
    female: bool,
    born: i64,
    death_age: f32,
    status: crate::people::Status,
    village: u32,
    home: Option<u32>,
    job: Option<crate::people::Job>,
    partner: Option<u32>,
    parents: Vec<u32>,
    children: Vec<u32>,
    friends: BTreeMap<u32, f32>,
    master: Option<u32>,
    apprentice_until: u64,
    x: f32,
    y: f32,
    looks: [u8; 3],
}

impl PersonV4 {
    fn upgrade(self) -> (crate::people::Person, BTreeMap<u32, f32>) {
        let p = crate::people::Person {
            id: self.id,
            first: self.first,
            last: self.last,
            female: self.female,
            born: self.born,
            death_age: self.death_age,
            status: self.status,
            village: self.village,
            home: self.home,
            job: self.job,
            partner: self.partner,
            parents: self.parents,
            children: self.children,
            master: self.master,
            apprentice_until: self.apprentice_until,
            x: self.x,
            y: self.y,
            looks: self.looks,
            mind: Default::default(),
            brain: Default::default(),
        };
        (p, self.friends)
    }
}

/// Économie des versions 4 et 5 : revenus par métier en tableaux de taille fixe.
#[derive(Deserialize)]
#[cfg_attr(test, derive(serde::Serialize))]
struct EconomyV5 {
    households: BTreeMap<u32, crate::economy::Household>,
    markets: Vec<crate::economy::Market>,
    treasury: Vec<f32>,
    workshops: BTreeMap<u32, crate::economy::Workshop>,
    income: Vec<[f32; JOBS_V5]>,
    today: Vec<[f32; JOBS_V5]>,
    access: Vec<[bool; crate::economy::GOODS]>,
    switched: BTreeMap<u32, u64>,
}

impl EconomyV5 {
    fn upgrade(self) -> crate::economy::Economy {
        crate::economy::Economy {
            households: self.households,
            markets: self.markets,
            treasury: self.treasury,
            workshops: self.workshops,
            income: self.income.iter().map(|r| r.to_vec()).collect(),
            today: self.today.iter().map(|r| r.to_vec()).collect(),
            access: self.access,
            switched: self.switched,
        }
    }
}

/// Esprit de la version 5 : compétences en tableau de taille fixe.
#[derive(Deserialize)]
#[cfg_attr(test, derive(serde::Serialize))]
struct MindV5 {
    traits: crate::mind::Traits,
    needs: crate::mind::Needs,
    mood: crate::mind::Mood,
    memories: Vec<crate::mind::Memory>,
    relations: BTreeMap<u32, crate::mind::Relation>,
    skills: [f32; JOBS_V5],
    goals: Vec<crate::mind::LifeGoal>,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(serde::Serialize))]
struct PersonV5 {
    id: u32,
    first: String,
    last: String,
    female: bool,
    born: i64,
    death_age: f32,
    status: crate::people::Status,
    village: u32,
    home: Option<u32>,
    job: Option<crate::people::Job>,
    partner: Option<u32>,
    parents: Vec<u32>,
    children: Vec<u32>,
    master: Option<u32>,
    apprentice_until: u64,
    x: f32,
    y: f32,
    looks: [u8; 3],
    mind: MindV5,
}

impl PersonV5 {
    fn upgrade(self) -> crate::people::Person {
        let m = self.mind;
        crate::people::Person {
            id: self.id,
            first: self.first,
            last: self.last,
            female: self.female,
            born: self.born,
            death_age: self.death_age,
            status: self.status,
            village: self.village,
            home: self.home,
            job: self.job,
            partner: self.partner,
            parents: self.parents,
            children: self.children,
            master: self.master,
            apprentice_until: self.apprentice_until,
            x: self.x,
            y: self.y,
            looks: self.looks,
            mind: crate::mind::Mind {
                traits: m.traits,
                needs: m.needs,
                mood: m.mood,
                memories: m.memories,
                relations: m.relations,
                skills: m.skills.to_vec(),
                goals: m.goals,
            },
            brain: Default::default(),
        }
    }
}

/// Monde de la phase 6 : celui d'aujourd'hui sans l'histoire.
#[derive(Deserialize)]
#[cfg_attr(test, derive(serde::Serialize))]
struct WorldV7 {
    seed: u64,
    tick: u64,
    map: Map,
    villages: Vec<crate::settlement::Village>,
    people: Vec<crate::people::Person>,
    chronicle: crate::events::Chronicle,
    rng: crate::rng::Rng,
    next_building: u32,
    chunk_rev: Vec<u64>,
    map_rev: u64,
    economy: crate::economy::Economy,
    trade: crate::trade::Trade,
    society: crate::society::Society,
}

/// Monde de la phase 5 : sans la société.
#[derive(Deserialize)]
#[cfg_attr(test, derive(serde::Serialize))]
struct WorldV6 {
    seed: u64,
    tick: u64,
    map: Map,
    villages: Vec<crate::settlement::Village>,
    people: Vec<crate::people::Person>,
    chronicle: crate::events::Chronicle,
    rng: crate::rng::Rng,
    next_building: u32,
    chunk_rev: Vec<u64>,
    map_rev: u64,
    economy: crate::economy::Economy,
    trade: crate::trade::Trade,
}

/// Monde de la phase 4 : sans commerce.
#[derive(Deserialize)]
#[cfg_attr(test, derive(serde::Serialize))]
struct WorldV5 {
    seed: u64,
    tick: u64,
    map: Map,
    villages: Vec<crate::settlement::Village>,
    people: Vec<PersonV5>,
    chronicle: crate::events::Chronicle,
    rng: crate::rng::Rng,
    next_building: u32,
    chunk_rev: Vec<u64>,
    map_rev: u64,
    economy: EconomyV5,
}

/// Monde de la phase 3 : habitants sans esprit.
#[derive(Deserialize)]
#[cfg_attr(test, derive(serde::Serialize))]
struct WorldV4 {
    seed: u64,
    tick: u64,
    map: Map,
    villages: Vec<crate::settlement::Village>,
    people: Vec<PersonV4>,
    chronicle: crate::events::Chronicle,
    rng: crate::rng::Rng,
    next_building: u32,
    chunk_rev: Vec<u64>,
    map_rev: u64,
    economy: EconomyV5,
}

/// Monde de la phase 2 : celui de la phase 3 sans son économie.
#[derive(Deserialize)]
struct WorldV3 {
    seed: u64,
    tick: u64,
    map: Map,
    villages: Vec<crate::settlement::Village>,
    people: Vec<PersonV4>,
    chronicle: crate::events::Chronicle,
    rng: crate::rng::Rng,
    next_building: u32,
    chunk_rev: Vec<u64>,
    map_rev: u64,
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
        3 => {
            let v: WorldV3 = postcard::from_bytes(&raw)?;
            let (people, friends) = v.people.into_iter().map(PersonV4::upgrade).unzip();
            let mut w = World {
                seed: v.seed,
                tick: v.tick,
                map: std::sync::Arc::new(v.map),
                villages: v.villages,
                people,
                chronicle: v.chronicle,
                rng: v.rng,
                next_building: v.next_building,
                chunk_rev: v.chunk_rev,
                map_rev: v.map_rev,
                economy: Default::default(),
                trade: Default::default(),
                society: Default::default(),
                history: Default::default(),
                caches: Default::default(),
            };
            w.ensure_caches();
            w.bootstrap_economy();
            w.bootstrap_minds(friends);
            w.bootstrap_trade();
            w.bootstrap_society();
            w.bootstrap_history();
            Ok(Loaded::Current(w))
        }
        4 => {
            let v: WorldV4 = postcard::from_bytes(&raw)?;
            let (people, friends) = v.people.into_iter().map(PersonV4::upgrade).unzip();
            let mut w = World {
                seed: v.seed,
                tick: v.tick,
                map: std::sync::Arc::new(v.map),
                villages: v.villages,
                people,
                chronicle: v.chronicle,
                rng: v.rng,
                next_building: v.next_building,
                chunk_rev: v.chunk_rev,
                map_rev: v.map_rev,
                economy: v.economy.upgrade(),
                trade: Default::default(),
                society: Default::default(),
                history: Default::default(),
                caches: Default::default(),
            };
            w.ensure_caches();
            w.bootstrap_minds(friends);
            w.bootstrap_trade();
            w.bootstrap_society();
            w.bootstrap_history();
            Ok(Loaded::Current(w))
        }
        5 => {
            let v: WorldV5 = postcard::from_bytes(&raw)?;
            let mut w = World {
                seed: v.seed,
                tick: v.tick,
                map: std::sync::Arc::new(v.map),
                villages: v.villages,
                people: v.people.into_iter().map(PersonV5::upgrade).collect(),
                chronicle: v.chronicle,
                rng: v.rng,
                next_building: v.next_building,
                chunk_rev: v.chunk_rev,
                map_rev: v.map_rev,
                economy: v.economy.upgrade(),
                trade: Default::default(),
                society: Default::default(),
                history: Default::default(),
                caches: Default::default(),
            };
            w.ensure_caches();
            w.bootstrap_trade();
            w.bootstrap_society();
            w.bootstrap_history();
            Ok(Loaded::Current(w))
        }
        6 => {
            let v: WorldV6 = postcard::from_bytes(&raw)?;
            let mut w = World {
                seed: v.seed,
                tick: v.tick,
                map: std::sync::Arc::new(v.map),
                villages: v.villages,
                people: v.people,
                chronicle: v.chronicle,
                rng: v.rng,
                next_building: v.next_building,
                chunk_rev: v.chunk_rev,
                map_rev: v.map_rev,
                economy: v.economy,
                trade: v.trade,
                society: Default::default(),
                history: Default::default(),
                caches: Default::default(),
            };
            w.ensure_caches();
            w.bootstrap_society();
            w.bootstrap_history();
            Ok(Loaded::Current(w))
        }
        7 => {
            let v: WorldV7 = postcard::from_bytes(&raw)?;
            let mut w = World {
                seed: v.seed,
                tick: v.tick,
                map: std::sync::Arc::new(v.map),
                villages: v.villages,
                people: v.people,
                chronicle: v.chronicle,
                rng: v.rng,
                next_building: v.next_building,
                chunk_rev: v.chunk_rev,
                map_rev: v.map_rev,
                economy: v.economy,
                trade: v.trade,
                society: v.society,
                history: Default::default(),
                caches: Default::default(),
            };
            w.ensure_caches();
            w.bootstrap_history();
            Ok(Loaded::Current(w))
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
        map.biome.fill(Biome::Meadow as u8);
        map.elevation.fill(100);
        map.fertility.fill(200);
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

    /// Le monde tel que la phase 3 l'aurait sauvegardé (amitiés au lieu des esprits).
    fn as_v4(w: &World) -> WorldV4 {
        WorldV4 {
            seed: w.seed,
            tick: w.tick,
            map: (*w.map).clone(),
            villages: w.villages.clone(),
            people: w
                .people
                .iter()
                .map(|p| PersonV4 {
                    id: p.id,
                    first: p.first.clone(),
                    last: p.last.clone(),
                    female: p.female,
                    born: p.born,
                    death_age: p.death_age,
                    status: p.status,
                    village: p.village,
                    home: p.home,
                    job: p.job,
                    partner: p.partner,
                    parents: p.parents.clone(),
                    children: p.children.clone(),
                    friends: p.mind.relations.iter().map(|(&k, r)| (k, r.affection)).collect(),
                    master: p.master,
                    apprentice_until: p.apprentice_until,
                    x: p.x,
                    y: p.y,
                    looks: p.looks,
                })
                .collect(),
            chronicle: w.chronicle.clone(),
            rng: w.rng.clone(),
            next_building: w.next_building,
            chunk_rev: w.chunk_rev.clone(),
            map_rev: w.map_rev,
            economy: economy_v5(&w.economy),
        }
    }

    fn economy_v5(e: &crate::economy::Economy) -> EconomyV5 {
        let fixed = |rows: &Vec<Vec<f32>>| -> Vec<[f32; JOBS_V5]> {
            rows.iter()
                .map(|r| std::array::from_fn(|k| r.get(k).copied().unwrap_or(0.0)))
                .collect()
        };
        EconomyV5 {
            households: e.households.clone(),
            markets: e.markets.clone(),
            treasury: e.treasury.clone(),
            workshops: e.workshops.clone(),
            income: fixed(&e.income),
            today: fixed(&e.today),
            access: e.access.clone(),
            switched: e.switched.clone(),
        }
    }

    /// Le monde tel que la phase 4 l'aurait sauvegardé (sans commerce, 10 métiers).
    fn as_v5(w: &World) -> WorldV5 {
        WorldV5 {
            seed: w.seed,
            tick: w.tick,
            map: (*w.map).clone(),
            villages: w.villages.clone(),
            people: w
                .people
                .iter()
                .map(|p| PersonV5 {
                    id: p.id,
                    first: p.first.clone(),
                    last: p.last.clone(),
                    female: p.female,
                    born: p.born,
                    death_age: p.death_age,
                    status: p.status,
                    village: p.village,
                    home: p.home,
                    job: p.job.filter(|&j| j != crate::people::Job::Merchant),
                    partner: p.partner,
                    parents: p.parents.clone(),
                    children: p.children.clone(),
                    master: p.master,
                    apprentice_until: p.apprentice_until,
                    x: p.x,
                    y: p.y,
                    looks: p.looks,
                    mind: MindV5 {
                        traits: p.mind.traits,
                        needs: p.mind.needs,
                        mood: p.mind.mood,
                        memories: p.mind.memories.clone(),
                        relations: p.mind.relations.clone(),
                        skills: std::array::from_fn(|k| p.mind.skills.get(k).copied().unwrap_or(0.0)),
                        goals: p.mind.goals.clone(),
                    },
                })
                .collect(),
            chronicle: w.chronicle.clone(),
            rng: w.rng.clone(),
            next_building: w.next_building,
            chunk_rev: w.chunk_rev.clone(),
            map_rev: w.map_rev,
            economy: economy_v5(&w.economy),
        }
    }

    /// Une sauvegarde de la phase 6 est reprise : même société, un premier relevé de chaque
    /// village tout de suite, et les événements marquants de la chronique dans les annales.
    #[test]
    fn reads_v7() {
        let mut w = sample();
        w.event(
            Some(w.villages[0].id),
            vec![],
            "Fondation de Testville (9 habitants)".into(),
        );
        let v7 = WorldV7 {
            seed: w.seed,
            tick: w.tick,
            map: (*w.map).clone(),
            villages: w.villages.clone(),
            people: w.people.clone(),
            chronicle: w.chronicle.clone(),
            rng: w.rng.clone(),
            next_building: w.next_building,
            chunk_rev: w.chunk_rev.clone(),
            map_rev: w.map_rev,
            economy: w.economy.clone(),
            trade: w.trade.clone(),
            society: w.society.clone(),
        };
        let raw = postcard::to_stdvec(&v7).unwrap();
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&7u32.to_le_bytes());
        bytes.extend_from_slice(&zstd::encode_all(&raw[..], 3).unwrap());
        match decode(&bytes).unwrap() {
            Loaded::Current(back) => {
                assert_eq!(back.society, w.society);
                let id = back.villages[0].id;
                assert_eq!(back.history.villages[&id].days.len(), 1);
                assert!(
                    back.history
                        .annals
                        .iter()
                        .any(|e| e.text.starts_with("Fondation"))
                );
                assert_eq!(back.history.annals_seen, back.chronicle.next_seq);
            }
            other => panic!("{other:?}"),
        }
    }

    /// Une sauvegarde de la phase 5 est reprise : mêmes habitants et commerce, une société
    /// neuve (chaque village élit bientôt son chef).
    #[test]
    fn reads_v6() {
        let w = sample();
        let v6 = WorldV6 {
            seed: w.seed,
            tick: w.tick,
            map: (*w.map).clone(),
            villages: w.villages.clone(),
            people: w.people.clone(),
            chronicle: w.chronicle.clone(),
            rng: w.rng.clone(),
            next_building: w.next_building,
            chunk_rev: w.chunk_rev.clone(),
            map_rev: w.map_rev,
            economy: w.economy.clone(),
            trade: w.trade.clone(),
        };
        let raw = postcard::to_stdvec(&v6).unwrap();
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&6u32.to_le_bytes());
        bytes.extend_from_slice(&zstd::encode_all(&raw[..], 3).unwrap());
        match decode(&bytes).unwrap() {
            Loaded::Current(back) => {
                assert_eq!(back.tick, w.tick);
                assert_eq!(back.economy, w.economy);
                assert_eq!(back.people.len(), w.people.len());
                let p = back.society.polity(back.villages[0].id).unwrap();
                assert!(p.chief.is_none_or(|c| back.people[c as usize].alive()));
                assert!(back.society.sick.is_empty());
            }
            other => panic!("{other:?}"),
        }
    }

    /// Une sauvegarde de la phase 4 est reprise : mêmes habitants et compétences, revenus
    /// complétés pour le métier de marchand, commerce vide.
    #[test]
    fn reads_v5() {
        let w = sample();
        let raw = postcard::to_stdvec(&as_v5(&w)).unwrap();
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&5u32.to_le_bytes());
        bytes.extend_from_slice(&zstd::encode_all(&raw[..], 3).unwrap());
        match decode(&bytes).unwrap() {
            Loaded::Current(back) => {
                assert_eq!(back.tick, w.tick);
                assert_eq!(back.people.len(), w.people.len());
                for (a, b) in back.people.iter().zip(&w.people) {
                    if let Some(j) = b.job {
                        assert_eq!(a.mind.skill(j), b.mind.skill(j));
                    }
                }
                let jobs = crate::economy::JOBS;
                assert!(back.economy.income.iter().all(|r| r.len() == jobs));
                assert!(back.trade.traders.is_empty());
            }
            other => panic!("{other:?}"),
        }
    }

    /// Une sauvegarde de la phase 3 est reprise : chacun reçoit une personnalité, ses
    /// amitiés deviennent des relations, les adultes ont des compétences et des objectifs.
    #[test]
    fn reads_v4() {
        let w = sample();
        let raw = postcard::to_stdvec(&as_v4(&w)).unwrap();
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(&zstd::encode_all(&raw[..], 3).unwrap());
        match decode(&bytes).unwrap() {
            Loaded::Current(back) => {
                assert_eq!(back.tick, w.tick);
                assert_eq!(back.economy, w.economy);
                let tick = back.tick;
                let adults: Vec<_> = back.alive().filter(|p| !p.child(tick)).collect();
                assert!(!adults.is_empty());
                assert!(adults.iter().all(|p| !p.mind.goals.is_empty()));
                assert!(adults.iter().all(|p| p.job.is_none_or(|j| p.mind.skill(j) > 0.0)));
                // Les amitiés d'avant sont gardées.
                let p = &w.people[0];
                for (k, r) in &p.mind.relations {
                    assert!(back.people[0].mind.affection(*k) >= r.affection.min(50.0) - 0.01);
                }
            }
            other => panic!("{other:?}"),
        }
    }

    /// Une sauvegarde de la phase 2 (sans économie) est reprise avec des foyers approvisionnés.
    #[test]
    fn reads_v3() {
        let w = sample();
        // Encodage « v3 » : le monde de la phase 3 sans son dernier champ (l'économie).
        let mut raw = postcard::to_stdvec(&as_v4(&w)).unwrap();
        let eco = postcard::to_stdvec(&economy_v5(&w.economy)).unwrap();
        raw.truncate(raw.len() - eco.len());
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&zstd::encode_all(&raw[..], 3).unwrap());
        match decode(&bytes).unwrap() {
            Loaded::Current(back) => {
                assert_eq!(back.tick, w.tick);
                assert_eq!(back.people.len(), w.people.len());
                assert!(!back.economy.households.is_empty());
                assert!(back.economy.households.values().all(|h| h.coins > 0.0));
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

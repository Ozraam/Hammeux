//! Moteur de Hameaux : monde, carte, habitants, villages.
//!
//! Ce crate ne touche ni au réseau ni au disque : il avance l'état d'un pas à la fois
//! et sait le sérialiser. Tout le reste (boucle temps réel, fichiers, web) vit dans `hameaux-server`.

pub mod clock;
pub mod economy;
pub mod events;
pub mod history;
pub mod map;
pub mod mind;
pub mod names;
pub mod path;
pub mod people;
pub mod rng;
pub mod save;
pub mod settlement;
pub mod sim;
pub mod society;
pub mod trade;
pub mod tuning;
pub mod world;

pub use clock::{Clock, Season};
pub use map::{Biome, Map, Resource};
pub use world::{Site, World};

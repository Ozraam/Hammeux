//! Chronique : ce qui s'est passé dans le monde.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

/// Nombre d'événements gardés en mémoire (l'historique complet ira en base plus tard).
pub const KEEP: usize = 2_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// Numéro d'ordre croissant (pour demander « ce qui est nouveau depuis »).
    pub seq: u64,
    pub tick: u64,
    pub village: Option<u32>,
    /// Habitants concernés.
    pub people: Vec<u32>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Chronicle {
    pub events: VecDeque<Event>,
    pub next_seq: u64,
}

impl Chronicle {
    pub fn push(&mut self, tick: u64, village: Option<u32>, people: Vec<u32>, text: String) {
        self.events.push_back(Event {
            seq: self.next_seq,
            tick,
            village,
            people,
            text,
        });
        self.next_seq += 1;
        while self.events.len() > KEEP {
            self.events.pop_front();
        }
    }

    /// Événements de numéro ≥ `since`, du plus ancien au plus récent.
    pub fn since(&self, since: u64) -> impl Iterator<Item = &Event> {
        self.events.iter().filter(move |e| e.seq >= since)
    }
}

//! Calendrier du monde.
//!
//! Un tick dure [`TICK_SECONDS`] secondes de jeu. Une année compte 4 saisons de
//! [`DAYS_PER_SEASON`] jours. Le monde commence au printemps de l'an 1, à [`START_HOUR`] h.

use serde::{Deserialize, Serialize};

/// Durée d'un tick en secondes de jeu.
pub const TICK_SECONDS: u64 = 10;
pub const SECONDS_PER_DAY: u64 = 24 * 3600;
pub const TICKS_PER_DAY: u64 = SECONDS_PER_DAY / TICK_SECONDS;
pub const DAYS_PER_SEASON: u64 = 7;
pub const DAYS_PER_YEAR: u64 = DAYS_PER_SEASON * 4;
/// Heure du premier tick (le monde se réveille le matin).
pub const START_HOUR: u64 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Season {
    Spring,
    Summer,
    Autumn,
    Winter,
}

impl Season {
    pub fn label(self) -> &'static str {
        match self {
            Season::Spring => "Printemps",
            Season::Summer => "Été",
            Season::Autumn => "Automne",
            Season::Winter => "Hiver",
        }
    }
}

/// Date et heure dérivées d'un numéro de tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clock {
    /// Année, à partir de 1.
    pub year: u64,
    pub season: Season,
    /// Jour dans la saison, à partir de 1.
    pub day: u64,
    pub hour: u64,
    pub minute: u64,
}

impl Clock {
    pub fn at(tick: u64) -> Self {
        let seconds = tick * TICK_SECONDS + START_HOUR * 3600;
        let days = seconds / SECONDS_PER_DAY;
        let in_day = seconds % SECONDS_PER_DAY;
        let year = days / DAYS_PER_YEAR + 1;
        let in_year = days % DAYS_PER_YEAR;
        let season = match in_year / DAYS_PER_SEASON {
            0 => Season::Spring,
            1 => Season::Summer,
            2 => Season::Autumn,
            _ => Season::Winter,
        };
        Clock {
            year,
            season,
            day: in_year % DAYS_PER_SEASON + 1,
            hour: in_day / 3600,
            minute: in_day % 3600 / 60,
        }
    }

    /// Heure du jour en fraction (0.0 = minuit, 0.5 = midi), pour la lumière.
    pub fn day_fraction(&self) -> f32 {
        (self.hour * 60 + self.minute) as f32 / 1440.0
    }
}

impl std::fmt::Display for Clock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "An {}, {} jour {}, {:02}:{:02}",
            self.year,
            self.season.label(),
            self.day,
            self.hour,
            self.minute
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_spring_morning() {
        let c = Clock::at(0);
        assert_eq!(
            (c.year, c.season, c.day, c.hour, c.minute),
            (1, Season::Spring, 1, 6, 0)
        );
    }

    #[test]
    fn rolls_over_seasons_and_years() {
        // 18 h plus tard : minuit du jour 2.
        let c = Clock::at(18 * 3600 / TICK_SECONDS);
        assert_eq!((c.day, c.hour), (2, 0));
        let c = Clock::at(DAYS_PER_SEASON * TICKS_PER_DAY);
        assert_eq!((c.season, c.day, c.hour), (Season::Summer, 1, 6));
        let c = Clock::at(DAYS_PER_YEAR * TICKS_PER_DAY);
        assert_eq!((c.year, c.season, c.day), (2, Season::Spring, 1));
    }
}

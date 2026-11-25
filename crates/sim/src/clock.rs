//! Simulationszeit. Der Kern kennt nur seinen eigenen Taktzähler, nie die Wanduhr.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};

/// Takte pro Sekunde bei einfacher Geschwindigkeit (`FixedUpdate`-Rate).
pub const TICKS_PER_SECOND: u64 = 20;
/// Ein Kalendertag dauert bei einfacher Geschwindigkeit eine Sekunde.
pub const TICKS_PER_DAY: u64 = 20;
pub const DAYS_PER_MONTH: u64 = 30;
pub const DAYS_PER_YEAR: u64 = 360;
/// Jahr, in dem jede Partie beginnt.
pub const START_YEAR: u64 = 2400;

#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimClock {
    pub tick: u64,
}

impl SimClock {
    #[inline]
    pub fn day(&self) -> u64 {
        self.tick / TICKS_PER_DAY
    }

    /// Wahr im ersten Takt eines Tages; dann laufen die strategischen Systeme.
    #[inline]
    pub fn is_day_start(&self) -> bool {
        self.tick.is_multiple_of(TICKS_PER_DAY)
    }

    pub fn date(&self) -> Date {
        Date::from_day(self.day())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub year: u64,
    pub month: u64,
    pub day: u64,
}

impl Date {
    pub fn from_day(day: u64) -> Self {
        Self {
            year: START_YEAR + day / DAYS_PER_YEAR,
            month: (day % DAYS_PER_YEAR) / DAYS_PER_MONTH + 1,
            day: day % DAYS_PER_MONTH + 1,
        }
    }
}

impl std::fmt::Display for Date {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:02}.{:02}.{}", self.day, self.month, self.year)
    }
}

/// Datum zu einem Takt, z. B. für Chronikeinträge.
pub fn date_of_tick(tick: u64) -> Date {
    Date::from_day(tick / TICKS_PER_DAY)
}

/// Laufbedingung für strategische Systeme.
pub fn day_start(clock: Res<SimClock>) -> bool {
    clock.is_day_start()
}

/// Laufbedingung: erster Takt eines Monats.
pub fn month_start(clock: Res<SimClock>) -> bool {
    clock.is_day_start() && clock.day().is_multiple_of(DAYS_PER_MONTH)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_rolls_over() {
        assert_eq!(Date::from_day(0).to_string(), "01.01.2400");
        assert_eq!(Date::from_day(29).to_string(), "30.01.2400");
        assert_eq!(Date::from_day(30).to_string(), "01.02.2400");
        assert_eq!(Date::from_day(360).to_string(), "01.01.2401");
    }

    #[test]
    fn day_start_only_on_boundary() {
        assert!(SimClock { tick: 0 }.is_day_start());
        assert!(!SimClock { tick: 1 }.is_day_start());
        assert!(
            SimClock {
                tick: TICKS_PER_DAY * 5
            }
            .is_day_start()
        );
    }
}

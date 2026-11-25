//! KI der Gemeinschaften. Sie wirkt wie der Spieler ausschließlich über Befehle.
//!
//! Stand Meilenstein 1: nur Ausdehnung. Jede KI-Gemeinschaft besiedelt die beste freie
//! Welt in Reichweite, bis der endliche Raum verteilt ist.

mod plugin;

pub use plugin::AiPlugin;

use crate::data::GameData;
use crate::economy::Economy;
use crate::galaxy::{Galaxy, Planet};
use crate::ids::{PlanetId, PolityId};
use crate::polity::{Polities, claimable_planets};

/// Wert einer freien Welt aus Sicht eines Siedlers.
pub fn colony_value(galaxy: &Galaxy, planet: &Planet) -> f64 {
    let living_space = planet.capacity * planet.habitability as f64 / 1000.0;
    let deposits: f64 = planet
        .deposits
        .iter()
        .map(|deposit| (deposit.richness * deposit.reserve) as f64)
        .sum();
    let position = galaxy.system(planet.system).centrality as f64;
    // Lebensfeindliche Welten ohne reiche Vorkommen tragen keine Kolonie.
    let viability = 0.3 + 0.7 * planet.habitability as f64;
    (living_space + deposits * 1.5 + position * 2.0) * viability
}

/// Beste freie Welt, die `polity` heute besiedeln darf. Gleichstand entscheidet die ID.
pub fn best_colony_target(
    data: &GameData,
    galaxy: &Galaxy,
    polities: &Polities,
    economy: &Economy,
    polity: PolityId,
    day: u64,
) -> Option<PlanetId> {
    let mut best: Option<(PlanetId, f64)> = None;
    let treasury = economy.treasury(polity);
    for id in claimable_planets(
        galaxy,
        polities,
        &data.balance.colonization,
        treasury,
        polity,
        day,
    ) {
        let value = colony_value(galaxy, galaxy.planet(id));
        if best.is_none_or(|(_, best_value)| value > best_value) {
            best = Some((id, value));
        }
    }
    best.map(|(id, _)| id)
}

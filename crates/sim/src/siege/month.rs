//! Monatsschritt der Belagerung, die Buchführung: Miliz im Frieden, das Abklingen von
//! Kriegsschäden und Besatzung, die Spiegelwerte der Regionen, aus denen der Zusammenhalt
//! rechnet, und die Anteile an den Betrieben belagerter Welten.
//!
//! Beschaffung, Aushebung, Ausbau und Sold der stehenden Verteidigung stehen in
//! [`super::arms`].

use super::step::militia_strength;
use super::{Sieges, mirror_region};
use crate::data::GameData;
use crate::finance::{Finance, ownership_of_region};
use crate::galaxy::Galaxy;
use crate::ids::{PlanetId, PolityId};
use crate::logistics::Logistics;
use crate::polity::Polities;
use std::collections::BTreeMap;

/// Unter diesem Wert gelten Kriegsschäden und Besatzung als verheilt.
const HEALED: f64 = 0.01;

/// Anteile am Wert der Betriebe einer Welt je Gemeinschaft: Fremde mit ihrem Anteil, der
/// Rest liegt beim Besitzer `owner` und seinen Regionen.
pub fn stakes_in(
    galaxy: &Galaxy,
    polities: &Polities,
    finance: &Finance,
    logistics: &Logistics,
    planet: PlanetId,
    owner: PolityId,
) -> BTreeMap<PolityId, f64> {
    let capitals: Vec<Option<PlanetId>> = polities.list.iter().map(|p| p.capital).collect();
    let mut stakes = ownership_of_region(finance, galaxy, logistics, &capitals, planet).foreign;
    for stake in stakes.values_mut() {
        *stake = stake.clamp(0.0, 1.0);
    }
    let foreign: f64 = stakes.values().sum();
    stakes.insert(owner, (1.0 - foreign).clamp(0.0, 1.0));
    stakes
}

/// Monatsbeginn, in der Reihenfolge der Planeten:
/// - Die Miliz einer Welt, die nicht belagert wird, folgt Bevölkerung und Zusammenhalt.
/// - Kriegsschäden und Besatzung klingen ab; unter 1 % sind sie vergessen. Eine Besatzung
///   endet sofort, wenn die Welt nicht mehr dem gehört, der sie erobert hat.
/// - Die Spiegelwerte jeder Region werden neu geschrieben ([`mirror_region`]).
/// - Laufende Belagerungen lesen die Anteile an den Betrieben ihrer Welt neu.
pub fn run_month(
    data: &GameData,
    galaxy: &Galaxy,
    polities: &Polities,
    finance: &mut Finance,
    logistics: &Logistics,
    sieges: &mut Sieges,
) {
    let balance = &data.balance.siege;
    for planet in &galaxy.planets {
        let index = planet.id.index();
        let besieged = sieges.active.contains_key(&planet.id);
        let (Some(defense), Some(region)) = (
            sieges.defenses.get_mut(index),
            finance.regions.get_mut(index),
        ) else {
            continue;
        };
        if !besieged {
            let militia = match planet.owner {
                Some(_) => militia_strength(planet.population, region.cohesion, balance),
                None => 0.0,
            };
            // Das Depot zählt in Tagen für die ganze Garnison: Wächst die Miliz, reichen
            // dieselben Vorräte kürzer, schrumpft sie, länger. Eine eben erst aufgestellte
            // Miliz bringt ihre Vorräte mit.
            let old = defense.militia + defense.regulars;
            let new = defense.regulars + militia;
            if defense.militia > 0.0 && new > 0.0 {
                defense.depot = (defense.depot * old / new).min(balance.depot_days_cap as f64);
            }
            defense.militia = militia;
        }
        defense.scar *= 1.0 - balance.scar_decay;
        if defense.scar < HEALED {
            defense.scar = 0.0;
            defense.scar_by = None;
        }
        defense.occupation *= 1.0 - balance.occupation_decay;
        if defense.occupation < HEALED || defense.occupied_by != planet.owner {
            defense.occupation = 0.0;
            defense.occupied_from = None;
            defense.occupied_by = None;
        }
        mirror_region(balance, defense, planet.owner, planet.population, region);
    }
    let besieged: Vec<(PlanetId, PolityId)> = sieges
        .active
        .iter()
        .map(|(planet, siege)| (*planet, siege.defender))
        .collect();
    for (planet, defender) in besieged {
        let stakes = stakes_in(galaxy, polities, finance, logistics, planet, defender);
        if let Some(siege) = sieges.active.get_mut(&planet) {
            siege.stakes = stakes;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{SimConfig, SimState};

    #[test]
    fn the_month_heals_damage_ends_stale_occupations_and_mirrors_the_rest() {
        let data = GameData::load_workspace().unwrap();
        let config = SimConfig {
            seed: 6,
            player: None,
        };
        let mut state = SimState::generate(&data, config);
        state.galaxy.rebuild_index();
        let balance = &data.balance.siege;
        let (me, foe) = (PolityId(0), PolityId(1));
        let home = state.polities.get(me).capital.unwrap();
        let other = state.polities.get(foe).capital.unwrap();
        let free = state
            .galaxy
            .planets
            .iter()
            .find(|planet| planet.owner.is_none())
            .unwrap()
            .id;
        // Die eigene Hauptwelt: erobert und besetzt, mit Schäden, die der Besitzer
        // verantwortet. Die fremde: eine Besatzung, deren Eroberer sie nicht mehr hält,
        // und Schäden von fremder Hand, fast verheilt.
        {
            let defense = state.sieges.defense_mut(home);
            defense.scar = 0.4;
            defense.scar_by = Some(me);
            defense.occupation = 0.8;
            defense.occupied_from = Some(foe);
            defense.occupied_by = Some(me);
            defense.regulars = 0.0;
            let defense = state.sieges.defense_mut(other);
            defense.scar = 0.0101;
            defense.scar_by = Some(me);
            defense.occupation = 0.8;
            defense.occupied_from = Some(me);
            defense.occupied_by = Some(PolityId(2));
            state.sieges.defense_mut(free).militia = 3.0;
        }
        let run = |state: &mut SimState| {
            run_month(
                &data,
                &state.galaxy,
                &state.polities,
                &mut state.finance,
                &state.logistics,
                &mut state.sieges,
            )
        };
        run(&mut state);
        let defense = state.sieges.defense(home);
        assert!((defense.scar - 0.4 * (1.0 - balance.scar_decay)).abs() < 1e-12);
        assert!((defense.occupation - 0.8 * (1.0 - balance.occupation_decay)).abs() < 1e-12);
        assert_eq!(defense.occupied_from, Some(foe));
        // Die Miliz folgt Bevölkerung und Zusammenhalt.
        let planet = state.galaxy.planet(home);
        let region = state.finance.region(home);
        let militia = militia_strength(planet.population, region.cohesion, balance);
        assert!(militia > 10.0 && defense.militia == militia);
        // Die Region spiegelt, woraus der Zusammenhalt rechnet.
        assert_eq!(region.occupation, defense.occupation);
        assert_eq!(region.war_damage, defense.scar);
        assert_eq!(region.garrison_cover, 0.0);
        // Die fremde Hauptwelt: Besatzung beendet, Schäden vergessen, nichts gespiegelt.
        let defense = state.sieges.defense(other);
        assert_eq!((defense.scar, defense.scar_by), (0.0, None));
        assert_eq!(defense.occupation, 0.0);
        assert_eq!((defense.occupied_from, defense.occupied_by), (None, None));
        let region = state.finance.region(other);
        assert_eq!((region.occupation, region.war_damage), (0.0, 0.0));
        assert!(region.garrison_cover > 0.0, "die Regulären der Heimatwelt");
        // Eine herrenlose Welt hat keine Miliz.
        assert_eq!(state.sieges.defense(free).militia, 0.0);
        // Über die Monate klingt alles ab, bis es vergessen ist.
        for _ in 0..400 {
            run(&mut state);
        }
        let defense = state.sieges.defense(home);
        assert_eq!((defense.scar, defense.scar_by), (0.0, None));
        assert_eq!((defense.occupation, defense.occupied_by), (0.0, None));
        assert_eq!(state.sieges.find_anomaly(state.galaxy.planets.len()), None);
    }

    #[test]
    fn a_running_siege_keeps_its_militia_and_reads_the_stakes_anew() {
        use crate::siege::orbit::testing::{FOE, ME, Scene};
        let mut scene = Scene::colony();
        scene.day();
        let planet = scene.planet;
        let militia = scene.state.sieges.defense(planet).militia;
        assert!(militia > 0.0);
        scene.state.sieges.active.get_mut(&planet).unwrap().stakes = BTreeMap::new();
        let state = &mut scene.state;
        run_month(
            &scene.data,
            &state.galaxy,
            &state.polities,
            &mut state.finance,
            &state.logistics,
            &mut state.sieges,
        );
        // Die Miliz einer belagerten Welt steht in ihren Zonen: Der Monat rührt sie nicht an.
        assert_eq!(state.sieges.defense(planet).militia, militia);
        let stakes = &state.sieges.active[&planet].stakes;
        assert!(stakes[&FOE] > 0.5 && !stakes.contains_key(&ME));
        assert!((stakes.values().sum::<f64>() - 1.0).abs() < 1e-9);
    }
}

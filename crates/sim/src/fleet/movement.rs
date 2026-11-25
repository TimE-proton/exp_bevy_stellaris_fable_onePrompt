//! Tagesschritt auf der Karte: Marsch entlang der Routen und die Blockaderegel.

use super::{Fleets, Stance};
use crate::data::GameData;
use crate::diplomacy::Diplomacy;
use crate::galaxy::Galaxy;

/// Strecke, die eine Flotte an einem Tag zurücklegt, und ob sie damit das nächste System
/// erreicht. `fuel` ist der knappste Tank der Flotte (0..1), `remaining` der Rest der
/// Route bis zum nächsten System.
///
/// Solange der knappste Tank reicht, läuft die Flotte volle Fahrt; den Rest des Tages
/// schleppt sie sich mit `dry_speed` ihrer Fahrt weiter. Ein leerer Tank hält also nie
/// endgültig fest, er kostet Zeit.
pub fn day_travel(
    fuel: f32,
    speed: f32,
    fuel_per_distance: f32,
    dry_speed: f32,
    remaining: f32,
) -> (f32, bool) {
    let full = if fuel_per_distance > 0.0 {
        speed.min(fuel.max(0.0) / fuel_per_distance)
    } else {
        speed
    };
    let possible = full + (speed - full) * dry_speed;
    if possible >= remaining {
        (remaining, true)
    } else {
        (possible, false)
    }
}

/// Flotten rücken entlang ihrer Route vor und verbrauchen dabei Treibstoff: je
/// Streckeneinheit denselben Anteil jedes Tanks.
pub fn move_fleets(galaxy: &Galaxy, fleets: &mut Fleets, data: &GameData) {
    let speed = data.balance.fleets.speed;
    let balance = &data.balance.military;
    for fleet in &mut fleets.list {
        if !fleet.alive || fleet.route.is_empty() || fleet.in_battle {
            continue;
        }
        let Some(lane) = galaxy.lane_between(fleet.system, fleet.route[0]) else {
            fleet.route.clear();
            fleet.progress = 0.0;
            continue;
        };
        let length = galaxy.lane(lane).length.max(1.0);
        let remaining = (1.0 - fleet.progress).max(0.0) * length;
        let (travelled, arrived) = day_travel(
            fleet.min_fuel(),
            speed,
            balance.fuel_per_distance,
            balance.dry_speed,
            remaining,
        );
        let burned = travelled * balance.fuel_per_distance;
        for ship in &mut fleet.ships {
            ship.fuel = (ship.fuel - burned).max(0.0);
        }
        if arrived {
            fleet.system = fleet.route.remove(0);
            fleet.progress = 0.0;
        } else {
            fleet.progress += travelled / length;
        }
    }
}

/// Ein System ist blockiert, wenn eine feindliche Flotte im Blockadeauftrag dort steht
/// und der Besitzer keine eigene Flotte im Orbit hat: Erst die orbitale Überlegenheit,
/// dann die Sperre.
pub fn update_blockades(galaxy: &Galaxy, fleets: &mut Fleets, diplomacy: &Diplomacy) {
    let mut blockaded = Vec::new();
    for system in &galaxy.systems {
        let Some(owner) = galaxy.system_owner(system.id) else {
            continue;
        };
        let defended = fleets
            .docked_in(system.id)
            .any(|fleet| fleet.owner == owner);
        let besieged = fleets
            .docked_in(system.id)
            .any(|fleet| fleet.stance == Stance::Blockade && diplomacy.at_war(fleet.owner, owner));
        if besieged && !defended {
            blockaded.push(system.id);
        }
    }
    fleets.blockaded = blockaded;
}

#[cfg(test)]
mod tests {
    use super::super::testing::setup;
    use super::super::{order_move, set_stance};
    use super::*;
    use crate::ids::{FleetId, PolityId};

    /// Marschiert, bis die Flotte angekommen ist, und gibt die Tage zurück.
    fn travel(galaxy: &Galaxy, fleets: &mut Fleets, data: &GameData, fleet: FleetId) -> u32 {
        for day in 0..5000 {
            if fleets.list[fleet.index()].route.is_empty() {
                return day;
            }
            move_fleets(galaxy, fleets, data);
        }
        panic!("Flotte kam nie an");
    }

    #[test]
    fn fleets_follow_lanes_to_their_target() {
        let (data, galaxy, polities, mut fleets) = setup();
        let target = galaxy.planet(polities.list[1].capital.unwrap()).system;
        order_move(&galaxy, &mut fleets, PolityId(0), FleetId(0), target).unwrap();
        let route = fleets.list[0].route.clone();
        let mut previous = fleets.list[0].system;
        for step in &route {
            assert!(galaxy.lane_between(previous, *step).is_some());
            previous = *step;
        }
        travel(&galaxy, &mut fleets, &data, FleetId(0));
        assert_eq!(fleets.list[0].system, target);
        assert!(order_move(&galaxy, &mut fleets, PolityId(1), FleetId(0), target).is_err());
    }

    #[test]
    fn travelling_burns_fuel_in_proportion_to_the_distance() {
        let (data, galaxy, _, mut fleets) = setup();
        let balance = &data.balance.military;
        let home = fleets.list[0].system;
        let neighbor = galaxy.neighbors(home)[0];
        let length = galaxy.lane(neighbor.lane).length;
        order_move(
            &galaxy,
            &mut fleets,
            PolityId(0),
            FleetId(0),
            neighbor.system,
        )
        .unwrap();
        let days = travel(&galaxy, &mut fleets, &data, FleetId(0));
        // Ein voller Tank reicht für diese eine Route; die Fahrt war nie gedrosselt.
        assert!(length * balance.fuel_per_distance < 1.0);
        assert_eq!(days, (length / data.balance.fleets.speed).ceil() as u32);
        let expected = 1.0 - length * balance.fuel_per_distance;
        for ship in &fleets.list[0].ships {
            assert!(
                (ship.fuel - expected).abs() < 1e-4,
                "Tank {} statt {expected}",
                ship.fuel
            );
        }
        // Wer liegt, verbraucht nichts.
        move_fleets(&galaxy, &mut fleets, &data);
        assert!((fleets.list[0].min_fuel() - expected).abs() < 1e-4);
    }

    #[test]
    fn a_fleet_without_fuel_crawls_but_arrives() {
        let (data, galaxy, _, mut fleets) = setup();
        let balance = &data.balance.military;
        let home = fleets.list[0].system;
        let neighbor = galaxy.neighbors(home)[0];
        let length = galaxy.lane(neighbor.lane).length;
        let mut dry = fleets.clone();
        // Ein einziges Schiff ohne Treibstoff hält die ganze Flotte auf.
        dry.list[0].ships[0].fuel = 0.0;
        for fleets in [&mut fleets, &mut dry] {
            order_move(&galaxy, fleets, PolityId(0), FleetId(0), neighbor.system).unwrap();
        }
        let fast = travel(&galaxy, &mut fleets, &data, FleetId(0));
        let slow = travel(&galaxy, &mut dry, &data, FleetId(0));
        assert_eq!(dry.list[0].system, neighbor.system);
        let crawl = data.balance.fleets.speed * balance.dry_speed;
        assert_eq!(slow, (length / crawl).ceil() as u32);
        assert!(slow > fast, "trocken {slow} Tage, versorgt {fast} Tage");
        // Die übrigen Schiffe haben für die Strecke bezahlt, das leere bleibt leer.
        assert_eq!(dry.list[0].ships[0].fuel, 0.0);
        let expected = 1.0 - length * balance.fuel_per_distance;
        assert!((dry.list[0].ships[1].fuel - expected).abs() < 1e-4);
    }

    #[test]
    fn the_last_drop_carries_part_of_the_day() {
        // Reicht der Tank nur für einen Teil des Tages, zählt dieser Teil in voller Fahrt.
        let (travelled, arrived) = day_travel(0.01, 20.0, 0.001, 0.5, 1000.0);
        assert!(!arrived);
        assert!((travelled - (10.0 + 10.0 * 0.5)).abs() < 1e-4);
        // Voller Tank: volle Fahrt. Leerer Tank: nur die gedrosselte.
        assert_eq!(day_travel(1.0, 20.0, 0.001, 0.5, 1000.0), (20.0, false));
        assert_eq!(day_travel(0.0, 20.0, 0.001, 0.5, 1000.0), (10.0, false));
        // Das Ziel liegt näher als die Tagesstrecke: ankommen und nur den Rest fahren.
        assert_eq!(day_travel(1.0, 20.0, 0.001, 0.5, 7.0), (7.0, true));
    }

    #[test]
    fn blockade_needs_war_and_an_undefended_orbit() {
        let (data, galaxy, polities, mut fleets) = setup();
        let mut diplomacy = Diplomacy::default();
        let mut chronicle = crate::chronicle::Chronicle::default();
        let target = galaxy.planet(polities.list[1].capital.unwrap()).system;
        order_move(&galaxy, &mut fleets, PolityId(0), FleetId(0), target).unwrap();
        set_stance(&mut fleets, PolityId(0), FleetId(0), Stance::Blockade).unwrap();
        travel(&galaxy, &mut fleets, &data, FleetId(0));

        // Ohne Krieg keine Blockade.
        update_blockades(&galaxy, &mut fleets, &diplomacy);
        assert!(!fleets.is_blockaded(target));

        // Im Krieg verhindert die Heimatflotte die Sperre, solange sie im Orbit steht.
        crate::diplomacy::declare_war(
            &mut diplomacy,
            &polities,
            &mut chronicle,
            0,
            PolityId(0),
            PolityId(1),
        )
        .unwrap();
        update_blockades(&galaxy, &mut fleets, &diplomacy);
        assert!(!fleets.is_blockaded(target));

        // Zieht sie ab, steht die Sperre.
        let away = galaxy.neighbors(target)[0].system;
        order_move(&galaxy, &mut fleets, PolityId(1), FleetId(1), away).unwrap();
        travel(&galaxy, &mut fleets, &data, FleetId(1));
        update_blockades(&galaxy, &mut fleets, &diplomacy);
        assert!(fleets.is_blockaded(target));
    }
}

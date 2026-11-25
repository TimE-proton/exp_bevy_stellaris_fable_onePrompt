//! Grobe Gefechtsauflösung, vorläufig: Feindliche Flotten im selben System zehren einander
//! Tag für Tag auf. Der taktische Rechner aus [`crate::battle`] ersetzt diese Datei.

use super::{Fleet, Fleets, value_parts};
use crate::chronicle::{Chronicle, ChronicleKind};
use crate::data::GameData;
use crate::diplomacy::Diplomacy;
use crate::polity::Polities;
use crate::rng::{Domain, Rng};

/// Feindliche Flotten im selben System fügen einander je Tag einen Anteil ihrer Stärke als
/// Schaden zu. Der Schaden trifft die Rümpfe; ein Schiff ohne Rumpf fällt aus, eine Flotte
/// ohne Schiffe besteht nicht mehr.
pub fn resolve_skirmishes(
    fleets: &mut Fleets,
    diplomacy: &Diplomacy,
    polities: &Polities,
    chronicle: &mut Chronicle,
    data: &GameData,
    seed: u64,
    tick: u64,
) {
    let balance = &data.balance.fleets;
    let day = tick / crate::clock::TICKS_PER_DAY;
    let count = fleets.list.len();
    let mut damage = vec![0.0f32; count];
    let mut fighting = vec![false; count];
    for a in 0..count {
        if !fleets.list[a].is_docked() {
            continue;
        }
        for b in (a + 1)..count {
            let (first, second) = (&fleets.list[a], &fleets.list[b]);
            if !second.is_docked()
                || first.system != second.system
                || !diplomacy.at_war(first.owner, second.owner)
            {
                continue;
            }
            let mut rng = Rng::stream(seed, Domain::Battle, (a as u64) << 32 | b as u64, day);
            damage[b] += first.strength * balance.daily_damage * rng.range_f32(0.7, 1.3);
            damage[a] += second.strength * balance.daily_damage * rng.range_f32(0.7, 1.3);
            fighting[a] = true;
            fighting[b] = true;
        }
    }
    for index in 0..count {
        let fleet = &mut fleets.list[index];
        if !fleet.alive {
            continue;
        }
        if fighting[index] && !fleet.in_battle {
            chronicle.push(
                tick,
                ChronicleKind::War,
                Some(fleet.owner),
                Some(fleet.system),
                format!("{} steht im Gefecht.", fleet.name),
            );
        }
        fleet.in_battle = fighting[index];
        if damage[index] <= 0.0 {
            continue;
        }
        let lost = spread_damage(data, fleet, damage[index]);
        if fleet.ships.is_empty() {
            chronicle.push(
                tick,
                ChronicleKind::War,
                Some(fleet.owner),
                Some(fleet.system),
                format!(
                    "{} der Gemeinschaft {} hat im Gefecht alle Schiffe verloren und besteht nicht mehr.",
                    fleet.name,
                    polities.get(fleet.owner).name
                ),
            );
        } else if lost > 0 {
            chronicle.push(
                tick,
                ChronicleKind::War,
                Some(fleet.owner),
                Some(fleet.system),
                format!(
                    "{}: Verluste im Gefecht, {} ausgefallen.",
                    fleet.name,
                    if lost == 1 {
                        "ein Schiff".to_string()
                    } else {
                        format!("{lost} Schiffe")
                    }
                ),
            );
        }
    }
    fleets.refresh(data);
}

/// Verteilt Schaden (in Stärkepunkten) zu gleichen Teilen auf die Rümpfe. Kleine Schiffe
/// fallen so zuerst aus; was sie nicht mehr aufnehmen, trifft die übrigen. Gibt die Zahl
/// der ausgefallenen Schiffe zurück.
fn spread_damage(data: &GameData, fleet: &mut Fleet, damage: f32) -> usize {
    let per_value = data.balance.military.strength_per_value;
    // Stärkepunkte, die jedes Schiff über seinen Rumpf noch aufnehmen kann.
    let capacity: Vec<f32> = fleet
        .ships
        .iter()
        .map(|ship| {
            let (body, _) = value_parts(data, ship.design);
            (body * per_value) as f32 * ship.condition(data).max(0.0)
        })
        .collect();
    let mut afloat = vec![true; capacity.len()];
    let mut damage = damage.max(0.0);
    loop {
        let standing = afloat.iter().filter(|afloat| **afloat).count();
        if standing == 0 || damage <= 0.0 {
            break;
        }
        let share = damage / standing as f32;
        let mut any_lost = false;
        for index in 0..capacity.len() {
            if afloat[index] && capacity[index] <= share {
                afloat[index] = false;
                damage = (damage - capacity[index]).max(0.0);
                any_lost = true;
            }
        }
        if !any_lost {
            for (index, ship) in fleet.ships.iter_mut().enumerate() {
                if afloat[index] {
                    ship.hull *= (capacity[index] - share) / capacity[index];
                }
            }
            break;
        }
    }
    let before = fleet.ships.len();
    let mut index = 0;
    fleet.ships.retain(|_| {
        index += 1;
        afloat[index - 1]
    });
    before - fleet.ships.len()
}

#[cfg(test)]
mod tests {
    use super::super::testing::setup;
    use super::super::{Stance, fleet_strength, set_stance, update_blockades};
    use super::*;
    use crate::ids::{FleetId, PolityId};

    #[test]
    fn damage_is_spread_over_the_hulls_and_small_ships_fall_first() {
        let (data, _, _, mut fleets) = setup();
        let fleet = &mut fleets.list[0];
        let ships = fleet.ships.len();
        let before = fleet_strength(&data, fleet);

        // Ein leichter Treffer: Jedes Schiff verliert gleich viele Stärkepunkte, keines fällt aus.
        assert_eq!(spread_damage(&data, fleet, 6.0), 0);
        assert_eq!(fleet.ships.len(), ships);
        assert!((before - fleet_strength(&data, fleet) - 6.0).abs() < 1e-3);
        let cruiser = fleet.ships[0].hull;
        let corvette = fleet.ships[ships - 1].hull;
        assert!(
            corvette < cruiser && cruiser < 1.0,
            "die Korvette ({corvette}) leidet am gleichen Treffer mehr als der Kreuzer ({cruiser})"
        );

        // Ein schwerer Treffer, der von jedem Schiff etwas mehr verlangt, als eine Korvette
        // noch aufnehmen kann: Die Korvetten fallen aus, ihr Rest trifft die Großen.
        let dagger = fleet.ships[ships - 1].design;
        assert_eq!(data.designs[dagger as usize].key, "dolch");
        let corvette_points = (data.designs[dagger as usize].stats.value
            * data.balance.military.strength_per_value) as f32
            * corvette;
        let hit = (corvette_points + 0.5) * ships as f32;
        let before = fleet_strength(&data, fleet);
        let lost = spread_damage(&data, fleet, hit);
        assert_eq!(lost, 3, "die drei Korvetten fallen zuerst aus");
        assert!(fleet.ships.iter().all(|ship| ship.design != dagger));
        assert!(fleet.ships.iter().all(|ship| ship.hull > 0.0));
        assert!((before - fleet_strength(&data, fleet) - hit).abs() < 1e-2);

        // Mehr Schaden, als die Flotte aufnehmen kann: Nichts bleibt.
        let rest = fleet_strength(&data, fleet);
        let remaining = fleet.ships.len();
        assert_eq!(spread_damage(&data, fleet, rest + 1.0), remaining);
        assert!(fleet.ships.is_empty());
    }

    #[test]
    fn the_stronger_fleet_wins_pays_for_it_and_then_blockades() {
        let (data, galaxy, polities, mut fleets) = setup();
        let mut diplomacy = Diplomacy::default();
        let mut chronicle = Chronicle::default();
        let target = galaxy.planet(polities.list[1].capital.unwrap()).system;
        // Der Angreifer steht mit doppelter Flotte im Heimatsystem des Gegners.
        for (design, count) in data.balance.military.starting_fleet_ids.clone() {
            for _ in 0..count {
                fleets.add_ship(&data, FleetId(0), design).unwrap();
            }
        }
        let attacker_ships = fleets.list[0].ships.len();
        let start = data.balance.fleets.start_strength;
        assert!((fleets.list[0].strength - 2.0 * start).abs() < 1e-2);
        fleets.list[0].system = target;
        set_stance(&mut fleets, PolityId(0), FleetId(0), Stance::Blockade).unwrap();
        crate::diplomacy::declare_war(
            &mut diplomacy,
            &polities,
            &mut chronicle,
            0,
            PolityId(0),
            PolityId(1),
        )
        .unwrap();

        // Solange die Heimatflotte steht, gibt es keine Sperre, aber ein Gefecht.
        let mut days = 0;
        for day in 0..400 {
            resolve_skirmishes(
                &mut fleets,
                &diplomacy,
                &polities,
                &mut chronicle,
                &data,
                9,
                day * 20,
            );
            update_blockades(&galaxy, &mut fleets, &diplomacy);
            if !fleets.list[1].alive {
                break;
            }
            assert!(fleets.list[0].in_battle && fleets.list[1].in_battle);
            assert!(!fleets.is_blockaded(target));
            days += 1;
        }
        assert!(days >= 2, "ein Gefecht dieser Größe dauert mehrere Tage");
        assert!(!fleets.list[1].alive && fleets.list[0].alive);
        assert!(fleets.list[1].ships.is_empty());
        assert_eq!(fleets.list[1].strength, 0.0);
        // Auch der Sieger zahlt: Rümpfe beschädigt, Stärke gesunken, und sie stimmt mit den
        // Schiffen überein.
        let winner = &fleets.list[0];
        assert!(winner.strength < 2.0 * start);
        assert!(winner.ships.iter().any(|ship| ship.hull < 1.0));
        assert!(winner.ships.len() <= attacker_ships);
        assert_eq!(fleets.find_anomaly(&data), None);
        // Der Orbit ist frei, die Sperre steht.
        assert!(fleets.is_blockaded(target));
        // Das Gefecht ist vorbei.
        resolve_skirmishes(
            &mut fleets,
            &diplomacy,
            &polities,
            &mut chronicle,
            &data,
            9,
            401 * 20,
        );
        assert!(!fleets.list[0].in_battle);
        assert!(
            chronicle
                .entries
                .iter()
                .any(|entry| entry.text.contains("besteht nicht mehr"))
        );
    }
}

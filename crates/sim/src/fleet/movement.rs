//! Tagesschritt auf der Karte: Marsch entlang der Routen und die Blockaderegel.

use super::{Battles, Fleet, Fleets, Stance, map_speed};
use crate::data::GameData;
use crate::diplomacy::Diplomacy;
use crate::galaxy::Galaxy;
use crate::tech::Research;

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
/// Streckeneinheit denselben Anteil jedes Tanks. Die Fahrt folgt der Antriebstechnik der
/// Gemeinschaft ([`map_speed`]); der Verbrauch je Strecke bleibt gleich. Wer in einem
/// System liegt, verbraucht `fuel_per_day` seines Tanks: Im eigenen Gebiet fasst die
/// Flotte ihn nach, fern davon läuft sie trocken. Im Gefecht rechnet der Gefechtsrechner
/// den Verbrauch.
pub fn move_fleets(galaxy: &Galaxy, fleets: &mut Fleets, research: &Research, data: &GameData) {
    let balance = &data.balance.military;
    for fleet in &mut fleets.list {
        if !fleet.alive || fleet.in_battle {
            continue;
        }
        if fleet.route.is_empty() {
            for ship in &mut fleet.ships {
                ship.fuel = (ship.fuel - balance.fuel_per_day).max(0.0);
            }
            continue;
        }
        let speed = map_speed(data, research, fleet.owner);
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
            // Die Flotte merkt sich, woher sie kam: ihr Rückweg nach einem verlorenen Gefecht.
            fleet.came_from = Some(fleet.system);
            fleet.system = fleet.route.remove(0);
            fleet.progress = 0.0;
        } else {
            fleet.progress += travelled / length;
        }
    }
}

/// Ein System ist blockiert, wenn eine feindliche Flotte im Blockadeauftrag dort steht
/// und der Besitzer keine eigene Flotte im Orbit hat: Erst die orbitale Überlegenheit,
/// dann die Sperre. Wer sich nach einem Rückzug sammelt, sperrt nicht und hält keinen
/// Orbit.
///
/// Ein Gefecht ändert daran erst etwas, wenn es entschieden ist. Bis dahin gilt, was vor
/// ihm galt: Eine bestehende Sperre hält, solange die Blockadeflotte auf dem Feld kämpft,
/// und fällt erst, wenn sie es räumt. Ein Entsatz muss sie also schlagen; anzukommen und
/// wieder abzudrehen öffnet das System nicht. Umgekehrt bleibt ein verteidigter Orbit
/// offen, solange die Verteidiger kämpfen. Wer im Gefecht nur noch flieht oder treibt,
/// zählt für keine Seite.
pub fn update_blockades(
    galaxy: &Galaxy,
    fleets: &mut Fleets,
    diplomacy: &Diplomacy,
    battles: &Battles,
) {
    // Steht die Flotte für ihre Aufgabe bereit: einsatzbereit, oder im Gefecht und noch
    // kämpfend?
    let stands = |fleet: &Fleet| {
        !fleet.is_regrouping() && (!fleet.in_battle || battles.is_fighting(fleet.id))
    };
    // Eine Sperre will gefahren sein: Ohne Treibstoff hält sie niemand aufrecht.
    let mut blockaded = Vec::new();
    for system in &galaxy.systems {
        let Some(owner) = galaxy.system_owner(system.id) else {
            continue;
        };
        let was_blockaded = fleets.is_blockaded(system.id);
        // Wer in eine bestehende Sperre hinein angreift, hebt sie erst mit dem Sieg auf.
        let defended = fleets.docked_in(system.id).any(|fleet| {
            fleet.owner == owner && stands(fleet) && !(was_blockaded && fleet.in_battle)
        });
        let besieged = fleets.docked_in(system.id).any(|fleet| {
            fleet.stance == Stance::Blockade
                && diplomacy.at_war(fleet.owner, owner)
                && stands(fleet)
                && fleet.has_fuel()
        });
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

    /// Marschiert mit der Technik aus `research`, bis die Flotte angekommen ist, und gibt
    /// die Tage zurück.
    fn travel_with(
        galaxy: &Galaxy,
        fleets: &mut Fleets,
        research: &Research,
        data: &GameData,
        fleet: FleetId,
    ) -> u32 {
        for day in 0..5000 {
            if fleets.list[fleet.index()].route.is_empty() {
                return day;
            }
            move_fleets(galaxy, fleets, research, data);
        }
        panic!("Flotte kam nie an");
    }

    /// Marschiert ohne Antriebstechnik, bis die Flotte angekommen ist.
    fn travel(galaxy: &Galaxy, fleets: &mut Fleets, data: &GameData, fleet: FleetId) -> u32 {
        travel_with(galaxy, fleets, &Research::default(), data, fleet)
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
        // Auch wer liegt, verbraucht: je Tag einen festen Anteil des Tanks, bis er leer ist.
        let idle = balance.fuel_per_day;
        assert!(idle > 0.0 && idle < 0.05);
        move_fleets(&galaxy, &mut fleets, &Research::default(), &data);
        assert!((fleets.list[0].min_fuel() - (expected - idle)).abs() < 1e-4);
        let days = (1.0 / idle).ceil() as usize;
        for _ in 0..days {
            move_fleets(&galaxy, &mut fleets, &Research::default(), &data);
        }
        assert_eq!(fleets.list[0].min_fuel(), 0.0);
        assert!(!fleets.list[0].has_fuel());
        // Im Gefecht rechnet der Gefechtsrechner den Verbrauch, nicht die Karte.
        let (_, _, _, mut busy) = setup();
        busy.list[0].in_battle = true;
        move_fleets(&galaxy, &mut busy, &Research::default(), &data);
        assert_eq!(busy.list[0].min_fuel(), 1.0);
    }

    #[test]
    fn a_blockade_needs_fuel() {
        let (data, galaxy, polities, mut fleets) = setup();
        let mut diplomacy = Diplomacy::default();
        diplomacy.wars.insert((PolityId(0), PolityId(1)));
        let target = galaxy.planet(polities.list[1].capital.unwrap()).system;
        let away = galaxy.neighbors(target)[0].system;
        fleets.list[1].system = away;
        fleets.list[0].system = target;
        fleets.list[0].stance = Stance::Blockade;
        let blockaded = |fleets: &mut Fleets| {
            update_blockades(&galaxy, fleets, &diplomacy, &Battles::default());
            fleets.is_blockaded(target)
        };
        assert!(blockaded(&mut fleets));
        // Fern der Heimat fasst niemand nach: Tag für Tag sinken die Tanks, und mit dem
        // letzten Tropfen fällt die Sperre.
        let days = (1.0 / data.balance.military.fuel_per_day).ceil() as usize;
        for day in 0..days {
            assert!(blockaded(&mut fleets), "Tag {day}");
            move_fleets(&galaxy, &mut fleets, &Research::default(), &data);
        }
        assert!(!fleets.list[0].has_fuel());
        assert!(!blockaded(&mut fleets), "ohne Treibstoff keine Sperre");
        // Ein einziges trockenes Schiff genügt, um die Flotte festzulegen; versorgt sperrt
        // sie wieder.
        for ship in &mut fleets.list[0].ships[1..] {
            ship.fuel = 1.0;
        }
        assert!(!blockaded(&mut fleets));
        fleets.list[0].ships[0].fuel = 0.2;
        assert!(blockaded(&mut fleets));
        // Kaperfahrt und Geleit brauchen ihn ebenso.
        fleets.list[0].stance = Stance::Raid;
        let strength = fleets.list[0].strength;
        assert_eq!(
            fleets.raid_pressure(&diplomacy, target, PolityId(1)).0,
            strength
        );
        fleets.list[0].ships[0].fuel = 0.0;
        assert_eq!(fleets.raid_pressure(&diplomacy, target, PolityId(1)).0, 0.0);
    }

    #[test]
    fn propulsion_technology_makes_a_fleet_faster_on_the_map() {
        let (data, galaxy, _, fleets) = setup();
        let me = PolityId(0);
        let base = data.balance.fleets.speed;
        let mut research = Research::default();
        assert_eq!(map_speed(&data, &research, me), base);
        assert!(research.grant(&data, me, data.tech_index("antrieb_1").unwrap()));
        let factor = research.effects(me).fleet_speed as f32;
        assert!(
            factor > 1.0,
            "die erste Antriebsstufe hebt die Flottenfahrt"
        );
        assert_eq!(map_speed(&data, &research, me), base * factor);
        // Die Technik gehört der Gemeinschaft: Andere fahren weiter mit dem Grundwert.
        assert_eq!(map_speed(&data, &research, PolityId(1)), base);

        // Der längste Weg aus dem Heimatsystem zu einem Nachbarn, damit die Tage zählen.
        let home = fleets.list[0].system;
        let neighbor = *galaxy
            .neighbors(home)
            .iter()
            .max_by(|a, b| {
                galaxy
                    .lane(a.lane)
                    .length
                    .total_cmp(&galaxy.lane(b.lane).length)
            })
            .unwrap();
        let length = galaxy.lane(neighbor.lane).length;
        let (mut slow, mut fast) = (fleets.clone(), fleets.clone());
        for fleets in [&mut slow, &mut fast] {
            order_move(&galaxy, fleets, me, FleetId(0), neighbor.system).unwrap();
        }
        let plain = travel(&galaxy, &mut slow, &data, FleetId(0));
        let boosted = travel_with(&galaxy, &mut fast, &research, &data, FleetId(0));
        assert_eq!(plain, (length / base).ceil() as u32);
        assert_eq!(boosted, (length / (base * factor)).ceil() as u32);
        assert!(boosted <= plain);
        // Der Verbrauch hängt an der Strecke, nicht an der Fahrt.
        assert!((slow.list[0].min_fuel() - fast.list[0].min_fuel()).abs() < 1e-4);
        // Was die Flotte an einem Tag schafft, zeigt auch ihre Tagesreichweite.
        assert_eq!(fast.list[0].day_range(&data, &research), base * factor);
        assert_eq!(slow.list[0].day_range(&data, &Research::default()), base);
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
        update_blockades(&galaxy, &mut fleets, &diplomacy, &Battles::default());
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
        update_blockades(&galaxy, &mut fleets, &diplomacy, &Battles::default());
        assert!(!fleets.is_blockaded(target));

        // Zieht sie ab, steht die Sperre.
        let away = galaxy.neighbors(target)[0].system;
        order_move(&galaxy, &mut fleets, PolityId(1), FleetId(1), away).unwrap();
        travel(&galaxy, &mut fleets, &data, FleetId(1));
        update_blockades(&galaxy, &mut fleets, &diplomacy, &Battles::default());
        assert!(fleets.is_blockaded(target));
    }

    /// Stellt die genannten Flotten als Parteien eines Gefechts in `system` auf; alle sind
    /// einander feind, wie es die Kriegslage sagt.
    fn battle_of(
        data: &GameData,
        fleets: &mut Fleets,
        diplomacy: &Diplomacy,
        system: crate::ids::SystemId,
        members: &[usize],
    ) -> Battles {
        use crate::battle::{Battle, CombatantSetup};
        let mut battle = Battle::new(crate::ids::BattleId(0), system, 1);
        for index in members {
            let fleet = &mut fleets.list[*index];
            fleet.system = system;
            let setup = CombatantSetup {
                fleet: Some(fleet.id),
                ..CombatantSetup::new(fleet.owner, data)
            };
            battle.add_combatant(data, setup, &fleet.specs());
            fleet.battle = Some(battle.id);
            fleet.in_battle = true;
        }
        for a in 0..members.len() {
            for b in 0..members.len() {
                let hostile =
                    diplomacy.at_war(fleets.list[members[a]].owner, fleets.list[members[b]].owner);
                battle.set_hostile(a, b, hostile);
            }
        }
        Battles {
            active: vec![battle],
            next_id: 1,
            ..Default::default()
        }
    }

    #[test]
    fn whoever_regroups_neither_blockades_nor_holds_an_orbit() {
        let (_, galaxy, polities, mut fleets) = setup();
        let mut diplomacy = Diplomacy::default();
        diplomacy.wars.insert((PolityId(0), PolityId(1)));
        let target = galaxy.planet(polities.list[1].capital.unwrap()).system;
        let away = galaxy.neighbors(target)[0].system;
        let blockaded = |fleets: &mut Fleets| {
            update_blockades(&galaxy, fleets, &diplomacy, &Battles::default());
            fleets.is_blockaded(target)
        };
        // Der Verteidiger ist fort, der Angreifer liegt im Blockadeauftrag im System.
        fleets.list[1].system = away;
        fleets.list[0].system = target;
        fleets.list[0].stance = Stance::Blockade;
        assert!(blockaded(&mut fleets));
        // Sammelt er sich, ist das System offen.
        fleets.list[0].regroup_until = Some(40);
        assert!(!blockaded(&mut fleets));
        fleets.list[0].regroup_until = None;
        assert!(blockaded(&mut fleets));
        // Ein Verteidiger im Orbit verhindert die Sperre; einer, der sich nur sammelt,
        // hält den Orbit nicht.
        fleets.list[1].system = target;
        assert!(!blockaded(&mut fleets));
        fleets.list[1].regroup_until = Some(40);
        assert!(blockaded(&mut fleets));
    }

    #[test]
    fn a_battle_changes_a_blockade_only_once_it_is_decided() {
        use crate::battle::ShipStatus;
        let (data, galaxy, polities, mut fleets) = setup();
        let mut diplomacy = Diplomacy::default();
        diplomacy.wars.insert((PolityId(0), PolityId(1)));
        let target = galaxy.planet(polities.list[1].capital.unwrap()).system;
        let away = galaxy.neighbors(target)[0].system;
        fleets.list[0].stance = Stance::Blockade;
        let rout = |battles: &mut Battles, side: u16| {
            for ship in &mut battles.active[0].ships {
                if ship.side == side {
                    ship.status = ShipStatus::Routing;
                }
            }
        };

        // Der Blockierer trifft auf einen verteidigten Orbit: Solange die Verteidiger
        // kämpfen, bleibt das System offen.
        let mut open = fleets.clone();
        let mut battles = battle_of(&data, &mut open, &diplomacy, target, &[1, 0]);
        update_blockades(&galaxy, &mut open, &diplomacy, &battles);
        assert!(!open.is_blockaded(target));
        assert!(battles.is_fighting(open.list[0].id) && battles.is_fighting(open.list[1].id));
        // Fliehen die Verteidiger, halten sie den Orbit nicht mehr, und die Sperre steht.
        rout(&mut battles, 0);
        assert!(!battles.is_fighting(open.list[1].id));
        update_blockades(&galaxy, &mut open, &diplomacy, &battles);
        assert!(open.is_blockaded(target));

        // Die Sperre steht schon; die Heimatflotte kehrt zurück und greift an. Anzukommen
        // öffnet das System nicht: Die Sperre hält, solange der Blockierer kämpft.
        let mut closed = fleets.clone();
        closed.list[0].system = target;
        closed.list[1].system = away;
        update_blockades(&galaxy, &mut closed, &diplomacy, &Battles::default());
        assert!(closed.is_blockaded(target));
        let mut battles = battle_of(&data, &mut closed, &diplomacy, target, &[0, 1]);
        for _ in 0..3 {
            update_blockades(&galaxy, &mut closed, &diplomacy, &battles);
            assert!(closed.is_blockaded(target), "der Entsatz muss erst siegen");
        }
        // Dreht der Entsatz wieder ab, ändert sich nichts.
        let mut turned = battles.clone();
        rout(&mut turned, 1);
        update_blockades(&galaxy, &mut closed, &diplomacy, &turned);
        assert!(closed.is_blockaded(target));
        // Räumt der Blockierer das Feld, ist das System offen, und am Tag darauf hält die
        // Heimatflotte den Orbit, auch wenn noch geschossen wird.
        rout(&mut battles, 0);
        update_blockades(&galaxy, &mut closed, &diplomacy, &battles);
        assert!(!closed.is_blockaded(target));
        update_blockades(&galaxy, &mut closed, &diplomacy, &battles);
        assert!(!closed.is_blockaded(target));

        // Ein Dritter, der nur mit dem Blockierer im Krieg liegt, greift ihn an: Auch dann
        // hält die Sperre, bis der Blockierer das Feld räumt.
        diplomacy.wars.insert((PolityId(0), PolityId(2)));
        let mut third = fleets.clone();
        third.list[0].system = target;
        third.list[1].system = away;
        update_blockades(&galaxy, &mut third, &diplomacy, &Battles::default());
        assert!(third.is_blockaded(target));
        let mut battles = battle_of(&data, &mut third, &diplomacy, target, &[0, 2]);
        update_blockades(&galaxy, &mut third, &diplomacy, &battles);
        assert!(third.is_blockaded(target));
        rout(&mut battles, 0);
        update_blockades(&galaxy, &mut third, &diplomacy, &battles);
        assert!(!third.is_blockaded(target));
    }

    #[test]
    fn a_fleet_remembers_the_system_it_came_from() {
        let (data, galaxy, _, mut fleets) = setup();
        let home = fleets.list[0].system;
        assert_eq!(fleets.list[0].came_from, None);
        let first = galaxy.neighbors(home)[0].system;
        order_move(&galaxy, &mut fleets, PolityId(0), FleetId(0), first).unwrap();
        travel(&galaxy, &mut fleets, &data, FleetId(0));
        assert_eq!(fleets.list[0].came_from, Some(home));
        // Zurück: Jetzt kommt sie aus dem Nachbarsystem.
        order_move(&galaxy, &mut fleets, PolityId(0), FleetId(0), home).unwrap();
        travel(&galaxy, &mut fleets, &data, FleetId(0));
        assert_eq!(fleets.list[0].came_from, Some(first));
        // Wer im Gefecht steht, marschiert nicht und nimmt keinen Marschbefehl an.
        fleets.list[0].in_battle = true;
        let error = order_move(&galaxy, &mut fleets, PolityId(0), FleetId(0), first).unwrap_err();
        assert!(error.contains("Rückzug"), "{error}");
        assert!(fleets.list[0].route.is_empty());
    }
}

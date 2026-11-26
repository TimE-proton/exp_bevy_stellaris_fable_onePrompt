//! Was mit Flotten, Bauaufträgen und laufenden Gefechten geschieht, wenn eine Welt den
//! Besitzer wechselt, eine Gemeinschaft entsteht oder eine endet. Aufgerufen nur aus
//! [`crate::polity::succession`]; dort steht, wann welcher Fall eintritt.
//!
//! Wo ein Eintrag je Gemeinschaft, eine Flotte oder ein Bauauftrag neu beschriftet wird,
//! ist die Struktur erschöpfend zerlegt: Ein später ergänztes Feld lässt diese Datei nicht
//! mehr übersetzen, statt stillschweigend beim alten Besitzer zu bleiben.

use super::{Battles, BuildOrder, Fleet, Fleets};
use crate::battle::{Battle, ShipStatus};
use crate::chronicle::{Chronicle, ChronicleKind};
use crate::data::GameData;
use crate::diplomacy::Diplomacy;
use crate::galaxy::Galaxy;
use crate::ids::{PlanetId, PolityId};
use crate::polity::Polities;

/// Bringt die Listen je Gemeinschaft auf `count` Einträge: zu Spielbeginn für alle, bei
/// einer Gründung für die eine neue. Wer hinzukommt, hat keinen Soldrückstand und keine
/// Schiffe verloren; bestehende Einträge bleiben.
pub(crate) fn size_for_polities(fleets: &mut Fleets, battles: &mut Battles, count: usize) {
    let Fleets {
        list: _,
        blockaded: _,
        orders: _,
        next_ship: _,
        next_order: _,
        arrears,
    } = fleets;
    arrears.resize(count, 0.0);
    let Battles {
        active: _,
        reports: _,
        debris: _,
        next_id: _,
        ships_lost,
    } = battles;
    ships_lost.resize(count, 0);
}

/// Länge der Listen je Gemeinschaft, für die Prüfung der Invarianten: Soldrückstand und
/// verlorene Schiffe.
pub(crate) fn per_polity_lengths(fleets: &Fleets, battles: &Battles) -> [(&'static str, usize); 2] {
    [
        ("Soldrückstand", fleets.arrears.len()),
        ("verlorene Schiffe", battles.ships_lost.len()),
    ]
}

/// Streicht die Bauaufträge, die `owner` bei der Werft einer Welt stehen hat, weil die
/// Welt ihm nicht mehr gehört. Was schon verbaut ist, bleibt bezahlt: Bauteile und
/// Werftaufschlag sind bei Kontor und Rüstungsfirma, erstattet wird nichts. Gibt die Zahl
/// der gestrichenen Aufträge zurück.
pub(crate) fn cancel_orders_at(
    fleets: &mut Fleets,
    galaxy: &Galaxy,
    chronicle: &mut Chronicle,
    data: &GameData,
    planet: PlanetId,
    owner: PolityId,
    tick: u64,
) -> usize {
    let world = galaxy.planet(planet);
    let before = fleets.orders.len();
    fleets.orders.retain(|order| {
        if order.planet != planet || order.polity != owner {
            return true;
        }
        let design = &data.designs[order.design as usize];
        chronicle.push(
            tick,
            ChronicleKind::War,
            Some(owner),
            Some(world.system),
            format!(
                "Werft {}: Der Auftrag über {} {} verfällt, die Welt hat den Besitzer gewechselt.",
                world.name,
                design.stats.class.label(),
                design.name
            ),
        );
        false
    });
    before - fleets.orders.len()
}

/// Flotten und Bauaufträge von `from` gehen an `into`: friedlicher Weg. Jede Flotte behält
/// Schiffe, Ort, Marschweg, Auftrag, stehende Befehle und Sammelfrist und bekommt die
/// nächste Nummer in der Zählung der neuen Gemeinschaft. Steht sie im Gefecht, kämpft ihre
/// Partei fortan für `into`; gegen wen, sagt danach [`sync_hostility`]. Gibt die Zahl der
/// übergebenen Flotten zurück.
pub(crate) fn pass_fleets(
    fleets: &mut Fleets,
    battles: &mut Battles,
    polities: &Polities,
    from: PolityId,
    into: PolityId,
) -> usize {
    let short = &polities.get(into).short;
    // Nummeriert wird wie bei jeder neuen Flotte: fortlaufend, aufgelöste zählen mit.
    let mut number = fleets
        .list
        .iter()
        .filter(|fleet| fleet.owner == into)
        .count();
    let mut passed = 0;
    for fleet in &mut fleets.list {
        let Fleet {
            id: _,
            owner,
            name,
            strength: _,
            stance: _,
            system: _,
            route: _,
            progress: _,
            alive,
            in_battle: _,
            ships: _,
            formation: _,
            doctrine: _,
            battle: _,
            came_from: _,
            regroup_until: _,
        } = fleet;
        // Aufgelöste Flotten bleiben, was sie waren: Geschichte der alten Gemeinschaft.
        if *owner != from || !*alive {
            continue;
        }
        number += 1;
        *owner = into;
        *name = format!("{number}. Flotte ({short})");
        passed += 1;
    }
    for order in &mut fleets.orders {
        let BuildOrder {
            id: _,
            polity,
            planet: _,
            design: _,
            progress: _,
            spent: _,
            stalled: _,
            started: _,
        } = order;
        if *polity == from {
            *polity = into;
        }
    }
    relabel_combatants(battles, from, into);
    passed
}

/// Flotten und Bauaufträge von `from` werden aufgelöst: erzwungener Weg. Schiffe und
/// angefangene Bauten sind fort; wer im Gefecht stand, verlässt das Feld, ohne dass es als
/// Verlust zählt. Die Parteien bleiben bis zum Ende ihres Gefechts in dessen Bilanz stehen
/// und werden `into` zugerechnet, damit nichts mehr auf die beendete Gemeinschaft zeigt.
/// Gibt die Zahl der aufgelösten Flotten und der gestrichenen Aufträge zurück.
pub(crate) fn dissolve_fleets(
    fleets: &mut Fleets,
    battles: &mut Battles,
    data: &GameData,
    from: PolityId,
    into: PolityId,
) -> (usize, usize) {
    let mut dissolved = 0;
    for fleet in &mut fleets.list {
        if fleet.owner != from || !fleet.alive {
            continue;
        }
        if let Some(battle) = fleet
            .battle
            .and_then(|id| battles.active.iter_mut().find(|battle| battle.id == id))
        {
            leave_field(battle, fleet);
        }
        fleet.ships.clear();
        dissolved += 1;
    }
    // Eine Flotte ohne Schiffe besteht nicht mehr: weder im Gefecht noch auf dem Marsch.
    fleets.refresh(data);
    let before = fleets.orders.len();
    fleets.orders.retain(|order| order.polity != from);
    relabel_combatants(battles, from, into);
    (dissolved, before - fleets.orders.len())
}

/// Nimmt die Schiffe einer Flotte aus ihrem laufenden Gefecht: Sie gelten als dem Feld
/// entkommen, ihre Geschosse im Flug verfallen.
fn leave_field(battle: &mut Battle, fleet: &Fleet) {
    let Some(side) = battle
        .combatants
        .iter()
        .position(|combatant| combatant.fleet == Some(fleet.id))
    else {
        return;
    };
    let combatant = &mut battle.combatants[side];
    for ship in &mut battle.ships {
        if ship.side as usize == side && ship.status.in_play() {
            ship.status = ShipStatus::Escaped;
            combatant.escaped[ship.class.index()] += 1;
        }
    }
    battle.shots.retain(|shot| shot.side as usize != side);
}

/// Parteien laufender Gefechte, die für `from` kämpften, kämpfen für `into`.
fn relabel_combatants(battles: &mut Battles, from: PolityId, into: PolityId) {
    for battle in &mut battles.active {
        for combatant in &mut battle.combatants {
            if combatant.owner == from {
                combatant.owner = into;
            }
        }
    }
}

/// Wer in den laufenden Gefechten gegen wen kämpft, folgt der Kriegslage: dieselbe Regel,
/// die [`super::open_battles`] jeden Morgen anwendet. Nach einem Besitzwechsel, einem Ende
/// oder einer Unterstellung gilt sie sofort, nicht erst am nächsten Tag: Ein Gefecht, in
/// dem niemand mehr feind ist, stellt der Rechner von selbst ein.
pub(crate) fn sync_hostility(battles: &mut Battles, diplomacy: &Diplomacy) {
    for battle in &mut battles.active {
        for a in 0..battle.combatants.len() {
            for b in (a + 1)..battle.combatants.len() {
                let hostile =
                    diplomacy.at_war(battle.combatants[a].owner, battle.combatants[b].owner);
                battle.set_hostile(a, b, hostile);
            }
        }
    }
}

/// Erste Stelle, an der Flotten, Bauaufträge oder laufende Gefechte noch auf eine
/// Gemeinschaft zeigen, die geendet hat. Aufgelöste Flotten und die Bilanzen beendeter
/// Gefechte sind Geschichte und zählen nicht.
pub(crate) fn reference_to_ended(
    fleets: &Fleets,
    battles: &Battles,
    polities: &Polities,
) -> Option<String> {
    let ended = |polity: PolityId| {
        !polities
            .list
            .get(polity.index())
            .is_some_and(|found| found.alive)
    };
    if let Some(fleet) = fleets
        .list
        .iter()
        .find(|fleet| fleet.alive && ended(fleet.owner))
    {
        return Some(format!(
            "{} gehört {}, einer Gemeinschaft, die geendet hat",
            fleet.name, fleet.owner
        ));
    }
    if let Some(order) = fleets.orders.iter().find(|order| ended(order.polity)) {
        return Some(format!(
            "Bauauftrag {} gehört {}, einer Gemeinschaft, die geendet hat",
            order.id, order.polity
        ));
    }
    for battle in &battles.active {
        if let Some(combatant) = battle
            .combatants
            .iter()
            .find(|combatant| ended(combatant.owner))
        {
            return Some(format!(
                "Gefecht {}: eine Partei kämpft für {}, eine Gemeinschaft, die geendet hat",
                battle.id, combatant.owner
            ));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::super::testing::setup;
    use super::super::{open_battles, order_move, order_ship};
    use super::*;
    use crate::economy::setup_economy;
    use crate::finance::Finance;
    use crate::ids::FleetId;
    use crate::tech::Research;

    /// Zwei Startflotten im Gefecht im Heimatsystem der zweiten.
    fn fighting() -> (GameData, Galaxy, Polities, Fleets, Battles, Diplomacy) {
        let (data, galaxy, polities, mut fleets) = setup();
        let mut diplomacy = Diplomacy::default();
        diplomacy.wars.insert((PolityId(0), PolityId(1)));
        let mut battles = Battles::default();
        fleets.list[0].system = fleets.list[1].system;
        open_battles(
            &galaxy,
            &mut fleets,
            &mut battles,
            &diplomacy,
            &Finance::default(),
            &Research::default(),
            &mut Chronicle::default(),
            &data,
            1,
            0,
        );
        assert_eq!(battles.active.len(), 1);
        assert!(fleets.list[0].in_battle && fleets.list[1].in_battle);
        (data, galaxy, polities, fleets, battles, diplomacy)
    }

    #[test]
    fn a_new_polity_gets_an_entry_in_every_list() {
        let (_, _, polities, mut fleets) = setup();
        let mut battles = Battles::default();
        let count = polities.list.len();
        // Beide Listen wuchsen früher erst bei Bedarf; so stehen sie vollständig.
        assert!(fleets.arrears.is_empty() && battles.ships_lost.is_empty());
        size_for_polities(&mut fleets, &mut battles, count);
        assert_eq!(fleets.arrears, vec![0.0; count]);
        assert_eq!(battles.ships_lost, vec![0; count]);
        // Bei einer Gründung kommt genau ein Eintrag hinzu; die bestehenden bleiben.
        fleets.arrears[2] = 0.5;
        battles.ships_lost[1] = 3;
        size_for_polities(&mut fleets, &mut battles, count + 1);
        for (_, length) in per_polity_lengths(&fleets, &battles) {
            assert_eq!(length, count + 1);
        }
        assert_eq!((fleets.arrears[2], fleets.arrears[count]), (0.5, 0.0));
        assert_eq!((battles.ships_lost[1], battles.ships_lost[count]), (3, 0));
    }

    #[test]
    fn orders_of_the_old_owner_lapse_with_the_world() {
        let (data, galaxy, polities, mut fleets) = setup();
        let economy = setup_economy(&data, &galaxy, &polities);
        let mut chronicle = Chronicle::default();
        let corvette = data.design_index("dolch").unwrap();
        let home = |index: usize| polities.list[index].capital.unwrap();
        for (polity, count) in [(0usize, 2), (1usize, 1)] {
            for _ in 0..count {
                order_ship(
                    &data,
                    &galaxy,
                    &economy,
                    &polities,
                    &mut fleets,
                    PolityId(polity as u32),
                    home(polity),
                    corvette,
                    &[],
                    0,
                )
                .unwrap();
            }
        }
        fleets.orders[0].spent = 5_000;
        // Nur die Aufträge des alten Besitzers an dieser Werft verfallen.
        let gone = cancel_orders_at(
            &mut fleets,
            &galaxy,
            &mut chronicle,
            &data,
            home(0),
            PolityId(0),
            40,
        );
        assert_eq!(gone, 2);
        assert_eq!(fleets.orders.len(), 1);
        assert_eq!(fleets.orders[0].polity, PolityId(1));
        assert_eq!(chronicle.entries.len(), 2);
        assert!(chronicle.entries[0].text.contains("verfällt"));
        // Ein zweiter Aufruf findet nichts mehr; fremde Aufträge bleiben unberührt.
        let none = cancel_orders_at(
            &mut fleets,
            &galaxy,
            &mut chronicle,
            &data,
            home(1),
            PolityId(0),
            41,
        );
        assert_eq!((none, fleets.orders.len()), (0, 1));
        assert_eq!(fleets.find_anomaly(&data), None);
    }

    #[test]
    fn passed_fleets_keep_everything_but_owner_and_name() {
        let (data, galaxy, polities, mut fleets, mut battles, mut diplomacy) = fighting();
        let economy = setup_economy(&data, &galaxy, &polities);
        let (from, into, third) = (PolityId(1), PolityId(2), PolityId(0));
        let capital = polities.get(from).capital.unwrap();
        order_ship(
            &data,
            &galaxy,
            &economy,
            &polities,
            &mut fleets,
            from,
            capital,
            data.design_index("dolch").unwrap(),
            &[],
            0,
        )
        .unwrap();
        // Eine längst aufgelöste Flotte der alten Gemeinschaft bleibt Geschichte.
        let ghost = fleets.new_fleet(&polities, from, fleets.list[1].system);
        fleets.refresh(&data);
        assert!(!fleets.list[ghost.index()].alive);
        let before = fleets.list[1].clone();

        assert_eq!(
            pass_fleets(&mut fleets, &mut battles, &polities, from, into),
            1
        );
        let passed = &fleets.list[1];
        assert_eq!(passed.owner, into);
        assert_eq!(
            passed.name,
            format!("2. Flotte ({})", polities.get(into).short),
            "die nächste Nummer der neuen Gemeinschaft"
        );
        assert_eq!(passed.ships, before.ships);
        assert_eq!(
            (passed.system, passed.battle, passed.in_battle),
            (before.system, before.battle, true)
        );
        assert_eq!(fleets.list[ghost.index()].owner, from);
        assert_eq!(fleets.orders[0].polity, into);
        // Im Gefecht kämpft die Partei jetzt für die neue Gemeinschaft …
        let battle = &battles.active[0];
        let side = battle
            .combatants
            .iter()
            .position(|combatant| combatant.fleet == Some(FleetId(1)))
            .unwrap();
        assert_eq!(battle.combatants[side].owner, into);
        assert!(
            battle.is_hostile(0, 1),
            "bis die Kriegslage nachgeführt ist"
        );
        // … und weil die mit niemandem im Krieg liegt, ist danach niemand mehr feind.
        sync_hostility(&mut battles, &diplomacy);
        assert!(!battles.active[0].is_hostile(0, 1));
        // Liegt sie mit dem Gegner im Krieg, geht das Gefecht weiter.
        diplomacy.wars.insert((third, into));
        sync_hostility(&mut battles, &diplomacy);
        assert!(battles.active[0].is_hostile(0, 1));
        assert_eq!(fleets.find_anomaly(&data), None);
        assert_eq!(battles.find_anomaly(&fleets), None);
        assert_eq!(reference_to_ended(&fleets, &battles, &polities), None);
    }

    #[test]
    fn dissolved_fleets_leave_the_field_the_road_and_the_yard() {
        let (data, galaxy, mut polities, mut fleets, mut battles, mut diplomacy) = fighting();
        let economy = setup_economy(&data, &galaxy, &polities);
        let (from, into) = (PolityId(1), PolityId(0));
        let capital = polities.get(from).capital.unwrap();
        // Eine zweite Flotte derselben Gemeinschaft ist auf dem Marsch, ein Schiff im Bau.
        let ships: Vec<_> = fleets.list[2].ships.iter().map(|ship| ship.id).collect();
        fleets.list[2].owner = from;
        let away = galaxy.neighbors(fleets.list[2].system)[0].system;
        order_move(&galaxy, &mut fleets, from, FleetId(2), away).unwrap();
        fleets.list[2].progress = 0.4;
        order_ship(
            &data,
            &galaxy,
            &economy,
            &polities,
            &mut fleets,
            from,
            capital,
            data.design_index("dolch").unwrap(),
            &[],
            0,
        )
        .unwrap();
        let fielded = battles.active[0]
            .ships
            .iter()
            .filter(|ship| ship.side == 1)
            .count();
        assert!(fielded > 0 && !ships.is_empty());

        // Solange die Gemeinschaft lebt, zeigt nichts auf eine beendete.
        assert_eq!(reference_to_ended(&fleets, &battles, &polities), None);
        polities.get_mut(from).alive = false;
        assert!(reference_to_ended(&fleets, &battles, &polities).is_some());

        assert_eq!(
            dissolve_fleets(&mut fleets, &mut battles, &data, from, into),
            (2, 1)
        );
        for index in [1, 2] {
            let fleet = &fleets.list[index];
            assert!(!fleet.alive && fleet.ships.is_empty());
            assert!(!fleet.in_battle && fleet.battle.is_none() && fleet.route.is_empty());
            assert_eq!(fleet.strength, 0.0);
        }
        assert!(fleets.orders.is_empty());
        assert!(fleets.list[0].alive, "die andere Seite bleibt");
        // Auf dem Feld steht von der aufgelösten Partei niemand mehr; als Verlust zählt das
        // nicht, und ihre Geschosse sind fort.
        let battle = &battles.active[0];
        assert!(
            battle
                .ships
                .iter()
                .filter(|ship| ship.side == 1)
                .all(|ship| ship.status == ShipStatus::Escaped)
        );
        assert_eq!(battle.combatants[1].lost, [0; 6]);
        assert_eq!(
            battle.combatants[1].escaped.iter().sum::<u32>() as usize,
            fielded
        );
        assert_eq!(battle.combatants[1].owner, into);
        assert!(!battle.is_fighting(1) && battle.is_fighting(0));
        assert_eq!(fleets.find_anomaly(&data), None);
        assert_eq!(battles.find_anomaly(&fleets), None);
        assert_eq!(reference_to_ended(&fleets, &battles, &polities), None);

        // Mit dem Ende der Gemeinschaft enden ihre Kriege: Das Gefecht hat keine Gegner
        // mehr und schließt beim nächsten Blick des Rechners.
        diplomacy.wars.clear();
        sync_hostility(&mut battles, &diplomacy);
        let mut battle = battles.active[0].clone();
        for _ in 0..40 {
            battle.step(&data);
        }
        assert!(battle.finished && !battle.timed_out);
    }
}

//! Werften: Der Staat bestellt Schiffe, die Werft baut sie aus Bauteilen vom Markt ihres
//! Systems.
//!
//! Eine Welt hat eine Werft, wenn in ihrem Zweig Rüstung Kapital steht; die Hauptwelt hat
//! immer einen Bauplatz. Jeden Tag kauft der Staat für jeden Auftrag auf einem Bauplatz
//! den Tagesanteil der Bauteile zum lokalen Preis und zahlt der Rüstungsfirma der Welt
//! einen Aufschlag. Fehlt ein Bauteil am Markt oder Geld in der Kasse, rückt der Bau nur
//! so weit vor, wie der knappste Posten reicht: Eine blockierte oder schlecht versorgte
//! Werft steht still, ohne dass es dafür eine eigene Regel bräuchte.

use super::{Fleets, Ship};
use crate::chronicle::{Chronicle, ChronicleKind};
use crate::data::{GameData, Sector, ShipClass};
use crate::economy::{Economy, Money, pay, purchase, recipes_of, to_credits, to_money};
use crate::galaxy::Galaxy;
use crate::ids::{PlanetId, PolityId};
use crate::polity::Polities;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Bauauftrag einer Werft.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BuildOrder {
    pub id: u32,
    pub polity: PolityId,
    /// Welt, deren Werft baut.
    pub planet: PlanetId,
    /// Index in [`GameData::designs`].
    pub design: u16,
    /// Baufortschritt 0..1.
    pub progress: f32,
    /// Bisher gezahlt: Bauteile und Werftaufschlag.
    pub spent: Money,
    /// Am letzten Bautag fehlten Bauteile oder Geld. Aufträge, die nur auf einen freien
    /// Bauplatz warten, gelten nicht als gestockt.
    pub stalled: bool,
    /// Tag der Auftragserteilung.
    pub started: u64,
}

/// Kapitalstock im Zweig Rüstung einer Welt.
pub fn arms_capital(data: &GameData, economy: &Economy, planet: PlanetId) -> f64 {
    let local = economy.planet(planet);
    recipes_of(data, Sector::Arms)
        .map(|(index, _)| local.capital[index])
        .sum()
}

/// Bauplätze der Werft einer Welt. Null heißt: keine Werft. Rüstungskapital gibt je
/// `yard_capital_per_slot` einen Platz, mindestens einen; die Hauptwelt hat auch ohne
/// Rüstungskapital einen.
pub fn yard_slots(
    data: &GameData,
    galaxy: &Galaxy,
    economy: &Economy,
    polities: &Polities,
    planet: PlanetId,
) -> u32 {
    let Some(world) = galaxy.planets.get(planet.index()) else {
        return 0;
    };
    let Some(owner) = world.owner else {
        return 0;
    };
    let capital = arms_capital(data, economy, planet);
    let is_capital = polities
        .list
        .get(owner.index())
        .is_some_and(|polity| polity.capital == Some(planet));
    if capital <= 0.0 && !is_capital {
        return 0;
    }
    ((capital / data.balance.military.yard_capital_per_slot) as u32).max(1)
}

/// Darf eine Gemeinschaft diesen Entwurf bauen? `known[tech]` sagt je Technologie, ob sie
/// erforscht ist; was die Liste nicht nennt, gilt als unbekannt. Bordjäger werden nie
/// einzeln gebaut.
pub fn design_buildable(data: &GameData, design: u16, known: &[bool]) -> Result<(), String> {
    let Some(def) = data.designs.get(design as usize) else {
        return Err("Entwurf unbekannt".into());
    };
    if def.stats.class == ShipClass::Fighter {
        return Err(format!(
            "{} ist ein Bordjäger und entsteht nur zusammen mit seinem Träger",
            def.name
        ));
    }
    for tech in &def.stats.techs {
        if !known.get(*tech as usize).copied().unwrap_or(false) {
            return Err(format!(
                "{} setzt die Technologie „{}“ voraus",
                def.name, data.techs[*tech as usize].name
            ));
        }
    }
    Ok(())
}

/// Gibt bei der Werft einer eigenen Welt ein Schiff in Auftrag. Gibt die Auftragsnummer
/// zurück.
pub fn order_ship(
    data: &GameData,
    galaxy: &Galaxy,
    economy: &Economy,
    polities: &Polities,
    fleets: &mut Fleets,
    polity: PolityId,
    planet: PlanetId,
    design: u16,
    known: &[bool],
    day: u64,
) -> Result<u32, String> {
    let Some(world) = galaxy.planets.get(planet.index()) else {
        return Err("Planet unbekannt".into());
    };
    if world.owner != Some(polity) {
        return Err(format!(
            "{} gehört nicht zur eigenen Gemeinschaft",
            world.name
        ));
    }
    if yard_slots(data, galaxy, economy, polities, planet) == 0 {
        return Err(format!(
            "{} hat keine Werft: Im Zweig Rüstung steht dort kein Kapital",
            world.name
        ));
    }
    design_buildable(data, design, known)?;
    let limit = data.balance.military.yard_queue as usize;
    if fleets.orders_at(planet).count() >= limit {
        return Err(format!(
            "die Werft von {} nimmt nicht mehr als {limit} Aufträge an",
            world.name
        ));
    }
    let id = fleets.next_order;
    fleets.next_order += 1;
    fleets.orders.push(BuildOrder {
        id,
        polity,
        planet,
        design,
        progress: 0.0,
        spent: 0,
        stalled: false,
        started: day,
    });
    Ok(id)
}

/// Streicht einen eigenen Bauauftrag. Was schon verbaut ist, wird nicht erstattet.
pub fn cancel_order(fleets: &mut Fleets, polity: PolityId, order: u32) -> Result<(), String> {
    let Some(index) = fleets.orders.iter().position(|entry| entry.id == order) else {
        return Err("Bauauftrag unbekannt".into());
    };
    if fleets.orders[index].polity != polity {
        return Err("Bauauftrag gehört einer anderen Gemeinschaft".into());
    }
    fleets.orders.remove(index);
    Ok(())
}

/// Tagesschritt aller Werften. Je Welt rücken die Aufträge in Nummernfolge vor, höchstens
/// so viele zugleich, wie die Werft Bauplätze hat. Fertige Schiffe stoßen zu einer eigenen
/// Flotte, die im System liegt, sonst entsteht dort eine neue.
pub fn advance_yards(
    galaxy: &Galaxy,
    economy: &mut Economy,
    fleets: &mut Fleets,
    polities: &Polities,
    chronicle: &mut Chronicle,
    data: &GameData,
    tick: u64,
) {
    if fleets.orders.is_empty() {
        return;
    }
    let balance = &data.balance.military;
    let spread = data.balance.economy.spread;
    let mut busy: BTreeMap<PlanetId, u32> = BTreeMap::new();
    let mut finished: Vec<u32> = Vec::new();
    let mut lapsed: Vec<u32> = Vec::new();
    for index in 0..fleets.orders.len() {
        let order = &mut fleets.orders[index];
        let world = galaxy.planet(order.planet);
        if world.owner != Some(order.polity) {
            lapsed.push(order.id);
            continue;
        }
        let slots = yard_slots(data, galaxy, economy, polities, order.planet);
        let used = busy.entry(order.planet).or_default();
        if *used >= slots {
            // Wartet auf einen freien Bauplatz.
            order.stalled = false;
            continue;
        }
        *used += 1;

        let stats = &data.designs[order.design as usize].stats;
        // Tagesanteil des Baus; am letzten Tag nur noch der Rest.
        let step = (1.0 / stats.build_days.max(1) as f32)
            .min(1.0 - order.progress)
            .max(0.0);
        let market = &mut economy.markets[world.system.index()];
        let treasury = &mut economy.treasuries[order.polity.index()];

        // Der knappste Posten bestimmt, wie weit der Tag reicht: zuerst die Ware …
        let mut share = 1.0f64;
        let mut cost = 0.0;
        for (good, amount) in &stats.parts {
            let wanted = *amount as f64 * step as f64;
            if wanted > 0.0 {
                share = share.min(market.available(*good) / wanted);
                cost += wanted * market.ask(*good, spread);
            }
        }
        // … dann die Kasse, aus der auch der Aufschlag der Werft bezahlt wird.
        let total = cost * (1.0 + balance.yard_fee);
        let affordable = if total > 0.0 {
            (to_credits((*treasury).max(0)) / total).min(1.0)
        } else {
            1.0
        };
        // Der bezahlbare Bedarf zählt als Nachfrage, auch wenn die Ware fehlt: So steigen
        // die Preise, Frachter bringen Bauteile und die Rüstungsbetriebe wachsen.
        for (good, amount) in &stats.parts {
            market.demand_today[good.index()] += *amount as f64 * step as f64 * affordable;
        }
        let mut share = share.min(affordable).clamp(0.0, 1.0);
        if to_money(cost) <= 0 {
            // Ein Rest unter einem Tausendstel Credit (Rundung am letzten Bautag) wird
            // nicht mehr gehandelt; der Bau schließt ab.
            share = 1.0;
            order.progress += step;
        } else if to_money(cost * share) <= 0 {
            share = 0.0;
        } else {
            let mut parts: Money = 0;
            for (good, amount) in &stats.parts {
                let quantity = *amount as f64 * step as f64 * share;
                parts += purchase(market, *good, quantity, treasury, spread).1;
            }
            let firm = &mut economy.planets[order.planet.index()].firms[Sector::Arms.index()];
            let fee = pay(
                treasury,
                &mut firm.cash,
                (parts as f64 * balance.yard_fee).round() as Money,
            );
            firm.revenue_today += to_credits(fee);
            firm.profit_today += to_credits(fee);
            order.spent += parts + fee;
            order.progress += step * share as f32;
        }
        order.stalled = share < 1.0;
        if order.progress >= 1.0 - 1e-6 {
            finished.push(order.id);
        }
    }

    for id in lapsed {
        let index = fleets
            .orders
            .iter()
            .position(|order| order.id == id)
            .expect("eben gelesen");
        let order = fleets.orders.remove(index);
        let world = galaxy.planet(order.planet);
        let design = &data.designs[order.design as usize];
        chronicle.push(
            tick,
            ChronicleKind::War,
            Some(order.polity),
            Some(world.system),
            format!(
                "Werft {}: Der Auftrag über {} {} verfällt, die Welt hat den Besitzer gewechselt.",
                world.name,
                design.stats.class.label(),
                design.name
            ),
        );
    }
    if finished.is_empty() {
        return;
    }
    for id in finished {
        let index = fleets
            .orders
            .iter()
            .position(|order| order.id == id)
            .expect("eben gelesen");
        let order = fleets.orders.remove(index);
        let world = galaxy.planet(order.planet);
        let ship = fleets.next_ship_id();
        // Zur Flotte mit der kleinsten Nummer, die im System liegt und nicht kämpft.
        let fleet = fleets
            .list
            .iter()
            .find(|fleet| {
                fleet.alive
                    && fleet.owner == order.polity
                    && fleet.system == world.system
                    && fleet.route.is_empty()
                    && !fleet.in_battle
            })
            .map(|fleet| fleet.id)
            .unwrap_or_else(|| fleets.new_fleet(polities, order.polity, world.system));
        let fleet = &mut fleets.list[fleet.index()];
        fleet.ships.push(Ship::from_yard(data, ship, order.design));
        let design = &data.designs[order.design as usize];
        chronicle.push(
            tick,
            ChronicleKind::War,
            Some(order.polity),
            Some(world.system),
            format!(
                "Werft {}: {} {} fertiggestellt und in die {} eingereiht.",
                world.name,
                design.stats.class.label(),
                design.name,
                fleet.name
            ),
        );
    }
    fleets.refresh(data);
}

#[cfg(test)]
mod tests {
    use super::super::testing::setup;
    use super::*;
    use crate::economy::setup_economy;
    use crate::ids::FleetId;

    #[test]
    fn slots_follow_arms_capital_and_the_capital_always_has_one() {
        let (data, galaxy, polities, _) = setup();
        let mut economy = setup_economy(&data, &galaxy, &polities);
        let home = polities.list[0].capital.unwrap();
        let per_slot = data.balance.military.yard_capital_per_slot;
        let arms = data.recipe_index("rumpfwerk").unwrap();
        let slots =
            |economy: &Economy, planet| yard_slots(&data, &galaxy, economy, &polities, planet);

        // Die Hauptwelt baut auch ohne Rüstungsbetriebe, auf genau einem Platz.
        assert_eq!(arms_capital(&data, &economy, home), 0.0);
        assert_eq!(slots(&economy, home), 1);
        economy.planets[home.index()].capital[arms] = per_slot * 3.5;
        assert_eq!(slots(&economy, home), 3);

        // Eine fremde, unbesiedelte Welt hat keine Werft, auch nicht mit Kapital.
        let free = galaxy
            .planets
            .iter()
            .find(|planet| planet.owner.is_none())
            .unwrap()
            .id;
        economy.planets[free.index()].capital[arms] = per_slot * 2.0;
        assert_eq!(slots(&economy, free), 0);
        assert_eq!(slots(&economy, PlanetId(99_999)), 0);

        // Nur Kapital im Zweig Rüstung zählt.
        let other = polities.list[1].capital.unwrap();
        let mine = data.recipe_index("erzabbau").unwrap();
        economy.planets[other.index()].capital[mine] = 100.0;
        assert_eq!(slots(&economy, other), 1);
    }

    #[test]
    fn only_designs_without_missing_technology_are_buildable() {
        let (data, _, _, _) = setup();
        let none: [bool; 0] = [];
        let cruiser = data.design_index("speer").unwrap();
        let advanced = data.design_index("speer_2").unwrap();
        let fighter = data.design_index("wespe").unwrap();
        assert!(design_buildable(&data, cruiser, &none).is_ok());
        assert!(
            design_buildable(&data, fighter, &none)
                .unwrap_err()
                .contains("Bordjäger")
        );
        assert!(design_buildable(&data, 9_999, &none).is_err());
        // Ohne die Technologie nicht, mit ihr schon.
        let error = design_buildable(&data, advanced, &none).unwrap_err();
        assert!(error.contains("Technologie"), "{error}");
        let needed = data.designs[advanced as usize].stats.techs.clone();
        assert!(!needed.is_empty());
        let mut known = vec![false; data.techs.len()];
        for tech in &needed[1..] {
            known[*tech as usize] = true;
        }
        assert!(design_buildable(&data, advanced, &known).is_err());
        known[needed[0] as usize] = true;
        assert!(design_buildable(&data, advanced, &known).is_ok());
    }

    #[test]
    fn orders_are_validated_and_the_queue_is_bounded() {
        let (data, galaxy, polities, mut fleets) = setup();
        let economy = setup_economy(&data, &galaxy, &polities);
        let me = PolityId(0);
        let home = polities.list[0].capital.unwrap();
        let theirs = polities.list[1].capital.unwrap();
        let corvette = data.design_index("dolch").unwrap();
        let order = |fleets: &mut Fleets, planet, design| {
            order_ship(
                &data,
                &galaxy,
                &economy,
                &polities,
                fleets,
                me,
                planet,
                design,
                &[],
                7,
            )
        };
        assert!(order(&mut fleets, PlanetId(99_999), corvette).is_err());
        assert!(order(&mut fleets, theirs, corvette).is_err());
        assert!(order(&mut fleets, home, data.design_index("wespe").unwrap()).is_err());
        assert!(order(&mut fleets, home, data.design_index("speer_2").unwrap()).is_err());
        assert!(fleets.orders.is_empty());

        let limit = data.balance.military.yard_queue;
        for expected in 0..limit {
            assert_eq!(order(&mut fleets, home, corvette), Ok(expected));
        }
        let full = order(&mut fleets, home, corvette).unwrap_err();
        assert!(full.contains("Aufträge"), "{full}");
        assert_eq!(fleets.orders.len(), limit as usize);
        assert_eq!(fleets.orders[0].started, 7);

        // Streichen: nur eigene, nur vorhandene; danach ist wieder Platz.
        assert!(cancel_order(&mut fleets, PolityId(1), 0).is_err());
        assert!(cancel_order(&mut fleets, me, 999).is_err());
        cancel_order(&mut fleets, me, 0).unwrap();
        assert!(cancel_order(&mut fleets, me, 0).is_err());
        assert_eq!(order(&mut fleets, home, corvette), Ok(limit));
        assert_eq!(fleets.find_anomaly(&data), None);
    }

    /// Füllt den Markt des Systems mit allem, was der Entwurf braucht.
    fn stock_parts(data: &GameData, economy: &mut Economy, galaxy: &Galaxy, planet: PlanetId) {
        let market = &mut economy.markets[galaxy.planet(planet).system.index()];
        for key in ["rumpfsegmente", "waffensysteme", "antriebe"] {
            market.stock[data.good_id(key).unwrap().index()] = 500.0;
        }
    }

    #[test]
    fn a_day_of_building_moves_goods_and_money_exactly() {
        let (data, galaxy, polities, mut fleets) = setup();
        let mut economy = setup_economy(&data, &galaxy, &polities);
        let mut chronicle = Chronicle::default();
        let me = PolityId(0);
        let home = polities.list[0].capital.unwrap();
        let system = galaxy.planet(home).system;
        stock_parts(&data, &mut economy, &galaxy, home);
        let cruiser = data.design_index("speer").unwrap();
        let stats = &data.designs[cruiser as usize].stats;
        order_ship(
            &data,
            &galaxy,
            &economy,
            &polities,
            &mut fleets,
            me,
            home,
            cruiser,
            &[],
            0,
        )
        .unwrap();

        let before = economy.clone();
        advance_yards(
            &galaxy,
            &mut economy,
            &mut fleets,
            &polities,
            &mut chronicle,
            &data,
            0,
        );
        let order = &fleets.orders[0];
        let step = 1.0 / stats.build_days as f64;
        assert!((order.progress as f64 - step).abs() < 1e-7);
        assert!(!order.stalled);

        // Die Bauteile des Tages haben den Markt verlassen …
        let spread = data.balance.economy.spread;
        let mut expected_cost = 0.0;
        for (good, amount) in &stats.parts {
            let taken = before.markets[system.index()].stock[good.index()]
                - economy.markets[system.index()].stock[good.index()];
            assert!((taken - *amount as f64 * step).abs() < 1e-6, "{good}");
            expected_cost +=
                *amount as f64 * step * before.markets[system.index()].ask(*good, spread);
        }
        // … der Staat hat sie zum lokalen Preis bezahlt, das Kontor hat das Geld …
        let paid = before.treasury(me) - economy.treasury(me);
        let to_market = economy.markets[system.index()].cash - before.markets[system.index()].cash;
        let firm = |economy: &Economy| economy.planet(home).firms[Sector::Arms.index()].cash;
        let to_yard = firm(&economy) - firm(&before);
        assert!((to_credits(to_market) - expected_cost).abs() < 0.01);
        // … und die Werft ihren Aufschlag, als Erlös der Rüstungsfirma.
        let fee = data.balance.military.yard_fee;
        assert!(to_yard > 0);
        assert!((to_yard as f64 - to_market as f64 * fee).abs() <= 1.0);
        assert!(economy.planet(home).firms[Sector::Arms.index()].revenue_today > 0.0);
        // Auf das Tausendstel: Was der Staat zahlt, kommt bei Kontor und Werft an.
        assert_eq!(paid, to_market + to_yard);
        assert_eq!(order.spent, paid);
        assert_eq!(economy.money_in_accounts(), before.money_in_accounts());
        // Niemand sonst hat sich bewegt.
        assert_eq!(economy.treasury(PolityId(1)), before.treasury(PolityId(1)));
        // Der Bedarf ist als Nachfrage gemeldet.
        let hull = data.balance.military.hull_good_id;
        assert!(economy.markets[system.index()].demand_today[hull.index()] > 0.0);
    }

    #[test]
    fn a_ship_takes_its_build_days_and_joins_the_fleet_in_the_system() {
        let (data, galaxy, polities, mut fleets) = setup();
        let mut economy = setup_economy(&data, &galaxy, &polities);
        let mut chronicle = Chronicle::default();
        let me = PolityId(0);
        let home = polities.list[0].capital.unwrap();
        stock_parts(&data, &mut economy, &galaxy, home);
        let corvette = data.design_index("dolch").unwrap();
        let days = data.designs[corvette as usize].stats.build_days;
        let ships = fleets.list[0].ships.len();
        // Zwei Aufträge, ein Bauplatz: Der zweite wartet.
        for _ in 0..2 {
            order_ship(
                &data,
                &galaxy,
                &economy,
                &polities,
                &mut fleets,
                me,
                home,
                corvette,
                &[],
                0,
            )
            .unwrap();
        }
        let day = |fleets: &mut Fleets, economy: &mut Economy, chronicle: &mut Chronicle| {
            advance_yards(&galaxy, economy, fleets, &polities, chronicle, &data, 0)
        };
        for _ in 0..days - 1 {
            day(&mut fleets, &mut economy, &mut chronicle);
        }
        assert_eq!(
            fleets.list[0].ships.len(),
            ships,
            "vor der Bauzeit kein Schiff"
        );
        assert_eq!(fleets.orders.len(), 2);
        assert_eq!(fleets.orders[1].progress, 0.0);
        assert!(!fleets.orders[1].stalled, "warten ist nicht stocken");
        day(&mut fleets, &mut economy, &mut chronicle);
        assert_eq!(fleets.list[0].ships.len(), ships + 1);
        assert_eq!(fleets.orders.len(), 1);
        let built = fleets.list[0].ships.last().unwrap();
        assert_eq!(built.design, corvette);
        assert_eq!((built.hull, built.ammo, built.fuel), (1.0, 0.0, 0.0));
        let gain =
            data.designs[corvette as usize].stats.value * data.balance.military.strength_per_value;
        let start = data.balance.fleets.start_strength as f64;
        assert!((fleets.list[0].strength as f64 - start - gain).abs() < 1e-3);
        assert_eq!(chronicle.entries.len(), 1);

        // Liegt keine eigene Flotte im System, entsteht eine neue.
        let away = galaxy.neighbors(fleets.list[0].system)[0].system;
        fleets.list[0].system = away;
        for _ in 0..days {
            day(&mut fleets, &mut economy, &mut chronicle);
        }
        assert!(fleets.orders.is_empty());
        let new = fleets.list.last().unwrap();
        assert_eq!(fleets.list.len(), polities.list.len() + 1);
        assert_eq!((new.owner, new.ships.len()), (me, 1));
        assert_eq!(new.system, galaxy.planet(home).system);
        assert!(new.alive && new.strength > 0.0);
        assert_eq!(fleets.find_anomaly(&data), None);
        assert!(economy.find_anomaly().is_none());
    }

    #[test]
    fn a_new_ship_joins_the_lowest_numbered_fleet_that_is_not_fighting() {
        let (data, galaxy, polities, mut fleets) = setup();
        let mut economy = setup_economy(&data, &galaxy, &polities);
        let mut chronicle = Chronicle::default();
        let me = PolityId(0);
        let home = polities.list[0].capital.unwrap();
        stock_parts(&data, &mut economy, &galaxy, home);
        let corvette = data.design_index("dolch").unwrap();
        let days = data.designs[corvette as usize].stats.build_days;
        // Zwei eigene Flotten liegen im Heimatsystem.
        let detached = vec![fleets.list[0].ships[0].id];
        let second =
            super::super::split_fleet(&data, &polities, &mut fleets, me, FleetId(0), &detached)
                .unwrap();
        let build = |fleets: &mut Fleets, economy: &mut Economy, chronicle: &mut Chronicle| {
            order_ship(
                &data,
                &galaxy,
                economy,
                &polities,
                fleets,
                me,
                home,
                corvette,
                &[],
                0,
            )
            .unwrap();
            for _ in 0..days {
                advance_yards(&galaxy, economy, fleets, &polities, chronicle, &data, 0);
            }
        };
        let sizes = |fleets: &Fleets| {
            (
                fleets.list[0].ships.len(),
                fleets.list[second.index()].ships.len(),
            )
        };
        let before = sizes(&fleets);
        build(&mut fleets, &mut economy, &mut chronicle);
        assert_eq!(
            sizes(&fleets),
            (before.0 + 1, before.1),
            "kleinste Nummer zuerst"
        );
        // Steht die erste im Gefecht oder hat sie Marschbefehl, geht das Schiff an die zweite.
        fleets.list[0].in_battle = true;
        build(&mut fleets, &mut economy, &mut chronicle);
        assert_eq!(sizes(&fleets), (before.0 + 1, before.1 + 1));
        fleets.list[0].in_battle = false;
        fleets.list[0].route = vec![galaxy.neighbors(fleets.list[0].system)[0].system];
        build(&mut fleets, &mut economy, &mut chronicle);
        assert_eq!(sizes(&fleets), (before.0 + 1, before.1 + 2));
        assert_eq!(
            fleets.list.len(),
            polities.list.len() + 1,
            "keine neue Flotte nötig"
        );
        assert_eq!(fleets.find_anomaly(&data), None);
    }

    #[test]
    fn the_scarcest_input_sets_the_pace_and_marks_the_order_stalled() {
        let (data, galaxy, polities, mut fleets) = setup();
        let mut economy = setup_economy(&data, &galaxy, &polities);
        let mut chronicle = Chronicle::default();
        let me = PolityId(0);
        let home = polities.list[0].capital.unwrap();
        let system = galaxy.planet(home).system;
        let corvette = data.design_index("dolch").unwrap();
        let stats = &data.designs[corvette as usize].stats;
        let step = 1.0 / stats.build_days as f32;
        order_ship(
            &data,
            &galaxy,
            &economy,
            &polities,
            &mut fleets,
            me,
            home,
            corvette,
            &[],
            0,
        )
        .unwrap();
        let day = |fleets: &mut Fleets, economy: &mut Economy, chronicle: &mut Chronicle| {
            advance_yards(&galaxy, economy, fleets, &polities, chronicle, &data, 0);
        };

        // Leerer Markt: kein Fortschritt, kein Geld, aber gemeldete Nachfrage.
        let before = economy.clone();
        day(&mut fleets, &mut economy, &mut chronicle);
        assert_eq!(
            (fleets.orders[0].progress, fleets.orders[0].spent),
            (0.0, 0)
        );
        assert!(fleets.orders[0].stalled);
        assert_eq!(economy.treasury(me), before.treasury(me));
        for (good, _) in &stats.parts {
            assert!(economy.markets[system.index()].demand_today[good.index()] > 0.0);
        }

        // Ein Bauteil reicht nur für ein Viertel des Tages: Alle Teile werden nur zu einem
        // Viertel gekauft, der Bau rückt um ein Viertel vor.
        stock_parts(&data, &mut economy, &galaxy, home);
        let (scarce, amount) = stats.parts[0];
        let need = amount as f64 * step as f64;
        economy.markets[system.index()].stock[scarce.index()] =
            0.25 * need / crate::economy::BUYER_SHARE;
        let before = economy.clone();
        day(&mut fleets, &mut economy, &mut chronicle);
        assert!((fleets.orders[0].progress - 0.25 * step).abs() < 1e-6);
        assert!(fleets.orders[0].stalled);
        for (good, amount) in &stats.parts {
            let taken = before.markets[system.index()].stock[good.index()]
                - economy.markets[system.index()].stock[good.index()];
            assert!((taken - 0.25 * *amount as f64 * step as f64).abs() < 1e-6);
        }

        // Eine Kasse, die nur für den halben Tag reicht, bremst genauso; dabei bleibt nichts
        // im Minus und der Aufschlag der Werft ist mitbezahlt.
        stock_parts(&data, &mut economy, &galaxy, home);
        let spread = data.balance.economy.spread;
        let cost: f64 = stats
            .parts
            .iter()
            .map(|(good, amount)| {
                *amount as f64 * step as f64 * economy.markets[system.index()].ask(*good, spread)
            })
            .sum();
        economy.treasuries[0] = to_money(0.5 * cost * (1.0 + data.balance.military.yard_fee));
        let progress = fleets.orders[0].progress;
        day(&mut fleets, &mut economy, &mut chronicle);
        assert!((fleets.orders[0].progress - progress - 0.5 * step).abs() < 1e-4);
        assert!(fleets.orders[0].stalled);
        assert!(economy.treasury(me) >= 0 && economy.treasury(me) <= 2);
        assert!(economy.planet(home).firms[Sector::Arms.index()].cash > 0);
        assert!(economy.find_anomaly().is_none());

        // Leere Kasse: nichts, und auch keine Nachfrage mehr, die die Preise triebe.
        economy.treasuries[0] = 0;
        let hull = data.balance.military.hull_good_id;
        let demand = economy.markets[system.index()].demand_today[hull.index()];
        let progress = fleets.orders[0].progress;
        day(&mut fleets, &mut economy, &mut chronicle);
        assert_eq!(fleets.orders[0].progress, progress);
        assert!(fleets.orders[0].stalled);
        assert_eq!(
            economy.markets[system.index()].demand_today[hull.index()],
            demand
        );
    }

    #[test]
    fn an_order_lapses_when_the_world_changes_hands() {
        let (data, mut galaxy, polities, mut fleets) = setup();
        let mut economy = setup_economy(&data, &galaxy, &polities);
        let mut chronicle = Chronicle::default();
        let home = polities.list[0].capital.unwrap();
        let corvette = data.design_index("dolch").unwrap();
        order_ship(
            &data,
            &galaxy,
            &economy,
            &polities,
            &mut fleets,
            PolityId(0),
            home,
            corvette,
            &[],
            0,
        )
        .unwrap();
        galaxy.planet_mut(home).owner = Some(PolityId(1));
        advance_yards(
            &galaxy,
            &mut economy,
            &mut fleets,
            &polities,
            &mut chronicle,
            &data,
            0,
        );
        assert!(fleets.orders.is_empty());
        assert!(chronicle.entries[0].text.contains("verfällt"));
    }
}

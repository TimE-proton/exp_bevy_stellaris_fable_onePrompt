//! Nachschub, Instandsetzung und Sold: was eine Flotte den Staat laufend kostet.
//!
//! Munition, Treibstoff und Rumpfsegmente sind Waren. Der Staat kauft sie am Markt des
//! Systems, in dem die Flotte liegt, und nur dort, wo ihre Gemeinschaft zu Hause ist. Fehlt
//! die Ware, bleibt das Magazin leer und der Rumpf beschädigt.

use super::{Fleets, Ship, yard_slots};
use crate::chronicle::{Chronicle, ChronicleKind};
use crate::data::GameData;
use crate::diplomacy::Diplomacy;
use crate::economy::{
    Economy, Market, Money, pay, pay_households, procure, purchase, to_credits, to_money,
};
use crate::galaxy::Galaxy;
use crate::ids::{GoodId, PolityId, SystemId};
use crate::polity::Polities;

/// Bis auf diesen Rest gilt ein Vorrat oder ein Rumpf als voll.
const FULL: f32 = 1.0 - 1e-6;
/// Unter diesem Wert gilt ein Soldrückstand als abgetragen.
const ARREARS_FLOOR: f32 = 0.01;

/// Anteil eines Wunsches, der erfüllt wurde. Wer nichts braucht, dem fehlt nichts.
fn filled(got: f64, wanted: f64) -> f32 {
    if wanted > 0.0 {
        (got / wanted).clamp(0.0, 1.0) as f32
    } else {
        1.0
    }
}

/// Hebt einen Anteil 0..1 um höchstens `rate`, davon den Teil `share`.
fn top_up(level: f32, rate: f32, share: f32) -> f32 {
    let raised = level + rate.min(1.0 - level).max(0.0) * share;
    if raised >= FULL { 1.0 } else { raised }
}

/// Kann eine Gemeinschaft am Markt eines Systems für ihre Flotten und Werften einkaufen?
/// Ja, wo sie eine Welt besitzt, es sei denn, das System untersteht (nach der Mehrheit
/// seiner Bevölkerung) einer Gemeinschaft, mit der sie im Krieg liegt: Deren Kontor
/// verkauft dem Gegner nichts. Nachschub, Instandsetzung und der Einkauf der Werften
/// fragen alle hier: Wo eine Werft bauen darf, wird ihr Schiff auch versorgt.
pub fn supply_base(
    galaxy: &Galaxy,
    diplomacy: &Diplomacy,
    owner: PolityId,
    system: SystemId,
) -> bool {
    galaxy.owns_planet_in(owner, system)
        && galaxy
            .system_owner(system)
            .is_none_or(|holder| holder == owner || !diplomacy.at_war(owner, holder))
}

/// Flotten, die in einem System liegen, in dem ihre Gemeinschaft einkaufen kann
/// ([`supply_base`]), fassen je Tag einen Teil ihrer Munition und ihres Treibstoffs nach.
/// Der Staat kauft beides am Markt des Systems; ist der Markt knapp oder die Kasse leer,
/// gibt es entsprechend weniger. In fremden und in blockierten Systemen gibt es nichts.
pub fn resupply_fleets(
    galaxy: &Galaxy,
    economy: &mut Economy,
    fleets: &mut Fleets,
    diplomacy: &Diplomacy,
    data: &GameData,
) {
    let balance = &data.balance.military;
    let rate = balance.resupply_per_day;
    let spread = data.balance.economy.spread;
    let fuel_good = data.balance.economy.fuel_id;
    for index in 0..fleets.list.len() {
        let fleet = &fleets.list[index];
        let (system, owner) = (fleet.system, fleet.owner);
        if !fleet.is_docked()
            || fleet.in_battle
            || fleets.is_blockaded(system)
            || !supply_base(galaxy, diplomacy, owner, system)
        {
            continue;
        }
        // Bedarf des Tages in Einheiten der beiden Güter.
        let (mut ammo, mut fuel) = (0.0f64, 0.0f64);
        for ship in &fleet.ships {
            let stats = &data.designs[ship.design as usize].stats;
            ammo += (rate.min(1.0 - ship.ammo).max(0.0) * stats.ammo) as f64 * balance.ammo_goods;
            fuel += (rate.min(1.0 - ship.fuel).max(0.0) * stats.fuel) as f64 * balance.fuel_goods;
        }
        if !fleet
            .ships
            .iter()
            .any(|ship| ship.ammo < 1.0 || ship.fuel < 1.0)
        {
            continue;
        }
        let market = &mut economy.markets[system.index()];
        let treasury = &mut economy.treasuries[owner.index()];
        let ammo_share = filled(
            procure(market, balance.ammo_good_id, ammo, treasury, spread).0,
            ammo,
        );
        let fuel_share = filled(procure(market, fuel_good, fuel, treasury, spread).0, fuel);
        for ship in &mut fleets.list[index].ships {
            ship.ammo = top_up(ship.ammo, rate, ammo_share);
            ship.fuel = top_up(ship.fuel, rate, fuel_share);
        }
    }
}

/// Anteil einer vollständigen Instandsetzung, den ein Schiff an einem Tag erhalten kann.
/// `hull_share` ist der Teil, der auf den Rumpf entfällt; der Rest gilt den Modulen.
fn repair_share(ship: &Ship, rate: f32, hull_share: f32) -> f32 {
    let gap = |level: f32| rate.min(1.0 - level).max(0.0);
    let modules = if ship.modules.is_empty() {
        0.0
    } else {
        ship.modules.iter().map(|module| gap(*module)).sum::<f32>() / ship.modules.len() as f32
    };
    hull_share * gap(ship.hull) + (1.0 - hull_share) * modules
}

/// Kauft vollständige Sätze von Bauteilen, höchstens `count`, und gibt zurück, wie viele
/// es wurden. Ein Satz, dem ein Teil oder das Geld fehlt, wird nicht angefangen.
fn buy_kits(
    market: &mut Market,
    parts: &[(GoodId, f32)],
    count: u32,
    treasury: &mut Money,
    spread: f64,
) -> u32 {
    for bought in 0..count {
        let cost: f64 = parts
            .iter()
            .map(|(good, amount)| *amount as f64 * market.ask(*good, spread))
            .sum();
        let affordable = to_money(cost) <= (*treasury).max(0);
        if affordable {
            // Der bezahlbare Wunsch zählt als Nachfrage, auch wenn ein Teil fehlt.
            for (good, amount) in parts {
                market.demand_today[good.index()] += *amount as f64;
            }
        }
        let in_stock = parts
            .iter()
            .all(|(good, amount)| market.available(*good) >= *amount as f64);
        if !affordable || !in_stock {
            return bought;
        }
        for (good, amount) in parts {
            purchase(market, *good, *amount as f64, treasury, spread);
        }
    }
    count
}

/// Flotten, die in einem System mit eigener Werft liegen, in dem ihre Gemeinschaft
/// einkaufen kann ([`supply_base`]), werden instand gesetzt: Rumpf und Module gewinnen je
/// Tag `repair_per_day`, verlorene Bordjäger werden ersetzt. Die Rumpfsegmente dafür (und
/// die Bauteile der Jäger) kauft der Staat am Markt des Systems; ohne Ware keine
/// Instandsetzung.
pub fn repair_fleets(
    galaxy: &Galaxy,
    economy: &mut Economy,
    fleets: &mut Fleets,
    polities: &Polities,
    diplomacy: &Diplomacy,
    data: &GameData,
) {
    let balance = &data.balance.military;
    let rate = balance.repair_per_day;
    let spread = data.balance.economy.spread;
    let mut repaired = false;
    for index in 0..fleets.list.len() {
        let fleet = &fleets.list[index];
        let (system, owner) = (fleet.system, fleet.owner);
        if !fleet.is_docked()
            || fleet.in_battle
            || !fleet.ships.iter().any(|ship| ship.is_damaged(data))
            || !supply_base(galaxy, diplomacy, owner, system)
        {
            continue;
        }
        let has_yard = galaxy.planets_in(system).any(|planet| {
            planet.owner == Some(owner)
                && yard_slots(data, galaxy, economy, polities, fleets, planet.id) > 0
        });
        if !has_yard {
            continue;
        }
        // Rumpfsegmente für den Tagesanteil: je vollständiger Instandsetzung ein fester
        // Anteil der Segmente, aus denen der Rumpf gebaut wurde.
        let mut wanted = 0.0f64;
        for ship in &fleet.ships {
            let segments = data.hulls[data.designs[ship.design as usize].hull_id as usize].segments;
            let share = repair_share(ship, rate, balance.repair_hull_share);
            wanted += (share * segments) as f64 * balance.repair_parts;
        }
        let market = &mut economy.markets[system.index()];
        let treasury = &mut economy.treasuries[owner.index()];
        let share = filled(
            procure(market, balance.hull_good_id, wanted, treasury, spread).0,
            wanted,
        );
        for ship in &mut fleets.list[index].ships {
            if share > 0.0 {
                ship.hull = top_up(ship.hull, rate, share);
                for module in &mut ship.modules {
                    *module = top_up(*module, rate, share);
                }
                if ship.modules.iter().all(|module| *module >= 1.0) {
                    ship.modules.clear();
                }
            }
            // Bordjäger kommen als ganze Maschinen aus ihren eigenen Bauteilen.
            let stats = &data.designs[ship.design as usize].stats;
            if let Some(craft) = stats.craft
                && ship.fighters < stats.fighters
            {
                let per_day = ((rate * stats.fighters as f32).ceil() as u32).max(1);
                ship.fighters += buy_kits(
                    market,
                    &data.designs[craft as usize].stats.parts,
                    per_day.min(stats.fighters - ship.fighters),
                    treasury,
                    spread,
                );
            }
        }
        repaired = true;
    }
    if repaired {
        fleets.refresh(data);
    }
}

/// Monatlicher Sold einer Gemeinschaft: ein Anteil am Bauwert aller ihrer Schiffe.
pub fn monthly_upkeep(data: &GameData, fleets: &Fleets, polity: PolityId) -> Money {
    let value: f64 = fleets.of(polity).map(|fleet| fleet.build_value(data)).sum();
    to_money(value * data.balance.military.upkeep_per_month)
}

/// Monatlich: Jede Gemeinschaft zahlt den Sold ihrer Besatzungen aus der Staatskasse an
/// die Haushalte ihrer Welten. Reicht die Kasse nicht, zahlt sie, was sie hat; der
/// ausgebliebene Anteil geht geglättet in den Soldrückstand ein.
pub fn pay_upkeep(
    galaxy: &Galaxy,
    economy: &mut Economy,
    fleets: &mut Fleets,
    polities: &Polities,
    chronicle: &mut Chronicle,
    data: &GameData,
    tick: u64,
) {
    let smoothing = data.balance.military.arrears_smoothing;
    if fleets.arrears.len() < polities.list.len() {
        fleets.arrears.resize(polities.list.len(), 0.0);
    }
    for polity in &polities.list {
        let index = polity.id.index();
        let due = monthly_upkeep(data, fleets, polity.id);
        // Die Heuer wird nach Bevölkerung verteilt; der Rundungsrest geht an die Hauptwelt,
        // damit genau der geschuldete Betrag fließt.
        let seat = polity
            .capital
            .filter(|planet| galaxy.planet(*planet).owner == Some(polity.id))
            .or_else(|| galaxy.planets_of(polity.id).next().map(|planet| planet.id));
        let mut paid: Money = 0;
        if let Some(seat) = seat
            && due > 0
        {
            let budget = due.min(economy.treasury(polity.id).max(0));
            paid = pay_households(galaxy, economy, polity.id, budget);
            paid += pay(
                &mut economy.treasuries[index],
                &mut economy.planets[seat.index()].households,
                budget - paid,
            );
        }
        let missed = if due > 0 {
            (1.0 - paid as f64 / due as f64) as f32
        } else {
            0.0
        };
        let before = fleets.arrears[index];
        let mut arrears = (before + smoothing * (missed - before)).clamp(0.0, 1.0);
        if missed <= 0.0 && arrears < ARREARS_FLOOR {
            arrears = 0.0;
        }
        fleets.arrears[index] = arrears;
        if missed > 0.0 && before <= 0.0 {
            chronicle.push(
                tick,
                ChronicleKind::Finance,
                Some(polity.id),
                None,
                format!(
                    "{} bleibt den Besatzungen der Flotte Sold schuldig: Von {:.0} Credits fehlen {:.0}.",
                    polity.name,
                    to_credits(due),
                    to_credits(due - paid)
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::setup;
    use super::*;
    use crate::data::Sector;
    use crate::economy::setup_economy;
    use crate::ids::FleetId;

    /// Niemand führt Krieg.
    fn peace() -> Diplomacy {
        Diplomacy::default()
    }

    fn drain(fleets: &mut Fleets, fleet: usize) {
        for ship in &mut fleets.list[fleet].ships {
            ship.ammo = 0.0;
            ship.fuel = 0.0;
        }
    }

    #[test]
    fn empty_magazines_are_restocked_at_home_from_the_market() {
        let (data, galaxy, polities, mut fleets) = setup();
        let mut economy = setup_economy(&data, &galaxy, &polities);
        let balance = &data.balance.military;
        let home = fleets.list[0].system;
        let ammo = balance.ammo_good_id;
        let fuel = data.balance.economy.fuel_id;
        economy.markets[home.index()].stock[ammo.index()] = 100.0;
        assert!(economy.markets[home.index()].stock[fuel.index()] > 50.0);
        drain(&mut fleets, 0);

        let before = economy.clone();
        resupply_fleets(&galaxy, &mut economy, &mut fleets, &peace(), &data);
        let rate = balance.resupply_per_day;
        let (mut ammo_units, mut fuel_units) = (0.0, 0.0);
        for ship in &fleets.list[0].ships {
            assert!((ship.ammo - rate).abs() < 1e-6 && (ship.fuel - rate).abs() < 1e-6);
            let stats = &data.designs[ship.design as usize].stats;
            ammo_units += (rate * stats.ammo) as f64 * balance.ammo_goods;
            fuel_units += (rate * stats.fuel) as f64 * balance.fuel_goods;
        }
        // Die Ware hat den Markt verlassen, der Staat hat sie dem Kontor bezahlt.
        let taken = |good: GoodId| {
            before.markets[home.index()].stock[good.index()]
                - economy.markets[home.index()].stock[good.index()]
        };
        assert!((taken(ammo) - ammo_units).abs() < 1e-6);
        assert!((taken(fuel) - fuel_units).abs() < 1e-6);
        let paid = before.treasury(PolityId(0)) - economy.treasury(PolityId(0));
        assert!(paid > 0);
        assert_eq!(
            paid,
            economy.markets[home.index()].cash - before.markets[home.index()].cash
        );
        assert_eq!(economy.money_in_accounts(), before.money_in_accounts());
        // Die anderen Flotten sind voll und kaufen nichts.
        assert_eq!(economy.treasury(PolityId(1)), before.treasury(PolityId(1)));

        // Nach genügend Tagen ist alles voll, und dann wird nichts mehr gekauft.
        for _ in 0..(1.0 / rate).ceil() as u32 {
            resupply_fleets(&galaxy, &mut economy, &mut fleets, &peace(), &data);
        }
        assert!(
            fleets.list[0]
                .ships
                .iter()
                .all(|ship| ship.ammo == 1.0 && ship.fuel == 1.0)
        );
        let full = economy.clone();
        resupply_fleets(&galaxy, &mut economy, &mut fleets, &peace(), &data);
        assert_eq!(economy.treasury(PolityId(0)), full.treasury(PolityId(0)));
    }

    #[test]
    fn a_short_market_or_an_empty_treasury_gives_less() {
        let (data, galaxy, polities, mut fleets) = setup();
        let mut economy = setup_economy(&data, &galaxy, &polities);
        let balance = &data.balance.military;
        let home = fleets.list[0].system;
        let ammo = balance.ammo_good_id;
        drain(&mut fleets, 0);
        // Keine Munition am Markt: Nur die Tanks füllen sich.
        assert_eq!(economy.markets[home.index()].stock[ammo.index()], 0.0);
        resupply_fleets(&galaxy, &mut economy, &mut fleets, &peace(), &data);
        assert!(fleets.list[0].ships.iter().all(|ship| ship.ammo == 0.0));
        assert!(fleets.list[0].ships.iter().all(|ship| ship.fuel > 0.0));
        // Der unerfüllte Wunsch steht als Nachfrage im Markt.
        assert!(economy.markets[home.index()].demand_today[ammo.index()] > 0.0);

        // Ein knapper Bestand wird geteilt: Jedes Schiff bekommt denselben Anteil.
        economy.markets[home.index()].stock[ammo.index()] = 0.2;
        resupply_fleets(&galaxy, &mut economy, &mut fleets, &peace(), &data);
        let levels: Vec<f32> = fleets.list[0].ships.iter().map(|ship| ship.ammo).collect();
        assert!(levels[0] > 0.0 && levels[0] < balance.resupply_per_day);
        assert!(levels.iter().all(|level| (level - levels[0]).abs() < 1e-6));

        // Leere Kasse: nichts, auch wenn der Markt voll ist.
        economy.markets[home.index()].stock[ammo.index()] = 100.0;
        let cash = std::mem::take(&mut economy.treasuries[0]);
        let before = fleets.clone();
        resupply_fleets(&galaxy, &mut economy, &mut fleets, &peace(), &data);
        for (ship, old) in fleets.list[0].ships.iter().zip(&before.list[0].ships) {
            assert_eq!((ship.ammo, ship.fuel), (old.ammo, old.fuel));
        }
        economy.treasuries[0] = cash;
        resupply_fleets(&galaxy, &mut economy, &mut fleets, &peace(), &data);
        assert!(fleets.list[0].ships[0].ammo > before.list[0].ships[0].ammo);
    }

    #[test]
    fn no_resupply_abroad_under_blockade_or_in_battle() {
        let (data, galaxy, polities, mut fleets) = setup();
        let mut economy = setup_economy(&data, &galaxy, &polities);
        let ammo = data.balance.military.ammo_good_id;
        let home = fleets.list[0].system;
        let abroad = fleets.list[1].system;
        for system in [home, abroad] {
            economy.markets[system.index()].stock[ammo.index()] = 100.0;
        }
        drain(&mut fleets, 0);
        let untouched = |fleets: &Fleets| {
            fleets.list[0]
                .ships
                .iter()
                .all(|ship| ship.ammo == 0.0 && ship.fuel == 0.0)
        };
        let before = economy.clone();

        // Im Heimatsystem einer anderen Gemeinschaft.
        fleets.list[0].system = abroad;
        resupply_fleets(&galaxy, &mut economy, &mut fleets, &peace(), &data);
        assert!(untouched(&fleets));

        // Zu Hause, aber das System ist gesperrt.
        fleets.list[0].system = home;
        fleets.blockaded = vec![home];
        resupply_fleets(&galaxy, &mut economy, &mut fleets, &peace(), &data);
        assert!(untouched(&fleets));

        // Zu Hause, aber im Gefecht oder auf dem Marsch.
        fleets.blockaded.clear();
        fleets.list[0].in_battle = true;
        resupply_fleets(&galaxy, &mut economy, &mut fleets, &peace(), &data);
        assert!(untouched(&fleets));
        fleets.list[0].in_battle = false;
        fleets.list[0].route = vec![galaxy.neighbors(home)[0].system];
        fleets.list[0].progress = 0.5;
        resupply_fleets(&galaxy, &mut economy, &mut fleets, &peace(), &data);
        assert!(untouched(&fleets));
        assert_eq!(economy.money_in_accounts(), before.money_in_accounts());
        assert_eq!(economy.treasury(PolityId(0)), before.treasury(PolityId(0)));

        // Gegenprobe: liegt sie frei im eigenen System, wird nachgefasst.
        fleets.list[0].route.clear();
        fleets.list[0].progress = 0.0;
        resupply_fleets(&galaxy, &mut economy, &mut fleets, &peace(), &data);
        assert!(!untouched(&fleets));
    }

    #[test]
    fn repair_needs_a_yard_and_hull_segments() {
        let (data, mut galaxy, polities, mut fleets) = setup();
        let mut economy = setup_economy(&data, &galaxy, &polities);
        let balance = &data.balance.military;
        let me = PolityId(0);
        let hull = balance.hull_good_id;
        let home = fleets.list[0].system;
        let slots = data.designs[fleets.list[0].ships[0].design as usize]
            .modules
            .len();
        let damage = |fleets: &mut Fleets| {
            for ship in &mut fleets.list[0].ships {
                ship.hull = 0.5;
            }
            fleets.list[0].ships[0].modules = vec![0.25; slots];
            fleets.refresh(&data);
        };
        damage(&mut fleets);
        let weak = fleets.list[0].strength;
        let repair = |galaxy: &Galaxy, economy: &mut Economy, fleets: &mut Fleets| {
            repair_fleets(galaxy, economy, fleets, &polities, &peace(), &data)
        };

        // Ohne Rumpfsegmente am Markt geschieht nichts.
        assert_eq!(economy.markets[home.index()].stock[hull.index()], 0.0);
        repair(&galaxy, &mut economy, &mut fleets);
        assert!(fleets.list[0].ships.iter().all(|ship| ship.hull == 0.5));
        assert_eq!(fleets.list[0].ships[0].modules[0], 0.25);

        // Mit Ware: Rumpf und Module gewinnen den Tagesanteil, der Staat zahlt die Segmente.
        economy.markets[home.index()].stock[hull.index()] = 100.0;
        let before = economy.clone();
        let rate = balance.repair_per_day;
        let mut expected = 0.0;
        for ship in &fleets.list[0].ships {
            let segments = data.hulls[data.designs[ship.design as usize].hull_id as usize].segments;
            let share = if ship.modules.is_empty() {
                balance.repair_hull_share * rate
            } else {
                rate
            };
            expected += (share * segments) as f64 * balance.repair_parts;
        }
        repair(&galaxy, &mut economy, &mut fleets);
        for ship in &fleets.list[0].ships {
            assert!((ship.hull - 0.5 - rate).abs() < 1e-6);
        }
        assert!((fleets.list[0].ships[0].modules[0] - 0.25 - rate).abs() < 1e-6);
        let taken = before.markets[home.index()].stock[hull.index()]
            - economy.markets[home.index()].stock[hull.index()];
        assert!((taken - expected).abs() < 1e-6, "{taken} statt {expected}");
        let paid = before.treasury(me) - economy.treasury(me);
        assert!(paid > 0);
        assert_eq!(
            paid,
            economy.markets[home.index()].cash - before.markets[home.index()].cash
        );
        assert!(
            fleets.list[0].strength > weak,
            "die Stärke folgt dem Zustand"
        );
        assert_eq!(fleets.find_anomaly(&data), None);

        // Lange genug im Dock ist alles heil; die Modulliste ist dann wieder leer.
        for _ in 0..(1.0 / rate).ceil() as u32 {
            repair(&galaxy, &mut economy, &mut fleets);
        }
        assert!(
            fleets.list[0]
                .ships
                .iter()
                .all(|ship| !ship.is_damaged(&data))
        );
        assert!(fleets.list[0].ships[0].modules.is_empty());
        assert!((fleets.list[0].strength - data.balance.fleets.start_strength).abs() < 1e-3);

        // Ein eigenes System ohne Werft setzt nichts instand, auch nicht mit Ware am Markt.
        let colony = galaxy
            .planets
            .iter()
            .find(|planet| galaxy.system_owner(planet.system).is_none())
            .unwrap()
            .id;
        let outpost = galaxy.planet(colony).system;
        galaxy.planet_mut(colony).owner = Some(me);
        galaxy.planet_mut(colony).population = 50.0;
        assert_eq!(galaxy.system_owner(outpost), Some(me));
        economy.markets[outpost.index()].stock[hull.index()] = 100.0;
        damage(&mut fleets);
        fleets.list[0].system = outpost;
        repair(&galaxy, &mut economy, &mut fleets);
        assert!(fleets.list[0].ships.iter().all(|ship| ship.hull == 0.5));
        // Mit Rüstungskapital auf der Kolonie gibt es dort eine Werft.
        let arms = data
            .recipes
            .iter()
            .position(|recipe| recipe.sector == Sector::Arms)
            .unwrap();
        economy.planets[colony.index()].capital[arms] = balance.yard_open_capital;
        super::super::update_yards(&data, &galaxy, &economy, &mut fleets);
        repair(&galaxy, &mut economy, &mut fleets);
        assert!(fleets.list[0].ships.iter().all(|ship| ship.hull > 0.5));

        // Im Gefecht und in der Fremde wird nicht instand gesetzt.
        let hulls = |fleets: &Fleets| -> Vec<f32> {
            fleets.list[0].ships.iter().map(|ship| ship.hull).collect()
        };
        let frozen = hulls(&fleets);
        fleets.list[0].in_battle = true;
        repair(&galaxy, &mut economy, &mut fleets);
        fleets.list[0].in_battle = false;
        fleets.list[0].system = fleets.list[1].system;
        economy.markets[fleets.list[1].system.index()].stock[hull.index()] = 100.0;
        repair(&galaxy, &mut economy, &mut fleets);
        assert_eq!(hulls(&fleets), frozen);
    }

    #[test]
    fn lost_craft_are_replaced_from_their_own_parts() {
        let (data, galaxy, polities, mut fleets) = setup();
        let mut economy = setup_economy(&data, &galaxy, &polities);
        let home = fleets.list[0].system;
        let carrier = data.design_index("horst").unwrap();
        let id = fleets.add_ship(&data, FleetId(0), carrier).unwrap();
        let stats = &data.designs[carrier as usize].stats;
        let craft = &data.designs[stats.craft.unwrap() as usize].stats;
        let index = fleets.list[0].ships.len() - 1;
        assert_eq!(fleets.list[0].ships[index].id, id);
        fleets.list[0].ships[index].fighters = stats.fighters - 3;
        fleets.refresh(&data);
        let fighters = |fleets: &Fleets| fleets.list[0].ships[index].fighters;

        // Ohne Bauteile kommt kein Jäger.
        repair_fleets(
            &galaxy,
            &mut economy,
            &mut fleets,
            &polities,
            &peace(),
            &data,
        );
        assert_eq!(fighters(&fleets), stats.fighters - 3);

        // Mit Bauteilen kommt je Tag eine Maschine, bezahlt zum Preis ihrer Teile.
        for (good, _) in &craft.parts {
            economy.markets[home.index()].stock[good.index()] = 10.0;
        }
        let before = economy.clone();
        repair_fleets(
            &galaxy,
            &mut economy,
            &mut fleets,
            &polities,
            &peace(),
            &data,
        );
        assert_eq!(fighters(&fleets), stats.fighters - 2);
        for (good, amount) in &craft.parts {
            let taken = before.markets[home.index()].stock[good.index()]
                - economy.markets[home.index()].stock[good.index()];
            assert!((taken - *amount as f64).abs() < 1e-9);
        }
        let paid = before.treasury(PolityId(0)) - economy.treasury(PolityId(0));
        let spread = data.balance.economy.spread;
        assert!((to_credits(paid) - craft.value * (1.0 + spread)).abs() < 0.01);
        assert_eq!(economy.money_in_accounts(), before.money_in_accounts());
        for _ in 0..5 {
            repair_fleets(
                &galaxy,
                &mut economy,
                &mut fleets,
                &polities,
                &peace(),
                &data,
            );
        }
        assert_eq!(
            fighters(&fleets),
            stats.fighters,
            "nie mehr als der Hangar fasst"
        );
        assert_eq!(fleets.find_anomaly(&data), None);
    }

    #[test]
    fn upkeep_moves_exactly_the_stated_amount_to_the_households() {
        let (data, galaxy, polities, mut fleets) = setup();
        let mut economy = setup_economy(&data, &galaxy, &polities);
        let mut chronicle = Chronicle::default();
        let me = PolityId(0);
        let value = fleets.list[0].build_value(&data);
        let due = monthly_upkeep(&data, &fleets, me);
        assert_eq!(
            due,
            to_money(value * data.balance.military.upkeep_per_month)
        );
        assert!(due > 0);
        let households = |economy: &Economy, polity: PolityId| -> Money {
            galaxy
                .planets_of(polity)
                .map(|planet| economy.planet(planet.id).households)
                .sum()
        };
        let before = economy.clone();
        pay_upkeep(
            &galaxy,
            &mut economy,
            &mut fleets,
            &polities,
            &mut chronicle,
            &data,
            0,
        );
        for polity in &polities.list {
            assert_eq!(
                before.treasury(polity.id) - economy.treasury(polity.id),
                due
            );
            assert_eq!(
                households(&economy, polity.id) - households(&before, polity.id),
                due
            );
            assert_eq!(fleets.arrears_of(polity.id), 0.0);
        }
        assert_eq!(economy.money_in_accounts(), before.money_in_accounts());
        assert!(chronicle.entries.is_empty());

        // Verteilt wird nach Bevölkerung. Mit mehreren Welten geht die Teilung nicht auf;
        // der Rundungsrest fließt an die Hauptwelt, die Summe bleibt genau der Sold.
        let mut galaxy = galaxy;
        let capital = polities.list[0].capital.unwrap();
        let colonies: Vec<_> = galaxy
            .planets
            .iter()
            .filter(|planet| planet.owner.is_none())
            .take(2)
            .map(|planet| planet.id)
            .collect();
        for (colony, people) in colonies.iter().zip([333.3, 77.7]) {
            galaxy.planet_mut(*colony).owner = Some(me);
            galaxy.planet_mut(*colony).population = people;
        }
        let population = galaxy.population_of(me);
        let before = economy.clone();
        pay_upkeep(
            &galaxy,
            &mut economy,
            &mut fleets,
            &polities,
            &mut chronicle,
            &data,
            0,
        );
        let gained = |planet: crate::ids::PlanetId| {
            economy.planet(planet).households - before.planet(planet).households
        };
        let total: Money = galaxy.planets_of(me).map(|planet| gained(planet.id)).sum();
        assert_eq!(total, due);
        assert_eq!(before.treasury(me) - economy.treasury(me), due);
        for colony in &colonies {
            let share = due as f64 * galaxy.planet(*colony).population / population;
            assert_eq!(gained(*colony), share as Money, "Anteil nach Bevölkerung");
        }
        let rounded: Money = galaxy
            .planets_of(me)
            .map(|planet| (due as f64 * planet.population / population) as Money)
            .sum();
        assert!(rounded < due, "die Teilung geht nicht auf");
        assert_eq!(
            gained(capital),
            (due as f64 * galaxy.planet(capital).population / population) as Money + due - rounded
        );
        assert_eq!(fleets.arrears_of(me), 0.0);

        // Mehr Schiffe, mehr Sold.
        let cruiser = data.design_index("speer").unwrap();
        fleets.add_ship(&data, FleetId(0), cruiser).unwrap();
        let more = monthly_upkeep(&data, &fleets, me);
        let extra =
            data.designs[cruiser as usize].stats.value * data.balance.military.upkeep_per_month;
        assert!((to_credits(more - due) - extra).abs() < 0.002);
    }

    #[test]
    fn an_empty_treasury_pays_what_it_has_and_records_arrears() {
        let (data, galaxy, polities, mut fleets) = setup();
        let mut economy = setup_economy(&data, &galaxy, &polities);
        let mut chronicle = Chronicle::default();
        let me = PolityId(0);
        let due = monthly_upkeep(&data, &fleets, me);
        let smoothing = data.balance.military.arrears_smoothing;
        let month = |economy: &mut Economy, fleets: &mut Fleets, chronicle: &mut Chronicle| {
            pay_upkeep(&galaxy, economy, fleets, &polities, chronicle, &data, 0)
        };

        // Die Kasse deckt nur ein Viertel: Das fließt, der Rest bleibt aus.
        let rich = economy.treasuries[0];
        economy.treasuries[0] = due / 4;
        let capital = polities.list[0].capital.unwrap();
        let before = economy.planet(capital).households;
        month(&mut economy, &mut fleets, &mut chronicle);
        assert_eq!(economy.treasury(me), 0);
        assert_eq!(economy.planet(capital).households - before, due / 4);
        let missed = 1.0 - (due / 4) as f32 / due as f32;
        assert!((fleets.arrears_of(me) - smoothing * missed).abs() < 1e-6);
        assert_eq!(fleets.arrears_of(PolityId(1)), 0.0);
        assert_eq!(chronicle.entries.len(), 1);
        assert!(chronicle.entries[0].text.contains("Sold"));
        assert_eq!(chronicle.entries[0].polity, Some(me));

        // Bleibt der Sold weiter aus, wächst der Rückstand, aber die Chronik meldet nur den Beginn.
        let first = fleets.arrears_of(me);
        month(&mut economy, &mut fleets, &mut chronicle);
        assert!(fleets.arrears_of(me) > first && fleets.arrears_of(me) <= 1.0);
        assert_eq!(chronicle.entries.len(), 1);

        // Wird wieder gezahlt, klingt er ab und erlischt.
        economy.treasuries[0] = rich;
        let mut last = fleets.arrears_of(me);
        for _ in 0..24 {
            month(&mut economy, &mut fleets, &mut chronicle);
            assert!(fleets.arrears_of(me) <= last);
            last = fleets.arrears_of(me);
        }
        assert_eq!(last, 0.0);
        assert_eq!(fleets.find_anomaly(&data), None);
        // Ein neuer Ausfall ist wieder ein Beginn.
        economy.treasuries[0] = 0;
        month(&mut economy, &mut fleets, &mut chronicle);
        assert_eq!(chronicle.entries.len(), 2);
        assert!((fleets.arrears_of(me) - smoothing).abs() < 1e-6);
    }
}

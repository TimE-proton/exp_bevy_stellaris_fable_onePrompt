//! Flotten in der laufenden Partie: Schiffe, Werften, Nachschub, Instandsetzung, Sold und
//! Gefechte. Die Rechenregeln selbst prüfen die Tests neben dem Code in `sim::fleet`; hier
//! geht es darum, dass sie in der Partie greifen und das Geld dabei erhalten bleibt.
//!
//! Wo ein Betrag auf das Tausendstel stimmen soll, läuft eine zweite, sonst gleiche Partie
//! als Gegenprobe mit: Bis zum Schritt der Flotten sind beide am selben Tag identisch, der
//! Unterschied danach ist genau das, was die Flotte bewirkt hat.
//!
//! Gefechte rechnet der taktische Rechner; ihr Ausgang hängt an Seed und Schiffsdaten. Die
//! Tests dazu prüfen deshalb zuerst, was für jeden Ausgang gelten muss, und sagen dann
//! ausdrücklich, was sie von ihrem Szenario erwarten.

use sim::GameData;
use sim::battle::{Battle, BattleShip, CombatModifiers, Doctrine, Formation, ShipStatus};
use sim::chronicle::Chronicle;
use sim::clock::{DAYS_PER_MONTH, DAYS_PER_YEAR, TICKS_PER_DAY};
use sim::command::{CommandOutcome, SimCommand};
use sim::data::{Sector, ShipClass, TechBranch};
use sim::diplomacy::Diplomacy;
use sim::economy::{BudgetMonth, Claim, Economy, Money, to_credits, to_money};
use sim::finance::{Finance, Secession, apply_secession, polity_of_issuer};
use sim::fleet::{
    BattleOutcome, BattleReport, Battles, Fleet, Fleets, Ship, SideReport, Stance,
    combat_modifiers, fleet_strength, maintenance_parts, material_value, monthly_upkeep,
    yard_slots,
};
use sim::galaxy::Galaxy;
use sim::harness::{Headless, check_invariants, summarize};
use sim::ids::{BattleId, FleetId, GoodId, IssuerId, PlanetId, PolityId, ShipId, SystemId};
use sim::logistics::Logistics;
use sim::needs_fine_ticks;
use sim::polity::{Polities, claimable_planets};
use sim::state::{SaveGame, SimConfig, money_drift};
use sim::tech::Research;
use std::sync::OnceLock;

/// Die drei Güter, aus denen jedes Schiff besteht.
const PARTS: [&str; 3] = ["rumpfsegmente", "waffensysteme", "antriebe"];

/// Die Spieldaten des Workspace, einmal geladen.
fn data() -> &'static GameData {
    static DATA: OnceLock<GameData> = OnceLock::new();
    DATA.get_or_init(|| GameData::load_workspace().unwrap())
}

fn observer(seed: u64) -> Headless {
    Headless::new(data().clone(), SimConfig { seed, player: None })
}

/// Partie, in der Gemeinschaft 0 vom Menschen geführt wird: Die KI greift dort nicht ein.
fn with_player(seed: u64) -> Headless {
    Headless::new(
        data().clone(),
        SimConfig {
            seed,
            player: Some(PolityId(0)),
        },
    )
}

fn good(key: &str) -> GoodId {
    data().good_id(key).unwrap()
}

fn design(key: &str) -> u16 {
    data().design_index(key).unwrap()
}

/// Hauptwelt und Heimatsystem einer Gemeinschaft.
fn home_of(sim: &Headless, polity: PolityId) -> (PlanetId, SystemId) {
    let capital = sim.polities().get(polity).capital.unwrap();
    (capital, sim.galaxy().planet(capital).system)
}

fn fleets(sim: &Headless) -> &Fleets {
    sim.world().resource::<Fleets>()
}

fn fleet(sim: &Headless, id: FleetId) -> &Fleet {
    &fleets(sim).list[id.index()]
}

fn economy(sim: &Headless) -> &Economy {
    sim.world().resource::<Economy>()
}

fn treasury(sim: &Headless, polity: PolityId) -> Money {
    economy(sim).treasury(polity)
}

fn stock_of(sim: &Headless, system: SystemId, key: &str) -> f64 {
    economy(sim).market(system).stock_of(good(key))
}

/// Legt Ware in das Lager eines Kontors (Ware ist kein Geld; die Geldsumme bleibt).
fn set_stock(sim: &mut Headless, system: SystemId, keys: &[&str], amount: f64) {
    let mut economy = sim.world_mut().resource_mut::<Economy>();
    for key in keys {
        economy.markets[system.index()].stock[good(key).index()] = amount;
    }
}

/// Geld der Haushalte aller Welten einer Gemeinschaft.
fn households(sim: &Headless, polity: PolityId) -> Money {
    sim.galaxy()
        .planets_of(polity)
        .map(|planet| economy(sim).planet(planet.id).households)
        .sum()
}

/// Geld des Staatsdienstes einer Gemeinschaft: dorthin gehen Sold und Gehälter.
fn staff(sim: &Headless, polity: PolityId) -> Money {
    economy(sim).staff_money(polity)
}

/// Der laufende Haushaltsmonat einer Gemeinschaft.
fn budget_month(sim: &Headless, polity: PolityId) -> &BudgetMonth {
    &economy(sim).budget(polity).unwrap().current
}

/// Was ein Anspruch im laufenden Haushaltsmonat bisher bekommen hat.
fn paid_for(sim: &Headless, polity: PolityId, claim: Claim) -> Money {
    budget_month(sim, polity).line(claim).paid
}

/// Ersatzteile, die die Wartung der Flotte einer Gemeinschaft an einem Tag verbraucht.
fn daily_maintenance(sim: &Headless, polity: PolityId, key: &str) -> f64 {
    maintenance_parts(data(), fleets(sim), polity)
        .get(&good(key))
        .copied()
        .unwrap_or(0.0)
        / DAYS_PER_MONTH as f64
}

/// Wert der Ersatzteile eines Monats zum Briefkurs am Markt des Systems.
fn maintenance_cost(sim: &Headless, polity: PolityId, system: SystemId) -> Money {
    let market = economy(sim).market(system);
    let spread = data().balance.economy.spread;
    to_money(
        maintenance_parts(data(), fleets(sim), polity)
            .iter()
            .map(|(good, amount)| amount * market.ask(*good, spread))
            .sum(),
    )
}

/// Leert Magazine und Tanks einer Flotte.
fn drain(sim: &mut Headless, id: FleetId, ammo: bool, fuel: bool) {
    let mut fleets = sim.world_mut().resource_mut::<Fleets>();
    for ship in &mut fleets.list[id.index()].ships {
        if ammo {
            ship.ammo = 0.0;
        }
        if fuel {
            ship.fuel = 0.0;
        }
    }
}

/// Gibt einen Befehl, lässt ihn im nächsten Takt anwenden und liefert das Ergebnis. Die
/// Uhr rückt dabei um einen Takt vor; Tests, die Tage genau abzählen, stellen Befehle
/// stattdessen mit `issue` ein und lassen ganze Tage laufen.
fn command(sim: &mut Headless, polity: PolityId, command: SimCommand) -> CommandOutcome {
    sim.issue(polity, command);
    sim.tick();
    sim.command_log().entries.last().unwrap().outcome.clone()
}

/// Grund, aus dem ein Befehl abgelehnt wurde.
fn rejection(sim: &mut Headless, polity: PolityId, issued: SimCommand) -> String {
    match command(sim, polity, issued.clone()) {
        CommandOutcome::Rejected(reason) => reason,
        CommandOutcome::Applied => panic!("{issued:?} hätte abgelehnt werden müssen"),
    }
}

/// Lässt die Partie laufen, bis die Flotte im Zielsystem liegt, und gibt die Tage zurück.
fn sail(sim: &mut Headless, id: FleetId, to: SystemId) -> u64 {
    for day in 0..2 * DAYS_PER_YEAR {
        let fleet = fleet(sim, id);
        if fleet.system == to && fleet.route.is_empty() {
            return day;
        }
        sim.run_days(1);
    }
    panic!("die Flotte kam nie an");
}

/// Das nächstgelegene Heimatsystem einer anderen Gemeinschaft und die Länge des Wegs dorthin.
fn nearest_foreign_home(sim: &Headless, me: PolityId) -> (SystemId, f32) {
    let home = home_of(sim, me).1;
    sim.polities()
        .list
        .iter()
        .filter(|polity| polity.id != me)
        .map(|polity| {
            let system = home_of(sim, polity.id).1;
            let path = sim.galaxy().shortest_path(home, system, |_| true).unwrap();
            (system, sim.galaxy().path_length(&path))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
        .unwrap()
}

// --- 1. Startflotten ----------------------------------------------------------------

#[test]
fn starting_fleets_match_the_data_and_strength_follows_the_ships() {
    let data = data();
    let mut sim = observer(11);
    let balance = &data.balance.military;
    let wanted: Vec<(u16, usize)> = balance
        .starting_fleet
        .iter()
        .map(|(key, count)| (design(key), *count as usize))
        .collect();
    let ships: usize = wanted.iter().map(|(_, count)| count).sum();
    let value: f64 = wanted
        .iter()
        .map(|(design, count)| data.designs[*design as usize].stats.value * *count as f64)
        .sum();
    let start = data.balance.fleets.start_strength;
    let check = |sim: &Headless| {
        assert_eq!(fleets(sim).list.len(), sim.polities().list.len());
        for polity in &sim.polities().list {
            let own: Vec<&Fleet> = fleets(sim).of(polity.id).collect();
            assert_eq!(own.len(), 1, "{}", polity.name);
            let fleet = own[0];
            assert_eq!(fleet.system, home_of(sim, polity.id).1);
            assert!(fleet.is_docked() && !fleet.in_battle);
            assert_eq!(fleet.ships.len(), ships);
            for (design, count) in &wanted {
                let found = fleet
                    .ships
                    .iter()
                    .filter(|ship| ship.design == *design)
                    .count();
                assert_eq!(found, *count, "{}", data.designs[*design as usize].name);
            }
            for ship in &fleet.ships {
                assert_eq!((ship.hull, ship.ammo, ship.fuel), (1.0, 1.0, 1.0));
                assert!(ship.modules.is_empty());
                assert_eq!(
                    ship.fighters,
                    data.designs[ship.design as usize].stats.fighters
                );
            }
            assert!((fleet.build_value(data) - value).abs() < 1e-6);
            assert!((fleet.strength - start).abs() < 1e-3, "{}", fleet.strength);
        }
        assert_eq!(
            fleets(sim).next_ship as usize,
            ships * sim.polities().list.len()
        );
    };
    check(&sim);
    // Eine Flotte, die im Heimathafen liegt, verbraucht nichts und verliert nichts.
    sim.run_days(90);
    check(&sim);
    let summary = summarize(&sim);
    assert!(
        summary
            .polities
            .iter()
            .all(|polity| polity.fleets == 1 && polity.ships == ships)
    );

    // Die Stärke folgt dem Zustand der Schiffe: Bauwert mal Zustand.
    let per_value = balance.strength_per_value;
    let lost = {
        let mut fleets = sim.world_mut().resource_mut::<Fleets>();
        fleets.list[0].ships[0].hull = 0.4;
        let lost = 0.6
            * data.designs[fleets.list[0].ships[0].design as usize]
                .stats
                .value;
        fleets.refresh(data);
        lost
    };
    let damaged = fleet(&sim, FleetId(0));
    assert!((damaged.strength as f64 - (start as f64 - lost * per_value)).abs() < 1e-3);
    assert!((damaged.strength - fleet_strength(data, damaged)).abs() < 1e-6);
    // Auch nach einem Tag in der Partie passt die Stärke zu den Schiffen.
    sim.run_days(1);
    let later = fleet(&sim, FleetId(0));
    assert!((later.strength - fleet_strength(data, later)).abs() < 1e-6);
    assert!(later.strength < start - 1.0);

    // Eine Flotte ohne Schiffe besteht nicht mehr; die Invarianten halten.
    sim.world_mut().resource_mut::<Fleets>().list[1]
        .ships
        .clear();
    sim.run_days(1);
    assert!(!fleet(&sim, FleetId(1)).alive);
    assert_eq!(check_invariants(sim.world()), Ok(()));
}

/// Kopiert die Spieldaten in ein eigenes Verzeichnis, ändert `balance.ron` und lädt sie.
fn load_with(name: &str, change: impl Fn(&str) -> String) -> Result<GameData, String> {
    let source = sim::data::workspace_data_dir();
    let dir = std::env::temp_dir().join(format!("konfluenz-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for entry in std::fs::read_dir(&source).unwrap() {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        let text = if path.file_name().unwrap() == "balance.ron" {
            let changed = change(&text);
            assert_ne!(changed, text, "{name}: die Änderung hat nichts getroffen");
            changed
        } else {
            text
        };
        std::fs::write(dir.join(path.file_name().unwrap()), text).unwrap();
    }
    let loaded = GameData::load(&dir).map_err(|error| error.to_string());
    std::fs::remove_dir_all(&dir).unwrap();
    loaded
}

#[test]
fn the_military_balance_is_checked_when_the_data_is_loaded() {
    // Eine unschädliche Änderung lädt, und die Eichung der Stärke folgt der Startflotte.
    let doubled = load_with("doppelt", |text| {
        text.replace("(\"speer\", 1)", "(\"speer\", 2)")
    })
    .unwrap();
    let value: f64 = doubled
        .balance
        .military
        .starting_fleet_ids
        .iter()
        .map(|(design, count)| doubled.designs[*design as usize].stats.value * *count as f64)
        .sum();
    assert!(value > data().designs[design("speer") as usize].stats.value * 2.0);
    let start = doubled.balance.fleets.start_strength as f64;
    assert!((value * doubled.balance.military.strength_per_value - start).abs() < 1e-9);

    // Unsinnige Werte werden mit Datei und Grund gemeldet statt irgendwo in der Partie.
    let rejected = |name: &str, from: &str, to: &str| {
        let error = load_with(name, |text| text.replace(from, to)).unwrap_err();
        assert!(error.contains("balance.ron"), "{name}: {error}");
        error
    };
    let fleet = "starting_fleet: [(\"speer\", 1), (\"waechter\", 2), (\"dolch\", 3)]";
    assert!(rejected("leer", fleet, "starting_fleet: []").contains("starting_fleet"));
    assert!(rejected("jaeger", fleet, "starting_fleet: [(\"wespe\", 4)]").contains("Bordjäger"));
    assert!(rejected("null", fleet, "starting_fleet: [(\"speer\", 0)]").contains("Anzahl"));
    assert!(rejected("fremd", fleet, "starting_fleet: [(\"galeere\", 1)]").contains("galeere"));
    assert!(rejected("fahrt", "dry_speed: 0.4", "dry_speed: 0.0").contains("dry_speed"));
    let military = &data().balance.military;
    let pay = format!("upkeep_per_month: {}", military.upkeep_per_month);
    assert!(rejected("sold", &pay, "upkeep_per_month: 1.5").contains("upkeep_per_month"));
    let parts = format!("maintenance_per_month: {}", military.maintenance_per_month);
    assert!(
        rejected("wartung", &parts, "maintenance_per_month: -0.1")
            .contains("maintenance_per_month")
    );
    // Auch die Anteile des Staatshaushalts sind Anteile.
    let civil = format!("civil_share: {}", data().balance.economy.civil_share);
    assert!(rejected("grund", &civil, "civil_share: 1.5").contains("civil_share"));
    let handout = format!("handout_share: {}", data().balance.economy.handout_share);
    assert!(rejected("aussch", &handout, "handout_share: -0.5").contains("handout_share"));
    assert!(
        rejected(
            "werft",
            "yard_capital_per_slot: 4.0",
            "yard_capital_per_slot: 0.0"
        )
        .contains("yard_capital_per_slot")
    );
    assert!(rejected("schlange", "yard_queue: 8", "yard_queue: 0").contains("yard_queue"));
    assert!(
        rejected(
            "nachschub",
            "resupply_per_day: 0.05",
            "resupply_per_day: -0.1"
        )
        .contains("resupply_per_day")
    );
    assert!(
        rejected("module", "module_weight: 0.5", "module_weight: 1.5").contains("module_weight")
    );
    // Die Stellschrauben der Gefechte: Ohne Sammelfrist begänne jedes Gefecht am nächsten
    // Morgen von vorn.
    assert!(rejected("sammeln", "regroup_days: 30", "regroup_days: 0").contains("regroup_days"));
    // Was ein Gefecht über die Technik des Gegners verrät, ist ein Anteil.
    let insight = format!(
        "intel_per_battle: {}",
        data().balance.research.intel_per_battle
    );
    assert!(rejected("einblick", &insight, "intel_per_battle: 1.5").contains("intel_per_battle"));
    assert!(rejected("schock", "defeat_shock: 0.9", "defeat_shock: 1.5").contains("defeat_shock"));
    assert!(
        rejected(
            "muede",
            "weariness_per_navy_lost: 20.0",
            "weariness_per_navy_lost: -1.0"
        )
        .contains("weariness_per_navy_lost")
    );
    assert!(
        rejected(
            "truemmer",
            "debris_fade_per_month: 0.02",
            "debris_fade_per_month: 0.0"
        )
        .contains("debris_fade_per_month")
    );
}

// --- 2. Werften -----------------------------------------------------------------------

#[test]
fn a_shipyard_turns_market_goods_and_treasury_money_into_a_ship() {
    let data = data();
    let me = PolityId(0);
    let mut sim = with_player(12);
    let mut control = with_player(12);
    let (capital, home) = home_of(&sim, me);
    for sim in [&mut sim, &mut control] {
        set_stock(sim, home, &PARTS, 100.0);
    }
    let corvette = design("dolch");
    let stats = &data.designs[corvette as usize].stats;
    let ships = fleet(&sim, FleetId(0)).ships.len();
    sim.issue(
        me,
        SimCommand::BuildShip {
            planet: capital,
            design: corvette,
        },
    );
    sim.run_days(1);
    control.run_days(1);
    assert_eq!(
        sim.command_log().entries[0].outcome,
        CommandOutcome::Applied
    );

    // Erster Bautag: ein Sechzigstel der Bauteile hat den Markt verlassen …
    let order = fleets(&sim).orders[0].clone();
    let step = 1.0 / stats.build_days as f64;
    assert_eq!((order.id, order.polity, order.planet), (0, me, capital));
    assert!((order.progress as f64 - step).abs() < 1e-6);
    assert!(!order.stalled());
    for (good, amount) in &stats.parts {
        let taken = economy(&control).market(home).stock_of(*good)
            - economy(&sim).market(home).stock_of(*good);
        assert!(
            (taken - *amount as f64 * step).abs() < 1e-6,
            "{good}: {taken}"
        );
    }
    // … der Staat hat sie bezahlt: an das Kontor die Ware, an die Rüstungsfirma der Welt
    // den Aufschlag der Werft. Kein Tausendstel entsteht oder verschwindet. Im Haushalt
    // steht der Kauf beim Anspruch der Beschaffung.
    let to_market = economy(&sim).market(home).cash - economy(&control).market(home).cash;
    let firm = |sim: &Headless| economy(sim).planet(capital).firms[Sector::Arms.index()].cash;
    let to_yard = firm(&sim) - firm(&control);
    assert!(to_market > 0 && to_yard > 0);
    let paid = to_market + to_yard;
    assert_eq!(order.spent, paid);
    assert_eq!(
        paid_for(&sim, me, Claim::Procurement) - paid_for(&control, me, Claim::Procurement),
        paid
    );
    // Der Tag war ein Monatserster: Der Haushalt hat den Bau des Monats zurückgelegt,
    // bevor er ausschüttete. Die Haushalte bekamen deshalb weniger als in der Gegenprobe,
    // und nur um diese Kürzung liegt die Kasse über dem, was der Kauf sie gekostet hat.
    let withheld = budget_month(&control, me).handout() - budget_month(&sim, me).handout();
    assert!(withheld > 0);
    assert_eq!(households(&control, me) - households(&sim, me), withheld);
    assert_eq!(treasury(&control, me) - treasury(&sim, me), paid - withheld);
    assert!((to_yard as f64 - to_market as f64 * data.balance.military.yard_fee).abs() <= 1.0);
    let spread = data.balance.economy.spread;
    let price: f64 = stats
        .parts
        .iter()
        .map(|(good, amount)| *amount as f64 * step * economy(&sim).market(home).ask(*good, spread))
        .sum();
    assert!(
        (to_credits(to_market) - price).abs() < 0.01,
        "lokaler Preis"
    );
    assert_eq!(money_drift(sim.world()), Some(0));
    assert_eq!(money_drift(control.world()), Some(0));

    // Das Schiff kommt nach der Bauzeit, keinen Tag früher.
    sim.run_days(stats.build_days as u64 - 2);
    assert_eq!(fleets(&sim).ship_count(me), ships);
    assert_eq!(fleets(&sim).orders.len(), 1);
    assert!(fleets(&sim).orders[0].progress < 1.0);
    sim.run_days(1);
    assert!(fleets(&sim).orders.is_empty());
    assert_eq!(fleets(&sim).ship_count(me), ships + 1);
    // Es stößt zur Flotte im System: unversehrt, aber noch ohne Munition und Treibstoff.
    let home_fleet = fleet(&sim, FleetId(0));
    let built = home_fleet.ships.last().unwrap();
    assert_eq!(built.design, corvette);
    assert_eq!((built.hull, built.ammo, built.fuel), (1.0, 0.0, 0.0));
    let gain = stats.value * data.balance.military.strength_per_value;
    let start = data.balance.fleets.start_strength as f64;
    assert!((home_fleet.strength as f64 - start - gain).abs() < 1e-3);
    let chronicle = sim.world().resource::<Chronicle>();
    assert!(
        chronicle
            .entries
            .iter()
            .any(|entry| entry.text.contains("fertiggestellt") && entry.polity == Some(me))
    );
    // Am Tag darauf fasst es wie jedes andere Schiff nach.
    sim.run_days(1);
    assert!(fleet(&sim, FleetId(0)).ships.last().unwrap().fuel > 0.0);
    assert_eq!(money_drift(sim.world()), Some(0));
    assert_eq!(check_invariants(sim.world()), Ok(()));
}

#[test]
fn more_slots_build_more_ships_at_once() {
    let data = data();
    let me = PolityId(0);
    let corvette = design("dolch");
    let days = data.designs[corvette as usize].stats.build_days as u64;
    let hull_works = data.recipe_index("rumpfwerk").unwrap();
    // Zwei gleiche Partien; in der zweiten hat die Hauptwelt Rüstungskapital für zwei Plätze.
    let run = |slots: u32| {
        let mut sim = with_player(12);
        let (capital, home) = home_of(&sim, me);
        set_stock(&mut sim, home, &PARTS, 300.0);
        if slots > 1 {
            let per_slot = data.balance.military.yard_capital_per_slot;
            sim.world_mut().resource_mut::<Economy>().planets[capital.index()].capital
                [hull_works] = per_slot * (slots as f64 + 0.7);
        }
        for _ in 0..3 {
            sim.issue(
                me,
                SimCommand::BuildShip {
                    planet: capital,
                    design: corvette,
                },
            );
        }
        let ships = fleets(&sim).ship_count(me);
        sim.run_days(1);
        let world = sim.world();
        assert_eq!(
            yard_slots(
                data,
                world.resource::<Galaxy>(),
                world.resource::<Economy>(),
                world.resource::<Polities>(),
                world.resource::<Fleets>(),
                capital
            ),
            slots
        );
        // Am ersten Tag wird auf genau so vielen Plätzen gebaut; der Rest wartet, ohne zu stocken.
        let building = fleets(&sim)
            .orders
            .iter()
            .filter(|order| order.progress > 0.0)
            .count();
        assert_eq!(building, slots as usize);
        assert!(fleets(&sim).orders.iter().all(|order| !order.stalled()));
        sim.run_days(days - 1);
        let after_one_build = fleets(&sim).ship_count(me) - ships;
        sim.run_days(days);
        let after_two_builds = fleets(&sim).ship_count(me) - ships;
        assert_eq!(check_invariants(sim.world()), Ok(()));
        (after_one_build, after_two_builds)
    };
    assert_eq!(run(1), (1, 2), "ein Bauplatz: ein Schiff je Bauzeit");
    assert_eq!(run(2), (2, 3), "zwei Bauplätze: zwei Schiffe zugleich");
}

// --- 3. Stillstand ----------------------------------------------------------------------

#[test]
fn a_yard_stands_still_without_goods_or_money_and_resumes_when_they_return() {
    let data = data();
    let me = PolityId(0);
    let mut sim = with_player(13);
    let (capital, home) = home_of(&sim, me);
    let corvette = design("dolch");
    let stats = &data.designs[corvette as usize].stats;
    let step = 1.0 / stats.build_days as f32;
    for key in PARTS {
        assert_eq!(
            stock_of(&sim, home, key),
            0.0,
            "zu Beginn liegt nichts am Markt"
        );
    }
    sim.issue(
        me,
        SimCommand::BuildShip {
            planet: capital,
            design: corvette,
        },
    );
    let order = |sim: &Headless| fleets(sim).orders[0].clone();

    // Ohne Bauteile steht die Werft; gezahlt wird nichts, aber der Bedarf ist gemeldet.
    sim.run_days(3);
    assert_eq!((order(&sim).progress, order(&sim).spent), (0.0, 0));
    assert!(order(&sim).stalled());
    let hull = data.balance.military.hull_good_id;
    assert!(economy(&sim).market(home).demand[hull.index()] > 0.0);

    // Zwei von drei Bauteilen genügen nicht, und die Werft kauft sie auch nicht auf Vorrat:
    // Vom Markt geht nur, was die Wartung der Flotte an diesem Tag verbraucht.
    set_stock(&mut sim, home, &["rumpfsegmente", "antriebe"], 50.0);
    sim.run_days(1);
    assert_eq!(order(&sim).progress, 0.0);
    assert!(order(&sim).stalled());
    for key in ["rumpfsegmente", "antriebe"] {
        let worn = daily_maintenance(&sim, me, key);
        assert!(worn > 0.0 && worn < 1.0);
        assert!(
            (50.0 - stock_of(&sim, home, key) - worn).abs() < 1e-9,
            "{key}"
        );
    }

    // Mit dem dritten läuft der Bau an.
    set_stock(&mut sim, home, &["waffensysteme"], 50.0);
    sim.run_days(1);
    assert!((order(&sim).progress - step).abs() < 1e-6);
    assert!(!order(&sim).stalled());
    assert!(order(&sim).spent > 0);

    // Reicht ein Bauteil nur für einen Teil des Tages, rückt der Bau nur so weit vor, wie
    // dieses knappste Teil reicht, und gilt als gestockt.
    let weapons = good("waffensysteme");
    let need = stats
        .parts
        .iter()
        .find(|(good, _)| *good == weapons)
        .unwrap()
        .1 as f64
        * step as f64;
    set_stock(&mut sim, home, &["waffensysteme"], need);
    let before = order(&sim).progress;
    sim.run_days(1);
    let gained = order(&sim).progress - before;
    assert!(
        gained > 0.2 * step && gained < 0.8 * step,
        "Fortschritt {gained} bei halbem Angebot, voller Tag wäre {step}"
    );
    assert!(order(&sim).stalled());
    set_stock(&mut sim, home, &["waffensysteme"], 50.0);

    // Leere Staatskasse: Der Staat nimmt keine Steuern mehr ein und hat nichts mehr.
    assert_eq!(
        command(&mut sim, me, SimCommand::SetTaxRate { rate: 0.0 }),
        CommandOutcome::Applied
    );
    sim.run_days(1);
    assert!(!order(&sim).stalled(), "mit Ware und Geld läuft der Bau");
    let purse = {
        let mut economy = sim.world_mut().resource_mut::<Economy>();
        let purse = std::mem::take(&mut economy.treasuries[me.index()]);
        // Das Geld liegt solange bei den Haushalten der Hauptwelt: Die Summe bleibt.
        economy.planets[capital.index()].households += purse;
        purse
    };
    assert!(purse > 0);
    let (before, spent) = (order(&sim).progress, order(&sim).spent);
    assert!(
        sim.clock().day() + 5 < DAYS_PER_MONTH,
        "kein Monatsanfang in den nächsten Tagen: sonst flössen Zölle in die Kasse"
    );
    sim.run_days(4);
    assert_eq!(treasury(&sim, me), 0);
    assert_eq!((order(&sim).progress, order(&sim).spent), (before, spent));
    assert!(order(&sim).stalled());

    // Kommt das Geld zurück, geht es weiter.
    {
        let mut economy = sim.world_mut().resource_mut::<Economy>();
        economy.planets[capital.index()].households -= purse;
        economy.treasuries[me.index()] += purse;
    }
    sim.run_days(1);
    assert!((order(&sim).progress - before - step).abs() < 1e-6);
    assert!(!order(&sim).stalled());
    assert_eq!(money_drift(sim.world()), Some(0));
    assert_eq!(check_invariants(sim.world()), Ok(()));
}

/// Das bevölkerungsreichste Koloniesystem einer Gemeinschaft außerhalb ihres Heimatsystems.
fn largest_colony_system(sim: &Headless, polity: PolityId) -> SystemId {
    let galaxy = sim.galaxy();
    let home = home_of(sim, polity).1;
    let people = |system: SystemId| -> f64 {
        galaxy
            .planets_in(system)
            .filter(|planet| planet.owner == Some(polity))
            .map(|planet| planet.population)
            .sum()
    };
    galaxy
        .systems
        .iter()
        .filter(|system| system.id != home && galaxy.system_owner(system.id) == Some(polity))
        .max_by(|a, b| people(a.id).total_cmp(&people(b.id)).then(b.id.cmp(&a.id)))
        .map(|system| system.id)
        .expect("nach einigen Jahren gibt es Kolonien")
}

#[test]
fn a_blockade_starves_a_yard_that_lives_on_imports() {
    let data = data();
    let attacker = PolityId(0);
    let victim = PolityId(1);
    let mut blockaded = observer(31);
    let mut open = observer(31);
    for sim in [&mut blockaded, &mut open] {
        sim.run_days(6 * DAYS_PER_YEAR);
    }
    // Eine Kolonie mit kleiner Werft; die Bauteile liegen reichlich am fernen Heimatmarkt.
    let target = largest_colony_system(&open, victim);
    let home = home_of(&open, victim).1;
    let yard = open
        .galaxy()
        .planets_in(target)
        .filter(|planet| planet.owner == Some(victim))
        .max_by(|a, b| a.population.total_cmp(&b.population).then(b.id.cmp(&a.id)))
        .unwrap()
        .id;
    let arms = data
        .recipes
        .iter()
        .position(|recipe| recipe.sector == Sector::Arms)
        .unwrap();
    // Am Ort entsteht kein Bauteil: Die Kolonie hat ein Munitionswerk, das sie zur Werft
    // macht, aber keine Rumpf-, Waffen- und Triebwerkswerke.
    let part_works: Vec<usize> = data
        .recipes
        .iter()
        .enumerate()
        .filter(|(_, recipe)| PARTS.iter().any(|key| good(key) == recipe.output_id))
        .map(|(index, _)| index)
        .collect();
    assert_eq!(part_works.len(), PARTS.len());
    assert!(!part_works.contains(&arms));
    let worlds: Vec<PlanetId> = open
        .galaxy()
        .planets_in(target)
        .map(|planet| planet.id)
        .collect();
    for sim in [&mut blockaded, &mut open] {
        let mut economy = sim.world_mut().resource_mut::<Economy>();
        for world in &worlds {
            for recipe in &part_works {
                economy.planets[world.index()].capital[*recipe] = 0.0;
            }
        }
        economy.planets[yard.index()].capital[arms] += 1.0;
        for key in PARTS {
            economy.markets[home.index()].stock[good(key).index()] += 80.0;
        }
    }

    // Krieg, Blockadeauftrag, Marschbefehl: die Sperre aus den Befehlen des Spiels.
    blockaded.issue(attacker, SimCommand::DeclareWar { target: victim });
    blockaded.issue(
        attacker,
        SimCommand::SetStance {
            fleet: FleetId(0),
            stance: Stance::Blockade,
        },
    );
    blockaded.issue(
        attacker,
        SimCommand::MoveFleet {
            fleet: FleetId(0),
            to: target,
        },
    );
    let mut established = false;
    for _ in 0..2 * DAYS_PER_YEAR {
        blockaded.run_days(1);
        open.run_days(1);
        if fleets(&blockaded).is_blockaded(target) {
            established = true;
            break;
        }
    }
    assert!(established, "die Flotte hat die Blockade nie errichtet");

    // Derselbe Auftrag in beiden Partien am selben Tag. Am Ort liegt kein einziges Bauteil:
    // Die Werft lebt von dem, was Frachter bringen.
    let corvette = design("dolch");
    let ships = fleets(&open).ship_count(victim);
    for sim in [&mut blockaded, &mut open] {
        set_stock(sim, target, &PARTS, 0.0);
        assert_eq!(
            command(
                sim,
                victim,
                SimCommand::BuildShip {
                    planet: yard,
                    design: corvette
                }
            ),
            CommandOutcome::Applied
        );
    }
    // Bis die offene Werft liefert, zählen wir die Tage, an denen der Bau stockt.
    let mut stalled = [0u32; 2];
    let mut delivered = false;
    for _ in 0..DAYS_PER_YEAR {
        for (index, sim) in [&mut blockaded, &mut open].into_iter().enumerate() {
            sim.run_days(1);
            if fleets(sim)
                .orders
                .first()
                .is_some_and(|order| order.stalled())
            {
                stalled[index] += 1;
            }
        }
        assert!(fleets(&blockaded).is_blockaded(target));
        if fleets(&open).orders.is_empty() {
            delivered = true;
            break;
        }
    }
    assert!(delivered, "die offene Werft hat nie geliefert");
    assert_eq!(fleets(&open).ship_count(victim), ships + 1);

    // Die gesperrte Werft ist weit zurück: Ihr fehlen die Bauteile, die niemand bringt. Die
    // offene Werft wartete nur, bis die ersten Frachter kamen.
    let behind = fleets(&blockaded)
        .orders
        .first()
        .expect("unter Blockade ist das Schiff noch nicht fertig");
    assert!(
        behind.progress < 0.5 && behind.stalled(),
        "Baufortschritt unter Blockade {:.2}",
        behind.progress
    );
    assert!(
        stalled[0] >= stalled[1] + 30,
        "Tage mit stockendem Bau: gesperrt {}, offen {}",
        stalled[0],
        stalled[1]
    );
    // Ursache ist die Ware: Frachter brachten der offenen Werft Bauteile, der gesperrten nicht.
    let delivered_parts = |sim: &Headless| -> f64 {
        sim.world()
            .resource::<Logistics>()
            .flows
            .iter()
            .filter(|((_, to, good), _)| {
                *to == target && PARTS.iter().any(|key| self::good(key) == *good)
            })
            .map(|(_, volume)| *volume)
            .sum()
    };
    assert!(
        delivered_parts(&open) > delivered_parts(&blockaded) + 1.0,
        "Lieferungen offen {:.1}, gesperrt {:.1}",
        delivered_parts(&open),
        delivered_parts(&blockaded)
    );
    for sim in [&blockaded, &open] {
        assert_eq!(money_drift(sim.world()), Some(0));
        assert_eq!(check_invariants(sim.world()), Ok(()));
    }
}

// --- 4. Nachschub -----------------------------------------------------------------------

/// Kolonien mit Werft, und ob ihr Rüstungskapital dazu passt: Zurückgegeben werden die
/// Welten mit Werft außer den Hauptwelten.
fn colony_yards(sim: &Headless) -> Vec<PlanetId> {
    let data = data();
    let military = &data.balance.military;
    let world = sim.world();
    let (galaxy, economy, polities) = (
        world.resource::<Galaxy>(),
        world.resource::<Economy>(),
        world.resource::<Polities>(),
    );
    let mut yards = Vec::new();
    for planet in galaxy
        .planets
        .iter()
        .filter(|planet| planet.owner.is_some())
    {
        let owner = polities.get(planet.owner.unwrap());
        if owner.capital == Some(planet.id) {
            continue;
        }
        let slots = yard_slots(data, galaxy, economy, polities, fleets(sim), planet.id);
        let capital = sim::fleet::arms_capital(data, economy, planet.id);
        // Eine Werft steht nie auf weniger als dem unteren, keine Welt ohne Werft hat den
        // oberen Schwellenwert an Rüstungskapital.
        if slots > 0 {
            assert!(capital >= military.yard_close_capital, "{}", planet.name);
            yards.push(planet.id);
        } else {
            assert!(capital < military.yard_open_capital, "{}", planet.name);
        }
    }
    yards
}

#[test]
fn colony_yards_follow_real_arms_industry_and_do_not_flicker() {
    // Eine Partie im Frieden, Monat für Monat: Auf den meisten Kolonien liegt etwas
    // Rüstungskapital, das niemand braucht und das langsam verfällt. Solche Reste machen
    // keine Werft. Wo eine entsteht, bleibt sie, bis ihre Betriebe wirklich abgebaut sind:
    // Keine Welt wechselt in zwanzig Jahren öfter als zweimal (gewonnen, verloren).
    let mut sim = observer(3);
    let months = 20 * 12;
    let mut changes: std::collections::BTreeMap<PlanetId, u32> = std::collections::BTreeMap::new();
    let mut before = colony_yards(&sim);
    let (mut seen, mut leftovers) = (0usize, 0usize);
    for _ in 0..months {
        sim.run_days(DAYS_PER_MONTH);
        let now = colony_yards(&sim);
        for planet in before.iter().filter(|planet| !now.contains(planet)) {
            *changes.entry(*planet).or_default() += 1;
        }
        for planet in now.iter().filter(|planet| !before.contains(planet)) {
            *changes.entry(*planet).or_default() += 1;
        }
        seen = seen.max(now.len());
        // Welten mit einem Rest an Rüstungskapital, aber ohne Werft.
        let world = sim.world();
        leftovers = leftovers.max(
            sim.galaxy()
                .planets
                .iter()
                .filter(|planet| planet.owner.is_some() && !now.contains(&planet.id))
                .filter(|planet| {
                    sim::fleet::arms_capital(data(), world.resource::<Economy>(), planet.id) > 0.0
                })
                .count(),
        );
        before = now;
    }
    assert!(
        seen > 0,
        "Szenario neu wählen: In zwanzig Jahren sollte eine Kolonie eine Werft bekommen"
    );
    assert!(
        leftovers >= 10,
        "Reste von Rüstungskapital ohne Werft: höchstens {leftovers} zugleich"
    );
    let worst = changes.values().copied().max().unwrap_or(0);
    assert!(worst <= 2, "eine Welt wechselte {worst}-mal: {changes:?}");
    assert_eq!(check_invariants(sim.world()), Ok(()));
}

#[test]
fn a_fleet_is_supplied_wherever_its_community_owns_a_world() {
    let mut sim = observer(5);
    sim.run_days(18 * DAYS_PER_YEAR);
    // Ein System, das eine Gemeinschaft nach der Mehrheit seiner Bevölkerung prägt, in dem
    // aber auch eine andere eine Welt besitzt. Für diese andere ist es eigenes Gebiet.
    let found = sim.galaxy().systems.iter().find_map(|system| {
        let holder = sim.galaxy().system_owner(system.id)?;
        sim.galaxy()
            .planets_in(system.id)
            .filter_map(|planet| planet.owner)
            .find(|owner| *owner != holder && fleets(&sim).of(*owner).next().is_some())
            .map(|minority| (minority, holder, system.id))
    });
    let (me, holder, shared) =
        found.expect("Szenario neu wählen: Nach 18 Jahren sollte es ein geteiltes System geben");
    assert_ne!(sim.galaxy().system_owner(shared), Some(me));
    assert!(sim.galaxy().owns_planet_in(me, shared));
    assert!(!sim.world().resource::<Diplomacy>().at_war(me, holder));
    let id = fleets(&sim).of(me).next().unwrap().id;
    assert!(fleet(&sim, id).ships.iter().all(|ship| ship.fuel == 1.0));

    // Die Flotte marschiert dorthin; unterwegs verbraucht sie Treibstoff.
    march(&mut sim, id, shared);
    let arrived = fleet(&sim, id).min_fuel();
    assert!(
        arrived < 0.99,
        "Szenario neu wählen: Der Marsch sollte Treibstoff kosten ({arrived})"
    );
    // Am Markt des Systems, an dem auch ihre Welt dort handelt, fasst sie nach: Die Tanks
    // füllen sich wieder.
    let mut days = 0;
    while fleet(&sim, id).min_fuel() < 1.0 {
        sim.run_days(1);
        days += 1;
        assert!(
            days <= 60,
            "nach {days} Tagen steht der knappste Tank bei {}",
            fleet(&sim, id).min_fuel()
        );
    }
    assert!(days >= 1);
    assert!(fleet(&sim, id).system == shared && !fleet(&sim, id).in_battle);
    assert_eq!(money_drift(sim.world()), Some(0));
    assert_eq!(check_invariants(sim.world()), Ok(()));
}

#[test]
fn a_fleet_restocks_at_home_from_the_market_and_the_treasury_pays() {
    let data = data();
    let balance = &data.balance.military;
    let me = PolityId(0);
    let mut sim = with_player(14);
    let mut control = with_player(14);
    let home = home_of(&sim, me).1;
    for sim in [&mut sim, &mut control] {
        set_stock(sim, home, &["munition"], 50.0);
    }
    drain(&mut sim, FleetId(0), true, true);
    sim.run_days(1);
    control.run_days(1);

    // Ein Tagesanteil ist nachgefasst. Die Gegenprobe liegt mit vollen Tanks im Dock und
    // fasst nur nach, was sie an diesem Tag verbraucht hat: Der Unterschied ist, was die
    // leere Flotte darüber hinaus bekam.
    let rate = balance.resupply_per_day;
    let idle = balance.fuel_per_day;
    assert!(idle > 0.0 && idle < rate);
    let (mut ammo_units, mut fuel_units) = (0.0, 0.0);
    for ship in &fleet(&sim, FleetId(0)).ships {
        assert!((ship.ammo - rate).abs() < 1e-6 && (ship.fuel - rate).abs() < 1e-6);
        let stats = &data.designs[ship.design as usize].stats;
        ammo_units += (rate * stats.ammo) as f64 * balance.ammo_goods;
        fuel_units += ((rate - idle) * stats.fuel) as f64 * balance.fuel_goods;
    }
    for ship in &fleet(&control, FleetId(0)).ships {
        assert_eq!((ship.ammo, ship.fuel), (1.0, 1.0));
    }
    // … die Ware fehlt dem Markt, und die Staatskasse hat sie dem Kontor bezahlt.
    let ammo = balance.ammo_good_id;
    let fuel = data.balance.economy.fuel_id;
    let taken = |good: GoodId| {
        economy(&control).market(home).stock_of(good) - economy(&sim).market(home).stock_of(good)
    };
    assert!((taken(ammo) - ammo_units).abs() < 1e-9);
    assert!((taken(fuel) - fuel_units).abs() < 1e-6);
    let paid = paid_for(&sim, me, Claim::Procurement) - paid_for(&control, me, Claim::Procurement);
    assert!(paid > 0);
    assert_eq!(
        paid,
        economy(&sim).market(home).cash - economy(&control).market(home).cash
    );
    // Der Tag war ein Monatserster: Was das Auffüllen im Monat kosten wird, hat der
    // Haushalt zurückgelegt, bevor er ausschüttete.
    let withheld = budget_month(&control, me).handout() - budget_month(&sim, me).handout();
    assert!(withheld > 0);
    assert_eq!(treasury(&control, me) - treasury(&sim, me), paid - withheld);
    let spread = data.balance.economy.spread;
    let market = economy(&sim).market(home);
    let price = ammo_units * market.ask(ammo, spread) + fuel_units * market.ask(fuel, spread);
    assert!((to_credits(paid) - price).abs() < 0.01);
    assert_eq!(money_drift(sim.world()), Some(0));

    // Nach genügend Tagen sind Magazine und Tanks voll: Je Tag bleibt vom Nachfassen
    // übrig, was die Flotte nicht selbst verbraucht.
    sim.run_days((1.0 / (rate - idle)).ceil() as u64 + 1);
    assert!(
        fleet(&sim, FleetId(0))
            .ships
            .iter()
            .all(|ship| ship.ammo == 1.0 && ship.fuel == 1.0)
    );
    assert_eq!(check_invariants(sim.world()), Ok(()));
}

#[test]
fn travelling_burns_fuel_and_there_is_no_resupply_abroad() {
    let data = data();
    let balance = &data.balance.military;
    let me = PolityId(0);
    let mut sim = with_player(14);
    let home = home_of(&sim, me).1;
    let (abroad, length) = nearest_foreign_home(&sim, me);
    // Munition liegt in beiden Systemen bereit; gekauft wird sie nur zu Hause.
    set_stock(&mut sim, home, &["munition"], 50.0);
    set_stock(&mut sim, abroad, &["munition"], 50.0);
    assert_eq!(
        command(
            &mut sim,
            me,
            SimCommand::MoveFleet {
                fleet: FleetId(0),
                to: abroad
            }
        ),
        CommandOutcome::Applied
    );
    sail(&mut sim, FleetId(0), abroad);
    // Je Streckeneinheit derselbe Anteil jedes Tanks.
    let expected = 1.0 - length * balance.fuel_per_distance;
    assert!(
        expected > 0.1 && expected < 0.9,
        "die Reise ist lang genug, um etwas zu messen, und ein Tank reicht dafür: {expected}"
    );
    for ship in &fleet(&sim, FleetId(0)).ships {
        assert!(
            (ship.fuel - expected).abs() < 1e-3,
            "Tank {} statt {expected} nach {length} Strecke",
            ship.fuel
        );
    }

    // In der Fremde gibt es nichts: weder Munition noch Treibstoff. Die Tanks leeren sich
    // weiter, auch wenn die Flotte nur liegt: je Tag um einen festen Anteil.
    drain(&mut sim, FleetId(0), true, false);
    assert!(
        stock_of(&sim, abroad, "munition") > 1.0,
        "Munition läge bereit"
    );
    sim.run_days(20);
    let idle = expected - 20.0 * balance.fuel_per_day;
    assert!(balance.fuel_per_day > 0.0 && idle > 0.0);
    for ship in &fleet(&sim, FleetId(0)).ships {
        assert_eq!(ship.ammo, 0.0);
        assert!(
            (ship.fuel - idle).abs() < 1e-3,
            "Tank {} statt {idle} nach zwanzig Tagen auf Station",
            ship.fuel
        );
    }

    // Zurück im eigenen System wird nachgefasst.
    sim.issue(
        me,
        SimCommand::MoveFleet {
            fleet: FleetId(0),
            to: home,
        },
    );
    sail(&mut sim, FleetId(0), home);
    sim.run_days(3);
    assert!(
        fleet(&sim, FleetId(0))
            .ships
            .iter()
            .all(|ship| ship.ammo > 0.0)
    );
    assert_eq!(money_drift(sim.world()), Some(0));
    assert_eq!(check_invariants(sim.world()), Ok(()));
}

#[test]
fn a_fleet_without_fuel_takes_much_longer() {
    let data = data();
    let me = PolityId(0);
    let voyage = |dry: bool| {
        let mut sim = with_player(14);
        let home = home_of(&sim, me).1;
        let (target, length) = nearest_foreign_home(&sim, me);
        if dry {
            drain(&mut sim, FleetId(0), false, true);
        }
        sim.issue(
            me,
            SimCommand::MoveFleet {
                fleet: FleetId(0),
                to: target,
            },
        );
        let days = sail(&mut sim, FleetId(0), target);
        let path = sim.galaxy().shortest_path(home, target, |_| true).unwrap();
        (days, length, path.len() as u64 - 1)
    };
    let (fast, length, lanes) = voyage(false);
    let (slow, _, _) = voyage(true);
    let speed = data.balance.fleets.speed;
    let crawl = speed * data.balance.military.dry_speed;
    // Jede Route wird auf ganze Tage aufgerundet: je Route höchstens ein Tag mehr.
    let bounds = |days: u64, speed: f32| {
        let least = (length / speed).floor() as u64;
        days >= least && days <= least + lanes + 1
    };
    assert!(
        length * data.balance.military.fuel_per_distance < 1.0,
        "ein voller Tank reicht für die ganze Strecke"
    );
    assert!(
        bounds(fast, speed),
        "{fast} Tage für {length} bei voller Fahrt"
    );
    assert!(
        bounds(slow, crawl),
        "{slow} Tage für {length} ohne Treibstoff"
    );
    assert!(
        slow * 10 > fast * 18,
        "trocken {slow} Tage, versorgt {fast} Tage"
    );
}

// --- 5. Instandsetzung --------------------------------------------------------------------

#[test]
fn damaged_ships_are_repaired_only_at_a_yard_and_only_with_hull_segments() {
    let data = data();
    let balance = &data.balance.military;
    let me = PolityId(0);
    let mut sim = with_player(15);
    let home = home_of(&sim, me).1;
    let damage = |sim: &mut Headless| {
        let mut fleets = sim.world_mut().resource_mut::<Fleets>();
        for ship in &mut fleets.list[0].ships {
            ship.hull = 0.6;
        }
        fleets.refresh(data);
    };
    let hulls = |sim: &Headless| -> Vec<f32> {
        fleet(sim, FleetId(0))
            .ships
            .iter()
            .map(|ship| ship.hull)
            .collect()
    };

    // Im Heimathafen gibt es eine Werft, aber noch keine Rumpfsegmente: nichts geschieht.
    damage(&mut sim);
    assert_eq!(stock_of(&sim, home, "rumpfsegmente"), 0.0);
    sim.run_days(3);
    assert!(hulls(&sim).iter().all(|hull| *hull == 0.6));

    // Mit Rumpfsegmenten am Markt gewinnt jeder Rumpf den Tagesanteil; die Segmente dafür
    // verlassen den Markt.
    set_stock(&mut sim, home, &["rumpfsegmente"], 40.0);
    let rate = balance.repair_per_day;
    let mut wanted = 0.0;
    for ship in &fleet(&sim, FleetId(0)).ships {
        let segments = data.hulls[data.designs[ship.design as usize].hull_id as usize].segments;
        // Nur der Rumpf ist beschädigt: Es zählt sein Anteil an einer vollständigen
        // Instandsetzung; die Module sind heil.
        wanted += (balance.repair_hull_share * rate * segments) as f64 * balance.repair_parts;
    }
    let weak = fleet(&sim, FleetId(0)).strength;
    sim.run_days(1);
    assert!(
        hulls(&sim)
            .iter()
            .all(|hull| (*hull - 0.6 - rate).abs() < 1e-6)
    );
    // Vom Markt gehen die Segmente der Instandsetzung und der Tagesanteil der Wartung.
    let worn = daily_maintenance(&sim, me, "rumpfsegmente");
    assert!(worn > 0.0 && worn < wanted);
    assert!((40.0 - stock_of(&sim, home, "rumpfsegmente") - wanted - worn).abs() < 1e-6);
    assert!(fleet(&sim, FleetId(0)).strength > weak);

    // Eine eigene Kolonie in einem anderen System hat keine Werft: Dort bleibt der Schaden,
    // auch wenn Rumpfsegmente am Markt liegen.
    let colony = {
        let world = sim.world();
        claimable_planets(
            world.resource::<Galaxy>(),
            world.resource::<Polities>(),
            &data.balance.colonization,
            treasury(&sim, me),
            me,
            sim.clock().day(),
        )
        .into_iter()
        .find(|planet| sim.galaxy().planet(*planet).system != home)
        .expect("eine freie Welt im Nachbarsystem")
    };
    assert_eq!(
        command(&mut sim, me, SimCommand::ClaimPlanet { planet: colony }),
        CommandOutcome::Applied
    );
    let outpost = sim.galaxy().planet(colony).system;
    assert_eq!(sim.galaxy().system_owner(outpost), Some(me));
    sim.issue(
        me,
        SimCommand::MoveFleet {
            fleet: FleetId(0),
            to: outpost,
        },
    );
    sail(&mut sim, FleetId(0), outpost);
    damage(&mut sim);
    set_stock(&mut sim, outpost, &["rumpfsegmente"], 40.0);
    {
        let world = sim.world();
        assert_eq!(
            yard_slots(
                data,
                world.resource::<Galaxy>(),
                world.resource::<Economy>(),
                world.resource::<Polities>(),
                world.resource::<Fleets>(),
                colony
            ),
            0
        );
    }
    sim.run_days(5);
    assert!(hulls(&sim).iter().all(|hull| *hull == 0.6));
    assert!(
        stock_of(&sim, outpost, "rumpfsegmente") > 1.0,
        "Ware läge bereit"
    );

    // Entsteht dort Rüstungskapital, hat die Kolonie eine Werft und setzt instand.
    let arms = data
        .recipes
        .iter()
        .position(|recipe| recipe.sector == Sector::Arms)
        .unwrap();
    sim.world_mut().resource_mut::<Economy>().planets[colony.index()].capital[arms] = 1.0;
    sim.run_days(1);
    assert!(hulls(&sim).iter().all(|hull| *hull > 0.6));
    assert_eq!(money_drift(sim.world()), Some(0));
    assert_eq!(check_invariants(sim.world()), Ok(()));
}

// --- 6. Sold ----------------------------------------------------------------------------

#[test]
fn the_pay_goes_to_the_crews_once_a_month_and_the_households_get_less() {
    let data = data();
    let me = PolityId(0);
    let mut sim = with_player(16);
    let mut control = with_player(16);
    let cruiser = design("speer");
    let home = home_of(&sim, me).1;
    // Beide Partien laufen bis an den Monatsersten. Dann wächst in der einen die Flotte um
    // vier Kreuzer; sonst sind beide gleich.
    for sim in [&mut sim, &mut control] {
        sim.run_days(DAYS_PER_MONTH);
    }
    assert_eq!(sim.clock().day(), DAYS_PER_MONTH);
    {
        let mut fleets = sim.world_mut().resource_mut::<Fleets>();
        for _ in 0..4 {
            fleets.add_ship(data, FleetId(0), cruiser).unwrap();
        }
    }
    let due = monthly_upkeep(data, fleets(&sim), me);
    let extra = due - monthly_upkeep(data, fleets(&control), me);
    let value = 4.0 * data.designs[cruiser as usize].stats.value;
    assert!((to_credits(extra) - value * data.balance.military.upkeep_per_month).abs() < 0.002);
    assert!(extra > 0);

    // Im Takt des Monatsersten fließt genau der Mehrbetrag als Sold an die Besatzungen,
    // nicht an die Haushalte der Welten. Dazu kauft der Staat dem Kontor an diesem Tag ab,
    // was vier Kreuzer mehr verbrauchen: Treibstoff im Dock und Ersatzteile.
    sim.tick();
    control.tick();
    assert_eq!(staff(&sim, me) - staff(&control, me), extra);
    let bought = economy(&sim).market(home).cash - economy(&control).market(home).cash;
    assert!(bought > 0 && bought < extra);
    assert_eq!(
        paid_for(&sim, me, Claim::Fleet) + paid_for(&sim, me, Claim::Procurement)
            - paid_for(&control, me, Claim::Fleet)
            - paid_for(&control, me, Claim::Procurement),
        extra + bought
    );
    // Die Flotte geht der Ausschüttung vor: Was sie in diesem Monat mehr verlangt, hat der
    // Haushalt zurückgehalten. Die Haushalte bekommen also weniger als in der Gegenprobe,
    // statt wie früher den Sold selbst zu erhalten.
    let withheld = budget_month(&control, me).handout() - budget_month(&sim, me).handout();
    assert!(withheld > 0, "die größere Flotte kürzt die Ausschüttung");
    assert_eq!(households(&control, me) - households(&sim, me), withheld);
    assert_eq!(
        treasury(&control, me) - treasury(&sim, me),
        extra + bought - withheld
    );
    assert_eq!(fleets(&sim).arrears_of(me), 0.0);
    assert_eq!(money_drift(sim.world()), Some(0));
    assert_eq!(check_invariants(sim.world()), Ok(()));
}

#[test]
fn an_unaffordable_fleet_empties_the_treasury_and_leaves_arrears() {
    let data = data();
    let me = PolityId(0);
    let mut sim = with_player(16);
    let mut control = with_player(16);
    for sim in [&mut sim, &mut control] {
        sim.run_days(DAYS_PER_MONTH);
    }
    // Eine Flotte, deren Sold keine Staatskasse trägt. (Wie eine Gemeinschaft im Lauf
    // einer Partie dorthin kommt, zeigen die Prüfungen in `tests/budget.rs`; hier geht es
    // um die Rechnung des einen Monatsersten.)
    {
        let mut fleets = sim.world_mut().resource_mut::<Fleets>();
        for _ in 0..2000 {
            fleets.add_ship(data, FleetId(0), design("speer")).unwrap();
        }
    }
    let due = monthly_upkeep(data, fleets(&sim), me);
    let usual = monthly_upkeep(data, fleets(&control), me);
    assert!(due > 3 * treasury(&sim, me));
    sim.run_days(1);
    control.run_days(1);

    // Der Haushalt bedient die Ansprüche der Reihe nach. Die Grundleistungen gehen der
    // Flotte vor und fließen wie in der Gegenprobe; alles Übrige bekommt die Flotte, auch
    // die Rücklage. Für Beschaffung, Forschung und Ausschüttung bleibt nichts.
    assert_eq!(treasury(&sim, me), 0);
    let civil = paid_for(&control, me, Claim::Civil);
    assert!(civil > 0);
    assert_eq!(paid_for(&sim, me, Claim::Civil), civil);
    let researched = control.research().get(me).unwrap().spent_last_month;
    assert!(researched > 0, "in der Gegenprobe wird geforscht");
    assert_eq!(sim.research().get(me).unwrap().spent_last_month, 0);
    assert_eq!(paid_for(&sim, me, Claim::Procurement), 0);
    assert_eq!(budget_month(&sim, me).handout(), 0);
    let handout = budget_month(&control, me).handout();
    assert!(handout > 0);
    assert_eq!(households(&control, me) - households(&sim, me), handout);
    // Was die Gegenprobe nach den Grundleistungen ausgab oder behielt, ging hier ganz als
    // Sold an die Besatzungen: Sold, Gehälter, Einkäufe des Tages, Ausschüttung und Rest.
    let home = home_of(&sim, me).1;
    let bought = economy(&control).market(home).cash - economy(&sim).market(home).cash;
    assert!(bought > 0);
    let available = treasury(&control, me) + usual + researched + bought + handout;
    assert_eq!(
        staff(&sim, me) - staff(&control, me),
        available - usual - researched,
        "die ganze Kasse ging an die Besatzungen"
    );
    assert_eq!(paid_for(&sim, me, Claim::Fleet), available);
    // Der ausgebliebene Anteil des Anspruchs (Sold und Wartung) steht geglättet als
    // Rückstand in den Büchern …
    let claim = due + maintenance_cost(&sim, me, home);
    assert!(claim > due);
    let missed = (1.0 - available as f64 / claim as f64) as f32;
    let arrears = fleets(&sim).arrears_of(me);
    assert!((arrears - data.balance.military.arrears_smoothing * missed).abs() < 1e-5);
    assert!(arrears > 0.3);
    assert_eq!(fleets(&control).arrears_of(me), 0.0);
    assert_eq!(fleets(&sim).arrears_of(PolityId(1)), 0.0);
    // … im Haushalt als gekürzter Anspruch …
    assert!(budget_month(&sim, me).unfunded >= claim - available);
    assert_eq!(budget_month(&control, me).unfunded, 0);
    // … und die Chronik meldet den Beginn, einmal.
    let notices = |sim: &Headless| {
        sim.world()
            .resource::<Chronicle>()
            .entries
            .iter()
            .filter(|entry| {
                entry.text.contains("kann seine Flotte nicht mehr bezahlen")
                    && entry.polity == Some(me)
            })
            .count()
    };
    assert_eq!(notices(&sim), 1);
    assert_eq!(notices(&control), 0);
    assert_eq!(money_drift(sim.world()), Some(0));

    // Im Lauf des Monats ändert sich der Rückstand nicht; am nächsten Monatsanfang wächst er.
    sim.run_days(DAYS_PER_MONTH - 1);
    assert_eq!(fleets(&sim).arrears_of(me), arrears);
    sim.run_days(1);
    assert!(fleets(&sim).arrears_of(me) > arrears);
    assert_eq!(notices(&sim), 1);
    assert_eq!(
        summarize(&sim).polities[0].arrears,
        fleets(&sim).arrears_of(me) as f64
    );
    assert_eq!(money_drift(sim.world()), Some(0));
    assert_eq!(check_invariants(sim.world()), Ok(()));
}

// --- 7. Befehle -------------------------------------------------------------------------

#[test]
fn fleets_are_split_merged_and_given_standing_orders_by_command() {
    let me = PolityId(0);
    let rival = PolityId(1);
    let mut sim = with_player(17);
    let home = home_of(&sim, me).1;
    let all: Vec<ShipId> = fleet(&sim, FleetId(0))
        .ships
        .iter()
        .map(|ship| ship.id)
        .collect();
    let foreign_ship = fleet(&sim, FleetId(1)).ships[0].id;
    let first = FleetId(0);
    let split = |ships: &[ShipId]| SimCommand::SplitFleet {
        fleet: first,
        ships: ships.to_vec(),
    };

    // Teilen: nicht ohne Schiffe, nicht mit fremden, nicht doppelt, nicht alle und keine
    // fremden Flotten.
    assert!(rejection(&mut sim, me, split(&[])).contains("keine Schiffe"));
    assert!(rejection(&mut sim, me, split(&[foreign_ship])).contains("gehört nicht"));
    assert!(rejection(&mut sim, me, split(&[all[0], all[0]])).contains("mehrfach"));
    assert!(rejection(&mut sim, me, split(&all)).contains("mindestens ein Schiff"));
    assert!(rejection(&mut sim, rival, split(&all[..1])).contains("anderen Gemeinschaft"));
    assert_eq!(fleets(&sim).list.len(), sim.polities().list.len());

    assert_eq!(
        command(&mut sim, me, split(&all[..2])),
        CommandOutcome::Applied
    );
    let second = FleetId(fleets(&sim).list.len() as u32 - 1);
    assert_eq!(fleet(&sim, second).owner, me);
    assert_eq!(fleet(&sim, second).system, home);
    let ids = |sim: &Headless, id: FleetId| -> Vec<ShipId> {
        fleet(sim, id).ships.iter().map(|ship| ship.id).collect()
    };
    assert_eq!(ids(&sim, second), all[..2].to_vec());
    assert_eq!(ids(&sim, first), all[2..].to_vec());
    let start = data().balance.fleets.start_strength;
    assert!((fleet(&sim, first).strength + fleet(&sim, second).strength - start).abs() < 1e-3);

    // Stehende Befehle: nur für eigene, bestehende Flotten.
    let orders = |fleet: FleetId| SimCommand::SetFleetOrders {
        fleet,
        formation: Formation::Pincer,
        doctrine: Doctrine::Defensive,
    };
    assert!(rejection(&mut sim, rival, orders(second)).contains("anderen Gemeinschaft"));
    assert!(rejection(&mut sim, me, orders(FleetId(9_999))).contains("unbekannt"));
    assert_eq!(fleet(&sim, second).formation, Formation::Line);
    assert_eq!(
        command(&mut sim, me, orders(second)),
        CommandOutcome::Applied
    );
    assert_eq!(fleet(&sim, second).formation, Formation::Pincer);
    assert_eq!(fleet(&sim, second).doctrine, Doctrine::Defensive);
    assert_eq!(fleet(&sim, first).doctrine, Doctrine::Balanced);

    // Vereinigen: nicht mit sich selbst, nicht mit Fremden, nicht im Gefecht, nicht unterwegs,
    // nicht über Systeme hinweg.
    let merge = |from: FleetId, into: FleetId| SimCommand::MergeFleets { from, into };
    assert!(rejection(&mut sim, me, merge(second, second)).contains("sich selbst"));
    assert!(rejection(&mut sim, me, merge(FleetId(1), first)).contains("anderen Gemeinschaft"));
    assert!(rejection(&mut sim, me, merge(second, FleetId(1))).contains("anderen Gemeinschaft"));
    let engage = |sim: &mut Headless, engaged: bool| {
        assert!(
            !sim.clock().is_day_start(),
            "sonst setzt der Tagesschritt die Lage neu"
        );
        sim.world_mut().resource_mut::<Fleets>().list[second.index()].in_battle = engaged;
    };
    engage(&mut sim, true);
    assert!(rejection(&mut sim, me, merge(second, first)).contains("Gefecht"));
    engage(&mut sim, true);
    assert!(rejection(&mut sim, me, merge(first, second)).contains("Gefecht"));
    engage(&mut sim, true);
    let one_of_two = SimCommand::SplitFleet {
        fleet: second,
        ships: all[..1].to_vec(),
    };
    assert!(rejection(&mut sim, me, one_of_two).contains("Gefecht"));
    engage(&mut sim, false);

    let far = home_of(&sim, rival).1;
    assert_eq!(
        command(
            &mut sim,
            me,
            SimCommand::MoveFleet {
                fleet: second,
                to: far
            }
        ),
        CommandOutcome::Applied
    );
    sim.run_days(1);
    assert!(
        !fleet(&sim, second).is_docked(),
        "die Flotte ist auf der Route"
    );
    assert!(rejection(&mut sim, me, merge(second, first)).contains("unterwegs"));
    assert!(rejection(&mut sim, me, merge(first, second)).contains("unterwegs"));
    let part = ids(&sim, second);
    assert!(
        rejection(
            &mut sim,
            me,
            SimCommand::SplitFleet {
                fleet: second,
                ships: part[..1].to_vec()
            }
        )
        .contains("unterwegs")
    );
    sail(&mut sim, second, far);
    assert!(rejection(&mut sim, me, merge(second, first)).contains("selben System"));

    // Wieder im selben System gelingt es: Kein Schiff geht verloren, keines ist doppelt.
    sim.issue(
        me,
        SimCommand::MoveFleet {
            fleet: second,
            to: home,
        },
    );
    sail(&mut sim, second, home);
    assert_eq!(
        command(&mut sim, me, merge(second, first)),
        CommandOutcome::Applied
    );
    assert!(!fleet(&sim, second).alive);
    assert!(fleet(&sim, second).ships.is_empty());
    let mut merged = ids(&sim, first);
    merged.sort_unstable();
    assert_eq!(merged, all);
    // Die aufgelöste Flotte nimmt keine Befehle mehr an.
    assert!(rejection(&mut sim, me, merge(first, second)).contains("besteht nicht mehr"));
    assert!(rejection(&mut sim, me, orders(second)).contains("besteht nicht mehr"));
    assert_eq!(check_invariants(sim.world()), Ok(()));
}

#[test]
fn build_orders_are_validated_and_can_be_cancelled_without_refund() {
    let data = data();
    let me = PolityId(0);
    let rival = PolityId(1);
    let mut sim = with_player(17);
    let (capital, home) = home_of(&sim, me);
    let theirs = home_of(&sim, rival).0;
    set_stock(&mut sim, home, &PARTS, 100.0);
    let build = |planet: PlanetId, key: &str| SimCommand::BuildShip {
        planet,
        design: design(key),
    };

    // Abgelehnt: fremde Welt, unbekannte Welt, Welt ohne Werft, Bordjäger, fehlende
    // Technologie, unbekannter Entwurf.
    assert!(rejection(&mut sim, me, build(theirs, "dolch")).contains("gehört nicht"));
    assert!(rejection(&mut sim, me, build(PlanetId(99_999), "dolch")).contains("unbekannt"));
    assert!(rejection(&mut sim, me, build(capital, "wespe")).contains("Bordjäger"));
    assert!(rejection(&mut sim, me, build(capital, "speer_2")).contains("Technologie"));
    let unknown = SimCommand::BuildShip {
        planet: capital,
        design: 9_999,
    };
    assert!(rejection(&mut sim, me, unknown).contains("Entwurf unbekannt"));
    // Eine frisch gegründete Kolonie hat kein Rüstungskapital und damit keine Werft.
    let colony = {
        let world = sim.world();
        sim::ai::best_colony_target(
            data,
            world.resource::<Galaxy>(),
            world.resource::<Polities>(),
            world.resource::<Economy>(),
            me,
            sim.clock().day(),
        )
        .expect("eine freie Welt in Reichweite")
    };
    assert_eq!(
        command(&mut sim, me, SimCommand::ClaimPlanet { planet: colony }),
        CommandOutcome::Applied
    );
    assert!(rejection(&mut sim, me, build(colony, "dolch")).contains("keine Werft"));
    assert!(fleets(&sim).orders.is_empty());

    // Die Warteschlange einer Werft ist begrenzt.
    let limit = data.balance.military.yard_queue as usize;
    for _ in 0..limit {
        assert_eq!(
            command(&mut sim, me, build(capital, "dolch")),
            CommandOutcome::Applied
        );
    }
    assert!(rejection(&mut sim, me, build(capital, "dolch")).contains("Aufträge"));
    assert_eq!(fleets(&sim).orders_at(capital).count(), limit);
    assert_eq!(fleets(&sim).orders_of(me).count(), limit);

    // Streichen: nur eigene, nur vorhandene Aufträge.
    assert!(
        rejection(&mut sim, rival, SimCommand::CancelBuild { order: 0 })
            .contains("anderen Gemeinschaft")
    );
    assert!(
        rejection(&mut sim, me, SimCommand::CancelBuild { order: 9_999 }).contains("unbekannt")
    );
    // Der erste Auftrag ist angefangen und bezahlt …
    sim.run_days(5);
    if sim.clock().is_day_start() {
        sim.tick();
    }
    let spent = fleets(&sim).orders[0].spent;
    assert!(spent > 0 && fleets(&sim).orders[0].progress > 0.0);
    let (cash, ships) = (treasury(&sim, me), fleets(&sim).ship_count(me));
    // … und wird gestrichen, in einem Takt, in dem sonst nichts geschieht: Der Auftrag
    // ist fort, die Staatskasse bekommt nichts zurück.
    assert_eq!(
        command(&mut sim, me, SimCommand::CancelBuild { order: 0 }),
        CommandOutcome::Applied
    );
    assert_eq!(treasury(&sim, me), cash);
    assert_eq!(fleets(&sim).orders_at(capital).count(), limit - 1);
    assert!(fleets(&sim).orders.iter().all(|order| order.id != 0));
    assert!(rejection(&mut sim, me, SimCommand::CancelBuild { order: 0 }).contains("unbekannt"));
    // Der nächste Auftrag rückt auf den freien Bauplatz.
    sim.run_days(1);
    assert!(fleets(&sim).orders[0].progress > 0.0);
    assert_eq!(fleets(&sim).ship_count(me), ships);
    assert_eq!(money_drift(sim.world()), Some(0));
    assert_eq!(check_invariants(sim.world()), Ok(()));
}

// --- 8. Determinismus -------------------------------------------------------------------

/// Eine Partie mit Bauaufträgen, Flottenbefehlen und einer Reise über 720 Tage. `run`
/// lässt die Zeit laufen: mit oder ohne Überspringen der Leertakte.
fn scripted(sim: &mut Headless, run: fn(&mut Headless, u64)) {
    let me = PolityId(0);
    let rival = PolityId(2);
    let (capital, home) = home_of(sim, me);
    let (their_capital, their_home) = home_of(sim, rival);
    // Bauteile für den Anfang; danach muss die Wirtschaft liefern.
    set_stock(sim, home, &PARTS, 12.0);
    set_stock(sim, their_home, &PARTS, 12.0);
    let build = |planet: PlanetId, key: &str| SimCommand::BuildShip {
        planet,
        design: design(key),
    };
    for key in ["dolch", "waechter", "dolch", "wespe"] {
        sim.issue(me, build(capital, key));
    }
    sim.issue(rival, build(their_capital, "speer"));
    run(sim, 130);

    let ships: Vec<ShipId> = fleet(sim, FleetId(0)).ships[1..3]
        .iter()
        .map(|ship| ship.id)
        .collect();
    sim.issue(
        me,
        SimCommand::SplitFleet {
            fleet: FleetId(0),
            ships,
        },
    );
    let detached = FleetId(fleets(sim).list.len() as u32);
    sim.issue(
        me,
        SimCommand::SetFleetOrders {
            fleet: detached,
            formation: Formation::Wedge,
            doctrine: Doctrine::Aggressive,
        },
    );
    sim.issue(
        me,
        SimCommand::MoveFleet {
            fleet: detached,
            to: their_home,
        },
    );
    sim.issue(me, SimCommand::CancelBuild { order: 2 });
    run(sim, 170);

    sim.issue(me, build(capital, "speer"));
    sim.issue(
        me,
        SimCommand::MoveFleet {
            fleet: detached,
            to: home,
        },
    );
    run(sim, 240);
    sim.issue(
        me,
        SimCommand::MergeFleets {
            from: detached,
            into: FleetId(0),
        },
    );
    run(sim, 180);
}

fn skipping(sim: &mut Headless, days: u64) {
    sim.run_days(days);
}

fn every_tick(sim: &mut Headless, days: u64) {
    sim.run_days_exact(days);
}

#[test]
fn a_scripted_game_with_build_orders_is_deterministic() {
    let mut first = with_player(18);
    let mut second = with_player(18);
    scripted(&mut first, skipping);
    scripted(&mut second, skipping);
    assert_eq!(first.hash(), second.hash());

    // Die Befehle haben wirklich etwas bewirkt: Schiffe sind entstanden, das Protokoll
    // kennt angenommene und abgelehnte Befehle, und ohne sie verliefe die Partie anders.
    let me = PolityId(0);
    let started = data()
        .balance
        .military
        .starting_fleet
        .iter()
        .map(|(_, count)| *count as usize)
        .sum::<usize>();
    assert!(
        fleets(&first).ship_count(me) > started,
        "in zwei Jahren ist kein Schiff fertig geworden"
    );
    let log = first.command_log();
    assert!(log.entries.iter().any(
        |entry| matches!(entry.command, SimCommand::BuildShip { .. })
            && entry.outcome == CommandOutcome::Applied
    ));
    assert!(
        log.entries
            .iter()
            .any(|entry| matches!(entry.outcome, CommandOutcome::Rejected(_)))
    );
    let mut passive = with_player(18);
    passive.run_days(first.clock().day());
    assert_ne!(passive.hash(), first.hash());
    assert_eq!(money_drift(first.world()), Some(0));
    assert_eq!(check_invariants(first.world()), Ok(()));

    // Jeden Takt zu rechnen ergibt dasselbe wie das Überspringen der Leertakte.
    let mut exact = with_player(18);
    scripted(&mut exact, every_tick);
    assert_eq!(exact.clock(), first.clock());
    assert_eq!(exact.hash(), first.hash());

    // Die Wiedergabe des Befehlsprotokolls führt zum selben Zustand.
    let mut replay = with_player(18);
    for polity in [me, PolityId(2)] {
        let home = home_of(&replay, polity).1;
        set_stock(&mut replay, home, &PARTS, 12.0);
    }
    replay.set_replay(&log.clone());
    replay.run_days(first.clock().day());
    assert_eq!(replay.hash(), first.hash());
}

#[test]
fn a_save_in_the_middle_of_a_build_continues_identically() {
    let data = data();
    let me = PolityId(0);
    let prepare = || {
        let mut sim = with_player(19);
        let (capital, home) = home_of(&sim, me);
        set_stock(&mut sim, home, &PARTS, 200.0);
        for key in ["waechter", "dolch"] {
            sim.issue(
                me,
                SimCommand::BuildShip {
                    planet: capital,
                    design: design(key),
                },
            );
        }
        // Ein beschädigtes, halb versorgtes Schiff: Auch sein Zustand gehört in den Spielstand.
        {
            let mut fleets = sim.world_mut().resource_mut::<Fleets>();
            let slots = data.designs[fleets.list[0].ships[0].design as usize]
                .modules
                .len();
            let ship = &mut fleets.list[0].ships[0];
            ship.hull = 0.7;
            ship.modules = vec![0.5; slots];
            ship.ammo = 0.3;
            fleets.refresh(data);
        }
        sim
    };
    let mut straight = prepare();
    straight.run_days(160);

    let mut first_half = prepare();
    first_half.run_days(40);
    {
        let fleets = fleets(&first_half);
        assert_eq!(fleets.orders.len(), 2);
        let progress = fleets.orders[0].progress;
        assert!(
            progress > 0.1 && progress < 0.9,
            "mitten im Bau: {progress}"
        );
    }
    let text = first_half.save().to_ron().unwrap();
    let save = SaveGame::from_ron(&text).unwrap();
    assert!(save.matches_data(data));
    let mut resumed = Headless::from_save(data.clone(), save);
    assert_eq!(
        resumed.hash(),
        first_half.hash(),
        "Laden stellt den Zustand exakt her"
    );
    assert_eq!(fleets(&resumed).orders, fleets(&first_half).orders);
    assert_eq!(
        fleet(&resumed, FleetId(0)).ships,
        fleet(&first_half, FleetId(0)).ships
    );
    resumed.run_days(120);
    assert_eq!(resumed.hash(), straight.hash());
    // In der Zwischenzeit sind beide Schiffe fertig geworden.
    assert!(fleets(&resumed).orders.is_empty());
    assert_eq!(check_invariants(resumed.world()), Ok(()));

    // Spielstände aus der Zeit vor den Schiffen werden am Format erkannt und abgelehnt.
    let mut old = first_half.save();
    old.format = 1;
    let error = SaveGame::from_ron(&old.to_ron().unwrap()).unwrap_err();
    assert!(error.to_string().contains("Format 1"), "{error}");
}

// --- 9. Gefechte: Beginn, Verlauf, Rückgabe ------------------------------------------------

fn battles(sim: &Headless) -> &Battles {
    sim.world().resource::<Battles>()
}

fn finance(sim: &Headless) -> &Finance {
    sim.world().resource::<Finance>()
}

fn weariness(sim: &Headless, polity: PolityId) -> f64 {
    finance(sim).polity(polity).war_weariness
}

/// Gibt es zu diesem System einen Chronikeintrag, der alle genannten Textstücke enthält?
fn chronicled(sim: &Headless, system: SystemId, parts: &[&str]) -> bool {
    sim.world()
        .resource::<Chronicle>()
        .entries
        .iter()
        .any(|entry| {
            entry.system == Some(system) && parts.iter().all(|part| entry.text.contains(part))
        })
}

/// Reiht zusätzliche, voll versorgte Schiffe in eine Flotte ein (Aufbau eines Szenarios).
fn add_ships(sim: &mut Headless, id: FleetId, ships: &[(&str, u32)]) {
    let mut fleets = sim.world_mut().resource_mut::<Fleets>();
    for (key, count) in ships {
        for _ in 0..*count {
            fleets.add_ship(data(), id, design(key)).unwrap();
        }
    }
}

/// Stellt für eine Gemeinschaft eine weitere Flotte auf (Aufbau eines Szenarios).
fn raise_fleet(
    sim: &mut Headless,
    owner: PolityId,
    system: SystemId,
    ships: &[(&str, u32)],
) -> FleetId {
    let polities = sim.polities().clone();
    let id = sim
        .world_mut()
        .resource_mut::<Fleets>()
        .new_fleet(&polities, owner, system);
    add_ships(sim, id, ships);
    id
}

/// Versetzt eine Flotte ohne Marsch in ein System (Aufbau eines Szenarios).
fn place(sim: &mut Headless, id: FleetId, system: SystemId) {
    let mut fleets = sim.world_mut().resource_mut::<Fleets>();
    let fleet = &mut fleets.list[id.index()];
    fleet.system = system;
    fleet.route.clear();
    fleet.progress = 0.0;
}

/// Bauwert der Schiffe einer Flotte, wie sie dastehen.
fn value_of(fleet: &Fleet) -> f64 {
    fleet
        .ships
        .iter()
        .map(|ship| material_value(data(), ship))
        .sum()
}

/// Bauwert aller Schiffe einer Gemeinschaft.
fn navy_value(sim: &Headless, polity: PolityId) -> f64 {
    fleets(sim).of(polity).map(value_of).sum()
}

/// Schiffe je Klasse, die Bordjäger an Bord eingeschlossen.
fn class_counts(fleet: &Fleet) -> [u32; 6] {
    let mut counts = [0; 6];
    for ship in &fleet.ships {
        counts[data().designs[ship.design as usize].stats.class.index()] += 1;
        counts[ShipClass::Fighter.index()] += ship.fighters;
    }
    counts
}

/// Längste Dauer eines Gefechts in Tagen: die Zeitgrenze des Rechners.
fn longest_battle_days(data: &GameData) -> u64 {
    (data.balance.battle.time_limit / data.balance.battle.dt) as u64 / TICKS_PER_DAY + 2
}

/// Lässt die Partie tageweise laufen, bis das Gefecht im System beendet ist, und gibt
/// seine Bilanz zurück. Geld und Invarianten stimmen an jedem Tag.
fn fight_out(sim: &mut Headless, system: SystemId) -> BattleReport {
    let id = battles(sim)
        .in_system(system)
        .expect("im System läuft ein Gefecht")
        .id;
    for _ in 0..longest_battle_days(data()) {
        sim.run_days(1);
        assert_eq!(money_drift(sim.world()), Some(0));
        assert_eq!(check_invariants(sim.world()), Ok(()));
        if battles(sim).get(id).is_none() {
            return battles(sim)
                .report(id)
                .expect("zu jedem beendeten Gefecht gibt es eine Bilanz")
                .clone();
        }
    }
    panic!("das Gefecht endete nie");
}

fn side_of(report: &BattleReport, fleet: FleetId) -> &SideReport {
    report
        .sides
        .iter()
        .find(|side| side.fleet == Some(fleet))
        .expect("die Flotte war eine Partei des Gefechts")
}

/// Tag, an dem ein Gefecht endete.
fn end_day(report: &BattleReport) -> u64 {
    report.ended / TICKS_PER_DAY
}

/// Zwei benachbarte Systeme, die niemandem gehören und in denen keine Flotte liegt.
fn free_neighbors(sim: &Headless) -> (SystemId, SystemId) {
    let galaxy = sim.galaxy();
    let free = |system: SystemId| {
        galaxy.system_owner(system).is_none() && fleets(sim).docked_in(system).next().is_none()
    };
    for system in &galaxy.systems {
        if !free(system.id) {
            continue;
        }
        if let Some(neighbor) = galaxy
            .neighbors(system.id)
            .iter()
            .find(|neighbor| free(neighbor.system))
        {
            return (system.id, neighbor.system);
        }
    }
    panic!("keine zwei freien Nachbarsysteme");
}

/// Hat ein Schiff das Gefecht überstanden? Nach dem letzten Schritt des Rechners ist alles
/// zerstört, was verloren ging, auch treibende Schiffe, die dem Gegner überlassen blieben.
fn survived(ship: &BattleShip) -> bool {
    ship.status != ShipStatus::Destroyed
}

/// Steht am Ende noch ein Schiff der Flotte auf dem Feld: kämpfend oder treibend? Wer
/// entkommen ist, steht dort nicht mehr; Bordjäger zählen nicht.
fn stands_on_the_field(battle: &Battle, side: usize) -> bool {
    battle.ships.iter().any(|ship| {
        ship.side as usize == side
            && ship.source.is_some()
            && matches!(ship.status, ShipStatus::Active | ShipStatus::Disabled)
    })
}

#[test]
fn fleets_at_war_fight_it_out_and_the_result_is_written_back() {
    let data = data();
    let (a, b) = (PolityId(0), PolityId(1));
    let (ours, theirs) = (FleetId(0), FleetId(1));
    let mut sim = with_player(22);
    let target = home_of(&sim, b).1;
    let place_name = sim.galaxy().system(target).name.clone();
    // Beide Flotten führen einen Träger mit; die Angreiferin liegt schon im System.
    for id in [ours, theirs] {
        add_ships(&mut sim, id, &[("horst", 1)]);
    }
    place(&mut sim, ours, target);

    // Ohne Krieg kein Gefecht, so lange die Flotten auch nebeneinanderliegen.
    sim.run_days(3);
    assert_eq!(battles(&sim).next_id, 0);
    assert!(battles(&sim).active.is_empty() && battles(&sim).reports.is_empty());
    assert!(!fleet(&sim, ours).in_battle && !fleet(&sim, theirs).in_battle);
    assert!(!needs_fine_ticks(sim.world()));

    // Der Krieg wird mitten am Tag erklärt. Das Gefecht beginnt am nächsten Morgen.
    for _ in 0..7 {
        sim.tick();
    }
    assert_eq!(
        command(&mut sim, a, SimCommand::DeclareWar { target: b }),
        CommandOutcome::Applied
    );
    while !sim.clock().is_day_start() {
        sim.tick();
        assert!(
            battles(&sim).active.is_empty(),
            "kein Gefecht vor Tagesbeginn"
        );
    }
    let began = sim.clock().tick;
    sim.tick();
    let before = [fleet(&sim, ours).clone(), fleet(&sim, theirs).clone()];
    {
        let battle = battles(&sim)
            .in_system(target)
            .expect("das Gefecht hat begonnen");
        assert_eq!((battle.id, battle.tick), (BattleId(0), 1));
        assert_eq!(battles(&sim).next_id, 1);
        assert_eq!(battle.combatants.len(), 2);
        assert!(battle.is_hostile(0, 1));
        // Die Parteien stehen in der Nummernfolge der Flotten.
        for (place, fleet) in before.iter().enumerate() {
            let (found, side) = battles(&sim).of_fleet(fleet.id).unwrap();
            assert_eq!((found.id, side), (BattleId(0), place));
            let combatant = &battle.combatants[side];
            assert_eq!(
                (combatant.fleet, combatant.owner),
                (Some(fleet.id), fleet.owner)
            );
            assert_eq!(
                (combatant.formation, combatant.doctrine),
                (fleet.formation, fleet.doctrine)
            );
            // Noch ist niemand kriegsmüde und der Sold bezahlt: die volle Ausgangsmoral.
            assert_eq!(combatant.morale, data.balance.battle.morale_start);
            // Die Schiffe kämpfen mit der Technik ihrer Gemeinschaft.
            assert_eq!(
                combatant.modifiers,
                combat_modifiers(sim.research(), fleet.owner)
            );
            assert!(fleet.in_battle && fleet.battle == Some(BattleId(0)));
            // Jedes Schiff steht mit seiner Nummer im Gefecht.
            let sources: Vec<ShipId> = battle
                .ships
                .iter()
                .filter(|ship| ship.side as usize == side)
                .filter_map(|ship| ship.source)
                .collect();
            let ids: Vec<ShipId> = fleet.ships.iter().map(|ship| ship.id).collect();
            assert_eq!(sources, ids);
        }
        assert!(chronicled(
            &sim,
            target,
            &[
                "beginnt ein Gefecht",
                &place_name,
                &before[0].name,
                &before[1].name
            ]
        ));
        assert!(needs_fine_ticks(sim.world()));
    }

    // Takt für Takt bis zum Ende. Ein Schattenbild rechnet den letzten Schritt nach: Es
    // ist der Stand, aus dem das Ergebnis in die Flotten zurückgeschrieben wurde.
    let mut shadow: Battle = battles(&sim).active[0].clone();
    let limit = began + longest_battle_days(data) * TICKS_PER_DAY;
    let mut thinned_while_fighting = false;
    loop {
        let morning = sim.clock().is_day_start();
        sim.tick();
        let Some(battle) = battles(&sim).get(BattleId(0)) else {
            break;
        };
        assert!(sim.clock().tick < limit, "das Gefecht endet nie");
        if morning {
            // Jeden Morgen steht in den Flotten, was das Gefecht sie bis dahin gekostet hat:
            // `shadow` ist der Stand vor diesem Takt. Zerstörte Schiffe sind fort, auch
            // aus dem Sold; die übrigen tragen ihren Rumpf, wie der Rechner ihn führt.
            for old in &before {
                let now = self::fleet(&sim, old.id);
                assert!(now.in_battle && now.battle == Some(BattleId(0)));
                let fought = |ship: ShipId| {
                    shadow
                        .ships
                        .iter()
                        .find(|fought| fought.source == Some(ship))
                        .expect("das Schiff stand im Gefecht")
                };
                let afloat: Vec<ShipId> = old
                    .ships
                    .iter()
                    .filter(|ship| survived(fought(ship.id)))
                    .map(|ship| ship.id)
                    .collect();
                let listed: Vec<ShipId> = now.ships.iter().map(|ship| ship.id).collect();
                assert_eq!(listed, afloat);
                thinned_while_fighting |= afloat.len() < old.ships.len();
                for ship in &now.ships {
                    let fought = fought(ship.id);
                    if fought.status != ShipStatus::Disabled {
                        assert!((ship.hull - fought.hull / fought.hull_max).abs() < 1e-6);
                    }
                }
                let value: f64 = now
                    .ships
                    .iter()
                    .map(|ship| data.designs[ship.design as usize].stats.value)
                    .sum();
                assert_eq!(
                    monthly_upkeep(data, fleets(&sim), old.owner),
                    sim::economy::to_money(value * data.balance.military.upkeep_per_month)
                );
                assert!((now.strength - fleet_strength(data, now)).abs() < 1e-3);
            }
        }
        shadow = battle.clone();
    }
    assert!(
        thinned_while_fighting,
        "Szenario neu wählen: Schon vor dem Ende sollte ein Schiff verloren gehen"
    );
    let ended = sim.clock().tick - 1;
    shadow.step(data);
    assert!(shadow.finished && !shadow.timed_out);
    assert_eq!(shadow.tick, ended + 1 - began, "ein Schritt je Takt");
    assert!(!needs_fine_ticks(sim.world()));

    let report = battles(&sim).reports.last().unwrap().clone();
    assert_eq!(battles(&sim).report(BattleId(0)), Some(&report));
    assert_eq!(
        (report.id, report.system, report.started, report.ended),
        (BattleId(0), target, began, ended)
    );
    assert_eq!(report.outcome, BattleOutcome::Decided);
    assert_eq!(report.days(), (ended - began).div_ceil(TICKS_PER_DAY));
    assert_eq!(report.sides.len(), 2);

    let mut ships_lost = 0;
    let mut value_lost = 0.0;
    for old in &before {
        let side = shadow
            .combatants
            .iter()
            .position(|combatant| combatant.fleet == Some(old.id))
            .unwrap();
        let held = stands_on_the_field(&shadow, side);
        let after = fleet(&sim, old.id);
        let place_of = |ship: ShipId| {
            shadow
                .ships
                .iter()
                .position(|fought| fought.source == Some(ship))
                .expect("das Schiff stand im Gefecht")
        };
        // Verlorene Schiffe sind fort, die übrigen in ihrer Reihenfolge noch da.
        let survivors: Vec<ShipId> = old
            .ships
            .iter()
            .filter(|ship| survived(&shadow.ships[place_of(ship.id)]))
            .map(|ship| ship.id)
            .collect();
        let remaining: Vec<ShipId> = after.ships.iter().map(|ship| ship.id).collect();
        assert_eq!(remaining, survivors);
        ships_lost += old.ships.len() - survivors.len();
        // Eine Flotte ohne Schiffe besteht nicht mehr.
        assert_eq!(after.alive, !survivors.is_empty());
        assert!(!after.in_battle && after.battle.is_none());
        // Im Hangar steht, was nie gestartet war, dazu jeder Jäger der Flotte, der
        // überlebt hat: Der Rechner hat sie auf die Träger verteilt.
        let aloft = shadow
            .ships
            .iter()
            .filter(|craft| {
                craft.side as usize == side && craft.source.is_none() && survived(craft)
            })
            .count() as u32;
        let hangars: u32 = after
            .ships
            .iter()
            .map(|ship| shadow.ships[place_of(ship.id)].hangar)
            .sum();
        let on_board: u32 = after.ships.iter().map(|ship| ship.fighters).sum();
        assert_eq!(on_board, hangars + aloft);
        for ship in &after.ships {
            let place = place_of(ship.id);
            let fought = &shadow.ships[place];
            let design = &data.designs[ship.design as usize];
            // Rumpf, Module, Munition und Treibstoff als Anteile am Höchstwert.
            assert!((ship.hull - fought.hull / fought.hull_max).abs() < 1e-6);
            assert!(ship.hull > 0.0);
            assert!((ship.ammo - fought.ammo / fought.ammo_max).abs() < 1e-6);
            assert!((ship.fuel - fought.fuel / fought.fuel_max).abs() < 1e-6);
            let modules: Vec<f32> = design
                .modules
                .iter()
                .zip(&fought.modules)
                .map(|(slot, state)| state.hp / data.modules[slot.module_id as usize].hp)
                .collect();
            if modules.iter().all(|module| *module >= 1.0) {
                assert!(ship.modules.is_empty());
            } else {
                assert_eq!(ship.modules, modules);
            }
            // Mehr als der Hangar fasst, kommt nicht an Bord.
            assert!(ship.fighters >= fought.hangar);
            assert!(ship.fighters <= design.stats.fighters);
        }

        // Die Bilanz: Was zu Beginn da war, ist verloren oder noch da, Klasse für Klasse.
        let entry = side_of(&report, old.id);
        assert_eq!(*entry, report.sides[side], "die Bilanz folgt den Parteien");
        assert_eq!((entry.owner, entry.fleet), (old.owner, Some(old.id)));
        assert_eq!(entry.fleet_name, old.name);
        assert_eq!(entry.held_field, held);
        let (initial, left) = (class_counts(old), class_counts(after));
        assert_eq!(entry.initial, initial);
        for class in 0..initial.len() {
            assert_eq!(
                entry.lost[class],
                initial[class] - left[class],
                "Klasse {class}"
            );
        }
        assert!((entry.initial_value as f64 - value_of(old)).abs() < 0.5);
        assert!((entry.lost_value as f64 - (value_of(old) - value_of(after))).abs() < 0.5);
        value_lost += entry.lost_value;
    }
    assert_eq!(battles(&sim).total_ships_lost() as usize, ships_lost);
    assert_eq!(
        battles(&sim).ships_lost_by(a) + battles(&sim).ships_lost_by(b),
        ships_lost as u32
    );
    // Was verloren ging, treibt als Trümmer im System.
    assert!((battles(&sim).debris_in(target) - value_lost).abs() < 0.5);
    assert_eq!(battles(&sim).debris.len(), 1);
    assert!(chronicled(
        &sim,
        target,
        &["Gefecht bei", &place_name, "entschieden"]
    ));

    // Dieses Szenario: Eine Flotte hält das Feld und trägt, was das Gefecht ihr zugefügt
    // und abverlangt hat; die andere räumt es mit dem, was ihr blieb, und sammelt sich.
    let (lost, won) = if side_of(&report, ours).held_field {
        (1, 0)
    } else {
        (0, 1)
    };
    let (loser, winner) = (fleet(&sim, before[lost].id), fleet(&sim, before[won].id));
    assert!(!side_of(&report, loser.id).held_field && side_of(&report, winner.id).held_field);
    assert!(
        loser.alive && winner.alive,
        "Szenario neu wählen: Die unterlegene Flotte sollte mit Schiffen entkommen"
    );
    assert!(side_of(&report, loser.id).retreated);
    assert!(loser.ships.len() < before[lost].ships.len());
    assert!(loser.is_regrouping() && loser.route.len() == 1);
    assert!(winner.is_ready() && winner.system == target && winner.route.is_empty());
    assert!(chronicled(
        &sim,
        target,
        &[&before[lost].name, "weicht nach"]
    ));
    assert!(winner.ships.iter().any(|ship| ship.hull < 1.0));
    assert!(winner.ships.iter().any(|ship| !ship.modules.is_empty()));
    assert!(winner.ships.iter().any(|ship| ship.ammo < 0.9));
    assert!(winner.ships.iter().all(|ship| ship.fuel < 1.0));
    assert!((winner.strength - fleet_strength(data, winner)).abs() < 1e-3);
    assert!(winner.strength < before[won].strength);
    let carrier = winner
        .ships
        .iter()
        .find(|ship| ship.design == design("horst"))
        .expect("Szenario neu wählen: Der Träger der Siegerin sollte überleben");
    assert!(
        carrier.fighters < data.designs[carrier.design as usize].stats.fighters,
        "verlorene Jäger kehren nicht zurück"
    );
    // Verluste machen kriegsmüde, den Sieger wie den Geschlagenen.
    let (beaten, victor) = (before[lost].owner, before[won].owner);
    assert!(side_of(&report, loser.id).lost_value > 0.0);
    assert!(side_of(&report, winner.id).lost_value > 0.0);
    assert!(weariness(&sim, beaten) > 0.0 && weariness(&sim, victor) > 0.0);
    assert_eq!(money_drift(sim.world()), Some(0));
    assert_eq!(check_invariants(sim.world()), Ok(()));

    // Am nächsten Morgen beginnt nichts von vorn: Die Geschlagene sammelt sich und zieht ab.
    sim.run_days(2);
    assert_eq!(battles(&sim).next_id, 1);
    assert!(battles(&sim).active.is_empty());
}

#[test]
fn the_inferior_fleet_falls_back_where_it_came_from_and_regroups() {
    let data = data();
    let (a, b) = (PolityId(0), PolityId(1));
    let mut sim = with_player(40);
    // Abseits der Heimatwelten: Eine dreifach überlegene Flotte hält ein System, eine
    // einfache marschiert aus dem Nachbarsystem heran.
    let (field, rear) = free_neighbors(&sim);
    let strong = raise_fleet(
        &mut sim,
        b,
        field,
        &[("speer", 3), ("waechter", 6), ("dolch", 9)],
    );
    let weak = raise_fleet(
        &mut sim,
        a,
        rear,
        &[("speer", 1), ("waechter", 2), ("dolch", 3)],
    );
    sim.issue(a, SimCommand::DeclareWar { target: b });
    sim.issue(
        a,
        SimCommand::MoveFleet {
            fleet: weak,
            to: field,
        },
    );
    sail(&mut sim, weak, field);
    assert_eq!(fleet(&sim, weak).came_from, Some(rear));
    {
        let (battle, weak_side) = battles(&sim)
            .of_fleet(weak)
            .expect("mit der Ankunft beginnt das Gefecht");
        let (_, strong_side) = battles(&sim).of_fleet(strong).unwrap();
        assert!(
            battle.fighting_value(data, strong_side) > 2.5 * battle.fighting_value(data, weak_side),
            "die Verteidigerin ist klar überlegen"
        );
    }
    let ships = fleet(&sim, weak).ships.len();
    let report = fight_out(&mut sim, field);

    // Die Überlegene hält das Feld und bleibt, einsatzbereit.
    assert!(side_of(&report, strong).held_field && !side_of(&report, strong).retreated);
    assert!(!side_of(&report, weak).held_field);
    let holder = fleet(&sim, strong);
    assert!(holder.system == field && holder.route.is_empty() && holder.is_ready());
    // Die Unterlegene räumt das System: dorthin, woher sie kam. Sie sammelt sich.
    let beaten = fleet(&sim, weak);
    assert!(
        beaten.alive && !beaten.ships.is_empty(),
        "Szenario neu wählen: Die Unterlegene sollte mit Schiffen davonkommen"
    );
    assert_eq!(
        beaten.ships.len() as u32,
        ships as u32 - side_of(&report, weak).lost.iter().sum::<u32>()
    );
    assert_eq!(beaten.route, vec![rear]);
    let until = beaten.regroup_until.expect("die Flotte sammelt sich");
    assert_eq!(until, end_day(&report) + data.balance.military.regroup_days);
    assert!(beaten.is_regrouping() && !beaten.is_ready() && !beaten.in_battle);
    let rear_name = sim.galaxy().system(rear).name.clone();
    assert!(chronicled(&sim, field, &["weicht nach", &rear_name]));

    // Auch solange beide noch im selben System liegen, beginnt kein neues Gefecht.
    let begun = battles(&sim).next_id;
    sail(&mut sim, weak, rear);
    assert_eq!(battles(&sim).next_id, begun);
    assert_eq!(fleet(&sim, weak).came_from, Some(field));

    // Die Siegerin setzt nach. Solange sich die Geschlagene sammelt, greift sie niemand an.
    sim.issue(
        b,
        SimCommand::MoveFleet {
            fleet: strong,
            to: rear,
        },
    );
    sail(&mut sim, strong, rear);
    assert!(
        sim.clock().day() + 3 < until,
        "die Verfolgerin ist da, lange bevor die Sammelfrist endet"
    );
    let mut days_together = 0;
    while fleet(&sim, weak).is_regrouping() {
        assert!(battles(&sim).active.is_empty());
        assert_eq!(battles(&sim).next_id, begun);
        assert!(sim.clock().day() <= until);
        sim.run_days(1);
        days_together += 1;
    }
    assert!(days_together >= 3);
    // Mit dem Ende der Sammelfrist ist sie wieder einsatzbereit, und das Gefecht beginnt.
    assert!(sim.clock().day() >= until);
    assert_eq!(battles(&sim).next_id, begun + 1);
    assert!(fleet(&sim, weak).in_battle && fleet(&sim, strong).in_battle);
    assert_eq!(battles(&sim).in_system(rear).unwrap().combatants.len(), 2);
    assert_eq!(check_invariants(sim.world()), Ok(()));
}

// --- 10. Verstärkungen ---------------------------------------------------------------------

/// Ausgang eines Gefechts aus Sicht der Angreifer.
struct Clash {
    /// Hielt eine Flotte der Angreifer am Ende das Feld?
    held: bool,
    /// Verlorener Bauwert der Angreifer und der Verteidiger.
    lost_own: f32,
    lost_enemy: f32,
    /// Gefechtszeit in Sekunden und bis dahin verlorener Bauwert, als die Verstärkung eintraf.
    joined: Option<(f32, f32)>,
    sides: usize,
}

/// Zwei gleich starke Flotten kämpfen im Heimatsystem der Verteidiger. Mit `reinforce`
/// bricht eine zweite Flotte der Angreifer aus dem Nachbarsystem auf, sobald im Gefecht das
/// erste Schiff verloren ist: Sie trifft also mitten im Kampf ein.
fn clash(seed: u64, reinforce: bool) -> Clash {
    let (a, b) = (PolityId(0), PolityId(1));
    let mut sim = with_player(seed);
    let target = home_of(&sim, b).1;
    let rear = sim.galaxy().neighbors(target)[0].system;
    place(&mut sim, FleetId(0), target);
    let second = raise_fleet(
        &mut sim,
        a,
        rear,
        &[("speer", 1), ("waechter", 2), ("dolch", 3)],
    );
    sim.issue(a, SimCommand::DeclareWar { target: b });
    for day in 0.. {
        sim.run_days(1);
        let battle = battles(&sim)
            .in_system(target)
            .unwrap_or_else(|| panic!("Seed {seed}: das Gefecht endete vor dem ersten Verlust"));
        if battle.combatants.iter().any(|side| side.lost_value > 0.0) {
            break;
        }
        assert!(
            day < longest_battle_days(data()),
            "Seed {seed}: kein Verlust"
        );
    }
    if reinforce {
        sim.issue(
            a,
            SimCommand::MoveFleet {
                fleet: second,
                to: target,
            },
        );
    }
    let mut joined = None;
    for _ in 0..longest_battle_days(data()) {
        sim.run_days(1);
        let Some(battle) = battles(&sim).in_system(target) else {
            break;
        };
        if joined.is_none() && fleet(&sim, second).in_battle {
            // Die Verstärkung ist als dritte Partei dabei: Freund der eigenen, Feind der
            // gegnerischen Flotte.
            let (found, side) = battles(&sim).of_fleet(second).unwrap();
            assert_eq!((found.id, side), (battle.id, 2));
            let side_of_fleet = |id: u32| battles(&sim).of_fleet(FleetId(id)).unwrap().1 as u16;
            assert!(!battle.is_hostile(side_of_fleet(0), 2));
            assert!(battle.is_hostile(side_of_fleet(1), 2));
            assert_eq!(fleet(&sim, second).battle, Some(battle.id));
            let lost: f32 = battle.combatants.iter().map(|c| c.lost_value).sum();
            joined = Some((battle.time, lost));
            let place = sim.galaxy().system(target).name.clone();
            assert!(chronicled(
                &sim,
                target,
                &["greift mit", "in das Gefecht bei", &place]
            ));
        }
    }
    assert!(
        battles(&sim).active.is_empty(),
        "Seed {seed}: das Gefecht endet"
    );
    assert_eq!(
        battles(&sim).next_id,
        1,
        "Seed {seed}: es blieb bei einem Gefecht"
    );
    assert_eq!(check_invariants(sim.world()), Ok(()));
    let report = battles(&sim).reports.last().unwrap();
    let lost = |owner: PolityId| -> f32 {
        report
            .sides
            .iter()
            .filter(|side| side.owner == owner)
            .map(|side| side.lost_value)
            .sum()
    };
    Clash {
        held: report
            .sides
            .iter()
            .any(|side| side.owner == a && side.held_field),
        lost_own: lost(a),
        lost_enemy: lost(b),
        joined,
        sides: report.sides.len(),
    }
}

#[test]
fn reinforcements_join_a_running_battle_and_help_their_side() {
    let seeds = 50..58u64;
    let count = seeds.clone().count();
    let (mut held_alone, mut held_helped) = (0, 0);
    // Bilanz der Verluste aus Sicht der Angreifer: was der Gegner verlor, minus was sie
    // selbst verloren, über alle Seeds.
    let (mut balance_alone, mut balance_helped) = (0.0, 0.0);
    for seed in seeds {
        let alone = clash(seed, false);
        let helped = clash(seed, true);
        assert_eq!((alone.sides, alone.joined), (2, None));
        assert_eq!(helped.sides, 3, "Seed {seed}: die Verstärkung nahm teil");
        let (time, lost_before) = helped.joined.expect("die Verstärkung traf im Gefecht ein");
        // Aufgebrochen nach dem ersten Verlust: Bei der Ankunft wird seit mindestens
        // zwanzig Sekunden Gefechtszeit gekämpft, und es sind schon Schiffe verloren.
        assert!(time >= 20.0, "Seed {seed}: eingetroffen nach {time} s");
        assert!(
            lost_before > 0.0,
            "Seed {seed}: eingetroffen vor dem ersten Verlust"
        );
        held_alone += alone.held as usize;
        held_helped += helped.held as usize;
        balance_alone += alone.lost_enemy - alone.lost_own;
        balance_helped += helped.lost_enemy - helped.lost_own;
    }
    // Mit Verstärkung halten die Angreifer das Feld mindestens so oft wie ohne und in der
    // Mehrzahl der Fälle, und die Verluste verschieben sich deutlich zu ihren Gunsten: um
    // mehr als den halben Bauwert einer Startflotte.
    assert!(
        held_helped >= held_alone && held_helped * 2 > count,
        "Feld gehalten: {held_helped} mit, {held_alone} ohne Verstärkung"
    );
    let starting_fleet: f64 = data()
        .balance
        .military
        .starting_fleet_ids
        .iter()
        .map(|(design, count)| data().designs[*design as usize].stats.value * *count as f64)
        .sum();
    assert!(
        (balance_helped - balance_alone) as f64 > 0.5 * starting_fleet,
        "Verlustbilanz mit Verstärkung {balance_helped:.0}, ohne {balance_alone:.0}"
    );
}

// --- 11. Kosten des Krieges ----------------------------------------------------------------

/// Eine Partie bis zum Takt, in dem ein Gefecht mit einer schweren Niederlage endet.
struct Defeat {
    sim: Headless,
    enemy: PolityId,
    target: SystemId,
    report: BattleReport,
    /// Die Flotte des Siegers, wie sie ins Gefecht ging.
    before: Fleet,
    /// Bauwert der gesamten Flotte von Sieger und Verlierer vor dem Gefecht.
    navy: [f64; 2],
    /// Kriegsmüdigkeit von Sieger und Verlierer vor und nach dem letzten Takt.
    weary: [(f64, f64); 2],
    /// Kursstimmung jedes Emittenten vor dem letzten Takt.
    shocks: Vec<(IssuerId, f64)>,
}

/// Gemeinschaft 0 greift mit verstärkter Flotte das nächste fremde Heimatsystem an.
fn heavy_defeat(data: &GameData) -> Defeat {
    let me = PolityId(0);
    let ours = FleetId(0);
    let mut sim = Headless::new(
        data.clone(),
        SimConfig {
            seed: 28,
            player: Some(me),
        },
    );
    let (target, _) = nearest_foreign_home(&sim, me);
    let enemy = sim.galaxy().system_owner(target).unwrap();
    add_ships(&mut sim, ours, &[("speer", 1)]);
    sim.issue(me, SimCommand::DeclareWar { target: enemy });
    sim.issue(
        me,
        SimCommand::MoveFleet {
            fleet: ours,
            to: target,
        },
    );
    // Der Anmarsch; das Geld stimmt an jedem Tag.
    for _ in 0..2 * DAYS_PER_YEAR {
        if fleet(&sim, ours).in_battle {
            break;
        }
        sim.run_days(1);
        assert_eq!(money_drift(sim.world()), Some(0));
    }
    assert!(
        fleet(&sim, ours).in_battle,
        "die Flotte hat den Gegner nie erreicht"
    );
    {
        // Seit der Kriegserklärung ist ein Monatsabschluss vergangen: Die Besatzungen
        // gehen mit der Kriegsmüdigkeit ihrer Gemeinschaft ins Gefecht.
        let (battle, side) = battles(&sim).of_fleet(ours).unwrap();
        let rules = &data.balance.battle;
        let weary = weariness(&sim, me) as f32;
        assert!(weary > 0.0);
        let morale = battle.combatants[side].morale;
        assert!((morale - (rules.morale_start - rules.morale_per_weariness * weary)).abs() < 1e-4);
        assert!(morale < rules.morale_start);
    }
    let before = fleet(&sim, ours).clone();
    let navy = [navy_value(&sim, me), navy_value(&sim, enemy)];
    let limit = sim.clock().tick + longest_battle_days(data) * TICKS_PER_DAY;
    loop {
        let weary = [weariness(&sim, me), weariness(&sim, enemy)];
        let shocks: Vec<(IssuerId, f64)> = finance(&sim)
            .issuers
            .iter()
            .map(|(id, issuer)| (*id, issuer.shock))
            .collect();
        sim.tick();
        if battles(&sim).active.is_empty() {
            let report = battles(&sim).reports.last().unwrap().clone();
            let weary = [
                (weary[0], weariness(&sim, me)),
                (weary[1], weariness(&sim, enemy)),
            ];
            return Defeat {
                sim,
                enemy,
                target,
                report,
                before,
                navy,
                weary,
                shocks,
            };
        }
        assert!(sim.clock().tick < limit, "das Gefecht endet nie");
    }
}

#[test]
fn losses_wear_a_community_down_and_a_heavy_defeat_moves_its_share_prices() {
    let data = data();
    let me = PolityId(0);
    let ours = FleetId(0);
    let balance = &data.balance.military;
    let month = TICKS_PER_DAY * DAYS_PER_MONTH;
    // Dieselbe Partie zweimal; in der Gegenprobe bleibt der Kursschock nach einer schweren
    // Niederlage aus. Bis zum Ende des Gefechts verlaufen beide gleich.
    let mut calm_data = data.clone();
    calm_data.balance.military.defeat_shock = 1.0;
    let hit = heavy_defeat(data);
    let calm = heavy_defeat(&calm_data);
    assert_eq!(hit.report, calm.report);
    assert_eq!(hit.sim.clock(), calm.sim.clock());
    assert_eq!(hit.shocks, calm.shocks);
    let (enemy, target) = (hit.enemy, hit.target);
    let ended = hit.report.ended;
    assert!(
        !ended.is_multiple_of(month),
        "Szenario neu wählen: Das Gefecht endet im Takt eines Monatsabschlusses"
    );

    // Die Angreiferin hält das Feld und zahlt dafür; der Verteidiger verliert seine Flotte.
    let won = side_of(&hit.report, ours).clone();
    let lost = hit
        .report
        .sides
        .iter()
        .find(|side| side.owner == enemy)
        .unwrap()
        .clone();
    assert!(won.held_field && !lost.held_field);
    assert!(
        won.lost_value > 0.0,
        "Szenario neu wählen: Auch die Siegerin sollte Schiffe verlieren"
    );

    // Verluste machen kriegsmüde, im Verhältnis zum verlorenen Anteil an der ganzen Flotte.
    let mut jumps = [0.0; 2];
    for (index, side) in [&won, &lost].into_iter().enumerate() {
        let share = side.lost_value as f64 / hit.navy[index];
        assert!(share > 0.0 && share <= 1.0 + 1e-6, "Anteil {share}");
        let (before, after) = hit.weary[index];
        jumps[index] = after - before;
        let expected = balance.weariness_per_navy_lost * share;
        assert!(
            (jumps[index] - expected).abs() < 1e-3,
            "Kriegsmüdigkeit stieg um {} statt um {expected}",
            jumps[index]
        );
    }
    assert!(jumps[1] > jumps[0], "wer mehr verliert, wird müder");
    assert!(lost.lost_value as f64 / hit.navy[1] >= balance.heavy_defeat_share);

    // Die schwere Niederlage drückt die Kursstimmung aller Emittenten des Verlierers, in
    // der Gegenprobe nicht; die Siegerin und Unbeteiligte trifft sie nie.
    let stricken = |sim: &Headless, id: IssuerId| {
        polity_of_issuer(sim.galaxy(), sim.world().resource::<Logistics>(), id) == Some(enemy)
    };
    assert!(balance.defeat_shock < 1.0);
    let mut struck = 0;
    for (id, before) in &hit.shocks {
        let after = finance(&hit.sim).issuers[id].shock;
        if stricken(&hit.sim, *id) {
            assert!(
                (after - (before * balance.defeat_shock).max(0.2)).abs() < 1e-12,
                "{id:?}"
            );
            assert!(after < *before, "{id:?}");
            struck += 1;
        } else {
            assert_eq!(after, *before, "{id:?}");
        }
        assert_eq!(finance(&calm.sim).issuers[id].shock, *before, "{id:?}");
    }
    assert!(struck >= 3, "der Verlierer hat nur {struck} Emittenten");
    assert!(chronicled(
        &hit.sim,
        target,
        &["Niederlage", "Kurse geben nach"]
    ));

    // Beim nächsten Monatsabschluss stehen die Kurse des Verlierers tiefer als in der
    // Gegenprobe; alle anderen Kurse sind auf den Punkt gleich.
    let Defeat {
        mut sim, before, ..
    } = hit;
    let mut calm = calm.sim;
    let close = (ended / month + 1) * month;
    for sim in [&mut sim, &mut calm] {
        while sim.clock().tick <= close {
            sim.tick();
        }
    }
    let (mut lower, mut value_hit, mut value_calm) = (0, 0.0, 0.0);
    for (id, issuer) in &finance(&sim).issuers {
        let other = &finance(&calm).issuers[id];
        if stricken(&sim, *id) {
            assert!(
                issuer.price <= other.price,
                "{id:?}: {} statt {}",
                issuer.price,
                other.price
            );
            lower += (issuer.price < other.price) as usize;
            value_hit += issuer.market_value();
            value_calm += other.market_value();
        } else {
            assert_eq!(issuer.price, other.price, "{id:?}");
        }
    }
    assert!(lower >= 3, "nur {lower} Kurse des Verlierers liegen tiefer");
    assert!(
        value_hit < 0.97 * value_calm,
        "Börsenwert des Verlierers {value_hit:.0} statt {value_calm:.0}"
    );

    // Die Siegerin kehrt heim. Was sie verschossen und eingebüßt hat, kauft der Staat am
    // Markt nach: Gegenprobe ist dieselbe Partie mit einer Flotte, der nichts fehlt.
    let (_, home) = home_of(&sim, me);
    sim.issue(
        me,
        SimCommand::MoveFleet {
            fleet: ours,
            to: home,
        },
    );
    sail(&mut sim, ours, home);
    set_stock(&mut sim, home, &["munition", "rumpfsegmente"], 300.0);
    let worn = fleet(&sim, ours).clone();
    assert!(worn.supply(data).0 < 0.9, "nach dem Gefecht fehlt Munition");
    assert!(worn.ships.iter().any(|ship| ship.is_damaged(data)));
    let mut whole = Headless::from_save(data.clone(), sim.save());
    {
        let mut fleets = whole.world_mut().resource_mut::<Fleets>();
        for ship in &mut fleets.list[ours.index()].ships {
            ship.hull = 1.0;
            ship.modules.clear();
            ship.ammo = 1.0;
            ship.fuel = 1.0;
        }
        fleets.refresh(data);
    }
    sim.run_days(1);
    whole.run_days(1);
    let paid = paid_for(&sim, me, Claim::Procurement) - paid_for(&whole, me, Claim::Procurement);
    let to_market = economy(&sim).market(home).cash - economy(&whole).market(home).cash;
    assert!(paid > 0, "Nachschub und Instandsetzung kosten Geld");
    assert_eq!(paid, to_market, "die Staatskasse zahlt es dem Kontor");
    let served = fleet(&sim, ours);
    assert!(served.supply(data).0 > worn.supply(data).0);
    assert!(
        served.strength > worn.strength,
        "die Instandsetzung hat begonnen"
    );
    assert_eq!(money_drift(sim.world()), Some(0));

    // Sold: Für verlorene Schiffe zahlt der Staat nicht mehr. Gegenprobe ist dieselbe
    // Partie am Monatsanfang, in der die verlorenen Schiffe noch da sind.
    let fallen: Vec<Ship> = before
        .ships
        .iter()
        .filter(|ship| fleet(&sim, ours).ship(ship.id).is_none())
        .cloned()
        .collect();
    assert!(!fallen.is_empty());
    while !sim.clock().tick.is_multiple_of(month) {
        sim.tick();
    }
    let mut intact = Headless::from_save(data.clone(), sim.save());
    {
        let mut fleets = intact.world_mut().resource_mut::<Fleets>();
        for ship in &fallen {
            fleets.list[ours.index()]
                .ships
                .push(Ship::fresh(data, ship.id, ship.design));
        }
        fleets.refresh(data);
    }
    let due = monthly_upkeep(data, fleets(&sim), me);
    let spared = monthly_upkeep(data, fleets(&intact), me) - due;
    let value: f64 = fallen
        .iter()
        .map(|ship| data.designs[ship.design as usize].stats.value)
        .sum();
    assert!((to_credits(spared) - value * balance.upkeep_per_month).abs() < 0.002);
    assert!(due > 0 && spared > 0);
    sim.tick();
    intact.tick();
    // Gespart ist der Sold der verlorenen Schiffe und was sie an diesem Tag verbraucht
    // hätten: Treibstoff im Dock und Ersatzteile.
    assert_eq!(staff(&intact, me) - staff(&sim, me), spared);
    let bought = economy(&intact).market(home).cash - economy(&sim).market(home).cash;
    assert!(bought > 0 && bought < spared);
    // Was die kleinere Flotte den Haushalt weniger kostet, kann er ausschütten: Die
    // Haushalte bekommen nie weniger als in der Gegenprobe, und die Kasse liegt um das
    // Gesparte höher, soweit es nicht ausgeschüttet wurde.
    let released = budget_month(&sim, me).handout() - budget_month(&intact, me).handout();
    assert!(released >= 0);
    assert_eq!(households(&sim, me) - households(&intact, me), released);
    assert_eq!(
        treasury(&sim, me) - treasury(&intact, me),
        spared + bought - released
    );
    assert_eq!(fleets(&sim).arrears_of(me), 0.0);

    // Frieden: Die Kriegsmüdigkeit klingt Monat für Monat ab.
    assert_eq!(
        command(&mut sim, me, SimCommand::MakePeace { target: enemy }),
        CommandOutcome::Applied
    );
    let weary = weariness(&sim, me);
    assert!(weary > 0.0);
    sim.run_days(3 * DAYS_PER_MONTH);
    let recovery = data.balance.finance.weariness_recovery;
    assert!(recovery > 0.0);
    assert!((weariness(&sim, me) - (weary - 3.0 * recovery).max(0.0)).abs() < 1e-9);
    assert!(weariness(&sim, me) < weary);
    assert_eq!(money_drift(sim.world()), Some(0));
    assert_eq!(check_invariants(sim.world()), Ok(()));
}

// --- 12. Befehle im Gefecht ----------------------------------------------------------------

#[test]
fn orders_reach_a_running_battle_and_a_fleet_can_break_off() {
    let (a, b) = (PolityId(0), PolityId(1));
    let (ours, theirs) = (FleetId(0), FleetId(1));
    let mut sim = with_player(23);
    let (_, home) = home_of(&sim, a);
    let target = home_of(&sim, b).1;
    let rear = sim.galaxy().neighbors(target)[0].system;
    // Eine zweite eigene Flotte bleibt zu Hause: Sie steht in keinem Gefecht.
    let reserve = raise_fleet(&mut sim, a, home, &[("dolch", 2)]);
    // Aus dem Nachbarsystem marschiert die Flotte in das Heimatsystem des Gegners.
    place(&mut sim, ours, rear);
    sim.issue(a, SimCommand::DeclareWar { target: b });
    sim.issue(
        a,
        SimCommand::MoveFleet {
            fleet: ours,
            to: target,
        },
    );
    sail(&mut sim, ours, target);
    assert!(fleet(&sim, ours).in_battle && fleet(&sim, theirs).in_battle);
    let ships = fleet(&sim, ours).ships.len();
    let combatant = |sim: &Headless, id: FleetId| {
        let (battle, side) = battles(sim)
            .of_fleet(id)
            .expect("die Flotte steht im Gefecht");
        battle.combatants[side].clone()
    };
    let march = |to: SystemId| SimCommand::MoveFleet { fleet: ours, to };

    // Marschbefehl: abgelehnt, solange gekämpft wird.
    let reason = rejection(&mut sim, a, march(rear));
    assert!(
        reason.contains("Gefecht") && reason.contains("Rückzug"),
        "{reason}"
    );
    assert!(fleet(&sim, ours).route.is_empty());

    // Aufstellung und Doktrin gelten sofort im Gefecht und bleiben stehende Befehle.
    assert_eq!(
        (
            combatant(&sim, ours).formation,
            combatant(&sim, ours).doctrine
        ),
        (Formation::Line, Doctrine::Balanced)
    );
    let orders = |fleet: FleetId| SimCommand::SetFleetOrders {
        fleet,
        formation: Formation::Wedge,
        doctrine: Doctrine::Aggressive,
    };
    assert_eq!(command(&mut sim, a, orders(ours)), CommandOutcome::Applied);
    assert_eq!(
        (
            combatant(&sim, ours).formation,
            combatant(&sim, ours).doctrine
        ),
        (Formation::Wedge, Doctrine::Aggressive)
    );
    assert_eq!(
        (fleet(&sim, ours).formation, fleet(&sim, ours).doctrine),
        (Formation::Wedge, Doctrine::Aggressive)
    );
    // Die Gegenseite bleibt bei ihren Befehlen; fremde Flotten befehligt niemand.
    assert!(rejection(&mut sim, a, orders(theirs)).contains("anderen Gemeinschaft"));
    assert_eq!(
        (
            combatant(&sim, theirs).formation,
            combatant(&sim, theirs).doctrine
        ),
        (Formation::Line, Doctrine::Balanced)
    );

    // Rückzug: nur für eine eigene Flotte, die im Gefecht steht, und nur einmal.
    let retreat = |fleet: FleetId| SimCommand::Retreat { fleet };
    assert!(rejection(&mut sim, b, retreat(ours)).contains("anderen Gemeinschaft"));
    assert!(rejection(&mut sim, a, retreat(reserve)).contains("in keinem Gefecht"));
    assert!(rejection(&mut sim, a, retreat(FleetId(9_999))).contains("unbekannt"));
    assert!(!combatant(&sim, ours).retreating);
    assert_eq!(command(&mut sim, a, retreat(ours)), CommandOutcome::Applied);
    assert!(combatant(&sim, ours).retreating);
    {
        let (battle, side) = battles(&sim).of_fleet(ours).unwrap();
        let (_, other) = battles(&sim).of_fleet(theirs).unwrap();
        assert!(!battle.is_fighting(side) && battle.is_fighting(other));
    }
    assert!(rejection(&mut sim, a, retreat(ours)).contains("bereits"));
    // Auch auf dem Rückzug gilt: kein Marschbefehl, bis das Feld geräumt ist.
    assert!(rejection(&mut sim, a, march(rear)).contains("Rückzug"));

    let report = fight_out(&mut sim, target);
    let (left, stayed) = (side_of(&report, ours), side_of(&report, theirs));
    assert!(left.retreated && !left.held_field);
    assert!(stayed.held_field && !stayed.retreated);
    // Die Flotte räumt das Feld mit allem, was entkam, …
    let gone = fleet(&sim, ours);
    assert_eq!(
        gone.ships.len() as u32,
        ships as u32 - left.lost.iter().sum::<u32>()
    );
    assert_eq!(left.escaped.iter().sum::<u32>(), gone.ships.len() as u32);
    // … in diesem Szenario, noch vor der ersten Berührung befohlen, mit allen Schiffen.
    assert_eq!(
        gone.ships.len(),
        ships,
        "Szenario neu wählen: Der frühe Rückzug sollte ohne Verluste gelingen"
    );
    assert_eq!(gone.route, vec![rear], "zurück, woher sie kam");
    assert!(gone.is_regrouping() && !gone.in_battle);
    // Die Verteidigerin bleibt und ist einsatzbereit.
    let keeper = fleet(&sim, theirs);
    assert!(keeper.system == target && keeper.route.is_empty() && keeper.is_ready());
    // Der Abzug lässt sich nicht widerrufen: Solange sich die Flotte sammelt, nimmt sie
    // keinen Marschbefehl an, weder den Halt im Gefechtssystem noch ein anderes Ziel.
    let until = gone.regroup_until.unwrap();
    for to in [target, home] {
        let reason = rejection(&mut sim, a, march(to));
        assert!(reason.contains("sammelt sich bis zum"), "{reason}");
        assert_eq!(fleet(&sim, ours).route, vec![rear]);
    }
    sail(&mut sim, ours, rear);
    assert!(sim.clock().day() < until);
    assert!(rejection(&mut sim, a, march(home)).contains("sammelt sich"));
    assert!(fleet(&sim, ours).route.is_empty());
    // Ist die Frist um, marschiert sie wieder, wohin sie soll.
    sim.run_days(until - sim.clock().day());
    assert!(fleet(&sim, ours).is_ready());
    assert_eq!(command(&mut sim, a, march(home)), CommandOutcome::Applied);
    assert_eq!(fleet(&sim, ours).route.last(), Some(&home));
    assert_eq!(battles(&sim).next_id, 1);
    assert_eq!(money_drift(sim.world()), Some(0));
    assert_eq!(check_invariants(sim.world()), Ok(()));
}

#[test]
fn an_unpaid_war_weary_fleet_breaks_before_the_first_shot() {
    let data = data();
    let rules = &data.balance.battle;
    let (a, b) = (PolityId(0), PolityId(1));
    let (ours, theirs) = (FleetId(0), FleetId(1));
    let mut sim = with_player(32);
    let target = home_of(&sim, b).1;
    place(&mut sim, ours, target);
    sim.run_days(1);
    // Die Verteidiger sind des Krieges müde bis zum Äußersten und seit Monaten ohne Sold.
    sim.world_mut().resource_mut::<Finance>().polities[b.index()].war_weariness = 100.0;
    sim.world_mut().resource_mut::<Fleets>().arrears = vec![0.0, 1.0];
    sim.issue(a, SimCommand::DeclareWar { target: b });
    sim.run_days(1);
    {
        let (battle, side) = battles(&sim)
            .of_fleet(theirs)
            .expect("das Gefecht hat begonnen");
        let (_, other) = battles(&sim).of_fleet(ours).unwrap();
        let morale = rules.morale_start
            - rules.morale_per_weariness * 100.0
            - data.balance.military.morale_per_arrears;
        assert_eq!(battle.combatants[side].morale, morale.clamp(0.0, 100.0));
        assert_eq!(battle.combatants[other].morale, rules.morale_start);
        assert!(
            morale < rules.morale_break,
            "mit dieser Moral ({morale}) hält keine Besatzung stand"
        );
        // Schon am ersten Tag kämpft auf dieser Seite niemand mehr.
        assert!(!battle.is_fighting(side) && battle.is_fighting(other));
        assert!(
            !battle.combatants[side].retreating,
            "befohlen hat es niemand"
        );
    }
    let ships = fleet(&sim, theirs).ships.len();
    let report = fight_out(&mut sim, target);
    let (stayed, fled) = (side_of(&report, ours), side_of(&report, theirs));
    assert!(stayed.held_field, "die Angreiferin hält das Feld");
    assert!(!fled.held_field, "die Verteidigerin hat es geräumt");
    assert_eq!(
        stayed.lost, [0; 6],
        "wer nicht kämpft, fügt keinen Schaden zu"
    );
    // Die Flotte räumt das eigene Heimatsystem und sammelt sich nebenan.
    let beaten = fleet(&sim, theirs);
    assert!(beaten.alive && beaten.is_regrouping());
    assert_eq!(
        beaten.ships.len() as u32,
        ships as u32 - fled.lost.iter().sum::<u32>()
    );
    assert_eq!(beaten.route.len(), 1);
    assert!(fleet(&sim, ours).is_ready() && fleet(&sim, ours).system == target);
    assert_eq!(check_invariants(sim.world()), Ok(()));
}

#[test]
fn peace_ends_a_running_battle() {
    let (a, b) = (PolityId(0), PolityId(1));
    let (ours, theirs) = (FleetId(0), FleetId(1));
    let mut sim = with_player(31);
    let target = home_of(&sim, b).1;
    place(&mut sim, ours, target);
    sim.issue(a, SimCommand::DeclareWar { target: b });
    sim.run_days(4);
    assert!(battles(&sim).in_system(target).is_some());
    let before = [fleet(&sim, ours).clone(), fleet(&sim, theirs).clone()];
    let weary = [weariness(&sim, a), weariness(&sim, b)];

    // Frieden mitten im Gefecht: Am nächsten Morgen ist niemand mehr feind, und das
    // Gefecht endet, bevor ein Schuss gefallen ist.
    assert_eq!(
        command(&mut sim, a, SimCommand::MakePeace { target: b }),
        CommandOutcome::Applied
    );
    assert!(battles(&sim).in_system(target).is_some(), "bis zum Morgen");
    sim.run_days(2);
    assert!(battles(&sim).active.is_empty());
    let report = battles(&sim).reports.last().unwrap().clone();
    assert_eq!(report.outcome, BattleOutcome::CalledOff);
    assert!(report.days() <= 6);
    for old in &before {
        let side = side_of(&report, old.id);
        assert!(side.held_field && !side.retreated);
        assert_eq!(side.lost, [0; 6]);
        let now = fleet(&sim, old.id);
        assert!(now.is_ready() && now.system == target && now.route.is_empty());
        assert_eq!(now.ships.len(), old.ships.len());
    }
    assert!(chronicled(
        &sim,
        target,
        &["eingestellt", "kein Krieg mehr"]
    ));
    assert_eq!(battles(&sim).debris_in(target), 0.0);
    // Ohne Verluste keine zusätzliche Kriegsmüdigkeit, und im Frieden beginnt nichts Neues.
    assert!(weariness(&sim, a) <= weary[0] && weariness(&sim, b) <= weary[1]);
    sim.run_days(5);
    assert_eq!(battles(&sim).next_id, 1);
    assert_eq!(check_invariants(sim.world()), Ok(()));
}

// --- 13. Determinismus im Krieg ------------------------------------------------------------

/// Ein Krieg nach Drehbuch: ein Gefecht mit Trägern im Heimatsystem des Gegners, neue
/// Befehle mitten im Gefecht, ein Dritter, der später eingreift, Heimkehr und Frieden. Die
/// Angreiferin ist technisch voraus und lässt ihre Werft den besten Kreuzer bauen, den sie
/// kennt. `run` lässt die Zeit laufen. Gibt die Prüfsumme mitten im ersten Gefecht zurück.
fn scripted_war(sim: &mut Headless, run: fn(&mut Headless, u64)) -> u64 {
    let (a, b, c) = (PolityId(0), PolityId(1), PolityId(2));
    let (capital, home) = home_of(sim, a);
    let target = home_of(sim, b).1;
    for id in [FleetId(0), FleetId(1)] {
        add_ships(sim, id, &[("horst", 1)]);
    }
    grant(sim, a, &["waffen_2", "verteidigung_2", "antrieb_2"]);
    place(sim, FleetId(0), target);
    sim.issue(
        a,
        SimCommand::BuildClass {
            planet: capital,
            class: ShipClass::Cruiser,
        },
    );
    sim.issue(a, SimCommand::DeclareWar { target: b });
    sim.issue(
        a,
        SimCommand::SetStance {
            fleet: FleetId(0),
            stance: Stance::Blockade,
        },
    );
    // Ein Dritter zieht mit in den Krieg; seine Flotte hat den längeren Weg.
    sim.issue(c, SimCommand::DeclareWar { target: b });
    sim.issue(
        c,
        SimCommand::MoveFleet {
            fleet: FleetId(2),
            to: target,
        },
    );
    run(sim, 9);
    assert!(
        battles(sim).in_system(target).is_some(),
        "das Gefecht läuft"
    );
    sim.issue(
        a,
        SimCommand::SetFleetOrders {
            fleet: FleetId(0),
            formation: Formation::Wedge,
            doctrine: Doctrine::Aggressive,
        },
    );
    run(sim, 6);
    let battle = battles(sim)
        .in_system(target)
        .expect("das Gefecht läuft noch");
    assert!(
        battle.ships.iter().any(|ship| ship.carrier.is_some()),
        "die Bordjäger sind gestartet"
    );
    let mid = sim.hash();
    run(sim, 160);
    sim.issue(
        a,
        SimCommand::MoveFleet {
            fleet: FleetId(0),
            to: home,
        },
    );
    sim.issue(c, SimCommand::MakePeace { target: b });
    run(sim, 100);
    sim.issue(a, SimCommand::MakePeace { target: b });
    run(sim, 60);
    mid
}

#[test]
fn a_scripted_war_is_deterministic() {
    let mut first = with_player(27);
    let mut second = with_player(27);
    let mid_first = scripted_war(&mut first, skipping);
    let mid_second = scripted_war(&mut second, skipping);
    assert_eq!(mid_first, mid_second);
    assert_eq!(first.hash(), second.hash());

    // Jeden Takt zu rechnen ergibt dasselbe wie das Überspringen der Leertakte: mitten im
    // Gefecht und am Ende.
    let mut exact = with_player(27);
    let mid_exact = scripted_war(&mut exact, every_tick);
    assert_eq!(mid_exact, mid_first, "mitten im Gefecht");
    assert_eq!(exact.clock(), first.clock());
    assert_eq!(exact.hash(), first.hash());
    assert_eq!(battles(&exact).reports, battles(&first).reports);

    // Der Krieg hat stattgefunden, und ohne ihn verliefe die Partie anders.
    assert!(!battles(&first).reports.is_empty());
    assert!(battles(&first).total_ships_lost() > 0);
    assert!(battles(&first).active.is_empty());
    let log = first.command_log();
    assert!(log.entries.iter().any(|entry| matches!(
        entry.command,
        SimCommand::SetFleetOrders { .. }
    ) && entry.outcome == CommandOutcome::Applied));
    // Auch die Forschung war im Spiel: Die Werft hat den Entwurf gewählt, den erst die
    // Technik erlaubt, und die Gegner kennen einander seit dem Gefecht besser als zuvor.
    let (a, b) = (PolityId(0), PolityId(1));
    let ordered = log
        .entries
        .iter()
        .find(|entry| matches!(entry.command, SimCommand::BuildClass { .. }))
        .expect("der Bauauftrag nach Klasse");
    assert_eq!(ordered.outcome, CommandOutcome::Applied);
    let improved = design("speer_2");
    assert!(
        fleets(&first)
            .orders
            .iter()
            .any(|order| order.design == improved)
            || fleets(&first)
                .of(a)
                .any(|fleet| fleet.ships.iter().any(|ship| ship.design == improved))
    );
    assert!(research(&first).intel(a, b) > data().balance.research.intel_base);
    assert_eq!(
        research(&first).intel(a, b),
        research(&exact).intel(a, b),
        "auch der Einblick ist in beiden Läufen derselbe"
    );
    let mut passive = with_player(27);
    passive.run_days(first.clock().day());
    assert_ne!(passive.hash(), first.hash());
    assert_eq!(money_drift(first.world()), Some(0));
    assert_eq!(check_invariants(first.world()), Ok(()));
}

#[test]
fn a_save_in_the_middle_of_a_battle_continues_identically() {
    let data = data();
    let (a, b) = (PolityId(0), PolityId(1));
    let prepare = || {
        let mut sim = with_player(29);
        let target = home_of(&sim, b).1;
        for id in [FleetId(0), FleetId(1)] {
            add_ships(&mut sim, id, &[("horst", 1)]);
        }
        // Die Angreiferin kämpft mit den Aufschlägen ihrer Technik: Auch sie stehen im
        // Spielstand.
        grant(&mut sim, a, &TIER_TWO);
        place(&mut sim, FleetId(0), target);
        sim.issue(a, SimCommand::DeclareWar { target: b });
        sim
    };
    // Die Vergleichspartie läuft ohne Unterbrechung bis mitten in den Tag 214.
    let mut straight = prepare();
    straight.run_days(214);
    for _ in 0..7 {
        straight.tick();
    }

    // Gespeichert wird mitten am Tag, mitten im Gefecht: Jäger sind gestartet, Geschosse
    // in der Luft, Schiffe beschädigt.
    let mut first_half = prepare();
    first_half.run_days(14);
    for _ in 0..7 {
        first_half.tick();
    }
    {
        let battle = &battles(&first_half).active[0];
        assert!(battle.time > 25.0 && !battle.finished);
        assert!(battle.ships.iter().any(|ship| ship.carrier.is_some()));
        assert!(!battle.shots.is_empty(), "Geschosse sind in der Luft");
        assert!(battle.ships.iter().any(|ship| ship.hull < ship.hull_max));
    }
    let text = first_half.save().to_ron().unwrap();
    let save = SaveGame::from_ron(&text).unwrap();
    assert!(save.matches_data(data));
    let mut resumed = Headless::from_save(data.clone(), save);
    assert_eq!(
        resumed.hash(),
        first_half.hash(),
        "Laden stellt den Zustand exakt her"
    );
    assert_eq!(battles(&resumed).active.len(), 1);
    assert!(fleet(&resumed, FleetId(0)).in_battle);
    {
        let (battle, side) = battles(&resumed).of_fleet(FleetId(0)).unwrap();
        assert_eq!(
            battle.combatants[side].modifiers,
            combat_modifiers(research(&resumed), a)
        );
        assert!(battle.combatants[side].modifiers.damage > 1.0);
    }
    resumed.run_days(200);
    assert_eq!(resumed.clock(), straight.clock());
    assert_eq!(resumed.hash(), straight.hash());
    // In der Zwischenzeit ist das Gefecht zu Ende gegangen, in beiden Partien gleich.
    assert!(battles(&resumed).active.is_empty());
    assert_eq!(battles(&resumed).reports.len(), 1);
    assert_eq!(battles(&resumed).reports, battles(&straight).reports);
    assert_eq!(money_drift(resumed.world()), Some(0));
    assert_eq!(check_invariants(resumed.world()), Ok(()));

    // Spielstände aus der Zeit der groben Gefechtsregel werden am Format erkannt.
    let mut old = first_half.save();
    old.format = 2;
    let error = SaveGame::from_ron(&old.to_ron().unwrap()).unwrap_err();
    assert!(error.to_string().contains("Format 2"), "{error}");
}

// --- 14. Blockade, Sammelfrist, Gefechte ohne Entscheidung -----------------------------------

#[test]
fn a_fleet_in_battle_or_regrouping_does_not_blockade() {
    let (a, b, c) = (PolityId(0), PolityId(1), PolityId(2));
    let (ours, theirs, third) = (FleetId(0), FleetId(1), FleetId(2));
    let mut sim = with_player(60);
    sim.run_days(3 * DAYS_PER_YEAR);
    let home = home_of(&sim, b).1;
    // Ein Koloniesystem der Verteidiger neben ihrem Heimatsystem, ohne Flotte.
    let colony = sim
        .galaxy()
        .neighbors(home)
        .iter()
        .map(|neighbor| neighbor.system)
        .find(|system| sim.galaxy().system_owner(*system) == Some(b))
        .expect("Szenario neu wählen: nach drei Jahren liegt eine Kolonie neben dem Heimatsystem");
    assert!(fleets(&sim).docked_in(colony).next().is_none());
    let blockaded = |sim: &Headless, system: SystemId| fleets(sim).is_blockaded(system);

    // Noch im Frieden marschiert die Flotte von der Kolonie ins Heimatsystem: Von dort
    // kommt sie also, und dorthin wird sie ausweichen.
    place(&mut sim, ours, colony);
    sim.issue(
        a,
        SimCommand::SetStance {
            fleet: ours,
            stance: Stance::Blockade,
        },
    );
    sim.issue(
        a,
        SimCommand::MoveFleet {
            fleet: ours,
            to: home,
        },
    );
    sail(&mut sim, ours, home);
    assert_eq!(fleet(&sim, ours).came_from, Some(colony));
    assert_eq!(battles(&sim).next_id, 0, "im Frieden kein Gefecht");

    // Krieg: Das Gefecht beginnt, die Flotte bricht es sofort ab.
    sim.issue(a, SimCommand::DeclareWar { target: b });
    sim.run_days(1);
    assert!(fleet(&sim, ours).in_battle && fleet(&sim, theirs).in_battle);
    assert!(!blockaded(&sim, home) && !blockaded(&sim, colony));
    assert_eq!(
        command(&mut sim, a, SimCommand::Retreat { fleet: ours }),
        CommandOutcome::Applied
    );
    let report = fight_out(&mut sim, home);
    assert!(side_of(&report, theirs).held_field && !side_of(&report, ours).held_field);
    assert!(fleet(&sim, ours).alive, "der frühe Rückzug gelingt");
    assert_eq!(fleet(&sim, ours).route, vec![colony]);
    let until = fleet(&sim, ours).regroup_until.unwrap();

    // Die Flotte liegt nun im Blockadeauftrag in einem unverteidigten System des Gegners.
    // Solange sie sich sammelt, sperrt sie es nicht.
    sail(&mut sim, ours, colony);
    assert!(
        sim.clock().day() + 5 < until,
        "Szenario neu wählen: Von der Sammelfrist sollten bei der Ankunft Tage übrig sein"
    );
    let mut days = 0;
    while fleet(&sim, ours).is_regrouping() {
        let waiting = fleet(&sim, ours);
        assert!(waiting.is_docked() && waiting.system == colony);
        assert_eq!(waiting.stance, Stance::Blockade);
        assert!(fleets(&sim).docked_in(colony).all(|fleet| fleet.owner == a));
        assert!(!blockaded(&sim, colony), "wer sich sammelt, sperrt nicht");
        assert!(sim.clock().day() <= until);
        sim.run_days(1);
        days += 1;
    }
    assert!(days >= 5);
    // Mit dem Ende der Sammelfrist steht die Sperre.
    assert!(sim.clock().day() >= until);
    assert!(fleet(&sim, ours).is_ready());
    assert!(
        blockaded(&sim, colony),
        "nach der Sammelfrist steht die Sperre"
    );
    assert!(!blockaded(&sim, home));

    // Ein Dritter, der nur mit den Blockierern im Krieg liegt, greift sie in der Kolonie
    // an. Das Gefecht ändert an der Sperre nichts, bevor es entschieden ist: Sie hält,
    // solange die Blockadeflotte auf dem Feld kämpft.
    place(&mut sim, third, colony);
    sim.issue(c, SimCommand::DeclareWar { target: a });
    sim.run_days(1);
    assert!(fleet(&sim, ours).in_battle && fleet(&sim, third).in_battle);
    assert!(
        blockaded(&sim, colony),
        "der Angriff allein hebt die Sperre nicht auf"
    );
    let (mut battle_days, mut closed_days) = (0, 0);
    while battles(&sim).in_system(colony).is_some() {
        if blockaded(&sim, colony) {
            closed_days += 1;
        } else {
            assert!(
                !battles(&sim).is_fighting(ours),
                "offen ist das System erst, wenn die Blockadeflotte nicht mehr kämpft"
            );
        }
        assert!(fleets(&sim).docked_in(colony).all(|fleet| fleet.owner != b));
        sim.run_days(1);
        battle_days += 1;
        assert!(battle_days < longest_battle_days(data()));
    }
    assert!(battle_days >= 5 && closed_days >= 5);
    // Danach sperrt die Flotte wieder, wenn sie das Feld behauptet hat, sonst nicht.
    let report = battles(&sim).reports.last().unwrap().clone();
    sim.run_days(1);
    let holds = side_of(&report, ours).held_field && fleet(&sim, ours).alive;
    assert_eq!(blockaded(&sim, colony), holds);
    assert_eq!(money_drift(sim.world()), Some(0));
    assert_eq!(check_invariants(sim.world()), Ok(()));
}

#[test]
fn a_battle_without_decision_at_the_time_limit_sends_the_intruder_away() {
    // Mit einer Zeitgrenze von zwölf Sekunden kommen die Flotten einander nicht nahe.
    let mut short = data().clone();
    short.balance.battle.time_limit = 12.0;
    let regroup = short.balance.military.regroup_days;
    let (a, b) = (PolityId(0), PolityId(1));
    let (ours, theirs) = (FleetId(0), FleetId(1));
    let mut sim = Headless::new(
        short,
        SimConfig {
            seed: 23,
            player: Some(a),
        },
    );
    let target = home_of(&sim, b).1;
    let rear = sim.galaxy().neighbors(target)[0].system;
    // Aus dem Nachbarsystem marschiert die Flotte in das Heimatsystem des Gegners.
    place(&mut sim, ours, rear);
    sim.issue(a, SimCommand::DeclareWar { target: b });
    sim.issue(
        a,
        SimCommand::MoveFleet {
            fleet: ours,
            to: target,
        },
    );
    sail(&mut sim, ours, target);
    assert!(fleet(&sim, ours).in_battle && fleet(&sim, theirs).in_battle);
    let before = [fleet(&sim, ours).clone(), fleet(&sim, theirs).clone()];
    let weary = [weariness(&sim, a), weariness(&sim, b)];
    let report = fight_out(&mut sim, target);
    assert_eq!(report.outcome, BattleOutcome::TimedOut);
    assert!(report.outcome.undecided());
    assert_eq!(
        report.days(),
        6,
        "zwölf Sekunden Gefechtszeit sind sechs Tage"
    );
    // Niemand hat etwas verloren; beide standen bis zuletzt auf dem Feld.
    for old in &before {
        let side = side_of(&report, old.id);
        assert!(side.held_field && !side.retreated && !side.stalemate);
        assert_eq!(side.lost, [0; 6]);
        let now = fleet(&sim, old.id);
        assert!(now.alive && !now.in_battle);
        assert_eq!(now.ships.len(), old.ships.len());
        assert!(now.ships.iter().all(|ship| ship.hull == 1.0));
    }
    let place_name = sim.galaxy().system(target).name.clone();
    assert!(chronicled(
        &sim,
        target,
        &[&place_name, "ohne Entscheidung", "weicht nach"]
    ));
    assert_eq!(battles(&sim).debris_in(target), 0.0);
    // Ohne Verluste macht ein Gefecht niemanden kriegsmüder.
    assert_eq!([weariness(&sim, a), weariness(&sim, b)], weary);

    // Ohne Entscheidung bleibt nur, wer zu Hause ist: Die Verteidigerin liegt weiter im
    // System und ist einsatzbereit, die Eingedrungene weicht aus, woher sie kam, und
    // sammelt sich.
    let home = fleet(&sim, theirs);
    assert!(home.system == target && home.route.is_empty() && home.is_ready());
    let intruder = fleet(&sim, ours);
    assert_eq!(intruder.route, vec![rear]);
    assert_eq!(intruder.regroup_until, Some(end_day(&report) + regroup));
    assert!(intruder.is_regrouping() && !intruder.is_ready());

    // Dasselbe Gefecht beginnt nicht von vorn: weder solange beide noch im System liegen
    // noch später.
    sail(&mut sim, ours, rear);
    sim.run_days(2 * regroup);
    assert_eq!(battles(&sim).next_id, 1);
    assert!(battles(&sim).active.is_empty());
    assert!(fleet(&sim, ours).is_ready() && fleet(&sim, ours).system == rear);
    assert_eq!(money_drift(sim.world()), Some(0));
    assert_eq!(check_invariants(sim.world()), Ok(()));
}

/// Ersetzt die Schiffe einer Flotte (Aufbau eines Szenarios).
fn rebuild_fleet(sim: &mut Headless, id: FleetId, ships: &[(&str, u32)]) {
    {
        let mut fleets = sim.world_mut().resource_mut::<Fleets>();
        fleets.list[id.index()].ships.clear();
    }
    add_ships(sim, id, ships);
}

/// Zwei Flotten aus den genannten Schiffen treffen im Heimatsystem der Verteidiger
/// aufeinander; die Angreiferin kommt aus dem Nachbarsystem. Gibt Partie, Heimatsystem,
/// Nachbarsystem und die Bilanz des Gefechts zurück.
fn standoff(seed: u64, ships: &[(&str, u32)]) -> (Headless, SystemId, SystemId, BattleReport) {
    let (a, b) = (PolityId(0), PolityId(1));
    let (ours, theirs) = (FleetId(0), FleetId(1));
    let mut sim = with_player(seed);
    let target = home_of(&sim, b).1;
    let rear = sim.galaxy().neighbors(target)[0].system;
    for id in [ours, theirs] {
        rebuild_fleet(&mut sim, id, ships);
    }
    place(&mut sim, ours, rear);
    sim.issue(a, SimCommand::DeclareWar { target: b });
    sim.issue(
        a,
        SimCommand::MoveFleet {
            fleet: ours,
            to: target,
        },
    );
    sail(&mut sim, ours, target);
    assert!(fleet(&sim, ours).in_battle && fleet(&sim, theirs).in_battle);
    let report = fight_out(&mut sim, target);
    (sim, target, rear, report)
}

#[test]
fn a_stalemate_has_no_winner_and_does_not_start_over() {
    let data = data();
    let (ours, theirs) = (FleetId(0), FleetId(1));
    let regroup = data.balance.military.regroup_days;
    // Einzelne Korvetten kommen nicht durch die Schilde der anderen; Träger mit wenig
    // Geleit zermürben einander, ohne dass eine Seite die andere vom Feld bringt. Früher
    // liefen solche Gefechte an die Zeitgrenze und begannen nach einer Pause von vorn.
    let pairings: [&[(&str, u32)]; 2] = [&[("dolch", 1)], &[("horst", 2), ("dolch", 2)]];
    let mut stalemates = 0;
    for (index, ships) in pairings.into_iter().enumerate() {
        for seed in [61, 62, 63] {
            let (mut sim, target, rear, report) = standoff(seed, ships);
            assert_ne!(
                report.outcome,
                BattleOutcome::TimedOut,
                "Paarung {index}, Seed {seed}: kein Gefecht läuft mehr an die Zeitgrenze"
            );
            assert!(
                report.days() < longest_battle_days(data) / 2,
                "Paarung {index}, Seed {seed}: {} Tage",
                report.days()
            );
            if report.outcome == BattleOutcome::Stalemate {
                stalemates += 1;
                // Niemand hält das Feld, niemand hat gewonnen: Alle haben sich gelöst.
                for side in &report.sides {
                    assert!(side.stalemate && side.retreated && !side.held_field);
                }
                assert!(chronicled(&sim, target, &["im Patt"]));
                // Wer zu Hause ist, bleibt und ist einsatzbereit; wer eingedrungen ist,
                // weicht aus, woher er kam, und sammelt sich.
                let home = fleet(&sim, theirs);
                if home.alive {
                    assert!(home.system == target && home.route.is_empty() && home.is_ready());
                }
                let intruder = fleet(&sim, ours);
                if intruder.alive {
                    assert_eq!(intruder.route, vec![rear]);
                    assert_eq!(intruder.regroup_until, Some(end_day(&report) + regroup));
                }
                // Ein Patt ist keine Niederlage: Die Kurse bleiben, wo sie sind.
                assert!(!chronicled(&sim, target, &["Niederlage"]));
            }
            assert_aftermath(&sim, &report);
            // Was auch immer herauskam: Dasselbe Gefecht beginnt nicht von vorn.
            sim.run_days(3 * regroup);
            assert_eq!(
                battles(&sim).next_id,
                1,
                "Paarung {index}, Seed {seed}: es blieb bei einem Gefecht"
            );
            assert!(battles(&sim).active.is_empty());
            assert!(!fleet(&sim, ours).in_battle && !fleet(&sim, theirs).in_battle);
            assert_eq!(money_drift(sim.world()), Some(0));
            assert_eq!(check_invariants(sim.world()), Ok(()));
        }
    }
    assert!(
        stalemates >= 3,
        "Szenario neu wählen: nur {stalemates} von 6 Gefechten endeten im Patt"
    );
}

#[test]
fn opponents_who_disengage_abroad_do_not_meet_again_next_door() {
    let (a, b) = (PolityId(0), PolityId(1));
    let (ours, theirs) = (FleetId(0), FleetId(1));
    let regroup = data().balance.military.regroup_days;
    let mut sim = with_player(61);
    // Zwei einzelne Korvetten marschieren im Frieden aus demselben System in ein freies
    // Nachbarsystem: Beide kamen also von dort, und beide wichen ohne Weiteres dorthin
    // zurück.
    let (field, rear) = free_neighbors(&sim);
    for id in [ours, theirs] {
        rebuild_fleet(&mut sim, id, &[("dolch", 1)]);
        place(&mut sim, id, rear);
    }
    for (polity, fleet) in [(a, ours), (b, theirs)] {
        sim.issue(polity, SimCommand::MoveFleet { fleet, to: field });
    }
    sail(&mut sim, ours, field);
    sail(&mut sim, theirs, field);
    assert_eq!(fleet(&sim, ours).came_from, Some(rear));
    assert_eq!(fleet(&sim, theirs).came_from, Some(rear));
    assert_eq!(battles(&sim).next_id, 0);

    // Krieg: Die beiden können einander nichts anhaben, das Gefecht endet im Patt.
    sim.issue(a, SimCommand::DeclareWar { target: b });
    sim.run_days(1);
    let report = fight_out(&mut sim, field);
    assert_eq!(
        report.outcome,
        BattleOutcome::Stalemate,
        "Szenario neu wählen: Zwei einzelne Korvetten sollten im Patt enden"
    );
    assert_aftermath(&sim, &report);
    // Niemand ist hier zu Hause, beide weichen aus, aber nicht in dasselbe System: Die
    // erste geht zurück, woher sie kam, die zweite nimmt einen anderen Weg oder bleibt.
    assert_eq!(fleet(&sim, ours).route, vec![rear]);
    assert_ne!(fleet(&sim, theirs).route, vec![rear]);
    assert!(fleet(&sim, ours).is_regrouping() && fleet(&sim, theirs).is_regrouping());

    // Auch nach der Sammelfrist stehen sie einander nicht wieder gegenüber: Es bleibt bei
    // dem einen Gefecht.
    sim.run_days(4 * regroup);
    assert_eq!(battles(&sim).next_id, 1);
    assert!(battles(&sim).active.is_empty());
    let (first, second) = (fleet(&sim, ours), fleet(&sim, theirs));
    assert!(first.is_ready() && second.is_ready());
    assert_ne!(first.system, second.system);
    assert_eq!(
        first.ships.len() + second.ships.len(),
        2,
        "niemand hat etwas verloren"
    );
    assert_eq!(money_drift(sim.world()), Some(0));
    assert_eq!(check_invariants(sim.world()), Ok(()));
}

// --- 14a. Abzug, Sammelfrist und Sperre -------------------------------------------------------

/// Schickt eine Flotte auf den Marsch und lässt die Partie laufen, bis sie angekommen ist.
fn march(sim: &mut Headless, id: FleetId, to: SystemId) {
    let owner = fleet(sim, id).owner;
    sim.issue(owner, SimCommand::MoveFleet { fleet: id, to });
    sail(sim, id, to);
}

/// Das letzte System auf dem Weg einer Flotte in ein Ziel, bevor sie es erreicht.
fn last_stop(sim: &Headless, id: FleetId, to: SystemId) -> SystemId {
    let path = sim
        .galaxy()
        .shortest_path(fleet(sim, id).system, to, |_| true)
        .expect("ein Weg zum Ziel");
    path[path.len() - 2]
}

/// Befiehlt am ersten Morgen des Gefechts den Rückzug und rechnet Takt für Takt bis zu
/// dem Takt, in dem das Gefecht endet: Die Partie steht danach mitten im Tag, und noch ist
/// niemand abmarschiert.
fn break_off(sim: &mut Headless, id: FleetId, system: SystemId) -> BattleReport {
    let owner = fleet(sim, id).owner;
    assert!(fleet(sim, id).in_battle, "die Flotte steht im Gefecht");
    assert_eq!(
        command(sim, owner, SimCommand::Retreat { fleet: id }),
        CommandOutcome::Applied
    );
    let battle = battles(sim).in_system(system).unwrap().id;
    let limit = sim.clock().tick + longest_battle_days(data()) * TICKS_PER_DAY;
    while battles(sim).get(battle).is_some() {
        sim.tick();
        assert!(sim.clock().tick < limit, "das Gefecht endet nie");
    }
    battles(sim).report(battle).unwrap().clone()
}

#[test]
fn a_withdrawal_cannot_be_called_off_and_the_regroup_period_stays_with_the_fleet() {
    let (a, b) = (PolityId(0), PolityId(1));
    let (ours, theirs) = (FleetId(0), FleetId(1));
    let regroup = data().balance.military.regroup_days;
    let mut sim = with_player(23);
    let target = home_of(&sim, b).1;
    // Die Flotte marschiert im Frieden bis vor das Heimatsystem des Gegners. Dort bleiben
    // zwei Korvetten als Rückhalt zurück; sie stehen nie im Gefecht.
    let rear = last_stop(&sim, ours, target);
    march(&mut sim, ours, rear);
    let escorts: Vec<ShipId> = fleet(&sim, ours)
        .ships
        .iter()
        .rev()
        .take(2)
        .map(|ship| ship.id)
        .collect();
    assert_eq!(
        command(
            &mut sim,
            a,
            SimCommand::SplitFleet {
                fleet: ours,
                ships: escorts
            }
        ),
        CommandOutcome::Applied
    );
    let reserve = fleets(&sim).of(a).last().unwrap().id;
    assert!(reserve != ours && fleet(&sim, reserve).system == rear);
    let ships = fleet(&sim, ours).ships.len();

    // Krieg, Marsch ins Ziel, Gefecht, sofortiger Rückzug: Die Flotte räumt das Feld.
    sim.issue(a, SimCommand::DeclareWar { target: b });
    march(&mut sim, ours, target);
    let report = break_off(&mut sim, ours, target);
    assert!(side_of(&report, theirs).held_field && !side_of(&report, ours).held_field);
    assert_eq!(
        fleet(&sim, ours).ships.len(),
        ships,
        "Szenario neu wählen: Der frühe Rückzug sollte ohne Verluste gelingen"
    );
    let until = end_day(&report) + regroup;
    assert_eq!(fleet(&sim, ours).route, vec![rear]);
    assert_eq!(fleet(&sim, ours).regroup_until, Some(until));
    assert!(fleet(&sim, ours).system == target && fleet(&sim, ours).is_docked());

    // Noch liegt sie im Gefechtssystem. Kein Befehl hält sie dort: weder der Halt im
    // System noch ein anderes Ziel, …
    let order = |to: SystemId| SimCommand::MoveFleet { fleet: ours, to };
    for to in [target, home_of(&sim, a).1] {
        let reason = rejection(&mut sim, a, order(to));
        assert!(reason.contains("sammelt sich bis zum"), "{reason}");
        assert_eq!(fleet(&sim, ours).route, vec![rear]);
    }
    // … und wer einen Teil ablöst, lässt ihn nicht zurück: Auch der Teil zieht ab und
    // sammelt sich.
    let one = vec![fleet(&sim, ours).ships[0].id];
    assert_eq!(
        command(
            &mut sim,
            a,
            SimCommand::SplitFleet {
                fleet: ours,
                ships: one
            }
        ),
        CommandOutcome::Applied
    );
    let part = fleets(&sim).of(a).last().unwrap().id;
    assert_eq!(fleet(&sim, part).route, vec![rear]);
    assert_eq!(fleet(&sim, part).regroup_until, Some(until));
    assert!(
        rejection(
            &mut sim,
            a,
            SimCommand::MoveFleet {
                fleet: part,
                to: target
            }
        )
        .contains("sammelt sich")
    );
    sim.run_days(1);
    for id in [ours, part] {
        assert!(
            !fleets(&sim).docked_in(target).any(|fleet| fleet.id == id),
            "am nächsten Morgen ist die Flotte unterwegs"
        );
    }
    sail(&mut sim, ours, rear);
    sail(&mut sim, part, rear);
    assert!(sim.clock().day() < until, "die Frist läuft noch");

    // Im Rückhalt liegen nun drei eigene Flotten: eine einsatzbereite und zwei, die sich
    // sammeln. Die Frist wandert weder auf die Bereite noch streift man sie bei ihr ab.
    for (from, into) in [(ours, reserve), (reserve, ours)] {
        let reason = rejection(&mut sim, a, SimCommand::MergeFleets { from, into });
        assert!(reason.contains("sammelt sich noch"), "{reason}");
    }
    assert!(fleet(&sim, reserve).is_ready() && fleet(&sim, reserve).ships.len() == 2);
    // Zwei, die sich sammeln, lassen sich vereinigen und sammeln sich weiter.
    assert_eq!(
        command(
            &mut sim,
            a,
            SimCommand::MergeFleets {
                from: part,
                into: ours
            }
        ),
        CommandOutcome::Applied
    );
    assert_eq!(fleet(&sim, ours).ships.len(), ships);
    assert_eq!(fleet(&sim, ours).regroup_until, Some(until));
    // Bis zum Ende der Frist bleibt die Flotte, wo sie ist, und nichts beginnt von vorn.
    while sim.clock().day() < until {
        assert!(rejection(&mut sim, a, order(target)).contains("sammelt sich"));
        assert!(fleet(&sim, ours).system == rear && fleet(&sim, ours).route.is_empty());
        sim.run_days(1);
    }
    assert_eq!(battles(&sim).next_id, 1);
    // Danach ist sie eine Flotte wie jede andere.
    assert!(fleet(&sim, ours).is_ready());
    assert_eq!(
        command(
            &mut sim,
            a,
            SimCommand::MergeFleets {
                from: reserve,
                into: ours
            }
        ),
        CommandOutcome::Applied
    );
    assert_eq!(command(&mut sim, a, order(target)), CommandOutcome::Applied);
    assert_eq!(fleet(&sim, ours).route, vec![target]);
    assert_eq!(money_drift(sim.world()), Some(0));
    assert_eq!(check_invariants(sim.world()), Ok(()));
}

#[test]
fn turning_up_does_not_lift_a_blockade_but_winning_does() {
    let (a, b) = (PolityId(0), PolityId(1));
    let (ours, theirs) = (FleetId(0), FleetId(1));
    let mut sim = with_player(60);
    let home = home_of(&sim, b).1;
    let blockaded = |sim: &Headless| fleets(sim).is_blockaded(home);
    // Im Frieden zieht die Heimatflotte in ein Nachbarsystem, das nicht auf dem Weg der
    // Angreifer liegt; die marschieren im Blockadeauftrag in das Heimatsystem.
    let approach = last_stop(&sim, ours, home);
    let away = sim
        .galaxy()
        .neighbors(home)
        .iter()
        .map(|neighbor| neighbor.system)
        .find(|system| *system != approach)
        .expect("Szenario neu wählen: Das Heimatsystem braucht einen zweiten Nachbarn");
    march(&mut sim, theirs, away);
    sim.issue(
        a,
        SimCommand::SetStance {
            fleet: ours,
            stance: Stance::Blockade,
        },
    );
    march(&mut sim, ours, home);
    assert!(!blockaded(&sim), "im Frieden keine Sperre");
    sim.issue(a, SimCommand::DeclareWar { target: b });
    sim.run_days(2);
    assert!(
        blockaded(&sim),
        "ohne Verteidiger im Orbit steht die Sperre"
    );
    assert_eq!(battles(&sim).next_id, 0);

    // Die Heimatflotte kehrt zurück. Ihre Ankunft öffnet das System nicht, und auch nicht,
    // dass sie am ersten Morgen wieder abdreht: Die Sperre hält an jedem Tag.
    sim.issue(
        b,
        SimCommand::MoveFleet {
            fleet: theirs,
            to: home,
        },
    );
    while fleet(&sim, theirs).system != home {
        sim.run_days(1);
        assert!(blockaded(&sim));
    }
    assert!(fleet(&sim, ours).in_battle && fleet(&sim, theirs).in_battle);
    assert!(
        blockaded(&sim),
        "wer ankommt, hat die Sperre noch nicht gebrochen"
    );
    assert_eq!(
        command(&mut sim, b, SimCommand::Retreat { fleet: theirs }),
        CommandOutcome::Applied
    );
    let id = battles(&sim).in_system(home).unwrap().id;
    let mut days = 0;
    while battles(&sim).get(id).is_some() {
        sim.run_days(1);
        days += 1;
        assert!(blockaded(&sim), "Tag {days} des Gefechts");
        assert!(days < longest_battle_days(data()));
    }
    let report = battles(&sim).report(id).unwrap().clone();
    assert!(days >= 5, "das Abdrehen dauert Tage: {days}");
    assert!(side_of(&report, ours).held_field && !side_of(&report, theirs).held_field);
    assert!(fleet(&sim, theirs).alive, "der frühe Rückzug gelingt");
    // Die Heimatflotte weicht aus und sammelt sich; vor Ablauf der Frist kommt sie nicht
    // zurück.
    let until = fleet(&sim, theirs).regroup_until.expect("sie sammelt sich");
    let back = |fleet: FleetId| SimCommand::MoveFleet { fleet, to: home };
    while sim.clock().day() < until {
        assert!(rejection(&mut sim, b, back(theirs)).contains("sammelt sich"));
        sim.run_days(1);
        assert!(blockaded(&sim));
    }

    // Nach der Frist greift sie an und kämpft es aus. Erst der Ausgang entscheidet über
    // die Sperre: Sie fällt, wenn die Blockadeflotte das Feld räumt, und hält sonst.
    assert_eq!(command(&mut sim, b, back(theirs)), CommandOutcome::Applied);
    while battles(&sim).in_system(home).is_none() {
        sim.run_days(1);
        assert!(blockaded(&sim));
        assert!(sim.clock().day() < until + DAYS_PER_YEAR);
    }
    assert_eq!(battles(&sim).next_id, 2);
    let report = fight_out(&mut sim, home);
    sim.run_days(1);
    let holds = fleet(&sim, ours).alive && side_of(&report, ours).held_field;
    assert_eq!(blockaded(&sim), holds);
    if !holds {
        assert!(
            side_of(&report, theirs).held_field || report.outcome.undecided(),
            "{:?}",
            report.outcome
        );
    }
    assert_eq!(money_drift(sim.world()), Some(0));
    assert_eq!(check_invariants(sim.world()), Ok(()));
}

/// Drei Flotten liegen im Frieden im Heimatsystem von B; dann erklären A und C dem B den
/// Krieg. Gibt die Partie am ersten Morgen des Gefechts zurück.
fn two_against_one(seed: u64) -> (Headless, SystemId) {
    let (a, b, c) = (PolityId(0), PolityId(1), PolityId(2));
    let mut sim = with_player(seed);
    let target = home_of(&sim, b).1;
    for id in [FleetId(0), FleetId(2)] {
        march(&mut sim, id, target);
    }
    assert_eq!(battles(&sim).next_id, 0, "im Frieden kein Gefecht");
    for polity in [a, c] {
        sim.issue(polity, SimCommand::DeclareWar { target: b });
    }
    sim.run_days(1);
    assert_eq!(battles(&sim).in_system(target).unwrap().combatants.len(), 3);
    (sim, target)
}

#[test]
fn peace_frees_a_fleet_while_the_others_fight_on() {
    let (a, b) = (PolityId(0), PolityId(1));
    let (ours, theirs, third) = (FleetId(0), FleetId(1), FleetId(2));
    let (mut sim, target) = two_against_one(24);
    let id = battles(&sim).in_system(target).unwrap().id;
    let (_, side) = battles(&sim).of_fleet(ours).unwrap();
    sim.run_days(2);
    let before = fleet(&sim, ours).clone();
    let weary = weariness(&sim, a);
    let order = |to: SystemId| SimCommand::MoveFleet { fleet: ours, to };
    let home = home_of(&sim, a).1;
    assert!(rejection(&mut sim, a, order(home)).contains("Gefecht"));

    // A schließt Frieden mit B. Am nächsten Morgen hat die Flotte keinen Gegner mehr und
    // ist frei, obwohl B und C weiterkämpfen.
    assert_eq!(
        command(&mut sim, a, SimCommand::MakePeace { target: b }),
        CommandOutcome::Applied
    );
    sim.run_days(1);
    let battle = battles(&sim).get(id).expect("das Gefecht läuft weiter");
    assert!(battles(&sim).has_departed(id, side));
    assert!(battles(&sim).of_fleet(ours).is_none());
    let (_, b_side) = battles(&sim).of_fleet(theirs).unwrap();
    let (_, c_side) = battles(&sim).of_fleet(third).unwrap();
    assert!(battle.is_fighting(b_side) && battle.is_fighting(c_side));
    let free = fleet(&sim, ours);
    assert!(free.is_ready() && !free.in_battle && free.system == target);
    assert_eq!(free.ships.len(), before.ships.len());
    assert!(chronicled(
        &sim,
        target,
        &[&before.name, "keinen Gegner mehr"]
    ));
    // Sie marschiert, wohin sie soll, ohne Rückzug und ohne Sammelfrist.
    assert_eq!(command(&mut sim, a, order(home)), CommandOutcome::Applied);
    sim.run_days(1);
    assert!(!fleets(&sim).docked_in(target).any(|fleet| fleet.id == ours));
    assert!(battles(&sim).get(id).is_some(), "die anderen kämpfen noch");

    // Das Gefecht der anderen geht zu Ende. In der Bilanz steht die Flotte als früh
    // ausgeschieden, ohne Verluste; mit ihr geschieht nichts mehr.
    let report = fight_out(&mut sim, target);
    assert_eq!(report.sides.len(), 3);
    let left = side_of(&report, ours);
    assert!(left.left_early.is_some() && left.held_field);
    assert_eq!(left.lost, [0; 6]);
    assert!(left.left_early.unwrap() < report.ended);
    assert_aftermath(&sim, &report);
    let gone = fleet(&sim, ours);
    assert!(gone.alive && !gone.is_regrouping() && gone.ships.len() == before.ships.len());
    assert!(
        weariness(&sim, a) <= weary,
        "ohne Verluste keine Kriegsmüdigkeit"
    );
    assert!(battles(&sim).departed.is_empty());
    assert_eq!(money_drift(sim.world()), Some(0));
    assert_eq!(check_invariants(sim.world()), Ok(()));
}

#[test]
fn a_fleet_that_leaves_the_field_is_free_at_once_and_shares_the_fate_of_its_side() {
    let a = PolityId(0);
    let (ours, theirs, third) = (FleetId(0), FleetId(1), FleetId(2));
    // Eine lange Sammelfrist, damit sie jedes Gefecht überdauert.
    let mut patient = data().clone();
    patient.balance.military.regroup_days = 200;
    let regroup = patient.balance.military.regroup_days;
    // Zwei Flotten gegen eine; die erste bricht am ersten Morgen ab, die zweite kämpft
    // weiter. Über mehrere Seeds endet es mal mit dem Gegner, mal mit der eigenen Seite
    // auf dem Feld.
    let mut outcomes = [0; 2];
    for seed in 24..30 {
        let (a_side, b, c) = (a, PolityId(1), PolityId(2));
        let mut sim = Headless::new(
            patient.clone(),
            SimConfig {
                seed,
                player: Some(a),
            },
        );
        let target = home_of(&sim, b).1;
        for id in [ours, third] {
            march(&mut sim, id, target);
        }
        for polity in [a_side, c] {
            sim.issue(polity, SimCommand::DeclareWar { target: b });
        }
        sim.run_days(1);
        let id = battles(&sim).in_system(target).unwrap().id;
        let ships = fleet(&sim, ours).ships.len();
        assert_eq!(
            command(&mut sim, a, SimCommand::Retreat { fleet: ours }),
            CommandOutcome::Applied
        );
        // Sobald kein Schiff der Flotte mehr auf dem Feld steht, ist sie frei: am Morgen
        // darauf, obwohl die anderen noch wochenlang kämpfen.
        let mut days = 0;
        while fleet(&sim, ours).in_battle {
            sim.run_days(1);
            days += 1;
            assert!(days < longest_battle_days(data()));
        }
        let freed = sim.clock().day();
        assert!(
            battles(&sim).get(id).is_some(),
            "Seed {seed}: die anderen kämpfen noch"
        );
        assert!(battles(&sim).of_fleet(ours).is_none());
        assert!(battles(&sim).of_fleet(third).is_some());
        // Für ihre Seite wird noch gekämpft: Sie räumt das System nicht, sondern sammelt
        // sich hinter den Linien der Mitstreiter. Befehle nimmt sie so lange nicht an.
        let rallying = fleet(&sim, ours).clone();
        assert_eq!(rallying.ships.len(), ships, "der frühe Rückzug gelingt");
        assert!(rallying.system == target && rallying.route.is_empty());
        assert_eq!(rallying.regroup_until, Some(freed + regroup));
        assert!(chronicled(&sim, target, &["hinter den eigenen Linien"]));
        let home = home_of(&sim, a).1;
        let reason = rejection(
            &mut sim,
            a,
            SimCommand::MoveFleet {
                fleet: ours,
                to: home,
            },
        );
        assert!(reason.contains("sammelt sich"), "{reason}");

        let report = fight_out(&mut sim, target);
        assert!(end_day(&report) > freed + 5, "Seed {seed}");
        let left = side_of(&report, ours);
        assert!(left.left_early.is_some() && !left.held_field && left.retreated);
        let ally_holds = side_of(&report, third).held_field;
        let foe_holds = side_of(&report, theirs).held_field;
        let after = fleet(&sim, ours);
        assert!(after.is_regrouping() && after.ships.len() == ships);
        if ally_holds {
            // Die eigene Seite hält das Feld: Die Flotte bleibt im System, und die Frist
            // läuft weiter wie zuvor.
            outcomes[1] += 1;
            assert!(!foe_holds);
            assert!(after.system == target && after.route.is_empty());
            assert_eq!(after.regroup_until, rallying.regroup_until);
        } else if foe_holds {
            // Der Gegner hält das Feld: Jetzt räumt auch sie das System, und die Frist
            // beginnt mit dem Abzug.
            outcomes[0] += 1;
            assert!(!after.route.is_empty(), "sie weicht aus");
            assert_eq!(after.regroup_until, Some(end_day(&report) + regroup));
            assert!(chronicled(&sim, target, &["weicht nun nach"]));
        }
        assert_aftermath(&sim, &report);
        assert!(battles(&sim).departed.is_empty());
        assert_eq!(money_drift(sim.world()), Some(0));
        assert_eq!(check_invariants(sim.world()), Ok(()));
    }
    assert!(
        outcomes[0] > 0 && outcomes[1] > 0,
        "Szenario neu wählen: mal sollte der Gegner, mal die eigene Seite das Feld halten \
         ({outcomes:?})"
    );
}

#[test]
fn a_fleet_that_gave_way_does_not_drift_back_into_the_same_battle() {
    let (a, b) = (PolityId(0), PolityId(1));
    let (ours, third) = (FleetId(0), FleetId(2));
    let (mut sim, target) = two_against_one(24);
    let id = battles(&sim).in_system(target).unwrap().id;
    assert_eq!(
        command(&mut sim, a, SimCommand::Retreat { fleet: ours }),
        CommandOutcome::Applied
    );
    while fleet(&sim, ours).in_battle {
        sim.run_days(1);
    }
    let until = fleet(&sim, ours).regroup_until.expect("sie sammelt sich");
    // Die Sammelfrist läuft ab, während die anderen noch kämpfen. Die Flotte ist wieder
    // einsatzbereit und liegt neben dem Gefecht, tritt ihm aber nicht von selbst bei: Wer
    // das Feld geräumt hat, kehrt nur auf Befehl in einen neuen Kampf zurück.
    while sim.clock().day() <= until {
        sim.run_days(1);
    }
    assert!(
        battles(&sim).get(id).is_some(),
        "Szenario neu wählen: Das Gefecht sollte die Sammelfrist überdauern"
    );
    let waiting = fleet(&sim, ours);
    assert!(waiting.is_ready() && waiting.system == target && !waiting.in_battle);
    assert!(battles(&sim).of_fleet(ours).is_none());
    assert!(battles(&sim).of_fleet(third).is_some());
    assert!(sim.world().resource::<Diplomacy>().at_war(a, b));
    sim.run_days(3);
    assert!(!fleet(&sim, ours).in_battle);
    assert_eq!(battles(&sim).get(id).unwrap().combatants.len(), 3);
    // Befehle nimmt sie wieder an: Sie marschiert nach Hause.
    let home = home_of(&sim, a).1;
    assert_eq!(
        command(
            &mut sim,
            a,
            SimCommand::MoveFleet {
                fleet: ours,
                to: home
            }
        ),
        CommandOutcome::Applied
    );
    sim.run_days(1);
    assert!(!fleets(&sim).docked_in(target).any(|fleet| fleet.id == ours));
    let report = fight_out(&mut sim, target);
    assert_aftermath(&sim, &report);
    assert_eq!(check_invariants(sim.world()), Ok(()));
}

// --- 14b. Station fern der Heimat, Verluste einer aufgeriebenen Flotte -----------------------

#[test]
fn a_blockade_far_from_home_runs_dry_without_supply() {
    let data = data();
    let balance = &data.balance.military;
    let (a, b) = (PolityId(0), PolityId(1));
    let (ours, theirs) = (FleetId(0), FleetId(1));
    let mut sim = with_player(60);
    let (own_home, home) = (home_of(&sim, a).1, home_of(&sim, b).1);
    // Wie zuvor: Die Heimatflotte ist fort, die Angreifer sperren das Heimatsystem.
    let approach = last_stop(&sim, ours, home);
    let away = sim
        .galaxy()
        .neighbors(home)
        .iter()
        .map(|neighbor| neighbor.system)
        .find(|system| *system != approach)
        .expect("Szenario neu wählen: Das Heimatsystem braucht einen zweiten Nachbarn");
    march(&mut sim, theirs, away);
    sim.issue(
        a,
        SimCommand::SetStance {
            fleet: ours,
            stance: Stance::Blockade,
        },
    );
    let left = sim.clock().day();
    march(&mut sim, ours, home);
    let way_out = sim.clock().day() - left;
    sim.issue(a, SimCommand::DeclareWar { target: b });
    sim.run_days(2);
    assert!(fleets(&sim).is_blockaded(home));
    let arrived = fleet(&sim, ours).min_fuel();
    assert!(
        arrived > 0.3 && arrived < 1.0,
        "Tank bei der Ankunft: {arrived}"
    );

    // Niemand versorgt die Flotte dort. Jeden Tag sinkt der Tank um denselben Anteil; die
    // Sperre hält, solange er nicht leer ist, und fällt mit dem letzten Tropfen.
    let mut days = 0u64;
    let mut last = arrived;
    while fleet(&sim, ours).has_fuel() {
        assert!(fleets(&sim).is_blockaded(home), "Tag {days} auf Station");
        sim.run_days(1);
        days += 1;
        let now = fleet(&sim, ours).min_fuel();
        assert!(now < last, "kein Nachschub in der Fremde");
        last = now;
        assert!(days < 2 * DAYS_PER_YEAR);
    }
    let expected = (arrived / balance.fuel_per_day) as u64;
    assert!(
        days.abs_diff(expected) <= 3,
        "{days} Tage auf Station, erwartet rund {expected}"
    );
    assert!(
        days > 60 && days < DAYS_PER_YEAR,
        "eine Sperre hält Monate, aber kein Jahr: {days} Tage"
    );
    sim.run_days(1);
    assert!(
        !fleets(&sim).is_blockaded(home),
        "ohne Treibstoff keine Sperre"
    );
    assert!(fleet(&sim, ours).alive && fleet(&sim, ours).system == home);
    assert_eq!(battles(&sim).next_id, 0, "gekämpft wurde nie");
    sim.run_days(30);
    assert!(!fleets(&sim).is_blockaded(home));

    // Der Heimweg ist ein Kriechen: mit leeren Tanks dauert er ein Vielfaches des Hinwegs.
    // Zu Hause fasst die Flotte nach und ist wieder zu gebrauchen.
    let turned = sim.clock().day();
    march(&mut sim, ours, own_home);
    let way_back = sim.clock().day() - turned;
    assert!(
        way_back >= 2 * way_out,
        "Hinweg {way_out} Tage, Heimweg {way_back}"
    );
    sim.run_days(5);
    assert!(fleet(&sim, ours).has_fuel());
    sim.run_days(30);
    assert!(fleet(&sim, ours).min_fuel() > 0.9);
    assert_eq!(money_drift(sim.world()), Some(0));
    assert_eq!(check_invariants(sim.world()), Ok(()));
}

#[test]
fn losses_are_weighed_against_a_stable_measure_not_against_what_is_left() {
    use sim::harness::{ScriptedEvent, WarScript};
    let data = data();
    let balance = &data.balance.military;
    let start = balance.starting_fleet_value;
    // Drei Kriege, und alle zehn Monate marschieren die Flotten wieder aufeinander zu: Was
    // vom ersten Gefecht übrig ist, kämpft weiter, bis manche Flotte aufgerieben ist.
    let mut events = Vec::new();
    for round in 0..6u64 {
        for pair in [(0, 1), (2, 3), (4, 5)] {
            let (a, b) = if round % 2 == 0 {
                pair
            } else {
                (pair.1, pair.0)
            };
            events.push(ScriptedEvent::war(
                30 + round * 300,
                PolityId(a),
                PolityId(b),
            ));
        }
    }
    let script = WarScript::new(events);
    let navy = |sim: &Headless, polity: PolityId| navy_value(sim, polity);
    // Fälle, in denen eine schon geschwächte Flotte weiter verlor und sich der Zuwachs auf
    // den Punkt nachrechnen ließ.
    let mut worn_down = 0;
    let mut battles_seen = 0;
    for seed in 0..4u64 {
        let mut sim = observer(seed);
        let polities = sim.polities().list.len();
        let mut reported = 0;
        for day in 0..6 * DAYS_PER_YEAR {
            let before: Vec<f64> = (0..polities)
                .map(|index| weariness(&sim, PolityId::from_index(index)))
                .collect();
            script.run_until(&mut sim, day + 1, false);
            let reports =
                battles(&sim).reports[reported.min(battles(&sim).reports.len())..].to_vec();
            reported = battles(&sim).next_id as usize - battles(&sim).active.len();
            if reports.is_empty() {
                continue;
            }
            battles_seen += reports.len();
            // Am Monatsersten schreibt auch der Monatsabschluss die Kriegsmüdigkeit fort:
            // Dann lässt sich der Beitrag des Gefechts nicht auf den Punkt trennen.
            let month_start = (day + 1).is_multiple_of(DAYS_PER_MONTH);
            for index in 0..polities {
                let polity = PolityId::from_index(index);
                let lost: f64 = reports
                    .iter()
                    .flat_map(|report| report.sides.iter())
                    .filter(|side| side.owner == polity)
                    .map(|side| side.lost_value as f64)
                    .sum();
                if lost <= 0.0 {
                    continue;
                }
                // Die Flotte, wie sie in das Gefecht ging: was blieb, und was verloren ist.
                let went_in = navy(&sim, polity) + lost;
                let share = lost / went_in.max(start);
                let rise = weariness(&sim, polity) - before[index];
                let expected = balance.weariness_per_navy_lost * share;
                if !month_start && weariness(&sim, polity) < 100.0 && reports.len() == 1 {
                    assert!(
                        (rise - expected).abs() < 0.05,
                        "Seed {seed}, Tag {day}: Kriegsmüdigkeit +{rise:.2} statt +{expected:.2} \
                         bei {lost:.0} von {went_in:.0} Credits"
                    );
                    // Eine geschwächte Flotte: Gemessen an dem, was von ihr übrig war,
                    // wöge derselbe Verlust um ein Viertel schwerer oder mehr.
                    if went_in < 0.8 * start {
                        worn_down += 1;
                        assert!(rise < 0.81 * balance.weariness_per_navy_lost * lost / went_in);
                    }
                }
                // Eine schwere Niederlage setzt einen schweren Verlust voraus, gemessen am
                // selben Maßstab.
                if share < balance.heavy_defeat_share {
                    let name = &sim.polities().get(polity).name;
                    let shaken_now = sim
                        .world()
                        .resource::<Chronicle>()
                        .entries
                        .iter()
                        .filter(|entry| {
                            entry.text.contains("Die Niederlage bei")
                                && entry.text.contains(name.as_str())
                        })
                        .count();
                    let shaken_then = sim
                        .world()
                        .resource::<Chronicle>()
                        .entries
                        .iter()
                        .take_while(|entry| entry.tick < day * TICKS_PER_DAY)
                        .filter(|entry| {
                            entry.text.contains("Die Niederlage bei")
                                && entry.text.contains(name.as_str())
                        })
                        .count();
                    assert_eq!(
                        shaken_now, shaken_then,
                        "Seed {seed}, Tag {day}: kleine Verluste sind keine schwere Niederlage"
                    );
                }
            }
        }
        assert_eq!(check_invariants(sim.world()), Ok(()));
    }
    assert!(battles_seen >= 12, "nur {battles_seen} Gefechte");
    assert!(
        worn_down >= 3,
        "Szenario neu wählen: nur {worn_down}-mal verlor eine schon geschwächte Flotte weiter"
    );
}

// --- 15. Mehr als zwei Parteien, Abspaltung, Erlöschen ---------------------------------------

/// Nach einem Gefecht steht keine Flotte mehr darin. Gefragt wird je Seite: Wer das Feld
/// hält, bleibt und ist einsatzbereit. Wer es einem Gegner überlassen hat und noch Schiffe
/// hat, räumt das System und sammelt sich. Wer es geräumt hat, während jemand von der
/// eigenen Seite es hält, sammelt sich im System. Ohne Entscheidung (Patt, Zeitgrenze)
/// bleibt nur, wer im eigenen System steht. Wer keine Schiffe mehr hat, besteht nicht mehr.
/// Wer schon früher ausgeschieden ist, steht nur noch in der Bilanz.
fn assert_aftermath(sim: &Headless, report: &BattleReport) {
    let home = sim.galaxy().system_owner(report.system);
    let diplomacy = sim.world().resource::<Diplomacy>();
    let holds = |side: &SideReport| side.held_field && side.left_early.is_none();
    for (index, side) in report.sides.iter().enumerate() {
        let fleet = fleet(sim, side.fleet.expect("jede Partei ist eine Flotte"));
        if side.left_early.is_some() {
            continue;
        }
        assert!(!fleet.in_battle && fleet.battle.is_none(), "{}", fleet.name);
        let foe_holds = report
            .sides
            .iter()
            .any(|other| holds(other) && diplomacy.at_war(side.owner, other.owner));
        let friend_holds = report.sides.iter().enumerate().any(|(place, other)| {
            place != index && holds(other) && !diplomacy.at_war(side.owner, other.owner)
        });
        let at_home = home == Some(side.owner);
        // 0: bleibt einsatzbereit, 1: sammelt sich im System, 2: räumt das System.
        let expected = if report.outcome == BattleOutcome::TimedOut && side.held_field {
            if at_home { 0 } else { 2 }
        } else if side.held_field {
            0
        } else if foe_holds {
            2
        } else if friend_holds {
            1
        } else if side.stalemate && at_home {
            0
        } else {
            2
        };
        if !fleet.alive {
            assert!(fleet.ships.is_empty());
        } else if expected == 0 {
            assert!(
                fleet.system == report.system && fleet.is_ready(),
                "{}",
                fleet.name
            );
        } else if expected == 1 {
            assert!(fleet.is_regrouping(), "{}", fleet.name);
            assert!(
                fleet.system == report.system && fleet.route.is_empty(),
                "{}",
                fleet.name
            );
        } else {
            assert!(fleet.is_regrouping(), "{}", fleet.name);
            // Der Abzug folgt den Routen, notfalls über mehrere Sprünge.
            assert!(!fleet.route.is_empty(), "{}", fleet.name);
            let mut from = report.system;
            for step in &fleet.route {
                assert!(sim.galaxy().lane_between(from, *step).is_some());
                from = *step;
            }
        }
    }
    // Gegner aus demselben Gefecht weichen nie in dasselbe System aus.
    let leaving: Vec<(PolityId, SystemId)> = report
        .sides
        .iter()
        .filter_map(|side| {
            let fleet = fleet(sim, side.fleet?);
            (fleet.alive && fleet.is_regrouping())
                .then(|| fleet.route.last().map(|target| (side.owner, *target)))
                .flatten()
        })
        .collect();
    for (index, (owner, target)) in leaving.iter().enumerate() {
        for (other, same) in &leaving[..index] {
            assert!(
                target != same || !diplomacy.at_war(*owner, *other),
                "zwei Gegner weichen in dasselbe System aus"
            );
        }
    }
}

#[test]
fn three_communities_fight_in_one_system() {
    let (a, b, c) = (PolityId(0), PolityId(1), PolityId(2));
    let ids = [FleetId(0), FleetId(1), FleetId(2)];
    let meet = |wars: &[(PolityId, PolityId)]| {
        let mut sim = with_player(24);
        let target = home_of(&sim, b).1;
        place(&mut sim, ids[0], target);
        place(&mut sim, ids[2], target);
        for (from, to) in wars {
            sim.issue(*from, SimCommand::DeclareWar { target: *to });
        }
        sim.run_days(1);
        (sim, target)
    };

    // Jeder gegen jeden.
    let (mut sim, target) = meet(&[(a, b), (a, c), (c, b)]);
    {
        let battle = battles(&sim)
            .in_system(target)
            .expect("das Gefecht hat begonnen");
        // Jede Flotte ist eine Partei, in der Nummernfolge der Flotten.
        let parties: Vec<Option<FleetId>> = battle.combatants.iter().map(|c| c.fleet).collect();
        assert_eq!(parties, ids.map(Some).to_vec());
        for x in 0..3u16 {
            for y in 0..3u16 {
                assert_eq!(battle.is_hostile(x, y), x != y);
            }
        }
        assert!(chronicled(
            &sim,
            target,
            &["Gefecht mehrerer Gemeinschaften"]
        ));
    }
    let report = fight_out(&mut sim, target);
    assert_eq!(report.sides.len(), 3);
    // Wo jeder jedem feind ist, hält am Ende höchstens einer das Feld.
    let holders = report.sides.iter().filter(|side| side.held_field).count();
    assert!(
        report.outcome.undecided() || holders <= 1,
        "{holders} Parteien halten das Feld"
    );
    assert!(
        report
            .sides
            .iter()
            .any(|side| side.lost.iter().sum::<u32>() > 0)
    );
    assert_aftermath(&sim, &report);

    // Zwei gegen einen: A und C führen Krieg gegen B, aber nicht gegeneinander.
    let (mut sim, target) = meet(&[(a, b), (c, b)]);
    {
        let battle = battles(&sim).in_system(target).unwrap();
        assert_eq!(battle.combatants.len(), 3);
        let side = |id: FleetId| battles(&sim).of_fleet(id).unwrap().1 as u16;
        let (first, enemy, second) = (side(ids[0]), side(ids[1]), side(ids[2]));
        assert!(battle.is_hostile(first, enemy) && battle.is_hostile(enemy, second));
        assert!(
            !battle.is_hostile(first, second),
            "wer nicht im Krieg liegt, kämpft nicht"
        );
        // Die ersten beiden Parteien stehen einander gegenüber: Sie sind einander feind.
        assert!(battle.is_hostile(0, 1));
    }
    let report = fight_out(&mut sim, target);
    assert_aftermath(&sim, &report);
    // In diesem Szenario schlagen die beiden den Dritten und halten gemeinsam das Feld.
    assert!(
        side_of(&report, ids[0]).held_field
            && side_of(&report, ids[2]).held_field
            && !side_of(&report, ids[1]).held_field,
        "Szenario neu wählen: Zwei Startflotten sollten eine schlagen"
    );
    assert!(chronicled(&sim, target, &["halten das Feld"]));
    // Danach liegen sie in Frieden nebeneinander.
    sim.run_days(5);
    assert_eq!(battles(&sim).next_id, 1);
    assert_eq!(check_invariants(sim.world()), Ok(()));

    // Sind die ersten beiden Flotten nicht miteinander im Krieg, rückt der erste Gegner
    // der ersten an die zweite Stelle: Die ersten beiden Parteien stellt der Rechner
    // einander gegenüber auf.
    let (sim, target) = meet(&[(a, c), (b, c)]);
    let battle = battles(&sim).in_system(target).unwrap();
    let parties: Vec<Option<FleetId>> = battle.combatants.iter().map(|c| c.fleet).collect();
    assert_eq!(parties, vec![Some(ids[0]), Some(ids[2]), Some(ids[1])]);
    assert!(battle.is_hostile(0, 1) && battle.is_hostile(1, 2) && !battle.is_hostile(0, 2));
}

#[test]
fn a_battle_outlasts_the_secession_and_the_end_of_a_participant() {
    let data = data();
    let (a, b) = (PolityId(0), PolityId(1));
    let (ours, theirs) = (FleetId(0), FleetId(1));
    let mut sim = with_player(25);
    let (capital, target) = home_of(&sim, b);
    place(&mut sim, ours, target);
    sim.issue(a, SimCommand::DeclareWar { target: b });
    sim.run_days(8);
    assert!(battles(&sim).in_system(target).is_some());
    let colonies: Vec<PlanetId> = sim
        .galaxy()
        .planets_of(b)
        .map(|planet| planet.id)
        .filter(|planet| *planet != capital)
        .collect();

    // Mitten im Gefecht sagt sich die Heimatwelt der Verteidiger los und ruft einen
    // Freistaat aus; ihre Kolonien treten zu ihm über. Die alte Gemeinschaft hält keine
    // Welt mehr: Sie ist erloschen. Ihre Flotte kämpft weiter.
    let tick = sim.clock().tick;
    let mut state = sim.state();
    let mut secede = |planet: PlanetId, joins: Option<PolityId>| {
        apply_secession(
            &Secession {
                planet,
                from: b,
                joins,
            },
            &mut state.finance,
            &mut state.galaxy,
            &mut state.economy,
            &state.logistics,
            &mut state.polities,
            &mut state.chronicle,
            data,
            tick,
        )
    };
    let free_state = secede(capital, None).expect("die Heimatwelt ruft den Freistaat aus");
    for colony in colonies {
        assert_eq!(secede(colony, Some(free_state)), Some(free_state));
    }
    // Wie im Monatsschritt der Partie: Der Freistaat nimmt das Wissen des alten Reichs mit.
    state.research.inherit(data, b, free_state);
    state.economy.sync_polities();
    state
        .research
        .publish_effects(&mut state.economy, &mut state.logistics);
    state.polities.get_mut(b).alive = false;
    state.polities.get_mut(b).capital = None;
    state.install(sim.world_mut());
    assert_eq!(sim.galaxy().system_owner(target), Some(free_state));
    assert_eq!(sim.galaxy().planets_of(b).count(), 0);
    assert!(!sim.polities().get(b).alive && sim.polities().get(free_state).alive);
    assert!(fleet(&sim, theirs).in_battle);
    assert_eq!(check_invariants(sim.world()), Ok(()));

    // Das Gefecht geht ordentlich zu Ende.
    let report = fight_out(&mut sim, target);
    assert_eq!(report.sides.len(), 2);
    assert_eq!(side_of(&report, theirs).owner, b);
    assert!(report.sides.iter().any(|side| side.held_field) || report.outcome.undecided());
    assert_aftermath(&sim, &report);
    let place_name = sim.galaxy().system(target).name.clone();
    assert!(chronicled(
        &sim,
        target,
        &["Gefecht bei", &place_name, "nach"]
    ));

    // Der Freistaat führt mit niemandem Krieg: Wer im System bleibt, bleibt unbehelligt.
    // Auch die Monatsabschlüsse danach (Sold, Finanzen, Zusammenhalt) laufen durch.
    sim.run_days(3 * DAYS_PER_MONTH);
    assert_eq!(battles(&sim).next_id, 1);
    assert!(battles(&sim).active.is_empty());
    assert_eq!(money_drift(sim.world()), Some(0));
    assert_eq!(check_invariants(sim.world()), Ok(()));
}

// --- 16. Forschung und Flotte ----------------------------------------------------------------

fn research(sim: &Headless) -> &Research {
    sim.research()
}

/// Gewährt einer Gemeinschaft Technologien samt ihren Voraussetzungen (Aufbau eines
/// Szenarios).
fn grant(sim: &mut Headless, polity: PolityId, keys: &[&str]) {
    let data = data();
    let mut research = sim.world_mut().resource_mut::<Research>();
    for key in keys {
        research.grant(data, polity, data.tech_index(key).unwrap());
    }
}

/// Waffen und Verteidigung der zweiten Stufe.
const TIER_TWO: [&str; 2] = ["waffen_2", "verteidigung_2"];

/// Zwei Startflotten treffen im Heimatsystem der Gemeinschaft 1 aufeinander. Mit `advanced`
/// beherrscht die Angreiferin (Gemeinschaft 0) zusätzlich Waffen und Verteidigung der
/// zweiten Stufe; alles andere ist gleich. Gibt die Bilanz zurück und die Aufschläge, mit
/// denen beide Seiten kämpften.
fn duel_of_equals(seed: u64, advanced: bool) -> (BattleReport, [CombatModifiers; 2]) {
    let (a, b) = (PolityId(0), PolityId(1));
    let (ours, theirs) = (FleetId(0), FleetId(1));
    let mut sim = with_player(seed);
    if advanced {
        grant(&mut sim, a, &TIER_TWO);
    }
    let target = home_of(&sim, b).1;
    place(&mut sim, ours, target);
    sim.issue(a, SimCommand::DeclareWar { target: b });
    sim.run_days(1);
    let modifiers = [ours, theirs].map(|id| {
        let (battle, side) = battles(&sim)
            .of_fleet(id)
            .expect("das Gefecht hat begonnen");
        battle.combatants[side].modifiers
    });
    (fight_out(&mut sim, target), modifiers)
}

#[test]
fn better_weapons_and_defences_win_battles_between_equal_fleets() {
    let data = data();
    let (ours, theirs) = (FleetId(0), FleetId(1));
    // Viele Partien: Ein einzelnes Gefecht unter Gleichen entscheidet der Zufall, und auch
    // zwei Dutzend schwanken noch um mehrere Siege.
    let seeds = 100..196u64;
    let count = seeds.clone().count();
    let won = |report: &BattleReport| {
        report.outcome == BattleOutcome::Decided
            && side_of(report, ours).held_field
            && !side_of(report, theirs).held_field
    };
    // Was die beiden Technologien samt ihren Voraussetzungen bewirken, steht in den
    // Spieldaten.
    let mut lesson = Research::default();
    for key in TIER_TWO {
        lesson.grant(data, PolityId(0), data.tech_index(key).unwrap());
    }
    let effects = lesson.effects(PolityId(0));
    assert!(effects.weapon_damage > 1.0 && effects.shields > 1.0);
    let (mut plain_wins, mut advanced_wins) = (0, 0);
    let (mut plain_lost, mut advanced_lost) = (0.0f64, 0.0f64);
    for seed in seeds {
        let (plain, base) = duel_of_equals(seed, false);
        let (advanced, raised) = duel_of_equals(seed, true);
        // Die Technik kommt im Gefecht an: Die Angreiferin kämpft mit genau den Aufschlägen
        // ihrer Forschung, der Verteidiger wie in der Gegenprobe.
        assert_eq!(
            raised[0].damage, effects.weapon_damage as f32,
            "Seed {seed}"
        );
        assert_eq!(raised[0].range, effects.weapon_range as f32, "Seed {seed}");
        assert_eq!(raised[0].shields, effects.shields as f32, "Seed {seed}");
        assert_eq!(raised[0].hull, effects.hull as f32, "Seed {seed}");
        assert!(raised[0].damage > base[0].damage && raised[0].shields > base[0].shields);
        assert_eq!(raised[1], base[1], "Seed {seed}");
        // Gleiche Flotten: In beiden Partien beginnt jede Seite mit demselben Bauwert.
        for report in [&plain, &advanced] {
            assert_eq!(report.sides[0].initial, report.sides[1].initial);
        }
        plain_wins += won(&plain) as usize;
        advanced_wins += won(&advanced) as usize;
        plain_lost += side_of(&plain, ours).lost_value as f64;
        advanced_lost += side_of(&advanced, ours).lost_value as f64;
    }
    // Ohne Vorsprung ist es ein Gefecht unter Gleichen, das jede Seite gewinnen kann. Mit
    // ihm gewinnt die Angreiferin deutlich öfter (um mindestens ein Fünftel aller Gefechte
    // mehr) und in der klaren Mehrzahl der Fälle, und sie verliert dabei weniger.
    assert!(
        plain_wins * 4 >= count && plain_wins * 4 <= count * 3,
        "ohne Vorsprung {plain_wins} von {count} Siegen: kein Gefecht unter Gleichen"
    );
    assert!(
        advanced_wins >= plain_wins + count / 5 && advanced_wins * 5 >= count * 3,
        "mit Technik {advanced_wins}, ohne {plain_wins} von {count} Siegen"
    );
    assert!(
        advanced_lost < 0.9 * plain_lost,
        "Verluste mit Technik {advanced_lost:.0}, ohne {plain_lost:.0}"
    );
}

#[test]
fn a_design_needs_its_technology_and_the_yard_picks_the_best_known() {
    let data = data();
    let (me, other) = (PolityId(0), PolityId(1));
    let mut sim = with_player(12);
    let (capital, home) = home_of(&sim, me);
    let (basic, improved) = (design("speer"), design("speer_2"));
    let needed = data.designs[improved as usize].stats.techs.clone();
    assert!(
        !needed.is_empty(),
        "der verbesserte Entwurf setzt Forschung voraus"
    );
    assert!(data.designs[basic as usize].stats.techs.is_empty());
    assert!(
        needed.iter().all(|tech| !research(&sim).knows(me, *tech)),
        "Szenario neu wählen: Die Gemeinschaft beginnt schon mit dieser Technik"
    );
    let build = |planet: PlanetId, design: u16| SimCommand::BuildShip { planet, design };
    let build_class = |planet: PlanetId, class: ShipClass| SimCommand::BuildClass { planet, class };

    // Ohne die Technologie nimmt die Werft den Auftrag nicht an und nennt sie beim Namen.
    let reason = rejection(&mut sim, me, build(capital, improved));
    let missing = &data.techs[needed[0] as usize].name;
    assert!(
        reason.contains("Technologie") && reason.contains(missing.as_str()),
        "{reason}"
    );
    assert!(fleets(&sim).orders.is_empty());
    // Die Wahl nach Klasse nimmt den besten Entwurf, den die Gemeinschaft bauen kann.
    assert_eq!(
        data.best_design(ShipClass::Cruiser, research(&sim).known(me)),
        Some(basic)
    );
    assert_eq!(
        command(&mut sim, me, build_class(capital, ShipClass::Cruiser)),
        CommandOutcome::Applied
    );
    assert_eq!(fleets(&sim).orders.len(), 1);
    assert_eq!(fleets(&sim).orders[0].design, basic);
    // Bordjäger gibt es auch auf diesem Weg nicht einzeln.
    assert!(
        rejection(&mut sim, me, build_class(capital, ShipClass::Fighter)).contains("Bordjäger")
    );
    assert_eq!(
        command(&mut sim, me, SimCommand::CancelBuild { order: 0 }),
        CommandOutcome::Applied
    );

    // Mit der Technologie ist der Entwurf baubar, und die Werft wählt ihn von selbst.
    let keys: Vec<&str> = needed
        .iter()
        .map(|tech| data.techs[*tech as usize].key.as_str())
        .collect();
    grant(&mut sim, me, &keys);
    assert!(needed.iter().all(|tech| research(&sim).knows(me, *tech)));
    assert_eq!(
        command(&mut sim, me, build(capital, improved)),
        CommandOutcome::Applied
    );
    assert_eq!(
        command(&mut sim, me, build_class(capital, ShipClass::Cruiser)),
        CommandOutcome::Applied
    );
    let designs: Vec<u16> = fleets(&sim)
        .orders
        .iter()
        .map(|order| order.design)
        .collect();
    assert_eq!(designs, vec![improved, improved]);
    // Wissen gehört der Gemeinschaft: Eine andere baut den Entwurf weiterhin nicht.
    let (their_capital, _) = home_of(&sim, other);
    assert!(
        needed
            .iter()
            .any(|tech| !research(&sim).knows(other, *tech))
    );
    assert!(rejection(&mut sim, other, build(their_capital, improved)).contains("Technologie"));
    assert_eq!(
        data.best_design(ShipClass::Cruiser, research(&sim).known(other)),
        Some(basic)
    );

    // Der Auftrag wird gebaut wie jeder andere, und das Schiff stößt zur Flotte.
    let build_days = data.designs[improved as usize].stats.build_days as u64;
    let ships = fleets(&sim).ship_count(me);
    for _ in 0..build_days + 30 {
        set_stock(&mut sim, home, &PARTS, 100.0);
        sim.run_days(1);
        if fleets(&sim).ship_count(me) > ships {
            break;
        }
    }
    let built = fleet(&sim, FleetId(0)).ships.last().unwrap();
    assert_eq!(built.design, improved, "das verbesserte Schiff ist fertig");
    assert_eq!(money_drift(sim.world()), Some(0));
    assert_eq!(check_invariants(sim.world()), Ok(()));
}

#[test]
fn a_battle_sharpens_the_picture_of_the_opponents_technology() {
    let data = data();
    let balance = &data.balance.research;
    let (a, b, c) = (PolityId(0), PolityId(1), PolityId(2));
    let mut sim = with_player(22);
    let target = home_of(&sim, b).1;
    place(&mut sim, FleetId(0), target);
    sim.issue(a, SimCommand::DeclareWar { target: b });
    sim.run_days(1);
    assert!(battles(&sim).in_system(target).is_some());
    let intel = |sim: &Headless, viewer: PolityId, of: PolityId| research(sim).intel(viewer, of);
    let pairs = [(a, b), (b, a), (a, c), (c, a), (b, c), (c, b)];
    let width = |sim: &Headless, viewer: PolityId, of: PolityId| -> u32 {
        research(sim)
            .estimate(data, viewer, of)
            .iter()
            .map(|range| range.width() as u32)
            .sum()
    };
    let wide = [width(&sim, a, b), width(&sim, b, a)];
    assert!(
        wide.iter().all(|sum| *sum > 0),
        "noch ist das Bild unscharf"
    );

    // Takt für Takt bis zum Ende: Im letzten Takt gewinnen die Gegner Einblick.
    let month = TICKS_PER_DAY * DAYS_PER_MONTH;
    let limit = sim.clock().tick + longest_battle_days(data) * TICKS_PER_DAY;
    let (before, ended) = loop {
        let before = pairs.map(|(viewer, of)| intel(&sim, viewer, of));
        let tick = sim.clock().tick;
        sim.tick();
        if battles(&sim).active.is_empty() {
            break (before, tick);
        }
        assert!(sim.clock().tick < limit, "das Gefecht endet nie");
    };
    assert!(
        !ended.is_multiple_of(month),
        "Szenario neu wählen: Das Gefecht endet im Takt eines Monatsabschlusses"
    );
    let report = battles(&sim).reports.last().unwrap().clone();
    assert!(
        report.sides.iter().any(|side| side.lost_value > 0.0),
        "Szenario neu wählen: Es sollte zum Kampf gekommen sein"
    );
    let after = pairs.map(|(viewer, of)| intel(&sim, viewer, of));
    let gain = balance.intel_per_battle;
    assert!(gain > 0.0);
    // Beide Gegner kennen einander um genau den Hebel besser; Unbeteiligte erfahren nichts,
    // und über Unbeteiligte erfährt niemand etwas.
    for index in 0..2 {
        assert_eq!(
            after[index],
            (before[index] + gain).min(1.0),
            "Paar {index}"
        );
        assert!(after[index] > before[index]);
    }
    assert_eq!(after[2..], before[2..]);

    // Am nächsten Monatsersten folgt das Bild dem Einblick: Die Spannen werden enger und
    // enthalten weiter die Wahrheit.
    sim.run_days(DAYS_PER_MONTH);
    let sharp = [width(&sim, a, b), width(&sim, b, a)];
    for (index, (viewer, of)) in pairs[..2].iter().enumerate() {
        assert!(
            sharp[index] < wide[index],
            "Paar {index}: Spannen {} statt {}",
            sharp[index],
            wide[index]
        );
        let picture = research(&sim).estimate(data, *viewer, *of);
        for (range, branch) in picture.iter().zip(TechBranch::ALL) {
            assert!(range.contains(research(&sim).tier(data, *of, branch)));
        }
    }
    // Ohne neue Gefechte verblasst der Einblick wieder, aber nicht sofort.
    sim.run_days(DAYS_PER_YEAR);
    let faded = intel(&sim, a, b);
    assert!(faded < after[0] && faded > before[0], "{faded}");
    assert_eq!(check_invariants(sim.world()), Ok(()));
}

// --- 15. Ganze Partien ohne Eingriff ------------------------------------------------
//
// Die Prüfungen oben legen Ware in die Lager, wo sie auf das Tausendstel genau rechnen. Hier
// läuft die Wirtschaft unberührt: Was Werften, Flotten und Kriege brauchen und bewirken,
// muss aus der Partie selbst kommen.

/// Preis der drei Bauteile am Markt eines Systems.
fn part_prices(sim: &Headless, system: SystemId) -> [f64; 3] {
    PARTS.map(|key| economy(sim).market(system).price_of(good(key)))
}

#[test]
fn an_order_in_an_untouched_economy_is_built_from_the_market_and_moves_it() {
    let me = PolityId(0);
    let (corvette, cruiser) = (design("dolch"), design("speer"));
    let days_of = |design: u16| u64::from(data().designs[design as usize].stats.build_days);
    // Rüstungskapital auf allen Welten der Gemeinschaft.
    let arms = |sim: &Headless| -> f64 {
        sim.galaxy()
            .planets_of(me)
            .map(|planet| sim::fleet::arms_capital(data(), economy(sim), planet.id))
            .sum()
    };
    for seed in [3, 12, 18, 40] {
        let mut ordered = observer(seed);
        let mut control = observer(seed);
        ordered.run_days(20 * DAYS_PER_YEAR);
        control.run_days(20 * DAYS_PER_YEAR);
        let (capital, home) = home_of(&ordered, me);
        let before = fleets(&ordered).ship_count(me);
        // Eine Korvette, danach zwei Kreuzer: ein kleines Flottenprogramm auf dem einen
        // Bauplatz der Hauptwelt.
        for design in [corvette, cruiser, cruiser] {
            ordered.issue(
                me,
                SimCommand::BuildShip {
                    planet: capital,
                    design,
                },
            );
        }
        let mut first_ship = None;
        let mut all_ships = None;
        let mut peak = [0.0f64; 3];
        let (mut works, mut usual) = (0.0, 0.0);
        for day in 1..=2 * DAYS_PER_YEAR {
            ordered.run_days(1);
            control.run_days(1);
            let built = fleets(&ordered).ship_count(me) - before;
            if built >= 1 && first_ship.is_none() {
                first_ship = Some(day);
            }
            if built >= 3 && all_ships.is_none() {
                all_ships = Some(day);
            }
            let (with, without) = (part_prices(&ordered, home), part_prices(&control, home));
            for index in 0..3 {
                peak[index] = peak[index].max(with[index] / without[index]);
            }
            works += arms(&ordered);
            usual += arms(&control);
        }
        // Die Wirtschaft liefert, was die Werft braucht: Die Schiffe sind kaum später
        // fertig, als ihre Bauzeit verlangt.
        let first_ship = first_ship.expect("die Korvette wurde nie fertig");
        let all_ships = all_ships.expect("das Programm wurde nie fertig");
        assert!(
            first_ship <= days_of(corvette) * 3 / 2,
            "Seed {seed}: Korvette nach {first_ship} Tagen"
        );
        assert!(
            all_ships <= (days_of(corvette) + 2 * days_of(cruiser)) * 5 / 4,
            "Seed {seed}: Programm nach {all_ships} Tagen"
        );
        // Der Bedarf der Werft kommt am Markt an: Jedes Bauteil wird zeitweise spürbar
        // teurer als in der Partie ohne Auftrag, im Mittel der drei um die Hälfte. (Die
        // Vergleichspartie ist kein toter Markt: Auch dort kauft die Wartung der Flotte
        // Bauteile, und Rüstungsbetriebe liefern sie.)
        for (index, key) in PARTS.iter().enumerate() {
            assert!(
                peak[index] >= 1.2,
                "Seed {seed}: {key} höchstens {:.2}-mal so teuer",
                peak[index]
            );
        }
        let mean = peak.iter().sum::<f64>() / 3.0;
        assert!(
            mean >= 1.5,
            "Seed {seed}: Bauteile im Mittel höchstens {mean:.2}-mal so teuer"
        );
        // … und die Rüstungsbetriebe der Gemeinschaft wachsen daran. Gemessen wird über
        // die ganzen zwei Jahre: An einem einzelnen Tag entscheidet, wo gerade eine
        // Investition fiel.
        assert!(
            works > usual * 1.2,
            "Seed {seed}: Rüstungskapital im Mittel {:.2} gegen {:.2}",
            works / (2 * DAYS_PER_YEAR) as f64,
            usual / (2 * DAYS_PER_YEAR) as f64
        );
        assert_eq!(money_drift(ordered.world()), Some(0));
        assert_eq!(check_invariants(ordered.world()), Ok(()));
    }
}

#[test]
fn a_war_wears_on_cohesion_and_makes_borrowing_dearer() {
    use sim::finance::bond_yield;
    use sim::harness::{ScriptedEvent, WarScript};
    let (a, b) = (PolityId(0), PolityId(1));
    for seed in [3, 22, 28] {
        // Zwei Jahre Krieg gegen dieselbe Partie im Frieden.
        let script = WarScript::new(vec![ScriptedEvent::war(5 * DAYS_PER_YEAR, a, b)]);
        let mut war = observer(seed);
        let mut peace = observer(seed);
        script.run_until(&mut war, 7 * DAYS_PER_YEAR, false);
        peace.run_days(7 * DAYS_PER_YEAR);
        assert!(battles(&war).next_id > 0, "Seed {seed}: kein Gefecht");
        for polity in [a, b] {
            let cohesion = |sim: &Headless| summarize(sim).polities[polity.index()].cohesion;
            let rate = |sim: &Headless| bond_yield(data(), finance(sim), polity);
            assert!(weariness(&war, polity) > 10.0);
            assert_eq!(weariness(&peace, polity), 0.0);
            // Die Müdigkeit drückt den Zusammenhalt der eigenen Welten …
            assert!(
                cohesion(&war) < cohesion(&peace) - 1.5,
                "Seed {seed}, {polity:?}: Zusammenhalt {:.1} gegen {:.1}",
                cohesion(&war),
                cohesion(&peace)
            );
            // … und der Markt verlangt für neue Anleihen mehr Zins.
            assert!(
                rate(&war) > rate(&peace) + 0.01,
                "Seed {seed}, {polity:?}: Rendite {:.3} gegen {:.3}",
                rate(&war),
                rate(&peace)
            );
        }
        assert_eq!(check_invariants(war.world()), Ok(()));
    }
}

#[test]
fn commands_given_during_a_battle_replay_to_the_same_war() {
    let (a, b) = (PolityId(0), PolityId(1));
    for seed in [5, 23] {
        let mut original = observer(seed);
        original.run_days(40);
        let home = home_of(&original, a).1;
        let target = home_of(&original, b).1;
        original.issue(a, SimCommand::DeclareWar { target: b });
        original.issue(
            a,
            SimCommand::SetStance {
                fleet: FleetId(0),
                stance: Stance::Blockade,
            },
        );
        original.issue(
            a,
            SimCommand::MoveFleet {
                fleet: FleetId(0),
                to: target,
            },
        );
        for _ in 0..2 * DAYS_PER_YEAR {
            if battles(&original).in_system(target).is_some() {
                break;
            }
            original.run_days(1);
        }
        assert!(
            battles(&original).in_system(target).is_some(),
            "Seed {seed}: kein Gefecht"
        );
        // Mitten am Tag eine neue Aufstellung …
        for _ in 0..7 {
            original.tick();
        }
        original.issue(
            a,
            SimCommand::SetFleetOrders {
                fleet: FleetId(0),
                formation: Formation::Wedge,
                doctrine: Doctrine::Aggressive,
            },
        );
        original.run_days(3);
        // … ein Marschbefehl, den das Gefecht abweist …
        original.issue(
            a,
            SimCommand::MoveFleet {
                fleet: FleetId(0),
                to: home,
            },
        );
        for _ in 0..5 {
            original.tick();
        }
        // … und der Rückzug.
        original.issue(a, SimCommand::Retreat { fleet: FleetId(0) });
        original.run_days(200);
        let log = original.command_log().clone();
        let outcome_of = |wanted: fn(&SimCommand) -> bool| {
            log.entries
                .iter()
                .rev()
                .find(|entry| wanted(&entry.command))
                .map(|entry| entry.outcome.clone())
        };
        assert_eq!(
            outcome_of(|command| matches!(command, SimCommand::SetFleetOrders { .. })),
            Some(CommandOutcome::Applied)
        );
        assert!(matches!(
            outcome_of(|command| matches!(command, SimCommand::MoveFleet { .. })),
            Some(CommandOutcome::Rejected(_))
        ));
        assert_eq!(
            outcome_of(|command| matches!(command, SimCommand::Retreat { .. })),
            Some(CommandOutcome::Applied)
        );
        let report = battles(&original)
            .reports
            .first()
            .expect("das Gefecht ist zu Ende");
        assert!(
            report
                .sides
                .iter()
                .any(|side| side.owner == a && side.retreated)
        );

        // Dieselbe Partie noch einmal, nur aus dem Protokoll: derselbe Krieg.
        let mut replay = observer(seed);
        replay.set_replay(&log);
        while replay.clock().tick < original.clock().tick {
            replay.tick();
        }
        assert_eq!(replay.hash(), original.hash(), "Seed {seed}");
        assert_eq!(battles(&replay).reports, battles(&original).reports);
        assert_eq!(replay.command_log().entries, log.entries);
        // Ohne die Befehle im Gefecht verläuft sie anders.
        let mut passive = observer(seed);
        passive.set_replay(&sim::command::CommandLog {
            entries: log.entries[..3].to_vec(),
            ..Default::default()
        });
        while passive.clock().tick < original.clock().tick {
            passive.tick();
        }
        assert_ne!(passive.hash(), original.hash(), "Seed {seed}");
    }
}

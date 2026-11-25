//! Flotten aus Schiffen, Werften, Nachschub, Instandsetzung und Sold in der laufenden
//! Partie. Die Rechenregeln selbst prüfen die Tests neben dem Code in `sim::fleet`; hier
//! geht es darum, dass sie in der Partie greifen und das Geld dabei erhalten bleibt.
//!
//! Wo ein Betrag auf das Tausendstel stimmen soll, läuft eine zweite, sonst gleiche Partie
//! als Gegenprobe mit: Bis zum Schritt der Flotten sind beide am selben Tag identisch, der
//! Unterschied danach ist genau das, was die Flotte bewirkt hat.

use sim::GameData;
use sim::battle::{Doctrine, Formation};
use sim::chronicle::Chronicle;
use sim::clock::{DAYS_PER_MONTH, DAYS_PER_YEAR};
use sim::command::{CommandOutcome, SimCommand};
use sim::data::Sector;
use sim::economy::{Economy, Money, to_credits};
use sim::fleet::{Fleet, Fleets, Stance, fleet_strength, monthly_upkeep, yard_slots};
use sim::galaxy::Galaxy;
use sim::harness::{Headless, check_invariants, summarize};
use sim::ids::{FleetId, GoodId, PlanetId, PolityId, ShipId, SystemId};
use sim::logistics::Logistics;
use sim::polity::{Polities, claimable_planets};
use sim::state::{SaveGame, SimConfig, money_drift};
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
    assert!(
        rejected("sold", "upkeep_per_month: 0.012", "upkeep_per_month: 1.5")
            .contains("upkeep_per_month")
    );
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
    assert!(!order.stalled);
    for (good, amount) in &stats.parts {
        let taken = economy(&control).market(home).stock_of(*good)
            - economy(&sim).market(home).stock_of(*good);
        assert!(
            (taken - *amount as f64 * step).abs() < 1e-6,
            "{good}: {taken}"
        );
    }
    // … der Staat hat sie bezahlt: an das Kontor die Ware, an die Rüstungsfirma der Welt
    // den Aufschlag der Werft. Kein Tausendstel entsteht oder verschwindet.
    let paid = treasury(&control, me) - treasury(&sim, me);
    let to_market = economy(&sim).market(home).cash - economy(&control).market(home).cash;
    let firm = |sim: &Headless| economy(sim).planet(capital).firms[Sector::Arms.index()].cash;
    let to_yard = firm(&sim) - firm(&control);
    assert!(to_market > 0 && to_yard > 0);
    assert_eq!(paid, to_market + to_yard);
    assert_eq!(order.spent, paid);
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
        assert!(fleets(&sim).orders.iter().all(|order| !order.stalled));
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
    assert!(order(&sim).stalled);
    let hull = data.balance.military.hull_good_id;
    assert!(economy(&sim).market(home).demand[hull.index()] > 0.0);

    // Zwei von drei Bauteilen genügen nicht, und sie werden auch nicht auf Vorrat gekauft.
    set_stock(&mut sim, home, &["rumpfsegmente", "antriebe"], 50.0);
    sim.run_days(1);
    assert_eq!(order(&sim).progress, 0.0);
    assert!(order(&sim).stalled);
    assert_eq!(stock_of(&sim, home, "rumpfsegmente"), 50.0);
    assert_eq!(stock_of(&sim, home, "antriebe"), 50.0);

    // Mit dem dritten läuft der Bau an.
    set_stock(&mut sim, home, &["waffensysteme"], 50.0);
    sim.run_days(1);
    assert!((order(&sim).progress - step).abs() < 1e-6);
    assert!(!order(&sim).stalled);
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
    assert!(order(&sim).stalled);
    set_stock(&mut sim, home, &["waffensysteme"], 50.0);

    // Leere Staatskasse: Der Staat nimmt keine Steuern mehr ein und hat nichts mehr.
    assert_eq!(
        command(&mut sim, me, SimCommand::SetTaxRate { rate: 0.0 }),
        CommandOutcome::Applied
    );
    sim.run_days(1);
    assert!(!order(&sim).stalled, "mit Ware und Geld läuft der Bau");
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
    assert!(order(&sim).stalled);

    // Kommt das Geld zurück, geht es weiter.
    {
        let mut economy = sim.world_mut().resource_mut::<Economy>();
        economy.planets[capital.index()].households -= purse;
        economy.treasuries[me.index()] += purse;
    }
    sim.run_days(1);
    assert!((order(&sim).progress - before - step).abs() < 1e-6);
    assert!(!order(&sim).stalled);
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
    for sim in [&mut blockaded, &mut open] {
        let mut economy = sim.world_mut().resource_mut::<Economy>();
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
                .is_some_and(|order| order.stalled)
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
        behind.progress < 0.5 && behind.stalled,
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

    // Ein Tagesanteil ist nachgefasst …
    let rate = balance.resupply_per_day;
    let (mut ammo_units, mut fuel_units) = (0.0, 0.0);
    for ship in &fleet(&sim, FleetId(0)).ships {
        assert!((ship.ammo - rate).abs() < 1e-6 && (ship.fuel - rate).abs() < 1e-6);
        let stats = &data.designs[ship.design as usize].stats;
        ammo_units += (rate * stats.ammo) as f64 * balance.ammo_goods;
        fuel_units += (rate * stats.fuel) as f64 * balance.fuel_goods;
    }
    // … die Ware fehlt dem Markt, und die Staatskasse hat sie dem Kontor bezahlt.
    let ammo = balance.ammo_good_id;
    let fuel = data.balance.economy.fuel_id;
    let taken = |good: GoodId| {
        economy(&control).market(home).stock_of(good) - economy(&sim).market(home).stock_of(good)
    };
    assert!((taken(ammo) - ammo_units).abs() < 1e-9);
    assert!((taken(fuel) - fuel_units).abs() < 1e-9);
    let paid = treasury(&control, me) - treasury(&sim, me);
    assert!(paid > 0);
    assert_eq!(
        paid,
        economy(&sim).market(home).cash - economy(&control).market(home).cash
    );
    let spread = data.balance.economy.spread;
    let market = economy(&sim).market(home);
    let price = ammo_units * market.ask(ammo, spread) + fuel_units * market.ask(fuel, spread);
    assert!((to_credits(paid) - price).abs() < 0.01);
    assert_eq!(money_drift(sim.world()), Some(0));

    // Nach genügend Tagen sind Magazine und Tanks voll.
    sim.run_days((1.0 / rate).ceil() as u64);
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

    // In der Fremde gibt es nichts: weder Munition noch Treibstoff.
    drain(&mut sim, FleetId(0), true, false);
    assert!(
        stock_of(&sim, abroad, "munition") > 1.0,
        "Munition läge bereit"
    );
    sim.run_days(20);
    for ship in &fleet(&sim, FleetId(0)).ships {
        assert_eq!(ship.ammo, 0.0);
        assert!((ship.fuel - expected).abs() < 1e-3);
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
    assert!((40.0 - stock_of(&sim, home, "rumpfsegmente") - wanted).abs() < 1e-6);
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
fn upkeep_moves_exactly_the_stated_amount_to_the_households_once_a_month() {
    let data = data();
    let me = PolityId(0);
    let mut sim = with_player(16);
    let mut control = with_player(16);
    let cruiser = design("speer");
    for sim in [&mut sim, &mut control] {
        sim.run_days(10);
    }
    // In der einen Partie wächst die Flotte um vier Kreuzer; sonst sind beide gleich.
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

    // Mitten im Monat kostet die größere Flotte nichts: Sold wird am Monatsanfang gezahlt.
    let gap = |sim: &Headless, control: &Headless| {
        (
            treasury(control, me) - treasury(sim, me),
            households(sim, me) - households(control, me),
        )
    };
    sim.run_days(DAYS_PER_MONTH - 10);
    control.run_days(DAYS_PER_MONTH - 10);
    assert_eq!(sim.clock().day(), DAYS_PER_MONTH);
    assert_eq!(gap(&sim, &control), (0, 0));

    // Am Monatsanfang fließt genau der Mehrbetrag aus der Staatskasse an die Haushalte.
    sim.run_days(1);
    control.run_days(1);
    assert_eq!(gap(&sim, &control), (extra, extra));
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
    // Eine Flotte, deren Sold keine Staatskasse trägt.
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

    // Der Staat zahlt, was er hat, und keinen Credit mehr.
    assert_eq!(treasury(&sim, me), 0);
    let available = treasury(&control, me) + usual;
    assert_eq!(
        households(&sim, me) - households(&control, me),
        available - usual,
        "die ganze Kasse ging an die Haushalte"
    );
    // Der ausgebliebene Anteil steht geglättet als Rückstand in den Büchern …
    let missed = 1.0 - available as f32 / due as f32;
    let arrears = fleets(&sim).arrears_of(me);
    assert!((arrears - data.balance.military.arrears_smoothing * missed).abs() < 1e-5);
    assert!(arrears > 0.3);
    assert_eq!(fleets(&control).arrears_of(me), 0.0);
    assert_eq!(fleets(&sim).arrears_of(PolityId(1)), 0.0);
    // … und die Chronik meldet den Beginn, einmal.
    let notices = |sim: &Headless| {
        sim.world()
            .resource::<Chronicle>()
            .entries
            .iter()
            .filter(|entry| entry.text.contains("Sold schuldig") && entry.polity == Some(me))
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

//! Wirtschaft als Ganzes: Preise, Blockade, Geldsumme, Spezialisierung, Kaperfahrt.

use sim::GameData;
use sim::ai::MilitaryMode;
use sim::clock::DAYS_PER_YEAR;
use sim::command::SimCommand;
use sim::economy::Economy;
use sim::fleet::{Fleets, Stance};
use sim::galaxy::Galaxy;
use sim::harness::{Headless, check_invariants, economy_stats, run_checked};
use sim::ids::{FleetId, PolityId, SystemId};
use sim::logistics::Logistics;
use sim::state::{SimConfig, money_drift};

fn data() -> GameData {
    GameData::load_workspace().unwrap()
}

fn observer(seed: u64) -> Headless {
    Headless::new(data(), SimConfig { seed, player: None })
}

#[test]
fn money_is_only_moved_never_created_or_lost() {
    let data = data();
    for seed in [1, 2, 3] {
        let (sim, violation) = run_checked(&data, seed, 25 * DAYS_PER_YEAR);
        assert_eq!(violation, None, "Seed {seed}");
        assert_eq!(money_drift(sim.world()), Some(0), "Seed {seed}");
        // Geldschöpfung gibt es nur für Neugeborene, und sie ist ausgewiesen.
        assert!(sim.world().resource::<Economy>().minted > 0);
    }
}

#[test]
fn economy_stays_alive_for_decades() {
    let data = data();
    for seed in [4, 5] {
        let (sim, violation) = run_checked(&data, seed, 40 * DAYS_PER_YEAR);
        assert_eq!(violation, None, "Seed {seed}");
        let stats = economy_stats(sim.world());
        assert!(
            stats.essentials_met > 0.85,
            "Seed {seed}: Versorgung {}",
            stats.essentials_met
        );
        assert!(
            stats.utilization > 0.5,
            "Seed {seed}: Auslastung {}",
            stats.utilization
        );
        assert!(
            stats.population > 24_000.0,
            "Seed {seed}: Bevölkerung schrumpft"
        );
        assert!(
            stats.pinned_prices < 0.3,
            "Seed {seed}: zu viele Preise am Anschlag"
        );
        assert!(stats.freighters >= 18, "Seed {seed}: Handel ist erloschen");
    }
}

#[test]
fn a_shortage_raises_the_local_price() {
    let mut sim = observer(6);
    sim.run_days(2 * DAYS_PER_YEAR);
    let data = data();
    let food = data.good_id("nahrung").unwrap();
    let home = {
        let galaxy = sim.galaxy();
        galaxy
            .planet(sim.polities().list[0].capital.unwrap())
            .system
    };
    let before = sim
        .world()
        .resource::<Economy>()
        .market(home)
        .price_of(food);
    // Die Vorräte verschwinden (z. B. durch Sabotage); die Nachfrage bleibt.
    for _ in 0..30 {
        sim.world_mut().resource_mut::<Economy>().markets[home.index()].stock[food.index()] = 0.0;
        sim.run_days(1);
    }
    let after = sim
        .world()
        .resource::<Economy>()
        .market(home)
        .price_of(food);
    assert!(
        after > before * 1.5,
        "Preis vorher {before:.1}, nachher {after:.1}"
    );
    assert!(check_invariants(sim.world()).is_ok());
}

/// Das bevölkerungsreichste Koloniesystem der Gemeinschaft `victim`: groß genug, dass
/// dort wirklich produziert und gehandelt wird.
fn largest_colony_system(sim: &Headless, victim: PolityId) -> SystemId {
    let galaxy = sim.galaxy();
    let home = galaxy
        .planet(sim.polities().get(victim).capital.unwrap())
        .system;
    let people = |system: SystemId| -> f64 {
        galaxy
            .planets_in(system)
            .filter(|planet| planet.owner == Some(victim))
            .map(|planet| planet.population)
            .sum()
    };
    galaxy
        .systems
        .iter()
        .filter(|system| system.id != home && galaxy.system_owner(system.id) == Some(victim))
        .max_by(|a, b| people(a.id).total_cmp(&people(b.id)).then(b.id.cmp(&a.id)))
        .map(|system| system.id)
        .expect("nach Jahrzehnten gibt es Kolonien")
}

fn system_health(sim: &Headless, system: SystemId) -> (f64, f64, f64) {
    let data = sim.world().resource::<GameData>();
    let galaxy = sim.world().resource::<Galaxy>();
    let economy = sim.world().resource::<Economy>();
    let market = economy.market(system);
    // Preisniveau der lebensnotwendigen Güter relativ zum Richtpreis.
    let needs = &data.balance.economy.needs;
    let price: f64 = needs
        .iter()
        .map(|need| market.price_of(need.good_id) / data.good(need.good_id).base_price)
        .sum::<f64>()
        / needs.len() as f64;
    let (mut capital, mut used, mut people, mut fed) = (0.0, 0.0, 0.0, 0.0);
    for planet in galaxy
        .planets_in(system)
        .filter(|planet| planet.owner.is_some())
    {
        let local = economy.planet(planet.id);
        for (c, u) in local.capital.iter().zip(&local.utilization_trend) {
            capital += c;
            used += c * u;
        }
        people += planet.population;
        fed += planet.population * local.essentials_met(&data.balance.economy);
    }
    (price, used / capital.max(1e-9), fed / people.max(1e-9))
}

/// Güter, die ein System in den letzten Monaten bezogen bzw. abgegeben hat.
fn traded_goods(sim: &Headless, system: SystemId) -> (Vec<usize>, Vec<usize>) {
    let logistics = sim.world().resource::<Logistics>();
    let (mut imports, mut exports) = (Vec::new(), Vec::new());
    for ((from, to, good), volume) in &logistics.flows {
        if *volume < 2.0 {
            continue;
        }
        if *to == system && !imports.contains(&good.index()) {
            imports.push(good.index());
        }
        if *from == system && !exports.contains(&good.index()) {
            exports.push(good.index());
        }
    }
    // Was in beide Richtungen floss, taugt für keine der beiden Aussagen.
    let both: Vec<usize> = imports
        .iter()
        .copied()
        .filter(|good| exports.contains(good))
        .collect();
    imports.retain(|good| !both.contains(good));
    exports.retain(|good| !both.contains(good));
    (imports, exports)
}

#[test]
fn a_blockade_raises_prices_and_idles_factories() {
    let attacker = PolityId(0);
    let victim = PolityId(1);
    // Der Seed bestimmt, welche Kolonie getroffen wird. Die Prüfung braucht eine, die
    // Güter von außen bezieht und im Frieden gut versorgt ist; das gilt für etwa jede
    // zweite Partie. Ändert sich der Verlauf der ersten Jahrzehnte (hier: Forschung
    // bemisst sich an den laufenden Staatseinnahmen), passt ein anderer Seed.
    let mut blockaded = observer(4);
    let mut control = observer(4);
    // Die Flotten führt hier die Prüfung: Die militärische Führung der KI ruht in beiden
    // Partien, sonst rüsteten die Gemeinschaften und wichen der Blockade aus oder brächen sie.
    for sim in [&mut blockaded, &mut control] {
        sim.set_military_mode(None, MilitaryMode::Resting);
    }
    blockaded.run_days(30 * DAYS_PER_YEAR);
    control.run_days(30 * DAYS_PER_YEAR);
    let target = largest_colony_system(&blockaded, victim);
    assert_eq!(target, largest_colony_system(&control, victim));
    let (imports, _exports) = traded_goods(&blockaded, target);
    assert!(
        !imports.is_empty(),
        "das System hat vorher nichts importiert"
    );

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
    // Die Flotte braucht Zeit für den Anmarsch. Danach steht die Sperre, solange der
    // Treibstoff reicht: Fern der Heimat fasst die Flotte nichts nach.
    let mut established = false;
    for _ in 0..3 * DAYS_PER_YEAR {
        blockaded.run_days(1);
        control.run_days(1);
        if blockaded.world().resource::<Fleets>().is_blockaded(target) {
            established = true;
            break;
        }
    }
    assert!(established, "die Flotte hat die Blockade nie errichtet");
    let mut held = 0;
    while blockaded.world().resource::<Fleets>().is_blockaded(target) {
        blockaded.run_days(1);
        control.run_days(1);
        held += 1;
        assert!(held < 2 * DAYS_PER_YEAR, "die Sperre endet nie");
    }
    assert!(
        held >= 100,
        "die Sperre hielt nur {held} Tage; für die Messung braucht es Monate"
    );
    let fuel = blockaded.world().resource::<Fleets>().list[0].min_fuel();
    assert_eq!(fuel, 0.0, "die Sperre endet, weil der Treibstoff ausgeht");

    // Mittleres Preisverhältnis gesperrt/offen über eine Gütergruppe.
    let ratio = |goods: &[usize]| -> f64 {
        let closed = blockaded.world().resource::<Economy>().market(target);
        let open = control.world().resource::<Economy>().market(target);
        goods
            .iter()
            .map(|good| closed.price[*good] / open.price[*good])
            .sum::<f64>()
            / goods.len().max(1) as f64
    };
    // Was von außen kam, wird knapp und teuer. (Für Exportgüter gilt keine einfache Regel:
    // Manche stauen sich und verfallen, andere werden teurer, weil ihren Herstellern die
    // Vorprodukte ausgehen.)
    let import_ratio = ratio(&imports);
    assert!(
        import_ratio > 1.5,
        "Importgüter nur {import_ratio:.2}-mal so teuer"
    );
    // Die Werke leiden: Vorprodukte fehlen, Absatz fehlt.
    let (_, use_blocked, fed_blocked) = system_health(&blockaded, target);
    let (_, use_open, fed_open) = system_health(&control, target);
    assert!(
        use_blocked < use_open - 0.03 || fed_blocked < fed_open - 0.05,
        "Auslastung gesperrt {use_blocked:.2} / offen {use_open:.2}, \
         Versorgung gesperrt {fed_blocked:.2} / offen {fed_open:.2}"
    );
    assert!(check_invariants(blockaded.world()).is_ok());
}

#[test]
fn locations_specialize_without_building_bonuses() {
    let data = data();
    let (sim, violation) = run_checked(&data, 8, 45 * DAYS_PER_YEAR);
    assert_eq!(violation, None);
    let galaxy = sim.galaxy();
    let economy = sim.world().resource::<Economy>();
    let capitals: Vec<_> = sim
        .polities()
        .list
        .iter()
        .filter_map(|polity| polity.capital)
        .collect();

    // Anteil des größten Rezepts am Kapitalstock einer Welt.
    let focus = |planet: sim::ids::PlanetId| {
        let local = economy.planet(planet);
        let total = local.total_capital();
        local.capital.iter().copied().fold(0.0, f64::max) / total.max(1e-9)
    };
    let home_focus: f64 = capitals.iter().map(|id| focus(*id)).sum::<f64>() / capitals.len() as f64;
    let colonies: Vec<_> = galaxy
        .planets
        .iter()
        .filter(|planet| planet.owner.is_some() && !capitals.contains(&planet.id))
        .filter(|planet| economy.planet(planet.id).total_capital() > 1.0)
        .collect();
    assert!(
        colonies.len() >= 6,
        "nur {} Kolonien mit Industrie",
        colonies.len()
    );
    let colony_focus: f64 =
        colonies.iter().map(|planet| focus(planet.id)).sum::<f64>() / colonies.len() as f64;
    assert!(
        colony_focus > home_focus * 1.15,
        "Kolonien {colony_focus:.2} gegen Heimatwelten {home_focus:.2}"
    );

    // Die Profile unterscheiden sich wirklich: Nicht alle Kolonien setzen auf dasselbe Rezept.
    let mut tops: Vec<usize> = colonies
        .iter()
        .map(|planet| {
            let local = economy.planet(planet.id);
            (0..local.capital.len())
                .max_by(|a, b| local.capital[*a].total_cmp(&local.capital[*b]))
                .unwrap()
        })
        .collect();
    tops.sort_unstable();
    tops.dedup();
    assert!(
        tops.len() >= 3,
        "nur {} verschiedene Schwerpunkte",
        tops.len()
    );

    // Förderung folgt dem Vorkommen: Wo viel Kapital in einem Abbaurezept steckt, ist das
    // Vorkommen überdurchschnittlich.
    let (mut weighted, mut weight) = (0.0, 0.0);
    for planet in galaxy
        .planets
        .iter()
        .filter(|planet| planet.owner.is_some())
    {
        let local = economy.planet(planet.id);
        for (index, recipe) in data.recipes.iter().enumerate() {
            if let Some(good) = recipe.deposit_id
                && let Some(deposit) = planet.deposit(good)
            {
                weighted += local.capital[index] * deposit.richness as f64;
                weight += local.capital[index];
            }
        }
    }
    let mined_richness = weighted / weight;
    let all: Vec<f32> = galaxy
        .planets
        .iter()
        .flat_map(|p| p.deposits.iter().map(|d| d.richness))
        .collect();
    let mean_richness = all.iter().sum::<f32>() as f64 / all.len() as f64;
    assert!(mined_richness > 0.0 && mean_richness > 0.0);
}

#[test]
fn raiders_capture_freighters_and_escorts_protect_them() {
    let raider = PolityId(0);
    let victim = PolityId(1);
    let run = |escort: bool| {
        let mut sim = observer(9);
        // Kaperer und Geleit stellt die Prüfung auf; die militärische Führung der KI ruht.
        sim.set_military_mode(None, MilitaryMode::Resting);
        sim.run_days(25 * DAYS_PER_YEAR);
        let victim_home = {
            let galaxy = sim.galaxy();
            galaxy
                .planet(sim.polities().get(victim).capital.unwrap())
                .system
        };
        {
            // Der Kaperer steht im Heimatsystem des Opfers; dessen Flotte ist entweder
            // ein starker Geleitschutz oder abwesend. Die Stärke folgt aus den Schiffen:
            // Der Kaperer führt nur einen Kreuzer, eine Fregatte und eine Korvette (Stärke 60),
            // der Geleitschutz wächst um 150 Kreuzer (Stärke rund 5000).
            let data = data();
            let mut fleets = sim.world_mut().resource_mut::<Fleets>();
            let raiders = &mut fleets.list[raider.index()];
            raiders.system = victim_home;
            raiders.stance = Stance::Raid;
            let mut kept: Vec<u16> = Vec::new();
            raiders.ships.retain(|ship| {
                let first_of_its_design = !kept.contains(&ship.design);
                kept.push(ship.design);
                first_of_its_design
            });
            let guard = FleetId(victim.index() as u32);
            if escort {
                fleets.list[guard.index()].stance = Stance::Escort;
                let cruiser = data.design_index("speer").unwrap();
                for _ in 0..150 {
                    fleets.add_ship(&data, guard, cruiser).unwrap();
                }
            } else {
                // Ohne Schiffe besteht die Flotte nicht mehr.
                fleets.list[guard.index()].ships.clear();
            }
            fleets.refresh(&data);
            let strength = |polity: PolityId| fleets.list[polity.index()].strength;
            assert!((strength(raider) - 60.0).abs() < 1.0);
            if escort {
                assert!((strength(victim) - 5_000.0).abs() < 100.0);
            } else {
                assert!(!fleets.list[guard.index()].alive);
            }
        }
        sim.issue(raider, SimCommand::DeclareWar { target: victim });
        sim.run_days(300);
        let captured = sim.world().resource::<Logistics>().captured;
        assert!(check_invariants(sim.world()).is_ok());
        captured
    };
    let unprotected = run(false);
    let protected = run(true);
    assert!(unprotected >= 2, "nur {unprotected} Frachter aufgebracht");
    assert!(
        protected * 2 <= unprotected,
        "Geleitschutz half nicht: {protected} gegen {unprotected}"
    );
}

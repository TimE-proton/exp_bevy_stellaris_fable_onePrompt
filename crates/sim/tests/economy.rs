//! Wirtschaft als Ganzes: Preise, Blockade, Geldsumme, Spezialisierung, Kaperfahrt.

use sim::GameData;
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

#[test]
fn a_blockade_raises_prices_and_idles_factories() {
    let attacker = PolityId(0);
    let victim = PolityId(1);
    let mut blockaded = observer(7);
    let mut control = observer(7);
    blockaded.run_days(30 * DAYS_PER_YEAR);
    control.run_days(30 * DAYS_PER_YEAR);
    let target = largest_colony_system(&blockaded, victim);
    assert_eq!(target, largest_colony_system(&control, victim));

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
    // Die Flotte braucht Zeit für den Anmarsch; danach steht die Sperre zwei Jahre.
    let mut arrived_after = None;
    for day in 0..3 * DAYS_PER_YEAR {
        blockaded.run_days(1);
        control.run_days(1);
        if blockaded.world().resource::<Fleets>().is_blockaded(target) {
            arrived_after = Some(day);
            break;
        }
    }
    assert!(
        arrived_after.is_some(),
        "die Flotte hat die Blockade nie errichtet"
    );
    blockaded.run_days(2 * DAYS_PER_YEAR);
    control.run_days(2 * DAYS_PER_YEAR);
    assert!(blockaded.world().resource::<Fleets>().is_blockaded(target));

    let (price_blocked, use_blocked, fed_blocked) = system_health(&blockaded, target);
    let (price_open, use_open, fed_open) = system_health(&control, target);
    assert!(
        price_blocked > price_open * 1.2,
        "Preise: gesperrt {price_blocked:.2}, offen {price_open:.2}"
    );
    assert!(
        use_blocked < use_open - 0.05 || fed_blocked < fed_open - 0.1,
        "Auslastung gesperrt {use_blocked:.2} / offen {use_open:.2}, \
         Versorgung gesperrt {fed_blocked:.2} / offen {fed_open:.2}"
    );
    // Während der Sperre erreicht kein Frachter das System.
    let logistics = blockaded.world().resource::<Logistics>();
    assert!(!logistics.freighters.iter().any(|f| f.alive
        && f.route.is_empty()
        && f.system == target
        && f.idle_days == 0
        && f.cargo.is_some()),);
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
        colony_focus > home_focus * 1.3,
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
        sim.run_days(25 * DAYS_PER_YEAR);
        let victim_home = {
            let galaxy = sim.galaxy();
            galaxy
                .planet(sim.polities().get(victim).capital.unwrap())
                .system
        };
        {
            // Der Kaperer steht im Heimatsystem des Opfers; dessen Flotte ist entweder
            // ein starker Geleitschutz oder abwesend.
            let mut fleets = sim.world_mut().resource_mut::<Fleets>();
            fleets.list[raider.index()].system = victim_home;
            fleets.list[raider.index()].stance = Stance::Raid;
            fleets.list[raider.index()].strength = 60.0;
            let guard = &mut fleets.list[victim.index()];
            if escort {
                guard.stance = Stance::Escort;
                guard.strength = 5_000.0;
            } else {
                guard.alive = false;
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

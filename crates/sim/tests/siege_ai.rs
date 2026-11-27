//! Belagerung und Verteidigung durch die KI in gestellten Lagen einer echten Partie: Was
//! die Führung täglich befiehlt, wenn eine eigene Flotte belagert und im Gefecht
//! unterliegt, wenn eine eigene Welt belagert wird, wenn Entsatz möglich ist und wenn
//! nach einem Feldzug Truppen an Bord bleiben.
//!
//! Jede Lage entsteht auf einem echten Seed nach den ersten Jahren der Partie. Die Seite,
//! deren Befehle der Test prüft, führt die KI; die andere führt der Spieler, damit sie
//! nur tut, was der Test ihr befiehlt.

use sim::GameData;
use sim::TICKS_PER_DAY;
use sim::ai::AiState;
use sim::clock::DAYS_PER_YEAR;
use sim::command::{CommandOutcome, CommandQueue, CommandSource, SimCommand};
use sim::fleet::{Fleets, Stance};
use sim::harness::{Headless, nearest_neighbour};
use sim::ids::{FleetId, PlanetId, PolityId, SystemId};
use sim::siege::{Embarked, Posture, Sieges, orbit};
use sim::state::SimConfig;
use std::sync::OnceLock;

const SEED: u64 = 3;

fn data() -> &'static GameData {
    static DATA: OnceLock<GameData> = OnceLock::new();
    DATA.get_or_init(|| GameData::load_workspace().unwrap())
}

/// Eine Partie nach zwei Jahren; `player` führt der Mensch.
fn game(player: Option<PolityId>) -> Headless {
    let mut sim = Headless::new(data().clone(), SimConfig { seed: SEED, player });
    sim.run_days(2 * DAYS_PER_YEAR);
    sim
}

fn applied(sim: &mut Headless, polity: PolityId, command: SimCommand) {
    sim.issue(polity, command.clone());
    sim.tick();
    assert_eq!(
        sim.command_log().entries.last().unwrap().outcome,
        CommandOutcome::Applied,
        "{command:?}"
    );
}

fn fleets(sim: &Headless) -> &Fleets {
    sim.world().resource::<Fleets>()
}

fn sieges(sim: &Headless) -> &Sieges {
    sim.world().resource::<Sieges>()
}

/// Die erste Flotte einer Gemeinschaft.
fn fleet_of(sim: &Headless, polity: PolityId) -> FleetId {
    fleets(sim).of(polity).next().expect("eine Flotte").id
}

fn home(sim: &Headless, polity: PolityId) -> (PlanetId, SystemId) {
    let capital = sim.polities().get(polity).capital.unwrap();
    (capital, sim.galaxy().planet(capital).system)
}

/// Die volkreichste Welt einer Gemeinschaft außer ihrer Hauptwelt.
fn colony_of(sim: &Headless, polity: PolityId) -> PlanetId {
    let capital = home(sim, polity).0;
    sim.galaxy()
        .planets_of(polity)
        .filter(|planet| {
            planet.id != capital && planet.system != sim.galaxy().planet(capital).system
        })
        .max_by(|a, b| a.population.total_cmp(&b.population))
        .expect("eine Kolonie")
        .id
}

/// Versetzt eine Flotte ohne Marsch in ein System (Aufbau der Lage).
fn place(sim: &mut Headless, id: FleetId, system: SystemId) {
    let mut fleets = sim.world_mut().resource_mut::<Fleets>();
    let fleet = &mut fleets.list[id.index()];
    fleet.system = system;
    fleet.route.clear();
    fleet.progress = 0.0;
}

/// Lässt einer Flotte nur ihr erstes Schiff (Aufbau der Lage).
fn shrink(sim: &mut Headless, id: FleetId) {
    let mut fleets = sim.world_mut().resource_mut::<Fleets>();
    fleets.list[id.index()].ships.truncate(1);
    let data = data().clone();
    fleets.refresh(&data);
}

/// Lässt die Partie `days` Tage laufen und sammelt die Befehle, die die KI der
/// Gemeinschaft `polity` dabei erteilt. Das Protokoll führt KI-Befehle nicht; sie stehen
/// nach jedem Takt in der Warteschlange und werden im nächsten angewandt.
fn watch(sim: &mut Headless, polity: PolityId, days: u64) -> Vec<SimCommand> {
    let mut seen = Vec::new();
    for _ in 0..days * TICKS_PER_DAY {
        sim.tick();
        seen.extend(
            sim.world()
                .resource::<CommandQueue>()
                .pending
                .iter()
                .filter(|queued| queued.polity == polity && queued.source == CommandSource::Ai)
                .map(|queued| queued.command.clone()),
        );
    }
    seen
}

/// Der Spieler (`attacker`) erklärt dem Nachbarn den Krieg und belagert mit seiner
/// Flotte dessen Kolonie. Gibt Nachbar, Kolonie und Flotte zurück.
fn besiege_a_colony(sim: &mut Headless, attacker: PolityId) -> (PolityId, PlanetId, FleetId) {
    let defender = nearest_neighbour(sim.galaxy(), sim.polities(), attacker).unwrap();
    let colony = colony_of(sim, defender);
    let fleet = fleet_of(sim, attacker);
    applied(sim, attacker, SimCommand::DeclareWar { target: defender });
    place(sim, fleet, sim.galaxy().planet(colony).system);
    applied(
        sim,
        attacker,
        SimCommand::SetStance {
            fleet,
            stance: Stance::Blockade,
        },
    );
    applied(
        sim,
        attacker,
        SimCommand::BeginSiege {
            fleet,
            planet: colony,
        },
    );
    (defender, colony, fleet)
}

#[test]
fn a_besieged_world_of_the_ai_saves_its_stores_and_its_fleet_comes_to_the_relief() {
    let me = PolityId(0);
    let mut sim = game(Some(me));
    let (defender, colony, besieger) = besiege_a_colony(&mut sim, me);
    // Der Belagerer ist schwach: ein Schiff. Die Kolonie hat Geschütze und knappe Vorräte.
    shrink(&mut sim, besieger);
    {
        let mut state = sim.world_mut().resource_mut::<Sieges>();
        let defense = state.defense_mut(colony);
        defense.guns = 1.0;
        defense.depot = 5.0;
    }
    // Die Flotte des Verteidigers liegt in seinem Heimatsystem; dort steht kein Gegner.
    let relief = fleet_of(&sim, defender);
    let seat = home(&sim, defender).1;
    place(&mut sim, relief, seat);
    let orders = watch(&mut sim, defender, 3);
    assert!(sieges(&sim).siege(colony).is_some(), "die Belagerung läuft");
    // Unter `hold_fire_days` Vorrat schweigen die Geschütze.
    assert!(
        orders.iter().any(|command| matches!(
            command,
            SimCommand::SetGarrisonOrders {
                planet,
                posture: Posture::Forward,
                hold_fire: true,
            } if *planet == colony
        )),
        "keine Befehle an die Garnison: {orders:?}"
    );
    // Der Entsatz: Die Flotte des Verteidigers ist dem Belagerer gewachsen und läuft zur
    // belagerten Welt.
    let target = sim.galaxy().planet(colony).system;
    assert!(
        orders.iter().any(|command| matches!(
            command,
            SimCommand::MoveFleet { fleet, to } if *fleet == relief && *to == target
        )),
        "kein Entsatz: {orders:?}"
    );
}

#[test]
fn the_relief_goes_to_the_besieged_world_before_a_more_populous_one() {
    // Zwei Eindringlinge im Gebiet des Verteidigers: einer an der Hauptwelt, einer über
    // einer kleineren Kolonie, die er belagert. Der Entsatz geht zur belagerten Welt.
    let me = PolityId(0);
    let mut sim = game(Some(me));
    let (defender, colony, besieger) = besiege_a_colony(&mut sim, me);
    shrink(&mut sim, besieger);
    sim.run_days(1);
    assert!(sieges(&sim).siege(colony).is_some(), "die Belagerung läuft");
    let seat = home(&sim, defender).1;
    let polities = sim.polities().clone();
    let raider = {
        let mut state = sim.world_mut().resource_mut::<Fleets>();
        let raider = state.new_fleet(&polities, me, seat);
        let design = data()
            .designs
            .iter()
            .position(|design| design.key == "dolch")
            .unwrap() as u16;
        state.add_ship(data(), raider, design).unwrap();
        raider
    };
    assert!(fleets(&sim).get(raider).is_some());
    // Die Flotte des Verteidigers liegt in einem eigenen System, das weder die Hauptwelt
    // noch die Kolonie hält.
    let relief = fleet_of(&sim, defender);
    let target = sim.galaxy().planet(colony).system;
    let elsewhere = sim
        .galaxy()
        .systems
        .iter()
        .map(|system| system.id)
        .find(|system| {
            *system != seat
                && *system != target
                && sim.galaxy().system_owner(*system) == Some(defender)
        })
        .expect("ein drittes eigenes System");
    place(&mut sim, relief, elsewhere);
    let orders = watch(&mut sim, defender, 1);
    let moved: Vec<SystemId> = orders
        .iter()
        .filter_map(|command| match command {
            SimCommand::MoveFleet { fleet, to } if *fleet == relief => Some(*to),
            _ => None,
        })
        .collect();
    assert_eq!(
        moved,
        vec![target],
        "Hauptwelt {seat:?}, Kolonie {target:?}, Flotte in {elsewhere:?}: {orders:?}"
    );
}

#[test]
fn a_besieging_fleet_of_the_ai_retreats_from_a_battle_it_is_losing() {
    // Die KI belagert eine Kolonie; im Orbit erscheint eine weit überlegene Flotte des
    // Verteidigers.
    let mut sim = game(None);
    let defender = {
        sim.polities()
            .alive()
            .map(|polity| polity.id)
            .find(|polity| {
                let capital = home(&sim, *polity).1;
                sim.galaxy()
                    .planets_of(*polity)
                    .any(|planet| planet.system != capital)
            })
            .expect("eine Gemeinschaft mit Kolonie")
    };
    let attacker = nearest_neighbour(sim.galaxy(), sim.polities(), defender).unwrap();
    let colony = colony_of(&sim, defender);
    let fleet = fleet_of(&sim, attacker);
    let system = sim.galaxy().planet(colony).system;
    applied(
        &mut sim,
        attacker,
        SimCommand::DeclareWar { target: defender },
    );
    place(&mut sim, fleet, system);
    applied(
        &mut sim,
        attacker,
        SimCommand::BeginSiege {
            fleet,
            planet: colony,
        },
    );
    assert!(sim.world().resource::<AiState>().is_reserved(fleet));
    shrink(&mut sim, fleet);
    let polities = sim.polities().clone();
    {
        let mut state = sim.world_mut().resource_mut::<Fleets>();
        let wall = state.new_fleet(&polities, defender, system);
        let design = data()
            .designs
            .iter()
            .position(|design| design.key == "speer")
            .unwrap() as u16;
        for _ in 0..6 {
            state.add_ship(data(), wall, design).unwrap();
        }
    }
    let orders = watch(&mut sim, attacker, 3);
    assert!(
        orders
            .iter()
            .any(|command| matches!(command, SimCommand::Retreat { fleet: id } if *id == fleet)),
        "kein Rückzug: {orders:?}"
    );
}

/// Eine Gemeinschaft außer `not`, deren erste Flotte Plätze für Truppen hat.
fn with_berths(sim: &Headless, not: PolityId) -> PolityId {
    sim.polities()
        .alive()
        .map(|polity| polity.id)
        .find(|polity| {
            *polity != not
                && fleets(sim)
                    .of(*polity)
                    .next()
                    .is_some_and(|fleet| orbit::berths_of(data(), fleet) >= 2.0)
        })
        .expect("eine Flotte mit Plätzen für Truppen")
}

/// Truppen an Bord der ersten Flotte einer KI-Gemeinschaft, die in ihrem Heimatsystem
/// liegt; so viele, wie Platz haben, höchstens sechs Verbände.
fn troops_aboard(sim: &mut Headless, polity: PolityId) -> f64 {
    let fleet = fleet_of(sim, polity);
    place(sim, fleet, home(sim, polity).1);
    let troops = orbit::berths_of(data(), fleets(sim).get(fleet).unwrap()).min(6.0);
    sim.world_mut().resource_mut::<Sieges>().embarked.insert(
        fleet,
        Embarked {
            troops,
            supply: 0.0,
        },
    );
    troops
}

/// Truppen an Bord aller Flotten einer Gemeinschaft (Flotten vereinigen sich).
fn aboard(sim: &Headless, polity: PolityId) -> f64 {
    fleets(sim)
        .of(polity)
        .filter_map(|fleet| sieges(sim).embarked.get(&fleet.id))
        .map(|force| force.troops)
        .sum()
}

#[test]
fn troops_left_aboard_go_ashore_in_peace_at_once_and_in_war_after_some_days() {
    // Frieden: am nächsten Tag an Land.
    let player = PolityId(0);
    let mut sim = game(None);
    let polity = with_berths(&sim, player);
    let troops = troops_aboard(&mut sim, polity);
    assert!(troops >= 2.0);
    assert_eq!(aboard(&sim, polity), troops);
    let orders = watch(&mut sim, polity, 2);
    assert!(
        orders
            .iter()
            .any(|command| matches!(command, SimCommand::LandTroops { .. })),
        "{orders:?}"
    );
    assert_eq!(aboard(&sim, polity), 0.0);

    // Krieg ohne Feldzug: Die Truppen warten land_after_days auf einen neuen Feldzug und
    // gehen dann an Land.
    let wait = data().balance.military.ai.siege.land_after_days;
    let mut sim = game(Some(player));
    let foe = nearest_neighbour(sim.galaxy(), sim.polities(), polity).unwrap();
    let troops = troops_aboard(&mut sim, polity);
    {
        let day = sim.clock().day();
        let mut ai = sim.world_mut().resource_mut::<AiState>();
        let plan = ai.plan_mut(polity);
        // Kein neuer Feldzug, kein Angriff; der letzte Feldzug endete heute.
        plan.no_offensive_before = u64::MAX;
        plan.campaign_end = day;
    }
    applied(&mut sim, polity, SimCommand::DeclareWar { target: foe });
    for day in 0..wait - 2 {
        let orders = watch(&mut sim, polity, 1);
        assert_eq!(
            aboard(&sim, polity),
            troops,
            "zu früh an Land, Tag {day}: {orders:?}"
        );
    }
    for _ in 0..60 {
        if aboard(&sim, polity) == 0.0 {
            break;
        }
        sim.run_days(1);
    }
    assert_eq!(aboard(&sim, polity), 0.0, "die Truppen bleiben an Bord");
}

//! Belagerung in der laufenden Partie: aushungern, Stationen, Entsatz, Frieden, Befehle
//! und Wiederholbarkeit. Die Rechenregeln selbst prüfen die Tests neben dem Code in
//! `sim::siege`; hier geht es darum, dass sie in ganzen Partien greifen.
//!
//! Gespielt wird auf echten Seeds und nur über Befehle: Krieg erklären, Truppen
//! einschiffen, marschieren, belagern. Die eine Ausnahme ist die Station im Test dazu:
//! Anlagen lassen sich in der Partie noch nicht bauen, sie wird als Ausgangslage gestellt.
//!
//! Wer einen Verlauf sehen will: `cargo test -p sim --test siege -- --nocapture` druckt
//! Tagesberichte.

use sim::GameData;
use sim::clock::DAYS_PER_YEAR;
use sim::command::{CommandOutcome, SimCommand};
use sim::diplomacy::Diplomacy;
use sim::economy::Economy;
use sim::finance::{Finance, Stage, ownership_of_region};
use sim::fleet::{Battles, Fleet, Fleets, Stance};
use sim::harness::{Headless, check_invariants};
use sim::ids::{FleetId, HolderId, IssuerId, PlanetId, PolityId, ShipId, SystemId};
use sim::logistics::Logistics;
use sim::siege::{
    Blocked, Course, Fire, Limit, Outcome, Phase, PlanetDefense, Posture, Siege, SiegeOrders,
    SiegeView, Sieges, firm_value, forecast, occupation_force, orbit, planet_facts, will,
};
use sim::state::{SaveGame, SimConfig, money_drift};
use sim::tech::Research;
use std::sync::OnceLock;

/// Seed aller Partien hier; derselbe, an dem das Headless-Werkzeug die Prüfsummen zeigt.
const SEED: u64 = 3;
/// So viele Jahre läuft die Partie, bevor der Krieg beginnt: Dann haben die
/// Gemeinschaften ihre ersten Kolonien gegründet.
const YEARS: u64 = 2;
/// Verbände, die der Belagerer mitnimmt.
const TROOPS: f64 = 8.0;

fn data() -> &'static GameData {
    static DATA: OnceLock<GameData> = OnceLock::new();
    DATA.get_or_init(|| GameData::load_workspace().unwrap())
}

fn observer() -> Headless {
    Headless::new(
        data().clone(),
        SimConfig {
            seed: SEED,
            player: None,
        },
    )
}

fn sieges(sim: &Headless) -> &Sieges {
    sim.world().resource::<Sieges>()
}

fn fleets(sim: &Headless) -> &Fleets {
    sim.world().resource::<Fleets>()
}

fn fleet(sim: &Headless, id: FleetId) -> &Fleet {
    &fleets(sim).list[id.index()]
}

fn finance(sim: &Headless) -> &Finance {
    sim.world().resource::<Finance>()
}

fn defense(sim: &Headless, planet: PlanetId) -> &PlanetDefense {
    sieges(sim).defense(planet)
}

fn owner(sim: &Headless, planet: PlanetId) -> Option<PolityId> {
    sim.galaxy().planet(planet).owner
}

fn home_of(sim: &Headless, polity: PolityId) -> (PlanetId, SystemId) {
    let capital = sim.polities().get(polity).capital.unwrap();
    (capital, sim.galaxy().planet(capital).system)
}

fn fleet_of(sim: &Headless, polity: PolityId) -> FleetId {
    fleets(sim).of(polity).next().unwrap().id
}

/// Gibt einen Befehl, lässt ihn im nächsten Takt anwenden und liefert das Ergebnis.
fn command(sim: &mut Headless, polity: PolityId, command: SimCommand) -> CommandOutcome {
    sim.issue(polity, command);
    sim.tick();
    sim.command_log().entries.last().unwrap().outcome.clone()
}

fn applied(sim: &mut Headless, polity: PolityId, issued: SimCommand) {
    let outcome = command(sim, polity, issued.clone());
    assert_eq!(outcome, CommandOutcome::Applied, "{issued:?}");
}

/// Grund, aus dem ein Befehl abgelehnt wurde.
fn rejection(sim: &mut Headless, polity: PolityId, issued: SimCommand) -> String {
    match command(sim, polity, issued.clone()) {
        CommandOutcome::Rejected(reason) => reason,
        CommandOutcome::Applied => panic!("{issued:?} hätte abgelehnt werden müssen"),
    }
}

/// Lässt die Partie laufen, bis die Flotte im Zielsystem liegt.
fn sail(sim: &mut Headless, id: FleetId, to: SystemId) -> u64 {
    for day in 0..DAYS_PER_YEAR {
        let fleet = fleet(sim, id);
        if fleet.system == to && fleet.route.is_empty() {
            return day;
        }
        sim.run_days(1);
    }
    panic!("die Flotte kam nie an");
}

/// Die nächstgelegene fremde Kolonie, zu der ein Weg an der Flotte ihres Besitzers vorbei
/// führt: nicht dessen Hauptwelt, nicht in dessen Heimatsystem, und der kürzeste Weg vom
/// Heimatsystem des Angreifers berührt es nicht. Gibt Angreifer und Planet zurück.
fn nearest_colony(sim: &Headless) -> (PolityId, PlanetId) {
    let galaxy = sim.galaxy();
    let mut best: Option<(f32, PolityId, PlanetId)> = None;
    for attacker in &sim.polities().list {
        let home = home_of(sim, attacker.id).1;
        for planet in &galaxy.planets {
            let Some(owner) = planet.owner.filter(|owner| *owner != attacker.id) else {
                continue;
            };
            let (capital, guarded) = home_of(sim, owner);
            let path = galaxy.shortest_path(home, planet.system, |_| true).unwrap();
            if planet.id == capital || path.contains(&guarded) {
                continue;
            }
            let length = galaxy.path_length(&path);
            if best.is_none_or(|(shortest, ..)| length < shortest) {
                best = Some((length, attacker.id, planet.id));
            }
        }
    }
    let (_, attacker, planet) = best.expect("eine erreichbare fremde Kolonie");
    (attacker, planet)
}

/// Ein Feldzug: Der Angreifer liegt mit Truppen an Bord im System einer Welt des
/// Verteidigers, mit dem er im Krieg steht. Belagert wird noch nicht.
struct Campaign {
    sim: Headless,
    attacker: PolityId,
    defender: PolityId,
    fleet: FleetId,
    planet: PlanetId,
    system: SystemId,
}

impl Campaign {
    /// Krieg, Truppen an Bord, Blockadeauftrag und Marsch in das System des Planeten.
    fn march(mut sim: Headless, attacker: PolityId, planet: PlanetId) -> Self {
        let defender = owner(&sim, planet).unwrap();
        let system = sim.galaxy().planet(planet).system;
        let fleet = fleet_of(&sim, attacker);
        let home = home_of(&sim, attacker).0;
        applied(
            &mut sim,
            attacker,
            SimCommand::DeclareWar { target: defender },
        );
        applied(
            &mut sim,
            attacker,
            SimCommand::EmbarkTroops {
                fleet,
                planet: home,
                strength: TROOPS,
                supply_days: 0,
            },
        );
        applied(
            &mut sim,
            attacker,
            SimCommand::SetStance {
                fleet,
                stance: Stance::Blockade,
            },
        );
        applied(
            &mut sim,
            attacker,
            SimCommand::MoveFleet { fleet, to: system },
        );
        sail(&mut sim, fleet, system);
        Self {
            sim,
            attacker,
            defender,
            fleet,
            planet,
            system,
        }
    }

    /// Gegen die nächstgelegene fremde Kolonie, nach den ersten Jahren der Partie.
    fn against_a_colony() -> Self {
        let mut sim = observer();
        sim.run_days(YEARS * DAYS_PER_YEAR);
        let (attacker, planet) = nearest_colony(&sim);
        Self::march(sim, attacker, planet)
    }

    /// Gegen die nächstgelegene fremde Hauptwelt, vom ersten Tag an. Ihre Flotte verlegt
    /// der Verteidiger vorher in ein Nachbarsystem abseits des Wegs; `prepare` stellt
    /// davor die Ausgangslage der Hauptwelt.
    fn against_a_capital(prepare: impl FnOnce(&mut Headless, PlanetId)) -> Self {
        let mut sim = observer();
        let attacker = PolityId(0);
        let home = home_of(&sim, attacker).1;
        let (defender, path) = sim
            .polities()
            .list
            .iter()
            .filter(|polity| polity.id != attacker)
            .map(|polity| {
                let target = home_of(&sim, polity.id).1;
                let path = sim.galaxy().shortest_path(home, target, |_| true).unwrap();
                (polity.id, path)
            })
            .min_by(|a, b| {
                let length = |path: &[SystemId]| sim.galaxy().path_length(path);
                length(&a.1).total_cmp(&length(&b.1)).then(a.0.cmp(&b.0))
            })
            .unwrap();
        let (capital, system) = home_of(&sim, defender);
        prepare(&mut sim, capital);
        let aside = sim
            .galaxy()
            .neighbors(system)
            .iter()
            .map(|neighbor| neighbor.system)
            .find(|neighbor| !path.contains(neighbor))
            .expect("ein Nachbarsystem abseits des Wegs");
        let guard = fleet_of(&sim, defender);
        applied(
            &mut sim,
            defender,
            SimCommand::MoveFleet {
                fleet: guard,
                to: aside,
            },
        );
        sail(&mut sim, guard, aside);
        Self::march(sim, attacker, capital)
    }

    fn begin(&mut self) {
        applied(
            &mut self.sim,
            self.attacker,
            SimCommand::BeginSiege {
                fleet: self.fleet,
                planet: self.planet,
            },
        );
    }

    fn siege(&self) -> Option<&Siege> {
        sieges(&self.sim).siege(self.planet)
    }

    fn defense(&self) -> &PlanetDefense {
        defense(&self.sim, self.planet)
    }

    fn blockaded(&self) -> bool {
        fleets(&self.sim).is_blockaded(self.system)
    }

    fn aboard(&self) -> f64 {
        sieges(&self.sim)
            .embarked
            .get(&self.fleet)
            .map_or(0.0, |force| force.troops)
    }

    /// Die laufende Belagerung als Lesemodell, aus der Welt gebaut: der Datensatz, die
    /// Verteidigung, der Planet am nächsten Tag und der Orbit, wie ihn auch der
    /// Tagesschritt sieht. Darauf rechnet die Prognose.
    fn view(&self) -> SiegeView {
        let world = self.sim.world();
        let state = sieges(&self.sim);
        let siege = self.siege().expect("eine laufende Belagerung");
        let squadron = orbit::squadron(
            fleets(&self.sim),
            world.resource::<Research>(),
            data(),
            world.resource::<Diplomacy>(),
            state,
            self.planet,
            self.system,
            siege.attacker,
            siege.defender,
        );
        let economy = world.resource::<Economy>();
        SiegeView {
            planet: self.planet,
            siege: siege.clone(),
            defense: self.defense().clone(),
            facts: planet_facts(
                data(),
                self.sim.galaxy(),
                economy,
                finance(&self.sim),
                self.planet,
                self.sim.clock().day() + 1,
                1.0,
            ),
            orbit: squadron.view,
            toughness: squadron.toughness,
            fuel: squadron.fuel,
            supply_aboard: squadron.supply_aboard,
            firm_value: firm_value(
                data(),
                self.sim.galaxy(),
                economy,
                world.resource::<Logistics>(),
                self.planet,
            ),
        }
    }

    /// Ein Tag weiter; Invarianten und Geldsumme halten an jedem Tag.
    fn day(&mut self) {
        self.sim.run_days(1);
        let day = self.sim.clock().day();
        assert_eq!(check_invariants(self.sim.world()), Ok(()), "Tag {day}");
        assert_eq!(money_drift(self.sim.world()), Some(0), "Tag {day}");
    }

    /// Eine Zeile Tagesbericht: Phase, Vorräte, Kräfte, Schiffe.
    fn report(&self) -> String {
        let fleet = fleet(&self.sim, self.fleet);
        let hull = fleet.ships.iter().map(|ship| ship.hull).fold(1.0, f32::min);
        let (ammo, fuel) = fleet.supply(data());
        let defense = self.defense();
        let (phase, army, stores) = match self.siege() {
            Some(siege) => (
                siege.phase(defense).label(),
                siege.army,
                siege.report.depot_used,
            ),
            None => ("keine Belagerung", 0.0, 0.0),
        };
        format!(
            "Tag {:>4}  {:<16} Vorräte {:>5.1} Tage (heute {:>4.2})  Stationen {:>4.2}  Miliz {:>5.2}  \
             Reguläre {:>5.2}  Truppe am Boden {:>5.2}, an Bord {:>5.2}  Schiffe {} (schwächster \
             Rumpf {:>3.0} %, Munition {:>3.0} %, Treibstoff {:>3.0} %)  {}",
            self.sim.clock().day(),
            phase,
            defense.depot,
            stores,
            defense.stations,
            defense.militia,
            defense.regulars,
            army,
            self.aboard(),
            fleet.ships.len(),
            100.0 * hull,
            100.0 * ammo,
            100.0 * fuel,
            if self.blockaded() {
                "System gesperrt"
            } else {
                "System offen"
            }
        )
    }
}

// --- Aushungern ---------------------------------------------------------------------

#[test]
fn a_colony_is_starved_out_and_changes_its_owner_without_a_shot() {
    let balance = &data().balance.siege;
    let mut campaign = Campaign::against_a_colony();
    let (attacker, defender, planet) = (campaign.attacker, campaign.defender, campaign.planet);
    assert_eq!(campaign.aboard(), TROOPS);
    assert!(campaign.siege().is_none(), "ohne Befehl keine Belagerung");
    let home = home_of(&campaign.sim, defender).0;
    let firm = *finance(&campaign.sim)
        .issuers
        .keys()
        .find(|id| matches!(id, IssuerId::Firm { planet: seat, .. } if *seat == planet))
        .expect("die Kolonie hat einen Betrieb an der Börse");
    let before = finance(&campaign.sim).issuers[&firm].clone();
    assert!(before.held_by(HolderId::State(defender)) > 0);
    let fuel_on_arrival = fleet(&campaign.sim, campaign.fleet).min_fuel();

    campaign.begin();
    campaign.day();
    println!("{}", campaign.report());
    // Der Datensatz steht am Tag nach dem Befehl: eingeschlossen, das System gesperrt.
    let siege = campaign.siege().expect("die Belagerung hat begonnen");
    let start = siege.started_day;
    assert_eq!((siege.attacker, siege.defender), (attacker, defender));
    assert_eq!(siege.phase(campaign.defense()), Phase::Investment);
    assert!(siege.garrison_start > 1.0, "die Miliz der Kolonie steht");
    assert!(siege.prize_start > 0.0);
    assert!(finance(&campaign.sim).region(planet).besieged);
    assert!(campaign.blockaded());
    let population = campaign.sim.galaxy().planet(planet).population;
    let needed = occupation_force(population, balance);
    assert!(needed < TROOPS, "die Besatzungstruppe ist an Bord");
    // Die Prognose rechnet auf den Zahlen der Welt: Aushungern gelingt ohne Verluste.
    let predicted = forecast(&campaign.view(), Course::Starve, balance);
    assert!(predicted.falls, "Prognose: {predicted:?}");
    assert_eq!(predicted.outcome, Some(Outcome::Starved));
    assert_eq!((predicted.own_losses, predicted.hull_loss), (0.0, 0.0));

    // Kein Feuer, keine Landung: Der Planet fällt in der Frist, ohne Verluste am Boden.
    let mut days = 1;
    while owner(&campaign.sim, planet) == Some(defender) {
        campaign.day();
        days += 1;
        assert!(
            days <= balance.max_siege_days,
            "die Einschließung endet nicht in der Frist"
        );
        if days <= 3 || days % 10 == 0 || campaign.defense().militia < 1.0 {
            println!("{}", campaign.report());
        }
        let Some(siege) = campaign.siege() else {
            continue;
        };
        assert_eq!(siege.army, 0.0, "niemand ist gelandet");
        let report = &siege.report;
        assert_eq!((report.attacker_lost, report.defender_lost), (0.0, 0.0));
        assert_eq!((report.fire_damage, report.ground_damage), (0.0, 0.0));
        assert!(
            report.blocked.is_empty(),
            "nichts ist befohlen, nichts gesperrt"
        );
    }
    assert_eq!(owner(&campaign.sim, planet), Some(attacker));
    assert!(
        days > 30,
        "auch ohne Vorräte hält die Miliz Wochen: {days} Tage"
    );
    // Die Prognose vom ersten Tag trifft den Verlauf bis auf das, was sie festhält: Der
    // Zusammenhalt der Welt bewegt sich in den Wochen der Einschließung.
    let forecast_days = 1 + predicted.days as i64;
    println!("gefallen nach {days} Tagen, vorausgesagt waren {forecast_days}");
    assert!(
        (forecast_days - days as i64).abs() <= 10,
        "{days} Tage, vorausgesagt {forecast_days}"
    );

    // Die Belagerung ist verbucht und geschlossen.
    let state = sieges(&campaign.sim);
    assert!(state.active.is_empty() && state.pending.is_empty() && state.assign.is_empty());
    let stats = &state.stats;
    assert_eq!((stats.begun, stats.starved, stats.conquests()), (1, 1, 1));
    assert_eq!(stats.days, campaign.sim.clock().day() - start);
    assert_eq!((stats.lost_attackers, stats.lost_defenders), (0.0, 0.0));
    assert!(stats.prize_kept > 0.0);
    // Wer zur Besatzung fehlte, ging ohne Verluste an Land; der Rest ist noch an Bord.
    let garrison = campaign.defense().regulars;
    assert!(garrison > 0.0 && garrison < TROOPS);
    assert!((garrison + campaign.aboard() - TROOPS).abs() < 1e-9);
    assert_eq!(campaign.defense().occupied_from, Some(defender));
    assert_eq!(campaign.defense().scar, 0.0);
    // Die Schiffe haben nur Treibstoff verbraucht.
    let ships = &fleet(&campaign.sim, campaign.fleet).ships;
    assert_eq!(ships.len(), 6);
    assert!(
        ships
            .iter()
            .all(|ship| ship.hull == 1.0 && ship.ammo == 1.0)
    );
    let burned = (fuel_on_arrival - fleet(&campaign.sim, campaign.fleet).min_fuel()) as f64;
    assert!(
        (burned - days as f64 * balance.station_fuel).abs() < 1e-3,
        "Treibstoff für {days} Tage im Orbit: {burned}"
    );

    // Die Region gehört dem Eroberer, unruhig und besetzt; die Anteile des alten Staates
    // sind seine, private Halter des alten Reichs sind jetzt fremder Besitz.
    let region = finance(&campaign.sim).region(planet);
    assert_eq!(region.stage, Stage::Restless);
    assert_eq!(region.owner_seen, Some(attacker));
    assert!(region.occupation > 0.0 && !region.besieged);
    let issuer = &finance(&campaign.sim).issuers[&firm];
    assert_eq!(issuer.held_by(HolderId::State(defender)), 0);
    assert!(issuer.held_by(HolderId::State(attacker)) >= before.held_by(HolderId::State(defender)));
    assert_eq!(
        issuer.held_by(HolderId::Elite(home)),
        before.held_by(HolderId::Elite(home))
    );
    let capitals: Vec<_> = campaign
        .sim
        .polities()
        .list
        .iter()
        .map(|polity| polity.capital)
        .collect();
    let ownership = ownership_of_region(
        finance(&campaign.sim),
        campaign.sim.galaxy(),
        campaign.sim.world().resource::<Logistics>(),
        &capitals,
        planet,
    );
    assert!(ownership.foreign.get(&defender).copied().unwrap_or(0.0) > 0.0);
    let told = campaign
        .sim
        .world()
        .resource::<sim::chronicle::Chronicle>()
        .entries
        .iter()
        .any(|entry| entry.text.contains("ausgehungert") && entry.text.contains("besetzt"));
    assert!(told, "die Chronik nennt die Eroberung");

    // Danach läuft die Partie weiter: Der Verlierer hält seine Hauptwelt, und der
    // Eroberer kann die Welt nicht noch einmal belagern.
    for _ in 0..40 {
        campaign.day();
    }
    assert_eq!(owner(&campaign.sim, planet), Some(attacker));
    assert_eq!(owner(&campaign.sim, home), Some(defender));
    let reason = rejection(
        &mut campaign.sim,
        attacker,
        SimCommand::BeginSiege {
            fleet: campaign.fleet,
            planet,
        },
    );
    assert!(reason.contains("eigenen Gemeinschaft"), "{reason}");
}

// --- Stationen ----------------------------------------------------------------------

#[test]
fn a_station_keeps_the_system_open_and_the_stores_full_until_it_falls() {
    let balance = &data().balance.siege;
    let mut open = Campaign::against_a_capital(|_, _| {});
    // Der Zwilling: dieselbe Partie, nur trägt die Hauptwelt eine Station.
    let mut held = Campaign::against_a_capital(|sim, capital| {
        let mut sieges = sim.world_mut().resource_mut::<Sieges>();
        let defense = sieges.defense_mut(capital);
        defense.stations = 1.0;
        defense.plan.stations = 1;
    });
    assert_eq!(
        (open.attacker, open.planet),
        (held.attacker, held.planet),
        "beide Partien spielen denselben Feldzug"
    );
    let stores = balance.home_start.depot_days as f64;
    assert_eq!(open.defense().depot, stores);
    open.begin();
    held.begin();
    for day in 1..=12 {
        open.day();
        held.day();
        // Ohne Station: eingeschlossen und gesperrt, die Vorräte schwinden.
        let siege = open.siege().expect("Belagerung ohne Station");
        assert_eq!(siege.phase(open.defense()), Phase::Investment);
        assert!(open.blockaded());
        assert!(open.defense().depot < stores - 0.5 * day as f64);
        // Mit Station: Anmarsch. Das System bleibt offen, die Vorräte bleiben voll.
        let siege = held.siege().expect("Belagerung mit Station");
        assert_eq!(siege.phase(held.defense()), Phase::Approach);
        assert!(
            !held.blockaded(),
            "Tag {day}: die Station hält das System offen"
        );
        assert_eq!(held.defense().depot, stores);
        assert!(siege.report.hull > 0.0 && siege.report.stations_lost > 0.0);
        println!("ohne Station  {}", open.report());
        println!("mit Station   {}", held.report());
    }
    // Das Duell kostet die Flotte Rumpf und Munition, die Station Stufe um Stufe.
    let unhurt = &fleet(&open.sim, open.fleet).ships[0];
    let hurt = &fleet(&held.sim, held.fleet).ships[0];
    assert_eq!((unhurt.hull, unhurt.ammo), (1.0, 1.0));
    assert!(hurt.hull < 0.95 && hurt.ammo < 0.95);
    assert!(held.defense().stations < 1.0);

    // Ist die Station zerstört, beginnt auch hier die Einschließung.
    let mut days = 12;
    while held.defense().stations > 0.0 {
        held.day();
        days += 1;
        assert!(days < 100, "die Station fällt nie");
        println!("mit Station   {}", held.report());
    }
    for _ in 0..2 {
        held.day();
        println!("mit Station   {}", held.report());
    }
    let siege = held.siege().expect("die Belagerung dauert an");
    assert_eq!(siege.phase(held.defense()), Phase::Investment);
    assert!(held.blockaded());
    assert!(held.defense().depot < stores);
    assert_eq!(fleet(&held.sim, held.fleet).ships.len(), 6);
}

// --- Treibstoff ---------------------------------------------------------------------

#[test]
fn a_fleet_with_empty_tanks_loses_the_orbit() {
    let balance = &data().balance.siege;
    let mut campaign = Campaign::against_a_capital(|_, _| {});
    let (defender, planet) = (campaign.defender, campaign.planet);
    campaign.begin();
    campaign.day();
    let siege = campaign.siege().expect("die Belagerung hat begonnen");
    let (start, garrison) = (siege.started_day, siege.garrison_start);
    let fuel = fleet(&campaign.sim, campaign.fleet).min_fuel() as f64;
    // Der Marsch hat Treibstoff gekostet. Der Rest reicht nicht, um die Hauptwelt
    // auszuhungern: erst die Vorräte, die der Markt vor Ort streckt, dann die Wochen, in
    // denen sich die Garnison auflöst.
    let endurance = (fuel / balance.station_fuel).ceil() as u64;
    let stores = campaign.defense().depot / (1.0 - balance.forage_share);
    let cohesion = finance(&campaign.sim).region(planet).cohesion;
    let resolve = will(cohesion, 0.0, balance);
    let dissolving = (1.0 / balance.capitulate_frac).ln() * resolve / balance.starve_rate;
    assert!(
        (endurance as f64) < stores + dissolving,
        "die Flotte hält {endurance} Tage, die Hauptwelt {:.0}",
        stores + dissolving
    );
    // Die Prognose sieht es kommen: Aushungern scheitert am Treibstoff.
    let predicted = forecast(&campaign.view(), Course::Starve, balance);
    println!("Prognose: {predicted:?}");
    assert!(!predicted.falls);
    assert_eq!(predicted.limited_by, Some(Limit::Fuel));
    assert!((predicted.days as i64 + 1 - endurance as i64).abs() <= 2);
    let mut days = 1;
    while campaign.siege().is_some() {
        campaign.day();
        days += 1;
        assert!(days <= endurance + 3, "die Belagerung überdauert die Tanks");
    }
    println!("{}", campaign.report());
    assert!(
        days + 1 >= endurance,
        "{days} Tage bei Treibstoff für {endurance}"
    );
    assert_eq!(fleet(&campaign.sim, campaign.fleet).min_fuel(), 0.0);
    // Ohne Treibstoff hält die Flotte keine Position: Die Belagerung ist aufgehoben. Die
    // Hauptwelt hat ihre Vorräte aufgezehrt und einen Teil der Garnison verloren, aber
    // sie hat gehalten.
    let stats = &sieges(&campaign.sim).stats;
    assert_eq!((stats.begun, stats.lifted, stats.conquests()), (1, 1, 0));
    assert_eq!(stats.days, campaign.sim.clock().day() - start);
    assert_eq!(owner(&campaign.sim, planet), Some(defender));
    let left = campaign.defense().militia + campaign.defense().regulars;
    assert!(
        left > balance.capitulate_frac * garrison && left < garrison,
        "{left} von {garrison} Verbänden"
    );
    assert_eq!(campaign.defense().depot, 0.0);
    // Sie liegt weiter im System, belagert aber nichts mehr, solange die Tanks leer sind.
    for _ in 0..5 {
        campaign.day();
    }
    assert!(campaign.siege().is_none());
    assert_eq!(sieges(&campaign.sim).stats.begun, 1);
    assert_eq!(campaign.aboard(), TROOPS);
}

// --- Entsatz ------------------------------------------------------------------------

#[test]
fn a_relief_fleet_lifts_the_siege() {
    let mut campaign = Campaign::against_a_colony();
    let (attacker, defender, planet) = (campaign.attacker, campaign.defender, campaign.planet);
    campaign.begin();
    for _ in 0..50 {
        campaign.day();
    }
    let siege = campaign.siege().expect("die Belagerung läuft");
    let start = siege.started_day;
    // Sieben Wochen ohne Vorräte: Ein Teil der Miliz hat sich aufgelöst.
    let militia = campaign.defense().militia;
    assert!(militia > 0.0 && militia < 0.6 * siege.garrison_start);
    assert!(campaign.blockaded());
    println!("{}", campaign.report());

    // Am 50. Tag bricht die Flotte des Verteidigers zum Entsatz auf.
    let relief = fleet_of(&campaign.sim, defender);
    applied(
        &mut campaign.sim,
        defender,
        SimCommand::MoveFleet {
            fleet: relief,
            to: campaign.system,
        },
    );
    let mut days = 0;
    while campaign.siege().is_some() {
        campaign.day();
        days += 1;
        assert!(days < 120, "der Entsatz kommt nie an");
    }
    println!("{}", campaign.report());
    // Mit dem Eintreffen des Entsatzes beginnt das Gefecht, und die Belagerung ist
    // aufgehoben: Der Planet bleibt beim Verteidiger, das System ist wieder offen, die
    // übrige Miliz steht als Miliz der Welt. (Dass der Besitzer danach Vorräte gegen
    // Bezahlung auffüllt, kommt erst mit der Beschaffung der stehenden Verteidigung.)
    assert!(
        fleet(&campaign.sim, relief).in_battle && fleet(&campaign.sim, campaign.fleet).in_battle
    );
    assert_eq!(owner(&campaign.sim, planet), Some(defender));
    assert!(!finance(&campaign.sim).region(planet).besieged);
    assert!(!campaign.blockaded());
    let stats = &sieges(&campaign.sim).stats;
    assert_eq!((stats.begun, stats.lifted, stats.conquests()), (1, 1, 0));
    assert_eq!(stats.days, campaign.sim.clock().day() - start);
    assert!(campaign.defense().militia > 0.0 && campaign.defense().militia <= militia);
    let told = campaign
        .sim
        .world()
        .resource::<sim::chronicle::Chronicle>()
        .entries
        .iter()
        .any(|entry| entry.text.contains("aufgehoben"));
    assert!(told, "die Chronik nennt das Ende der Belagerung");
    // Solange gekämpft wird, entsteht keine neue Belagerung; die Flotte bleibt zugeordnet.
    campaign.day();
    assert!(campaign.siege().is_none());
    assert_eq!(
        sieges(&campaign.sim).assign.get(&campaign.fleet),
        Some(&planet)
    );

    // Das Gefecht läuft zu Ende. Für jeden Ausgang gilt: Hält der Belagerer danach
    // einsatzbereit das System, beginnt die Belagerung von vorn; sonst ist seine
    // Zuordnung gefallen, und was er an Truppen noch trägt, hat Platz an Bord.
    let mut days = 0;
    while !campaign.sim.world().resource::<Battles>().active.is_empty() {
        campaign.day();
        days += 1;
        assert!(days < 400, "das Gefecht endet nie");
    }
    campaign.day();
    campaign.day();
    println!("{}", campaign.report());
    let besieger = fleet(&campaign.sim, campaign.fleet);
    let holds = besieger.alive
        && besieger.is_docked()
        && besieger.system == campaign.system
        && besieger.is_ready()
        && !fleets(&campaign.sim)
            .docked_in(campaign.system)
            .any(|fleet| fleet.owner == defender && !fleet.is_regrouping());
    let state = sieges(&campaign.sim);
    if holds {
        assert_eq!(
            state.siege(planet).map(|siege| siege.attacker),
            Some(attacker)
        );
        assert_eq!(state.stats.begun, 2);
    } else {
        assert!(state.siege(planet).is_none());
        assert_eq!(state.stats.begun, 1);
        let gone = !besieger.alive || besieger.system != campaign.system || !besieger.is_docked();
        assert_eq!(state.assign.contains_key(&campaign.fleet), !gone);
    }
    assert_eq!(owner(&campaign.sim, planet), Some(defender));
    if !besieger.alive {
        assert!(!state.embarked.contains_key(&campaign.fleet));
    }
}

// --- Frieden ------------------------------------------------------------------------

#[test]
fn only_the_besieger_can_make_peace_and_peace_lifts_the_siege() {
    let mut campaign = Campaign::against_a_colony();
    let (attacker, defender, planet) = (campaign.attacker, campaign.defender, campaign.planet);
    campaign.begin();
    for _ in 0..10 {
        campaign.day();
    }
    assert!(campaign.siege().is_some());
    // Der Belagerte kann den Frieden nicht selbst schließen.
    let reason = rejection(
        &mut campaign.sim,
        defender,
        SimCommand::MakePeace { target: attacker },
    );
    let besieger = campaign.sim.polities().get(attacker).name.clone();
    let world = campaign.sim.galaxy().planet(planet).name.clone();
    assert_eq!(
        reason,
        format!("{besieger} belagert {world}: Frieden kann nur der Belagerer anbieten")
    );
    assert!(
        campaign
            .sim
            .world()
            .resource::<Diplomacy>()
            .at_war(attacker, defender)
    );
    campaign.day();
    assert!(campaign.siege().is_some(), "die Belagerung dauert an");

    // Der Belagerer kann es: Mit dem Frieden ist die Belagerung im selben Takt aufgehoben.
    applied(
        &mut campaign.sim,
        attacker,
        SimCommand::MakePeace { target: defender },
    );
    assert!(campaign.siege().is_none());
    assert_eq!(check_invariants(campaign.sim.world()), Ok(()));
    let stats = &sieges(&campaign.sim).stats;
    assert_eq!((stats.lifted, stats.conquests()), (1, 0));
    assert!(!finance(&campaign.sim).region(planet).besieged);
    campaign.day();
    assert_eq!(owner(&campaign.sim, planet), Some(defender));
    assert!(sieges(&campaign.sim).assign.is_empty());
    assert!(!campaign.blockaded());
    // Die Truppen sind noch an Bord, und im Frieden lässt sich nichts belagern.
    assert_eq!(campaign.aboard(), TROOPS);
    let reason = rejection(
        &mut campaign.sim,
        attacker,
        SimCommand::BeginSiege {
            fleet: campaign.fleet,
            planet,
        },
    );
    assert!(reason.contains("herrscht kein Krieg"), "{reason}");
    // Danach kann auch der frühere Verteidiger wieder Frieden schließen wollen: Es
    // herrscht keiner mehr zu schließen, aber niemand belagert ihn.
    let reason = rejection(
        &mut campaign.sim,
        defender,
        SimCommand::MakePeace { target: attacker },
    );
    assert_eq!(reason, "es herrscht kein Krieg");
}

// --- Befehle ------------------------------------------------------------------------

#[test]
fn siege_orders_are_validated_and_rejected_with_reasons() {
    let balance = &data().balance.siege;
    let mut sim = observer();
    sim.run_days(YEARS * DAYS_PER_YEAR);
    let (attacker, planet) = nearest_colony(&sim);
    let defender = owner(&sim, planet).unwrap();
    let system = sim.galaxy().planet(planet).system;
    let fleet = fleet_of(&sim, attacker);
    let (home, home_system) = home_of(&sim, attacker);
    let name = sim.galaxy().planet(planet).name.clone();
    let fleet_name = fleets(&sim).list[fleet.index()].name.clone();
    let embark = |strength: f64, supply_days: u16| SimCommand::EmbarkTroops {
        fleet,
        planet: home,
        strength,
        supply_days,
    };
    let begin = SimCommand::BeginSiege { fleet, planet };

    // Einschiffen: nur eigene Truppen, nur was da ist, nur was Platz hat.
    let regulars = defense(&sim, home).regulars;
    assert_eq!(regulars, balance.home_start.regulars);
    let reason = rejection(&mut sim, attacker, embark(regulars + 1.0, 0));
    assert!(reason.contains("reguläre Verbände bereit"), "{reason}");
    let reason = rejection(&mut sim, attacker, embark(0.0, 0));
    assert!(reason.contains("positiv"), "{reason}");
    let reason = rejection(&mut sim, attacker, embark(1.0, balance.supply_days_cap + 1));
    assert!(reason.contains("Tage Nachschub"), "{reason}");
    let reason = rejection(
        &mut sim,
        attacker,
        SimCommand::EmbarkTroops {
            fleet,
            planet,
            strength: 1.0,
            supply_days: 0,
        },
    );
    assert!(reason.contains("gehört nicht der eigenen"), "{reason}");
    let reason = rejection(&mut sim, defender, embark(1.0, 0));
    assert!(reason.contains("Flotte gehört einer anderen"), "{reason}");
    applied(&mut sim, attacker, embark(TROOPS, 0));
    assert_eq!(defense(&sim, home).regulars, regulars - TROOPS);
    assert_eq!(sieges(&sim).embarked[&fleet].troops, TROOPS);
    assert_eq!(check_invariants(sim.world()), Ok(()));

    // Beim Teilen der Flotte fahren Truppen ohne Platz mit den abgelösten Schiffen;
    // vereinigt sind sie wieder beisammen.
    let ships: Vec<ShipId> = fleets(&sim).list[fleet.index()]
        .ships
        .iter()
        .skip(1)
        .map(|ship| ship.id)
        .collect();
    applied(&mut sim, attacker, SimCommand::SplitFleet { fleet, ships });
    let detached = fleets(&sim)
        .of(attacker)
        .map(|fleet| fleet.id)
        .find(|id| *id != fleet)
        .unwrap();
    let cruiser = 14.0 * balance.berth_per_segment;
    assert!((sieges(&sim).embarked[&fleet].troops - cruiser).abs() < 1e-9);
    assert!((sieges(&sim).embarked[&detached].troops - (TROOPS - cruiser)).abs() < 1e-9);
    assert_eq!(check_invariants(sim.world()), Ok(()));
    applied(
        &mut sim,
        attacker,
        SimCommand::MergeFleets {
            from: detached,
            into: fleet,
        },
    );
    assert_eq!(sieges(&sim).embarked.len(), 1);
    assert!((sieges(&sim).embarked[&fleet].troops - TROOPS).abs() < 1e-9);
    assert_eq!(check_invariants(sim.world()), Ok(()));

    // Belagern: nur im Krieg, nur im System des Planeten, nur fremde Welten.
    let reason = rejection(&mut sim, attacker, begin.clone());
    assert_eq!(
        reason,
        format!("{fleet_name} liegt nicht im System von {name}")
    );
    applied(
        &mut sim,
        attacker,
        SimCommand::MoveFleet { fleet, to: system },
    );
    sail(&mut sim, fleet, system);
    let reason = rejection(&mut sim, attacker, begin.clone());
    let foe = sim.polities().get(defender).name.clone();
    assert_eq!(reason, format!("mit {foe} herrscht kein Krieg"));
    let reason = rejection(
        &mut sim,
        attacker,
        SimCommand::BeginSiege {
            fleet,
            planet: PlanetId(9_999),
        },
    );
    assert_eq!(reason, "Planet unbekannt");
    let reason = rejection(&mut sim, defender, begin.clone());
    assert!(reason.contains("Flotte gehört einer anderen"), "{reason}");
    applied(
        &mut sim,
        attacker,
        SimCommand::DeclareWar { target: defender },
    );
    // Truppen kommen nur von einer Welt im System der Flotte an Bord.
    let reason = rejection(&mut sim, attacker, embark(1.0, 0));
    assert!(reason.contains("liegt nicht im System"), "{reason}");
    assert_ne!(home_system, system);

    // Befehle an eine Belagerung, die es nicht gibt.
    let orders = SiegeOrders {
        fire: Fire::Precise,
        assault: false,
        focus: 0,
        sack: false,
    };
    let reason = rejection(
        &mut sim,
        attacker,
        SimCommand::SetSiegeOrders { planet, orders },
    );
    assert_eq!(
        reason,
        format!("{name} wird nicht von der eigenen Gemeinschaft belagert")
    );
    let reason = rejection(&mut sim, attacker, SimCommand::LiftSiege { planet });
    assert!(reason.contains("wird nicht von der eigenen"), "{reason}");

    applied(&mut sim, attacker, begin.clone());
    sim.run_days(1);
    let zones = sieges(&sim).siege(planet).expect("Belagerung").zones.len();
    // Stehende Befehle: nur der Belagerer, der Schwerpunkt muss eine Zone sein.
    let reason = rejection(
        &mut sim,
        defender,
        SimCommand::SetSiegeOrders { planet, orders },
    );
    assert!(reason.contains("wird nicht von der eigenen"), "{reason}");
    let reason = rejection(
        &mut sim,
        attacker,
        SimCommand::SetSiegeOrders {
            planet,
            orders: SiegeOrders {
                focus: zones as u8,
                ..orders
            },
        },
    );
    assert!(reason.contains("Schwerpunkt"), "{reason}");
    applied(
        &mut sim,
        attacker,
        SimCommand::SetSiegeOrders { planet, orders },
    );
    assert_eq!(sieges(&sim).siege(planet).unwrap().orders, orders);
    // Gezielter Beschuss wirkt am nächsten Tag: Munition wird verbraucht, die Miliz
    // getroffen, und die Kolonie ohne Geschütze kann nicht antworten.
    sim.run_days(1);
    let report = sieges(&sim).siege(planet).unwrap().report.clone();
    assert!(report.ammo > 0.0 && report.defender_bombed > 0.0 && report.fire_damage > 0.0);
    assert_eq!(report.hull, 0.0);
    assert!(!report.blocked.contains(Blocked::NoAmmo));
    assert!(fleets(&sim).list[fleet.index()].ships[0].ammo < 1.0);
    assert_eq!(defense(&sim, planet).scar_by, Some(attacker));
    assert_eq!(check_invariants(sim.world()), Ok(()));

    // Die Garnison: nur der Besitzer befiehlt ihr.
    let garrison = |posture, hold_fire| SimCommand::SetGarrisonOrders {
        planet,
        posture,
        hold_fire,
    };
    let reason = rejection(&mut sim, attacker, garrison(Posture::Deep, true));
    assert!(reason.contains("gehört nicht der eigenen"), "{reason}");
    applied(&mut sim, defender, garrison(Posture::Deep, true));
    assert_eq!(defense(&sim, planet).posture, Posture::Deep);
    assert!(defense(&sim, planet).hold_fire);
    applied(&mut sim, defender, garrison(Posture::Forward, false));
    assert_eq!(defense(&sim, planet).posture, Posture::Forward);

    // Der Belagerer hebt auf: sofort, und die Truppen bleiben an Bord.
    applied(&mut sim, attacker, SimCommand::LiftSiege { planet });
    assert!(sieges(&sim).siege(planet).is_none());
    assert!(sieges(&sim).assign.is_empty());
    assert_eq!(sieges(&sim).stats.lifted, 1);
    assert_eq!(sieges(&sim).embarked[&fleet].troops, TROOPS);
    assert_eq!(check_invariants(sim.world()), Ok(()));
    sim.run_days(2);
    assert!(
        sieges(&sim).siege(planet).is_none(),
        "ohne Zuordnung beginnt die Belagerung nicht von selbst wieder"
    );
    // Auf Befehl beginnt sie neu.
    applied(&mut sim, attacker, begin);
    sim.run_days(1);
    assert!(sieges(&sim).siege(planet).is_some());
    assert_eq!(sieges(&sim).stats.begun, 2);
    assert_eq!(owner(&sim, planet), Some(defender));
    assert_eq!(money_drift(sim.world()), Some(0));
}

// --- Wiederholbarkeit ---------------------------------------------------------------

/// Derselbe Feldzug, Schritt für Schritt nach Tagen: Krieg, Truppen, Marsch, Belagerung
/// mit einigen Tagen gezieltem Beschuss, bis über die Eroberung hinaus. `exact` rechnet
/// jeden Takt; `reload_after` speichert nach so vielen Belagerungstagen, lädt den Stand
/// und spielt mit ihm weiter.
fn replay(exact: bool, reload_after: Option<u64>) -> (u64, Option<PolityId>, u32) {
    let mut sim = observer();
    let advance = |sim: &mut Headless, days: u64| {
        if exact {
            sim.run_days_exact(days);
        } else {
            sim.run_days(days);
        }
    };
    advance(&mut sim, YEARS * DAYS_PER_YEAR);
    let (attacker, planet) = nearest_colony(&sim);
    let defender = owner(&sim, planet).unwrap();
    let system = sim.galaxy().planet(planet).system;
    let fleet = fleet_of(&sim, attacker);
    let home = home_of(&sim, attacker).0;
    sim.issue(attacker, SimCommand::DeclareWar { target: defender });
    sim.issue(
        attacker,
        SimCommand::EmbarkTroops {
            fleet,
            planet: home,
            strength: TROOPS,
            supply_days: 30,
        },
    );
    sim.issue(attacker, SimCommand::MoveFleet { fleet, to: system });
    // Der Marsch dauert in jeder Spielart gleich lang.
    advance(&mut sim, 30);
    assert!(fleets(&sim).list[fleet.index()].system == system);
    sim.issue(attacker, SimCommand::BeginSiege { fleet, planet });
    advance(&mut sim, 5);
    let orders = SiegeOrders {
        fire: Fire::Precise,
        assault: false,
        focus: 0,
        sack: false,
    };
    sim.issue(attacker, SimCommand::SetSiegeOrders { planet, orders });
    advance(&mut sim, 5);
    sim.issue(
        attacker,
        SimCommand::SetSiegeOrders {
            planet,
            orders: SiegeOrders::default(),
        },
    );
    let mut remaining = 150;
    if let Some(days) = reload_after {
        advance(&mut sim, days);
        remaining -= days;
        assert!(
            sieges(&sim).siege(planet).is_some(),
            "mitten in der Belagerung"
        );
        let text = sim.save().to_ron().unwrap();
        let save = SaveGame::from_ron(&text).unwrap();
        let resumed = Headless::from_save(data().clone(), save);
        assert_eq!(resumed.hash(), sim.hash(), "Laden stellt den Zustand her");
        sim = resumed;
    }
    advance(&mut sim, remaining);
    assert_eq!(check_invariants(sim.world()), Ok(()));
    (
        sim.hash(),
        owner(&sim, planet),
        sieges(&sim).stats.conquests(),
    )
}

#[test]
fn a_siege_replays_identically_with_every_tick_and_across_saving() {
    let plain = replay(false, None);
    // Die Belagerung hat in der Zeit stattgefunden und ist entschieden.
    assert_eq!(plain.2, 1, "der Planet ist gefallen");
    // Gleicher Seed, gleiche Befehle: derselbe Verlauf.
    assert_eq!(replay(false, None), plain);
    // Jeder Takt gerechnet statt Leertakte übersprungen.
    assert_eq!(replay(true, None), plain);
    // Gespeichert und geladen mitten in der Belagerung.
    assert_eq!(replay(false, Some(20)), plain);
}

// --- Viele Kriege -------------------------------------------------------------------

/// Die Welt des Gegners `foe`, die `fleet` als Nächstes belagern soll: im eigenen System
/// die bevölkerungsreichste, sonst die nächstgelegene. Dazu ihr System.
fn next_target(sim: &Headless, fleet: &Fleet, foe: PolityId) -> Option<(PlanetId, SystemId)> {
    let galaxy = sim.galaxy();
    let here = galaxy
        .planets_in(fleet.system)
        .filter(|planet| planet.owner == Some(foe))
        .max_by(|a, b| a.population.total_cmp(&b.population).then(b.id.cmp(&a.id)));
    if let Some(planet) = here {
        return Some((planet.id, planet.system));
    }
    galaxy
        .planets_of(foe)
        .map(|planet| {
            let path = galaxy
                .shortest_path(fleet.system, planet.system, |_| true)
                .unwrap();
            (galaxy.path_length(&path), planet.id, planet.system)
        })
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
        .map(|(_, planet, system)| (planet, system))
}

/// Jede Gemeinschaft erklärt ihrer Nachbarin in der Liste den Krieg, schifft Truppen ein
/// und belagert, was sie erreicht; alle zehn Tage neue Befehle. Kein Drehbuch für einen
/// bestimmten Verlauf: Gefechte, Entsatz, Rückzüge, verlorene Flotten und Eroberungen
/// greifen über zehn Jahre ineinander, und an jedem Tag müssen alle Invarianten und die
/// Geldsumme halten. Die Seeds sind so gewählt, dass viel davon vorkommt.
#[test]
fn sieges_in_many_wars_keep_every_invariant() {
    let mut conquests = 0;
    let mut lifted = 0;
    for seed in [4, 10, 14] {
        let mut sim = Headless::new(data().clone(), SimConfig { seed, player: None });
        sim.run_days(3 * DAYS_PER_YEAR);
        let count = sim.polities().list.len() as u32;
        let foe_of = |polity: PolityId| PolityId((polity.0 + 1) % count);
        for index in 0..count {
            let polity = PolityId(index);
            let fleet = fleet_of(&sim, polity);
            let home = home_of(&sim, polity).0;
            sim.issue(
                polity,
                SimCommand::DeclareWar {
                    target: foe_of(polity),
                },
            );
            sim.issue(
                polity,
                SimCommand::EmbarkTroops {
                    fleet,
                    planet: home,
                    strength: TROOPS,
                    supply_days: 30,
                },
            );
            sim.issue(
                polity,
                SimCommand::SetStance {
                    fleet,
                    stance: Stance::Blockade,
                },
            );
        }
        for step in 0..(10 * DAYS_PER_YEAR / 10) {
            let orders: Vec<(PolityId, SimCommand)> = fleets(&sim)
                .list
                .iter()
                .filter(|fleet| fleet.alive && fleet.owner.0 < count && fleet.is_ready())
                .filter(|fleet| fleet.is_docked() && fleet.route.is_empty())
                .filter_map(|fleet| {
                    let foe = foe_of(fleet.owner);
                    let (planet, system) = next_target(&sim, fleet, foe)?;
                    if sieges(&sim).assign.get(&fleet.id) == Some(&planet) {
                        return None;
                    }
                    let order = if system == fleet.system {
                        SimCommand::BeginSiege {
                            fleet: fleet.id,
                            planet,
                        }
                    } else {
                        SimCommand::MoveFleet {
                            fleet: fleet.id,
                            to: system,
                        }
                    };
                    Some((fleet.owner, order))
                })
                .collect();
            for (polity, order) in orders {
                sim.issue(polity, order);
            }
            for _ in 0..10 {
                sim.run_days(1);
                let day = sim.clock().day();
                assert_eq!(
                    check_invariants(sim.world()),
                    Ok(()),
                    "Seed {seed}, Tag {day} (Schritt {step})"
                );
                assert_eq!(money_drift(sim.world()), Some(0), "Seed {seed}, Tag {day}");
            }
        }
        let state = sieges(&sim);
        let stats = &state.stats;
        println!(
            "Seed {seed}: {} Belagerungen begonnen, {} ausgehungert, {} gestürmt, {} aufgehoben, \
             {} laufen, {} Besitzwechsel stehen aus; längste {} Tage; Gefechte {}",
            stats.begun,
            stats.starved,
            stats.stormed,
            stats.lifted,
            state.active.len(),
            state.pending.len(),
            stats.longest,
            sim.world().resource::<Battles>().next_id
        );
        // Jeder Datensatz ist genau einmal verbucht oder läuft noch.
        assert_eq!(
            stats.begun as usize,
            (stats.lifted + stats.conquests()) as usize + state.active.len()
        );
        assert!(stats.begun > 0, "Seed {seed}: niemand hat belagert");
        conquests += stats.conquests();
        lifted += stats.lifted;
    }
    assert!(conquests > 0, "in keiner Partie fiel eine Welt");
    assert!(
        lifted > 0,
        "in keiner Partie wurde eine Belagerung aufgehoben"
    );
}

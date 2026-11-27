//! Belagerung in der laufenden Partie: aushungern, Stationen, Entsatz, Frieden, Befehle
//! und Wiederholbarkeit. Die Rechenregeln selbst prüfen die Tests neben dem Code in
//! `sim::siege`; hier geht es darum, dass sie in ganzen Partien greifen.
//!
//! Gespielt wird auf echten Seeds und nur über Befehle: Krieg erklären, Truppen
//! einschiffen, marschieren, belagern. Zwei Ausnahmen werden als Ausgangslage gestellt,
//! weil es die Beschaffung dafür in der Partie noch nicht gibt: die Station im Test dazu
//! und der Vorrat des Heimatmarkts, aus dem eine Landungstruppe ihren Nachschub kauft.
//!
//! Wer einen Verlauf sehen will: `cargo test -p sim --test siege -- --nocapture` druckt
//! Tagesberichte.

use sim::GameData;
use sim::ai::{AiState, MilitaryMode};
use sim::clock::DAYS_PER_YEAR;
use sim::command::{CommandOutcome, SimCommand};
use sim::diplomacy::Diplomacy;
use sim::economy::Economy;
use sim::finance::{Finance, Stage, issuers_of_region, ownership_of_region};
use sim::fleet::{Battles, Fleet, Fleets, Stance};
use sim::harness::{
    Headless, WarScript, check_invariants, conquest_war, nearest_neighbour, summarize,
};
use sim::ids::{FleetId, HolderId, IssuerId, PlanetId, PolityId, ShipId, SystemId};
use sim::logistics::Logistics;
use sim::siege::{
    Blocked, Course, Fire, Limit, Outcome, Phase, PlanetDefense, Posture, Siege, SiegeOrders,
    SiegeView, Sieges, firm_value, forecast, occupation_force, orbit, planet_facts, will,
};
use sim::state::{SaveGame, SimConfig, money_drift};
use sim::tech::Research;
use std::sync::OnceLock;

/// Seed der Partien hier, soweit ein Test keinen eigenen nennt.
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

/// Eine Partie ohne Spieler, in der die militärische Führung der KI ruht: Die Flotten
/// stellen die Tests selbst.
fn observer() -> Headless {
    let mut sim = Headless::new(
        data().clone(),
        SimConfig {
            seed: SEED,
            player: None,
        },
    );
    sim.set_military_mode(None, MilitaryMode::Resting);
    sim
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
    /// Krieg, `troops` Verbände an Bord, Blockadeauftrag und Marsch in das System des
    /// Planeten. Die Truppen nehmen Nachschub für `supply_days` Tage mit, soweit der
    /// Heimatmarkt ihn hat.
    fn march(
        mut sim: Headless,
        attacker: PolityId,
        planet: PlanetId,
        troops: f64,
        supply_days: u16,
    ) -> Self {
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
                strength: troops,
                supply_days,
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

    /// Gegen die nächstgelegene fremde Kolonie, nach den ersten Jahren der Partie. Ohne
    /// Nachschub: Die Truppen bleiben an Bord.
    fn against_a_colony() -> Self {
        Self::against_a_colony_with(TROOPS, 0)
    }

    /// Dasselbe mit `troops` Verbänden und Nachschub für eine Landung.
    fn against_a_colony_with(troops: f64, supply_days: u16) -> Self {
        Self::against_a_prepared_colony(troops, supply_days, |_, _| {})
    }

    /// Dasselbe; `prepare` stellt vor dem Krieg die Ausgangslage der Kolonie.
    fn against_a_prepared_colony(
        troops: f64,
        supply_days: u16,
        prepare: impl FnOnce(&mut Headless, PlanetId),
    ) -> Self {
        let mut sim = observer();
        sim.run_days(YEARS * DAYS_PER_YEAR);
        let (attacker, planet) = nearest_colony(&sim);
        prepare(&mut sim, planet);
        if supply_days > 0 {
            // Ausgangslage: Der Heimatmarkt führt, was die Truppe an Nachschub braucht.
            // Rüstungsgüter sind in den ersten Jahren knapp, und eine Beschaffung, die
            // sie über Monate ansammelt, gibt es noch nicht.
            let home = home_of(&sim, attacker).1;
            let wanted = troops * supply_days as f64;
            let mut economy = sim.world_mut().resource_mut::<Economy>();
            let market = &mut economy.markets[home.index()];
            for (good, amount) in &data().balance.siege.day_bundle_ids {
                market.stock[good.index()] += 10.0 * wanted * amount;
            }
        }
        Self::march(sim, attacker, planet, troops, supply_days)
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
        Self::march(sim, attacker, capital, TROOPS, 0)
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

    /// Stehende Befehle des Belagerers.
    fn order(&mut self, orders: SiegeOrders) {
        applied(
            &mut self.sim,
            self.attacker,
            SimCommand::SetSiegeOrders {
                planet: self.planet,
                orders,
            },
        );
    }

    /// Setzt eine Welle der eingeschifften Truppen an Land.
    fn land(&mut self, strength: f64) {
        applied(
            &mut self.sim,
            self.attacker,
            SimCommand::LandTroops {
                fleet: self.fleet,
                planet: self.planet,
                strength,
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

    /// Nachschub an Bord in Verbandstagen.
    fn supply_aboard(&self) -> f64 {
        sieges(&self.sim)
            .embarked
            .get(&self.fleet)
            .map_or(0.0, |force| force.supply)
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
fn a_relief_fleet_rests_the_siege_and_breaks_it_if_it_wins() {
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

    // Am 50. Tag bricht die Flotte des Verteidigers zum Entsatz auf. Mit ihrem Eintreffen
    // beginnt nach den Regeln der Flotten das Gefecht.
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
    while !(fleet(&campaign.sim, relief).in_battle
        && fleet(&campaign.sim, campaign.fleet).in_battle)
    {
        campaign.day();
        days += 1;
        assert!(days < 120, "der Entsatz kommt nie an");
    }
    campaign.day();
    println!("{}", campaign.report());
    // Solange gekämpft wird, ruht die Belagerung: Sie besteht weiter, die Miliz bleibt in
    // ihren Zonen, niemand zehrt an Vorräten oder Garnison, und die Sperre des Systems
    // hält, solange die Flotte des Belagerers auf dem Feld kämpft.
    let resting = campaign.siege().expect("die Belagerung ruht nur");
    assert_eq!(resting.started_day, start);
    assert!(finance(&campaign.sim).region(planet).besieged);
    assert!(campaign.blockaded());
    let cards = campaign.sim.siege_cards();
    assert_eq!(cards.len(), 1);
    assert!(
        !cards[0].control,
        "das Lesemodell nennt den umstrittenen Orbit"
    );
    // Die Flotte liegt noch über der Welt: Die Karte nennt sie, ihre Tanks und ihre
    // Truppen, und als Grund den umstrittenen Orbit, nicht den Treibstoff.
    assert_eq!(cards[0].fleets, vec![campaign.fleet]);
    assert!(cards[0].fuel > 0.0);
    assert_eq!(cards[0].troops.embarked, campaign.aboard());
    for (_, told) in cards[0].forecasts {
        assert_eq!(told.limited_by, Some(Limit::NoControl));
    }
    let held = campaign.defense().militia;
    assert!(held > 0.0 && held <= militia);
    campaign.day();
    assert_eq!(campaign.defense().militia, held);
    assert_eq!(sieges(&campaign.sim).stats.begun, 1);
    assert_eq!(
        sieges(&campaign.sim).assign.get(&campaign.fleet),
        Some(&planet)
    );

    // Das Gefecht läuft zu Ende. Hält der Belagerer danach das System, geht dieselbe
    // Belagerung weiter; räumt er es, ist sie gebrochen und aufgehoben.
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
    let stays = besieger.alive && besieger.is_docked() && besieger.system == campaign.system;
    let state = sieges(&campaign.sim);
    assert_eq!(
        state.stats.begun, 1,
        "kein Neubeginn: Die Garnison füllt sich nicht auf"
    );
    if stays && besieger.min_fuel() > 0.0 {
        let siege = state.siege(planet).expect("die Belagerung geht weiter");
        assert_eq!((siege.attacker, siege.started_day), (attacker, start));
        assert_eq!(state.stats.lifted, 0);
    } else {
        assert!(state.siege(planet).is_none());
        assert_eq!((state.stats.lifted, state.stats.conquests()), (1, 0));
        assert!(!finance(&campaign.sim).region(planet).besieged);
        assert!(campaign.defense().militia > 0.0 && campaign.defense().militia <= militia);
        let told = campaign
            .sim
            .world()
            .resource::<sim::chronicle::Chronicle>()
            .entries
            .iter()
            .any(|entry| {
                entry.text.contains("Die Belagerung") && entry.text.contains("aufgehoben")
            });
        assert!(told, "die Chronik nennt das Ende der Belagerung");
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

    // Landen: nur von der eigenen Flotte, nur was an Bord ist; übergeben kann nur der
    // Besitzer.
    let land = |strength: f64| SimCommand::LandTroops {
        fleet,
        planet,
        strength,
    };
    let reason = rejection(&mut sim, defender, land(1.0));
    assert!(reason.contains("Flotte gehört einer anderen"), "{reason}");
    let reason = rejection(&mut sim, attacker, land(TROOPS + 1.0));
    assert_eq!(
        reason,
        format!("an Bord von {fleet_name} sind nur {TROOPS:.1} Verbände")
    );
    let reason = rejection(&mut sim, attacker, SimCommand::Capitulate { planet });
    assert_eq!(
        reason,
        format!("{name} gehört nicht der eigenen Gemeinschaft")
    );
    let reason = rejection(&mut sim, defender, SimCommand::Capitulate { planet: home });
    let home_name = sim.galaxy().planet(home).name.clone();
    assert!(
        reason.contains(&home_name),
        "die Hauptwelt des Belagerers gehört ihm nicht: {reason}"
    );
    let reason = rejection(&mut sim, attacker, SimCommand::Capitulate { planet: home });
    assert_eq!(reason, format!("{home_name} wird nicht belagert"));
    // Eine Welle landet auf der eingeschlossenen Kolonie; ohne Geschütze kommt sie ganz an.
    applied(&mut sim, attacker, land(2.0));
    let siege = sieges(&sim).siege(planet).unwrap();
    assert_eq!((siege.army, siege.army_peak), (2.0, 2.0));
    assert_eq!(siege.landed_day, Some(sim.clock().day()));
    assert_eq!(sieges(&sim).embarked[&fleet].troops, TROOPS - 2.0);
    assert_eq!(check_invariants(sim.world()), Ok(()));

    // Der Belagerer hebt auf: sofort. Die Truppe am Boden schifft sich wieder ein.
    applied(&mut sim, attacker, SimCommand::LiftSiege { planet });
    assert!(sieges(&sim).siege(planet).is_none());
    assert!(sieges(&sim).assign.is_empty());
    assert_eq!(sieges(&sim).stats.lifted, 1);
    assert_eq!(sieges(&sim).stats.lost_attackers, 0.0);
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

// --- Sturm --------------------------------------------------------------------------

#[test]
fn a_landing_storms_a_colony_as_the_forecast_says() {
    let balance = &data().balance.siege;
    // Alle zehn regulären Verbände der Heimatwelt, mit Nachschub für drei Wochen.
    let troops = balance.home_start.regulars;
    let mut campaign = Campaign::against_a_colony_with(troops, 21);
    let (attacker, defender, planet) = (campaign.attacker, campaign.defender, campaign.planet);
    let supply = campaign.supply_aboard();
    assert!(
        supply > 0.0,
        "der Heimatmarkt hatte Nachschub für die Truppe"
    );
    campaign.begin();
    campaign.day();
    let siege = campaign.siege().expect("die Belagerung hat begonnen");
    let (start, garrison, stake) = (siege.started_day, siege.garrison_start, siege.stake());
    let zones = siege.zones.len();
    assert!(garrison > 2.0, "die Miliz der Kolonie steht: {garrison}");
    // Die Prognose für den Sturm, auf den Zahlen der Welt gerechnet. Die Kolonie hat
    // keine Vorräte: Ihre Miliz löst sich auf, während gekämpft wird. Zehn Verbände sind
    // ihr so überlegen, dass das Zentrum im Kampf fällt, bevor der Hunger entscheidet.
    assert!(
        troops > 1.5 * garrison,
        "{troops} gegen {garrison} Verbände"
    );
    let predicted = forecast(&campaign.view(), Course::Storm, balance);
    println!("Prognose: {predicted:?}");
    assert_eq!(predicted.outcome, Some(Outcome::Stormed));

    // Sturm mit Stützfeuer, alle Truppen landen. Die Kolonie hat keine Geschütze.
    campaign.order(SiegeOrders {
        fire: Fire::Support,
        assault: true,
        focus: 0,
        sack: false,
    });
    campaign.land(troops);
    let siege = campaign.siege().unwrap();
    assert_eq!((siege.army, campaign.aboard()), (troops, 0.0));
    assert_eq!(siege.phase(campaign.defense()), Phase::Ground);
    // Mit den letzten Verbänden ist auch der Nachschub von Bord gegangen.
    assert_eq!((siege.army_supply, campaign.supply_aboard()), (supply, 0.0));

    let mut days = 1;
    let (mut fought, mut fallen, mut damage) = (0, 0, 0.0);
    loop {
        if let Some(siege) = campaign.siege() {
            let report = &siege.report;
            if report.zone.is_some() {
                fought += 1;
                assert!(report.attacker_lost > 0.0 && report.defender_lost > 0.0);
                assert!(report.ammo > 0.0, "Stützfeuer kostet Munition");
                assert!(report.ground_damage > 0.0);
            }
            fallen += report.fell.is_some() as usize;
            damage += report.ground_damage;
            assert_eq!(report.fire_damage, 0.0, "Stützfeuer trifft die Welt nicht");
            assert!(report.blocked.is_empty(), "{:?}", report.blocked);
            if days <= 3 || days % 5 == 0 || report.fell.is_some() {
                println!("{}", campaign.report());
            }
        }
        if owner(&campaign.sim, planet) != Some(defender) {
            break;
        }
        campaign.day();
        days += 1;
        assert!(days < 120, "der Sturm endet nie");
    }
    println!(
        "gestürmt nach {days} Tagen, vorausgesagt waren {}",
        predicted.days + 1
    );
    assert_eq!(owner(&campaign.sim, planet), Some(attacker));
    let state = sieges(&campaign.sim);
    let stats = &state.stats;
    assert_eq!((stats.stormed, stats.conquests()), (1, 1));
    assert_eq!(stats.days, campaign.sim.clock().day() - start);
    // Um jede Zone wurde gekämpft, das Zentrum fiel zuletzt.
    assert!(
        fought >= 5 && fallen >= 1,
        "{fought} Kampftage, {fallen} Zonen"
    );
    assert!(zones >= 2);
    // Der Verlauf trifft die Prognose: Dauer und Verluste. Sie hält Zusammenhalt und
    // Versorgung fest, die sich in den Wochen des Sturms bewegen.
    assert!(
        (predicted.days as i64 + 1 - days as i64).abs() <= 3,
        "{days} Tage, vorausgesagt {}",
        predicted.days + 1
    );
    assert!(
        (stats.lost_attackers - predicted.own_losses).abs() <= 0.15 * predicted.own_losses,
        "Verluste {:.2}, vorausgesagt {:.2}",
        stats.lost_attackers,
        predicted.own_losses
    );
    // Verluste auf beiden Seiten, die des Angreifers tragbar.
    assert!(stats.lost_attackers > 0.2 && stats.lost_attackers < 0.5 * troops);
    assert!(stats.lost_defenders > 0.5 * stats.lost_attackers);
    // Wer überlebt hat, ist die Garnison der Welt; ihr Nachschub liegt im Depot.
    let defense = campaign.defense();
    assert!((defense.regulars - (troops - stats.lost_attackers)).abs() < 1e-6);
    assert_eq!(defense.militia, 0.0);
    assert_eq!(defense.occupied_from, Some(defender));
    // Der Bodenkrieg hat Spuren hinterlassen, die der neue Besitzer verantwortet: die
    // Schäden aller Kampftage, auch des letzten. Sie lasten auf dem Zusammenhalt der
    // eroberten Welt.
    let scar = defense.scar;
    assert!(
        damage > 0.0 && scar > damage && scar < 2.0 * damage,
        "{scar} zu {damage}"
    );
    assert_eq!(defense.scar_by, Some(attacker));
    let region = finance(&campaign.sim).region(planet);
    assert_eq!(region.war_damage, scar);
    let cohesion = balance.conquest_base + balance.stake_pts * stake - balance.scar_pts * scar;
    assert!(
        (region.cohesion - cohesion).abs() < 1e-9,
        "Zusammenhalt {} statt {cohesion}",
        region.cohesion
    );
    assert_eq!(region.stage, Stage::Restless);
    // Die Schiffe haben Munition verschossen, aber keinen Treffer erhalten.
    let ships = &fleet(&campaign.sim, campaign.fleet).ships;
    assert!(ships.iter().all(|ship| ship.hull == 1.0));
    assert!(ships.iter().any(|ship| ship.ammo < 1.0));
    let told = campaign
        .sim
        .world()
        .resource::<sim::chronicle::Chronicle>()
        .entries
        .iter()
        .any(|entry| entry.text.contains("gestürmt") && entry.text.contains("besetzt"));
    assert!(told, "die Chronik nennt die Eroberung");
}

// --- Verluste im Orbit --------------------------------------------------------------

/// Eine Hauptwelt mit Station, deren Feuer die angeschlagene Flotte des Belagerers am
/// nächsten Tag zerstört. Gibt den Feldzug nach diesem Tag zurück.
fn wrecked_over_a_station() -> Campaign {
    let mut campaign = Campaign::against_a_capital(|sim, capital| {
        let mut sieges = sim.world_mut().resource_mut::<Sieges>();
        let defense = sieges.defense_mut(capital);
        defense.stations = 1.0;
        defense.plan.stations = 1;
    });
    campaign.begin();
    campaign.day();
    assert!(campaign.siege().is_some());
    let reserved = &campaign.sim.world().resource::<AiState>().reserved;
    assert!(reserved.contains(&campaign.fleet));
    let fleet = campaign.fleet;
    let mut fleets = campaign.sim.world_mut().resource_mut::<Fleets>();
    for ship in &mut fleets.list[fleet.index()].ships {
        ship.hull = 1e-4;
    }
    // Der Tagesbeginn, an dem die Station die Flotte zerstört; geprüft wird der Stand
    // nach genau diesem Takt.
    campaign.sim.run_days(1);
    assert!(!self::fleet(&campaign.sim, fleet).alive);
    campaign
}

#[test]
fn a_fleet_wrecked_over_a_world_is_released_in_the_same_tick() {
    let campaign = wrecked_over_a_station();
    assert_eq!(check_invariants(campaign.sim.world()), Ok(()));
    assert!(sieges(&campaign.sim).held.is_empty());
    let reserved = &campaign.sim.world().resource::<AiState>().reserved;
    assert!(!reserved.contains(&campaign.fleet));
}

#[test]
fn ships_wrecked_over_a_world_are_counted_and_told() {
    let campaign = wrecked_over_a_station();
    let battles = campaign.sim.world().resource::<Battles>();
    assert_eq!(battles.ships_lost_by(campaign.attacker), 6);
    assert_eq!(battles.fleets_lost_by(campaign.attacker), 1);
    let told = campaign
        .sim
        .world()
        .resource::<sim::chronicle::Chronicle>()
        .entries
        .iter()
        .any(|entry| {
            entry.text.contains("verliert 6 Schiffe") && entry.text.contains("ist verloren")
        });
    assert!(told, "die Chronik nennt den Verlust der Flotte");
}

// --- Ausbau -------------------------------------------------------------------------

#[test]
fn a_planned_station_is_built_from_the_market_in_play() {
    let mut sim = observer();
    sim.run_days(YEARS * DAYS_PER_YEAR);
    let polity = PolityId(0);
    let home = home_of(&sim, polity).0;
    let mut plan = defense(&sim, home).plan.clone();
    plan.stations = 1;
    applied(
        &mut sim,
        polity,
        SimCommand::SetDefensePlan { planet: home, plan },
    );
    let mut years = 0;
    while defense(&sim, home).stations < 1.0 {
        assert!(years < 5, "die Station steht nach fünf Jahren nicht");
        sim.run_days(DAYS_PER_YEAR);
        years += 1;
    }
    assert_eq!(check_invariants(sim.world()), Ok(()));
    assert_eq!(money_drift(sim.world()), Some(0));
}

// --- Plünderung ---------------------------------------------------------------------

/// Kassen einer Welt in Tausendstel Credits: Haushalte, Firmen, Elite.
fn purses(sim: &Headless, planet: PlanetId) -> [i64; 3] {
    let local = sim.world().resource::<Economy>().planet(planet);
    let firms = local.firms.iter().map(|firm| firm.cash).sum();
    [local.households, firms, finance(sim).elites[planet.index()]]
}

fn treasury(sim: &Headless, polity: PolityId) -> i64 {
    sim.world().resource::<Economy>().treasury(polity)
}

/// Sturm auf die nächstgelegene Kolonie mit allen regulären Verbänden der Heimatwelt, bis
/// sie fällt; mit oder ohne Befehl zur Plünderung. `Campaign::day` prüft an jedem Tag
/// Invarianten und Geldsumme: beim Einkauf des Nachschubs, bei der Landung, im Bodenkrieg
/// und am Tag der Eroberung.
fn storm_until_the_fall(sack: bool) -> Campaign {
    let troops = data().balance.siege.home_start.regulars;
    let mut campaign = Campaign::against_a_colony_with(troops, 21);
    assert!(campaign.supply_aboard() > 0.0);
    assert_eq!(money_drift(campaign.sim.world()), Some(0));
    campaign.begin();
    campaign.day();
    campaign.order(SiegeOrders {
        fire: Fire::Support,
        assault: true,
        focus: 0,
        sack,
    });
    campaign.land(troops);
    let mut days = 1;
    while owner(&campaign.sim, campaign.planet) == Some(campaign.defender) {
        campaign.day();
        days += 1;
        assert!(days < 120, "der Sturm endet nie");
    }
    assert_eq!(
        owner(&campaign.sim, campaign.planet),
        Some(campaign.attacker)
    );
    campaign
}

#[test]
fn landing_sack_and_conquest_only_move_money() {
    let balance = &data().balance.siege;
    // Zwillinge: derselbe Sturm, einmal mit und einmal ohne Plünderung. Bis zum Tag des
    // Falls verlaufen beide Partien gleich; an seinem Ende unterscheidet sie nur, was die
    // Plünderung bewegt hat.
    let mut sacked = storm_until_the_fall(true);
    let spared = storm_until_the_fall(false);
    let (attacker, planet) = (sacked.attacker, sacked.planet);
    assert_eq!((attacker, planet), (spared.attacker, spared.planet));
    assert_eq!(sacked.sim.clock(), spared.sim.clock());
    let stats = sieges(&sacked.sim).stats.clone();
    assert_eq!((stats.conquests(), stats.sacked), (1, 1));
    assert_eq!(sieges(&spared.sim).stats.sacked, 0);
    assert!(stats.lost_attackers > 0.0 && stats.lost_defenders > 0.0);
    assert_eq!(
        stats.lost_attackers,
        sieges(&spared.sim).stats.lost_attackers
    );

    // Jede Kasse der Welt hat den Anteil sack_share verloren, auf das Tausendstel, und
    // genau die Summe davon liegt in der Staatskasse des Eroberers. Bei den Firmen wird
    // je Kasse abgerundet: Ihre Summe darf um ein Tausendstel je Firma darunter liegen.
    let firms = sacked
        .sim
        .world()
        .resource::<Economy>()
        .planet(planet)
        .firms
        .len() as i64;
    let (taken, left) = (purses(&sacked.sim, planet), purses(&spared.sim, planet));
    let mut loot = 0;
    for (index, (after, before)) in taken.into_iter().zip(left).enumerate() {
        let part = (before as f64 * balance.sack_share) as i64;
        assert!(part > 0, "jede Kasse der Kolonie hat Geld");
        let slack = if index == 1 { firms } else { 0 };
        let gone = before - after;
        assert!(
            (part - slack..=part).contains(&gone),
            "Kasse {index}: {gone} statt {part}"
        );
        loot += gone;
    }
    assert_eq!(
        treasury(&sacked.sim, attacker),
        treasury(&spared.sim, attacker) + loot
    );
    let credits = sim::economy::to_credits;
    assert!((stats.looted - credits(loot)).abs() < 1e-9);
    println!(
        "geplündert {:.0} Credits; Kassen der verschonten Welt: Haushalte {:.0}, Firmen {:.0}, \
         Elite {:.0}",
        stats.looted,
        credits(left[0]),
        credits(left[1]),
        credits(left[2])
    );
    // Die Plünderung zählt als Kriegsschaden über den des Bodenkriegs hinaus, drückt die
    // Kurse und steht in der Chronik.
    let scar = sacked.defense().scar;
    assert!((scar - spared.defense().scar - balance.sack_scar).abs() < 1e-12);
    assert_eq!(sacked.defense().scar_by, Some(attacker));
    assert_eq!(finance(&sacked.sim).region(planet).war_damage, scar);
    let firm = *finance(&sacked.sim)
        .issuers
        .keys()
        .find(|id| matches!(id, IssuerId::Firm { planet: seat, .. } if *seat == planet))
        .unwrap();
    let shock = |sim: &Headless| finance(sim).issuers[&firm].shock;
    assert!((shock(&sacked.sim) - shock(&spared.sim) * balance.sack_shock).abs() < 1e-12);
    let told = |sim: &Headless| {
        sim.world()
            .resource::<sim::chronicle::Chronicle>()
            .entries
            .iter()
            .any(|entry| entry.text.contains("plündert"))
    };
    assert!(told(&sacked.sim) && !told(&spared.sim));
    // Auch danach stimmt die Geldsumme an jedem Tag.
    for _ in 0..60 {
        sacked.day();
    }
}

// --- Übergabe -----------------------------------------------------------------------

#[test]
fn a_colony_is_handed_over_and_the_rest_of_the_troops_return_home() {
    let balance = &data().balance.siege;
    let mut campaign = Campaign::against_a_colony();
    let (attacker, defender, planet) = (campaign.attacker, campaign.defender, campaign.planet);
    let (home, home_system) = home_of(&campaign.sim, attacker);
    let name = campaign.sim.galaxy().planet(planet).name.clone();
    // Ohne Belagerung gibt es nichts zu übergeben.
    let reason = rejection(
        &mut campaign.sim,
        defender,
        SimCommand::Capitulate { planet },
    );
    assert_eq!(reason, format!("{name} wird nicht belagert"));
    campaign.begin();
    for _ in 0..5 {
        campaign.day();
    }
    // Der Belagerer will plündern; nach einer Übergabe unterbleibt das.
    campaign.order(SiegeOrders {
        sack: true,
        ..Default::default()
    });
    let siege = campaign.siege().expect("die Belagerung läuft");
    let stake = siege.stake();
    let militia = campaign.defense().militia;
    assert!(militia > 1.0, "die Miliz steht noch: {militia}");
    let population = campaign.sim.galaxy().planet(planet).population;
    let needed = occupation_force(population, balance);
    assert!(needed < TROOPS);

    applied(
        &mut campaign.sim,
        defender,
        SimCommand::Capitulate { planet },
    );
    // Die Besatzungstruppe ist an Land, der Besitzwechsel steht bis zum Tagesabschluss aus.
    assert_eq!(sieges(&campaign.sim).pending.len(), 1);
    assert!((campaign.siege().unwrap().army - needed).abs() < 1e-9);
    assert!((campaign.aboard() - (TROOPS - needed)).abs() < 1e-9);
    assert_eq!(owner(&campaign.sim, planet), Some(defender));
    assert_eq!(check_invariants(campaign.sim.world()), Ok(()));
    let reason = rejection(
        &mut campaign.sim,
        defender,
        SimCommand::Capitulate { planet },
    );
    assert_eq!(reason, format!("die Übergabe von {name} steht aus"));
    campaign.day();

    assert_eq!(owner(&campaign.sim, planet), Some(attacker));
    let state = sieges(&campaign.sim);
    assert!(state.active.is_empty() && state.pending.is_empty());
    assert_eq!((state.stats.capitulated, state.stats.conquests()), (1, 1));
    assert_eq!(
        (state.stats.lost_attackers, state.stats.lost_defenders),
        (0.0, 0.0)
    );
    // Mildere Besatzung als nach einer Eroberung mit Gewalt; keine Schäden.
    let defense = campaign.defense();
    let occupation = (1.0 - stake).max(balance.occupation_min) * balance.capitulation_mercy;
    assert!((defense.occupation - occupation).abs() < 1e-9);
    assert!(defense.occupation < 1.0);
    assert_eq!(defense.occupied_from, Some(defender));
    assert_eq!(defense.scar, 0.0);
    assert!((defense.regulars - needed).abs() < 1e-9);
    let told = campaign
        .sim
        .world()
        .resource::<sim::chronicle::Chronicle>()
        .entries
        .iter()
        .any(|entry| entry.text.contains("übergeben") && entry.text.contains(&name));
    assert!(told, "die Chronik nennt die Übergabe");

    // Der Rest der Truppen fährt heim und geht dort wieder an Land.
    let fleet = campaign.fleet;
    let at_home = defense_of_home(&campaign.sim, home);
    let reason = rejection(
        &mut campaign.sim,
        attacker,
        SimCommand::LandTroops {
            fleet,
            planet: home,
            strength: 1.0,
        },
    );
    assert!(reason.contains("liegt nicht im System"), "{reason}");
    applied(
        &mut campaign.sim,
        attacker,
        SimCommand::MoveFleet {
            fleet,
            to: home_system,
        },
    );
    sail(&mut campaign.sim, fleet, home_system);
    let aboard = campaign.aboard();
    assert!((aboard - (TROOPS - needed)).abs() < 1e-9);
    applied(
        &mut campaign.sim,
        attacker,
        SimCommand::LandTroops {
            fleet,
            planet: home,
            strength: aboard,
        },
    );
    assert!((defense_of_home(&campaign.sim, home) - (at_home + aboard)).abs() < 1e-9);
    assert!(!sieges(&campaign.sim).embarked.contains_key(&fleet));
    campaign.day();
}

/// Reguläre Verbände auf einer Welt.
fn defense_of_home(sim: &Headless, home: PlanetId) -> f64 {
    defense(sim, home).regulars
}

// --- Folgen für Wirtschaft, Kurse, Zusammenhalt und Kriegsmüdigkeit -------------------

/// Ausstoß aller Werke einer Welt am letzten gerechneten Tag.
fn output(sim: &Headless, planet: PlanetId) -> f64 {
    let economy = sim.world().resource::<Economy>();
    economy.planet(planet).output.iter().sum()
}

/// Börsenwert der Betriebe einer Welt.
fn local_market_value(sim: &Headless, planet: PlanetId) -> f64 {
    let capitals: Vec<_> = sim.polities().list.iter().map(|p| p.capital).collect();
    issuers_of_region(
        finance(sim),
        sim.galaxy(),
        sim.world().resource::<Logistics>(),
        &capitals,
        planet,
    )
    .filter(|(id, _)| matches!(id, IssuerId::Firm { .. }))
    .map(|(_, issuer)| issuer.market_value())
    .sum()
}

/// Tage vor dem Sturm. Mit dem Sturm zusammen bleibt die Belagerung unter einem Monat:
/// Sonst überdeckte die Kriegsmüdigkeit jedes vollen Belagerungsmonats (`wear_month`) den
/// Vergleich.
const PRELUDE: u64 = 8;

/// Bis zu [`PRELUDE`] Tage vor der Kolonie mit der Feuerstufe `prelude`, dann der Sturm mit
/// allen Verbänden bis zum Fall. Gibt den Feldzug am Tag der Eroberung zurück.
fn storm_after(prelude: Fire) -> Campaign {
    let troops = data().balance.siege.home_start.regulars;
    let mut campaign = Campaign::against_a_colony_with(troops, 60);
    // Ausgangslage: Der Belagerer hat einen langen Krieg hinter sich. So bleibt sichtbar,
    // was der Feldzug an Kriegsmüdigkeit kostet, auch wenn die Eroberung sie senkt.
    campaign.sim.world_mut().resource_mut::<Finance>().polities[campaign.attacker.index()]
        .war_weariness = 20.0;
    campaign.begin();
    campaign.day();
    campaign.order(SiegeOrders {
        fire: prelude,
        ..Default::default()
    });
    for _ in 0..PRELUDE {
        campaign.day();
        if campaign.siege().is_none() {
            break;
        }
    }
    // Hält die Welt noch, wird sie gestürmt. Unter Flächenbeschuss gibt die Miliz einer
    // Kolonie ohne Vorräte schon vorher auf; dann ist die Besatzung ohne Kampf gelandet.
    if campaign.siege().is_some() {
        campaign.order(SiegeOrders {
            fire: Fire::Support,
            assault: true,
            focus: 0,
            sack: false,
        });
        campaign.land(troops);
    }
    let mut days = 0;
    while owner(&campaign.sim, campaign.planet) == Some(campaign.defender) {
        campaign.day();
        days += 1;
        assert!(days < 120, "der Sturm endet nie");
    }
    assert_eq!(
        owner(&campaign.sim, campaign.planet),
        Some(campaign.attacker)
    );
    campaign
}

#[test]
fn bombardment_weighs_on_the_conquered_world_for_a_year() {
    // Zwillinge: derselbe Feldzug, einmal mit Flächenbeschuss vor dem Sturm, einmal mit
    // Warten. Verglichen wird am selben Tag der Partie.
    let mut bombed = storm_after(Fire::Area);
    let mut spared = storm_after(Fire::Hold);
    let day = |campaign: &Campaign| campaign.sim.clock().day();
    while day(&bombed) < day(&spared) {
        bombed.day();
    }
    while day(&spared) < day(&bombed) {
        spared.day();
    }
    assert!(sieges(&spared.sim).stats.days < 30);
    assert_eq!(
        (bombed.attacker, bombed.planet),
        (spared.attacker, spared.planet)
    );
    let (attacker, planet) = (bombed.attacker, bombed.planet);
    let weariness = |campaign: &Campaign| finance(&campaign.sim).polity(attacker).war_weariness;
    // Am Tag des Falls: Der Beschuss hat Schäden hinterlassen, die der Eroberer
    // verantwortet, und ihn selbst Kriegsmüdigkeit gekostet.
    let (scar, occupation) = (bombed.defense().scar, bombed.defense().occupation);
    println!(
        "am Tag {}: Kriegsmüdigkeit {:.3} gegen {:.3}, Kriegsschäden {:.2} gegen {:.2}",
        day(&bombed),
        weariness(&bombed),
        weariness(&spared),
        scar,
        spared.defense().scar
    );
    assert!(scar > 0.2 && scar > 5.0 * spared.defense().scar);
    assert_eq!(bombed.defense().scar_by, Some(attacker));
    assert!(occupation > 0.5);
    assert!(weariness(&bombed) > weariness(&spared));
    let population = |campaign: &Campaign| campaign.sim.galaxy().planet(planet).population;
    assert!(population(&bombed) < 0.7 * population(&spared));

    // Zwölf Monate später.
    for campaign in [&mut bombed, &mut spared] {
        for _ in 0..12 {
            campaign.sim.run_days(30);
            assert_eq!(check_invariants(campaign.sim.world()), Ok(()));
            assert_eq!(money_drift(campaign.sim.world()), Some(0));
        }
    }
    let region = |campaign: &Campaign| finance(&campaign.sim).region(planet).clone();
    let (hit, kept) = (region(&bombed), region(&spared));
    println!(
        "nach zwölf Monaten: Zusammenhalt {:.1} gegen {:.1}, Börsenwert {:.0} gegen {:.0}, \
         Kriegsmüdigkeit {:.2} gegen {:.2}, Kriegsschäden {:.2} gegen {:.2}",
        hit.cohesion,
        kept.cohesion,
        local_market_value(&bombed.sim, planet),
        local_market_value(&spared.sim, planet),
        weariness(&bombed),
        weariness(&spared),
        hit.war_damage,
        kept.war_damage,
    );
    assert!(hit.cohesion < kept.cohesion);
    // Gemessen an den Firmen der Welt: Das Kontor ihres Systems lebt seit der Eroberung
    // auch von den Einkäufen der Garnison und den Einlagen des neuen Staates.
    assert!(local_market_value(&bombed.sim, planet) < local_market_value(&spared.sim, planet));
    assert!(weariness(&bombed) > weariness(&spared));
    // Die Bücher des Monats: Schäden und Besatzung klingen ab, sind aber nicht vergessen,
    // und die Region spiegelt sie; daraus rechnet der Zusammenhalt seine Posten.
    let defense = bombed.defense();
    assert!(defense.scar > 0.0 && defense.scar < scar);
    assert!(defense.occupation > 0.0 && defense.occupation < occupation);
    assert_eq!(hit.war_damage, defense.scar);
    assert_eq!(hit.occupation, defense.occupation);
    assert!(hit.war_damage > kept.war_damage);
    let points = |region: &sim::finance::Region, name: &str| {
        region
            .factors
            .contributions(&data().balance.finance.cohesion)
            .into_iter()
            .find(|(label, _)| *label == name)
            .map(|(_, points)| points)
            .unwrap()
    };
    assert!(points(&hit, "Kriegsschäden") < points(&kept, "Kriegsschäden"));
    assert!(points(&hit, "Besatzung") < 0.0 && points(&kept, "Besatzung") < 0.0);
    // Die neue Miliz der Welt wächst mit dem Zusammenhalt nach.
    assert!(defense.militia > 0.0);
}

#[test]
fn ground_war_in_the_center_idles_the_economy_until_it_is_over() {
    // Zwillinge: dieselbe Belagerung, einmal mit Landung und Sturm, einmal ohne.
    let troops = data().balance.siege.home_start.regulars;
    let start = || {
        let mut campaign = Campaign::against_a_colony_with(troops, 60);
        campaign.begin();
        campaign.day();
        campaign
    };
    let (mut fought, mut quiet) = (start(), start());
    let planet = fought.planet;
    assert_eq!(planet, quiet.planet);
    let before = output(&fought.sim, planet);
    assert!(before > 0.0 && before == output(&quiet.sim, planet));
    fought.order(SiegeOrders {
        fire: Fire::Hold,
        assault: true,
        focus: 0,
        sack: false,
    });
    fought.land(troops);
    // Bis im Zentrum gekämpft wird.
    let center = |campaign: &Campaign| {
        let siege = campaign.siege().expect("die Belagerung läuft");
        siege
            .report
            .zone
            .is_some_and(|zone| siege.zones[zone as usize].kind == sim::siege::ZoneKind::Center)
    };
    let mut days = 0;
    while !center(&fought) {
        fought.day();
        quiet.day();
        days += 1;
        assert!(days < 60, "der Sturm erreicht das Zentrum nie");
    }
    // Der Stillstand des Kampftags trifft die Produktion des nächsten Tages.
    let level = |campaign: &Campaign| {
        let economy = campaign.sim.world().resource::<Economy>();
        economy.planet(planet).disruption
    };
    assert!(level(&fought) > 0.3 && level(&quiet) == 0.0);
    fought.day();
    quiet.day();
    let (idle, busy) = (output(&fought.sim, planet), output(&quiet.sim, planet));
    println!("Ausstoß im Bodenkrieg {idle:.2}, ohne {busy:.2}");
    assert!(idle < 0.7 * busy, "Ausstoß {idle} gegen {busy}");

    // Beide heben auf; die Truppe schifft sich wieder ein. Sechzig Tage später läuft die
    // Wirtschaft wieder.
    for campaign in [&mut fought, &mut quiet] {
        if campaign.siege().is_some() {
            let (attacker, planet) = (campaign.attacker, campaign.planet);
            applied(
                &mut campaign.sim,
                attacker,
                SimCommand::LiftSiege { planet },
            );
        }
        for _ in 0..60 {
            campaign.day();
        }
    }
    assert_eq!(owner(&fought.sim, planet), Some(fought.defender));
    assert!(level(&fought) < 0.01);
    let (after, twin) = (output(&fought.sim, planet), output(&quiet.sim, planet));
    println!("sechzig Tage nach der Aufhebung {after:.2}, ohne Bodenkrieg {twin:.2}");
    assert!(after >= 0.9 * twin, "Ausstoß {after} gegen {twin}");
}

// --- Alle fünf Phasen ---------------------------------------------------------------

#[test]
fn a_siege_runs_through_all_five_phases_to_the_hand_over() {
    let troops = data().balance.siege.home_start.regulars;
    // Ausgangslage der Kolonie: eine angeschlagene Station, ein Bunker, zwei reguläre
    // Verbände und Vorräte für zwei Monate. Anlagen und Garnisonen lassen sich in der
    // Partie noch nicht aufbauen.
    let mut campaign = Campaign::against_a_prepared_colony(troops, 90, |sim, planet| {
        let mut sieges = sim.world_mut().resource_mut::<Sieges>();
        let defense = sieges.defense_mut(planet);
        defense.stations = 0.3;
        defense.plan.stations = 1;
        defense.bunkers = 1;
        defense.plan.bunkers = 1;
        defense.regulars = 2.0;
        defense.plan.regulars = 2.0;
        defense.depot = 60.0;
        defense.plan.depot_days = 60;
    });
    let (attacker, defender, planet) = (campaign.attacker, campaign.defender, campaign.planet);
    // Der Verteidiger zieht seine Regulären in den Bunker zurück.
    applied(
        &mut campaign.sim,
        defender,
        SimCommand::SetGarrisonOrders {
            planet,
            posture: Posture::Deep,
            hold_fire: false,
        },
    );
    campaign.begin();
    let mut seen: Vec<Phase> = Vec::new();
    // In jeder Phase wird einmal gespeichert und geladen: Der geladene Stand trägt dieselbe
    // Prüfsumme und läuft am nächsten Tag genauso weiter wie die Partie selbst.
    let mut resumed: Option<Headless> = None;
    let mut reloads = 0;
    let mut note = |campaign: &Campaign, seen: &mut Vec<Phase>, resumed: &mut Option<Headless>| {
        if let Some(siege) = campaign.siege() {
            let phase = siege.phase(campaign.defense());
            if seen.last() != Some(&phase) {
                println!("{}", campaign.report());
                seen.push(phase);
                let text = campaign.sim.save().to_ron().unwrap();
                let twin = Headless::from_save(data().clone(), SaveGame::from_ron(&text).unwrap());
                assert_eq!(twin.hash(), campaign.sim.hash(), "{}", phase.label());
                *resumed = Some(twin);
                reloads += 1;
            }
        }
    };
    let day = |campaign: &mut Campaign, resumed: &mut Option<Headless>| {
        campaign.day();
        if let Some(mut twin) = resumed.take() {
            twin.run_days(1);
            assert_eq!(
                twin.hash(),
                campaign.sim.hash(),
                "Tag {}: der geladene Stand läuft anders weiter",
                campaign.sim.clock().day()
            );
        }
    };
    // Anmarsch: Die Flotte kämpft die Station nieder; das System bleibt offen.
    day(&mut campaign, &mut resumed);
    note(&campaign, &mut seen, &mut resumed);
    assert_eq!(seen, [Phase::Approach]);
    assert!(!campaign.blockaded());
    let mut days = 0;
    while campaign.defense().stations > 0.0 {
        day(&mut campaign, &mut resumed);
        days += 1;
        assert!(days < 60, "die Station fällt nie");
    }
    // Einschließung: Die Garnison zehrt von ihren Vorräten.
    note(&campaign, &mut seen, &mut resumed);
    let stores = campaign.defense().depot;
    for _ in 0..3 {
        day(&mut campaign, &mut resumed);
    }
    assert!(campaign.defense().depot < stores);
    assert!(campaign.blockaded());
    // Beschuss: gezielt, einige Tage.
    campaign.order(SiegeOrders {
        fire: Fire::Precise,
        ..Default::default()
    });
    note(&campaign, &mut seen, &mut resumed);
    for _ in 0..5 {
        day(&mut campaign, &mut resumed);
    }
    assert!(campaign.siege().unwrap().report.fire_damage > 0.0);
    // Bodenkrieg: Landung und Sturm mit Stützfeuer.
    campaign.order(SiegeOrders {
        fire: Fire::Support,
        assault: true,
        focus: 0,
        sack: false,
    });
    campaign.land(troops);
    note(&campaign, &mut seen, &mut resumed);
    // Untergrund, sobald die Oberfläche verloren ist, und zuletzt die Übergabe.
    let mut days = 0;
    while owner(&campaign.sim, planet) == Some(defender) {
        day(&mut campaign, &mut resumed);
        note(&campaign, &mut seen, &mut resumed);
        days += 1;
        assert!(days < 200, "die Belagerung endet nie");
    }
    println!("{}", campaign.report());
    assert_eq!(reloads, 5, "in jeder Phase einmal gespeichert und geladen");
    assert_eq!(
        seen,
        [
            Phase::Approach,
            Phase::Investment,
            Phase::Bombardment,
            Phase::Ground,
            Phase::Underground
        ]
    );
    // Die Welt ist gestürmt und übergeben: Sie gehört dem Belagerer, seine Truppe ist
    // ihre Garnison, die Belagerung ist geschlossen, und kein Geld ist entstanden oder
    // vergangen (`Campaign::day` prüft Invarianten und Geldsumme an jedem Tag).
    assert_eq!(owner(&campaign.sim, planet), Some(attacker));
    let state = sieges(&campaign.sim);
    assert!(state.active.is_empty() && state.pending.is_empty() && state.assign.is_empty());
    assert_eq!((state.stats.stormed, state.stats.conquests()), (1, 1));
    assert!(state.stats.lost_attackers > 0.0 && state.stats.lost_defenders > 0.0);
    let defense = campaign.defense();
    assert!(defense.regulars > 0.0 && defense.regulars < troops);
    assert_eq!(defense.occupied_from, Some(defender));
    assert_eq!(defense.bunkers, 1);
    let region = finance(&campaign.sim).region(planet);
    assert_eq!(
        (region.stage, region.owner_seen),
        (Stage::Restless, Some(attacker))
    );
    assert!(!region.besieged && region.occupation > 0.0);
    assert_eq!(money_drift(campaign.sim.world()), Some(0));
    // Ein Monat danach: Die Partie läuft weiter.
    for _ in 0..30 {
        day(&mut campaign, &mut resumed);
    }
}

// --- Militärische Führung der KI ------------------------------------------------------

#[test]
fn the_fleet_ai_leaves_besieging_fleets_alone() {
    let mut campaign = Campaign::against_a_colony();
    let (attacker, planet, fleet) = (campaign.attacker, campaign.planet, campaign.fleet);
    let reserved = |campaign: &Campaign| {
        campaign
            .sim
            .world()
            .resource::<sim::ai::AiState>()
            .is_reserved(fleet)
    };
    assert!(!reserved(&campaign));
    campaign.begin();
    assert!(reserved(&campaign), "mit dem Befehl, im selben Takt");
    // Die militärische Führung der KI übernimmt den Belagerer. Die belagernde Flotte
    // lässt sie liegen: Die Belagerung läuft Tag für Tag weiter.
    campaign
        .sim
        .set_military_mode(Some(attacker), MilitaryMode::Active);
    for _ in 0..40 {
        campaign.day();
        let besieger = fleets(&campaign.sim).get(fleet).unwrap();
        assert!(besieger.route.is_empty() && besieger.system == campaign.system);
        assert_eq!(sieges(&campaign.sim).assign.get(&fleet), Some(&planet));
        assert!(reserved(&campaign));
    }
    assert!(campaign.siege().is_some());
    // Hebt der Belagerer auf, gehört die Flotte wieder der Flotten-KI, und die gibt ihr
    // bald neue Befehle.
    applied(
        &mut campaign.sim,
        attacker,
        SimCommand::LiftSiege { planet },
    );
    assert!(!reserved(&campaign));
    assert!(sieges(&campaign.sim).held.is_empty());
    let mut days = 0;
    loop {
        campaign.day();
        days += 1;
        let besieger = fleets(&campaign.sim).get(fleet).unwrap();
        if !besieger.route.is_empty() || besieger.system != campaign.system {
            break;
        }
        assert!(days < 60, "die Flotten-KI rührt die freie Flotte nicht an");
    }
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

// --- Eroberungszug nach Drehbuch ---------------------------------------------------------

#[test]
fn the_conquest_script_takes_worlds_and_ends_a_community() {
    // Das Drehbuch „Eroberung" in zwei ganzen Partien: Gemeinschaft 0 hebt Truppen aus,
    // erklärt im Jahr 25 dem nächsten Nachbarn den Krieg und belagert erst dessen nächste
    // Welt, dann seinen Sitz. Die Flotten des Nachbarn führt die KI.
    let script = WarScript::new(conquest_war());
    let attacker = PolityId(0);
    let war = 25 * DAYS_PER_YEAR;
    let mut ended = 0;
    for seed in [2, 9] {
        let mut sim = Headless::new(data().clone(), SimConfig { seed, player: None });
        script.run_until(&mut sim, war, false);
        assert_eq!(check_invariants(sim.world()), Ok(()), "Seed {seed}");
        let enemy = nearest_neighbour(sim.galaxy(), sim.polities(), attacker).unwrap();
        let worlds = sim.galaxy().planets_of(enemy).count();
        assert!(worlds > 1, "Seed {seed}: der Nachbar hält Kolonien");
        // Fünf Jahre Krieg, Tag für Tag: Kein Tag schafft oder vernichtet Geld, auch nicht
        // der einer Übergabe, einer Plünderung oder des Endes einer Gemeinschaft.
        let mut seats_fallen = 0;
        let mut seat = sim.polities().get(enemy).capital;
        for day in war + 1..=war + 5 * DAYS_PER_YEAR {
            script.run_until(&mut sim, day, false);
            assert_eq!(money_drift(sim.world()), Some(0), "Seed {seed}, Tag {day}");
            // Auch die Invarianten halten an jedem Tagesbeginn: an dem Tag, an dem eine
            // Flotte über einer Welt verloren geht, wie an dem, an dem ein Sitz fällt.
            assert_eq!(
                check_invariants(sim.world()),
                Ok(()),
                "Seed {seed}, Tag {day}"
            );
            let now = sim.polities().get(enemy).capital;
            if now != seat {
                seats_fallen += 1;
                seat = now;
            }
        }
        let stats = sieges(&sim).stats.clone();
        println!("Seed {seed}: {stats:?}, {seats_fallen} Hauptwelten gefallen");
        // Eine Welt ist gefallen, und jede gefallene steht beim Eroberer.
        assert!(stats.fallen >= 1, "Seed {seed}: {stats:?}");
        assert_eq!(stats.fallen, stats.conquests(), "Seed {seed}");
        let taken = sim
            .galaxy()
            .planets_of(attacker)
            .filter(|planet| sieges(&sim).defense(planet.id).occupied_from == Some(enemy))
            .count();
        assert!(taken >= 1, "Seed {seed}");
        assert_eq!(
            summarize(&sim).sieges,
            stats,
            "die Kennzahlen stehen im Bericht"
        );
        let foe = sim.polities().get(enemy);
        if !foe.alive {
            // Der Sitz ist gefallen, und mit der letzten Welt hat die Gemeinschaft geendet.
            ended += 1;
            assert!(seats_fallen >= 1, "Seed {seed}");
            assert_eq!(stats.ended, 1, "Seed {seed}");
            assert_eq!(foe.capital, None);
            assert_eq!(sim.galaxy().planets_of(enemy).count(), 0);
            assert_eq!(fleets(&sim).of(enemy).count(), 0);
            assert!(!sim.world().resource::<Diplomacy>().is_at_war(attacker));
            let state = sieges(&sim);
            assert!(state.active.is_empty() && state.pending.is_empty());
            assert!(sim.siege_cards().is_empty());
        }
        // Ein Jahr danach: Die Partie läuft weiter.
        script.run_until(&mut sim, war + 6 * DAYS_PER_YEAR, false);
        assert_eq!(check_invariants(sim.world()), Ok(()), "Seed {seed}");
    }
    assert!(
        ended >= 1,
        "in mindestens einer Partie endet die Gemeinschaft des Nachbarn"
    );
}

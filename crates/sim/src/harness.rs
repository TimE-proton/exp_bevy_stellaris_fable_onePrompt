//! Läufe ohne Grafik: Grundlage für Tests und das `headless`-Programm.

use crate::clock::{SimClock, TICKS_PER_DAY};
use crate::command::{CommandLog, CommandQueue, CommandSource, ReplayScript, SimCommand};
use crate::data::GameData;
use crate::fleet::{Fleets, Stance};
use crate::galaxy::Galaxy;
use crate::ids::{FleetId, PolityId};
use crate::polity::Polities;
use crate::state::{SaveGame, SimConfig, SimState, hash_world, new_game};
use crate::tech::Research;
use crate::{SimPlugins, needs_fine_ticks, sim_schedule};
use bevy_app::App;
use bevy_ecs::prelude::*;

/// Eine Partie ohne Fenster. Ruft das Simulations-Schedule direkt auf, ohne Wanduhr.
pub struct Headless {
    app: App,
}

impl Headless {
    pub fn new(data: GameData, config: SimConfig) -> Self {
        let mut sim = Self::empty(data);
        new_game(sim.app.world_mut(), config);
        sim
    }

    pub fn from_save(data: GameData, save: SaveGame) -> Self {
        let mut sim = Self::empty(data);
        save.state.install(sim.app.world_mut());
        sim
    }

    fn empty(data: GameData) -> Self {
        let mut app = App::new();
        app.insert_resource(data).add_plugins(SimPlugins);
        app.finish();
        app.cleanup();
        Self { app }
    }

    pub fn world(&self) -> &World {
        self.app.world()
    }

    pub fn world_mut(&mut self) -> &mut World {
        self.app.world_mut()
    }

    pub fn clock(&self) -> SimClock {
        *self.world().resource::<SimClock>()
    }

    pub fn galaxy(&self) -> &Galaxy {
        self.world().resource::<Galaxy>()
    }

    pub fn polities(&self) -> &Polities {
        self.world().resource::<Polities>()
    }

    pub fn research(&self) -> &Research {
        self.world().resource::<Research>()
    }

    pub fn hash(&self) -> u64 {
        hash_world(self.world()).expect("Partie läuft")
    }

    pub fn state(&self) -> SimState {
        SimState::capture(self.world()).expect("Partie läuft")
    }

    pub fn save(&self) -> SaveGame {
        SaveGame::capture(self.world()).expect("Partie läuft")
    }

    pub fn command_log(&self) -> &CommandLog {
        self.world().resource::<CommandLog>()
    }

    /// Stellt einen Befehl ein, als käme er vom Spieler.
    pub fn issue(&mut self, polity: PolityId, command: SimCommand) {
        self.world_mut().resource_mut::<CommandQueue>().push(
            polity,
            CommandSource::Player,
            command,
        );
    }

    /// Spielt ein Befehlsprotokoll ab (Wiedergabe einer früheren Partie).
    pub fn set_replay(&mut self, log: &CommandLog) {
        self.world_mut()
            .insert_resource(ReplayScript::new(log.entries.clone()));
    }

    /// Genau ein Takt.
    pub fn tick(&mut self) {
        self.app.world_mut().run_schedule(sim_schedule());
    }

    /// Läuft `days` Tage weiter und überspringt Takte, in denen nichts geschieht.
    pub fn run_days(&mut self, days: u64) {
        let target = self.clock().tick + days * TICKS_PER_DAY;
        while self.clock().tick < target {
            self.tick();
            let tick = self.clock().tick;
            if tick.is_multiple_of(TICKS_PER_DAY) || needs_fine_ticks(self.world()) {
                continue;
            }
            let mut jump = (tick / TICKS_PER_DAY + 1) * TICKS_PER_DAY;
            if let Some(next) = self
                .world()
                .get_resource::<ReplayScript>()
                .and_then(|s| s.next_tick())
            {
                jump = jump.min(next.max(tick));
            }
            self.world_mut().resource_mut::<SimClock>().tick = jump.min(target);
        }
    }

    /// Läuft `days` Tage weiter und rechnet jeden Takt, wie es das Spiel tut.
    pub fn run_days_exact(&mut self, days: u64) {
        for _ in 0..days * TICKS_PER_DAY {
            self.tick();
        }
    }
}

/// Volkswirtschaftliche Kennzahlen der ganzen Galaxie.
#[derive(Clone, Debug, Default)]
pub struct EconomyStats {
    /// Bevölkerung in Millionen.
    pub population: f64,
    pub capital: f64,
    /// Anteil der Arbeitskräfte in Arbeit.
    pub employment: f64,
    /// Kapitalgewichtete Auslastung der Werke.
    pub utilization: f64,
    /// Bevölkerungsgewichtete Deckung des Lebensnotwendigen.
    pub essentials_met: f64,
    /// Mittleres Verhältnis von Preis zu Richtpreis auf belebten Märkten.
    pub price_level: f64,
    /// Mittlere Preisspanne zwischen teuerstem und billigstem belebten Markt je Gut.
    pub price_spread: f64,
    /// Anteil der Preise, die an Boden oder Decke kleben.
    pub pinned_prices: f64,
    pub mean_wage: f64,
    pub freighters: usize,
    pub laden_freighters: usize,
    /// Abweichung der Geldmenge vom Soll in Tausendstel Credits; muss null sein.
    pub money_drift: i64,
    /// Summe der Staatskassen in Credits.
    pub treasuries: f64,
    /// Geldverteilung in Credits: Haushalte, Firmen, Kontore, Handelshäuser.
    pub household_money: f64,
    pub firm_money: f64,
    pub market_money: f64,
    pub house_money: f64,
    pub elite_money: f64,
    /// Bevölkerungsgewichteter Zusammenhalt der besiedelten Welten.
    pub cohesion: f64,
    pub restless_regions: usize,
    pub autonomous_regions: usize,
    pub secessions: u32,
    /// Lebende Gemeinschaften.
    pub polities_alive: usize,
    /// Mittlerer Anteil der Einheimischen bzw. des Zentrums an den Betrieben der Kolonien.
    pub colony_local_share: f64,
    pub colony_center_share: f64,
    /// Summe aller Staatsschulden in Credits.
    pub debt: f64,
    pub anomaly: Option<String>,
}

pub fn economy_stats(world: &World) -> EconomyStats {
    let data = world.resource::<GameData>();
    let galaxy = world.resource::<Galaxy>();
    let economy = world.resource::<crate::economy::Economy>();
    let logistics = world.resource::<crate::logistics::Logistics>();
    let balance = &data.balance.economy;
    let mut stats = EconomyStats::default();
    let (mut labor, mut employed, mut used_capital, mut fed, mut wages) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for planet in &galaxy.planets {
        if planet.owner.is_none() || planet.population <= 0.0 {
            continue;
        }
        let local = economy.planet(planet.id);
        let supply = planet.population * balance.workforce_share;
        stats.population += planet.population;
        labor += supply;
        employed += supply.min(local.labor_demand);
        fed += planet.population * local.essentials_met(balance);
        wages += planet.population * local.wage;
        for (capital, utilization) in local.capital.iter().zip(&local.utilization) {
            stats.capital += capital;
            used_capital += capital * utilization;
        }
    }
    stats.employment = employed / labor.max(1e-9);
    stats.utilization = used_capital / stats.capital.max(1e-9);
    stats.essentials_met = fed / stats.population.max(1e-9);
    stats.mean_wage = wages / stats.population.max(1e-9);

    let live: Vec<usize> = galaxy
        .systems
        .iter()
        .filter(|system| galaxy.system_owner(system.id).is_some())
        .map(|system| system.index())
        .collect();
    let (mut level, mut level_count, mut spread, mut spread_count, mut pinned) =
        (0.0, 0, 0.0, 0, 0);
    for (good, def) in data.goods.iter().enumerate() {
        let (mut low, mut high) = (f64::MAX, 0.0f64);
        for index in &live {
            let ratio = economy.markets[*index].price[good] / def.base_price;
            level += ratio;
            level_count += 1;
            low = low.min(ratio);
            high = high.max(ratio);
            if ratio <= balance.price_floor * 1.02 || ratio >= balance.price_ceiling * 0.98 {
                pinned += 1;
            }
        }
        if !live.is_empty() {
            spread += high / low;
            spread_count += 1;
        }
    }
    stats.price_level = level / (level_count.max(1) as f64);
    stats.price_spread = spread / (spread_count.max(1) as f64);
    stats.pinned_prices = pinned as f64 / (level_count.max(1) as f64);
    stats.freighters = logistics.active_freighters();
    stats.laden_freighters = logistics
        .freighters
        .iter()
        .filter(|f| f.alive && f.cargo.is_some())
        .count();
    stats.money_drift = crate::state::money_drift(world).unwrap_or(0);
    stats.treasuries = economy
        .treasuries
        .iter()
        .map(|cash| crate::economy::to_credits(*cash))
        .sum();
    let credits = crate::economy::to_credits;
    stats.household_money = economy.planets.iter().map(|p| credits(p.households)).sum();
    stats.firm_money = economy
        .planets
        .iter()
        .flat_map(|p| p.firms.iter())
        .map(|firm| credits(firm.cash))
        .sum();
    stats.market_money = economy
        .markets
        .iter()
        .map(|market| credits(market.cash))
        .sum();
    stats.house_money = credits(logistics.money());
    let finance = world.resource::<crate::finance::Finance>();
    let polities = world.resource::<Polities>();
    stats.elite_money = credits(finance.money());
    let (mut weight, mut weighted) = (0.0, 0.0);
    let (mut colonies, mut local, mut center) = (0.0, 0.0, 0.0);
    for planet in &galaxy.planets {
        let Some(owner) = planet.owner else {
            continue;
        };
        let region = finance.region(planet.id);
        weight += planet.population;
        weighted += planet.population * region.cohesion;
        match region.stage {
            crate::finance::Stage::Loyal => {}
            crate::finance::Stage::Restless => stats.restless_regions += 1,
            crate::finance::Stage::Autonomous => stats.autonomous_regions += 1,
        }
        if polities.get(owner).capital != Some(planet.id) {
            colonies += 1.0;
            local += region.factors.local_share;
            center += region.factors.center_share;
        }
    }
    stats.cohesion = weighted / f64::max(weight, 1e-9);
    stats.colony_local_share = local / f64::max(colonies, 1.0);
    stats.colony_center_share = center / f64::max(colonies, 1.0);
    stats.secessions = finance.secessions;
    stats.polities_alive = polities.alive().count();
    stats.debt = finance
        .bonds
        .iter()
        .map(|bond| credits(bond.principal))
        .sum();
    stats.anomaly = economy.find_anomaly();
    stats
}

/// Harte Invarianten. Ein Verstoß ist ein Programmfehler, kein Balanceproblem.
pub fn check_invariants(world: &World) -> Result<(), String> {
    let stats = economy_stats(world);
    if stats.money_drift != 0 {
        return Err(format!(
            "Geldmenge weicht um {} Tausendstel Credits ab",
            stats.money_drift
        ));
    }
    if let Some(anomaly) = stats.anomaly {
        return Err(anomaly);
    }
    // Flotten: Schiffe in ihren Grenzen, keine Flotte ohne Schiffe, Nummern eindeutig,
    // die Stärke passend zu den Schiffen.
    let fleets = world.resource::<crate::fleet::Fleets>();
    if let Some(anomaly) = fleets.find_anomaly(world.resource::<GameData>()) {
        return Err(anomaly);
    }
    // Gefechte: Jede Flotte im Gefecht gehört zu genau einem, das läuft, und umgekehrt.
    let battles = world.resource::<crate::fleet::Battles>();
    if let Some(anomaly) = battles.find_anomaly(fleets) {
        return Err(anomaly);
    }
    let galaxy = world.resource::<Galaxy>();
    for planet in &galaxy.planets {
        if !planet.population.is_finite() || planet.population < 0.0 {
            return Err(format!(
                "{}: Bevölkerung {}",
                planet.name, planet.population
            ));
        }
        if planet.population > planet.capacity * 1.001 {
            return Err(format!(
                "{}: Bevölkerung über der Tragfähigkeit",
                planet.name
            ));
        }
    }
    let polities = world.resource::<Polities>().list.len();
    if let Some(anomaly) = world
        .resource::<Research>()
        .find_anomaly(world.resource::<GameData>(), polities)
    {
        return Err(anomaly);
    }
    Ok(())
}

/// Kennzahlen einer Partie für Berichte und Sweeps.
#[derive(Clone, Debug)]
pub struct RunSummary {
    pub seed: u64,
    pub days: u64,
    pub hash: u64,
    pub systems: usize,
    pub planets: usize,
    pub free_planets: usize,
    pub polities: Vec<PolitySummary>,
    pub economy: EconomyStats,
    /// Gefechte, die in der Partie begonnen haben, und Schiffe, die darin verloren gingen
    /// (ohne Bordjäger).
    pub battles: u64,
    pub ships_lost: u64,
    /// Erster Invariantenverstoß, falls es einen gab.
    pub violation: Option<String>,
}

#[derive(Clone, Debug)]
pub struct PolitySummary {
    pub name: String,
    pub alive: bool,
    pub planets: usize,
    pub population: f64,
    /// Zahl der erforschten Technologien.
    pub techs: usize,
    /// Höchste erforschte Stufe je Zweig, in der Reihenfolge von `TechBranch::ALL`.
    pub tiers: [u8; 5],
    /// Bestehende Flotten und die Schiffe darin.
    pub fleets: usize,
    pub ships: usize,
    /// Summe der Flottenstärken.
    pub strength: f64,
    /// Offene Bauaufträge.
    pub orders: usize,
    /// Soldrückstand 0..1.
    pub arrears: f64,
    /// Schiffe, die in Gefechten verloren gingen (ohne Bordjäger).
    pub ships_lost: usize,
    /// Staatskasse in Credits.
    pub treasury: f64,
    /// Kriegsmüdigkeit 0..100.
    pub war_weariness: f64,
    /// Bevölkerungsgewichteter Zusammenhalt der eigenen Welten, 0..100.
    pub cohesion: f64,
}

pub fn summarize(sim: &Headless) -> RunSummary {
    let galaxy = sim.galaxy();
    let config = sim.world().resource::<SimConfig>();
    let data = sim.world().resource::<GameData>();
    let research = sim.research();
    let fleets = sim.world().resource::<crate::fleet::Fleets>();
    let battles = sim.world().resource::<crate::fleet::Battles>();
    let economy = sim.world().resource::<crate::economy::Economy>();
    let finance = sim.world().resource::<crate::finance::Finance>();
    let cohesion = |polity: PolityId| {
        let (mut weight, mut weighted) = (0.0, 0.0);
        for planet in galaxy.planets_of(polity) {
            weight += planet.population;
            weighted += planet.population * finance.region(planet.id).cohesion;
        }
        weighted / f64::max(weight, 1e-9)
    };
    RunSummary {
        seed: config.seed,
        days: sim.clock().day(),
        hash: sim.hash(),
        systems: galaxy.systems.len(),
        planets: galaxy.planets.len(),
        free_planets: galaxy
            .planets
            .iter()
            .filter(|planet| planet.owner.is_none())
            .count(),
        polities: sim
            .polities()
            .list
            .iter()
            .map(|polity| PolitySummary {
                name: polity.name.clone(),
                alive: polity.alive,
                planets: galaxy.planets_of(polity.id).count(),
                population: galaxy.population_of(polity.id),
                techs: research.known_count(polity.id),
                tiers: crate::data::TechBranch::ALL
                    .map(|branch| research.tier(data, polity.id, branch)),
                fleets: fleets.of(polity.id).count(),
                ships: fleets.ship_count(polity.id),
                // Von null an gezählt: Die Summe über nichts wäre sonst minus null.
                strength: fleets
                    .of(polity.id)
                    .fold(0.0, |sum, fleet| sum + fleet.strength as f64),
                orders: fleets.orders_of(polity.id).count(),
                arrears: fleets.arrears_of(polity.id) as f64,
                ships_lost: battles.ships_lost_by(polity.id) as usize,
                treasury: crate::economy::to_credits(economy.treasury(polity.id)),
                war_weariness: finance.polity(polity.id).war_weariness,
                cohesion: cohesion(polity.id),
            })
            .collect(),
        economy: economy_stats(sim.world()),
        battles: battles.next_id as u64,
        ships_lost: battles.total_ships_lost() as u64,
        violation: check_invariants(sim.world()).err(),
    }
}

/// Ein kompletter KI-gegen-KI-Lauf.
pub fn run_ai_game(data: &GameData, seed: u64, days: u64) -> Headless {
    let mut sim = Headless::new(data.clone(), SimConfig { seed, player: None });
    sim.run_days(days);
    sim
}

/// Was ein Ereignis eines Kriegsdrehbuchs auslöst.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptedAct {
    /// A erklärt B den Krieg und schickt alle seine Flotten im Blockadeauftrag in das
    /// Heimatsystem von B.
    War,
    /// A schließt mit B Frieden.
    Peace,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScriptedEvent {
    /// Tag, an dessen Beginn die Befehle eingestellt werden.
    pub day: u64,
    pub act: ScriptedAct,
    pub a: PolityId,
    pub b: PolityId,
}

/// Kriege nach Drehbuch. Solange keine KI Krieg führt, lassen sich Gefechte, Blockaden und
/// ihre Folgen nur so in ganzen Partien ansehen und prüfen. Das Drehbuch stellt gewöhnliche
/// Befehle ein, wie sie auch ein Spieler gäbe; abgelehnte stehen im Befehlsprotokoll.
#[derive(Clone, Debug, Default)]
pub struct WarScript {
    events: Vec<ScriptedEvent>,
}

impl WarScript {
    pub fn new(mut events: Vec<ScriptedEvent>) -> Self {
        // Nach Tag; am selben Tag zuerst die Friedensschlüsse, sonst wie angegeben.
        events.sort_by_key(|event| (event.day, event.act == ScriptedAct::War));
        Self { events }
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    fn issue(sim: &mut Headless, event: &ScriptedEvent) {
        let (a, b) = (event.a, event.b);
        if event.act == ScriptedAct::Peace {
            sim.issue(a, SimCommand::MakePeace { target: b });
            return;
        }
        sim.issue(a, SimCommand::DeclareWar { target: b });
        // Nennt das Drehbuch eine Gemeinschaft, die es nicht gibt, wird die Erklärung
        // abgelehnt und steht so im Protokoll; Flottenbefehle folgen ihr dann nicht.
        let exists = |polity: PolityId| {
            sim.polities()
                .list
                .get(polity.index())
                .is_some_and(|found| found.alive)
        };
        if !exists(a) || !exists(b) {
            return;
        }
        let target = sim
            .polities()
            .list
            .get(b.index())
            .and_then(|polity| polity.capital)
            .map(|capital| sim.galaxy().planet(capital).system);
        let own: Vec<FleetId> = sim
            .world()
            .resource::<Fleets>()
            .of(a)
            .map(|fleet| fleet.id)
            .collect();
        for fleet in own {
            sim.issue(
                a,
                SimCommand::SetStance {
                    fleet,
                    stance: Stance::Blockade,
                },
            );
            if let Some(to) = target {
                sim.issue(a, SimCommand::MoveFleet { fleet, to });
            }
        }
    }

    /// Lässt die Partie bis zum Beginn des Tages `day` laufen und stellt unterwegs die
    /// fälligen Befehle ein. `exact` rechnet jeden Takt, statt Leertakte zu überspringen.
    pub fn run_until(&self, sim: &mut Headless, day: u64, exact: bool) {
        let advance = |sim: &mut Headless, to: u64| {
            let days = to.saturating_sub(sim.clock().day());
            if exact {
                sim.run_days_exact(days);
            } else {
                sim.run_days(days);
            }
        };
        for event in &self.events {
            if event.day < sim.clock().day() || event.day >= day {
                continue;
            }
            advance(sim, event.day);
            Self::issue(sim, event);
        }
        advance(sim, day);
    }
}

/// Wie [`run_ai_game`], prüft aber jährlich die Invarianten und meldet den ersten Verstoß.
pub fn run_checked(data: &GameData, seed: u64, days: u64) -> (Headless, Option<String>) {
    run_scripted(data, seed, days, &WarScript::default())
}

/// Wie [`run_checked`], mit Kriegen nach Drehbuch.
pub fn run_scripted(
    data: &GameData,
    seed: u64,
    days: u64,
    script: &WarScript,
) -> (Headless, Option<String>) {
    let mut sim = Headless::new(data.clone(), SimConfig { seed, player: None });
    let mut violation = None;
    let mut done = 0;
    while done < days {
        let step = crate::clock::DAYS_PER_YEAR.min(days - done);
        script.run_until(&mut sim, done + step, false);
        done += step;
        if violation.is_none()
            && let Err(message) = check_invariants(sim.world())
        {
            violation = Some(format!("Tag {done}: {message}"));
        }
    }
    (sim, violation)
}

/// Viele Seeds parallel. Jeder Lauf ist für sich einfädig und deterministisch; die
/// Ergebnisse kommen in Seed-Reihenfolge zurück, unabhängig von der Thread-Zahl.
pub fn sweep(data: &GameData, seeds: &[u64], days: u64, jobs: usize) -> Vec<RunSummary> {
    sweep_scripted(data, seeds, days, jobs, &WarScript::default())
}

/// Wie [`sweep`], jeder Lauf mit denselben Kriegen nach Drehbuch.
pub fn sweep_scripted(
    data: &GameData,
    seeds: &[u64],
    days: u64,
    jobs: usize,
    script: &WarScript,
) -> Vec<RunSummary> {
    let jobs = jobs.clamp(1, seeds.len().max(1));
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results = std::sync::Mutex::new(Vec::with_capacity(seeds.len()));
    std::thread::scope(|scope| {
        for _ in 0..jobs {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(seed) = seeds.get(index) else {
                        break;
                    };
                    let (sim, violation) = run_scripted(data, *seed, days, script);
                    let mut summary = summarize(&sim);
                    summary.violation = violation.or(summary.violation);
                    results
                        .lock()
                        .expect("Sweep-Thread abgestürzt")
                        .push((index, summary));
                }
            });
        }
    });
    let mut results = results.into_inner().expect("Sweep-Thread abgestürzt");
    results.sort_by_key(|(index, _)| *index);
    results.into_iter().map(|(_, summary)| summary).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::best_colony_target;
    use crate::clock::DAYS_PER_YEAR;
    use crate::command::CommandOutcome;

    fn data() -> GameData {
        GameData::load_workspace().unwrap()
    }

    #[test]
    fn same_seed_gives_identical_run() {
        let data = data();
        let a = run_ai_game(&data, 7, 3 * DAYS_PER_YEAR);
        let b = run_ai_game(&data, 7, 3 * DAYS_PER_YEAR);
        let c = run_ai_game(&data, 8, 3 * DAYS_PER_YEAR);
        assert_eq!(a.hash(), b.hash());
        assert_ne!(a.hash(), c.hash());
        // Die Welt hat sich wirklich bewegt.
        assert!(
            a.hash()
                != Headless::new(
                    data,
                    SimConfig {
                        seed: 7,
                        player: None
                    }
                )
                .hash()
        );
    }

    #[test]
    fn skipping_idle_ticks_changes_nothing() {
        let data = data();
        let config = SimConfig {
            seed: 21,
            player: None,
        };
        let mut fast = Headless::new(data.clone(), config.clone());
        let mut exact = Headless::new(data, config);
        fast.run_days(400);
        exact.run_days_exact(400);
        assert_eq!(fast.clock(), exact.clock());
        assert_eq!(fast.hash(), exact.hash());
    }

    /// Spielt als Gemeinschaft 0 und gründet zu festen Zeiten Kolonien.
    fn play_scripted(sim: &mut Headless) {
        let me = PolityId(0);
        for _ in 0..4 {
            sim.run_days(250);
            let day = sim.clock().day();
            let target = {
                let world = sim.world();
                best_colony_target(
                    world.resource::<GameData>(),
                    world.resource::<Galaxy>(),
                    world.resource::<Polities>(),
                    world.resource::<crate::economy::Economy>(),
                    me,
                    day,
                )
            };
            if let Some(planet) = target {
                sim.issue(me, SimCommand::ClaimPlanet { planet });
            }
            // Ein abgelehnter Befehl gehört ebenfalls ins Protokoll.
            sim.issue(
                me,
                SimCommand::ClaimPlanet {
                    planet: crate::ids::PlanetId(9_999),
                },
            );
        }
        sim.run_days(100);
    }

    #[test]
    fn command_log_replays_to_the_same_state() {
        let data = data();
        let config = SimConfig {
            seed: 5,
            player: Some(PolityId(0)),
        };

        let mut original = Headless::new(data.clone(), config.clone());
        play_scripted(&mut original);
        let log = original.command_log().clone();
        assert!(
            log.entries
                .iter()
                .any(|entry| entry.outcome == CommandOutcome::Applied)
        );
        assert!(
            log.entries
                .iter()
                .any(|entry| matches!(entry.outcome, CommandOutcome::Rejected(_)))
        );

        let mut replay = Headless::new(data.clone(), config.clone());
        replay.set_replay(&log);
        replay.run_days(original.clock().day());
        assert_eq!(replay.hash(), original.hash());

        // Ohne die Befehle verläuft die Partie anders.
        let mut passive = Headless::new(data, config);
        passive.run_days(original.clock().day());
        assert_ne!(passive.hash(), original.hash());
    }

    #[test]
    fn load_and_continue_matches_uninterrupted_run() {
        let data = data();
        let config = SimConfig {
            seed: 13,
            player: None,
        };
        let mut straight = Headless::new(data.clone(), config.clone());
        straight.run_days(900);

        let mut first_half = Headless::new(data.clone(), config);
        first_half.run_days(450);
        let text = first_half.save().to_ron().unwrap();
        let save = SaveGame::from_ron(&text).unwrap();
        assert!(save.matches_data(&data));
        let mut resumed = Headless::from_save(data, save);
        assert_eq!(
            resumed.hash(),
            first_half.hash(),
            "Laden muss den Zustand exakt herstellen"
        );
        resumed.run_days(450);
        assert_eq!(resumed.hash(), straight.hash());
    }

    #[test]
    fn space_runs_out() {
        let data = data();
        let sim = run_ai_game(&data, 2, 40 * DAYS_PER_YEAR);
        let summary = summarize(&sim);
        // Nach 40 Jahren ist der Raum verteilt; frei sind höchstens Welten, die gerade
        // aufgegeben wurden.
        assert!(
            summary.free_planets * 10 <= summary.planets,
            "{} von {} Planeten noch frei",
            summary.free_planets,
            summary.planets
        );
        // Niemand geht bei der Ausdehnung leer aus.
        for polity in &summary.polities {
            assert!(polity.planets >= 2, "{} blieb ohne Kolonien", polity.name);
        }
    }

    #[test]
    fn sweep_is_independent_of_thread_count() {
        let data = data();
        let seeds = [1, 2, 3, 4, 5];
        let serial = sweep(&data, &seeds, 200, 1);
        let parallel = sweep(&data, &seeds, 200, 4);
        let hashes =
            |runs: &[RunSummary]| runs.iter().map(|r| (r.seed, r.hash)).collect::<Vec<_>>();
        assert_eq!(hashes(&serial), hashes(&parallel));
    }

    #[test]
    fn a_scripted_war_leads_to_battles_and_is_the_same_with_every_tick() {
        let data = data();
        let event = |day, act, a, b| ScriptedEvent {
            day,
            act,
            a: PolityId(a),
            b: PolityId(b),
        };
        // Angegeben in beliebiger Reihenfolge; gespielt nach Tagen.
        let script = WarScript::new(vec![
            event(700, ScriptedAct::Peace, 0, 1),
            event(40, ScriptedAct::War, 0, 1),
            event(40, ScriptedAct::War, 2, 1),
        ]);
        assert!(!script.is_empty() && WarScript::default().is_empty());
        let (sim, violation) = run_scripted(&data, 4, 2 * DAYS_PER_YEAR, &script);
        assert_eq!(violation, None);
        let summary = summarize(&sim);
        assert!(
            summary.battles >= 1,
            "die Flotten sind aufeinandergetroffen"
        );
        assert!(summary.ships_lost > 0);
        assert_eq!(
            summary.ships_lost as usize,
            summary
                .polities
                .iter()
                .map(|polity| polity.ships_lost)
                .sum::<usize>()
        );
        // Die Befehle stehen im Protokoll; der Friede kam am genannten Tag.
        let log = sim.command_log();
        let declared = log
            .entries
            .iter()
            .filter(|entry| matches!(entry.command, SimCommand::DeclareWar { .. }))
            .count();
        assert_eq!(declared, 2);
        let peace = log
            .entries
            .iter()
            .find(|entry| matches!(entry.command, SimCommand::MakePeace { .. }))
            .expect("der Friedensschluss");
        assert_eq!(peace.tick, 700 * TICKS_PER_DAY);
        assert_eq!(peace.outcome, CommandOutcome::Applied);
        assert!(
            !sim.world()
                .resource::<crate::diplomacy::Diplomacy>()
                .at_war(PolityId(0), PolityId(1))
        );

        // In Jahresschritten oder am Stück, mit oder ohne Leertakte: derselbe Verlauf.
        let mut exact = Headless::new(
            data.clone(),
            SimConfig {
                seed: 4,
                player: None,
            },
        );
        script.run_until(&mut exact, 2 * DAYS_PER_YEAR, true);
        assert_eq!(exact.clock(), sim.clock());
        assert_eq!(exact.hash(), sim.hash());
        // Ohne Drehbuch ist es der gewöhnliche Lauf.
        let (plain, _) = run_scripted(&data, 4, 200, &WarScript::default());
        assert_eq!(plain.hash(), run_ai_game(&data, 4, 200).hash());
    }

    #[test]
    fn wrong_save_format_is_rejected() {
        let data = data();
        let sim = Headless::new(
            data,
            SimConfig {
                seed: 1,
                player: None,
            },
        );
        let mut save = sim.save();
        save.format = 99;
        let text = save.to_ron().unwrap();
        assert!(SaveGame::from_ron(&text).is_err());
    }
}

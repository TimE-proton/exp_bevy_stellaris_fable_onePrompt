//! Läufe ohne Grafik: Grundlage für Tests und das `headless`-Programm.

use crate::clock::{SimClock, TICKS_PER_DAY};
use crate::command::{CommandLog, CommandQueue, CommandSource, ReplayScript, SimCommand};
use crate::data::GameData;
use crate::galaxy::Galaxy;
use crate::ids::PolityId;
use crate::polity::Polities;
use crate::state::{SaveGame, SimConfig, SimState, hash_world, new_game};
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
}

#[derive(Clone, Debug)]
pub struct PolitySummary {
    pub name: String,
    pub alive: bool,
    pub planets: usize,
    pub population: f64,
}

pub fn summarize(sim: &Headless) -> RunSummary {
    let galaxy = sim.galaxy();
    let config = sim.world().resource::<SimConfig>();
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
            })
            .collect(),
    }
}

/// Ein kompletter KI-gegen-KI-Lauf.
pub fn run_ai_game(data: &GameData, seed: u64, days: u64) -> Headless {
    let mut sim = Headless::new(data.clone(), SimConfig { seed, player: None });
    sim.run_days(days);
    sim
}

/// Viele Seeds parallel. Jeder Lauf ist für sich einfädig und deterministisch; die
/// Ergebnisse kommen in Seed-Reihenfolge zurück, unabhängig von der Thread-Zahl.
pub fn sweep(data: &GameData, seeds: &[u64], days: u64, jobs: usize) -> Vec<RunSummary> {
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
                    let summary = summarize(&run_ai_game(data, *seed, days));
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
        assert_eq!(
            summary.free_planets, 0,
            "nach 40 Jahren ist jeder Planet vergeben"
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

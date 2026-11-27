//! Simulationskern.
//!
//! Keine Abhängigkeit auf Rendering, Fenster oder Eingabe. Jedes Teilsystem ist ein Modul
//! mit reinen Daten und Funktionen sowie einem Bevy-Plugin, das den Zustand als Resource
//! hält und Systeme in `FixedUpdate` einhängt.
//!
//! Determinismus: fester Takt, eigener Zufallsgenerator mit abgeleiteten Strömen, eine
//! einzige verkettete Systemreihenfolge ([`SimSet`]), Zustand nur in `Vec`/`BTreeMap`.
//! Gleicher Seed, gleiche Befehle und gleiches Programm ergeben denselben Verlauf.

pub mod ai;
pub mod battle;
pub mod chronicle;
pub mod clock;
pub mod command;
pub mod data;
pub mod diplomacy;
pub mod economy;
pub mod endgame;
pub mod finance;
pub mod fleet;
pub mod galaxy;
pub mod harness;
pub mod hash;
pub mod ids;
pub mod logistics;
pub mod polity;
pub mod rng;
pub mod siege;
pub mod state;
pub mod tech;

use bevy_app::prelude::*;
use bevy_app::{PluginGroup, PluginGroupBuilder};
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::{LogLevel, ScheduleBuildSettings, SingleThreadedExecutor};

pub use clock::{SimClock, TICKS_PER_DAY, TICKS_PER_SECOND};
pub use data::GameData;
pub use state::{SaveGame, SimConfig, SimState, new_game};

/// Das Schedule, in dem die Simulation läuft.
pub fn sim_schedule() -> FixedUpdate {
    FixedUpdate
}

/// Wurzel aller Simulationssysteme. `game` hängt hier seine Zustandsbedingung an.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SimRoot;

/// Die eine, verkettete Reihenfolge eines Takts. Die Kausalität des Spiels steht hier:
/// Befehle und Entscheidungen, dann Warenflüsse und Produktion, daraus Preise und
/// Finanzen, daraus Zusammenhalt; danach Krieg und seine Folgen.
///
/// Wer wie viel aus der Staatskasse bekommt, entscheidet am Monatsersten der
/// Staatshaushalt im Monatsabschluss (`Finance`, [`economy::settle_budget`]): Er bedient
/// die Ansprüche in ihrer Rangfolge ([`economy::Claim`]) und bewilligt Flotte und Forschung,
/// was sie später im selben Takt ausgeben (`Fleets`, `Research`).
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SimSet {
    Commands,
    Ai,
    Diplomacy,
    Logistics,
    Production,
    Prices,
    Finance,
    Cohesion,
    Fleets,
    Research,
    SpaceCombat,
    Siege,
    Aftermath,
    Victory,
    Log,
    Clock,
}

/// Grundgerüst: Reihenfolge, Befehle, Uhr.
pub struct CorePlugin;

impl Plugin for CorePlugin {
    fn build(&self, app: &mut App) {
        app.edit_schedule(sim_schedule(), |schedule| {
            // Einfädig und mit Mehrdeutigkeiten als Fehler: Die Reihenfolge ist vollständig
            // festgelegt oder das Programm startet nicht.
            schedule.set_executor(SingleThreadedExecutor::new());
            schedule.set_build_settings(ScheduleBuildSettings {
                ambiguity_detection: LogLevel::Error,
                ..Default::default()
            });
        });
        app.configure_sets(
            sim_schedule(),
            (
                SimSet::Commands,
                SimSet::Ai,
                SimSet::Diplomacy,
                SimSet::Logistics,
                SimSet::Production,
                SimSet::Prices,
                SimSet::Finance,
                SimSet::Cohesion,
                SimSet::Fleets,
                SimSet::Research,
                SimSet::SpaceCombat,
                SimSet::Siege,
                SimSet::Aftermath,
                SimSet::Victory,
                SimSet::Log,
                SimSet::Clock,
            )
                .chain()
                .in_set(SimRoot),
        );
        app.configure_sets(sim_schedule(), SimRoot.run_if(game_running));
        app.add_systems(
            sim_schedule(),
            (
                (command::inject_replay, command::apply_commands)
                    .chain()
                    .in_set(SimSet::Commands),
                advance_clock.in_set(SimSet::Clock),
            ),
        );
    }
}

/// Alle Teilsysteme in fester Reihenfolge. `game` und `headless` nutzen dieselbe Gruppe.
pub struct SimPlugins;

impl PluginGroup for SimPlugins {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>()
            .add(CorePlugin)
            .add(galaxy::GalaxyPlugin)
            .add(diplomacy::DiplomacyPlugin)
            .add(logistics::LogisticsPlugin)
            .add(economy::EconomyPlugin)
            .add(finance::FinancePlugin)
            .add(tech::TechPlugin)
            .add(fleet::FleetPlugin)
            .add(siege::SiegePlugin)
            .add(ai::AiPlugin)
            .add(endgame::EndgamePlugin)
    }
}

/// Die Simulation läuft, solange eine Partie geladen und nicht zu Ende ist. Mit dem
/// Ausgang steht alles still, auch die Uhr: Der Zustand ändert sich danach nicht mehr.
fn game_running(
    clock: Option<Res<SimClock>>,
    data: Option<Res<GameData>>,
    endgame: Option<Res<endgame::Endgame>>,
) -> bool {
    clock.is_some() && data.is_some() && endgame.is_none_or(|endgame| !endgame.ended())
}

fn advance_clock(mut clock: ResMut<SimClock>) {
    clock.tick += 1;
}

/// Braucht die Simulation gerade jeden Takt? Sonst darf ein Headless-Lauf direkt zum
/// nächsten Tagesbeginn springen; zwischen den Tagesgrenzen geschieht dann nichts. Jeden
/// Takt braucht sie, solange Befehle warten oder ein Gefecht läuft.
pub fn needs_fine_ticks(world: &World) -> bool {
    world
        .get_resource::<command::CommandQueue>()
        .is_some_and(|queue| !queue.pending.is_empty())
        || world
            .get_resource::<fleet::Battles>()
            .is_some_and(|battles| !battles.active.is_empty())
}

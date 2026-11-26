//! Gesamtzustand der Simulation: Erzeugen, Speichern, Laden, Prüfsumme.
//!
//! Jedes Teilsystem hält seinen Zustand als eigene Resource. Die Liste hier ist die eine
//! Stelle, die festlegt, was zum Spielstand gehört; neue Teilsysteme tragen sich ein.

use crate::chronicle::{Chronicle, ChronicleKind};
use crate::clock::SimClock;
use crate::command::{CommandLog, CommandQueue};
use crate::data::GameData;
use crate::diplomacy::Diplomacy;
use crate::economy::{Economy, setup_economy};
use crate::finance::{Finance, setup_finance};
use crate::fleet::{Fleets, setup_fleets};
use crate::galaxy::{Galaxy, generate_galaxy};
use crate::hash::Fnv64;
use crate::ids::PolityId;
use crate::logistics::{Logistics, setup_logistics};
use crate::polity::{Polities, found_polities};
use crate::siege::{Sieges, setup_sieges};
use crate::tech::{Research, setup_research};
use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::Path;

/// Einstellungen einer Partie.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimConfig {
    pub seed: u64,
    /// Vom Menschen geführte Gemeinschaft; `None` = alle von der KI geführt.
    pub player: Option<PolityId>,
}

macro_rules! sim_state {
    ($( $field:ident : $ty:ty ),* $(,)?) => {
        /// Abbild aller Zustands-Resources.
        #[derive(Clone, Debug, Default, Serialize, Deserialize)]
        pub struct SimState {
            $( #[serde(default)] pub $field: $ty, )*
        }

        impl SimState {
            /// Liest den Zustand aus der Welt. `None`, wenn keine Partie läuft.
            pub fn capture(world: &World) -> Option<Self> {
                Some(Self { $( $field: world.get_resource::<$ty>()?.clone(), )* })
            }

            /// Setzt den Zustand in die Welt ein und ersetzt eine laufende Partie.
            pub fn install(mut self, world: &mut World) {
                self.galaxy.rebuild_index();
                // Ältere Spielstände kennen neuere Listen je Gemeinschaft noch nicht.
                self.economy.sync_polities();
                $( world.insert_resource(self.$field); )*
            }

            /// Entfernt die Partie aus der Welt.
            pub fn remove(world: &mut World) {
                $( world.remove_resource::<$ty>(); )*
            }
        }

        /// Prüfsumme über den gesamten Zustand. `None`, wenn keine Partie läuft.
        pub fn hash_world(world: &World) -> Option<u64> {
            let mut hasher = Fnv64::new();
            let mut scratch = Vec::new();
            $( hasher.write_value(world.get_resource::<$ty>()?, &mut scratch); )*
            Some(hasher.finish())
        }
    };
}

sim_state! {
    config: SimConfig,
    clock: SimClock,
    galaxy: Galaxy,
    polities: Polities,
    economy: Economy,
    finance: Finance,
    research: Research,
    logistics: Logistics,
    fleets: Fleets,
    sieges: Sieges,
    diplomacy: Diplomacy,
    queue: CommandQueue,
    log: CommandLog,
    chronicle: Chronicle,
}

/// Abweichung der Geldmenge vom Soll (Anfangsbestand plus ausgewiesene Schöpfung).
/// Muss immer null sein: Geld wird nur verschoben.
pub fn money_drift(world: &World) -> Option<i64> {
    let economy = world.get_resource::<Economy>()?;
    let logistics = world.get_resource::<Logistics>()?;
    let finance = world.get_resource::<Finance>()?;
    let in_transit = world
        .get_resource::<crate::economy::EquityFlows>()
        .map_or(0, |flows| flows.in_transit());
    Some(
        economy.money_in_accounts() + logistics.money() + finance.money() + in_transit
            - economy.initial_money
            - economy.minted,
    )
}

impl SimState {
    /// Neue Partie aus Daten und Einstellungen. Reine Funktion.
    pub fn generate(data: &GameData, mut config: SimConfig) -> Self {
        let mut galaxy = generate_galaxy(data, config.seed);
        let polities = found_polities(data, &mut galaxy, config.seed);
        if config
            .player
            .is_some_and(|player| player.index() >= polities.list.len())
        {
            config.player = None;
        }
        let mut chronicle = Chronicle::default();
        chronicle.push(
            0,
            ChronicleKind::Info,
            None,
            None,
            format!(
                "{} Gemeinschaften teilen sich {} Systeme. Der Raum ist endlich.",
                polities.list.len(),
                galaxy.systems.len()
            ),
        );
        let mut economy = setup_economy(data, &galaxy, &polities);
        let mut logistics = setup_logistics(data, &galaxy, &polities);
        economy.initial_money += logistics.money();
        // Die Eliten erhalten ihr Geld aus dem der Haushalte: Die Summe bleibt gleich.
        let finance = setup_finance(data, &galaxy, &polities, &mut economy, &logistics);
        let research = setup_research(data, &polities);
        // Das Startwissen wirkt vom ersten Takt an, auch für Befehle vor dem ersten Tag.
        research.publish_effects(&mut economy, &mut logistics);
        let fleets = setup_fleets(data, &galaxy, &polities);
        let sieges = setup_sieges(data, &galaxy, &polities);
        Self {
            config,
            clock: SimClock::default(),
            galaxy,
            polities,
            economy,
            finance,
            research,
            logistics,
            fleets,
            sieges,
            diplomacy: Diplomacy::default(),
            queue: CommandQueue::default(),
            log: CommandLog::default(),
            chronicle,
        }
    }

    pub fn hash(&self) -> u64 {
        let mut world = World::new();
        self.clone().install(&mut world);
        hash_world(&world).expect("gerade eingesetzt")
    }
}

/// Startet eine neue Partie in `world`. Braucht die Resource [`GameData`].
pub fn new_game(world: &mut World, config: SimConfig) {
    let state = SimState::generate(world.resource::<GameData>(), config);
    state.install(world);
}

pub const SAVE_FORMAT: u32 = 1;

/// Spielstand auf der Platte (RON).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SaveGame {
    pub format: u32,
    /// Prüfsumme der Spieldaten, mit denen gespeichert wurde.
    pub data_hash: u64,
    pub state: SimState,
}

#[derive(Debug)]
pub enum SaveError {
    NoGame,
    Io(std::io::Error),
    Encode(String),
    Decode(String),
    Format { found: u32 },
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveError::NoGame => write!(f, "keine laufende Partie"),
            SaveError::Io(error) => write!(f, "Dateifehler: {error}"),
            SaveError::Encode(error) => write!(f, "Speichern fehlgeschlagen: {error}"),
            SaveError::Decode(error) => write!(f, "Spielstand unlesbar: {error}"),
            SaveError::Format { found } => write!(
                f,
                "Spielstand hat Format {found}, dieses Programm liest Format {SAVE_FORMAT}"
            ),
        }
    }
}

impl std::error::Error for SaveError {}

impl SaveGame {
    pub fn capture(world: &World) -> Result<Self, SaveError> {
        let state = SimState::capture(world).ok_or(SaveError::NoGame)?;
        let data_hash = world
            .get_resource::<GameData>()
            .map(|data| data.hash)
            .unwrap_or(0);
        Ok(Self {
            format: SAVE_FORMAT,
            data_hash,
            state,
        })
    }

    pub fn to_ron(&self) -> Result<String, SaveError> {
        ron::to_string(self).map_err(|error| SaveError::Encode(error.to_string()))
    }

    pub fn from_ron(text: &str) -> Result<Self, SaveError> {
        let save: Self =
            ron::from_str(text).map_err(|error| SaveError::Decode(error.to_string()))?;
        if save.format != SAVE_FORMAT {
            return Err(SaveError::Format { found: save.format });
        }
        Ok(save)
    }

    pub fn write(&self, path: &Path) -> Result<(), SaveError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(SaveError::Io)?;
        }
        std::fs::write(path, self.to_ron()?).map_err(SaveError::Io)
    }

    pub fn read(path: &Path) -> Result<Self, SaveError> {
        Self::from_ron(&std::fs::read_to_string(path).map_err(SaveError::Io)?)
    }

    /// Passen die Spieldaten zu denen, mit denen gespeichert wurde?
    pub fn matches_data(&self, data: &GameData) -> bool {
        self.data_hash == data.hash
    }
}

//! Befehle: der einzige Weg, auf dem Spieler und KI in die Simulation eingreifen.
//!
//! Die Oberfläche stellt Befehle in die Warteschlange; der erste Schritt des nächsten
//! Takts wendet sie in Eingangsreihenfolge an. Befehle von außen (Spieler, Skript)
//! werden mit Takt und laufender Nummer protokolliert. Seed, Datenprüfsumme und dieses
//! Protokoll genügen, um eine Partie exakt zu wiederholen.

use crate::chronicle::Chronicle;
use crate::clock::SimClock;
use crate::data::GameData;
use crate::diplomacy::{self, Diplomacy};
use crate::economy::Economy;
use crate::fleet::{self, Fleets, Stance};
use crate::galaxy::Galaxy;
use crate::ids::{FleetId, PlanetId, PolityId, SystemId};
use crate::polity::{self, Polities};
use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SimCommand {
    /// Kolonie auf einem freien Planeten in Reichweite gründen.
    ClaimPlanet {
        planet: PlanetId,
    },
    /// Flotte auf dem kürzesten Weg in ein System schicken.
    MoveFleet {
        fleet: FleetId,
        to: SystemId,
    },
    /// Auftrag einer Flotte ändern.
    SetStance {
        fleet: FleetId,
        stance: Stance,
    },
    DeclareWar {
        target: PolityId,
    },
    MakePeace {
        target: PolityId,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandSource {
    Player,
    Ai,
    Script,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QueuedCommand {
    pub polity: PolityId,
    pub source: CommandSource,
    pub command: SimCommand,
}

#[derive(Resource, Clone, Debug, Default, Serialize, Deserialize)]
pub struct CommandQueue {
    pub pending: Vec<QueuedCommand>,
}

impl CommandQueue {
    pub fn push(&mut self, polity: PolityId, source: CommandSource, command: SimCommand) {
        self.pending.push(QueuedCommand {
            polity,
            source,
            command,
        });
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CommandOutcome {
    Applied,
    Rejected(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LoggedCommand {
    pub tick: u64,
    /// Laufende Nummer innerhalb des Takts.
    pub seq: u32,
    pub polity: PolityId,
    pub source: CommandSource,
    pub command: SimCommand,
    pub outcome: CommandOutcome,
}

/// Protokoll aller Befehle von außen. KI-Befehle fehlen: Die KI ist Teil der Simulation
/// und erzeugt sie bei einer Wiedergabe von selbst wieder.
#[derive(Resource, Clone, Debug, Default, Serialize, Deserialize)]
pub struct CommandLog {
    pub entries: Vec<LoggedCommand>,
    pub ai_applied: u64,
    pub ai_rejected: u64,
}

/// Spielt ein früheres Befehlsprotokoll ab. Nicht Teil des Spielstands.
#[derive(Resource, Clone, Debug, Default)]
pub struct ReplayScript {
    pub entries: Vec<LoggedCommand>,
    pub cursor: usize,
}

impl ReplayScript {
    pub fn new(mut entries: Vec<LoggedCommand>) -> Self {
        entries.sort_by_key(|entry| (entry.tick, entry.seq));
        Self { entries, cursor: 0 }
    }

    /// Takt des nächsten ausstehenden Befehls.
    pub fn next_tick(&self) -> Option<u64> {
        self.entries.get(self.cursor).map(|entry| entry.tick)
    }
}

/// Schreibzugriff auf alles, was Befehle verändern dürfen.
#[derive(SystemParam)]
pub struct CommandContext<'w> {
    pub data: Res<'w, GameData>,
    pub clock: Res<'w, SimClock>,
    pub galaxy: ResMut<'w, Galaxy>,
    pub polities: ResMut<'w, Polities>,
    pub economy: ResMut<'w, Economy>,
    pub fleets: ResMut<'w, Fleets>,
    pub diplomacy: ResMut<'w, Diplomacy>,
    pub chronicle: ResMut<'w, Chronicle>,
}

pub(crate) fn inject_replay(
    clock: Res<SimClock>,
    script: Option<ResMut<ReplayScript>>,
    mut queue: ResMut<CommandQueue>,
) {
    let Some(mut script) = script else {
        return;
    };
    while let Some(entry) = script.entries.get(script.cursor) {
        if entry.tick > clock.tick {
            break;
        }
        if entry.tick == clock.tick {
            queue.push(entry.polity, entry.source, entry.command.clone());
        }
        script.cursor += 1;
    }
}

pub(crate) fn apply_commands(
    mut queue: ResMut<CommandQueue>,
    mut log: ResMut<CommandLog>,
    mut ctx: CommandContext,
) {
    if queue.pending.is_empty() {
        return;
    }
    let tick = ctx.clock.tick;
    for (seq, queued) in std::mem::take(&mut queue.pending).into_iter().enumerate() {
        let outcome = match execute(&mut ctx, queued.polity, &queued.command) {
            Ok(()) => CommandOutcome::Applied,
            Err(reason) => CommandOutcome::Rejected(reason),
        };
        if queued.source == CommandSource::Ai {
            match outcome {
                CommandOutcome::Applied => log.ai_applied += 1,
                CommandOutcome::Rejected(_) => log.ai_rejected += 1,
            }
        } else {
            log.entries.push(LoggedCommand {
                tick,
                seq: seq as u32,
                polity: queued.polity,
                source: queued.source,
                command: queued.command,
                outcome,
            });
        }
    }
}

fn execute(ctx: &mut CommandContext, polity: PolityId, command: &SimCommand) -> Result<(), String> {
    let tick = ctx.clock.tick;
    match command {
        SimCommand::ClaimPlanet { planet } => polity::claim_planet(
            &mut ctx.galaxy,
            &mut ctx.polities,
            &mut ctx.economy,
            &mut ctx.chronicle,
            &ctx.data,
            polity,
            *planet,
            tick,
        )
        .map_err(|error| error.to_string()),
        SimCommand::MoveFleet { fleet, to } => {
            fleet::order_move(&ctx.galaxy, &mut ctx.fleets, polity, *fleet, *to)
        }
        SimCommand::SetStance { fleet, stance } => {
            fleet::set_stance(&mut ctx.fleets, polity, *fleet, *stance)
        }
        SimCommand::DeclareWar { target } => diplomacy::declare_war(
            &mut ctx.diplomacy,
            &ctx.polities,
            &mut ctx.chronicle,
            tick,
            polity,
            *target,
        ),
        SimCommand::MakePeace { target } => diplomacy::make_peace(
            &mut ctx.diplomacy,
            &ctx.polities,
            &mut ctx.chronicle,
            tick,
            polity,
            *target,
        ),
    }
}

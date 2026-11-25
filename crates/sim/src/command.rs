//! Befehle: der einzige Weg, auf dem Spieler und KI in die Simulation eingreifen.
//!
//! Die Oberfläche stellt Befehle in die Warteschlange; der erste Schritt des nächsten
//! Takts wendet sie in Eingangsreihenfolge an. Befehle von außen (Spieler, Skript)
//! werden mit Takt und laufender Nummer protokolliert. Seed, Datenprüfsumme und dieses
//! Protokoll genügen, um eine Partie exakt zu wiederholen.

use crate::chronicle::{Chronicle, ChronicleKind};
use crate::clock::SimClock;
use crate::data::GameData;
use crate::diplomacy::{self, Diplomacy};
use crate::economy::{Economy, to_credits, to_money};
use crate::finance::shares::ExpropriationScope;
use crate::finance::{self, Finance};
use crate::fleet::{self, Fleets, Stance};
use crate::galaxy::Galaxy;
use crate::ids::{FleetId, HolderId, IssuerId, PlanetId, PolityId, SystemId};
use crate::logistics::Logistics;
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
    /// Der Staat kauft Anteile am Markt, höchstens für `max_spend` Credits.
    BuyShares {
        issuer: IssuerId,
        shares: u64,
        max_spend: f64,
    },
    /// Der Staat stellt eigene Anteile zum Verkauf.
    OfferShares {
        issuer: IssuerId,
        shares: u64,
    },
    /// Der Staat überträgt eigene Anteile ohne Gegenleistung an einen anderen Halter.
    DistributeShares {
        issuer: IssuerId,
        shares: u64,
        to: HolderId,
    },
    /// Übernahmeangebot des Staates mit Aufschlag auf den Kurs (0,3 = 30 %).
    TakeoverBid {
        issuer: IssuerId,
        premium: f64,
    },
    /// Der Staat zieht Anteile auf eigenem Gebiet ohne Entschädigung ein.
    Expropriate {
        issuer: IssuerId,
        scope: ExpropriationScope,
    },
    /// Staatsanleihen ausgeben, Betrag in Credits.
    IssueBonds {
        amount: f64,
    },
    SetTaxRate {
        rate: f64,
    },
    /// Rücklage in Credits, die der Staat hält, statt sie wieder auszugeben.
    SetTreasuryTarget {
        amount: f64,
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
    pub finance: ResMut<'w, Finance>,
    pub logistics: Res<'w, Logistics>,
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
        SimCommand::DeclareWar { target } => {
            diplomacy::declare_war(
                &mut ctx.diplomacy,
                &ctx.polities,
                &mut ctx.chronicle,
                tick,
                polity,
                *target,
            )?;
            // Kriegsausbruch: Die Kurse beider Seiten geben nach.
            let factor = ctx.data.balance.finance.war_shock;
            for side in [polity, *target] {
                finance::shares::shock_polity(
                    &mut ctx.finance,
                    &ctx.galaxy,
                    &ctx.logistics,
                    side,
                    factor,
                );
            }
            Ok(())
        }
        SimCommand::MakePeace { target } => diplomacy::make_peace(
            &mut ctx.diplomacy,
            &ctx.polities,
            &mut ctx.chronicle,
            tick,
            polity,
            *target,
        ),
        SimCommand::BuyShares {
            issuer,
            shares,
            max_spend,
        } => {
            let capitals: Vec<_> = ctx.polities.list.iter().map(|p| p.capital).collect();
            let seat =
                finance::shares::seat_of_issuer(&ctx.galaxy, &ctx.logistics, &capitals, *issuer);
            let (bought, _) = finance::buy_shares(
                &mut ctx.finance,
                &mut ctx.economy,
                &ctx.data,
                HolderId::State(polity),
                *issuer,
                seat,
                *shares,
                to_money(max_spend.max(0.0)),
            );
            if bought == 0 {
                Err("nichts gekauft: kein Angebot am Markt oder Kasse zu leer".into())
            } else {
                Ok(())
            }
        }
        SimCommand::OfferShares { issuer, shares } => {
            finance::offer_shares(&mut ctx.finance, HolderId::State(polity), *issuer, *shares)
                .map(|_| ())
        }
        SimCommand::DistributeShares { issuer, shares, to } => finance::distribute_shares(
            &mut ctx.finance,
            HolderId::State(polity),
            *issuer,
            *shares,
            *to,
        )
        .map(|_| ()),
        SimCommand::TakeoverBid { issuer, premium } => finance::takeover_bid(
            &mut ctx.finance,
            &mut ctx.economy,
            &ctx.galaxy,
            HolderId::State(polity),
            *issuer,
            *premium,
        )
        .map(|_| ()),
        SimCommand::Expropriate { issuer, scope } => {
            finance::expropriate(
                &mut ctx.finance,
                &ctx.galaxy,
                &ctx.logistics,
                &ctx.data,
                polity,
                *issuer,
                *scope,
            )?;
            let name = finance::issuer_name(&ctx.galaxy, &ctx.logistics, *issuer);
            let who = ctx.polities.get(polity).name.clone();
            ctx.chronicle.push(
                tick,
                ChronicleKind::Finance,
                Some(polity),
                None,
                format!(
                    "{who} enteignet Anteilseigner von {name}. Die Kurse im Reich brechen ein."
                ),
            );
            Ok(())
        }
        SimCommand::IssueBonds { amount } => {
            let day = ctx.clock.day();
            let placed = finance::issue_bonds(
                &mut ctx.finance,
                &mut ctx.economy,
                &ctx.galaxy,
                &ctx.diplomacy,
                &ctx.data,
                polity,
                to_money(amount.max(0.0)),
                day,
            )?;
            let who = ctx.polities.get(polity).name.clone();
            ctx.chronicle.push(
                tick,
                ChronicleKind::Finance,
                Some(polity),
                None,
                format!(
                    "{who} platziert Anleihen über {:.0} Credits.",
                    to_credits(placed)
                ),
            );
            Ok(())
        }
        SimCommand::SetTaxRate { rate } => {
            if !(0.0..=0.6).contains(rate) {
                return Err("Steuersatz muss zwischen 0 und 60 % liegen".into());
            }
            ctx.economy.tax_rates[polity.index()] = *rate;
            Ok(())
        }
        SimCommand::SetTreasuryTarget { amount } => {
            ctx.economy.treasury_targets[polity.index()] = to_money(amount.max(0.0));
            Ok(())
        }
    }
}

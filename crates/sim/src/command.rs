//! Befehle: der einzige Weg, auf dem Spieler und KI in die Simulation eingreifen.
//!
//! Die Oberfläche stellt Befehle in die Warteschlange; der erste Schritt des nächsten
//! Takts wendet sie in Eingangsreihenfolge an. Befehle von außen (Spieler, Skript)
//! werden mit Takt und laufender Nummer protokolliert. Seed, Datenprüfsumme und dieses
//! Protokoll genügen, um eine Partie exakt zu wiederholen.

use crate::ai::AiState;
use crate::battle::{Doctrine, Formation};
use crate::chronicle::{Chronicle, ChronicleKind};
use crate::clock::SimClock;
use crate::data::{GameData, ShipClass};
use crate::diplomacy::{self, Diplomacy};
use crate::economy::{Economy, to_credits, to_money};
use crate::endgame::Endgame;
use crate::finance::shares::ExpropriationScope;
use crate::finance::{self, Finance};
use crate::fleet::{self, Battles, Fleets, Stance};
use crate::galaxy::Galaxy;
use crate::ids::{FleetId, HolderId, IssuerId, PlanetId, PolityId, ShipId, SystemId};
use crate::logistics::Logistics;
use crate::polity::{self, Polities, Succession};
use crate::state::SimConfig;
use crate::tech::{self, Research};
use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SimCommand {
    /// Kolonie auf einem freien Planeten in Reichweite gründen.
    ClaimPlanet {
        planet: PlanetId,
    },
    /// Forschungsziel setzen (Index in `techs.ron`). `None` gibt die Wahl an die
    /// Automatik zurück.
    SetResearchTarget {
        tech: Option<u16>,
    },
    /// Anteil der laufenden Staatseinnahmen, der in die Forschung fließt (0,1 = 10 %),
    /// zwischen 0 und `max_budget_share` aus `balance.ron`.
    SetResearchBudget {
        share: f64,
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
    /// Bei der Werft einer eigenen Welt ein Schiff in Auftrag geben (`design` ist der
    /// Index in `GameData::designs`). Der Entwurf muss baubar sein: Die Gemeinschaft kennt
    /// alle Technologien, die er voraussetzt.
    BuildShip {
        planet: PlanetId,
        design: u16,
    },
    /// Einen eigenen Bauauftrag streichen. Verbautes wird nicht erstattet.
    CancelBuild {
        order: u32,
    },
    /// Zwei eigene Flotten vereinigen, die im selben System liegen.
    MergeFleets {
        from: FleetId,
        into: FleetId,
    },
    /// Die genannten Schiffe aus einer Flotte lösen; sie bilden im selben System eine neue.
    SplitFleet {
        fleet: FleetId,
        ships: Vec<ShipId>,
    },
    /// Aufstellung und Doktrin einer Flotte: stehende Befehle für künftige Gefechte, die
    /// auch in einem laufenden Gefecht sofort gelten.
    SetFleetOrders {
        fleet: FleetId,
        formation: Formation,
        doctrine: Doctrine,
    },
    /// Eine Flotte bricht das Gefecht ab, in dem sie steht: Ihre Schiffe verlassen das
    /// Feld, und was entkommt, weicht in ein Nachbarsystem aus.
    Retreat {
        fleet: FleetId,
    },
    /// Bei der Werft einer eigenen Welt ein Schiff einer Klasse in Auftrag geben. Den
    /// Entwurf wählt die Werft: den wertvollsten der Klasse, den die Gemeinschaft mit
    /// ihrem Wissen bauen kann (`GameData::best_design`).
    BuildClass {
        planet: PlanetId,
        class: ShipClass,
    },
    /// Schiffe einer eigenen Flotte für immer stilllegen. Die Flotte muss in einem System
    /// mit eigener Werft liegen. Die Schiffe verlassen das Spiel, Sold und Wartung enden;
    /// einen Teil ihrer Bauteile (`military.layup_salvage`) verkauft der Staat am Markt
    /// des Systems.
    LayUpShips {
        fleet: FleetId,
        ships: Vec<ShipId>,
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
    pub config: ResMut<'w, SimConfig>,
    pub galaxy: ResMut<'w, Galaxy>,
    pub polities: ResMut<'w, Polities>,
    pub economy: ResMut<'w, Economy>,
    pub finance: ResMut<'w, Finance>,
    pub research: ResMut<'w, Research>,
    pub logistics: ResMut<'w, Logistics>,
    pub fleets: ResMut<'w, Fleets>,
    pub battles: ResMut<'w, Battles>,
    pub diplomacy: ResMut<'w, Diplomacy>,
    pub endgame: ResMut<'w, Endgame>,
    pub chronicle: ResMut<'w, Chronicle>,
    pub ai: ResMut<'w, AiState>,
}

impl CommandContext<'_> {
    /// Leihgabe für Besitzwechsel, Gründung und Ende einer Gemeinschaft: So greift ein
    /// Befehl auf [`crate::polity::succession`] zu.
    pub fn succession(&mut self) -> Succession<'_> {
        Succession {
            data: &self.data,
            config: &mut self.config,
            galaxy: &mut self.galaxy,
            polities: &mut self.polities,
            economy: &mut self.economy,
            finance: &mut self.finance,
            logistics: &mut self.logistics,
            fleets: &mut self.fleets,
            battles: &mut self.battles,
            research: &mut self.research,
            diplomacy: &mut self.diplomacy,
            endgame: &mut self.endgame,
            chronicle: &mut self.chronicle,
            ai: &mut self.ai,
        }
    }
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
    // Befehle gibt nur, wen es gibt: Eine Gemeinschaft, die geendet hat, befiehlt nichts
    // mehr, auch wenn ihr Befehl schon in der Warteschlange stand.
    if !ctx
        .polities
        .list
        .get(polity.index())
        .is_some_and(|issuer| issuer.alive)
    {
        return Err("Gemeinschaft besteht nicht mehr".into());
    }
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
        SimCommand::SetResearchTarget { tech: target } => tech::set_target(
            &mut ctx.research,
            &ctx.data,
            &ctx.polities,
            &ctx.diplomacy,
            polity,
            *target,
        ),
        SimCommand::SetResearchBudget { share } => {
            tech::set_budget(&mut ctx.research, &ctx.data, &ctx.polities, polity, *share)
        }
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
            // Kriegsausbruch: Die Kurse beider Seiten geben nach, samt derer, die ihnen
            // unterstehen und den Krieg teilen.
            let factor = ctx.data.balance.finance.war_shock;
            let mut sides = ctx.diplomacy.bloc(polity);
            sides.extend(ctx.diplomacy.bloc(*target));
            for side in sides {
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
            ctx.data.balance.military.truce_days,
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
        SimCommand::DistributeShares { issuer, shares, to } => {
            // Empfangen kann nur, wen es gibt: ein lebender Staat oder die Halter einer
            // bekannten Welt. Sonst hielte ein Staat, der geendet hat, wieder Anteile und
            // bekäme Dividenden, die niemand mehr ausgibt.
            let known = match to {
                HolderId::State(receiver) => ctx
                    .polities
                    .list
                    .get(receiver.index())
                    .is_some_and(|found| found.alive),
                HolderId::Elite(planet) | HolderId::Public(planet) => {
                    planet.index() < ctx.galaxy.planets.len()
                }
            };
            if !known {
                return Err("Empfänger besteht nicht (mehr)".into());
            }
            finance::distribute_shares(
                &mut ctx.finance,
                HolderId::State(polity),
                *issuer,
                *shares,
                *to,
            )
            .map(|_| ())
        }
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
        SimCommand::BuildShip { planet, design } => {
            let day = ctx.clock.day();
            fleet::order_ship(
                &ctx.data,
                &ctx.galaxy,
                &ctx.economy,
                &ctx.polities,
                &mut ctx.fleets,
                polity,
                *planet,
                *design,
                ctx.research.known(polity),
                day,
            )
            .map(|_| ())
        }
        SimCommand::BuildClass { planet, class } => {
            let day = ctx.clock.day();
            let known = ctx.research.known(polity);
            let design = ctx.data.best_design(*class, known).ok_or_else(|| {
                format!(
                    "für die Klasse {} beherrscht die Gemeinschaft keinen Entwurf",
                    class.label()
                )
            })?;
            fleet::order_ship(
                &ctx.data,
                &ctx.galaxy,
                &ctx.economy,
                &ctx.polities,
                &mut ctx.fleets,
                polity,
                *planet,
                design,
                known,
                day,
            )
            .map(|_| ())
        }
        SimCommand::CancelBuild { order } => fleet::cancel_order(&mut ctx.fleets, polity, *order),
        SimCommand::MergeFleets { from, into } => {
            fleet::merge_fleets(&ctx.data, &mut ctx.fleets, polity, *from, *into)
        }
        SimCommand::SplitFleet { fleet, ships } => fleet::split_fleet(
            &ctx.data,
            &ctx.polities,
            &mut ctx.fleets,
            polity,
            *fleet,
            ships,
        )
        .map(|_| ()),
        SimCommand::SetFleetOrders {
            fleet,
            formation,
            doctrine,
        } => {
            fleet::set_fleet_orders(&mut ctx.fleets, polity, *fleet, *formation, *doctrine)?;
            // Steht die Flotte im Gefecht, gelten die neuen Befehle dort sofort.
            fleet::relay_orders(&ctx.data, &mut ctx.battles, *fleet, *formation, *doctrine);
            Ok(())
        }
        SimCommand::Retreat { fleet } => {
            fleet::order_retreat(&ctx.fleets, &mut ctx.battles, polity, *fleet)
        }
        SimCommand::LayUpShips { fleet, ships } => fleet::lay_up(
            &ctx.data,
            &ctx.galaxy,
            &mut ctx.economy,
            &ctx.polities,
            &mut ctx.fleets,
            &mut ctx.chronicle,
            tick,
            polity,
            *fleet,
            ships,
        )
        .map(|_| ()),
    }
}

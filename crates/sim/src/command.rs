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
use crate::diplomacy::evidence::{Act, ActKind, Audience, log_act};
use crate::diplomacy::occasion::{
    ACCEPT, BIND, DECLINE, LIMITED, Observed, Occasion, STRIKE, SUBJUGATE, decline_reason,
};
use crate::diplomacy::sight::{Ground, Perspective};
use crate::diplomacy::{self, Deal, Diplomacy, ProposalStatus, TreatyKind, WarAim, proposals};
use crate::economy::{Economy, to_credits, to_money};
use crate::endgame::Endgame;
use crate::finance::shares::ExpropriationScope;
use crate::finance::{self, Finance};
use crate::fleet::{self, Battles, Fleets, Stance};
use crate::galaxy::Galaxy;
use crate::ids::{FleetId, HolderId, IssuerId, PlanetId, PolityId, ShipId, SystemId};
use crate::logistics::Logistics;
use crate::polity::{self, Polities, Succession};
use crate::siege::{self, Posture, SiegeOrders, Sieges};
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
    /// Krieg erklären, mit begrenztem Ziel.
    DeclareWar {
        target: PolityId,
    },
    /// Sofortiger Frieden ohne Bedingungen, ohne Zustimmung der Gegenseite. Bleibt für
    /// Drehbücher und Prüfungen; die Oberfläche bietet Frieden als Vorschlag an
    /// (`Propose` mit `Deal::Peace`).
    MakePeace {
        target: PolityId,
    },
    /// Krieg erklären, mit einem Kriegsziel. Bestehen Nichtangriff, Verteidigung oder
    /// Union, ist das ein Vertragsbruch.
    DeclareWarWithAim {
        target: PolityId,
        aim: WarAim,
    },
    /// Einer anderen freien Gemeinschaft etwas vorschlagen: Vertrag, Frieden zu
    /// Bedingungen, Schutz, Vereinigung.
    Propose {
        to: PolityId,
        deal: Deal,
    },
    /// Einen Vorschlag an die eigene Gemeinschaft annehmen oder ablehnen.
    AnswerProposal {
        proposal: u32,
        accept: bool,
    },
    /// Einen eigenen offenen Vorschlag zurückziehen.
    WithdrawProposal {
        proposal: u32,
    },
    /// Einen Vertrag kündigen: Er und alle, die auf ihm aufbauen, enden nach der Frist
    /// (`notice_days`).
    CancelTreaty {
        with: PolityId,
        kind: TreatyKind,
    },
    /// Embargo gegen eine andere Gemeinschaft verhängen oder aufheben. Unter einem
    /// Handelsvertrag nicht möglich.
    SetEmbargo {
        target: PolityId,
        on: bool,
    },
    /// Den Flotten einer anderen Gemeinschaft Durchfahrt gewähren oder entziehen.
    SetTransit {
        target: PolityId,
        open: bool,
    },
    /// Einen eigenen Schützling oder Vasallen in die Eigenständigkeit entlassen.
    ReleaseWard {
        ward: PolityId,
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
    /// Eine eigene Flotte belagert einen Planeten in ihrem System, mit dessen Besitzer
    /// Krieg herrscht. Ersetzt ein früheres Ziel der Flotte.
    BeginSiege {
        fleet: FleetId,
        planet: PlanetId,
    },
    /// Stehende Befehle für eine eigene Belagerung: Feuerstufe, Sturm oder Halten,
    /// Schwerpunkt (Index der Zone), Plünderung nach dem Fall.
    SetSiegeOrders {
        planet: PlanetId,
        orders: SiegeOrders,
    },
    /// Eine eigene Belagerung aufheben. Truppen am Boden schiffen sich wieder ein, soweit
    /// an Bord Platz ist; stehen welche am Boden, braucht der Abzug die Orbitalhoheit.
    LiftSiege {
        planet: PlanetId,
    },
    /// Haltung der Garnison einer eigenen Welt, und ob ihre Geschütze schweigen, um
    /// Vorräte zu sparen.
    SetGarrisonOrders {
        planet: PlanetId,
        posture: Posture,
        hold_fire: bool,
    },
    /// Reguläre Verbände einer eigenen Welt auf eine eigene Flotte in deren System
    /// einschiffen, mit Nachschub für `supply_days` Tage vom Markt des Systems, soweit
    /// Ware und Staatskasse reichen.
    EmbarkTroops {
        fleet: FleetId,
        planet: PlanetId,
        strength: f64,
        supply_days: u16,
    },
    /// Eingeschiffte Verbände einer eigenen Flotte an Land setzen. Auf einer eigenen Welt
    /// verstärken sie die Garnison, und ihr Nachschub füllt das Depot. Auf einer fremden
    /// Welt landen sie als Welle der eigenen Belagerung; der Planet muss eingeschlossen
    /// sein, und aktive Geschütze fangen einen Teil der Welle ab.
    LandTroops {
        fleet: FleetId,
        planet: PlanetId,
        strength: f64,
    },
    /// Eine belagerte eigene Welt übergeben. Der Belagerer braucht eine Besatzungstruppe
    /// am Boden oder im Orbit; geplündert wird nach einer Übergabe nicht.
    Capitulate {
        planet: PlanetId,
    },
    /// Ausbauziel der stehenden Verteidigung einer eigenen Welt: Stufen der
    /// Orbitalstationen, Geschütze, des Schilds und der Bunker, reguläre Verbände und
    /// Tage im Depot. Bezahlt wird aus dem Anspruch „Verteidigung" des Staatshaushalts.
    SetDefensePlan {
        planet: PlanetId,
        plan: siege::DefensePlan,
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
    pub sieges: ResMut<'w, Sieges>,
    pub diplomacy: ResMut<'w, Diplomacy>,
    pub endgame: ResMut<'w, Endgame>,
    pub chronicle: ResMut<'w, Chronicle>,
    pub ai: ResMut<'w, AiState>,
}

impl CommandContext<'_> {
    /// Lesender Blick für Schätzungen und das öffentliche Bild einer Lage.
    pub fn ground(&self) -> Ground<'_> {
        Ground {
            data: &self.data,
            galaxy: &self.galaxy,
            polities: &self.polities,
            diplomacy: &self.diplomacy,
            fleets: &self.fleets,
            research: &self.research,
            finance: &self.finance,
            seed: self.config.seed,
            day: self.clock.day(),
        }
    }

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
    // Flotten, die jetzt belagern, lässt die Flotten-KI schon in diesem Takt in Ruhe.
    siege::reserve_besiegers(&mut ctx.sieges, &mut ctx.ai);
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
            let owners: Vec<Option<PolityId>> = ctx
                .galaxy
                .systems
                .iter()
                .map(|system| ctx.galaxy.system_owner(system.id))
                .collect();
            let diplomacy = &*ctx.diplomacy;
            let closed = ctx.data.diplomacy.levers.closed_borders;
            let mover = ctx.fleets.get(*fleet).map_or(polity, |found| found.owner);
            if closed
                && let Some(owner) = owners.get(to.index()).copied().flatten()
                && !diplomacy.may_enter(mover, Some(owner))
            {
                return Err(format!(
                    "keine Durchfahrt: {} gewährt den Flotten von {} keinen Zugang",
                    ctx.polities.get(owner).name,
                    ctx.polities.get(polity).name
                ));
            }
            fleet::order_move_within(
                &ctx.galaxy,
                &mut ctx.fleets,
                polity,
                *fleet,
                *to,
                |system| !closed || diplomacy.may_enter(mover, owners[system.index()]),
            )
        }
        SimCommand::SetStance { fleet, stance } => {
            fleet::set_stance(&mut ctx.fleets, polity, *fleet, *stance)
        }
        SimCommand::DeclareWar { target } => declare(ctx, polity, *target, WarAim::Limited),
        SimCommand::DeclareWarWithAim { target, aim } => declare(ctx, polity, *target, *aim),
        SimCommand::Propose { to, deal } => propose(ctx, polity, *to, deal),
        SimCommand::AnswerProposal { proposal, accept } => answer(ctx, polity, *proposal, *accept),
        SimCommand::WithdrawProposal { proposal } => {
            let day = ctx.clock.day();
            match ctx
                .diplomacy
                .proposals
                .iter()
                .find(|open| open.id == *proposal)
            {
                Some(open) if open.from == polity => {}
                Some(_) => return Err("nur der Absender kann einen Vorschlag zurückziehen".into()),
                None => return Err("kein offener Vorschlag mit dieser Nummer".into()),
            }
            if let Some(open) = proposals::take(&mut ctx.diplomacy, *proposal) {
                proposals::record(&mut ctx.diplomacy, open, ProposalStatus::Withdrawn, day);
            }
            Ok(())
        }
        SimCommand::CancelTreaty { with, kind } => diplomacy::treaties::denounce(
            &mut ctx.diplomacy,
            &ctx.polities,
            &mut ctx.chronicle,
            tick,
            polity,
            *with,
            *kind,
            ctx.data.diplomacy.levers.notice_days,
        ),
        SimCommand::SetEmbargo { target, on } => {
            diplomacy::check_pair(&ctx.diplomacy, &ctx.polities, polity, *target)?;
            let relation = ctx.diplomacy.relation(polity, *target);
            let side = diplomacy::relations::side(polity, *target);
            if relation.is_some_and(|relation| relation.embargo[side] == *on) {
                return Err(if *on {
                    "das Embargo besteht bereits".into()
                } else {
                    "es besteht kein Embargo".into()
                });
            }
            if *on
                && relation.is_some_and(|relation| {
                    relation.has(TreatyKind::Trade) || relation.has(TreatyKind::Union)
                })
            {
                return Err(
                    "unter einem Handelsvertrag gibt es kein Embargo; erst kündigen".into(),
                );
            }
            ctx.diplomacy.relation_mut(polity, *target).embargo[side] = *on;
            if *on {
                ctx.diplomacy.tally.embargoes += 1;
                ctx.diplomacy.tally.actor(polity).embargoes += 1;
            }
            let (who, whom) = (
                ctx.polities.get(polity).name.clone(),
                ctx.polities.get(*target).name.clone(),
            );
            ctx.chronicle.push(
                tick,
                ChronicleKind::Diplomacy,
                Some(polity),
                None,
                if *on {
                    format!("{who} verhängt ein Embargo gegen {whom}.")
                } else {
                    format!("{who} hebt das Embargo gegen {whom} auf.")
                },
            );
            Ok(())
        }
        SimCommand::SetTransit { target, open } => {
            diplomacy::check_pair(&ctx.diplomacy, &ctx.polities, polity, *target)?;
            if *open && ctx.diplomacy.at_war(polity, *target) {
                return Err("einem Kriegsgegner wird keine Durchfahrt gewährt".into());
            }
            let side = diplomacy::relations::side(polity, *target);
            let relation = ctx.diplomacy.relation_mut(polity, *target);
            if relation.transit[side] == *open {
                return Err(if *open {
                    "die Durchfahrt ist bereits gewährt".into()
                } else {
                    "es ist keine Durchfahrt gewährt".into()
                });
            }
            relation.transit[side] = *open;
            let (who, whom) = (
                ctx.polities.get(polity).name.clone(),
                ctx.polities.get(*target).name.clone(),
            );
            ctx.chronicle.push(
                tick,
                ChronicleKind::Diplomacy,
                Some(polity),
                None,
                if *open {
                    format!("{who} gewährt den Flotten von {whom} Durchfahrt.")
                } else {
                    format!("{who} entzieht den Flotten von {whom} die Durchfahrt.")
                },
            );
            Ok(())
        }
        SimCommand::ReleaseWard { ward } => diplomacy::proposals::release_ward(
            &mut ctx.diplomacy,
            &ctx.polities,
            &ctx.galaxy,
            &mut ctx.endgame,
            &mut ctx.chronicle,
            tick,
            polity,
            *ward,
        ),
        SimCommand::MakePeace { target } => {
            // Wer belagert wird, kann den Frieden nicht selbst schließen.
            siege::peace_veto(&ctx.galaxy, &ctx.polities, &ctx.sieges, polity, *target)?;
            diplomacy::make_peace(
                &mut ctx.diplomacy,
                &ctx.polities,
                &mut ctx.chronicle,
                tick,
                polity,
                *target,
                ctx.data.balance.military.truce_days,
            )?;
            ctx.diplomacy.tally.peace(diplomacy::PeaceKind::Imposed);
            Ok(())
        }
        SimCommand::BuyShares {
            issuer,
            shares,
            max_spend,
        } => {
            across_hostility(ctx, polity, *issuer)?;
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
        SimCommand::TakeoverBid { issuer, premium } => {
            across_hostility(ctx, polity, *issuer)?;
            finance::takeover_bid(
                &mut ctx.finance,
                &mut ctx.economy,
                &ctx.galaxy,
                HolderId::State(polity),
                *issuer,
                *premium,
            )
            .map(|_| ())
        }
        SimCommand::Expropriate { issuer, scope } => {
            let expropriation = finance::expropriate(
                &mut ctx.finance,
                &ctx.galaxy,
                &ctx.logistics,
                &ctx.data,
                polity,
                *issuer,
                *scope,
            )?;
            // Wer Anteile eines Investitionspartners einzieht, bricht den Vertrag.
            for wronged in expropriation.wronged {
                if wronged != polity
                    && ctx
                        .diplomacy
                        .has_treaty(polity, wronged, TreatyKind::Investment)
                {
                    breach(ctx, polity, wronged, &[TreatyKind::Investment]);
                }
            }
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
            fleet::merge_fleets(&ctx.data, &mut ctx.fleets, polity, *from, *into)?;
            // Truppen an Bord und das Ziel einer Belagerung gehen mit den Schiffen.
            siege::Orbit::new(&mut ctx.fleets, &ctx.research).merged(&mut ctx.sieges, *from, *into);
            Ok(())
        }
        SimCommand::SplitFleet { fleet, ships } => {
            let detached = fleet::split_fleet(
                &ctx.data,
                &ctx.polities,
                &mut ctx.fleets,
                polity,
                *fleet,
                ships,
            )?;
            siege::Orbit::new(&mut ctx.fleets, &ctx.research).split(
                &ctx.data,
                &mut ctx.sieges,
                *fleet,
                detached,
            );
            Ok(())
        }
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
        SimCommand::BeginSiege { fleet, planet } => siege::begin_siege(
            &ctx.galaxy,
            &ctx.polities,
            &ctx.diplomacy,
            &mut ctx.sieges,
            &siege::Orbit::new(&mut ctx.fleets, &ctx.research),
            polity,
            *fleet,
            *planet,
        ),
        SimCommand::SetSiegeOrders { planet, orders } => {
            siege::set_siege_orders(&ctx.galaxy, &mut ctx.sieges, polity, *planet, *orders)
        }
        SimCommand::LiftSiege { planet } => siege::lift_siege(
            &ctx.data,
            &ctx.galaxy,
            &ctx.polities,
            &ctx.diplomacy,
            &mut ctx.finance,
            &mut ctx.sieges,
            &siege::Orbit::new(&mut ctx.fleets, &ctx.research),
            &mut ctx.chronicle,
            polity,
            *planet,
            tick,
        ),
        SimCommand::SetGarrisonOrders {
            planet,
            posture,
            hold_fire,
        } => siege::set_garrison_orders(
            &ctx.data,
            &ctx.galaxy,
            &mut ctx.sieges,
            polity,
            *planet,
            *posture,
            *hold_fire,
        ),
        SimCommand::EmbarkTroops {
            fleet,
            planet,
            strength,
            supply_days,
        } => siege::embark_troops(
            &ctx.data,
            &ctx.galaxy,
            &ctx.diplomacy,
            &mut ctx.economy,
            &mut ctx.sieges,
            &siege::Orbit::new(&mut ctx.fleets, &ctx.research),
            polity,
            *fleet,
            *planet,
            *strength,
            *supply_days,
        ),
        SimCommand::LandTroops {
            fleet,
            planet,
            strength,
        } => siege::land_troops(
            &ctx.data,
            &ctx.galaxy,
            &ctx.polities,
            &ctx.diplomacy,
            &mut ctx.finance,
            &mut ctx.sieges,
            &siege::Orbit::new(&mut ctx.fleets, &ctx.research),
            polity,
            *fleet,
            *planet,
            *strength,
            tick,
        ),
        SimCommand::Capitulate { planet } => siege::capitulate(
            &ctx.data,
            &ctx.galaxy,
            &ctx.diplomacy,
            &mut ctx.sieges,
            &siege::Orbit::new(&mut ctx.fleets, &ctx.research),
            polity,
            *planet,
        ),
        SimCommand::SetDefensePlan { planet, plan } => siege::set_defense_plan(
            &ctx.data,
            &ctx.galaxy,
            &ctx.research,
            &mut ctx.sieges,
            polity,
            *planet,
            plan,
        ),
    }
}

/// Keine Käufe über Krieg und Embargo hinweg.
fn across_hostility(
    ctx: &CommandContext,
    polity: PolityId,
    issuer: IssuerId,
) -> Result<(), String> {
    if let Some(seat) = finance::polity_of_issuer(&ctx.galaxy, &ctx.logistics, issuer)
        && seat != polity
        && (ctx.diplomacy.at_war(polity, seat) || ctx.diplomacy.embargoed(polity, seat))
    {
        return Err(format!(
            "über Krieg und Embargo hinweg kauft niemand Anteile an Betrieben von {}",
            ctx.polities.get(seat).name
        ));
    }
    Ok(())
}

/// Vertragsbruch: Verträge enden, das Opfer grollt, die Kurse des Brechenden fallen,
/// und das Opfer wird misstrauischer.
fn breach(ctx: &mut CommandContext, breaker: PolityId, victim: PolityId, broken: &[TreatyKind]) {
    let tick = ctx.clock.tick;
    diplomacy::treaties::breach(
        &mut ctx.diplomacy,
        &ctx.polities,
        &mut ctx.chronicle,
        tick,
        breaker,
        victim,
        broken,
    );
    finance::shares::shock_polity(
        &mut ctx.finance,
        &ctx.galaxy,
        &ctx.logistics,
        breaker,
        ctx.data.diplomacy.levers.breach_shock,
    );
    ctx.ai.minds.jolt(victim, crate::ai::mind::Jolt::Betrayed);
}

/// Kriegserklärung mit Ziel. Die Erklärung und das Ziel sind öffentliche Taten.
fn declare(
    ctx: &mut CommandContext,
    polity: PolityId,
    target: PolityId,
    aim: WarAim,
) -> Result<(), String> {
    let tick = ctx.clock.tick;
    let day = ctx.clock.day();
    diplomacy::check_pair(&ctx.diplomacy, &ctx.polities, polity, target)?;
    // Das öffentliche Bild der Lage vor der Erklärung.
    let picture = ctx.ground().picture(Perspective::Public, polity, target);
    let figures = diplomacy::war::figures(
        &ctx.data,
        &ctx.galaxy,
        &ctx.diplomacy,
        &ctx.fleets,
        polity,
        target,
    );
    let declared = diplomacy::declare_war_with(
        &mut ctx.diplomacy,
        &ctx.polities,
        &mut ctx.chronicle,
        tick,
        polity,
        target,
        aim,
        Some(figures),
    )?;
    if !declared.breached.is_empty() {
        finance::shares::shock_polity(
            &mut ctx.finance,
            &ctx.galaxy,
            &ctx.logistics,
            polity,
            ctx.data.diplomacy.levers.breach_shock,
        );
        ctx.ai.minds.jolt(target, crate::ai::mind::Jolt::Betrayed);
    }
    // Kriegsausbruch: Die Kurse beider Seiten geben nach, samt derer, die ihnen
    // unterstehen und den Krieg teilen.
    let factor = ctx.data.balance.finance.war_shock;
    let mut sides = ctx.diplomacy.bloc(polity);
    sides.extend(ctx.diplomacy.bloc(target));
    for side in sides {
        finance::shares::shock_polity(&mut ctx.finance, &ctx.galaxy, &ctx.logistics, side, factor);
    }
    let whom = ctx.polities.get(target).name.clone();
    log_act(
        &mut ctx.diplomacy,
        Act {
            day,
            actor: polity,
            toward: target,
            kind: ActKind::Strike,
            occasion: Occasion::Opening(picture),
            observed: Observed::Option(STRIKE),
            audience: Audience::Public,
            source: format!("erklärt {whom} den Krieg"),
        },
    );
    log_act(
        &mut ctx.diplomacy,
        Act {
            day,
            actor: polity,
            toward: target,
            kind: ActKind::Aim,
            occasion: Occasion::Aim(picture),
            observed: Observed::Option(match aim {
                WarAim::Subjugation => SUBJUGATE,
                WarAim::Limited => LIMITED,
            }),
            audience: Audience::Public,
            source: format!("Kriegsziel gegen {whom}: {}", aim.label()),
        },
    );
    Ok(())
}

/// Einen Vorschlag einstellen. Der erste Antrag einer Bindung im Jahr ist eine
/// öffentliche Tat.
fn propose(
    ctx: &mut CommandContext,
    polity: PolityId,
    to: PolityId,
    deal: &Deal,
) -> Result<(), String> {
    let tick = ctx.clock.tick;
    let day = ctx.clock.day();
    diplomacy::check_pair(&ctx.diplomacy, &ctx.polities, polity, to)?;
    let picture = ctx.ground().picture(Perspective::Public, polity, to);
    proposals::propose(
        &ctx.data,
        &ctx.galaxy,
        &ctx.polities,
        &mut ctx.diplomacy,
        &ctx.finance,
        tick,
        polity,
        to,
        deal.clone(),
    )?;
    let year = day / crate::clock::DAYS_PER_YEAR;
    let whom = ctx.polities.get(to).name.clone();
    if let Deal::Treaty(kind) = deal
        && ctx.diplomacy.bind_year.get(&(polity, to)) != Some(&year)
    {
        ctx.diplomacy.bind_year.insert((polity, to), year);
        log_act(
            &mut ctx.diplomacy,
            Act {
                day,
                actor: polity,
                toward: to,
                kind: ActKind::Bind,
                occasion: Occasion::Opening(picture),
                observed: Observed::Option(BIND),
                audience: Audience::Public,
                source: format!("trägt {whom} den Vertrag „{}“ an", kind.label()),
            },
        );
    }
    // Verträge und Anteilstausch sind Alltag: Die Chronik nennt nur, was geschlossen wird.
    if !matches!(deal, Deal::Treaty(_) | Deal::Swap { .. }) {
        let who = ctx.polities.get(polity).name.clone();
        let label = deal.label(&ctx.polities);
        ctx.chronicle.push(
            tick,
            ChronicleKind::Diplomacy,
            Some(polity),
            None,
            format!("{who} schlägt {whom} vor: {label}."),
        );
    }
    Ok(())
}

/// Einen Vorschlag annehmen oder ablehnen. Die Annahme ist öffentlich, die Ablehnung
/// sieht nur der Absender.
fn answer(ctx: &mut CommandContext, polity: PolityId, id: u32, accept: bool) -> Result<(), String> {
    let tick = ctx.clock.tick;
    let day = ctx.clock.day();
    let proposal = match ctx.diplomacy.proposals.iter().find(|open| open.id == id) {
        Some(open) if open.to == polity => open.clone(),
        Some(_) => return Err("nur der Empfänger kann einen Vorschlag beantworten".into()),
        None => return Err("kein offener Vorschlag mit dieser Nummer".into()),
    };
    let levers = ctx.data.diplomacy.levers.clone();
    let picture = ctx
        .ground()
        .picture(Perspective::Public, polity, proposal.from);
    let answering = proposals::answering(&ctx.galaxy, &proposal.deal, polity, &ctx.data);
    let sender = ctx.polities.get(proposal.from).name.clone();
    let label = proposal.deal.label(&ctx.polities);
    let occasion = Occasion::Answer(answering, picture);
    proposals::take(&mut ctx.diplomacy, id);
    if accept {
        if let Err(reason) = proposals::conclude(&mut ctx.succession(), &proposal, tick) {
            proposals::record(&mut ctx.diplomacy, proposal, ProposalStatus::Void, day);
            return Err(format!("Vorschlag hinfällig: {reason}"));
        }
        if let Deal::Peace(terms) = &proposal.deal
            && terms.subjugate
            && let Some(yielder) = terms.yielder
        {
            let victor = if yielder == proposal.from {
                proposal.to
            } else {
                proposal.from
            };
            ctx.ai.minds.jolt(victor, crate::ai::mind::Jolt::Subjugated);
        }
        // Wer eben aufgegangen ist, hinterlässt keine Tat mehr.
        if ctx.polities.list[polity.index()].alive && ctx.polities.list[proposal.from.index()].alive
        {
            log_act(
                &mut ctx.diplomacy,
                Act {
                    day,
                    actor: polity,
                    toward: proposal.from,
                    kind: ActKind::Answer,
                    occasion,
                    observed: Observed::Option(ACCEPT),
                    audience: Audience::Public,
                    source: format!("nimmt an: {label} ({sender})"),
                },
            );
        }
        proposals::record(&mut ctx.diplomacy, proposal, ProposalStatus::Accepted, day);
    } else {
        let uniform = crate::data::ArchetypeDef::blend(&ctx.data.diplomacy.archetypes, &[0.25; 4]);
        let reason = decline_reason(answering, &picture, &uniform, &levers);
        log_act(
            &mut ctx.diplomacy,
            Act {
                day,
                actor: polity,
                toward: proposal.from,
                kind: ActKind::Answer,
                occasion,
                observed: Observed::Option(DECLINE),
                audience: Audience::Counterpart,
                source: format!("lehnt ab: {label} ({})", reason.label()),
            },
        );
        if matches!(
            proposal.deal,
            Deal::Peace(_) | Deal::Merge { .. } | Deal::Protectorate { .. }
        ) {
            let who = ctx.polities.get(polity).name.clone();
            ctx.chronicle.push(
                tick,
                ChronicleKind::Diplomacy,
                Some(polity),
                None,
                format!(
                    "{who} lehnt ab: {label} ({sender}). Grund: {}.",
                    reason.label()
                ),
            );
        }
        proposals::decline(&mut ctx.diplomacy, &ctx.data, proposal, Some(reason), day);
    }
    Ok(())
}

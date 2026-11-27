use super::military::{Situation, lead_fleets, plan_month};
use super::siege::{lead_sieges, plan_defenses};
use super::{AiState, MilitaryMode, best_colony_target};
use crate::clock::{SimClock, day_start, month_start};
use crate::command::{CommandQueue, CommandSource, SimCommand};
use crate::data::GameData;
use crate::diplomacy::Diplomacy;
use crate::economy::{Economy, Money, to_credits, to_money};
use crate::finance::{Finance, Stage};
use crate::fleet::{Battles, Fleets, fleet_claim};
use crate::galaxy::Galaxy;
use crate::ids::{HolderId, IssuerId};
use crate::logistics::Logistics;
use crate::polity::{Polities, claimable_planets};
use crate::siege::Sieges;
use crate::state::SimConfig;
use crate::tech::Research;
use crate::{SimSet, sim_schedule};
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;

pub struct AiPlugin;

impl Plugin for AiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            sim_schedule(),
            (
                expand.run_if(day_start),
                steward_finances.run_if(month_start),
                plan_military.run_if(month_start),
                plan_defense.run_if(month_start),
                lead_siege.run_if(day_start),
                lead_military.run_if(day_start),
            )
                .chain()
                .in_set(SimSet::Ai),
        );
    }
}

/// Lesender Zugriff der militärischen Führung auf den Stand der Welt.
#[derive(SystemParam)]
struct MilitaryView<'w> {
    data: Res<'w, GameData>,
    galaxy: Res<'w, Galaxy>,
    polities: Res<'w, Polities>,
    economy: Res<'w, Economy>,
    fleets: Res<'w, Fleets>,
    battles: Res<'w, Battles>,
    diplomacy: Res<'w, Diplomacy>,
    research: Res<'w, Research>,
    finance: Res<'w, Finance>,
    logistics: Res<'w, Logistics>,
    sieges: Res<'w, Sieges>,
    clock: Res<'w, SimClock>,
}

impl MilitaryView<'_> {
    fn situation(&self, seed: u64) -> Situation<'_> {
        Situation {
            data: &self.data,
            galaxy: &self.galaxy,
            polities: &self.polities,
            economy: &self.economy,
            fleets: &self.fleets,
            battles: &self.battles,
            diplomacy: &self.diplomacy,
            research: &self.research,
            finance: &self.finance,
            logistics: &self.logistics,
            sieges: &self.sieges,
            seed,
            day: self.clock.day(),
        }
    }
}

/// Monatsbeginn: Forschungspolitik, Rüstung und Stilllegung ([`plan_month`]).
fn plan_military(
    view: MilitaryView,
    config: Res<SimConfig>,
    mut ai: ResMut<AiState>,
    mut queue: ResMut<CommandQueue>,
) {
    plan_month(
        &view.situation(config.seed),
        &mut ai,
        &mut queue,
        config.player,
    );
}

/// Monatsbeginn, nach der Rüstung: Verteidigungspläne der Welten ([`plan_defenses`]).
fn plan_defense(
    view: MilitaryView,
    config: Res<SimConfig>,
    mut ai: ResMut<AiState>,
    mut queue: ResMut<CommandQueue>,
) {
    plan_defenses(
        &view.situation(config.seed),
        &mut ai,
        &mut queue,
        config.player,
    );
}

/// Tagesbeginn, vor der Führung der Flotten: Feldzüge gegen Welten und die Befehle
/// belagerter eigener Welten ([`lead_sieges`]).
fn lead_siege(
    view: MilitaryView,
    config: Res<SimConfig>,
    mut ai: ResMut<AiState>,
    mut queue: ResMut<CommandQueue>,
) {
    lead_sieges(
        &view.situation(config.seed),
        &mut ai,
        &mut queue,
        config.player,
    );
}

/// Tagesbeginn: Führung der Flotten ([`lead_fleets`]).
fn lead_military(
    view: MilitaryView,
    config: Res<SimConfig>,
    mut ai: ResMut<AiState>,
    mut queue: ResMut<CommandQueue>,
) {
    lead_fleets(
        &view.situation(config.seed),
        &mut ai,
        &mut queue,
        config.player,
    );
}

/// Jede KI-Gemeinschaft, in ID-Reihenfolge, gründet eine Kolonie, sobald sie darf.
fn expand(
    data: Res<GameData>,
    config: Res<SimConfig>,
    clock: Res<SimClock>,
    galaxy: Res<Galaxy>,
    polities: Res<Polities>,
    economy: Res<Economy>,
    mut queue: ResMut<CommandQueue>,
) {
    let day = clock.day();
    let balance = &data.balance.economy;
    for polity in polities.alive() {
        if config.player == Some(polity.id) || polity.next_claim_day > day {
            continue;
        }
        // Eine KI gründet nur aus dem, was über dem Sockel ihrer Rücklage liegt: Der
        // Sockel bleibt für die Ansprüche des Haushalts in schwachen Monaten. Das Geld
        // dafür spart sie über ihr Rücklagenziel an (`steward_finances`).
        let free = economy.treasury(polity.id)
            - to_money(galaxy.population_of(polity.id) * balance.state_reserve_per_million);
        if free < to_money(data.balance.colonization.cost) {
            continue;
        }
        if let Some(planet) =
            best_colony_target(&data, &galaxy, &polities, &economy, polity.id, day)
        {
            queue.push(
                polity.id,
                CommandSource::Ai,
                SimCommand::ClaimPlanet { planet },
            );
        }
    }
}

/// Finanzpolitik der KI-Staaten, einmal im Monat.
///
/// Zusammenhalt: Wo eine Region unruhig wird und der Staat noch viele Anteile an ihren
/// Betrieben hält, überträgt er einen Teil an den Fonds der Region. Ausdehnung: Solange es
/// etwas zu besiedeln gibt, hebt der Staat sein Rücklagenziel um die Kosten einer Gründung
/// über den Sockel; der Haushalt füllt sie auf, bevor er ausschüttet. Flotte: Wer rüstet,
/// hält dazu einige Monate Sold und Wartung zurück (`military.ai.reserve_months`). Krieg: Ist die Kasse
/// leer, werden Anleihen ausgegeben und eine Kriegskasse aufgebaut.
fn steward_finances(
    data: Res<GameData>,
    config: Res<SimConfig>,
    clock: Res<SimClock>,
    galaxy: Res<Galaxy>,
    polities: Res<Polities>,
    economy: Res<Economy>,
    finance: Res<Finance>,
    diplomacy: Res<Diplomacy>,
    fleets: Res<Fleets>,
    ai: Res<AiState>,
    mut queue: ResMut<CommandQueue>,
) {
    let cohesion = &data.balance.finance.cohesion;
    for polity in polities.alive() {
        if config.player == Some(polity.id) {
            continue;
        }
        let state = HolderId::State(polity.id);

        // Besitz streuen, bevor eine Region kippt: eigene Anteile an den Fonds der Region
        // übertragen und, wenn Geld da ist, weitere am Markt dazukaufen. So holt der Staat
        // zurück, was sich bei der Elite der Hauptwelt gesammelt hat.
        let reserve = to_credits(economy.treasury_targets[polity.id.index()])
            .max(data.balance.colonization.cost * 1.5);
        let mut budget = (to_credits(economy.treasury(polity.id)) - reserve).max(0.0) * 0.25;
        for (id, issuer) in &finance.issuers {
            let IssuerId::Firm { planet, .. } = id else {
                continue;
            };
            if galaxy.planet(*planet).owner != Some(polity.id) || polity.capital == Some(*planet) {
                continue;
            }
            let region = finance.region(*planet);
            let worried =
                region.stage != Stage::Loyal || region.cohesion < cohesion.loyal_above + 5.0;
            if !worried {
                continue;
            }
            let held = issuer.held_by(state);
            let keep = issuer.shares / 20;
            if held > keep {
                queue.push(
                    polity.id,
                    CommandSource::Ai,
                    SimCommand::DistributeShares {
                        issuer: *id,
                        shares: (held - keep).min(issuer.shares / 10),
                        to: HolderId::Public(*planet),
                    },
                );
            }
            let local = issuer.share_of(HolderId::Public(*planet))
                + issuer.share_of(HolderId::Elite(*planet));
            let spend = (issuer.market_value() * 0.05).min(budget);
            if local < 0.5 && spend > 20.0 {
                budget -= spend;
                queue.push(
                    polity.id,
                    CommandSource::Ai,
                    SimCommand::BuyShares {
                        issuer: *id,
                        shares: issuer.shares / 20,
                        max_spend: spend,
                    },
                );
            }
        }

        // Koloniefonds: der Sockel der Rücklage und die Kosten einer Gründung, solange
        // noch eine Welt in Reichweite frei ist (Kasse und Wartefrist außer Acht).
        let base = galaxy.population_of(polity.id) * data.balance.economy.state_reserve_per_million;
        let prospects = !claimable_planets(
            &galaxy,
            &polities,
            &data.balance.colonization,
            Money::MAX,
            polity.id,
            clock.day().max(polity.next_claim_day),
        )
        .is_empty();
        let colony_fund = if prospects {
            base + data.balance.colonization.cost
        } else {
            0.0
        };
        // Rücklage für die Flotte: Die Einnahmen schwanken von Monat zu Monat stark, Sold
        // und Wartung nicht. Wer rüstet, hält deshalb einige Monate Flottenkosten zurück,
        // damit ein schwacher Abschnitt nicht gleich Sold kostet.
        let fleet_fund = if ai.mode(polity.id) == MilitaryMode::Resting {
            0.0
        } else {
            data.balance.military.ai.reserve_months
                * to_credits(fleet_claim(&data, &galaxy, &economy, &fleets, polity))
        };
        let colony_fund = if fleet_fund > 0.0 {
            colony_fund.max(base) + fleet_fund
        } else {
            colony_fund
        };
        let target = to_credits(economy.treasury_targets[polity.id.index()]);

        // Kriegsfinanzierung.
        let treasury = to_credits(economy.treasury(polity.id));
        if diplomacy.is_at_war(polity.id) {
            let revenue = finance.polity(polity.id).revenue.max(100.0);
            // Die Kriegskasse wird einmal gesetzt: wenn das Ziel noch das des Friedens ist.
            if target <= colony_fund.max(base) * 1.05 {
                let chest = (revenue * 24.0).max(colony_fund);
                if (chest - target).abs() > 0.02 * target.max(1.0) {
                    queue.push(
                        polity.id,
                        CommandSource::Ai,
                        SimCommand::SetTreasuryTarget { amount: chest },
                    );
                }
            }
            if treasury < revenue * 3.0 {
                queue.push(
                    polity.id,
                    CommandSource::Ai,
                    SimCommand::IssueBonds {
                        amount: revenue * 6.0,
                    },
                );
            }
        } else if (target - colony_fund).abs() > 0.02 * colony_fund.max(1.0) {
            queue.push(
                polity.id,
                CommandSource::Ai,
                SimCommand::SetTreasuryTarget {
                    amount: colony_fund,
                },
            );
        }
    }
}

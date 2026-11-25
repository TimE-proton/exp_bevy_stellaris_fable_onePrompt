use super::cohesion::apply_secession;
use super::{
    Finance, bond_yield, elite_spending, found_missing_issuers, invest_holders, pay_dividend,
    record_injection, reprice, service_bonds, shares::seat_of_issuer, update_cohesion,
};
use crate::chronicle::Chronicle;
use crate::clock::{SimClock, month_start};
use crate::data::GameData;
use crate::diplomacy::Diplomacy;
use crate::economy::{
    Economy, Money, invest_and_depreciate, settle_public_accounts, steer_money_supply, to_credits,
};
use crate::fleet::Fleets;
use crate::galaxy::Galaxy;
use crate::ids::PlanetId;
use crate::logistics::Logistics;
use crate::polity::Polities;
use crate::{SimSet, sim_schedule};
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

pub use crate::economy::EquityFlows;

/// Markt: Börse, Besitzverteilung, Staatsfinanzen und Zusammenhalt.
pub struct FinancePlugin;

impl Plugin for FinancePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            sim_schedule(),
            (
                monthly_books.in_set(SimSet::Finance).run_if(month_start),
                // Nach der Wanderung des Monats: erst dann steht fest, wer wo wohnt.
                cohesion
                    .in_set(SimSet::Cohesion)
                    .after(crate::galaxy::MigrationStep)
                    .run_if(month_start),
            ),
        );
    }
}

/// Monatsabschluss in fester Reihenfolge: Kapital wandert, öffentliche Kassen gleichen
/// aus, Einlagen werden zu Anteilen, Dividenden fließen an die Halter, der Staat bedient
/// seine Schulden, Halter legen an, Kurse bilden sich.
fn monthly_books(
    data: Res<GameData>,
    clock: Res<SimClock>,
    galaxy: Res<Galaxy>,
    polities: Res<Polities>,
    fleets: Res<Fleets>,
    diplomacy: Res<Diplomacy>,
    logistics: Res<Logistics>,
    mut economy: ResMut<Economy>,
    mut finance: ResMut<Finance>,
    mut flows: ResMut<EquityFlows>,
    mut chronicle: ResMut<Chronicle>,
) {
    let balance = &data.balance.finance;
    let capitals: Vec<Option<PlanetId>> = polities.list.iter().map(|p| p.capital).collect();

    // Die Eliten der Hauptwelten stellen jeden Monat einen Teil ihres Geldes für lohnende
    // Betriebe in den Kolonien bereit.
    let mut funds: Vec<Money> = capitals
        .iter()
        .map(|capital| {
            capital.map_or(0, |id| {
                (finance.elites[id.index()] as f64 * balance.colony_fund_share) as Money
            })
        })
        .collect();
    let offered = funds.clone();
    invest_and_depreciate(
        &galaxy,
        &mut economy,
        &data,
        &capitals,
        &mut funds,
        clock.day(),
        &mut flows,
    );
    for (polity, capital) in capitals.iter().enumerate() {
        if let Some(capital) = capital {
            finance.elites[capital.index()] -= offered[polity] - funds[polity];
        }
    }
    steer_money_supply(&galaxy, &mut economy, &data, &capitals);
    settle_public_accounts(&galaxy, &mut economy, &data, &mut flows);

    found_missing_issuers(
        &data,
        &galaxy,
        &economy,
        &logistics,
        &capitals,
        &mut finance,
    );
    let EquityFlows {
        dividends,
        injections,
    } = std::mem::take(&mut *flows);
    for (issuer, investor, amount) in injections {
        record_injection(&mut finance, issuer, investor, amount);
    }
    for (issuer, amount) in dividends {
        let seat = seat_of_issuer(&galaxy, &logistics, &capitals, issuer);
        pay_dividend(&mut finance, &mut economy, seat, issuer, amount);
    }

    service_bonds(
        &mut finance,
        &mut economy,
        &galaxy,
        &logistics,
        &polities,
        &mut chronicle,
        &data,
        clock.tick,
    );

    // Staatseinnahmen schätzen und Kriegsmüdigkeit fortschreiben.
    for polity in &polities.list {
        let index = polity.id.index();
        let treasury = economy.treasuries[index];
        let state = &mut finance.polities[index];
        let change = to_credits(treasury - state.last_treasury).max(0.0);
        state.revenue += 0.2 * (change - state.revenue);
        state.last_treasury = treasury;
        if diplomacy.is_at_war(polity.id) {
            state.war_weariness = (state.war_weariness + balance.weariness_per_month).min(100.0);
        } else {
            state.war_weariness = (state.war_weariness - balance.weariness_recovery).max(0.0);
        }
    }

    invest_holders(
        &mut finance,
        &mut economy,
        &galaxy,
        &logistics,
        &diplomacy,
        &data,
        &capitals,
    );
    elite_spending(&galaxy, &mut economy, &mut finance, balance.elite_spending);
    reprice(
        &mut finance,
        &data,
        &galaxy,
        &economy,
        &logistics,
        &fleets,
        &diplomacy,
        &capitals,
    );
    for polity in &polities.list {
        let rate = bond_yield(&data, &finance, polity.id);
        finance.polities[polity.id.index()].bond_yield = rate;
    }
}

fn cohesion(
    data: Res<GameData>,
    clock: Res<SimClock>,
    fleets: Res<Fleets>,
    logistics: Res<Logistics>,
    mut galaxy: ResMut<Galaxy>,
    mut polities: ResMut<Polities>,
    mut economy: ResMut<Economy>,
    mut finance: ResMut<Finance>,
    mut chronicle: ResMut<Chronicle>,
) {
    let secessions = update_cohesion(
        &mut finance,
        &galaxy,
        &mut economy,
        &logistics,
        &fleets,
        &polities,
        &data,
        clock.day(),
    );
    for secession in secessions {
        apply_secession(
            &secession,
            &mut finance,
            &mut galaxy,
            &mut economy,
            &logistics,
            &mut polities,
            &mut chronicle,
            &data,
            clock.tick,
        );
    }
}

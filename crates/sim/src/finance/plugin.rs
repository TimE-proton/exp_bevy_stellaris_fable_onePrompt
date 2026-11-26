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
    Claim, Economy, Money, Wants, invest_and_depreciate, settle_budget, settle_public_accounts,
    steer_money_supply, to_credits,
};
use crate::fleet::{Fleets, fleet_claim, procurement_estimate};
use crate::galaxy::Galaxy;
use crate::ids::{HolderId, IssuerId, PlanetId, PolityId};
use crate::logistics::Logistics;
use crate::polity::Polities;
use crate::state::SimConfig;
use crate::tech::{Research, research_claim};
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
/// aus, Einlagen werden zu Anteilen, Dividenden fließen an die Halter. Dann der
/// Staatshaushalt: Schuldendienst, Grundleistungen, die Bewilligungen für Flotte,
/// Beschaffung und Forschung, Rücklage und Ausschüttung. Zuletzt legen die Halter an, und
/// die Kurse bilden sich.
fn monthly_books(
    data: Res<GameData>,
    config: Res<SimConfig>,
    clock: Res<SimClock>,
    galaxy: Res<Galaxy>,
    polities: Res<Polities>,
    fleets: Res<Fleets>,
    research: Res<Research>,
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
    let before_dividends = economy.treasuries.clone();
    for (issuer, amount) in dividends {
        let seat = seat_of_issuer(&galaxy, &logistics, &capitals, issuer);
        pay_dividend(&mut finance, &mut economy, seat, issuer, amount);
    }
    // Dividenden auf staatliche Anteile zählen wie Lohnsteuer und Zölle zu den laufenden
    // Einnahmen. Damit ist der Monat der Einnahmenrechnung vollständig.
    for (index, before) in before_dividends.into_iter().enumerate() {
        let received = economy.treasuries[index] - before;
        economy.book_state_income(PolityId::from_index(index), received);
    }
    economy.close_income_month();

    // Der Haushalt des neuen Monats: Was der abgeschlossene eingebracht hat, steht fest.
    // Der Schuldendienst ist der erste Anspruch, dann folgen die übrigen in ihrer
    // Rangfolge. Flotte und Forschung geben ihre Bewilligung später in diesem Takt aus
    // (`SimSet::Fleets`, `SimSet::Research`), die Beschaffung den Monat über.
    economy.open_budget_month(clock.day(), data.balance.economy.income_tax);
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
    for polity in polities.alive() {
        let mut wants = Wants::new();
        wants.insert(
            Claim::Fleet,
            fleet_claim(&data, &galaxy, &economy, &fleets, polity),
        );
        wants.insert(
            Claim::Procurement,
            procurement_estimate(
                &data, &galaxy, &economy, &polities, &fleets, &diplomacy, polity.id,
            ),
        );
        wants.insert(
            Claim::Research,
            research_claim(
                &research, &economy, &galaxy, &polities, &diplomacy, &data, &config, polity.id,
            ),
        );
        // Für Einlagen in Kontore gibt die Börse dem Staat neue Anteile aus.
        for (system, amount) in settle_budget(&galaxy, &mut economy, &data, polity.id, &wants) {
            record_injection(
                &mut finance,
                IssuerId::Kontor(system),
                HolderId::State(polity.id),
                amount,
            );
        }
    }

    // Staatseinnahmen glätten und Kriegsmüdigkeit fortschreiben. Maß sind die laufenden
    // Einnahmen des abgeschlossenen Monats, dieselben, mit denen der Haushalt rechnet
    // (`BudgetMonth::revenue`): Der Kassenstand taugt dafür nicht, denn der Haushalt gibt
    // die Einnahmen Monat für Monat wieder aus.
    for polity in &polities.list {
        let index = polity.id.index();
        let treasury = economy.treasuries[index];
        let income = to_credits(economy.state_income_of(polity.id)).max(0.0);
        let state = &mut finance.polities[index];
        state.revenue += 0.2 * (income - state.revenue);
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
    mut logistics: ResMut<Logistics>,
    mut galaxy: ResMut<Galaxy>,
    mut polities: ResMut<Polities>,
    mut economy: ResMut<Economy>,
    mut finance: ResMut<Finance>,
    mut research: ResMut<Research>,
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
        let founded = apply_secession(
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
        // Ein neuer Freistaat nimmt das Wissen des Reichs mit, das er verlässt. Noch im
        // selben Takt hat er in jeder Liste je Gemeinschaft seinen Eintrag, und seine
        // Werke und Frachter rechnen mit der geerbten Technik.
        if let (None, Some(child)) = (secession.joins, founded) {
            research.inherit(&data, secession.from, child);
            economy.sync_polities();
            research.publish_effects(&mut economy, &mut logistics);
        }
    }
}

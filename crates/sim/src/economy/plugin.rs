use super::{Economy, EquityFlows, produce_and_consume, update_prices_and_wages};
use crate::clock::day_start;
use crate::data::GameData;
use crate::galaxy::Galaxy;
use crate::tech::Research;
use crate::{SimSet, sim_schedule};
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

/// Markiert den Produktionsschritt, damit andere Plugins sich danach einordnen können.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProductionStep;

/// Wirtschaft: Produktion, Konsum und Preise täglich. Die Monatsbücher (Kapital,
/// öffentliche Kassen, Dividenden) führt das Finanz-Plugin, weil sie die Börse brauchen.
pub struct EconomyPlugin;

impl Plugin for EconomyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EquityFlows>();
        app.add_systems(
            sim_schedule(),
            (
                production
                    .in_set(SimSet::Production)
                    .in_set(ProductionStep)
                    .run_if(day_start),
                prices.in_set(SimSet::Prices).run_if(day_start),
            ),
        );
    }
}

fn production(
    data: Res<GameData>,
    research: Res<Research>,
    mut galaxy: ResMut<Galaxy>,
    mut economy: ResMut<Economy>,
) {
    // Der Ertragsfaktor der Technik gilt ab dem Tag nach dem Abschluss einer Technologie
    // und bis zum Monatsabschluss auch für die Investitionsrechnung.
    economy.tech_yield.clear();
    economy.tech_yield.extend(
        research
            .polities
            .iter()
            .map(|entry| entry.effects.yield_factor),
    );
    produce_and_consume(&mut galaxy, &mut economy, &data);
}

fn prices(data: Res<GameData>, galaxy: Res<Galaxy>, mut economy: ResMut<Economy>) {
    update_prices_and_wages(&galaxy, &mut economy, &data);
}

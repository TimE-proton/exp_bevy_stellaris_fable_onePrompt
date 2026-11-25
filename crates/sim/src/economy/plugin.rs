use super::{
    Economy, invest_and_depreciate, produce_and_consume, settle_public_accounts,
    update_prices_and_wages,
};
use crate::clock::{SimClock, day_start, month_start};
use crate::data::GameData;
use crate::galaxy::Galaxy;
use crate::polity::Polities;
use crate::{SimSet, sim_schedule};
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

/// Markiert den Produktionsschritt, damit andere Plugins sich danach einordnen können.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProductionStep;

/// Wirtschaft: Produktion und Konsum täglich, Preise täglich, Kapital monatlich.
pub struct EconomyPlugin;

impl Plugin for EconomyPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            sim_schedule(),
            (
                production
                    .in_set(SimSet::Production)
                    .in_set(ProductionStep)
                    .run_if(day_start),
                prices.in_set(SimSet::Prices).run_if(day_start),
                monthly.in_set(SimSet::Finance).run_if(month_start),
            ),
        );
    }
}

fn production(data: Res<GameData>, mut galaxy: ResMut<Galaxy>, mut economy: ResMut<Economy>) {
    produce_and_consume(&mut galaxy, &mut economy, &data);
}

fn prices(data: Res<GameData>, galaxy: Res<Galaxy>, mut economy: ResMut<Economy>) {
    update_prices_and_wages(&galaxy, &mut economy, &data);
}

fn monthly(
    data: Res<GameData>,
    clock: Res<SimClock>,
    galaxy: Res<Galaxy>,
    polities: Res<Polities>,
    mut economy: ResMut<Economy>,
) {
    let capitals: Vec<_> = polities.list.iter().map(|polity| polity.capital).collect();
    invest_and_depreciate(&galaxy, &mut economy, &data, &capitals, clock.day());
    settle_public_accounts(&galaxy, &mut economy, &data);
}

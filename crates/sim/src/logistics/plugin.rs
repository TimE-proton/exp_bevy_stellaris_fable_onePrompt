use super::{Logistics, expand_fleets_of_trade, move_and_trade};
use crate::chronicle::Chronicle;
use crate::clock::{SimClock, day_start, month_start};
use crate::data::GameData;
use crate::diplomacy::Diplomacy;
use crate::economy::{Economy, EquityFlows};
use crate::fleet::Fleets;
use crate::galaxy::Galaxy;
use crate::polity::Polities;
use crate::state::SimConfig;
use crate::{SimSet, sim_schedule};
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

/// Warenflüsse: Frachter fahren, kaufen und verkaufen.
pub struct LogisticsPlugin;

impl Plugin for LogisticsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            sim_schedule(),
            (trade.run_if(day_start), shipbuilding.run_if(month_start))
                .chain()
                .in_set(SimSet::Logistics),
        );
    }
}

fn trade(
    data: Res<GameData>,
    config: Res<SimConfig>,
    clock: Res<SimClock>,
    galaxy: Res<Galaxy>,
    fleets: Res<Fleets>,
    diplomacy: Res<Diplomacy>,
    polities: Res<Polities>,
    mut economy: ResMut<Economy>,
    mut logistics: ResMut<Logistics>,
    mut chronicle: ResMut<Chronicle>,
) {
    move_and_trade(
        &galaxy,
        &mut economy,
        &mut logistics,
        &fleets,
        &diplomacy,
        &polities,
        &mut chronicle,
        &data,
        config.seed,
        clock.tick,
    );
}

fn shipbuilding(
    data: Res<GameData>,
    galaxy: Res<Galaxy>,
    mut economy: ResMut<Economy>,
    mut logistics: ResMut<Logistics>,
    mut flows: ResMut<EquityFlows>,
) {
    expand_fleets_of_trade(&galaxy, &mut economy, &mut logistics, &data, &mut flows);
}

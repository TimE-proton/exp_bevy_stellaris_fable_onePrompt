use super::{
    Fleets, advance_yards, move_fleets, pay_upkeep, repair_fleets, resolve_skirmishes,
    resupply_fleets, update_blockades,
};
use crate::chronicle::Chronicle;
use crate::clock::{SimClock, day_start, month_start};
use crate::data::GameData;
use crate::diplomacy::Diplomacy;
use crate::economy::Economy;
use crate::galaxy::Galaxy;
use crate::polity::Polities;
use crate::state::SimConfig;
use crate::{SimSet, sim_schedule};
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

/// Raumkampf auf der strategischen Ebene: Flottenbewegung, Sold, Nachschub, Werften,
/// Gefechte, Blockaden.
pub struct FleetPlugin;

impl Plugin for FleetPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            sim_schedule(),
            (
                // Erst der Marsch, dann der Sold (er geht dem Einkauf vor), dann Nachschub,
                // Instandsetzung und Werften aus derselben Staatskasse.
                (
                    movement.run_if(day_start),
                    upkeep.run_if(month_start),
                    service.run_if(day_start),
                )
                    .chain()
                    .in_set(SimSet::Fleets),
                combat.in_set(SimSet::SpaceCombat).run_if(day_start),
            ),
        );
    }
}

fn movement(data: Res<GameData>, galaxy: Res<Galaxy>, mut fleets: ResMut<Fleets>) {
    move_fleets(&galaxy, &mut fleets, &data);
}

fn upkeep(
    data: Res<GameData>,
    clock: Res<SimClock>,
    galaxy: Res<Galaxy>,
    polities: Res<Polities>,
    mut economy: ResMut<Economy>,
    mut fleets: ResMut<Fleets>,
    mut chronicle: ResMut<Chronicle>,
) {
    pay_upkeep(
        &galaxy,
        &mut economy,
        &mut fleets,
        &polities,
        &mut chronicle,
        &data,
        clock.tick,
    );
}

/// Was der Staat täglich für seine Flotten einkauft, in fester Reihenfolge: Munition und
/// Treibstoff, Instandsetzung, Neubauten.
fn service(
    data: Res<GameData>,
    clock: Res<SimClock>,
    galaxy: Res<Galaxy>,
    polities: Res<Polities>,
    mut economy: ResMut<Economy>,
    mut fleets: ResMut<Fleets>,
    mut chronicle: ResMut<Chronicle>,
) {
    resupply_fleets(&galaxy, &mut economy, &mut fleets, &data);
    repair_fleets(&galaxy, &mut economy, &mut fleets, &polities, &data);
    advance_yards(
        &galaxy,
        &mut economy,
        &mut fleets,
        &polities,
        &mut chronicle,
        &data,
        clock.tick,
    );
}

fn combat(
    data: Res<GameData>,
    config: Res<SimConfig>,
    clock: Res<SimClock>,
    galaxy: Res<Galaxy>,
    polities: Res<Polities>,
    diplomacy: Res<Diplomacy>,
    mut fleets: ResMut<Fleets>,
    mut chronicle: ResMut<Chronicle>,
) {
    resolve_skirmishes(
        &mut fleets,
        &diplomacy,
        &polities,
        &mut chronicle,
        &data,
        config.seed,
        clock.tick,
    );
    update_blockades(&galaxy, &mut fleets, &diplomacy);
}

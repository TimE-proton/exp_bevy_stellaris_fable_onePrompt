use super::{Fleets, move_fleets, resolve_skirmishes, update_blockades};
use crate::chronicle::Chronicle;
use crate::clock::{SimClock, day_start};
use crate::data::GameData;
use crate::diplomacy::Diplomacy;
use crate::galaxy::Galaxy;
use crate::polity::Polities;
use crate::state::SimConfig;
use crate::{SimSet, sim_schedule};
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

/// Raumkampf auf der strategischen Ebene: Flottenbewegung, Gefechte, Blockaden.
pub struct FleetPlugin;

impl Plugin for FleetPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            sim_schedule(),
            (
                movement.in_set(SimSet::Fleets).run_if(day_start),
                combat.in_set(SimSet::SpaceCombat).run_if(day_start),
            ),
        );
    }
}

fn movement(data: Res<GameData>, galaxy: Res<Galaxy>, mut fleets: ResMut<Fleets>) {
    move_fleets(&galaxy, &mut fleets, &data.balance.fleets);
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
        &data.balance.fleets,
        config.seed,
        clock.tick,
    );
    update_blockades(&galaxy, &mut fleets, &diplomacy);
}

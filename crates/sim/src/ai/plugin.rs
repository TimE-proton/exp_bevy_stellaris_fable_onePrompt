use super::best_colony_target;
use crate::clock::{SimClock, day_start};
use crate::command::{CommandQueue, CommandSource, SimCommand};
use crate::data::GameData;
use crate::economy::Economy;
use crate::galaxy::Galaxy;
use crate::polity::Polities;
use crate::state::SimConfig;
use crate::{SimSet, sim_schedule};
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

pub struct AiPlugin;

impl Plugin for AiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(sim_schedule(), expand.in_set(SimSet::Ai).run_if(day_start));
    }
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
    for polity in polities.alive() {
        if config.player == Some(polity.id) || polity.next_claim_day > day {
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

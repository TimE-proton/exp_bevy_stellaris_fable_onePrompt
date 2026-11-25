use super::Galaxy;
use crate::clock::{DAYS_PER_YEAR, day_start};
use crate::data::GameData;
use crate::{SimSet, sim_schedule};
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

pub struct GalaxyPlugin;

impl Plugin for GalaxyPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            sim_schedule(),
            grow_population.in_set(SimSet::Production).run_if(day_start),
        );
    }
}

/// Logistisches Wachstum zur Tragfähigkeit. Ab Meilenstein 2 hängt es an der Versorgung.
fn grow_population(data: Res<GameData>, mut galaxy: ResMut<Galaxy>) {
    let rate = data.balance.population.growth_per_year / DAYS_PER_YEAR as f64;
    for planet in &mut galaxy.planets {
        if planet.owner.is_none() || planet.population <= 0.0 {
            continue;
        }
        let headroom = 1.0 - planet.population / planet.capacity;
        planet.population = (planet.population + rate * planet.population * headroom).max(0.0);
    }
}

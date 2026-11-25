use super::Galaxy;
use crate::chronicle::{Chronicle, ChronicleKind};
use crate::clock::{SimClock, day_start, month_start};
use crate::data::GameData;
use crate::economy::{Economy, abandon_dead_colonies, grow_population, migrate};
use crate::polity::Polities;
use crate::{SimSet, sim_schedule};
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

/// Galaxie: die Planeten und ihre Bevölkerung.
pub struct GalaxyPlugin;

impl Plugin for GalaxyPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            sim_schedule(),
            (
                // Nach Produktion und Konsum des Tages, vor der Preisbildung.
                demography
                    .in_set(SimSet::Production)
                    .after(crate::economy::ProductionStep)
                    .run_if(day_start),
                migration.in_set(SimSet::Cohesion).run_if(month_start),
            ),
        );
    }
}

/// Wachstum folgt der Versorgung, Mangel lässt Welten schrumpfen.
fn demography(data: Res<GameData>, mut galaxy: ResMut<Galaxy>, mut economy: ResMut<Economy>) {
    grow_population(&mut galaxy, &mut economy, &data);
}

/// Menschen ziehen zu besseren Löhnen und Lebensbedingungen; erloschene Kolonien werden
/// aufgegeben.
fn migration(
    data: Res<GameData>,
    clock: Res<SimClock>,
    polities: Res<Polities>,
    mut galaxy: ResMut<Galaxy>,
    mut economy: ResMut<Economy>,
    mut chronicle: ResMut<Chronicle>,
) {
    migrate(&mut galaxy, &mut economy, &data, polities.list.len());
    let capitals: Vec<_> = polities.list.iter().map(|polity| polity.capital).collect();
    for (planet, owner) in abandon_dead_colonies(&mut galaxy, &mut economy, &capitals, &data) {
        chronicle.push(
            clock.tick,
            ChronicleKind::Colony,
            Some(owner),
            Some(galaxy.planet(planet).system),
            format!(
                "{} gibt die Kolonie {} auf.",
                polities.get(owner).name,
                galaxy.planet(planet).name
            ),
        );
    }
}

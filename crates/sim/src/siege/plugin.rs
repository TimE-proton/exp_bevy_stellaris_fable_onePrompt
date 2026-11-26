use super::{OrbitAccess, Sieges, run_day, tidy};
use crate::chronicle::Chronicle;
use crate::clock::{SimClock, day_start};
use crate::data::GameData;
use crate::diplomacy::Diplomacy;
use crate::economy::Economy;
use crate::finance::Finance;
use crate::galaxy::Galaxy;
use crate::logistics::Logistics;
use crate::polity::Polities;
use crate::{SimSet, sim_schedule};
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

/// Belagerung in der Partie: der Kampf um Planeten zwischen Flotten im Orbit und der
/// Verteidigung am Boden.
pub struct SiegePlugin;

impl Plugin for SiegePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            sim_schedule(),
            // Nach den Gefechten des Takts: Wer dort eine Flotte verliert, belagert im
            // selben Takt nicht mehr. Der Tagesschritt folgt am Tagesbeginn.
            (siege_watch, siege_day.run_if(day_start))
                .chain()
                .in_set(SimSet::Siege),
        );
    }
}

/// Jeder Takt: Zuordnungen und Truppen an Bord folgen den Flotten, Belagerungen ohne
/// Grund enden. Ohne Belagerungen und ohne Truppen an Bord geschieht nichts.
fn siege_watch(
    data: Res<GameData>,
    clock: Res<SimClock>,
    galaxy: Res<Galaxy>,
    polities: Res<Polities>,
    diplomacy: Res<Diplomacy>,
    mut finance: ResMut<Finance>,
    mut sieges: ResMut<Sieges>,
    mut access: OrbitAccess,
    mut chronicle: ResMut<Chronicle>,
) {
    if sieges.active.is_empty() && sieges.assign.is_empty() && sieges.embarked.is_empty() {
        return;
    }
    tidy(
        &data,
        &galaxy,
        &polities,
        &diplomacy,
        &mut finance,
        &mut sieges,
        &access.orbit(),
        &mut chronicle,
        clock.tick,
    );
}

/// Tagesbeginn: Lebenslauf der Belagerungen, Tagesrechner je belagertem Planeten und
/// seine Wirkung auf Schiffe, Wirtschaft und Finanzen.
fn siege_day(
    data: Res<GameData>,
    clock: Res<SimClock>,
    polities: Res<Polities>,
    diplomacy: Res<Diplomacy>,
    logistics: Res<Logistics>,
    mut galaxy: ResMut<Galaxy>,
    mut economy: ResMut<Economy>,
    mut finance: ResMut<Finance>,
    mut sieges: ResMut<Sieges>,
    mut access: OrbitAccess,
    mut chronicle: ResMut<Chronicle>,
) {
    run_day(
        &data,
        &mut galaxy,
        &polities,
        &diplomacy,
        &mut economy,
        &mut finance,
        &logistics,
        &mut sieges,
        &mut access.orbit(),
        &mut chronicle,
        clock.tick,
    );
}

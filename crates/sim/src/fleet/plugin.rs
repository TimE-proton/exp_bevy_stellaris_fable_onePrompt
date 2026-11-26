use super::{
    Battles, Fleets, advance_battles, advance_yards, move_fleets, muster_battles, open_battles,
    pay_upkeep, repair_fleets, resupply_fleets, update_blockades,
};
use crate::chronicle::Chronicle;
use crate::clock::{SimClock, day_start, month_start};
use crate::data::GameData;
use crate::diplomacy::Diplomacy;
use crate::economy::Economy;
use crate::finance::Finance;
use crate::galaxy::Galaxy;
use crate::logistics::Logistics;
use crate::polity::Polities;
use crate::state::SimConfig;
use crate::tech::Research;
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
                // Zuerst der Stand der laufenden Gefechte: Was sie gekostet haben, steht in
                // den Flotten, bevor marschiert, bezahlt und eingekauft wird. Dann der
                // Marsch, dann der Sold (er geht dem Einkauf vor), dann Nachschub,
                // Instandsetzung und Werften aus derselben Staatskasse. Die Forschung folgt
                // erst danach (`SimSet::Research`) und bekommt, was über der Rücklage bleibt.
                (
                    muster.run_if(day_start),
                    movement.run_if(day_start),
                    upkeep.run_if(month_start),
                    service.run_if(day_start),
                )
                    .chain()
                    .in_set(SimSet::Fleets),
                // Gefechte rechnen jeden Takt, nicht nur am Tagesanfang.
                combat.in_set(SimSet::SpaceCombat),
            ),
        );
    }
}

fn muster(
    data: Res<GameData>,
    clock: Res<SimClock>,
    galaxy: Res<Galaxy>,
    diplomacy: Res<Diplomacy>,
    mut fleets: ResMut<Fleets>,
    mut battles: ResMut<Battles>,
    mut chronicle: ResMut<Chronicle>,
) {
    muster_battles(
        &galaxy,
        &mut fleets,
        &mut battles,
        &diplomacy,
        &mut chronicle,
        &data,
        clock.tick,
    );
}

fn movement(
    data: Res<GameData>,
    galaxy: Res<Galaxy>,
    research: Res<Research>,
    mut fleets: ResMut<Fleets>,
) {
    move_fleets(&galaxy, &mut fleets, &research, &data);
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
    diplomacy: Res<Diplomacy>,
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
        &diplomacy,
        &mut chronicle,
        &data,
        clock.tick,
    );
}

/// Gefechte und Blockaden. Am Tagesanfang beginnen Gefechte und greifen Verstärkungen
/// ein; jeden Takt rücken die laufenden Gefechte um einen Schritt vor.
fn combat(
    data: Res<GameData>,
    config: Res<SimConfig>,
    clock: Res<SimClock>,
    galaxy: Res<Galaxy>,
    polities: Res<Polities>,
    diplomacy: Res<Diplomacy>,
    logistics: Res<Logistics>,
    mut fleets: ResMut<Fleets>,
    mut battles: ResMut<Battles>,
    mut finance: ResMut<Finance>,
    mut research: ResMut<Research>,
    mut chronicle: ResMut<Chronicle>,
) {
    let day_start = clock.is_day_start();
    if !day_start && battles.active.is_empty() {
        return;
    }
    if day_start {
        // Tägliches Netz: Die Stärke folgt den Schiffen, eine Flotte ohne Schiffe besteht
        // nicht mehr, auch wenn sich ihre Schiffe außerhalb eines Gefechts geändert haben.
        fleets.refresh(&data);
        open_battles(
            &galaxy,
            &mut fleets,
            &mut battles,
            &diplomacy,
            &finance,
            &research,
            &mut chronicle,
            &data,
            config.seed,
            clock.tick,
        );
    }
    advance_battles(
        &galaxy,
        &mut fleets,
        &mut battles,
        &diplomacy,
        &mut finance,
        &mut research,
        &logistics,
        &polities,
        &mut chronicle,
        &data,
        clock.tick,
    );
    if day_start {
        update_blockades(&galaxy, &mut fleets, &diplomacy, &battles);
    }
}

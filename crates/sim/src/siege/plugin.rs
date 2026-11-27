use super::{OrbitAccess, Sieges, reserve_besiegers, resolve_conquests, run_day, run_month, tidy};
use crate::ai::AiState;
use crate::chronicle::Chronicle;
use crate::clock::{SimClock, day_start, month_start};
use crate::data::GameData;
use crate::diplomacy::Diplomacy;
use crate::economy::Economy;
use crate::finance::Finance;
use crate::fleet::Battles;
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
            (
                // Nach den Gefechten des Takts: Wer dort eine Flotte verliert, belagert im
                // selben Takt nicht mehr. Der Tagesschritt folgt am Tagesbeginn.
                (siege_watch, siege_day.run_if(day_start))
                    .chain()
                    .in_set(SimSet::Siege),
                // Urteile des Tages werden noch am selben Tag zum Besitzwechsel; danach, am
                // Monatsbeginn, die Bücher des Monats.
                (conquests.run_if(day_start), siege_month.run_if(month_start))
                    .chain()
                    .in_set(SimSet::Aftermath),
            ),
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
    mut ai: ResMut<AiState>,
) {
    if sieges.active.is_empty()
        && sieges.assign.is_empty()
        && sieges.embarked.is_empty()
        && sieges.held.is_empty()
    {
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
    // Wer nicht mehr belagert, steht der Flotten-KI wieder zur Verfügung.
    reserve_besiegers(&mut sieges, &mut ai);
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

/// Tagesabschluss: Fällige Besitzwechsel werden vollzogen.
fn conquests(
    data: Res<GameData>,
    clock: Res<SimClock>,
    polities: Res<Polities>,
    diplomacy: Res<Diplomacy>,
    battles: Res<Battles>,
    logistics: Res<Logistics>,
    mut galaxy: ResMut<Galaxy>,
    mut economy: ResMut<Economy>,
    mut finance: ResMut<Finance>,
    mut sieges: ResMut<Sieges>,
    mut access: OrbitAccess,
    mut chronicle: ResMut<Chronicle>,
) {
    if sieges.pending.is_empty() {
        return;
    }
    resolve_conquests(
        &data,
        &mut galaxy,
        &polities,
        &diplomacy,
        &battles,
        &mut economy,
        &mut finance,
        &logistics,
        &mut sieges,
        &mut access.orbit(),
        &mut chronicle,
        clock.tick,
    );
}

/// Monatsbeginn: Miliz im Frieden, Abklingen von Schäden und Besatzung, Spiegelwerte der
/// Regionen, Anteile an den Betrieben belagerter Welten.
fn siege_month(
    data: Res<GameData>,
    galaxy: Res<Galaxy>,
    polities: Res<Polities>,
    logistics: Res<Logistics>,
    mut finance: ResMut<Finance>,
    mut sieges: ResMut<Sieges>,
) {
    run_month(
        &data,
        &galaxy,
        &polities,
        &mut finance,
        &logistics,
        &mut sieges,
    );
}

use super::{Research, research_month};
use crate::chronicle::Chronicle;
use crate::clock::{SimClock, month_start};
use crate::data::GameData;
use crate::diplomacy::Diplomacy;
use crate::economy::Economy;
use crate::finance::Finance;
use crate::galaxy::Galaxy;
use crate::logistics::Logistics;
use crate::polity::Polities;
use crate::state::SimConfig;
use crate::{SimSet, sim_schedule};
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

/// Forschung: einmal im Monat, nach Monatsabschluss, Zusammenhalt und den Flotten. Die
/// Einnahmen des Monats stehen dann fest, eine eben abgespaltene Gemeinschaft forscht
/// schon mit, und der Staatshaushalt hat der Forschung bewilligt, was die Ansprüche vor
/// ihr übrig ließen (`economy::settle_budget`). Etat und Ziel der KI-Gemeinschaften
/// setzt deren Führung zu Monatsbeginn (`ai::military::plan_month`).
pub struct TechPlugin;

impl Plugin for TechPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            sim_schedule(),
            research.in_set(SimSet::Research).run_if(month_start),
        );
    }
}

fn research(
    data: Res<GameData>,
    config: Res<SimConfig>,
    clock: Res<SimClock>,
    galaxy: Res<Galaxy>,
    polities: Res<Polities>,
    finance: Res<Finance>,
    diplomacy: Res<Diplomacy>,
    mut logistics: ResMut<Logistics>,
    mut economy: ResMut<Economy>,
    mut research: ResMut<Research>,
    mut chronicle: ResMut<Chronicle>,
) {
    research_month(
        &mut research,
        &mut economy,
        &galaxy,
        &polities,
        &finance,
        &logistics,
        &diplomacy,
        &mut chronicle,
        &data,
        &config,
        clock.tick,
    );
    // Wirtschaft und Logistik rechnen ab dem nächsten Tag mit der neuen Technik.
    research.publish_effects(&mut economy, &mut logistics);
}

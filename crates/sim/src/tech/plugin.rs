use super::{Research, ai_budget_share, research_month};
use crate::chronicle::Chronicle;
use crate::clock::{SimClock, month_start};
use crate::command::{CommandQueue, CommandSource, SimCommand};
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

/// Forschung: einmal im Monat, nach Monatsabschluss und Zusammenhalt. Die Einnahmen des
/// Monats stehen dann fest, und eine eben abgespaltene Gemeinschaft forscht schon mit.
/// Danach legt die KI ihren Etat für den nächsten Monat fest.
pub struct TechPlugin;

impl Plugin for TechPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            sim_schedule(),
            (research, research_policy)
                .chain()
                .in_set(SimSet::Research)
                .run_if(month_start),
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

/// Forschungspolitik der KI-Gemeinschaften: Jede setzt den Etat ihrer Vorlage an, im
/// Krieg einen höheren. Wie der Spieler wirkt sie über einen Befehl; er gilt ab dem
/// nächsten Forschungsschritt.
fn research_policy(
    data: Res<GameData>,
    config: Res<SimConfig>,
    polities: Res<Polities>,
    diplomacy: Res<Diplomacy>,
    research: Res<Research>,
    mut queue: ResMut<CommandQueue>,
) {
    for polity in polities.alive() {
        if config.player == Some(polity.id) {
            continue;
        }
        let Some(entry) = research.get(polity.id) else {
            continue;
        };
        let wanted = ai_budget_share(
            &data.balance.research,
            entry.usual_share,
            diplomacy.is_at_war(polity.id),
        );
        if (wanted - entry.budget_share).abs() > 1e-9 {
            queue.push(
                polity.id,
                CommandSource::Ai,
                SimCommand::SetResearchBudget { share: wanted },
            );
        }
    }
}

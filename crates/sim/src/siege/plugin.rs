use super::{
    OrbitAccess, Sieges, open_arms_month, procure_day, reserve_besiegers, resolve_conquests,
    run_day, run_month, tidy,
};
use crate::ai::AiState;
use crate::chronicle::Chronicle;
use crate::clock::{DAYS_PER_MONTH, SimClock, day_start, month_start};
use crate::data::GameData;
use crate::diplomacy::Diplomacy;
use crate::economy::Economy;
use crate::finance::Finance;
use crate::fleet::{Battles, Fleets};
use crate::galaxy::Galaxy;
use crate::logistics::Logistics;
use crate::polity::{Polities, SuccessionParams};
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
                // selben Takt nicht mehr. Der Tagesschritt folgt am Tagesbeginn. Am ersten
                // Tag eines Monats kaufen die Depots erst nach dem Sold (`siege_month`).
                (
                    siege_watch,
                    siege_day.run_if(day_start),
                    siege_procure.run_if(later_day_start),
                )
                    .chain()
                    .in_set(SimSet::Siege),
                // Urteile des Tages werden noch am selben Tag zum Besitzwechsel; danach, am
                // Monatsbeginn, die Bücher des Monats. Zuletzt gibt die Belagerung der
                // Flotten-KI zurück, was der Tag aus den Zuordnungen genommen hat.
                (
                    conquests.run_if(day_start),
                    siege_month.run_if(month_start),
                    siege_release.run_if(day_start),
                )
                    .chain()
                    .in_set(SimSet::Aftermath),
            ),
        );
    }
}

/// Laufbedingung: erster Takt eines Tages, der nicht der erste eines Monats ist.
fn later_day_start(clock: Res<SimClock>) -> bool {
    clock.is_day_start() && !clock.day().is_multiple_of(DAYS_PER_MONTH)
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
    mut battles: ResMut<Battles>,
    mut chronicle: ResMut<Chronicle>,
) {
    let mut orbit = access.orbit();
    run_day(
        &data,
        &mut galaxy,
        &polities,
        &diplomacy,
        &mut economy,
        &mut finance,
        &logistics,
        &mut sieges,
        &mut orbit,
        &mut chronicle,
        clock.tick,
    );
    // Was die Verteidigung der Welten zerstört hat, zählt bei den Verlusten der Flotten.
    for (owner, (ships, fleets)) in orbit.take_wrecks() {
        battles.count_wrecked(owner, ships, fleets);
    }
}

/// Tagesabschluss: Fällige Besitzwechsel werden über den gemeinsamen Pfad vollzogen.
fn conquests(clock: Res<SimClock>, mut sieges: ResMut<Sieges>, mut params: SuccessionParams) {
    if sieges.pending.is_empty() {
        return;
    }
    resolve_conquests(&mut params.borrow(), &mut sieges, clock.tick);
}

/// Monatsbeginn: Sold, Unterhalt, Aushebung und Ausbau aus der Bewilligung der
/// Verteidigung, danach der Depotkauf des Tages aus dem, was davon bleibt; dann Miliz im
/// Frieden, Abklingen von Schäden und Besatzung, Spiegelwerte der Regionen, Anteile an
/// den Betrieben belagerter Welten.
fn siege_month(
    data: Res<GameData>,
    polities: Res<Polities>,
    logistics: Res<Logistics>,
    fleets: Res<Fleets>,
    mut galaxy: ResMut<Galaxy>,
    mut economy: ResMut<Economy>,
    mut finance: ResMut<Finance>,
    mut sieges: ResMut<Sieges>,
) {
    open_arms_month(
        &data,
        &mut galaxy,
        &polities,
        &mut economy,
        &mut sieges,
        &|fleet| fleets.get(fleet).map(|fleet| fleet.owner),
        &|system| fleets.is_blockaded(system),
    );
    run_month(
        &data,
        &galaxy,
        &polities,
        &mut finance,
        &logistics,
        &mut sieges,
    );
}

/// Tagesbeginn außer am ersten Tag eines Monats, nach dem Tagesschritt der Belagerungen:
/// Die Welten füllen ihre Depots aus der Bewilligung der Verteidigung.
fn siege_procure(
    data: Res<GameData>,
    galaxy: Res<Galaxy>,
    fleets: Res<Fleets>,
    mut economy: ResMut<Economy>,
    mut sieges: ResMut<Sieges>,
) {
    procure_day(&data, &galaxy, &mut economy, &mut sieges, &|system| {
        fleets.is_blockaded(system)
    });
}

/// Ende des Tagesbeginns: Zuordnungen, die der Tagesschritt oder ein Besitzwechsel
/// beendet hat (aufgehobene Belagerung, zerstörte Flotte, gefallene Welt), gibt die
/// Belagerung noch im selben Takt bei der Flotten-KI frei.
fn siege_release(mut sieges: ResMut<Sieges>, mut ai: ResMut<AiState>) {
    reserve_besiegers(&mut sieges, &mut ai);
}

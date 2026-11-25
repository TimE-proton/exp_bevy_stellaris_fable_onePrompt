//! Selbsttest: startet eine Partie, lässt sie einige Frames laufen, speichert auf Wunsch
//! ein Bildschirmfoto und beendet sich. Damit lässt sich prüfen, dass Fenster, Karte,
//! Oberfläche und Simulation gemeinsam hochkommen, auch ohne Menschen davor.

use crate::args::Args;
use crate::map::{FocusSystem, Selection};
use crate::session::SessionRequest;
use crate::state::AppState;
use crate::time_control::SimSpeed;
use crate::ui::{LeftTab, UiState};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use sim::clock::SimClock;
use sim::galaxy::Galaxy;

#[derive(Resource)]
struct Selftest {
    total: u32,
    frame: u32,
    shot_taken: bool,
}

pub struct SelftestPlugin;

impl Plugin for SelftestPlugin {
    fn build(&self, app: &mut App) {
        let Some(frames) = app.world().resource::<Args>().selftest else {
            return;
        };
        app.insert_resource(Selftest {
            total: frames.max(30),
            frame: 0,
            shot_taken: false,
        })
        .add_systems(OnEnter(AppState::MainMenu), start)
        .add_systems(Update, drive.run_if(in_state(AppState::InGame)));
    }
}

fn start(args: Res<Args>, test: Res<Selftest>, mut requests: MessageWriter<SessionRequest>) {
    if test.frame == 0 && args.seed.is_none() && args.load.is_none() {
        requests.write(SessionRequest::NewGame { seed: 1 });
    }
}

fn drive(
    mut commands: Commands,
    args: Res<Args>,
    mut test: ResMut<Selftest>,
    mut speed: ResMut<SimSpeed>,
    selection: Res<Selection>,
    mut ui_state: ResMut<UiState>,
    clock: Res<SimClock>,
    galaxy: Res<Galaxy>,
    mut focus: MessageWriter<FocusSystem>,
    mut exit: MessageWriter<AppExit>,
) {
    test.frame += 1;
    let total = test.total;
    if test.frame == 5 {
        // Zeit laufen lassen, schnellste Stufe.
        speed.paused = false;
        speed.level = crate::time_control::SPEEDS.len() - 1;
    }
    if test.frame == total / 2
        && let Some(system) = selection.system
    {
        // Zur Halbzeit heranzoomen und die datenlastigen Tafeln öffnen.
        focus.write(FocusSystem(system));
        ui_state.tab = LeftTab::Market;
        ui_state.flows_open = true;
    }
    if test.frame == total.saturating_sub(12)
        && !test.shot_taken
        && let Some(path) = args.screenshot.clone()
    {
        test.shot_taken = true;
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
    }
    if test.frame >= total {
        let owned = galaxy
            .planets
            .iter()
            .filter(|planet| planet.owner.is_some())
            .count();
        println!(
            "SELBSTTEST OK: {} Frames, Datum {}, Takt {}, {} von {} Planeten besiedelt",
            test.frame,
            clock.date(),
            clock.tick,
            owned,
            galaxy.planets.len()
        );
        exit.write(AppExit::Success);
    }
}

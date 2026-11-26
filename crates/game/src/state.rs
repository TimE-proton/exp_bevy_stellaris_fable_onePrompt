//! Spielphasen als Bevy-States.

use bevy::prelude::*;
use sim::SimRoot;

/// Phasen der App.
#[derive(States, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum AppState {
    /// Gemeinsame Assets anlegen.
    #[default]
    Loading,
    MainMenu,
    InGame,
    /// Gefechtsübung: ein einzelnes Gefecht außerhalb einer Partie.
    Practice,
    /// Die Partie ist entschieden (ab Meilenstein 6 erreichbar).
    #[allow(dead_code)]
    GameOver,
}

/// Was im Spiel gerade gezeigt wird.
#[derive(SubStates, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[source(AppState = AppState::InGame)]
pub enum ViewState {
    #[default]
    Galaxy,
    /// 3D-Schlachtansicht eines Gefechts der Partie.
    Battle,
    /// Belagerungsansicht (ab Meilenstein 5).
    #[allow(dead_code)]
    Siege,
}

pub struct StatePlugin;

impl Plugin for StatePlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<AppState>()
            .add_sub_state::<ViewState>()
            // Die Simulation rechnet nur, solange eine Partie läuft.
            .configure_sets(FixedUpdate, SimRoot.run_if(in_state(AppState::InGame)))
            .add_systems(OnEnter(AppState::Loading), finish_loading);
    }
}

fn finish_loading(mut next: ResMut<NextState<AppState>>) {
    next.set(AppState::MainMenu);
}

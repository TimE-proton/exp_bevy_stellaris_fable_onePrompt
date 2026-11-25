//! Oberfläche (egui). Die Fenster sind reine Funktionen über dem Simulationszustand:
//! Sie lesen, zeichnen und geben Aktionen zurück. Erst das System unten wendet sie an.

mod format;
mod panels;

pub use panels::{SimRefs, UiAction};

use crate::map::{FocusSystem, Selection};
use crate::session::{Notice, SessionRequest, quicksave_path};
use crate::state::AppState;
use crate::time_control::SimSpeed;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy_egui::input::egui_wants_any_keyboard_input;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};
use sim::chronicle::Chronicle;
use sim::clock::SimClock;
use sim::command::{CommandQueue, CommandSource};
use sim::galaxy::Galaxy;
use sim::polity::Polities;
use sim::state::SimConfig;

/// Zustand der Oberfläche selbst (kein Spielzustand).
#[derive(Resource, Debug, Clone)]
pub struct UiState {
    pub seed_text: String,
    pub menu_open: bool,
    pub help_open: bool,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            seed_text: "1".into(),
            menu_open: false,
            help_open: false,
        }
    }
}

/// Lesender Zugriff auf die laufende Partie; fehlt im Hauptmenü.
#[derive(SystemParam)]
pub struct SimParams<'w> {
    galaxy: Res<'w, Galaxy>,
    polities: Res<'w, Polities>,
    clock: Res<'w, SimClock>,
    config: Res<'w, SimConfig>,
    chronicle: Res<'w, Chronicle>,
}

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiState>()
            .add_systems(
                Update,
                hotkeys
                    .run_if(in_state(AppState::InGame))
                    .run_if(not(egui_wants_any_keyboard_input)),
            )
            .add_systems(EguiPrimaryContextPass, draw_ui);
    }
}

fn hotkeys(
    keys: Res<ButtonInput<KeyCode>>,
    mut ui_state: ResMut<UiState>,
    mut session: MessageWriter<SessionRequest>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        ui_state.menu_open = !ui_state.menu_open;
    }
    if keys.just_pressed(KeyCode::F1) {
        ui_state.help_open = !ui_state.help_open;
    }
    if keys.just_pressed(KeyCode::F5) {
        session.write(SessionRequest::Save(quicksave_path()));
    }
    if keys.just_pressed(KeyCode::F9) {
        session.write(SessionRequest::Load(quicksave_path()));
    }
}

fn draw_ui(
    mut contexts: EguiContexts,
    app_state: Res<State<AppState>>,
    data: Res<sim::GameData>,
    sim: Option<SimParams>,
    notice: Res<Notice>,
    mut ui_state: ResMut<UiState>,
    mut speed: ResMut<SimSpeed>,
    mut selection: ResMut<Selection>,
    mut queue: Option<ResMut<CommandQueue>>,
    mut session: MessageWriter<SessionRequest>,
    mut focus: MessageWriter<FocusSystem>,
    mut exit: MessageWriter<AppExit>,
    mut styled: Local<bool>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    if !*styled {
        // Der egui-Kontext entsteht erst mit seiner Kamera, daher hier und nicht beim Start.
        panels::apply_style(ctx);
        *styled = true;
    }
    let mut root = egui::Ui::new(
        ctx.clone(),
        "wurzel".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );
    let mut actions = Vec::new();
    match (app_state.get(), &sim) {
        (AppState::InGame | AppState::GameOver, Some(sim)) => {
            let refs = SimRefs {
                data: &data,
                galaxy: &sim.galaxy,
                polities: &sim.polities,
                clock: &sim.clock,
                config: &sim.config,
                chronicle: &sim.chronicle,
            };
            panels::in_game(
                &mut root,
                &refs,
                &mut ui_state,
                &speed,
                &selection,
                &mut actions,
            );
        }
        (AppState::MainMenu, _) => {
            panels::main_menu(
                &mut root,
                &mut ui_state,
                quicksave_path().is_file(),
                &mut actions,
            );
        }
        _ => {}
    }
    panels::notice(&mut root, &notice);

    for action in actions {
        match action {
            UiAction::Session(request) => {
                ui_state.menu_open = false;
                session.write(request);
            }
            UiAction::TogglePause => speed.paused = !speed.paused,
            UiAction::SetSpeed(level) => {
                speed.level = level;
                speed.paused = false;
            }
            UiAction::Select(new) => *selection = new,
            UiAction::Focus(system) => {
                focus.write(FocusSystem(system));
            }
            UiAction::Command(command) => {
                if let (Some(queue), Some(sim)) = (queue.as_mut(), sim.as_ref())
                    && let Some(player) = sim.config.player
                {
                    queue.push(player, CommandSource::Player, command);
                }
            }
            UiAction::Quit => {
                exit.write(AppExit::Success);
            }
        }
    }
    Ok(())
}

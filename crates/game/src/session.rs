//! Partie starten, speichern, laden, beenden.

use crate::args::Args;
use crate::state::AppState;
use crate::time_control::SimSpeed;
use bevy::ecs::message::Messages;
use bevy::prelude::*;
use sim::ids::PolityId;
use sim::state::{SaveGame, SimConfig, SimState};
use std::path::PathBuf;

/// Standardpfad für Schnellspeichern und -laden.
pub fn quicksave_path() -> PathBuf {
    PathBuf::from("saves/schnell.ron")
}

#[derive(Message, Clone, Debug)]
pub enum SessionRequest {
    NewGame { seed: u64 },
    Save(PathBuf),
    Load(PathBuf),
    QuitToMenu,
}

/// Zählt hoch, sobald eine andere Partie in der Welt liegt. Ansichten bauen sich dann neu.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionEpoch(pub u32);

/// Kurze Rückmeldung für den Spieler (gespeichert, Fehler, …).
#[derive(Resource, Default, Debug, Clone)]
pub struct Notice {
    pub text: String,
    pub is_error: bool,
    /// Verbleibende Anzeigezeit in Sekunden.
    pub seconds_left: f32,
}

impl Notice {
    pub fn info(&mut self, text: impl Into<String>) {
        *self = Self {
            text: text.into(),
            is_error: false,
            seconds_left: 4.0,
        };
    }

    pub fn error(&mut self, text: impl Into<String>) {
        *self = Self {
            text: text.into(),
            is_error: true,
            seconds_left: 8.0,
        };
    }
}

pub struct SessionPlugin;

impl Plugin for SessionPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SessionRequest>()
            .init_resource::<SessionEpoch>()
            .init_resource::<Notice>()
            .add_systems(OnEnter(AppState::MainMenu), start_from_args)
            .add_systems(Update, (handle_requests, fade_notice));
    }
}

/// `--seed` und `--load` überspringen das Hauptmenü, aber nur beim Programmstart.
fn start_from_args(mut args: ResMut<Args>, mut requests: MessageWriter<SessionRequest>) {
    if args.practice {
        // Mit `--practice` gehört der Seed der Gefechtsübung, nicht einer Partie.
        return;
    }
    if let Some(path) = args.load.take() {
        requests.write(SessionRequest::Load(path));
    } else if let Some(seed) = args.seed.take() {
        requests.write(SessionRequest::NewGame { seed });
    }
}

fn fade_notice(time: Res<Time<Real>>, mut notice: ResMut<Notice>) {
    if notice.seconds_left > 0.0 {
        notice.seconds_left -= time.delta_secs();
    }
}

fn handle_requests(world: &mut World) {
    let requests: Vec<SessionRequest> = world
        .resource_mut::<Messages<SessionRequest>>()
        .drain()
        .collect();
    for request in requests {
        match request {
            SessionRequest::NewGame { seed } => {
                // Der Mensch führt immer die erste Gemeinschaft; welche Vorlage und welche
                // Lage sie bekommt, entscheidet der Seed.
                sim::new_game(
                    world,
                    SimConfig {
                        seed,
                        player: Some(PolityId(0)),
                    },
                );
                enter_game(world);
                world
                    .resource_mut::<Notice>()
                    .info(format!("Neue Partie, Seed {seed}"));
            }
            SessionRequest::Save(path) => {
                let result = SaveGame::capture(world).and_then(|save| save.write(&path));
                let mut notice = world.resource_mut::<Notice>();
                match result {
                    Ok(()) => notice.info(format!("Gespeichert: {}", path.display())),
                    Err(error) => notice.error(error.to_string()),
                }
            }
            SessionRequest::Load(path) => match SaveGame::read(&path) {
                Ok(save) => {
                    let same_data = save.matches_data(world.resource::<sim::GameData>());
                    save.state.install(world);
                    enter_game(world);
                    let mut notice = world.resource_mut::<Notice>();
                    if same_data {
                        notice.info(format!("Geladen: {}", path.display()));
                    } else {
                        notice.error(
                            "Geladen, aber die Spieldaten haben sich seit dem Speichern geändert. \
                             Der weitere Verlauf weicht vom Original ab.",
                        );
                    }
                }
                Err(error) => world.resource_mut::<Notice>().error(error.to_string()),
            },
            SessionRequest::QuitToMenu => {
                // Erst die Zeit anhalten, dann die Partie entfernen: Ohne Partie darf kein
                // Takt mehr laufen, sonst lesen die Laufbedingungen des Kerns eine Uhr,
                // die es nicht mehr gibt.
                *world.resource_mut::<SimSpeed>() = SimSpeed::default();
                world.resource_mut::<Time<Virtual>>().pause();
                SimState::remove(world);
                world
                    .resource_mut::<NextState<AppState>>()
                    .set(AppState::MainMenu);
            }
        }
    }
}

fn enter_game(world: &mut World) {
    world.resource_mut::<SessionEpoch>().0 += 1;
    // Jede Partie beginnt pausiert: erst umsehen, dann die Zeit laufen lassen.
    *world.resource_mut::<SimSpeed>() = SimSpeed::default();
    world
        .resource_mut::<NextState<AppState>>()
        .set(AppState::InGame);
}

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
            .add_systems(
                OnEnter(AppState::MainMenu),
                (remove_session, start_from_args).chain(),
            )
            .add_systems(Update, (handle_requests, fade_notice));
    }
}

/// Räumt die Partie ab, sobald das Hauptmenü erreicht ist.
///
/// Erst hier und nicht schon mit der Bitte „Zum Hauptmenü“: Bis zum Zustandswechsel im
/// nächsten Frame laufen noch Systeme der Partie (Karte, Verkehr, Preisverläufe), die die
/// Simulation brauchen. Auch nicht beim Verlassen der Partie: Laden mitten in der Partie
/// wechselt von „Partie“ nach „Partie“ und würde den frisch geladenen Stand mitnehmen.
fn remove_session(world: &mut World) {
    SimState::remove(world);
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
                // Die Zeit hält sofort an: Sobald die Partie im Hauptmenü abgeräumt ist,
                // darf kein Takt mehr laufen, sonst lesen die Laufbedingungen des Kerns
                // eine Uhr, die es nicht mehr gibt. Die Partie selbst bleibt bis zum
                // Zustandswechsel liegen (siehe `remove_session`).
                *world.resource_mut::<SimSpeed>() = SimSpeed::default();
                world.resource_mut::<Time<Virtual>>().pause();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle_view::BattleViewRequest;
    use crate::battle_view::practice::{PracticeBattle, PracticeSetup, StartPractice};
    use crate::testing::TestGame;
    use sim::clock::SimClock;
    use sim::galaxy::Galaxy;

    fn start_campaign(game: &mut TestGame, seed: u64) {
        game.send(SessionRequest::NewGame { seed });
        game.frames(3);
        assert_eq!(game.state(), AppState::InGame);
        assert!(game.world().contains_resource::<Galaxy>());
    }

    fn quit_to_menu(game: &mut TestGame) {
        game.send(SessionRequest::QuitToMenu);
        // Im Frame der Bitte gilt noch der Zustand der Partie; ihre Systeme laufen weiter
        // und brauchen die Simulation. Sie darf erst mit dem Zustandswechsel verschwinden.
        game.frames(1);
        assert_eq!(game.state(), AppState::InGame);
        assert!(
            game.world().contains_resource::<Galaxy>(),
            "die Simulation wurde entfernt, solange die Partie noch lief"
        );
        game.frames(3);
        assert_eq!(game.state(), AppState::MainMenu);
        assert!(!game.world().contains_resource::<Galaxy>());
        assert!(!game.world().contains_resource::<SimClock>());
    }

    fn start_practice(game: &mut TestGame) {
        let config = PracticeSetup::default()
            .resolve(game.resource::<sim::GameData>())
            .unwrap();
        game.send(StartPractice(config));
        game.frames(3);
        assert_eq!(game.state(), AppState::Practice);
        assert!(game.world().contains_resource::<PracticeBattle>());
    }

    fn leave_practice(game: &mut TestGame) {
        game.send(BattleViewRequest::Leave);
        game.frames(4);
        assert_eq!(game.state(), AppState::MainMenu);
        assert!(!game.world().contains_resource::<PracticeBattle>());
    }

    #[test]
    fn quitting_a_campaign_reaches_the_main_menu() {
        let mut game = TestGame::start("");
        start_campaign(&mut game, 3);
        game.frames(5);
        quit_to_menu(&mut game);
        // Auch mit laufender Zeit, auf der schnellsten Stufe.
        start_campaign(&mut game, 4);
        *game.world_mut().resource_mut::<SimSpeed>() = SimSpeed {
            paused: false,
            level: 5,
        };
        game.frames(10);
        assert!(game.resource::<SimClock>().tick > 0, "die Zeit läuft");
        quit_to_menu(&mut game);
        // Im Hauptmenü steht die Zeit; es läuft kein Takt mehr.
        assert!(game.resource::<Time<Virtual>>().is_paused());
        game.frames(10);
    }

    #[test]
    fn campaign_menu_practice_menu_campaign_menu_works_repeatedly() {
        let mut game = TestGame::start("");
        for round in 0..3 {
            start_campaign(&mut game, 10 + round);
            game.frames(4);
            quit_to_menu(&mut game);
            start_practice(&mut game);
            game.frames(4);
            leave_practice(&mut game);
            start_campaign(&mut game, 20 + round);
            game.frames(4);
            quit_to_menu(&mut game);
        }
    }

    #[test]
    fn loading_inside_a_running_campaign_keeps_the_new_state() {
        // Laden mitten in der Partie wechselt von „Partie“ nach „Partie“. Die geladene
        // Simulation darf dabei nicht mit abgeräumt werden.
        let mut game = TestGame::start("");
        start_campaign(&mut game, 3);
        game.frames(3);
        let path =
            std::env::temp_dir().join(format!("konfluenz_test_{}_laden.ron", std::process::id()));
        game.send(SessionRequest::Save(path.clone()));
        game.frames(2);
        game.send(SessionRequest::Load(path.clone()));
        game.frames(4);
        let _ = std::fs::remove_file(&path);
        assert_eq!(game.state(), AppState::InGame);
        assert!(game.world().contains_resource::<Galaxy>());
        game.frames(4);
    }
}

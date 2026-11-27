//! Partie starten, speichern, laden, beenden.

use crate::args::Args;
use crate::state::AppState;
use crate::time_control::SimSpeed;
use bevy::ecs::message::Messages;
use bevy::prelude::*;
use sim::command::{CommandLog, CommandOutcome, CommandSource};
use sim::ids::PolityId;
use sim::state::{SAVE_FORMAT, SaveError, SaveGame, SimConfig, SimState};
use std::path::{Path, PathBuf};

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

/// Eine Partie, die bereitliegt und mit dem nächsten Zustandswechsel eingesetzt wird.
///
/// Neue Partie und Laden tauschen die Simulation nie mitten in einem Frame aus: Dessen
/// übrige Systeme (Karte, Verkehr, Tafeln) halten noch Kennungen der Partie, die auf dem
/// Bildschirm steht, und griffen damit in die neue. Die neue wartet deshalb hier, bis die
/// Karte der alten abgeräumt und die Auswahl aufgehoben ist.
#[derive(Resource)]
struct PendingSession {
    state: SimState,
    /// Was der Spieler erfährt, sobald die Partie steht.
    notice: Notice,
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
            .add_systems(OnEnter(AppState::InGame), install_session)
            .add_systems(
                Update,
                (
                    handle_requests,
                    fade_notice,
                    report_rejections.run_if(in_state(AppState::InGame)),
                ),
            );
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
    // Wurde im selben Frame noch geladen und dann doch das Hauptmenü verlangt, gilt das
    // Hauptmenü.
    world.remove_resource::<PendingSession>();
}

/// Setzt die bereitliegende Partie ein. Läuft im Zustandswechsel, nachdem die Karte der
/// bisherigen Partie abgeräumt und ihre Auswahl aufgehoben ist und bevor ein System der
/// Partie wieder läuft: Kein System sieht je die neue Simulation mit Kennungen der alten.
fn install_session(world: &mut World) {
    let Some(pending) = world.remove_resource::<PendingSession>() else {
        return;
    };
    pending.state.install(world);
    world.resource_mut::<SessionEpoch>().0 += 1;
    world.insert_resource(pending.notice);
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

/// Sagt dem Spieler, wenn der Kern einen seiner Befehle abgelehnt hat, und warum. Die
/// Oberfläche bietet nur an, was der Kern annimmt; zwischen Klick und nächstem Takt kann
/// sich die Lage aber ändern (eine Flotte gerät ins Gefecht, ein Ziel wird erforscht).
fn report_rejections(
    epoch: Res<SessionEpoch>,
    log: Option<Res<CommandLog>>,
    mut seen: Local<(Option<SessionEpoch>, usize)>,
    mut notice: ResMut<Notice>,
) {
    let Some(log) = log else {
        return;
    };
    if seen.0 != Some(*epoch) {
        // Eine andere Partie: Was ihr Protokoll schon enthält, ist Vergangenheit.
        *seen = (Some(*epoch), log.entries.len());
        return;
    }
    let fresh = log.entries.get(seen.1..).unwrap_or_default();
    seen.1 = log.entries.len();
    let rejected = fresh.iter().rev().find_map(|entry| match &entry.outcome {
        CommandOutcome::Rejected(reason) if entry.source == CommandSource::Player => Some(reason),
        _ => None,
    });
    if let Some(reason) = rejected {
        let end = if reason.ends_with(['.', '!', '?']) {
            ""
        } else {
            "."
        };
        let reason = crate::ui::format::german_numbers(reason);
        notice.error(format!("Befehl abgelehnt: {reason}{end}"));
    }
}

/// Speicherformat, das am Anfang eines Spielstands steht, ohne den Zustand zu lesen. Für
/// Stände, deren Zustand sich gar nicht mehr deuten lässt: So erfährt der Spieler trotzdem,
/// dass es am Alter des Stands liegt und nicht an einer beschädigten Datei.
fn declared_format(path: &Path) -> Option<u32> {
    let text = std::fs::read_to_string(path).ok()?;
    let number = text
        .trim_start()
        .strip_prefix('(')?
        .trim_start()
        .strip_prefix("format")?
        .trim_start()
        .strip_prefix(':')?
        .trim_start();
    let end = number
        .find(|sign: char| !sign.is_ascii_digit())
        .unwrap_or(number.len());
    number[..end].parse().ok()
}

/// Was der Spieler erfährt, wenn sich ein Spielstand nicht laden lässt. Ein Stand aus
/// einer anderen Fassung des Spiels wird am Speicherformat erkannt und abgelehnt; die
/// laufende Partie bleibt davon unberührt.
fn load_failure(path: &Path, error: &SaveError) -> String {
    let other_format = match error {
        SaveError::Format { found } => Some(*found),
        // Der Kern prüft das Format erst, nachdem er den ganzen Zustand gelesen hat. Einem
        // Stand, an dem er schon dabei scheitert, sieht man sein Format am Dateianfang an.
        SaveError::Decode(_) => declared_format(path).filter(|found| *found != SAVE_FORMAT),
        _ => None,
    };
    match other_format {
        Some(found) if found < SAVE_FORMAT => format!(
            "{} stammt aus einer älteren Fassung des Spiels (Speicherformat {found}) und lässt \
             sich nicht mehr laden. Diese Fassung liest Format {SAVE_FORMAT}.",
            path.display()
        ),
        Some(found) => format!(
            "{} stammt aus einer neueren Fassung des Spiels (Speicherformat {found}). Diese \
             Fassung liest Format {SAVE_FORMAT}.",
            path.display()
        ),
        None => error.to_string(),
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
                let config = SimConfig {
                    seed,
                    player: Some(PolityId(0)),
                };
                let state = SimState::generate(world.resource::<sim::GameData>(), config);
                let mut notice = Notice::default();
                notice.info(format!("Neue Partie, Seed {seed}"));
                enter_game(world, state, notice);
            }
            SessionRequest::Save(path) => {
                // Gespeichert wird die Partie, die auf dem Bildschirm steht, auch wenn im
                // selben Frame schon eine andere bereitliegt.
                let result = SaveGame::capture(world).and_then(|save| save.write(&path));
                let mut notice = world.resource_mut::<Notice>();
                match result {
                    Ok(()) => notice.info(format!("Gespeichert: {}", path.display())),
                    Err(error) => notice.error(error.to_string()),
                }
            }
            SessionRequest::Load(path) => match SaveGame::read(&path) {
                Ok(save) => {
                    let mut notice = Notice::default();
                    if save.matches_data(world.resource::<sim::GameData>()) {
                        notice.info(format!("Geladen: {}", path.display()));
                    } else {
                        notice.error(
                            "Geladen, aber die Spieldaten haben sich seit dem Speichern geändert. \
                             Der weitere Verlauf weicht vom Original ab.",
                        );
                    }
                    enter_game(world, save.state, notice);
                }
                // Die laufende Partie bleibt, wie sie ist.
                Err(error) => world
                    .resource_mut::<Notice>()
                    .error(load_failure(&path, &error)),
            },
            SessionRequest::QuitToMenu => {
                // Die Zeit hält sofort an: Sobald die Partie im Hauptmenü abgeräumt ist,
                // darf kein Takt mehr laufen, sonst lesen die Laufbedingungen des Kerns
                // eine Uhr, die es nicht mehr gibt. Die Partie selbst bleibt bis zum
                // Zustandswechsel liegen (siehe `remove_session`).
                stop_time(world);
                world
                    .resource_mut::<NextState<AppState>>()
                    .set(AppState::MainMenu);
            }
        }
    }
}

/// Hält die Zeit sofort an, nicht erst, wenn die Zeitsteuerung die Stufe das nächste Mal
/// überträgt: Bis dahin liefen noch Takte.
fn stop_time(world: &mut World) {
    *world.resource_mut::<SimSpeed>() = SimSpeed::default();
    world.resource_mut::<Time<Virtual>>().pause();
}

/// Legt eine Partie bereit und verlangt den Wechsel in sie. Eingesetzt wird sie erst mit
/// dem Zustandswechsel zu Beginn des nächsten Frames (`install_session`); bis dahin läuft,
/// was auf dem Bildschirm steht, unverändert zu Ende.
fn enter_game(world: &mut World, state: SimState, notice: Notice) {
    world.insert_resource(PendingSession { state, notice });
    // Jede Partie beginnt pausiert: erst umsehen, dann die Zeit laufen lassen. Die Zeit
    // steht ab sofort, damit die neue Partie nicht schon Takte rechnet, bevor der Spieler
    // sie sieht.
    stop_time(world);
    // Auch von „Partie“ nach „Partie“ ist das ein voller Wechsel: Die Karte der alten wird
    // abgeräumt, ihre Auswahl aufgehoben.
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

    /// Systeme und Planeten der Partie, die gerade in der Welt liegt.
    fn galaxy_size(game: &TestGame) -> (usize, usize) {
        let galaxy = game.resource::<Galaxy>();
        (galaxy.systems.len(), galaxy.planets.len())
    }

    /// Das Heimatsystem des Spielers in der Partie, die gerade in der Welt liegt.
    fn home_system(game: &TestGame) -> Option<sim::ids::SystemId> {
        let galaxy = game.resource::<Galaxy>();
        let polities = game.resource::<sim::polity::Polities>();
        let player = game.resource::<SimConfig>().player?;
        Some(galaxy.planet(polities.get(player).capital?).system)
    }

    fn run_fast(game: &mut TestGame, frames: usize) {
        *game.world_mut().resource_mut::<SimSpeed>() = SimSpeed {
            paused: false,
            level: 5,
        };
        game.frames(frames);
    }

    #[test]
    fn loading_another_galaxy_inside_a_running_campaign_works() {
        use crate::map::Selection;
        use crate::testing::{save_of_new_game, small_and_large_galaxy};
        use crate::ui::UiState;

        let mut game = TestGame::start("");
        let (small, large) = small_and_large_galaxy(game.resource::<sim::GameData>());
        let small_save = save_of_new_game(game.resource::<sim::GameData>(), small, "klein");
        let large_save = save_of_new_game(game.resource::<sim::GameData>(), large, "gross");

        // Die Partie mit der großen Galaxie läuft, mit allem, was dazugehört: Karte,
        // Auswahl, Verkehr, Preisverläufe, Tafeln.
        start_campaign(&mut game, large);
        let large_size = galaxy_size(&game);
        // An der Börse ist der letzte Emittent der großen Partie gewählt, und der Spieler
        // hat am Steuersatz gedreht.
        let last_issuer = *game
            .resource::<sim::finance::Finance>()
            .issuers
            .keys()
            .next_back()
            .expect("Emittenten");
        {
            let mut ui_state = game.world_mut().resource_mut::<UiState>();
            ui_state.exchange_open = true;
            ui_state.issuer = Some(last_issuer);
            ui_state.tax_percent = 33.0;
        }
        run_fast(&mut game, 6);
        assert!(game.resource::<SimClock>().tick > 0);

        // Mittendrin wird der Spielstand der kleinen geladen. Kennungen der laufenden
        // Partie (Systeme der Karte, die Auswahl) gibt es in der geladenen nicht alle.
        game.send(SessionRequest::Load(small_save.clone()));
        game.frames(1);
        // Im Frame der Bitte gilt noch die alte Partie, bis zu seinem Ende.
        assert_eq!(galaxy_size(&game), large_size);
        game.frames(5);
        assert_eq!(game.state(), AppState::InGame);
        let small_size = galaxy_size(&game);
        assert!(small_size.0 < large_size.0 && small_size.1 < large_size.1);
        assert_eq!(game.resource::<SimConfig>().seed, small);
        assert_eq!(game.resource::<SimClock>().tick, 0, "der Stand der Datei");
        // Gewählt ist das Heimatsystem der geladenen Partie.
        assert_eq!(game.resource::<Selection>().system, home_system(&game));
        assert!(home_system(&game).is_some());
        // Was die Oberfläche sich zur alten Partie gemerkt hatte, ist vergessen; die Fenster
        // bleiben offen, wie sie waren.
        let ui_state = game.resource::<UiState>();
        assert_eq!(ui_state.issuer, None);
        assert_eq!(ui_state.tax_percent, UiState::default().tax_percent);
        assert!(ui_state.exchange_open);
        // Und sie läuft weiter wie jede andere.
        run_fast(&mut game, 8);
        assert!(game.resource::<SimClock>().tick > 0);

        // Der umgekehrte Weg, ohne Pause dazwischen, und derselbe Stand gleich noch einmal.
        game.send(SessionRequest::Load(large_save.clone()));
        game.frames(6);
        assert_eq!(galaxy_size(&game), large_size);
        assert_eq!(game.resource::<Selection>().system, home_system(&game));
        run_fast(&mut game, 4);
        game.send(SessionRequest::Load(large_save.clone()));
        game.frames(6);
        assert_eq!(galaxy_size(&game), large_size);
        assert_eq!(game.resource::<SimClock>().tick, 0);
        run_fast(&mut game, 4);

        // Zwei Bitten im selben Frame: Die letzte gilt.
        game.send(SessionRequest::Load(large_save.clone()));
        game.send(SessionRequest::Load(small_save.clone()));
        game.frames(6);
        assert_eq!(galaxy_size(&game), small_size);
        run_fast(&mut game, 4);
        quit_to_menu(&mut game);

        // Aus dem Hauptmenü geladen, wie mit „Letzten Spielstand laden“.
        game.send(SessionRequest::Load(small_save.clone()));
        game.frames(4);
        assert_eq!(game.state(), AppState::InGame);
        assert_eq!(galaxy_size(&game), small_size);
        game.frames(4);

        for path in [small_save, large_save] {
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn a_save_that_cannot_be_read_leaves_the_running_campaign_alone() {
        let mut game = TestGame::start("");
        start_campaign(&mut game, 3);
        run_fast(&mut game, 6);
        let tick = game.resource::<SimClock>().tick;
        let seed = game.resource::<SimConfig>().seed;
        let missing = std::env::temp_dir().join(format!(
            "konfluenz_test_{}_gibt_es_nicht.ron",
            std::process::id()
        ));
        game.send(SessionRequest::Load(missing));
        game.frames(4);
        // Die Partie läuft weiter, als wäre nichts gewesen; der Spieler erfährt den Grund.
        assert_eq!(game.state(), AppState::InGame);
        assert_eq!(game.resource::<SimConfig>().seed, seed);
        assert!(game.resource::<SimClock>().tick > tick);
        assert!(!game.resource::<SimSpeed>().paused);
        let notice = game.resource::<Notice>();
        assert!(notice.is_error && notice.text.contains("Dateifehler"));
    }

    #[test]
    fn a_save_of_an_older_format_is_rejected_with_a_message() {
        use crate::testing::save_of_new_game;

        let mut game = TestGame::start("");
        let path = save_of_new_game(game.resource::<sim::GameData>(), 5, "alt");
        // Derselbe Stand, wie ihn eine ältere Fassung des Spiels geschrieben hätte.
        let text = std::fs::read_to_string(&path).unwrap();
        let current = format!("(format:{SAVE_FORMAT},");
        assert!(text.starts_with(&current), "Spielstand beginnt anders");
        let older = SAVE_FORMAT - 1;
        std::fs::write(
            &path,
            text.replacen(&current, &format!("(format:{older},"), 1),
        )
        .unwrap();

        // Aus dem Hauptmenü: Es bleibt beim Hauptmenü, und der Spieler erfährt den Grund.
        game.send(SessionRequest::Load(path.clone()));
        game.frames(4);
        assert_eq!(game.state(), AppState::MainMenu);
        assert!(!game.world().contains_resource::<Galaxy>());
        let notice = game.resource::<Notice>().clone();
        assert!(notice.is_error && notice.seconds_left > 0.0);
        assert!(notice.text.contains("älteren Fassung"), "{}", notice.text);
        assert!(notice.text.contains(&format!("Speicherformat {older}")));
        assert!(notice.text.contains(&format!("liest Format {SAVE_FORMAT}")));
        // Der Hinweis steht auch wirklich auf dem Bildschirm.
        assert!(game.text(&notice.text).is_some());

        // Mitten in einer Partie (F9): Sie läuft weiter, als wäre nichts gewesen.
        start_campaign(&mut game, 3);
        run_fast(&mut game, 6);
        let tick = game.resource::<SimClock>().tick;
        game.send(SessionRequest::Load(path.clone()));
        game.frames(4);
        assert_eq!(game.state(), AppState::InGame);
        assert_eq!(game.resource::<SimConfig>().seed, 3);
        assert!(game.resource::<SimClock>().tick > tick);
        assert!(!game.resource::<SimSpeed>().paused);
        let notice = game.resource::<Notice>();
        assert!(notice.is_error && notice.text.contains("älteren Fassung"));

        // Ein alter Stand, dessen Zustand sich gar nicht mehr lesen lässt, wird ebenso am
        // Format erkannt; eine beschädigte Datei im geltenden Format ist schlicht unlesbar,
        // und ein Stand aus einer neueren Fassung heißt auch so.
        let cases = [
            (
                format!("(format:{older},data_hash:1,state:(galaxy:42))"),
                "älteren Fassung",
            ),
            (
                format!("(format:{SAVE_FORMAT},data_hash:1,state:(galaxy:42))"),
                "Spielstand unlesbar",
            ),
            (
                format!("(format:{},data_hash:1,state:())", SAVE_FORMAT + 1),
                "neueren Fassung",
            ),
            ("kein Spielstand".to_string(), "Spielstand unlesbar"),
        ];
        for (content, expected) in cases {
            std::fs::write(&path, &content).unwrap();
            game.send(SessionRequest::Load(path.clone()));
            game.frames(3);
            let notice = game.resource::<Notice>();
            assert!(
                notice.is_error && notice.text.contains(expected),
                "{content}: {}",
                notice.text
            );
            assert_eq!(game.state(), AppState::InGame);
            assert_eq!(game.resource::<SimConfig>().seed, 3);
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_rejected_command_of_the_player_is_reported_with_its_reason() {
        use sim::command::{CommandQueue, SimCommand};
        use sim::tech::Research;

        let mut game = TestGame::start("");
        start_campaign(&mut game, 3);
        run_fast(&mut game, 6);
        let player = game.resource::<SimConfig>().player.unwrap();
        // Ein Befehl, den der Kern ablehnen muss: ein Forschungsziel, das schon erforscht
        // ist oder dem die Voraussetzung fehlt.
        let data = game.resource::<sim::GameData>().clone();
        let research = game.resource::<Research>();
        let tech = (0..data.techs.len() as u16)
            .find(|tech| !research.available(&data, player).contains(tech))
            .expect("eine Technologie, die nicht erforschbar ist");
        let known = research.knows(player, tech);
        game.world_mut().resource_mut::<CommandQueue>().push(
            player,
            CommandSource::Player,
            SimCommand::SetResearchTarget { tech: Some(tech) },
        );
        game.frames(4);
        let notice = game.resource::<Notice>();
        assert!(notice.is_error, "{}", notice.text);
        assert!(
            notice.text.starts_with("Befehl abgelehnt: "),
            "{}",
            notice.text
        );
        let name = &data.techs[tech as usize].name;
        if known {
            assert!(
                notice
                    .text
                    .contains(&format!("{name} ist bereits erforscht"))
            );
        } else {
            assert!(
                notice.text.contains("Voraussetzung fehlt"),
                "{}",
                notice.text
            );
        }
        // Befehle der KI-Gemeinschaften gehen den Spieler nichts an.
        game.world_mut().resource_mut::<Notice>().seconds_left = 0.0;
        game.world_mut().resource_mut::<Notice>().text.clear();
        game.world_mut().resource_mut::<CommandQueue>().push(
            sim::ids::PolityId(1),
            CommandSource::Ai,
            SimCommand::SetResearchTarget { tech: Some(9999) },
        );
        game.frames(4);
        assert!(game.resource::<Notice>().text.is_empty());
    }

    #[test]
    fn a_loaded_game_waits_at_the_saved_tick() {
        let mut game = TestGame::start("");
        start_campaign(&mut game, 3);
        run_fast(&mut game, 8);
        game.world_mut().resource_mut::<SimSpeed>().paused = true;
        game.frames(2);
        let saved = game.resource::<SimClock>().tick;
        assert!(saved > 0);
        let path =
            std::env::temp_dir().join(format!("konfluenz_test_{}_stand.ron", std::process::id()));
        game.send(SessionRequest::Save(path.clone()));
        game.frames(2);
        assert!(path.is_file());

        // Geladen wird, während die Zeit auf der schnellsten Stufe läuft.
        run_fast(&mut game, 8);
        assert!(game.resource::<SimClock>().tick > saved);
        game.send(SessionRequest::Load(path.clone()));
        game.frames(8);
        let _ = std::fs::remove_file(&path);
        // Die geladene Partie steht genau dort, wo gespeichert wurde, und wartet.
        assert_eq!(game.resource::<SimClock>().tick, saved);
        assert!(game.resource::<SimSpeed>().paused);
        assert!(game.resource::<Time<Virtual>>().is_paused());
        let notice = game.resource::<Notice>();
        assert!(!notice.is_error && notice.text.starts_with("Geladen"));
        // Die Leertaste lässt sie weiterlaufen.
        game.key(KeyCode::Space);
        game.frames(4);
        assert!(game.resource::<SimClock>().tick > saved);
    }

    #[test]
    fn a_new_game_starts_paused_at_its_first_tick() {
        let mut game = TestGame::start("");
        start_campaign(&mut game, 7);
        game.frames(6);
        assert_eq!(game.resource::<SimClock>().tick, 0);
        assert!(game.resource::<SimSpeed>().paused);
        let notice = game.resource::<Notice>();
        assert_eq!(notice.text, "Neue Partie, Seed 7");
    }
}

//! Ein Spieler klickt sich durch das ganze Spiel: Hauptmenü, Partie, Gefechtsübung und
//! zurück. Anders als die Tests der einzelnen Teile schickt dieser keine Bitten an die
//! Systeme, sondern klickt wie ein Mensch auf die Knöpfe, die gerade gezeichnet sind. So
//! fällt auf, wenn ein Knopf nichts auslöst, ein Wechsel das Spiel beendet oder ein Klick
//! auf eine Leiste in die Szene dahinter durchschlägt.

use crate::battle_view::feed::BattleFeed;
use crate::battle_view::practice::PracticeBattle;
use crate::battle_view::{SelectedShip, ViewFrame, ViewOptions};
use crate::state::AppState;
use crate::testing::TestGame;
use crate::time_control::SimSpeed;
use bevy::prelude::*;
use sim::clock::SimClock;
use sim::galaxy::Galaxy;

/// Ein Schiff, das frei in der Szene steht (nicht unter einer Tafel), mit seiner Stelle
/// im Bild.
fn ship_in_the_open(game: &TestGame) -> Vec2 {
    let frame = game.resource::<ViewFrame>();
    let battle = game.resource::<BattleFeed>().battle().expect("Gefecht");
    battle
        .ships
        .iter()
        .enumerate()
        .filter(|(_, ship)| ship.status.in_play())
        .filter_map(|(index, _)| Some(frame.projector.project(frame.position(index)?)?.0))
        .find(|at| at.x > 300.0 && at.x < 1200.0 && at.y > 60.0 && at.y < 880.0)
        .expect("ein Schiff in der freien Szene")
}

fn battle_tick(game: &TestGame) -> u64 {
    game.resource::<PracticeBattle>().battle.tick
}

#[test]
fn a_player_clicks_from_campaign_to_practice_and_back_again() {
    let mut game = TestGame::start("");
    assert_eq!(game.state(), AppState::MainMenu);

    for round in 0..3 {
        // --- Partie: beginnen, Zeit laufen lassen, über das Menü zurück ---
        game.click_on("Neue Partie");
        game.frames(4);
        assert_eq!(game.state(), AppState::InGame, "Runde {round}");
        assert!(game.world().contains_resource::<Galaxy>());
        game.key(KeyCode::Space);
        game.frames(12);
        assert!(game.resource::<SimClock>().tick > 0, "die Zeit läuft");
        assert_eq!(game.button("Zum Hauptmenü"), None, "das Menü ist noch zu");
        game.key(KeyCode::Escape);
        game.click_on("Zum Hauptmenü");
        game.frames(4);
        assert_eq!(game.state(), AppState::MainMenu, "Runde {round}");
        assert!(!game.world().contains_resource::<Galaxy>());

        // --- Gefechtsübung: aufstellen und beginnen ---
        game.click_on("Gefechtsübung");
        game.click_on("Gefecht beginnen");
        game.frames(6);
        assert_eq!(game.state(), AppState::Practice, "Runde {round}");
        assert_eq!(battle_tick(&game), 0);
        assert!(game.resource::<SimSpeed>().paused);

        // Ein Schiff wählen; die Knöpfe der Kopfleiste tun ihre Arbeit, ohne dass der Klick
        // in die Szene durchschlägt und die Auswahl aufhebt.
        let at = ship_in_the_open(&game);
        game.click(at);
        let selected = game.resource::<SelectedShip>().0;
        assert!(selected.is_some(), "Klick auf ein Schiff wählt es");
        let labels = game.resource::<ViewOptions>().labels;
        game.click_on("Namen");
        assert_eq!(game.resource::<ViewOptions>().labels, !labels);
        game.click_on("Übersicht");
        game.click_on("Gefecht");
        assert_eq!(game.resource::<SelectedShip>().0, selected);

        // Die Zeit läuft mit der schnellsten Stufe bis zum Ende des Gefechts.
        game.click_on("25×");
        assert!(!game.resource::<SimSpeed>().paused);
        for _ in 0..1500 {
            if game.resource::<PracticeBattle>().battle.finished {
                break;
            }
            game.frames(1);
        }
        assert!(game.resource::<PracticeBattle>().battle.finished);
        assert_eq!(game.resource::<SelectedShip>().0, selected);

        // „Neu starten“ in der Bilanz, während die Uhr noch läuft: Das Gefecht steht wieder
        // am Anfang und wartet.
        game.frames(3);
        assert!(!game.resource::<SimSpeed>().paused, "die Uhr läuft weiter");
        game.click_on("Neu starten");
        game.frames(10);
        assert_eq!(game.state(), AppState::Practice);
        assert_eq!(battle_tick(&game), 0, "Runde {round}");
        assert!(game.resource::<SimSpeed>().paused);
        assert_eq!(game.resource::<SelectedShip>().0, None);

        // „Verlassen“ führt ins Hauptmenü; von der Übung bleibt nichts.
        game.click_on("Verlassen");
        game.frames(4);
        assert_eq!(game.state(), AppState::MainMenu, "Runde {round}");
        assert!(!game.world().contains_resource::<PracticeBattle>());
    }
}

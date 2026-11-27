//! Ein Spieler klickt sich durch das ganze Spiel: Hauptmenü, Partie mit Forschung,
//! Haushalt und Flotten, Gefechtsübung und zurück. Anders als die Tests der einzelnen
//! Teile schickt dieser keine Bitten an die Systeme, sondern klickt wie ein Mensch auf
//! die Knöpfe, die gerade gezeichnet sind. So fällt auf, wenn ein Knopf nichts auslöst, ein Wechsel das Spiel
//! beendet oder ein Klick auf eine Leiste in die Szene dahinter durchschlägt.

use crate::battle_view::feed::BattleFeed;
use crate::battle_view::practice::PracticeBattle;
use crate::battle_view::{SelectedShip, ViewFrame, ViewOptions};
use crate::map::Selection;
use crate::state::AppState;
use crate::testing::TestGame;
use crate::time_control::SimSpeed;
use crate::ui::{TechTab, UiState};
use bevy::prelude::*;
use sim::clock::SimClock;
use sim::galaxy::Galaxy;
use sim::state::SimConfig;
use sim::tech::Research;

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

/// Eine Technologie, die der Spieler jetzt erforschen kann und die nicht schon sein Ziel
/// ist: Nummer und Name.
fn open_technology(game: &TestGame) -> (u16, String) {
    let data = game.resource::<sim::GameData>();
    let research = game.resource::<Research>();
    let player = game.resource::<SimConfig>().player.expect("Spieler");
    let target = research.get(player).and_then(|entry| entry.target);
    let tech = research
        .available(data, player)
        .into_iter()
        .find(|tech| Some(*tech) != target)
        .expect("eine erforschbare Technologie");
    (tech, data.techs[tech as usize].name.clone())
}

/// Der Spieler öffnet das Forschungsfenster, legt ein Ziel fest, sieht nach den anderen
/// und schließt es wieder. Kein Klick davon erreicht die Karte dahinter.
fn visit_the_technology_window(game: &mut TestGame) {
    let selected = game.resource::<Selection>().system;
    assert!(selected.is_some(), "das Heimatsystem ist gewählt");
    assert!(game.window_rect("Forschung").is_none());
    game.click_on("Forschung");
    assert!(game.window_rect("Forschung").is_some());
    let (tech, name) = open_technology(game);
    game.click_on(&format!("Erforschen: {name}"));
    game.frames(6);
    // Die Zeit läuft: Der Befehl gilt sofort, ein Hinweis auf Wartendes erscheint nicht.
    assert_ne!(
        game.resource::<crate::session::Notice>().text,
        crate::ui::COMMAND_WAITS
    );
    let player = game.resource::<SimConfig>().player.expect("Spieler");
    let entry = game.resource::<Research>().get(player).expect("Forschung");
    assert_eq!(entry.target, Some(tech), "das Ziel ist {name}");
    assert!(!entry.auto);
    game.click_on("Einschätzung der anderen");
    assert_eq!(game.resource::<UiState>().tech_tab, TechTab::Others);
    assert!(game.text("Einblick").is_some());
    game.click_on("Eigene Forschung");
    assert!(game.text("Wirkung der eigenen Technik").is_some());
    game.click_on("Forschung");
    // Das Fenster blendet sich einige Frames lang aus.
    game.frames(10);
    assert!(game.window_rect("Forschung").is_none());
    assert_eq!(game.resource::<Selection>().system, selected);
}

/// Der Spieler sieht in den Haushalt, bestellt im Flottenfenster ein Schiff, streicht
/// den Auftrag wieder und schließt beide Fenster. Kein Klick erreicht die Karte dahinter.
fn visit_the_state_and_fleet_windows(game: &mut TestGame) {
    use crate::ui::{budget, fleets};
    use sim::fleet::Fleets;

    let selected = game.resource::<Selection>().system;
    game.click_on("Staat");
    assert!(game.window_rect("Staatsfinanzen").is_some());
    assert!(game.text(budget::HEADING).is_some());
    game.click_on("Staat");
    game.frames(10);
    assert!(game.window_rect("Staatsfinanzen").is_none());

    game.click_on(fleets::TITLE);
    assert!(game.window_rect(fleets::TITLE).is_some());
    assert!(game.button(fleets::SHOW_ON_MAP).is_some());
    game.click_on(fleets::TAB_YARDS);
    let player = game.resource::<SimConfig>().player.expect("Spieler");
    let data = game.resource::<sim::GameData>();
    let design = data.design_index("dolch").expect("Entwurf");
    let name = data.designs[design as usize].name.clone();
    game.click_on(&format!("{}: {name}", fleets::ORDER));
    game.frames(6);
    let order = {
        let fleets = game.resource::<Fleets>();
        let orders: Vec<_> = fleets.orders_of(player).collect();
        assert_eq!(orders.len(), 1, "der Auftrag steht in der Werft");
        assert_eq!(orders[0].design, design);
        orders[0].id
    };
    game.click_on(&format!("{}: Nr. {order}", fleets::CANCEL));
    game.frames(6);
    assert_eq!(game.resource::<Fleets>().orders_of(player).count(), 0);
    game.click_on(fleets::TAB_FLEETS);
    game.click_on(fleets::TITLE);
    game.frames(10);
    assert!(game.window_rect(fleets::TITLE).is_none());
    assert_eq!(game.resource::<Selection>().system, selected);
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
        visit_the_technology_window(&mut game);
        visit_the_state_and_fleet_windows(&mut game);
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

/// Ein Spieler verfolgt eine Belagerung: von der Kopfleiste ins Fenster, auf die Karte,
/// über die Marke am System zurück ins Fenster, in die Orbitansicht, dort mit der Kamera
/// unterwegs, bis die Welt fällt, und über das Ergebnis zurück zur Karte.
#[test]
fn a_player_follows_a_siege_from_the_map_into_orbit_until_the_world_falls() {
    use crate::battle_view::BattleCamera;
    use crate::battle_view::orbit::hud::RESULT_TITLE;
    use crate::battle_view::orbit::{BACK, TO_WINDOW, WATCH, WatchedSiege};
    use crate::siege_markers::SiegeMarkers;
    use crate::state::ViewState;
    use crate::testing::conquest;
    use crate::ui::{fleets, siege};
    use bevy::input::ButtonState;
    use sim::ids::PolityId;

    let path = conquest::save_file(conquest::GROUND, Some(PolityId(0)), "rundgang_belagerung");
    let mut game = TestGame::start(&format!("--load {}", path.display()));
    game.frames(6);
    let _ = std::fs::remove_file(&path);
    assert_eq!(game.state(), AppState::InGame);
    let view = |game: &TestGame| *game.resource::<State<ViewState>>().get();
    let eye = |game: &mut TestGame| {
        let world = game.world_mut();
        world
            .query_filtered::<&Transform, With<BattleCamera>>()
            .single(world)
            .expect("Schlachtkamera")
            .translation
    };

    // Kopfleiste: Das Fenster nennt die eigene Belagerung; „Auf der Karte zeigen“ führt
    // die Karte zu ihrem System.
    game.click_on(&siege::toggle_label(1));
    game.frames(3);
    assert!(game.window_rect(siege::TITLE).is_some());
    game.click_on(fleets::SHOW_ON_MAP);
    game.frames(40);
    let planet = game
        .resource::<UiState>()
        .siege
        .chosen
        .expect("gewählte Belagerung");
    let system = game.resource::<Galaxy>().planet(planet).system;
    assert_eq!(game.resource::<Selection>().system, Some(system));
    game.click_on(&siege::toggle_label(1));
    game.frames(10);
    assert!(game.window_rect(siege::TITLE).is_none());

    // Karte: Am System steht die Marke mit der Phase; ein Klick öffnet das Fenster wieder.
    let marker = game.resource::<SiegeMarkers>().0[0].clone();
    assert_eq!(marker.planet, planet);
    assert_eq!(marker.phase_number, 4);
    game.click_on(&format!("{}: {}", siege::TITLE, marker.planet_name));
    game.frames(3);
    assert!(game.window_rect(siege::TITLE).is_some());
    assert_eq!(
        game.resource::<Selection>().system,
        Some(system),
        "der Klick auf die Marke wählt auf der Karte nichts anderes"
    );

    // Orbitansicht: Die Kamera fliegt mit den Tasten der Schlachtansicht, „Übersicht“
    // holt sie zurück.
    game.click_on(WATCH);
    game.frames(4);
    assert_eq!(view(&game), ViewState::Siege);
    let home = eye(&mut game);
    game.key_event(KeyCode::KeyW, ButtonState::Pressed);
    game.frames(20);
    game.key_event(KeyCode::KeyW, ButtonState::Released);
    game.frames(20);
    let moved = eye(&mut game);
    assert!(
        moved.distance(home) > 50.0,
        "W bewegt die Kamera: {home} → {moved}"
    );
    game.click_on("Übersicht");
    game.frames(3);
    assert!(
        eye(&mut game).distance(home) < 1.0,
        "zurück in der Übersicht"
    );
    // Hin und her zwischen Ansicht und Fenster.
    game.click_on(TO_WINDOW);
    game.frames(4);
    assert_eq!(view(&game), ViewState::Galaxy);
    assert!(game.window_rect(siege::TITLE).is_some());
    game.click_on(WATCH);
    game.frames(4);
    assert_eq!(view(&game), ViewState::Siege);

    // Die Zeit läuft, bis die Welt fällt: Das Ergebnis erscheint, die Partie hält an.
    game.click_on("25×");
    assert!(!game.resource::<SimSpeed>().paused);
    for _ in 0..400 {
        if game.resource::<WatchedSiege>().ended() {
            break;
        }
        game.frames(1);
    }
    game.frames(3);
    assert!(game.resource::<WatchedSiege>().ended(), "die Welt fällt");
    assert!(game.resource::<SimSpeed>().paused);
    assert!(game.window_rect(RESULT_TITLE).is_some());
    assert_eq!(
        game.resource::<Galaxy>().planet(planet).owner,
        Some(PolityId(0))
    );
    // Zurück zur Karte: dieselbe Auswahl, keine Belagerung mehr, die Marke ist fort.
    game.click_on(BACK);
    game.frames(6);
    assert_eq!(view(&game), ViewState::Galaxy);
    assert_eq!(game.resource::<Selection>().system, Some(system));
    assert!(game.resource::<SiegeMarkers>().0.is_empty());
    assert!(game.world().get_resource::<WatchedSiege>().is_none());
}

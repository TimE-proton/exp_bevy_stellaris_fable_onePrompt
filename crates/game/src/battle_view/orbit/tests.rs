//! Tests der Orbitansicht: Aufstellung und Meldungen als reine Funktionen, dann die
//! Ansicht im laufenden Spiel, betreten aus dem Fenster „Belagerung“ wie von einem
//! Menschen, bis zum Fall der Welt.

use super::hud::{HEADING_EVENTS, HEADING_NUMBERS, KEEP_WATCHING, RESULT_TITLE, day_report};
use super::scene::{
    LANDERS, Lander, OrbitHull, OrbitScene, OrbitVisual, PLANET_RADIUS, fleet_altitude, show_rates,
    zone_direction, zone_radius,
};
use super::*;
use crate::testing::{TestGame, conquest};
use crate::time_control::SPEEDS;
use crate::ui::siege as window;
use crate::ui::siege::detail::{ASSAULT, ORDERS_WINDOW};
use sim::ids::PolityId;
use sim::siege::siege_cards;

const ME: PolityId = PolityId(0);

/// Die eine Belagerung des Stands, als Karte des Kerns.
fn card(days: u64) -> SiegeCard {
    let (data, state) = conquest::state(days);
    let cards = siege_cards(&SiegeRefs::of_state(state, data), state.clock.day());
    assert!(!cards.is_empty(), "eine Belagerung am Tag {days}");
    cards[0].clone()
}

#[test]
fn large_fleets_are_shown_to_scale_and_every_class_keeps_a_hull() {
    assert_eq!(shown_hulls([0, 3, 2, 1, 1, 0]), ([0, 3, 2, 1, 1, 0], 1));
    let (hulls, per_hull) = shown_hulls([120, 40, 20, 8, 3, 1]);
    assert_eq!(per_hull, 4);
    assert_eq!(hulls, [30, 10, 5, 2, 1, 1]);
    assert!(hulls.iter().sum::<u32>() <= MAX_HULLS + 5);
    assert_eq!(shown_hulls([0; 6]), ([0; 6], 1));
}

#[test]
fn the_layout_follows_the_card_of_the_core() {
    let (data, state) = conquest::state(conquest::BOMBARDMENT);
    let card = card(conquest::BOMBARDMENT);
    let layout = Layout::of(data, &state.polities, &state.fleets, &card);
    let ships: u32 = ships_by_class(data, &state.fleets, &card).iter().sum();
    assert!(ships > 0, "die Flotte des Belagerers steht im Orbit");
    assert_eq!(layout.hulls.iter().sum::<u32>(), ships);
    assert_eq!(layout.phase, card.phase);
    assert_eq!(layout.zones.len(), card.view.siege.zones.len());
    assert_eq!(layout.attacker, state.polities.get(card.attacker).color);
    assert!(!layout.landed, "im Beschuss steht noch niemand am Boden");
    // Im Bodenkrieg steht die Truppe unten, und die Flotte ist näher gerückt.
    let ground = card_and_layout(conquest::GROUND);
    assert!(ground.landed);
    assert!(
        fleet_altitude(ground.phase, true) < fleet_altitude(layout.phase, true),
        "die Flotte rückt mit den Phasen näher"
    );
    assert!(fleet_altitude(ground.phase, false) > fleet_altitude(Phase::Approach, true));
}

fn card_and_layout(days: u64) -> Layout {
    let (data, state) = conquest::state(days);
    Layout::of(data, &state.polities, &state.fleets, &card(days))
}

#[test]
fn zones_lie_apart_on_the_side_facing_the_fleet() {
    for count in 1..=6usize {
        let spots: Vec<Vec3> = (0..count)
            .map(|index| zone_direction(index, count))
            .collect();
        for (index, spot) in spots.iter().enumerate() {
            assert!((spot.length() - 1.0).abs() < 1e-4);
            assert!(spot.dot(scene::landing_direction()) > 0.7);
            for other in &spots[index + 1..] {
                // Auch die größten Scheiben überdecken einander nicht.
                let apart = spot.angle_between(*other) * PLANET_RADIUS;
                assert!(apart >= 2.0 * zone_radius(1.0), "{count} Zonen: {apart}");
            }
        }
    }
    assert!(zone_radius(0.03) < zone_radius(0.4));
}

#[test]
fn changes_between_two_days_become_messages() {
    let old = card(conquest::BOMBARDMENT);
    assert_eq!(changes(&old, &old, (8, 8)), (Vec::new(), false));
    let mut new = old.clone();
    new.phase_number = 4;
    new.phase_label = "Bodenkrieg";
    new.troops.army = 6.0;
    new.view.siege.landed_day = Some(123_456);
    new.view.siege.zones[0].taken = true;
    new.control = false;
    let (notes, landed) = changes(&old, &new, (8, 6));
    assert!(landed);
    let all = notes.join(" | ");
    for expected in [
        "Die Belagerung ruht",
        "Phase 4 von 5: Bodenkrieg.",
        "Landung: Die Truppe am Boden zählt 6,0 Verbände.",
        "genommen",
        "verliert 2 Schiffe",
    ] {
        assert!(all.contains(expected), "„{expected}“ fehlt in: {all}");
    }
    // Zurück: Die Zone geht verloren, der Orbit ist wieder gehalten.
    let (notes, landed) = changes(&new, &old, (6, 6));
    assert!(!landed);
    let all = notes.join(" | ");
    assert!(all.contains("wieder in der Hand der Verteidiger"), "{all}");
    assert!(all.contains("hält den Orbit wieder"), "{all}");
}

#[test]
fn nobody_shoots_through_the_world() {
    use super::scene::{clear_path, station_position};
    let fleet = scene::attack_direction() * fleet_altitude(Phase::Approach, true);
    // Über dem Landegebiet ist freie Sicht, zur Rückseite der Welt nicht.
    assert!(clear_path(
        fleet,
        scene::landing_direction() * PLANET_RADIUS * 1.2
    ));
    assert!(!clear_path(
        fleet,
        -scene::attack_direction() * PLANET_RADIUS * 1.34
    ));
    // Von vier Stationen auf ihrer Bahn sieht die Flotte einige, aber nicht alle.
    let seen = (0..4)
        .filter(|index| clear_path(fleet, station_position(*index, 4, 0.0)))
        .count();
    assert!((1..4).contains(&seen), "{seen}");
}

fn watched(days: u64) -> WatchedSiege {
    let (data, state) = conquest::state(days);
    let card = card(days);
    WatchedSiege {
        planet: card.planet,
        epoch: 0,
        stamp: 0,
        layout: Layout::of(data, &state.polities, &state.fleets, &card),
        ships: 8,
        events: Vec::new(),
        landings: 0,
        result: None,
        result_closed: false,
        card,
    }
}

#[test]
fn the_scene_only_fires_where_the_core_reports_fire() {
    let mut siege = watched(conquest::BOMBARDMENT);
    let today = siege.card.view.siege.report.day;
    let rates = show_rates(&siege, today);
    assert!(rates[0] > 0.0, "im Beschuss wird geschossen: {rates:?}");
    assert_eq!(rates[2], 0.0, "ohne Truppen am Boden kein Bodenkampf");
    assert!(day_report(&siege.card).is_some());
    // Ein Bericht von vorgestern gilt nicht mehr.
    assert_eq!(show_rates(&siege, today + 2), [0.0; 4]);
    // Hält der Belagerer den Orbit nicht, fällt kein Schuss aus dem Orbit.
    siege.card.control = false;
    assert_eq!(show_rates(&siege, today), [0.0; 4]);
    siege.card.control = true;
    // Hält er ihn, meldet der Kern für den Tag aber keinen Beschuss (kein Feuer befohlen,
    // leere Magazine), fällt auch kein Schuss: Die Szene zeigt den Bericht, nicht die
    // bloße Anwesenheit der Flotte.
    let mut silent = watched(conquest::BOMBARDMENT);
    assert!(silent.card.control);
    let report = &mut silent.card.view.siege.report;
    report.ammo = 0.0;
    report.shield_absorbed = 0.0;
    report.defender_bombed = 0.0;
    report.fire_damage = 0.0;
    report.stations_lost = 0.0;
    report.guns_lost = 0.0;
    let rates = show_rates(&silent, today);
    assert_eq!(
        rates[0], 0.0,
        "kein gemeldeter Beschuss, kein Schuss: {rates:?}"
    );
    // Jedes einzelne Zeichen von Beschuss im Bericht genügt wieder.
    silent.card.view.siege.report.ammo = 1.0;
    assert!(show_rates(&silent, today)[0] > 0.0);
    // Nach dem Ende der Belagerung zeigt die Szene nichts mehr.
    siege.result = Some(SiegeResult {
        fell: true,
        headline: String::new(),
        lines: Vec::new(),
    });
    assert_eq!(show_rates(&siege, today), [0.0; 4]);
    // Im Bodenkrieg wird in der umkämpften Zone gekämpft, auch wenn die Flotte gerade
    // nicht über der Welt liegt: Die Truppe am Boden kämpft weiter.
    let ground = watched(conquest::GROUND);
    let today = ground.card.view.siege.report.day;
    let rates = show_rates(&ground, today);
    assert!(rates[2] > 0.0, "{rates:?}");
    assert_eq!(rates[0], 0.0, "kein Feuer aus dem Orbit: {rates:?}");
}

fn view(game: &TestGame) -> ViewState {
    *game.resource::<State<ViewState>>().get()
}

/// Lädt den Stand als Partie der ersten Gemeinschaft und öffnet das Fenster „Belagerung“.
fn at_the_siege_window(days: u64, name: &str) -> TestGame {
    let path = conquest::save_file(days, Some(ME), name);
    let mut game = TestGame::start(&format!("--load {}", path.display()));
    game.frames(6);
    let _ = std::fs::remove_file(&path);
    game.click_on(&window::toggle_label(1));
    game.frames(3);
    assert!(game.window_rect(window::TITLE).is_some());
    game
}

#[test]
fn the_orbit_view_opens_from_the_siege_window_and_leads_back() {
    let mut game = at_the_siege_window(conquest::BOMBARDMENT, "orbit_beschuss");
    game.click_on(WATCH);
    game.frames(4);
    assert_eq!(view(&game), ViewState::Siege);
    let planet = game.resource::<WatchedSiege>().planet;
    // Die Szene steht: der Planet und ein Rumpf je Schiff der Flotte.
    let ships = game.resource::<WatchedSiege>().ships as usize;
    assert!(ships > 0);
    assert_eq!(game.count::<OrbitHull>(), ships);
    assert!(game.count::<OrbitVisual>() > ships);
    assert_eq!(game.resource::<OrbitScene>().hulls.len(), ships);
    // Die Tafeln: Phase, Lage, Meldungen; von der Partie bleibt nur das Menü.
    assert!(game.text("Phase 3 von 5: Beschuss").is_some());
    assert!(game.text(HEADING_NUMBERS).is_some());
    assert!(game.text(HEADING_EVENTS).is_some());
    assert!(game.window_rect(window::TITLE).is_none());
    assert!(game.button(BACK).is_some());
    // Die Zeit der Partie läuft in der Ansicht weiter, und Pause hält sie an.
    let day = game.resource::<SimClock>().day();
    game.key(KeyCode::Digit6);
    game.frames(12);
    assert!(game.resource::<SimClock>().day() > day, "die Zeit läuft");
    assert!(
        game.resource::<OrbitScene>().time > 0.0,
        "die Szene läuft mit"
    );
    game.key(KeyCode::Space);
    game.frames(2);
    let (day, time) = (
        game.resource::<SimClock>().day(),
        game.resource::<OrbitScene>().time,
    );
    game.frames(10);
    assert_eq!(
        game.resource::<SimClock>().day(),
        day,
        "Pause hält die Partie an"
    );
    assert_eq!(game.resource::<OrbitScene>().time, time, "und die Szene");
    // Die Befehle der eigenen Seite stehen in der Ansicht selbst: Ein Klick auf „Sturm“
    // geht als Befehl an die Simulation, ohne die Ansicht zu verlassen.
    assert!(game.window_rect(ORDERS_WINDOW).is_some());
    assert!(game.text(window::detail::ORDERS_ATTACKER).is_some());
    let land = format!(
        "{}: {}",
        window::LAND,
        game.resource::<WatchedSiege>().card.planet_name
    );
    assert!(game.button(&land).is_some() || game.disabled_button(&land).is_some());
    assert!(game.button(window::LIFT).is_some());
    let assault = |game: &TestGame| {
        game.resource::<Sieges>()
            .siege(planet)
            .expect("Belagerung")
            .orders
            .assault
    };
    let before = assault(&game);
    let at = game.checkbox(ASSAULT).expect("Kästchen „Sturm“");
    game.click(at);
    game.frames(2);
    let waiting = game
        .resource::<sim::command::CommandQueue>()
        .pending
        .iter()
        .any(|entry| {
            matches!(
                entry.command,
                sim::command::SimCommand::SetSiegeOrders { planet: named, orders }
                    if named == planet && orders.assault != before
            )
        });
    assert!(waiting, "der Befehl wartet auf den nächsten Takt");
    assert_eq!(view(&game), ViewState::Siege, "die Ansicht bleibt");
    game.key(KeyCode::Space);
    game.frames(6);
    game.key(KeyCode::Space);
    game.frames(2);
    assert_eq!(assault(&game), !before, "die Simulation hat ihn ausgeführt");
    // Zum Fenster „Belagerung“: zurück auf die Karte, das Fenster bei dieser Welt.
    game.click_on(TO_WINDOW);
    game.frames(4);
    assert_eq!(view(&game), ViewState::Galaxy);
    assert!(game.window_rect(window::TITLE).is_some());
    assert_eq!(
        game.resource::<crate::ui::UiState>().siege.chosen,
        Some(planet)
    );
    assert_eq!(game.count::<OrbitVisual>(), 0, "die Szene ist abgeräumt");
    assert!(game.world().get_resource::<WatchedSiege>().is_none());
    // Und wieder hinein, dann mit „Zur Karte“ hinaus.
    game.click_on(WATCH);
    game.frames(4);
    assert_eq!(view(&game), ViewState::Siege);
    game.click_on(BACK);
    game.frames(4);
    assert_eq!(view(&game), ViewState::Galaxy);
}

#[test]
fn the_fall_of_the_world_ends_the_view_with_a_result_and_a_pause() {
    let mut game = at_the_siege_window(conquest::GROUND, "orbit_fall");
    game.click_on(WATCH);
    game.frames(4);
    assert_eq!(view(&game), ViewState::Siege);
    let (planet, attacker) = {
        let watched = game.resource::<WatchedSiege>();
        assert_eq!(watched.card.phase_number, 4, "Bodenkrieg");
        (watched.planet, watched.card.attacker)
    };
    assert!(game.text("Phase 4 von 5: Bodenkrieg").is_some());
    // Schnellste Stufe, bis die Simulation die Belagerung beendet.
    game.key(KeyCode::Digit6);
    let mut frames = 0;
    while !game.resource::<WatchedSiege>().ended() {
        game.frames(1);
        frames += 1;
        assert!(frames < 400, "die Welt fällt nicht");
    }
    game.frames(3);
    let watched = game.resource::<WatchedSiege>().clone();
    let result = watched.result.clone().expect("Ausgang");
    assert!(result.fell, "{result:?}");
    assert!(result.headline.starts_with(RESULT_FELL));
    assert!(
        result.lines.iter().any(|line| line.contains("besetzt")),
        "die Chronik nennt den Fall: {result:?}"
    );
    assert_eq!(
        game.resource::<sim::galaxy::Galaxy>().planet(planet).owner,
        Some(attacker)
    );
    // Die Partie hält an, das Fenster mit dem Ausgang steht da, die Ansicht bleibt.
    assert!(game.resource::<SimSpeed>().paused);
    assert_eq!(SPEEDS.len(), 6);
    assert!(game.window_rect(RESULT_TITLE).is_some());
    assert_eq!(view(&game), ViewState::Siege);
    // Nach dem Ende fällt kein Schuss mehr; die Szene bleibt zum Ansehen stehen.
    let today = game.resource::<SimClock>().day();
    assert_eq!(show_rates(&watched, today), [0.0; 4]);
    game.click_on(KEEP_WATCHING);
    game.frames(12);
    assert!(game.window_rect(RESULT_TITLE).is_none());
    // Nach dem Ende zeigt die Tafel den letzten Stand als solchen: keine Prognose mehr,
    // und keine Befehle.
    assert!(game.text(super::hud::LAST_STATE).is_some());
    assert!(
        game.text(window::FORECAST).is_none(),
        "keine Prognose nach dem Ende"
    );
    assert!(game.window_rect(ORDERS_WINDOW).is_none());
    assert_eq!(view(&game), ViewState::Siege);
    game.click_on(BACK);
    game.frames(4);
    assert_eq!(view(&game), ViewState::Galaxy);
}

#[test]
fn a_landing_sends_boats_from_the_fleet_to_the_ground() {
    let mut game = at_the_siege_window(conquest::BOMBARDMENT, "orbit_landung");
    game.click_on(WATCH);
    game.frames(4);
    assert_eq!(game.count::<Lander>(), 0, "noch ist niemand gelandet");
    // Die Simulation meldet eine Landung: Die Ansicht zählt sie, die Szene zeigt sie.
    game.world_mut().resource_mut::<WatchedSiege>().landings += 1;
    game.key(KeyCode::Digit2);
    game.frames(3);
    assert_eq!(game.count::<Lander>(), LANDERS);
    assert!(game.resource::<OrbitScene>().landing_spot().is_some());
    // Nach ihrer Flugzeit sind die Boote am Boden und verschwunden.
    game.frames(700);
    assert_eq!(game.count::<Lander>(), 0);
    assert!(game.resource::<OrbitScene>().landing_spot().is_none());
}

#[test]
fn a_resting_siege_says_so_and_the_way_into_the_battle_leads_over_the_map() {
    // Im Bodenkrieg dieses Stands liegt die Flotte nicht über der Welt: Die Belagerung
    // ruht, die Truppe am Boden kämpft weiter.
    let mut game = at_the_siege_window(conquest::GROUND, "orbit_ruht");
    game.click_on(WATCH);
    game.frames(4);
    assert_eq!(view(&game), ViewState::Siege);
    assert!(!game.resource::<WatchedSiege>().card.control);
    assert!(
        game.text(&format!("Die Belagerung {}", window::RESTING))
            .is_some()
    );
    assert_eq!(game.count::<OrbitHull>(), 0, "keine Flotte im Orbit");
    // Der Knopf zum Gefecht steht nur da, wenn im System eines läuft; hier keines.
    assert!(game.button(super::super::campaign::WATCH).is_none());
    // Die Bitte um ein Gefecht reist über die Karte: Die Ansicht schließt, die
    // Schlachtansicht antwortet (hier: Das Gefecht gibt es nicht mehr).
    game.world_mut().resource_mut::<BattleAfterLeave>().0 = Some(BattleId(9_999));
    game.world_mut()
        .resource_mut::<NextState<ViewState>>()
        .set(ViewState::Galaxy);
    game.frames(4);
    assert_eq!(view(&game), ViewState::Galaxy);
    assert!(
        game.resource::<Notice>().text.contains("Gefecht"),
        "{}",
        game.resource::<Notice>().text
    );
    assert_eq!(game.resource::<BattleAfterLeave>().0, None);
}

/// Schreibt Spielstände für Bildschirmfotos der Orbitansicht in das Verzeichnis aus
/// `KONFLUENZ_FOTOSTAENDE`: Beschuss, der Tag vor der Landung, Bodenkrieg kurz vor dem Fall, die letzte Welt einer
/// Gemeinschaft aus beiden Sichten und Welten mit Schild und mit Stationen (dafür sind
/// die Anlagen von Hand gesetzt). Aufruf:
/// `KONFLUENZ_FOTOSTAENDE=/pfad cargo test -p game -- --ignored fotostaende`.
#[test]
#[ignore = "schreibt Spielstände für Bildschirmfotos"]
fn fotostaende() {
    use sim::state::{SAVE_FORMAT, SaveGame, SimConfig};
    let Ok(dir) = std::env::var("KONFLUENZ_FOTOSTAENDE") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    // `works`: Stufen von Schild und Geschützen und Stufen der Stationen, von Hand gesetzt.
    let write = |name: &str, days: u64, player: Option<PolityId>, works: Option<(u8, f32)>| {
        let (data, state) = conquest::state(days);
        let mut state = state.clone();
        state.config = SimConfig {
            player,
            ..state.config
        };
        if let Some((shield, stations)) = works {
            let planet = *state.sieges.active.keys().next().expect("Belagerung");
            let full = (shield as f64 * data.balance.siege.shield_cap) as f32;
            let defense = state.sieges.defense_mut(planet);
            defense.shield = shield;
            defense.charge = full;
            defense.guns = 2.0;
            defense.stations = stations;
        }
        SaveGame {
            format: SAVE_FORMAT,
            data_hash: data.hash,
            state,
        }
        .write(&dir.join(format!("{name}.ron")))
        .expect("Spielstand schreiben");
    };
    let defender = card(conquest::LAST_WORLD).defender;
    write("beschuss", conquest::BOMBARDMENT, Some(ME), None);
    write("schild", conquest::BOMBARDMENT, Some(ME), Some((2, 0.0)));
    write("stationen", conquest::BOMBARDMENT, Some(ME), Some((1, 3.0)));
    write("landung", conquest::LANDING, Some(ME), None);
    write("boden", conquest::GROUND, Some(ME), None);
    write(
        "letzte_welt_belagerer",
        conquest::LAST_WORLD,
        Some(ME),
        None,
    );
    write(
        "letzte_welt_verteidiger",
        conquest::LAST_WORLD,
        Some(defender),
        None,
    );
}

#[test]
fn a_new_state_of_the_card_is_noted_and_a_landing_counted() {
    let mut siege = watched(conquest::BOMBARDMENT);
    let date = Date::from_day(siege.card.view.siege.report.day);
    assert_eq!(siege.landings, 0);
    assert!(siege.events.is_empty());
    // Derselbe Stand noch einmal: nichts Neues.
    let same = siege.card.clone();
    siege.take(date, same, 8);
    assert_eq!((siege.landings, siege.events.len()), (0, 0));
    // Eine Welle ist gelandet: Die Truppe am Boden wächst, der Tag der Landung ist neu.
    let mut landed = siege.card.clone();
    landed.troops.army += 4.0;
    landed.view.siege.landed_day = Some(siege.card.view.siege.report.day + 1);
    siege.take(date, landed.clone(), 8);
    assert_eq!(siege.landings, 1, "die Szene schickt ihre Boote");
    assert!(
        siege
            .events
            .iter()
            .any(|(_, text)| text.starts_with("Landung:")),
        "{:?}",
        siege.events
    );
    assert_eq!(siege.card, landed);
    // Ein verlorenes Schiff wird gemeldet und gezählt; eine Landung ist das nicht.
    siege.take(date, landed, 7);
    assert_eq!((siege.landings, siege.ships), (1, 7));
    assert!(
        siege
            .events
            .iter()
            .any(|(_, text)| text.contains("verliert ein Schiff"))
    );
}

#[test]
fn the_result_only_quotes_entries_that_name_this_world() {
    use sim::chronicle::ChronicleEntry;
    let (_, state) = conquest::state(conquest::BOMBARDMENT);
    let last = card(conquest::BOMBARDMENT);
    let entry = |text: String, kind: ChronicleKind| ChronicleEntry {
        tick: state.clock.tick,
        kind,
        polity: None,
        system: Some(last.system),
        text,
    };
    let mut chronicle = Chronicle::default();
    let name = &last.planet_name;
    // Eine Folge steht in der Chronik vor dem Ereignis; daneben eine Meldung über die
    // Nachbarwelt „… II“ und eine von vor drei Tagen.
    chronicle.entries.push_back(entry(
        format!("Auf {name} verfällt ein Bauauftrag."),
        ChronicleKind::Economy,
    ));
    chronicle
        .entries
        .push_back(entry(format!("{name}I wird besetzt."), ChronicleKind::War));
    chronicle.entries.push_back(entry(
        format!("Die Belagerung von {name} ist aufgehoben."),
        ChronicleKind::War,
    ));
    chronicle.entries.push_back(ChronicleEntry {
        tick: state.clock.tick - 3 * TICKS_PER_DAY,
        ..entry(format!("Über {name}: Phase 2."), ChronicleKind::War)
    });
    let result = result_of(&state.galaxy, &chronicle, &state.clock, &last);
    assert!(!result.fell, "die Welt gehört noch dem Verteidiger");
    assert_eq!(result.headline, format!("{RESULT_OVER}: {name}"));
    assert_eq!(
        result.lines,
        vec![
            format!("Die Belagerung von {name} ist aufgehoben."),
            format!("Auf {name} verfällt ein Bauauftrag."),
        ],
        "die Entscheidung voran, nichts über die Nachbarwelt, nichts Altes"
    );
}

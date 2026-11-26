//! Systemtests der Schlachtansicht im ganzen Spiel (ohne Fenster und Grafikkarte): Was der
//! Feed hält, steht in der Szene, auch wenn Schiffe hinzukommen, die Ansicht verlassen und
//! wieder betreten wird oder das Gefecht mitten im Frame wechselt.

use super::effects::PoolSlot;
use super::feed::testing::{MIXED, info, stepped};
use super::feed::{BattleFeed, BattleViewRequest, SideInfo};
use super::practice::PracticeBattle;
use super::scene::{BattleVisual, ShipVisual, SideAssets};
use super::{BattleViewSet, BattleViewShown, EffectStats, SelectedShip, ViewFrame};
use crate::state::{AppState, ViewState};
use crate::testing::TestGame;
use crate::time_control::SimSpeed;
use bevy::camera::visibility::ViewVisibility;
use bevy::prelude::*;
use sim::GameData;
use sim::battle::{Battle, CombatantSetup, ShipSpec};
use sim::ids::PolityId;

fn practice(fleet_a: &str, fleet_b: &str) -> TestGame {
    let mut game = TestGame::start(&format!(
        "--practice --fleet-a {fleet_a} --fleet-b {fleet_b} --seed 5"
    ));
    game.frames(4);
    assert_eq!(game.state(), AppState::Practice);
    game
}

fn shown_battle(game: &TestGame) -> &Battle {
    game.resource::<BattleFeed>()
        .battle()
        .expect("Gefecht im Feed")
}

fn in_play(game: &TestGame) -> usize {
    shown_battle(game)
        .ships
        .iter()
        .filter(|ship| ship.status.in_play())
        .count()
}

/// Rümpfe in der Szene; je Schiff des Gefechts höchstens einer.
fn hulls(game: &mut TestGame) -> usize {
    let world = game.world_mut();
    let mut indices: Vec<u32> = world
        .query::<&ShipVisual>()
        .iter(world)
        .map(|visual| visual.index)
        .collect();
    let count = indices.len();
    indices.sort_unstable();
    indices.dedup();
    assert_eq!(indices.len(), count, "ein Schiff hat zwei Rümpfe");
    count
}

/// Rümpfe, die die Engine zuletzt als sichtbar eingestuft hat: im Bildkegel der Kamera und
/// nicht ausgeblendet.
fn visible_hulls(game: &mut TestGame) -> usize {
    let world = game.world_mut();
    world
        .query_filtered::<&ViewVisibility, With<ShipVisual>>()
        .iter(world)
        .filter(|visibility| visibility.get())
        .count()
}

/// Wracks in der Szene: alles, was zum Gefecht gehört und weder Rumpf noch Effektplatz ist.
fn wrecks(game: &mut TestGame) -> usize {
    game.count::<BattleVisual>() - game.count::<ShipVisual>() - game.count::<PoolSlot>()
}

/// Lässt die Zeit auf der schnellsten Stufe laufen, hält sie wieder an und wartet, bis die
/// Szene den Stand übernommen hat.
fn advance(game: &mut TestGame, frames: usize) {
    *game.world_mut().resource_mut::<SimSpeed>() = SimSpeed {
        paused: false,
        level: 5,
    };
    game.frames(frames);
    game.world_mut().resource_mut::<SimSpeed>().paused = true;
    game.frames(3);
}

/// Was im Feed steht, steht in der Szene: je Schiff auf dem Feld ein Rumpf, je Wrack ein
/// Wrack, je gezeigtem Effekt ein Platz.
fn assert_scene_matches_feed(game: &mut TestGame, when: &str) {
    assert_eq!(hulls(game), in_play(game), "Rümpfe {when}");
    assert_eq!(
        wrecks(game),
        shown_battle(game).wrecks.len(),
        "Wracks {when}"
    );
    let stats = *game.resource::<EffectStats>();
    assert_eq!(
        game.count::<PoolSlot>(),
        stats.entities,
        "Effektplätze {when}"
    );
    assert_eq!(
        stats.shots,
        shown_battle(game).shots.len(),
        "gezeigte Geschosse {when}"
    );
}

#[test]
fn every_ship_in_play_has_a_hull_and_every_wreck_its_remains() {
    let mut game = practice("horst:1,waechter:3,dolch:3", "bastion:1,speer:2,dolch:4");
    let initial = shown_battle(&game).ships.len();
    assert_scene_matches_feed(&mut game, "vor dem ersten Takt");
    assert_eq!(hulls(&mut game), initial);
    // In der Übersicht liegt jedes Schiff im Bildkegel der Schlachtkamera.
    assert_eq!(visible_hulls(&mut game), initial);

    let (mut launched, mut lost) = (0, 0);
    for round in 0..80 {
        advance(&mut game, 5);
        assert_scene_matches_feed(&mut game, &format!("in Runde {round}"));
        let battle = shown_battle(&game);
        launched = launched.max(battle.ships.len() - initial);
        lost = lost.max(battle.wrecks.len());
        if battle.finished {
            break;
        }
    }
    assert!(shown_battle(&game).finished, "das Gefecht kam zum Ende");
    assert!(launched > 0, "der Träger hat Jäger gestartet");
    assert!(lost > 3, "es gab Verluste");
    assert!(game.resource::<EffectStats>().peak_shots > 0);

    // Neu starten: die Ausgangslage, ohne Wracks und ohne alte Effekte.
    game.send(BattleViewRequest::Restart);
    game.frames(4);
    assert_eq!(shown_battle(&game).tick, 0);
    assert_scene_matches_feed(&mut game, "nach dem Neustart");
    assert_eq!(hulls(&mut game), initial);
    assert_eq!(game.count::<PoolSlot>(), 0);

    // Verlassen: Nichts vom Gefecht bleibt stehen.
    game.send(BattleViewRequest::Leave);
    game.frames(4);
    assert_eq!(game.state(), AppState::MainMenu);
    assert_eq!(game.count::<BattleVisual>(), 0);
    assert_eq!(game.count::<ShipVisual>(), 0);
    assert!(!game.resource::<BattleViewShown>().0);
}

#[test]
fn a_fleet_joining_mid_battle_is_shown() {
    let mut game = practice("speer:2,dolch:3", "speer:2,dolch:3");
    advance(&mut game, 8);
    assert!(shown_battle(&game).tick > 40);
    assert_scene_matches_feed(&mut game, "vor der Verstärkung");
    game.world_mut().resource_mut::<SelectedShip>().0 = Some(1);
    let camera = |game: &mut TestGame| {
        let world = game.world_mut();
        *world
            .query_filtered::<&Transform, With<super::camera::BattleCamera>>()
            .single(world)
            .unwrap()
    };
    let view = camera(&mut game);

    // Eine dritte Partei stößt dazu, wie der Kern es für Verstärkungen vorsieht; ihr
    // Besitzer legt den neuen Stand im Feed ab.
    let world = game.world_mut();
    world.resource_scope(|world, mut practice: Mut<PracticeBattle>| {
        world.resource_scope(|world, mut feed: Mut<BattleFeed>| {
            let data = world.resource::<GameData>();
            let design = |key: &str| data.design_index(key).unwrap();
            let specs: Vec<ShipSpec> = ["speer", "speer", "waechter", "dolch"]
                .iter()
                .map(|key| ShipSpec::fresh(data, design(key)))
                .collect();
            let setup = CombatantSetup::new(PolityId(2), data);
            let side = practice.battle.add_combatant(data, setup, &specs);
            assert_eq!(side, 2);
            practice.battle.set_hostile(0, 2, true);
            practice.battle.set_hostile(1, 2, true);
            feed.refresh(&practice.battle);
        });
    });
    game.frames(3);
    let third = |game: &TestGame| {
        shown_battle(game)
            .ships
            .iter()
            .filter(|ship| ship.side == 2 && ship.status.in_play())
            .count()
    };
    assert_eq!(third(&game), 4);
    assert_scene_matches_feed(&mut game, "nach der Verstärkung");
    assert_eq!(game.resource::<SideAssets>().sides.len(), 3);
    // Auswahl und Kamera bleiben, wo sie waren.
    assert_eq!(game.resource::<SelectedShip>().0, Some(1));
    assert_eq!(camera(&mut game), view);

    // Die dritte Partei kämpft mit; ihre Geschosse werden gezeigt wie alle anderen.
    let mut third_shots = 0;
    for round in 0..40 {
        advance(&mut game, 4);
        assert_scene_matches_feed(&mut game, &format!("Runde {round} mit dritter Partei"));
        third_shots += shown_battle(&game)
            .shots
            .iter()
            .filter(|shot| shot.side == 2)
            .count();
        if shown_battle(&game).finished {
            break;
        }
    }
    assert!(third_shots > 0, "die dritte Partei hat geschossen");
}

#[test]
fn a_joining_side_gets_its_colour_when_it_is_named_later() {
    let mut game = practice("dolch:2", "dolch:2");
    let world = game.world_mut();
    world.resource_scope(|world, mut practice: Mut<PracticeBattle>| {
        world.resource_scope(|world, mut feed: Mut<BattleFeed>| {
            let data = world.resource::<GameData>();
            let specs = vec![ShipSpec::fresh(data, data.design_index("dolch").unwrap()); 2];
            let setup = CombatantSetup::new(PolityId(2), data);
            practice.battle.add_combatant(data, setup, &specs);
            feed.refresh(&practice.battle);
        });
    });
    game.frames(2);
    // Solange die Partei keinen Namen und keine Farbe hat, ist sie grau.
    let grey = game.resource::<BattleFeed>().side_color(2);
    assert_eq!(game.resource::<SideAssets>().sides[2].color, grey);
    let green = (0.3, 0.9, 0.4);
    game.world_mut()
        .resource_mut::<BattleFeed>()
        .info
        .sides
        .push(SideInfo {
            name: "Grüner Verband".into(),
            color: green,
        });
    game.frames(2);
    assert_eq!(game.resource::<SideAssets>().sides[2].color, green);
    assert_eq!(hulls(&mut game), 6);
}

/// Ein Gefecht, das ein System mitten im Frame in den Feed legt.
#[derive(Resource, Default)]
struct Pending(Option<Battle>);

fn begin_pending(mut pending: ResMut<Pending>, mut feed: ResMut<BattleFeed>) {
    if let Some(battle) = pending.0.take() {
        feed.begin(&battle, info(2));
    }
}

#[test]
fn another_battle_arriving_mid_frame_is_shown_completely() {
    let mut game = practice("dolch:2", "dolch:2");
    assert_eq!(hulls(&mut game), 4);
    // Das neue Gefecht kommt in den Feed, nachdem die Lagen dieses Frames schon für das
    // alte berechnet sind, und es hat mehr Schiffe.
    game.app.init_resource::<Pending>().add_systems(
        Update,
        begin_pending
            .after(BattleViewSet::Poses)
            .before(BattleViewSet::Scene),
    );
    let (larger, _) = stepped(game.resource::<GameData>(), MIXED, 120);
    let expected = larger
        .ships
        .iter()
        .filter(|ship| ship.status.in_play())
        .count();
    assert!(expected > 12);
    game.world_mut().resource_mut::<Pending>().0 = Some(larger.clone());
    game.frames(1);
    // Schon in diesem Frame gelten die Lagen für das neue Gefecht.
    assert!(
        game.resource::<ViewFrame>()
            .matches(game.resource::<BattleFeed>())
    );
    game.frames(1);
    assert_eq!(hulls(&mut game), expected);
    assert_eq!(wrecks(&mut game), larger.wrecks.len());
    // Kein Rumpf steht an der Lage eines Schiffs des alten Gefechts.
    let frame = game.resource::<ViewFrame>();
    for (index, ship) in larger.ships.iter().enumerate() {
        assert_eq!(frame.poses[index].0, super::to_world(ship.pos));
    }
    // Und zurück zu einem kleineren, wieder mitten im Frame.
    let (smaller, _) = stepped(game.resource::<GameData>(), ["dolch:1", "dolch:2"], 0);
    game.world_mut().resource_mut::<Pending>().0 = Some(smaller);
    game.frames(2);
    assert_eq!(hulls(&mut game), 3);
    assert_eq!(wrecks(&mut game), 0);
}

/// Eine Partie, in deren Schlachtansicht ein laufendes Gefecht gezeigt wird.
fn campaign_battle() -> (TestGame, Battle) {
    let mut game = TestGame::start("--seed 3");
    game.frames(4);
    assert_eq!(game.state(), AppState::InGame);
    // Ein Augenblick mitten im Gefecht, in dem geschossen wird.
    let data = game.resource::<GameData>();
    let (mut battle, _) = stepped(data, MIXED, 260);
    while battle.shots.len() < 8 {
        battle.step(data);
    }
    assert!(!battle.finished);
    game.world_mut()
        .resource_mut::<BattleFeed>()
        .begin(&battle, info(2));
    show_battle(&mut game);
    (game, battle)
}

fn show_battle(game: &mut TestGame) {
    game.world_mut()
        .resource_mut::<NextState<ViewState>>()
        .set(ViewState::Battle);
    game.frames(3);
    assert!(game.resource::<BattleViewShown>().0);
}

fn show_map(game: &mut TestGame) {
    game.world_mut()
        .resource_mut::<NextState<ViewState>>()
        .set(ViewState::Galaxy);
    game.frames(3);
    assert!(!game.resource::<BattleViewShown>().0);
}

#[test]
fn the_same_battle_shown_again_gets_its_ships_and_effects_back() {
    let (mut game, battle) = campaign_battle();
    assert_scene_matches_feed(&mut game, "beim ersten Zeigen");
    let slots = game.count::<PoolSlot>();
    assert!(slots >= battle.shots.len() && slots > 0);

    // Zur Karte und zurück, ohne dass das Gefecht neu angesagt wird.
    show_map(&mut game);
    assert_eq!(game.count::<BattleVisual>(), 0);
    show_battle(&mut game);
    assert_scene_matches_feed(&mut game, "beim zweiten Zeigen");
    assert_eq!(game.count::<PoolSlot>(), slots);

    // Dieselbe Ansicht noch einmal anfordern, während sie schon gezeigt wird.
    show_battle(&mut game);
    assert_scene_matches_feed(&mut game, "nach erneutem Anfordern");
    assert_eq!(game.count::<PoolSlot>(), slots);

    // Das Gefecht verschwindet aus dem Feed, während die Ansicht gezeigt wird: Die Szene
    // leert sich. Kommt ein neues, steht es da.
    game.world_mut().resource_mut::<BattleFeed>().clear();
    game.frames(2);
    assert_eq!(game.count::<BattleVisual>(), 0);
    game.world_mut()
        .resource_mut::<BattleFeed>()
        .begin(&battle, info(2));
    game.frames(2);
    assert_scene_matches_feed(&mut game, "nach neuem Gefecht");

    // Zurück auf der Karte und im Hauptmenü bleibt nichts stehen.
    show_map(&mut game);
    game.send(crate::session::SessionRequest::QuitToMenu);
    game.frames(4);
    assert_eq!(game.state(), AppState::MainMenu);
    assert_eq!(game.count::<BattleVisual>(), 0);
}

#[test]
fn a_battle_that_is_only_shown_does_not_jump_back_and_forth() {
    // Die Zeit der Partie läuft, das gezeigte Gefecht rechnet aber niemand weiter: Die
    // Anzeige bleibt auf seinem letzten Stand, statt mit jedem festen Takt zwischen
    // vorletztem und letztem Stand hin- und herzulaufen.
    let (mut game, battle) = campaign_battle();
    *game.world_mut().resource_mut::<SimSpeed>() = SimSpeed {
        paused: false,
        level: 1,
    };
    game.frames(4);
    for _ in 0..12 {
        game.frames(1);
        let frame = game.resource::<ViewFrame>();
        assert_eq!(frame.alpha, 1.0);
        assert_eq!(frame.poses[0].0, super::to_world(battle.ships[0].pos));
    }
}

// --- Gefechte der Partie, wie die Simulation sie führt ---

mod campaign {
    use super::*;
    use crate::battle_view::campaign::{WATCH, WatchBattle, WatchedBattle, result_of};
    use crate::battle_view::hud::{BACK_TO_MAP, RESULT_TITLE};
    use crate::map::FocusSystem;
    use crate::session::{Notice, SessionRequest};
    use sim::battle::Doctrine;
    use sim::clock::SimClock;
    use sim::command::{CommandLog, CommandOutcome, CommandQueue, CommandSource, SimCommand};
    use sim::fleet::{Battles, Fleets};
    use sim::galaxy::Galaxy;
    use sim::ids::{BattleId, FleetId, SystemId};
    use sim::polity::Polities;
    use sim::state::SimConfig;

    fn view(game: &TestGame) -> ViewState {
        *game.resource::<State<ViewState>>().get()
    }

    fn run(game: &mut TestGame, level: usize, frames: usize) {
        *game.world_mut().resource_mut::<SimSpeed>() = SimSpeed {
            paused: false,
            level,
        };
        game.frames(frames);
        game.world_mut().resource_mut::<SimSpeed>().paused = true;
        game.frames(3);
    }

    fn run_until(game: &mut TestGame, limit: usize, what: &str, done: impl Fn(&TestGame) -> bool) {
        *game.world_mut().resource_mut::<SimSpeed>() = SimSpeed {
            paused: false,
            level: 5,
        };
        for _ in 0..limit {
            if done(game) {
                game.world_mut().resource_mut::<SimSpeed>().paused = true;
                game.frames(3);
                return;
            }
            game.frames(1);
        }
        panic!("nach {limit} Frames noch nicht: {what}");
    }

    fn home_of(game: &TestGame, polity: PolityId) -> SystemId {
        let capital = game.resource::<Polities>().get(polity).capital.unwrap();
        game.resource::<Galaxy>().planet(capital).system
    }

    /// Eine Partie, in der die Flotte des Spielers im Heimatsystem des nächsten Nachbarn
    /// im Gefecht steht: Spiel, Spieler, eigene Flotte, Gefecht.
    fn war() -> (TestGame, PolityId, FleetId, BattleId) {
        let mut game = TestGame::start("--seed 3");
        game.frames(4);
        let me = game.resource::<SimConfig>().player.expect("Spieler");
        let mine = game.resource::<Fleets>().of(me).next().expect("Flotte").id;
        let home = home_of(&game, me);
        let enemy = {
            let galaxy = game.resource::<Galaxy>();
            game.resource::<Polities>()
                .alive()
                .filter(|polity| polity.id != me)
                .map(|polity| {
                    let path = galaxy
                        .shortest_path(home, home_of(&game, polity.id), |_| true)
                        .expect("Weg");
                    (polity.id, galaxy.path_length(&path))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .expect("eine andere Gemeinschaft")
                .0
        };
        let front = home_of(&game, enemy);
        for command in [
            SimCommand::DeclareWar { target: enemy },
            SimCommand::MoveFleet {
                fleet: mine,
                to: front,
            },
        ] {
            game.world_mut().resource_mut::<CommandQueue>().push(
                me,
                CommandSource::Player,
                command,
            );
        }
        run_until(&mut game, 4000, "Gefecht", |game| {
            game.resource::<Fleets>().get(mine).unwrap().in_battle
        });
        let battle = game
            .resource::<Battles>()
            .of_fleet(mine)
            .expect("die Flotte steht im Gefecht")
            .0
            .id;
        (game, me, mine, battle)
    }

    /// Was die Ansicht zeigt, ist das Gefecht der Simulation: derselbe Takt, dieselben
    /// Schiffe in demselben Zustand, und je Schiff auf dem Feld ein Rumpf in der Szene.
    fn assert_view_shows_the_simulation(game: &mut TestGame, id: BattleId, when: &str) {
        let real = game.resource::<Battles>().get(id).expect("Gefecht läuft");
        let shown = shown_battle(game);
        assert_eq!(shown.id, real.id);
        assert_eq!(shown.tick, real.tick, "Takt {when}");
        assert_eq!(shown.ships.len(), real.ships.len(), "Schiffe {when}");
        assert_eq!(
            sim::hash::hash_value(shown),
            sim::hash::hash_value(real),
            "die Ansicht weicht {when} vom Gefecht der Simulation ab"
        );
        assert_scene_matches_feed(game, when);
    }

    #[test]
    fn a_campaign_battle_is_watched_commanded_left_and_concluded() {
        let (mut game, me, mine, id) = war();
        let system = game.resource::<Battles>().get(id).unwrap().system;
        let place = game.resource::<Galaxy>().system(system).name.clone();
        // Der Spieler erfährt vom Gefecht.
        let notice = game.resource::<Notice>().text.clone();
        assert!(
            notice.contains(&format!(
                "Bei {place} beginnt ein Gefecht mit eigenen Schiffen"
            )),
            "{notice}"
        );
        assert_eq!(view(&game), ViewState::Galaxy);

        // --- Von der Marke auf der Karte in die Schlachtansicht ---
        game.send(FocusSystem(system));
        game.frames(4);
        game.click_on(&format!("{WATCH}: {place}"));
        game.frames(4);
        assert_eq!(view(&game), ViewState::Battle);
        assert!(game.resource::<BattleViewShown>().0);
        assert_eq!(game.resource::<WatchedBattle>().id, id);
        let info = game.resource::<BattleFeed>().info.clone();
        assert_eq!(info.title, format!("Gefecht bei {place}"));
        let own = info.player_side.expect("der Spieler führt eine Partei");
        assert_eq!(
            game.resource::<Battles>().get(id).unwrap().combatants[own].fleet,
            Some(mine)
        );
        assert!(
            !info.can_restart,
            "ein Gefecht der Partie beginnt nicht von vorn"
        );
        assert_view_shows_the_simulation(&mut game, id, "beim Öffnen");

        // --- Die Ansicht folgt der Simulation über mehrere Tage, mit der Uhr der Partie ---
        let per_day = sim::TICKS_PER_DAY;
        for round in 0..4 {
            let (clock, tick) = (game.resource::<SimClock>().tick, shown_battle(&game).tick);
            run(&mut game, 2, 40);
            let passed = game.resource::<SimClock>().tick - clock;
            assert!(passed >= per_day, "in Runde {round} verging kein Tag");
            // Ein Takt der Partie ist ein Takt des Gefechts: kein eigener Zeitlauf.
            assert_eq!(shown_battle(&game).tick - tick, passed);
            assert_view_shows_the_simulation(&mut game, id, &format!("in Runde {round}"));
        }
        // Die Kopfleiste sagt, wie Gefechtszeit und Kalender zusammenhängen.
        let calendar = game.resource::<BattleFeed>().info.calendar.clone();
        assert!(
            calendar.starts_with(&game.resource::<SimClock>().date().to_string())
                && calendar.contains("ein Kalendertag = 2,0 s Gefechtszeit"),
            "{calendar}"
        );
        assert!(game.text(&calendar).is_some());
        // In der Pause der Partie steht auch das Gefecht.
        let tick = shown_battle(&game).tick;
        game.frames(20);
        assert_eq!(shown_battle(&game).tick, tick);
        assert_eq!(game.resource::<Battles>().get(id).unwrap().tick, tick);

        // --- Ein Befehl aus der Ansicht erreicht die Simulation ---
        let before = game.resource::<Battles>().get(id).unwrap().combatants[own].clone();
        let wanted = Doctrine::ALL
            .into_iter()
            .find(|doctrine| *doctrine != before.doctrine)
            .unwrap();
        let entries = game.resource::<CommandLog>().entries.len();
        game.click_on(wanted.label());
        // Die Zeit steht: Der Befehl wartet, und der Spieler erfährt es.
        assert_eq!(game.resource::<CommandLog>().entries.len(), entries);
        assert_eq!(game.resource::<Notice>().text, crate::ui::COMMAND_WAITS);
        run(&mut game, 0, 6);
        let log = game.resource::<CommandLog>();
        let sent = &log.entries[entries];
        assert_eq!(
            sent.command,
            SimCommand::SetFleetOrders {
                fleet: mine,
                formation: before.formation,
                doctrine: wanted
            }
        );
        assert_eq!((sent.polity, &sent.outcome), (me, &CommandOutcome::Applied));
        assert_eq!(
            game.resource::<Battles>().get(id).unwrap().combatants[own].doctrine,
            wanted
        );
        assert_eq!(
            game.resource::<Fleets>().get(mine).unwrap().doctrine,
            wanted
        );
        assert_eq!(shown_battle(&game).combatants[own].doctrine, wanted);

        // --- Verlassen: zurück zur Karte, das Gefecht läuft in der Partie weiter ---
        game.click_on("Verlassen");
        game.frames(4);
        assert_eq!(view(&game), ViewState::Galaxy);
        assert!(!game.resource::<BattleViewShown>().0);
        assert!(!game.world().contains_resource::<WatchedBattle>());
        assert!(!game.resource::<BattleFeed>().is_active());
        assert_eq!(game.count::<ShipVisual>(), 0);
        let tick = game.resource::<Battles>().get(id).unwrap().tick;
        run(&mut game, 2, 20);
        assert!(game.resource::<Battles>().get(id).unwrap().tick > tick);

        // --- Wieder hinein, diesmal aus dem Flottenfenster ---
        game.click_on(crate::ui::fleets::TITLE);
        game.frames(3);
        game.click_on(WATCH);
        game.frames(4);
        assert_eq!(view(&game), ViewState::Battle);
        assert_view_shows_the_simulation(&mut game, id, "nach dem Wiedereintritt");

        // --- Der Rückzug geht als Befehl an die Simulation; das Gefecht kommt zum Ende ---
        game.click_on("Rückzug befehlen");
        run(&mut game, 0, 6);
        let log = game.resource::<CommandLog>();
        let sent = log.entries.last().unwrap();
        assert_eq!(sent.command, SimCommand::Retreat { fleet: mine });
        assert_eq!(sent.outcome, CommandOutcome::Applied);
        assert!(game.resource::<Battles>().get(id).unwrap().combatants[own].retreating);
        assert_view_shows_the_simulation(&mut game, id, "nach dem Rückzugsbefehl");
        run_until(&mut game, 6000, "Ende des Gefechts", |game| {
            game.resource::<Battles>().get(id).is_none()
        });
        game.frames(3);

        // --- Das Ende bei offener Ansicht: das Ergebnis aus der Bilanz der Simulation ---
        assert_eq!(view(&game), ViewState::Battle, "die Ansicht bleibt offen");
        assert!(game.resource::<WatchedBattle>().ended);
        assert!(shown_battle(&game).finished);
        let report = game
            .resource::<Battles>()
            .report(id)
            .expect("Bilanz")
            .clone();
        let expected = result_of(
            game.resource::<Galaxy>(),
            game.resource::<Fleets>(),
            game.resource::<Polities>(),
            &report,
        );
        let shown = game
            .resource::<BattleFeed>()
            .info
            .result
            .clone()
            .expect("Ergebnis");
        assert_eq!(shown.days, report.days());
        assert_eq!(shown.sides.len(), report.sides.len());
        for (side, entry) in shown.sides.iter().zip(&report.sides) {
            assert_eq!(side.name, entry.fleet_name);
            assert_eq!(
                (side.initial, side.lost, side.escaped),
                (entry.initial, entry.lost, entry.escaped)
            );
        }
        // Der Gegner hält das Feld, die eigene Flotte weicht aus: Das Fenster nennt beides.
        assert!(game.window_rect(RESULT_TITLE).is_some());
        assert!(game.text(&shown.headline).is_some(), "{}", shown.headline);
        let winner = report
            .sides
            .iter()
            .find(|side| side.held_field)
            .expect("jemand hält das Feld");
        assert_ne!(winner.owner, me);
        assert!(
            shown.headline.contains(&winner.fleet_name),
            "{}",
            shown.headline
        );
        let fleet = game.resource::<Fleets>().get(mine).unwrap().clone();
        assert!(fleet.alive && fleet.is_regrouping());
        let target = *fleet.route.last().expect("die Flotte weicht aus");
        let target = game.resource::<Galaxy>().system(target).name.clone();
        let mine_shown = &shown.sides[own];
        assert!(
            mine_shown
                .whereabouts
                .contains(&format!("Weicht aus nach {target}")),
            "{}",
            mine_shown.whereabouts
        );
        assert!(game.text(&mine_shown.whereabouts).is_some());
        assert_eq!(shown, expected);

        // --- Zurück zur Karte ---
        game.click_on(BACK_TO_MAP);
        game.frames(4);
        assert_eq!(view(&game), ViewState::Galaxy);
        assert!(!game.resource::<BattleFeed>().is_active());
        assert_eq!(game.count::<BattleVisual>(), 0);
        // Ein beendetes Gefecht lässt sich nicht mehr öffnen; der Spieler erfährt, warum.
        game.send(WatchBattle(id));
        game.frames(3);
        assert_eq!(view(&game), ViewState::Galaxy);
        assert!(game.resource::<Notice>().text.contains("schon zu Ende"));
    }

    #[test]
    fn loading_and_quitting_with_an_open_campaign_battle_leave_nothing_behind() {
        let (mut game, _, _, id) = war();
        // Über die Meldung in der Chronik in die Ansicht.
        game.click_on(&format!("{WATCH}: Chronik"));
        game.frames(4);
        assert_eq!(view(&game), ViewState::Battle);
        run(&mut game, 2, 20);
        let path = std::env::temp_dir().join(format!(
            "konfluenz_test_{}_gefecht_offen.ron",
            std::process::id()
        ));
        game.send(SessionRequest::Save(path.clone()));
        game.frames(2);
        run(&mut game, 2, 20);
        let later = game.resource::<SimClock>().tick;

        // Laden bei offener Ansicht: zurück auf der Karte des geladenen Stands.
        game.send(SessionRequest::Load(path.clone()));
        game.frames(6);
        let _ = std::fs::remove_file(&path);
        assert_eq!(game.state(), AppState::InGame);
        assert_eq!(view(&game), ViewState::Galaxy);
        assert!(
            game.resource::<SimClock>().tick < later,
            "der frühere Stand gilt"
        );
        assert!(!game.resource::<BattleViewShown>().0);
        assert!(!game.world().contains_resource::<WatchedBattle>());
        assert!(!game.resource::<BattleFeed>().is_active());
        assert_eq!(game.count::<BattleVisual>(), 0);
        // Das Gefecht läuft im geladenen Stand und lässt sich wieder öffnen.
        assert!(game.resource::<Battles>().get(id).is_some());
        game.send(WatchBattle(id));
        game.frames(4);
        assert_eq!(view(&game), ViewState::Battle);
        run(&mut game, 2, 20);
        assert_eq!(
            shown_battle(&game).tick,
            game.resource::<Battles>().get(id).unwrap().tick
        );

        // Zum Hauptmenü bei offener Ansicht: Nichts vom Gefecht bleibt stehen.
        game.send(SessionRequest::QuitToMenu);
        game.frames(5);
        assert_eq!(game.state(), AppState::MainMenu);
        assert!(!game.resource::<BattleViewShown>().0);
        assert!(!game.world().contains_resource::<WatchedBattle>());
        assert!(!game.resource::<BattleFeed>().is_active());
        assert_eq!(game.count::<BattleVisual>(), 0);
        // Eine neue Partie beginnt auf der Karte.
        game.send(SessionRequest::NewGame { seed: 5 });
        game.frames(4);
        assert_eq!(view(&game), ViewState::Galaxy);
    }

    #[test]
    fn foreign_battles_are_watched_only_where_the_player_is_present() {
        use crate::battle_view::campaign::can_watch;

        let (game, me, mine, id) = war();
        let galaxy = game.resource::<Galaxy>();
        let battle = game.resource::<Battles>().get(id).unwrap();
        let mut fleets = game.resource::<Fleets>().clone();
        assert!(can_watch(galaxy, &fleets, Some(me), battle));
        // Ein Dritter ohne Flotte und Welt im System sieht es nicht; ein Beobachter schon.
        let third = game
            .resource::<Polities>()
            .alive()
            .map(|polity| polity.id)
            .find(|polity| battle.combatants.iter().all(|c| c.owner != *polity))
            .expect("eine unbeteiligte Gemeinschaft");
        assert!(!can_watch(galaxy, &fleets, Some(third), battle));
        assert!(can_watch(galaxy, &fleets, None, battle));
        // Liegt eine seiner Flotten dort, sieht er zu.
        let visitor = fleets.of(third).next().expect("Flotte").id;
        fleets.list[visitor.index()].system = battle.system;
        assert!(can_watch(galaxy, &fleets, Some(third), battle));
        let _ = mine;
    }
}

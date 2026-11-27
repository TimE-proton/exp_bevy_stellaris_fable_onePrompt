//! Tests des Fensters „Belagerung“ und der Tafeln der Verteidigung: Texte, Sichtbarkeit,
//! Befehle aus Klicks, und das Ganze im laufenden Spiel.

use super::defense::{self, PLAN_IN_FORCE, SET_PLAN};
use super::detail::{
    ASSAULT, CANCEL, CONFIRM_LIFT, ORDERS_ATTACKER, ORDERS_DEFENDER, blocked_text, effective_orders,
};
use super::troops::{aboard_text, commanded, snap, target_text};
use super::*;
use crate::history::PriceHistory;
use crate::testing::TestGame;
use crate::time_control::SimSpeed;
use crate::ui::testing::{Painted, Screen};
use sim::GameData;
use sim::chronicle::ChronicleEntry;
use sim::clock::TICKS_PER_DAY;
use sim::command::{CommandQueue, CommandSource, QueuedCommand};
use sim::harness::{WarScript, conquest_war, run_scripted};
use sim::siege::arms::Works;
use sim::siege::{Fire, Outcome, Posture, SiegeOrders, Sieges, defense_card};
use sim::state::{SAVE_FORMAT, SaveGame, SimConfig, SimState};
use std::sync::OnceLock;

/// Tag, an dem das Drehbuch „Eroberung“ den Krieg erklärt.
const WAR_DAY: u64 = 25 * sim::clock::DAYS_PER_YEAR;
const ME: PolityId = PolityId(0);

/// Eine Partie nach dem Drehbuch „Eroberung“, `days` Tage nach der Kriegserklärung.
fn conquest(days: u64) -> (GameData, SimState) {
    let data = GameData::load_workspace().expect("Spieldaten");
    let script = WarScript::new(conquest_war());
    let (sim, violation) = run_scripted(&data, 3, WAR_DAY + days, &script);
    assert_eq!(violation, None);
    let mut state = sim.state();
    state.galaxy.rebuild_index();
    (data, state)
}

/// Hundert Tage nach der Kriegserklärung belagert die erste Gemeinschaft eine Welt ihres
/// Nachbarn, seit einer Woche, in der Phase „Beschuss“. Gerechnet wird die Partie einmal
/// für alle Tests.
fn bombardment() -> &'static (GameData, SimState) {
    static GAME: OnceLock<(GameData, SimState)> = OnceLock::new();
    GAME.get_or_init(|| conquest(100))
}

/// Die Partie aus der Sicht einer Gemeinschaft oder eines Beobachters.
struct Scene {
    data: GameData,
    state: SimState,
    history: PriceHistory,
    pending: Vec<QueuedCommand>,
}

impl Scene {
    fn new(player: Option<PolityId>) -> Self {
        let (data, state) = bombardment();
        let mut state = state.clone();
        state.config.player = player;
        Self {
            data: data.clone(),
            state,
            history: PriceHistory::default(),
            pending: Vec::new(),
        }
    }

    fn refs(&self) -> SimRefs<'_> {
        SimRefs {
            data: &self.data,
            galaxy: &self.state.galaxy,
            polities: &self.state.polities,
            clock: &self.state.clock,
            config: &self.state.config,
            chronicle: &self.state.chronicle,
            economy: &self.state.economy,
            finance: &self.state.finance,
            logistics: &self.state.logistics,
            fleets: &self.state.fleets,
            battles: &self.state.battles,
            research: &self.state.research,
            diplomacy: &self.state.diplomacy,
            sieges: &self.state.sieges,
            history: &self.history,
            pending: &self.pending,
        }
    }

    /// Die eine laufende Belagerung.
    fn card(&self) -> SiegeCard {
        let cards = siege_cards(&siege_refs(&self.refs()), self.state.clock.day());
        assert_eq!(cards.len(), 1, "eine Belagerung");
        cards[0].clone()
    }

    /// Merkt einen Befehl des Spielers vor, als stünde die Zeit.
    fn wait(&mut self, command: SimCommand) {
        self.pending.push(QueuedCommand {
            polity: self.state.config.player.expect("Spieler"),
            source: CommandSource::Player,
            command,
        });
    }
}

fn commands(actions: &[UiAction]) -> Vec<SimCommand> {
    actions
        .iter()
        .filter_map(|action| match action {
            UiAction::Command(command) => Some(command.clone()),
            _ => None,
        })
        .collect()
}

/// Zeichnet das Fenster, bis es seine Größe gefunden hat.
fn window(screen: &mut Screen, scene: &Scene, ui: &mut SiegeUi) -> Painted {
    let refs = scene.refs();
    let mut draw = |root: &mut egui::Ui| {
        ui.cache.refresh(&refs);
        let area = root.ctx().content_rect();
        siege_window(root.ctx(), area, &refs, ui, &mut Vec::new());
    };
    for _ in 0..3 {
        screen.settle(&mut draw);
    }
    screen.settle(&mut draw)
}

/// Klickt im Fenster auf den Text `label` und liefert, was der Klick auslöst.
fn click(screen: &mut Screen, scene: &Scene, ui: &mut SiegeUi, label: &str) -> Vec<UiAction> {
    let at = window(screen, scene, ui)
        .text(label)
        .unwrap_or_else(|| panic!("„{label}“ steht nicht im Fenster"))
        .center();
    let refs = scene.refs();
    let mut actions = Vec::new();
    screen.click(at, |root| {
        ui.cache.refresh(&refs);
        let area = root.ctx().content_rect();
        siege_window(root.ctx(), area, &refs, ui, &mut actions);
    });
    actions
}

fn open() -> SiegeUi {
    SiegeUi {
        open: true,
        ..Default::default()
    }
}

#[test]
fn forecasts_and_limits_read_as_plain_sentences() {
    let falls = Forecast {
        falls: true,
        outcome: Some(Outcome::Starved),
        days: 84,
        own_losses: 0.0,
        enemy_losses: 12.25,
        hull_loss: 0.0,
        prize_loss: 0.0,
        limited_by: None,
    };
    assert_eq!(
        forecast_text(Course::Starve, &falls),
        "Aushungern: Die Welt fällt in 84 Tagen (ausgehungert)."
    );
    let tomorrow = Forecast {
        days: 1,
        outcome: Some(Outcome::Stormed),
        ..falls
    };
    assert_eq!(
        forecast_text(Course::Storm, &tomorrow),
        "Sturm: Die Welt fällt in 1 Tag (gestürmt)."
    );
    let fails = Forecast {
        falls: false,
        outcome: None,
        days: 15,
        own_losses: 4.0,
        enemy_losses: 0.5,
        hull_loss: 0.14,
        prize_loss: 0.3,
        limited_by: Some(Limit::Repelled),
    };
    assert_eq!(
        forecast_text(Course::Storm, &fails),
        "Sturm: Die Welt fällt nicht; nach 15 Tagen ist Schluss. Die Landung würde abgeschlagen."
    );
    assert_eq!(
        forecast_cost(&fails),
        "eigene Verluste 4,0 Verbände · Verluste der Verteidiger 0,5 · Rumpf der schweren \
         Schiffe −14 % · Beute −30 %"
    );
    // Wo nichts verloren geht, steht kein Minus vor der Null.
    assert_eq!(
        forecast_cost(&falls),
        "eigene Verluste 0,0 Verbände · Verluste der Verteidiger 12,2 · Rumpf der schweren \
         Schiffe 0 % · Beute 0 %"
    );
    assert_eq!(loss_text(0.004), "0 %");
    assert_eq!(loss_text(0.006), "−1 %");
    let resting = Forecast {
        days: 0,
        limited_by: Some(Limit::NoControl),
        ..fails
    };
    assert_eq!(
        forecast_text(Course::Area, &resting),
        "Flächenbeschuss: Die Welt fällt nicht. Der Orbit ist umstritten; ohne Orbitalhoheit \
         geht nichts voran."
    );
    // Jede Grenze hat ihren eigenen ganzen Satz.
    let limits = [
        Limit::NoControl,
        Limit::Stations,
        Limit::Fleet,
        Limit::Fuel,
        Limit::Ammunition,
        Limit::Shield,
        Limit::NoTroops,
        Limit::Repelled,
        Limit::Time,
    ];
    let texts: Vec<&str> = limits.into_iter().map(limit_text).collect();
    for (index, text) in texts.iter().enumerate() {
        assert!(text.ends_with('.') && text.len() > 20, "{text}");
        assert!(!texts[..index].contains(text), "doppelt: {text}");
    }
    assert!(limit_text(Limit::Fuel).contains("Treibstoff"));
    assert!(limit_text(Limit::Ammunition).contains("Munition"));
    assert!(limit_text(Limit::Shield).contains("Schild"));
    assert!(limit_text(Limit::Stations).contains("Orbitalstationen"));
}

#[test]
fn small_texts_follow_german_usage() {
    assert_eq!(days_text(1.0), "1 Tag");
    assert_eq!(days_text(7.0), "7 Tage");
    assert_eq!(days_text(61.13), "61,1 Tage");
    assert_eq!(units(4.04), "4,0");
    assert_eq!(toggle_label(0), "Belagerung");
    assert_eq!(toggle_label(2), "Belagerung (2)");
    assert_eq!(defense::cell("1", "1"), "1");
    assert_eq!(defense::cell("1", "3"), "1, Plan 3");
    assert_eq!(defense::level_text(2.0), "2");
    assert_eq!(defense::level_text(0.4), "0,4");
    // Der Regler rastet auf halbe Verbände und am Ende auf alles, was bereitsteht.
    assert_eq!(snap(1.2, 4.3), 1.0);
    assert_eq!(snap(1.3, 4.3), 1.5);
    assert_eq!(snap(4.2, 4.3), 4.3);
    assert_eq!(snap(0.0, 4.3), 0.5);
    // Der Befehl nennt nie mehr, als bereitsteht.
    let ready = 4.3_f64;
    assert_eq!(commanded(ready as f32, ready), ready);
    assert_eq!(commanded(1.5, ready), 1.5);
    assert_eq!(
        commanded(1.0, 0.0),
        1.0,
        "ohne Truppen nennt der Kern den Grund"
    );
}

#[test]
fn the_card_is_described_as_the_core_tells_it() {
    let scene = Scene::new(Some(ME));
    let card = scene.card();
    assert_eq!(phase_text(&card), "Phase 3 von 5: Beschuss");
    assert_eq!(state_text(&card), "läuft");
    assert_eq!(Role::of(Some(ME), &card), Role::Attacker);
    assert_eq!(Role::of(Some(card.defender), &card), Role::Defender);
    assert_eq!(Role::of(Some(PolityId(3)), &card), Role::Onlooker);
    assert_eq!(Role::of(None, &card), Role::Onlooker);
    // Ohne Orbitalhoheit ruht die Belagerung, auch wenn das System gesperrt bleibt.
    let resting = SiegeCard {
        control: false,
        blockaded: true,
        ..card.clone()
    };
    assert_eq!(state_text(&resting), RESTING);
    assert_eq!(phase_text(&resting), "Phase 3 von 5: Beschuss");
    // Eine entschiedene Belagerung wartet auf den Besitzwechsel.
    let decided = SiegeCard {
        handover: Some(Outcome::Stormed),
        control: false,
        ..card.clone()
    };
    assert_eq!(
        state_text(&decided),
        "entschieden (gestürmt), Übergabe steht aus"
    );
    // Was die Befehle zuletzt nicht bewirken konnten, steht in den Worten des Kerns da.
    let told: Vec<&str> = card
        .view
        .siege
        .report
        .blocked
        .iter()
        .map(|b| b.label())
        .collect();
    match blocked_text(&card) {
        Some(text) => {
            assert!(!told.is_empty());
            for reason in told {
                assert!(text.contains(reason), "{text}");
            }
        }
        None => assert!(told.is_empty()),
    }
}

#[test]
fn own_sieges_are_always_seen_foreign_ones_only_from_the_same_system() {
    let mut scene = Scene::new(Some(ME));
    let card = scene.card();
    let (attacker, defender, system) = (card.attacker, card.defender, card.system);
    assert_eq!(attacker, ME);
    let see = |scene: &Scene, player| {
        may_see(
            &scene.state.galaxy,
            &scene.state.fleets,
            player,
            attacker,
            defender,
            system,
        )
    };
    assert!(see(&scene, Some(attacker)), "der Belagerer");
    assert!(see(&scene, Some(defender)), "der Verteidiger");
    assert!(see(&scene, None), "Beobachter sehen jede");
    // Ein Dritter ohne Flotte und ohne Welt im System sieht sie nicht.
    let third = scene
        .state
        .polities
        .alive()
        .map(|polity| polity.id)
        .find(|id| {
            *id != attacker
                && *id != defender
                && !scene
                    .state
                    .galaxy
                    .planets_of(*id)
                    .any(|planet| planet.system == system)
                && !scene
                    .state
                    .fleets
                    .docked_in(system)
                    .any(|fleet| fleet.owner == *id)
        })
        .expect("ein Unbeteiligter");
    assert!(!see(&scene, Some(third)));
    assert!(!concerns(Some(third), attacker, defender));
    scene.state.config.player = Some(third);
    let cards = vec![card.clone()];
    assert!(visible(&scene.refs(), &cards).is_empty());
    assert_eq!(concerning(&scene.refs()), 0);
    // Liegt eine seiner Flotten im System, sieht er sie, ohne dass sie ihn angeht.
    let fleet = scene
        .state
        .fleets
        .list
        .iter_mut()
        .find(|fleet| fleet.owner == third && fleet.alive)
        .expect("Flotte des Dritten");
    fleet.system = system;
    fleet.route.clear();
    assert!(see(&scene, Some(third)));
    assert_eq!(visible(&scene.refs(), &cards).len(), 1);
    assert_eq!(concerning(&scene.refs()), 0);
    // Belagerer und Verteidiger geht sie an; Beobachter zählen alle.
    for player in [Some(attacker), Some(defender), None] {
        scene.state.config.player = player;
        assert_eq!(concerning(&scene.refs()), 1, "{player:?}");
    }
}

#[test]
fn the_window_lists_the_siege_and_shows_its_detail() {
    let scene = Scene::new(Some(ME));
    let card = scene.card();
    for (width, height) in [(1600.0, 900.0), (1280.0, 720.0)] {
        let mut ui = open();
        let mut screen = Screen::new(width, height);
        let painted = window(&mut screen, &scene, &mut ui);
        assert_eq!(ui.chosen, Some(card.planet), "die erste ist gewählt");
        for text in [
            TAB_SIEGES,
            TAB_TROOPS,
            card.planet_name.as_str(),
            "3 Beschuss",
            "Phase 3 von 5: Beschuss",
            "1 Anmarsch",
            "2 Einschließung",
            "4 Bodenkrieg",
            "5 Untergrund",
            "Vorräte der Garnison",
            "Anlagen der Welt",
            "Truppen des Belagerers",
            "Truppen der Verteidiger",
            "Flotten im Orbit",
        ] {
            assert!(
                painted.text(text).is_some(),
                "„{text}“ fehlt bei {width}×{height}"
            );
        }
        // Die Vorräte in Tagen, wie die Karte sie nennt.
        assert!(painted.text(&days_text(card.stores_days)).is_some());
        // Nichts ragt aus dem Schirm.
        for (text, rect) in &painted.texts {
            assert!(
                rect.right() <= width + 0.5 && rect.bottom() <= height + 0.5,
                "{text}"
            );
        }
    }
    // Auch im kleinen Fenster stehen die Befehle beim Öffnen da, ohne zu rollen: Reiter
    // und Liste bleiben fest, die Befehle kommen vor Zahlen und Prognose.
    let mut ui = open();
    let mut screen = Screen::new(1280.0, 720.0);
    let painted = window(&mut screen, &scene, &mut ui);
    for text in [TAB_SIEGES, ORDERS_ATTACKER, ASSAULT, LAND, LIFT] {
        let rect = painted
            .text(text)
            .unwrap_or_else(|| panic!("„{text}“ fehlt bei 1280×720"));
        assert!(rect.bottom() <= 720.0, "{text}: {rect:?}");
    }
    let orders = painted.text(ORDERS_ATTACKER).unwrap();
    let numbers = painted.text("Vorräte der Garnison").unwrap();
    assert!(orders.top() < numbers.top(), "Befehle vor den Zahlen");
    // Im großen Fenster stehen Prognose und Befehle ohne Rollen da.
    let mut ui = open();
    let mut screen = Screen::new(1600.0, 900.0);
    let painted = window(&mut screen, &scene, &mut ui);
    assert!(painted.text(FORECAST).is_some());
    for (course, forecast) in &card.forecasts {
        let text = forecast_text(*course, forecast);
        assert!(painted.text(&text).is_some(), "„{text}“ fehlt");
    }
    assert!(painted.text(ORDERS_ATTACKER).is_some());
    assert_eq!(painted.text(ORDERS_DEFENDER), None);
    assert!(painted.text(LIFT).is_some());
}

#[test]
fn a_resting_siege_says_so_and_never_claims_the_world_is_invested() {
    let mut scene = Scene::new(Some(ME));
    let fleets = scene.card().fleets;
    // Ein Entsatz ist eingetroffen: Die Flotte des Belagerers steht im Gefecht.
    for fleet in &mut scene.state.fleets.list {
        if fleets.contains(&fleet.id) {
            fleet.in_battle = true;
        }
    }
    let card = scene.card();
    assert!(!card.control);
    let mut ui = open();
    let mut screen = Screen::new(1600.0, 900.0);
    let painted = window(&mut screen, &scene, &mut ui);
    assert!(
        painted.all(RESTING).len() >= 2,
        "in der Liste und im Einzelnen"
    );
    assert_eq!(painted.text("eingeschlossen"), None);
    // Statt dreier Wege, die der Verlauf widerlegte, steht ein Satz: Die Prognose gilt
    // erst wieder mit der Orbitalhoheit.
    let resting = resting_forecast(&card).expect("ruht");
    assert!(resting.starts_with(limit_text(Limit::NoControl)));
    assert!(resting.contains("gilt erst wieder mit der Orbitalhoheit"));
    assert!(!resting.contains("Truppe am Boden"), "niemand ist gelandet");
    assert!(painted.text(&resting).is_some(), "„{resting}“ fehlt");
    for (course, forecast) in &card.forecasts {
        assert_eq!(painted.text(&forecast_text(*course, forecast)), None);
    }
    // Mit einer Truppe am Boden sagt der Satz, dass sie weiterkämpft und dass die
    // Prognose das nicht vorausrechnet.
    let mut landed = card.clone();
    landed.troops.army = 12.0;
    let text = resting_forecast(&landed).expect("ruht");
    assert!(
        text.contains("Die Truppe am Boden kämpft") && text.contains("rechnet die Prognose nicht")
    );
    // Läuft die Belagerung oder ist sie entschieden, gelten die drei Wege.
    assert_eq!(resting_forecast(&Scene::new(Some(ME)).card()), None);
    let decided = SiegeCard {
        handover: Some(Outcome::Stormed),
        ..card.clone()
    };
    assert_eq!(resting_forecast(&decided), None);
    // Liegt keine Flotte mehr im Orbit, nennt die Zeile keine Füllstände.
    assert_eq!(super::detail::orbit_text(&[], &card), "keine");
    let named = super::detail::orbit_text(&["1. Flotte"], &card);
    assert!(named.starts_with("1. Flotte · Feuerkraft ") && named.contains("Treibstoff"));
}

#[test]
fn the_core_is_asked_once_per_day_and_situation_not_per_frame() {
    let mut scene = Scene::new(Some(ME));
    let mut ui = open();
    let mut screen = Screen::new(1600.0, 900.0);
    window(&mut screen, &scene, &mut ui);
    let asked = ui.cache.asked;
    assert!(asked >= 3, "Karten, Befehle, Landung, Aufheben: {asked}");
    for _ in 0..5 {
        window(&mut screen, &scene, &mut ui);
    }
    assert_eq!(ui.cache.asked, asked, "dieselbe Lage: keine neue Frage");
    // Ein neuer Tag, und der Kern wird neu gefragt.
    scene.state.clock.tick += sim::clock::TICKS_PER_DAY;
    window(&mut screen, &scene, &mut ui);
    assert!(ui.cache.asked > asked);
    // Ebenso, wenn ein Befehl vorgemerkt wird.
    let asked = ui.cache.asked;
    scene.wait(SimCommand::SetTaxRate { rate: 0.1 });
    window(&mut screen, &scene, &mut ui);
    assert!(ui.cache.asked > asked);
}

#[test]
fn the_attacker_changes_orders_and_lifts_only_after_confirming() {
    let mut scene = Scene::new(Some(ME));
    let card = scene.card();
    let orders = card.view.siege.orders;
    let mut ui = open();
    let mut screen = Screen::new(1600.0, 900.0);
    // Ein Klick auf „Sturm“ schickt die stehenden Befehle mit umgelegtem Sturm.
    let actions = click(&mut screen, &scene, &mut ui, ASSAULT);
    let toggled = SiegeOrders {
        assault: !orders.assault,
        ..orders
    };
    let expected = SimCommand::SetSiegeOrders {
        planet: card.planet,
        orders: toggled,
    };
    assert_eq!(commands(&actions), vec![expected.clone()]);
    // Steht die Zeit, gilt das Vorgemerkte schon: Der nächste Klick baut darauf auf.
    scene.wait(expected);
    assert_eq!(effective_orders(&scene.refs(), &card), toggled);
    let actions = click(&mut screen, &scene, &mut ui, super::detail::SACK);
    assert_eq!(
        commands(&actions),
        vec![SimCommand::SetSiegeOrders {
            planet: card.planet,
            orders: SiegeOrders {
                sack: !orders.sack,
                ..toggled
            },
        }]
    );
    // Aufheben fragt erst nach.
    let actions = click(&mut screen, &scene, &mut ui, LIFT);
    assert!(commands(&actions).is_empty());
    assert_eq!(ui.confirm, Some(Confirm::Lift(card.planet)));
    let actions = click(&mut screen, &scene, &mut ui, CANCEL);
    assert!(commands(&actions).is_empty());
    assert_eq!(ui.confirm, None);
    click(&mut screen, &scene, &mut ui, LIFT);
    let actions = click(&mut screen, &scene, &mut ui, CONFIRM_LIFT);
    assert_eq!(
        commands(&actions),
        vec![SimCommand::LiftSiege {
            planet: card.planet
        }]
    );
    assert_eq!(ui.confirm, None);
}

#[test]
fn landing_is_offered_with_what_is_aboard_and_blocked_in_the_words_of_the_core() {
    let scene = Scene::new(Some(ME));
    let card = scene.card();
    let fleet = card.fleets[0];
    let (troops, _) = super::troops::aboard(&scene.refs(), fleet);
    assert!(troops > 0.0, "das Drehbuch hat Truppen eingeschifft");
    let mut ui = open();
    ui.strength = 1.0;
    let mut screen = Screen::new(1600.0, 900.0);
    let land = |strength: f64| SimCommand::LandTroops {
        fleet,
        planet: card.planet,
        strength,
    };
    let actions = click(&mut screen, &scene, &mut ui, LAND);
    let refs = scene.refs();
    match command_block(&siege_refs(&refs), ME, &land(1.0), refs.clock.tick) {
        // Der Kern nähme die Welle an: Der Klick schickt sie.
        None => assert_eq!(commands(&actions), vec![land(1.0)]),
        // Sonst ist der Knopf gesperrt, und der Grund steht daneben.
        Some(reason) => {
            assert!(commands(&actions).is_empty());
            let painted = window(&mut screen, &scene, &mut ui);
            assert!(painted.text(&sentence(reason)).is_some());
        }
    }
    // Mehr als an Bord ist, nennt der Befehl nie; und was der Kern ablehnt, sperrt den Knopf.
    assert_eq!(ui.cache.check(&refs, &land(troops)), Ok(()));
    let too_many = ui.cache.check(&refs, &land(troops + 5.0)).unwrap_err();
    assert!(too_many.contains("Verbände"), "{too_many}");
    assert!(too_many.ends_with('.'));
    // Ein Befehl, der schon wartet, wird nicht noch einmal angeboten.
    let mut scene = scene;
    scene.wait(land(1.0));
    assert_eq!(
        ui.cache.check(&scene.refs(), &land(1.0)),
        Err(crate::ui::fleets::WAITING.to_string())
    );
    // Beobachter geben keine Befehle.
    scene.state.config.player = None;
    assert_eq!(
        ui.cache.check(&scene.refs(), &land(1.0)),
        Err(NO_COMMANDS.to_string())
    );
}

#[test]
fn the_defender_sets_garrison_orders_and_capitulates_only_after_confirming() {
    let attacker_view = Scene::new(Some(ME));
    let card = attacker_view.card();
    let scene = Scene::new(Some(card.defender));
    let mut ui = open();
    let mut screen = Screen::new(1600.0, 900.0);
    let painted = window(&mut screen, &scene, &mut ui);
    assert!(painted.text(ORDERS_DEFENDER).is_some());
    assert_eq!(painted.text(ORDERS_ATTACKER), None);
    assert_eq!(painted.text(LIFT), None);
    assert!(painted.text("eigene Welt unter Belagerung").is_some());
    let defense = scene.state.sieges.defense(card.planet);
    assert_eq!(
        (defense.posture, defense.hold_fire),
        (Posture::Forward, false)
    );
    // Haltung „Tiefe“ und das Feuerverbot gehen als Befehl der Garnison hinaus.
    let actions = click(&mut screen, &scene, &mut ui, Posture::Deep.label());
    assert_eq!(
        commands(&actions),
        vec![SimCommand::SetGarrisonOrders {
            planet: card.planet,
            posture: Posture::Deep,
            hold_fire: false,
        }]
    );
    let actions = click(&mut screen, &scene, &mut ui, super::detail::HOLD_FIRE);
    assert_eq!(
        commands(&actions),
        vec![SimCommand::SetGarrisonOrders {
            planet: card.planet,
            posture: Posture::Forward,
            hold_fire: true,
        }]
    );
    // Die Übergabe: gesperrt mit dem Grund des Kerns, oder erst nach der Rückfrage.
    let refs = scene.refs();
    let surrender = SimCommand::Capitulate {
        planet: card.planet,
    };
    let actions = click(&mut screen, &scene, &mut ui, CAPITULATE);
    assert!(commands(&actions).is_empty());
    match command_block(
        &siege_refs(&refs),
        card.defender,
        &surrender,
        refs.clock.tick,
    ) {
        Some(reason) => {
            assert_eq!(ui.confirm, None);
            let painted = window(&mut screen, &scene, &mut ui);
            assert!(painted.text(&sentence(reason)).is_some());
        }
        None => {
            assert_eq!(ui.confirm, Some(Confirm::Capitulate(card.planet)));
            let actions = click(
                &mut screen,
                &scene,
                &mut ui,
                super::detail::CONFIRM_CAPITULATE,
            );
            assert_eq!(commands(&actions), vec![surrender]);
        }
    }
}

#[test]
fn fleets_begin_sieges_and_carry_troops_through_the_second_tab() {
    let scene = Scene::new(Some(ME));
    let card = scene.card();
    let fleet = card.fleets[0];
    let refs = scene.refs();
    assert_eq!(
        target_text(&refs, fleet),
        Some(format!("belagert {}", card.planet_name))
    );
    assert!(aboard_text(&refs, fleet).contains("Verbände an Bord, Nachschub für"));
    let mut ui = SiegeUi {
        tab: SiegeTab::Troops,
        fleet: Some(fleet),
        ..open()
    };
    let mut screen = Screen::new(1600.0, 900.0);
    let painted = window(&mut screen, &scene, &mut ui);
    assert!(
        painted
            .text(&format!("belagert {}", card.planet_name))
            .is_some()
    );
    // An der belagerten Welt führt ein Knopf zu ihren Einzelheiten.
    click(&mut screen, &scene, &mut ui, SHOW_SIEGE);
    assert_eq!((ui.tab, ui.chosen), (SiegeTab::Sieges, Some(card.planet)));

    // Eine Flotte über der eigenen Hauptwelt: einschiffen und wieder an Land setzen.
    let mut home = Scene::new(Some(ME));
    let capital = home.state.polities.get(ME).capital.expect("Hauptwelt");
    let system = home.state.galaxy.planet(capital).system;
    let ship = home
        .state
        .fleets
        .list
        .iter_mut()
        .find(|f| f.id == fleet)
        .unwrap();
    ship.system = system;
    home.state.sieges.assign.remove(&fleet);
    home.state.sieges.embarked.remove(&fleet);
    let ready = super::troops::ready_regulars(&home.refs(), capital);
    assert!(ready >= 2.0, "Reguläre der Hauptwelt: {ready}");
    let mut ui = SiegeUi {
        tab: SiegeTab::Troops,
        fleet: Some(fleet),
        strength: 2.0,
        ..open()
    };
    ui.supply_days = 30;
    let actions = click(&mut screen, &home, &mut ui, EMBARK);
    assert_eq!(
        commands(&actions),
        vec![SimCommand::EmbarkTroops {
            fleet,
            planet: capital,
            strength: 2.0,
            supply_days: 30
        }]
    );
    // Ohne Truppen an Bord ist das Ausschiffen gesperrt, mit dem Grund des Kerns.
    let refs = home.refs();
    let land = SimCommand::LandTroops {
        fleet,
        planet: capital,
        strength: 2.0,
    };
    let reason = command_block(&siege_refs(&refs), ME, &land, refs.clock.tick).expect("Grund");
    let painted = window(&mut screen, &home, &mut ui);
    assert!(painted.text(&sentence(reason)).is_some());
    let actions = click(&mut screen, &home, &mut ui, DISEMBARK);
    assert!(commands(&actions).is_empty());

    // Über einer fremden Welt ohne Krieg nennt „Belagern“ den Grund des Kerns.
    let mut abroad = Scene::new(Some(ME));
    let neutral = abroad
        .state
        .galaxy
        .planets
        .iter()
        .find(|planet| {
            planet
                .owner
                .is_some_and(|owner| owner != ME && !abroad.state.diplomacy.at_war(ME, owner))
        })
        .map(|planet| (planet.id, planet.system, planet.name.clone()))
        .expect("eine Welt, mit deren Besitzer Frieden herrscht");
    let ship = abroad
        .state
        .fleets
        .list
        .iter_mut()
        .find(|f| f.id == fleet)
        .unwrap();
    ship.system = neutral.1;
    abroad.state.sieges.assign.remove(&fleet);
    let mut ui = SiegeUi {
        tab: SiegeTab::Troops,
        fleet: Some(fleet),
        ..open()
    };
    let label = format!("{BEGIN}: {}", neutral.2);
    let actions = click(&mut screen, &abroad, &mut ui, &label);
    assert!(commands(&actions).is_empty(), "gesperrt");
    let painted = window(&mut screen, &abroad, &mut ui);
    let told = painted
        .texts
        .iter()
        .any(|(text, _)| text.contains("herrscht kein Krieg"));
    assert!(told, "der Grund des Kerns steht neben dem Knopf");
}

#[test]
fn a_fleet_over_an_enemy_world_begins_a_siege() {
    // Die belagernde Flotte gibt ihr Ziel auf; der Knopf bietet es wieder an.
    let mut scene = Scene::new(Some(ME));
    let card = scene.card();
    let fleet = card.fleets[0];
    scene.state.sieges.assign.remove(&fleet);
    let mut ui = SiegeUi {
        tab: SiegeTab::Troops,
        fleet: Some(fleet),
        ..open()
    };
    let mut screen = Screen::new(1600.0, 900.0);
    let label = format!("{BEGIN}: {}", card.planet_name);
    let actions = click(&mut screen, &scene, &mut ui, &label);
    assert_eq!(
        commands(&actions),
        vec![SimCommand::BeginSiege {
            fleet,
            planet: card.planet
        }]
    );
}

#[test]
fn chronicle_entries_about_a_running_siege_lead_to_it() {
    let scene = Scene::new(Some(ME));
    let card = scene.card();
    let refs = scene.refs();
    let cards = vec![card.clone()];
    let linked: Vec<&str> = scene
        .state
        .chronicle
        .entries
        .iter()
        .filter(|entry| siege_of_entry(&refs, &cards, entry) == Some(card.planet))
        .map(|entry| entry.text.as_str())
        .collect();
    assert!(linked.len() >= 2, "Beginn und Phase: {linked:?}");
    assert!(
        linked
            .iter()
            .any(|text| text.contains("beginnt die Belagerung"))
    );
    for text in &linked {
        assert!(text.contains(&card.planet_name), "{text}");
    }
    // Einträge über frühere, beendete Belagerungen führen nirgends hin.
    let earlier = scene
        .state
        .chronicle
        .entries
        .iter()
        .filter(|entry| entry.text.contains("beginnt die Belagerung"))
        .count();
    assert!(earlier > 1, "das Drehbuch hat schon eine Welt genommen");
    assert_eq!(
        scene
            .state
            .chronicle
            .entries
            .iter()
            .filter(|entry| entry.text.contains("beginnt die Belagerung")
                && siege_of_entry(&refs, &cards, entry).is_some())
            .count(),
        1
    );
    // Wer die Belagerung nicht sehen darf, bekommt auch keinen Weg zu ihr.
    let mut stranger = Scene::new(Some(ME));
    let third = stranger
        .state
        .polities
        .alive()
        .map(|polity| polity.id)
        .find(|id| {
            !may_see(
                &stranger.state.galaxy,
                &stranger.state.fleets,
                Some(*id),
                card.attacker,
                card.defender,
                card.system,
            )
        })
        .expect("ein Unbeteiligter");
    stranger.state.config.player = Some(third);
    let refs = stranger.refs();
    assert!(
        stranger
            .state
            .chronicle
            .entries
            .iter()
            .all(|entry| siege_of_entry(&refs, &cards, entry).is_none())
    );
}

/// Zeichnet die Tafel der Verteidigung einer Welt in ein Fenster.
fn editor(
    screen: &mut Screen,
    scene: &Scene,
    ui: &mut SiegeUi,
    planet: PlanetId,
    at: Option<&str>,
) -> (Painted, Vec<UiAction>) {
    let refs = scene.refs();
    let mut actions = Vec::new();
    let mut draw = |root: &mut egui::Ui, actions: &mut Vec<UiAction>| {
        ui.cache.refresh(&refs);
        egui::Window::new("Probe")
            .default_width(520.0)
            .show(root.ctx(), |inner| {
                defense::defense_editor(inner, &refs, planet, ui, actions);
            });
    };
    let mut painted = Painted::default();
    for _ in 0..4 {
        painted = screen.settle(|root| draw(root, &mut Vec::new()));
    }
    if let Some(label) = at {
        let at = painted
            .text(label)
            .unwrap_or_else(|| panic!("„{label}“ steht nicht in der Tafel"))
            .center();
        painted = screen.click(at, |root| draw(root, &mut actions));
    }
    (painted, actions)
}

#[test]
fn the_defence_panel_shows_the_card_of_the_core_and_sends_the_changed_plan() {
    let scene = Scene::new(Some(ME));
    let refs = scene.refs();
    let capital = scene.state.polities.get(ME).capital.expect("Hauptwelt");
    let card = defense_card(&siege_refs(&refs), capital).expect("Karte");
    let mut ui = SiegeUi::default();
    let mut screen = Screen::new(1600.0, 900.0);
    let (painted, _) = editor(&mut screen, &scene, &mut ui, capital, None);
    for works in Works::ALL {
        assert!(painted.text(works.label()).is_some(), "{}", works.label());
        // In der Zeile der Anlage: was steht, die höchste Stufe und der Ausbaustand.
        let row = painted.text(works.label()).unwrap().center().y;
        let cells: Vec<&str> = painted
            .texts
            .iter()
            .filter(|(_, rect)| (rect.center().y - row).abs() < 4.0)
            .map(|(text, _)| text.as_str())
            .collect();
        let level = defense::level_text(works.level(&card.defense));
        let cap = card.level_caps[works.index()].to_string();
        let progress = defense::progress_text(&card, works);
        for wanted in [level.as_str(), cap.as_str(), progress.as_str()] {
            assert!(
                cells.contains(&wanted),
                "{}: „{wanted}“ fehlt in {cells:?}",
                works.label()
            );
        }
    }
    for text in [
        defense::REGULARS.to_string(),
        defense::DEPOT.to_string(),
        days_text(card.defense.depot),
        format!("{} Tage", card.depot_days_cap),
        units(card.regulars_cap),
        defense::militia_text(&card),
        format!(
            "Eine besetzte Welt dieser Größe hält eine Garnison von {} Verbänden ruhig.",
            units(card.hold_need)
        ),
    ] {
        assert!(painted.text(&text).is_some(), "„{text}“ fehlt");
    }
    let costs = |upkeep: f64, plan: f64, changed: bool| {
        format!(
            "Unterhalt im Monat: heute {} Credits · nach {} {} Credits",
            crate::ui::format::grouped(upkeep),
            if changed {
                "dem geänderten Plan"
            } else {
                "Plan"
            },
            crate::ui::format::grouped(plan)
        )
    };
    assert!(
        painted
            .text(&costs(card.upkeep, card.plan_upkeep, false))
            .is_some()
    );
    // Solange nichts geändert ist, gibt es nichts festzusetzen.
    let (_, actions) = editor(&mut screen, &scene, &mut ui, capital, Some(SET_PLAN));
    assert!(commands(&actions).is_empty());
    assert_eq!(
        ui.cache.check(
            &refs,
            &SimCommand::SetDefensePlan {
                planet: capital,
                plan: card.plan.clone()
            }
        ),
        Ok(())
    );
    assert!(PLAN_IN_FORCE.ends_with('.'));

    // Ein Geschütz mehr: Die Tafel nennt Unterhalt und Kosten des geänderten Plans, wie
    // der Kern sie nennte, und der Klick schickt genau diesen Plan.
    let more = defense::with_level(card.plan.clone(), Works::Guns, card.plan.guns + 1);
    assert!(
        more.guns <= card.level_caps[Works::Guns.index()],
        "die Technik erlaubt es"
    );
    let preview = defense::preview_card(&refs, capital, &more).expect("Vorschau");
    assert!(preview.plan_upkeep > card.plan_upkeep && preview.build_cost > card.build_cost);
    ui.plan_edit = Some((capital, more.clone()));
    let (painted, actions) = editor(&mut screen, &scene, &mut ui, capital, Some(SET_PLAN));
    assert!(
        painted
            .text(&costs(card.upkeep, preview.plan_upkeep, true))
            .is_some()
    );
    assert_eq!(
        commands(&actions),
        vec![SimCommand::SetDefensePlan {
            planet: capital,
            plan: more.clone()
        }]
    );
    assert_eq!(
        ui.plan_edit,
        Some((capital, more)),
        "bleibt stehen, bis der Plan gilt"
    );
    // „Verwerfen“ stellt den geltenden Plan wieder her.
    editor(
        &mut screen,
        &scene,
        &mut ui,
        capital,
        Some(defense::DISCARD),
    );
    assert_eq!(ui.plan_edit, None);

    // Was der Kern ablehnte, lässt sich nicht festsetzen; sein Grund steht da.
    let beyond = defense::with_level(card.plan.clone(), Works::Stations, 200);
    ui.plan_edit = Some((capital, beyond.clone()));
    let (painted, actions) = editor(&mut screen, &scene, &mut ui, capital, Some(SET_PLAN));
    assert!(commands(&actions).is_empty());
    let command = SimCommand::SetDefensePlan {
        planet: capital,
        plan: beyond,
    };
    let reason = command_block(&siege_refs(&refs), ME, &command, refs.clock.tick).expect("Grund");
    assert!(painted.text(&sentence(reason)).is_some());
}

#[test]
fn the_overview_sums_what_the_cards_say() {
    let scene = Scene::new(Some(ME));
    let refs = scene.refs();
    let cards: Vec<_> = scene
        .state
        .galaxy
        .planets_of(ME)
        .filter_map(|planet| defense_card(&siege_refs(&refs), planet.id))
        .collect();
    assert!(cards.len() > 2);
    let (upkeep, plan_upkeep, build_cost) = defense::totals(&cards);
    assert!((upkeep - cards.iter().map(|card| card.upkeep).sum::<f64>()).abs() < 1e-6);
    assert!(plan_upkeep >= upkeep - 1e-6 || build_cost == 0.0);
    let mut ui = SiegeUi::default();
    let mut screen = Screen::new(1600.0, 900.0);
    let mut draw = |root: &mut egui::Ui| {
        ui.cache.refresh(&refs);
        egui::Window::new("Probe")
            .default_width(600.0)
            .default_height(860.0)
            .vscroll(true)
            .show(root.ctx(), |inner| {
                defense::defense_overview(inner, &refs, ME, &mut ui, &mut Vec::new());
            });
    };
    for _ in 0..4 {
        screen.settle(&mut draw);
    }
    let painted = screen.settle(&mut draw);
    assert!(painted.text(defense::HEADING).is_some());
    assert!(
        painted.text(SET_PLAN).is_some(),
        "die Tafel der gewählten Welt"
    );
    // Die Hauptwelt führt die Liste an.
    let capital = scene.state.polities.get(ME).capital.expect("Hauptwelt");
    let first = &scene.state.galaxy.planet(capital).name;
    assert!(
        painted.all(first).len() >= 2,
        "in der Liste und über der Tafel"
    );
}

/// Schreibt die Partie als Spielstand, den nur dieser Testlauf kennt.
fn save_file(name: &str, player: PolityId) -> std::path::PathBuf {
    let (data, state) = bombardment();
    let mut state = state.clone();
    state.config = SimConfig {
        player: Some(player),
        ..state.config
    };
    let path =
        std::env::temp_dir().join(format!("konfluenz_test_{}_{name}.ron", std::process::id()));
    SaveGame {
        format: SAVE_FORMAT,
        data_hash: data.hash,
        state,
    }
    .write(&path)
    .expect("Spielstand schreiben");
    path
}

#[test]
fn a_player_finds_the_siege_from_the_top_bar_the_chronicle_and_the_fleet_window() {
    let path = save_file("belagerung", ME);
    let mut game = TestGame::start(&format!("--load {}", path.display()));
    game.frames(6);
    let _ = std::fs::remove_file(&path);
    let planet = *game
        .resource::<Sieges>()
        .active
        .keys()
        .next()
        .expect("Belagerung");
    let name = game
        .resource::<sim::galaxy::Galaxy>()
        .planet(planet)
        .name
        .clone();
    assert!(game.window_rect(TITLE).is_none());
    // Die Kopfleiste nennt die Zahl der eigenen Belagerungen; ein Klick öffnet das Fenster.
    game.click_on(&toggle_label(1));
    game.frames(3);
    assert!(game.window_rect(TITLE).is_some());
    assert!(game.text("Phase 3 von 5: Beschuss").is_some());
    assert!(game.text(ORDERS_ATTACKER).is_some());
    // Schließen, und über die Chronik wieder hin.
    game.click_on(&toggle_label(1));
    // Das Fenster blendet sich einige Frames lang aus.
    game.frames(10);
    assert!(game.window_rect(TITLE).is_none());
    game.click_on(SHOW_SIEGE);
    game.frames(3);
    assert!(game.window_rect(TITLE).is_some());
    assert_eq!(
        game.resource::<crate::ui::UiState>().siege.chosen,
        Some(planet)
    );
    // Das Fenster „Flotten“ nennt das Ziel der Flotte.
    game.click_on(crate::ui::fleets::TITLE);
    game.frames(3);
    assert!(game.text(&format!("belagert {name}")).is_some());
    game.click_on(crate::ui::fleets::TITLE);
    game.frames(10);
    // Plünderung befehlen: Bei stehender Zeit ist der Befehl vorgemerkt, mit dem
    // nächsten Takt gilt er.
    let before = game.resource::<Sieges>().siege(planet).unwrap().orders;
    let sack = game.checkbox(super::detail::SACK).expect("Ankreuzfeld");
    game.click(sack);
    assert_eq!(game.resource::<CommandQueue>().pending.len(), 1);
    game.world_mut().resource_mut::<SimSpeed>().paused = false;
    game.frames(8);
    game.world_mut().resource_mut::<SimSpeed>().paused = true;
    game.frames(2);
    let after = game.resource::<Sieges>().siege(planet).unwrap().orders;
    assert_eq!(
        after,
        SiegeOrders {
            sack: !before.sack,
            ..before
        }
    );
    assert_ne!(after.fire, Fire::Hold, "das Drehbuch lässt schießen");
    // Aufheben nach Rückfrage: Die Belagerung endet, die Zahl verschwindet.
    game.click_on(LIFT);
    assert!(
        game.resource::<CommandQueue>().pending.is_empty(),
        "erst die Rückfrage"
    );
    game.click_on(CONFIRM_LIFT);
    game.world_mut().resource_mut::<SimSpeed>().paused = false;
    game.frames(8);
    game.world_mut().resource_mut::<SimSpeed>().paused = true;
    game.frames(3);
    assert!(game.resource::<Sieges>().active.is_empty());
    assert!(game.button(&toggle_label(0)).is_some());
    assert!(game.text(NONE_IN_SIGHT).is_some());
}

#[test]
fn the_state_window_shows_defence_and_its_budget_line() {
    let path = save_file("verteidigung", ME);
    let mut game = TestGame::start(&format!("--load {}", path.display()));
    game.frames(6);
    let _ = std::fs::remove_file(&path);
    game.click_on("Staat");
    game.frames(4);
    assert!(
        game.text(&crate::ui::budget::claim_label(
            sim::economy::Claim::Defense
        ))
        .is_some()
    );
    assert!(game.text("4. Verteidigung").is_some());
    assert!(game.button(&format!("{SET_PLAN}: ")).is_none());
    // Die Übersicht nennt jede eigene Welt; die Tafel der ersten steht darunter.
    let worlds: Vec<String> = {
        let galaxy = game.resource::<sim::galaxy::Galaxy>();
        galaxy
            .planets_of(ME)
            .map(|planet| planet.name.clone())
            .collect()
    };
    assert!(worlds.len() > 2);
    assert!(game.text(defense::REGULARS).is_some());
    let capital = game
        .resource::<sim::polity::Polities>()
        .get(ME)
        .capital
        .unwrap();
    let capital = game
        .resource::<sim::galaxy::Galaxy>()
        .planet(capital)
        .name
        .clone();
    assert!(worlds.contains(&capital));
    assert!(
        game.disabled_button(&format!("{SET_PLAN}: {capital}"))
            .is_some()
    );
}

#[test]
fn build_progress_counts_delivered_sets_against_the_next_level() {
    let scene = Scene::new(Some(ME));
    let capital = scene.state.polities.get(ME).capital.expect("Hauptwelt");
    let mut card = defense_card(&siege_refs(&scene.refs()), capital).expect("Karte");
    let guns = Works::Guns;
    card.defense.guns = 1.0;
    card.plan.guns = 1;
    assert_eq!(defense::progress_text(&card, guns), "erfüllt");
    // Stufe 2 braucht zwei Sätze; was geliefert ist, zählt dagegen.
    card.plan.guns = 3;
    card.defense.delivered[guns.index()] = 1.3;
    assert_eq!(defense::progress_text(&card, guns), "1,3 von 2 Sätzen");
    card.defense.guns = 0.0;
    card.defense.delivered[guns.index()] = 0.5;
    assert_eq!(defense::progress_text(&card, guns), "0,5 von 1 Satz");
}

#[test]
fn the_defence_panel_does_not_widen_the_left_panel() {
    use crate::map::Selection;
    use crate::ui::UiState;
    use crate::ui::panels::in_game;

    let scene = Scene::new(Some(ME));
    let refs = scene.refs();
    let capital = scene.state.polities.get(ME).capital.expect("Hauptwelt");
    let home = scene.state.galaxy.planet(capital).system;
    let abroad = scene.card().system;
    let speed = SimSpeed::default();
    // Rechter Rand der linken Tafel bei dieser Auswahl.
    let edge = |system, planet| {
        let selection = Selection {
            system: Some(system),
            planet,
        };
        let mut state = UiState::default();
        let mut screen = Screen::new(1600.0, 900.0);
        let mut draw = |root: &mut egui::Ui| {
            in_game(
                root,
                &refs,
                &mut state,
                &speed,
                &selection,
                &[],
                &mut Vec::new(),
            )
        };
        for _ in 0..3 {
            screen.settle(&mut draw);
        }
        let painted = screen.settle(&mut draw);
        let shown = painted.text(SET_PLAN).is_some();
        let right = painted
            .rects
            .iter()
            .filter(|(rect, _)| {
                rect.left() <= 0.0 && rect.height() >= 300.0 && rect.width() < 800.0
            })
            .map(|(rect, _)| rect.right())
            .fold(0.0, f32::max);
        (right, shown)
    };
    let (foreign, shown) = edge(abroad, None);
    assert!(!shown, "fremde Welten zeigen keinen Plan");
    let (own, shown) = edge(home, Some(capital));
    assert!(shown, "die eigene Welt zeigt ihren Plan");
    assert!(foreign > 300.0);
    assert_eq!(
        own, foreign,
        "die Tafel der Verteidigung macht die linke Tafel breiter"
    );
}

#[test]
fn names_are_matched_as_whole_words() {
    assert!(names("Zeloth I ist gestürmt.", "Zeloth I"));
    assert!(names("Über Zeloth I: Phase 3.", "Zeloth I"));
    assert!(names("Belagerung von Zeloth I", "Zeloth I"));
    assert!(!names(
        "Meldung über Zeloth II, eine andere Welt.",
        "Zeloth I"
    ));
    assert!(!names("AltZeloth I fällt", "Zeloth I"));
    assert!(
        names("Zeloth II und Zeloth I", "Zeloth I"),
        "ein späterer Treffer zählt"
    );
    assert!(!names("irgendetwas", ""));

    // Ein Eintrag über die Nachbarwelt „… II“ führt nicht zur Belagerung von „… I“, und
    // einer aus der Zeit vor ihrem Beginn auch nicht.
    let scene = Scene::new(Some(ME));
    let card = scene.card();
    let refs = scene.refs();
    let cards = vec![card.clone()];
    let entry = scene
        .state
        .chronicle
        .entries
        .iter()
        .find(|entry| siege_of_entry(&refs, &cards, entry) == Some(card.planet))
        .expect("ein Eintrag der Belagerung")
        .clone();
    let neighbour = ChronicleEntry {
        text: format!("Meldung über {}I, eine andere Welt.", card.planet_name),
        ..entry.clone()
    };
    assert!(neighbour.text.contains(&card.planet_name));
    assert_eq!(siege_of_entry(&refs, &cards, &neighbour), None);
    let earlier = ChronicleEntry {
        tick: (scene.state.clock.day() - card.days - 5) * TICKS_PER_DAY,
        ..entry.clone()
    };
    assert_eq!(
        siege_of_entry(&refs, &cards, &earlier),
        None,
        "eine frühere Belagerung derselben Welt"
    );
    let elsewhere = ChronicleEntry {
        system: Some(SystemId(card.system.0 + 1)),
        ..entry.clone()
    };
    assert_eq!(siege_of_entry(&refs, &cards, &elsewhere), None);
    assert_eq!(siege_of_entry(&refs, &cards, &entry), Some(card.planet));
}

#[test]
fn own_sieges_come_first_and_a_new_game_forgets_the_old_choices() {
    let scene = Scene::new(Some(ME));
    let card = scene.card();
    // Eine fremde Belagerung im selben System (dort liegt die eigene Flotte) steht in
    // der Liste des Kerns vor der eigenen; die Oberfläche zeigt die eigene zuerst.
    let others: Vec<PolityId> = scene
        .state
        .polities
        .alive()
        .map(|polity| polity.id)
        .filter(|id| *id != card.attacker && *id != card.defender)
        .take(2)
        .collect();
    let foreign = SiegeCard {
        planet: PlanetId(card.planet.0 + 1),
        attacker: others[0],
        defender: others[1],
        ..card.clone()
    };
    let cards = vec![foreign.clone(), card.clone()];
    let seen = visible(&scene.refs(), &cards);
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].planet, card.planet, "die eigene zuerst");
    assert_eq!(seen[1].planet, foreign.planet);

    // Was nur für die alte Partie galt, überlebt den Wechsel nicht: gewählte Welt und
    // Flotte, ein angefangener Plan, eine offene Rückfrage, Hinweise.
    let mut ui = SiegeUi {
        open: true,
        tab: SiegeTab::Troops,
        chosen: Some(card.planet),
        fleet: Some(card.fleets[0]),
        strength: 7.5,
        supply_days: 90,
        confirm: Some(Confirm::Lift(card.planet)),
        plan_planet: Some(card.planet),
        plan_edit: Some((card.planet, DefensePlan::default())),
        embark_note: Some(super::troops::EmbarkNote {
            fleet: card.fleets[0],
            troops: 1.0,
            text: "alt".into(),
        }),
        ..Default::default()
    };
    ui.forget_game();
    assert!(ui.open, "ob das Fenster offen ist, bleibt");
    assert_eq!((ui.chosen, ui.fleet, ui.confirm), (None, None, None));
    assert_eq!((ui.plan_planet, ui.plan_edit.clone()), (None, None));
    assert_eq!(ui.embark_note, None);
    assert_eq!(ui.tab, SiegeTab::Sieges);
    assert_eq!((ui.strength, ui.supply_days), (1.0, 30));
}

#[test]
fn a_waiting_garrison_order_is_the_base_of_the_next_click() {
    let card = Scene::new(Some(ME)).card();
    let mut scene = Scene::new(Some(card.defender));
    let mut ui = open();
    let mut screen = Screen::new(1600.0, 900.0);
    // Bei stehender Zeit ist „Tiefe“ schon vorgemerkt: Der Klick auf das Feuerverbot
    // nimmt die Haltung nicht zurück.
    scene.wait(SimCommand::SetGarrisonOrders {
        planet: card.planet,
        posture: Posture::Deep,
        hold_fire: false,
    });
    assert_eq!(
        super::detail::effective_garrison(&scene.refs(), card.planet),
        (Posture::Deep, false)
    );
    let actions = click(&mut screen, &scene, &mut ui, super::detail::HOLD_FIRE);
    assert_eq!(
        commands(&actions),
        vec![SimCommand::SetGarrisonOrders {
            planet: card.planet,
            posture: Posture::Deep,
            hold_fire: true,
        }]
    );
    // Ohne Vorgemerktes gilt, was der Kern führt; Befehle für andere Welten zählen nicht.
    scene.pending.clear();
    scene.wait(SimCommand::SetGarrisonOrders {
        planet: PlanetId(card.planet.0 + 1),
        posture: Posture::Deep,
        hold_fire: true,
    });
    assert_eq!(
        super::detail::effective_garrison(&scene.refs(), card.planet),
        (Posture::Forward, false)
    );
}

#[test]
fn landing_and_embarking_are_locked_with_the_reason_of_the_core() {
    // Ohne Truppen an Bord lehnt der Kern die Landung ab: Der Knopf tut nichts, und sein
    // Grund steht daneben, mit deutschen Zahlen.
    let mut scene = Scene::new(Some(ME));
    let card = scene.card();
    let fleet = card.fleets[0];
    scene.state.sieges.embarked.remove(&fleet);
    let mut ui = open();
    let mut screen = Screen::new(1600.0, 900.0);
    let land = SimCommand::LandTroops {
        fleet,
        planet: card.planet,
        strength: 1.0,
    };
    let refs = scene.refs();
    let reason = command_block(&siege_refs(&refs), ME, &land, refs.clock.tick).expect("Grund");
    let actions = click(&mut screen, &scene, &mut ui, LAND);
    assert!(commands(&actions).is_empty(), "gesperrt: {actions:?}");
    let painted = window(&mut screen, &scene, &mut ui);
    let told = sentence(reason.clone());
    assert!(painted.text(&told).is_some(), "„{told}“ fehlt");
    assert!(
        reason.contains("0.0") && told.contains("0,0"),
        "{reason} / {told}"
    );

    // Eine Welle wartet schon: Auch mit anderer Stärke am Regler gibt es keine zweite.
    let mut scene = Scene::new(Some(ME));
    scene.wait(SimCommand::LandTroops {
        fleet,
        planet: card.planet,
        strength: 1.0,
    });
    let mut ui = open();
    ui.strength = 2.0;
    let actions = click(&mut screen, &scene, &mut ui, LAND);
    assert!(commands(&actions).is_empty(), "eine Welle wartet schon");
    let painted = window(&mut screen, &scene, &mut ui);
    assert!(painted.text(crate::ui::fleets::WAITING).is_some());
    assert!(super::troops::move_waiting(&scene.refs(), fleet));
    assert!(!super::troops::move_waiting(
        &scene.refs(),
        FleetId(fleet.0 + 77)
    ));

    // Stehen auf der Hauptwelt keine Regulären bereit, lehnt der Kern das Einschiffen ab.
    let mut home = Scene::new(Some(ME));
    let capital = home.state.polities.get(ME).capital.expect("Hauptwelt");
    let system = home.state.galaxy.planet(capital).system;
    let ship = home
        .state
        .fleets
        .list
        .iter_mut()
        .find(|f| f.id == fleet)
        .unwrap();
    ship.system = system;
    home.state.sieges.assign.remove(&fleet);
    home.state.sieges.embarked.remove(&fleet);
    home.state.sieges.defense_mut(capital).regulars = 0.0;
    let mut ui = SiegeUi {
        tab: SiegeTab::Troops,
        fleet: Some(fleet),
        strength: 2.0,
        ..open()
    };
    let actions = click(&mut screen, &home, &mut ui, EMBARK);
    assert!(commands(&actions).is_empty(), "gesperrt: {actions:?}");
    assert_eq!(ui.embark_asked, None);
    let painted = window(&mut screen, &home, &mut ui);
    let told = painted
        .texts
        .iter()
        .any(|(text, _)| text.contains("reguläre Verbände bereit."));
    assert!(told, "der Grund des Kerns steht neben dem Knopf");
}

/// Eine Flotte der ersten Gemeinschaft über ihrer Hauptwelt, ohne Auftrag und ohne
/// Truppen an Bord.
fn at_home() -> (Scene, FleetId, PlanetId) {
    let mut home = Scene::new(Some(ME));
    let fleet = home.card().fleets[0];
    let capital = home.state.polities.get(ME).capital.expect("Hauptwelt");
    let system = home.state.galaxy.planet(capital).system;
    let ship = home
        .state
        .fleets
        .list
        .iter_mut()
        .find(|f| f.id == fleet)
        .unwrap();
    ship.system = system;
    home.state.sieges.assign.remove(&fleet);
    home.state.sieges.embarked.remove(&fleet);
    (home, fleet, capital)
}

#[test]
fn embarking_says_beforehand_how_much_supply_there_is_and_afterwards_what_came_aboard() {
    use super::troops::{
        EmbarkAsked, OfferKey, SupplyLimit, offer_text, short_supply_text, supply_limit,
        supply_limits, supply_offer,
    };
    let (mut home, fleet, capital) = at_home();
    let system = home.state.galaxy.planet(capital).system;
    let key = OfferKey {
        fleet,
        planet: capital,
        strength: 2.0,
        supply_days: 105,
    };
    // Was der Probelauf des Kerns nennt, ist das Kleinste aus Wunsch, Etat und Markt:
    // Die Begründung der Tafel rechnet mit denselben Zahlen.
    let refs = home.refs();
    let offer = supply_offer(&refs, key).expect("der Kern schifft ein");
    let (affordable, available) = supply_limits(&refs, ME, system);
    let expected = (key.strength * 105.0).min(affordable).min(available);
    assert!(
        (offer - expected).abs() <= 1e-6 * expected.max(1.0),
        "Probelauf {offer}, nachgerechnet {expected}"
    );
    // Der Probelauf ändert nichts an der Partie.
    assert_eq!(super::troops::aboard(&refs, fleet), (0.0, 0.0));
    // Was der Kern ablehnte, hat kein Angebot.
    let refused = OfferKey {
        strength: 10_000.0,
        ..key
    };
    assert_eq!(supply_offer(&refs, refused), None);

    // Ein leerer Markt: nichts zu haben, und die Tafel sagt es vor dem Klick.
    for (good, _) in &home.data.balance.siege.day_bundle_ids.clone() {
        home.state.economy.markets[system.index()].stock[good.index()] = 0.0;
    }
    let refs = home.refs();
    assert_eq!(supply_offer(&refs, key), Some(0.0));
    assert_eq!(supply_limit(&refs, ME, system), SupplyLimit::Market);
    let mut ui = SiegeUi {
        tab: SiegeTab::Troops,
        fleet: Some(fleet),
        strength: 2.0,
        supply_days: 105,
        ..open()
    };
    let mut screen = Screen::new(1600.0, 900.0);
    let painted = window(&mut screen, &home, &mut ui);
    let name = &home.state.galaxy.system(system).name;
    let warning = offer_text(2.0, 105, 0.0, SupplyLimit::Market, name).unwrap_err();
    assert!(warning.starts_with("Heute gäbe es nur Nachschub für 0 Tage statt für 105 Tage."));
    assert!(warning.contains(name.as_str()));
    assert!(painted.text(&warning).is_some(), "„{warning}“ fehlt");
    // Dieselbe Stellung der Regler fragt den Kern nicht in jedem Frame neu.
    let asked = ui.cache.asked;
    window(&mut screen, &home, &mut ui);
    assert_eq!(ui.cache.asked, asked);

    // Der Klick schifft trotzdem ein; die Tafel merkt sich, wonach sie sehen muss.
    let actions = click(&mut screen, &home, &mut ui, EMBARK);
    assert_eq!(commands(&actions), vec![key.command()]);
    assert_eq!(
        ui.embark_asked,
        Some(EmbarkAsked {
            key,
            before: (0.0, 0.0),
            tick: home.state.clock.tick
        })
    );
    // Solange der Befehl wartet, gibt es nichts zu melden.
    home.wait(key.command());
    assert_eq!(ui.follow_embark(&home.refs()), None);
    assert!(ui.embark_asked.is_some());
    // Der Kern führt ihn aus: Verbände an Bord, kein Nachschub. Jetzt kommt der Hinweis,
    // einmal als Einblendung und bleibend an der Flotte.
    home.pending.clear();
    home.state.clock.tick += 1;
    home.state.sieges.embarked.entry(fleet).or_default().troops = 2.0;
    let note = ui.follow_embark(&home.refs()).expect("Hinweis");
    assert_eq!(
        note,
        "Eingeschifft: 2,0 Verbände, aber nur mit Nachschub für 0 Tage statt für 105 Tage. \
         Markt und Etat gaben nicht mehr her."
    );
    assert_eq!(ui.embark_asked, None);
    assert_eq!(ui.follow_embark(&home.refs()), None, "nur einmal");
    let painted = window(&mut screen, &home, &mut ui);
    assert!(
        painted.text(&note).is_some(),
        "der Hinweis steht an der Flotte"
    );
    // Ändert sich, was an Bord ist, ist der Hinweis überholt.
    home.state.sieges.embarked.entry(fleet).or_default().troops = 1.0;
    ui.follow_embark(&home.refs());
    assert_eq!(ui.embark_note, None);

    // Kam alles Verlangte an Bord, gibt es keinen Hinweis; ebenso ohne Nachschubwunsch.
    assert_eq!(short_supply_text(2.0, 105.0, 105), None);
    assert_eq!(short_supply_text(2.0, 104.6, 105), None);
    assert_eq!(short_supply_text(2.0, 0.0, 0), None);
    assert!(
        short_supply_text(2.0, 40.0, 105)
            .unwrap()
            .contains("für 40 Tage statt für 105 Tage")
    );
    assert_eq!(
        offer_text(2.0, 30, 60.0, SupplyLimit::Budget, "X"),
        Ok("Nachschub für 30 Tage ist heute zu haben.".to_string())
    );
    assert!(
        offer_text(2.0, 30, 20.0, SupplyLimit::Budget, "X")
            .unwrap_err()
            .contains("Etat für Verteidigung und Beschaffung")
    );
    // Ein abgelehnter Befehl hinterlässt nichts: Er wartet nicht mehr, nichts kam an Bord.
    let (home, fleet, _) = at_home();
    let mut ui = open();
    ui.embark_asked = Some(EmbarkAsked {
        key: OfferKey { fleet, ..key },
        before: (0.0, 0.0),
        tick: home.state.clock.tick - 5,
    });
    assert_eq!(ui.follow_embark(&home.refs()), None);
    assert_eq!(ui.embark_asked, None);
}

#[test]
fn a_fleet_waiting_for_the_orbit_says_why_and_can_drop_the_task() {
    use super::troops::{CANCEL_TASK, WAITS_FOR_ORBIT, waiting_reason};
    // Die Flotte hat den Auftrag, aber die Belagerung hat (noch) nicht begonnen.
    let mut scene = Scene::new(Some(ME));
    let card = scene.card();
    let fleet = card.fleets[0];
    scene.state.sieges.active.remove(&card.planet);
    let ship = |scene: &Scene| scene.state.fleets.get(fleet).expect("Flotte").clone();
    // Hält sie den Orbit, beginnt die Belagerung mit dem nächsten Tag.
    let reason = waiting_reason(&scene.refs(), &ship(&scene), card.planet);
    assert!(reason.contains("beginnt mit dem nächsten Tag"), "{reason}");
    // Ohne Treibstoff hält sie keine Position: Das steht da, mit dem Weg zurück.
    for vessel in &mut scene
        .state
        .fleets
        .list
        .iter_mut()
        .find(|f| f.id == fleet)
        .unwrap()
        .ships
    {
        vessel.fuel = 0.0;
    }
    let reason = waiting_reason(&scene.refs(), &ship(&scene), card.planet);
    assert!(reason.contains("keinen Treibstoff"), "{reason}");
    let mut ui = SiegeUi {
        tab: SiegeTab::Troops,
        fleet: Some(fleet),
        ..open()
    };
    let mut screen = Screen::new(1600.0, 900.0);
    let painted = window(&mut screen, &scene, &mut ui);
    assert!(painted.text(WAITS_FOR_ORBIT).is_some());
    assert!(painted.text(&reason).is_some(), "„{reason}“ fehlt");
    let actions = click(&mut screen, &scene, &mut ui, CANCEL_TASK);
    assert_eq!(
        commands(&actions),
        vec![SimCommand::LiftSiege {
            planet: card.planet
        }]
    );
    // Im Gefecht und beim Sammeln nennt der Grund eben das.
    let mut fighting = Scene::new(Some(ME));
    fighting.state.sieges.active.remove(&card.planet);
    let vessel = fighting
        .state
        .fleets
        .list
        .iter_mut()
        .find(|f| f.id == fleet)
        .unwrap();
    vessel.in_battle = true;
    let reason = waiting_reason(&fighting.refs(), &ship(&fighting), card.planet);
    assert!(reason.contains("steht im Gefecht"), "{reason}");
    let vessel = fighting
        .state
        .fleets
        .list
        .iter_mut()
        .find(|f| f.id == fleet)
        .unwrap();
    vessel.in_battle = false;
    vessel.regroup_until = Some(u64::MAX);
    let reason = waiting_reason(&fighting.refs(), &ship(&fighting), card.planet);
    assert!(reason.contains("sammelt sich"), "{reason}");
}

#[test]
fn progress_underground_does_not_read_as_finished() {
    let card = Scene::new(Some(ME)).card();
    // Vor dem Untergrund gilt die Zahl des Kerns.
    let shown = progress_of(&card);
    assert_eq!(shown.share, card.progress as f32);
    assert_eq!(shown.text, crate::ui::format::percent(card.progress as f32));
    // Im Untergrund ist die Oberfläche genommen und die Zahl des Kerns voll, während die
    // Welt noch hält: Gezeigt wird, was von der Garnison nicht mehr steht.
    let mut deep = card.clone();
    deep.phase = sim::siege::Phase::Underground;
    deep.phase_number = 5;
    deep.progress = 1.0;
    deep.troops.militia = 0.0;
    deep.troops.reserve = 0.0;
    deep.troops.deep = 4.6;
    deep.troops.garrison_start = 23.0;
    let shown = progress_of(&deep);
    assert!((shown.share - 0.8).abs() < 1e-6, "{shown:?}");
    assert_eq!(
        shown.text,
        "Oberfläche genommen · Garnison zu 80 % gebrochen · im Untergrund noch 4,6 Verbände"
    );
    assert!(!shown.text.contains("100 %"));
    // Eine genommene Zone ist für den Belagerer ein Erfolg, für den Verteidiger ein
    // Verlust.
    use super::detail::zone_state;
    assert_eq!(zone_state(Role::Attacker, true, false), ("genommen", GOOD));
    assert_eq!(zone_state(Role::Defender, true, false), ("genommen", BAD));
    assert_eq!(zone_state(Role::Onlooker, true, true).0, "genommen");
    assert_eq!(zone_state(Role::Attacker, false, true), ("umkämpft", WARN));
    assert_eq!(zone_state(Role::Defender, false, false).0, "gehalten");
}

#[test]
fn a_question_nobody_sees_any_more_expires() {
    use super::detail::{ASK_CAPITULATE, ASK_LAST_WORLD, ASK_LIFT, capitulate_question};
    let scene = Scene::new(Some(ME));
    let card = scene.card();
    let mut ui = open();
    let mut screen = Screen::new(1600.0, 900.0);
    click(&mut screen, &scene, &mut ui, LIFT);
    assert_eq!(ui.confirm, Some(Confirm::Lift(card.planet)));
    // Solange das Fenster gezeichnet wird, bleibt die Rückfrage stehen.
    let painted = window(&mut screen, &scene, &mut ui);
    assert!(painted.text(ASK_LIFT).is_some());
    assert_eq!(ui.confirm, Some(Confirm::Lift(card.planet)));
    // Einige Frames ohne das Fenster (geschlossen, Orbitansicht): Beim nächsten Zeichnen
    // ist sie verfallen, und „Ja, aufheben“ steht nicht mehr da.
    for _ in 0..3 {
        screen.settle(|_| {});
    }
    let painted = window(&mut screen, &scene, &mut ui);
    assert_eq!(ui.confirm, None);
    assert_eq!(painted.text(CONFIRM_LIFT), None);
    assert!(painted.text(LIFT).is_some());
    // Die Rückfrage vor der Übergabe nennt die Folge, wenn es die letzte Welt ist.
    assert_eq!(capitulate_question(3), ASK_CAPITULATE);
    assert_eq!(capitulate_question(1), ASK_LAST_WORLD);
    assert!(ASK_LAST_WORLD.contains("endet die eigene Gemeinschaft"));
}

#[test]
fn a_plan_off_the_grid_is_not_an_edit_and_the_build_column_follows_the_shown_plan() {
    // Der Kern führt für eine besetzte Welt die Stärke ihrer Besatzung als Plan; sie
    // liegt nicht auf halben Verbänden. Die Tafel zeigt sie, ohne sie zu runden: nichts
    // ist geändert, nichts festzusetzen, nichts zu verwerfen.
    let mut scene = Scene::new(Some(ME));
    let capital = scene.state.polities.get(ME).capital.expect("Hauptwelt");
    let regulars = scene.state.sieges.defense(capital).plan.regulars.floor() + 0.2446;
    scene.state.sieges.defense_mut(capital).plan.regulars = regulars;
    let refs = scene.refs();
    let card = defense_card(&siege_refs(&refs), capital).expect("Karte");
    assert_eq!(card.plan.regulars, regulars);
    let mut ui = SiegeUi::default();
    let mut screen = Screen::new(1600.0, 900.0);
    let (painted, _) = editor(&mut screen, &scene, &mut ui, capital, None);
    assert_eq!(ui.plan_edit, None, "ohne Zutun kein geänderter Plan");
    assert_eq!(painted.text(defense::DISCARD), None);
    assert!(
        !painted
            .texts
            .iter()
            .any(|(text, _)| text.contains("dem geänderten Plan")),
        "die Tafel spricht von einem geänderten Plan"
    );
    let (_, actions) = editor(&mut screen, &scene, &mut ui, capital, Some(SET_PLAN));
    assert!(commands(&actions).is_empty());
    // „Verwerfen“ stellt auch einen solchen Plan genau wieder her.
    let more = defense::with_level(card.plan.clone(), Works::Guns, card.plan.guns + 1);
    ui.plan_edit = Some((capital, more));
    editor(
        &mut screen,
        &scene,
        &mut ui,
        capital,
        Some(defense::DISCARD),
    );
    assert_eq!(ui.plan_edit, None);
    let (painted, _) = editor(&mut screen, &scene, &mut ui, capital, None);
    assert_eq!(ui.plan_edit, None);
    assert_eq!(painted.text(defense::DISCARD), None);
    // Der Regler selbst rastet auf halbe Verbände und am Ende auf die Grenze.
    assert_eq!(defense::regulars_step(3.3, 22.74), 3.5);
    assert_eq!(defense::regulars_step(3.2, 22.74), 3.0);
    assert_eq!(defense::regulars_step(22.6, 22.74), 22.74);
    assert_eq!(defense::regulars_step(-1.0, 22.74), 0.0);

    // Die Spalte „Ausbau“ misst am gezeigten Plan: Ein höheres Ziel ist nicht „erfüllt“.
    let works = Works::ALL
        .into_iter()
        .find(|works| {
            works.planned(&card.plan) as f64 <= works.level(&card.defense).floor()
                && card.level_caps[works.index()] as f64 > works.level(&card.defense).floor()
        })
        .expect("eine Anlage, deren Plan erfüllt ist und die höher ginge");
    assert_eq!(defense::progress_text(&card, works), "erfüllt");
    let level = works.level(&card.defense).floor() as u8;
    let higher = defense::with_level(card.plan.clone(), works, level + 1);
    let told = defense::progress_under(&card, &higher, works);
    assert!(told.contains(" von ") && told != "erfüllt", "{told}");
    ui.plan_edit = Some((capital, higher));
    let (painted, _) = editor(&mut screen, &scene, &mut ui, capital, None);
    let row = painted.text(works.label()).unwrap().center().y;
    let cells: Vec<&str> = painted
        .texts
        .iter()
        .filter(|(_, rect)| (rect.center().y - row).abs() < 4.0)
        .map(|(text, _)| text.as_str())
        .collect();
    assert!(cells.contains(&told.as_str()), "{cells:?}");
    assert!(!cells.contains(&"erfüllt"), "{cells:?}");
}

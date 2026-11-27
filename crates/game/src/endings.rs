//! Das Ende einer Gemeinschaft in der Oberfläche.
//!
//! Der Kern beendet eine Gemeinschaft, wenn ihre letzte Welt fällt oder sie in einer
//! anderen aufgeht (`polity::succession`). Hier wird nach jedem Takt nachgesehen, wer
//! nicht mehr besteht; die Oberfläche zeigt dazu eine Meldung in den Worten der Chronik.
//! Trifft es die Gemeinschaft des Spielers, hält die Partie an: Sie läuft danach als
//! Beobachterpartie weiter, Befehle sind gesperrt.

use crate::battle_view::BattleFeedSet;
use crate::session::SessionEpoch;
use crate::state::AppState;
use crate::time_control::SimSpeed;
use crate::ui::panels::{ACCENT, color32, detail as small};
use bevy::prelude::*;
use bevy_egui::egui::{self, Align2, Color32, RichText};
use sim::SimRoot;
use sim::chronicle::Chronicle;
use sim::clock::{Date, SimClock, TICKS_PER_DAY};
use sim::data::Rgb;
use sim::endgame::PlayerFate;
use sim::ids::PolityId;
use sim::polity::Polities;
use sim::state::SimConfig;

/// Überschriften und Knöpfe; README und Tests nennen sie.
pub const TITLE_OTHER: &str = "Eine Gemeinschaft endet";
pub const TITLE_PLAYER: &str = "Die eigene Gemeinschaft hat geendet";
pub const CLOSE: &str = "Schließen";
pub const CONTINUE_AS_OBSERVER: &str = "Als Beobachter weiter";
/// Warum nach dem Ende der eigenen Gemeinschaft kein Befehl mehr geht.
pub const OUT_REASON: &str = "Die eigene Gemeinschaft besteht nicht mehr: Befehle sind \
     gesperrt, die Partie läuft als Beobachterpartie weiter.";
/// Was die Kopfleiste dann neben „Beobachter“ zeigt.
pub const OUT_LABEL: &str = "eigene Gemeinschaft ausgeschieden · Befehle gesperrt";

/// Meldung über das Ende einer Gemeinschaft.
#[derive(Debug, Clone, PartialEq)]
pub struct EndMessage {
    pub polity: PolityId,
    pub name: String,
    pub color: Rgb,
    pub date: Date,
    /// Was geschah, in den Worten der Chronik.
    pub text: String,
    /// Es war die Gemeinschaft des Spielers.
    pub player: bool,
}

/// Wer in der laufenden Partie noch besteht, und die Meldungen, die noch zu lesen sind.
#[derive(Resource, Default, Debug, Clone)]
pub struct Endings {
    epoch: Option<u32>,
    alive: Vec<bool>,
    /// Die Gemeinschaft, die der Spieler zuletzt geführt hat.
    player: Option<PolityId>,
    /// Ungelesene Meldungen, die älteste zuerst.
    pub messages: Vec<EndMessage>,
}

/// Was die Chronik über das Ende dieser Gemeinschaft sagt: ihr jüngster Eintrag der
/// letzten Tage, der sie nennt und vom Enden oder Aufgehen spricht.
pub fn end_text(chronicle: &Chronicle, clock: &SimClock, name: &str) -> String {
    let since = clock.tick.saturating_sub(3 * TICKS_PER_DAY);
    chronicle
        .entries
        .iter()
        .rev()
        .take_while(|entry| entry.tick >= since)
        .find(|entry| {
            entry.text.contains(name)
                && (entry.text.contains("endet") || entry.text.contains("geht in"))
        })
        .map(|entry| entry.text.clone())
        .unwrap_or_else(|| format!("{name} besteht nicht mehr."))
}

/// Vergleicht, wer besteht, mit dem Stand des letzten Takts. Eine neue oder geladene
/// Partie beginnt ohne Meldungen: Was dort schon geendet hat, ist keine Neuigkeit.
pub fn observe(
    endings: &mut Endings,
    epoch: u32,
    polities: &Polities,
    config: &SimConfig,
    chronicle: &Chronicle,
    clock: &SimClock,
) -> bool {
    let now: Vec<bool> = polities.list.iter().map(|polity| polity.alive).collect();
    if endings.epoch != Some(epoch) {
        *endings = Endings {
            epoch: Some(epoch),
            alive: now,
            player: config.player,
            messages: Vec::new(),
        };
        return false;
    }
    let mut player_out = false;
    for (index, alive) in now.iter().enumerate() {
        // Gemeinschaften, die erst in der Partie entstanden sind, kommen ohne Meldung dazu.
        let before = endings.alive.get(index).copied().unwrap_or(*alive);
        if before && !alive {
            let polity = &polities.list[index];
            let player = endings.player == Some(polity.id);
            player_out |= player;
            endings.messages.push(EndMessage {
                polity: polity.id,
                name: polity.name.clone(),
                color: polity.color,
                date: clock.date(),
                text: end_text(chronicle, clock, &polity.name),
                player,
            });
        }
    }
    endings.alive = now;
    if config.player.is_some() {
        endings.player = config.player;
    }
    player_out
}

fn watch(
    mut endings: ResMut<Endings>,
    epoch: Res<SessionEpoch>,
    polities: Res<Polities>,
    config: Res<SimConfig>,
    chronicle: Res<Chronicle>,
    clock: Res<SimClock>,
    mut speed: ResMut<SimSpeed>,
) {
    if observe(
        &mut endings,
        epoch.0,
        &polities,
        &config,
        &chronicle,
        &clock,
    ) {
        // Während der Spieler liest, was geschehen ist, vergeht keine Zeit ungesehen.
        speed.paused = true;
    }
}

/// Auch bei stehender Zeit (gleich nach dem Laden) gilt der Stand der neuen Partie.
fn adopt(
    mut endings: ResMut<Endings>,
    epoch: Res<SessionEpoch>,
    polities: Res<Polities>,
    config: Res<SimConfig>,
    chronicle: Res<Chronicle>,
    clock: Res<SimClock>,
) {
    if endings.epoch != Some(epoch.0) {
        observe(
            &mut endings,
            epoch.0,
            &polities,
            &config,
            &chronicle,
            &clock,
        );
    }
}

/// Was die Kopfleiste über das Schicksal der eigenen Gemeinschaft sagt; `None`, solange
/// der Spieler eine führt oder von Anfang an nur zusieht.
pub fn fate_label(fate: PlayerFate) -> Option<&'static str> {
    match fate {
        PlayerFate::Active => None,
        PlayerFate::Merged => Some("eigene Gemeinschaft aufgegangen · Befehle gesperrt"),
        PlayerFate::Out => Some(OUT_LABEL),
    }
}

/// Das Fenster zur ältesten ungelesenen Meldung. Es liegt über allem und bleibt, bis der
/// Spieler es schließt.
pub fn window(ctx: &egui::Context, endings: &mut Endings) {
    let Some(message) = endings.messages.first().cloned() else {
        return;
    };
    let title = if message.player {
        TITLE_PLAYER
    } else {
        TITLE_OTHER
    };
    let mut close = false;
    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .anchor(
            Align2::CENTER_CENTER,
            [0.0, if message.player { 0.0 } else { -40.0 }],
        )
        .show(ctx, |ui| {
            ui.set_width(470.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(&message.name)
                        .color(color32(message.color))
                        .strong(),
                );
                ui.label(
                    RichText::new(format!("besteht seit dem {} nicht mehr.", message.date))
                        .strong(),
                );
            });
            ui.add_space(4.0);
            ui.label(&message.text);
            ui.add_space(4.0);
            if message.player {
                ui.label(RichText::new("Die Partie ist angehalten.").color(ACCENT));
                ui.label(
                    "Sie geht als Beobachterpartie weiter: Karte, Fenster und Chronik bleiben \
                     offen, die Zeit läuft mit der Leertaste wieder an.",
                );
                ui.label(RichText::new(OUT_REASON).color(Color32::from_rgb(235, 110, 95)));
                ui.label(small(
                    "Die ausführlichen Abschlussbilder einer Partie kommen mit Meilenstein 6.",
                ));
            } else {
                ui.label(small(
                    "Ihre Welten zeigt die Karte in der Farbe der neuen Besitzer; in der \
                     Liste der Gemeinschaften steht sie durchgestrichen als beendet.",
                ));
            }
            ui.add_space(6.0);
            let label = if message.player {
                CONTINUE_AS_OBSERVER
            } else {
                CLOSE
            };
            if ui.button(label).clicked() {
                close = true;
            }
        });
    // Die Meldung bleibt über den anderen Fenstern, auch wenn eines davon angeklickt wird.
    ctx.move_to_top(egui::LayerId::new(
        egui::Order::Middle,
        egui::Id::new(Some(title)),
    ));
    if close {
        endings.messages.remove(0);
    }
}

pub struct EndingsPlugin;

impl Plugin for EndingsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Endings>()
            .add_systems(Update, adopt.run_if(in_state(AppState::InGame)))
            .add_systems(
                FixedUpdate,
                watch
                    .in_set(BattleFeedSet::Watchers)
                    .after(SimRoot)
                    .run_if(in_state(AppState::InGame)),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{TestGame, conquest};
    use crate::ui::panels::ENDED;
    use sim::galaxy::Galaxy;
    use sim::siege::{SiegeRefs, siege_cards};

    /// Die Gemeinschaft, deren letzte Welt belagert wird, und der Belagerer.
    fn sides() -> (PolityId, PolityId) {
        let (data, state) = conquest::state(conquest::LAST_WORLD);
        let cards = siege_cards(&SiegeRefs::of_state(state, data), state.clock.day());
        assert_eq!(cards.len(), 1, "die Belagerung der letzten Welt");
        let defender = cards[0].defender;
        assert_eq!(
            state.galaxy.planets_of(defender).count(),
            1,
            "ihre letzte Welt"
        );
        (defender, cards[0].attacker)
    }

    #[test]
    fn an_ending_is_noticed_once_and_a_loaded_game_starts_without_news() {
        let (_, state) = conquest::state(conquest::LAST_WORLD);
        let (defender, _) = sides();
        let mut polities = state.polities.clone();
        let mut config = state.config.clone();
        config.player = Some(defender);
        let mut endings = Endings::default();
        let look = |endings: &mut Endings, polities: &Polities, config: &SimConfig| {
            observe(endings, 7, polities, config, &state.chronicle, &state.clock)
        };
        assert!(!look(&mut endings, &polities, &config));
        assert!(endings.messages.is_empty());
        // Die Gemeinschaft des Spielers endet: eine Meldung, die Partie soll anhalten.
        polities.get_mut(defender).alive = false;
        config.player = None;
        assert!(look(&mut endings, &polities, &config));
        assert_eq!(endings.messages.len(), 1);
        let message = &endings.messages[0];
        assert!(message.player);
        assert_eq!(message.polity, defender);
        assert!(message.text.contains(&message.name), "{message:?}");
        // Beim nächsten Blick ist es keine Neuigkeit mehr.
        assert!(!look(&mut endings, &polities, &config));
        assert_eq!(endings.messages.len(), 1);
        // Eine andere Partie (geladen, neu begonnen) beginnt ohne Meldungen.
        assert!(!observe(
            &mut endings,
            8,
            &polities,
            &config,
            &state.chronicle,
            &state.clock
        ));
        assert!(endings.messages.is_empty());
        assert_eq!(fate_label(PlayerFate::Active), None);
        assert_eq!(fate_label(PlayerFate::Out), Some(OUT_LABEL));
    }

    /// Lädt die Partie kurz vor dem Fall der letzten Welt und lässt sie laufen, bis die
    /// Gemeinschaft endet.
    fn until_the_end(player: PolityId, name: &str) -> TestGame {
        let (defender, _) = sides();
        let path = conquest::save_file(conquest::LAST_WORLD, Some(player), name);
        let mut game = TestGame::start(&format!("--load {}", path.display()));
        game.frames(6);
        let _ = std::fs::remove_file(&path);
        assert!(game.resource::<Endings>().messages.is_empty());
        game.key(KeyCode::Digit6);
        let mut frames = 0;
        while game.resource::<Polities>().get(defender).alive {
            game.frames(1);
            frames += 1;
            assert!(frames < 600, "die Gemeinschaft endet nicht");
        }
        game.frames(3);
        game
    }

    #[test]
    fn the_end_of_the_players_community_pauses_the_game_and_leaves_an_observer() {
        let (defender, attacker) = sides();
        let mut game = until_the_end(defender, "ende_eigen");
        // Die Partie hält an; der Spieler führt niemanden mehr.
        assert!(game.resource::<SimSpeed>().paused);
        assert_eq!(game.resource::<SimConfig>().player, None);
        let message = game.resource::<Endings>().messages[0].clone();
        assert!(message.player);
        assert!(message.text.contains("endet"), "{message:?}");
        assert!(game.window_rect(TITLE_PLAYER).is_some());
        assert!(game.text(OUT_REASON).is_some());
        // Die Kopfleiste sagt, warum er nur noch zusieht; die Liste führt die
        // Gemeinschaft als beendet.
        assert!(game.text("Beobachter").is_some());
        assert!(game.text(OUT_LABEL).is_some());
        assert!(game.text(ENDED).is_some());
        // Ihre Welten gehören dem Belagerer: Die Karte färbt nach dem Besitz.
        assert_eq!(game.resource::<Galaxy>().planets_of(defender).count(), 0);
        assert!(game.resource::<Galaxy>().planets_of(attacker).count() > 0);
        // Das Fenster bleibt, bis er es schließt; danach läuft die Partie auf Wunsch
        // als Beobachterpartie weiter.
        game.frames(20);
        assert!(game.window_rect(TITLE_PLAYER).is_some());
        game.click_on(CONTINUE_AS_OBSERVER);
        game.frames(12);
        assert!(game.window_rect(TITLE_PLAYER).is_none());
        assert!(game.resource::<Endings>().messages.is_empty());
        let day = game.resource::<sim::clock::SimClock>().day();
        game.key(KeyCode::Space);
        game.frames(10);
        assert!(game.resource::<sim::clock::SimClock>().day() > day);
        // Befehle gibt es nicht mehr: Das Fenster „Belagerung“ sagt es.
        game.click_on(&crate::ui::siege::toggle_label(
            game.resource::<sim::siege::Sieges>().active.len(),
        ));
        game.frames(3);
        assert!(game.window_rect(crate::ui::siege::TITLE).is_some());
    }

    #[test]
    fn the_end_of_another_community_is_announced_without_a_pause() {
        let (_, attacker) = sides();
        let mut game = until_the_end(attacker, "ende_fremd");
        assert!(
            !game.resource::<SimSpeed>().paused,
            "die Partie läuft weiter"
        );
        assert_eq!(game.resource::<SimConfig>().player, Some(attacker));
        let message = game.resource::<Endings>().messages[0].clone();
        assert!(!message.player);
        assert!(game.window_rect(TITLE_OTHER).is_some());
        assert!(game.text(ENDED).is_some());
        game.click_on(CLOSE);
        game.frames(12);
        assert!(game.window_rect(TITLE_OTHER).is_none());
    }
}

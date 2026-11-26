//! Gefechtsübung: ein Gefecht außerhalb jeder Partie, mit frei gewählten Flotten.
//!
//! Die Übung besitzt ihr Gefecht selbst. Sie rechnet es im festen Takt weiter (Pause und
//! Geschwindigkeitsstufen wirken wie in der Partie), legt den Zustand nach jedem Takt im
//! [`BattleFeed`] ab und führt die Befehle des Spielers unmittelbar aus. Dasselbe wird
//! später die Partie für ihre Gefechte tun.

use super::BattleFeedSet;
use super::camera::ViewOptions;
use super::feed::{BattleFeed, BattleOrder, BattleViewRequest, FeedInfo, SideInfo};
use crate::args::Args;
use crate::session::Notice;
use crate::state::AppState;
use crate::time_control::SimSpeed;
use crate::ui::UiState;
use crate::ui::format::decimal;
use crate::ui::panels::{ACCENT, MUTED, color32};
use bevy::prelude::*;
use bevy_egui::egui::{self, Align2, Color32, RichText};
use bevy_egui::input::egui_wants_any_keyboard_input;
use sim::GameData;
use sim::battle::matchup::{self, Side};
use sim::battle::{Battle, Doctrine, Formation, V2};
use sim::clock::SimClock;
use sim::data::{Rgb, ShipClass};
use std::time::{Duration, Instant};

/// Namen und Farben der beiden Übungsparteien.
pub const SIDE_NAMES: [&str; 2] = ["Blauer Verband", "Roter Verband"];
pub const SIDE_COLORS: [Rgb; 2] = [(0.30, 0.62, 1.0), (1.0, 0.40, 0.30)];

/// Fertige Aufstellung zum Auswählen.
pub struct Preset {
    pub name: &'static str,
    pub hint: &'static str,
    pub fleets: [&'static str; 2],
    pub formations: [Formation; 2],
}

pub const PRESETS: [Preset; 4] = [
    Preset {
        name: "Geplänkel",
        hint: "Korvetten und Fregatten, schnell entschieden",
        fleets: ["dolch:4,waechter:2", "dolch:3,waechter:3"],
        formations: [Formation::Line, Formation::Line],
    },
    Preset {
        name: "Linienschlacht",
        hint: "Zwei gleiche Schlachtlinien mit Geleit",
        fleets: [
            "bastion:2,speer:6,waechter:6,dolch:4",
            "bastion:2,speer:6,waechter:6,dolch:4",
        ],
        formations: [Formation::Line, Formation::Line],
    },
    Preset {
        name: "Träger gegen Schlachtschiffe",
        hint: "Jägerstaffeln gegen schwere Geschütze",
        fleets: ["horst:3,waechter:6,dolch:4", "bastion:3,speer:4,waechter:4"],
        formations: [Formation::Screen, Formation::Wedge],
    },
    Preset {
        name: "Großgefecht",
        hint: "Rund 200 Schiffe je Seite",
        fleets: [
            "bastion:6,horst:4,speer:30,waechter:60,dolch:100",
            "bastion:10,speer:40,waechter:60,dolch:90",
        ],
        formations: [Formation::Line, Formation::Line],
    },
];

/// Aufstellung, mit der die Übung ohne weitere Angaben beginnt.
const DEFAULT_PRESET: usize = 1;

/// So viele Schiffe darf ein Verband höchstens aufbieten, Bordjäger mitgezählt. Bis zu
/// dieser Größe ist der Gefechtsrechner erprobt; weit darüber braucht ein einzelner Takt
/// Sekunden bis Minuten, und das Spiel stünde so lange still.
pub const MAX_SHIPS_PER_SIDE: u64 = 1000;

/// Eingaben des Aufstellungsdialogs.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct PracticeSetup {
    /// Flottenlisten in der Schreibweise `speer:4,waechter:6`.
    pub fleets: [String; 2],
    pub formations: [Formation; 2],
    pub doctrines: [Doctrine; 2],
    pub seed_text: String,
}

impl Default for PracticeSetup {
    fn default() -> Self {
        Self::from_preset(&PRESETS[DEFAULT_PRESET])
    }
}

impl PracticeSetup {
    pub fn from_preset(preset: &Preset) -> Self {
        Self {
            fleets: preset.fleets.map(String::from),
            formations: preset.formations,
            doctrines: [Doctrine::default(); 2],
            seed_text: "1".into(),
        }
    }

    /// Prüft eine Flottenliste und setzt Formation und Doktrin der Partei ein.
    fn side(&self, data: &GameData, index: usize) -> Result<Side, String> {
        parse_fleet(data, &self.fleets[index]).map(|side| {
            side.with_formation(self.formations[index])
                .with_doctrine(self.doctrines[index])
        })
    }

    /// Die Aufstellung, wenn alle Eingaben gültig sind.
    pub fn resolve(&self, data: &GameData) -> Result<PracticeConfig, String> {
        let seed = self
            .seed_text
            .trim()
            .parse::<u64>()
            .map_err(|_| "Der Seed muss eine ganze Zahl sein.".to_string())?;
        let a = self
            .side(data, 0)
            .map_err(|error| format!("{}: {error}", SIDE_NAMES[0]))?;
        let b = self
            .side(data, 1)
            .map_err(|error| format!("{}: {error}", SIDE_NAMES[1]))?;
        Ok(PracticeConfig {
            sides: [a, b],
            seed,
            lineup: false,
        })
    }
}

/// Zahl der Schiffe einer Flottenliste und der Jäger an Bord ihrer Träger. Gerechnet wird
/// so, dass auch unsinnig große Eingaben nichts überlaufen lassen.
fn head_count(data: &GameData, side: &Side) -> (u64, u64) {
    side.ships
        .iter()
        .fold((0u64, 0u64), |(ships, fighters), (design, count)| {
            let count = u64::from(*count);
            let aboard = u64::from(data.designs[*design as usize].stats.fighters);
            (
                ships.saturating_add(count),
                fighters.saturating_add(aboard.saturating_mul(count)),
            )
        })
}

/// Liest eine Flottenliste und prüft ihre Größe: jeder Eintrag mit mindestens einem Schiff,
/// der Verband insgesamt nicht größer als [`MAX_SHIPS_PER_SIDE`]. Dialog und Kommandozeile
/// gehen beide hier durch.
fn parse_fleet(data: &GameData, text: &str) -> Result<Side, String> {
    let side = Side::parse(data, text)?;
    if let Some((design, _)) = side.ships.iter().find(|(_, count)| *count == 0) {
        return Err(format!(
            "von „{}“ muss mindestens ein Schiff dabei sein",
            data.designs[*design as usize].key
        ));
    }
    let (ships, fighters) = head_count(data, &side);
    if ships.saturating_add(fighters) > MAX_SHIPS_PER_SIDE {
        let aboard = if fighters > 0 {
            format!(" und {fighters} Bordjäger")
        } else {
            String::new()
        };
        return Err(format!(
            "{ships} Schiffe{aboard} sind zu viele: höchstens {MAX_SHIPS_PER_SIDE} je \
             Verband, Bordjäger mitgezählt"
        ));
    }
    Ok(side)
}

/// Geprüfte Aufstellung einer Übung.
#[derive(Clone, Debug)]
pub struct PracticeConfig {
    pub sides: [Side; 2],
    pub seed: u64,
    /// Schaustellung: je Partei ein Schiff jeder Klasse nebeneinander, ohne Gefecht.
    pub lineup: bool,
}

impl PracticeConfig {
    /// Je Partei ein Schiff jeder Klasse, zum Betrachten der Rümpfe.
    pub fn lineup(data: &GameData) -> Self {
        let ships: Vec<(u16, u32)> = ShipClass::ALL
            .iter()
            .filter_map(|class| {
                data.designs
                    .iter()
                    .position(|design| design.stats.class == *class)
                    .map(|design| (design as u16, 1))
            })
            .collect();
        Self {
            sides: [Side::new(ships.clone()), Side::new(ships)],
            seed: 1,
            lineup: true,
        }
    }

    /// Stellt das Gefecht auf.
    pub fn build(&self, data: &GameData) -> Battle {
        let mut battle = matchup::build(data, self.seed, &self.sides);
        if self.lineup {
            arrange_lineup(&mut battle);
        }
        battle
    }
}

/// Abstand der beiden Reihen einer Schaustellung.
pub const LINEUP_ROW_GAP: f32 = 105.0;

/// Stellt die Schiffe einer Schaustellung in zwei Reihen nebeneinander: vorn die erste
/// Partei, dahinter die zweite, jeweils vom Jäger bis zum Träger.
fn arrange_lineup(battle: &mut Battle) {
    for side in 0..battle.combatants.len() {
        let mut members: Vec<usize> = (0..battle.ships.len())
            .filter(|index| battle.ships[*index].side as usize == side)
            .collect();
        members.sort_by_key(|index| battle.ships[*index].class);
        let mut x = 0.0;
        let mut places = Vec::with_capacity(members.len());
        for (position, index) in members.iter().enumerate() {
            let radius = battle.ships[*index].radius;
            if position > 0 {
                x += radius * 0.8 + 5.0;
            }
            places.push((*index, x));
            x += radius * 0.8 + 5.0;
        }
        let width = x;
        for (index, at) in places {
            let ship = &mut battle.ships[index];
            ship.pos = V2::new(at - width * 0.5, side as f32 * LINEUP_ROW_GAP);
            // Bug schräg zum Betrachter, der von Süden auf das Feld blickt.
            ship.heading = (-62.0f32).to_radians();
            ship.vel = V2::ZERO;
            // Fahrt nur zum Schein, damit die Triebwerke leuchten.
            ship.speed = ship.max_speed * 0.8;
            ship.target = None;
        }
        let combatant = &mut battle.combatants[side];
        combatant.anchor = V2::new(0.0, side as f32 * LINEUP_ROW_GAP);
        combatant.facing = (-62.0f32).to_radians();
    }
}

/// Bitte, eine Übung zu beginnen.
#[derive(Message, Clone, Debug)]
pub struct StartPractice(pub PracticeConfig);

/// Das Gefecht der laufenden Übung.
#[derive(Resource)]
pub struct PracticeBattle {
    pub battle: Battle,
    config: PracticeConfig,
    /// Rechenzeit: gerechnete Takte, Zeit im Gefechtsrechner und Zeit für das Ablegen im
    /// Feed. Der Selbsttest weist sie aus.
    pub ticks: u64,
    pub step_time: Duration,
    pub feed_time: Duration,
}

impl PracticeBattle {
    fn new(battle: Battle, config: PracticeConfig) -> Self {
        Self {
            battle,
            config,
            ticks: 0,
            step_time: Duration::ZERO,
            feed_time: Duration::ZERO,
        }
    }
}

fn feed_info(config: &PracticeConfig) -> FeedInfo {
    FeedInfo {
        title: if config.lineup {
            "Schaustellung".into()
        } else {
            "Gefechtsübung".into()
        },
        sides: (0..2)
            .map(|side| SideInfo {
                name: SIDE_NAMES[side].into(),
                color: SIDE_COLORS[side],
            })
            .collect(),
        player_side: (!config.lineup).then_some(0),
        can_restart: !config.lineup,
        still: config.lineup,
        ..default()
    }
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<PracticeSetup>()
        .add_message::<StartPractice>()
        .add_systems(OnEnter(AppState::MainMenu), start_from_args)
        .add_systems(Update, start_requested)
        .add_systems(
            Update,
            (
                keys.run_if(not(egui_wants_any_keyboard_input)),
                handle_view_requests,
                apply_orders,
            )
                .chain()
                .run_if(in_state(AppState::Practice)),
        )
        .add_systems(
            FixedUpdate,
            step.in_set(BattleFeedSet::Practice)
                .run_if(in_state(AppState::Practice)),
        )
        .add_systems(OnExit(AppState::Practice), leave);
}

/// `--practice` überspringt das Hauptmenü, aber nur beim Programmstart.
fn start_from_args(
    mut args: ResMut<Args>,
    data: Res<GameData>,
    mut notice: ResMut<Notice>,
    mut requests: MessageWriter<StartPractice>,
) {
    if !std::mem::take(&mut args.practice) {
        return;
    }
    let seed = args.seed.take().unwrap_or(1);
    if args.lineup {
        requests.write(StartPractice(PracticeConfig::lineup(&data)));
        return;
    }
    let mut setup = PracticeSetup {
        seed_text: seed.to_string(),
        ..default()
    };
    if let Some(fleet) = args.fleet_a.take() {
        setup.fleets[0] = fleet;
    }
    if let Some(fleet) = args.fleet_b.take() {
        setup.fleets[1] = fleet;
    }
    match setup.resolve(&data) {
        Ok(config) => {
            requests.write(StartPractice(config));
        }
        Err(error) => {
            eprintln!("Gefechtsübung: {error}");
            notice.error(error);
        }
    }
}

fn start_requested(
    mut commands: Commands,
    mut requests: MessageReader<StartPractice>,
    data: Res<GameData>,
    mut feed: ResMut<BattleFeed>,
    mut speed: ResMut<SimSpeed>,
    mut ui_state: ResMut<UiState>,
    mut options: ResMut<ViewOptions>,
    mut next: ResMut<NextState<AppState>>,
) {
    let Some(StartPractice(config)) = requests.read().last().cloned() else {
        return;
    };
    let battle = config.build(&data);
    feed.begin(&battle, feed_info(&config));
    options.labels = config.lineup;
    commands.insert_resource(PracticeBattle::new(battle, config));
    // Die Laufbedingungen des Kerns (`day_start`, `month_start`) lesen die Uhr der Partie,
    // sobald der feste Takt läuft, auch wenn die Simulation selbst ruht. Ohne Partie gibt
    // es keine Uhr; die Übung stellt deshalb eine leere hin und räumt sie beim Verlassen
    // wieder ab.
    commands.init_resource::<SimClock>();
    // Wie eine Partie beginnt die Übung pausiert: erst umsehen, dann die Zeit laufen lassen.
    *speed = SimSpeed::default();
    ui_state.practice_open = false;
    ui_state.menu_open = false;
    next.set(AppState::Practice);
}

fn keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut ui_state: ResMut<UiState>,
    mut requests: MessageWriter<BattleViewRequest>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        if ui_state.help_open {
            ui_state.help_open = false;
        } else {
            requests.write(BattleViewRequest::Leave);
        }
    }
    if keys.just_pressed(KeyCode::F1) {
        ui_state.help_open = !ui_state.help_open;
    }
}

fn handle_view_requests(
    mut requests: MessageReader<BattleViewRequest>,
    data: Res<GameData>,
    practice: Option<ResMut<PracticeBattle>>,
    mut feed: ResMut<BattleFeed>,
    mut speed: ResMut<SimSpeed>,
    mut time: ResMut<Time<Virtual>>,
    mut next: ResMut<NextState<AppState>>,
) {
    let Some(mut practice) = practice else {
        requests.clear();
        return;
    };
    for request in requests.read() {
        match request {
            BattleViewRequest::Leave => {
                // Die Zeit hält sofort an, nicht erst beim Zustandswechsel im nächsten
                // Frame: Danach darf kein Takt mehr laufen, denn die Uhr verschwindet.
                *speed = SimSpeed::default();
                time.pause();
                next.set(AppState::MainMenu);
            }
            BattleViewRequest::Restart => {
                let config = practice.config.clone();
                *practice = PracticeBattle::new(config.build(&data), config);
                feed.begin(&practice.battle, feed_info(&practice.config));
                // Auch hier hält die Zeit sofort an. Die Zeitsteuerung überträgt die neue
                // Stufe erst, wenn sie das nächste Mal läuft; bis dahin rechnete das
                // frische Gefecht mit der alten Stufe schon etliche Takte weiter.
                *speed = SimSpeed::default();
                time.pause();
            }
        }
    }
}

/// Befehle des Spielers wirken sofort, auch in der Pause.
fn apply_orders(
    mut orders: MessageReader<BattleOrder>,
    data: Res<GameData>,
    practice: Option<ResMut<PracticeBattle>>,
    mut feed: ResMut<BattleFeed>,
) {
    let Some(mut practice) = practice else {
        orders.clear();
        return;
    };
    let mut changed = false;
    for order in orders.read() {
        changed = true;
        match *order {
            BattleOrder::SetOrders {
                side,
                formation,
                doctrine,
            } => practice.battle.set_orders(&data, side, formation, doctrine),
            BattleOrder::Retreat { side } => practice.battle.order_retreat(side),
        }
    }
    if changed {
        feed.refresh(&practice.battle);
    }
}

/// Ein Takt Gefecht. Läuft im festen Takt der Simulation.
fn step(
    data: Res<GameData>,
    practice: Option<ResMut<PracticeBattle>>,
    mut feed: ResMut<BattleFeed>,
) {
    let Some(mut practice) = practice else {
        return;
    };
    if practice.config.lineup || practice.battle.finished {
        return;
    }
    let start = Instant::now();
    practice.battle.step(&data);
    let stepped = Instant::now();
    feed.refresh(&practice.battle);
    practice.ticks += 1;
    practice.step_time += stepped - start;
    practice.feed_time += stepped.elapsed();
}

fn leave(
    mut commands: Commands,
    mut feed: ResMut<BattleFeed>,
    mut speed: ResMut<SimSpeed>,
    mut ui_state: ResMut<UiState>,
) {
    commands.remove_resource::<PracticeBattle>();
    commands.remove_resource::<SimClock>();
    feed.clear();
    *speed = SimSpeed::default();
    ui_state.help_open = false;
}

/// Kurzbeschreibung einer Flottenliste oder der Grund, warum sie nicht gilt. Läuft im
/// Dialog bei jedem Tastendruck über den gerade getippten Text.
fn fleet_summary(data: &GameData, text: &str) -> Result<String, String> {
    let side = parse_fleet(data, text)?;
    let (ships, fighters) = head_count(data, &side);
    let mut summary = format!("{ships} Schiffe, Bauwert {}", decimal(side.value(data), 0));
    if fighters > 0 {
        summary.push_str(&format!(", dazu {fighters} Bordjäger"));
    }
    Ok(summary)
}

/// Aufstellungsdialog im Hauptmenü. Gibt die Aufstellung zurück, sobald der Spieler das
/// Gefecht beginnt.
pub fn setup_dialog(
    ctx: &egui::Context,
    data: &GameData,
    setup: &mut PracticeSetup,
    open: &mut bool,
) -> Option<PracticeConfig> {
    let mut started = None;
    let mut keep_open = *open;
    egui::Window::new("Gefechtsübung")
        .open(&mut keep_open)
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_width(620.0);
            ui.label("Zwei Flotten treten gegeneinander an. Du führst den blauen Verband.");
            ui.add_space(6.0);
            ui.label(RichText::new("Vorlagen").color(MUTED));
            ui.horizontal_wrapped(|ui| {
                for preset in &PRESETS {
                    if ui.button(preset.name).on_hover_text(preset.hint).clicked() {
                        let seed_text = std::mem::take(&mut setup.seed_text);
                        *setup = PracticeSetup {
                            seed_text,
                            ..PracticeSetup::from_preset(preset)
                        };
                    }
                }
            });
            ui.add_space(8.0);
            for side in 0..2 {
                let color = color32(SIDE_COLORS[side]);
                ui.label(RichText::new(SIDE_NAMES[side]).strong().color(color));
                ui.add(
                    egui::TextEdit::singleline(&mut setup.fleets[side])
                        .desired_width(f32::INFINITY)
                        .hint_text("speer:4,waechter:6"),
                );
                match fleet_summary(data, &setup.fleets[side]) {
                    Ok(summary) => ui.label(RichText::new(summary).color(MUTED)),
                    Err(error) => {
                        ui.label(RichText::new(error).color(Color32::from_rgb(235, 110, 95)))
                    }
                };
                ui.horizontal(|ui| {
                    ui.label("Formation");
                    egui::ComboBox::from_id_salt(("formation", side))
                        .selected_text(setup.formations[side].label())
                        .show_ui(ui, |ui| {
                            for option in Formation::ALL {
                                ui.selectable_value(
                                    &mut setup.formations[side],
                                    option,
                                    option.label(),
                                );
                            }
                        });
                    ui.label("Doktrin");
                    egui::ComboBox::from_id_salt(("doktrin", side))
                        .selected_text(setup.doctrines[side].label())
                        .show_ui(ui, |ui| {
                            for option in Doctrine::ALL {
                                ui.selectable_value(
                                    &mut setup.doctrines[side],
                                    option,
                                    option.label(),
                                );
                            }
                        });
                });
                ui.add_space(8.0);
            }
            ui.label(
                RichText::new(format!(
                    "Entwürfe: {}",
                    data.designs
                        .iter()
                        .map(|design| format!("{} ({})", design.key, design.stats.class.label()))
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
                .color(MUTED)
                .small(),
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("Seed");
                ui.add(egui::TextEdit::singleline(&mut setup.seed_text).desired_width(120.0));
                ui.label(
                    RichText::new("Gleicher Seed, gleicher Verlauf.")
                        .color(MUTED)
                        .small(),
                );
            });
            ui.add_space(8.0);
            let config = setup.resolve(data);
            ui.horizontal(|ui| {
                let begin =
                    egui::Button::new(RichText::new("Gefecht beginnen").size(17.0).color(ACCENT));
                let response = ui.add_enabled(config.is_ok(), begin);
                match config {
                    Ok(config) => {
                        if response.clicked() {
                            started = Some(config);
                        }
                    }
                    Err(error) => {
                        response.on_disabled_hover_text(error);
                    }
                }
                if ui.button("Abbrechen").clicked() {
                    *open = false;
                }
            });
        });
    *open = *open && keep_open;
    started
}

#[cfg(test)]
mod tests {
    use super::super::feed::testing::data;
    use super::*;
    use sim::battle::ShipStatus;

    #[test]
    fn every_preset_is_a_valid_battle() {
        let data = data();
        for preset in &PRESETS {
            let setup = PracticeSetup::from_preset(preset);
            let config = setup
                .resolve(&data)
                .unwrap_or_else(|error| panic!("{}: {error}", preset.name));
            let battle = config.build(&data);
            assert_eq!(battle.combatants.len(), 2);
            assert!(battle.is_hostile(0, 1));
            assert_eq!(battle.combatants[0].formation, preset.formations[0]);
            for side in 0..2 {
                assert!(
                    battle.is_fighting(side),
                    "{}: Partei {side} ohne Schiffe",
                    preset.name
                );
            }
            assert!(fleet_summary(&data, preset.fleets[0]).is_ok());
        }
        // Das Großgefecht bringt rund 200 Schiffe je Seite auf das Feld.
        let large = PracticeSetup::from_preset(&PRESETS[3])
            .resolve(&data)
            .unwrap();
        for side in &large.sides {
            assert!(
                (190..=210).contains(&side.ship_count()),
                "{} Schiffe",
                side.ship_count()
            );
        }
    }

    #[test]
    fn setup_reports_what_is_wrong() {
        let data = data();
        let mut setup = PracticeSetup::default();
        assert!(setup.resolve(&data).is_ok());
        setup.seed_text = "sieben".into();
        assert!(setup.resolve(&data).unwrap_err().contains("Seed"));
        setup.seed_text = " 42 ".into();
        assert_eq!(setup.resolve(&data).unwrap().seed, 42);
        setup.fleets[1] = "gibtesnicht:3".into();
        let error = setup.resolve(&data).unwrap_err();
        assert!(
            error.contains(SIDE_NAMES[1]) && error.contains("gibtesnicht"),
            "{error}"
        );
        setup.fleets[1] = String::new();
        assert!(setup.resolve(&data).is_err());
        assert!(
            fleet_summary(&data, "horst:2")
                .unwrap()
                .contains("48 Bordjäger")
        );
        // Formation und Doktrin des Dialogs kommen im Gefecht an.
        let mut setup = PracticeSetup::default();
        setup.formations[1] = Formation::Pincer;
        setup.doctrines[0] = Doctrine::Aggressive;
        let battle = setup.resolve(&data).unwrap().build(&data);
        assert_eq!(battle.combatants[1].formation, Formation::Pincer);
        assert_eq!(battle.combatants[0].doctrine, Doctrine::Aggressive);
    }

    #[test]
    fn lineup_shows_one_ship_of_every_class_per_side() {
        let data = data();
        let battle = PracticeConfig::lineup(&data).build(&data);
        for side in 0..2u16 {
            let mut row: Vec<_> = battle
                .ships
                .iter()
                .filter(|ship| ship.side == side)
                .collect();
            assert_eq!(row.len(), ShipClass::ALL.len());
            row.sort_by(|a, b| a.pos.x.total_cmp(&b.pos.x));
            // Vom Jäger links bis zum Träger rechts, ohne Überschneidung.
            for (ship, class) in row.iter().zip(ShipClass::ALL) {
                assert_eq!(ship.class, class);
                assert_eq!(ship.status, ShipStatus::Active);
                assert_eq!(ship.pos.y, side as f32 * LINEUP_ROW_GAP);
            }
            for pair in row.windows(2) {
                let gap = pair[1].pos.x - pair[0].pos.x;
                assert!(
                    gap > 0.75 * (pair[0].radius + pair[1].radius),
                    "Schiffe überschneiden sich"
                );
            }
        }
    }

    #[test]
    fn practice_steps_its_battle_and_obeys_orders_at_once() {
        use bevy::ecs::message::Messages;
        use bevy::ecs::system::RunSystemOnce;

        let data = data();
        let config = PracticeSetup::default().resolve(&data).unwrap();
        let battle = config.build(&data);
        let mut feed = BattleFeed::default();
        feed.begin(&battle, feed_info(&config));
        let mut world = World::new();
        world.insert_resource(data);
        world.insert_resource(feed);
        world.insert_resource(PracticeBattle::new(battle, config));
        world.init_resource::<Messages<BattleOrder>>();

        // Ein Takt rechnet das Gefecht weiter und legt den Zustand im Feed ab.
        for _ in 0..30 {
            world.run_system_once(step).unwrap();
        }
        assert_eq!(world.resource::<PracticeBattle>().battle.tick, 30);
        assert_eq!(world.resource::<PracticeBattle>().ticks, 30);
        assert_eq!(world.resource::<BattleFeed>().battle().unwrap().tick, 30);

        // Befehle wirken ohne weiteren Takt, also auch in der Pause, und stehen sofort im
        // Feed, aus dem die Tafeln lesen.
        world
            .resource_mut::<Messages<BattleOrder>>()
            .write(BattleOrder::SetOrders {
                side: 0,
                formation: Formation::Wedge,
                doctrine: Doctrine::Defensive,
            });
        world.run_system_once(apply_orders).unwrap();
        let shown = world.resource::<BattleFeed>().battle().unwrap();
        assert_eq!(shown.tick, 30);
        assert_eq!(shown.combatants[0].formation, Formation::Wedge);
        assert_eq!(shown.combatants[0].doctrine, Doctrine::Defensive);
        assert_eq!(shown.combatants[1].formation, Formation::Line);
        assert!(!shown.combatants[0].retreating);

        world
            .resource_mut::<Messages<BattleOrder>>()
            .write(BattleOrder::Retreat { side: 0 });
        world.run_system_once(apply_orders).unwrap();
        let shown = world.resource::<BattleFeed>().battle().unwrap();
        assert!(shown.combatants[0].retreating);
        assert!(
            shown
                .ships
                .iter()
                .filter(|ship| ship.side == 0)
                .all(|ship| ship.status != ShipStatus::Active)
        );
        // Nach dem Rückzug der einen Seite läuft das Gefecht aus und bleibt dann stehen.
        for _ in 0..12_000 {
            world.run_system_once(step).unwrap();
            if world.resource::<PracticeBattle>().battle.finished {
                break;
            }
        }
        let ended = world.resource::<PracticeBattle>().battle.tick;
        assert!(world.resource::<PracticeBattle>().battle.finished);
        world.run_system_once(step).unwrap();
        assert_eq!(world.resource::<PracticeBattle>().battle.tick, ended);
        assert!(world.resource::<BattleFeed>().battle().unwrap().finished);
    }

    #[test]
    fn lineup_is_a_still_image() {
        use bevy::ecs::system::RunSystemOnce;

        let data = data();
        let config = PracticeConfig::lineup(&data);
        let battle = config.build(&data);
        let info = feed_info(&config);
        assert!(info.still && !info.can_restart && info.player_side.is_none());
        let mut feed = BattleFeed::default();
        feed.begin(&battle, info);
        let mut world = World::new();
        world.insert_resource(data);
        world.insert_resource(feed);
        world.insert_resource(PracticeBattle::new(battle, config));
        for _ in 0..5 {
            world.run_system_once(step).unwrap();
        }
        assert_eq!(world.resource::<PracticeBattle>().battle.tick, 0);
    }

    #[test]
    fn fleet_sizes_are_bounded() {
        let data = data();
        // Was beim Tippen einer großen Zahl durchlaufen wird, was sich aufsummiert, und
        // Einträge ohne Schiffe: nichts davon stürzt ab, alles wird abgelehnt.
        for text in [
            "horst:200000000",
            "horst:4294967295",
            "dolch:4000000000,dolch:4000000000",
            "dolch:20000",
            "dolch:1001",
            "dolch:600,waechter:401",
            // 41 Träger bringen 984 Jäger mit: zusammen über der Grenze.
            "horst:41",
            "dolch:0",
            "speer:2,dolch:0",
        ] {
            assert!(fleet_summary(&data, text).is_err(), "{text}");
            for side in 0..2 {
                let mut setup = PracticeSetup::default();
                setup.fleets[side] = text.into();
                let error = setup.resolve(&data).unwrap_err();
                assert!(error.contains(SIDE_NAMES[side]), "{text}: {error}");
            }
        }
        // An der Grenze geht es noch, Bordjäger mitgezählt.
        for text in [
            "dolch:1000",
            "horst:40",
            "dolch:1",
            "dolch:500,waechter:500",
        ] {
            assert!(fleet_summary(&data, text).is_ok(), "{text}");
            let mut setup = PracticeSetup::default();
            setup.fleets[1] = text.into();
            assert!(setup.resolve(&data).is_ok(), "{text}");
        }
        assert!(
            fleet_summary(&data, "horst:40")
                .unwrap()
                .contains("960 Bordjäger")
        );
        // Die Hinweise sagen, was nicht geht und was ginge.
        let too_many = fleet_summary(&data, "horst:20000000").unwrap_err();
        assert!(
            too_many.contains("20000000 Schiffe")
                && too_many.contains("480000000 Bordjäger")
                && too_many.contains(&format!("höchstens {MAX_SHIPS_PER_SIDE}")),
            "{too_many}"
        );
        let none = fleet_summary(&data, "speer:2,dolch:0").unwrap_err();
        assert!(
            none.contains("dolch") && none.contains("mindestens ein Schiff"),
            "{none}"
        );
    }

    #[test]
    fn the_dialog_rejects_oversized_fleets_and_starts_valid_ones() {
        use crate::ui::testing::Screen;

        /// Zeichnet den Dialog und hält fest, ob er ein Gefecht beginnt.
        fn draw<'a>(
            data: &'a GameData,
            setup: &'a mut PracticeSetup,
            started: &'a mut Option<PracticeConfig>,
        ) -> impl FnMut(&mut egui::Ui) + 'a {
            move |ui| {
                let mut open = true;
                if let Some(config) = setup_dialog(ui.ctx(), data, setup, &mut open) {
                    *started = Some(config);
                }
            }
        }

        let data = data();
        let mut screen = Screen::new(1600.0, 900.0);
        let mut setup = PracticeSetup::default();
        let mut started = None;

        // Jede Stufe einer getippten Riesenzahl wird gezeichnet, ohne abzustürzen; ab der
        // Grenze steht der Hinweis da, und „Gefecht beginnen“ tut nichts.
        let mut button = None;
        for digits in 1..=10 {
            setup.fleets[0] = format!("horst:2{}", "0".repeat(digits - 1));
            let painted = screen.settle(draw(&data, &mut setup, &mut started));
            let hint = painted.text("höchstens");
            assert_eq!(
                hint.is_some(),
                digits >= 3,
                "Hinweis bei {}",
                setup.fleets[0]
            );
            button = painted.text("Gefecht beginnen");
            let at = button.expect("Knopf").center();
            screen.click(at, draw(&data, &mut setup, &mut started));
            assert_eq!(started.is_some(), digits < 3, "{}", setup.fleets[0]);
            started = None;
        }
        // Eine Flotte ohne Schiffe ebenso.
        setup.fleets[0] = "dolch:0".into();
        let painted = screen.settle(draw(&data, &mut setup, &mut started));
        assert!(painted.text("mindestens").is_some());
        screen.click(
            button.unwrap().center(),
            draw(&data, &mut setup, &mut started),
        );
        assert!(started.is_none());
        // Mit gültigen Flotten beginnt derselbe Klick das Gefecht.
        setup.fleets[0] = "dolch:3".into();
        screen.settle(draw(&data, &mut setup, &mut started));
        screen.click(
            button.unwrap().center(),
            draw(&data, &mut setup, &mut started),
        );
        let config = started.expect("das Gefecht beginnt");
        assert_eq!(config.sides[0].ship_count(), 3);
    }

    #[test]
    fn restart_stops_the_clock_at_once() {
        use bevy::ecs::message::Messages;
        use bevy::ecs::system::RunSystemOnce;

        let data = data();
        let config = PracticeSetup::default().resolve(&data).unwrap();
        let mut battle = config.build(&data);
        let mut feed = BattleFeed::default();
        feed.begin(&battle, feed_info(&config));
        for _ in 0..40 {
            battle.step(&data);
            feed.refresh(&battle);
        }
        let epoch = feed.epoch;
        let mut world = World::new();
        world.insert_resource(data);
        world.insert_resource(feed);
        world.insert_resource(PracticeBattle::new(battle, config));
        // Die Zeit läuft auf der schnellsten Stufe, wie meist am Ende eines Gefechts.
        world.insert_resource(SimSpeed {
            paused: false,
            level: 5,
        });
        world.insert_resource(Time::<Virtual>::default());
        world.init_resource::<NextState<AppState>>();
        world.init_resource::<Messages<BattleViewRequest>>();
        assert!(!world.resource::<Time<Virtual>>().is_paused());

        world
            .resource_mut::<Messages<BattleViewRequest>>()
            .write(BattleViewRequest::Restart);
        world.run_system_once(handle_view_requests).unwrap();
        // Sofort, nicht erst wenn die Zeitsteuerung die neue Stufe überträgt: Bis dahin
        // liefe das frische Gefecht schon einige Takte weiter.
        assert!(world.resource::<Time<Virtual>>().is_paused());
        assert!(world.resource::<SimSpeed>().paused);
        assert_eq!(world.resource::<PracticeBattle>().battle.tick, 0);
        let feed = world.resource::<BattleFeed>();
        assert_eq!(feed.battle().unwrap().tick, 0);
        assert_eq!(feed.epoch, epoch + 1, "die Ansicht baut sich neu auf");
    }

    #[test]
    fn a_restarted_practice_waits_at_the_first_tick() {
        use crate::testing::TestGame;

        let mut game = TestGame::start("--practice --seed 3");
        game.frames(4);
        assert_eq!(game.state(), AppState::Practice);
        *game.world_mut().resource_mut::<SimSpeed>() = SimSpeed {
            paused: false,
            level: 5,
        };
        game.frames(12);
        assert!(game.resource::<PracticeBattle>().battle.tick > 20);
        game.send(BattleViewRequest::Restart);
        game.frames(8);
        assert_eq!(game.resource::<PracticeBattle>().battle.tick, 0);
        assert_eq!(game.resource::<BattleFeed>().battle().unwrap().tick, 0);
        assert!(game.resource::<SimSpeed>().paused);
        // Die Leertaste beginnt das Gefecht dann wie beim ersten Mal.
        game.key(KeyCode::Space);
        game.frames(4);
        assert!(game.resource::<PracticeBattle>().battle.tick > 0);
    }

    #[test]
    fn orders_clicked_on_the_side_panel_reach_the_battle() {
        use crate::testing::TestGame;

        let mut game = TestGame::start("--practice --seed 3");
        game.frames(4);
        let orders = |game: &TestGame| {
            let combatant = &game.resource::<PracticeBattle>().battle.combatants[0];
            (
                combatant.formation,
                combatant.doctrine,
                combatant.retreating,
            )
        };
        let start = orders(&game);
        assert_eq!(start, (Formation::Line, Doctrine::Balanced, false));
        // Die Befehle stehen unten auf der Lagetafel der eigenen Partei. Einmal über alle
        // Knöpfe dort klicken: Formation, Doktrin und zuletzt der Rückzug kommen im
        // Gefecht an, auch in der Pause.
        let panel = game.window_rect("partei_0").expect("Lagetafel");
        let (mut formations, mut doctrines) = (Vec::new(), Vec::new());
        let mut y = panel.bottom() - 6.0;
        while y > panel.top() + 190.0 && !orders(&game).2 {
            let mut x = panel.left() + 12.0;
            while x < panel.right() - 6.0 && !orders(&game).2 {
                game.click(Vec2::new(x, y));
                let now = orders(&game);
                if !formations.contains(&now.0) {
                    formations.push(now.0);
                }
                if !doctrines.contains(&now.1) {
                    doctrines.push(now.1);
                }
                x += 14.0;
            }
            y -= 9.0;
        }
        assert!(orders(&game).2, "der Rückzug wurde befohlen");
        assert_eq!(
            game.resource::<PracticeBattle>().battle.tick,
            0,
            "ohne einen Takt"
        );
        // Der Rückzugsknopf liegt zuunterst; die übrigen Befehle sind danach gesperrt.
        // Ein zweiter Durchgang von oben nach unten erreicht deshalb erst sie.
        let mut game = TestGame::start("--practice --seed 3");
        game.frames(4);
        let panel = game.window_rect("partei_0").expect("Lagetafel");
        let mut y = panel.top() + 200.0;
        while y < panel.bottom() - 26.0 {
            let mut x = panel.left() + 12.0;
            while x < panel.right() - 6.0 {
                game.click(Vec2::new(x, y));
                let now = orders(&game);
                if !formations.contains(&now.0) {
                    formations.push(now.0);
                }
                if !doctrines.contains(&now.1) {
                    doctrines.push(now.1);
                }
                x += 14.0;
            }
            y += 9.0;
        }
        assert!(!orders(&game).2);
        assert_eq!(formations.len(), Formation::ALL.len(), "{formations:?}");
        assert_eq!(doctrines.len(), Doctrine::ALL.len(), "{doctrines:?}");
        // Die Ansicht zeigt, was befohlen wurde.
        let shown = game.resource::<BattleFeed>().battle().unwrap().combatants[0].formation;
        assert_eq!(shown, orders(&game).0);
    }

    #[test]
    fn dialog_draws_without_a_window() {
        let data = data();
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1600.0, 900.0),
            )),
            ..Default::default()
        };
        let mut setup = PracticeSetup::default();
        setup.fleets[1] = "unsinn".into();
        let mut open = true;
        let mut started = None;
        let mut output = ctx.run_ui(input, |ui| {
            started = setup_dialog(ui.ctx(), &data, &mut setup, &mut open);
        });
        output.textures_delta.clear();
        assert!(started.is_none(), "ohne Eingabe beginnt kein Gefecht");
        assert!(open);
    }
}

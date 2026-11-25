//! Gefechtsübung: ein Gefecht außerhalb jeder Partie, mit frei gewählten Flotten.
//!
//! Die Übung besitzt ihr Gefecht selbst. Sie rechnet es im festen Takt weiter (Pause und
//! Geschwindigkeitsstufen wirken wie in der Partie), legt den Zustand nach jedem Takt im
//! [`BattleFeed`] ab und führt die Befehle des Spielers unmittelbar aus. Dasselbe wird
//! später die Partie für ihre Gefechte tun.

use super::camera::ViewOptions;
use super::BattleFeedSet;
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
        Side::parse(data, &self.fleets[index]).map(|side| {
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
                *speed = SimSpeed::default();
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

/// Kurzbeschreibung einer Flottenliste oder der Grund, warum sie nicht gilt.
fn fleet_summary(data: &GameData, text: &str) -> Result<String, String> {
    let side = Side::parse(data, text)?;
    let fighters: u32 = side
        .ships
        .iter()
        .map(|(design, count)| data.designs[*design as usize].stats.fighters * count)
        .sum();
    let mut summary = format!(
        "{} Schiffe, Bauwert {}",
        side.ship_count(),
        decimal(side.value(data), 0)
    );
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

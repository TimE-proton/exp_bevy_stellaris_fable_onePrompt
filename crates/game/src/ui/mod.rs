//! Oberfläche (egui). Die Fenster sind reine Funktionen über dem Simulationszustand:
//! Sie lesen, zeichnen und geben Aktionen zurück. Erst das System unten wendet sie an.

pub(crate) mod budget;
mod economy;
mod finance;
pub(crate) mod fleets;
pub(crate) mod format;
pub(crate) mod panels;
pub(crate) mod siege;
pub(crate) mod tech;
#[cfg(test)]
pub(crate) mod testing;

pub use panels::{SimRefs, UiAction};

use crate::battle_view::BattleViewShown;
use crate::battle_view::campaign::{BattleMarkers, WatchBattle};
use crate::battle_view::orbit::WatchSiege;
use crate::battle_view::practice::{PracticeSetup, StartPractice};
use crate::endings::{self, Endings};
use crate::history::PriceHistory;
use crate::map::{FocusSystem, Selection};
use crate::pointer::UiCover;
use crate::session::{Notice, SessionRequest, quicksave_path};
use crate::siege_markers::SiegeMarkers;
use crate::state::AppState;
use crate::time_control::SimSpeed;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy_egui::input::egui_wants_any_keyboard_input;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass};
use sim::chronicle::Chronicle;
use sim::clock::SimClock;
use sim::command::{CommandQueue, CommandSource};
use sim::diplomacy::Diplomacy;
use sim::economy::Economy;
use sim::endgame::{Endgame, PlayerFate};
use sim::finance::Finance;
use sim::fleet::{Battles, Fleets};
use sim::galaxy::Galaxy;
use sim::logistics::Logistics;
use sim::polity::Polities;
use sim::siege::Sieges;
use sim::state::SimConfig;
use sim::tech::Research;

/// Reiter der linken Tafel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LeftTab {
    #[default]
    Overview,
    Market,
    Production,
    Ownership,
}

/// Reiter des Forschungsfensters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TechTab {
    /// Der eigene Baum mit Ziel und Etat.
    #[default]
    Own,
    /// Was über die Forschung der anderen Gemeinschaften bekannt ist.
    Others,
}

/// Reiter des Flottenfensters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FleetsTab {
    /// Die eigenen Flotten und die Schiffe der gewählten.
    #[default]
    Fleets,
    /// Werften, Bauaufträge, Entwürfe und was die Flotte kostet.
    Yards,
}

/// Zustand der Oberfläche selbst (kein Spielzustand).
#[derive(Resource, Debug, Clone)]
pub struct UiState {
    pub seed_text: String,
    pub menu_open: bool,
    pub help_open: bool,
    /// Aufstellungsdialog der Gefechtsübung im Hauptmenü.
    pub practice_open: bool,
    pub tab: LeftTab,
    pub flows_open: bool,
    pub chains_open: bool,
    pub fleets_open: bool,
    pub fleets_tab: FleetsTab,
    /// Flotte, deren Schiffe das Flottenfenster zeigt.
    pub fleet_chosen: Option<sim::ids::FleetId>,
    /// Schiffe der gewählten Flotte, die abgeteilt werden sollen.
    pub split_ships: Vec<sim::ids::ShipId>,
    /// Werft, bei der bestellt wird.
    pub yard_chosen: Option<sim::ids::PlanetId>,
    /// Entwurf, dessen Kosten das Fenster ausweist.
    pub design_chosen: Option<u16>,
    pub tech_open: bool,
    pub tech_tab: TechTab,
    /// Forschungsetat in Prozent, solange der Spieler am Regler dreht und noch nicht
    /// festgesetzt hat; sonst zeigt der Regler den geltenden Etat.
    pub research_percent: Option<f32>,
    /// Güter, deren Preisverlauf im Diagramm gezeigt wird (Index = Gut).
    pub chart_goods: Vec<bool>,
    pub exchange_open: bool,
    pub treasury_open: bool,
    /// An der Börse gewählter Emittent.
    pub issuer: Option<sim::ids::IssuerId>,
    pub exchange_all: bool,
    pub trade_percent: f32,
    pub premium_percent: f32,
    pub tax_percent: f32,
    pub reserve_target: f32,
    pub bond_amount: f32,
    /// Fenster „Belagerung“ und die Tafeln der Verteidigung.
    pub siege: siege::SiegeUi,
    /// Was aus der Gemeinschaft des Spielers geworden ist; die Kopfleiste nennt es, wenn
    /// er nur noch zusieht.
    pub player_fate: PlayerFate,
}

impl UiState {
    /// Vergisst, was nur für die Partie galt, die gerade endet oder ersetzt wird: den
    /// gewählten Emittenten, die Eingaben zu Staatshaushalt und Forschungsetat und die Wahl
    /// im Flottenfenster. Unter
    /// gleicher Nummer stünde in der nächsten Partie ein anderer Betrieb, und Steuersatz
    /// und Etat gälten einem anderen Staat. Welche Fenster offen sind, bleibt, wie der
    /// Spieler es eingerichtet hat.
    fn forget_game(&mut self) {
        let fresh = Self::default();
        self.issuer = fresh.issuer;
        self.tax_percent = fresh.tax_percent;
        self.reserve_target = fresh.reserve_target;
        self.research_percent = fresh.research_percent;
        self.fleet_chosen = fresh.fleet_chosen;
        self.split_ships = fresh.split_ships;
        self.yard_chosen = fresh.yard_chosen;
        self.design_chosen = fresh.design_chosen;
        self.siege.forget_game();
    }
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            seed_text: "1".into(),
            menu_open: false,
            help_open: false,
            practice_open: false,
            tab: LeftTab::default(),
            flows_open: false,
            chains_open: false,
            fleets_open: false,
            fleets_tab: FleetsTab::default(),
            fleet_chosen: None,
            split_ships: Vec::new(),
            yard_chosen: None,
            design_chosen: None,
            tech_open: false,
            tech_tab: TechTab::default(),
            research_percent: None,
            chart_goods: Vec::new(),
            exchange_open: false,
            treasury_open: false,
            issuer: None,
            exchange_all: false,
            trade_percent: 5.0,
            premium_percent: 30.0,
            tax_percent: 0.0,
            reserve_target: 0.0,
            bond_amount: 20_000.0,
            siege: siege::SiegeUi::default(),
            player_fate: PlayerFate::default(),
        }
    }
}

/// Was der Spieler erfährt, wenn er bei stehender Zeit einen Befehl gibt.
pub const COMMAND_WAITS: &str =
    "Vorgemerkt: Der Befehl gilt, sobald die Zeit weiterläuft (Leertaste).";

/// Lesender Zugriff auf die laufende Partie; fehlt im Hauptmenü.
#[derive(SystemParam)]
pub struct SimParams<'w> {
    galaxy: Res<'w, Galaxy>,
    polities: Res<'w, Polities>,
    clock: Res<'w, SimClock>,
    config: Res<'w, SimConfig>,
    chronicle: Res<'w, Chronicle>,
    economy: Res<'w, Economy>,
    finance: Res<'w, Finance>,
    logistics: Res<'w, Logistics>,
    fleets: Res<'w, Fleets>,
    battles: Res<'w, Battles>,
    research: Res<'w, Research>,
    diplomacy: Res<'w, Diplomacy>,
    sieges: Res<'w, Sieges>,
}

/// Was nur das Hauptmenü und die Schlachtansicht brauchen.
#[derive(SystemParam)]
pub struct PracticeParams<'w> {
    setup: ResMut<'w, PracticeSetup>,
    start: MessageWriter<'w, StartPractice>,
    battle_view: Res<'w, BattleViewShown>,
    markers: Res<'w, BattleMarkers>,
    watch: MessageWriter<'w, WatchBattle>,
    watch_siege: MessageWriter<'w, WatchSiege>,
    siege_markers: Res<'w, SiegeMarkers>,
    endings: ResMut<'w, Endings>,
    endgame: Option<Res<'w, Endgame>>,
}

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiState>()
            // Am selben Punkt, an dem die Karte der Partie abgeräumt wird: beim Verlassen
            // der Partie, auch wenn gleich die nächste folgt (Laden mitten im Spiel).
            .add_systems(OnExit(AppState::InGame), forget_game)
            .add_systems(
                Update,
                hotkeys
                    .run_if(in_state(AppState::InGame))
                    .run_if(not(egui_wants_any_keyboard_input)),
            )
            .add_systems(EguiPrimaryContextPass, draw_ui);
    }
}

fn forget_game(mut ui_state: ResMut<UiState>) {
    ui_state.forget_game();
}

fn hotkeys(
    keys: Res<ButtonInput<KeyCode>>,
    mut ui_state: ResMut<UiState>,
    mut session: MessageWriter<SessionRequest>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        ui_state.menu_open = !ui_state.menu_open;
    }
    if keys.just_pressed(KeyCode::F1) {
        ui_state.help_open = !ui_state.help_open;
    }
    if keys.just_pressed(KeyCode::F5) {
        session.write(SessionRequest::Save(quicksave_path()));
    }
    if keys.just_pressed(KeyCode::F9) {
        session.write(SessionRequest::Load(quicksave_path()));
    }
}

fn draw_ui(
    mut contexts: EguiContexts,
    app_state: Res<State<AppState>>,
    data: Res<sim::GameData>,
    sim: Option<SimParams>,
    mut notice: ResMut<Notice>,
    history: Res<PriceHistory>,
    mut ui_state: ResMut<UiState>,
    mut speed: ResMut<SimSpeed>,
    mut selection: ResMut<Selection>,
    mut queue: Option<ResMut<CommandQueue>>,
    mut session: MessageWriter<SessionRequest>,
    mut focus: MessageWriter<FocusSystem>,
    mut exit: MessageWriter<AppExit>,
    mut practice: PracticeParams,
    mut cover: ResMut<UiCover>,
    mut styled: Local<bool>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    if !*styled {
        // Der egui-Kontext entsteht erst mit seiner Kamera, daher hier und nicht beim Start.
        panels::apply_style(ctx);
        *styled = true;
    }
    let mut root = UiCover::root(ctx, "wurzel");
    let mut actions = Vec::new();
    ui_state.player_fate = practice
        .endgame
        .as_ref()
        .map_or(PlayerFate::Active, |endgame| endgame.player);
    match (app_state.get(), &sim) {
        (AppState::InGame, _) if practice.battle_view.0 => {
            // In der Schlachtansicht und der Orbitansicht zeichnen diese ihre eigenen
            // Tafeln; von der Partie bleiben das Menü und die Meldung vom Ende einer
            // Gemeinschaft.
            panels::battle_menu(&mut root, &mut ui_state, &mut actions);
            endings::window(root.ctx(), &mut practice.endings);
        }
        (AppState::InGame | AppState::GameOver, Some(sim)) => {
            let refs = SimRefs {
                data: &data,
                galaxy: &sim.galaxy,
                polities: &sim.polities,
                clock: &sim.clock,
                config: &sim.config,
                chronicle: &sim.chronicle,
                economy: &sim.economy,
                finance: &sim.finance,
                logistics: &sim.logistics,
                fleets: &sim.fleets,
                battles: &sim.battles,
                research: &sim.research,
                diplomacy: &sim.diplomacy,
                sieges: &sim.sieges,
                history: &history,
                pending: queue.as_deref().map_or(&[], |queue| &queue.pending),
            };
            panels::in_game(
                &mut root,
                &refs,
                &mut ui_state,
                &speed,
                &selection,
                &practice.markers.0,
                &mut actions,
            );
            panels::siege_markers(
                root.ctx(),
                root.available_rect_before_wrap(),
                &practice.siege_markers.0,
                &mut actions,
            );
            endings::window(root.ctx(), &mut practice.endings);
        }
        (AppState::MainMenu, _) => {
            // Der Aufstellungsdialog der Gefechtsübung tritt an die Stelle des Menüs.
            if !ui_state.practice_open {
                panels::main_menu(
                    &mut root,
                    &mut ui_state,
                    quicksave_path().is_file(),
                    &mut actions,
                );
            }
            if let Some(config) = crate::battle_view::practice::setup_dialog(
                root.ctx(),
                &data,
                &mut practice.setup,
                &mut ui_state.practice_open,
            ) {
                practice.start.write(StartPractice(config));
            }
        }
        _ => {}
    }
    panels::notice(&mut root, &notice);
    // Was die Leisten und Tafeln belegen, gehört nicht der Karte.
    cover.record(&root);

    for action in actions {
        match action {
            UiAction::Session(request) => {
                ui_state.menu_open = false;
                session.write(request);
            }
            UiAction::TogglePause => speed.paused = !speed.paused,
            UiAction::SetSpeed(level) => {
                speed.level = level;
                speed.paused = false;
            }
            UiAction::Select(new) => *selection = new,
            UiAction::Focus(system) => {
                focus.write(FocusSystem(system));
            }
            UiAction::ShowIssuer(issuer) => {
                ui_state.issuer = Some(issuer);
                ui_state.exchange_open = true;
                ui_state.exchange_all = true;
            }
            UiAction::Command(command) => {
                if let (Some(queue), Some(sim)) = (queue.as_mut(), sim.as_ref())
                    && let Some(player) = sim.config.player
                {
                    queue.push(player, CommandSource::Player, command);
                    if speed.paused {
                        // Ein Befehl gilt erst mit dem nächsten Takt. Steht die Zeit, sähe
                        // es sonst aus, als hätte der Klick nichts bewirkt.
                        notice.info(COMMAND_WAITS);
                    }
                }
            }
            UiAction::ShowSiege(planet) => ui_state.siege.show(planet),
            UiAction::WatchSiege(planet) => {
                practice.watch_siege.write(WatchSiege(planet));
            }
            UiAction::WatchBattle(battle) => {
                practice.watch.write(WatchBattle(battle));
            }
            UiAction::Quit => {
                exit.write(AppExit::Success);
            }
        }
    }
    Ok(())
}

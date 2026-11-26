//! Flotten des Spielers: Liste mit Zustand und Befehlen, die Schiffe der gewählten Flotte
//! mit Vereinigen und Abteilen (`detail`), Werften, Bauaufträge und Kosten (`yards`).

mod detail;
mod yards;

use super::format::{decimal, percent};
use super::panels::{MUTED, SimRefs, UiAction, color32, detail as small};
use super::{FleetsTab, UiState};
use crate::map::Selection;
use bevy_egui::egui::{self, Color32, RichText};
use sim::clock::Date;
use sim::command::SimCommand;
use sim::data::{GameData, ShipClass};
use sim::fleet::{Battles, Fleet, Fleets};
use sim::ids::{FleetId, PolityId, SystemId};

/// Beschriftungen des Fensters. Das README nennt sie; ein Test hält beide beieinander.
pub const TITLE: &str = "Flotten";
pub const TAB_FLEETS: &str = "Flotten und Schiffe";
pub const TAB_YARDS: &str = "Werften und Kosten";
pub const MARCH: &str = "Marsch zum Ziel";
pub const RETREAT: &str = "Rückzug";
pub const SHOW_ON_MAP: &str = "Auf der Karte zeigen";
pub const MERGE: &str = "Vereinigen mit";
pub const SPLIT: &str = "Abteilen";
pub const ORDER: &str = "Bestellen";
pub const CANCEL: &str = "Streichen";

const BATTLE: Color32 = Color32::from_rgb(235, 110, 95);
const REGROUP: Color32 = Color32::from_rgb(240, 190, 80);
pub(super) const WARN: Color32 = REGROUP;

/// Macht aus dem Grund, den der Kern nennt, einen Satz für die Oberfläche.
pub(super) fn sentence(reason: String) -> String {
    let mut chars = reason.chars();
    let mut text: String = match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => return reason,
    };
    if !text.ends_with(['.', '!', '?']) {
        text.push('.');
    }
    text
}

/// Fragt den Kern, ob er einen Befehl an die Flotten annähme: Der Befehl läuft auf einer
/// Kopie der Flotten, die danach verworfen wird. So sperrt die Oberfläche einen Knopf aus
/// genau dem Grund, mit dem der Kern ablehnen würde, ohne seine Regeln nachzubauen.
pub(super) fn dry_run<T>(
    sim: &SimRefs,
    order: impl FnOnce(&mut Fleets) -> Result<T, String>,
) -> Result<(), String> {
    let mut scratch = sim.fleets.clone();
    order(&mut scratch).map(|_| ()).map_err(sentence)
}

/// Ein Knopf, der gesperrt ist, wenn der Kern den Befehl ablehnen würde; der Grund steht
/// im Schwebetext. `name` ist die Beschriftung für Vorleseprogramme, wo mehrere Knöpfe
/// gleich heißen. Liefert, ob geklickt wurde.
pub(super) fn gated_button(
    ui: &mut egui::Ui,
    text: impl Into<egui::WidgetText>,
    name: Option<String>,
    check: &Result<(), String>,
    hover: &str,
) -> bool {
    let enabled = check.is_ok();
    let mut response = ui.add_enabled(enabled, egui::Button::new(text));
    if let Some(name) = name {
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, name.clone())
        });
    }
    if !hover.is_empty() {
        response = response.on_hover_text(hover);
    }
    if let Err(reason) = check {
        response = response.on_disabled_hover_text(reason);
    }
    response.clicked()
}

/// Was eine Flotte gerade tut, so wie Karte und Flottenliste es zeigen. Gelesen wird nur,
/// was der Kern an der Flotte und an ihrem Gefecht führt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FleetState {
    /// Steht in einem Gefecht und kämpft.
    Fighting,
    /// Steht in einem Gefecht; der Rückzug ist befohlen, die Schiffe verlassen das Feld.
    Retreating,
    /// Sammelt sich nach dem Abzug aus einem Gefecht, bis zu diesem Tag.
    Regrouping { until: u64 },
    /// Auf dem Marsch.
    Marching,
    /// Liegt einsatzbereit in einem System.
    Stationed,
}

impl FleetState {
    pub fn of(fleet: &Fleet, battles: &Battles) -> Self {
        if fleet.in_battle {
            let retreating = battles
                .of_fleet(fleet.id)
                .is_some_and(|(battle, side)| battle.combatants[side].retreating);
            if retreating {
                Self::Retreating
            } else {
                Self::Fighting
            }
        } else if let Some(until) = fleet.regroup_until {
            Self::Regrouping { until }
        } else if !fleet.route.is_empty() {
            Self::Marching
        } else {
            Self::Stationed
        }
    }

    /// Steht die Flotte in einem Gefecht? Dann nimmt sie keinen Marschbefehl an.
    pub fn in_battle(self) -> bool {
        matches!(self, Self::Fighting | Self::Retreating)
    }

    pub fn label(self) -> String {
        match self {
            Self::Fighting => "im Gefecht".into(),
            Self::Retreating => "im Gefecht, Rückzug befohlen".into(),
            Self::Regrouping { until } => format!("sammelt sich bis {}", Date::from_day(until)),
            Self::Marching => "auf dem Marsch".into(),
            Self::Stationed => "einsatzbereit".into(),
        }
    }

    fn color(self) -> Color32 {
        match self {
            Self::Fighting | Self::Retreating => BATTLE,
            Self::Regrouping { .. } => REGROUP,
            Self::Marching | Self::Stationed => MUTED,
        }
    }
}

/// Schiffe einer Flotte je Klasse, schwere zuerst: „1 Kreuzer, 2 Fregatten, 3 Korvetten“.
pub fn ships_by_class(data: &GameData, fleet: &Fleet) -> String {
    let mut counts = [0u32; 6];
    for ship in &fleet.ships {
        counts[data.designs[ship.design as usize].stats.class.index()] += 1;
    }
    let parts: Vec<String> = [
        ShipClass::Carrier,
        ShipClass::Battleship,
        ShipClass::Cruiser,
        ShipClass::Frigate,
        ShipClass::Corvette,
        ShipClass::Fighter,
    ]
    .into_iter()
    .filter(|class| counts[class.index()] > 0)
    .map(|class| {
        let count = counts[class.index()];
        let name = if count == 1 {
            class.label()
        } else {
            class.plural()
        };
        format!("{count} {name}")
    })
    .collect();
    if parts.is_empty() {
        "keine Schiffe".into()
    } else {
        parts.join(", ")
    }
}

/// Wo die Flotte steht; mit einer Route das zuletzt verlassene System und ihr Ziel.
pub(super) fn location(sim: &SimRefs, fleet: &Fleet) -> String {
    let here = &sim.galaxy.system(fleet.system).name;
    match fleet.route.last() {
        Some(target) => format!("{} nach {}", here, sim.galaxy.system(*target).name),
        None => here.clone(),
    }
}

/// Auswahlfeld für einen stehenden Befehl; liefert die neue Wahl, wenn sie sich ändert.
pub(super) fn pick<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    id: (&str, u32),
    current: T,
    options: &[T],
    label: fn(T) -> &'static str,
) -> Option<T> {
    let mut chosen = current;
    egui::ComboBox::from_id_salt(id)
        .selected_text(label(current))
        .width(92.0)
        .show_ui(ui, |ui| {
            for option in options {
                ui.selectable_value(&mut chosen, *option, label(*option));
            }
        });
    (chosen != current).then_some(chosen)
}

/// Marsch oder Rückzug: Was die Flotte in ihrem Zustand annimmt, wird angeboten.
fn movement_button(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    fleet: &Fleet,
    state: FleetState,
    target: Option<SystemId>,
    actions: &mut Vec<UiAction>,
) {
    match state {
        FleetState::Fighting | FleetState::Retreating => {
            // Im Gefecht gibt es keinen Marsch: Der Weg hinaus ist der Rückzug.
            let ordered = state == FleetState::Retreating;
            let button = egui::Button::new(RichText::new(RETREAT).color(BATTLE));
            let response = ui
                .add_enabled(!ordered, button)
                .on_hover_text(
                    "Die Schiffe verlassen das Feld; was entkommt, weicht in ein Nachbarsystem \
                     aus und sammelt sich. Nicht umkehrbar.",
                )
                .on_disabled_hover_text(
                    "Der Rückzug ist befohlen; die Schiffe verlassen das Feld.",
                );
            if response.clicked() {
                actions.push(UiAction::Command(SimCommand::Retreat { fleet: fleet.id }));
            }
        }
        FleetState::Regrouping { until } => {
            ui.add_enabled(false, egui::Button::new(MARCH))
                .on_disabled_hover_text(format!(
                    "Die Flotte sammelt sich bis zum {} und nimmt so lange keinen Marschbefehl \
                     an. Einen Abzug führt sie zu Ende.",
                    Date::from_day(until)
                ));
        }
        FleetState::Marching | FleetState::Stationed => {
            let can_move =
                target.is_some_and(|system| system != fleet.system || !fleet.route.is_empty());
            let response = ui
                .add_enabled(can_move, egui::Button::new(MARCH))
                .on_disabled_hover_text(match target {
                    Some(system) => format!(
                        "Die Flotte liegt schon in {}.",
                        sim.galaxy.system(system).name
                    ),
                    None => "Zuerst ein System auf der Karte wählen.".to_string(),
                });
            if response.clicked()
                && let Some(to) = target
            {
                actions.push(UiAction::Command(SimCommand::MoveFleet {
                    fleet: fleet.id,
                    to,
                }));
            }
        }
    }
}

/// Die stehenden Befehle einer Flotte in Worten.
fn orders_text(fleet: &Fleet) -> String {
    format!("{} · {}", fleet.formation.label(), fleet.doctrine.label())
}

fn fleet_row(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    fleet: &Fleet,
    chosen: &mut Option<FleetId>,
    target: Option<SystemId>,
    actions: &mut Vec<UiAction>,
) {
    let state = FleetState::of(fleet, sim.battles);
    if ui
        .selectable_label(*chosen == Some(fleet.id), &fleet.name)
        .on_hover_text("Schiffe und Befehle dieser Flotte zeigen")
        .clicked()
    {
        *chosen = Some(fleet.id);
    }
    ui.vertical(|ui| {
        // Die Spalten sind so breit wie ihr Text; umbrochen wird in der Tabelle nicht.
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
        ui.label(location(sim, fleet));
        // Ein leerer Tank ändert, was die Flotte leisten kann; im Gefecht zählt er nicht.
        let dry = !fleet.has_fuel() && !state.in_battle();
        let note = match state {
            FleetState::Stationed if dry => "ohne Treibstoff".to_string(),
            _ if dry => format!("{}, Tank leer", state.label()),
            _ => state.label(),
        };
        let color = if dry { REGROUP } else { state.color() };
        let shown = ui.label(small(note).color(color));
        if dry {
            shown.on_hover_text(
                "Ohne Treibstoff schleppt sich die Flotte nur noch voran; sie sperrt kein \
                 System, kapert nicht und gibt kein Geleit.",
            );
        }
    });
    ui.vertical(|ui| {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
        ui.label(ships_by_class(sim.data, fleet));
        let (ammo, fuel) = fleet.supply(sim.data);
        ui.label(
            small(format!(
                "Stärke {} · Munition {} · Treibstoff {}",
                decimal(fleet.strength as f64, 0),
                percent(ammo),
                percent(fuel)
            ))
            .color(MUTED),
        );
    });
    ui.vertical(|ui| {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
        ui.label(fleet.stance.label());
        ui.label(small(orders_text(fleet)).color(MUTED));
    });
    movement_button(ui, sim, fleet, state, target, actions);
    ui.end_row();
}

/// Die Flotte, deren Einzelheiten gezeigt werden: die gewählte, solange es sie gibt, sonst
/// die erste eigene.
fn shown_fleet<'a>(
    sim: &'a SimRefs,
    player: PolityId,
    chosen: &mut Option<FleetId>,
) -> Option<&'a Fleet> {
    let found = chosen
        .and_then(|id| sim.fleets.get(id))
        .filter(|fleet| fleet.alive && fleet.owner == player)
        .or_else(|| sim.fleets.of(player).next());
    *chosen = found.map(|fleet| fleet.id);
    found
}

fn fleets_tab(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    player: PolityId,
    state: &mut UiState,
    selection: &Selection,
    actions: &mut Vec<UiAction>,
) {
    let target = selection.system;
    match target {
        Some(system) => ui.label(format!(
            "Marschziel: {} (auf der Karte gewählt)",
            sim.galaxy.system(system).name
        )),
        None => ui.label("Für einen Marschbefehl zuerst ein System auf der Karte wählen."),
    };
    let shown = shown_fleet(sim, player, &mut state.fleet_chosen);
    egui::Grid::new("flotten")
        .striped(true)
        .num_columns(5)
        .show(ui, |ui| {
            for title in [
                "Flotte",
                "Ort und Zustand",
                "Schiffe und Vorräte",
                "Auftrag, Aufstellung, Doktrin",
                "",
            ] {
                ui.label(RichText::new(title).color(MUTED).small());
            }
            ui.end_row();
            let mut any = false;
            for fleet in sim.fleets.of(player) {
                any = true;
                fleet_row(ui, sim, fleet, &mut state.fleet_chosen, target, actions);
            }
            if !any {
                ui.label(RichText::new("keine Flotte mehr").color(MUTED));
                ui.end_row();
            }
        });
    if let Some(fleet) = shown {
        ui.separator();
        detail::fleet_detail(ui, sim, player, fleet, state, actions);
    }
    ui.separator();
    yards::fleet_cost_line(ui, sim, player);
    egui::CollapsingHeader::new("Aufträge und Gefechte")
        .default_open(false)
        .show(ui, |ui| {
            ui.label(small(
                "Wache: hält die Stellung. Blockade: sperrt ein feindliches System, sobald \
                 dort keine Flotte des Besitzers mehr steht. Kaperfahrt: bringt feindliche \
                 Frachter auf. Geleitschutz: schützt eigene Frachter im selben System.",
            ));
            ui.label(small(format!(
                "Treffen verfeindete Flotten in einem System aufeinander, beginnt ein \
                 Gefecht. Solange es läuft, nimmt die Flotte keinen Marschbefehl an; \
                 Aufstellung und Doktrin gelten sofort, und der Rückzug lässt sich \
                 befehlen. Wer das Feld räumt, sammelt sich {} Tage und ist so lange aus \
                 dem Spiel.",
                sim.data.balance.military.regroup_days
            )));
        });

    if let Some(system) = target {
        let others: Vec<&Fleet> = sim
            .fleets
            .docked_in(system)
            .filter(|fleet| fleet.owner != player)
            .collect();
        if !others.is_empty() {
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!(
                    "Fremde Flotten in {}",
                    sim.galaxy.system(system).name
                ))
                .color(MUTED),
            );
            for fleet in others {
                let owner = sim.polities.get(fleet.owner);
                ui.label(
                    RichText::new(format!(
                        "{} · {} · Stärke {} · {} · {}",
                        fleet.name,
                        ships_by_class(sim.data, fleet),
                        decimal(fleet.strength as f64, 0),
                        fleet.stance.label(),
                        FleetState::of(fleet, sim.battles).label()
                    ))
                    .color(color32(owner.color)),
                );
            }
        }
    }
}

pub fn fleets_window(
    ctx: &egui::Context,
    sim: &SimRefs,
    state: &mut UiState,
    selection: &Selection,
    actions: &mut Vec<UiAction>,
) {
    let mut open = state.fleets_open;
    // Das Fenster rollt, wo Liste, Schiffe und Werften nicht mehr auf den Schirm passen.
    let room = (ctx.content_rect().height() - 140.0).max(240.0);
    egui::Window::new(TITLE)
        .open(&mut open)
        .default_pos([300.0, 56.0])
        .default_width(820.0)
        .default_height(room)
        .vscroll(true)
        .show(ctx, |ui| {
            let Some(player) = sim.config.player else {
                ui.label("Beobachter führen keine Flotten.");
                return;
            };
            ui.horizontal(|ui| {
                ui.selectable_value(&mut state.fleets_tab, FleetsTab::Fleets, TAB_FLEETS);
                ui.selectable_value(&mut state.fleets_tab, FleetsTab::Yards, TAB_YARDS);
            });
            ui.separator();
            match state.fleets_tab {
                FleetsTab::Fleets => fleets_tab(ui, sim, player, state, selection, actions),
                FleetsTab::Yards => yards::yards_tab(ui, sim, player, state, actions),
            }
        });
    state.fleets_open = open;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Notice;
    use crate::testing::TestGame;
    use crate::time_control::SimSpeed;
    use crate::ui::UiState;
    use sim::command::{CommandLog, CommandOutcome, CommandQueue, CommandSource};
    use sim::galaxy::Galaxy;
    use sim::polity::Polities;
    use sim::state::{SimConfig, SimState};

    #[test]
    fn ships_are_counted_by_class_and_the_state_follows_the_core() {
        let data = GameData::load_workspace().unwrap();
        let mut state = SimState::generate(
            &data,
            SimConfig {
                seed: 3,
                player: Some(PolityId(0)),
            },
        );
        state.galaxy.rebuild_index();
        // Die Startflotte aus den Spieldaten, schwere Schiffe zuerst.
        let mut expected = Vec::new();
        for class in [
            ShipClass::Carrier,
            ShipClass::Battleship,
            ShipClass::Cruiser,
            ShipClass::Frigate,
            ShipClass::Corvette,
        ] {
            let count: u32 = data
                .balance
                .military
                .starting_fleet_ids
                .iter()
                .filter(|(design, _)| data.designs[*design as usize].stats.class == class)
                .map(|(_, count)| *count)
                .sum();
            match count {
                0 => {}
                1 => expected.push(format!("1 {}", class.label())),
                more => expected.push(format!("{more} {}", class.plural())),
            }
        }
        let fleet = &mut state.fleets.list[0];
        assert_eq!(ships_by_class(&data, fleet), expected.join(", "));
        assert!(expected.len() >= 2, "eine gemischte Startflotte");

        let battles = Battles::default();
        assert_eq!(FleetState::of(fleet, &battles), FleetState::Stationed);
        let neighbor = state.galaxy.neighbors(fleet.system)[0].system;
        fleet.route = vec![neighbor];
        assert_eq!(FleetState::of(fleet, &battles), FleetState::Marching);
        // Wer sich sammelt, sammelt sich, auch auf dem Abzug.
        fleet.regroup_until = Some(45);
        let regrouping = FleetState::of(fleet, &battles);
        assert_eq!(regrouping, FleetState::Regrouping { until: 45 });
        assert_eq!(regrouping.label(), "sammelt sich bis 16.02.2400");
        // Das Gefecht geht allem vor.
        fleet.in_battle = true;
        assert!(FleetState::of(fleet, &battles).in_battle());
        assert!(!regrouping.in_battle() && !FleetState::Marching.in_battle());
    }

    fn run(game: &mut TestGame, frames: usize) {
        *game.world_mut().resource_mut::<SimSpeed>() = SimSpeed {
            paused: false,
            level: 5,
        };
        game.frames(frames);
        game.world_mut().resource_mut::<SimSpeed>().paused = true;
        game.frames(2);
    }

    /// Lässt die Partie laufen, bis `done` zutrifft; höchstens `limit` Frames.
    fn run_until(game: &mut TestGame, limit: usize, what: &str, done: impl Fn(&TestGame) -> bool) {
        *game.world_mut().resource_mut::<SimSpeed>() = SimSpeed {
            paused: false,
            level: 5,
        };
        for _ in 0..limit {
            if done(game) {
                game.world_mut().resource_mut::<SimSpeed>().paused = true;
                game.frames(2);
                return;
            }
            game.frames(1);
        }
        panic!("nach {limit} Frames noch nicht: {what}");
    }

    fn fleet(game: &TestGame, id: FleetId) -> &Fleet {
        game.resource::<Fleets>().get(id).expect("Flotte")
    }

    fn home_of(game: &TestGame, polity: PolityId) -> SystemId {
        let capital = game.resource::<Polities>().get(polity).capital.unwrap();
        game.resource::<Galaxy>().planet(capital).system
    }

    fn command(game: &mut TestGame, polity: PolityId, command: SimCommand) {
        game.world_mut().resource_mut::<CommandQueue>().push(
            polity,
            CommandSource::Player,
            command,
        );
    }

    #[test]
    fn a_fleet_in_a_battle_offers_retreat_and_not_march() {
        use crate::map::Selection;

        let mut game = TestGame::start("--seed 3");
        game.frames(4);
        let me = game.resource::<SimConfig>().player.expect("Spieler");
        let mine = game.resource::<Fleets>().of(me).next().expect("Flotte").id;
        let home = home_of(&game, me);
        // Die nächstgelegene andere Gemeinschaft; ihre Flotte liegt in ihrem Heimatsystem.
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
        command(&mut game, me, SimCommand::DeclareWar { target: enemy });
        command(
            &mut game,
            me,
            SimCommand::MoveFleet {
                fleet: mine,
                to: front,
            },
        );
        game.click_on("Flotten");
        run(&mut game, 4);
        // Marschziel ist das eigene Heimatsystem: Für eine Flotte, die woanders steht, ist
        // der Marsch dorthin ein gültiger Befehl.
        *game.world_mut().resource_mut::<Selection>() = Selection {
            system: Some(home),
            planet: None,
        };
        game.frames(3);
        // Unterwegs: Die Liste nennt die Schiffe und bietet den Marsch an, keinen Rückzug.
        assert!(!fleet(&game, mine).in_battle);
        let ships = ships_by_class(game.resource::<GameData>(), fleet(&game, mine));
        assert!(
            game.text(&ships).is_some(),
            "„{ships}“ steht nicht in der Liste"
        );
        assert!(game.text("auf dem Marsch").is_some());
        assert!(game.button("Marsch zum Ziel").is_some());
        assert_eq!(game.button("Rückzug"), None);

        // Am Ziel beginnt das Gefecht.
        run_until(&mut game, 4000, "Gefecht", |game| {
            fleet(game, mine).in_battle
        });
        assert_eq!(fleet(&game, mine).system, front);
        assert!(game.text("im Gefecht").is_some());
        assert_eq!(
            game.button("Marsch zum Ziel"),
            None,
            "im Gefecht wird kein Marsch angeboten"
        );
        assert!(game.button("Rückzug").is_some());
        // Ein Marschbefehl, der trotzdem ankommt, wird abgelehnt, und der Spieler erfährt es.
        command(
            &mut game,
            me,
            SimCommand::MoveFleet {
                fleet: mine,
                to: home,
            },
        );
        run(&mut game, 2);
        let notice = game.resource::<Notice>();
        assert!(
            notice.is_error && notice.text.contains("steht im Gefecht"),
            "{}",
            notice.text
        );
        assert!(fleet(&game, mine).in_battle && fleet(&game, mine).route.is_empty());

        // Der Rückzug geht als Befehl an den Kern.
        let entries = game.resource::<CommandLog>().entries.len();
        game.click_on("Rückzug");
        run(&mut game, 2);
        let log = game.resource::<CommandLog>();
        assert_eq!(log.entries.len(), entries + 1);
        let sent = log.entries.last().unwrap();
        assert_eq!(sent.command, SimCommand::Retreat { fleet: mine });
        assert_eq!(sent.outcome, CommandOutcome::Applied);
        let battles = game.resource::<Battles>();
        let (battle, side) = battles.of_fleet(mine).expect("die Flotte steht im Gefecht");
        assert!(battle.combatants[side].retreating);
        assert_eq!(
            FleetState::of(fleet(&game, mine), battles),
            FleetState::Retreating
        );
        // Befohlen ist befohlen: Der Knopf ist gesperrt, die Liste sagt, was geschieht.
        assert!(game.text("im Gefecht, Rückzug befohlen").is_some());
        assert_eq!(game.button("Rückzug"), None);
        assert_eq!(game.button("Marsch zum Ziel"), None);

        // Die Schiffe verlassen das Feld; danach sammelt sich die Flotte und nimmt so
        // lange keinen Marschbefehl an.
        run_until(&mut game, 4000, "Gefecht verlassen", |game| {
            !fleet(game, mine).in_battle
        });
        let after = fleet(&game, mine).clone();
        assert!(after.alive, "die Flotte hat den Rückzug überstanden");
        let until = after.regroup_until.expect("die Flotte sammelt sich");
        assert!(
            game.text(&FleetState::Regrouping { until }.label())
                .is_some(),
            "die Liste zeigt die Sammelfrist nicht"
        );
        assert_eq!(game.button("Marsch zum Ziel"), None);
        assert_eq!(game.button("Rückzug"), None);

        // Nach der Frist ist sie wieder einsatzbereit: Der Marsch wird wieder angeboten.
        run_until(&mut game, 4000, "Sammelfrist vorbei", |game| {
            !fleet(game, mine).is_regrouping()
        });
        assert!(game.button("Marsch zum Ziel").is_some());
        assert_eq!(game.button("Rückzug"), None);
    }

    /// Eine laufende Partie mit geöffnetem Flottenfenster.
    fn campaign_with_window() -> (TestGame, PolityId) {
        let mut game = TestGame::start("--seed 3");
        game.frames(4);
        game.click_on(TITLE);
        game.frames(3);
        assert!(game.window_rect(TITLE).is_some());
        let me = game.resource::<SimConfig>().player.expect("Spieler");
        (game, me)
    }

    #[test]
    fn the_window_fits_both_sizes_and_keeps_clicks_from_the_map() {
        use bevy::prelude::Vec2;

        let (mut game, _) = campaign_with_window();
        let home = game.resource::<Selection>().system;
        assert!(home.is_some());
        for size in [Vec2::new(1280.0, 720.0), Vec2::new(1600.0, 900.0)] {
            game.resize(size);
            game.frames(4);
            for tab in [TAB_YARDS, TAB_FLEETS] {
                game.click_on(tab);
                game.frames(3);
                let rect = game.window_rect(TITLE).expect("Fenster");
                assert!(
                    rect.left() >= 0.0
                        && rect.top() >= 0.0
                        && rect.right() <= size.x
                        && rect.bottom() <= size.y,
                    "das Fenster {rect:?} ragt bei {size} im Reiter „{tab}“ über den Rand"
                );
            }
            // Ein Klick in die Titelleiste gilt dem Fenster, nicht der Karte dahinter.
            let rect = game.window_rect(TITLE).expect("Fenster");
            game.click(Vec2::new(rect.center().x, rect.top() + 12.0));
            assert_eq!(game.resource::<Selection>().system, home, "{size}");
        }
        game.click_on(TITLE);
        game.frames(10);
        assert!(game.window_rect(TITLE).is_none());
    }

    #[test]
    fn fleets_are_split_and_merged_through_the_window() {
        let (mut game, me) = campaign_with_window();
        let first = game.resource::<Fleets>().of(me).next().expect("Flotte").id;
        let ships: Vec<_> = fleet(&game, first).ships.iter().map(|s| s.id).collect();
        assert!(ships.len() >= 3);
        // Ohne gewählte Schiffe ist „Abteilen“ gesperrt, und der Grund des Kerns steht da.
        assert_eq!(game.button(SPLIT), None);
        assert!(
            game.text("Keine Schiffe für die neue Flotte genannt.")
                .is_some()
        );
        // Allein im System: Auch zu vereinigen gibt es nichts.
        assert_eq!(game.button("Vereinigen"), None);
        let home = game
            .resource::<Galaxy>()
            .system(home_of(&game, me))
            .name
            .clone();
        assert!(
            game.text(&format!("Keine zweite eigene Flotte liegt in {home}."))
                .is_some()
        );
        // Alle Schiffe gewählt: Eines muss bleiben.
        for ship in &ships {
            game.click_on(&format!("Schiff {ship}"));
        }
        assert_eq!(game.button(SPLIT), None);
        assert!(
            game.text("Mindestens ein Schiff muss in der Flotte bleiben.")
                .is_some()
        );
        // Zwei Schiffe bleiben gewählt und werden abgeteilt.
        for ship in &ships[2..] {
            game.click_on(&format!("Schiff {ship}"));
        }
        assert!(
            game.text(&format!("2 von {} Schiffen gewählt", ships.len()))
                .is_some()
        );
        game.click_on(SPLIT);
        run(&mut game, 3);
        let log = game.resource::<CommandLog>();
        let sent = log.entries.last().expect("ein Befehl");
        assert_eq!(
            sent.command,
            SimCommand::SplitFleet {
                fleet: first,
                ships: ships[..2].to_vec()
            }
        );
        assert_eq!(sent.outcome, CommandOutcome::Applied);
        let mine: Vec<FleetId> = game.resource::<Fleets>().of(me).map(|f| f.id).collect();
        assert_eq!(mine.len(), 2);
        let second = mine[1];
        let taken: Vec<_> = fleet(&game, second).ships.iter().map(|s| s.id).collect();
        assert_eq!(taken, ships[..2]);
        assert_eq!(fleet(&game, first).ships.len(), ships.len() - 2);
        // Beide Flotten stehen in der Liste; die neue lässt sich wählen und zeigt ihre Schiffe.
        let name = fleet(&game, second).name.clone();
        game.click_on(&name);
        assert_eq!(game.resource::<UiState>().fleet_chosen, Some(second));
        assert!(game.button(&format!("Schiff {}", ships[0])).is_some());
        assert_eq!(game.button(&format!("Schiff {}", ships[2])), None);
        // Zurück in die erste Flotte.
        let into = fleet(&game, first).name.clone();
        game.click_on(&format!("{MERGE} {into}"));
        run(&mut game, 3);
        let log = game.resource::<CommandLog>();
        let sent = log.entries.last().expect("ein Befehl");
        assert_eq!(
            sent.command,
            SimCommand::MergeFleets {
                from: second,
                into: first
            }
        );
        assert_eq!(sent.outcome, CommandOutcome::Applied);
        assert_eq!(game.resource::<Fleets>().of(me).count(), 1);
        assert_eq!(fleet(&game, first).ships.len(), ships.len());
        // Das Fenster zeigt wieder die eine Flotte mit allen Schiffen.
        assert_eq!(game.resource::<UiState>().fleet_chosen, Some(first));
        for ship in &ships {
            assert!(game.button(&format!("Schiff {ship}")).is_some());
        }
    }

    #[test]
    fn a_ship_ordered_in_the_window_is_built_and_joins_a_fleet() {
        let (mut game, me) = campaign_with_window();
        game.click_on(TAB_YARDS);
        game.frames(2);
        let data = game.resource::<GameData>().clone();
        let capital = game.resource::<Polities>().get(me).capital.unwrap();
        let world = game.resource::<Galaxy>().planet(capital).name.clone();
        assert_eq!(game.resource::<UiState>().yard_chosen, Some(capital));
        let corvette = data.design_index("dolch").expect("Entwurf");
        let name = data.designs[corvette as usize].name.clone();
        // Ein Entwurf, dessen Technologie fehlt, ist gesperrt und nennt sie.
        let locked = data.design_index("speer_2").expect("Entwurf");
        let locked_name = &data.designs[locked as usize].name;
        assert_eq!(game.button(&format!("{ORDER}: {locked_name}")), None);
        let tech = &data.techs[data.designs[locked as usize].stats.techs[0] as usize].name;
        assert!(game.text(&format!("gesperrt · braucht: {tech}")).is_some());

        let ships_before = game.resource::<Fleets>().ship_count(me);
        game.click_on(&format!("{ORDER}: {name}"));
        run(&mut game, 3);
        let order = {
            let fleets = game.resource::<Fleets>();
            let orders: Vec<_> = fleets.orders_of(me).collect();
            assert_eq!(orders.len(), 1);
            assert_eq!((orders[0].planet, orders[0].design), (capital, corvette));
            orders[0].clone()
        };
        // Die Werft zeigt den Auftrag mit seinem Stand, wie der Kern ihn nennt.
        assert!(game.text(&format!("Nr. {}", order.id)).is_some());
        assert!(game.text(&order.status_text(&data)).is_some());
        // Ein zweiter Auftrag über die Klasse; er lässt sich wieder streichen.
        game.click_on(&format!("{ORDER}: Klasse {}", ShipClass::Frigate.label()));
        run(&mut game, 3);
        let second = {
            let fleets = game.resource::<Fleets>();
            let orders: Vec<_> = fleets.orders_of(me).collect();
            assert_eq!(orders.len(), 2);
            assert_eq!(
                Some(orders[1].design),
                data.best_design(
                    ShipClass::Frigate,
                    game.resource::<sim::tech::Research>().known(me)
                )
            );
            orders[1].id
        };
        game.click_on(&format!("{CANCEL}: Nr. {second}"));
        run(&mut game, 3);
        assert_eq!(game.resource::<Fleets>().orders_of(me).count(), 1);

        // Die Partie läuft, bis das Schiff fertig ist: Es stößt zur Flotte im System.
        run_until(&mut game, 20_000, "das Schiff ist gebaut", |game| {
            game.resource::<Fleets>().orders_of(me).count() == 0
        });
        let fleets = game.resource::<Fleets>();
        assert_eq!(fleets.ship_count(me), ships_before + 1);
        let joined = fleets
            .of(me)
            .flat_map(|fleet| fleet.ships.iter())
            .filter(|ship| ship.design == corvette)
            .count();
        let start = data
            .balance
            .military
            .starting_fleet_ids
            .iter()
            .filter(|(design, _)| *design == corvette)
            .map(|(_, count)| *count as usize)
            .sum::<usize>();
        assert_eq!(joined, start + 1);
        // Die Kosten der Flotte stehen im Fenster, wie der Haushalt sie führt.
        let budget = game.resource::<sim::economy::Economy>().budget(me).unwrap();
        let line = budget.last().unwrap().line(sim::economy::Claim::Fleet);
        let wanted = crate::ui::format::grouped(sim::economy::to_credits(line.wanted));
        assert!(
            game.text(&wanted).is_some(),
            "{wanted} steht nicht im Fenster"
        );

        // Eine volle Werft nimmt nichts mehr an: Der Knopf ist gesperrt, der Grund steht da.
        let limit = data.balance.military.yard_queue;
        for _ in 0..limit {
            game.world_mut().resource_mut::<CommandQueue>().push(
                me,
                CommandSource::Player,
                SimCommand::BuildShip {
                    planet: capital,
                    design: corvette,
                },
            );
        }
        run(&mut game, 3);
        assert_eq!(
            game.resource::<Fleets>().orders_at(capital).count(),
            limit as usize
        );
        assert_eq!(game.button(&format!("{ORDER}: {name}")), None);
        assert!(
            game.text(&format!(
                "Die Werft von {world} nimmt nicht mehr als {limit} Aufträge an."
            ))
            .is_some()
        );
    }
}

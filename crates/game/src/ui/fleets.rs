//! Flotten des Spielers: Schiffe, Zustand, Aufträge, Marsch und Rückzug.

use super::format::{decimal, percent};
use super::panels::{ACCENT, MUTED, SimRefs, UiAction, color32};
use crate::map::Selection;
use bevy_egui::egui::{self, Color32, RichText};
use sim::battle::{Doctrine, Formation};
use sim::clock::Date;
use sim::command::SimCommand;
use sim::data::{GameData, ShipClass};
use sim::fleet::{Battles, Fleet, Stance};

const BATTLE: Color32 = Color32::from_rgb(235, 110, 95);
const REGROUP: Color32 = Color32::from_rgb(240, 190, 80);

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

fn location(sim: &SimRefs, fleet: &Fleet) -> String {
    let here = &sim.galaxy.system(fleet.system).name;
    match fleet.route.last() {
        Some(target) => format!("{} → {}", here, sim.galaxy.system(*target).name),
        None => here.clone(),
    }
}

/// Auswahlfeld für einen stehenden Befehl; liefert die neue Wahl, wenn sie sich ändert.
fn pick<T: Copy + PartialEq>(
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
    target: Option<sim::ids::SystemId>,
    actions: &mut Vec<UiAction>,
) {
    match state {
        FleetState::Fighting | FleetState::Retreating => {
            // Im Gefecht gibt es keinen Marsch: Der Weg hinaus ist der Rückzug.
            let ordered = state == FleetState::Retreating;
            let button = egui::Button::new(RichText::new("Rückzug").color(BATTLE));
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
            ui.add_enabled(false, egui::Button::new("Marsch zum Ziel"))
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
                .add_enabled(can_move, egui::Button::new("Marsch zum Ziel"))
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

fn fleet_row(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    fleet: &Fleet,
    target: Option<sim::ids::SystemId>,
    actions: &mut Vec<UiAction>,
) {
    let state = FleetState::of(fleet, sim.battles);
    if ui
        .link(&fleet.name)
        .on_hover_text("System der Flotte wählen")
        .clicked()
    {
        actions.push(UiAction::Select(Selection {
            system: Some(fleet.system),
            planet: None,
        }));
        actions.push(UiAction::Focus(fleet.system));
    }
    ui.vertical(|ui| {
        ui.label(location(sim, fleet));
        let mut note = state.label();
        if !fleet.has_fuel() && !state.in_battle() {
            note.push_str(", Tank leer");
        }
        let shown = ui.label(RichText::new(note).color(state.color()).small());
        if !fleet.has_fuel() {
            shown.on_hover_text(
                "Ohne Treibstoff schleppt sich die Flotte nur noch voran; sie sperrt kein \
                 System, kapert nicht und gibt kein Geleit.",
            );
        }
    });
    ui.vertical(|ui| {
        ui.label(ships_by_class(sim.data, fleet));
        let (ammo, fuel) = fleet.supply(sim.data);
        ui.label(
            RichText::new(format!(
                "Stärke {} · Munition {} · Treibstoff {}",
                decimal(fleet.strength as f64, 0),
                percent(ammo),
                percent(fuel)
            ))
            .color(MUTED)
            .small(),
        );
    });
    if let Some(stance) = pick(
        ui,
        ("auftrag", fleet.id.0),
        fleet.stance,
        &Stance::ALL,
        Stance::label,
    ) {
        actions.push(UiAction::Command(SimCommand::SetStance {
            fleet: fleet.id,
            stance,
        }));
    }
    // Aufstellung und Doktrin gelten für künftige Gefechte und sofort in einem laufenden.
    let formation = pick(
        ui,
        ("aufstellung", fleet.id.0),
        fleet.formation,
        &Formation::ALL,
        Formation::label,
    );
    let doctrine = pick(
        ui,
        ("doktrin", fleet.id.0),
        fleet.doctrine,
        &Doctrine::ALL,
        Doctrine::label,
    );
    if formation.is_some() || doctrine.is_some() {
        actions.push(UiAction::Command(SimCommand::SetFleetOrders {
            fleet: fleet.id,
            formation: formation.unwrap_or(fleet.formation),
            doctrine: doctrine.unwrap_or(fleet.doctrine),
        }));
    }
    movement_button(ui, sim, fleet, state, target, actions);
    ui.end_row();
}

pub fn fleets_window(
    ctx: &egui::Context,
    sim: &SimRefs,
    open: &mut bool,
    selection: &Selection,
    actions: &mut Vec<UiAction>,
) {
    egui::Window::new("Flotten")
        .open(open)
        .default_pos([330.0, 110.0])
        .default_width(560.0)
        .show(ctx, |ui| {
            let Some(player) = sim.config.player else {
                ui.label("Beobachter führen keine Flotten.");
                return;
            };
            let target = selection.system;
            match target {
                Some(system) => ui.label(format!(
                    "Marschziel: {} (auf der Karte gewählt)",
                    sim.galaxy.system(system).name
                )),
                None => ui.label("Für einen Marschbefehl zuerst ein System auf der Karte wählen."),
            };
            egui::Grid::new("flotten")
                .striped(true)
                .num_columns(7)
                .show(ui, |ui| {
                    for title in [
                        "Flotte",
                        "Ort und Zustand",
                        "Schiffe",
                        "Auftrag",
                        "Aufstellung",
                        "Doktrin",
                        "",
                    ] {
                        ui.label(RichText::new(title).color(MUTED).small());
                    }
                    ui.end_row();
                    let mut any = false;
                    for fleet in sim.fleets.of(player) {
                        any = true;
                        fleet_row(ui, sim, fleet, target, actions);
                    }
                    if !any {
                        ui.label(RichText::new("keine Flotte mehr").color(MUTED));
                        ui.end_row();
                    }
                });
            ui.add_space(6.0);
            ui.label(RichText::new("Aufträge").color(ACCENT));
            ui.label(
                "Wache: hält die Stellung. Blockade: sperrt ein feindliches System, sobald dort \
                 keine Flotte des Besitzers mehr steht. Kaperfahrt: bringt feindliche Frachter \
                 auf. Geleitschutz: schützt eigene Frachter im selben System.",
            );
            ui.label(RichText::new("Gefechte").color(ACCENT));
            ui.label(format!(
                "Treffen verfeindete Flotten in einem System aufeinander, beginnt ein Gefecht. \
                 Solange es läuft, nimmt die Flotte keinen Marschbefehl an; Aufstellung und \
                 Doktrin gelten sofort, und der Rückzug lässt sich befehlen. Wer das Feld \
                 räumt, sammelt sich {} Tage und ist so lange aus dem Spiel.",
                sim.data.balance.military.regroup_days
            ));

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
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Notice;
    use crate::testing::TestGame;
    use crate::time_control::SimSpeed;
    use sim::command::{CommandLog, CommandOutcome, CommandQueue, CommandSource};
    use sim::fleet::Fleets;
    use sim::galaxy::Galaxy;
    use sim::ids::{FleetId, PolityId, SystemId};
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
}

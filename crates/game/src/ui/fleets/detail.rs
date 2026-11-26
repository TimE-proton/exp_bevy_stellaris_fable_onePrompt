//! Die gewählte Flotte: stehende Befehle, Vereinigen und Abteilen, und jedes Schiff mit
//! dem Zustand, den der Kern führt.

use super::{
    FleetState, MERGE, SHOW_ON_MAP, SPLIT, WARN, dry_run, dry_run_once, gated_button, location,
    pick,
};
use crate::map::Selection;
use crate::ui::UiState;
use crate::ui::format::percent;
use crate::ui::panels::{ACCENT, MUTED, SimRefs, UiAction, detail as small, watch_button};
use bevy_egui::egui::{self, RichText};
use sim::battle::{Doctrine, Formation};
use sim::command::SimCommand;
use sim::fleet::{Fleet, Ship, Stance, merge_fleets, split_fleet};
use sim::ids::PolityId;

/// Beschriftung eines Schiffs in der Liste: seine Nummer, wie der Kern sie führt.
pub fn ship_label(ship: &Ship) -> String {
    format!("Schiff {}", ship.id)
}

/// Auftrag, Aufstellung und Doktrin der Flotte als Auswahlfelder.
fn standing_orders(ui: &mut egui::Ui, fleet: &Fleet, actions: &mut Vec<UiAction>) {
    ui.label(RichText::new("Auftrag").color(MUTED));
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
    ui.label(RichText::new("Aufstellung").color(MUTED));
    let formation = pick(
        ui,
        ("aufstellung", fleet.id.0),
        fleet.formation,
        &Formation::ALL,
        Formation::label,
    );
    ui.label(RichText::new("Doktrin").color(MUTED));
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
}

/// Vereinigen mit einer anderen eigenen Flotte im selben System, Abteilen der gewählten
/// Schiffe. Ob der Kern den Befehl annähme, sagt er selbst (`dry_run`).
fn reorganize(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    player: PolityId,
    fleet: &Fleet,
    state: &mut UiState,
    actions: &mut Vec<UiAction>,
) {
    ui.horizontal_wrapped(|ui| {
        let here: Vec<&Fleet> = sim
            .fleets
            .of(player)
            .filter(|other| other.id != fleet.id && other.system == fleet.system)
            .collect();
        for other in &here {
            let command = SimCommand::MergeFleets {
                from: fleet.id,
                into: other.id,
            };
            let check = dry_run_once(sim, &command, |fleets| {
                merge_fleets(sim.data, fleets, player, fleet.id, other.id)
            });
            let label = format!("{MERGE} {}", other.name);
            if gated_button(
                ui,
                &label,
                None,
                &check,
                "Die Schiffe dieser Flotte gehen an die andere; diese Flotte besteht danach \
                 nicht mehr.",
            ) {
                actions.push(UiAction::Command(command));
            }
            if let Err(reason) = &check {
                ui.label(small(reason).color(WARN));
            }
        }
        if here.is_empty() {
            let reason = format!(
                "Keine zweite eigene Flotte liegt in {}.",
                sim.galaxy.system(fleet.system).name
            );
            gated_button(ui, "Vereinigen", None, &Err(reason.clone()), "");
            ui.label(small(reason).color(MUTED));
        }
    });
    ui.horizontal_wrapped(|ui| {
        let chosen = state.split_ships.clone();
        let check = dry_run(sim, |fleets| {
            split_fleet(sim.data, sim.polities, fleets, player, fleet.id, &chosen)
        });
        if gated_button(
            ui,
            SPLIT,
            None,
            &check,
            "Die gewählten Schiffe bilden im selben System eine neue Flotte.",
        ) {
            actions.push(UiAction::Command(SimCommand::SplitFleet {
                fleet: fleet.id,
                ships: chosen.clone(),
            }));
            state.split_ships.clear();
        }
        match &check {
            Ok(()) => ui.label(small(format!(
                "{} von {} Schiffen gewählt",
                chosen.len(),
                fleet.ships.len()
            ))),
            Err(reason) => ui.label(small(reason).color(MUTED)),
        };
    });
}

/// Die Schiffe der Flotte: Klasse, Entwurf und Zustand. Ein Klick auf die Nummer wählt
/// das Schiff zum Abteilen.
fn ship_table(ui: &mut egui::Ui, sim: &SimRefs, fleet: &Fleet, state: &mut UiState) {
    egui::Grid::new(("schiffe", fleet.id.0))
        .striped(true)
        .num_columns(9)
        .show(ui, |ui| {
            for title in [
                "Schiff",
                "Klasse",
                "Entwurf",
                "Rumpf",
                "Module",
                "Bordjäger",
                "Munition",
                "Treibstoff",
                "Zustand",
            ] {
                ui.label(RichText::new(title).color(MUTED).small());
            }
            ui.end_row();
            for ship in &fleet.ships {
                let design = &sim.data.designs[ship.design as usize];
                let chosen = state.split_ships.contains(&ship.id);
                if ui
                    .selectable_label(chosen, ship_label(ship))
                    .on_hover_text("Zum Abteilen wählen oder abwählen")
                    .clicked()
                {
                    if chosen {
                        state.split_ships.retain(|id| *id != ship.id);
                    } else {
                        state.split_ships.push(ship.id);
                    }
                }
                ui.label(design.stats.class.label());
                ui.label(&design.name);
                let share = |ui: &mut egui::Ui, value: f32, low: f32| {
                    let text = RichText::new(percent(value)).monospace();
                    ui.label(if value < low { text.color(WARN) } else { text });
                };
                share(ui, ship.hull, 1.0);
                share(ui, ship.module_condition(), 1.0);
                if design.stats.fighters > 0 {
                    let text = format!("{} von {}", ship.fighters, design.stats.fighters);
                    let text = RichText::new(text).monospace();
                    ui.label(if ship.fighters < design.stats.fighters {
                        text.color(WARN)
                    } else {
                        text
                    });
                } else {
                    ui.label(RichText::new("–").color(MUTED));
                }
                share(ui, ship.ammo, 0.25);
                share(ui, ship.fuel, 0.25);
                if ship.is_damaged(sim.data) {
                    ui.label(RichText::new("beschädigt").color(WARN))
                        .on_hover_text(format!(
                            "Kampfwert {} des Bauwerts. Instand gesetzt wird im eigenen Gebiet \
                             mit Werft, gegen Bauteile vom Markt.",
                            percent(ship.condition(sim.data))
                        ));
                } else {
                    ui.label(RichText::new("unversehrt").color(MUTED));
                }
                ui.end_row();
            }
        });
}

pub fn fleet_detail(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    player: PolityId,
    fleet: &Fleet,
    state: &mut UiState,
    actions: &mut Vec<UiAction>,
) {
    // Gewählt sein können nur Schiffe dieser Flotte.
    state
        .split_ships
        .retain(|id| fleet.ships.iter().any(|ship| ship.id == *id));
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(&fleet.name).color(ACCENT).strong());
        ui.label(format!(
            "{} · {}",
            location(sim, fleet),
            FleetState::of(fleet, sim.battles).label()
        ));
        if ui
            .button(SHOW_ON_MAP)
            .on_hover_text("Wählt das System der Flotte und führt die Karte dorthin.")
            .clicked()
        {
            actions.push(UiAction::Select(Selection {
                system: Some(fleet.system),
                planet: None,
            }));
            actions.push(UiAction::Focus(fleet.system));
        }
        if let Some((battle, _)) = sim.battles.of_fleet(fleet.id)
            && watch_button(ui, "")
        {
            actions.push(UiAction::WatchBattle(battle.id));
        }
    });
    ui.horizontal_wrapped(|ui| standing_orders(ui, fleet, actions));
    reorganize(ui, sim, player, fleet, state, actions);
    ship_table(ui, sim, fleet, state);
    let arrears = sim.fleets.arrears_of(player);
    ui.label(
        small(format!(
            "Einen Zustand der Besatzung je Schiff gibt es nicht: Ihre Moral im Gefecht \
             sinkt mit dem Soldrückstand der Gemeinschaft (zurzeit {}) und mit der \
             Kriegsmüdigkeit daheim.",
            percent(arrears)
        ))
        .color(if arrears > 0.005 { WARN } else { MUTED }),
    );
}

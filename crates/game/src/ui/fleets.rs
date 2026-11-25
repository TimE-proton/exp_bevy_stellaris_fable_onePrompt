//! Flotten des Spielers: Aufträge und Marschbefehle.

use super::format::decimal;
use super::panels::{ACCENT, MUTED, SimRefs, UiAction, color32};
use crate::map::Selection;
use bevy_egui::egui::{self, RichText};
use sim::command::SimCommand;
use sim::fleet::{Fleet, Stance};

fn location(sim: &SimRefs, fleet: &Fleet) -> String {
    let here = &sim.galaxy.system(fleet.system).name;
    match fleet.route.last() {
        Some(target) if !fleet.is_docked() || !fleet.route.is_empty() => {
            format!("{} → {}", here, sim.galaxy.system(*target).name)
        }
        _ => here.clone(),
    }
}

pub fn fleets_window(
    ctx: &egui::Context,
    sim: &SimRefs,
    open: &mut bool,
    selection: &Selection,
    actions: &mut Vec<UiAction>,
) {
    egui::Window::new("Flotten").open(open)
        .default_pos([470.0, 110.0]).default_width(560.0).show(ctx, |ui| {
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
        egui::Grid::new("flotten").striped(true).num_columns(5).show(ui, |ui| {
            for title in ["Flotte", "Ort", "Stärke", "Auftrag", ""] {
                ui.label(RichText::new(title).color(MUTED).small());
            }
            ui.end_row();
            let mut any = false;
            for fleet in sim.fleets.of(player) {
                any = true;
                if ui.link(&fleet.name).on_hover_text("System der Flotte wählen").clicked() {
                    actions.push(UiAction::Select(Selection { system: Some(fleet.system), planet: None }));
                    actions.push(UiAction::Focus(fleet.system));
                }
                let mut place = RichText::new(location(sim, fleet));
                if fleet.in_battle {
                    place = place.color(egui::Color32::from_rgb(235, 110, 95));
                }
                ui.label(place);
                ui.label(decimal(fleet.strength as f64, 0));
                let mut stance = fleet.stance;
                egui::ComboBox::from_id_salt(("auftrag", fleet.id.0))
                    .selected_text(stance.label())
                    .show_ui(ui, |ui| {
                        for option in Stance::ALL {
                            ui.selectable_value(&mut stance, option, option.label());
                        }
                    });
                if stance != fleet.stance {
                    actions.push(UiAction::Command(SimCommand::SetStance { fleet: fleet.id, stance }));
                }
                let can_move = target.is_some_and(|system| system != fleet.system || !fleet.route.is_empty());
                if ui.add_enabled(can_move, egui::Button::new("Marsch zum Ziel")).clicked()
                    && let Some(to) = target
                {
                    actions.push(UiAction::Command(SimCommand::MoveFleet { fleet: fleet.id, to }));
                }
                ui.end_row();
            }
            if !any {
                ui.label(RichText::new("keine Flotte mehr").color(MUTED));
                ui.end_row();
            }
        });
        ui.add_space(6.0);
        ui.label(RichText::new("Aufträge").color(ACCENT));
        ui.label("Wache: hält die Stellung. Blockade: sperrt ein feindliches System, sobald dort keine Flotte des Besitzers mehr steht. Kaperfahrt: bringt feindliche Frachter auf. Geleitschutz: schützt eigene Frachter im selben System.");

        if let Some(system) = target {
            let others: Vec<&Fleet> =
                sim.fleets.docked_in(system).filter(|fleet| fleet.owner != player).collect();
            if !others.is_empty() {
                ui.add_space(6.0);
                ui.label(RichText::new(format!("Fremde Flotten in {}", sim.galaxy.system(system).name)).color(MUTED));
                for fleet in others {
                    let owner = sim.polities.get(fleet.owner);
                    ui.label(
                        RichText::new(format!(
                            "{} · Stärke {} · {}",
                            fleet.name,
                            decimal(fleet.strength as f64, 0),
                            fleet.stance.label()
                        ))
                        .color(color32(owner.color)),
                    );
                }
            }
        }
    });
}

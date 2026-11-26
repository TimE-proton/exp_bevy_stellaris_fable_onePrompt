//! Die gewählte Flotte: stehende Befehle, Vereinigen und Abteilen, und jedes Schiff mit
//! dem Zustand, den der Kern führt.

use super::{
    FleetState, LAY_UP, MERGE, SHOW_ON_MAP, SPLIT, WAITING, WARN, apply_waiting, dry_run,
    dry_run_once, gated_button, location, pick, sentence, waiting,
};
use crate::map::Selection;
use crate::ui::UiState;
use crate::ui::format::grouped;
use crate::ui::format::percent;
use crate::ui::panels::{ACCENT, MUTED, SimRefs, UiAction, detail as small, watch_button};
use bevy_egui::egui::{self, RichText};
use sim::battle::{Doctrine, Formation};
use sim::chronicle::Chronicle;
use sim::command::SimCommand;
use sim::economy::{Money, to_credits};
use sim::fleet::{Fleet, Ship, Stance, fleet_claim, lay_up, merge_fleets, split_fleet};
use sim::ids::{FleetId, PolityId, ShipId};

/// Beschriftung eines Schiffs in der Liste: seine Nummer, wie der Kern sie führt.
pub fn ship_label(ship: &Ship) -> String {
    format!("Schiff {}", ship.id)
}

/// Was das Stilllegen der gewählten Schiffe brächte: Erlös und Ersparnis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayUpPreview {
    /// Erlös aus dem Verkauf der Bauteile, wie der Kern ihn am heutigen Markt erzielte.
    pub proceeds: Money,
    /// Um so viel sinkt, was die Flotte im Monat verlangt (Sold und Ersatzteile).
    pub saving: Money,
}

/// Der Probelauf von gestern gilt, solange sich nichts geändert hat: Takt, Flotte, Wahl
/// und Zahl der wartenden Befehle.
type LayUpKey = (u64, FleetId, Vec<ShipId>, usize);

/// Fragt den Kern, was das Stilllegen brächte: Der Befehl läuft auf einer Kopie von
/// Flotten und Wirtschaft, die danach verworfen wird. Lehnt der Kern ab, ist sein Grund
/// das Ergebnis. Vorgemerkte Befehle gelten auf der Kopie als geschehen.
pub fn lay_up_preview(
    sim: &SimRefs,
    player: PolityId,
    fleet: FleetId,
    ships: &[ShipId],
) -> Result<LayUpPreview, String> {
    let already = waiting(sim).any(|command| match command {
        SimCommand::LayUpShips { ships: named, .. } => {
            named.iter().any(|ship| ships.contains(ship))
        }
        _ => false,
    });
    if already {
        return Err(WAITING.into());
    }
    let polity = sim.polities.get(player);
    let mut fleets = sim.fleets.clone();
    let mut economy = sim.economy.clone();
    apply_waiting(sim, &mut fleets, Some(&mut economy));
    // Beide Male zu den Preisen von heute: Der Unterschied ist allein der der Schiffe.
    let before = fleet_claim(sim.data, sim.galaxy, sim.economy, &fleets, polity);
    let proceeds = lay_up(
        sim.data,
        sim.galaxy,
        &mut economy,
        sim.polities,
        &mut fleets,
        &mut Chronicle::default(),
        sim.clock.tick,
        player,
        fleet,
        ships,
    )
    .map_err(sentence)?;
    let after = fleet_claim(sim.data, sim.galaxy, sim.economy, &fleets, polity);
    Ok(LayUpPreview {
        proceeds,
        saving: before - after,
    })
}

/// Stilllegen der gewählten Schiffe: Erlös und Ersparnis stehen daneben, sonst der Grund
/// des Kerns.
fn lay_up_row(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    player: PolityId,
    fleet: &Fleet,
    state: &mut UiState,
    actions: &mut Vec<UiAction>,
) {
    let chosen = state.split_ships.clone();
    let key: LayUpKey = (sim.clock.tick, fleet.id, chosen.clone(), sim.pending.len());
    let id = egui::Id::new("stilllegen-probelauf");
    type Kept = (LayUpKey, Result<LayUpPreview, String>);
    let kept: Option<Kept> = ui.data(|data| data.get_temp(id));
    let preview = match kept {
        Some((known, preview)) if known == key => preview,
        _ => {
            let preview = lay_up_preview(sim, player, fleet.id, &chosen);
            ui.data_mut(|data| data.insert_temp::<Kept>(id, (key, preview.clone())));
            preview
        }
    };
    let check = preview.clone().map(|_| ());
    if gated_button(
        ui,
        LAY_UP,
        None,
        &check,
        "Die gewählten Schiffe verlassen das Spiel für immer; Sold und Wartung enden mit \
         ihnen. Einen Teil ihrer Bauteile verkauft der Staat am Markt des Systems. Nicht \
         umkehrbar.",
    ) {
        actions.push(UiAction::Command(SimCommand::LayUpShips {
            fleet: fleet.id,
            ships: chosen.clone(),
        }));
        state.split_ships.clear();
    }
    match preview {
        Ok(preview) => ui.label(small(format!(
            "{} von {} Schiffen: bringt {} Credits am heutigen Markt und spart {} Credits \
             im Monat an Sold und Ersatzteilen.",
            chosen.len(),
            fleet.ships.len(),
            grouped(to_credits(preview.proceeds)),
            grouped(to_credits(preview.saving))
        ))),
        Err(reason) => ui.label(small(reason).color(MUTED)),
    };
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
    ui.horizontal_wrapped(|ui| lay_up_row(ui, sim, player, fleet, state, actions));
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
                    .on_hover_text("Zum Abteilen oder Stilllegen wählen oder abwählen")
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

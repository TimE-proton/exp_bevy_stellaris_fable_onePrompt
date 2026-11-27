//! Eigene Flotten im Fenster „Belagerung“: eine Belagerung beginnen, reguläre Verbände
//! einschiffen und an Land setzen. Dieselben Zeilen stehen gekürzt im Fenster „Flotten“.

use super::{
    BEGIN, DISEMBARK, EMBARK, NO_COMMANDS, SHOW_SIEGE, SiegeTab, SiegeUi, WARN, days_text, gated,
    polity_name, units,
};
use crate::ui::fleets::{FleetState, location};
use crate::ui::panels::{ACCENT, MUTED, SimRefs, UiAction, detail as small};
use bevy_egui::egui::{self, RichText};
use sim::command::SimCommand;
use sim::fleet::Fleet;
use sim::ids::{FleetId, PlanetId, PolityId};

pub const TROOPS_AND_SIEGE: &str = "Truppen und Belagerung …";

/// Schritt der Regler für Verbände.
pub const STEP: f32 = 0.5;

/// Oberes Ende eines Reglers für `ready` Verbände; nie unter einem Schritt, damit der
/// Regler auch dann einen Bereich hat, wenn nichts bereitsteht.
pub fn slider_max(ready: f64) -> f32 {
    (ready as f32).max(STEP)
}

/// Rastet die Stellung eines Reglers auf halbe Verbände, am oberen Ende auf alles, was
/// bereitsteht. So fragt das Ziehen den Kern nur für wenige verschiedene Stärken.
pub fn snap(value: f32, most: f32) -> f32 {
    if value >= most - STEP * 0.5 {
        most
    } else {
        ((value / STEP).round() * STEP).clamp(STEP, most)
    }
}

/// Die Stärke für den Befehl: die Stellung des Reglers, höchstens was bereitsteht (der
/// Regler rechnet in einfacher Genauigkeit und läge sonst um ein Haar darüber).
pub fn commanded(strength: f32, ready: f64) -> f64 {
    let wanted = strength as f64;
    if ready > 0.0 {
        wanted.min(ready)
    } else {
        wanted
    }
}

/// Truppen an Bord einer Flotte: Verbände und Nachschub in Verbandstagen.
pub fn aboard(sim: &SimRefs, fleet: FleetId) -> (f64, f64) {
    sim.sieges
        .embarked
        .get(&fleet)
        .map_or((0.0, 0.0), |force| (force.troops, force.supply))
}

/// „3,0 Verbände an Bord, Nachschub für 30 Tage“ oder „keine Truppen an Bord“.
pub fn aboard_text(sim: &SimRefs, fleet: FleetId) -> String {
    let (troops, supply) = aboard(sim, fleet);
    if troops <= 0.0 {
        "keine Truppen an Bord".to_string()
    } else {
        format!(
            "{} Verbände an Bord, Nachschub für {}",
            units(troops),
            days_text(supply / troops)
        )
    }
}

/// Reguläre einer eigenen Welt, die an Bord gehen können: Wer im Untergrund einer
/// belagerten Welt sitzt, kommt nicht hinaus.
pub fn ready_regulars(sim: &SimRefs, planet: PlanetId) -> f64 {
    let deep = sim.sieges.siege(planet).map_or(0.0, |siege| siege.deep);
    (sim.sieges.defense(planet).regulars - deep).max(0.0)
}

/// Was die Flotte gerade belagert oder belagern soll.
pub fn target_text(sim: &SimRefs, fleet: FleetId) -> Option<String> {
    let planet = *sim.sieges.assign.get(&fleet)?;
    let name = &sim.galaxy.planet(planet).name;
    Some(if sim.sieges.siege(planet).is_some() {
        format!("belagert {name}")
    } else {
        format!("soll {name} belagern; die Belagerung beginnt mit der Orbitalhoheit")
    })
}

/// Je fremder Welt im System der Flotte der Knopf „Belagern“, gesperrt mit dem Grund des
/// Kerns; an der Welt, die sie schon belagert, der Weg zu deren Einzelheiten.
fn begin_buttons(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    player: PolityId,
    fleet: &Fleet,
    state: &mut SiegeUi,
    actions: &mut Vec<UiAction>,
) -> bool {
    let assigned = sim.sieges.assign.get(&fleet.id).copied();
    let mut any = false;
    for planet_id in &sim.galaxy.system(fleet.system).planets {
        let planet = sim.galaxy.planet(*planet_id);
        let Some(owner) = planet.owner.filter(|owner| *owner != player) else {
            continue;
        };
        any = true;
        if assigned == Some(*planet_id) {
            ui.label(RichText::new(&planet.name).color(ACCENT));
            if sim.sieges.siege(*planet_id).is_some() {
                if ui
                    .button(SHOW_SIEGE)
                    .on_hover_text("Phase, Lage, Prognose und Befehle dieser Belagerung.")
                    .clicked()
                {
                    state.show(*planet_id);
                }
            } else {
                ui.label(small("wartet auf die Orbitalhoheit").color(WARN));
            }
            continue;
        }
        let command = SimCommand::BeginSiege {
            fleet: fleet.id,
            planet: *planet_id,
        };
        let check = state.cache.check(sim, &command);
        let label = format!("{BEGIN}: {}", planet.name);
        if gated(
            ui,
            &label,
            label.clone(),
            &check,
            "Die Flotte belagert diese Welt. Die Belagerung beginnt am nächsten Tag, sobald \
             die Flotte den Orbit hält; ein früheres Ziel gibt sie dafür auf.",
        ) {
            actions.push(UiAction::Command(command));
        }
        ui.label(polity_name(sim, owner));
        if let Err(reason) = &check {
            ui.label(small(reason).color(MUTED));
        }
    }
    any
}

/// Einschiffen von einer eigenen Welt und an Land setzen auf ihr, mit den Reglern für
/// Stärke und Nachschub.
fn own_world(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    fleet: &Fleet,
    planet: PlanetId,
    state: &mut SiegeUi,
    actions: &mut Vec<UiAction>,
) {
    let name = &sim.galaxy.planet(planet).name;
    let ready = ready_regulars(sim, planet);
    let (troops, _) = aboard(sim, fleet.id);
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(name).color(ACCENT));
        ui.label(format!("{} reguläre Verbände bereit", units(ready)));
    });
    ui.horizontal_wrapped(|ui| {
        // Der Regler gilt für alle Welten und Flotten des Fensters: Hier zeigt er, was
        // davon an dieser Welt geht, und ändert die Wahl nur, wenn der Spieler ihn zieht.
        let most = slider_max(ready.max(troops));
        let mut strength = snap(state.strength, most);
        let slider = egui::Slider::new(&mut strength, STEP..=most)
            .custom_formatter(|value, _| crate::ui::format::decimal(value, 1))
            .custom_parser(|text| text.replace(',', ".").parse().ok())
            .suffix(" Verbände");
        if ui.add(slider).changed() {
            state.strength = snap(strength, most);
        }
        let cap = sim.data.balance.siege.supply_days_cap as u32;
        state.supply_days = state.supply_days.min(cap);
        ui.add(
            egui::Slider::new(&mut state.supply_days, 0..=cap)
                .step_by(5.0)
                .suffix(" Tage Nachschub"),
        )
        .on_hover_text(
            "Nachschub für die eingeschifften Verbände, gekauft am Markt des Systems, soweit \
             Ware und Staatskasse reichen.",
        );
    });
    let strength = snap(state.strength, slider_max(ready.max(troops)));
    ui.horizontal_wrapped(|ui| {
        let embark = SimCommand::EmbarkTroops {
            fleet: fleet.id,
            planet,
            strength: commanded(strength, ready),
            supply_days: state.supply_days as u16,
        };
        let check = state.cache.check(sim, &embark);
        if gated(
            ui,
            EMBARK,
            format!("{EMBARK}: {name}"),
            &check,
            "Reguläre Verbände der Welt gehen mit ihrem Nachschub an Bord der Flotte.",
        ) {
            actions.push(UiAction::Command(embark));
        }
        if let Err(reason) = &check {
            ui.label(small(reason).color(MUTED));
        }
    });
    ui.horizontal_wrapped(|ui| {
        let land = SimCommand::LandTroops {
            fleet: fleet.id,
            planet,
            strength: commanded(strength, troops),
        };
        let check = state.cache.check(sim, &land);
        if gated(
            ui,
            DISEMBARK,
            format!("{DISEMBARK}: {name}"),
            &check,
            "Die Verbände verstärken die Garnison der eigenen Welt; ihr Nachschub füllt das \
             Depot.",
        ) {
            actions.push(UiAction::Command(land));
        }
        if let Err(reason) = &check {
            ui.label(small(reason).color(MUTED));
        }
    });
}

/// Die Flotte, mit der gearbeitet wird: die gewählte, solange es sie gibt, sonst die erste
/// eigene.
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

pub fn troops_tab(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    state: &mut SiegeUi,
    actions: &mut Vec<UiAction>,
) {
    let Some(player) = sim.config.player else {
        ui.label(NO_COMMANDS);
        return;
    };
    let shown = shown_fleet(sim, player, &mut state.fleet).map(|fleet| fleet.id);
    egui::Grid::new("belagerung_flotten")
        .striped(true)
        .num_columns(4)
        .show(ui, |ui| {
            for title in ["Flotte", "Ort und Zustand", "Truppen", "Belagerung"] {
                ui.label(RichText::new(title).color(MUTED).small());
            }
            ui.end_row();
            let mut any = false;
            for fleet in sim.fleets.of(player) {
                any = true;
                if ui
                    .selectable_label(shown == Some(fleet.id), &fleet.name)
                    .clicked()
                {
                    state.fleet = Some(fleet.id);
                }
                ui.label(format!(
                    "{} · {}",
                    location(sim, fleet),
                    FleetState::of(fleet, sim.battles).label()
                ));
                ui.label(aboard_text(sim, fleet.id));
                ui.label(target_text(sim, fleet.id).unwrap_or_else(|| "–".into()));
                ui.end_row();
            }
            if !any {
                ui.label(RichText::new("keine Flotte mehr").color(MUTED));
                ui.end_row();
            }
        });
    let Some(fleet) = state.fleet.and_then(|id| sim.fleets.get(id)) else {
        return;
    };
    ui.separator();
    ui.label(RichText::new(&fleet.name).color(ACCENT).strong());
    if !fleet.route.is_empty() {
        ui.label(
            small(
                "Die Flotte ist auf dem Marsch. Belagern, einschiffen und landen kann sie, \
                   wo sie liegt.",
            )
            .color(MUTED),
        );
        return;
    }
    let system = sim.galaxy.system(fleet.system);
    ui.label(RichText::new(format!("Welten in {}", system.name)).color(MUTED));
    ui.horizontal_wrapped(|ui| {
        if !begin_buttons(ui, sim, player, fleet, state, actions) {
            ui.label(small("Keine fremde Welt in diesem System.").color(MUTED));
        }
    });
    for planet in &system.planets {
        if sim.galaxy.planet(*planet).owner == Some(player) {
            ui.add_space(4.0);
            ui.push_id(("eigene_welt", planet.0), |ui| {
                own_world(ui, sim, fleet, *planet, state, actions);
            });
        }
    }
    ui.add_space(4.0);
    ui.label(
        small(
            "Eingeschifft werden reguläre Verbände einer eigenen Welt auf eine Flotte in \
             ihrem System. Über einer belagerten Welt landen sie im Reiter „Belagerungen“ \
             bei der Belagerung selbst.",
        )
        .color(MUTED),
    );
}

/// Die Zeile im Fenster „Flotten“.
pub fn fleet_entry(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    fleet: &Fleet,
    state: &mut SiegeUi,
    actions: &mut Vec<UiAction>,
) {
    let Some(player) = sim.config.player else {
        return;
    };
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Belagerung").color(MUTED));
        if let Some(text) = target_text(sim, fleet.id) {
            ui.label(text);
        }
        ui.label(small(aboard_text(sim, fleet.id)).color(MUTED));
        if fleet.route.is_empty() {
            begin_buttons(ui, sim, player, fleet, state, actions);
        }
        if ui
            .button(TROOPS_AND_SIEGE)
            .on_hover_text(
                "Öffnet das Fenster „Belagerung“ bei dieser Flotte: belagern, Truppen \
                 einschiffen und an Land setzen.",
            )
            .clicked()
        {
            state.open = true;
            state.tab = SiegeTab::Troops;
            state.fleet = Some(fleet.id);
        }
    });
}

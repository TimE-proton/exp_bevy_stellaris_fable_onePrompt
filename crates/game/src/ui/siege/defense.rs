//! Stehende Verteidigung der eigenen Welten: was steht, was der Plan nennt, was es kostet,
//! und der Plan zum Ändern. Dieselbe Tafel steht an der Welt (linke Tafel) und im Fenster
//! „Staatsfinanzen“ unter der Übersicht aller Welten.

use super::detail::garrison_orders;
use super::{SiegeUi, WARN, days_text, gated, siege_refs, units};
use crate::ui::format::{decimal, grouped};
use crate::ui::panels::{ACCENT, MUTED, SimRefs, UiAction, detail as small};
use bevy_egui::egui::{self, RichText};
use sim::command::SimCommand;
use sim::ids::{PlanetId, PolityId};
use sim::siege::arms::Works;
use sim::siege::{DefenseCard, DefensePlan, SiegeRefs, defense_card};

pub const HEADING: &str = "Verteidigung";
pub const SET_PLAN: &str = "Plan festsetzen";
pub const DISCARD: &str = "Verwerfen";
pub const PLAN_IN_FORCE: &str = "Der Plan gilt schon.";
pub const REGULARS: &str = "Reguläre Verbände";
pub const DEPOT: &str = "Depot";

/// Eine Stufe, wie sie steht: ganze Stufen ohne Nachkommastelle, beschädigte mit einer.
pub fn level_text(level: f64) -> String {
    if (level - level.round()).abs() < 0.05 {
        decimal(level.round(), 0)
    } else {
        decimal(level, 1)
    }
}

/// Wie weit der Ausbau einer Anlage ist: gelieferte Gütersätze gegen die der nächsten
/// Stufe (Stufe n braucht n Sätze), oder dass der Plan erfüllt ist.
pub fn progress_text(card: &DefenseCard, works: Works) -> String {
    let level = works.level(&card.defense).floor() as u8;
    let planned = works.planned(&card.plan);
    if planned <= level {
        return "erfüllt".to_string();
    }
    let next = level + 1;
    format!(
        "{} von {next} {}",
        decimal(card.defense.delivered[works.index()].min(next as f64), 1),
        if next == 1 { "Satz" } else { "Sätzen" }
    )
}

/// Die Miliz der Welt: An sie ist die Zahl der Regulären gebunden.
pub fn militia_text(card: &DefenseCard) -> String {
    format!("Miliz {}", units(card.defense.militia))
}

/// Eine Zelle der Übersicht: was steht, und dahinter der Plan, wenn er abweicht.
pub fn cell(stands: &str, planned: &str) -> String {
    if stands == planned {
        stands.to_string()
    } else {
        format!("{stands}, Plan {planned}")
    }
}

/// Der Plan mit einer geänderten Stufe.
pub fn with_level(mut plan: DefensePlan, works: Works, level: u8) -> DefensePlan {
    match works {
        Works::Stations => plan.stations = level,
        Works::Guns => plan.guns = level,
        Works::Shield => plan.shield = level,
        Works::Bunkers => plan.bunkers = level,
    }
    plan
}

/// Die Karte der Verteidigung, wie der Kern sie nennte, gälte schon der Plan `plan`: Sie
/// wird auf einer Kopie der Verteidigung gelesen, die danach verworfen wird.
pub fn preview_card(sim: &SimRefs, planet: PlanetId, plan: &DefensePlan) -> Option<DefenseCard> {
    let mut sieges = sim.sieges.clone();
    sieges.defense_mut(planet).plan = plan.clone();
    let refs = SiegeRefs {
        sieges: &sieges,
        ..siege_refs(sim)
    };
    defense_card(&refs, planet)
}

/// Unterhalt und Ausbaukosten unter dem gezeigten Plan. Für einen geänderten Plan fragt
/// die Tafel den Kern einmal je Tag und Stellung der Regler, nicht in jedem Frame.
fn costs_under(ui: &egui::Ui, sim: &SimRefs, card: &DefenseCard, plan: &DefensePlan) -> (f64, f64) {
    if *plan == card.plan {
        return (card.plan_upkeep, card.build_cost);
    }
    type Key = (u64, PlanetId, DefensePlan);
    type Kept = (Key, (f64, f64));
    let key: Key = (sim.clock.day(), card.planet, plan.clone());
    let id = egui::Id::new("verteidigungsplan-probelauf");
    let kept: Option<Kept> = ui.data(|data| data.get_temp(id));
    match kept {
        Some((known, costs)) if known == key => costs,
        _ => {
            let costs = preview_card(sim, card.planet, plan)
                .map_or((card.plan_upkeep, card.build_cost), |preview| {
                    (preview.plan_upkeep, preview.build_cost)
                });
            ui.data_mut(|data| data.insert_temp::<Kept>(id, (key, costs)));
            costs
        }
    }
}

/// Die Tafel einer eigenen Welt: Anlagen mit Stufe, Grenze und Ausbaustand, Reguläre und
/// Depot mit ihren Grenzen, Kosten, Haltung der Garnison, und der Plan zum Ändern.
pub fn defense_editor(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    planet: PlanetId,
    state: &mut SiegeUi,
    actions: &mut Vec<UiAction>,
) {
    let Some(card) = defense_card(&siege_refs(sim), planet) else {
        ui.label(small("Eine unbesiedelte Welt hat keine Verteidigung.").color(MUTED));
        return;
    };
    let mut plan = match &state.plan_edit {
        Some((edited, plan)) if *edited == planet => plan.clone(),
        _ => card.plan.clone(),
    };
    let before = plan.clone();
    egui::Grid::new(("verteidigung", planet.0))
        .striped(true)
        .num_columns(5)
        .show(ui, |ui| {
            for title in ["", "steht", "höchstens", "Plan", "Ausbau"] {
                ui.label(RichText::new(title).color(MUTED).small());
            }
            ui.end_row();
            for works in Works::ALL {
                let cap = card.level_caps[works.index()];
                ui.label(works.label());
                ui.label(level_text(works.level(&card.defense)));
                ui.label(cap.to_string())
                    .on_hover_text("Höchste Stufe nach dem Stand der Technik.");
                let mut level = works.planned(&plan);
                // Ein Plan über der Grenze (die Technik ging verloren) bleibt wählbar,
                // bis der Spieler ihn senkt; der Kern nennt den Grund.
                let most = cap.max(level);
                ui.add(egui::DragValue::new(&mut level).range(0..=most).speed(0.05))
                    .on_hover_text(format!(
                        "Ausbauziel: Stufe 0 bis {cap}, begrenzt durch die Technik. Stufe n \
                         braucht n Gütersätze."
                    ));
                plan = with_level(plan.clone(), works, level);
                ui.label(small(progress_text(&card, works)).color(MUTED))
                    .on_hover_text(
                        "Gütersätze, die für die nächste Stufe schon geliefert sind. Der \
                         Markt gibt je Kauf nur einen Teil seines Bestands her.",
                    );
                ui.end_row();
            }
            ui.label(REGULARS);
            ui.label(units(card.defense.regulars));
            ui.label(units(card.regulars_cap)).on_hover_text(
                "Reguläre sind besoldet und beweglich, aus der Bevölkerung ausgehoben; ihre \
                 Zahl ist an die Miliz der Welt gebunden.",
            );
            let most = card.regulars_cap.max(plan.regulars as f64) as f32;
            ui.add(
                egui::DragValue::new(&mut plan.regulars)
                    .range(0.0..=most)
                    .speed(0.1)
                    .custom_formatter(|value, _| crate::ui::format::decimal(value, 1))
                    .custom_parser(|text| text.replace(',', ".").parse().ok()),
            );
            plan.regulars = (plan.regulars * 2.0).round() / 2.0;
            ui.label(small(militia_text(&card)).color(MUTED));
            ui.end_row();
            ui.label(DEPOT);
            ui.label(days_text(card.defense.depot));
            ui.label(format!("{} Tage", card.depot_days_cap));
            let most = card.depot_days_cap.max(plan.depot_days);
            ui.add(
                egui::DragValue::new(&mut plan.depot_days)
                    .range(0..=most)
                    .speed(1.0),
            )
            .on_hover_text("Vorräte der Garnison für eine Einschließung, in Tagen.");
            ui.label("");
            ui.end_row();
        });
    let edited = plan != card.plan;
    let (plan_upkeep, build_cost) = costs_under(ui, sim, &card, &plan);
    ui.label(format!(
        "Unterhalt im Monat: heute {} Credits · nach {} {} Credits",
        grouped(card.upkeep),
        if edited {
            "dem geänderten Plan"
        } else {
            "Plan"
        },
        grouped(plan_upkeep)
    ));
    ui.label(format!(
        "Ausbau bis zum Plan: noch {} Credits zu heutigen Preisen",
        grouped(build_cost)
    ))
    .on_hover_text(
        "Depot, Aushebung und alle fehlenden Stufen. Bezahlt wird aus dem Anspruch \
         „Verteidigung“ des Staatshaushalts; was nicht bezahlt wird, verfällt.",
    );
    ui.label(
        small(format!(
            "Eine besetzte Welt dieser Größe hält eine Garnison von {} Verbänden ruhig.",
            units(card.hold_need)
        ))
        .color(MUTED),
    );
    if card.besieged {
        ui.label(small("Die Welt wird belagert.").color(WARN));
    }
    ui.horizontal_wrapped(|ui| {
        let command = SimCommand::SetDefensePlan {
            planet,
            plan: plan.clone(),
        };
        let check = if edited {
            state.cache.check(sim, &command)
        } else {
            Err(PLAN_IN_FORCE.to_string())
        };
        let name = &sim.galaxy.planet(planet).name;
        if gated(
            ui,
            SET_PLAN,
            format!("{SET_PLAN}: {name}"),
            &check,
            "Die Stufen wachsen monatlich auf das Ziel zu, soweit Güter und Kasse reichen.",
        ) {
            actions.push(UiAction::Command(command));
        }
        if edited {
            if ui.button(DISCARD).clicked() {
                plan = card.plan.clone();
            }
            if let Err(reason) = &check {
                ui.label(small(reason).color(MUTED));
            }
        }
    });
    garrison_orders(ui, sim, planet, state, actions);
    // Der geänderte Plan bleibt stehen, bis er gilt oder verworfen wird.
    if plan != card.plan {
        state.plan_edit = Some((planet, plan));
    } else if plan != before || matches!(&state.plan_edit, Some((edited, _)) if *edited == planet) {
        state.plan_edit = None;
    }
}

/// Summen über alle Welten: Unterhalt heute, Unterhalt nach Plan, Ausbau offen.
pub fn totals(cards: &[DefenseCard]) -> (f64, f64, f64) {
    cards.iter().fold((0.0, 0.0, 0.0), |sum, card| {
        (
            sum.0 + card.upkeep,
            sum.1 + card.plan_upkeep,
            sum.2 + card.build_cost,
        )
    })
}

/// Abschnitt „Verteidigung“ im Fenster „Staatsfinanzen“: alle eigenen Welten mit Stand
/// und Plan, darunter die Tafel der gewählten.
pub fn defense_overview(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    player: PolityId,
    state: &mut SiegeUi,
    actions: &mut Vec<UiAction>,
) {
    ui.label(RichText::new(HEADING).color(ACCENT));
    let refs = siege_refs(sim);
    let mut cards: Vec<DefenseCard> = sim
        .galaxy
        .planets_of(player)
        .filter_map(|planet| defense_card(&refs, planet.id))
        .collect();
    // Die Hauptwelt zuerst, dann die Welten, für die am meisten ausgegeben wird.
    let capital = sim.polities.get(player).capital;
    cards.sort_by(|a, b| {
        let weight = |card: &DefenseCard| card.upkeep + card.plan_upkeep + card.build_cost;
        (Some(b.planet) == capital)
            .cmp(&(Some(a.planet) == capital))
            .then(weight(b).total_cmp(&weight(a)))
    });
    if cards.is_empty() {
        ui.label(small("Keine eigene Welt.").color(MUTED));
        return;
    }
    if !cards
        .iter()
        .any(|card| Some(card.planet) == state.plan_planet)
    {
        state.plan_planet = Some(cards[0].planet);
    }
    egui::Grid::new("verteidigung_welten")
        .striped(true)
        .num_columns(9)
        .show(ui, |ui| {
            for (title, hint) in [
                ("Welt", "Ein Klick zeigt den Plan dieser Welt zum Ändern."),
                (
                    "Stat.",
                    "Orbitalstationen: Stufe, dahinter der Plan, wenn er abweicht",
                ),
                (
                    "Gesch.",
                    "Bodengeschütze: Stufe, dahinter der Plan, wenn er abweicht",
                ),
                (
                    "Schild",
                    "Schild: Stufe, dahinter der Plan, wenn er abweicht",
                ),
                (
                    "Bunker",
                    "Bunker: Stufe, dahinter der Plan, wenn er abweicht",
                ),
                (
                    "Reguläre",
                    "Reguläre Verbände, dahinter der Plan, wenn er abweicht",
                ),
                (
                    "Depot",
                    "Vorräte in Tagen, dahinter der Plan, wenn er abweicht",
                ),
                (
                    "Unterhalt",
                    "Sold und Unterhalt im Monat in Credits, dahinter unter dem Plan",
                ),
                (
                    "Ausbau",
                    "Was der Ausbau bis zum Plan noch kostet, in Credits zu heutigen Preisen",
                ),
            ] {
                ui.label(RichText::new(title).color(MUTED).small())
                    .on_hover_text(hint);
            }
            ui.end_row();
            for card in &cards {
                let name = &sim.galaxy.planet(card.planet).name;
                if ui
                    .selectable_label(state.plan_planet == Some(card.planet), name)
                    .clicked()
                {
                    state.plan_planet = Some(card.planet);
                }
                for works in Works::ALL {
                    ui.label(cell(
                        &level_text(works.level(&card.defense)),
                        &works.planned(&card.plan).to_string(),
                    ))
                    .on_hover_text(format!(
                        "höchstens Stufe {} · Ausbau: {}",
                        card.level_caps[works.index()],
                        progress_text(card, works)
                    ));
                }
                ui.label(cell(
                    &units(card.defense.regulars),
                    &units(card.plan.regulars as f64),
                ))
                .on_hover_text(format!("höchstens {}", units(card.regulars_cap)));
                ui.label(cell(
                    &decimal(card.defense.depot, 0),
                    &card.plan.depot_days.to_string(),
                ))
                .on_hover_text(format!("höchstens {} Tage", card.depot_days_cap));
                ui.label(cell(&grouped(card.upkeep), &grouped(card.plan_upkeep)));
                ui.label(grouped(card.build_cost));
                ui.end_row();
            }
        });
    let (upkeep, plan_upkeep, build_cost) = totals(&cards);
    ui.label(small(format!(
        "{} {}: Unterhalt im Monat {} Credits, nach Plan {} Credits · Ausbau offen {} Credits. \
         Bezahlt aus dem Anspruch „Verteidigung“ des Haushalts.",
        cards.len(),
        if cards.len() == 1 { "Welt" } else { "Welten" },
        grouped(upkeep),
        grouped(plan_upkeep),
        grouped(build_cost)
    )));
    if let Some(planet) = state.plan_planet {
        ui.add_space(2.0);
        ui.label(RichText::new(&sim.galaxy.planet(planet).name).strong());
        ui.push_id("staat_verteidigung", |ui| {
            defense_editor(ui, sim, planet, state, actions);
        });
    }
}

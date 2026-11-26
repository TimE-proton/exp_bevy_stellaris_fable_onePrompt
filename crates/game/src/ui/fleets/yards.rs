//! Werften, Bauaufträge, Entwürfe und die Kosten der Flotte.

use super::{CANCEL, ORDER, WAITING, WARN, dry_run, gated_button, is_waiting, sentence, waiting};
use crate::ui::UiState;
use crate::ui::budget::claim_label;
use crate::ui::format::{decimal, grouped, percent};
use crate::ui::panels::{ACCENT, MUTED, SimRefs, UiAction, detail as small};
use bevy_egui::egui::{self, RichText};
use sim::command::SimCommand;
use sim::data::ShipClass;
use sim::economy::{Claim, Money, to_credits};
use sim::fleet::{
    BuildOrder, BuildStatus, design_buildable, fleet_claim, monthly_upkeep, order_ship, yard_slots,
};
use sim::ids::{PlanetId, PolityId};

pub const COSTS: &str = "Was die Flotte kostet";
pub const YARDS: &str = "Werften";
pub const DESIGNS: &str = "Schiffe bestellen";

fn credits(money: Money) -> String {
    grouped(to_credits(money))
}

/// Die Zeilen Flotte und Beschaffung des Haushalts in einem Satz, für den Reiter der
/// Flotten.
pub fn fleet_cost_line(ui: &mut egui::Ui, sim: &SimRefs, player: PolityId) {
    let Some(month) = sim.economy.budget(player).and_then(|budget| budget.last()) else {
        return;
    };
    let fleet = month.line(Claim::Fleet);
    let procurement = month.line(Claim::Procurement);
    ui.label(
        small(format!(
            "Letzter Monat: Flotte (Sold und Ersatzteile) {} von {} Credits bezahlt, \
             Beschaffung (Werften, Nachschub, Instandsetzung) {} von {}. Einzelheiten unter \
             „{}“.",
            credits(fleet.paid),
            credits(fleet.wanted),
            credits(procurement.paid),
            credits(procurement.wanted),
            super::TAB_YARDS
        ))
        .color(MUTED),
    );
}

/// Flotte und Beschaffung, wie der Haushalt sie führt: letzter Monat, Summe der Monate
/// und was im laufenden Monat schon ausgegeben ist.
fn costs(ui: &mut egui::Ui, sim: &SimRefs, player: PolityId) {
    ui.label(RichText::new(COSTS).color(ACCENT));
    let budget = sim.economy.budget(player);
    let Some((budget, month)) = budget.and_then(|budget| Some((budget, budget.last()?))) else {
        ui.label(small("Noch ist kein Monat abgeschlossen.").color(MUTED));
        return;
    };
    let year = budget.year();
    let head = |text: String| RichText::new(text).color(MUTED).small();
    egui::Grid::new("flottenkosten")
        .striped(true)
        .num_columns(6)
        .show(ui, |ui| {
            for title in [
                "Anspruch im Haushalt".to_string(),
                "letzter Monat verlangt".into(),
                "bezahlt".into(),
                "offen".into(),
                format!("bezahlt in {} Monaten", budget.history.len()),
                "laufender Monat bisher".into(),
            ] {
                ui.label(head(title));
            }
            ui.end_row();
            for claim in [Claim::Fleet, Claim::Procurement] {
                let line = month.line(claim);
                let open = (line.wanted - line.paid).max(0);
                ui.label(claim_label(claim));
                ui.label(RichText::new(credits(line.wanted)).monospace());
                ui.label(RichText::new(credits(line.paid)).monospace());
                if open > 0 {
                    ui.label(RichText::new(credits(open)).monospace().color(WARN));
                } else {
                    ui.label("");
                }
                ui.label(RichText::new(credits(year.line(claim).paid)).monospace());
                ui.label(RichText::new(credits(budget.current.line(claim).paid)).monospace());
                ui.end_row();
            }
        });
    let arrears = sim.fleets.arrears_of(player);
    ui.label(
        small(format!(
            "Beträge in Credits. Flotte: Sold der Besatzungen und Ersatzteile. Beschaffung: \
             Werften, Nachschub und Instandsetzung, bezahlt, wenn gekauft wird. \
             Soldrückstand {}.",
            percent(arrears)
        ))
        .color(if arrears > 0.005 { WARN } else { MUTED }),
    );
}

/// Was ein weiteres Schiff eines Entwurfs die Flotte im Monat mehr kostet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExtraCost {
    /// Sold der Besatzung.
    pub pay: Money,
    /// Ersatzteile der Wartung zum heutigen Preis am Sitz der Gemeinschaft.
    pub parts: Money,
}

impl ExtraCost {
    pub fn total(self) -> Money {
        self.pay + self.parts
    }
}

/// Der Mehrbedarf je Entwurf (Index wie `GameData::designs`). Gefragt wird der Kern
/// selbst: der Anspruch der Flotte mit einem weiteren Schiff gegen den heutigen, auf einer
/// Kopie der Flotten. `None`, wo sich das nicht sagen lässt (kein Sitz, keine Flotte).
pub fn extra_costs(sim: &SimRefs, player: PolityId) -> Vec<Option<ExtraCost>> {
    let polity = sim.polities.get(player);
    let mut scratch = sim.fleets.clone();
    let existing = scratch.of(player).next().map(|fleet| fleet.id);
    let fleet = existing.or_else(|| {
        let seat = polity.seat(sim.galaxy)?;
        Some(scratch.new_fleet(sim.polities, player, sim.galaxy.planet(seat).system))
    });
    let Some(fleet) = fleet else {
        return vec![None; sim.data.designs.len()];
    };
    let claim = |fleets: &sim::fleet::Fleets| {
        (
            monthly_upkeep(sim.data, fleets, player),
            fleet_claim(sim.data, sim.galaxy, sim.economy, fleets, polity),
        )
    };
    let (pay, total) = claim(&scratch);
    (0..sim.data.designs.len() as u16)
        .map(|design| {
            scratch.add_ship(sim.data, fleet, design)?;
            let (with_pay, with_total) = claim(&scratch);
            scratch.list[fleet.index()].ships.pop();
            Some(ExtraCost {
                pay: with_pay - pay,
                parts: (with_total - total) - (with_pay - pay),
            })
        })
        .collect()
}

/// Welten des Spielers mit Werft, dazu jene, auf denen noch Aufträge ruhen.
pub fn yard_worlds(sim: &SimRefs, player: PolityId) -> Vec<(PlanetId, u32)> {
    let mut worlds: Vec<(PlanetId, u32)> = sim
        .galaxy
        .planets_of(player)
        .map(|planet| {
            let slots = yard_slots(
                sim.data,
                sim.galaxy,
                sim.economy,
                sim.polities,
                sim.fleets,
                planet.id,
            );
            (planet.id, slots)
        })
        .filter(|(planet, slots)| *slots > 0 || sim.fleets.orders_at(*planet).next().is_some())
        .collect();
    for order in sim.fleets.orders_of(player) {
        if !worlds.iter().any(|(planet, _)| *planet == order.planet) {
            worlds.push((order.planet, 0));
        }
    }
    worlds
}

fn order_row(ui: &mut egui::Ui, sim: &SimRefs, order: &BuildOrder, actions: &mut Vec<UiAction>) {
    let design = &sim.data.designs[order.design as usize];
    ui.label(format!("Nr. {}", order.id));
    ui.label(format!("{} ({})", design.name, design.stats.class.label()));
    ui.add(
        egui::ProgressBar::new(order.progress.clamp(0.0, 1.0))
            .desired_width(90.0)
            .text(percent(order.progress)),
    );
    let waits = !matches!(order.status, BuildStatus::Building);
    let status = RichText::new(order.status_text(sim.data));
    ui.label(if waits { status.color(WARN) } else { status });
    ui.label(RichText::new(credits(order.spent)).monospace())
        .on_hover_text("Bisher gezahlt: Bauteile und Werftaufschlag, in Credits.");
    let command = SimCommand::CancelBuild { order: order.id };
    // Ein Auftrag lässt sich nur einmal streichen.
    let free = !is_waiting(sim, &command);
    let button = ui
        .add_enabled(free, egui::Button::new(CANCEL).small())
        .on_hover_text("Streicht den Auftrag. Was verbaut ist, wird nicht erstattet.")
        .on_disabled_hover_text(WAITING);
    button.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            free,
            format!("{CANCEL}: Nr. {}", order.id),
        )
    });
    if button.clicked() {
        actions.push(UiAction::Command(command));
    }
    ui.end_row();
}

/// Was an einer Werft steht, solange Bestellungen auf den nächsten Takt warten.
pub fn waiting_orders(count: usize) -> String {
    match count {
        1 => "1 Bestellung vorgemerkt".to_string(),
        more => format!("{more} Bestellungen vorgemerkt"),
    }
}

/// Jede Werft mit Bauplätzen und Aufträgen. Ein Klick auf den Namen wählt sie für neue
/// Bestellungen.
fn yards(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    worlds: &[(PlanetId, u32)],
    state: &mut UiState,
    actions: &mut Vec<UiAction>,
) {
    ui.label(RichText::new(YARDS).color(ACCENT));
    if worlds.is_empty() {
        ui.label(
            small(
                "Keine eigene Welt hat eine Werft: Im Zweig Rüstung steht nirgends genug \
                 Kapital.",
            )
            .color(MUTED),
        );
        return;
    }
    let limit = sim.data.balance.military.yard_queue;
    for (planet, slots) in worlds {
        let world = sim.galaxy.planet(*planet);
        let orders: Vec<&BuildOrder> = sim.fleets.orders_at(*planet).collect();
        ui.horizontal_wrapped(|ui| {
            if ui
                .selectable_label(state.yard_chosen == Some(*planet), &world.name)
                .on_hover_text("Bei dieser Werft bestellen")
                .clicked()
            {
                state.yard_chosen = Some(*planet);
            }
            let place = &sim.galaxy.system(world.system).name;
            let slots_text = match slots {
                0 => "keine Werft mehr".to_string(),
                1 => "1 Bauplatz".to_string(),
                more => format!("{more} Bauplätze"),
            };
            ui.label(format!(
                "System {place} · {slots_text} · {} von höchstens {limit} Aufträgen",
                orders.len()
            ));
            // Bei stehender Zeit ändert eine Bestellung die Liste noch nicht: Hier steht,
            // dass sie angekommen ist.
            let waits = waiting(sim)
                .filter(|command| match command {
                    SimCommand::BuildShip { planet: at, .. }
                    | SimCommand::BuildClass { planet: at, .. } => at == planet,
                    _ => false,
                })
                .count();
            if waits > 0 {
                ui.label(RichText::new(waiting_orders(waits)).color(WARN));
            }
            if sim.fleets.is_blockaded(world.system) {
                ui.label(RichText::new("blockiert: Es kommt keine Ware an").color(WARN));
            }
        });
        if orders.is_empty() {
            ui.label(small("keine Aufträge").color(MUTED));
            continue;
        }
        egui::Grid::new(("auftraege", planet.0))
            .striped(true)
            .num_columns(6)
            .show(ui, |ui| {
                for title in ["Auftrag", "Entwurf", "Fortschritt", "Stand", "bezahlt", ""] {
                    ui.label(RichText::new(title).color(MUTED).small());
                }
                ui.end_row();
                for order in orders {
                    order_row(ui, sim, order, actions);
                }
            });
    }
}

/// Die Bauteile eines Entwurfs in Worten: „3 Rumpfsegmente, 2 Antriebe“.
fn parts_text(sim: &SimRefs, design: u16) -> String {
    let parts: Vec<String> = sim.data.designs[design as usize]
        .stats
        .parts
        .iter()
        .map(|(good, amount)| {
            format!(
                "{} {}",
                decimal(*amount as f64, 1),
                sim.data.good(*good).name
            )
        })
        .collect();
    parts.join(", ")
}

/// Die Technologien, die dem Spieler für einen Entwurf noch fehlen.
fn missing_techs(sim: &SimRefs, player: PolityId, design: u16) -> String {
    let names: Vec<&str> = sim.data.designs[design as usize]
        .stats
        .techs
        .iter()
        .filter(|tech| !sim.research.knows(player, **tech))
        .map(|tech| sim.data.techs[*tech as usize].name.as_str())
        .collect();
    names.join(", ")
}

/// Die Entwürfe mit Klasse, Bauwert, Bauzeit, Unterhalt und Bauteilen; bestellt wird bei
/// der gewählten Werft.
fn designs(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    player: PolityId,
    yard: PlanetId,
    state: &mut UiState,
    actions: &mut Vec<UiAction>,
) {
    let world = sim.galaxy.planet(yard);
    ui.label(RichText::new(format!("{DESIGNS} bei der Werft von {}", world.name)).color(ACCENT));
    let known = sim.research.known(player);
    let day = sim.clock.day();
    let would_take = |design: u16| {
        dry_run(sim, |fleets| {
            order_ship(
                sim.data,
                sim.galaxy,
                sim.economy,
                sim.polities,
                fleets,
                player,
                yard,
                design,
                known,
                day,
            )
        })
    };
    let extra = extra_costs(sim, player);
    let buildable: Vec<u16> = (0..sim.data.designs.len() as u16)
        .filter(|design| sim.data.designs[*design as usize].stats.class != ShipClass::Fighter)
        .collect();
    // Lehnt die Werft schon einen Entwurf ab, den die Gemeinschaft beherrscht, liegt es an
    // ihr und nicht am Entwurf: Der Grund steht dann einmal über der Liste.
    let yard_reason = buildable
        .iter()
        .find(|design| design_buildable(sim.data, **design, known).is_ok())
        .and_then(|design| would_take(*design).err());
    if let Some(reason) = &yard_reason {
        ui.label(small(reason).color(WARN));
    }
    let fee = sim.data.balance.military.yard_fee;
    egui::Grid::new("entwuerfe")
        .striped(true)
        .num_columns(7)
        .show(ui, |ui| {
            for title in [
                "Entwurf",
                "Klasse",
                "Bauwert",
                "Bauzeit",
                "Unterhalt je Monat",
                "Bauteile",
                "",
            ] {
                ui.label(RichText::new(title).color(MUTED).small());
            }
            ui.end_row();
            for design in &buildable {
                let def = &sim.data.designs[*design as usize];
                let locked = design_buildable(sim.data, *design, known).is_err();
                let tint = |text: RichText| if locked { text.color(MUTED) } else { text };
                if ui
                    .selectable_label(
                        state.design_chosen == Some(*design),
                        tint(RichText::new(&def.name)),
                    )
                    .on_hover_text("Kosten dieses Entwurfs unten ausweisen")
                    .clicked()
                {
                    state.design_chosen = Some(*design);
                }
                ui.label(tint(RichText::new(def.stats.class.label())));
                ui.label(tint(RichText::new(grouped(def.stats.value)).monospace()))
                    .on_hover_text(format!(
                        "Bauwert in Credits zu Richtpreisen. Gezahlt werden die Bauteile zum \
                         Tagespreis am Markt der Werft und {} Werftaufschlag, Tag für Tag \
                         über die Bauzeit.",
                        percent(fee as f32)
                    ));
                ui.label(tint(RichText::new(format!(
                    "{} Tage",
                    def.stats.build_days
                ))));
                match extra[*design as usize] {
                    Some(cost) => ui.label(tint(RichText::new(credits(cost.total())).monospace())),
                    None => ui.label(RichText::new("–").color(MUTED)),
                };
                ui.label(tint(small(parts_text(sim, *design))));
                if locked {
                    ui.label(
                        small(format!(
                            "gesperrt · braucht: {}",
                            missing_techs(sim, player, *design)
                        ))
                        .color(WARN),
                    );
                } else {
                    let check = would_take(*design);
                    if gated_button(
                        ui,
                        ORDER,
                        Some(format!("{ORDER}: {}", def.name)),
                        &check,
                        "Gibt das Schiff bei der gewählten Werft in Auftrag.",
                    ) {
                        actions.push(UiAction::Command(SimCommand::BuildShip {
                            planet: yard,
                            design: *design,
                        }));
                    }
                }
                ui.end_row();
            }
        });
    ui.horizontal_wrapped(|ui| {
        ui.label(small(
            "Nach Klasse bestellen (die Werft wählt den besten beherrschten Entwurf):",
        ));
        for class in ShipClass::ALL {
            if class == ShipClass::Fighter {
                continue;
            }
            let best = sim.data.best_design(class, known);
            let check = match best {
                Some(design) => would_take(design),
                None => Err(sentence(format!(
                    "für die Klasse {} beherrscht die Gemeinschaft keinen Entwurf",
                    class.label()
                ))),
            };
            let hover = best.map_or(String::new(), |design| {
                format!("Die Werft baut: {}", sim.data.designs[design as usize].name)
            });
            if gated_button(
                ui,
                class.label(),
                Some(format!("{ORDER}: Klasse {}", class.label())),
                &check,
                &hover,
            ) {
                actions.push(UiAction::Command(SimCommand::BuildClass {
                    planet: yard,
                    class,
                }));
            }
        }
    });
    // Was ein Schiff des gewählten Entwurfs im Monat mehr kostet.
    let chosen = state
        .design_chosen
        .filter(|design| buildable.contains(design))
        .or(buildable.first().copied());
    state.design_chosen = chosen;
    if let Some(design) = chosen
        && let Some(cost) = extra[design as usize]
    {
        let def = &sim.data.designs[design as usize];
        let today = sim
            .polities
            .list
            .get(player.index())
            .map(|polity| fleet_claim(sim.data, sim.galaxy, sim.economy, sim.fleets, polity))
            .unwrap_or(0);
        ui.label(format!(
            "Ein weiteres Schiff „{}“ kostet im Monat {} Credits mehr: {} Sold und {} \
             Ersatzteile zum heutigen Preis. Die Flotte verlangte dann {} statt {} Credits im \
             Monat.",
            def.name,
            credits(cost.total()),
            credits(cost.pay),
            credits(cost.parts),
            credits(today + cost.total()),
            credits(today)
        ));
    }
}

pub fn yards_tab(
    ui: &mut egui::Ui,
    sim: &SimRefs,
    player: PolityId,
    state: &mut UiState,
    actions: &mut Vec<UiAction>,
) {
    costs(ui, sim, player);
    ui.separator();
    let worlds = yard_worlds(sim, player);
    yards(ui, sim, &worlds, state, actions);
    // Bestellt wird bei der gewählten Werft, solange sie eine ist; sonst bei der ersten.
    let open: Vec<PlanetId> = worlds
        .iter()
        .filter(|(_, slots)| *slots > 0)
        .map(|(planet, _)| *planet)
        .collect();
    state.yard_chosen = state
        .yard_chosen
        .filter(|planet| open.contains(planet))
        .or(open.first().copied());
    if let Some(yard) = state.yard_chosen {
        ui.separator();
        designs(ui, sim, player, yard, state, actions);
    }
}

//! Wirtschaftstafeln: Markt mit Preisverlauf, Produktion, Produktionsketten, Warenflüsse.

use super::UiState;
use super::format::{decimal, percent, population};
use super::panels::{ACCENT, MUTED, SimRefs, color32};
use bevy_egui::egui::{self, Color32, RichText};
use egui_plot::{Legend, Line, Plot};
use sim::clock::{DAYS_PER_YEAR, START_YEAR};
use sim::data::Sector;
use sim::economy::{expected_return, to_credits, unit_economics_at};
use sim::galaxy::Planet;
use sim::ids::{GoodId, PlanetId, SystemId};

const GOOD: Color32 = Color32::from_rgb(120, 200, 130);
const BAD: Color32 = Color32::from_rgb(235, 110, 95);

/// Farbe für ein Verhältnis zum Richtwert: teuer rot, billig grün.
fn ratio_color(ratio: f64) -> Color32 {
    if ratio >= 1.5 {
        BAD
    } else if ratio <= 0.7 {
        GOOD
    } else {
        Color32::from_rgb(210, 215, 225)
    }
}

/// Markt eines Systems: Preise aus Angebot und Nachfrage, mit Verlauf.
pub fn market_tab(ui: &mut egui::Ui, sim: &SimRefs, state: &mut UiState, system: SystemId) {
    let market = sim.economy.market(system);
    let balance = &sim.data.balance.economy;
    if state.chart_goods.len() != sim.data.goods.len() {
        // Voreinstellung: das Lebensnotwendige und Treibstoff.
        state.chart_goods = (0..sim.data.goods.len())
            .map(|index| {
                let good = GoodId::from_index(index);
                good == balance.fuel_id || balance.needs.iter().any(|need| need.good_id == good)
            })
            .collect();
    }
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Kontor").color(MUTED));
        ui.label(format!(
            "Kasse {} · Umsatz {}/Tag",
            decimal(to_credits(market.cash), 0),
            decimal(market.turnover, 0)
        ));
        if sim.fleets.is_blockaded(system) {
            ui.label(RichText::new("BLOCKIERT").color(BAD).strong())
                .on_hover_text("Kein Frachter kommt hinein oder heraus.");
        }
    });
    egui::ScrollArea::vertical()
        .max_height(330.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            egui::Grid::new("markt")
                .striped(true)
                .num_columns(6)
                .show(ui, |ui| {
                    for title in [
                        "",
                        "Gut",
                        "Preis",
                        "zum Richtpreis",
                        "Bestand",
                        "Nachfrage / Zufluss",
                    ] {
                        ui.label(RichText::new(title).color(MUTED).small());
                    }
                    ui.end_row();
                    for (index, good) in sim.data.goods.iter().enumerate() {
                        let ratio = market.price[index] / good.base_price;
                        ui.checkbox(&mut state.chart_goods[index], "")
                            .on_hover_text("Im Diagramm zeigen");
                        ui.label(&good.name);
                        ui.label(decimal(market.price[index], 1));
                        ui.label(RichText::new(percent(ratio as f32)).color(ratio_color(ratio)));
                        ui.label(decimal(market.stock[index], 0));
                        ui.label(format!(
                            "{} / {}",
                            decimal(market.demand[index], 1),
                            decimal(market.supply[index], 1)
                        ));
                        ui.end_row();
                    }
                });
        });
    ui.add_space(4.0);
    ui.label(RichText::new("Preisverlauf (Richtpreis = 100 %)").color(MUTED));
    Plot::new("preisverlauf")
        .height(190.0)
        .legend(Legend::default())
        .allow_scroll(false)
        .include_y(0.0)
        .include_y(200.0)
        .x_axis_formatter(|mark, _| {
            format!(
                "{}",
                START_YEAR as f64 + (mark.value / DAYS_PER_YEAR as f64).floor()
            )
        })
        .show(ui, |plot| {
            for (index, good) in sim.data.goods.iter().enumerate() {
                if !state.chart_goods[index] {
                    continue;
                }
                let series: Vec<[f64; 2]> = sim
                    .history
                    .series(system, GoodId::from_index(index))
                    .into_iter()
                    .map(|[day, price]| [day, 100.0 * price / good.base_price])
                    .collect();
                plot.line(Line::new(good.name.clone(), series));
            }
        });
}

/// Produktion eines Planeten und die Antwort auf die Frage, was sich hier lohnt.
pub fn production_tab(ui: &mut egui::Ui, sim: &SimRefs, planet_id: PlanetId) {
    let planet = sim.galaxy.planet(planet_id);
    if planet.owner.is_none() {
        ui.label("Unbesiedelt. Hier wird nichts hergestellt.");
        return;
    }
    let local = sim.economy.planet(planet_id);
    let market = sim.economy.market(planet.system);
    let balance = &sim.data.balance.economy;
    let labor_supply = planet.population * balance.workforce_share;

    ui.label(RichText::new(&planet.name).strong());
    egui::Grid::new("arbeit").num_columns(2).show(ui, |ui| {
        let mut row = |label: &str, value: String| {
            ui.label(RichText::new(label).color(MUTED));
            ui.label(value);
            ui.end_row();
        };
        row("Arbeitskräfte", population(labor_supply));
        row(
            "Stellen",
            format!(
                "{} ({} des Angebots)",
                population(local.labor_demand),
                percent((local.labor_demand / labor_supply.max(1e-9)) as f32)
            ),
        );
        row(
            "Lohn",
            format!("{} je Mio. und Tag", decimal(local.wage, 2)),
        );
        row(
            "Haushalte",
            format!("{} Credits", decimal(to_credits(local.households), 0)),
        );
        for (need, met) in balance.needs.iter().zip(&local.needs_met) {
            row(
                &format!("Versorgung {}", sim.data.good(need.good_id).name),
                percent(*met as f32),
            );
        }
    });
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Firmen:").color(MUTED));
        for sector in Sector::ALL {
            let firm = &local.firms[sector.index()];
            if firm.cash == 0 && firm.revenue.abs() < 0.01 {
                continue;
            }
            ui.label(format!(
                "{} {}",
                sector.label(),
                decimal(to_credits(firm.cash), 0)
            ))
            .on_hover_text(format!(
                "Umsatz {}/Tag, Gewinn {}/Tag",
                decimal(firm.revenue, 1),
                decimal(firm.profit, 1)
            ));
        }
    });
    ui.separator();

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.label(RichText::new("Werke").color(MUTED));
            egui::Grid::new("werke")
                .striped(true)
                .num_columns(5)
                .show(ui, |ui| {
                    for title in [
                        "Rezept",
                        "Kapital",
                        "Ausstoß/Tag",
                        "Auslastung",
                        "Rendite/Jahr",
                    ] {
                        ui.label(RichText::new(title).color(MUTED).small());
                    }
                    ui.end_row();
                    let mut any = false;
                    for (index, recipe) in sim.data.recipes.iter().enumerate() {
                        if local.capital[index] <= 0.0 {
                            continue;
                        }
                        any = true;
                        ui.label(&recipe.name).on_hover_text(recipe.sector.label());
                        ui.label(decimal(local.capital[index], 1));
                        ui.label(decimal(local.output[index], 1));
                        let used = local.utilization_trend[index];
                        ui.label(RichText::new(percent(used as f32)).color(if used < 0.5 {
                            BAD
                        } else {
                            GOOD
                        }));
                        let roc = local.roc[index];
                        ui.label(RichText::new(percent(roc as f32)).color(if roc < 0.0 {
                            BAD
                        } else {
                            GOOD
                        }));
                        ui.end_row();
                    }
                    if !any {
                        ui.label("keine");
                        ui.end_row();
                    }
                });

            ui.add_space(8.0);
            ui.label(RichText::new("Was sich hier lohnt").color(MUTED)).on_hover_text(
            "Erwartete Jahresrendite einer neuen Kapitaleinheit zu heutigen Preisen und Löhnen. \
             Sie ergibt sich aus Vorkommen, Lohn, Energiebedarf, Startkosten in den Orbit und \
             den Preisen im System. Es gibt keine Gebäudeboni.",
        );
            let mut ranked: Vec<(usize, f64, sim::economy::UnitEconomics)> = sim
                .data
                .recipes
                .iter()
                .enumerate()
                .filter_map(|(index, recipe)| {
                    let unit = unit_economics_at(
                        sim.data,
                        planet,
                        local.wage,
                        market,
                        recipe,
                        tech_yield(sim, planet),
                    );
                    (unit.local_yield > 0.0)
                        .then(|| (index, expected_return(sim.data, market, &unit), unit))
                })
                .collect();
            ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
            egui::Grid::new("lohnt")
                .striped(true)
                .num_columns(7)
                .show(ui, |ui| {
                    for title in [
                        "Rezept", "Rendite", "Erlös", "Vorprod.", "Energie", "Start", "Löhne",
                    ] {
                        ui.label(RichText::new(title).color(MUTED).small());
                    }
                    ui.end_row();
                    for (index, expected, unit) in ranked.iter().take(8) {
                        let recipe = &sim.data.recipes[*index];
                        ui.label(&recipe.name).on_hover_text(format!(
                            "Lokaler Ertragsfaktor {}",
                            decimal(unit.local_yield, 2)
                        ));
                        ui.label(
                            RichText::new(percent(*expected as f32)).color(if *expected < 0.0 {
                                BAD
                            } else {
                                GOOD
                            }),
                        );
                        for value in [
                            unit.revenue,
                            unit.inputs,
                            unit.energy,
                            unit.lift,
                            unit.wages,
                        ] {
                            ui.label(decimal(value, 1));
                        }
                        ui.end_row();
                    }
                });
        });
}

/// Ertragsfaktor der Technik, mit dem die Werke einer Welt rechnen: der ihres Besitzers.
/// Der Kern rechnet Ausstoß und Rendite damit; die Tafeln zeigen dieselben Zahlen.
fn tech_yield(sim: &SimRefs, planet: &Planet) -> f64 {
    planet
        .owner
        .map_or(1.0, |owner| sim.economy.yield_factor(owner))
}

/// Produktionsketten: Wer braucht was, und was kommt heraus.
pub fn chains_window(
    ctx: &egui::Context,
    sim: &SimRefs,
    open: &mut bool,
    planet: Option<PlanetId>,
) {
    egui::Window::new("Produktionsketten").open(open)
        .default_pos([450.0, 70.0]).default_width(720.0).show(ctx, |ui| {
        ui.label(
            "Mengen je Kapitaleinheit und Tag. Der Deckungsbeitrag gilt für die gewählte Welt \
             zu heutigen Preisen (vor Löhnen).",
        );
        let context = planet.map(|id| {
            let planet = sim.galaxy.planet(id);
            (planet, sim.economy.planet(id), sim.economy.market(planet.system))
        });
        egui::ScrollArea::vertical().max_height(520.0).show(ui, |ui| {
            for sector in Sector::ALL {
                ui.add_space(4.0);
                ui.label(RichText::new(sector.label()).color(ACCENT).strong());
                egui::Grid::new(("ketten", sector.index())).striped(true).num_columns(6).show(ui, |ui| {
                    for title in ["Rezept", "Vorprodukte", "", "Ausstoß", "Arbeit · Energie", "Deckungsbeitrag hier"] {
                        ui.label(RichText::new(title).color(MUTED).small());
                    }
                    ui.end_row();
                    for recipe in sim.data.recipes.iter().filter(|recipe| recipe.sector == sector) {
                        ui.label(&recipe.name);
                        let inputs: Vec<String> = recipe
                            .input_ids
                            .iter()
                            .map(|(good, amount)| {
                                format!("{} {}", decimal(*amount as f64, 2), sim.data.good(*good).name)
                            })
                            .collect();
                        let source = match recipe.deposit_id {
                            Some(good) => format!("Vorkommen: {}", sim.data.good(good).name),
                            None if inputs.is_empty() => "–".into(),
                            None => inputs.join(" + "),
                        };
                        ui.label(source);
                        // Der Pfeil „→“ fehlt in der Schrift der Oberfläche.
                        ui.label(RichText::new("▶").color(MUTED));
                        ui.label(format!(
                            "{} {}",
                            decimal(recipe.output.1 as f64, 2),
                            sim.data.good(recipe.output_id).name
                        ));
                        ui.label(format!(
                            "{} · {}",
                            decimal(recipe.labor as f64, 1),
                            decimal(recipe.energy as f64, 2)
                        ));
                        match &context {
                            Some((planet, local, market)) => {
                                let unit = unit_economics_at(sim.data, planet, local.wage, market, recipe, tech_yield(sim, planet));
                                if unit.local_yield <= 0.0 {
                                    ui.label(RichText::new("kein Vorkommen").color(MUTED));
                                } else {
                                    let value = unit.contribution();
                                    ui.label(
                                        RichText::new(decimal(value, 1))
                                            .color(if value <= 0.0 { BAD } else { GOOD }),
                                    )
                                    .on_hover_text(if value <= 0.0 {
                                        "Deckt nicht einmal Material und Energie: Das Werk stünde still."
                                    } else {
                                        "Erlös abzüglich Vorprodukten, Energie und Startkosten."
                                    });
                                }
                            }
                            None => {
                                ui.label(RichText::new("Welt wählen").color(MUTED));
                            }
                        }
                        ui.end_row();
                    }
                });
            }
        });
    });
}

/// Warenflüsse: was die Frachter tatsächlich bewegt haben.
pub fn flows_window(ctx: &egui::Context, sim: &SimRefs, open: &mut bool, focus: Option<SystemId>) {
    egui::Window::new("Warenflüsse")
        .open(open)
        .default_pos([450.0, 90.0])
        .default_width(520.0)
        .show(ctx, |ui| {
            let laden = sim
                .logistics
                .freighters
                .iter()
                .filter(|f| f.alive && f.cargo.is_some())
                .count();
            ui.label(format!(
                "{} Frachter, davon {} beladen unterwegs. {} wurden bisher aufgebracht.",
                sim.logistics.active_freighters(),
                laden,
                sim.logistics.captured
            ));
            ui.label(
                RichText::new("Mengen der letzten Monate, jüngere zählen stärker.")
                    .color(MUTED)
                    .small(),
            );
            let mut flows: Vec<_> = sim
                .logistics
                .flows
                .iter()
                .filter(|((from, to, _), _)| {
                    focus.is_none_or(|system| *from == system || *to == system)
                })
                .collect();
            flows.sort_by(|a, b| b.1.total_cmp(a.1).then(a.0.cmp(b.0)));
            if let Some(system) = focus {
                ui.label(format!("Gefiltert auf {}.", sim.galaxy.system(system).name));
            }
            egui::ScrollArea::vertical()
                .max_height(330.0)
                .show(ui, |ui| {
                    egui::Grid::new("fluesse")
                        .striped(true)
                        .num_columns(4)
                        .show(ui, |ui| {
                            for title in ["Gut", "von", "nach", "Menge"] {
                                ui.label(RichText::new(title).color(MUTED).small());
                            }
                            ui.end_row();
                            for ((from, to, good), volume) in flows.iter().take(40) {
                                ui.label(&sim.data.good(*good).name);
                                ui.label(&sim.galaxy.system(*from).name);
                                ui.label(&sim.galaxy.system(*to).name);
                                ui.label(decimal(**volume, 0));
                                ui.end_row();
                            }
                            if flows.is_empty() {
                                ui.label(RichText::new("noch kein Handel").color(MUTED));
                                ui.end_row();
                            }
                        });
                });
            ui.separator();
            egui::Grid::new("haeuser")
                .striped(true)
                .num_columns(4)
                .show(ui, |ui| {
                    for title in ["Handelshaus", "Frachter", "Gewinn/Tag", "Kasse"] {
                        ui.label(RichText::new(title).color(MUTED).small());
                    }
                    ui.end_row();
                    for house in &sim.logistics.houses {
                        let ships = sim
                            .logistics
                            .freighters
                            .iter()
                            .filter(|f| f.alive && f.house == house.id)
                            .count();
                        ui.label(
                            RichText::new(&house.name)
                                .color(color32(sim.polities.get(house.polity).color)),
                        );
                        ui.label(ships.to_string());
                        ui.label(decimal(house.profit, 1));
                        ui.label(decimal(to_credits(house.cash), 0));
                        ui.end_row();
                    }
                });
        });
}

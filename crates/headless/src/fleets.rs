//! Flotten, Schiffe, Vorräte und Bauaufträge einer Partie auf der Kommandozeile. Mit
//! `--order` gibt jede Gemeinschaft dieselben Schiffe in Auftrag: So lässt sich ohne
//! Militär-KI sehen, was Werften, Märkte und Staatskassen daraus machen.

use clap::Args;
use sim::GameData;
use sim::battle::matchup::Side;
use sim::clock::{DAYS_PER_YEAR, Date};
use sim::command::{CommandOutcome, SimCommand};
use sim::data::GoodTier;
use sim::economy::{Economy, to_credits};
use sim::fleet::{BuildOrder, Fleets, arms_capital, monthly_upkeep, yard_slots};
use sim::harness::{Headless, check_invariants};
use sim::ids::{GoodId, PolityId};
use sim::state::SimConfig;
use std::process::ExitCode;

#[derive(Args)]
pub struct FleetsArgs {
    #[arg(long, default_value_t = 1)]
    seed: u64,
    #[arg(long, default_value_t = 10)]
    years: u64,
    /// Jede Gemeinschaft bestellt diese Schiffe bei der Werft ihrer Hauptwelt,
    /// z. B. `speer:1,dolch:2`.
    #[arg(long)]
    order: Option<String>,
    /// Jahr, in dem die Bestellung aus `--order` aufgegeben wird.
    #[arg(long, default_value_t = 0)]
    order_year: u64,
    /// Nur diese Gemeinschaft ausgeben (Nummer ab 0).
    #[arg(long)]
    polity: Option<u32>,
    /// Jedes Schiff einzeln auflisten.
    #[arg(long)]
    ships: bool,
}

fn percent(share: f32) -> String {
    format!("{:.0} %", 100.0 * share)
}

/// Stand eines Auftrags in der Warteschlange seiner Werft.
fn order_state(order: &BuildOrder, position: usize, slots: u32) -> &'static str {
    if position >= slots as usize {
        "wartet auf einen Bauplatz"
    } else if order.stalled {
        "stockt: Bauteile oder Geld fehlen"
    } else {
        "im Bau"
    }
}

pub fn run(data: &GameData, args: FleetsArgs) -> ExitCode {
    let wanted = match args
        .order
        .as_deref()
        .map(|text| Side::parse(data, text))
        .transpose()
    {
        Ok(side) => side.map(|side| side.ships).unwrap_or_default(),
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let mut sim = Headless::new(
        data.clone(),
        SimConfig {
            seed: args.seed,
            player: None,
        },
    );
    let order_year = args.order_year.min(args.years);
    sim.run_days(order_year * DAYS_PER_YEAR);
    if !wanted.is_empty() {
        let capitals: Vec<_> = sim
            .polities()
            .alive()
            .filter_map(|polity| polity.capital.map(|capital| (polity.id, capital)))
            .collect();
        for (polity, planet) in capitals {
            for (design, count) in &wanted {
                for _ in 0..*count {
                    sim.issue(
                        polity,
                        SimCommand::BuildShip {
                            planet,
                            design: *design,
                        },
                    );
                }
            }
        }
    }
    sim.run_days((args.years - order_year) * DAYS_PER_YEAR);

    let world = sim.world();
    let galaxy = sim.galaxy();
    let polities = sim.polities();
    let economy = world.resource::<Economy>();
    let fleets = world.resource::<Fleets>();
    let military = &data.balance.military;
    println!(
        "Seed {} nach {} Jahren. Unterhalt {:.1} % des Bauwerts je Monat, Werftaufschlag {:.0} %.",
        args.seed,
        args.years,
        100.0 * military.upkeep_per_month,
        100.0 * military.yard_fee
    );
    for entry in &sim.command_log().entries {
        if let CommandOutcome::Rejected(reason) = &entry.outcome {
            println!(
                "Abgelehnt ({}): {:?}: {reason}",
                polities.get(entry.polity).name,
                entry.command
            );
        }
    }
    // Wovon Flotten und Werften leben: Treibstoff und alle Rüstungsgüter.
    let fuel = data.balance.economy.fuel_id;
    let supply_goods: Vec<GoodId> = std::iter::once(fuel)
        .chain(
            data.goods
                .iter()
                .enumerate()
                .filter(|(_, good)| good.tier == GoodTier::Military)
                .map(|(index, _)| GoodId::from_index(index)),
        )
        .collect();
    for polity in &polities.list {
        if args.polity.is_some_and(|only| PolityId(only) != polity.id) {
            continue;
        }
        let owned = fleets.of(polity.id).count();
        println!(
            "\n{}{}: {} {}, {} Schiffe. Staatskasse {:.0}, Sold {:.0} je Monat, Soldrückstand {}.",
            polity.name,
            if polity.alive { "" } else { " (erloschen)" },
            owned,
            if owned == 1 { "Flotte" } else { "Flotten" },
            fleets.ship_count(polity.id),
            to_credits(economy.treasury(polity.id)),
            to_credits(monthly_upkeep(data, fleets, polity.id)),
            percent(fleets.arrears_of(polity.id))
        );
        // Werften und der Markt, aus dem sie und die Flotte leben.
        for planet in galaxy.planets_of(polity.id) {
            let slots = yard_slots(data, galaxy, economy, polities, planet.id);
            if slots == 0 {
                continue;
            }
            let market = economy.market(planet.system);
            let stock: Vec<String> = supply_goods
                .iter()
                .map(|good| {
                    format!(
                        "{} {:.1} zu {:.0}",
                        data.good(*good).name,
                        market.stock_of(*good),
                        market.price_of(*good)
                    )
                })
                .collect();
            println!(
                "  Werft {:<14} {} {} (Rüstungskapital {:.1}){}  Markt {}: {}",
                planet.name,
                slots,
                if slots == 1 { "Bauplatz" } else { "Bauplätze" },
                arms_capital(data, economy, planet.id),
                if fleets.is_blockaded(planet.system) {
                    ", blockiert"
                } else {
                    ""
                },
                galaxy.system(planet.system).name,
                stock.join(", ")
            );
            for (position, order) in fleets.orders_at(planet.id).enumerate() {
                let design = &data.designs[order.design as usize];
                println!(
                    "    Auftrag {:<3} {:<14} {:<9} {:>5}  gezahlt {:>6.0}  seit {}  {}",
                    order.id,
                    design.stats.class.label(),
                    design.name,
                    percent(order.progress),
                    to_credits(order.spent),
                    Date::from_day(order.started),
                    order_state(order, position, slots)
                );
            }
        }
        for fleet in fleets.of(polity.id) {
            let (ammo, fuel) = fleet.supply(data);
            let place = match fleet.route.last() {
                Some(target) if !fleet.is_docked() || !fleet.route.is_empty() => format!(
                    "{} → {}",
                    galaxy.system(fleet.system).name,
                    galaxy.system(*target).name
                ),
                _ => galaxy.system(fleet.system).name.clone(),
            };
            let mut classes: Vec<String> = Vec::new();
            for class in sim::data::ShipClass::ALL {
                let count = fleet
                    .ships
                    .iter()
                    .filter(|ship| data.designs[ship.design as usize].stats.class == class)
                    .count();
                if count > 0 {
                    classes.push(format!(
                        "{count} {}",
                        if count == 1 {
                            class.label()
                        } else {
                            class.plural()
                        }
                    ));
                }
            }
            println!(
                "  {:<24} {:<22} {:<12} Stärke {:>6.1}  Bauwert {:>7.0}  Munition {:>5}  Treibstoff {:>5}  {}  ({}, {}){}",
                fleet.name,
                place,
                fleet.stance.label(),
                fleet.strength,
                fleet.build_value(data),
                percent(ammo),
                percent(fuel),
                classes.join(", "),
                fleet.formation.label(),
                fleet.doctrine.label(),
                if fleet.in_battle { "  im Gefecht" } else { "" }
            );
            if args.ships {
                for ship in &fleet.ships {
                    let design = &data.designs[ship.design as usize];
                    println!(
                        "    {:<5} {:<10} {:<14} Rumpf {:>5}  Module {:>5}  Munition {:>5}  Treibstoff {:>5}{}",
                        ship.id.to_string(),
                        design.name,
                        design.stats.class.label(),
                        percent(ship.hull),
                        percent(ship.module_condition()),
                        percent(ship.ammo),
                        percent(ship.fuel),
                        if design.stats.fighters > 0 {
                            format!("  Bordjäger {}/{}", ship.fighters, design.stats.fighters)
                        } else {
                            String::new()
                        }
                    );
                }
            }
        }
    }
    if let Err(message) = check_invariants(world) {
        eprintln!("INVARIANTE VERLETZT: {message}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

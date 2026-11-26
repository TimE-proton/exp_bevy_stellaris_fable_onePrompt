//! Flotten, Schiffe, Vorräte und Bauaufträge einer Partie auf der Kommandozeile. Mit
//! dem Drehbuch (`--order`, `--navy`, `--war`, `--peace`) bestellen und kämpfen die
//! Gemeinschaften: So lässt sich ohne Militär-KI sehen, was Werften, Märkte und
//! Staatskassen aus Rüstung, Verlusten und Nachbau machen.

use clap::Args;
use sim::GameData;
use sim::clock::{DAYS_PER_YEAR, Date};
use sim::data::GoodTier;
use sim::economy::{Economy, to_credits};
use sim::fleet::{Fleets, arms_capital, monthly_upkeep, yard_slots};
use sim::harness::{check_invariants, run_scripted};
use sim::ids::{GoodId, PolityId};
use sim::state::SaveGame;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Args)]
pub struct FleetsArgs {
    #[arg(long, default_value_t = 1)]
    seed: u64,
    #[arg(long, default_value_t = 10)]
    years: u64,
    /// Bestellungen, Flottenprogramme, Kriege und Friedensschlüsse nach Drehbuch.
    #[command(flatten)]
    script: crate::script::ScriptArgs,
    /// Nur diese Gemeinschaft ausgeben (Nummer ab 0).
    #[arg(long)]
    polity: Option<u32>,
    /// Jedes Schiff einzeln auflisten.
    #[arg(long)]
    ships: bool,
    /// Spielstand am Ende hierhin schreiben.
    #[arg(long)]
    save: Option<PathBuf>,
}

fn percent(share: f32) -> String {
    format!("{:.0} %", 100.0 * share)
}

pub fn run(data: &GameData, args: FleetsArgs) -> ExitCode {
    let script = match args.script.script(data) {
        Ok(script) => script,
        Err(error) => {
            eprintln!("Fehler im Drehbuch: {error}");
            return ExitCode::FAILURE;
        }
    };
    let (sim, violation) = run_scripted(data, args.seed, args.years * DAYS_PER_YEAR, &script);
    if let Some(only) = args.polity
        && only as usize >= sim.polities().list.len()
    {
        eprintln!(
            "--polity {only}: Diese Partie beginnt mit den Gemeinschaften 0 bis {}.",
            sim.polities().list.len() - 1
        );
        return ExitCode::FAILURE;
    }

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
    if !script.is_empty() {
        crate::war::print_aftermath(&sim);
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
        // Werften und der Markt, aus dem sie und die Flotte leben. Eine Welt, die ihre
        // Werft verloren hat, steht hier weiter, solange dort Aufträge ruhen.
        for planet in galaxy.planets_of(polity.id) {
            let slots = yard_slots(data, galaxy, economy, polities, fleets, planet.id);
            if slots == 0 && fleets.orders_at(planet.id).next().is_none() {
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
                if slots == 0 {
                    "keine".to_string()
                } else {
                    slots.to_string()
                },
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
            for order in fleets.orders_at(planet.id) {
                let design = &data.designs[order.design as usize];
                println!(
                    "    Auftrag {:<3} {:<14} {:<9} {:>5}  gezahlt {:>6.0}  seit {}  {}",
                    order.id,
                    design.stats.class.label(),
                    design.name,
                    percent(order.progress),
                    to_credits(order.spent),
                    Date::from_day(order.started),
                    order.status_text(data)
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
                match (fleet.in_battle, fleet.regroup_until) {
                    (true, _) => "  im Gefecht".to_string(),
                    (false, Some(day)) => format!("  sammelt sich bis {}", Date::from_day(day)),
                    (false, None) => String::new(),
                }
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
    if let Some(message) = violation.or_else(|| check_invariants(world).err()) {
        eprintln!("INVARIANTE VERLETZT: {message}");
        return ExitCode::FAILURE;
    }
    if let Some(path) = args.save {
        if let Err(error) = SaveGame::write(&sim.save(), &path) {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
        println!("Spielstand: {}", path.display());
    }
    ExitCode::SUCCESS
}

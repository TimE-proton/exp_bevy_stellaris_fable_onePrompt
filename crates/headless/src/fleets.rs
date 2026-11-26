//! Flotten, Schiffe, Vorräte und Bauaufträge einer Partie auf der Kommandozeile. Mit
//! dem Drehbuch (`--order`, `--navy`, `--war`, `--peace`) bestellen und kämpfen die
//! Gemeinschaften: So lässt sich ohne Militär-KI sehen, was Werften, Märkte und
//! Staatskassen aus Rüstung, Verlusten und Nachbau machen.

use crate::budget::{
    BudgetTotals, print_budget, print_months, print_table_header, print_table_row,
};
use clap::Args;
use sim::GameData;
use sim::clock::{DAYS_PER_YEAR, Date};
use sim::data::GoodTier;
use sim::economy::{Economy, to_credits};
use sim::fleet::{Fleets, arms_capital, monthly_upkeep, yard_slots};
use sim::harness::{Headless, WarScript, check_invariants};
use sim::ids::{GoodId, PolityId};
use sim::state::{SaveGame, SimConfig};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Args)]
pub struct FleetsArgs {
    #[arg(long, default_value_t = 1)]
    seed: u64,
    /// Mehrere Seeds, z. B. `0..8`: statt der Einzelheiten die Haushaltsübersicht je Seed,
    /// gemittelt über alle Gemeinschaften und die ganze Laufzeit.
    #[arg(long, value_parser = crate::parse_range)]
    seeds: Option<(u64, u64)>,
    #[arg(long, default_value_t = 10)]
    years: u64,
    /// Bestellungen, Flottenprogramme, Kriege und Friedensschlüsse nach Drehbuch.
    #[command(flatten)]
    script: crate::script::ScriptArgs,
    /// Jede Gemeinschaft beginnt mit so vielen Startflotten. Mit dieser Angabe, mit
    /// `--research-share` und mit `--fleet-cost` ruht die militärische Führung der KI,
    /// damit die Flotten bleiben, wie sie sind; `--ai alle:aktiv` schaltet sie ein.
    #[arg(long, default_value_t = 1)]
    start_fleets: u32,
    /// Forschungsetat aller Gemeinschaften als Anteil der laufenden Einnahmen, statt der
    /// Anteile ihrer Vorlagen.
    #[arg(long)]
    research_share: Option<f64>,
    /// Lohnsteuersatz, den alle Gemeinschaften vom ersten Tag an erheben, statt des
    /// üblichen. Die Grundleistungen des Haushalts bemessen sich weiter am üblichen Satz.
    #[arg(long)]
    tax: Option<f64>,
    /// Faktor auf die laufenden Kosten der Flotte (Sold, Wartung, Verbrauch auf Station);
    /// 0 schaltet sie ab.
    #[arg(long, default_value_t = 1.0)]
    fleet_cost: f64,
    /// Nur diese Gemeinschaft ausgeben (Nummer ab 0).
    #[arg(long)]
    polity: Option<u32>,
    /// Jedes Schiff einzeln auflisten.
    #[arg(long)]
    ships: bool,
    /// Die letzten zwölf Haushaltsmonate einzeln auflisten.
    #[arg(long)]
    months: bool,
    /// Spielstand am Ende hierhin schreiben.
    #[arg(long)]
    save: Option<PathBuf>,
}

fn percent(share: f32) -> String {
    format!("{:.0} %", 100.0 * share)
}

/// Die Spieldaten mit den Abweichungen, die die Angaben verlangen. Nichts davon wird
/// geschrieben; es gilt für diesen Lauf.
fn variant(data: &GameData, args: &FleetsArgs) -> Result<GameData, String> {
    let mut data = data.clone();
    let military = &mut data.balance.military;
    if args.start_fleets == 0 {
        return Err("--start-fleets muss mindestens 1 sein".into());
    }
    for entry in &mut military.starting_fleet_ids {
        entry.1 *= args.start_fleets;
    }
    for entry in &mut military.starting_fleet {
        entry.1 *= args.start_fleets;
    }
    if !(0.0..=10.0).contains(&args.fleet_cost) {
        return Err("--fleet-cost muss zwischen 0 und 10 liegen".into());
    }
    military.upkeep_per_month = (military.upkeep_per_month * args.fleet_cost).min(1.0);
    military.maintenance_per_month = (military.maintenance_per_month * args.fleet_cost).min(1.0);
    military.fuel_per_day = (military.fuel_per_day * args.fleet_cost as f32).min(1.0);
    if let Some(share) = args.research_share {
        let max = data.balance.research.max_budget_share;
        if !(0.0..=max).contains(&share) {
            return Err(format!(
                "--research-share muss zwischen 0 und {max} liegen (research.max_budget_share)"
            ));
        }
        data.balance.research.budget_share = share;
        for template in &mut data.polities {
            template.research_share = Some(share);
        }
    }
    if let Some(tax) = args.tax
        && !(0.0..=0.6).contains(&tax)
    {
        return Err("--tax muss zwischen 0 und 0.6 liegen".into());
    }
    Ok(data)
}

/// Wie es den Haushalten geht, gemittelt über die Jahresenden eines Laufs.
#[derive(Clone, Copy, Debug, Default)]
struct Welfare {
    /// Summe der Bevölkerung (Mio.) über die Jahresenden.
    people: f64,
    /// Geld der Haushalte in Credits, über dieselben Jahresenden summiert.
    cash: f64,
    /// Deckung aller Bedarfe, bevölkerungsgewichtet summiert.
    needs: f64,
}

impl Welfare {
    fn sample(&mut self, sim: &Headless) {
        let economy = sim.world().resource::<Economy>();
        for planet in &sim.galaxy().planets {
            if planet.owner.is_none() || planet.population <= 0.0 {
                continue;
            }
            let local = economy.planet(planet.id);
            self.people += planet.population;
            self.cash += to_credits(local.households);
            self.needs += planet.population * local.needs_met.iter().sum::<f64>()
                / local.needs_met.len().max(1) as f64;
        }
    }

    /// Geld der Haushalte je Mio. Einwohner.
    fn cash_per_million(&self) -> f64 {
        self.cash / self.people.max(1e-9)
    }

    fn needs_met(&self) -> f64 {
        self.needs / self.people.max(1e-9)
    }
}

/// Eine gespielte Partie mit dem, was ihr Haushalt über die Jahre ergab.
struct Played {
    sim: Headless,
    violation: Option<String>,
    /// Je Gemeinschaft die Summe aller abgeschlossenen Haushaltsmonate.
    totals: Vec<BudgetTotals>,
    welfare: Welfare,
}

impl Played {
    /// Alle Gemeinschaften zusammen.
    fn all(&self) -> BudgetTotals {
        let mut sum = BudgetTotals::default();
        for totals in &self.totals {
            sum.merge(totals);
        }
        sum
    }
}

/// Spielt eine Partie nach Drehbuch, prüft jedes Jahr die Invarianten und sammelt die
/// Haushaltsmonate ein. Mit `tax` setzt jede Gemeinschaft am ersten Tag diesen
/// Lohnsteuersatz, per Befehl wie ein Spieler.
fn play(data: &GameData, seed: u64, years: u64, script: &WarScript, tax: Option<f64>) -> Played {
    let mut sim = Headless::new(data.clone(), SimConfig { seed, player: None });
    if let Some(rate) = tax {
        let ids: Vec<PolityId> = sim.polities().list.iter().map(|polity| polity.id).collect();
        for id in ids {
            sim.issue(id, sim::command::SimCommand::SetTaxRate { rate });
        }
    }
    let mut violation = None;
    let mut totals: Vec<BudgetTotals> = Vec::new();
    let mut welfare = Welfare::default();
    for year in 1..=years {
        script.run_until(&mut sim, year * DAYS_PER_YEAR, false);
        if violation.is_none()
            && let Err(message) = check_invariants(sim.world())
        {
            violation = Some(format!("Jahr {year}: {message}"));
        }
        let count = sim.polities().list.len();
        if totals.len() < count {
            totals.resize_with(count, BudgetTotals::default);
        }
        for (index, entry) in totals.iter_mut().enumerate() {
            entry.add_year(&sim, PolityId::from_index(index));
        }
        welfare.sample(&sim);
    }
    Played {
        sim,
        violation,
        totals,
        welfare,
    }
}

/// Die Haushaltsübersicht über mehrere Seeds: je Seed alle Gemeinschaften zusammen.
fn overview(data: &GameData, args: &FleetsArgs, seeds: (u64, u64), script: &WarScript) -> ExitCode {
    let seeds: Vec<u64> = (seeds.0..seeds.1).collect();
    let jobs = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .clamp(1, seeds.len().max(1));
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results = std::sync::Mutex::new(Vec::with_capacity(seeds.len()));
    std::thread::scope(|scope| {
        for _ in 0..jobs {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(seed) = seeds.get(index) else {
                        break;
                    };
                    let played = play(data, *seed, args.years, script, args.tax);
                    let fleets = played.sim.world().resource::<Fleets>();
                    let ships: usize = played
                        .sim
                        .polities()
                        .alive()
                        .map(|polity| fleets.ship_count(polity.id))
                        .sum();
                    let alive = played.sim.polities().alive().count();
                    results.lock().expect("Lauf abgestürzt").push((
                        index,
                        played.all(),
                        played.welfare,
                        ships as f64 / alive.max(1) as f64,
                        played.violation,
                    ));
                }
            });
        }
    });
    let mut results = results.into_inner().expect("Lauf abgestürzt");
    results.sort_by_key(|(index, ..)| *index);
    println!(
        "Haushalt über {} Jahre, alle Gemeinschaften eines Seeds zusammen. Einnahmen in Credits \
         je Monat und Gemeinschaft; Ansprüche (bezahlt), Rest, Ausschüttung und Sonstiges in \
         Prozent der laufenden Einnahmen; dann der Anteil der Monate mit Defizit, mit gekürzten \
         Ansprüchen und mit der Kasse unter der Rücklage, und der höchste Soldrückstand.",
        args.years
    );
    print_table_header("Seed");
    let mut mean = BudgetTotals::default();
    let mut welfare = Welfare::default();
    let mut failed = false;
    for (index, totals, lives, ships, violation) in &results {
        print_table_row(&seeds[*index].to_string(), totals);
        println!(
            "{:>6} Schiffe je Gemeinschaft {ships:.1}, Geld der Haushalte je Mio. Einwohner \
             {:.2}, Bedarf gedeckt {:.3}",
            "",
            lives.cash_per_million(),
            lives.needs_met()
        );
        mean.merge(totals);
        welfare.people += lives.people;
        welfare.cash += lives.cash;
        welfare.needs += lives.needs;
        if let Some(message) = violation {
            eprintln!("INVARIANTE VERLETZT, Seed {}: {message}", seeds[*index]);
            failed = true;
        }
    }
    print_table_row("Mittel", &mean);
    println!(
        "{:>6} Geld der Haushalte je Mio. Einwohner {:.2}, Bedarf gedeckt {:.3}",
        "",
        welfare.cash_per_million(),
        welfare.needs_met()
    );
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

pub fn run(data: &GameData, mut args: FleetsArgs) -> ExitCode {
    // Der Vergleich misst Flotten fester Größe: Eine KI, die selbst rüstet, stilllegt und
    // ihren Forschungsetat kürzt, veränderte genau das, was verglichen wird.
    let compares =
        args.start_fleets != 1 || args.research_share.is_some() || args.fleet_cost != 1.0;
    if compares && args.script.ai.is_empty() {
        args.script.ai.push("alle:ruht".into());
        println!(
            "Vergleich mit festen Flotten: Die militärische Führung der KI ruht (--ai alle:aktiv schaltet sie ein)."
        );
    }
    let script = match args.script.script(data) {
        Ok(script) => script,
        Err(error) => {
            eprintln!("Fehler im Drehbuch: {error}");
            return ExitCode::FAILURE;
        }
    };
    let data = &match variant(data, &args) {
        Ok(data) => data,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    if let Some(seeds) = args.seeds {
        return overview(data, &args, seeds, &script);
    }
    let played = play(data, args.seed, args.years, &script, args.tax);
    let (sim, violation) = (&played.sim, played.violation.clone());
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
        "Seed {} nach {} Jahren. Je Monat Sold {:.1} % des Bauwerts und Ersatzteile für {:.1} % \
         der Bauteile; Werftaufschlag {:.0} %.",
        args.seed,
        args.years,
        100.0 * military.upkeep_per_month,
        100.0 * military.maintenance_per_month,
        100.0 * military.yard_fee
    );
    if !script.is_empty() {
        crate::war::print_aftermath(sim);
    }
    println!(
        "Haushalt über die ganze Laufzeit je Gemeinschaft (Einnahmen in Credits je Monat, \
         alles andere in Prozent der laufenden Einnahmen bzw. der Monate):"
    );
    print_table_header("Nr.");
    for (index, totals) in played.totals.iter().enumerate() {
        if args.polity.is_none_or(|only| only as usize == index) {
            print_table_row(&index.to_string(), totals);
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
            "\n{}{}: {} {}, {} Schiffe. Staatskasse {:.0}, Sold {:.0} je Monat, Soldrückstand {}. \
             Kasse des Staatsdienstes {:.0}.",
            polity.name,
            if polity.alive { "" } else { " (erloschen)" },
            owned,
            if owned == 1 { "Flotte" } else { "Flotten" },
            fleets.ship_count(polity.id),
            to_credits(economy.treasury(polity.id)),
            to_credits(monthly_upkeep(data, fleets, polity.id)),
            percent(fleets.arrears_of(polity.id)),
            to_credits(economy.staff_money(polity.id))
        );
        print_budget(sim, polity.id);
        if args.months {
            print_months(sim, polity.id);
        }
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

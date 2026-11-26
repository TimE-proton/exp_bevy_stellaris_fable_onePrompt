//! KI gegen KI ohne Grafik. Die Logik liegt in `sim::harness`; hier stehen nur
//! Kommandozeile und Ausgabe.

mod battle;
mod research;

use clap::{Parser, Subcommand};
use sim::GameData;
use sim::clock::{DAYS_PER_YEAR, date_of_tick};
use sim::harness::{Headless, RunSummary, run_ai_game, summarize, sweep};
use sim::state::{SaveGame, SimConfig};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "headless",
    about = "KONFLUENZ ohne Grafik: KI gegen KI, Sweeps, Determinismusprüfung"
)]
struct Cli {
    /// Verzeichnis mit den RON-Spieldaten (Standard: assets/data).
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Eine Partie durchlaufen lassen und die Chronik ausgeben.
    Run {
        #[arg(long, default_value_t = 1)]
        seed: u64,
        #[arg(long, default_value_t = 50)]
        years: u64,
        /// Spielstand am Ende hierhin schreiben.
        #[arg(long)]
        save: Option<PathBuf>,
        /// Nur die Zusammenfassung, keine Chronik.
        #[arg(long)]
        quiet: bool,
    },
    /// Viele Seeds parallel laufen lassen und Kennzahlen als Tabelle ausgeben.
    Sweep {
        /// Seed-Bereich, z. B. `0..32`.
        #[arg(long, default_value = "0..16", value_parser = parse_range)]
        seeds: (u64, u64),
        #[arg(long, default_value_t = 100)]
        years: u64,
        /// Parallele Läufe (Standard: Zahl der Kerne).
        #[arg(long)]
        jobs: Option<usize>,
        /// Ergebnis zusätzlich als CSV schreiben.
        #[arg(long)]
        csv: Option<PathBuf>,
    },
    /// Zustandsprüfsummen ausgeben. Zwei Aufrufe mit gleichem Seed müssen gleich sein.
    Hash {
        #[arg(long, default_value_t = 1)]
        seed: u64,
        #[arg(long, default_value_t = 3600)]
        days: u64,
        /// Abstand der Zwischenprüfsummen in Tagen.
        #[arg(long, default_value_t = 360)]
        every: u64,
        /// Jeden Takt rechnen statt Leertakte zu überspringen (muss dasselbe ergeben).
        #[arg(long)]
        exact: bool,
    },
    /// Volkswirtschaftliche Kennzahlen im Jahresabstand, zum Auswuchten der Balance.
    Economy {
        #[arg(long, default_value_t = 1)]
        seed: u64,
        #[arg(long, default_value_t = 30)]
        years: u64,
        /// Zusätzlich Preise und Bestände dieses Systems (Name) ausgeben.
        #[arg(long)]
        system: Option<String>,
        /// Nach den vollen Jahren noch so viele Tage weiterlaufen.
        #[arg(long, default_value_t = 0)]
        extra_days: u64,
    },
    /// Kennzahlen der erzeugten Galaxie.
    Galaxy {
        #[arg(long, default_value_t = 1)]
        seed: u64,
    },
    /// Übungsgefechte: Paarungen, Kontermatrix bei gleichen Kosten, Leistungsmessung.
    Battle(battle::BattleArgs),
    /// Forschung je Gemeinschaft und Jahrzehnt; mit --seeds die Übersicht über viele Seeds.
    Research(research::ResearchArgs),
}

fn parse_range(text: &str) -> Result<(u64, u64), String> {
    let (start, end) = text
        .split_once("..")
        .ok_or("erwartet wird START..ENDE, z. B. 0..32")?;
    let start: u64 = start.trim().parse().map_err(|_| "START ist keine Zahl")?;
    let end: u64 = end.trim().parse().map_err(|_| "ENDE ist keine Zahl")?;
    if end <= start {
        return Err("ENDE muss größer als START sein".into());
    }
    Ok((start, end))
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let data = match GameData::load_located(cli.data_dir.as_deref()) {
        Ok(data) => data,
        Err(error) => {
            eprintln!("Fehler in den Spieldaten: {error}");
            return ExitCode::FAILURE;
        }
    };
    match cli.command {
        Command::Battle(args) => return battle::run(&data, args),
        Command::Research(args) => return research::run(&data, args),
        Command::Run {
            seed,
            years,
            save,
            quiet,
        } => {
            let sim = run_ai_game(&data, seed, years * DAYS_PER_YEAR);
            if !quiet {
                for entry in &sim.world().resource::<sim::chronicle::Chronicle>().entries {
                    println!("{}  {}", date_of_tick(entry.tick), entry.text);
                }
                println!();
            }
            print_summary(&summarize(&sim));
            if let Some(path) = save {
                if let Err(error) = SaveGame::write(&sim.save(), &path) {
                    eprintln!("{error}");
                    return ExitCode::FAILURE;
                }
                println!("Spielstand: {}", path.display());
            }
        }
        Command::Sweep {
            seeds,
            years,
            jobs,
            csv,
        } => {
            let seeds: Vec<u64> = (seeds.0..seeds.1).collect();
            let jobs = jobs.unwrap_or_else(|| {
                std::thread::available_parallelism()
                    .map(|n| n.get())
                    .unwrap_or(1)
            });
            let started = std::time::Instant::now();
            let runs = sweep(&data, &seeds, years * DAYS_PER_YEAR, jobs);
            println!(
                "{:>6} {:>18} {:>6} {:>8} {:>12}  größte Gemeinschaft",
                "Seed", "Hash", "frei", "lebend", "Bevölk. Mio"
            );
            for run in &runs {
                let alive = run.polities.iter().filter(|p| p.alive).count();
                let total: f64 = run.polities.iter().map(|p| p.population).sum();
                let biggest = run
                    .polities
                    .iter()
                    .max_by(|a, b| a.population.total_cmp(&b.population))
                    .map(|p| {
                        format!(
                            "{} ({:.0} %)",
                            p.name,
                            100.0 * p.population / total.max(1.0)
                        )
                    })
                    .unwrap_or_default();
                println!(
                    "{:>6} {:>18x} {:>6} {:>8} {:>12.0}  {}",
                    run.seed, run.hash, run.free_planets, alive, total, biggest
                );
            }
            println!(
                "{} Läufe über {years} Jahre in {:.1} s mit {jobs} Threads",
                runs.len(),
                started.elapsed().as_secs_f64()
            );
            let mean = |f: fn(&RunSummary) -> f64| {
                runs.iter().map(f).sum::<f64>() / runs.len().max(1) as f64
            };
            println!(
                "Mittel: Versorgung {:.2}, Auslastung {:.2}, Stellen besetzt {:.2}, Preisniveau {:.2}, Preisspanne {:.1}, Preise am Anschlag {:.0} %, Frachter {:.0}",
                mean(|r| r.economy.essentials_met),
                mean(|r| r.economy.utilization),
                mean(|r| r.economy.employment),
                mean(|r| r.economy.price_level),
                mean(|r| r.economy.price_spread),
                100.0 * mean(|r| r.economy.pinned_prices),
                mean(|r| r.economy.freighters as f64)
            );
            let violations: Vec<_> = runs
                .iter()
                .filter_map(|run| run.violation.as_ref().map(|v| (run.seed, v.clone())))
                .collect();
            for (seed, violation) in &violations {
                eprintln!("INVARIANTE VERLETZT, Seed {seed}: {violation}");
            }
            if let Some(path) = csv {
                let mut text = String::from(
                    "seed,hash,days,free_planets,essentials_met,utilization,employment,price_level,price_spread,pinned_prices,freighters,polity,alive,planets,population\n",
                );
                for run in &runs {
                    for polity in &run.polities {
                        text.push_str(&format!(
                            "{},{:x},{},{},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{},{},{},{},{:.1}\n",
                            run.seed,
                            run.hash,
                            run.days,
                            run.free_planets,
                            run.economy.essentials_met,
                            run.economy.utilization,
                            run.economy.employment,
                            run.economy.price_level,
                            run.economy.price_spread,
                            run.economy.pinned_prices,
                            run.economy.freighters,
                            polity.name,
                            polity.alive,
                            polity.planets,
                            polity.population
                        ));
                    }
                }
                if let Err(error) = std::fs::write(&path, text) {
                    eprintln!("{}: {error}", path.display());
                    return ExitCode::FAILURE;
                }
                println!("CSV: {}", path.display());
            }
            if !violations.is_empty() {
                return ExitCode::FAILURE;
            }
        }
        Command::Hash {
            seed,
            days,
            every,
            exact,
        } => {
            let mut sim = Headless::new(data, SimConfig { seed, player: None });
            let every = every.max(1);
            println!("Tag {:>7}  {:016x}", 0, sim.hash());
            let mut done = 0;
            while done < days {
                let step = every.min(days - done);
                if exact {
                    sim.run_days_exact(step)
                } else {
                    sim.run_days(step)
                }
                done += step;
                println!("Tag {:>7}  {:016x}", done, sim.hash());
            }
        }
        Command::Economy {
            seed,
            years,
            system,
            extra_days,
        } => {
            let mut sim = Headless::new(data.clone(), SimConfig { seed, player: None });
            println!(
                "{:>5} {:>9} {:>8} {:>6} {:>6} {:>6} {:>6} {:>7} {:>6} {:>6} {:>9}   Geld in Tsd.: {:>7} {:>7} {:>7} {:>7} {:>7}",
                "Jahr",
                "Bev. Mio",
                "Kapital",
                "Arbeit",
                "Ausl.",
                "Vers.",
                "Preis",
                "Spanne",
                "fest",
                "Lohn",
                "Frachter",
                "Haush.",
                "Firmen",
                "Kontore",
                "Häuser",
                "Staat"
            );
            for year in 0..=years {
                if year > 0 {
                    sim.run_days(DAYS_PER_YEAR);
                }
                let stats = sim::harness::economy_stats(sim.world());
                println!(
                    "{:>5} {:>9.0} {:>8.0} {:>6.2} {:>6.2} {:>6.2} {:>6.2} {:>7.2} {:>6.2} {:>6.2} {:>4}/{:<4}                 {:>7.0} {:>7.0} {:>7.0} {:>7.0} {:>7.0}",
                    year,
                    stats.population,
                    stats.capital,
                    stats.employment,
                    stats.utilization,
                    stats.essentials_met,
                    stats.price_level,
                    stats.price_spread,
                    stats.pinned_prices,
                    stats.mean_wage,
                    stats.laden_freighters,
                    stats.freighters,
                    stats.household_money / 1000.0,
                    stats.firm_money / 1000.0,
                    stats.market_money / 1000.0,
                    stats.house_money / 1000.0,
                    stats.treasuries / 1000.0
                );
                println!(
                    "      Zusammenhalt {:>5.1}  unruhig {:>2}  autonom {:>2}  Abspaltungen {:>2}  Gemeinschaften {:>2}  Kolonien: lokal {:>4.2} Zentrum {:>4.2}  Eliten {:>6.0} Tsd.  Schulden {:>6.0} Tsd.",
                    stats.cohesion,
                    stats.restless_regions,
                    stats.autonomous_regions,
                    stats.secessions,
                    stats.polities_alive,
                    stats.colony_local_share,
                    stats.colony_center_share,
                    stats.elite_money / 1000.0,
                    stats.debt / 1000.0
                );
                if let Err(message) = sim::harness::check_invariants(sim.world()) {
                    eprintln!("INVARIANTE VERLETZT: {message}");
                    return ExitCode::FAILURE;
                }
            }
            sim.run_days(extra_days);
            let galaxy = sim.galaxy();
            let economy = sim.world().resource::<sim::economy::Economy>();
            let wanted = system.or_else(|| {
                sim.polities().list[0]
                    .capital
                    .map(|p| galaxy.system(galaxy.planet(p).system).name.clone())
            });
            let logistics = sim.world().resource::<sim::logistics::Logistics>();
            println!("\nWelten der ersten Gemeinschaft:");
            let first = sim.polities().list[0].id;
            let mut worlds: Vec<_> = galaxy.planets_of(first).collect();
            worlds.sort_by(|a, b| b.population.total_cmp(&a.population));
            for planet in worlds.iter().take(12) {
                let local = economy.planet(planet.id);
                let top = local
                    .capital
                    .iter()
                    .enumerate()
                    .max_by(|a, b| a.1.total_cmp(b.1))
                    .map(|(index, capital)| format!("{} {:.1}", data.recipes[index].name, capital))
                    .unwrap_or_default();
                println!(
                    "  {:<16} {:<11} {:>7.0} Mio.  Lohn {:>4.2}  Stellen {:>4.2}  Versorgung {:>4.2}  Kapital {:>6.1}  größtes: {}",
                    planet.name,
                    data.planet_kinds[planet.kind as usize].name,
                    planet.population,
                    local.wage,
                    local.employment,
                    local.essentials_met(&data.balance.economy),
                    local.total_capital(),
                    top
                );
                let region = sim
                    .world()
                    .resource::<sim::finance::Finance>()
                    .region(planet.id);
                let parts: Vec<String> = region
                    .factors
                    .contributions(&data.balance.finance.cohesion)
                    .into_iter()
                    .filter(|(_, points)| points.abs() >= 1.0)
                    .map(|(label, points)| format!("{label} {points:+.0}"))
                    .collect();
                println!(
                    "      Zusammenhalt {:>5.1} ({}), Ziel {:>5.1}: {}",
                    region.cohesion,
                    region.stage.label(),
                    region.factors.target,
                    parts.join(", ")
                );
            }
            println!("\nUnruhige und autonome Regionen (alle Gemeinschaften):");
            let finance = sim.world().resource::<sim::finance::Finance>();
            for planet in galaxy.planets.iter().filter(|p| p.owner.is_some()) {
                let region = finance.region(planet.id);
                if region.stage == sim::finance::Stage::Loyal {
                    continue;
                }
                let parts: Vec<String> = region
                    .factors
                    .contributions(&data.balance.finance.cohesion)
                    .into_iter()
                    .filter(|(_, points)| points.abs() >= 1.0)
                    .map(|(label, points)| format!("{label} {points:+.0}"))
                    .collect();
                println!(
                    "  {:<16} {:>6.0} Mio. {:>5.1} ({:<7}) lokal {:.2} Zentrum {:.2}: {}",
                    planet.name,
                    planet.population,
                    region.cohesion,
                    region.stage.label(),
                    region.factors.local_share,
                    region.factors.center_share,
                    parts.join(", ")
                );
            }
            let mut flows: Vec<_> = logistics.flows.iter().collect();
            flows.sort_by(|a, b| b.1.total_cmp(a.1));
            println!("\nGrößte Warenströme (gleitend):");
            for ((from, to, good), volume) in flows.iter().take(12) {
                println!(
                    "  {:>8.1}  {:<14} {} → {}",
                    volume,
                    data.good(*good).name,
                    galaxy.system(*from).name,
                    galaxy.system(*to).name
                );
            }
            for house in &logistics.houses {
                let ships = logistics
                    .freighters
                    .iter()
                    .filter(|f| f.alive && f.house == house.id)
                    .count();
                println!(
                    "  {:<28} {:>3} Frachter  Gewinn {:>7.1}/Tag  Kasse {:>8.0}",
                    house.name,
                    ships,
                    house.profit,
                    sim::economy::to_credits(house.cash)
                );
            }
            if let Some(found) =
                wanted.and_then(|name| galaxy.systems.iter().find(|s| s.name == name))
            {
                let market = economy.market(found.id);
                println!(
                    "\nMarkt {} (Kasse {:.0}, Umsatz {:.0}/Tag)",
                    found.name,
                    sim::economy::to_credits(market.cash),
                    market.turnover
                );
                println!(
                    "  {:<14} {:>8} {:>8} {:>9} {:>9} {:>9}",
                    "Gut", "Preis", "Richt", "Bestand", "Nachfr.", "Zufluss"
                );
                for (index, good) in data.goods.iter().enumerate() {
                    println!(
                        "  {:<14} {:>8.1} {:>8.1} {:>9.1} {:>9.2} {:>9.2}",
                        good.name,
                        market.price[index],
                        good.base_price,
                        market.stock[index],
                        market.demand[index],
                        market.supply[index]
                    );
                }
                for planet in galaxy.planets_in(found.id).filter(|p| p.owner.is_some()) {
                    let local = economy.planet(planet.id);
                    // Mit dem Ertragsfaktor der Technik des Besitzers: dieselbe Rechnung,
                    // aus der die Simulation Rendite und Ausstoß bildet.
                    let tech_yield = planet
                        .owner
                        .map_or(1.0, |owner| economy.yield_factor(owner));
                    println!(
                        "\n  {} ({:.0} Mio., Lohn {:.2}, Stellen besetzt {:.2}, Haushalte {:.0}, \
                         Ertragsfaktor der Technik {:.2})",
                        planet.name,
                        planet.population,
                        local.wage,
                        local.employment,
                        sim::economy::to_credits(local.households),
                        tech_yield
                    );
                    for (index, recipe) in data.recipes.iter().enumerate() {
                        if local.capital[index] > 0.0 {
                            let unit = sim::economy::unit_economics_at(
                                &data, planet, local.wage, market, recipe, tech_yield,
                            );
                            println!(
                                "    {:<16} Kapital {:>7.1}  Ausstoß {:>7.2}  Auslastung {:>4.2} (Trend {:>4.2})  Rendite {:>6.2}  je Einheit: Erlös {:>6.2} Vorprodukte {:>6.2} Energie {:>5.2} Start {:>5.2} Löhne {:>5.2}",
                                recipe.name,
                                local.capital[index],
                                local.output[index],
                                local.utilization[index],
                                local.utilization_trend[index],
                                local.roc[index],
                                unit.revenue,
                                unit.inputs,
                                unit.energy,
                                unit.lift,
                                unit.wages
                            );
                        }
                    }
                }
            }
        }
        Command::Galaxy { seed } => {
            let sim = Headless::new(data.clone(), SimConfig { seed, player: None });
            let galaxy = sim.galaxy();
            let bridges = galaxy.lanes.iter().filter(|lane| lane.bridge).count();
            println!(
                "Seed {seed}: {} Cluster, {} Systeme, {} Routen (davon {bridges} Brücken), {} Planeten",
                galaxy.clusters.len(),
                galaxy.systems.len(),
                galaxy.lanes.len(),
                galaxy.planets.len()
            );
            let chokepoints: Vec<&str> = galaxy
                .systems
                .iter()
                .filter(|s| s.chokepoint)
                .map(|s| s.name.as_str())
                .collect();
            println!("Engpässe: {}", chokepoints.join(", "));
            for (index, kind) in data.planet_kinds.iter().enumerate() {
                let count = galaxy
                    .planets
                    .iter()
                    .filter(|p| p.kind as usize == index)
                    .count();
                println!("  {:<12} {count}", kind.name);
            }
            for polity in &sim.polities().list {
                let home = galaxy.planet(polity.capital.expect("Heimatwelt"));
                println!(
                    "  {:<24} Heimat {} im System {}",
                    polity.name,
                    home.name,
                    galaxy.system(home.system).name
                );
            }
        }
    }
    ExitCode::SUCCESS
}

fn print_summary(summary: &RunSummary) {
    println!(
        "Seed {} nach {} Jahren: {} Systeme, {} Planeten, {} frei. Prüfsumme {:016x}",
        summary.seed,
        summary.days / DAYS_PER_YEAR,
        summary.systems,
        summary.planets,
        summary.free_planets,
        summary.hash
    );
    for polity in &summary.polities {
        println!(
            "  {:<24} {:>3} Planeten  {:>9.0} Mio. Einwohner{}",
            polity.name,
            polity.planets,
            polity.population,
            if polity.alive { "" } else { "  (erloschen)" }
        );
    }
}

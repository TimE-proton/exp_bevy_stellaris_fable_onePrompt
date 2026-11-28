//! KI gegen KI ohne Grafik. Die Logik liegt in `sim::harness`; hier stehen nur
//! Kommandozeile und Ausgabe.

mod battle;
mod budget;
mod diplo;
mod fleets;
mod research;
mod script;
mod siege;
mod war;

use clap::{Parser, Subcommand};
use sim::GameData;
use sim::clock::{DAYS_PER_YEAR, date_of_tick};
use sim::economy::{Claim, to_credits};
use sim::harness::{Headless, RunSummary, run_scripted, summarize, sweep_scripted};
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
        /// Kriege, Bestellungen und Flottenprogramme nach Drehbuch.
        #[command(flatten)]
        script: script::ScriptArgs,
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
        /// Dasselbe Drehbuch in jedem Lauf, wie bei `run`.
        #[command(flatten)]
        script: script::ScriptArgs,
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
        /// Drehbuch wie bei `run`: So gehen auch Gefechte und Werften in die Prüfsummen ein.
        #[command(flatten)]
        script: script::ScriptArgs,
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
    /// Flotten, Schiffe, Vorräte, Werften und Bauaufträge nach einigen Jahren.
    Fleets(fleets::FleetsArgs),
    /// Belagerung eines Planeten: Tagesbericht und die drei Wege im Vergleich.
    Siege(siege::SiegeArgs),
}

pub(crate) fn parse_range(text: &str) -> Result<(u64, u64), String> {
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
        Command::Fleets(args) => return fleets::run(&data, args),
        Command::Run {
            seed,
            years,
            save,
            quiet,
            script,
        } => {
            let script = match script.script(&data) {
                Ok(script) => script,
                Err(error) => return script_error(&error),
            };
            let (sim, violation) = run_scripted(&data, seed, years * DAYS_PER_YEAR, &script);
            if !quiet {
                for entry in &sim.world().resource::<sim::chronicle::Chronicle>().entries {
                    println!("{}  {}", date_of_tick(entry.tick), entry.text);
                }
                println!();
            }
            if !script.is_empty() {
                war::print_aftermath(&sim);
            }
            let summary = summarize(&sim);
            print_summary(&summary);
            if let Some(violation) = violation.or(summary.violation) {
                eprintln!("INVARIANTE VERLETZT: {violation}");
                return ExitCode::FAILURE;
            }
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
            script,
        } => {
            let script = match script.script(&data) {
                Ok(script) => script,
                Err(error) => return script_error(&error),
            };
            let seeds: Vec<u64> = (seeds.0..seeds.1).collect();
            let jobs = jobs.unwrap_or_else(|| {
                std::thread::available_parallelism()
                    .map(|n| n.get())
                    .unwrap_or(1)
            });
            let started = std::time::Instant::now();
            let runs = sweep_scripted(&data, &seeds, years * DAYS_PER_YEAR, jobs, &script);
            println!(
                "{:>6} {:>18} {:>6} {:>8} {:>12} {:>6} {:>6} {:>6} {:>8} {:>8} {:>8}  größte Gemeinschaft",
                "Seed",
                "Hash",
                "frei",
                "lebend",
                "Bevölk. Mio",
                "Vers.",
                "Zus.",
                "Preis",
                "Abspalt.",
                "Gefechte",
                "Verluste"
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
                    "{:>6} {:>18x} {:>6} {:>8} {:>12.0} {:>6.2} {:>6.1} {:>6.2} {:>8} {:>8} {:>8}  {}",
                    run.seed,
                    run.hash,
                    run.free_planets,
                    alive,
                    total,
                    run.economy.essentials_met,
                    run.economy.cohesion,
                    run.economy.price_level,
                    run.economy.secessions,
                    run.battles,
                    run.ships_lost,
                    biggest
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
                "Mittel: Bevölkerung {:.0} Mio., Versorgung {:.2}, Bedarf gedeckt {:.3}, Auslastung {:.2}, Stellen besetzt {:.2}, Preisniveau {:.2}, Preisspanne {:.1}, Preise am Anschlag {:.0} %, Frachter {:.0}, Zusammenhalt {:.1}, Abspaltungen {:.1}",
                mean(|r| r.economy.population),
                mean(|r| r.economy.essentials_met),
                mean(|r| r.economy.needs_met),
                mean(|r| r.economy.utilization),
                mean(|r| r.economy.employment),
                mean(|r| r.economy.price_level),
                mean(|r| r.economy.price_spread),
                100.0 * mean(|r| r.economy.pinned_prices),
                mean(|r| r.economy.freighters as f64),
                mean(|r| r.economy.cohesion),
                mean(|r| r.economy.secessions as f64)
            );
            let per_polity = |f: fn(&sim::harness::PolitySummary) -> f64| {
                let (sum, count) = runs
                    .iter()
                    .flat_map(|run| run.polities.iter())
                    .filter(|polity| polity.alive)
                    .fold((0.0, 0usize), |(sum, count), polity| {
                        (sum + f(polity), count + 1)
                    });
                sum / count.max(1) as f64
            };
            println!(
                "Je Gemeinschaft im Mittel: {:.1} Flotten, {:.1} Schiffe, Stärke {:.0}, Soldrückstand {:.1} %, {:.1} Technologien",
                per_polity(|p| p.fleets as f64),
                per_polity(|p| p.ships as f64),
                per_polity(|p| p.strength),
                100.0 * per_polity(|p| p.arrears),
                per_polity(|p| p.techs as f64)
            );
            // Haushalt der letzten zwölf Monate über alle lebenden Gemeinschaften: je
            // Anspruch der Anteil an den laufenden Einnahmen.
            let (mut revenue, mut months) = (0i64, 0usize);
            let mut paid: std::collections::BTreeMap<Claim, i64> = Default::default();
            for polity in runs.iter().flat_map(|run| run.polities.iter()) {
                if !polity.alive {
                    continue;
                }
                revenue += polity.budget.revenue;
                months += polity.budget_months;
                for claim in Claim::ALL {
                    *paid.entry(claim).or_default() += polity.budget.line(claim).paid;
                }
            }
            let share = |claim: Claim| 100.0 * paid[&claim] as f64 / revenue.max(1) as f64;
            println!(
                "Haushalt der letzten zwölf Monate im Mittel: Einnahmen {:.0} je Monat; Schulden {:.1} %, Grundleistungen {:.1} %, Flotte {:.1} %, Verteidigung {:.1} %, Beschaffung {:.1} %, Kontore {:.1} %, Forschung {:.1} %, Ausschüttung {:.1} %",
                to_credits(revenue) / months.max(1) as f64,
                share(Claim::DebtService),
                share(Claim::Civil),
                share(Claim::Fleet),
                share(Claim::Defense),
                share(Claim::Procurement),
                share(Claim::Markets),
                share(Claim::Research),
                share(Claim::Handout)
            );
            println!(
                "Gefechte je Lauf im Mittel: {:.1} mit {:.1} verlorenen Schiffen",
                mean(|r| r.battles as f64),
                mean(|r| r.ships_lost as f64)
            );
            let outcomes =
                |index: usize| -> u32 { runs.iter().map(|run| run.battle_outcomes[index]).sum() };
            let polity_sum = |f: fn(&sim::harness::PolitySummary) -> f64| -> f64 {
                runs.iter().flat_map(|run| run.polities.iter()).map(f).sum()
            };
            println!(
                "Gefechte nach Ausgang: {} entschieden, {} Patt, {} an der Zeitgrenze, {} eingestellt, {} ohne Sieger; ganze Flotten verloren: {:.0}",
                outcomes(0),
                outcomes(1),
                outcomes(2),
                outcomes(3),
                outcomes(4),
                polity_sum(|p| p.fleets_lost as f64)
            );
            let (applied, rejected) = runs.iter().fold((0u64, 0u64), |(a, r), run| {
                (a + run.ai_applied, r + run.ai_rejected)
            });
            println!(
                "Marine über alle Läufe: {:.0} Schiffe gebaut, {:.0} verloren, {:.0} stillgelegt; {:.0} Angriffe, {:.0} Blockadetage; {:.0} Monate im Krieg, {:.0} Monate mit Soldrückstand; Bauwert je Gemeinschaft {:.0}",
                polity_sum(|p| p.ships_built as f64),
                polity_sum(|p| p.ships_lost as f64),
                polity_sum(|p| p.ships_laid_up as f64),
                polity_sum(|p| p.offensives as f64),
                polity_sum(|p| p.blockade_days as f64),
                polity_sum(|p| p.months_at_war as f64),
                polity_sum(|p| p.months_in_arrears as f64),
                per_polity(|p| p.fleet_value)
            );
            println!(
                "Befehle der KI: {applied} angenommen, {rejected} abgelehnt ({:.2} %)",
                100.0 * rejected as f64 / (applied + rejected).max(1) as f64
            );
            let sieges = |f: fn(&sim::siege::SiegeStats) -> u32| -> u32 {
                runs.iter().map(|run| f(&run.sieges)).sum()
            };
            if sieges(|s| s.begun) > 0 {
                println!(
                    "Belagerungen über alle Läufe: {} begonnen; {} erstürmt, {} ausgehungert, {} übergeben, {} aufgehoben; {} Welten gefallen, {} Gemeinschaften beendet; längste {} Tage",
                    sieges(|s| s.begun),
                    sieges(|s| s.stormed),
                    sieges(|s| s.starved),
                    sieges(|s| s.capitulated),
                    sieges(|s| s.lifted),
                    sieges(|s| s.fallen),
                    sieges(|s| s.ended),
                    runs.iter().map(|run| run.sieges.longest).max().unwrap_or(0)
                );
                println!(
                    "  Seed  begonnen  erstürmt  ausgehungert  übergeben  aufgehoben  gefallen  beendet  längste  Feldzüge  Tanktage  Folge"
                );
                for run in &runs {
                    let s = &run.sieges;
                    println!(
                        "  {:>4}  {:>8}  {:>8}  {:>12}  {:>9}  {:>10}  {:>8}  {:>7}  {:>7}  {:>8}  {:>8}  {:>5}",
                        run.seed,
                        s.begun,
                        s.stormed,
                        s.starved,
                        s.capitulated,
                        s.lifted,
                        s.fallen,
                        s.ended,
                        s.longest,
                        run.polities
                            .iter()
                            .map(|polity| polity.campaigns)
                            .sum::<u32>(),
                        run.dry_fleet_days,
                        run.dry_longest
                    );
                }
            }
            {
                let ended = sieges(|s| s.stormed)
                    + sieges(|s| s.starved)
                    + sieges(|s| s.capitulated)
                    + sieges(|s| s.lifted);
                let days: u64 = runs.iter().map(|run| run.sieges.days).sum();
                let sum = |f: fn(&RunSummary) -> u32| -> u32 { runs.iter().map(f).sum() };
                println!(
                    "Belagerung und Verteidigung: {} Feldzüge der KI; mittlere Dauer einer Belagerung {:.0} Tage; verlorene Verbände {:.0} der Belagerer, {:.0} der Verteidiger; {} von {} Kolonien ohne Vorräte; {} Flottentage mit leerem Tank fern eigener Häfen, längste Folge {} Tage, {} Flottentage ohne jeden eigenen Hafen",
                    runs.iter()
                        .flat_map(|run| run.polities.iter())
                        .map(|polity| polity.campaigns)
                        .sum::<u32>(),
                    days as f64 / ended.max(1) as f64,
                    runs.iter()
                        .map(|run| run.sieges.lost_attackers)
                        .sum::<f64>(),
                    runs.iter()
                        .map(|run| run.sieges.lost_defenders)
                        .sum::<f64>(),
                    sum(|run| run.colonies_without_stores),
                    sum(|run| run.colonies),
                    sum(|run| run.dry_fleet_days),
                    runs.iter().map(|run| run.dry_longest).max().unwrap_or(0),
                    sum(|run| run.cut_off_days),
                );
            }
            let endings: Vec<String> = runs
                .iter()
                .filter_map(|run| ending_text(run).map(|text| format!("Seed {}: {text}", run.seed)))
                .collect();
            if endings.is_empty() {
                println!("Enden: Keine Partie endete in der Laufzeit.");
            } else {
                println!(
                    "Enden: {} von {} Partien endeten in der Laufzeit.",
                    endings.len(),
                    runs.len()
                );
                for ending in &endings {
                    println!("  {ending}");
                }
            }
            diplo::print(&runs, years);
            let violations: Vec<_> = runs
                .iter()
                .filter_map(|run| run.violation.as_ref().map(|v| (run.seed, v.clone())))
                .collect();
            for (seed, violation) in &violations {
                eprintln!("INVARIANTE VERLETZT, Seed {seed}: {violation}");
            }
            if let Some(path) = csv {
                let mut text = String::from(
                    "seed,hash,days,free_planets,essentials_met,utilization,employment,price_level,price_spread,pinned_prices,freighters,polity,alive,planets,population,fleets,ships,strength,arrears,battles,ships_lost,needs_met,revenue,fleet_paid,research_paid,handout,ships_built,ships_laid_up,fleets_lost,battles_decided,battles_stalemate,battles_timed_out,battles_called_off,battles_abandoned,months_at_war,months_in_arrears,fleet_value,blockade_days,offensives,ai_applied,ai_rejected,battles_won,ending,ending_year,defense_paid,sieges_begun,sieges_stormed,sieges_starved,sieges_capitulated,sieges_lifted,worlds_fallen,communities_ended,ai_campaigns,siege_days,siege_longest,siege_troops_lost_attackers,siege_troops_lost_defenders,colonies,colonies_without_stores,dry_fleet_days,dry_longest,cut_off_days",
                );
                text.push_str(diplo::CSV_HEADER);
                text.push('\n');
                for run in &runs {
                    for (index, polity) in run.polities.iter().enumerate() {
                        text.push_str(&format!(
                            "{},{:x},{},{},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{},{},{},{},{:.1},{},{},{:.1},{:.3},{},{},{:.3},{:.0},{:.0},{:.0},{:.0},{},{},{},{},{},{},{},{},{},{},{:.0},{},{},{},{},{},{},{},{:.0},{},{},{},{},{},{},{},{},{},{},{:.1},{:.1},{},{},{},{},{}{}\n",
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
                            polity.population,
                            polity.fleets,
                            polity.ships,
                            polity.strength,
                            polity.arrears,
                            run.battles,
                            polity.ships_lost,
                            run.economy.needs_met,
                            to_credits(polity.budget.revenue),
                            to_credits(polity.budget.line(Claim::Fleet).paid),
                            to_credits(polity.budget.line(Claim::Research).paid),
                            to_credits(polity.budget.handout()),
                            polity.ships_built,
                            polity.ships_laid_up,
                            polity.fleets_lost,
                            run.battle_outcomes[0],
                            run.battle_outcomes[1],
                            run.battle_outcomes[2],
                            run.battle_outcomes[3],
                            run.battle_outcomes[4],
                            polity.months_at_war,
                            polity.months_in_arrears,
                            polity.fleet_value,
                            polity.blockade_days,
                            polity.offensives,
                            run.ai_applied,
                            run.ai_rejected,
                            polity.battles_won,
                            // Ende der Partie und Jahr; leer, wenn sie nicht endete.
                            run.outcome
                                .map_or("", |outcome| outcome.ending.label()),
                            run.outcome
                                .map_or(String::new(), |outcome| outcome.year().to_string()),
                            to_credits(polity.budget.line(Claim::Defense).paid),
                            run.sieges.begun,
                            run.sieges.stormed,
                            run.sieges.starved,
                            run.sieges.capitulated,
                            run.sieges.lifted,
                            run.sieges.fallen,
                            run.sieges.ended,
                            polity.campaigns,
                            run.sieges.days,
                            run.sieges.longest,
                            run.sieges.lost_attackers,
                            run.sieges.lost_defenders,
                            run.colonies,
                            run.colonies_without_stores,
                            run.dry_fleet_days,
                            run.dry_longest,
                            run.cut_off_days,
                            diplo::csv_fields(run, index)
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
            script,
        } => {
            let script = match script.script(&data) {
                Ok(script) => script,
                Err(error) => return script_error(&error),
            };
            let mut sim = Headless::new(data, SimConfig { seed, player: None });
            let every = every.max(1);
            println!("Tag {:>7}  {:016x}", 0, sim.hash());
            let mut done = 0;
            while done < days {
                done += every.min(days - done);
                script.run_until(&mut sim, done, exact);
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
                "{:>5} {:>9} {:>8} {:>6} {:>6} {:>6} {:>6} {:>7} {:>6} {:>6} {:>9}   Geld in Tsd.: {:>7} {:>7} {:>7} {:>7} {:>7} {:>7}",
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
                "Staat",
                "Dienst"
            );
            for year in 0..=years {
                if year > 0 {
                    sim.run_days(DAYS_PER_YEAR);
                }
                let stats = sim::harness::economy_stats(sim.world());
                println!(
                    "{:>5} {:>9.0} {:>8.0} {:>6.2} {:>6.2} {:>6.2} {:>6.2} {:>7.2} {:>6.2} {:>6.2} {:>4}/{:<4}                 {:>7.0} {:>7.0} {:>7.0} {:>7.0} {:>7.0} {:>7.1}",
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
                    stats.treasuries / 1000.0,
                    stats.staff_money / 1000.0
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
        Command::Siege(args) => return siege::run(&data, args),
    }
    ExitCode::SUCCESS
}

/// Meldet eine Angabe des Drehbuchs, die sich nicht lesen ließ.
fn script_error(error: &str) -> ExitCode {
    eprintln!("Fehler im Drehbuch: {error}");
    ExitCode::FAILURE
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
    println!(
        "Gefechte: {}, darin verlorene Schiffe: {}",
        summary.battles, summary.ships_lost
    );
    println!(
        "Befehle der KI: {} angenommen, {} abgelehnt ({:.2} %)",
        summary.ai_applied,
        summary.ai_rejected,
        100.0 * summary.ai_rejected_share()
    );
    for polity in &summary.polities {
        println!(
            "  {:<24} {:>3} Planeten  {:>9.0} Mio. Einwohner  {:>2} {:<7}  {:>3} Schiffe  Stärke {:>5.0}{}{}{}{}",
            polity.name,
            polity.planets,
            polity.population,
            polity.fleets,
            if polity.fleets == 1 {
                "Flotte"
            } else {
                "Flotten"
            },
            polity.ships,
            polity.strength,
            if polity.orders > 0 {
                format!("  {} Bauaufträge", polity.orders)
            } else {
                String::new()
            },
            if polity.ships_lost > 0 {
                format!("  {} Schiffe verloren", polity.ships_lost)
            } else {
                String::new()
            },
            if polity.ships_built + polity.ships_laid_up > 0 {
                format!(
                    "  {} gebaut, {} stillgelegt",
                    polity.ships_built, polity.ships_laid_up
                )
            } else {
                String::new()
            },
            if polity.alive { "" } else { "  (erloschen)" }
        );
        println!(
            "  {:<24} Staatskasse {:>8.0}  Kriegsmüdigkeit {:>3.0}  Zusammenhalt {:>3.0}  Soldrückstand {:>3.0} %",
            "",
            polity.treasury,
            polity.war_weariness,
            polity.cohesion,
            100.0 * polity.arrears
        );
        if polity.budget_months > 0 {
            // Der Haushalt der letzten zwölf Monate: je Anspruch, was er von den laufenden
            // Einnahmen bekam.
            let budget = &polity.budget;
            let share = |claim: Claim| {
                100.0 * budget.line(claim).paid as f64 / budget.revenue.max(1) as f64
            };
            println!(
                "  {:<24} Haushalt: Einnahmen {:>6.0} je Monat; Schulden {:.0} %, Grundleistungen {:.0} %, Flotte {:.0} %, Verteidigung {:.1} %, Beschaffung {:.0} %, Kontore {:.0} %, Forschung {:.0} %, Ausschüttung {:.0} %",
                "",
                to_credits(budget.revenue) / polity.budget_months as f64,
                share(Claim::DebtService),
                share(Claim::Civil),
                share(Claim::Fleet),
                share(Claim::Defense),
                share(Claim::Procurement),
                share(Claim::Markets),
                share(Claim::Research),
                share(Claim::Handout)
            );
        }
    }
    match ending_text(summary) {
        Some(text) => println!("Ende: {text}"),
        None => println!("Ende: Die Partie ist in der Laufzeit nicht zu Ende gegangen."),
    }
}

/// Ausgang einer Partie in einem Satz: Ende, Jahr, Überlebende, die beiden Summen. `None`,
/// wenn sie in der Laufzeit nicht endete.
fn ending_text(summary: &RunSummary) -> Option<String> {
    let outcome = summary.outcome?;
    let survivor = outcome
        .survivor
        .and_then(|survivor| summary.polities.get(survivor.index()))
        .map_or_else(
            || "ohne Überlebende".to_string(),
            |polity| format!("es bleibt {}", polity.name),
        );
    Some(format!(
        "{} im Jahr {}, {survivor}; beigetreten {:.0} Mio., genommen {:.0} Mio.",
        outcome.ending.label(),
        outcome.year(),
        outcome.joined,
        outcome.forced
    ))
}

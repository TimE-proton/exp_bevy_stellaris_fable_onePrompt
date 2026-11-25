//! KI gegen KI ohne Grafik. Die Logik liegt in `sim::harness`; hier stehen nur
//! Kommandozeile und Ausgabe.

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
    /// Kennzahlen der erzeugten Galaxie.
    Galaxy {
        #[arg(long, default_value_t = 1)]
        seed: u64,
    },
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
            if let Some(path) = csv {
                let mut text =
                    String::from("seed,hash,days,free_planets,polity,alive,planets,population\n");
                for run in &runs {
                    for polity in &run.polities {
                        text.push_str(&format!(
                            "{},{:x},{},{},{},{},{},{:.1}\n",
                            run.seed,
                            run.hash,
                            run.days,
                            run.free_planets,
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

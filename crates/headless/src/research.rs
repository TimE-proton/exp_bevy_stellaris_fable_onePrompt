//! Forschung auf der Kommandozeile: der Verlauf einer Partie je Gemeinschaft und die
//! Übersicht über viele Seeds. Das Werkzeug zum Auswuchten von `balance.ron: research`.

use clap::Args;
use sim::GameData;
use sim::clock::{DAYS_PER_MONTH, DAYS_PER_YEAR};
use sim::data::TechBranch;
use sim::economy::to_credits;
use sim::finance::Finance;
use sim::harness::{Headless, RunSummary, check_invariants, summarize};
use sim::ids::PolityId;
use sim::state::SimConfig;
use std::process::ExitCode;

#[derive(Args)]
pub struct ResearchArgs {
    #[arg(long, default_value_t = 1)]
    seed: u64,
    #[arg(long, default_value_t = 100)]
    years: u64,
    /// Abstand der Zwischenstände in Jahren.
    #[arg(long, default_value_t = 10)]
    every: u64,
    /// Statt des Verlaufs einer Partie: Übersicht über viele Seeds, z. B. `0..8`.
    #[arg(long, value_parser = crate::parse_range)]
    seeds: Option<(u64, u64)>,
    /// Parallele Läufe der Übersicht (Standard: Zahl der Kerne).
    #[arg(long)]
    jobs: Option<usize>,
}

pub fn run(data: &GameData, args: ResearchArgs) -> ExitCode {
    match args.seeds {
        Some(range) => overview(data, range, args.years, args.every.max(1), args.jobs),
        None => history(data, args.seed, args.years, args.every.max(1)),
    }
}

/// Monatsmittel einer Gemeinschaft seit dem letzten Zwischenstand.
#[derive(Clone, Copy, Default)]
struct Sums {
    revenue: f64,
    spent: f64,
    points: f64,
    months: u32,
}

fn tiers(sim: &Headless, data: &GameData, polity: PolityId) -> String {
    TechBranch::ALL
        .map(|branch| sim.research().tier(data, polity, branch).to_string())
        .join(" ")
}

fn print_rows(sim: &Headless, data: &GameData, year: u64, sums: &[Sums]) {
    let research = sim.research();
    let galaxy = sim.galaxy();
    println!("\nJahr {year}");
    println!(
        "  {:<24} {:>8} {:>9} {:>9} {:>10} {:>7}  {:<9} {:>8}  Ziel",
        "Gemeinschaft",
        "Bev. Mio",
        "Einnahmen",
        "Forschung",
        "Punkte/Mon",
        "bekannt",
        "Stufen",
        "Nachlass"
    );
    for polity in &sim.polities().list {
        let Some(entry) = research.get(polity.id) else {
            continue;
        };
        let mean = sums.get(polity.id.index()).copied().unwrap_or_default();
        let months = mean.months.max(1) as f64;
        let (target, discount) = match entry.target {
            Some(tech) => {
                let slot = tech as usize;
                let cost = entry.cost(data, tech);
                (
                    format!(
                        "{} ({:.0} %)",
                        data.techs[slot].name,
                        100.0 * entry.progress[slot] / cost
                    ),
                    format!("{:.0} %", 100.0 * entry.spillover[slot]),
                )
            }
            None => ("–".to_string(), "–".to_string()),
        };
        println!(
            "  {:<24} {:>8.0} {:>9.0} {:>9.1} {:>10.1} {:>7}  {:<9} {:>8}  {}",
            polity.name,
            galaxy.population_of(polity.id),
            mean.revenue / months,
            mean.spent / months,
            mean.points / months,
            research.known_count(polity.id),
            tiers(sim, data, polity.id),
            discount,
            target
        );
    }
}

/// Eine Partie: Zwischenstände je Gemeinschaft, danach die Abschlüsse und der Einblick.
fn history(data: &GameData, seed: u64, years: u64, every: u64) -> ExitCode {
    let mut sim = Headless::new(data.clone(), SimConfig { seed, player: None });
    println!(
        "Seed {seed}, {years} Jahre. Einnahmen (geglättete Staatseinnahmen), Forschung \
         (Gehälter) und Punkte sind Monatsmittel seit dem vorigen Zwischenstand."
    );
    println!("Stufen je Zweig: Wirtschaft, Antrieb, Waffen, Verteidigung, Bodenkrieg.");
    let mut sums: Vec<Sums> = Vec::new();
    let mut known: Vec<Vec<bool>> = Vec::new();
    let mut completions: Vec<Vec<(f64, u16)>> = Vec::new();
    print_rows(&sim, data, 0, &sums);
    for month in 1..=years * 12 {
        sim.run_days(DAYS_PER_MONTH);
        let research = sim.research();
        let finance = sim.world().resource::<Finance>();
        for (index, entry) in research.polities.iter().enumerate() {
            if index >= sums.len() {
                // Neu (Spielbeginn oder Abspaltung): Was sie mitbringt, ist kein Abschluss.
                sums.push(Sums::default());
                known.push(entry.known.clone());
                completions.push(Vec::new());
            }
            let sum = &mut sums[index];
            sum.revenue += finance.polities.get(index).map_or(0.0, |s| s.revenue);
            sum.spent += to_credits(entry.spent_last_month);
            sum.points += entry.points_last_month;
            sum.months += 1;
            for (tech, now) in entry.known.iter().enumerate() {
                if *now && !known[index][tech] {
                    completions[index].push((month as f64 / 12.0, tech as u16));
                }
            }
            known[index].clone_from(&entry.known);
        }
        if month % (every * 12) == 0 || month == years * 12 {
            print_rows(&sim, data, month / 12, &sums);
            sums.iter_mut().for_each(|sum| *sum = Sums::default());
            if let Err(message) = check_invariants(sim.world()) {
                eprintln!("INVARIANTE VERLETZT: {message}");
                return ExitCode::FAILURE;
            }
        }
    }

    println!("\nAbschlüsse (Jahr, Technologie):");
    for polity in &sim.polities().list {
        let list: Vec<String> = completions
            .get(polity.id.index())
            .map(|entries| {
                entries
                    .iter()
                    .map(|(year, tech)| format!("{year:.1} {}", data.techs[*tech as usize].name))
                    .collect()
            })
            .unwrap_or_default();
        println!("  {:<24} {}", polity.name, list.join(" · "));
    }

    // Was die erste Gemeinschaft über die anderen weiß: Einblick, Schätzung, Wahrheit.
    let research = sim.research();
    let year = sim.clock().day() / DAYS_PER_YEAR;
    let Some(viewer) = sim.polities().list.first() else {
        return ExitCode::SUCCESS;
    };
    println!(
        "\nWas {} über die anderen weiß (Einblick, Schätzung je Zweig, in Klammern die Wahrheit):",
        viewer.name
    );
    for target in sim.polities().list.iter().skip(1) {
        let estimate = research.estimate(data, viewer.id, target.id, seed, year);
        let parts: Vec<String> = TechBranch::ALL
            .iter()
            .zip(estimate)
            .map(|(branch, range)| {
                format!(
                    "{} {range} ({})",
                    branch.label(),
                    research.tier(data, target.id, *branch)
                )
            })
            .collect();
        println!(
            "  {:<24} {:>4.2}  {}",
            target.name,
            research.intel(viewer.id, target.id),
            parts.join(", ")
        );
    }
    ExitCode::SUCCESS
}

/// Forschungsstand aller lebenden Gemeinschaften einer Partie zu einem Zeitpunkt.
struct Snapshot {
    year: u64,
    /// Je Gemeinschaft: Zahl der Technologien und höchste Stufe je Zweig.
    polities: Vec<(usize, [u8; 5])>,
}

fn snapshot(sim: &Headless, data: &GameData, year: u64) -> Snapshot {
    let research = sim.research();
    let galaxy = sim.galaxy();
    Snapshot {
        year,
        polities: sim
            .polities()
            .list
            .iter()
            .filter(|polity| polity.alive && galaxy.population_of(polity.id) > 0.0)
            .map(|polity| {
                (
                    research.known_count(polity.id),
                    TechBranch::ALL.map(|branch| research.tier(data, polity.id, branch)),
                )
            })
            .collect(),
    }
}

/// Eine Partie mit jährlicher Invariantenprüfung und Zwischenständen alle `every` Jahre.
fn run_with_snapshots(
    data: &GameData,
    seed: u64,
    years: u64,
    every: u64,
) -> (RunSummary, Vec<Snapshot>) {
    let mut sim = Headless::new(data.clone(), SimConfig { seed, player: None });
    let mut violation = None;
    let mut snapshots = Vec::new();
    for year in 1..=years {
        sim.run_days(DAYS_PER_YEAR);
        if violation.is_none()
            && let Err(message) = check_invariants(sim.world())
        {
            violation = Some(format!("Jahr {year}: {message}"));
        }
        if year % every == 0 || year == years {
            snapshots.push(snapshot(&sim, data, year));
        }
    }
    let mut summary = summarize(&sim);
    summary.violation = violation.or(summary.violation);
    (summary, snapshots)
}

fn mean_tiers(entries: &[(usize, [u8; 5])]) -> [f64; 5] {
    let mut sums = [0.0; 5];
    for (_, tiers) in entries {
        for (sum, tier) in sums.iter_mut().zip(tiers) {
            *sum += *tier as f64;
        }
    }
    sums.map(|sum| sum / entries.len().max(1) as f64)
}

/// Viele Seeds: der Forschungsstand im Verlauf und am Ende, daneben die Kennzahlen, an
/// denen sich zeigt, ob die Wirtschaft gesund bleibt.
fn overview(
    data: &GameData,
    seeds: (u64, u64),
    years: u64,
    every: u64,
    jobs: Option<usize>,
) -> ExitCode {
    let seeds: Vec<u64> = (seeds.0..seeds.1).collect();
    let jobs = jobs
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
        })
        .clamp(1, seeds.len().max(1));
    // Jeder Lauf ist für sich einfädig und deterministisch; die Ergebnisse werden nach
    // Seed geordnet, unabhängig davon, welcher Thread zuerst fertig ist.
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
                    let result = run_with_snapshots(data, *seed, years, every);
                    results
                        .lock()
                        .expect("Lauf abgestürzt")
                        .push((index, result));
                }
            });
        }
    });
    let mut results = results.into_inner().expect("Lauf abgestürzt");
    results.sort_by_key(|(index, _)| *index);
    let runs: Vec<(RunSummary, Vec<Snapshot>)> =
        results.into_iter().map(|(_, result)| result).collect();

    println!(
        "{:>5} {:>11} {:>8} {:>8} {:>7} {:>7} {:>7} {:>8} {:>6}  {:>15} {:>7}  Ø Stufen W A Wa V B",
        "Seed",
        "Bevölk. Mio",
        "Versorg.",
        "Auslast.",
        "Zusam.",
        "unruhig",
        "autonom",
        "Abspalt.",
        "lebend",
        "Techs min/Ø/max",
        "höchste"
    );
    for (run, snapshots) in &runs {
        let entries: &[(usize, [u8; 5])] = snapshots
            .last()
            .map_or(&[], |snapshot| snapshot.polities.as_slice());
        let counts: Vec<usize> = entries.iter().map(|(techs, _)| *techs).collect();
        let mean = counts.iter().sum::<usize>() as f64 / counts.len().max(1) as f64;
        let highest = entries
            .iter()
            .flat_map(|(_, tiers)| tiers.iter().copied())
            .max()
            .unwrap_or(0);
        let tiers = mean_tiers(entries);
        println!(
            "{:>5} {:>11.0} {:>8.2} {:>8.2} {:>7.1} {:>7} {:>7} {:>8} {:>6}  {:>6}/{:>4.1}/{:<3} {:>7}  {:.1} {:.1} {:.1} {:.1} {:.1}",
            run.seed,
            run.economy.population,
            run.economy.essentials_met,
            run.economy.utilization,
            run.economy.cohesion,
            run.economy.restless_regions,
            run.economy.autonomous_regions,
            run.economy.secessions,
            run.economy.polities_alive,
            counts.iter().min().copied().unwrap_or(0),
            mean,
            counts.iter().max().copied().unwrap_or(0),
            highest,
            tiers[0],
            tiers[1],
            tiers[2],
            tiers[3],
            tiers[4]
        );
    }
    let mean = |f: fn(&RunSummary) -> f64| {
        runs.iter().map(|(run, _)| f(run)).sum::<f64>() / runs.len().max(1) as f64
    };
    println!(
        "Mittel über {} Läufe nach {years} Jahren: Bevölkerung {:.0} Mio., Versorgung {:.3}, \
         Auslastung {:.3}, Stellen besetzt {:.3}, Preisniveau {:.3}, Zusammenhalt {:.1}, \
         unruhig {:.1}, autonom {:.1}, Abspaltungen {:.2}, Frachter {:.0}",
        runs.len(),
        mean(|r| r.economy.population),
        mean(|r| r.economy.essentials_met),
        mean(|r| r.economy.utilization),
        mean(|r| r.economy.employment),
        mean(|r| r.economy.price_level),
        mean(|r| r.economy.cohesion),
        mean(|r| r.economy.restless_regions as f64),
        mean(|r| r.economy.autonomous_regions as f64),
        mean(|r| r.economy.secessions as f64),
        mean(|r| r.economy.freighters as f64)
    );

    // Verlauf über alle Seeds: Wie weit sind die Gemeinschaften wann?
    let total = data.techs.len();
    println!(
        "\nVerlauf über alle Gemeinschaften aller Läufe ({total} Technologien im Baum):\n\
         {:>5} {:>15}  {:<24} {:>12} {:>10}",
        "Jahr", "Techs min/Ø/max", "Ø Stufen W A Wa V B", "höchste Stufe", "Baum fertig"
    );
    let steps = runs.first().map_or(0, |(_, snapshots)| snapshots.len());
    for step in 0..steps {
        let entries: Vec<(usize, [u8; 5])> = runs
            .iter()
            .filter_map(|(_, snapshots)| snapshots.get(step))
            .flat_map(|snapshot| snapshot.polities.iter().copied())
            .collect();
        let counts: Vec<usize> = entries.iter().map(|(techs, _)| *techs).collect();
        let tiers = mean_tiers(&entries);
        println!(
            "{:>5} {:>6}/{:>4.1}/{:<3}  {:<24} {:>12} {:>10}",
            runs[0].1[step].year,
            counts.iter().min().copied().unwrap_or(0),
            counts.iter().sum::<usize>() as f64 / counts.len().max(1) as f64,
            counts.iter().max().copied().unwrap_or(0),
            format!(
                "{:.1} {:.1} {:.1} {:.1} {:.1}",
                tiers[0], tiers[1], tiers[2], tiers[3], tiers[4]
            ),
            entries
                .iter()
                .flat_map(|(_, tiers)| tiers.iter().copied())
                .max()
                .unwrap_or(0),
            counts.iter().filter(|count| **count >= total).count()
        );
    }

    let mut failed = false;
    for (run, _) in &runs {
        if let Some(violation) = &run.violation {
            eprintln!("INVARIANTE VERLETZT, Seed {}: {violation}", run.seed);
            failed = true;
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

//! Übungsgefechte auf der Kommandozeile: einzelne Paarungen, die Kontermatrix bei gleichen
//! Kosten und die Leistungsmessung.

use clap::{Args, ValueEnum};
use sim::GameData;
use sim::battle::matchup::{self, Outcome, Side};
use sim::battle::{Doctrine, Formation};
use sim::data::ShipClass;
use std::process::ExitCode;
use std::time::Instant;

#[derive(Clone, Copy, ValueEnum)]
pub enum FormationArg {
    Linie,
    Keil,
    Schirm,
    Zange,
}

impl From<FormationArg> for Formation {
    fn from(value: FormationArg) -> Self {
        match value {
            FormationArg::Linie => Formation::Line,
            FormationArg::Keil => Formation::Wedge,
            FormationArg::Schirm => Formation::Screen,
            FormationArg::Zange => Formation::Pincer,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
pub enum DoctrineArg {
    Offensiv,
    Ausgewogen,
    Defensiv,
}

impl From<DoctrineArg> for Doctrine {
    fn from(value: DoctrineArg) -> Self {
        match value {
            DoctrineArg::Offensiv => Doctrine::Aggressive,
            DoctrineArg::Ausgewogen => Doctrine::Balanced,
            DoctrineArg::Defensiv => Doctrine::Defensive,
        }
    }
}

#[derive(Args)]
pub struct BattleArgs {
    /// Partei A, z. B. `speer:4,waechter:6`.
    #[arg(long)]
    a: Option<String>,
    /// Partei B.
    #[arg(long)]
    b: Option<String>,
    #[arg(long, value_enum, default_value_t = FormationArg::Linie)]
    formation_a: FormationArg,
    #[arg(long, value_enum, default_value_t = FormationArg::Linie)]
    formation_b: FormationArg,
    #[arg(long, value_enum, default_value_t = DoctrineArg::Ausgewogen)]
    doctrine_a: DoctrineArg,
    #[arg(long, value_enum, default_value_t = DoctrineArg::Ausgewogen)]
    doctrine_b: DoctrineArg,
    /// Munition der Partei A als Anteil (1 = volle Magazine).
    #[arg(long, default_value_t = 1.0)]
    ammo_a: f32,
    /// Aufschlag auf Schaden und Schutz der Partei A, z. B. 1.1 für eine Techstufe.
    #[arg(long, default_value_t = 1.0)]
    tech_a: f32,
    /// Seed-Bereich.
    #[arg(long, default_value = "0..8", value_parser = crate::parse_range)]
    seeds: (u64, u64),
    /// Kontermatrix: jede Klasse gegen jede, bei gleichem Bauwert und in beiden
    /// Aufstellungen (Zeile als Partei A und als Partei B).
    #[arg(long)]
    matrix: bool,
    /// Mit --matrix: statt der Tabelle die Kontertabelle `counter` für den Block `military`
    /// in `balance.ron` ausgeben, abgeleitet aus den Verlustanteilen der Matrix.
    #[arg(long)]
    ron: bool,
    /// Spiegelgefechte: jede Klasse und eine gemischte Flotte gegen sich selbst.
    #[arg(long)]
    mirror: bool,
    /// Die Paarung zusätzlich mit vertauschten Seiten rechnen.
    #[arg(long)]
    swap: bool,
    /// Partei A gegen jede einzelne Klasse vom selben Bauwert, in beiden Aufstellungen.
    #[arg(long)]
    versus_all: bool,
    /// Bauwert je Partei in der Matrix (Standard: zwei Schlachtschiffe).
    #[arg(long)]
    budget: Option<f64>,
    /// Leistungsmessung: gemischte Flotten mit zusammen so vielen Schiffen.
    #[arg(long)]
    bench: Option<u32>,
    /// Entwürfe und ihre Kennwerte auflisten.
    #[arg(long)]
    designs: bool,
    /// Verlauf einer Paarung im Abstand von so vielen Sekunden ausgeben (nur erster Seed).
    #[arg(long)]
    trace: Option<f32>,
}

pub fn run(data: &GameData, args: BattleArgs) -> ExitCode {
    if args.designs {
        list_designs(data);
        return ExitCode::SUCCESS;
    }
    if args.matrix {
        if args.ron {
            counter_table(data, &args);
        } else {
            matrix(data, &args);
        }
        return ExitCode::SUCCESS;
    }
    if args.mirror {
        mirror(data, &args);
        return ExitCode::SUCCESS;
    }
    if let Some(count) = args.bench {
        bench(data, count, args.seeds.0);
        return ExitCode::SUCCESS;
    }
    if args.versus_all {
        let Some(a) = &args.a else {
            eprintln!("Angabe fehlt: --versus-all braucht --a");
            return ExitCode::FAILURE;
        };
        match Side::parse(data, a) {
            Ok(mut side) => {
                side.formation = args.formation_a.into();
                side.doctrine = args.doctrine_a.into();
                versus_all(data, &side, args.seeds);
                return ExitCode::SUCCESS;
            }
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        }
    }
    let (Some(a), Some(b)) = (&args.a, &args.b) else {
        eprintln!(
            "Angabe fehlt: --a und --b, oder --a mit --versus-all, oder --matrix, --mirror, \
             --bench N, --designs"
        );
        return ExitCode::FAILURE;
    };
    let side = |text: &str| Side::parse(data, text);
    let (mut a, mut b) = match (side(a), side(b)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(error), _) | (_, Err(error)) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    a.formation = args.formation_a.into();
    b.formation = args.formation_b.into();
    a.doctrine = args.doctrine_a.into();
    b.doctrine = args.doctrine_b.into();
    a.ammo = args.ammo_a;
    a.modifiers.damage = args.tech_a;
    a.modifiers.shields = args.tech_a;
    a.modifiers.armor = args.tech_a;
    println!(
        "A: {} Schiffe, Bauwert {:.0}   B: {} Schiffe, Bauwert {:.0}",
        a.ship_count(),
        a.value(data),
        b.ship_count(),
        b.value(data)
    );
    if let Some(every) = args.trace {
        trace(data, args.seeds.0, &[a.clone(), b.clone()], every);
    }
    let sides = [a, b];
    let outcomes = fight_many(data, args.seeds, &sides);
    for (seed, outcome) in (args.seeds.0..).zip(&outcomes) {
        println!("  Seed {seed:<3} {}", describe(outcome));
    }
    println!("{}", Summary::of(&outcomes, false).line("A", "B"));
    if args.swap {
        let swapped = [sides[1].clone(), sides[0].clone()];
        let outcomes = fight_many(data, args.seeds, &swapped);
        println!(
            "Vertauscht: {}",
            Summary::of(&outcomes, true).line("A (als B)", "B (als A)")
        );
    }
    ExitCode::SUCCESS
}

/// Rechnet die Paarung für alle Seeds, auf alle Kerne verteilt. Jedes Gefecht ist für
/// sich deterministisch; die Ergebnisse kommen in Seed-Reihenfolge zurück.
fn fight_many(data: &GameData, seeds: (u64, u64), sides: &[Side]) -> Vec<Outcome> {
    let seeds: Vec<u64> = (seeds.0..seeds.1).collect();
    let jobs = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1)
        .min(seeds.len().max(1));
    let chunk = seeds.len().div_ceil(jobs.max(1)).max(1);
    std::thread::scope(|scope| {
        let handles: Vec<_> = seeds
            .chunks(chunk)
            .map(|part| {
                scope.spawn(move || {
                    part.iter()
                        .map(|seed| matchup::fight(data, *seed, sides))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("Gefechtsrechnung abgebrochen"))
            .collect()
    })
}

/// Zusammenfassung vieler Gefechte einer Paarung.
#[derive(Default)]
struct Summary {
    runs: u32,
    wins: [u32; 2],
    timeouts: u32,
    stalemates: u32,
    loss: [f32; 2],
    /// Mittlere Gefechtsdauer nach dem ersten Kontakt und die längste.
    fighting: f32,
    longest: f32,
    /// Mittlerer Verlust des Siegers und des Verlierers.
    winner_loss: f32,
    loser_loss: f32,
}

impl Summary {
    /// `swapped`: Die Seiten der Ausgänge sind vertauscht; gezählt wird aus Sicht der
    /// ursprünglichen Paarung.
    fn of(outcomes: &[Outcome], swapped: bool) -> Self {
        let mut summary = Summary::default();
        let map = |side: usize| if swapped { 1 - side } else { side };
        for outcome in outcomes {
            summary.runs += 1;
            if outcome.timed_out {
                summary.timeouts += 1;
            }
            if outcome.stalemate {
                summary.stalemates += 1;
            }
            if let Some(winner) = outcome.winner() {
                summary.wins[map(winner)] += 1;
                summary.winner_loss += outcome.sides[winner].loss_share();
                summary.loser_loss += outcome.sides[1 - winner].loss_share();
            }
            for side in 0..2 {
                summary.loss[map(side)] += outcome.sides[side].loss_share();
            }
            summary.fighting += outcome.fighting_time();
            summary.longest = summary.longest.max(outcome.fighting_time());
        }
        summary
    }

    fn decided(&self) -> u32 {
        self.wins[0] + self.wins[1]
    }

    fn line(&self, a: &str, b: &str) -> String {
        let runs = self.runs.max(1) as f32;
        let decided = self.decided().max(1) as f32;
        format!(
            "{a} siegt {}×, {b} siegt {}×, offen {}× (Zeit um {}×, Patt {}×). Verlust {a} {:.0} %, {b} {:.0} %; \
             Sieger {:.0} %, Verlierer {:.0} %. Gefecht nach Kontakt {:.0} s im Mittel, längstens {:.0} s.",
            self.wins[0],
            self.wins[1],
            self.runs - self.decided(),
            self.timeouts,
            self.stalemates,
            100.0 * self.loss[0] / runs,
            100.0 * self.loss[1] / runs,
            100.0 * self.winner_loss / decided,
            100.0 * self.loser_loss / decided,
            self.fighting / runs,
            self.longest,
        )
    }
}

/// Jede Klasse gegen sich selbst, dazu eine gemischte Flotte: Gewinnt Partei A so oft wie B?
fn mirror(data: &GameData, args: &BattleArgs) {
    let known = vec![false; data.techs.len()];
    let budget = args.budget.unwrap_or_else(|| default_budget(data));
    let mut fleets: Vec<(String, Side)> = ShipClass::ALL
        .iter()
        .filter(|class| **class != ShipClass::Fighter)
        .filter_map(|class| {
            data.best_design(*class, &known)
                .map(|design| (*class, design))
        })
        .map(|(class, design)| {
            (
                class.label().to_string(),
                Side::for_budget(data, design, budget),
            )
        })
        .collect();
    fleets.push((
        "gemischt".into(),
        Side::parse(data, MIXED).expect("gemischte Flotte"),
    ));
    for (label, side) in fleets {
        let outcomes = fight_many(data, args.seeds, &[side.clone(), side.clone()]);
        let summary = Summary::of(&outcomes, false);
        println!(
            "{label:<15} {}×: A {:.0} % der entschiedenen. {}",
            side.ship_count(),
            100.0 * summary.wins[0] as f32 / summary.decided().max(1) as f32,
            summary.line("A", "B")
        );
    }
}

/// Partei `fleet` gegen jede Klasse allein, vom selben Bauwert.
fn versus_all(data: &GameData, fleet: &Side, seeds: (u64, u64)) {
    let known = vec![false; data.techs.len()];
    let budget = fleet.value(data);
    for class in ShipClass::ALL {
        let Some(design) = (class != ShipClass::Fighter)
            .then(|| data.best_design(class, &known))
            .flatten()
        else {
            continue;
        };
        let other = Side::for_budget(data, design, budget);
        let forward = fight_many(data, seeds, &[fleet.clone(), other.clone()]);
        let backward = fight_many(data, seeds, &[other.clone(), fleet.clone()]);
        let forward = Summary::of(&forward, false);
        let backward = Summary::of(&backward, true);
        let runs = (forward.runs + backward.runs).max(1) as f32;
        println!(
            "gegen {:>2}×{:<15} (Wert {:>6.0}): siegt {:>2}/{} als A, {:>2}/{} als B; Verlust {:.0} % : {:.0} %",
            other.ship_count(),
            class.label(),
            other.value(data),
            forward.wins[0],
            forward.runs,
            backward.wins[0],
            backward.runs,
            100.0 * (forward.loss[0] + backward.loss[0]) / runs,
            100.0 * (forward.loss[1] + backward.loss[1]) / runs,
        );
    }
}

/// Eine ausgewogene Flotte vom Wert zweier Schlachtschiffe.
const MIXED: &str = "bastion:1,speer:1,waechter:1,dolch:2";

fn default_budget(data: &GameData) -> f64 {
    let known = vec![false; data.techs.len()];
    ShipClass::ALL
        .iter()
        .filter(|class| **class != ShipClass::Fighter)
        .filter_map(|class| data.best_design(*class, &known))
        .map(|design| data.designs[design as usize].stats.value)
        .fold(0.0, f64::max)
        * 2.0
}

fn counts(values: &[u32; 6]) -> String {
    let parts: Vec<String> = ShipClass::ALL
        .iter()
        .filter(|class| values[class.index()] > 0)
        .map(|class| format!("{} {}", values[class.index()], short(*class)))
        .collect();
    if parts.is_empty() {
        "–".into()
    } else {
        parts.join(" ")
    }
}

fn short(class: ShipClass) -> &'static str {
    match class {
        ShipClass::Fighter => "Jg",
        ShipClass::Corvette => "Kv",
        ShipClass::Frigate => "Fr",
        ShipClass::Cruiser => "Kr",
        ShipClass::Battleship => "Ss",
        ShipClass::Carrier => "Tr",
    }
}

fn describe(outcome: &Outcome) -> String {
    let result = match outcome.winner() {
        Some(0) => "A siegt",
        Some(_) => "B siegt",
        None if outcome.timed_out => "Zeit um",
        None if outcome.stalemate => "Patt",
        None => "kein Sieger",
    };
    let side = |index: usize| {
        let side = &outcome.sides[index];
        format!(
            "verliert {:>3.0} % ({}){}{}",
            100.0 * side.loss_share(),
            counts(&side.lost),
            if side.escaped.iter().sum::<u32>() > 0 {
                format!(", entkommen {}", counts(&side.escaped))
            } else {
                String::new()
            },
            if side.retreated { ", Rückzug" } else { "" }
        )
    };
    format!(
        "{result:<11} nach {:>4.0} s   A {}   B {}",
        outcome.time,
        side(0),
        side(1)
    )
}

fn list_designs(data: &GameData) {
    println!(
        "{:<10} {:<14} {:>8} {:>6} {:>6} {:>22} {:>5} {:>6} {:>6} {:>5} {:>5} {:>6}",
        "Entwurf",
        "Klasse",
        "Bauwert",
        "Rumpf",
        "Schild",
        "Panzer B/St/H/Bb",
        "Fahrt",
        "Reichw",
        "Breits",
        "Strom",
        "Mun",
        "Jäger"
    );
    for design in &data.designs {
        let stats = &design.stats;
        println!(
            "{:<10} {:<14} {:>8.0} {:>6.0} {:>6.0} {:>22} {:>5.0} {:>6.0} {:>6.2} {:>5.2} {:>5.0} {:>6}",
            design.key,
            stats.class.label(),
            stats.value,
            stats.hull,
            stats.shield,
            format!(
                "{:.0}/{:.0}/{:.0}/{:.0}",
                stats.armor[0], stats.armor[1], stats.armor[2], stats.armor[3]
            ),
            stats.speed,
            stats.main_range,
            stats.broadside,
            if stats.power_draw > 0.0 {
                stats.power_output / stats.power_draw
            } else {
                0.0
            },
            stats.ammo,
            stats.fighters
        );
    }
}

/// Jede Klasse gegen jede bei gleichem Bauwert, in beiden Aufstellungen. Eine Zelle zeigt,
/// wie oft die Zeilenklasse als Partei A und als Partei B siegt, dahinter die mittleren
/// Verlustanteile beider Seiten.
fn matrix(data: &GameData, args: &BattleArgs) {
    let known = vec![false; data.techs.len()];
    let mut classes: Vec<(String, Side)> = ShipClass::ALL
        .iter()
        .filter(|class| **class != ShipClass::Fighter)
        .filter_map(|class| {
            data.best_design(*class, &known)
                .map(|design| (*class, design))
        })
        .map(|(class, design)| {
            let budget = args.budget.unwrap_or_else(|| default_budget(data));
            let side = Side::for_budget(data, design, budget);
            (format!("{}×{}", side.ship_count(), class.label()), side)
        })
        .collect();
    classes.push((
        "gemischt".into(),
        Side::parse(data, MIXED).expect("gemischte Flotte"),
    ));
    println!(
        "Bauwert je Partei: {:.0}. Zelle: Siege der Zeile als A / als B (je {} Läufe), Verlust Zeile % : Spalte %.",
        args.budget.unwrap_or_else(|| default_budget(data)),
        args.seeds.1 - args.seeds.0
    );
    print!("{:<18}", "");
    for (label, _) in &classes {
        print!("{label:>20}");
    }
    println!();
    for (row_label, row) in &classes {
        print!("{row_label:<18}");
        for (_, column) in &classes {
            let forward = fight_many(data, args.seeds, &[row.clone(), column.clone()]);
            let backward = fight_many(data, args.seeds, &[column.clone(), row.clone()]);
            let forward = Summary::of(&forward, false);
            let backward = Summary::of(&backward, true);
            let runs = (forward.runs + backward.runs).max(1) as f32;
            print!(
                "{:>20}",
                format!(
                    "{}/{} {:>3.0}:{:<3.0}",
                    forward.wins[0],
                    backward.wins[0],
                    100.0 * (forward.loss[0] + backward.loss[0]) / runs,
                    100.0 * (forward.loss[1] + backward.loss[1]) / runs
                )
            );
        }
        println!();
    }
}

/// Sockel der Kontertabelle: Ein Verlustanteil von null zählt nicht als unendlich gut.
const COUNTER_FLOOR: f32 = 0.25;

/// Die Kontertabelle für `balance.ron: military.counter`, abgeleitet aus der Matrix: jede
/// Klasse gegen jede bei gleichem Bauwert, in beiden Aufstellungen. Der Wert einer Zelle
/// ist die Wurzel aus dem Verhältnis der Verlustanteile (Spalte zu Zeile, je mit einem
/// Sockel): 1 heißt gleichwertig, über 1 ist die Zeilenklasse der Spaltenklasse überlegen.
/// Zwei reine Flotten gleichen Werts stehen dann im Kräfteverhältnis Zelle durch
/// Gegenzelle. Jäger kämpfen nur mit ihrem Träger und stehen mit 1 in der Tabelle.
fn counter_table(data: &GameData, args: &BattleArgs) {
    let known = vec![false; data.techs.len()];
    let budget = args.budget.unwrap_or_else(|| default_budget(data));
    let side = |class: ShipClass| {
        (class != ShipClass::Fighter)
            .then(|| data.best_design(class, &known))
            .flatten()
            .map(|design| Side::for_budget(data, design, budget))
    };
    println!(
        "        // Kontertabelle: Zeile gegen Spalte in der Reihenfolge Jäger, Korvette, Fregatte,\n\
         \x20       // Kreuzer, Schlachtschiff, Träger. Abgeleitet mit `headless battle --matrix --ron\n\
         \x20       // --seeds {}..{}` bei Bauwert {budget:.0} je Partei.\n\
         \x20       counter: [",
        args.seeds.0, args.seeds.1
    );
    for row_class in ShipClass::ALL {
        let mut cells = Vec::new();
        for column_class in ShipClass::ALL {
            let value = match (side(row_class), side(column_class)) {
                (Some(row), Some(column)) if row_class != column_class => {
                    let forward = fight_many(data, args.seeds, &[row.clone(), column.clone()]);
                    let backward = fight_many(data, args.seeds, &[column, row]);
                    let forward = Summary::of(&forward, false);
                    let backward = Summary::of(&backward, true);
                    let runs = (forward.runs + backward.runs).max(1) as f32;
                    let own = (forward.loss[0] + backward.loss[0]) / runs;
                    let other = (forward.loss[1] + backward.loss[1]) / runs;
                    ((COUNTER_FLOOR + other) / (COUNTER_FLOOR + own)).sqrt()
                }
                _ => 1.0,
            };
            cells.push(format!("{value:.2}"));
        }
        println!(
            "            [{}], // {}",
            cells.join(", "),
            row_class.label()
        );
    }
    println!("        ],");
}

/// Zwei gemischte Flotten, dazu Zeit je Takt und Höchststand an Geschossen.
fn bench(data: &GameData, ships: u32, seed: u64) {
    let per_side = (ships / 2).max(6);
    let mix = |total: u32| {
        // Anteile: 1 Träger, 2 Schlachtschiffe, 5 Kreuzer, 8 Fregatten, 8 Korvetten je 24.
        let unit = total as f32 / 24.0;
        let count = |share: f32| ((share * unit).round() as u32).max(1);
        format!(
            "horst:{},bastion:{},speer:{},waechter:{},dolch:{}",
            count(1.0),
            count(2.0),
            count(5.0),
            count(8.0),
            count(8.0)
        )
    };
    let side = Side::parse(data, &mix(per_side)).expect("Benchmark-Flotte");
    let sides = [side.clone(), side];
    let mut battle = matchup::build(data, seed, &sides);
    let start = Instant::now();
    let (mut peak_shots, mut peak_ships, mut worst) = (0, 0, 0.0f64);
    while !battle.finished {
        let before = Instant::now();
        battle.step(data);
        worst = worst.max(before.elapsed().as_secs_f64());
        peak_shots = peak_shots.max(battle.shots.len());
        peak_ships = peak_ships.max(
            battle
                .ships
                .iter()
                .filter(|ship| ship.status.in_play())
                .count(),
        );
    }
    let elapsed = start.elapsed().as_secs_f64();
    let outcome = matchup::outcome_of(&battle);
    println!(
        "{} Schiffe je Partei, mit Jägern bis zu {peak_ships} zugleich im Feld, bis zu {peak_shots} Geschosse.",
        sides[0].ship_count()
    );
    println!(
        "{} Takte ({:.0} s Gefechtszeit) in {:.2} s: {:.0} µs je Takt im Mittel, {:.0} µs im schlechtesten. \
         Budget bei 20 Takten je Sekunde: 50000 µs.",
        battle.tick,
        battle.time,
        elapsed,
        1e6 * elapsed / battle.tick.max(1) as f64,
        1e6 * worst
    );
    println!("  {}", describe(&outcome));
}

fn trace(data: &GameData, seed: u64, sides: &[Side], every: f32) {
    let mut battle = matchup::build(data, seed, sides);
    let step = ((every / data.balance.battle.dt).round() as u64).max(1);
    println!("Verlauf (Seed {seed}):");
    while !battle.finished {
        battle.step(data);
        if battle.tick.is_multiple_of(step) || battle.finished {
            let mut line = format!("  {:>5.0} s", battle.time);
            for (index, combatant) in battle.combatants.iter().enumerate() {
                let ships: Vec<&sim::battle::BattleShip> = battle
                    .ships
                    .iter()
                    .filter(|ship| ship.side as usize == index && ship.status.can_act())
                    .collect();
                let hull: f32 = ships.iter().map(|ship| ship.hull).sum();
                let shield: f32 = ships.iter().map(|ship| ship.shield).sum();
                let armor: f32 = ships
                    .iter()
                    .map(|ship| ship.armor.iter().sum::<f32>())
                    .sum();
                let ammo: f32 = ships.iter().map(|ship| ship.ammo).sum();
                let morale: f32 =
                    ships.iter().map(|ship| ship.morale).sum::<f32>() / ships.len().max(1) as f32;
                let dealt = combatant.damage_dealt;
                line += &format!(
                    "   {}: {} aktiv ({}), Rumpf {hull:.0} Schild {shield:.0} Panzer {armor:.0} Mun {ammo:.0} Moral {morale:.0}{}  Anker ({:.0},{:.0}) Schaden S/G/L/P {:.0}/{:.0}/{:.0}/{:.0}",
                    if index == 0 { "A" } else { "B" },
                    ships.len(),
                    counts(&battle.active_by_class(index)),
                    if combatant.retreating {
                        " Rückzug"
                    } else {
                        ""
                    },
                    combatant.anchor.x,
                    combatant.anchor.y,
                    dealt[0],
                    dealt[1],
                    dealt[2],
                    dealt[3],
                );
            }
            line += &format!("   Geschosse {}", battle.shots.len());
            println!("{line}");
        }
    }
}

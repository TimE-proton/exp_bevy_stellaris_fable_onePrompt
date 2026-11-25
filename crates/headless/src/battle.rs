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
    /// Kontermatrix: jede Klasse gegen jede, bei gleichem Bauwert.
    #[arg(long)]
    matrix: bool,
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
        matrix(data, &args);
        return ExitCode::SUCCESS;
    }
    if let Some(count) = args.bench {
        bench(data, count, args.seeds.0);
        return ExitCode::SUCCESS;
    }
    let (Some(a), Some(b)) = (&args.a, &args.b) else {
        eprintln!("Angabe fehlt: --a und --b, oder --matrix, --bench N, --designs");
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
    let mut wins = [0u32; 2];
    let mut loss = [0.0f32; 2];
    let mut time = 0.0;
    let runs = (args.seeds.1 - args.seeds.0) as f32;
    for seed in args.seeds.0..args.seeds.1 {
        let outcome = matchup::fight(data, seed, &sides);
        if let Some(winner) = outcome.winner() {
            wins[winner] += 1;
        }
        for side in 0..2 {
            loss[side] += outcome.sides[side].loss_share();
        }
        time += outcome.time;
        println!("  Seed {seed:<3} {}", describe(&outcome));
    }
    println!(
        "A siegt {}×, B siegt {}×, unentschieden {}×. Mittlerer Verlust A {:.0} %, B {:.0} %, Dauer {:.0} s.",
        wins[0],
        wins[1],
        runs as u32 - wins[0] - wins[1],
        100.0 * loss[0] / runs,
        100.0 * loss[1] / runs,
        time / runs
    );
    ExitCode::SUCCESS
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

/// Jede Klasse gegen jede bei gleichem Bauwert. Eine Zelle zeigt, wie oft die Zeilenklasse
/// siegt, und dahinter die mittleren Verlustanteile beider Seiten.
fn matrix(data: &GameData, args: &BattleArgs) {
    let known = vec![false; data.techs.len()];
    let classes: Vec<(ShipClass, u16)> = ShipClass::ALL
        .iter()
        .filter(|class| **class != ShipClass::Fighter)
        .filter_map(|class| {
            data.best_design(*class, &known)
                .map(|design| (*class, design))
        })
        .collect();
    let budget = args.budget.unwrap_or_else(|| {
        classes
            .iter()
            .map(|(_, design)| data.designs[*design as usize].stats.value)
            .fold(0.0, f64::max)
            * 2.0
    });
    println!(
        "Bauwert je Partei: {budget:.0}. Zelle: Siege der Zeile / Läufe, Verlust Zeile % : Spalte %."
    );
    print!("{:<16}", "");
    for (class, design) in &classes {
        let count = Side::for_budget(data, *design, budget).ship_count();
        print!("{:>18}", format!("{}×{}", count, class.label()));
    }
    println!();
    let runs = args.seeds.1 - args.seeds.0;
    for (row_class, row_design) in &classes {
        print!("{:<16}", row_class.label());
        for (_, column_design) in &classes {
            let sides = [
                Side::for_budget(data, *row_design, budget),
                Side::for_budget(data, *column_design, budget),
            ];
            let (mut wins, mut loss_row, mut loss_column) = (0, 0.0, 0.0);
            for seed in args.seeds.0..args.seeds.1 {
                let outcome = matchup::fight(data, seed, &sides);
                if outcome.better_side() == Some(0) {
                    wins += 1;
                }
                loss_row += outcome.sides[0].loss_share();
                loss_column += outcome.sides[1].loss_share();
            }
            print!(
                "{:>18}",
                format!(
                    "{wins}/{runs} {:>3.0}:{:<3.0}",
                    100.0 * loss_row / runs as f32,
                    100.0 * loss_column / runs as f32
                )
            );
        }
        println!();
    }
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
                line += &format!(
                    "   {}: {} aktiv ({}), Rumpf {hull:.0} Schild {shield:.0} Panzer {armor:.0} Mun {ammo:.0} Moral {morale:.0}{}  Anker ({:.0},{:.0})",
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
                );
            }
            line += &format!("   Geschosse {}", battle.shots.len());
            println!("{line}");
        }
    }
}

//! Belagerung auf der Kommandozeile: ein Planet, eine Flotte, stehende Befehle. Rechnet
//! den Tagesbericht mit dem reinen Tagesrechner und zeigt die drei Wege des Belagerers im
//! Vergleich. Das Werkzeug zum Auswuchten von `balance.ron: siege`; eine Partie braucht es
//! nicht.

use clap::{Args, ValueEnum};
use sim::GameData;
use sim::data::{SiegeBalance, siege_day_bound};
use sim::ids::{PlanetId, PolityId};
use sim::siege::{
    Course, Fire, Forecast, Gate, OrbitView, PlanetDefense, PlanetFacts, Posture, Projection,
    SiegeView, TechView, forecast, form, occupation_force, will,
};
use std::collections::BTreeMap;
use std::process::ExitCode;

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum PresetArg {
    /// Kolonie mit 50 Mio. Einwohnern, nur Miliz.
    Kolonie,
    /// Welt mit 500 Mio. Einwohnern, nur Miliz.
    Welt,
    /// Heimatwelt mit 4000 Mio. Einwohnern und der Verteidigung vom Spielbeginn.
    Heimatwelt,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum FireArg {
    Keins,
    Stuetzen,
    Gezielt,
    Flaechig,
}

impl From<FireArg> for Fire {
    fn from(value: FireArg) -> Self {
        match value {
            FireArg::Keins => Fire::Hold,
            FireArg::Stuetzen => Fire::Support,
            FireArg::Gezielt => Fire::Precise,
            FireArg::Flaechig => Fire::Area,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
pub enum PostureArg {
    Vorwaerts,
    Tiefe,
}

#[derive(Args)]
pub struct SiegeArgs {
    /// Bezugswelt, deren Werte gelten, soweit keine Option sie überschreibt.
    #[arg(long, value_enum, default_value_t = PresetArg::Welt)]
    preset: PresetArg,
    /// Statt eines Verlaufs: die drei Wege für alle drei Bezugswelten.
    #[arg(long)]
    reference: bool,

    // --- Planet ---
    /// Bevölkerung in Millionen.
    #[arg(long)]
    population: Option<f64>,
    /// Zusammenhalt der Region, 0..100.
    #[arg(long, default_value_t = 70.0)]
    cohesion: f64,
    /// Anteil des Belagerers an den Betrieben der Welt, 0..1.
    #[arg(long, default_value_t = 0.0)]
    stake: f64,
    /// Deckung des Lebensnotwendigen, 0..1.
    #[arg(long, default_value_t = 1.0)]
    essentials: f64,
    /// Anteil, den die Garnison von ihrem Einkauf am Markt vor Ort bekommt, 0..1.
    #[arg(long, default_value_t = 1.0)]
    forage: f64,
    /// Buchwert der Betriebe in Credits (Standard: nach der Bevölkerung der Bezugswelt).
    #[arg(long)]
    firm_value: Option<f64>,

    // --- Verteidigung ---
    /// Stufen der Orbitalstationen.
    #[arg(long)]
    stations: Option<f32>,
    /// Stufen der Bodengeschütze.
    #[arg(long)]
    guns: Option<f32>,
    /// Schildstufe.
    #[arg(long)]
    shield: Option<u8>,
    /// Bunkerstufe.
    #[arg(long)]
    bunkers: Option<u8>,
    /// Reguläre Verbände.
    #[arg(long)]
    regulars: Option<f64>,
    /// Vorräte in Tagen.
    #[arg(long)]
    depot: Option<f64>,
    /// Haltung der Garnison.
    #[arg(long, value_enum, default_value_t = PostureArg::Vorwaerts)]
    posture: PostureArg,
    /// Die Geschütze schweigen.
    #[arg(long)]
    hold_fire: bool,

    // --- Belagerer ---
    /// Truppen an Bord als Vielfaches der Garnison, wenn --troops fehlt.
    #[arg(long, default_value_t = 1.5)]
    force_ratio: f64,
    /// Feuerkraft im Orbit (Standard: Startflotten mit dem Laderaum für die Truppen; eine
    /// Startflotte hat 150).
    #[arg(long)]
    firepower: Option<f64>,
    /// Standfestigkeit der Flotte: Gewicht je Klasse mal Rumpf, Schild und Panzerung
    /// (Standard: wie bei Startflotten gleicher Feuerkraft).
    #[arg(long)]
    toughness: Option<f64>,
    /// Truppen an Bord in Verbänden (eine Startflotte trägt 17,5).
    #[arg(long)]
    troops: Option<f64>,
    /// Nachschub an Bord in Tagen für die eingeschifften Truppen.
    #[arg(long, default_value_t = 90.0)]
    supply_days: f64,
    /// Füllung der Magazine, 0..1.
    #[arg(long, default_value_t = 1.0)]
    ammo: f64,
    /// Füllung der Tanks, 0..1.
    #[arg(long, default_value_t = 1.0)]
    fuel: f64,
    /// Ohne Orbitalhoheit rechnen.
    #[arg(long)]
    no_control: bool,

    // --- Technik (Aufschläge, 0.1 = +10 %) ---
    /// Angriff am Boden des Belagerers.
    #[arg(long, default_value_t = 0.0)]
    ground_attack: f64,
    /// Verteidigung am Boden des Verteidigers.
    #[arg(long, default_value_t = 0.0)]
    ground_defense: f64,
    /// Waffenschaden des Belagerers.
    #[arg(long, default_value_t = 0.0)]
    weapon_damage: f64,
    /// Waffenschaden des Verteidigers (Stationen, Geschütze).
    #[arg(long, default_value_t = 0.0)]
    defender_weapons: f64,
    /// Schilde des Verteidigers.
    #[arg(long, default_value_t = 0.0)]
    defender_shields: f64,
    /// Der Belagerer beherrscht „Orbitale Feuerleitung".
    #[arg(long)]
    support_tech: bool,
    /// Der Belagerer beherrscht „Tunnelkriegführung".
    #[arg(long)]
    tunnel_attacker: bool,
    /// Der Verteidiger beherrscht „Tunnelkriegführung".
    #[arg(long)]
    tunnel_defender: bool,

    // --- Stehende Befehle des Verlaufs ---
    /// Feuerstufe.
    #[arg(long, value_enum, default_value_t = FireArg::Keins)]
    fire: FireArg,
    /// Nach so vielen Tagen fällt die Feuerstufe auf Stützfeuer zurück.
    #[arg(long)]
    fire_days: Option<u32>,
    /// Sturm statt Halten.
    #[arg(long)]
    assault: bool,
    /// Schwerpunkt: Nummer der Zone (siehe Kopf der Ausgabe).
    #[arg(long, default_value_t = 0)]
    focus: u8,
    /// Tag, vor dem alle Truppen landen (1 = sofort); ohne Angabe bleiben sie an Bord.
    #[arg(long)]
    land_day: Option<u32>,
    /// Länge des Verlaufs in Tagen.
    #[arg(long, default_value_t = 120)]
    days: u32,
    /// Nur jeden so vielten Tag ausgeben (Tage mit Ereignissen immer).
    #[arg(long, default_value_t = 1)]
    every: u32,
}

/// Feuerkraft, Standfestigkeit und Laderaum der Startflotte (ein Kreuzer, zwei Fregatten,
/// drei Korvetten): der Maßstab für die Flotten der Bezugswelten.
const FLEET_FIREPOWER: f64 = 150.0;
const FLEET_TOUGHNESS: f64 = 1084.0;
const FLEET_BERTHS: f64 = 17.5;
/// Buchwert der Betriebe je Mio. Einwohner: Mittel über besiedelte Welten in Partien
/// (einzelne Welten liegen zwischen 20 und 80).
const FIRM_VALUE_PER_MILLION: f64 = 38.0;

/// Eine Bezugswelt samt der Flotte, die sie angreift.
struct Scenario {
    name: &'static str,
    population: f64,
    stations: f32,
    guns: f32,
    shield: u8,
    bunkers: u8,
    regulars: f64,
    depot: f64,
}

fn scenario(preset: PresetArg, balance: &SiegeBalance) -> Scenario {
    match preset {
        PresetArg::Kolonie => Scenario {
            name: "Kolonie",
            population: 50.0,
            stations: 0.0,
            guns: 0.0,
            shield: 0,
            bunkers: 0,
            regulars: 0.0,
            depot: 60.0,
        },
        PresetArg::Welt => Scenario {
            name: "Welt",
            population: 500.0,
            stations: 0.0,
            guns: 0.0,
            shield: 0,
            bunkers: 0,
            regulars: 0.0,
            depot: 60.0,
        },
        PresetArg::Heimatwelt => Scenario {
            name: "Heimatwelt",
            population: 4000.0,
            stations: 0.0,
            guns: balance.home_start.guns as f32,
            shield: 0,
            bunkers: 0,
            regulars: balance.home_start.regulars,
            depot: balance.home_start.depot_days as f64,
        },
    }
}

/// Baut die Sicht auf die Belagerung aus Bezugswelt und Optionen. Tag 1 ist der erste
/// gerechnete Tag.
fn build_view(args: &SiegeArgs, preset: PresetArg, balance: &SiegeBalance) -> SiegeView {
    let base = scenario(preset, balance);
    let population = args.population.unwrap_or(base.population).max(0.0);
    let mut defense = PlanetDefense {
        stations: args.stations.unwrap_or(base.stations).max(0.0),
        guns: args.guns.unwrap_or(base.guns).max(0.0),
        shield: args.shield.unwrap_or(base.shield),
        bunkers: args.bunkers.unwrap_or(base.bunkers),
        regulars: args.regulars.unwrap_or(base.regulars).max(0.0),
        depot: args.depot.unwrap_or(base.depot).max(0.0),
        posture: match args.posture {
            PostureArg::Vorwaerts => Posture::Forward,
            PostureArg::Tiefe => Posture::Deep,
        },
        hold_fire: args.hold_fire,
        ..Default::default()
    };
    // Der Schild beginnt geladen.
    defense.charge =
        (defense.shield as f64 * balance.shield_cap * (1.0 + args.defender_shields)) as f32;
    let facts = PlanetFacts {
        day: 1,
        population,
        cohesion: args.cohesion,
        essentials_met: args.essentials,
        forage: args.forage,
    };
    let (attacker, defender) = (PolityId(0), PolityId(1));
    let stake = args.stake.clamp(0.0, 1.0);
    let stakes = BTreeMap::from([(attacker, stake), (defender, 1.0 - stake)]);
    let firm_value = args
        .firm_value
        .unwrap_or(FIRM_VALUE_PER_MILLION * population)
        .max(0.0);
    let prize = firm_value + balance.prize_per_million * population;
    let mut siege = form(
        attacker,
        defender,
        &mut defense,
        &facts,
        stakes,
        prize,
        balance,
    );
    siege.orders.focus = args.focus;
    // Ohne Angabe: Truppen im festen Verhältnis zur Garnison, dazu so viele Startflotten,
    // wie sie Laderaum brauchen.
    let troops = args
        .troops
        .unwrap_or(args.force_ratio * siege.garrison_start)
        .max(0.0);
    let firepower = args
        .firepower
        .unwrap_or(troops / FLEET_BERTHS * FLEET_FIREPOWER)
        .max(0.0);
    let mut attacker_gates = [false; 4];
    attacker_gates[Gate::Support as usize] = args.support_tech;
    attacker_gates[Gate::Tunnel as usize] = args.tunnel_attacker;
    let mut defender_gates = [false; 4];
    defender_gates[Gate::Tunnel as usize] = args.tunnel_defender;
    let orbit = OrbitView {
        control: !args.no_control,
        firepower,
        ammo: args.ammo.clamp(0.0, 1.0),
        embarked: troops,
        att: TechView {
            ground_attack: args.ground_attack,
            weapon_damage: args.weapon_damage,
            gates: attacker_gates,
            ..Default::default()
        },
        def: TechView {
            ground_defense: args.ground_defense,
            weapon_damage: args.defender_weapons,
            shields: args.defender_shields,
            gates: defender_gates,
            ..Default::default()
        },
    };
    SiegeView {
        planet: PlanetId(0),
        siege,
        defense,
        facts,
        orbit,
        toughness: args
            .toughness
            .unwrap_or(firepower / FLEET_FIREPOWER * FLEET_TOUGHNESS)
            .max(0.0),
        fuel: args.fuel.clamp(0.0, 1.0),
        supply_aboard: troops * args.supply_days.max(0.0),
        firm_value,
    }
}

pub fn run(data: &GameData, args: SiegeArgs) -> ExitCode {
    let balance = &data.balance.siege;
    if args.reference {
        reference(&args, balance);
    } else {
        history(&args, balance);
    }
    ExitCode::SUCCESS
}

fn describe(name: &str, view: &SiegeView, balance: &SiegeBalance) {
    let defense = &view.defense;
    let siege = &view.siege;
    println!(
        "{name}: {:.0} Mio. Einwohner, Zusammenhalt {:.0}, Durchhaltewille {:.2}, Beute {:.0} Credits",
        view.facts.population,
        view.facts.cohesion,
        will(view.facts.cohesion, siege.stake(), balance),
        view.prize(balance)
    );
    println!(
        "  Verteidigung: Miliz {:.1} V, Reguläre {:.1} V (davon {:.1} im Untergrund), Vorräte {:.0} Tage, \
         Stationen {:.1}, Geschütze {:.1}, Schild {}, Bunker {}, Haltung {}",
        defense.militia,
        defense.regulars,
        siege.deep,
        defense.depot,
        defense.stations,
        defense.guns,
        defense.shield,
        defense.bunkers,
        defense.posture.label()
    );
    let zones: Vec<String> = siege
        .zones
        .iter()
        .enumerate()
        .map(|(index, zone)| {
            format!(
                "{index} {} {:.0} % ({:.1} V)",
                zone.kind.label(),
                100.0 * zone.share,
                zone.militia
            )
        })
        .collect();
    println!("  Zonen: {}", zones.join(", "));
    println!(
        "  Belagerer: Feuerkraft {:.0}, Standfestigkeit {:.0}, {:.1} V an Bord mit {:.0} Verbandstagen \
         Nachschub, Magazine {:.0} %, Tanks {:.0} %{}",
        view.orbit.firepower,
        view.toughness,
        view.orbit.embarked,
        view.supply_aboard,
        100.0 * view.orbit.ammo,
        100.0 * view.fuel,
        if view.orbit.control {
            ""
        } else {
            ", keine Orbitalhoheit"
        }
    );
    println!(
        "  Besatzungstruppe für die Übernahme: {:.1} V. Längste Einschließung laut Schranke: {:.0} Tage \
         (Frist {}).",
        occupation_force(view.facts.population, balance),
        siege_day_bound(balance),
        balance.max_siege_days
    );
}

fn forecast_row(course: Course, forecast: &Forecast) {
    let fate = match forecast.outcome {
        Some(outcome) => outcome.label(),
        None => "hält",
    };
    println!(
        "  {:<16} {:<13} {:>5} {:>14.1} {:>18.1} {:>7.0} % {:>11.0} %   {}",
        course.label(),
        fate,
        forecast.days,
        forecast.own_losses,
        forecast.enemy_losses,
        100.0 * forecast.hull_loss,
        100.0 * (1.0 - forecast.prize_loss),
        forecast.limited_by.map_or("–", |limit| limit.label())
    );
}

fn forecasts(view: &SiegeView, balance: &SiegeBalance) {
    println!(
        "  {:<16} {:<13} {:>5} {:>14} {:>18} {:>9} {:>13}   scheitert an",
        "Weg", "Ausgang", "Tage", "eig. Verluste", "Verl. Verteidiger", "Rumpf", "Beute bleibt"
    );
    for course in Course::ALL {
        forecast_row(course, &forecast(view, course, balance));
    }
}

/// Die drei Wege für die drei Bezugswelten: der veröffentlichte Tauschkurs.
fn reference(args: &SiegeArgs, balance: &SiegeBalance) {
    println!(
        "Drei Wege zum Fall eines Planeten. Verluste in Verbänden (V), Rumpf als Anteil der \
         schweren Schiffe, Beute als Anteil des heutigen Werts."
    );
    if args.troops.is_none() {
        println!(
            "Der Belagerer bringt das {:.2}-Fache der Garnison an Truppen und die Startflotten, \
             die sie tragen.",
            args.force_ratio
        );
    }
    for preset in [PresetArg::Kolonie, PresetArg::Welt, PresetArg::Heimatwelt] {
        let view = build_view(args, preset, balance);
        println!();
        describe(scenario(preset, balance).name, &view, balance);
        forecasts(&view, balance);
    }
}

/// Ein Verlauf mit stehenden Befehlen, Tag für Tag, danach die drei Wege vom selben
/// Ausgangspunkt.
fn history(args: &SiegeArgs, balance: &SiegeBalance) {
    let view = build_view(args, args.preset, balance);
    describe(scenario(args.preset, balance).name, &view, balance);
    let fire: Fire = args.fire.into();
    println!(
        "  Befehle: Feuer {}{}, {}, Schwerpunkt Zone {}{}",
        fire.label(),
        args.fire_days
            .map(|days| format!(" für {days} Tage, dann stützen"))
            .unwrap_or_default(),
        if args.assault { "Sturm" } else { "Halten" },
        args.focus,
        args.land_day
            .map(|day| format!(", Landung vor Tag {day}"))
            .unwrap_or_else(|| ", keine Landung".into())
    );
    println!();
    println!(
        "{:>4} {:<13} {:<10} {:>7} {:>8} {:>6} {:>7} {:>6} {:>7} {:>6} {:>5} {:>6} {:>8} {:>7} {:>7} {:>7} {:>6} {:>5}  Ereignisse",
        "Tag",
        "Phase",
        "Zone",
        "Truppe",
        "Nachsch.",
        "Miliz",
        "Reserve",
        "Tiefe",
        "Vorräte",
        "Schild",
        "Stat.",
        "Gesch.",
        "Bev. Mio",
        "Verl. A",
        "Verl. V",
        "aufgel.",
        "Rumpf",
        "Mun."
    );
    let mut projection = Projection::new(&view, balance);
    let every = args.every.max(1);
    for day in 1..=args.days {
        projection.siege.orders.fire = match args.fire_days {
            Some(days) if day > days && fire > Fire::Support => Fire::Support,
            _ => fire,
        };
        projection.siege.orders.assault = args.assault;
        let mut events: Vec<String> = Vec::new();
        if args.land_day == Some(day) {
            let landing = projection.land(projection.orbit.embarked, balance);
            events.push(format!(
                "{:.1} V gelandet, {:.1} V abgefangen",
                landing.landed, landing.lost
            ));
        }
        projection.step(balance);
        let report = &projection.siege.report;
        if report.sortie {
            events.push("Ausfall".into());
        }
        if let Some(zone) = report.fell {
            events.push(format!(
                "{} fällt",
                projection.siege.zones[zone as usize].kind.label()
            ));
        }
        if report.repelled {
            events.push("Landung abgeschlagen".into());
        }
        events.extend(
            report
                .blocked
                .iter()
                .map(|reason| reason.label().to_string()),
        );
        if let Some(outcome) = projection.verdict {
            events.push(format!("Planet {}", outcome.label()));
        }
        let quiet = report.fell.is_none()
            && !report.repelled
            && !report.sortie
            && projection.verdict.is_none()
            && args.land_day != Some(day);
        if quiet && day % every != 0 && day != 1 && day != args.days {
            continue;
        }
        let zone = if report.underground {
            "Untergrund".to_string()
        } else {
            report
                .zone
                .map(|zone| {
                    projection.siege.zones[zone as usize]
                        .kind
                        .label()
                        .to_string()
                })
                .unwrap_or_else(|| "–".into())
        };
        println!(
            "{:>4} {:<13} {:<10} {:>7.1} {:>8.0} {:>6.1} {:>7.1} {:>6.1} {:>7.1} {:>6.0} {:>5.2} {:>6.2} {:>8.1} {:>7.2} {:>7.2} {:>7.2} {:>4.0} % {:>3.0} %  {}",
            day,
            projection.siege.phase(&projection.defense).label(),
            zone,
            projection.siege.army,
            projection.siege.army_supply,
            projection.siege.zone_militia(),
            projection.siege.reserve(&projection.defense),
            projection.siege.deep,
            projection.defense.depot,
            projection.defense.charge,
            projection.defense.stations,
            projection.defense.guns,
            projection.facts.population,
            report.attacker_lost + report.attacker_stranded,
            report.defender_lost + report.defender_bombed,
            report.defender_dissolved,
            100.0 * projection.hull_loss(),
            100.0 * projection.orbit.ammo,
            events.join("; ")
        );
        if projection.verdict.is_some() {
            break;
        }
    }
    println!();
    match projection.verdict {
        Some(outcome) => println!(
            "Planet {} nach {} Tagen. Eigene Verluste {:.1} V, Verluste der Verteidiger {:.1} V, \
             Beute bleibt zu {:.0} %, Rumpf der schweren Schiffe −{:.0} %.",
            outcome.label(),
            projection.days,
            projection.own_losses,
            projection.enemy_losses,
            100.0 * (1.0 - projection.prize_loss(balance)),
            100.0 * projection.hull_loss()
        ),
        None => println!(
            "Planet hält nach {} Tagen. Eigene Verluste {:.1} V, Verluste der Verteidiger {:.1} V, \
             Beute bleibt zu {:.0} %, Rumpf der schweren Schiffe −{:.0} %.",
            projection.days,
            projection.own_losses,
            projection.enemy_losses,
            100.0 * (1.0 - projection.prize_loss(balance)),
            100.0 * projection.hull_loss()
        ),
    }
    println!("\nDie drei Wege vom selben Ausgangspunkt:");
    forecasts(&view, balance);
}

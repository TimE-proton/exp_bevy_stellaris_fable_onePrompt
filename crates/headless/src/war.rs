//! Kriege nach Drehbuch auf der Kommandozeile: die Angaben `--war` und `--peace` und die
//! Ausgabe dessen, was die Gefechte hinterlassen haben. Gespielt wird das Drehbuch von
//! `sim::harness::WarScript`.

use sim::clock::{DAYS_PER_YEAR, date_of_tick};
use sim::command::CommandOutcome;
use sim::data::ShipClass;
use sim::fleet::{BattleReport, Battles};
use sim::harness::{Headless, ScriptedAct, ScriptedEvent, WarScript};
use sim::ids::PolityId;

/// Liest `JAHR:A:B`, z. B. `12:0:3`, als Ereignis zu Beginn des Jahres.
fn parse_event(text: &str, act: ScriptedAct) -> Result<ScriptedEvent, String> {
    let parts: Vec<&str> = text.split(':').map(str::trim).collect();
    let [year, a, b] = parts.as_slice() else {
        return Err("erwartet wird JAHR:A:B, z. B. 12:0:3".into());
    };
    let number = |text: &str, what: &str| {
        text.parse::<u32>()
            .map_err(|_| format!("{what} „{text}“ ist keine Zahl"))
    };
    let event = ScriptedEvent {
        day: number(year, "Jahr")? as u64 * DAYS_PER_YEAR,
        act,
        a: PolityId(number(a, "Gemeinschaft")?),
        b: PolityId(number(b, "Gemeinschaft")?),
    };
    if event.a == event.b {
        return Err("A und B müssen verschiedene Gemeinschaften sein".into());
    }
    Ok(event)
}

pub fn parse_war(text: &str) -> Result<ScriptedEvent, String> {
    parse_event(text, ScriptedAct::War)
}

pub fn parse_peace(text: &str) -> Result<ScriptedEvent, String> {
    parse_event(text, ScriptedAct::Peace)
}

/// Das Drehbuch aus den Angaben `--war` und `--peace`.
pub fn script(wars: &[ScriptedEvent], peace: &[ScriptedEvent]) -> WarScript {
    WarScript::new(wars.iter().chain(peace).copied().collect())
}

fn counts(values: &[u32; 6]) -> String {
    let parts: Vec<String> = ShipClass::ALL
        .iter()
        .rev()
        .filter(|class| values[class.index()] > 0)
        .map(|class| {
            let count = values[class.index()];
            format!(
                "{count} {}",
                if count == 1 {
                    class.label()
                } else {
                    class.plural()
                }
            )
        })
        .collect();
    if parts.is_empty() {
        "nichts".into()
    } else {
        parts.join(", ")
    }
}

fn describe(sim: &Headless, report: &BattleReport) -> String {
    let mut text = format!(
        "  {:<4} {:<14} {} bis {} ({} Tage){}",
        report.id.to_string(),
        sim.galaxy().system(report.system).name,
        date_of_tick(report.started),
        date_of_tick(report.ended),
        report.days(),
        if report.timed_out {
            ", ohne Entscheidung"
        } else {
            ""
        }
    );
    for side in &report.sides {
        let share = if side.initial_value > 0.0 {
            100.0 * side.lost_value / side.initial_value
        } else {
            0.0
        };
        text += &format!(
            "\n         {:<26} {} von {} Schiffen verloren ({:.0} % des Bauwerts: {}){}{}",
            side.fleet_name,
            side.lost.iter().sum::<u32>(),
            side.initial.iter().sum::<u32>(),
            share,
            counts(&side.lost),
            if side.held_field {
                ", hält das Feld"
            } else {
                ", räumt das Feld"
            },
            if side.retreated { " (Rückzug)" } else { "" }
        );
    }
    text
}

/// Was die Kriege des Drehbuchs hinterlassen haben: abgelehnte Befehle, laufende Gefechte,
/// die Bilanzen der jüngsten und die Trümmerfelder.
pub fn print_aftermath(sim: &Headless) {
    for entry in &sim.command_log().entries {
        if let CommandOutcome::Rejected(reason) = &entry.outcome {
            println!(
                "Abgelehnt ({}, {}): {:?}: {reason}",
                date_of_tick(entry.tick),
                sim.polities().get(entry.polity).name,
                entry.command
            );
        }
    }
    let battles = sim.world().resource::<Battles>();
    println!(
        "Gefechte: {} begonnen, {} laufen noch, {} Schiffe verloren.",
        battles.next_id,
        battles.active.len(),
        battles.total_ships_lost()
    );
    for battle in &battles.active {
        println!(
            "  {:<4} {:<14} läuft seit {:.0} Tagen",
            battle.id.to_string(),
            sim.galaxy().system(battle.system).name,
            battle.tick as f64 / sim::TICKS_PER_DAY as f64
        );
    }
    for report in &battles.reports {
        println!("{}", describe(sim, report));
    }
    for debris in &battles.debris {
        println!(
            "  Trümmer bei {}: Bauwert {:.0} Credits",
            sim.galaxy().system(debris.system).name,
            debris.amount
        );
    }
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_are_parsed_and_checked() {
        assert_eq!(
            parse_war("12:0:3"),
            Ok(ScriptedEvent {
                day: 12 * DAYS_PER_YEAR,
                act: ScriptedAct::War,
                a: PolityId(0),
                b: PolityId(3)
            })
        );
        assert_eq!(parse_peace(" 2 : 1 : 0 ").unwrap().act, ScriptedAct::Peace);
        assert!(parse_war("12:0").is_err());
        assert!(parse_war("zwölf:0:3").is_err());
        assert!(parse_war("12:2:2").is_err());
        assert!(script(&[], &[]).is_empty());
        assert!(!script(&[parse_war("1:0:1").unwrap()], &[]).is_empty());
    }
}

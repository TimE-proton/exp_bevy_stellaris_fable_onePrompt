//! Was die Kriege eines Drehbuchs hinterlassen haben, auf der Kommandozeile: abgelehnte
//! Befehle, laufende Gefechte, Bilanzen und Trümmerfelder. Die Angaben des Drehbuchs
//! selbst liest `crate::script`.

use sim::clock::date_of_tick;
use sim::command::CommandOutcome;
use sim::data::ShipClass;
use sim::fleet::{BattleOutcome, BattleReport, Battles};
use sim::harness::Headless;
use sim::ids::PolityId;

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
        "  {:<4} {:<14} {} bis {} ({} Tage), {}",
        report.id.to_string(),
        sim.galaxy().system(report.system).name,
        date_of_tick(report.started),
        date_of_tick(report.ended),
        report.days(),
        report.outcome.label()
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
            if side.stalemate {
                " (im Patt gelöst)"
            } else if side.retreated {
                " (Rückzug)"
            } else {
                ""
            }
        );
    }
    text
}

/// Name einer Gemeinschaft für die Ausgabe. Eine Nummer, die es in der Partie nicht gibt
/// (ein Tippfehler im Drehbuch, ein Freistaat, der nie entstand), bleibt als Nummer stehen:
/// Die Ausgabe soll gerade solche Angaben zeigen können.
pub fn polity_name(sim: &Headless, polity: PolityId) -> String {
    sim.polities().list.get(polity.index()).map_or_else(
        || format!("Gemeinschaft {} (unbekannt)", polity.0),
        |found| found.name.clone(),
    )
}

/// Befehle der Partie, die abgelehnt wurden, je einer in einer Zeile.
pub fn rejected_commands(sim: &Headless) -> Vec<String> {
    sim.command_log()
        .entries
        .iter()
        .filter_map(|entry| match &entry.outcome {
            CommandOutcome::Rejected(reason) => Some(format!(
                "Abgelehnt ({}, {}): {:?}: {reason}",
                date_of_tick(entry.tick),
                polity_name(sim, entry.polity),
                entry.command
            )),
            CommandOutcome::Applied => None,
        })
        .collect()
}

/// Was die Kriege des Drehbuchs hinterlassen haben: abgelehnte Befehle, laufende Gefechte,
/// die Bilanzen der jüngsten und die Trümmerfelder.
pub fn print_aftermath(sim: &Headless) {
    for line in rejected_commands(sim) {
        println!("{line}");
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
    // Wie die Gefechte ausgingen, soweit ihre Bilanzen noch aufbewahrt sind.
    let endings: Vec<String> = BattleOutcome::ALL
        .iter()
        .filter_map(|outcome| {
            let count = battles
                .reports
                .iter()
                .filter(|report| report.outcome == *outcome)
                .count();
            (count > 0).then(|| format!("{count} {}", outcome.label()))
        })
        .collect();
    if !endings.is_empty() {
        println!("  Ausgang: {}.", endings.join(", "));
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
    use sim::harness::{ScriptedEvent, WarScript};

    #[test]
    fn a_script_naming_a_community_that_does_not_exist_is_reported_not_fatal() {
        let data = sim::GameData::load_workspace().unwrap();
        // Sechs Gemeinschaften, Nummern 0 bis 5: Die 6 ist der klassische Zählfehler, die 99
        // ein Tippfehler. Beides einmal als Urheber und einmal als Ziel.
        // Das Drehbuch führt die Flotten selbst: Die militärische Führung der KI ruht.
        let script = WarScript::new(vec![
            ScriptedEvent::rest_military(),
            ScriptedEvent::war(0, PolityId(6), PolityId(1)),
            ScriptedEvent::war(0, PolityId(0), PolityId(99)),
            ScriptedEvent::war(0, PolityId(2), PolityId(3)),
            ScriptedEvent::peace(0, PolityId(99), PolityId(0)),
        ]);
        let (sim, violation) = sim::harness::run_scripted(&data, 3, 40, &script);
        assert_eq!(violation, None);
        assert_eq!(polity_name(&sim, PolityId(2)), sim.polities().list[2].name);
        assert_eq!(polity_name(&sim, PolityId(6)), "Gemeinschaft 6 (unbekannt)");
        let rejected = rejected_commands(&sim);
        assert_eq!(rejected.len(), 3, "{rejected:?}");
        assert!(
            rejected
                .iter()
                .any(|line| line.contains("Gemeinschaft 6 (unbekannt)"))
        );
        assert!(
            rejected
                .iter()
                .any(|line| line.contains("Gemeinschaft 99 (unbekannt)"))
        );
        assert!(
            rejected
                .iter()
                .all(|line| line.contains("Gemeinschaft besteht nicht mehr")),
            "{rejected:?}"
        );
        // Die ganze Ausgabe läuft durch, samt Zusammenfassung.
        print_aftermath(&sim);
        let summary = sim::harness::summarize(&sim);
        assert_eq!(summary.polities.len(), 6);
        // Wer einer Gemeinschaft den Krieg erklären wollte, die es nicht gibt, schickt
        // auch keine Flotte in den Blockadeauftrag: Der gültige Krieg läuft, sonst nichts.
        let fleets = sim.world().resource::<sim::fleet::Fleets>();
        assert_eq!(fleets.list[0].stance, sim::fleet::Stance::Guard);
        assert_eq!(fleets.list[2].stance, sim::fleet::Stance::Blockade);
        let diplomacy = sim.world().resource::<sim::diplomacy::Diplomacy>();
        assert!(diplomacy.at_war(PolityId(2), PolityId(3)));
        assert_eq!(diplomacy.wars.len(), 1);
    }
}

//! Diplomatie und Enden im Sweep: Tabellen je Lauf und je Grundhaltung, CSV-Spalten.

use sim::diplomacy::{Archetype, TreatyKind};
use sim::endgame::Ending;
use sim::harness::RunSummary;

/// Jahre, zu denen die Zahl der souveränen Verbünde gezeigt wird.
const MARK_YEARS: [u64; 6] = [25, 50, 75, 100, 150, 200];

fn median(values: &mut [u64]) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    Some(values[values.len() / 2])
}

/// Tabellen der Diplomatie und der Enden.
pub fn print(runs: &[RunSummary], years: u64) {
    println!();
    println!("Diplomatie je Lauf (Verträge H/N/F/I/V/U, Kriege begrenzt/Unterwerfung, Frieden ohne/Tribut/Unterwerfung/Diktat/Befehl):");
    print!("  Seed  Verträge          Kriege  Frieden        Brüche Künd. Embargo Vereinig. Schutz Unterw.  Treffer öff.   souverän");
    for year in MARK_YEARS.iter().filter(|year| **year <= years) {
        print!(" J{year:<3}");
    }
    println!("  still  Ende");
    for run in runs {
        let tally = &run.diplomacy;
        let treaties = tally
            .treaties
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join("/");
        let peaces = tally
            .peaces
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join("/");
        let percent = |share: Option<f64>| {
            share.map_or_else(|| "  –".to_string(), |share| format!("{:>3.0}", 100.0 * share))
        };
        print!(
            "  {:>4}  {:<17} {:>2}/{:<3}  {:<13} {:>6} {:>5} {:>7} {:>9} {:>6} {:>7}  {:>5} % {:>3} %  ",
            run.seed,
            treaties,
            tally.wars[0],
            tally.wars[1],
            peaces,
            tally.breaches,
            tally.denounced,
            tally.embargoes,
            tally.merges,
            tally.protectorates,
            tally.subjugations,
            percent(run.dossier_accuracy),
            percent(run.public_accuracy),
        );
        for year in MARK_YEARS.iter().filter(|year| **year <= years) {
            match run.sovereigns_at(*year) {
                Some(count) if run.outcome.is_none_or(|outcome| outcome.year() >= *year) => {
                    print!(" {count:>4}")
                }
                _ => print!("    –"),
            }
        }
        println!(
            "  {:>5}  {}",
            run.longest_still(),
            ending_short(run).unwrap_or_else(|| "offen".into())
        );
    }

    // Enden: Verteilung, Jahr, Überlebende.
    let ended: Vec<&RunSummary> = runs.iter().filter(|run| run.outcome.is_some()).collect();
    let count = |ending: Ending| {
        ended
            .iter()
            .filter(|run| run.outcome.is_some_and(|outcome| outcome.ending == ending))
            .count()
    };
    let mut years_ended: Vec<u64> = ended
        .iter()
        .filter_map(|run| run.outcome.map(|outcome| outcome.year()))
        .collect();
    let earliest = years_ended.iter().min().copied();
    let latest = years_ended.iter().max().copied();
    let middle = median(&mut years_ended);
    println!(
        "Enden: {} von {} Läufen beendet; Metamorphose {}, Auslöschung {} (davon ohne Überlebende {}); Jahr frühestens {}, Median {}, spätestens {}",
        ended.len(),
        runs.len(),
        count(Ending::Metamorphosis),
        count(Ending::Extinction),
        ended
            .iter()
            .filter(|run| run.outcome.is_some_and(|outcome| outcome.survivor.is_none()))
            .count(),
        earliest.map_or("–".into(), |year| year.to_string()),
        middle.map_or("–".into(), |year| year.to_string()),
        latest.map_or("–".into(), |year| year.to_string()),
    );
    // Überlebende nach Grundhaltung.
    let mut survivors = [0u32; 4];
    for run in &ended {
        if let Some(archetype) = run
            .outcome
            .and_then(|outcome| outcome.survivor)
            .and_then(|survivor| run.archetypes.get(survivor.index()).copied().flatten())
        {
            survivors[archetype.index()] += 1;
        }
    }
    println!(
        "Überlebende nach Grundhaltung: {}",
        Archetype::ALL
            .iter()
            .map(|archetype| format!("{} {}", archetype.label(), survivors[archetype.index()]))
            .collect::<Vec<_>>()
            .join(", ")
    );

    // Verhalten je wahrer Grundhaltung: Summen über alle Gemeinschaften der Läufe, je
    // Gemeinschaft gemittelt.
    let mut rows = [[0u32; 9]; 4];
    let mut members = [0u32; 4];
    for run in runs {
        for (index, archetype) in run.archetypes.iter().enumerate() {
            let Some(archetype) = archetype else {
                continue;
            };
            let actor = run.diplomacy.by_polity.get(index).copied().unwrap_or_default();
            let row = &mut rows[archetype.index()];
            members[archetype.index()] += 1;
            for (slot, value) in row.iter_mut().zip([
                actor.wars,
                actor.subjugation_wars,
                actor.treaties,
                actor.bind_proposals,
                actor.breaches,
                actor.denounced,
                actor.embargoes,
                actor.merges,
                actor.subjugated,
            ]) {
                *slot += value;
            }
        }
    }
    if members.iter().any(|count| *count > 0) {
        println!("Verhalten je wahrer Grundhaltung (je Gemeinschaft im Mittel):");
        println!(
            "  {:<17} {:>6} {:>7} {:>10} {:>8} {:>8} {:>7} {:>7} {:>8} {:>10} {:>9}",
            "Grundhaltung",
            "Zahl",
            "Kriege",
            "Unterw.-Z.",
            "Verträge",
            "Anträge",
            "Brüche",
            "Künd.",
            "Embargos",
            "Vereinig.",
            "unterwarf"
        );
        for archetype in Archetype::ALL {
            let n = members[archetype.index()].max(1) as f64;
            let row = rows[archetype.index()];
            let mean = |slot: usize| row[slot] as f64 / n;
            println!(
                "  {:<17} {:>6} {:>7.2} {:>10.2} {:>8.2} {:>8.2} {:>7.2} {:>7.2} {:>8.2} {:>10.2} {:>9.2}",
                archetype.label(),
                members[archetype.index()],
                mean(0),
                mean(1),
                mean(2),
                mean(3),
                mean(4),
                mean(5),
                mean(6),
                mean(7),
                mean(8),
            );
        }
    }
    let judged: u32 = runs.iter().map(|run| run.dossiers_judged).sum();
    let hits: f64 = runs
        .iter()
        .filter_map(|run| {
            run.dossier_accuracy
                .map(|share| share * run.dossiers_judged as f64)
        })
        .sum();
    if judged > 0 {
        println!(
            "Einschätzungen am Ende: {:.0} % von {judged} Dossiers nennen die wahre Grundhaltung zuerst (Zufall: 25 %)",
            100.0 * hits / judged as f64
        );
    }
}

fn ending_short(run: &RunSummary) -> Option<String> {
    let outcome = run.outcome?;
    let survivor = outcome
        .survivor
        .and_then(|survivor| {
            let name = run.polities.get(survivor.index())?.name.clone();
            let archetype = run
                .archetypes
                .get(survivor.index())
                .copied()
                .flatten()
                .map_or(String::new(), |archetype| format!(", {}", archetype.label()));
            Some(format!("{name}{archetype}"))
        })
        .unwrap_or_else(|| "niemand".into());
    Some(format!(
        "{} J{} ({survivor})",
        outcome.ending.label(),
        outcome.year()
    ))
}

/// Kopfzeile der zusätzlichen CSV-Spalten.
pub const CSV_HEADER: &str = ",archetype,polity_wars,polity_subjugation_wars,polity_treaties,polity_bind_proposals,polity_breaches,polity_denounced,polity_embargoes,polity_merges,polity_subjugated,treaties_trade,treaties_nonaggression,treaties_research,treaties_investment,treaties_defence,treaties_union,wars_limited,wars_subjugation,peace_white,peace_terms,peace_subjugation,peace_diktat,peace_imposed,breaches,denounced,embargoes,merges,protectorates,subjugations,dossier_accuracy,public_accuracy,survivor,sovereigns_25,sovereigns_50,sovereigns_100,longest_still";

/// Zusätzliche CSV-Spalten einer Gemeinschaft eines Laufs.
pub fn csv_fields(run: &RunSummary, index: usize) -> String {
    let tally = &run.diplomacy;
    let actor = tally.by_polity.get(index).copied().unwrap_or_default();
    let archetype = run
        .archetypes
        .get(index)
        .copied()
        .flatten()
        .map_or("", |archetype| archetype.label());
    let mut fields = vec![
        archetype.to_string(),
        actor.wars.to_string(),
        actor.subjugation_wars.to_string(),
        actor.treaties.to_string(),
        actor.bind_proposals.to_string(),
        actor.breaches.to_string(),
        actor.denounced.to_string(),
        actor.embargoes.to_string(),
        actor.merges.to_string(),
        actor.subjugated.to_string(),
    ];
    for kind in TreatyKind::ALL {
        fields.push(tally.treaties[kind.index()].to_string());
    }
    fields.extend(tally.wars.iter().map(u32::to_string));
    fields.extend(tally.peaces.iter().map(u32::to_string));
    fields.extend(
        [
            tally.breaches,
            tally.denounced,
            tally.embargoes,
            tally.merges,
            tally.protectorates,
            tally.subjugations,
        ]
        .iter()
        .map(u32::to_string),
    );
    let share = |value: Option<f64>| value.map_or(String::new(), |value| format!("{value:.3}"));
    fields.push(share(run.dossier_accuracy));
    fields.push(share(run.public_accuracy));
    fields.push(
        run.outcome
            .and_then(|outcome| outcome.survivor)
            .and_then(|survivor| run.polities.get(survivor.index()))
            .map_or(String::new(), |polity| polity.name.clone()),
    );
    for year in [25, 50, 100] {
        fields.push(
            run.sovereigns_at(year)
                .map_or(String::new(), |count| count.to_string()),
        );
    }
    fields.push(run.longest_still().to_string());
    format!(",{}", fields.join(","))
}

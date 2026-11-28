//! Vertragsstufen: wann ein Vertrag geschlossen werden kann, Kündigung und Vertragsbruch.
//!
//! Die Stufen bauen aufeinander auf: Handel zuerst; Nichtangriff, Forschung und
//! Investition setzen Handel voraus; Verteidigung setzt Nichtangriff voraus; die Union
//! Investition und Verteidigung. Endet ein Vertrag, enden alle, die auf ihm aufbauen.

use super::relations::{TreatyKind, side};
use super::{Diplomacy, check_pair};
use crate::chronicle::{Chronicle, ChronicleKind};
use crate::clock::TICKS_PER_DAY;
use crate::ids::PolityId;
use crate::polity::Polities;

/// Kann zwischen den beiden der Vertrag geschlossen werden? Sonst der Grund.
pub fn check_treaty(
    diplomacy: &Diplomacy,
    polities: &Polities,
    a: PolityId,
    b: PolityId,
    kind: TreatyKind,
) -> Result<(), String> {
    check_pair(diplomacy, polities, a, b)?;
    if diplomacy.at_war(a, b) {
        return Err("im Krieg wird kein Vertrag geschlossen".into());
    }
    let relation = diplomacy.relation(a, b);
    if relation.is_some_and(|relation| relation.has(kind)) {
        return Err(format!("der Vertrag „{}“ besteht bereits", kind.label()));
    }
    if relation.is_some_and(|relation| relation.any_embargo()) {
        return Err("solange ein Embargo gilt, wird kein Vertrag geschlossen".into());
    }
    let missing: Vec<&str> = kind
        .requires()
        .iter()
        .filter(|need| !relation.is_some_and(|relation| relation.has(**need)))
        .map(|need| need.label())
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "„{}“ setzt {} voraus",
            kind.label(),
            missing
                .iter()
                .map(|label| format!("„{label}“"))
                .collect::<Vec<_>>()
                .join(" und ")
        ));
    }
    Ok(())
}

/// Schließt den Vertrag. Geprüft wird vorher mit [`check_treaty`].
pub fn sign(
    diplomacy: &mut Diplomacy,
    polities: &Polities,
    chronicle: &mut Chronicle,
    tick: u64,
    a: PolityId,
    b: PolityId,
    kind: TreatyKind,
) {
    let day = tick / TICKS_PER_DAY;
    let relation = diplomacy.relation_mut(a, b);
    // Ist eine Voraussetzung gekündigt, endet der neue Vertrag mit ihr.
    let ends_day = kind
        .requires()
        .iter()
        .filter_map(|need| relation.treaties[need.index()].and_then(|treaty| treaty.ends_day))
        .min();
    relation.treaties[kind.index()] = Some(super::Treaty {
        since_day: day,
        ends_day,
    });
    diplomacy.tally.treaties[kind.index()] += 1;
    diplomacy.tally.actor(a).treaties += 1;
    diplomacy.tally.actor(b).treaties += 1;
    chronicle.push(
        tick,
        ChronicleKind::Diplomacy,
        Some(a),
        None,
        format!(
            "{} und {} schließen einen Vertrag: {}.",
            polities.get(a).name,
            polities.get(b).name,
            kind.label()
        ),
    );
}

/// Verträge, die (mittelbar) auf `kind` aufbauen, `kind` eingeschlossen.
pub fn dependents(kind: TreatyKind) -> Vec<TreatyKind> {
    let mut found = vec![kind];
    let mut grew = true;
    while grew {
        grew = false;
        for other in TreatyKind::ALL {
            if !found.contains(&other) && other.requires().iter().any(|need| found.contains(need)) {
                found.push(other);
                grew = true;
            }
        }
    }
    found.sort();
    found
}

/// Kündigt einen Vertrag: Er und alle, die auf ihm aufbauen, enden nach der Frist.
pub fn denounce(
    diplomacy: &mut Diplomacy,
    polities: &Polities,
    chronicle: &mut Chronicle,
    tick: u64,
    actor: PolityId,
    other: PolityId,
    kind: TreatyKind,
    notice_days: u64,
) -> Result<(), String> {
    check_pair(diplomacy, polities, actor, other)?;
    let day = tick / TICKS_PER_DAY;
    let relation = diplomacy
        .relations
        .get_mut(&super::pair(actor, other))
        .filter(|relation| relation.has(kind))
        .ok_or_else(|| format!("kein Vertrag „{}“ mit dieser Gemeinschaft", kind.label()))?;
    if relation.treaties[kind.index()].is_some_and(|treaty| treaty.ends_day.is_some()) {
        return Err(format!("„{}“ ist bereits gekündigt", kind.label()));
    }
    let ends = day + notice_days;
    diplomacy.tally.denounced += 1;
    diplomacy.tally.actor(actor).denounced += 1;
    let Some(relation) = diplomacy.relations.get_mut(&super::pair(actor, other)) else {
        return Ok(());
    };
    for dependent in dependents(kind) {
        if let Some(treaty) = relation.treaties[dependent.index()].as_mut() {
            treaty.ends_day = Some(treaty.ends_day.map_or(ends, |old| old.min(ends)));
        }
    }
    chronicle.push(
        tick,
        ChronicleKind::Diplomacy,
        Some(actor),
        None,
        format!(
            "{} kündigt den Vertrag „{}“ mit {}.",
            polities.get(actor).name,
            kind.label(),
            polities.get(other).name
        ),
    );
    Ok(())
}

/// Täglich: Gekündigte Verträge enden an ihrem Tag.
pub fn expire(
    diplomacy: &mut Diplomacy,
    polities: &Polities,
    chronicle: &mut Chronicle,
    tick: u64,
) {
    let day = tick / TICKS_PER_DAY;
    for ((a, b), relation) in diplomacy.relations.iter_mut() {
        for kind in TreatyKind::ALL {
            let slot = &mut relation.treaties[kind.index()];
            if slot.is_some_and(|treaty| treaty.ends_day.is_some_and(|end| end <= day)) {
                *slot = None;
                chronicle.push(
                    tick,
                    ChronicleKind::Diplomacy,
                    Some(*a),
                    None,
                    format!(
                        "Der Vertrag „{}“ zwischen {} und {} ist ausgelaufen.",
                        kind.label(),
                        polities.get(*a).name,
                        polities.get(*b).name
                    ),
                );
            }
        }
    }
}

/// Vertragsbruch: Alle Verträge des Paares enden sofort, und das Opfer hegt vollen Groll.
/// Den Kursschock des Brechenden und die Erschütterung des Opfers trägt der Aufrufer
/// (`command`).
pub fn breach(
    diplomacy: &mut Diplomacy,
    polities: &Polities,
    chronicle: &mut Chronicle,
    tick: u64,
    breaker: PolityId,
    victim: PolityId,
    broken: &[TreatyKind],
) {
    let relation = diplomacy.relation_mut(breaker, victim);
    relation.treaties = [None; 6];
    relation.grievance[side(victim, breaker)] = 1.0;
    diplomacy.tally.breaches += 1;
    diplomacy.tally.actor(breaker).breaches += 1;
    let names = broken
        .iter()
        .map(|kind| format!("„{}“", kind.label()))
        .collect::<Vec<_>>()
        .join(", ");
    chronicle.push(
        tick,
        ChronicleKind::Diplomacy,
        Some(breaker),
        None,
        format!(
            "{} bricht den Vertrag {names} mit {}. Alle Verträge zwischen beiden sind erloschen.",
            polities.get(breaker).name,
            polities.get(victim).name
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dependents_follow_the_ladder() {
        assert_eq!(dependents(TreatyKind::Trade), TreatyKind::ALL.to_vec());
        assert_eq!(
            dependents(TreatyKind::NonAggression),
            vec![
                TreatyKind::NonAggression,
                TreatyKind::Defence,
                TreatyKind::Union
            ]
        );
        assert_eq!(dependents(TreatyKind::Research), vec![TreatyKind::Research]);
        assert_eq!(
            dependents(TreatyKind::Investment),
            vec![TreatyKind::Investment, TreatyKind::Union]
        );
    }
}

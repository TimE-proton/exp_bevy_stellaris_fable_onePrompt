//! Spieler nach Drehbuch: Eine Gemeinschaft, deren Diplomatie eine feste Strategie führt,
//! wie es ein menschlicher Spieler täte. Wirtschaft, Rüstung und Flotten führt weiter die
//! KI; die Strategie handelt nur über gewöhnliche Befehle und wird von den anderen wie
//! jede Gemeinschaft beobachtet.
//!
//! - [`PlayerStyle::Diplomat`] sucht die Vereinigung: Verträge Stufe um Stufe mit den
//!   kulturell nächsten Gemeinschaften, Anteilstausch unter Investitionsvertrag, die
//!   Vereinigung unter eigener Führung, Schutz für Kleine; nie Krieg, und Frieden, sobald
//!   ihr einer erklärt wird.
//! - [`PlayerStyle::Conqueror`] erobert: Vom Jahr [`CONQUEST_FROM_YEAR`] an erklärt sie
//!   dem schwächsten ihrer drei nächsten freien Nachbarn den Krieg mit dem Ziel der
//!   Unterwerfung, sobald ihre Flotte dessen geschätzte deutlich übertrifft, und fordert
//!   die Unterwerfung, sobald das Band es erlaubt, nach drei Jahren mit Vorsprung
//!   wenigstens Tribut (genommene Welten bleiben genommen). Verträge, Schutz und Vereinigung lehnt
//!   sie ab; Kriege, die andere ihr erklären, beendet sie gern ohne Bedingungen.

use super::Headless;
use crate::ai::AiState;
use crate::ai::diplomat::{may_propose, next_rung, swap_deal};
use crate::clock::{DAYS_PER_YEAR, TICKS_PER_DAY};
use crate::command::SimCommand;
use crate::diplomacy::sight::Ground;
use crate::diplomacy::{Deal, PeaceTerms, TreatyKind, WarAim, proposals, war};
use crate::ids::PolityId;

/// Strategie eines Spielers nach Drehbuch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerStyle {
    /// Vereinigung durch Verträge, Beteiligungen und Schutz.
    Diplomat,
    /// Eroberung durch Kriege mit dem Ziel der Unterwerfung.
    Conqueror,
}

/// Vom Beginn dieses Jahres an erklärt der Eroberer Kriege.
pub const CONQUEST_FROM_YEAR: u64 = 25;

/// Ab diesem Verhältnis der eigenen Flotte zur oberen Schätzung der fremden erklärt der
/// Eroberer den Krieg.
const CONQUEST_ODDS: f32 = 1.1;

/// Übernimmt die Diplomatie von `who`: Die KI tut dort fortan nichts mehr.
pub fn take_over(sim: &mut Headless, who: PolityId) {
    sim.world_mut()
        .resource_mut::<AiState>()
        .diplomacy_hold
        .insert(who);
}

/// Ein Monatszug des Spielers nach Drehbuch.
pub fn play(sim: &mut Headless, who: PolityId, style: PlayerStyle) {
    let commands = {
        let world = sim.world();
        let Some(ground) = Ground::of_world(world) else {
            return;
        };
        let alive = ground
            .polities
            .list
            .get(who.index())
            .is_some_and(|polity| polity.alive);
        if !alive || !ground.diplomacy.is_free(who) {
            return;
        }
        let mut commands = answers(&ground, who, style);
        match style {
            PlayerStyle::Diplomat => diplomat(&ground, who, &mut commands),
            PlayerStyle::Conqueror => conqueror(&ground, who, &mut commands),
        }
        commands
    };
    for command in commands {
        sim.issue(who, command);
    }
}

/// Antworten auf offene Vorschläge an den Spieler, drei Tage nach Eingang.
fn answers(ground: &Ground, who: PolityId, style: PlayerStyle) -> Vec<SimCommand> {
    let levers = &ground.data.diplomacy.levers;
    ground
        .diplomacy
        .proposals
        .iter()
        .filter(|proposal| proposal.to == who && ground.day >= proposal.made_day + 3)
        .map(|proposal| {
            let other = proposal.from;
            let accept = match (style, &proposal.deal) {
                (PlayerStyle::Diplomat, _) => true,
                (PlayerStyle::Conqueror, Deal::Peace(terms)) => {
                    let lead = war::lead_of(ground.diplomacy, ground.finance, levers, who, other);
                    // Kriege, die andere ihr erklärt haben, beendet sie gern ohne Bedingungen:
                    // Sie führt lieber ihren eigenen.
                    let theirs = ground
                        .diplomacy
                        .war_between(who, other)
                        .is_some_and(|known| known.attacker != who);
                    terms.yielder == Some(other)
                        || lead <= -30.0
                        || (theirs && terms.yielder != Some(who))
                }
                (PlayerStyle::Conqueror, _) => false,
            };
            SimCommand::AnswerProposal {
                proposal: proposal.id,
                accept,
            }
        })
        .collect()
}

/// Ein Vorschlag, wenn er zulässig und nichts Gleiches offen ist.
fn propose(ground: &Ground, who: PolityId, to: PolityId, deal: Deal, out: &mut Vec<SimCommand>) {
    if !may_propose(ground.diplomacy, who, to, &deal, ground.day) {
        return;
    }
    if proposals::check_deal(
        ground.data,
        ground.galaxy,
        ground.polities,
        ground.diplomacy,
        ground.finance,
        who,
        to,
        &deal,
    )
    .is_ok()
    {
        out.push(SimCommand::Propose { to, deal });
    }
}

/// Andere freie, lebende Gemeinschaften.
fn others(ground: &Ground, who: PolityId) -> Vec<PolityId> {
    ground
        .polities
        .alive()
        .map(|polity| polity.id)
        .filter(|id| *id != who && ground.diplomacy.is_free(*id))
        .collect()
}

fn diplomat(ground: &Ground, who: PolityId, out: &mut Vec<SimCommand>) {
    let levers = &ground.data.diplomacy.levers;
    let mut partners = others(ground, who);
    // Die kulturell Nächsten zuerst.
    partners.sort_by(|a, b| {
        ground
            .diplomacy
            .affinity(who, *b)
            .total_cmp(&ground.diplomacy.affinity(who, *a))
            .then(a.cmp(b))
    });
    for other in partners {
        if ground.diplomacy.at_war(who, other) {
            propose(ground, who, other, Deal::Peace(PeaceTerms::white()), out);
            continue;
        }
        let relation = ground.diplomacy.relation(who, other);
        if relation.is_some_and(|relation| {
            relation.has(TreatyKind::Union) && relation.merge_months >= levers.merge_hold_months
        }) {
            propose(ground, who, other, Deal::Merge { lead: who }, out);
            continue;
        }
        if ground.diplomacy.surveyed(other).power < levers.minor_share {
            propose(ground, who, other, Deal::Protectorate { ward: other }, out);
        }
        if let Some(kind) = next_rung(ground, who, other) {
            propose(ground, who, other, Deal::Treaty(kind), out);
        }
        if let Some(deal) = swap_deal(ground, who, other) {
            propose(ground, who, other, deal, out);
        }
    }
}

fn conqueror(ground: &Ground, who: PolityId, out: &mut Vec<SimCommand>) {
    let levers = &ground.data.diplomacy.levers;
    let day = ground.day;
    let rivals = others(ground, who);
    let at_war: Vec<PolityId> = rivals
        .iter()
        .copied()
        .filter(|other| ground.diplomacy.at_war(who, *other))
        .collect();
    for other in &at_war {
        let Some(known) = ground.diplomacy.war_between(who, *other) else {
            continue;
        };
        let lead = war::lead_of(ground.diplomacy, ground.finance, levers, who, *other);
        let band = war::terms_band(
            Some(known),
            lead,
            proposals::capital_taken(ground.diplomacy, ground.polities, who, *other),
            who,
            levers,
        );
        let months = war::months(known, day * TICKS_PER_DAY);
        let terms = if band.subjugation {
            Some(PeaceTerms {
                yielder: Some(*other),
                cede: Vec::new(),
                tribute_years: levers.terms_years / 2,
                subjugate: true,
            })
        } else if lead >= levers.terms_tribute + 10.0 && months >= 36 {
            // Genommene Welten bleiben genommen: Nach drei Jahren nimmt sie Tribut und
            // sucht sich das nächste Ziel.
            Some(PeaceTerms {
                yielder: Some(*other),
                cede: Vec::new(),
                tribute_years: levers.terms_years / 2,
                subjugate: false,
            })
        } else if lead <= -40.0 && months >= levers.peace_min_months {
            Some(PeaceTerms::white())
        } else {
            None
        };
        if let Some(terms) = terms {
            propose(ground, who, *other, Deal::Peace(terms), out);
        }
    }
    if !at_war.is_empty() || day < CONQUEST_FROM_YEAR * DAYS_PER_YEAR {
        return;
    }
    // Der nächste freie Nachbar, mit dem keine Waffenruhe gilt.
    let Some(home) = ground
        .polities
        .get(who)
        .capital
        .map(|capital| ground.galaxy.planet(capital).system)
    else {
        return;
    };
    let hops = ground.galaxy.hops_from(home);
    // Unter den drei nächsten Nachbarn der schwächste.
    let mut near: Vec<(u32, PolityId)> = rivals
        .iter()
        .copied()
        .filter(|other| ground.diplomacy.truce_until(who, *other, day).is_none())
        .filter_map(|other| {
            let seat = ground.polities.get(other).capital?;
            let distance = hops
                .get(ground.galaxy.planet(seat).system.index())
                .copied()?;
            Some((distance, other))
        })
        .collect();
    near.sort();
    let target = near
        .into_iter()
        .take(3)
        .map(|(_, other)| (ground.power_estimate(Some(who), other), other))
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
        .map(|(_, other)| other);
    let Some(target) = target else {
        return;
    };
    let own = ground.diplomacy.surveyed(who).fleet_value;
    let foe = ground.fleet_estimate(Some(who), target).hi;
    if own >= CONQUEST_ODDS * foe {
        out.push(SimCommand::DeclareWarWithAim {
            target,
            aim: WarAim::Subjugation,
        });
    }
}

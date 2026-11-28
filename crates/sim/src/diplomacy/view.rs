//! Lesemodell für die Oberfläche: was eine Gemeinschaft über eine andere wissen darf.
//!
//! [`relation_view`] ist eine reine Funktion über den öffentlichen Zustand und das
//! Dossier des Betrachters. Sie nimmt keine verborgene Haltung entgegen; was sie zeigt,
//! könnte jede Gemeinschaft über jede andere wissen. Fremde Flotten und Technik erscheinen
//! nur als Spannen ([`super::sight::Estimate`]).
//!
//! - [`RelationView`]: Stellung, Verträge samt Stufenleiter mit Gründen gesperrter
//!   Stufen, Krieg mit Ziel und Bilanz und den zulässigen Bedingungen, Waffenruhe,
//!   Embargo und Durchfahrt je Seite, Tribut, Groll, Spannung mit Gründen, kulturelle
//!   Nähe, Beteiligungen, Verflechtung und Fortschritt zur Vereinigung, offene Vorschläge
//!   in beide Richtungen, das Dossier ([`DossierView`]) und die Schätzungen.
//! - [`overview`]: dasselbe für alle anderen lebenden Gemeinschaften.
//! - [`inbox`]: offene Vorschläge an den Betrachter und von ihm, mit Fristen.

use super::evidence::Dossier;
use super::occasion::Archetype;
use super::proposals::{Deal, ProposalStatus, capital_taken};
use super::relations::{TensionReason, TreatyKind, WarAim, side};
use super::sight::{Estimate, Ground};
use super::{treaties, war};
use crate::ids::PolityId;

/// Zustand einer Vertragsstufe aus Sicht des Betrachters.
#[derive(Clone, Debug, PartialEq)]
pub enum RungState {
    /// Besteht seit diesem Tag; gekündigt bis zum zweiten.
    Active {
        since_day: u64,
        ends_day: Option<u64>,
    },
    /// Kann vorgeschlagen werden.
    Available,
    /// Gesperrt, mit Grund.
    Locked(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct RungView {
    pub kind: TreatyKind,
    pub label: &'static str,
    pub state: RungState,
}

/// Ein laufender Krieg aus Sicht des Betrachters.
#[derive(Clone, Debug, PartialEq)]
pub struct WarView {
    /// Angreifer; `None`, wenn die Umstände unbekannt sind.
    pub attacker: Option<PolityId>,
    pub aim: WarAim,
    pub since_day: u64,
    pub months: u32,
    /// Bilanz aus Sicht des Betrachters, −100..100.
    pub score: f32,
    /// Genommene Bevölkerung (Mio.) und verlorener Flottenwert (Credits), je eigen und
    /// fremd.
    pub taken_by_us: f64,
    pub taken_by_them: f64,
    pub lost_by_us: f64,
    pub lost_by_them: f64,
    pub our_weariness: f64,
    pub their_weariness: f64,
    /// Was der Betrachter fordern dürfte: Tribut und Abtretungen, höchster abtretbarer
    /// Anteil der Bevölkerung des Gegners, Unterwerfung.
    pub may_demand_concessions: bool,
    pub max_cede_share: f64,
    pub may_demand_subjugation: bool,
    /// Monate bis zum Diktat.
    pub months_to_diktat: u32,
}

/// Ein offener oder erledigter Vorschlag.
#[derive(Clone, Debug, PartialEq)]
pub struct ProposalView {
    pub id: u32,
    pub from: PolityId,
    pub to: PolityId,
    pub deal: Deal,
    pub label: String,
    pub made_day: u64,
    pub expires_day: u64,
    /// An den Betrachter gerichtet.
    pub incoming: bool,
}

/// Eine Grundhaltung im Dossier: Einschätzung mit Spanne.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArchetypeGuess {
    pub archetype: Archetype,
    pub label: &'static str,
    pub belief: f32,
    pub lo: f32,
    pub hi: f32,
}

/// Ein Indiz in Worten.
#[derive(Clone, Debug, PartialEq)]
pub struct ClueView {
    pub day: u64,
    pub kind: &'static str,
    pub source: String,
    pub toward: PolityId,
    /// Die Grundhaltung, für die es am stärksten sprach, und um wie viel es sie hob.
    pub points_to: Archetype,
    pub points_to_label: &'static str,
    pub shift: f32,
    pub delta: [f32; 4],
}

/// Das Dossier des Betrachters über die andere Gemeinschaft.
#[derive(Clone, Debug, PartialEq)]
pub struct DossierView {
    /// Alle vier Grundhaltungen, die wahrscheinlichste zuerst.
    pub guesses: Vec<ArchetypeGuess>,
    /// Gesammeltes Gewicht der Indizien und eine Einordnung in Worten.
    pub weight: f32,
    pub clarity: &'static str,
    /// Vertrauen, das die Einschätzung rechtfertigt, 0..1.
    pub trust: f32,
    /// Jüngste Indizien, die neuesten zuerst.
    pub clues: Vec<ClueView>,
    /// Die jüngsten Taten passten nicht mehr zur Einschätzung: seit diesem Tag.
    pub overturned_day: Option<u64>,
}

impl DossierView {
    pub fn of(dossier: &Dossier) -> Self {
        let belief = dossier.belief();
        let band = dossier.band();
        let mut guesses: Vec<ArchetypeGuess> = Archetype::ALL
            .iter()
            .map(|archetype| {
                let value = belief[archetype.index()];
                ArchetypeGuess {
                    archetype: *archetype,
                    label: archetype.label(),
                    belief: value,
                    lo: (value - band).max(0.0),
                    hi: (value + band).min(1.0),
                }
            })
            .collect();
        guesses.sort_by(|a, b| {
            b.belief
                .total_cmp(&a.belief)
                .then(a.archetype.cmp(&b.archetype))
        });
        let clarity = match dossier.weight {
            w if w < 1.0 => "kaum Anhaltspunkte",
            w if w < 4.0 => "erste Anhaltspunkte",
            w if w < 10.0 => "ein Bild zeichnet sich ab",
            _ => "gut belegt",
        };
        let clues = dossier
            .clues
            .iter()
            .rev()
            .map(|clue| {
                let points = clue.points_to();
                ClueView {
                    day: clue.day,
                    kind: clue.kind.label(),
                    source: clue.source.clone(),
                    toward: clue.toward,
                    points_to: points,
                    points_to_label: points.label(),
                    shift: clue.delta[points.index()],
                    delta: clue.delta,
                }
            })
            .collect();
        DossierView {
            guesses,
            weight: dossier.weight,
            clarity,
            trust: 0.0,
            clues,
            overturned_day: dossier.break_day,
        }
    }
}

/// Was `viewer` über `other` wissen darf.
#[derive(Clone, Debug, PartialEq)]
pub struct RelationView {
    pub viewer: PolityId,
    pub other: PolityId,
    pub other_name: String,
    /// Stellung: Oberhaupt der anderen; ob sie dem Betrachter untersteht; ob der
    /// Betrachter ihr untersteht.
    pub other_overlord: Option<PolityId>,
    pub other_is_our_ward: bool,
    pub we_are_their_ward: bool,
    /// Beide frei: nur dann gibt es eigene Verträge und Vorschläge.
    pub both_free: bool,
    pub ladder: Vec<RungView>,
    pub war: Option<WarView>,
    /// Erster Tag, an dem wieder Krieg erklärt werden kann.
    pub truce_until: Option<u64>,
    pub embargo_by_us: bool,
    pub embargo_by_them: bool,
    pub transit_by_us: bool,
    pub transit_by_them: bool,
    /// Laufender Tribut: wer zahlt, Anteil, bis wann.
    pub tribute: Option<(PolityId, f64, u64)>,
    pub our_grievance: f32,
    pub their_grievance: f32,
    pub tension: f32,
    /// Gründe der Spannung in Worten mit ihren Punkten.
    pub tension_reasons: Vec<(String, f32)>,
    pub affinity: f32,
    /// Unser Anteil am Marktwert ihrer Betriebe und ihr Anteil an unseren.
    pub our_stake: f32,
    pub their_stake: f32,
    pub our_dependence: f32,
    pub tie: f32,
    pub merge_tie: f32,
    pub merge_affinity: f32,
    /// Monate, die die Bedingungen der Vereinigung gehalten haben, und wie viele es braucht.
    pub merge_months: u32,
    pub merge_months_needed: u32,
    pub merge_possible: bool,
    pub proposals: Vec<ProposalView>,
    pub dossier: DossierView,
    /// Das öffentliche Bild des Betrachters, wie die andere es lesen kann.
    pub our_public_face: DossierView,
    pub fleet_estimate: Estimate,
    pub tech_estimate: Estimate,
    /// Einblick des Betrachters in die andere, 0..1.
    pub intel: f32,
}

fn proposal_view(proposal: &super::Proposal, viewer: PolityId, ground: &Ground) -> ProposalView {
    ProposalView {
        id: proposal.id,
        from: proposal.from,
        to: proposal.to,
        deal: proposal.deal.clone(),
        label: proposal.deal.label(ground.polities),
        made_day: proposal.made_day,
        expires_day: proposal.expires_day,
        incoming: proposal.to == viewer,
    }
}

/// Was `viewer` über `other` wissen darf. Rein; liest keine verborgene Haltung.
pub fn relation_view(ground: &Ground, viewer: PolityId, other: PolityId) -> RelationView {
    let diplomacy = ground.diplomacy;
    let levers = &ground.data.diplomacy.levers;
    let relation = diplomacy
        .relation(viewer, other)
        .cloned()
        .unwrap_or_default();
    let (us, them) = (side(viewer, other), side(other, viewer));
    let both_free = diplomacy.is_free(viewer) && diplomacy.is_free(other);
    let ladder = TreatyKind::ALL
        .iter()
        .map(|kind| {
            let state = match relation.treaty(*kind) {
                Some(treaty) => RungState::Active {
                    since_day: treaty.since_day,
                    ends_day: treaty.ends_day,
                },
                None => {
                    match treaties::check_treaty(diplomacy, ground.polities, viewer, other, *kind) {
                        Ok(()) => RungState::Available,
                        Err(reason) => RungState::Locked(reason),
                    }
                }
            };
            RungView {
                kind: *kind,
                label: kind.label(),
                state,
            }
        })
        .collect();
    let war = diplomacy.at_war(viewer, other).then(|| {
        let known = diplomacy.war_between(viewer, other);
        let score = war::lead_of(diplomacy, ground.finance, levers, viewer, other);
        let (head_us, head_them) = (diplomacy.head(viewer), diplomacy.head(other));
        let (hu, ht) = (side(head_us, head_them), side(head_them, head_us));
        let band = war::terms_band(
            known,
            score,
            capital_taken(diplomacy, ground.polities, viewer, other),
            head_us,
            levers,
        );
        let months = known.map_or(0, |war| {
            war::months(war, ground.day * crate::clock::TICKS_PER_DAY)
        });
        WarView {
            attacker: known.map(|war| war.attacker),
            aim: known.map_or(WarAim::Limited, |war| war.aim),
            since_day: known.map_or(0, |war| war.since_day),
            months,
            score,
            taken_by_us: known.map_or(0.0, |war| war.taken[hu]),
            taken_by_them: known.map_or(0.0, |war| war.taken[ht]),
            lost_by_us: known.map_or(0.0, |war| war.lost[hu]),
            lost_by_them: known.map_or(0.0, |war| war.lost[ht]),
            our_weariness: war::weariness(ground.finance, head_us),
            their_weariness: war::weariness(ground.finance, head_them),
            may_demand_concessions: band.concessions,
            max_cede_share: band.cede_share,
            may_demand_subjugation: band.subjugation,
            months_to_diktat: levers.war_max_months.saturating_sub(months),
        }
    });
    let reasons = relation
        .reasons
        .iter()
        .filter(|(reason, points)| *reason != TensionReason::Base || *points != 0.0)
        .map(|(reason, points)| (reason.label().to_string(), *points))
        .collect();
    let mut dossier = DossierView::of(&diplomacy.dossier(viewer, other));
    dossier.trust = diplomacy
        .dossier(viewer, other)
        .trust(&ground.data.diplomacy.archetypes);
    let mut face = DossierView::of(&diplomacy.public_face_of(viewer));
    face.trust = diplomacy
        .public_face_of(viewer)
        .trust(&ground.data.diplomacy.archetypes);
    RelationView {
        viewer,
        other,
        other_name: ground.polities.get(other).name.clone(),
        other_overlord: diplomacy.overlord_of(other),
        other_is_our_ward: diplomacy.overlord_of(other) == Some(viewer),
        we_are_their_ward: diplomacy.overlord_of(viewer) == Some(other),
        both_free,
        ladder,
        war,
        truce_until: diplomacy.truce_until(viewer, other, ground.day),
        embargo_by_us: relation.embargo[us],
        embargo_by_them: relation.embargo[them],
        transit_by_us: relation.transit[us],
        transit_by_them: relation.transit[them],
        tribute: relation
            .tribute
            .map(|tribute| (tribute.payer, tribute.share, tribute.until_day)),
        our_grievance: relation.grievance[us],
        their_grievance: relation.grievance[them],
        tension: relation.tension,
        tension_reasons: reasons,
        affinity: diplomacy.affinity(viewer, other),
        our_stake: relation.stake[us],
        their_stake: relation.stake[them],
        our_dependence: relation.dep[us],
        tie: relation.tie,
        merge_tie: levers.merge_tie,
        merge_affinity: diplomacy.merge_affinity_needed(levers, viewer, other),
        merge_months: relation.merge_months,
        merge_months_needed: levers.merge_hold_months,
        merge_possible: relation.has(TreatyKind::Union)
            && relation.merge_months >= levers.merge_hold_months,
        proposals: diplomacy
            .proposals_between(viewer, other)
            .map(|proposal| proposal_view(proposal, viewer, ground))
            .collect(),
        dossier,
        our_public_face: face,
        fleet_estimate: ground.fleet_estimate(Some(viewer), other),
        tech_estimate: ground.tech_estimate(Some(viewer), other),
        intel: ground.intel(Some(viewer), other),
    }
}

/// [`relation_view`] für alle anderen lebenden Gemeinschaften, nach Nummer.
pub fn overview(ground: &Ground, viewer: PolityId) -> Vec<RelationView> {
    ground
        .polities
        .alive()
        .map(|polity| polity.id)
        .filter(|id| *id != viewer)
        .map(|other| relation_view(ground, viewer, other))
        .collect()
}

/// Eingang und Ausgang des Betrachters: offene Vorschläge an ihn und von ihm, und die
/// zuletzt erledigten mit ihrem Ausgang (die neuesten zuerst).
pub fn inbox(
    ground: &Ground,
    viewer: PolityId,
) -> (Vec<ProposalView>, Vec<(ProposalView, ProposalStatus, u64)>) {
    let open = ground
        .diplomacy
        .proposals
        .iter()
        .filter(|proposal| proposal.from == viewer || proposal.to == viewer)
        .map(|proposal| proposal_view(proposal, viewer, ground))
        .collect();
    let done = ground
        .diplomacy
        .history
        .iter()
        .rev()
        .filter(|record| record.proposal.from == viewer || record.proposal.to == viewer)
        .map(|record| {
            (
                proposal_view(&record.proposal, viewer, ground),
                record.status,
                record.day,
            )
        })
        .collect();
    (open, done)
}

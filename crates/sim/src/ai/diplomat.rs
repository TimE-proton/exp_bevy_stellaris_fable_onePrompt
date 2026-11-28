//! Diplomatie der KI: Antworten auf Vorschläge, Annäherung, Frieden, Vereinigung.
//!
//! Die KI handelt wie der Spieler nur über Befehle (`SimCommand::AnswerProposal`,
//! `SimCommand::Propose`). Sie entscheidet mit ihrem gemischten, verborgenen Profil über
//! `choice_probs` und ihrem eigenen Bild der Lage (`Perspective::Own`): eigene Zahlen
//! genau, fremde Flotten und Technik geschätzt, die Haltung der anderen aus ihren
//! Dossiers. Sie fragt nie, ob die andere Seite ein Mensch ist.
//!
//! Was sie bisher tut:
//! - Sie beantwortet jeden Vorschlag zwischen `answer_days` Tagen nach Eingang.
//! - Einmal im Jahr wägt sie gegenüber jeder freien Gemeinschaft Zuschlagen, Binden und
//!   Abwarten ab (Monat (7i + 3j) mod 12). Fällt die Wahl auf Binden, schlägt sie die
//!   nächste mögliche Vertragsstufe vor. Zuschlagen bleibt vorerst ohne Folge. Diese
//!   Eröffnungen sind abgeschaltet, solange `ai_openings` in `diplomacy.ron` falsch ist:
//!   Sie gehören zur strategischen Führung, die noch folgt.
//! - Im Krieg bietet sie ab `peace_min_months` Frieden an, wenn sie zurückliegt oder
//!   kriegsmüde ist, und fordert Bedingungen, wenn sie deutlich vorn liegt (nur mit
//!   `ai_peace_offers`; bis die strategische Führung folgt, enden Kriege der KI durch
//!   Drehbuch, Spieler oder das Diktat).
//! - Halten die Bedingungen einer Vereinigung lange genug, schlägt die Stärkere sie unter
//!   ihrer Führung vor.
//!
//! Wird eine Gemeinschaft nicht aktiv geführt (`MilitaryMode` außer `Active`, also von
//! einem Drehbuch), bietet ihre Führung keinen Frieden an und lehnt jeden ab.

use super::{AiState, MilitaryMode};
use crate::clock::DAYS_PER_YEAR;
use crate::command::{CommandQueue, CommandSource, SimCommand};
use crate::data::GameData;
use crate::diplomacy::occasion::{ACCEPT, BIND, Occasion};
use crate::diplomacy::sight::{Ground, Perspective};
use crate::diplomacy::{
    Deal, Diplomacy, PeaceTerms, TreatyKind, choice_probs, proposals, treaties, war,
};
use crate::ids::PolityId;
use crate::rng::{Domain, Rng};
use crate::state::SimConfig;

/// Zieht eine Wahl nach ihren Wahrscheinlichkeiten.
fn draw(probs: &[f32], roll: f64) -> usize {
    let mut acc = 0.0f64;
    for (index, p) in probs.iter().enumerate() {
        acc += *p as f64;
        if roll < acc {
            return index;
        }
    }
    probs.len().saturating_sub(1)
}

/// Führt die KI die Diplomatie dieser Gemeinschaft?
fn led_by_ai(config: &SimConfig, ground: &Ground, polity: PolityId) -> bool {
    config.player != Some(polity)
        && ground
            .polities
            .list
            .get(polity.index())
            .is_some_and(|found| found.alive)
        && ground.diplomacy.is_free(polity)
}

/// Kann `from` diesen Vorschlag jetzt machen (nichts Gleiches offen, keine Sperre)?
fn may_propose(diplomacy: &Diplomacy, from: PolityId, to: PolityId, deal: &Deal, day: u64) -> bool {
    let code = deal.code();
    !diplomacy
        .proposals_between(from, to)
        .any(|open| open.deal.code() == code)
        && diplomacy
            .blocked
            .get(&(from, to, code))
            .is_none_or(|until| *until <= day)
}

/// Täglich: Antworten auf Vorschläge an KI-geführte Gemeinschaften, sobald ihre Frist
/// gekommen ist.
pub fn answer_proposals(
    ground: &Ground,
    config: &SimConfig,
    ai: &AiState,
    queue: &mut CommandQueue,
) {
    let data = ground.data;
    let levers = &data.diplomacy.levers;
    let (early, late) = levers.answer_days;
    for proposal in &ground.diplomacy.proposals {
        let me = proposal.to;
        if !led_by_ai(config, ground, me) {
            continue;
        }
        let delay = early + (proposal.id as u64).wrapping_mul(2_654_435_761) % (late - early + 1);
        if ground.day < proposal.made_day + delay {
            continue;
        }
        let accept = if matches!(proposal.deal, Deal::Peace(_))
            && ai.mode(me) != MilitaryMode::Active
        {
            false
        } else {
            let answering = proposals::answering(ground.galaxy, &proposal.deal, me, data);
            let picture = ground.picture(Perspective::Own, me, proposal.from);
            let profile = ai.minds.profile(data, me);
            let probs = choice_probs(&profile, &Occasion::Answer(answering, picture), levers);
            let mut rng = Rng::stream(config.seed, Domain::Ai, 0xA5_0000 | proposal.id as u64, 1);
            draw(&probs, rng.f64()) == ACCEPT
        };
        queue.push(
            me,
            CommandSource::Ai,
            SimCommand::AnswerProposal {
                proposal: proposal.id,
                accept,
            },
        );
    }
}

/// Nächste Vertragsstufe, die die beiden schließen könnten.
fn next_rung(ground: &Ground, a: PolityId, b: PolityId) -> Option<TreatyKind> {
    TreatyKind::ALL
        .into_iter()
        .find(|kind| treaties::check_treaty(ground.diplomacy, ground.polities, a, b, *kind).is_ok())
}

/// Monatlich: Druck auf die Haltungen, Eröffnungen, Frieden, Vereinigung.
pub fn diplomatic_month(
    ground: &Ground,
    config: &SimConfig,
    ai: &mut AiState,
    queue: &mut CommandQueue,
) {
    let data: &GameData = ground.data;
    let levers = &data.diplomacy.levers;
    let count = ground.polities.list.len();
    ai.minds.ensure(data, config.seed, count);
    let alive: Vec<PolityId> = ground.polities.alive().map(|polity| polity.id).collect();
    let day = ground.day;
    let month = (day / 30) % 12;
    let year = day / DAYS_PER_YEAR;

    // Druck auf die Haltungen: aus dem eigenen Bild der Lage, wie es auch der Spieler
    // hätte.
    for me in &alive {
        let mut pressure = [0.0f32; 4];
        for other in alive.iter().filter(|other| *other != me) {
            let picture = ground.picture(Perspective::Own, *me, *other);
            let tie = ground
                .diplomacy
                .relation(*me, *other)
                .map_or(0.0, |relation| relation.tie);
            pressure[0] = pressure[0].max((tie / levers.merge_tie).min(1.0).max(picture.dep));
            pressure[1] =
                pressure[1].max((picture.ratio - 0.3).clamp(0.0, 1.0) * (1.0 - picture.dep));
            pressure[2] = pressure[2].max(picture.threat);
            pressure[3] = pressure[3].max(picture.want);
        }
        ai.minds.drift(data, *me, pressure);
    }

    for me in alive
        .iter()
        .copied()
        .filter(|me| led_by_ai(config, ground, *me))
    {
        let profile = ai.minds.profile(data, me);
        for other in alive.iter().copied().filter(|other| *other != me) {
            if !ground.diplomacy.is_free(other) {
                continue;
            }
            // Frieden.
            if let Some(known) = ground.diplomacy.war_between(me, other) {
                if !levers.ai_peace_offers
                    || ai.mode(me) != MilitaryMode::Active
                    || war::months(known, day * crate::clock::TICKS_PER_DAY)
                        < levers.peace_min_months
                {
                    continue;
                }
                let lead = war::lead_of(ground.diplomacy, ground.finance, levers, me, other);
                let weary = war::weariness(ground.finance, me);
                let band = war::terms_band(
                    Some(known),
                    lead,
                    proposals::capital_taken(ground.diplomacy, ground.polities, me, other),
                    me,
                    levers,
                );
                let half = (levers.terms_years / 2).max(1);
                let terms = if lead >= levers.terms_subjugate && band.subjugation {
                    Some(PeaceTerms {
                        yielder: Some(other),
                        cede: Vec::new(),
                        tribute_years: half,
                        subjugate: true,
                    })
                } else if lead >= levers.terms_tribute + 10.0 {
                    Some(PeaceTerms {
                        yielder: Some(other),
                        cede: Vec::new(),
                        tribute_years: half,
                        subjugate: false,
                    })
                } else if lead <= -2.0 * levers.terms_tribute {
                    Some(PeaceTerms {
                        yielder: Some(me),
                        cede: Vec::new(),
                        tribute_years: half,
                        subjugate: false,
                    })
                } else if lead <= -levers.terms_tribute || weary >= 50.0 {
                    Some(PeaceTerms::white())
                } else {
                    None
                };
                if let Some(terms) = terms {
                    let deal = Deal::Peace(terms);
                    if may_propose(ground.diplomacy, me, other, &deal, day) {
                        queue.push(
                            me,
                            CommandSource::Ai,
                            SimCommand::Propose { to: other, deal },
                        );
                    }
                }
                continue;
            }
            // Vereinigung unter eigener Führung, wenn die Bedingungen lange genug halten
            // und sie die Stärkere ist.
            let relation = ground.diplomacy.relation(me, other);
            if relation.is_some_and(|relation| {
                relation.has(TreatyKind::Union) && relation.merge_months >= levers.merge_hold_months
            }) && ground.diplomacy.surveyed(me).power >= ground.power_estimate(Some(me), other)
            {
                let deal = Deal::Merge { lead: me };
                if may_propose(ground.diplomacy, me, other, &deal, day) {
                    queue.push(
                        me,
                        CommandSource::Ai,
                        SimCommand::Propose { to: other, deal },
                    );
                }
                continue;
            }
            // Eröffnung, einmal im Jahr je Paar; nur, wenn die Hebel sie schon zulassen.
            if !levers.ai_openings
                || (7 * me.index() as u64 + 3 * other.index() as u64) % 12 != month
            {
                continue;
            }
            let picture = ground.picture(Perspective::Own, me, other);
            let probs = choice_probs(&profile, &Occasion::Opening(picture), levers);
            let mut rng = Rng::stream(
                config.seed,
                Domain::Ai,
                ((me.index() as u64) << 16) | other.index() as u64,
                year,
            );
            if draw(&probs, rng.f64()) != BIND {
                continue;
            }
            if let Some(kind) = next_rung(ground, me, other) {
                let deal = Deal::Treaty(kind);
                if may_propose(ground.diplomacy, me, other, &deal, day) {
                    queue.push(
                        me,
                        CommandSource::Ai,
                        SimCommand::Propose { to: other, deal },
                    );
                }
            }
        }
    }
}

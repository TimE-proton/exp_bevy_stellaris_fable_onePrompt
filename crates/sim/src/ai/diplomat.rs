//! Strategische Führung der KI: Antworten, Pläne, Krieg mit Ziel, Frieden, Bindung.
//!
//! Die KI handelt wie der Spieler nur über Befehle. Sie entscheidet mit ihrem gemischten,
//! verborgenen Profil über `choice_probs` und ihrem eigenen Bild der Lage
//! (`Perspective::Own`): eigene Zahlen genau, fremde Flotten und Technik geschätzt, die
//! Haltung der anderen aus ihren Dossiers. Sie fragt nie, ob die andere Seite ein Mensch
//! ist.
//!
//! - Antworten: auf jeden Vorschlag `answer_days` Tage nach Eingang, von Anfang an.
//! - Gründerzeit: In den ersten `founding_years` Jahren besiedelt die Gemeinschaft nur;
//!   danach führt sie ihre Beziehungen selbst.
//! - Eröffnung: Einmal im Jahr wägt sie gegenüber jeder freien Gemeinschaft Zuschlagen,
//!   Binden und Abwarten ab (Monat (7i + 3j) mod 12). Die Wahl wird ein Plan, der
//!   `plan_years` gilt.
//! - Zuschlagen: Das Kriegsziel (Unterwerfung oder begrenzt) zieht sie bei der Wahl. Sie
//!   rüstet (`mobilize_factor`), verhängt als Verschlossene ein Embargo, und erklärt nach
//!   `prep_days`, wenn ihre Siegaussicht `strike_win` erreicht. Bestehen Verträge, kündigt
//!   eine Vertragstreue sie vorher (mit Wahrscheinlichkeit ihrer Treue) und wartet die
//!   Frist ab; die anderen brechen sie mit der Erklärung.
//! - Binden: die nächste Vertragsstufe; unter einem Investitionsvertrag kauft sie Anteile
//!   an Betrieben des Partners; halten die Bedingungen einer Vereinigung, schlägt die
//!   Stärkere sie unter ihrer Führung vor.
//! - Schutz: Eine kleine Gemeinschaft sucht einmal im Jahr Schutz bei der, der sie am
//!   meisten vertraut, sobald ihr Nutzen `shelter_act` erreicht.
//! - Frieden: ab `peace_min_months` nach Bilanz, Kriegsziel und Müdigkeit; Kriege, die
//!   ein Drehbuch erklärt hat, beendet nur das Drehbuch.
//!
//! Wird eine Gemeinschaft nicht aktiv geführt (`MilitaryMode` außer `Active`, also von
//! einem Drehbuch), bietet ihre Führung keinen Frieden an und lehnt jeden ab. Führt ein
//! Drehbuch ihre Diplomatie (`AiState::diplomacy_hold`), tut die KI dort nichts.

use super::mind::{Plan, PlanKind};
use super::{AiState, MilitaryMode};
use crate::clock::{DAYS_PER_YEAR, TICKS_PER_DAY};
use crate::command::{CommandQueue, CommandSource, SimCommand};
use crate::data::{ArchetypeDef, GameData};
use crate::diplomacy::occasion::{
    ACCEPT, Answering, BIND, Occasion, STRIKE, SUBJUGATE, u_accept, win_chance,
};
use crate::diplomacy::sight::{Ground, Perspective};
use crate::diplomacy::{
    Deal, Diplomacy, PeaceTerms, TreatyKind, WarAim, choice_probs, pair, proposals, treaties,
    war,
};
use crate::ids::{IssuerId, PolityId};
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
fn led_by_ai(config: &SimConfig, ai: &AiState, ground: &Ground, polity: PolityId) -> bool {
    config.player != Some(polity)
        && !ai.diplomacy_hold.contains(&polity)
        && ground
            .polities
            .list
            .get(polity.index())
            .is_some_and(|found| found.alive)
        && ground.diplomacy.is_free(polity)
}

/// Kann `from` diesen Vorschlag jetzt machen (nichts Gleiches offen, keine Sperre)?
pub(crate) fn may_propose(diplomacy: &Diplomacy, from: PolityId, to: PolityId, deal: &Deal, day: u64) -> bool {
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
        if !led_by_ai(config, ai, ground, me) {
            continue;
        }
        let delay = early + (proposal.id as u64).wrapping_mul(2_654_435_761) % (late - early + 1);
        if ground.day < proposal.made_day + delay {
            continue;
        }
        // Zu schneller Aufstieg, oder ein Vertrag mit dem, gegen den sie zuschlagen will.
        let striking = ai
            .minds
            .plan(me, proposal.from)
            .is_some_and(|plan| matches!(plan.kind, PlanKind::Strike(_)));
        let hasty = matches!(proposal.deal, Deal::Treaty(kind)
            if striking || !rung_ripe(ground, me, proposal.from, kind));
        let accept = if hasty
            || matches!(proposal.deal, Deal::Peace(_))
                && (ai.mode(me) != MilitaryMode::Active || pressing(ground, me, proposal))
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

/// Führt `me` einen Krieg um die Unterwerfung des Absenders und ist noch nicht müde und
/// hat die halbe Frist nicht verbraucht, oder liegt sie vorn? Dann nimmt sie keinen
/// Frieden an, der ihr die Unterwerfung nicht bringt.
fn pressing(ground: &Ground, me: PolityId, proposal: &crate::diplomacy::Proposal) -> bool {
    let levers = &ground.data.diplomacy.levers;
    let Deal::Peace(terms) = &proposal.deal else {
        return false;
    };
    let Some(known) = ground.diplomacy.war_between(me, proposal.from) else {
        return false;
    };
    let lead = war::lead_of(ground.diplomacy, ground.finance, levers, me, proposal.from);
    let fresh = war::weariness(ground.finance, me) < 50.0
        && war::months(known, ground.day * TICKS_PER_DAY) < levers.war_max_months / 2;
    // Wer vorn liegt, wartet notfalls auf das Diktat.
    known.attacker == me
        && known.aim == WarAim::Subjugation
        && !(terms.subjugate && terms.yielder == Some(proposal.from))
        && (fresh || lead >= levers.terms_tribute)
}

/// Ist diese Stufe reif? Ihre Voraussetzungen haben lange genug gehalten
/// (`rung_hold_months`), und eine Union verlangt kulturelle Nähe (`Diplomacy::merge_affinity_needed`). Die
/// KI steigt nicht schneller, als Vertrauen wächst, und nimmt keinen schnelleren Aufstieg
/// an.
pub(crate) fn rung_ripe(ground: &Ground, a: PolityId, b: PolityId, kind: TreatyKind) -> bool {
    let levers = &ground.data.diplomacy.levers;
    // Eine Union nur mit kulturell Nahen: Sie soll in die Vereinigung führen können.
    if kind == TreatyKind::Union
        && ground.diplomacy.affinity(a, b) < ground.diplomacy.merge_affinity_needed(levers, a, b)
    {
        return false;
    }
    let hold = u64::from(levers.rung_hold_months) * 30;
    let Some(relation) = ground.diplomacy.relation(a, b) else {
        return true;
    };
    kind.requires().iter().all(|need| {
        relation
            .treaty(*need)
            .is_some_and(|treaty| ground.day >= treaty.since_day + hold)
    })
}

/// Nächste Vertragsstufe, die die beiden schließen könnten.
pub(crate) fn next_rung(ground: &Ground, a: PolityId, b: PolityId) -> Option<TreatyKind> {
    TreatyKind::ALL.into_iter().find(|kind| {
        treaties::check_treaty(ground.diplomacy, ground.polities, a, b, *kind).is_ok()
            && rung_ripe(ground, a, b, *kind)
    })
}

/// Ist die Gründerzeit vorbei?
pub fn strategy_open(data: &GameData, day: u64) -> bool {
    day >= data.diplomacy.levers.founding_years * DAYS_PER_YEAR
}

/// Ein Befehl der KI.
fn push(queue: &mut CommandQueue, me: PolityId, command: SimCommand) {
    queue.push(me, CommandSource::Ai, command);
}

/// Ein Vorschlag, wenn er jetzt zulässig und nichts Gleiches offen ist.
fn offer(ground: &Ground, queue: &mut CommandQueue, me: PolityId, to: PolityId, deal: Deal) {
    if !may_propose(ground.diplomacy, me, to, &deal, ground.day) {
        return;
    }
    // Hat die andere Seite dasselbe in diesem Takt schon vorgeschlagen, genügt das.
    let code = deal.code();
    let crossing = queue.pending.iter().any(|queued| {
        matches!(&queued.command, SimCommand::Propose { to: back, deal: theirs }
            if queued.polity == to && *back == me && theirs.code() == code)
    });
    if crossing {
        return;
    }
    let data = ground.data;
    if proposals::check_deal(
        data,
        ground.galaxy,
        ground.polities,
        ground.diplomacy,
        ground.finance,
        me,
        to,
        &deal,
    )
    .is_err()
    {
        return;
    }
    push(queue, me, SimCommand::Propose { to, deal });
}

/// Monatlich: Druck auf die Haltungen, dann, nach der Gründerzeit, die Führung jeder
/// KI-geführten freien Gemeinschaft.
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
            pressure[0] = pressure[0].max((tie / levers.tie_scale).min(1.0).max(picture.dep));
            pressure[1] =
                pressure[1].max((picture.ratio - 0.3).clamp(0.0, 1.0) * (1.0 - picture.dep));
            pressure[2] = pressure[2].max(picture.threat);
            pressure[3] = pressure[3].max(picture.want);
        }
        ai.minds.drift(data, *me, pressure);
    }

    if !strategy_open(data, day) {
        return;
    }
    let free: Vec<PolityId> = alive
        .iter()
        .copied()
        .filter(|id| ground.diplomacy.is_free(*id))
        .collect();
    let led: Vec<PolityId> = free
        .iter()
        .copied()
        .filter(|me| led_by_ai(config, ai, ground, *me))
        .collect();
    for me in led {
        lead_community(ground, config, ai, queue, me, &free);
    }
}

/// Die Führung einer Gemeinschaft in einem Monat.
fn lead_community(
    ground: &Ground,
    config: &SimConfig,
    ai: &mut AiState,
    queue: &mut CommandQueue,
    me: PolityId,
    free: &[PolityId],
) {
    let data = ground.data;
    let levers = &data.diplomacy.levers;
    let day = ground.day;
    let year = day / DAYS_PER_YEAR;
    let month = (day / 30) % 12;
    let profile = ai.minds.profile(data, me);

    // Pläne gegen Gemeinschaften, die nicht mehr frei sind oder endeten, verfallen; ebenso
    // abgelaufene.
    for other in ground.polities.list.iter().map(|polity| polity.id) {
        if let Some(plan) = ai.minds.plan(me, other)
            && (!free.contains(&other) || day > plan.until_day)
        {
            ai.minds.set_plan(me, other, None);
        }
    }

    for other in free.iter().copied().filter(|other| *other != me) {
        if let Some(known) = ground.diplomacy.war_between(me, other).cloned() {
            ai.minds.set_plan(me, other, None);
            seek_peace(ground, ai, queue, me, other, &known, &profile);
            continue;
        }
        // Vereinigung unter eigener Führung, wenn die Bedingungen lange genug halten und
        // sie die Stärkere ist.
        let relation = ground.diplomacy.relation(me, other);
        if relation.is_some_and(|relation| {
            relation.has(TreatyKind::Union) && relation.merge_months >= levers.merge_hold_months
        }) && ground.diplomacy.surveyed(me).power >= ground.power_estimate(Some(me), other)
        {
            // Führen will nur, wem die Vereinigung selbst etwas gilt: Wer wenig auf
            // Verbindung gibt (`merge_lead_min`), wächst lieber durch Unterwerfung.
            let picture = ground.picture(Perspective::Own, me, other);
            if profile.merge >= levers.merge_lead_min
                && u_accept(Answering::Merge { i_lead: true }, &picture, &profile, levers)
                    >= levers.act
            {
                offer(ground, queue, me, other, Deal::Merge { lead: me });
                continue;
            }
        }
        match ai.minds.plan(me, other) {
            Some(plan) => match plan.kind {
                PlanKind::Strike(aim) => {
                    pursue_strike(ground, config, ai, queue, me, other, plan, aim, &profile)
                }
                PlanKind::Bind => pursue_bind(ground, queue, me, other, &profile),
            },
            None => {
                // Unter einer Union wächst die Verflechtung weiter, ohne eigenen Plan.
                if ground.diplomacy.has_treaty(me, other, TreatyKind::Union)
                    && let Some(deal) = swap_deal(ground, me, other)
                {
                    offer(ground, queue, me, other, deal);
                }
                // Eröffnung, einmal im Jahr je Paar.
                if (7 * me.index() as u64 + 3 * other.index() as u64) % 12 == month {
                    open_toward(ground, config, ai, me, other, &profile, year);
                }
            }
        }
        embargo_policy(ground, ai, queue, me, other, &profile);
    }

    // Schutz suchen, einmal im Jahr.
    if (5 * me.index() as u64 + 1) % 12 == month {
        seek_shelter(ground, queue, me, free, &profile);
    }
}

/// Eröffnung gegenüber `other`: Zuschlagen, Binden oder Abwarten.
fn open_toward(
    ground: &Ground,
    config: &SimConfig,
    ai: &mut AiState,
    me: PolityId,
    other: PolityId,
    profile: &ArchetypeDef,
    year: u64,
) {
    let levers = &ground.data.diplomacy.levers;
    if !levers.ai_openings {
        return;
    }
    let day = ground.day;
    let picture = ground.picture(Perspective::Own, me, other);
    let probs = choice_probs(profile, &Occasion::Opening(picture), levers);
    if std::env::var("DBG_OPEN").is_ok() && year >= 120 {
        eprintln!(
            "OPEN y{year} {}->{} us{:.2} ub{:.2} win{:.2} want{:.2} ratio{:.2} rungs{} truce{} dep{:.2} p{:?}",
            me.index(), other.index(),
            crate::diplomacy::occasion::u_strike(&picture, profile, levers),
            crate::diplomacy::occasion::u_bind(&picture, profile),
            win_chance(&picture, profile, levers), picture.want, picture.ratio, picture.rungs, picture.truce, picture.dep, probs
        );
    }
    let key = ((me.index() as u64) << 16) | other.index() as u64;
    let mut rng = Rng::stream(config.seed, Domain::Ai, key, year);
    let until_day = day + levers.plan_years * DAYS_PER_YEAR;
    let kind = match draw(&probs, rng.f64()) {
        STRIKE => {
            let aims = choice_probs(profile, &Occasion::Aim(picture), levers);
            let mut rng = Rng::stream(config.seed, Domain::Ai, 0xA1_0000_0000 | key, year);
            PlanKind::Strike(if draw(&aims, rng.f64()) == SUBJUGATE {
                WarAim::Subjugation
            } else {
                WarAim::Limited
            })
        }
        BIND => PlanKind::Bind,
        _ => return,
    };
    ai.minds.set_plan(
        me,
        other,
        Some(Plan {
            kind,
            made_day: day,
            ready_day: day + levers.prep_days,
            until_day,
            denounced: false,
        }),
    );
}

/// Verträge des Paares, die gelten.
fn treaties_between(diplomacy: &Diplomacy, a: PolityId, b: PolityId) -> Vec<TreatyKind> {
    diplomacy.relation(a, b).map_or(Vec::new(), |relation| {
        TreatyKind::ALL
            .into_iter()
            .filter(|kind| relation.has(*kind))
            .collect()
    })
}

/// Ein Plan zum Zuschlagen: vorbereiten, Verträge kündigen oder brechen, erklären.
#[allow(clippy::too_many_arguments)]
fn pursue_strike(
    ground: &Ground,
    config: &SimConfig,
    ai: &mut AiState,
    queue: &mut CommandQueue,
    me: PolityId,
    other: PolityId,
    mut plan: Plan,
    aim: WarAim,
    profile: &ArchetypeDef,
) {
    let levers = &ground.data.diplomacy.levers;
    let day = ground.day;
    if ai.mode(me) != MilitaryMode::Active || ground.diplomacy.is_at_war(me) {
        // Wer schon Krieg führt, eröffnet keinen zweiten: Der Plan wartet.
        return;
    }
    if day < plan.ready_day {
        return;
    }
    if let Some(until) = ground.diplomacy.truce_until(me, other, day) {
        if until > plan.until_day {
            ai.minds.set_plan(me, other, None);
        }
        return;
    }
    let bound = treaties_between(ground.diplomacy, me, other);
    if !bound.is_empty() && !plan.denounced {
        // Kündigen oder brechen: nach der eigenen Vertragstreue.
        let key = 0xB0_0000_0000 | ((me.index() as u64) << 16) | other.index() as u64;
        let mut rng = Rng::stream(config.seed, Domain::Ai, key, plan.made_day);
        if (rng.f64() as f32) < profile.honor {
            // Die unterste Stufe trägt alle anderen: Ihre Kündigung beendet alles.
            let base = bound[0];
            push(
                queue,
                me,
                SimCommand::CancelTreaty {
                    with: other,
                    kind: base,
                },
            );
            plan.denounced = true;
            plan.ready_day = day + levers.notice_days + 1;
            plan.until_day = plan.until_day.max(plan.ready_day + levers.prep_days);
            ai.minds.set_plan(me, other, Some(plan));
            return;
        }
    } else if !bound.is_empty() {
        // Gekündigt: Die Frist läuft noch.
        return;
    }
    let picture = ground.picture(Perspective::Own, me, other);
    if win_chance(&picture, profile, levers) < levers.strike_win {
        return;
    }
    push(
        queue,
        me,
        SimCommand::DeclareWarWithAim {
            target: other,
            aim,
        },
    );
    ai.minds.set_plan(me, other, None);
}

/// Marktwert der Betriebe auf dem Gebiet einer Gemeinschaft, Credits.
pub(crate) fn firm_value(ground: &Ground, polity: PolityId) -> f64 {
    ground
        .finance
        .issuers
        .iter()
        .filter(|(id, _)| {
            matches!(id, IssuerId::Firm { planet, .. }
                if ground.galaxy.planet(*planet).owner == Some(polity))
        })
        .map(|(_, issuer)| issuer.market_value())
        .sum()
}

/// Eine Tranche des Anteilstauschs: `swap_tranche` des Marktwerts der kleineren Seite.
pub(crate) fn swap_deal(ground: &Ground, a: PolityId, b: PolityId) -> Option<Deal> {
    let levers = &ground.data.diplomacy.levers;
    if !ground.diplomacy.has_treaty(a, b, TreatyKind::Investment) {
        return None;
    }
    let tie = ground.diplomacy.relation(a, b).map_or(0.0, |relation| relation.tie);
    if tie >= levers.merge_tie * 1.2 {
        return None;
    }
    let smaller = firm_value(ground, a).min(firm_value(ground, b));
    let value = (smaller * levers.swap_tranche).round() as u64;
    (value > 0).then_some(Deal::Swap { value })
}

/// Ein Plan zum Binden: die nächste Stufe; unter Investition Anteile tauschen, bis die
/// Verflechtung für eine Vereinigung reicht.
fn pursue_bind(
    ground: &Ground,
    queue: &mut CommandQueue,
    me: PolityId,
    other: PolityId,
    profile: &ArchetypeDef,
) {
    // Sie trägt nur an, was sie selbst annähme.
    let levers = &ground.data.diplomacy.levers;
    let picture = ground.picture(Perspective::Own, me, other);
    if let Some(kind) = next_rung(ground, me, other)
        && u_accept(Answering::Treaty(kind), &picture, profile, levers) >= 0.0
    {
        offer(ground, queue, me, other, Deal::Treaty(kind));
    }
    if let Some(deal) = swap_deal(ground, me, other) {
        offer(ground, queue, me, other, deal);
    }
}

/// Embargo gegen Rivalen: Wer verschlossen ist, verhängt es, solange er zuschlagen will
/// oder die Spannung hoch ist, und hebt es wieder auf, wenn beides vorbei ist.
fn embargo_policy(
    ground: &Ground,
    ai: &AiState,
    queue: &mut CommandQueue,
    me: PolityId,
    other: PolityId,
    profile: &ArchetypeDef,
) {
    let Some(relation) = ground.diplomacy.relation(me, other) else {
        return;
    };
    let mine = relation.embargo[crate::diplomacy::relations::side(me, other)];
    let striking = ai
        .minds
        .plan(me, other)
        .is_some_and(|plan| matches!(plan.kind, PlanKind::Strike(_)));
    let tension = relation.tension;
    let bound = relation.has(TreatyKind::Trade) || relation.has(TreatyKind::Union);
    let hostile = profile.open < 0.5 && (striking || tension >= 60.0);
    if !mine && hostile && !bound {
        push(
            queue,
            me,
            SimCommand::SetEmbargo {
                target: other,
                on: true,
            },
        );
    } else if mine
        && !striking
        // Das Embargo selbst trägt 20 Punkte zur Spannung bei: Gemessen wird ohne sie.
        && tension - 20.0 < 40.0
        && !ground.diplomacy.at_war(me, other)
    {
        push(
            queue,
            me,
            SimCommand::SetEmbargo {
                target: other,
                on: false,
            },
        );
    }
}

/// Eine kleine Gemeinschaft sucht Schutz bei der, der sie am meisten zutraut.
fn seek_shelter(
    ground: &Ground,
    queue: &mut CommandQueue,
    me: PolityId,
    free: &[PolityId],
    profile: &ArchetypeDef,
) {
    let levers = &ground.data.diplomacy.levers;
    if ground.diplomacy.is_at_war(me) {
        return;
    }
    let own = ground.diplomacy.surveyed(me).power;
    if own >= levers.minor_share {
        return;
    }
    let best = free
        .iter()
        .copied()
        .filter(|lord| *lord != me && !ground.diplomacy.is_at_war(*lord))
        .filter(|lord| ground.power_estimate(Some(me), *lord) > own)
        .map(|lord| {
            let picture = ground.picture(Perspective::Own, me, lord);
            let utility = u_accept(
                Answering::Protectorate { as_ward: true },
                &picture,
                profile,
                levers,
            );
            (utility, lord)
        })
        .max_by(|a, b| a.0.total_cmp(&b.0).then(b.1.cmp(&a.1)));
    if let Some((utility, lord)) = best
        && utility >= levers.shelter_act
    {
        offer(ground, queue, me, lord, Deal::Protectorate { ward: me });
    }
}

/// Frieden nach Bilanz, Kriegsziel und Müdigkeit.
fn seek_peace(
    ground: &Ground,
    ai: &AiState,
    queue: &mut CommandQueue,
    me: PolityId,
    other: PolityId,
    known: &crate::diplomacy::War,
    profile: &ArchetypeDef,
) {
    let levers = &ground.data.diplomacy.levers;
    let day = ground.day;
    if !levers.ai_peace_offers
        || ai.mode(me) != MilitaryMode::Active
        || ai.scripted_wars.contains(&pair(me, other))
    {
        return;
    }
    let months = war::months(known, day * TICKS_PER_DAY);
    if months < levers.peace_min_months {
        return;
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
    let aim = (known.attacker == me).then_some(known.aim);
    let half = (levers.terms_years / 2).max(1);
    let yield_to = |yielder: PolityId, subjugate: bool| PeaceTerms {
        yielder: Some(yielder),
        cede: Vec::new(),
        tribute_years: half,
        subjugate,
    };
    // Wer auf Unterwerfung aus ist, begnügt sich nicht mit Tribut, solange er nicht
    // müde ist und die halbe Frist nicht um ist, oder solange er vorn liegt: Dann wartet
    // er auf das Diktat.
    let pressing = aim == Some(WarAim::Subjugation)
        && ((weary < 50.0 && months < levers.war_max_months / 2) || lead >= levers.terms_tribute);
    let terms = if band.subjugation {
        Some(yield_to(other, true))
    } else if lead >= levers.terms_tribute + 10.0 && !pressing {
        Some(yield_to(other, false))
    } else if lead <= -2.0 * levers.terms_tribute {
        Some(yield_to(me, false))
    } else if lead <= -levers.terms_tribute || weary >= 50.0 {
        Some(PeaceTerms::white())
    } else if months >= levers.status_quo_months && lead.abs() < levers.terms_tribute {
        let picture = ground.picture(Perspective::Own, me, other);
        (win_chance(&picture, profile, levers) < 0.5 && !pressing).then(PeaceTerms::white)
    } else {
        None
    };
    if let Some(terms) = terms {
        offer(ground, queue, me, other, Deal::Peace(terms));
    }
}

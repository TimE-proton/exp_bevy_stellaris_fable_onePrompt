//! Vorschläge: was eine Gemeinschaft einer anderen anträgt, und was eine Annahme vollzieht.
//!
//! Ein Vorschlag geht von `from` an `to` und bleibt `proposal_days` Tage offen. Je Paar
//! und Art gibt es höchstens einen offenen Vorschlag. Der Empfänger nimmt an oder lehnt
//! ab; der Absender kann zurückziehen. Nach einer Ablehnung ruht derselbe Vorschlag
//! (`refusal_block_days`, bei Frieden `peace_block_days`). Bei der Annahme werden die
//! Voraussetzungen neu geprüft: Was inzwischen nicht mehr passt, verfällt.
//!
//! Was eine Annahme vollzieht ([`conclude`]):
//! - Vertrag: wird geschlossen.
//! - Frieden: Der Krieg endet mit Waffenruhe; der Nachgebende tritt die genannten Welten
//!   ab (als genommen, `Path::Forced`), zahlt Tribut und wird, wenn vereinbart, Vasall
//!   des anderen (`Succession::subordinate`, erzwungen).
//! - Schutz: Der Schützling unterstellt sich der Schutzmacht (gewollt, `Path::Joined`).
//! - Vereinigung: Die geführte Gemeinschaft geht in der führenden auf
//!   (`Succession::absorb` auf dem Weg `Path::Joined`).
//! - Anteilstausch: Jeder Staat überträgt dem anderen eigene Anteile an Betrieben auf
//!   seinem Gebiet im Wert von `value` Credits (soweit er sie hält). Es fließt kein Geld;
//!   die Verflechtung beider wächst.

use super::occasion::{Answering, DeclineReason};
use super::relations::{TreatyKind, Tribute};
use super::{Diplomacy, check_pair, treaties, war};
use crate::chronicle::{Chronicle, ChronicleKind};
use crate::clock::{DAYS_PER_YEAR, TICKS_PER_DAY};
use crate::data::GameData;
use crate::endgame::{Path, PlayerFate};
use crate::finance::{Finance, Stage};
use crate::galaxy::Galaxy;
use crate::ids::{HolderId, PlanetId, PolityId};
use crate::polity::Polities;
use crate::polity::succession::{Succession, TransferTerms};
use serde::{Deserialize, Serialize};

/// So viele erledigte Vorschläge bleiben im Verlauf.
pub const HISTORY: usize = 64;

/// Bedingungen eines Friedens.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeaceTerms {
    /// Wer nachgibt: tritt ab, zahlt, wird unterworfen. `None`: Frieden ohne Bedingungen.
    pub yielder: Option<PolityId>,
    /// Welten, die der Nachgebende abtritt (höchstens `max_cede`, nie die Hauptwelt).
    pub cede: Vec<PlanetId>,
    /// Jahre Tribut (Anteil `reparations` der Einnahmen), höchstens `terms_years`.
    pub tribute_years: u8,
    /// Der Nachgebende wird Vasall des anderen.
    pub subjugate: bool,
}

impl PeaceTerms {
    /// Weißer Frieden: Status quo, niemand gibt etwas.
    pub fn white() -> Self {
        Self::default()
    }

    pub fn is_white(&self) -> bool {
        self.yielder.is_none()
            || (self.cede.is_empty() && self.tribute_years == 0 && !self.subjugate)
    }

    pub fn label(&self, polities: &Polities) -> String {
        if self.is_white() {
            return "Frieden ohne Bedingungen".into();
        }
        let who = self
            .yielder
            .map_or("niemand", |id| polities.get(id).name.as_str());
        let mut parts = Vec::new();
        if !self.cede.is_empty() {
            parts.push(format!("tritt {} Welt(en) ab", self.cede.len()));
        }
        if self.tribute_years > 0 {
            parts.push(format!("zahlt {} Jahre Tribut", self.tribute_years));
        }
        if self.subjugate {
            parts.push("wird Vasall".into());
        }
        format!("Frieden: {who} {}", parts.join(", "))
    }
}

/// Inhalt eines Vorschlags.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Deal {
    /// Einen Vertrag schließen.
    Treaty(TreatyKind),
    /// Frieden zu diesen Bedingungen.
    Peace(PeaceTerms),
    /// `ward` unterstellt sich dem anderen als Schützling. Kann vom künftigen Schützling
    /// erbeten oder von der künftigen Schutzmacht angeboten werden.
    Protectorate { ward: PolityId },
    /// Vereinigung unter der Führung von `lead` (einer der beiden).
    Merge { lead: PolityId },
    /// Anteilstausch: Jeder gibt dem anderen eigene Anteile im Wert von `value` Credits.
    /// Setzt einen Investitionsvertrag voraus.
    Swap { value: u64 },
}

impl Deal {
    /// Art des Vorschlags für „ein offener Vorschlag je Paar und Art“ und die Sperren.
    pub fn code(&self) -> u8 {
        match self {
            Deal::Treaty(kind) => kind.index() as u8,
            Deal::Peace(_) => 10,
            Deal::Protectorate { .. } => 11,
            Deal::Merge { .. } => 12,
            Deal::Swap { .. } => 13,
        }
    }

    /// Nennt der Vorschlag diese Gemeinschaft (außer als Absender oder Empfänger)?
    pub fn names(&self, polity: PolityId) -> bool {
        match self {
            Deal::Treaty(_) => false,
            Deal::Peace(terms) => terms.yielder == Some(polity),
            Deal::Protectorate { ward } => *ward == polity,
            Deal::Merge { lead } => *lead == polity,
            Deal::Swap { .. } => false,
        }
    }

    pub fn label(&self, polities: &Polities) -> String {
        match self {
            Deal::Treaty(kind) => format!("Vertrag „{}“", kind.label()),
            Deal::Peace(terms) => terms.label(polities),
            Deal::Protectorate { ward } => {
                format!("Schutz: {} wird Schützling", polities.get(*ward).name)
            }
            Deal::Merge { lead } => {
                format!("Vereinigung unter Führung von {}", polities.get(*lead).name)
            }
            Deal::Swap { value } => format!("Anteilstausch über {value} Credits je Seite"),
        }
    }
}

/// Ein offener Vorschlag.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    pub id: u32,
    pub from: PolityId,
    pub to: PolityId,
    pub deal: Deal,
    pub made_day: u64,
    /// An diesem Tag verfällt er unbeantwortet.
    pub expires_day: u64,
}

/// Wie ein Vorschlag erledigt wurde.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProposalStatus {
    Accepted,
    /// Abgelehnt, mit dem genannten Grund (ein Mensch nennt keinen).
    Declined(Option<DeclineReason>),
    Withdrawn,
    Expired,
    /// Bei der Annahme passten die Voraussetzungen nicht mehr.
    Void,
}

impl ProposalStatus {
    pub fn label(self) -> &'static str {
        match self {
            ProposalStatus::Accepted => "angenommen",
            ProposalStatus::Declined(_) => "abgelehnt",
            ProposalStatus::Withdrawn => "zurückgezogen",
            ProposalStatus::Expired => "verfallen",
            ProposalStatus::Void => "hinfällig",
        }
    }
}

/// Ein erledigter Vorschlag.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalRecord {
    pub proposal: Proposal,
    pub status: ProposalStatus,
    pub day: u64,
}

/// Prüft einen Vorschlag auf seine Voraussetzungen, ohne Sperren und Doppelte.
pub fn check_deal(
    data: &GameData,
    galaxy: &Galaxy,
    polities: &Polities,
    diplomacy: &Diplomacy,
    finance: &Finance,
    from: PolityId,
    to: PolityId,
    deal: &Deal,
) -> Result<(), String> {
    let levers = &data.diplomacy.levers;
    check_pair(diplomacy, polities, from, to)?;
    match deal {
        Deal::Treaty(kind) => treaties::check_treaty(diplomacy, polities, from, to, *kind),
        Deal::Peace(terms) => {
            if !diplomacy.at_war(from, to) {
                return Err("es herrscht kein Krieg".into());
            }
            let Some(yielder) = terms.yielder.filter(|_| !terms.is_white()) else {
                return Ok(());
            };
            if yielder != from && yielder != to {
                return Err("nachgeben kann nur eine der beiden Kriegsparteien".into());
            }
            let victor = if yielder == from { to } else { from };
            if terms.cede.len() > levers.max_cede {
                return Err(format!(
                    "ein Frieden tritt höchstens {} Welten ab",
                    levers.max_cede
                ));
            }
            if usize::from(terms.tribute_years) > usize::from(levers.terms_years) {
                return Err(format!("Tribut höchstens {} Jahre", levers.terms_years));
            }
            let capital = polities.get(yielder).capital;
            for (index, planet) in terms.cede.iter().enumerate() {
                let Some(world) = galaxy.planets.get(planet.index()) else {
                    return Err("Planet unbekannt".into());
                };
                if world.owner != Some(yielder) {
                    return Err(format!(
                        "{} gehört nicht {}",
                        world.name,
                        polities.get(yielder).name
                    ));
                }
                if Some(*planet) == capital {
                    return Err(format!(
                        "{} ist die Hauptwelt und wird nicht abgetreten",
                        world.name
                    ));
                }
                if terms.cede[..index].contains(planet) {
                    return Err(format!("{} ist doppelt genannt", world.name));
                }
            }
            // Der Sieger fordert: nur im Band seines Vorsprungs. Ein Angebot des
            // Nachgebenden gilt, wie es ist.
            if from == victor {
                let lead = war::lead_of(diplomacy, finance, levers, victor, yielder);
                let capital_taken = capital_taken(diplomacy, polities, victor, yielder);
                let band = war::terms_band(
                    diplomacy.war_between(victor, yielder),
                    lead,
                    capital_taken,
                    victor,
                    levers,
                );
                war::check_terms(galaxy, terms, band, galaxy.population_of(yielder), levers)?;
            }
            Ok(())
        }
        Deal::Protectorate { ward } => {
            if *ward != from && *ward != to {
                return Err("Schützling ist eine der beiden".into());
            }
            if diplomacy.at_war(from, to) {
                return Err("im Krieg wird niemand Schützling des Gegners".into());
            }
            if diplomacy.is_at_war(*ward) {
                return Err(format!(
                    "{} führt Krieg und kann nicht unter Schutz treten",
                    polities.get(*ward).name
                ));
            }
            Ok(())
        }
        Deal::Swap { value } => {
            if *value == 0 {
                return Err("ein Tausch braucht einen Wert".into());
            }
            if !diplomacy.has_treaty(from, to, TreatyKind::Investment) {
                return Err("ein Anteilstausch setzt den Investitionsvertrag voraus".into());
            }
            if diplomacy.embargoed(from, to) {
                return Err("unter einem Embargo wird nicht getauscht".into());
            }
            Ok(())
        }
        Deal::Merge { lead } => {
            if *lead != from && *lead != to {
                return Err("führen kann nur eine der beiden".into());
            }
            if diplomacy.at_war(from, to) {
                return Err("im Krieg gibt es keine Vereinigung".into());
            }
            let relation = diplomacy.relation(from, to);
            if !relation.is_some_and(|relation| relation.has(TreatyKind::Union)) {
                return Err("eine Vereinigung setzt die Union voraus".into());
            }
            let held = relation.map_or(0, |relation| relation.merge_months);
            if held < levers.merge_hold_months {
                return Err(format!(
                    "die Bedingungen der Vereinigung (Union seit {} Monaten, Verflechtung ab \
                     {:.0} %, kulturelle Nähe ab {:.0} %) müssen {} Monate gehalten haben, \
                     bisher {held}",
                    levers.merge_union_months,
                    100.0 * levers.merge_tie,
                    100.0 * levers.merge_affinity,
                    levers.merge_hold_months
                ));
            }
            Ok(())
        }
    }
}

/// Hat `victor` in diesem Krieg die Hauptwelt von `loser` genommen?
pub fn capital_taken(
    diplomacy: &Diplomacy,
    _polities: &Polities,
    victor: PolityId,
    loser: PolityId,
) -> bool {
    let (victor, loser) = (diplomacy.head(victor), diplomacy.head(loser));
    diplomacy
        .war_between(victor, loser)
        .is_some_and(|war| war.took_capital[super::relations::side(victor, loser)])
}

/// Stellt einen Vorschlag ein. Gibt seine Nummer zurück.
#[allow(clippy::too_many_arguments)]
pub fn propose(
    data: &GameData,
    galaxy: &Galaxy,
    polities: &Polities,
    diplomacy: &mut Diplomacy,
    finance: &Finance,
    tick: u64,
    from: PolityId,
    to: PolityId,
    deal: Deal,
) -> Result<u32, String> {
    let day = tick / TICKS_PER_DAY;
    check_deal(data, galaxy, polities, diplomacy, finance, from, to, &deal)?;
    let code = deal.code();
    if diplomacy
        .proposals_between(from, to)
        .any(|open| open.deal.code() == code)
    {
        return Err("zwischen den beiden ist ein solcher Vorschlag schon offen".into());
    }
    if let Some(until) = diplomacy
        .blocked
        .get(&(from, to, code))
        .copied()
        .filter(|until| *until > day)
    {
        return Err(format!(
            "nach der letzten Ablehnung ruht dieser Vorschlag noch {} Tage",
            until - day
        ));
    }
    let id = diplomacy.next_proposal;
    diplomacy.next_proposal += 1;
    if matches!(deal, Deal::Treaty(_)) {
        diplomacy.tally.actor(from).bind_proposals += 1;
    }
    diplomacy.proposals.push(Proposal {
        id,
        from,
        to,
        deal,
        made_day: day,
        expires_day: day + data.diplomacy.levers.proposal_days,
    });
    Ok(id)
}

/// Trägt einen erledigten Vorschlag in den Verlauf ein.
pub fn record(diplomacy: &mut Diplomacy, proposal: Proposal, status: ProposalStatus, day: u64) {
    diplomacy.history.push_back(ProposalRecord {
        proposal,
        status,
        day,
    });
    while diplomacy.history.len() > HISTORY {
        diplomacy.history.pop_front();
    }
}

/// Nimmt einen offenen Vorschlag aus der Liste.
pub fn take(diplomacy: &mut Diplomacy, id: u32) -> Option<Proposal> {
    let index = diplomacy.proposals.iter().position(|open| open.id == id)?;
    Some(diplomacy.proposals.remove(index))
}

/// Lässt offene Vorschläge zwischen den beiden fallen, deren Inhalt `drop` trifft (als
/// hinfällig).
pub fn drop_between(
    diplomacy: &mut Diplomacy,
    a: PolityId,
    b: PolityId,
    drop: impl Fn(&Deal) -> bool,
) {
    let gone: Vec<u32> = diplomacy
        .proposals_between(a, b)
        .filter(|open| drop(&open.deal))
        .map(|open| open.id)
        .collect();
    for id in gone {
        if let Some(proposal) = take(diplomacy, id) {
            let day = proposal.made_day;
            record(diplomacy, proposal, ProposalStatus::Void, day);
        }
    }
}

/// Lehnt einen Vorschlag ab und sperrt ihn für die Frist.
pub fn decline(
    diplomacy: &mut Diplomacy,
    data: &GameData,
    proposal: Proposal,
    reason: Option<DeclineReason>,
    day: u64,
) {
    let levers = &data.diplomacy.levers;
    let block = match proposal.deal {
        Deal::Peace(_) => levers.peace_block_days,
        Deal::Merge { .. } => levers.merge_block_days,
        _ => levers.refusal_block_days,
    };
    diplomacy.blocked.retain(|_, until| *until > day);
    diplomacy.blocked.insert(
        (proposal.from, proposal.to, proposal.deal.code()),
        day + block,
    );
    record(diplomacy, proposal, ProposalStatus::Declined(reason), day);
}

/// Täglich: Unbeantwortete Vorschläge verfallen.
pub fn expire(diplomacy: &mut Diplomacy, day: u64) {
    let due: Vec<u32> = diplomacy
        .proposals
        .iter()
        .filter(|open| open.expires_day <= day)
        .map(|open| open.id)
        .collect();
    for id in due {
        if let Some(proposal) = take(diplomacy, id) {
            record(diplomacy, proposal, ProposalStatus::Expired, day);
        }
    }
}

/// Was ein Frieden die Seite `me` kostet und ihr einbringt, grob als Anteil an
/// Bevölkerung und Einnahmen: für die Abwägung der Antwort.
pub fn peace_stakes(
    galaxy: &Galaxy,
    terms: &PeaceTerms,
    me: PolityId,
    data: &GameData,
) -> Answering {
    let levers = &data.diplomacy.levers;
    let weight = |yielder: PolityId| -> f32 {
        let population = galaxy.population_of(yielder).max(1e-6);
        let ceded: f64 = terms
            .cede
            .iter()
            .filter_map(|planet| galaxy.planets.get(planet.index()))
            .map(|planet| planet.population)
            .sum();
        let tribute = f32::from(terms.tribute_years) / f32::from(levers.terms_years.max(1))
            * (levers.reparations as f32 * 3.0);
        (ceded / population) as f32 + tribute + if terms.subjugate { 1.5 } else { 0.0 }
    };
    match terms.yielder.filter(|_| !terms.is_white()) {
        None => Answering::Peace {
            cost: 0.0,
            gain: 0.0,
        },
        Some(yielder) if yielder == me => Answering::Peace {
            cost: weight(me),
            gain: 0.0,
        },
        Some(yielder) => Answering::Peace {
            cost: 0.0,
            gain: weight(yielder),
        },
    }
}

/// Die Antwort, die ein Vorschlag an `me` stellt.
pub fn answering(galaxy: &Galaxy, deal: &Deal, me: PolityId, data: &GameData) -> Answering {
    match deal {
        Deal::Treaty(kind) => Answering::Treaty(*kind),
        Deal::Peace(terms) => peace_stakes(galaxy, terms, me, data),
        Deal::Protectorate { ward } => Answering::Protectorate {
            as_ward: *ward == me,
        },
        Deal::Merge { lead } => Answering::Merge {
            i_lead: *lead == me,
        },
        Deal::Swap { .. } => Answering::Swap,
    }
}

/// Vollzieht einen angenommenen Vorschlag. Die Voraussetzungen werden vorher geprüft
/// ([`check_deal`]); ein abgelehnter Aufruf ändert nichts.
pub fn conclude(succession: &mut Succession, proposal: &Proposal, tick: u64) -> Result<(), String> {
    let data = succession.data;
    let levers = &data.diplomacy.levers;
    check_deal(
        data,
        succession.galaxy,
        succession.polities,
        succession.diplomacy,
        succession.finance,
        proposal.from,
        proposal.to,
        &proposal.deal,
    )?;
    let day = tick / TICKS_PER_DAY;
    let (from, to) = (proposal.from, proposal.to);
    match &proposal.deal {
        Deal::Treaty(kind) => {
            treaties::sign(
                succession.diplomacy,
                succession.polities,
                succession.chronicle,
                tick,
                from,
                to,
                *kind,
            );
            Ok(())
        }
        Deal::Peace(terms) => {
            let truce = data.balance.military.truce_days;
            let yielder = terms.yielder.filter(|_| !terms.is_white());
            let victor = yielder.map(|yielder| if yielder == from { to } else { from });
            // Abtretungen noch im Krieg: Die Welten wechseln als genommen.
            if let Some(victor) = victor {
                let balance = &data.balance.siege;
                for planet in &terms.cede {
                    let terms = TransferTerms {
                        state_shares_to: HolderId::State(victor),
                        state_share: 1.0,
                        cohesion: balance.conquest_base,
                        stage: Stage::Restless,
                        shock: balance.conquest_shock,
                        elite_flight: 0.0,
                        path: Path::Forced,
                    };
                    succession.transfer_planet(*planet, victor, terms, tick)?;
                }
            }
            super::make_peace(
                succession.diplomacy,
                succession.polities,
                succession.chronicle,
                tick,
                from,
                to,
                truce,
            )?;
            // Das Diktat (ohne eigene Nummer) zählt sein Aufrufer.
            if proposal.id != u32::MAX {
                succession.diplomacy.tally.peace(if terms.subjugate {
                    super::PeaceKind::Subjugation
                } else if yielder.is_some() {
                    super::PeaceKind::Terms
                } else {
                    super::PeaceKind::White
                });
            }
            if let (Some(victor), true) = (victor, terms.subjugate) {
                succession.diplomacy.tally.subjugations += 1;
                succession.diplomacy.tally.actor(victor).subjugated += 1;
            }
            if let (Some(yielder), Some(victor)) = (yielder, victor) {
                if terms.tribute_years > 0 {
                    succession.diplomacy.relation_mut(yielder, victor).tribute = Some(Tribute {
                        payer: yielder,
                        share: levers.reparations,
                        until_day: day + u64::from(terms.tribute_years) * DAYS_PER_YEAR,
                    });
                }
                if terms.subjugate {
                    succession.subordinate(yielder, victor, false, tick)?;
                }
                let text = format!(
                    "Friedensbedingungen zwischen {} und {}: {}.",
                    succession.polities.get(victor).name,
                    succession.polities.get(yielder).name,
                    terms.label(succession.polities)
                );
                succession
                    .chronicle
                    .push(tick, ChronicleKind::Diplomacy, Some(victor), None, text);
            }
            Ok(())
        }
        Deal::Protectorate { ward } => {
            let lord = if *ward == from { to } else { from };
            succession.subordinate(*ward, lord, true, tick)?;
            succession.diplomacy.tally.protectorates += 1;
            Ok(())
        }
        Deal::Swap { value } => {
            for (giver, taker) in [(from, to), (to, from)] {
                swap_out(succession, giver, taker, *value as f64);
            }
            Ok(())
        }
        Deal::Merge { lead } => {
            let junior = if *lead == from { to } else { from };
            let player_joins = succession.config.player == Some(junior);
            succession.absorb(junior, *lead, Path::Joined, tick)?;
            succession.diplomacy.tally.merges += 1;
            succession.diplomacy.tally.actor(junior).merges += 1;
            succession.diplomacy.tally.actor(*lead).merges += 1;
            if player_joins {
                succession.endgame.player = PlayerFate::Merged;
            }
            Ok(())
        }
    }
}

/// Lässt einen Schützling frei. Seine Welten gelten danach wieder als eigen.
pub fn release_ward(
    diplomacy: &mut Diplomacy,
    polities: &Polities,
    galaxy: &Galaxy,
    endgame: &mut crate::endgame::Endgame,
    chronicle: &mut Chronicle,
    tick: u64,
    lord: PolityId,
    ward: PolityId,
) -> Result<(), String> {
    if diplomacy.overlord_of(ward) != Some(lord) {
        return Err(format!(
            "{} untersteht nicht {}",
            polities.get(ward).name,
            polities.get(lord).name
        ));
    }
    let day = tick / TICKS_PER_DAY;
    diplomacy.standing[ward.index()] = super::Standing {
        overlord: None,
        voluntary: false,
        since_day: day,
    };
    for world in galaxy.planets_of(ward) {
        endgame.tag(world.id, Path::Own, 0.0, 0.0);
    }
    chronicle.push(
        tick,
        ChronicleKind::Diplomacy,
        Some(lord),
        None,
        format!(
            "{} entlässt {} in die Eigenständigkeit.",
            polities.get(lord).name,
            polities.get(ward).name
        ),
    );
    Ok(())
}

/// Überträgt eigene Anteile des Staates `giver` an Betrieben auf seinem Gebiet an den Staat
/// `taker`, bis `value` Credits erreicht sind oder nichts mehr da ist. Größte Betriebe
/// zuerst, bei Gleichstand die kleinere Nummer.
fn swap_out(succession: &mut Succession, giver: PolityId, taker: PolityId, value: f64) {
    let state = HolderId::State(giver);
    let mut offers: Vec<(f64, crate::ids::IssuerId, u64, f64)> = succession
        .finance
        .issuers
        .iter()
        .filter(|(id, _)| {
            matches!(id, crate::ids::IssuerId::Firm { planet, .. }
                if succession.galaxy.planet(*planet).owner == Some(giver))
        })
        .filter_map(|(id, issuer)| {
            let held = issuer.held_by(state);
            (held > 0 && issuer.price > 0.0).then(|| (issuer.market_value(), *id, held, issuer.price))
        })
        .collect();
    offers.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut left = value;
    for (_, issuer, held, price) in offers {
        if left <= 0.0 {
            break;
        }
        let shares = ((left / price).ceil() as u64).min(held);
        if shares == 0 {
            continue;
        }
        if crate::finance::distribute_shares(
            succession.finance,
            state,
            issuer,
            shares,
            HolderId::State(taker),
        )
        .is_ok()
        {
            left -= shares as f64 * price;
        }
    }
}

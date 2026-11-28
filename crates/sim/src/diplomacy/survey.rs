//! Der Monatsabschluss der Diplomatie.
//!
//! - Je Gemeinschaft: geglättete Einnahmen und ihr Wachstum, Anteile an Wirtschaft,
//!   Flotte und Technik aller, daraus der Machtanteil ([`SurveyEntry`]).
//! - Je Paar: Beteiligungen (`stake`: Anteil am Marktwert der Betriebe des anderen),
//!   Abhängigkeit (`dep`: halb Handel, halb Beteiligungen am eigenen Portfolio),
//!   Verflechtung (`tie`, größenneutral), Groll, Spannung mit ihren Gründen, und wie lange
//!   die Bedingungen einer Vereinigung schon gelten.
//! - Tribut: aus Friedensbedingungen und von Schützlingen und Vasallen an ihr Haupt.
//! - Dossiers verblassen.

use super::Diplomacy;
use super::relations::{Relation, TensionReason, TreatyKind, WarAim, side};
use crate::chronicle::{Chronicle, ChronicleKind};
use crate::clock::{DAYS_PER_MONTH, TICKS_PER_DAY};
use crate::data::GameData;
use crate::economy::{Economy, pay, to_money};
use crate::finance::{Finance, polity_of_holder, polity_of_issuer};
use crate::fleet::Fleets;
use crate::galaxy::Galaxy;
use crate::ids::PolityId;
use crate::logistics::Logistics;
use crate::polity::{Polities, claimable_planets};
use crate::tech::Research;
use serde::{Deserialize, Serialize};

/// Messung einer Gemeinschaft beim letzten Monatsabschluss.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SurveyEntry {
    /// Geglättete Staatseinnahmen im Monat, Credits.
    pub revenue: f32,
    /// Jährliches Wachstum der Einnahmen (Logarithmus), geglättet über `trend_months`.
    pub trend: f32,
    /// Anteil an den Einnahmen aller.
    pub econ_share: f32,
    /// Bauwert der eigenen Flotten, Credits.
    pub fleet_value: f32,
    pub fleet_share: f32,
    /// Erforschte Technologien und ihr Anteil an allen.
    pub techs: f32,
    pub tech_share: f32,
    /// Machtanteil: ½ Wirtschaft + 0,35 Flotte + 0,15 Technik.
    pub power: f32,
    /// Sie kann noch Welten besiedeln.
    pub can_claim: bool,
    /// Monatsabschlüsse in Folge, an denen keine freie Welt mehr in ihrer Reichweite lag
    /// (gleich, was die Kasse hergäbe): die Enge.
    #[serde(default)]
    pub crowded_months: u32,
    /// Knappheit σ: Anteil der Ergiebigkeit ihrer nicht nachwachsenden Vorkommen, der
    /// schon abgebaut ist (0 = unberührt, 1 = erschöpft).
    #[serde(default)]
    pub scarcity: f32,
}

impl SurveyEntry {
    /// Not, wie sie jeder aus öffentlichen Zahlen ablesen kann, 0..1: Schrumpfen (nur
    /// ein Drittel, solange noch Welten frei sind), Enge und Knappheit.
    pub fn want(&self, levers: &crate::data::DiplomacyLevers) -> f32 {
        let mut shrinking = (-self.trend * 10.0).clamp(0.0, 1.0);
        if self.can_claim {
            shrinking *= 0.3;
        }
        let crowding = (self.crowded_months as f32 / (12.0 * levers.crowding_years)).min(1.0);
        (shrinking + levers.crowding_weight * crowding + levers.scarcity_weight * self.scarcity)
            .clamp(0.0, 1.0)
    }
}

/// Knappheit der nicht nachwachsenden Vorkommen auf den Welten einer Gemeinschaft.
pub fn scarcity(data: &GameData, galaxy: &Galaxy, polity: PolityId) -> f32 {
    let (mut rich, mut left) = (0.0f64, 0.0f64);
    for planet in galaxy.planets_of(polity) {
        for deposit in &planet.deposits {
            if data.good(deposit.good).renewable {
                continue;
            }
            rich += deposit.richness as f64;
            left += (deposit.richness * deposit.reserve) as f64;
        }
    }
    if rich > 0.0 {
        (1.0 - left / rich).clamp(0.0, 1.0) as f32
    } else {
        0.0
    }
}

/// Bauwert der Flotten einer Gemeinschaft (ohne Verbündete), Credits.
pub fn fleet_value(data: &GameData, fleets: &Fleets, polity: PolityId) -> f64 {
    fleets
        .list
        .iter()
        .filter(|fleet| fleet.alive && fleet.owner == polity)
        .flat_map(|fleet| &fleet.ships)
        .filter_map(|ship| data.designs.get(ship.design as usize))
        .map(|design| design.stats.value)
        .sum()
}

/// Beteiligungen: `held[a][b]` ist der Marktwert, den Halter von a an Betrieben auf dem
/// Gebiet von b besitzen; `value[b]` der Marktwert aller Betriebe auf dem Gebiet von b.
pub fn holdings(
    finance: &Finance,
    galaxy: &Galaxy,
    logistics: &Logistics,
    count: usize,
) -> (Vec<Vec<f64>>, Vec<f64>) {
    let mut held = vec![vec![0.0f64; count]; count];
    let mut value = vec![0.0f64; count];
    for (id, issuer) in &finance.issuers {
        let Some(seat) =
            polity_of_issuer(galaxy, logistics, *id).filter(|seat| seat.index() < count)
        else {
            continue;
        };
        for (holder, shares) in &issuer.holdings {
            let worth = *shares as f64 * issuer.price;
            value[seat.index()] += worth;
            if let Some(owner) =
                polity_of_holder(galaxy, *holder).filter(|owner| owner.index() < count)
            {
                held[owner.index()][seat.index()] += worth;
            }
        }
    }
    (held, value)
}

/// Handel zwischen den Gebieten: `trade[a][b]` ist die gelieferte Menge zwischen Systemen
/// von a und von b (beide Richtungen), `total[a]` alles, was Systeme von a berührte.
fn trade(galaxy: &Galaxy, logistics: &Logistics, count: usize) -> (Vec<Vec<f64>>, Vec<f64>) {
    let owners: Vec<Option<PolityId>> = galaxy
        .systems
        .iter()
        .map(|system| galaxy.system_owner(system.id))
        .collect();
    let mut trade = vec![vec![0.0f64; count]; count];
    let mut total = vec![0.0f64; count];
    for ((from, to, _), quantity) in &logistics.flows {
        let a = owners
            .get(from.index())
            .copied()
            .flatten()
            .filter(|a| a.index() < count);
        let b = owners
            .get(to.index())
            .copied()
            .flatten()
            .filter(|b| b.index() < count);
        for side in [a, b].into_iter().flatten() {
            total[side.index()] += quantity;
        }
        if let (Some(a), Some(b)) = (a, b)
            && a != b
        {
            trade[a.index()][b.index()] += quantity;
            trade[b.index()][a.index()] += quantity;
        }
    }
    (trade, total)
}

/// Lesender Zugriff des Monatsabschlusses.
pub struct SurveyInputs<'a> {
    pub data: &'a GameData,
    pub galaxy: &'a Galaxy,
    pub polities: &'a Polities,
    pub finance: &'a Finance,
    pub logistics: &'a Logistics,
    pub fleets: &'a Fleets,
    pub research: &'a Research,
}

/// Monatsabschluss: Messungen, Beziehungen, Spannung, Fortschritt zur Vereinigung.
pub fn survey(inputs: &SurveyInputs, diplomacy: &mut Diplomacy, economy: &Economy, day: u64) {
    let SurveyInputs {
        data,
        galaxy,
        polities,
        finance,
        logistics,
        fleets,
        research,
    } = inputs;
    let levers = &data.diplomacy.levers;
    let count = polities.list.len();
    diplomacy.survey.resize(count, SurveyEntry::default());
    let alive: Vec<PolityId> = polities.alive().map(|polity| polity.id).collect();

    // Je Gemeinschaft.
    let mut revenue = vec![0.0f64; count];
    let mut fleet = vec![0.0f64; count];
    let mut techs = vec![0.0f64; count];
    for id in &alive {
        revenue[id.index()] = finance
            .polities
            .get(id.index())
            .map_or(0.0, |state| state.revenue.max(0.0));
        fleet[id.index()] = fleet_value(data, fleets, *id);
        techs[id.index()] = research.known_count(*id) as f64;
    }
    let sum = |values: &[f64]| values.iter().sum::<f64>().max(1e-9);
    let (all_revenue, all_fleet, all_techs) = (sum(&revenue), sum(&fleet), sum(&techs));
    for id in &alive {
        let index = id.index();
        let entry = &mut diplomacy.survey[index];
        let now = revenue[index] as f32;
        if entry.revenue > 0.0 && now > 0.0 {
            let growth = 12.0 * (now / entry.revenue).ln();
            entry.trend += (growth - entry.trend) / levers.trend_months;
        }
        entry.revenue = now;
        entry.econ_share = (revenue[index] / all_revenue) as f32;
        entry.fleet_value = fleet[index] as f32;
        entry.fleet_share = (fleet[index] / all_fleet) as f32;
        entry.techs = techs[index] as f32;
        entry.tech_share = (techs[index] / all_techs) as f32;
        entry.power = 0.5 * entry.econ_share + 0.35 * entry.fleet_share + 0.15 * entry.tech_share;
        entry.can_claim = !claimable_planets(
            galaxy,
            polities,
            &data.balance.colonization,
            economy.treasury(*id),
            *id,
            day,
        )
        .is_empty();
        // Enge: keine freie Welt in Reichweite, gleich was die Kasse hergäbe.
        let room = entry.can_claim
            || !claimable_planets(
                galaxy,
                polities,
                &data.balance.colonization,
                crate::economy::Money::MAX,
                *id,
                day,
            )
            .is_empty();
        entry.crowded_months = if room {
            0
        } else {
            entry.crowded_months.saturating_add(1)
        };
        entry.scarcity = scarcity(data, galaxy, *id);
    }
    for (index, entry) in diplomacy.survey.iter_mut().enumerate() {
        if !polities.list[index].alive {
            *entry = SurveyEntry::default();
        }
    }

    // Je Paar.
    let (held, value) = holdings(finance, galaxy, logistics, count);
    let portfolio: Vec<f64> = held.iter().map(|row| row.iter().sum::<f64>()).collect();
    let (traded, flow_total) = trade(galaxy, logistics, count);
    let conquerors: Vec<PolityId> = diplomacy
        .relations
        .values()
        .filter_map(|relation| relation.war.as_ref())
        .filter(|war| war.aim == WarAim::Subjugation)
        .map(|war| war.attacker)
        .collect();
    for (position, a) in alive.iter().enumerate() {
        for b in &alive[position + 1..] {
            let (a, b) = (*a, *b);
            let (ia, ib) = (a.index(), b.index());
            let ratio = |num: f64, den: f64| if den > 0.0 { (num / den) as f32 } else { 0.0 };
            let aff = diplomacy.affinity(a, b);
            let at_war = diplomacy.at_war(a, b);
            let union_months = diplomacy
                .relation(a, b)
                .and_then(|relation| relation.treaty(TreatyKind::Union))
                .map_or(0, |treaty| {
                    (day.saturating_sub(treaty.since_day) / DAYS_PER_MONTH) as u32
                });
            let conqueror = conquerors.contains(&a) || conquerors.contains(&b);
            let needed = diplomacy.merge_affinity_needed(levers, a, b);
            let relation: &mut Relation = diplomacy.relation_mut(a, b);
            let (sa, sb) = (side(a, b), side(b, a));
            relation.stake[sa] = ratio(held[ia][ib], value[ib]).min(1.0);
            relation.stake[sb] = ratio(held[ib][ia], value[ia]).min(1.0);
            relation.dep[sa] = (0.5 * ratio(traded[ia][ib], flow_total[ia])
                + 0.5 * ratio(held[ia][ib], portfolio[ia]))
            .clamp(0.0, 1.0);
            relation.dep[sb] = (0.5 * ratio(traded[ib][ia], flow_total[ib])
                + 0.5 * ratio(held[ib][ia], portfolio[ib]))
            .clamp(0.0, 1.0);
            relation.tie = ratio(held[ia][ib].min(held[ib][ia]), value[ia].min(value[ib])).min(1.0);
            for grudge in &mut relation.grievance {
                *grudge *= 1.0 - levers.grievance_decay;
                if *grudge < 1e-3 {
                    *grudge = 0.0;
                }
            }

            let mut reasons = vec![(TensionReason::Base, 10.0)];
            if at_war {
                reasons.push((TensionReason::War, 40.0));
            }
            if relation.any_embargo() {
                reasons.push((TensionReason::Embargo, 20.0));
            }
            let grudge = relation.grievance[0].max(relation.grievance[1]);
            if grudge > 0.0 {
                reasons.push((TensionReason::Breach, 30.0 * grudge));
            }
            if conqueror {
                reasons.push((TensionReason::Conqueror, 20.0));
            }
            reasons.push((TensionReason::CulturalDistance, 20.0 * (1.0 - aff)));
            let rungs = relation.rungs();
            if rungs > 0 {
                reasons.push((TensionReason::Treaties, -6.0 * rungs as f32));
            }
            let entangled = (relation.tie / levers.tie_scale).min(1.0);
            if entangled > 0.0 {
                reasons.push((TensionReason::Entanglement, -20.0 * entangled));
            }
            let target: f32 = reasons
                .iter()
                .map(|(_, points)| points)
                .sum::<f32>()
                .clamp(0.0, 100.0);
            relation.tension += levers.tension_adjust * (target - relation.tension);
            relation.tension = relation.tension.clamp(0.0, 100.0);
            relation.reasons = reasons;

            let ready = !at_war
                && union_months >= levers.merge_union_months
                && relation.tie >= levers.merge_tie
                && aff >= needed;
            relation.merge_months = if ready { relation.merge_months + 1 } else { 0 };
        }
    }

    // Gelebte Kultur gleicht sich an, wo man miteinander handelt und verbunden ist: je
    // Abhängigkeit, verstärkt unter Handels- und Unionsvertrag, nie im Krieg oder Embargo.
    if levers.contact_rate > 0.0 {
        let before = diplomacy.culture.clone();
        for a in &alive {
            let Some(own) = before.get(a.index()).copied() else {
                continue;
            };
            let mut shift = [0.0f32; 3];
            for b in alive.iter().filter(|b| *b != a) {
                let Some(relation) = diplomacy.relation(*a, *b) else {
                    continue;
                };
                if relation.war.is_some() || relation.any_embargo() {
                    continue;
                }
                let bond = if relation.has(TreatyKind::Union) {
                    levers.contact_union
                } else if relation.has(TreatyKind::Trade) {
                    levers.contact_trade
                } else {
                    0.0
                };
                let weight = levers.contact_rate * relation.dep[side(*a, *b)] * bond;
                let other = before.get(b.index()).copied().unwrap_or(own);
                for (slot, (mine, theirs)) in shift.iter_mut().zip(own.iter().zip(&other)) {
                    *slot += weight * (theirs - mine);
                }
            }
            if let Some(culture) = diplomacy.culture.get_mut(a.index()) {
                for (value, delta) in culture.iter_mut().zip(shift) {
                    *value = (*value + delta).clamp(0.0, 1.0);
                }
            }
        }
    }

    // Dossiers verblassen.
    for dossier in diplomacy.dossiers.values_mut() {
        dossier.decay(levers);
    }
    for dossier in diplomacy.public_face.values_mut() {
        dossier.decay(levers);
    }
}

/// Monatlich: Tribut aus Friedensbedingungen und von Schützlingen und Vasallen an ihr
/// Haupt, als Anteil der geglätteten Einnahmen, aus der Staatskasse, soweit sie reicht.
pub fn pay_tributes(
    data: &GameData,
    polities: &Polities,
    finance: &Finance,
    diplomacy: &mut Diplomacy,
    economy: &mut Economy,
    chronicle: &mut Chronicle,
    tick: u64,
) {
    let levers = &data.diplomacy.levers;
    let day = tick / TICKS_PER_DAY;
    let revenue = |polity: PolityId| {
        finance
            .polities
            .get(polity.index())
            .map_or(0.0, |state| state.revenue.max(0.0))
    };
    let mut payments: Vec<(PolityId, PolityId, f64)> = Vec::new();
    for ((a, b), relation) in diplomacy.relations.iter_mut() {
        let Some(tribute) = relation.tribute else {
            continue;
        };
        if tribute.until_day < day {
            relation.tribute = None;
            chronicle.push(
                tick,
                ChronicleKind::Diplomacy,
                Some(tribute.payer),
                None,
                format!(
                    "Der Tribut zwischen {} und {} ist abgegolten.",
                    polities.get(*a).name,
                    polities.get(*b).name
                ),
            );
            continue;
        }
        let receiver = if tribute.payer == *a { *b } else { *a };
        payments.push((
            tribute.payer,
            receiver,
            tribute.share * revenue(tribute.payer),
        ));
    }
    for (index, standing) in diplomacy.standing.iter().enumerate() {
        let Some(lord) = standing.overlord else {
            continue;
        };
        let ward = PolityId::from_index(index);
        if !polities.list.get(index).is_some_and(|polity| polity.alive) {
            continue;
        }
        let share = if standing.voluntary {
            levers.protectorate_tribute
        } else {
            levers.vassal_tribute
        };
        payments.push((ward, lord, share * revenue(ward)));
    }
    economy.sync_polities();
    for (payer, receiver, credits) in payments {
        if payer == receiver || credits <= 0.0 {
            continue;
        }
        let amount = to_money(credits);
        let (low, high) = (
            payer.index().min(receiver.index()),
            payer.index().max(receiver.index()),
        );
        let (left, right) = economy.treasuries.split_at_mut(high);
        let (first, second) = (&mut left[low], &mut right[0]);
        if payer.index() == low {
            pay(first, second, amount);
        } else {
            pay(second, first, amount);
        }
    }
}

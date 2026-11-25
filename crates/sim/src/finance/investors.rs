//! Anlageverhalten der Halter: Eliten suchen Rendite im ganzen Reich und ein wenig im
//! Ausland, Fonds kaufen sich in die Betriebe ihrer eigenen Welt ein.
//!
//! Beides zusammen streut den Besitz von selbst, sofern die Einheimischen Geld verdienen.
//! Wo die Gewinne abfließen, bleiben sie arm und können sich nicht einkaufen: Dann hilft
//! nur, dass der Staat Anteile verteilt.

use super::shares::{buy_shares, seat_of_issuer};
use super::{Finance, daily_earnings, polity_of_issuer};
use crate::clock::DAYS_PER_YEAR;
use crate::data::GameData;
use crate::diplomacy::Diplomacy;
use crate::economy::{Economy, Money};
use crate::galaxy::Galaxy;
use crate::ids::{HolderId, IssuerId, PlanetId, PolityId};
use crate::logistics::Logistics;

struct Candidate {
    id: IssuerId,
    polity: PolityId,
    seat: Option<PlanetId>,
    /// Gewinnrendite pro Jahr zum heutigen Kurs.
    earnings_yield: f64,
}

pub fn invest_holders(
    finance: &mut Finance,
    economy: &mut Economy,
    galaxy: &Galaxy,
    logistics: &Logistics,
    diplomacy: &Diplomacy,
    data: &GameData,
    capitals: &[Option<PlanetId>],
) {
    let balance = &data.balance.finance;
    let mut candidates: Vec<Candidate> = finance
        .issuers
        .iter()
        .filter_map(|(id, issuer)| {
            let polity = polity_of_issuer(galaxy, logistics, *id)?;
            let value = issuer.market_value();
            let earnings = daily_earnings(data, economy, logistics, *id) * DAYS_PER_YEAR as f64;
            (value > 1.0 && earnings > 0.0).then(|| Candidate {
                id: *id,
                polity,
                seat: seat_of_issuer(galaxy, logistics, capitals, *id),
                // Risiko drückt die Lust zu kaufen.
                earnings_yield: (earnings / value).min(2.0) * (1.0 - issuer.risk),
            })
        })
        .collect();
    // Beste Rendite zuerst; die ID entscheidet bei Gleichstand.
    candidates.sort_by(|a, b| {
        b.earnings_yield
            .total_cmp(&a.earnings_yield)
            .then(a.id.cmp(&b.id))
    });

    for planet in &galaxy.planets {
        let Some(owner) = planet.owner else {
            continue;
        };
        let index = planet.id.index();

        // Fonds: Wer mehr als das Übliche auf der hohen Kante hat, kauft sich vor Ort ein.
        let households = economy.planets[index].households;
        let comfortable =
            crate::economy::to_money(planet.population * data.balance.economy.cash_per_million);
        if households > comfortable {
            let budget = (households as f64 * balance.public_invest_share) as Money;
            spend(
                finance,
                economy,
                data,
                HolderId::Public(planet.id),
                budget,
                candidates.iter().filter(|c| c.seat == Some(planet.id)),
            );
        }

        // Eliten: der größere Teil im eigenen Reich, bevorzugt in anderen Regionen …
        let elite_cash = finance.elites[index];
        let budget = (elite_cash as f64 * balance.elite_invest_share) as Money;
        let foreign_budget = (budget as f64 * balance.foreign_invest_share) as Money;
        let elite = HolderId::Elite(planet.id);
        let mut domestic: Vec<&Candidate> =
            candidates.iter().filter(|c| c.polity == owner).collect();
        domestic.sort_by(|a, b| {
            let score = |c: &Candidate| {
                c.earnings_yield * if c.seat == Some(planet.id) { 1.0 } else { 1.15 }
            };
            score(b).total_cmp(&score(a)).then(a.id.cmp(&b.id))
        });
        spend(
            finance,
            economy,
            data,
            elite,
            budget - foreign_budget,
            domestic.into_iter(),
        );
        // … der kleinere im Ausland, wo kein Krieg herrscht.
        spend(
            finance,
            economy,
            data,
            elite,
            foreign_budget,
            candidates
                .iter()
                .filter(|c| c.polity != owner && !diplomacy.at_war(c.polity, owner)),
        );
    }
}

/// Gibt ein Budget der Reihe nach für die besten Kandidaten aus, höchstens drei Käufe.
fn spend<'a>(
    finance: &mut Finance,
    economy: &mut Economy,
    data: &GameData,
    buyer: HolderId,
    mut budget: Money,
    candidates: impl Iterator<Item = &'a Candidate>,
) {
    let mut purchases = 0;
    for candidate in candidates {
        if budget < crate::economy::to_money(5.0) || purchases >= 3 {
            break;
        }
        let (bought, paid) = buy_shares(
            finance,
            economy,
            data,
            buyer,
            candidate.id,
            candidate.seat,
            u64::MAX,
            budget,
        );
        if bought > 0 {
            budget -= paid;
            purchases += 1;
        }
    }
}

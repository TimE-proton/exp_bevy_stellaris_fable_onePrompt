//! Anteile: Dividenden, Kapitalerhöhungen, Handel, Übernahmen, Enteignung, Kursbildung.

use super::{
    Finance, Stage, book_value, daily_earnings, holder_cash, holder_cash_now, polity_of_holder,
    polity_of_issuer, region_of_holder, system_of_issuer,
};
use crate::clock::DAYS_PER_YEAR;
use crate::data::GameData;
use crate::diplomacy::Diplomacy;
use crate::economy::{Economy, Money, to_credits, to_money};
use crate::fleet::Fleets;
use crate::galaxy::Galaxy;
use crate::ids::{HolderId, IssuerId, PlanetId, PolityId};
use crate::logistics::Logistics;

/// Region, der die Gewinne eines Emittenten zugerechnet werden.
pub fn seat_of_issuer(
    galaxy: &Galaxy,
    logistics: &Logistics,
    capitals: &[Option<PlanetId>],
    issuer: IssuerId,
) -> Option<PlanetId> {
    match issuer {
        IssuerId::Firm { planet, .. } => Some(planet),
        IssuerId::House(house) => capitals
            .get(logistics.houses[house.index()].polity.index())
            .copied()
            .flatten(),
        IssuerId::Kontor(system) => {
            let owner = galaxy.system_owner(system)?;
            galaxy
                .planets_in(system)
                .filter(|planet| planet.owner == Some(owner))
                .max_by(|a, b| a.population.total_cmp(&b.population).then(b.id.cmp(&a.id)))
                .map(|planet| planet.id)
        }
    }
}

/// Verteilt eine Ausschüttung an die Halter, wo immer sie sitzen. Der Betrag wurde dem
/// Emittenten bereits abgebucht. Was die Region verlässt und was ihr von außen zufließt,
/// wird für den Zusammenhalt mitgeschrieben.
pub fn pay_dividend(
    finance: &mut Finance,
    economy: &mut Economy,
    seat: Option<PlanetId>,
    issuer_id: IssuerId,
    amount: Money,
) {
    if amount <= 0 {
        return;
    }
    let Finance {
        issuers,
        elites,
        regions,
        ..
    } = finance;
    let Some(issuer) = issuers.get_mut(&issuer_id) else {
        // Ohne Anteile gibt es keinen Empfänger: Das Geld bleibt bei den Haushalten vor Ort.
        if let Some(seat) = seat {
            economy.planets[seat.index()].households += amount;
        }
        return;
    };
    issuer.dividend_month += amount;
    let mut paid: Money = 0;
    let mut largest: Option<(HolderId, u64)> = None;
    for (holder, shares) in &issuer.holdings {
        let part = (amount as i128 * *shares as i128 / issuer.shares.max(1) as i128) as Money;
        *holder_cash(economy, elites, *holder) += part;
        paid += part;
        if largest.is_none_or(|(_, most)| *shares > most) {
            largest = Some((*holder, *shares));
        }
        let home = region_of_holder(*holder);
        if home != seat {
            if let Some(seat) = seat {
                regions[seat.index()].dividends_out += to_credits(part);
            }
            if let Some(home) = home {
                regions[home.index()].dividends_in += to_credits(part);
            }
        } else if let Some(seat) = seat {
            regions[seat.index()].dividends_local += to_credits(part);
        }
    }
    // Rundungsrest an den größten Halter: Kein Tausendstel geht verloren.
    if let Some((holder, _)) = largest {
        *holder_cash(economy, elites, holder) += amount - paid;
    }
}

/// Kapitalerhöhung: Der Investor hat bereits eingezahlt und erhält neue Anteile zum Kurs.
pub fn record_injection(
    finance: &mut Finance,
    issuer_id: IssuerId,
    investor: HolderId,
    amount: Money,
) {
    if amount <= 0 {
        return;
    }
    let Some(issuer) = finance.issuers.get_mut(&issuer_id) else {
        return;
    };
    let price = issuer.price.max(0.01);
    let shares = ((to_credits(amount) / price).round() as u64).max(1);
    issuer.shares += shares;
    issuer.add(investor, shares);
}

/// Wie viele Anteile ein Halter im laufenden Handel abgibt. Wer an den Betrieben der
/// eigenen Welt beteiligt ist, trennt sich davon nur zögernd: So wandert der Besitz nicht
/// von selbst in die Hauptstadt.
fn sellable(issuer: &super::Issuer, holder: HolderId, seat: Option<PlanetId>) -> u64 {
    let held = issuer.held_by(holder);
    let offered = issuer.offered.get(&holder).copied().unwrap_or(0).min(held);
    let local = region_of_holder(holder).is_some() && region_of_holder(holder) == seat;
    let willing = match holder {
        // Staaten verkaufen nur, was sie ausdrücklich anbieten.
        HolderId::State(_) => 0,
        HolderId::Elite(_) if local => held / 40,
        HolderId::Elite(_) => held / 12,
        HolderId::Public(_) if local => held / 60,
        HolderId::Public(_) => held / 8,
    };
    offered.max(willing)
}

/// Kauft Anteile am Markt. Verkäufer sind die übrigen Halter in fester Reihenfolge; der
/// Kurs steigt mit der gekauften Menge. Gibt gekaufte Anteile und gezahlten Betrag zurück.
pub fn buy_shares(
    finance: &mut Finance,
    economy: &mut Economy,
    data: &GameData,
    buyer: HolderId,
    issuer_id: IssuerId,
    seat: Option<PlanetId>,
    wanted: u64,
    max_spend: Money,
) -> (u64, Money) {
    let impact = data.balance.finance.price_impact;
    let Finance {
        issuers, elites, ..
    } = finance;
    let Some(issuer) = issuers.get_mut(&issuer_id) else {
        return (0, 0);
    };
    let budget = max_spend
        .min(holder_cash_now(economy, elites, buyer))
        .max(0);
    let sellers: Vec<(HolderId, u64)> = issuer
        .holdings
        .keys()
        .filter(|holder| **holder != buyer)
        .map(|holder| (*holder, sellable(issuer, *holder, seat)))
        .filter(|(_, shares)| *shares > 0)
        .collect();
    let available: u64 = sellers.iter().map(|(_, shares)| shares).sum();
    let total = issuer.shares.max(1) as f64;
    let cost = |quantity: u64| -> f64 {
        let fraction = quantity as f64 / total;
        quantity as f64 * issuer.price * (1.0 + impact * fraction / 2.0)
    };
    // Größte Menge, die Angebot und Geld hergeben (die Kosten wachsen monoton).
    let (mut low, mut high) = (0u64, wanted.min(available));
    while low < high {
        let middle = (low + high).div_ceil(2);
        if to_money(cost(middle)) <= budget {
            low = middle
        } else {
            high = middle - 1
        }
    }
    let quantity = low;
    if quantity == 0 {
        return (0, 0);
    }
    let total_cost = to_money(cost(quantity));
    let mut remaining = quantity;
    let mut paid: Money = 0;
    for (seller, shares) in sellers {
        if remaining == 0 {
            break;
        }
        let sold = issuer.remove(seller, shares.min(remaining));
        if let Some(offered) = issuer.offered.get_mut(&seller) {
            *offered = offered.saturating_sub(sold);
            if *offered == 0 {
                issuer.offered.remove(&seller);
            }
        }
        let proceeds = if sold == remaining {
            total_cost - paid
        } else {
            (total_cost as i128 * sold as i128 / quantity as i128) as Money
        };
        *holder_cash(economy, elites, seller) += proceeds;
        paid += proceeds;
        remaining -= sold;
    }
    *holder_cash(economy, elites, buyer) -= paid;
    issuer.add(buyer, quantity);
    issuer.pressure += impact * quantity as f64 / total;
    (quantity, paid)
}

/// Stellt Anteile zum Verkauf. Käufer bedienen sich daraus im laufenden Handel.
pub fn offer_shares(
    finance: &mut Finance,
    holder: HolderId,
    issuer_id: IssuerId,
    shares: u64,
) -> Result<u64, String> {
    let issuer = finance
        .issuers
        .get_mut(&issuer_id)
        .ok_or("Emittent unbekannt")?;
    let held = issuer.held_by(holder);
    if held == 0 {
        return Err("keine Anteile an diesem Emittenten".into());
    }
    let offered = shares.min(held);
    if offered == 0 {
        issuer.offered.remove(&holder);
    } else {
        issuer.offered.insert(holder, offered);
        // Ein Angebot drückt den Kurs etwas.
        issuer.pressure -= 0.3 * offered as f64 / issuer.shares.max(1) as f64;
    }
    Ok(offered)
}

/// Überträgt Anteile ohne Gegenleistung, etwa vom Staat an den Fonds einer Region.
pub fn distribute_shares(
    finance: &mut Finance,
    from: HolderId,
    issuer_id: IssuerId,
    shares: u64,
    to: HolderId,
) -> Result<u64, String> {
    if from == to {
        return Err("Geber und Empfänger sind dieselben".into());
    }
    let issuer = finance
        .issuers
        .get_mut(&issuer_id)
        .ok_or("Emittent unbekannt")?;
    let moved = issuer.remove(from, shares);
    if moved == 0 {
        return Err("keine Anteile zu verteilen".into());
    }
    issuer.add(to, moved);
    Ok(moved)
}

/// Übernahmeangebot: Der Bieter zahlt einen Aufschlag auf den Kurs. Fonds nehmen ein
/// gutes Angebot rasch an, Eliten verlangen mehr, fremde Staaten verkaufen nie.
pub fn takeover_bid(
    finance: &mut Finance,
    economy: &mut Economy,
    galaxy: &Galaxy,
    bidder: HolderId,
    issuer_id: IssuerId,
    premium: f64,
) -> Result<(u64, Money), String> {
    let premium = premium.clamp(0.0, 2.0);
    let bidder_polity = polity_of_holder(galaxy, bidder);
    let Finance {
        issuers, elites, ..
    } = finance;
    let issuer = issuers.get_mut(&issuer_id).ok_or("Emittent unbekannt")?;
    let offer_price = issuer.price * (1.0 + premium);
    let mut budget = holder_cash_now(economy, elites, bidder).max(0);
    let accepting: Vec<(HolderId, u64)> = issuer
        .holdings
        .iter()
        .filter(|(holder, _)| **holder != bidder)
        .filter_map(|(holder, shares)| {
            let same_polity = polity_of_holder(galaxy, *holder) == bidder_polity;
            let share = match holder {
                HolderId::State(_) => 0.0,
                HolderId::Public(_) if premium >= 0.10 => 0.8,
                HolderId::Elite(_) if same_polity && premium >= 0.25 => 0.7,
                HolderId::Elite(_) if !same_polity && premium >= 0.40 => 0.6,
                _ => 0.0,
            };
            let sold = (*shares as f64 * share) as u64;
            (sold > 0).then_some((*holder, sold))
        })
        .collect();
    let mut acquired = 0;
    let mut paid: Money = 0;
    for (seller, shares) in accepting {
        let affordable = (to_credits(budget) / offer_price.max(1e-9)) as u64;
        let sold = issuer.remove(seller, shares.min(affordable));
        if sold == 0 {
            continue;
        }
        let proceeds = to_money(sold as f64 * offer_price).min(budget);
        *holder_cash(economy, elites, seller) += proceeds;
        budget -= proceeds;
        paid += proceeds;
        acquired += sold;
    }
    if acquired == 0 {
        return Err("niemand nimmt das Angebot an: Aufschlag zu niedrig oder Kasse zu leer".into());
    }
    *holder_cash(economy, elites, bidder) -= paid;
    issuer.add(bidder, acquired);
    issuer.pressure += premium * 0.5 * acquired as f64 / issuer.shares.max(1) as f64;
    Ok((acquired, paid))
}

/// Wen eine Enteignung trifft.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ExpropriationScope {
    /// Nur Halter fremder Gemeinschaften.
    Foreign,
    /// Alle außer dem eigenen Staat.
    Everyone,
}

/// Ergebnis einer Enteignung.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Expropriation {
    pub shares: u64,
    /// Fremde Gemeinschaften, deren Halter getroffen wurden.
    pub wronged: Vec<PolityId>,
}

/// Der Staat zieht Anteile auf seinem Gebiet ohne Entschädigung ein. Das wirkt sofort und
/// kostet dauerhaft: Alle Kurse im Reich fallen, das Misstrauen bleibt über Jahre.
pub fn expropriate(
    finance: &mut Finance,
    galaxy: &Galaxy,
    logistics: &Logistics,
    data: &GameData,
    polity: PolityId,
    issuer_id: IssuerId,
    scope: ExpropriationScope,
) -> Result<Expropriation, String> {
    if polity_of_issuer(galaxy, logistics, issuer_id) != Some(polity) {
        return Err("der Emittent liegt nicht auf eigenem Gebiet".into());
    }
    let state = HolderId::State(polity);
    let issuer = finance
        .issuers
        .get_mut(&issuer_id)
        .ok_or("Emittent unbekannt")?;
    let victims: Vec<(HolderId, u64)> = issuer
        .holdings
        .iter()
        .filter(|(holder, _)| **holder != state)
        .filter(|(holder, _)| {
            scope == ExpropriationScope::Everyone
                || polity_of_holder(galaxy, **holder) != Some(polity)
        })
        .map(|(holder, shares)| (*holder, *shares))
        .collect();
    if victims.is_empty() {
        return Err("es gibt niemanden zu enteignen".into());
    }
    let mut result = Expropriation::default();
    for (holder, shares) in victims {
        let taken = issuer.remove(holder, shares);
        issuer.add(state, taken);
        result.shares += taken;
        if let Some(other) = polity_of_holder(galaxy, holder)
            && other != polity
            && !result.wronged.contains(&other)
        {
            result.wronged.push(other);
        }
    }
    let balance = &data.balance.finance;
    let added = match scope {
        ExpropriationScope::Foreign => balance.expropriation_risk_foreign,
        ExpropriationScope::Everyone => balance.expropriation_risk_everyone,
    };
    let memory = &mut finance.polities[polity.index()].expropriation_risk;
    *memory = (*memory + added).min(0.8);
    // Der Schreck trifft jeden Emittenten im Reich.
    let ids: Vec<IssuerId> = finance.issuers.keys().copied().collect();
    for id in ids {
        if polity_of_issuer(galaxy, logistics, id) == Some(polity)
            && let Some(issuer) = finance.issuers.get_mut(&id)
        {
            issuer.shock *= balance.expropriation_shock;
        }
    }
    Ok(result)
}

/// Risikoabschlag eines Emittenten (0..0,7): Blockade, Krieg, feindliche Flotten,
/// Unruhe in der Region, Misstrauen gegen den Staat.
pub fn risk_discount(
    finance: &Finance,
    galaxy: &Galaxy,
    logistics: &Logistics,
    fleets: &Fleets,
    diplomacy: &Diplomacy,
    capitals: &[Option<PlanetId>],
    issuer: IssuerId,
) -> f64 {
    let Some(polity) = polity_of_issuer(galaxy, logistics, issuer) else {
        return 0.5;
    };
    let system = system_of_issuer(galaxy, logistics, issuer);
    let mut risk = 0.0;
    if fleets.is_blockaded(system) {
        risk += 0.30;
    }
    if diplomacy.is_at_war(polity) {
        risk += 0.08;
    }
    if fleets
        .docked_in(system)
        .any(|fleet| diplomacy.at_war(fleet.owner, polity))
    {
        risk += 0.15;
    }
    if let Some(seat) = seat_of_issuer(galaxy, logistics, capitals, issuer) {
        risk += match finance.regions[seat.index()].stage {
            Stage::Loyal => 0.0,
            Stage::Restless => 0.08,
            Stage::Autonomous => 0.20,
        };
    }
    let state = &finance.polities[polity.index()];
    risk += 0.5 * state.expropriation_risk + 0.3 * state.default_memory;
    risk.clamp(0.0, 0.7)
}

/// Monatliche Kursbildung: Kurse folgen den Gewinnen (Vielfaches des Jahresgewinns),
/// gedrückt durch Risiken und Ereignisse, nie unter einem Bruchteil des Buchwerts.
pub fn reprice(
    finance: &mut Finance,
    data: &GameData,
    galaxy: &Galaxy,
    economy: &Economy,
    logistics: &Logistics,
    fleets: &Fleets,
    diplomacy: &Diplomacy,
    capitals: &[Option<PlanetId>],
) {
    let balance = &data.balance.finance;
    let ids: Vec<IssuerId> = finance.issuers.keys().copied().collect();
    for id in ids {
        let risk = risk_discount(finance, galaxy, logistics, fleets, diplomacy, capitals, id);
        let earnings = daily_earnings(data, economy, logistics, id) * DAYS_PER_YEAR as f64;
        let book = book_value(data, galaxy, economy, logistics, id);
        let issuer = finance.issuers.get_mut(&id).expect("eben gelesen");
        let fundamental = (earnings * balance.earnings_multiple).max(book * balance.book_floor);
        let target = fundamental * (1.0 - risk) * issuer.shock * (1.0 + issuer.pressure).max(0.2)
            / issuer.shares.max(1) as f64;
        issuer.price = (issuer.price + 0.5 * (target - issuer.price)).max(0.01);
        issuer.risk = risk;
        // Schreck und Handelsdruck klingen ab.
        issuer.shock += (1.0 - issuer.shock) * balance.shock_recovery;
        issuer.pressure *= 0.5;
        issuer.dividend += 0.25 * (to_credits(issuer.dividend_month) - issuer.dividend);
        issuer.dividend_month = 0;
    }
    for state in &mut finance.polities {
        state.expropriation_risk *= 1.0 - balance.memory_decay;
        state.default_memory *= 1.0 - balance.memory_decay;
    }
}

/// Ein Ereignis drückt die Kurse aller Emittenten einer Gemeinschaft.
pub fn shock_polity(
    finance: &mut Finance,
    galaxy: &Galaxy,
    logistics: &Logistics,
    polity: PolityId,
    factor: f64,
) {
    let ids: Vec<IssuerId> = finance.issuers.keys().copied().collect();
    for id in ids {
        if polity_of_issuer(galaxy, logistics, id) == Some(polity)
            && let Some(issuer) = finance.issuers.get_mut(&id)
        {
            issuer.shock = (issuer.shock * factor).max(0.2);
        }
    }
}

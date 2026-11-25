//! Staatsanleihen: Geld gegen Zins, im Krieg das Mittel der Wahl. Die verlangte Rendite
//! steigt mit Schuldenlast, Kriegsmüdigkeit und schlechtem Ruf; wer nicht zahlen kann,
//! fällt aus und zahlt mit Vertrauen.

use super::{Finance, holder_cash, holder_cash_now, polity_of_holder, shares::shock_polity};
use crate::chronicle::{Chronicle, ChronicleKind};
use crate::clock::DAYS_PER_YEAR;
use crate::data::GameData;
use crate::diplomacy::Diplomacy;
use crate::economy::{Economy, Money, to_credits, to_money};
use crate::galaxy::Galaxy;
use crate::ids::{HolderId, PolityId};
use crate::logistics::Logistics;
use crate::polity::Polities;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Bond {
    pub issuer: PolityId,
    pub holder: HolderId,
    pub principal: Money,
    /// Jahreszins.
    pub rate: f64,
    /// Tag der Rückzahlung.
    pub matures_on: u64,
}

/// Rendite, die der Markt heute für neue Anleihen der Gemeinschaft verlangt.
pub fn bond_yield(data: &GameData, finance: &Finance, polity: PolityId) -> f64 {
    let balance = &data.balance.finance;
    let state = &finance.polities[polity.index()];
    let debt = to_credits(finance.debt_of(polity));
    let annual_revenue = (state.revenue * 12.0).max(1.0);
    let burden = (debt / (annual_revenue * 3.0)).min(3.0);
    balance.base_yield
        + balance.yield_per_debt * burden
        + balance.yield_per_weariness * state.war_weariness
        + balance.yield_after_default * state.default_memory
        + 0.5 * balance.yield_after_default * state.expropriation_risk
}

/// Gibt Anleihen aus. Käufer sind zuerst die eigenen Eliten und Fonds, dann fremde Eliten,
/// mit denen kein Krieg herrscht. Gibt den platzierten Betrag zurück.
pub fn issue_bonds(
    finance: &mut Finance,
    economy: &mut Economy,
    galaxy: &Galaxy,
    diplomacy: &Diplomacy,
    data: &GameData,
    polity: PolityId,
    amount: Money,
    day: u64,
) -> Result<Money, String> {
    if amount <= 0 {
        return Err("Betrag muss positiv sein".into());
    }
    let balance = &data.balance.finance;
    let rate = bond_yield(data, finance, polity);
    if rate > balance.max_yield {
        return Err(format!(
            "niemand zeichnet: Der Markt verlangte {:.0} % Zins",
            rate * 100.0
        ));
    }
    let trust = 1.0 - finance.polities[polity.index()].default_memory;
    // Je höher der Zins über dem Grundzins, desto größer der Anteil der Kasse, der zeichnet.
    let appetite = ((rate - balance.base_yield * 0.5) * 6.0).clamp(0.05, 0.4) * trust;
    let mut buyers: Vec<(HolderId, f64)> = Vec::new();
    for planet in &galaxy.planets {
        match planet.owner {
            Some(owner) if owner == polity => {
                buyers.push((HolderId::Elite(planet.id), appetite));
                buyers.push((HolderId::Public(planet.id), appetite * 0.4));
            }
            Some(owner) if !diplomacy.at_war(owner, polity) => {
                buyers.push((HolderId::Elite(planet.id), appetite * 0.35));
            }
            _ => {}
        }
    }
    // Eigene zuerst, dann Fremde; innerhalb der Gruppen in fester Reihenfolge.
    buyers.sort_by_key(|(holder, _)| (polity_of_holder(galaxy, *holder) != Some(polity), *holder));
    let mut placed: Money = 0;
    let matures_on = day + balance.bond_years * DAYS_PER_YEAR;
    for (holder, share) in buyers {
        if placed >= amount {
            break;
        }
        let cash = holder_cash_now(economy, &finance.elites, holder);
        let part = ((cash as f64 * share) as Money).min(amount - placed);
        if part < to_money(1.0) {
            continue;
        }
        *holder_cash(economy, &mut finance.elites, holder) -= part;
        economy.treasuries[polity.index()] += part;
        placed += part;
        // Mehrere Zeichnungen desselben Halters am selben Tag werden zusammengelegt.
        match finance.bonds.iter_mut().find(|bond| {
            bond.issuer == polity && bond.holder == holder && bond.matures_on == matures_on
        }) {
            Some(bond) => {
                bond.rate = (bond.rate * bond.principal as f64 + rate * part as f64)
                    / (bond.principal + part) as f64;
                bond.principal += part;
            }
            None => finance.bonds.push(Bond {
                issuer: polity,
                holder,
                principal: part,
                rate,
                matures_on,
            }),
        }
    }
    if placed == 0 {
        return Err("niemand hat Geld für Anleihen übrig".into());
    }
    Ok(placed)
}

/// Monatlicher Schuldendienst: Zinsen, Rückzahlung bei Fälligkeit, Ausfall bei leerer Kasse.
pub fn service_bonds(
    finance: &mut Finance,
    economy: &mut Economy,
    galaxy: &Galaxy,
    logistics: &Logistics,
    polities: &Polities,
    chronicle: &mut Chronicle,
    data: &GameData,
    tick: u64,
) {
    let balance = &data.balance.finance;
    let day = tick / crate::clock::TICKS_PER_DAY;
    let mut defaulted: Vec<PolityId> = Vec::new();
    let mut bonds = std::mem::take(&mut finance.bonds);
    for bond in &mut bonds {
        let treasury = economy.treasuries[bond.issuer.index()].max(0);
        let interest = (bond.principal as f64 * bond.rate / 12.0) as Money;
        let due = interest
            + if day >= bond.matures_on {
                bond.principal
            } else {
                0
            };
        let paid = due.min(treasury);
        economy.treasuries[bond.issuer.index()] -= paid;
        *holder_cash(economy, &mut finance.elites, bond.holder) += paid;
        if day >= bond.matures_on {
            // Was über die Zinsen hinaus gezahlt wurde, tilgt.
            bond.principal -= (paid - interest).clamp(0, bond.principal);
        }
        if paid < due {
            // Ausfall: Die Gläubiger verlieren einen Teil ihrer Forderung, die Laufzeit
            // wird gestreckt.
            bond.principal = (bond.principal as f64 * (1.0 - balance.default_haircut)) as Money;
            bond.matures_on = bond.matures_on.max(day) + 5 * DAYS_PER_YEAR;
            if !defaulted.contains(&bond.issuer) {
                defaulted.push(bond.issuer);
            }
        }
    }
    bonds.retain(|bond| bond.principal > 0);
    finance.bonds = bonds;
    for polity in defaulted {
        let state = &mut finance.polities[polity.index()];
        state.default_memory = 1.0;
        state.defaults += 1;
        shock_polity(finance, galaxy, logistics, polity, balance.default_shock);
        chronicle.push(
            tick,
            ChronicleKind::Finance,
            Some(polity),
            None,
            format!(
                "{} kann seine Anleihen nicht bedienen. Die Gläubiger verlieren {:.0} %.",
                polities.get(polity).name,
                balance.default_haircut * 100.0
            ),
        );
    }
}

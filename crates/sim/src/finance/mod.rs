//! Börse, Besitz und Zusammenhalt.
//!
//! Firmen, Werften, Handelshäuser und Kontore geben Anteile aus. Halter sind Staaten,
//! regionale Eliten und Bevölkerungsfonds. Dividenden fließen an die Halter, wo immer sie
//! sitzen. Daraus ergibt sich die Kernmechanik: Ein Reich hält zusammen, wenn seine
//! Regionen aneinander beteiligt sind, und zerfällt, wenn Besitz sich im Zentrum oder bei
//! Fremden sammelt und die Gewinne einseitig abfließen.

mod bonds;
mod cohesion;
mod investors;
mod plugin;
pub mod shares;

pub use bonds::{Bond, bond_yield, issue_bonds, service_bonds};
pub use cohesion::{
    CohesionFactors, Region, Secession, Stage, apply_secession, cohesion_target, issuers_of_region,
    ownership_of_region, update_cohesion,
};
pub use investors::invest_holders;
pub use plugin::{EquityFlows, FinancePlugin};
pub use shares::{
    buy_shares, distribute_shares, expropriate, move_holdings, offer_shares, pass_holdings,
    pay_dividend, record_injection, reprice, takeover_bid,
};

use crate::data::{GameData, Sector};
use crate::economy::{Economy, Money, to_credits};
use crate::galaxy::Galaxy;
use crate::ids::{HolderId, IssuerId, PlanetId, PolityId, SystemId};
use crate::logistics::Logistics;
use crate::polity::Polities;
use bevy_ecs::prelude::Resource;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Anteile, mit denen ein neuer Emittent beginnt.
pub const FOUNDING_SHARES: u64 = 1000;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Issuer {
    /// Ausgegebene Anteile insgesamt.
    pub shares: u64,
    pub holdings: BTreeMap<HolderId, u64>,
    /// Anteile, die der jeweilige Halter zum Verkauf gestellt hat.
    pub offered: BTreeMap<HolderId, u64>,
    /// Kurs je Anteil in Credits.
    pub price: f64,
    /// Geglättete Dividende je Monat in Credits (alle Anteile zusammen).
    pub dividend: f64,
    /// Dividende des laufenden Monats.
    pub dividend_month: Money,
    /// Stimmung: Ereignisse drücken den Kurs, die Wirkung klingt ab (1 = neutral).
    pub shock: f64,
    /// Kursaufschlag durch Käufe bzw. Abschlag durch Verkäufe, klingt ab.
    pub pressure: f64,
    /// Zuletzt berechneter Risikoabschlag (0..1), zur Anzeige.
    pub risk: f64,
}

impl Issuer {
    pub fn held_by(&self, holder: HolderId) -> u64 {
        self.holdings.get(&holder).copied().unwrap_or(0)
    }

    pub fn share_of(&self, holder: HolderId) -> f64 {
        if self.shares == 0 {
            0.0
        } else {
            self.held_by(holder) as f64 / self.shares as f64
        }
    }

    pub fn market_value(&self) -> f64 {
        self.shares as f64 * self.price
    }

    /// Dividendenrendite pro Jahr zum heutigen Kurs.
    pub fn dividend_yield(&self) -> f64 {
        let value = self.market_value();
        if value <= 0.0 {
            0.0
        } else {
            self.dividend * 12.0 / value
        }
    }

    fn add(&mut self, holder: HolderId, shares: u64) {
        if shares > 0 {
            *self.holdings.entry(holder).or_default() += shares;
        }
    }

    /// Nimmt einem Halter Anteile; gibt zurück, wie viele es wirklich waren.
    fn remove(&mut self, holder: HolderId, shares: u64) -> u64 {
        let Some(held) = self.holdings.get_mut(&holder) else {
            return 0;
        };
        let taken = shares.min(*held);
        *held -= taken;
        if *held == 0 {
            self.holdings.remove(&holder);
        }
        if let Some(offered) = self.offered.get_mut(&holder) {
            *offered = (*offered).min(self.holdings.get(&holder).copied().unwrap_or(0));
            if *offered == 0 {
                self.offered.remove(&holder);
            }
        }
        taken
    }
}

/// Finanzlage einer Gemeinschaft.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PolityFinance {
    /// Nachwirkung von Enteignungen (0..1): drückt Kurse und Zusammenhalt, klingt ab.
    pub expropriation_risk: f64,
    /// Nachwirkung eines Zahlungsausfalls (0..1).
    pub default_memory: f64,
    /// Kriegsmüdigkeit 0..100.
    pub war_weariness: f64,
    /// Rendite, die Käufer heute für neue Anleihen verlangen.
    pub bond_yield: f64,
    /// Geglättete Monatseinnahmen des Staates in Credits: die laufenden Einnahmen der
    /// abgeschlossenen Monate (`Economy::state_income_of`), an denen der Markt die
    /// Schuldenlast misst.
    pub revenue: f64,
    /// Staatskasse am letzten Monatsende. Nur noch Auskunft; die Einnahmen werden nicht
    /// mehr daraus geschätzt.
    pub last_treasury: Money,
    pub defaults: u32,
}

#[derive(Resource, Clone, Debug, Default, Serialize, Deserialize)]
pub struct Finance {
    pub issuers: BTreeMap<IssuerId, Issuer>,
    /// Geld der Elite je Planet.
    pub elites: Vec<Money>,
    /// Je Planet: Zusammenhalt und Besitzlage.
    pub regions: Vec<Region>,
    /// Je Gemeinschaft.
    pub polities: Vec<PolityFinance>,
    pub bonds: Vec<Bond>,
    pub secessions: u32,
}

impl Finance {
    pub fn money(&self) -> Money {
        self.elites.iter().sum()
    }

    pub fn issuer(&self, id: IssuerId) -> Option<&Issuer> {
        self.issuers.get(&id)
    }

    pub fn region(&self, planet: PlanetId) -> &Region {
        &self.regions[planet.index()]
    }

    pub fn polity(&self, polity: PolityId) -> &PolityFinance {
        &self.polities[polity.index()]
    }

    /// Schulden einer Gemeinschaft aus laufenden Anleihen.
    pub fn debt_of(&self, polity: PolityId) -> Money {
        self.bonds
            .iter()
            .filter(|bond| bond.issuer == polity)
            .map(|bond| bond.principal)
            .sum()
    }

    /// Wert aller Anteile und Anleihen eines Halters in Credits.
    pub fn portfolio_value(&self, holder: HolderId) -> f64 {
        let shares: f64 = self
            .issuers
            .values()
            .map(|issuer| issuer.held_by(holder) as f64 * issuer.price)
            .sum();
        let bonds: f64 = self
            .bonds
            .iter()
            .filter(|bond| bond.holder == holder)
            .map(|bond| to_credits(bond.principal))
            .sum();
        shares + bonds
    }
}

/// Gemeinschaft, zu der ein Halter heute gehört.
pub fn polity_of_holder(galaxy: &Galaxy, holder: HolderId) -> Option<PolityId> {
    match holder {
        HolderId::State(polity) => Some(polity),
        HolderId::Elite(planet) | HolderId::Public(planet) => galaxy.planet(planet).owner,
    }
}

/// Region (Planet), in der ein Halter sitzt; Staaten sitzen in keiner.
pub fn region_of_holder(holder: HolderId) -> Option<PlanetId> {
    match holder {
        HolderId::State(_) => None,
        HolderId::Elite(planet) | HolderId::Public(planet) => Some(planet),
    }
}

/// System, in dem ein Emittent wirtschaftet. Handelshäuser sitzen im Heimathafen.
pub fn system_of_issuer(galaxy: &Galaxy, logistics: &Logistics, issuer: IssuerId) -> SystemId {
    match issuer {
        IssuerId::Firm { planet, .. } => galaxy.planet(planet).system,
        IssuerId::House(house) => logistics.houses[house.index()].home,
        IssuerId::Kontor(system) => system,
    }
}

/// Gemeinschaft, auf deren Gebiet ein Emittent wirtschaftet.
pub fn polity_of_issuer(
    galaxy: &Galaxy,
    logistics: &Logistics,
    issuer: IssuerId,
) -> Option<PolityId> {
    match issuer {
        IssuerId::Firm { planet, .. } => galaxy.planet(planet).owner,
        IssuerId::House(house) => Some(logistics.houses[house.index()].polity),
        IssuerId::Kontor(system) => galaxy.system_owner(system),
    }
}

/// Name eines Emittenten für Listen und Chronik.
pub fn issuer_name(galaxy: &Galaxy, logistics: &Logistics, issuer: IssuerId) -> String {
    match issuer {
        IssuerId::Firm { planet, sector } => {
            let noun = match sector {
                Sector::Mining => "Bergbau",
                Sector::Agrar => "Agrar",
                Sector::Industry => "Werke",
                Sector::Arms => "Werft",
            };
            format!("{} {}", galaxy.planet(planet).name, noun)
        }
        IssuerId::House(house) => logistics.houses[house.index()].name.clone(),
        IssuerId::Kontor(system) => format!("Kontor {}", galaxy.system(system).name),
    }
}

/// Name eines Halters.
pub fn holder_name(galaxy: &Galaxy, polities: &Polities, holder: HolderId) -> String {
    match holder {
        HolderId::State(polity) => format!("Staat {}", polities.get(polity).short),
        HolderId::Elite(planet) => format!("Elite {}", galaxy.planet(planet).name),
        HolderId::Public(planet) => format!("Fonds {}", galaxy.planet(planet).name),
    }
}

/// Kasse eines Halters.
pub fn holder_cash<'a>(
    economy: &'a mut Economy,
    elites: &'a mut [Money],
    holder: HolderId,
) -> &'a mut Money {
    match holder {
        HolderId::State(polity) => &mut economy.treasuries[polity.index()],
        HolderId::Elite(planet) => &mut elites[planet.index()],
        HolderId::Public(planet) => &mut economy.planets[planet.index()].households,
    }
}

pub fn holder_cash_now(economy: &Economy, elites: &[Money], holder: HolderId) -> Money {
    match holder {
        HolderId::State(polity) => economy.treasuries[polity.index()],
        HolderId::Elite(planet) => elites[planet.index()],
        HolderId::Public(planet) => economy.planets[planet.index()].households,
    }
}

/// Buchwert eines Emittenten in Credits: Sachkapital und Kasse.
pub fn book_value(
    data: &GameData,
    _galaxy: &Galaxy,
    economy: &Economy,
    logistics: &Logistics,
    issuer: IssuerId,
) -> f64 {
    let balance = &data.balance;
    match issuer {
        IssuerId::Firm { planet, sector } => {
            let local = economy.planet(planet);
            // Zum Richtpreis bewertet: Ein Preissprung bei Maschinen (etwa unter Blockade)
            // macht die Werke nicht wertvoller.
            let machines = data.good(balance.economy.capital_good_id).base_price;
            let capital: f64 = crate::economy::recipes_of(data, sector)
                .map(|(index, _)| local.capital[index])
                .sum();
            capital * balance.economy.capital_cost * machines
                + to_credits(local.firms[sector.index()].cash)
        }
        IssuerId::House(house) => {
            let house = &logistics.houses[house.index()];
            let ships = logistics
                .freighters
                .iter()
                .filter(|freighter| freighter.alive && freighter.house == house.id)
                .count() as f64;
            ships * 600.0 + to_credits(house.cash)
        }
        IssuerId::Kontor(system) => {
            let market = economy.market(system);
            let stock: f64 = market
                .stock
                .iter()
                .zip(&data.goods)
                .map(|(stock, good)| stock * good.base_price)
                .sum();
            to_credits(market.cash) + 0.5 * stock
        }
    }
}

/// Geglätteter Tagesgewinn eines Emittenten in Credits.
pub fn daily_earnings(
    data: &GameData,
    economy: &Economy,
    logistics: &Logistics,
    issuer: IssuerId,
) -> f64 {
    match issuer {
        IssuerId::Firm { planet, sector } => economy.planet(planet).firms[sector.index()].profit,
        IssuerId::House(house) => logistics.houses[house.index()].profit,
        // Das Kontor verdient die Handelsspanne auf beiden Seiten seines Umsatzes.
        IssuerId::Kontor(system) => economy.market(system).turnover * data.balance.economy.spread,
    }
}

/// Legt die Börse zu Spielbeginn an: Eliten erhalten einen Teil des Geldes ihrer Welt,
/// jede Firma der Heimatwelten bekommt ihre Gründer.
pub fn setup_finance(
    data: &GameData,
    galaxy: &Galaxy,
    polities: &Polities,
    economy: &mut Economy,
    logistics: &Logistics,
) -> Finance {
    let mut finance = Finance {
        issuers: BTreeMap::new(),
        elites: vec![0; galaxy.planets.len()],
        regions: galaxy.planets.iter().map(|_| Region::default()).collect(),
        polities: polities
            .list
            .iter()
            .map(|polity| PolityFinance {
                last_treasury: economy.treasury(polity.id),
                bond_yield: data.balance.finance.base_yield,
                ..Default::default()
            })
            .collect(),
        bonds: Vec::new(),
        secessions: 0,
    };
    for planet in &galaxy.planets {
        if planet.owner.is_some() {
            // Die Elite hält anfangs einen festen Anteil des Geldes ihrer Welt.
            let households = &mut economy.planets[planet.id.index()].households;
            let share = (*households as f64 * data.balance.finance.elite_cash_share) as Money;
            *households -= share;
            finance.elites[planet.id.index()] = share;
        }
    }
    let capitals: Vec<Option<PlanetId>> = polities.list.iter().map(|p| p.capital).collect();
    found_missing_issuers(data, galaxy, economy, logistics, &capitals, &mut finance);
    for region in finance.regions.iter_mut() {
        region.cohesion = 70.0;
    }
    finance
}

/// Jeder Emittent, der wirtschaftet, aber noch keine Anteile hat, wird gegründet. Anfangs
/// halten überall vor allem die Einheimischen. In Kolonien verschiebt sich das, sobald
/// das Zentrum Kapital nachschießt und dafür neue Anteile erhält: Gerade die
/// erfolgreichen Kolonien gehören dann zunehmend der Hauptwelt, ihre Gewinne fließen ab,
/// und der Druck entsteht, den der Spieler durch Streuung des Besitzes abbauen muss.
pub fn found_missing_issuers(
    data: &GameData,
    galaxy: &Galaxy,
    economy: &Economy,
    logistics: &Logistics,
    capitals: &[Option<PlanetId>],
    finance: &mut Finance,
) {
    let founders = |owner: PolityId, planet: Option<PlanetId>| -> Vec<(HolderId, u64)> {
        let capital = capitals.get(owner.index()).copied().flatten();
        match (planet, capital) {
            // Kolonie: Die Siedler halten die Mehrheit an dem, was sie mitgebracht haben;
            // der Staat hält den Anteil, den seine Gründungshilfe wert war, die Elite der
            // Hauptwelt einen kleinen. Was das Zentrum später an Kapital nachschießt,
            // bekommt es in neuen Anteilen: So wächst sein Besitz mit dem Erfolg der Kolonie.
            (Some(planet), Some(capital)) if planet != capital => vec![
                (HolderId::State(owner), 300),
                (HolderId::Elite(capital), 100),
                (HolderId::Elite(planet), 150),
                (HolderId::Public(planet), 450),
            ],
            // Heimatwelt oder reichsweiter Emittent: vor allem die Einheimischen.
            (_, Some(capital)) => vec![
                (HolderId::Elite(capital), 500),
                (HolderId::Public(capital), 300),
                (HolderId::State(owner), 200),
            ],
            (Some(planet), None) => {
                vec![
                    (HolderId::Elite(planet), 400),
                    (HolderId::Public(planet), 600),
                ]
            }
            (None, None) => vec![(HolderId::State(owner), FOUNDING_SHARES)],
        }
    };
    let found = |finance: &mut Finance, id: IssuerId, owner: PolityId, planet: Option<PlanetId>| {
        if finance.issuers.contains_key(&id) {
            return;
        }
        let mut issuer = Issuer {
            shares: FOUNDING_SHARES,
            shock: 1.0,
            ..Default::default()
        };
        for (holder, shares) in founders(owner, planet) {
            issuer.add(holder, shares);
        }
        let book = book_value(data, galaxy, economy, logistics, id);
        issuer.price = (book / FOUNDING_SHARES as f64).max(0.01);
        finance.issuers.insert(id, issuer);
    };

    for planet in &galaxy.planets {
        let Some(owner) = planet.owner else {
            continue;
        };
        let local = economy.planet(planet.id);
        for sector in Sector::ALL {
            let active = local.firms[sector.index()].cash > 0
                || crate::economy::recipes_of(data, sector)
                    .any(|(index, _)| local.capital[index] > 0.0);
            if active {
                found(
                    finance,
                    IssuerId::Firm {
                        planet: planet.id,
                        sector,
                    },
                    owner,
                    Some(planet.id),
                );
            }
        }
    }
    for house in &logistics.houses {
        found(finance, IssuerId::House(house.id), house.polity, None);
    }
    for system in &galaxy.systems {
        let Some(owner) = galaxy.system_owner(system.id) else {
            continue;
        };
        // Das Kontor gehört zur bevölkerungsreichsten Welt des Systems.
        let seat = galaxy
            .planets_in(system.id)
            .filter(|planet| planet.owner == Some(owner))
            .max_by(|a, b| a.population.total_cmp(&b.population).then(b.id.cmp(&a.id)))
            .map(|planet| planet.id);
        found(finance, IssuerId::Kontor(system.id), owner, seat);
    }
}

/// Regelmäßige Abgabe der Eliten an die Haushalte ihrer Welt (Dienste, Bauten, Feste):
/// Auch gehortetes Geld läuft wieder um.
pub fn elite_spending(galaxy: &Galaxy, economy: &mut Economy, finance: &mut Finance, share: f64) {
    for planet in &galaxy.planets {
        let index = planet.id.index();
        let amount = (finance.elites[index] as f64 * share) as Money;
        if amount <= 0 {
            continue;
        }
        finance.elites[index] -= amount;
        if planet.owner.is_some() {
            economy.planets[index].households += amount;
        } else {
            // Verlassene Welt: Das Geld folgt der Elite nirgendwohin; es bleibt liegen.
            finance.elites[index] += amount;
        }
    }
}

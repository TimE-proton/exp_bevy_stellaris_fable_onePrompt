//! Wirtschaft: Produktion, lokale Preise, Löhne, Kapital.
//!
//! Drei Zeitskalen halten das System ruhig: Preise reagieren täglich, Frachter in Wochen,
//! Kapital wandert monatlich. Geld ist ganzzahlig und wird nur verschoben; neu entsteht es
//! allein durch die ausgewiesene Geldschöpfung für Neugeborene (`minted`).

mod capital;
mod plugin;
pub mod production;
mod setup;

pub use capital::{invest_and_depreciate, migrate, settle_public_accounts};
pub use plugin::{EconomyPlugin, ProductionStep};
pub use production::{
    abandon_dead_colonies, grow_population, produce_and_consume, target_price,
    update_prices_and_wages,
};
pub use setup::{seed_colony, setup_economy};

use crate::data::{EconomyBalance, GameData, RecipeDef, Sector, YieldSource};
use crate::galaxy::Planet;
use crate::ids::{GoodId, PlanetId, PolityId, SystemId};
use bevy_ecs::prelude::Resource;
use serde::{Deserialize, Serialize};

/// Geld in Tausendstel Credits. Ganzzahlig, damit die Summe exakt prüfbar bleibt.
pub type Money = i64;

#[inline]
pub fn to_money(credits: f64) -> Money {
    (credits * 1000.0).round() as Money
}

#[inline]
pub fn to_credits(money: Money) -> f64 {
    money as f64 / 1000.0
}

/// Überweist höchstens `amount`, nie mehr als vorhanden. Gibt den gezahlten Betrag zurück.
#[inline]
pub fn pay(from: &mut Money, to: &mut Money, amount: Money) -> Money {
    let moved = amount.clamp(0, (*from).max(0));
    *from -= moved;
    *to += moved;
    moved
}

/// Das Kontor lagert höchstens dieses Vielfache der Soll-Reichweite.
pub const STOCK_CAP_FACTOR: f64 = 2.5;
/// Mindestlager je Gut, damit auch Märkte ohne eigene Nachfrage exportieren können.
pub const STOCK_CAP_FLOOR: f64 = 60.0;

/// Markt eines Sternsystems. Das Kontor kauft und verkauft zum lokalen Preis mit Spanne.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Market {
    pub stock: Vec<f64>,
    pub price: Vec<f64>,
    /// Geglättete Tagesnachfrage (auch die, die mangels Ware unerfüllt blieb).
    pub demand: Vec<f64>,
    /// Geglätteter Tageszufluss aus Produktion und Lieferungen.
    pub supply: Vec<f64>,
    pub demand_today: Vec<f64>,
    pub supply_today: Vec<f64>,
    /// Tägliche Nachfrage aus Monatsentscheidungen (Investitionen, Frachterbau). Wird
    /// monatlich neu gesetzt und zählt bis dahin jeden Tag.
    pub standing: Vec<f64>,
    /// Kasse des Kontors.
    pub cash: Money,
    /// Geglätteter Tagesumsatz in Credits.
    pub turnover: f64,
    pub turnover_today: f64,
}

impl Market {
    fn new(data: &GameData) -> Self {
        let goods = data.goods.len();
        Self {
            stock: vec![0.0; goods],
            price: data.goods.iter().map(|good| good.base_price).collect(),
            demand: vec![0.0; goods],
            supply: vec![0.0; goods],
            demand_today: vec![0.0; goods],
            supply_today: vec![0.0; goods],
            standing: vec![0.0; goods],
            cash: 0,
            turnover: 0.0,
            turnover_today: 0.0,
        }
    }

    #[inline]
    pub fn price_of(&self, good: GoodId) -> f64 {
        self.price[good.index()]
    }

    #[inline]
    pub fn stock_of(&self, good: GoodId) -> f64 {
        self.stock[good.index()]
    }

    /// Wie viel das Kontor von einem Gut noch annimmt. Über dem Mehrfachen der
    /// Soll-Reichweite kauft es nichts mehr: Überproduktion bleibt beim Erzeuger und
    /// drückt dessen Auslastung, statt die Kasse des Kontors zu leeren.
    #[inline]
    pub fn room(&self, good: GoodId, cover_days: f64) -> f64 {
        let cap = (self.demand[good.index()] * cover_days * STOCK_CAP_FACTOR).max(STOCK_CAP_FLOOR);
        (cap - self.stock[good.index()]).max(0.0)
    }

    /// Preis, den ein Käufer zahlt.
    #[inline]
    pub fn ask(&self, good: GoodId, spread: f64) -> f64 {
        self.price[good.index()] * (1.0 + spread)
    }

    /// Preis, den ein Verkäufer erhält.
    #[inline]
    pub fn bid(&self, good: GoodId, spread: f64) -> f64 {
        self.price[good.index()] * (1.0 - spread)
    }
}

/// Firma eines Wirtschaftszweigs auf einem Planeten.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Firm {
    pub cash: Money,
    /// Geglätteter Tagesgewinn in Credits.
    pub profit: f64,
    /// Geglätteter Tagesumsatz in Credits.
    pub revenue: f64,
    /// Gewinn und Umsatz des laufenden Tages.
    pub profit_today: f64,
    pub revenue_today: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PlanetEconomy {
    /// Kapitalstock je Rezept.
    pub capital: Vec<f64>,
    /// Ausstoß des letzten Tages je Rezept.
    pub output: Vec<f64>,
    /// Auslastung des letzten Tages je Rezept (0..1).
    pub utilization: Vec<f64>,
    /// Geglättete Auslastung je Rezept.
    pub utilization_trend: Vec<f64>,
    /// Geglättete Jahresrendite auf den Kapitalstock je Rezept.
    pub roc: Vec<f64>,
    pub firms: [Firm; 4],
    /// Geld der Haushalte.
    pub households: Money,
    /// Lohn je Mio. Arbeitskräfte und Tag.
    pub wage: f64,
    /// Anteil der Stellen, die besetzt sind (0..1).
    pub employment: f64,
    /// Arbeitskräftebedarf bei voller Auslastung.
    pub labor_demand: f64,
    /// Geglättete Deckung je Bedarf aus `balance.ron` (0..1).
    pub needs_met: Vec<f64>,
    /// Geglättetes Tageseinkommen der Haushalte.
    pub income: f64,
    pub income_today: f64,
    /// Tag der Besiedlung (0 für Heimatwelten).
    pub founded_day: u64,
}

impl PlanetEconomy {
    pub fn new(data: &GameData) -> Self {
        let recipes = data.recipes.len();
        Self {
            capital: vec![0.0; recipes],
            output: vec![0.0; recipes],
            utilization: vec![0.0; recipes],
            utilization_trend: vec![1.0; recipes],
            roc: vec![0.0; recipes],
            firms: Default::default(),
            households: 0,
            wage: data.balance.economy.base_wage,
            employment: 1.0,
            labor_demand: 0.0,
            needs_met: vec![1.0; data.balance.economy.needs.len()],
            income: 0.0,
            income_today: 0.0,
            founded_day: 0,
        }
    }

    pub fn total_capital(&self) -> f64 {
        self.capital.iter().sum()
    }

    /// Deckung der lebensnotwendigen Bedarfe (das Minimum über alle).
    pub fn essentials_met(&self, balance: &EconomyBalance) -> f64 {
        balance
            .needs
            .iter()
            .zip(&self.needs_met)
            .filter(|(need, _)| need.essential)
            .map(|(_, met)| *met)
            .fold(1.0, f64::min)
    }
}

#[derive(Resource, Clone, Debug, Default, Serialize, Deserialize)]
pub struct Economy {
    /// Je Sternsystem.
    pub markets: Vec<Market>,
    /// Je Planet.
    pub planets: Vec<PlanetEconomy>,
    /// Staatskasse je Gemeinschaft.
    pub treasuries: Vec<Money>,
    /// Geldmenge bei Spielbeginn (alle Konten, auch die der Handelshäuser).
    pub initial_money: Money,
    /// Seit Spielbeginn ausgewiesene Geldschöpfung.
    pub minted: Money,
}

impl Economy {
    #[inline]
    pub fn market(&self, system: SystemId) -> &Market {
        &self.markets[system.index()]
    }

    #[inline]
    pub fn planet(&self, planet: PlanetId) -> &PlanetEconomy {
        &self.planets[planet.index()]
    }

    #[inline]
    pub fn treasury(&self, polity: PolityId) -> Money {
        self.treasuries.get(polity.index()).copied().unwrap_or(0)
    }

    /// Summe aller Konten der Wirtschaft (ohne Handelshäuser).
    pub fn money_in_accounts(&self) -> Money {
        let markets: Money = self.markets.iter().map(|market| market.cash).sum();
        let planets: Money = self
            .planets
            .iter()
            .map(|planet| {
                planet.households + planet.firms.iter().map(|firm| firm.cash).sum::<Money>()
            })
            .sum();
        let states: Money = self.treasuries.iter().sum();
        markets + planets + states
    }

    /// Prüft, dass kein Wert entgleist ist. Gibt die erste Auffälligkeit zurück.
    pub fn find_anomaly(&self) -> Option<String> {
        for (index, market) in self.markets.iter().enumerate() {
            for good in 0..market.stock.len() {
                let (stock, price) = (market.stock[good], market.price[good]);
                if !stock.is_finite() || stock < -1e-6 {
                    return Some(format!("Markt {index}, Gut {good}: Bestand {stock}"));
                }
                if !price.is_finite() || price <= 0.0 {
                    return Some(format!("Markt {index}, Gut {good}: Preis {price}"));
                }
            }
            if market.cash < 0 {
                return Some(format!("Markt {index}: Kasse negativ"));
            }
        }
        for (index, planet) in self.planets.iter().enumerate() {
            if planet.households < 0 || planet.firms.iter().any(|firm| firm.cash < 0) {
                return Some(format!("Planet {index}: Konto negativ"));
            }
            if !planet.wage.is_finite() || planet.capital.iter().any(|c| !c.is_finite() || *c < 0.0)
            {
                return Some(format!("Planet {index}: Lohn oder Kapital entgleist"));
            }
        }
        if self.treasuries.iter().any(|cash| *cash < 0) {
            return Some("Staatskasse negativ".into());
        }
        None
    }
}

/// Mehrbedarf an Energie durch Kälte, Hitze und Schwerkraft.
pub fn energy_factor(planet: &Planet) -> f64 {
    let extremity = (((planet.temperature - 15.0).abs() / 150.0) as f64).min(1.5);
    1.0 + 0.5 * extremity + 0.3 * (planet.gravity as f64 - 1.0).max(0.0)
}

/// Kosten, eine Einheit des Guts zwischen Oberfläche und Orbit zu bewegen.
pub fn lift_cost(data: &GameData, planet: &Planet, good: GoodId) -> f64 {
    data.balance.economy.lift_cost * planet.gravity as f64 * data.good(good).mass as f64
}

/// Lokaler Ertragsfaktor eines Rezepts: Hier entscheidet sich, was sich wo lohnt.
pub fn local_yield(recipe: &RecipeDef, planet: &Planet) -> f64 {
    match recipe.source {
        YieldSource::None => 1.0,
        YieldSource::Soil => 0.25 + 1.25 * planet.habitability as f64,
        YieldSource::Deposit(_) => recipe
            .deposit_id
            .and_then(|good| planet.deposit(good))
            .map(|deposit| (deposit.richness * deposit.reserve.max(0.0).sqrt()) as f64)
            .unwrap_or(0.0),
    }
}

/// Wirtschaftlichkeit einer Kapitaleinheit je Tag bei voller Auslastung, in Credits.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UnitEconomics {
    /// Erlös nach Handelsspanne.
    pub revenue: f64,
    /// Einkauf der Vorprodukte.
    pub inputs: f64,
    /// Treibstoff für den Betrieb.
    pub energy: f64,
    /// Startkosten in den Orbit und zurück.
    pub lift: f64,
    pub wages: f64,
    /// Ertragsfaktor aus Vorkommen oder Boden.
    pub local_yield: f64,
}

impl UnitEconomics {
    /// Deckungsbeitrag vor Löhnen. Ist er nicht positiv, steht das Werk still.
    pub fn contribution(&self) -> f64 {
        self.revenue - self.inputs - self.energy - self.lift
    }

    pub fn margin(&self) -> f64 {
        self.contribution() - self.wages
    }
}

pub fn unit_economics(
    data: &GameData,
    planet: &Planet,
    wage: f64,
    market: &Market,
    recipe: &RecipeDef,
) -> UnitEconomics {
    let balance = &data.balance.economy;
    let local_yield = local_yield(recipe, planet);
    let output = recipe.output.1 as f64 * local_yield;
    let mut lift = output * lift_cost(data, planet, recipe.output_id);
    let mut inputs = 0.0;
    for (good, amount) in &recipe.input_ids {
        inputs += *amount as f64 * market.ask(*good, balance.spread);
        lift += *amount as f64 * lift_cost(data, planet, *good);
    }
    let fuel = recipe.energy as f64 * energy_factor(planet);
    lift += fuel * lift_cost(data, planet, balance.fuel_id);
    UnitEconomics {
        revenue: output * market.bid(recipe.output_id, balance.spread),
        inputs,
        energy: fuel * market.ask(balance.fuel_id, balance.spread),
        lift,
        wages: recipe.labor as f64 * wage,
        local_yield,
    }
}

/// Jahresrendite auf eine Kapitaleinheit zu heutigen Preisen.
pub fn expected_return(data: &GameData, market: &Market, economics: &UnitEconomics) -> f64 {
    let balance = &data.balance.economy;
    let cost = balance.capital_cost * market.ask(balance.capital_good_id, balance.spread);
    economics.margin() * crate::clock::DAYS_PER_YEAR as f64 / cost.max(1e-6)
}

/// Rezepte eines Wirtschaftszweigs.
pub fn recipes_of(data: &GameData, sector: Sector) -> impl Iterator<Item = (usize, &RecipeDef)> {
    data.recipes
        .iter()
        .enumerate()
        .filter(move |(_, recipe)| recipe.sector == sector)
}

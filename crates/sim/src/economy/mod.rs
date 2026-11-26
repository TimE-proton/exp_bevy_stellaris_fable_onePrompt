//! Wirtschaft: Produktion, lokale Preise, Löhne, Kapital.
//!
//! Drei Zeitskalen halten das System ruhig: Preise reagieren täglich, Frachter in Wochen,
//! Kapital wandert monatlich. Geld ist ganzzahlig und wird nur verschoben; neu entsteht es
//! allein durch die ausgewiesene Geldschöpfung für Neugeborene (`minted`).

mod budget;
mod capital;
mod plugin;
pub mod production;
mod setup;

pub use budget::{BUDGET_HISTORY, Budget, BudgetMonth, Claim, Line, Wants, settle_budget};
pub use capital::{
    invest_and_depreciate, market_capital_needs, migrate, pay_households, settle_public_accounts,
    state_reserve, steer_money_supply,
};
pub use plugin::{EconomyPlugin, ProductionStep};
pub use production::{
    abandon_dead_colonies, grow_population, produce_and_consume, staff_consume, target_price,
    update_prices_and_wages,
};
pub use setup::{seed_colony, setup_economy};

use crate::data::{EconomyBalance, GameData, RecipeDef, Sector, YieldSource};
use crate::galaxy::Planet;
use crate::ids::{GoodId, HolderId, IssuerId, PlanetId, PolityId, SystemId};
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

/// Anteil des Marktbestands, den ein einzelner Käufer an einem Tag höchstens abnimmt.
/// Verhindert, dass der erste Käufer eines Systems den Markt leerkauft.
pub const BUYER_SHARE: f64 = 0.5;

/// Bucht einen Kauf am Kontor zum Briefkurs: Ware aus dem Bestand, Geld aus `purse` in die
/// Kasse des Kontors. Gekauft wird höchstens, was vorhanden und bezahlbar ist. Gibt Menge
/// und gezahlten Betrag zurück.
pub fn purchase(
    market: &mut Market,
    good: GoodId,
    quantity: f64,
    purse: &mut Money,
    spread: f64,
) -> (f64, Money) {
    let index = good.index();
    let ask = market.ask(good, spread);
    let quantity = quantity
        .min(market.stock[index])
        .min(to_credits((*purse).max(0)) / ask)
        .max(0.0);
    if quantity <= 0.0 {
        return (0.0, 0);
    }
    let paid = pay(purse, &mut market.cash, to_money(quantity * ask));
    market.stock[index] = (market.stock[index] - quantity).max(0.0);
    market.turnover_today += to_credits(paid);
    (quantity, paid)
}

/// Einkauf eines Käufers ohne eigenes Lager (der Staat für Nachschub und Instandsetzung):
/// meldet den Wunsch als Nachfrage und nimmt höchstens den Anteil am Bestand, der einem
/// einzelnen Käufer zusteht. Gibt Menge und gezahlten Betrag zurück.
pub fn procure(
    market: &mut Market,
    good: GoodId,
    wanted: f64,
    purse: &mut Money,
    spread: f64,
) -> (f64, Money) {
    if wanted <= 0.0 {
        return (0.0, 0);
    }
    // Kaufkräftige Nachfrage zählt auch dann, wenn die Ware fehlt: Knappheit hebt den Preis
    // und zieht Frachter an. Wünsche ohne Geld bewegen keinen Preis.
    let affordable = to_credits((*purse).max(0)) / market.ask(good, spread);
    market.demand_today[good.index()] += wanted.min(affordable);
    purchase(
        market,
        good,
        wanted.min(market.available(good)),
        purse,
        spread,
    )
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

    /// Was ein einzelner Käufer heute höchstens abnehmen darf.
    #[inline]
    pub fn available(&self, good: GoodId) -> f64 {
        self.stock[good.index()] * BUYER_SHARE
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
    /// Autonome Regionen führen keine Steuern an das Zentrum ab.
    pub tax_exempt: bool,
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
            tax_exempt: false,
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
    /// Einkommensteuersatz je Gemeinschaft.
    pub tax_rates: Vec<f64>,
    /// Rücklage, die der Staat halten will; erst was darüber liegt, gibt er wieder aus.
    pub treasury_targets: Vec<Money>,
    /// Geldmenge bei Spielbeginn (alle Konten, auch die der Handelshäuser).
    pub initial_money: Money,
    /// Seit Spielbeginn ausgewiesene Geldschöpfung.
    pub minted: Money,
    /// Ertragsfaktor der Technik je Gemeinschaft (1 = Grundstand; fehlende Einträge
    /// gelten als 1). Folgt der Forschung: gesetzt bei Spielbeginn, nach jedem
    /// Forschungsschritt und bei einer Abspaltung, täglich aufgefrischt.
    #[serde(default)]
    pub tech_yield: Vec<f64>,
    /// Laufende Einnahmen des Staates im letzten abgeschlossenen Monat, je Gemeinschaft:
    /// Lohnsteuer, Zölle der Kontore und Dividenden auf staatliche Anteile. Geld der
    /// Notenbank, Anleihen und Verkäufe zählen nicht. Daran bemisst sich der Forschungsetat.
    #[serde(default)]
    pub state_income: Vec<Money>,
    /// Dieselben Einnahmen des laufenden Monats, bis der Monatsabschluss sie übernimmt.
    #[serde(default)]
    pub state_income_running: Vec<Money>,
    /// Besteuerte Lohnsumme des laufenden Monats je Gemeinschaft: die Löhne, von denen
    /// der Staat Lohnsteuer erhält. Aus ihr ergibt sich, was ein anderer Steuersatz
    /// brächte (`BudgetMonth::standard_revenue`).
    #[serde(default)]
    pub wage_bill_running: Vec<Money>,
    /// Der Haushalt je Gemeinschaft: Ansprüche an die Staatskasse in fester Rangfolge,
    /// der laufende Monat und die letzten zwölf.
    #[serde(default)]
    pub budgets: Vec<Budget>,
    /// Geld derer, die im Dienst des Staates stehen (Besatzungen der Flotte, Forschende),
    /// je Gemeinschaft: Sold und Gehälter gehen hierhin, nicht an die Haushalte der
    /// Welten. Ausgegeben wird es Tag für Tag am Markt des Sitzes der Gemeinschaft
    /// ([`staff_consume`]); was der Staatsdienst verbraucht, fehlt den übrigen.
    #[serde(default)]
    pub staff: Vec<Money>,
}

/// Faktor einer Gemeinschaft aus einer Liste je Gemeinschaft; fehlt der Eintrag, 1.
#[inline]
pub fn factor_of(factors: &[f64], polity: PolityId) -> f64 {
    factors.get(polity.index()).copied().unwrap_or(1.0)
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

    /// Ertragsfaktor der Technik, mit dem die Werke der Gemeinschaft produzieren.
    #[inline]
    pub fn yield_factor(&self, polity: PolityId) -> f64 {
        factor_of(&self.tech_yield, polity)
    }

    /// Laufende Einnahmen des Staates im letzten abgeschlossenen Monat (siehe
    /// [`Economy::state_income`]).
    #[inline]
    pub fn state_income_of(&self, polity: PolityId) -> Money {
        self.state_income.get(polity.index()).copied().unwrap_or(0)
    }

    /// Sorgt dafür, dass jede Liste je Gemeinschaft für jede Staatskasse einen Eintrag
    /// hat, auch für eben erst gegründete Gemeinschaften.
    pub fn sync_polities(&mut self) {
        let count = self.treasuries.len();
        if self.tech_yield.len() < count {
            self.tech_yield.resize(count, 1.0);
        }
        if self.state_income.len() < count {
            self.state_income.resize(count, 0);
        }
        if self.state_income_running.len() < count {
            self.state_income_running.resize(count, 0);
        }
        if self.wage_bill_running.len() < count {
            self.wage_bill_running.resize(count, 0);
        }
        if self.budgets.len() < count {
            self.budgets.resize_with(count, Budget::default);
        }
        if self.staff.len() < count {
            self.staff.resize(count, 0);
        }
    }

    /// Verbucht eine laufende Einnahme des Staates. Das Geld selbst ist schon in der
    /// Staatskasse; hier wird nur mitgeschrieben, woraus sie sich speist.
    #[inline]
    pub fn book_state_income(&mut self, polity: PolityId, amount: Money) {
        book_income(&mut self.state_income_running, polity, amount);
    }

    /// Monatsabschluss der Einnahmenrechnung: Der laufende Monat wird zum letzten.
    pub fn close_income_month(&mut self) {
        self.sync_polities();
        for (last, running) in self
            .state_income
            .iter_mut()
            .zip(&mut self.state_income_running)
        {
            *last = std::mem::take(running);
        }
    }

    /// Geld derer, die im Dienst des Staates stehen (siehe [`Economy::staff`]).
    #[inline]
    pub fn staff_money(&self, polity: PolityId) -> Money {
        self.staff.get(polity.index()).copied().unwrap_or(0)
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
        let staff: Money = self.staff.iter().sum();
        markets + planets + states + staff
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
        if self.staff.iter().any(|cash| *cash < 0) {
            return Some("Kasse des Staatsdienstes negativ".into());
        }
        let count = self.treasuries.len();
        if self.tech_yield.len() != count
            || self.state_income.len() != count
            || self.state_income_running.len() != count
            || self.wage_bill_running.len() != count
            || self.budgets.len() != count
            || self.staff.len() != count
        {
            return Some("Listen je Gemeinschaft sind unvollständig".into());
        }
        if self
            .state_income
            .iter()
            .chain(&self.state_income_running)
            .any(|income| *income < 0)
        {
            return Some("Staatseinnahmen negativ".into());
        }
        if self
            .tech_yield
            .iter()
            .any(|factor| !factor.is_finite() || *factor <= 0.0)
        {
            return Some("Ertragsfaktor der Technik entgleist".into());
        }
        for (index, budget) in self.budgets.iter().enumerate() {
            if budget.history.len() > BUDGET_HISTORY {
                return Some(format!("Haushalt {index}: zu viele Monate aufbewahrt"));
            }
            let negative = |month: &BudgetMonth| {
                month.revenue < 0
                    || month.wage_bill < 0
                    || month.standard_revenue < 0
                    || month
                        .lines
                        .values()
                        .any(|line| line.wanted < 0 || line.paid < 0)
            };
            if negative(&budget.current)
                || budget.history.iter().any(negative)
                || budget.grants.values().any(|grant| *grant < 0)
            {
                return Some(format!("Haushalt {index}: negativer Betrag"));
            }
        }
        None
    }
}

/// Schreibt eine laufende Einnahme in die Liste je Gemeinschaft; sie wächst bei Bedarf.
#[inline]
pub(crate) fn book_income(running: &mut Vec<Money>, polity: PolityId, amount: Money) {
    if amount <= 0 {
        return;
    }
    if running.len() <= polity.index() {
        running.resize(polity.index() + 1, 0);
    }
    running[polity.index()] += amount;
}

/// Kapitalbewegungen, die die Börse verbuchen muss. Das Geld ist dem Emittenten bereits
/// abgebucht (Ausschüttung) bzw. gutgeschrieben (Einlage); die Börse zahlt es im selben
/// Takt an die Halter aus bzw. gibt dafür neue Anteile aus.
#[derive(Resource, Clone, Debug, Default)]
pub struct EquityFlows {
    pub dividends: Vec<(IssuerId, Money)>,
    pub injections: Vec<(IssuerId, HolderId, Money)>,
}

impl EquityFlows {
    /// Geld, das zwischen Abbuchung und Auszahlung unterwegs ist.
    pub fn in_transit(&self) -> Money {
        self.dividends.iter().map(|(_, amount)| amount).sum()
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
    /// Ertragsfaktor aus Vorkommen oder Boden, samt dem der Technik.
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
    unit_economics_at(data, planet, wage, market, recipe, 1.0)
}

/// Wie [`unit_economics`], mit dem Ertragsfaktor der Technik der Gemeinschaft
/// ([`Economy::yield_factor`]): Er hebt den Ausstoß bei gleichem Einsatz.
pub fn unit_economics_at(
    data: &GameData,
    planet: &Planet,
    wage: f64,
    market: &Market,
    recipe: &RecipeDef,
    tech_yield: f64,
) -> UnitEconomics {
    let balance = &data.balance.economy;
    let local_yield = local_yield(recipe, planet) * tech_yield;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn munition_market(stock: f64) -> (GameData, Market, GoodId, f64) {
        let data = GameData::load_workspace().unwrap();
        let good = data.good_id("munition").unwrap();
        let mut market = Market::new(&data);
        market.stock[good.index()] = stock;
        let spread = data.balance.economy.spread;
        (data, market, good, spread)
    }

    #[test]
    fn a_purchase_moves_goods_and_money_and_never_more_than_there_is() {
        let (_, mut market, good, spread) = munition_market(10.0);
        let ask = market.ask(good, spread);
        let mut purse = to_money(1000.0);

        // Ein gewöhnlicher Kauf: Ware gegen Geld zum Briefkurs, auf das Tausendstel.
        let (got, paid) = purchase(&mut market, good, 4.0, &mut purse, spread);
        assert_eq!((got, paid), (4.0, to_money(4.0 * ask)));
        assert_eq!(market.stock_of(good), 6.0);
        assert_eq!(market.cash, paid);
        assert_eq!(purse, to_money(1000.0) - paid);
        assert!((market.turnover_today - to_credits(paid)).abs() < 1e-9);

        // Mehr als der Bestand gibt es nicht.
        let (got, _) = purchase(&mut market, good, 100.0, &mut purse, spread);
        assert_eq!(got, 6.0);
        assert_eq!(market.stock_of(good), 0.0);

        // Mehr als der Geldbeutel hergibt, auch nicht: Er wird leer, nie negativ.
        market.stock[good.index()] = 10.0;
        let before = market.cash;
        let mut purse = to_money(2.5 * ask);
        let (got, paid) = purchase(&mut market, good, 8.0, &mut purse, spread);
        assert!((got - 2.5).abs() < 1e-3);
        assert_eq!((purse, market.cash - before), (0, paid));

        // Ohne Geld oder ohne Wunsch geschieht nichts.
        let stock = market.stock_of(good);
        assert_eq!(
            purchase(&mut market, good, 3.0, &mut purse, spread),
            (0.0, 0)
        );
        let mut rich = to_money(100.0);
        assert_eq!(
            purchase(&mut market, good, 0.0, &mut rich, spread),
            (0.0, 0)
        );
        assert_eq!(
            purchase(&mut market, good, -2.0, &mut rich, spread),
            (0.0, 0)
        );
        assert_eq!((market.stock_of(good), rich), (stock, to_money(100.0)));
    }

    #[test]
    fn procuring_reports_solvent_demand_and_leaves_stock_for_others() {
        let (_, mut market, good, spread) = munition_market(10.0);
        let ask = market.ask(good, spread);

        // Ein einzelner Käufer bekommt höchstens seinen Anteil am Bestand; gemeldet ist der
        // ganze Wunsch.
        let mut purse = to_money(10_000.0);
        let (got, paid) = procure(&mut market, good, 8.0, &mut purse, spread);
        assert_eq!(got, 10.0 * BUYER_SHARE);
        assert_eq!(paid, to_money(got * ask));
        assert_eq!(market.demand_today[good.index()], 8.0);

        // Fehlt die Ware ganz, zählt der Wunsch trotzdem: Knappheit soll den Preis heben.
        market.stock[good.index()] = 0.0;
        assert_eq!(
            procure(&mut market, good, 3.0, &mut purse, spread),
            (0.0, 0)
        );
        assert_eq!(market.demand_today[good.index()], 11.0);

        // Wünsche ohne Geld bewegen keinen Preis: Gemeldet wird nur, was bezahlbar ist.
        market.stock[good.index()] = 10.0;
        let mut poor = to_money(1.5 * ask);
        let (got, _) = procure(&mut market, good, 4.0, &mut poor, spread);
        assert!((got - 1.5).abs() < 1e-3);
        assert!((market.demand_today[good.index()] - 12.5).abs() < 1e-3);
        let mut broke = 0;
        assert_eq!(
            procure(&mut market, good, 4.0, &mut broke, spread),
            (0.0, 0)
        );
        assert!((market.demand_today[good.index()] - 12.5).abs() < 1e-3);
    }
}

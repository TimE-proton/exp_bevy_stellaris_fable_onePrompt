//! Langsame Zeitskala (monatlich): Kapital wandert, Menschen wandern, öffentliche Kassen
//! werden ausgeglichen.

use super::{
    Economy, Money, expected_return, pay, recipes_of, to_credits, to_money, unit_economics,
};
use crate::data::{GameData, Sector};
use crate::galaxy::Galaxy;
use crate::ids::{PlanetId, PolityId};

const MONTHS_PER_YEAR: f64 = 12.0;

/// Kapital verschleißt, verlustreiche Werke werden abgebaut, Gewinne fließen in das
/// Rezept mit der höchsten erwarteten Rendite. So spezialisieren sich Standorte von
/// selbst: Es gibt keine Gebäudeboni, nur lokale Kosten und Preise.
pub fn invest_and_depreciate(
    galaxy: &Galaxy,
    economy: &mut Economy,
    data: &GameData,
    capitals: &[Option<PlanetId>],
    day: u64,
) {
    let balance = &data.balance.economy;
    let machines = balance.capital_good_id;
    let Economy {
        markets, planets, ..
    } = economy;
    for market in markets.iter_mut() {
        market.standing[machines.index()] = 0.0;
    }

    // Anlagefonds je Gemeinschaft: Ein kleiner Teil des Geldes der Hauptwelt sucht jeden
    // Monat lohnende Firmen auf den anderen Welten. So fließt Kapital in die Kolonien
    // (ab Meilenstein 3 gegen Anteile: die Verflechtung, die das Reich zusammenhält).
    let mut funds: Vec<Money> = capitals
        .iter()
        .map(|capital| {
            capital.map_or(0, |id| {
                (planets[id.index()].households as f64 * 0.015) as Money
            })
        })
        .collect();
    let mut drawn: Vec<Money> = vec![0; funds.len()];

    for planet in &galaxy.planets {
        let Some(owner) = planet.owner else {
            continue;
        };
        if planet.population <= 0.0 {
            continue;
        }
        let is_capital = capitals.get(owner.index()).copied().flatten() == Some(planet.id);
        let market = &mut markets[planet.system.index()];
        let local = &mut planets[planet.id.index()];
        let labor_supply = planet.population * balance.workforce_share;

        // Erwartete Rendite je Rezept zu heutigen Preisen und Löhnen.
        let mut returns = vec![f64::MIN; data.recipes.len()];
        for (index, recipe) in data.recipes.iter().enumerate() {
            let economics = unit_economics(data, planet, local.wage, market, recipe);
            if economics.local_yield > 0.0 {
                // Bestehende Werke rechnen mit ihrer tatsächlichen Auslastung: Wer nicht
                // absetzen kann, verdient auch bei guten Preisen nichts.
                let utilization = if local.capital[index] > 0.0 {
                    local.utilization_trend[index].clamp(0.0, 1.0)
                } else {
                    1.0
                };
                let mut realized = economics;
                realized.revenue *= utilization;
                realized.inputs *= utilization;
                realized.energy *= utilization;
                realized.lift *= utilization;
                returns[index] = expected_return(data, market, &realized);
            }
            local.roc[index] = if returns[index] == f64::MIN {
                0.0
            } else {
                returns[index]
            };
        }

        // Verschleiß und Abbau.
        for (index, capital) in local.capital.iter_mut().enumerate() {
            if *capital <= 0.0 {
                continue;
            }
            let mut decay = balance.depreciation_per_year / MONTHS_PER_YEAR;
            // Junge Kolonien bekommen Zeit, bis Frachter ihren Markt erschlossen haben.
            let infant =
                day < local.founded_day + balance.infant_years * crate::clock::DAYS_PER_YEAR;
            if returns[index] < 0.0 && !(infant && local.founded_day > 0) {
                decay += balance.scrap_per_year / MONTHS_PER_YEAR;
            }
            *capital *= 1.0 - decay;
            if *capital < 0.05 {
                *capital = 0.0;
            }
        }

        let labor_demand: f64 = local
            .capital
            .iter()
            .zip(&data.recipes)
            .map(|(capital, recipe)| capital * recipe.labor as f64)
            .sum();
        // Investiert wird auch über das Arbeitskräfteangebot hinaus: Dann steigen die Löhne,
        // und Menschen wandern zu. Die sinkende Auslastung bremst von selbst.
        let mut labor_room = (labor_supply * balance.overhire + 0.5 - labor_demand).max(0.0);

        for sector in Sector::ALL {
            // Betriebsmittel für einige Tage bleiben in der Kasse.
            let mut monthly_cost = 0.0;
            let mut best: Option<(usize, f64)> = None;
            for (index, recipe) in recipes_of(data, sector) {
                let economics = unit_economics(data, planet, local.wage, market, recipe);
                monthly_cost += local.capital[index]
                    * (economics.inputs + economics.energy + economics.lift + economics.wages)
                    * balance.firm_reserve_days;
                if returns[index] > best.map_or(f64::MIN, |(_, value)| value) {
                    best = Some((index, returns[index]));
                }
            }
            let reserve = to_money(monthly_cost);
            let firm_cash = local.firms[sector.index()].cash;

            if let Some((index, expected)) = best
                && expected > balance.hurdle_per_year
            {
                let unit_cost = balance.capital_cost * market.ask(machines, balance.spread);
                // Fehlt der Firma das Geld für eine lohnende Investition, schießen die
                // Haushalte Eigenkapital zu (ab Meilenstein 3: gegen Anteile).
                if firm_cash < reserve + to_money(unit_cost) {
                    let wanted =
                        reserve + to_money(unit_cost * balance.seed_investment) - firm_cash;
                    let offer = (local.households as f64 * 0.02) as Money;
                    let households = &mut local.households;
                    let got = pay(
                        households,
                        &mut local.firms[sector.index()].cash,
                        wanted.min(offer),
                    );
                    // Reicht das Geld vor Ort nicht, springt der Anlagefonds der Hauptwelt ein.
                    if !is_capital && got < wanted {
                        let fund = &mut funds[owner.index()];
                        let extra = (wanted - got).min(*fund);
                        *fund -= extra;
                        drawn[owner.index()] += extra;
                        local.firms[sector.index()].cash += extra;
                    }
                }
                let firm = &mut local.firms[sector.index()];
                let investable = to_credits((firm.cash - reserve).max(0)) * 0.6;
                let labor_per_unit = data.recipes[index].labor as f64;
                let growth_cap = (local.capital[index] * balance.max_growth_per_month)
                    .max(balance.seed_investment);
                let units = (investable / unit_cost)
                    .min(growth_cap)
                    .min(labor_room / labor_per_unit)
                    .min(market.stock[machines.index()] * 0.5 / balance.capital_cost)
                    .max(0.0);
                // Der Investitionswunsch zählt als Nachfrage, auch wenn Maschinen fehlen.
                market.standing[machines.index()] +=
                    growth_cap.min(investable / unit_cost).max(0.0) * balance.capital_cost / 30.0;
                if units > 0.01 {
                    let stock = &mut market.stock[machines.index()];
                    *stock = (*stock - units * balance.capital_cost).max(0.0);
                    pay(
                        &mut firm.cash,
                        &mut market.cash,
                        to_money(units * unit_cost),
                    );
                    local.capital[index] += units;
                    labor_room -= units * labor_per_unit;
                }
            }

            // Was über das Anderthalbfache der Betriebsmittel hinausgeht, wird großteils an
            // die Eigentümer ausgeschüttet (bis Meilenstein 3: die Haushalte vor Ort).
            let firm = &mut local.firms[sector.index()];
            let excess = firm.cash - (reserve + reserve / 2).max(to_money(50.0));
            if excess > 0 {
                let payout = pay(&mut firm.cash, &mut local.households, excess * 7 / 10);
                local.income_today += to_credits(payout);
            }
        }
    }

    // Was der Fonds ausgegeben hat, kommt von den Haushalten der Hauptwelt.
    for (polity, amount) in drawn.into_iter().enumerate() {
        if let Some(capital) = capitals[polity] {
            planets[capital.index()].households -= amount;
        }
    }
}

/// Menschen ziehen innerhalb ihrer Gemeinschaft dorthin, wo Löhne und Lebensbedingungen
/// besser sind. So wachsen Kolonien, deren Vorkommen Arbeit lohnend machen.
pub fn migrate(galaxy: &mut Galaxy, economy: &mut Economy, data: &GameData, polities: usize) {
    let rate = data.balance.population.migration_per_month;
    for polity in (0..polities).map(PolityId::from_index) {
        let members: Vec<usize> = galaxy
            .planets
            .iter()
            .filter(|planet| planet.owner == Some(polity) && planet.population > 0.0)
            .map(|planet| planet.id.index())
            .collect();
        if members.len() < 2 {
            continue;
        }
        let appeal = |index: usize| {
            let planet = &galaxy.planets[index];
            let local = &economy.planets[index];
            local.wage
                * local.employment
                * (0.7 + 0.3 * planet.habitability as f64)
                * (0.2 + 0.8 * local.essentials_met(&data.balance.economy))
        };
        let appeals: Vec<f64> = members.iter().map(|index| appeal(*index)).collect();
        let total_population: f64 = members
            .iter()
            .map(|index| galaxy.planets[*index].population)
            .sum();
        let average: f64 = members
            .iter()
            .zip(&appeals)
            .map(|(index, appeal)| galaxy.planets[*index].population * appeal)
            .sum::<f64>()
            / total_population;
        if average <= 0.0 {
            continue;
        }

        // Abwanderung aus den unattraktiven Welten in einen gemeinsamen Topf …
        let mut movers = 0.0;
        let mut purse: Money = 0;
        for (index, appeal) in members.iter().zip(&appeals) {
            if *appeal >= average {
                continue;
            }
            let planet = &mut galaxy.planets[*index];
            let share = rate * ((average - appeal) / average * 3.0).min(1.0);
            let leaving = planet.population * share;
            let cash = (economy.planets[*index].households as f64 * share) as Money;
            planet.population -= leaving;
            economy.planets[*index].households -= cash;
            movers += leaving;
            purse += cash;
        }
        // … und Zuzug in die attraktiven, soweit Platz ist.
        let weights: Vec<f64> = members
            .iter()
            .zip(&appeals)
            .map(|(index, appeal)| {
                let planet = &galaxy.planets[*index];
                let room = (planet.capacity - planet.population).max(0.0);
                if *appeal > average {
                    (appeal - average) * room
                } else {
                    0.0
                }
            })
            .collect();
        let total_weight: f64 = weights.iter().sum();
        let mut settled = 0.0;
        let mut paid: Money = 0;
        if total_weight > 0.0 {
            for (index, weight) in members.iter().zip(&weights) {
                if *weight <= 0.0 {
                    continue;
                }
                let planet = &mut galaxy.planets[*index];
                let room = (planet.capacity - planet.population).max(0.0);
                let arriving = (movers * weight / total_weight).min(room);
                let cash = (purse as f64 * weight / total_weight) as Money;
                planet.population += arriving;
                economy.planets[*index].households += cash;
                settled += arriving;
                paid += cash;
            }
        }
        // Wer keinen Platz fand, bleibt auf der bevölkerungsreichsten Welt; ebenso der
        // Rundungsrest des Geldes. Niemand und nichts geht verloren.
        let fallback = *members
            .iter()
            .max_by(|a, b| {
                galaxy.planets[**a]
                    .population
                    .total_cmp(&galaxy.planets[**b].population)
                    .then(b.cmp(a))
            })
            .expect("mindestens zwei Mitglieder");
        galaxy.planets[fallback].population += movers - settled;
        economy.planets[fallback].households += purse - paid;
    }
}

/// Kontore führen Überschüsse als Zölle ab, Staaten geben aus, was über ihre Rücklage
/// hinausgeht (bis zum Ausbau des Haushalts: als Leistungen an die Haushalte).
pub fn settle_public_accounts(galaxy: &Galaxy, economy: &mut Economy, data: &GameData) {
    let balance = &data.balance.economy;
    for system in &galaxy.systems {
        let Some(owner) = galaxy.system_owner(system.id) else {
            continue;
        };
        let market = &mut economy.markets[system.index()];
        // Das Kontor behält Betriebsmittel für einige Umsatztage und genug, um sein
        // Soll-Lager zur Hälfte vorzufinanzieren; der Rest ist Zoll.
        let stock_value: f64 = market
            .demand
            .iter()
            .zip(&market.price)
            .map(|(demand, price)| demand * balance.cover_days * price)
            .sum();
        let working =
            to_money(market.turnover * balance.market_reserve_days + 0.5 * stock_value + 1500.0);
        let excess = market.cash - working;
        if excess > 0 {
            pay(
                &mut market.cash,
                &mut economy.treasuries[owner.index()],
                excess,
            );
        }
    }
    for polity in 0..economy.treasuries.len() {
        let id = PolityId::from_index(polity);
        let population = galaxy.population_of(id);
        if population <= 0.0 {
            continue;
        }
        let reserve = to_money(population * balance.state_reserve_per_million);
        let excess = economy.treasuries[polity] - reserve;
        if excess <= 0 {
            continue;
        }
        let budget = excess / 2;
        let mut spent: Money = 0;
        for planet in galaxy.planets_of(id) {
            let share = (budget as f64 * planet.population / population) as Money;
            economy.planets[planet.id.index()].households += share;
            economy.planets[planet.id.index()].income_today += to_credits(share) / 30.0;
            spent += share;
        }
        economy.treasuries[polity] -= spent;
    }
}

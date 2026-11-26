//! Ausgangslage der Wirtschaft und Erstausstattung neuer Kolonien.

use super::{
    Economy, Market, Money, PlanetEconomy, energy_factor, expected_return, local_yield, to_money,
    unit_economics, unit_economics_at,
};
use crate::clock::DAYS_PER_YEAR;
use crate::data::GameData;
use crate::galaxy::{Galaxy, Planet};
use crate::ids::PlanetId;
use crate::polity::Polities;

/// Legt Märkte und Planetenwirtschaften an und stattet die Heimatwelten so aus, dass sie
/// ihren eigenen Grundbedarf ungefähr decken. Alles Weitere regelt der Markt.
pub fn setup_economy(data: &GameData, galaxy: &Galaxy, polities: &Polities) -> Economy {
    let mut economy = Economy {
        markets: galaxy.systems.iter().map(|_| Market::new(data)).collect(),
        planets: galaxy
            .planets
            .iter()
            .map(|_| PlanetEconomy::new(data))
            .collect(),
        treasuries: vec![0; polities.list.len()],
        tax_rates: vec![data.balance.economy.income_tax; polities.list.len()],
        treasury_targets: vec![0; polities.list.len()],
        initial_money: 0,
        minted: 0,
        tech_yield: Vec::new(),
        state_income: Vec::new(),
        state_income_running: Vec::new(),
        wage_bill_running: Vec::new(),
        wage_tax_running: Vec::new(),
        budgets: Vec::new(),
        staff: Vec::new(),
    };
    economy.sync_polities();
    for polity in &polities.list {
        if let Some(capital) = polity.capital {
            setup_home_world(data, galaxy.planet(capital), &mut economy);
            // Die Staatskasse beginnt mit dem Geld für einige Koloniegründungen.
            economy.treasuries[polity.id.index()] = to_money(
                galaxy.planet(capital).population * data.balance.economy.state_reserve_per_million
                    + 4.0 * data.balance.colonization.cost,
            );
        }
    }
    economy.initial_money = economy.money_in_accounts();
    economy
}

fn setup_home_world(data: &GameData, planet: &Planet, economy: &mut Economy) {
    let balance = &data.balance.economy;
    let population = planet.population;
    let goods = data.goods.len();
    let efactor = energy_factor(planet);

    // Welches Rezept stellt ein Gut hier am günstigsten her (zu Richtpreisen)?
    let unit_cost = |index: usize| -> Option<f64> {
        let recipe = &data.recipes[index];
        let yield_factor = local_yield(recipe, planet);
        if yield_factor <= 0.0 {
            return None;
        }
        let mut cost = recipe.labor as f64 * balance.base_wage
            + recipe.energy as f64 * efactor * data.good(balance.fuel_id).base_price;
        for (good, amount) in &recipe.input_ids {
            cost += *amount as f64 * data.good(*good).base_price;
        }
        Some(cost / (recipe.output.1 as f64 * yield_factor))
    };
    // Nur Ketten, die sich vor Ort vollständig schließen: Ein Rezept zählt erst, wenn alle
    // seine Vorprodukte lokal herstellbar sind. (Treibstoff bleibt außen vor: Förderung
    // und Raffinerie bedingen einander, der Anfangsbestand überbrückt den Anlauf.)
    let mut available = vec![false; goods];
    loop {
        let mut changed = false;
        for recipe in &data.recipes {
            if available[recipe.output_id.index()] || local_yield(recipe, planet) <= 0.0 {
                continue;
            }
            if recipe
                .input_ids
                .iter()
                .all(|(good, _)| available[good.index()])
            {
                available[recipe.output_id.index()] = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let mut producer: Vec<Option<usize>> = vec![None; goods];
    for (index, recipe) in data.recipes.iter().enumerate() {
        let Some(cost) = unit_cost(index) else {
            continue;
        };
        if !recipe
            .input_ids
            .iter()
            .all(|(good, _)| available[good.index()])
        {
            continue;
        }
        let slot = &mut producer[recipe.output_id.index()];
        if slot.is_none_or(|current| unit_cost(current).is_some_and(|best| cost < best)) {
            *slot = Some(index);
        }
    }

    // Bedarf rückwärts durch die Ketten rechnen. Wenige Durchläufe genügen, weil nur
    // Treibstoff und Maschinen Rückkopplungen bilden.
    let mut capital = vec![0.0f64; data.recipes.len()];
    let mut required = vec![0.0f64; goods];
    for _ in 0..10 {
        required.iter_mut().for_each(|value| *value = 0.0);
        for need in &balance.needs {
            required[need.good_id.index()] += population * need.per_million;
        }
        let mut total_capital = 0.0;
        for (index, recipe) in data.recipes.iter().enumerate() {
            for (good, amount) in &recipe.input_ids {
                required[good.index()] += capital[index] * *amount as f64;
            }
            required[balance.fuel_id.index()] += capital[index] * recipe.energy as f64 * efactor;
            total_capital += capital[index];
        }
        // Ersatz für Verschleiß und etwas Wachstum.
        required[balance.capital_good_id.index()] +=
            total_capital * balance.capital_cost * (balance.depreciation_per_year + 0.03)
                / DAYS_PER_YEAR as f64;
        capital.iter_mut().for_each(|value| *value = 0.0);
        for good in 0..goods {
            if let Some(index) = producer[good] {
                let recipe = &data.recipes[index];
                capital[index] =
                    required[good] / (recipe.output.1 as f64 * local_yield(recipe, planet));
            }
        }
    }

    // Die Arbeitskräfte sind die harte Grenze.
    let labor_supply = population * balance.workforce_share;
    let labor_demand: f64 = capital
        .iter()
        .zip(&data.recipes)
        .map(|(c, recipe)| c * recipe.labor as f64)
        .sum();
    let scale = if labor_demand > 0.0 {
        (0.92 * labor_supply / labor_demand).min(1.0)
    } else {
        1.0
    };
    capital.iter_mut().for_each(|value| *value *= scale);

    let market = &mut economy.markets[planet.system.index()];
    let local = &mut economy.planets[planet.id.index()];
    let mut turnover = 0.0;
    for good in 0..goods {
        market.stock[good] += required[good] * balance.cover_days;
        market.demand[good] += required[good];
        market.supply[good] += required[good];
        turnover += required[good] * data.goods[good].base_price;
    }
    market.turnover = turnover;
    market.cash += to_money(turnover * balance.market_reserve_days * 1.5);

    local.households = to_money(population * balance.cash_per_million);
    local.wage = balance.base_wage;
    for (index, recipe) in data.recipes.iter().enumerate() {
        if capital[index] <= 0.0 {
            continue;
        }
        let economics = unit_economics(data, planet, balance.base_wage, market, recipe);
        let daily_cost = capital[index]
            * (economics.inputs + economics.energy + economics.lift + economics.wages);
        local.firms[recipe.sector.index()].cash +=
            to_money(daily_cost * balance.firm_reserve_days * 1.5);
        local.capital[index] = capital[index];
    }
}

/// Erstausstattung einer neuen Kolonie: Die Siedler nehmen ihren Anteil am Geld der
/// Hauptwelt mit, der Staat bezahlt die Gründung, und die mitgebrachten Werke gehen in
/// die Rezepte, die sich an diesem Ort am meisten lohnen.
pub fn seed_colony(
    data: &GameData,
    galaxy: &Galaxy,
    economy: &mut Economy,
    colony: PlanetId,
    capital: PlanetId,
    settlers: f64,
    capital_population_before: f64,
    day: u64,
) {
    let balance = &data.balance;
    let planet = galaxy.planet(colony);
    let owner = planet.owner.expect("Kolonie hat einen Besitzer");

    let share = (settlers / capital_population_before.max(1e-9)).clamp(0.0, 1.0);
    let cash = (economy.planets[capital.index()].households as f64 * share) as Money;
    economy.planets[capital.index()].households -= cash;

    let market = &economy.markets[planet.system.index()];
    let tech = economy.yield_factor(owner);
    // Siedler bringen Förderanlagen mit, keine Fabriken: Rezepte ohne Vorprodukte, die
    // aus dem leben, was der Ort hergibt. Verarbeitung folgt später, wenn sie sich rechnet.
    let mut ranked: Vec<(usize, f64)> = data
        .recipes
        .iter()
        .enumerate()
        .filter(|(_, recipe)| recipe.input_ids.is_empty())
        .filter_map(|(index, recipe)| {
            let economics = unit_economics_at(
                data,
                planet,
                balance.economy.base_wage,
                market,
                recipe,
                tech,
            );
            (economics.local_yield > 0.0)
                .then(|| (index, expected_return(data, market, &economics)))
        })
        .collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));

    let mut local = PlanetEconomy::new(data);
    local.households = cash;
    local.founded_day = day;
    let grant = to_money(balance.colonization.cost).min(economy.treasuries[owner.index()].max(0));
    economy.treasuries[owner.index()] -= grant;

    // Ein Drittel der Gründungskosten wird Betriebskapital des Kontors im Zielsystem:
    // Ohne Kasse könnte es keinem Frachter etwas abkaufen und die Kolonie verhungerte.
    let market_share = grant / 3;
    economy.markets[planet.system.index()].cash += market_share;

    // Proviant: Die Siedler nehmen Vorräte für ein Jahr vom Heimatmarkt mit; der Staat
    // bezahlt sie aus dem zweiten Drittel beim dortigen Kontor.
    let home_system = galaxy.planet(capital).system;
    let mut provisions_budget = grant / 3;
    let mut spent: Money = 0;
    if home_system != planet.system {
        for need in &balance.economy.needs {
            let wanted = settlers * need.per_million * crate::clock::DAYS_PER_YEAR as f64;
            let home = &mut economy.markets[home_system.index()];
            let price = home.ask(need.good_id, balance.economy.spread);
            let quantity = wanted
                .min(home.stock[need.good_id.index()] * 0.25)
                .min(super::to_credits(provisions_budget) / price)
                .max(0.0);
            let cost = to_money(quantity * price).min(provisions_budget);
            home.stock[need.good_id.index()] -= quantity;
            home.cash += cost;
            provisions_budget -= cost;
            spent += cost;
            economy.markets[planet.system.index()].stock[need.good_id.index()] += quantity;
        }
    }
    // Der Rest geht als Startkapital an die Firmen der lohnendsten Rezepte.
    let grant = grant - market_share - spent;
    let picks: Vec<(usize, f64)> = match ranked.as_slice() {
        [] => Vec::new(),
        [only] => vec![(only.0, 1.0)],
        [first, second, ..] => vec![(first.0, 0.6), (second.0, 0.4)],
    };
    if picks.is_empty() {
        // Nichts lohnt sich hier: Das Geld geht an die Siedler.
        local.households += grant;
    }
    let mut granted: Money = 0;
    for (position, (index, weight)) in picks.iter().enumerate() {
        local.capital[*index] += balance.colonization.seed_capital as f64 * weight;
        let part = if position + 1 == picks.len() {
            grant - granted
        } else {
            (grant as f64 * weight) as Money
        };
        local.firms[data.recipes[*index].sector.index()].cash += part;
        granted += part;
    }
    // Geld, das auf der Welt noch lag (etwa Dividenden an die Halter einer zuvor
    // aufgegebenen Kolonie), bleibt erhalten.
    let previous = &economy.planets[colony.index()];
    local.households += previous.households;
    for (firm, old) in local.firms.iter_mut().zip(&previous.firms) {
        firm.cash += old.cash;
    }
    economy.planets[colony.index()] = local;
}

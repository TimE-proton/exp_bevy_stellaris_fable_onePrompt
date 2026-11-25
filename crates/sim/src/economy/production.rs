//! Tagesschritt: Produktion, Konsum, Bevölkerung, Preise, Löhne.

use super::{Economy, Money, energy_factor, lift_cost, local_yield, pay, to_money};
use crate::clock::DAYS_PER_YEAR;
use crate::data::GameData;
use crate::galaxy::Galaxy;

/// Anteil des Marktbestands, den ein einzelner Käufer an einem Tag höchstens abnimmt.
/// Verhindert, dass der erste Planet eines Systems den Markt leerkauft.
const BUYER_SHARE: f64 = 0.5;
/// Glättung der Tageswerte (Anteil des heutigen Werts).
const SMOOTHING: f64 = 0.1;

/// Alle Planeten produzieren, zahlen Löhne und Steuern, die Haushalte kaufen ein.
pub fn produce_and_consume(galaxy: &mut Galaxy, economy: &mut Economy, data: &GameData) {
    let balance = &data.balance.economy;
    let spread = balance.spread;
    let fuel = balance.fuel_id;
    let Economy {
        markets,
        planets,
        treasuries,
        ..
    } = economy;

    for system_index in 0..galaxy.systems.len() {
        let market = &mut markets[system_index];
        for slot in 0..galaxy.systems[system_index].planets.len() {
            let planet_id = galaxy.systems[system_index].planets[slot];
            let planet = &mut galaxy.planets[planet_id.index()];
            let Some(owner) = planet.owner else {
                continue;
            };
            if planet.population <= 0.0 {
                continue;
            }
            let local = &mut planets[planet_id.index()];
            let treasury = &mut treasuries[owner.index()];

            // Arbeitsmarkt: Reichen die Arbeitskräfte nicht, laufen alle Werke anteilig.
            let labor_supply = planet.population * balance.workforce_share;
            let labor_demand: f64 = local
                .capital
                .iter()
                .zip(&data.recipes)
                .map(|(capital, recipe)| capital * recipe.labor as f64)
                .sum();
            let employment = if labor_demand > 0.0 {
                (labor_supply / labor_demand).min(1.0)
            } else {
                1.0
            };
            local.labor_demand = labor_demand;
            local.employment = employment;
            let efactor = energy_factor(planet);
            let fuel_lift = lift_cost(data, planet, fuel);

            for (index, recipe) in data.recipes.iter().enumerate() {
                // Trend der Auslastung aus dem Vortag fortschreiben.
                local.utilization_trend[index] +=
                    0.05 * (local.utilization[index] - local.utilization_trend[index]);
                local.output[index] = 0.0;
                local.utilization[index] = 0.0;
                let capital = local.capital[index];
                if capital <= 0.0 {
                    continue;
                }
                let yield_factor = local_yield(recipe, planet);
                if yield_factor <= 0.0 {
                    continue;
                }
                let staffed = capital * employment;
                let fuel_per_unit = recipe.energy as f64 * efactor;

                // Die Nachfrage zählt auch dann, wenn die Ware fehlt: Knappheit hebt den Preis.
                for (good, amount) in &recipe.input_ids {
                    market.demand_today[good.index()] += staffed * *amount as f64;
                }
                market.demand_today[fuel.index()] += staffed * fuel_per_unit;

                // Stückrechnung zu heutigen Preisen.
                let mut cost_per_unit = fuel_per_unit * (market.ask(fuel, spread) + fuel_lift);
                for (good, amount) in &recipe.input_ids {
                    cost_per_unit += *amount as f64
                        * (market.ask(*good, spread) + lift_cost(data, planet, *good));
                }
                let output_per_unit = recipe.output.1 as f64 * yield_factor;
                let net_price = market.bid(recipe.output_id, spread)
                    - lift_cost(data, planet, recipe.output_id);
                // Deckt der Erlös nicht einmal Material und Energie, steht das Werk still.
                if output_per_unit * net_price <= cost_per_unit {
                    continue;
                }

                // Auslastung: begrenzt durch Vorprodukte, Treibstoff und Kasse der Firma.
                let mut run = 1.0f64;
                for (good, amount) in &recipe.input_ids {
                    let need = staffed * *amount as f64;
                    if need > 0.0 {
                        run = run.min(market.stock[good.index()] * BUYER_SHARE / need);
                    }
                }
                // Treibstoff: Fehlt er, läuft das Werk im Notbetrieb mit einem Bruchteil der
                // Leistung weiter. Der Engpass schmerzt, verklemmt die Wirtschaft aber nie.
                let fuel_need = staffed * fuel_per_unit;
                let mut fuel_share = 1.0f64;
                if fuel_need > 0.0 {
                    fuel_share = (market.stock[fuel.index()] * BUYER_SHARE / fuel_need).min(1.0);
                    run = run.min(fuel_share.max(balance.emergency_output));
                }
                // Absatz: Das Kontor nimmt nur ab, was sein Lager noch fasst.
                let full_output = staffed * output_per_unit;
                if full_output > 0.0 {
                    run = run.min(market.room(recipe.output_id, balance.cover_days) / full_output);
                }
                let firm = &mut local.firms[recipe.sector.index()];
                let full_cost = staffed * cost_per_unit;
                if full_cost > 0.0 {
                    run = run.min(super::to_credits(firm.cash) / full_cost);
                }
                let run = run.clamp(0.0, 1.0);
                if run <= 0.0 {
                    continue;
                }
                // Tatsächlich verbrannter Treibstoff (im Notbetrieb weniger als nötig).
                let fuel_used = (fuel_need * run).min(fuel_need * fuel_share);
                let fuel_saved =
                    (fuel_need * run - fuel_used) * (market.ask(fuel, spread) + fuel_lift);
                let full_cost = full_cost - if run > 0.0 { fuel_saved / run } else { 0.0 };

                // Einkauf.
                for (good, amount) in &recipe.input_ids {
                    let quantity = staffed * run * *amount as f64;
                    let stock = &mut market.stock[good.index()];
                    *stock = (*stock - quantity).max(0.0);
                }
                let fuel_stock = &mut market.stock[fuel.index()];
                *fuel_stock = (*fuel_stock - fuel_used).max(0.0);
                let paid_inputs = pay(&mut firm.cash, &mut market.cash, to_money(full_cost * run));

                // Verkauf an das Kontor; es zahlt, soweit seine Kasse reicht.
                let quantity = staffed * run * output_per_unit;
                market.stock[recipe.output_id.index()] += quantity;
                market.supply_today[recipe.output_id.index()] += quantity;
                let earned = pay(
                    &mut market.cash,
                    &mut firm.cash,
                    to_money(quantity * net_price),
                );
                market.turnover_today += super::to_credits(paid_inputs + earned);

                // Löhne für die geleistete Arbeit, davon Einkommensteuer an den Staat.
                let wages_due = to_money(staffed * run * recipe.labor as f64 * local.wage);
                let wages = wages_due.min(firm.cash.max(0));
                let tax = (wages as f64 * balance.income_tax).round() as Money;
                firm.cash -= wages;
                *treasury += tax;
                local.households += wages - tax;
                local.income_today += super::to_credits(wages - tax);

                let profit = super::to_credits(earned - paid_inputs - wages);
                firm.profit_today += profit;
                firm.revenue_today += super::to_credits(earned);
                local.output[index] = quantity;
                local.utilization[index] = employment * run;

                // Abbau zehrt das Vorkommen auf.
                if let Some(good) = recipe.deposit_id
                    && !data.good(good).renewable
                    && let Some(deposit) = planet
                        .deposits
                        .iter_mut()
                        .find(|deposit| deposit.good == good)
                {
                    let used = quantity * balance.depletion / deposit.richness.max(0.05) as f64;
                    deposit.reserve = (deposit.reserve - used as f32).max(0.02);
                }
            }

            // Haushalte: zuerst das Lebensnotwendige, dann der Rest, soweit das Geld reicht.
            let comfortable = planet.population * balance.cash_per_million;
            let wealth =
                (super::to_credits(local.households) / comfortable.max(1e-9)).clamp(0.3, 3.0);
            let mut budget = super::to_credits(local.households) * balance.spend_rate;
            let mut order: Vec<usize> = (0..balance.needs.len()).collect();
            order.sort_by_key(|index| !balance.needs[*index].essential);
            for index in order {
                let need = &balance.needs[index];
                let good = need.good_id;
                let appetite = if need.essential { 1.0 } else { wealth };
                let desired = planet.population * need.per_million * appetite;
                if desired <= 0.0 {
                    continue;
                }
                let unit_price = market.ask(good, spread) + 0.5 * lift_cost(data, planet, good);
                // Kaufkräftige Nachfrage: Wünsche ohne Geld bewegen keinen Preis.
                market.demand_today[good.index()] += desired.min(budget / unit_price).max(0.0);
                let quantity = desired
                    .min(market.stock[good.index()] * BUYER_SHARE)
                    .min(budget / unit_price)
                    .max(0.0);
                let paid = pay(
                    &mut local.households,
                    &mut market.cash,
                    to_money(quantity * unit_price),
                );
                let stock = &mut market.stock[good.index()];
                *stock = (*stock - quantity).max(0.0);
                budget -= super::to_credits(paid);
                market.turnover_today += super::to_credits(paid);
                // Gemessen wird gegen den Grundbedarf, nicht gegen den Wohlstandswunsch.
                let basic = planet.population * need.per_million;
                let met = (quantity / basic).min(1.0);
                local.needs_met[index] += SMOOTHING * (met - local.needs_met[index]);
            }
        }
    }
}

/// Bevölkerung wächst mit der Versorgung und schrumpft im Mangel. Neugeborene bringen
/// Bargeld mit: die einzige Geldschöpfung im Spiel, ausgewiesen in `minted`.
pub fn grow_population(galaxy: &mut Galaxy, economy: &mut Economy, data: &GameData) {
    let balance = &data.balance;
    for planet in &mut galaxy.planets {
        if planet.owner.is_none() || planet.population <= 0.0 {
            continue;
        }
        let local = &mut economy.planets[planet.id.index()];
        let essentials = local.essentials_met(&balance.economy);
        let comforts: Vec<f64> = balance
            .economy
            .needs
            .iter()
            .zip(&local.needs_met)
            .filter(|(need, _)| !need.essential)
            .map(|(_, met)| *met)
            .collect();
        let comfort = if comforts.is_empty() {
            1.0
        } else {
            comforts.iter().sum::<f64>() / comforts.len() as f64
        };
        let headroom = 1.0 - planet.population / planet.capacity;
        let rate = if essentials >= 0.95 {
            balance.population.growth_per_year * (0.5 + 0.5 * comfort) * headroom
        } else if essentials >= 0.6 {
            // Knappe Versorgung: Wachstum versiegt.
            balance.population.growth_per_year * headroom.min(1.0) * (essentials - 0.6) / 0.35 * 0.5
        } else {
            -balance.population.starvation_per_year * (0.6 - essentials) / 0.6
        };
        let delta = planet.population * rate / DAYS_PER_YEAR as f64;
        planet.population = (planet.population + delta).max(0.0);
        if delta > 0.0 {
            let minted = to_money(delta * balance.economy.cash_per_million);
            local.households += minted;
            economy.minted += minted;
        }
    }
}

/// Unter dieser Bevölkerung (Mio.) gilt eine Kolonie als erloschen.
pub const ABANDON_BELOW: f64 = 0.2;

/// Ausgestorbene Kolonien werden aufgegeben; der Planet ist wieder frei. Restgeld und
/// die letzten Bewohner gehen an die Hauptwelt, nichts verschwindet.
pub fn abandon_dead_colonies(
    galaxy: &mut Galaxy,
    economy: &mut Economy,
    capitals: &[Option<crate::ids::PlanetId>],
    data: &GameData,
) -> Vec<(crate::ids::PlanetId, crate::ids::PolityId)> {
    let mut abandoned = Vec::new();
    for index in 0..galaxy.planets.len() {
        let planet = &galaxy.planets[index];
        let Some(owner) = planet.owner else {
            continue;
        };
        let capital = capitals.get(owner.index()).copied().flatten();
        if planet.population >= ABANDON_BELOW || capital == Some(planet.id) {
            continue;
        }
        let Some(capital) = capital else {
            continue;
        };
        let leftover = galaxy.planets[index].population;
        galaxy.planets[index].population = 0.0;
        galaxy.planets[index].owner = None;
        galaxy.planets[capital.index()].population += leftover;
        let old = std::mem::replace(&mut economy.planets[index], super::PlanetEconomy::new(data));
        let cash = old.households + old.firms.iter().map(|firm| firm.cash).sum::<Money>();
        economy.planets[capital.index()].households += cash;
        abandoned.push((galaxy.planets[index].id, owner));
    }
    abandoned
}

/// Preis, auf den ein Markt bei gegebener Lagerreichweite zusteuert.
pub fn target_price(
    base_price: f64,
    stock: f64,
    daily_demand: f64,
    balance: &crate::data::EconomyBalance,
) -> f64 {
    // Ein kleiner Sockel verhindert Ausschläge bei winzigen Mengen.
    const FLOOR_UNITS: f64 = 0.5;
    let desired = daily_demand * balance.cover_days + FLOOR_UNITS;
    let cover = (stock + FLOOR_UNITS) / desired;
    let factor = cover.powf(-balance.price_elasticity);
    base_price * factor.clamp(balance.price_floor, balance.price_ceiling)
}

/// Preise folgen Angebot und Nachfrage je System, Löhne der Knappheit an Arbeitskräften.
/// Beide bewegen sich je Tag nur um einen begrenzten Schritt.
pub fn update_prices_and_wages(galaxy: &Galaxy, economy: &mut Economy, data: &GameData) {
    let balance = &data.balance.economy;
    for market in &mut economy.markets {
        for (good, def) in data.goods.iter().enumerate() {
            let demand_today = market.demand_today[good] + market.standing[good];
            market.demand[good] += SMOOTHING * (demand_today - market.demand[good]);
            market.supply[good] += SMOOTHING * (market.supply_today[good] - market.supply[good]);
            market.demand_today[good] = 0.0;
            market.supply_today[good] = 0.0;
            let target = target_price(
                def.base_price,
                market.stock[good],
                market.demand[good],
                balance,
            );
            let price = market.price[good];
            let step = price * balance.price_step;
            market.price[good] = price + (target - price).clamp(-step, step);
        }
        market.turnover += SMOOTHING * (market.turnover_today - market.turnover);
        market.turnover_today = 0.0;
    }
    for planet in &galaxy.planets {
        let local = &mut economy.planets[planet.id.index()];
        for firm in &mut local.firms {
            firm.profit += SMOOTHING * (firm.profit_today - firm.profit);
            firm.revenue += SMOOTHING * (firm.revenue_today - firm.revenue);
            firm.profit_today = 0.0;
            firm.revenue_today = 0.0;
        }
        local.income += SMOOTHING * (local.income_today - local.income);
        local.income_today = 0.0;
        if planet.owner.is_none() || planet.population <= 0.0 {
            continue;
        }
        let labor_supply = planet.population * balance.workforce_share;
        let tightness = local.labor_demand / labor_supply.max(1e-9);
        let target =
            balance.base_wage * (tightness / 0.92).clamp(balance.wage_floor, balance.wage_ceiling);
        let step = local.wage * balance.wage_step;
        local.wage += (target - local.wage).clamp(-step, step);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn balance() -> crate::data::EconomyBalance {
        GameData::load_workspace().unwrap().balance.economy
    }

    #[test]
    fn scarcity_raises_and_glut_lowers_the_target_price() {
        let balance = balance();
        let balanced = target_price(10.0, 200.0, 10.0, &balance);
        let scarce = target_price(10.0, 20.0, 10.0, &balance);
        let glut = target_price(10.0, 2000.0, 10.0, &balance);
        assert!(
            (balanced - 10.0).abs() < 0.1,
            "bei Soll-Reichweite gilt der Richtpreis"
        );
        assert!(scarce > balanced * 1.5);
        assert!(glut < balanced * 0.7);
    }

    #[test]
    fn target_price_is_monotone_and_bounded() {
        let balance = balance();
        let mut last = f64::MAX;
        for stock in [0.0, 1.0, 10.0, 50.0, 200.0, 1000.0, 1e6] {
            let price = target_price(10.0, stock, 10.0, &balance);
            assert!(price <= last, "mehr Bestand darf den Preis nie heben");
            assert!(price >= 10.0 * balance.price_floor - 1e-9);
            assert!(price <= 10.0 * balance.price_ceiling + 1e-9);
            last = price;
        }
        // Mehr Nachfrage bei gleichem Bestand hebt den Preis.
        assert!(
            target_price(10.0, 100.0, 20.0, &balance) > target_price(10.0, 100.0, 5.0, &balance)
        );
    }
}

//! Langsame Zeitskala (monatlich): Kapital wandert, Menschen wandern, öffentliche Kassen
//! werden ausgeglichen.

use super::{
    Economy, EquityFlows, Money, book_income, expected_return, factor_of, pay, recipes_of,
    to_credits, to_money, unit_economics_at,
};
use crate::data::{GameData, Sector};
use crate::galaxy::Galaxy;
use crate::ids::{HolderId, IssuerId, PlanetId, PolityId};

const MONTHS_PER_YEAR: f64 = 12.0;

/// Kapital verschleißt, verlustreiche Werke werden abgebaut, Gewinne fließen in das
/// Rezept mit der höchsten erwarteten Rendite. So spezialisieren sich Standorte von
/// selbst: Es gibt keine Gebäudeboni, nur lokale Kosten und Preise.
pub fn invest_and_depreciate(
    galaxy: &Galaxy,
    economy: &mut Economy,
    data: &GameData,
    capitals: &[Option<PlanetId>],
    funds: &mut [Money],
    day: u64,
    flows: &mut EquityFlows,
) {
    let balance = &data.balance.economy;
    let machines = balance.capital_good_id;
    let Economy {
        markets,
        planets,
        tech_yield,
        ..
    } = economy;
    for market in markets.iter_mut() {
        market.standing[machines.index()] = 0.0;
    }

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
        // Investoren rechnen mit dem Ertrag, den die Technik der Gemeinschaft hergibt.
        let tech = factor_of(tech_yield, owner);

        // Erwartete Rendite je Rezept zu heutigen Preisen und Löhnen.
        let mut returns = vec![f64::MIN; data.recipes.len()];
        for (index, recipe) in data.recipes.iter().enumerate() {
            let economics = unit_economics_at(data, planet, local.wage, market, recipe, tech);
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
                let economics = unit_economics_at(data, planet, local.wage, market, recipe, tech);
                monthly_cost += local.capital[index]
                    * (economics.inputs + economics.energy + economics.lift + economics.wages)
                    * balance.firm_reserve_days;
                if returns[index] > best.map_or(f64::MIN, |(_, value)| value) {
                    best = Some((index, returns[index]));
                }
            }
            let reserve = to_money(monthly_cost);
            let firm_cash = local.firms[sector.index()].cash;

            let mut funded = false;
            if let Some((index, expected)) = best
                && expected > balance.hurdle_per_year
            {
                let unit_cost = balance.capital_cost * market.ask(machines, balance.spread);
                // Fehlt der Firma das Geld für eine lohnende Investition, schießen die
                // Haushalte vor Ort Eigenkapital zu und erhalten dafür neue Anteile.
                // Eingezahlt wird nur, wenn die Investition auch möglich ist: Maschinen
                // liegen am Markt und es gibt Arbeitskräfte. Sonst läge das Geld brach und
                // flösse als Dividende zurück, während die Einlage die Altaktionäre verwässert.
                let labor_per_unit = data.recipes[index].labor as f64;
                let feasible = market.stock[machines.index()]
                    >= balance.capital_cost * balance.seed_investment * 2.0
                    && labor_room >= labor_per_unit * balance.seed_investment * 0.5;
                if feasible && firm_cash < reserve + to_money(unit_cost) {
                    funded = true;
                    let wanted =
                        reserve + to_money(unit_cost * balance.seed_investment) - firm_cash;
                    let offer = (local.households as f64 * 0.02) as Money;
                    let households = &mut local.households;
                    let got = pay(
                        households,
                        &mut local.firms[sector.index()].cash,
                        wanted.min(offer),
                    );
                    let issuer = IssuerId::Firm {
                        planet: planet.id,
                        sector,
                    };
                    if got > 0 {
                        flows
                            .injections
                            .push((issuer, HolderId::Public(planet.id), got));
                    }
                    // Reicht das Geld vor Ort nicht, springt die Elite der Hauptwelt ein und
                    // erhält dafür Anteile: So wächst der Besitz des Zentrums in den Kolonien.
                    if !is_capital
                        && got < wanted
                        && let Some(capital) = capitals[owner.index()]
                    {
                        let fund = &mut funds[owner.index()];
                        let extra = (wanted - got).min(*fund);
                        *fund -= extra;
                        local.firms[sector.index()].cash += extra;
                        if extra > 0 {
                            flows
                                .injections
                                .push((issuer, HolderId::Elite(capital), extra));
                        }
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
                // Der Investitionswunsch zählt als Nachfrage, auch wenn Maschinen oder Geld
                // noch fehlen: Mindestens eine Erstinvestition ist immer gemeldet, damit
                // Frachter die Maschinen bringen, die das Einzahlen erst sinnvoll machen.
                market.standing[machines.index()] += growth_cap
                    .min(investable / unit_cost)
                    .max(balance.seed_investment)
                    * balance.capital_cost
                    / 30.0;
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

            // Was über das Anderthalbfache der Betriebsmittel hinausgeht, wird großteils als
            // Dividende ausgeschüttet. Wer sie bekommt, entscheidet der Besitz an den Anteilen.
            let firm = &mut local.firms[sector.index()];
            let excess = firm.cash - (reserve + reserve / 2).max(to_money(50.0));
            // Im Monat einer Kapitalerhöhung wird nichts ausgeschüttet.
            if excess > 0 && !funded {
                let payout = excess * 7 / 10;
                firm.cash -= payout;
                flows.dividends.push((
                    IssuerId::Firm {
                        planet: planet.id,
                        sector,
                    },
                    payout,
                ));
            }
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
            // Anteil der Arbeitskräfte, die Arbeit finden. (Nicht der Anteil besetzter Stellen:
            // Wo Leute fehlen, ist die Arbeit sicher und der Lohn hoch.)
            let labor_supply = planet.population * data.balance.economy.workforce_share;
            let employed = (local.labor_demand / labor_supply.max(1e-9)).min(1.0);
            local.wage
                * (0.3 + 0.7 * employed)
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

        // Wer gehen möchte …
        let leaving: Vec<f64> = members
            .iter()
            .zip(&appeals)
            .map(|(index, appeal)| {
                if *appeal >= average {
                    return 0.0;
                }
                let share = rate * ((average - appeal) / average * 3.0).min(1.0);
                galaxy.planets[*index].population * share
            })
            .collect();
        let wanting: f64 = leaving.iter().sum();
        // … und wo Platz ist: Zuzug in die attraktiven Welten, begrenzt durch ihre
        // Tragfähigkeit.
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
        if wanting <= 0.0 || total_weight <= 0.0 {
            continue;
        }
        let arriving: Vec<f64> = members
            .iter()
            .zip(&weights)
            .map(|(index, weight)| {
                let planet = &galaxy.planets[*index];
                let room = (planet.capacity - planet.population).max(0.0);
                (wanting * weight / total_weight).min(room * 0.98)
            })
            .collect();
        let settled: f64 = arriving.iter().sum();
        // Es zieht nur um, wer auch ankommt: Niemand geht verloren, keine Welt läuft über.
        let moving = settled / wanting;
        let mut purse: Money = 0;
        for (position, index) in members.iter().enumerate() {
            let gone = leaving[position] * moving;
            if gone <= 0.0 {
                continue;
            }
            let planet = &mut galaxy.planets[*index];
            let share = gone / planet.population.max(1e-9);
            let cash = (economy.planets[*index].households as f64 * share) as Money;
            planet.population -= gone;
            economy.planets[*index].households -= cash;
            purse += cash;
        }
        // Die Umziehenden nehmen ihr Geld mit; der Rundungsrest geht an die erste Zielwelt.
        let mut paid: Money = 0;
        let mut first_target = None;
        for (position, index) in members.iter().enumerate() {
            if arriving[position] <= 0.0 {
                continue;
            }
            first_target.get_or_insert(*index);
            let cash = (purse as f64 * arriving[position] / settled) as Money;
            galaxy.planets[*index].population += arriving[position];
            economy.planets[*index].households += cash;
            paid += cash;
        }
        match first_target {
            Some(index) => economy.planets[index].households += purse - paid,
            // Kein Ziel: Dann ist auch niemand gegangen und der Topf leer.
            None => debug_assert_eq!(purse, 0),
        }
    }
}

/// Kontore führen Überschüsse als Zölle ab, Staaten geben aus, was über ihre Rücklage
/// hinausgeht (bis zum Ausbau des Haushalts: als Leistungen an die Haushalte).
pub fn settle_public_accounts(
    galaxy: &Galaxy,
    economy: &mut Economy,
    data: &GameData,
    flows: &mut EquityFlows,
) {
    let balance = &data.balance.economy;
    economy.sync_polities();
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
        // Ein Kontor ohne Kasse kann keinem Frachter etwas abkaufen. Der Staat schießt
        // dann Kapital nach und erhält dafür neue Anteile.
        let floor = to_money(600.0);
        if market.cash < floor {
            let needed = to_money(1500.0) - market.cash;
            let got = pay(
                &mut economy.treasuries[owner.index()],
                &mut market.cash,
                needed,
            );
            if got > 0 {
                flows
                    .injections
                    .push((IssuerId::Kontor(system.id), HolderId::State(owner), got));
            }
        }
        if excess > 0 {
            // Die Hälfte ist Zoll, die andere Hälfte Gewinn der Anteilseigner des Kontors.
            let tariff = pay(
                &mut market.cash,
                &mut economy.treasuries[owner.index()],
                excess / 2,
            );
            let dividend = excess - tariff;
            market.cash -= dividend;
            flows
                .dividends
                .push((IssuerId::Kontor(system.id), dividend));
            book_income(&mut economy.state_income_running, owner, tariff);
        }
    }
    for polity in 0..economy.treasuries.len() {
        let id = PolityId::from_index(polity);
        let population = galaxy.population_of(id);
        if population <= 0.0 {
            continue;
        }
        let reserve = state_reserve(economy, data, id, population);
        let excess = economy.treasuries[polity] - reserve;
        if excess <= 0 {
            continue;
        }
        pay_households(galaxy, economy, id, excess / 2);
    }
}

/// Rücklage, die ein Staat mit `population` Millionen Einwohnern hält: ein Sockel je
/// Einwohner, mindestens aber die befohlene (`treasury_targets`). Nur was darüber liegt,
/// gibt er aus.
pub fn state_reserve(
    economy: &Economy,
    data: &GameData,
    polity: PolityId,
    population: f64,
) -> Money {
    let ordered = economy
        .treasury_targets
        .get(polity.index())
        .copied()
        .unwrap_or(0);
    to_money(population * data.balance.economy.state_reserve_per_million).max(ordered)
}

/// Der Staat zahlt `budget` an die Haushalte seiner Welten, verteilt nach Bevölkerung:
/// Leistungen, Gehälter, Sold. Gezahlt wird nie mehr, als die Kasse hergibt; die Kasse
/// sinkt um genau den Betrag, den die Haushalte erhalten. Gibt ihn zurück.
pub fn pay_households(
    galaxy: &Galaxy,
    economy: &mut Economy,
    polity: PolityId,
    budget: Money,
) -> Money {
    let Some(treasury) = economy.treasuries.get(polity.index()).copied() else {
        return 0;
    };
    let budget = budget.min(treasury);
    let population = galaxy.population_of(polity);
    if budget <= 0 || population <= 0.0 {
        return 0;
    }
    let mut spent: Money = 0;
    for planet in galaxy.planets_of(polity) {
        let share = (budget as f64 * planet.population / population) as Money;
        economy.planets[planet.id.index()].households += share;
        economy.planets[planet.id.index()].income_today += to_credits(share) / 30.0;
        spent += share;
    }
    economy.treasuries[polity.index()] -= spent;
    spent
}

/// Geldmengensteuerung: Wächst die Wirtschaft, ohne dass Geld nachkommt, fallen die Preise
/// bis an ihre Böden. Liegt das Preisniveau am Heimatmarkt einer Gemeinschaft unter dem
/// Ziel, schöpft ihre Notenbank Geld und gibt es dem Staat, der es über seine Ausgaben in
/// Umlauf bringt. Die Schöpfung ist ausgewiesen (`minted`); sonst entsteht nirgends Geld.
pub fn steer_money_supply(
    galaxy: &Galaxy,
    economy: &mut Economy,
    data: &GameData,
    capitals: &[Option<PlanetId>],
) {
    let balance = &data.balance.economy;
    for (polity, capital) in capitals.iter().enumerate() {
        let Some(capital) = capital else {
            continue;
        };
        let market = &economy.markets[galaxy.planet(*capital).system.index()];
        // Preisniveau der Güter, die dort tatsächlich gehandelt werden.
        let (mut level, mut weight) = (0.0, 0.0);
        for (index, good) in data.goods.iter().enumerate() {
            let volume = market.demand[index] * good.base_price;
            level += volume * market.price[index] / good.base_price;
            weight += volume;
        }
        if weight <= 0.0 {
            continue;
        }
        let level = level / weight;
        let gap = ((balance.price_target - level) / 0.3).clamp(0.0, 1.0);
        if gap <= 0.0 {
            continue;
        }
        let id = PolityId::from_index(polity);
        let households: Money = galaxy
            .planets_of(id)
            .map(|planet| economy.planets[planet.id.index()].households)
            .sum();
        let minted = (households as f64 * balance.mint_per_month * gap) as Money;
        economy.treasuries[polity] += minted;
        economy.minted += minted;
    }
}

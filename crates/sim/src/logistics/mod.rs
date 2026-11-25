//! Logistik: Frachter der Handelshäuser fahren echte Routen.
//!
//! Ein Frachter kauft dort, wo ein Gut billig ist, fährt Kante für Kante durch das
//! Routennetz und verkauft dort, wo es teuer ist. Blockierte Systeme sind gesperrt,
//! Kaperer bringen Frachter auf, Geleitschutz hält sie ab. Fehlt die Lieferung, fehlen
//! den Fabriken die Vorprodukte: Die Blockade wirkt über die Ware, nicht über einen Malus.

mod plugin;

pub use plugin::LogisticsPlugin;

use crate::chronicle::{Chronicle, ChronicleKind};
use crate::data::GameData;
use crate::diplomacy::Diplomacy;
use crate::economy::production::target_price;
use crate::economy::{Economy, Money, pay, to_credits, to_money};
use crate::fleet::Fleets;
use crate::galaxy::Galaxy;
use crate::ids::{FreighterId, GoodId, HouseId, PolityId, SystemId};
use crate::polity::Polities;
use crate::rng::{Domain, Rng};
use bevy_ecs::prelude::Resource;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TradeHouse {
    pub id: HouseId,
    pub polity: PolityId,
    pub name: String,
    pub cash: Money,
    pub home: SystemId,
    /// Geglätteter Tagesgewinn in Credits.
    pub profit: f64,
    pub profit_today: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Cargo {
    pub good: GoodId,
    pub quantity: f64,
    /// Einkaufskosten samt Treibstoff, für die Gewinnrechnung.
    pub cost: Money,
    pub origin: SystemId,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Freighter {
    pub id: FreighterId,
    pub house: HouseId,
    /// Aktuelles System, unterwegs das zuletzt verlassene.
    pub system: SystemId,
    /// Verbleibender Weg; das erste Element ist das nächste System.
    pub route: Vec<SystemId>,
    /// Fortschritt zum nächsten System (0..1).
    pub progress: f32,
    pub cargo: Option<Cargo>,
    pub idle_days: u32,
    pub alive: bool,
}

/// So viele Frachter behält ein Handelshaus auch in schlechten Zeiten.
pub const MIN_FREIGHTERS: usize = 3;

/// Handelsstrom zwischen zwei Systemen, für die Anzeige der Warenflüsse.
pub type FlowKey = (SystemId, SystemId, GoodId);

#[derive(Resource, Clone, Debug, Default, Serialize, Deserialize)]
pub struct Logistics {
    pub houses: Vec<TradeHouse>,
    pub freighters: Vec<Freighter>,
    /// Gelieferte Mengen, monatlich halbiert (gleitendes Bild der letzten Monate).
    pub flows: BTreeMap<FlowKey, f64>,
    pub captured: u64,
}

impl Logistics {
    pub fn money(&self) -> Money {
        self.houses.iter().map(|house| house.cash).sum()
    }

    pub fn active_freighters(&self) -> usize {
        self.freighters
            .iter()
            .filter(|freighter| freighter.alive)
            .count()
    }
}

pub fn setup_logistics(data: &GameData, galaxy: &Galaxy, polities: &Polities) -> Logistics {
    let balance = &data.balance.logistics;
    let mut logistics = Logistics::default();
    for polity in &polities.list {
        let Some(capital) = polity.capital else {
            continue;
        };
        let home = galaxy.planet(capital).system;
        let house = HouseId::from_index(logistics.houses.len());
        logistics.houses.push(TradeHouse {
            id: house,
            polity: polity.id,
            name: format!("Handelshaus {}", galaxy.system(home).name),
            cash: to_money(4000.0),
            home,
            profit: 0.0,
            profit_today: 0.0,
        });
        let count = (galaxy.population_of(polity.id) / 1000.0 * balance.freighters_per_billion)
            .round()
            .max(2.0) as usize;
        for _ in 0..count {
            let id = FreighterId::from_index(logistics.freighters.len());
            logistics.freighters.push(Freighter {
                id,
                house,
                system: home,
                route: Vec::new(),
                progress: 0.0,
                cargo: None,
                idle_days: 0,
                alive: true,
            });
        }
    }
    logistics
}

/// Wahrscheinlichkeit, dass ein ankommender Frachter aufgebracht wird. Geleitschutz zählt
/// doppelt: Wer eskortiert, schützt mehr, als ein gleich starker Kaperer bedroht.
pub fn raid_chance(base: f64, raiders: f32, escorts: f32) -> f64 {
    if raiders <= 0.0 {
        return 0.0;
    }
    base * (raiders / (raiders + 2.0 * escorts.max(0.0))) as f64
}

/// Darf ein Frachter der Gemeinschaft `polity` das System betreten?
fn open_for(
    polity: PolityId,
    system: SystemId,
    owners: &[Option<PolityId>],
    fleets: &Fleets,
    diplomacy: &Diplomacy,
) -> bool {
    !fleets.is_blockaded(system)
        && owners[system.index()].is_none_or(|owner| !diplomacy.at_war(owner, polity))
}

/// Tagesschritt aller Frachter in ID-Reihenfolge.
pub fn move_and_trade(
    galaxy: &Galaxy,
    economy: &mut Economy,
    logistics: &mut Logistics,
    fleets: &Fleets,
    diplomacy: &Diplomacy,
    polities: &Polities,
    chronicle: &mut Chronicle,
    data: &GameData,
    seed: u64,
    tick: u64,
) {
    let balance = &data.balance.logistics;
    let day = tick / crate::clock::TICKS_PER_DAY;
    let owners: Vec<Option<PolityId>> = galaxy
        .systems
        .iter()
        .map(|system| galaxy.system_owner(system.id))
        .collect();

    // Was schon unterwegs ist, drückt den erwarteten Preis am Ziel: So fahren nicht alle
    // Frachter derselben Preisdifferenz hinterher.
    let goods = data.goods.len();
    let mut incoming = vec![0.0f64; galaxy.systems.len() * goods];
    for freighter in &logistics.freighters {
        if let (true, Some(cargo), Some(target)) =
            (freighter.alive, &freighter.cargo, freighter.route.last())
        {
            incoming[target.index() * goods + cargo.good.index()] += cargo.quantity;
        }
    }

    let Logistics {
        houses,
        freighters,
        flows,
        captured,
    } = logistics;
    for freighter in freighters.iter_mut() {
        if !freighter.alive {
            continue;
        }
        let house = &mut houses[freighter.house.index()];
        let polity = house.polity;

        // --- Fahrt ---
        if !freighter.route.is_empty() {
            let next = freighter.route[0];
            if !open_for(polity, next, &owners, fleets, diplomacy) {
                // Das nächste System ist gesperrt: am letzten Ort bleiben und neu planen.
                freighter.route.clear();
                freighter.progress = 0.0;
            } else if let Some(lane) = galaxy.lane_between(freighter.system, next) {
                // Heuer für den Fahrtag, gezahlt an die Haushalte des Heimathafens.
                if let Some(capital) = polities.get(polity).capital {
                    pay(
                        &mut house.cash,
                        &mut economy.planets[capital.index()].households,
                        to_money(balance.upkeep_per_day),
                    );
                }
                freighter.progress += balance.speed / galaxy.lane(lane).length.max(1.0);
                if freighter.progress < 1.0 {
                    continue;
                }
                freighter.system = freighter.route.remove(0);
                freighter.progress = 0.0;

                // Kaperer im System?
                let (raiders, escorts, raider) =
                    fleets.raid_pressure(diplomacy, freighter.system, polity);
                if let Some(raider) = raider {
                    let chance = raid_chance(balance.raid_chance, raiders, escorts);
                    let mut rng = Rng::stream(seed, Domain::Logistics, freighter.id.0 as u64, day);
                    if rng.chance(chance) {
                        freighter.alive = false;
                        *captured += 1;
                        // Die Prise landet auf dem Heimatmarkt des Kaperers.
                        if let (Some(cargo), Some(capital)) =
                            (freighter.cargo.take(), polities.get(raider).capital)
                        {
                            let prize_market = galaxy.planet(capital).system;
                            economy.markets[prize_market.index()].stock[cargo.good.index()] +=
                                cargo.quantity;
                        }
                        chronicle.push(
                            tick,
                            ChronicleKind::War,
                            Some(raider),
                            Some(freighter.system),
                            format!(
                                "Kaperer der Gemeinschaft {} bringen einen Frachter von {} auf.",
                                polities.get(raider).name,
                                house.name
                            ),
                        );
                        continue;
                    }
                }
                if !freighter.route.is_empty() {
                    continue;
                }
            } else {
                freighter.route.clear();
                freighter.progress = 0.0;
            }
        }

        // --- Im Hafen ---
        let here = freighter.system;
        let here_open = open_for(polity, here, &owners, fleets, diplomacy);
        let spread = data.balance.economy.spread;

        if let Some(cargo) = freighter.cargo.as_mut() {
            if here_open && owners[here.index()].is_some() {
                // Verkaufen, soweit die Kasse des Kontors reicht.
                let market = &mut economy.markets[here.index()];
                let bid = market.bid(cargo.good, spread);
                let affordable = to_credits(market.cash) / bid.max(1e-9);
                let room = market.room(cargo.good, data.balance.economy.cover_days);
                let sold = cargo.quantity.min(affordable).min(room).max(0.0);
                if sold > 0.0 {
                    let paid = pay(&mut market.cash, &mut house.cash, to_money(sold * bid));
                    market.stock[cargo.good.index()] += sold;
                    market.supply_today[cargo.good.index()] += sold;
                    market.turnover_today += to_credits(paid);
                    let share = sold / cargo.quantity;
                    let cost_share = (cargo.cost as f64 * share) as Money;
                    house.profit_today += to_credits(paid - cost_share);
                    cargo.cost -= cost_share;
                    cargo.quantity -= sold;
                    if cargo.origin != here {
                        *flows.entry((cargo.origin, here, cargo.good)).or_default() += sold;
                    }
                }
            }
            if cargo.quantity > 0.01 {
                // Rest an Bord: das beste erreichbare Ziel für genau diese Ware suchen.
                let good = cargo.good;
                let (distance, previous) = galaxy
                    .distances_from(here, |s| open_for(polity, s, &owners, fleets, diplomacy));
                let mut best: Option<(SystemId, f64)> = None;
                for system in &galaxy.systems {
                    if system.id == here
                        || owners[system.index()].is_none()
                        || !distance[system.index()].is_finite()
                    {
                        continue;
                    }
                    let market = &economy.markets[system.index()];
                    let days = (distance[system.index()] / balance.speed) as f64 + 1.0;
                    let score = market.bid(good, spread) / days.sqrt();
                    if to_credits(market.cash) > 10.0
                        && market.room(good, data.balance.economy.cover_days) > 1.0
                        && best.is_none_or(|(_, best_score)| score > best_score)
                    {
                        best = Some((system.id, score));
                    }
                }
                match best {
                    Some((target, _)) => {
                        freighter.route = Galaxy::route_to(&previous, target);
                        freighter.idle_days = 0;
                    }
                    None => freighter.idle_days += 1,
                }
                continue;
            }
            freighter.cargo = None;
        }

        if !here_open {
            freighter.idle_days += 1;
            continue;
        }

        // --- Neue Fahrt planen ---
        let (distance, previous) =
            galaxy.distances_from(here, |s| open_for(polity, s, &owners, fleets, diplomacy));
        let home_market = &economy.markets[here.index()];
        let fuel = data.balance.economy.fuel_id;
        let fuel_price = home_market.ask(fuel, spread);
        let mut best: Option<(SystemId, GoodId, f64, f64)> = None;
        if owners[here.index()].is_some() {
            for (good_index, def) in data.goods.iter().enumerate() {
                let good = GoodId::from_index(good_index);
                let ask = home_market.ask(good, spread);
                let quantity = (balance.freighter_capacity / def.mass as f64)
                    .min(home_market.stock[good_index] * 0.4)
                    .min(to_credits(house.cash) * 0.8 / ask);
                if quantity < 1.0 {
                    continue;
                }
                for system in &galaxy.systems {
                    let index = system.index();
                    if system.id == here || owners[index].is_none() || !distance[index].is_finite()
                    {
                        continue;
                    }
                    let market = &economy.markets[index];
                    // Geliefert wird, was der Zielmarkt sinnvoll aufnimmt: die Lücke bis zum
                    // Anderthalbfachen seines Soll-Lagers, abzüglich dessen, was schon
                    // unterwegs ist. Kleine Märkte bekommen so kleine, aber lohnende Ladungen.
                    let wanted = market.demand[good_index] * data.balance.economy.cover_days * 1.5
                        + 2.0
                        - market.stock[good_index]
                        - incoming[index * goods + good_index];
                    let quantity = quantity.min(wanted);
                    if quantity < 1.0 {
                        continue;
                    }
                    // Erwarteter Preis bei Ankunft: heutiger Preis, gedeckelt durch das,
                    // was nach allen unterwegs befindlichen Lieferungen zu erwarten ist.
                    let expected_stock = market.stock[good_index]
                        + incoming[index * goods + good_index]
                        + quantity * 0.5;
                    let later = target_price(
                        def.base_price,
                        expected_stock,
                        market.demand[good_index],
                        &data.balance.economy,
                    );
                    let sell = market.price[good_index].min(later) * (1.0 - spread);
                    let days = (distance[index] / balance.speed) as f64 + 1.0;
                    let fuel_cost = distance[index] as f64 * balance.fuel_per_distance * fuel_price;
                    let profit = (quantity * (sell - ask) - fuel_cost) / days;
                    if profit > balance.min_profit_per_day
                        && best.is_none_or(|(_, _, _, best_profit)| profit > best_profit)
                    {
                        best = Some((system.id, good, quantity, profit));
                    }
                }
            }
        }

        let Some((target, good, quantity, _)) = best else {
            freighter.idle_days += 1;
            // Wer lange nichts findet, kehrt in den Heimathafen zurück.
            if freighter.idle_days > 10
                && here != house.home
                && distance[house.home.index()].is_finite()
            {
                freighter.route = Galaxy::route_to(&previous, house.home);
                freighter.idle_days = 0;
            }
            continue;
        };

        // Einkauf: Ware und Treibstoff für die Strecke.
        let market = &mut economy.markets[here.index()];
        let ask = market.ask(good, spread);
        let paid = pay(&mut house.cash, &mut market.cash, to_money(quantity * ask));
        let bought = to_credits(paid) / ask;
        market.stock[good.index()] = (market.stock[good.index()] - bought).max(0.0);
        market.demand_today[good.index()] += bought;
        market.turnover_today += to_credits(paid);
        let fuel_wanted = distance[target.index()] as f64 * balance.fuel_per_distance;
        let fuel_bought = fuel_wanted.min(market.stock[fuel.index()] * 0.5);
        let fuel_paid = pay(
            &mut house.cash,
            &mut market.cash,
            to_money(fuel_bought * fuel_price),
        );
        market.stock[fuel.index()] = (market.stock[fuel.index()] - fuel_bought).max(0.0);
        market.demand_today[fuel.index()] += fuel_wanted;
        house.profit_today -= to_credits(fuel_paid);
        incoming[target.index() * goods + good.index()] += bought;
        freighter.cargo = Some(Cargo {
            good,
            quantity: bought,
            cost: paid,
            origin: here,
        });
        freighter.route = Galaxy::route_to(&previous, target);
        freighter.idle_days = 0;
    }

    for house in houses.iter_mut() {
        house.profit += 0.1 * (house.profit_today - house.profit);
        house.profit_today = 0.0;
    }
}

/// Monatlich: Handelsströme verblassen, Häuser schütten Überschüsse aus, und wer voll
/// ausgelastet ist, baut neue Frachter aus echten Bauteilen vom Heimatmarkt.
pub fn expand_fleets_of_trade(
    galaxy: &Galaxy,
    economy: &mut Economy,
    logistics: &mut Logistics,
    data: &GameData,
) {
    logistics.flows.retain(|_, volume| {
        *volume *= 0.5;
        *volume > 0.5
    });
    let balance = &data.balance.logistics;
    let spread = data.balance.economy.spread;
    let (Some(hull), Some(engines)) = (data.good_id("rumpfsegmente"), data.good_id("antriebe"))
    else {
        return;
    };
    for market in economy.markets.iter_mut() {
        market.standing[hull.index()] = 0.0;
        market.standing[engines.index()] = 0.0;
    }
    for house_index in 0..logistics.houses.len() {
        let id = HouseId::from_index(house_index);
        let (mut ships, mut idle) = (0usize, 0usize);
        for freighter in logistics
            .freighters
            .iter()
            .filter(|f| f.alive && f.house == id)
        {
            ships += 1;
            if freighter.idle_days > 2 {
                idle += 1;
            }
        }
        let house = &mut logistics.houses[house_index];

        let population = galaxy.population_of(house.polity);
        if population <= 0.0 {
            continue;
        }

        // Ausschüttung: Was über die Betriebsmittel hinausgeht, geht an die Eigentümer
        // (bis Meilenstein 3: die Haushalte der Gemeinschaft, nach Köpfen). Fehlt dem Haus
        // das Betriebskapital, schießen dieselben Haushalte nach: Ein Handelshaus kann
        // schrumpfen, aber nicht für immer verschwinden.
        let working = to_money(3000.0 + 400.0 * ships as f64);
        let floor = to_money(1500.0);
        let surplus = house.cash - working;
        let shortfall = floor - house.cash;
        if surplus > 0 {
            let mut paid: Money = 0;
            for planet in galaxy.planets_of(house.polity) {
                let share = (surplus as f64 * planet.population / population) as Money;
                economy.planets[planet.id.index()].households += share;
                paid += share;
            }
            house.cash -= paid;
        } else if shortfall > 0 {
            for planet in galaxy.planets_of(house.polity) {
                let local = &mut economy.planets[planet.id.index()];
                let wanted = (shortfall as f64 * planet.population / population) as Money;
                let offer = (local.households as f64 * 0.01) as Money;
                pay(&mut local.households, &mut house.cash, wanted.min(offer));
            }
        }

        // Dauerhaft unausgelastete Flotten schrumpfen, aber nie unter eine Mindestgröße.
        if ships > MIN_FREIGHTERS && house.profit <= 0.0 && idle * 2 > ships {
            if let Some(freighter) = logistics
                .freighters
                .iter_mut()
                .filter(|f| f.alive && f.house == id && f.cargo.is_none())
                .max_by_key(|f| (f.idle_days, std::cmp::Reverse(f.id)))
            {
                freighter.alive = false;
            }
            continue;
        }
        let house = &mut logistics.houses[house_index];

        // Neue Frachter nur, wenn die vorhandenen Arbeit haben und Geld verdienen.
        if ships == 0
            || house.profit / (ships as f64) < balance.min_profit_per_day
            || idle * 5 > ships
        {
            continue;
        }
        let market = &mut economy.markets[house.home.index()];
        let price = balance.freighter_hull * market.ask(hull, spread)
            + balance.freighter_engines * market.ask(engines, spread);
        if to_credits(house.cash) < price * 3.0 {
            continue;
        }
        // Der Wunsch zählt als Nachfrage, auch wenn die Werften noch nichts liefern.
        market.standing[hull.index()] += balance.freighter_hull / 30.0;
        market.standing[engines.index()] += balance.freighter_engines / 30.0;
        if market.stock[hull.index()] < balance.freighter_hull
            || market.stock[engines.index()] < balance.freighter_engines
        {
            continue;
        }
        market.stock[hull.index()] -= balance.freighter_hull;
        market.stock[engines.index()] -= balance.freighter_engines;
        pay(&mut house.cash, &mut market.cash, to_money(price));
        let id = FreighterId::from_index(logistics.freighters.len());
        let home = house.home;
        logistics.freighters.push(Freighter {
            id,
            house: HouseId::from_index(house_index),
            system: home,
            route: Vec::new(),
            progress: 0.0,
            cargo: None,
            idle_days: 0,
            alive: true,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escorts_cut_the_raid_chance() {
        assert_eq!(raid_chance(0.4, 0.0, 0.0), 0.0);
        assert!((raid_chance(0.4, 50.0, 0.0) - 0.4).abs() < 1e-9);
        let even = raid_chance(0.4, 50.0, 50.0);
        assert!(
            (even - 0.4 / 3.0).abs() < 1e-6,
            "gleich starker Geleitschutz drittelt"
        );
        assert!(raid_chance(0.4, 50.0, 500.0) < 0.02);
    }
}

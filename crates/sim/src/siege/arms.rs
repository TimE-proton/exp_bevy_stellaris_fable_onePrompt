//! Rüstung der Welten: die stehende Verteidigung als Anspruch des Staatshaushalts.
//!
//! Der Anspruch [`Claim::Defense`] steht im Rang direkt hinter der Flotte. Zum
//! Monatsabschluss meldet [`defense_claim`] den Bedarf; aus der Bewilligung zahlt
//! [`run_arms_month`] am Monatsbeginn Sold und Unterhalt, hebt Reguläre aus und baut
//! Anlagen aus, und [`procure_day`] füllt Tag für Tag die Depots. Was nicht bezahlt wird,
//! verfällt: Reguläre gehen, Depots bleiben leer, Anlagen wachsen nicht.

use super::{DefensePlan, PlanetDefense, Sieges};
use crate::clock::DAYS_PER_MONTH;
use crate::data::{GameData, SiegeBalance};
use crate::economy::{Claim, Economy, Market, Money, purchase, to_credits, to_money};
use crate::galaxy::Galaxy;
use crate::ids::{GoodId, PlanetId, PolityId};
use crate::polity::Polities;
use crate::tech::Research;

/// Anteil der Regulären einer Welt, der je Monat geht, wenn Sold und Unterhalt ganz
/// ausbleiben; bei teilweiser Zahlung entsprechend weniger.
pub const UNPAID_DECAY: f64 = 0.05;
/// Unter diese Bevölkerung (Millionen) hebt keine Welt Reguläre aus.
const RECRUIT_FLOOR: f64 = 2.0;
/// Anteil der Lücke im Depot, den ein Monat täglicher Käufe schließt, für den Voranschlag.
const MONTH_FILL: f64 = 0.65;

/// Die vier Anlagen einer Welt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Works {
    Stations,
    Guns,
    Shield,
    Bunkers,
}

impl Works {
    pub const ALL: [Works; 4] = [Works::Stations, Works::Guns, Works::Shield, Works::Bunkers];

    pub fn label(self) -> &'static str {
        match self {
            Works::Stations => "Orbitalstationen",
            Works::Guns => "Bodengeschütze",
            Works::Shield => "Schild",
            Works::Bunkers => "Bunker",
        }
    }

    /// Güter je Stufe; Stufe n kostet das n-Fache.
    fn bundle(self, balance: &SiegeBalance) -> &[(GoodId, f64)] {
        match self {
            Works::Stations => &balance.station_ids,
            Works::Guns => &balance.gun_ids,
            Works::Shield => &balance.shield_ids,
            Works::Bunkers => &balance.bunker_ids,
        }
    }

    /// Intakte Stufe.
    pub fn level(self, defense: &PlanetDefense) -> f64 {
        match self {
            Works::Stations => defense.stations as f64,
            Works::Guns => defense.guns as f64,
            Works::Shield => defense.shield as f64,
            Works::Bunkers => defense.bunkers as f64,
        }
    }

    fn set_level(self, defense: &mut PlanetDefense, level: u8) {
        match self {
            Works::Stations => defense.stations = level as f32,
            Works::Guns => defense.guns = level as f32,
            Works::Shield => defense.shield = level,
            Works::Bunkers => defense.bunkers = level,
        }
    }

    /// Stufe, die der Plan nennt.
    pub fn planned(self, plan: &DefensePlan) -> u8 {
        match self {
            Works::Stations => plan.stations,
            Works::Guns => plan.guns,
            Works::Shield => plan.shield,
            Works::Bunkers => plan.bunkers,
        }
    }
}

/// Höchste Ausbaustufen einer Gemeinschaft nach ihrem Stand der Technik, in der
/// Reihenfolge von [`Works::ALL`].
pub fn level_caps(balance: &SiegeBalance, research: &Research, polity: PolityId) -> [u8; 4] {
    // Reihenfolge der Schwellen wie in `Gate`: Bunker, Stützfeuer, Tunnel, Schild.
    let knows = |gate: usize| research.knows(polity, balance.gate_ids[gate]);
    let pick = |(without, with): (u8, u8), gate: usize| if knows(gate) { with } else { without };
    [
        balance.station_max,
        balance.gun_max,
        pick(balance.shield_max, 3),
        pick(balance.bunker_max, 0),
    ]
}

/// Befehl: Ausbauziel einer eigenen Welt. Die Stufen wachsen monatlich darauf zu, soweit
/// Bewilligung und Markt reichen; was über dem Ziel liegt, wird abgebaut.
pub fn set_defense_plan(
    data: &GameData,
    galaxy: &Galaxy,
    research: &Research,
    sieges: &mut Sieges,
    polity: PolityId,
    planet: PlanetId,
    plan: &DefensePlan,
) -> Result<(), String> {
    let balance = &data.balance.siege;
    let Some(target) = galaxy.planets.get(planet.index()) else {
        return Err("Planet unbekannt".into());
    };
    if target.owner != Some(polity) {
        return Err(format!(
            "{} gehört nicht der eigenen Gemeinschaft",
            target.name
        ));
    }
    let caps = level_caps(balance, research, polity);
    for (works, cap) in Works::ALL.into_iter().zip(caps) {
        if works.planned(plan) > cap {
            return Err(format!(
                "Ausbaustufe nicht verfügbar: {} höchstens Stufe {}",
                works.label(),
                cap
            ));
        }
    }
    if !(plan.regulars.is_finite() && plan.regulars >= 0.0) {
        return Err("die Zahl der Regulären darf nicht negativ sein".into());
    }
    let defense = sieges.defense(planet);
    // Was schon steht, darf der Plan immer nennen: So lässt sich eine Garnison halten,
    // die durch Landung oder Eroberung über die Aushebungsgrenze gewachsen ist.
    let most = (balance.regulars_cap * defense.militia).max(defense.regulars);
    if plan.regulars as f64 > most + 1e-6 {
        return Err(format!(
            "Ausbaustufe nicht verfügbar: {} trägt höchstens {:.1} reguläre Verbände",
            target.name, most
        ));
    }
    if plan.depot_days > balance.depot_days_cap {
        return Err(format!(
            "Ausbaustufe nicht verfügbar: Depot höchstens {} Tage",
            balance.depot_days_cap
        ));
    }
    sieges.defense_mut(planet).plan = plan.clone();
    Ok(())
}

/// Sold und Unterhalt je Monat für die stehende Verteidigung einer Welt, in Credits.
pub fn planet_upkeep(balance: &SiegeBalance, defense: &PlanetDefense) -> f64 {
    let levels: f64 = Works::ALL.iter().map(|works| works.level(defense)).sum();
    balance.pay * defense.regulars.max(0.0) + balance.level_upkeep * levels
}

/// Sold und Unterhalt je Monat für alles, was eine Gemeinschaft an Verteidigung und
/// Truppen hält: ihre Welten, die Truppen an Bord ihrer Flotten und ihre Landungstruppen.
pub fn upkeep_due(
    data: &GameData,
    galaxy: &Galaxy,
    fleet_owner: &dyn Fn(crate::ids::FleetId) -> Option<PolityId>,
    sieges: &Sieges,
    polity: PolityId,
) -> Money {
    let balance = &data.balance.siege;
    let worlds: f64 = galaxy
        .planets_of(polity)
        .map(|planet| planet_upkeep(balance, sieges.defense(planet.id)))
        .sum();
    let aboard: f64 = sieges
        .embarked
        .iter()
        .filter(|(fleet, _)| fleet_owner(**fleet) == Some(polity))
        .map(|(_, force)| force.troops.max(0.0))
        .sum();
    let ashore: f64 = sieges
        .active
        .values()
        .filter(|siege| siege.attacker == polity)
        .map(|siege| siege.army.max(0.0))
        .sum();
    to_money(worlds + balance.pay * (aboard + ashore))
}

/// Kosten eines Gütersatzes zum heutigen Briefkurs, in Credits.
fn bundle_cost(market: &Market, bundle: &[(GoodId, f64)], spread: f64) -> f64 {
    bundle
        .iter()
        .map(|(good, amount)| amount * market.ask(*good, spread))
        .sum()
}

/// Wie viele Sätze eines Gütersatzes der Markt heute hergibt: je Kauf höchstens der Anteil
/// `buy_share` des Bestands.
fn sets_in_stock(market: &Market, bundle: &[(GoodId, f64)], buy_share: f64) -> f64 {
    bundle
        .iter()
        .filter(|(_, amount)| *amount > 0.0)
        .map(|(good, amount)| market.stock[good.index()] * buy_share / amount)
        .fold(f64::INFINITY, f64::min)
}

/// Was einer Welt heute zum Plan fehlt.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Shortfall {
    /// Verbandstage, die dem Depot zum Ziel fehlen.
    pub depot: f64,
    /// Reguläre, die dieser Monat ausheben könnte.
    pub recruits: f64,
    /// Nächste Stufe je Anlage, wenn der Plan über dem Bestand liegt.
    pub next_level: [Option<u8>; 4],
}

/// Was der Welt zum Plan fehlt. Unter Belagerung wird nichts aufgefüllt.
pub fn shortfall(
    balance: &SiegeBalance,
    defense: &PlanetDefense,
    population: f64,
    besieged: bool,
) -> Shortfall {
    if besieged {
        return Shortfall::default();
    }
    let garrison = defense.militia + defense.regulars;
    let depot = (defense.plan.depot_days as f64 - defense.depot).max(0.0) * garrison;
    let people = ((population - RECRUIT_FLOOR) / balance.pop_per_strength).max(0.0);
    let recruits = (defense.plan.regulars as f64 - defense.regulars)
        .min(balance.recruit_per_month * defense.militia + 1.0)
        .min(people)
        .max(0.0);
    let next_level = Works::ALL.map(|works| {
        let level = works.level(defense);
        let next = level.floor() as u8 + 1;
        (next <= works.planned(&defense.plan)).then_some(next)
    });
    Shortfall {
        depot,
        recruits,
        next_level,
    }
}

/// Was die Verteidigung einer Gemeinschaft im beginnenden Monat vom Staatshaushalt
/// verlangt: Sold und Unterhalt, und was Depots, Aushebung und Ausbau zu heutigen Preisen
/// kosten, soweit der Markt die Ware hergibt. Was der Markt nicht hat, wird nicht verlangt:
/// So bindet fehlende Ware kein Geld, das Nachrangigen dann fehlte.
pub fn defense_claim(
    data: &GameData,
    galaxy: &Galaxy,
    economy: &Economy,
    fleet_owner: &dyn Fn(crate::ids::FleetId) -> Option<PolityId>,
    sieges: &Sieges,
    polity: PolityId,
) -> Money {
    let balance = &data.balance.siege;
    let spread = data.balance.economy.spread;
    let mut goods = 0.0;
    for planet in galaxy.planets_of(polity) {
        let defense = sieges.defense(planet.id);
        let Some(market) = economy.markets.get(planet.system.index()) else {
            continue;
        };
        let besieged = sieges.active.contains_key(&planet.id);
        let missing = shortfall(balance, defense, planet.population, besieged);
        let day = &balance.day_bundle_ids;
        let sets = (missing.depot * MONTH_FILL)
            .min(sets_in_stock(market, day, balance.buy_share) * DAYS_PER_MONTH as f64);
        goods += sets.max(0.0) * bundle_cost(market, day, spread);
        let regular = &balance.regular_bundle_ids;
        let recruits = missing
            .recruits
            .min(sets_in_stock(market, regular, balance.buy_share));
        goods += recruits.max(0.0) * bundle_cost(market, regular, spread);
        for (works, next) in Works::ALL.into_iter().zip(missing.next_level) {
            let Some(next) = next else { continue };
            let bundle = works.bundle(balance);
            if sets_in_stock(market, bundle, balance.buy_share) >= next as f64 {
                goods += next as f64 * bundle_cost(market, bundle, spread);
            }
        }
    }
    upkeep_due(data, galaxy, fleet_owner, sieges, polity) + to_money(goods)
}

/// Kauft `sets` Sätze eines Gütersatzes am Markt aus der Bewilligung der Verteidigung:
/// höchstens, was der Markt je Kauf hergibt und die Bewilligung trägt. Der bezahlbare
/// Wunsch zählt als Nachfrage. Gibt die gekauften Sätze zurück.
fn buy_sets(
    data: &GameData,
    economy: &mut Economy,
    polity: PolityId,
    market: usize,
    bundle: &[(GoodId, f64)],
    sets: f64,
    whole: bool,
) -> f64 {
    let balance = &data.balance.siege;
    let spread = data.balance.economy.spread;
    if sets <= 0.0 || market >= economy.markets.len() {
        return 0.0;
    }
    let allowance = economy.allowance(polity, Claim::Defense);
    let place = &mut economy.markets[market];
    let cost = bundle_cost(place, bundle, spread);
    let affordable = if cost > 0.0 {
        sets.min(to_credits(allowance) / cost)
    } else {
        sets
    };
    for (good, amount) in bundle {
        place.demand_today[good.index()] += affordable * amount;
    }
    let mut bought = affordable.min(sets_in_stock(place, bundle, balance.buy_share));
    if whole && bought < sets {
        bought = 0.0;
    }
    if bought <= 0.0 {
        economy.want(polity, Claim::Defense, to_money(affordable * cost));
        return 0.0;
    }
    let mut purse = allowance;
    for (good, amount) in bundle {
        purchase(place, *good, bought * amount, &mut purse, spread);
    }
    economy.want(polity, Claim::Defense, to_money(affordable * cost));
    economy.spend(polity, Claim::Defense, allowance - purse);
    bought
}

/// Tagesbeginn: Jede Welt, die nicht belagert wird und deren System nicht gesperrt ist,
/// kauft ein Dreißigstel dessen, was ihrem Depot zum Ziel fehlt; was ihr an Regulären und
/// Stufen fehlt, zählt mit einem Dreißigstel als Nachfrage am Markt.
pub fn procure_day(
    data: &GameData,
    galaxy: &Galaxy,
    economy: &mut Economy,
    sieges: &mut Sieges,
    blockaded: &dyn Fn(crate::ids::SystemId) -> bool,
) {
    let balance = &data.balance.siege;
    let share = 1.0 / DAYS_PER_MONTH as f64;
    for planet in &galaxy.planets {
        let Some(owner) = planet.owner else { continue };
        let besieged = sieges.active.contains_key(&planet.id);
        if besieged || blockaded(planet.system) {
            continue;
        }
        let defense = &sieges.defenses[planet.id.index()];
        let missing = shortfall(balance, defense, planet.population, false);
        let market = planet.system.index();
        if missing.depot > 1e-9 {
            let garrison = defense.militia + defense.regulars;
            let sets = buy_sets(
                data,
                economy,
                owner,
                market,
                &balance.day_bundle_ids,
                missing.depot * share,
                false,
            );
            if sets > 0.0 && garrison > 0.0 {
                let defense = &mut sieges.defenses[planet.id.index()];
                defense.depot =
                    (defense.depot + sets / garrison).min(defense.plan.depot_days as f64);
            }
        }
        // Was erst der Monatsbeginn kauft, kündigt sich täglich als Nachfrage an.
        let Some(place) = economy.markets.get_mut(market) else {
            continue;
        };
        for (good, amount) in &balance.regular_bundle_ids {
            place.demand_today[good.index()] += missing.recruits * amount * share;
        }
        for (works, next) in Works::ALL.into_iter().zip(missing.next_level) {
            let Some(next) = next else { continue };
            for (good, amount) in works.bundle(balance) {
                place.demand_today[good.index()] += next as f64 * amount * share;
            }
        }
    }
}

/// Monatsbeginn, nach dem Abschluss des Staatshaushalts, je lebender Gemeinschaft:
/// - Sold und Unterhalt aus der Bewilligung an den Staatsdienst. Bleibt ein Teil aus,
///   gehen auf jeder ihrer Welten [`UNPAID_DECAY`] der Regulären mal dem unbezahlten Teil.
/// - Je Welt, die Hauptwelt zuerst, dann in Nummernfolge: Was über dem Plan liegt, geht
///   (Reguläre kehren in die Bevölkerung zurück, soweit die Welt sie trägt). Dann
///   Aushebung auf den Plan zu, höchstens `recruit_per_month` der Miliz plus einen
///   Verband, jeder aus Gütern und Menschen; und je Anlage eine Stufe, ganz oder gar nicht.
pub fn run_arms_month(
    data: &GameData,
    galaxy: &mut Galaxy,
    polities: &Polities,
    economy: &mut Economy,
    sieges: &mut Sieges,
    fleet_owner: &dyn Fn(crate::ids::FleetId) -> Option<PolityId>,
) {
    let balance = &data.balance.siege;
    for polity in polities.alive() {
        let due = upkeep_due(data, galaxy, fleet_owner, sieges, polity.id);
        let granted = economy.allowance(polity.id, Claim::Defense);
        economy.want(polity.id, Claim::Defense, due);
        let paid = economy.pay_staff(polity.id, Claim::Defense, due.min(granted));
        let unpaid = if due > 0 {
            ((due - paid) as f64 / due as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        economy.book_unfunded(polity.id, due - paid);
        let mut worlds: Vec<PlanetId> = galaxy.planets_of(polity.id).map(|p| p.id).collect();
        worlds.sort_by_key(|planet| (Some(*planet) != polity.capital, *planet));
        for id in worlds {
            let index = id.index();
            let besieged = sieges.active.contains_key(&id);
            let (population, capacity, system) = {
                let planet = galaxy.planet(id);
                (planet.population, planet.capacity, planet.system)
            };
            let defense = &mut sieges.defenses[index];
            // Eine belagerte Welt ist abgeschnitten: Dort geht und kommt niemand.
            if besieged {
                continue;
            }
            if unpaid > 0.0 {
                defense.regulars *= 1.0 - UNPAID_DECAY * unpaid;
            }
            // Was über dem Plan liegt, geht.
            let surplus = defense.regulars - defense.plan.regulars as f64;
            if surplus > 1e-6 {
                defense.regulars -= surplus;
                let home = (population + surplus * balance.pop_per_strength).min(capacity);
                galaxy.planet_mut(id).population = home.max(population);
            }
            for works in Works::ALL {
                let planned = works.planned(&defense.plan);
                if works.level(defense) > planned as f64 {
                    works.set_level(defense, planned);
                }
            }
            let missing = shortfall(balance, defense, population, false);
            if missing.recruits > 1e-9 {
                let raised = buy_sets(
                    data,
                    economy,
                    polity.id,
                    system.index(),
                    &balance.regular_bundle_ids,
                    missing.recruits,
                    false,
                );
                if raised > 0.0 {
                    let defense = &mut sieges.defenses[index];
                    // Das Depot zählt in Tagen für die ganze Garnison: Wächst sie,
                    // reichen dieselben Vorräte kürzer.
                    let garrison = defense.militia + defense.regulars;
                    if garrison > 0.0 {
                        defense.depot *= garrison / (garrison + raised);
                    }
                    defense.regulars += raised;
                    let planet = galaxy.planet_mut(id);
                    planet.population =
                        (planet.population - raised * balance.pop_per_strength).max(0.0);
                }
            }
            for (works, next) in Works::ALL.into_iter().zip(missing.next_level) {
                let Some(next) = next else { continue };
                // Stufe n kostet das n-Fache des Satzes, ganz oder gar nicht.
                let built = buy_sets(
                    data,
                    economy,
                    polity.id,
                    system.index(),
                    works.bundle(balance),
                    next as f64,
                    true,
                );
                if built > 0.0 {
                    works.set_level(&mut sieges.defenses[index], next);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::economy::settle_budget;
    use crate::economy::{Wants, to_money};
    use crate::siege::orbit::testing::{FOE, ME, Scene};

    /// Eine neue Partie; die Heimatwelt der Gemeinschaft 0, deren Markt alles führt.
    fn home() -> (Scene, PlanetId) {
        let mut scene = Scene::new();
        scene.state.diplomacy.wars.clear();
        scene.state.sieges.assign.clear();
        let planet = scene.state.polities.get(ME).capital.unwrap();
        let system = scene.state.galaxy.planet(planet).system;
        let market = &mut scene.state.economy.markets[system.index()];
        for stock in &mut market.stock {
            *stock = stock.max(500.0);
        }
        market.cash = to_money(100_000.0);
        (scene, planet)
    }

    /// Schließt den Haushaltsmonat der Gemeinschaft 0 ab: mit der Verteidigung als
    /// einzigem Anspruch neben den Grundleistungen, bei der genannten Staatskasse.
    fn settle(scene: &mut Scene, treasury: Money) -> Money {
        let state = &mut scene.state;
        state.economy.treasuries[ME.index()] = treasury;
        state.economy.state_income[ME.index()] = 0;
        let day = scene.day;
        state
            .economy
            .open_budget_month(day, scene.data.balance.economy.income_tax);
        let owner = |_| None;
        let claim = defense_claim(
            &scene.data,
            &state.galaxy,
            &state.economy,
            &owner,
            &state.sieges,
            ME,
        );
        let wants = Wants::from([(Claim::Defense, claim)]);
        settle_budget(&state.galaxy, &mut state.economy, &scene.data, ME, &wants);
        claim
    }

    fn month(scene: &mut Scene) {
        let state = &mut scene.state;
        let owner = |_| None;
        run_arms_month(
            &scene.data,
            &mut state.galaxy,
            &state.polities,
            &mut state.economy,
            &mut state.sieges,
            &owner,
        );
    }

    fn days(scene: &mut Scene, count: u32) {
        let state = &mut scene.state;
        for _ in 0..count {
            procure_day(
                &scene.data,
                &state.galaxy,
                &mut state.economy,
                &mut state.sieges,
                &|_| false,
            );
        }
    }

    #[test]
    fn defence_ranks_right_after_the_fleet_and_may_use_the_reserve() {
        let at = |claim| Claim::ALL.iter().position(|c| *c == claim).unwrap();
        assert_eq!(at(Claim::Defense), at(Claim::Fleet) + 1);
        assert!(Claim::Defense < Claim::Procurement);
        assert!(Claim::Defense.may_use_reserve() && Claim::Defense.is_spending());
        assert_eq!(Claim::Defense.label(), "Verteidigung");
    }

    #[test]
    fn a_paid_defence_grows_towards_its_plan_out_of_its_grant() {
        let (mut scene, planet) = home();
        let balance = scene.data.balance.siege.clone();
        // Der Plan verlangt mehr, als steht: ein Geschütz mehr, zwei Reguläre mehr, ein
        // volleres Depot.
        let before = scene.state.sieges.defense(planet).clone();
        scene.state.sieges.defense_mut(planet).militia = 20.0;
        let plan = DefensePlan {
            guns: before.plan.guns + 1,
            regulars: before.plan.regulars + 2.0,
            depot_days: before.plan.depot_days + 30,
            ..before.plan.clone()
        };
        set_defense_plan(
            &scene.data,
            &scene.state.galaxy,
            &scene.state.research,
            &mut scene.state.sieges,
            ME,
            planet,
            &plan,
        )
        .unwrap();
        let claim = settle(&mut scene, to_money(50_000.0));
        let money = scene.money();
        let upkeep = to_money(planet_upkeep(&balance, &before));
        assert!(
            claim > upkeep,
            "der Bedarf nennt mehr als Sold und Unterhalt"
        );
        assert_eq!(scene.state.economy.allowance(ME, Claim::Defense), claim);
        let (treasury, staff) = (
            scene.state.economy.treasury(ME),
            scene.state.economy.staff[ME.index()],
        );
        let population = scene.state.galaxy.planet(planet).population;

        month(&mut scene);
        let state = &scene.state;
        let defense = state.sieges.defense(planet);
        // Sold und Unterhalt gingen an den Staatsdienst; Aushebung und Ausbau sind
        // bezahlt und stehen.
        assert_eq!(state.economy.staff[ME.index()] - staff, upkeep);
        assert_eq!(defense.guns, before.guns + 1.0);
        assert!((defense.regulars - before.regulars - 2.0).abs() < 1e-9);
        let raised = 2.0 * balance.pop_per_strength;
        assert!((population - state.galaxy.planet(planet).population - raised).abs() < 1e-9);
        let line = state
            .economy
            .budget(ME)
            .unwrap()
            .current
            .line(Claim::Defense);
        assert_eq!(line.paid, treasury - state.economy.treasury(ME));
        assert!(line.paid > upkeep && line.paid <= claim);

        // Das Depot füllt sich Tag für Tag aus dem Rest der Bewilligung.
        let depot = defense.depot;
        days(&mut scene, 30);
        let state = &scene.state;
        assert!(state.sieges.defense(planet).depot > depot + 5.0);
        let line = state
            .economy
            .budget(ME)
            .unwrap()
            .current
            .line(Claim::Defense);
        assert_eq!(
            line.paid,
            treasury - state.economy.treasury(ME),
            "jeder Kauf der Verteidigung steht in ihrer Zeile"
        );
        assert!(line.paid <= claim, "nie mehr als bewilligt");
        assert_eq!(scene.money(), money);
    }

    #[test]
    fn a_cut_defence_buys_nothing_and_its_regulars_drift_away() {
        let (mut scene, planet) = home();
        let before = scene.state.sieges.defense(planet).clone();
        scene.state.sieges.defense_mut(planet).militia = 20.0;
        scene.state.sieges.defense_mut(planet).plan.depot_days += 30;
        // Die Kasse ist leer: Der Anspruch wird ganz gekürzt.
        let claim = settle(&mut scene, 0);
        let money = scene.money();
        assert!(claim > 0);
        assert_eq!(scene.state.economy.allowance(ME, Claim::Defense), 0);
        month(&mut scene);
        days(&mut scene, 30);
        let state = &scene.state;
        let defense = state.sieges.defense(planet);
        // Nichts gekauft, nichts gezahlt; ein Zwanzigstel der Regulären ist gegangen.
        assert_eq!(defense.depot, before.depot);
        assert!((defense.regulars - before.regulars * (1.0 - UNPAID_DECAY)).abs() < 1e-9);
        let month_line = &state.economy.budget(ME).unwrap().current;
        assert_eq!(month_line.line(Claim::Defense).paid, 0);
        assert!(month_line.line(Claim::Defense).wanted > 0);
        assert!(month_line.unfunded > 0);
        assert_eq!(scene.money(), money);

        // Zwölf Monate ohne Sold: Fast die Hälfte der Garnison ist fort, die Anlagen
        // stehen noch, aber nichts wächst nach.
        for _ in 0..11 {
            settle(&mut scene, 0);
            month(&mut scene);
        }
        let defense = scene.state.sieges.defense(planet);
        let left = before.regulars * (1.0 - UNPAID_DECAY).powi(12);
        assert!((defense.regulars - left).abs() < 1e-6);
        assert!(defense.regulars < 0.6 * before.regulars);
        assert_eq!(defense.guns, before.guns);
    }

    #[test]
    fn plans_are_bounded_by_technology_militia_and_ownership() {
        let (mut scene, planet) = home();
        let balance = scene.data.balance.siege.clone();
        scene.state.sieges.defense_mut(planet).militia = 20.0;
        let base = scene.state.sieges.defense(planet).plan.clone();
        let order = |scene: &mut Scene, who, plan: DefensePlan| {
            set_defense_plan(
                &scene.data,
                &scene.state.galaxy,
                &scene.state.research,
                &mut scene.state.sieges,
                who,
                planet,
                &plan,
            )
        };
        let wrong = order(&mut scene, FOE, base.clone()).unwrap_err();
        assert!(wrong.contains("gehört nicht"), "{wrong}");
        let high = DefensePlan {
            stations: balance.station_max + 1,
            ..base.clone()
        };
        let wrong = order(&mut scene, ME, high).unwrap_err();
        assert!(wrong.contains("Ausbaustufe nicht verfügbar"), "{wrong}");
        // Ohne die Technologie bleibt der Schild auf der ersten Stufe.
        let shield = DefensePlan {
            shield: balance.shield_max.0 + 1,
            ..base.clone()
        };
        assert!(order(&mut scene, ME, shield).is_err());
        let many = DefensePlan {
            regulars: (balance.regulars_cap * 20.0) as f32 + 1.0,
            ..base.clone()
        };
        assert!(order(&mut scene, ME, many).is_err());
        let depot = DefensePlan {
            depot_days: balance.depot_days_cap + 1,
            ..base.clone()
        };
        assert!(order(&mut scene, ME, depot).is_err());
        let fine = DefensePlan {
            stations: balance.station_max,
            regulars: (balance.regulars_cap * 20.0) as f32,
            depot_days: balance.depot_days_cap,
            ..base
        };
        order(&mut scene, ME, fine.clone()).unwrap();
        assert_eq!(scene.state.sieges.defense(planet).plan, fine);
    }
}

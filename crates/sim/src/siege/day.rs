//! Die Belagerung in der Partie: der Lebenslauf der Datensätze, der Tagesrechner je
//! belagertem Planeten und was er in der Welt bewirkt.
//!
//! [`run_day`] ist der Tagesschritt. [`tidy`] hält in jedem Takt, was nicht bis zum
//! nächsten Tag warten kann: Flotten vergehen mitten am Tag, Frieden schließt ein Befehl.

use super::step::{DayEffects, form, resolve, stations_after_duel};
use super::{
    Blocked, BlockedSet, Conquest, Orbit, Outcome, PlanetFacts, Siege, SiegeReport, Sieges, close,
    stakes_in,
};
use crate::chronicle::{Chronicle, ChronicleKind};
use crate::clock::{DAYS_PER_MONTH, TICKS_PER_DAY};
use crate::data::{GameData, Sector};
use crate::diplomacy::Diplomacy;
use crate::economy::{Claim, Economy, procure};
use crate::finance::shares::seat_of_issuer;
use crate::finance::{Finance, book_value};
use crate::galaxy::Galaxy;
use crate::ids::{FleetId, IssuerId, PlanetId, PolityId, SystemId};
use crate::logistics::Logistics;
use crate::polity::Polities;
use std::collections::BTreeMap;

/// Tiefer drückt kein einzelnes Ereignis die Stimmung an der Börse; so hält es auch
/// `finance::shares::shock_polity`.
pub(super) const SHOCK_FLOOR: f64 = 0.2;
/// Unter diesem Wert gilt der Stillstand der Wirtschaft als abgeklungen.
const DISRUPTION_FLOOR: f64 = 1e-6;

/// Hebt die Kriegsmüdigkeit einer Gemeinschaft um `points` (negativ: senkt sie).
pub(super) fn weary(finance: &mut Finance, polity: PolityId, points: f64) {
    if let Some(state) = finance.polities.get_mut(polity.index()) {
        state.war_weariness = (state.war_weariness + points).clamp(0.0, 100.0);
    }
}

/// Betriebe, die einer Welt zugerechnet werden: ihre Firmen, und das Kontor des Systems,
/// wenn es bei ihr sitzt. Handelshäuser gehören dem ganzen Reich.
pub(super) fn local_issuers(
    finance: &Finance,
    galaxy: &Galaxy,
    logistics: &Logistics,
    polities: &Polities,
    planet: PlanetId,
) -> Vec<IssuerId> {
    let capitals: Vec<Option<PlanetId>> = polities.list.iter().map(|p| p.capital).collect();
    finance
        .issuers
        .keys()
        .filter(|id| match id {
            IssuerId::Firm { planet: seat, .. } => *seat == planet,
            IssuerId::Kontor(_) => {
                seat_of_issuer(galaxy, logistics, &capitals, **id) == Some(planet)
            }
            IssuerId::House(_) => false,
        })
        .copied()
        .collect()
}

/// Buchwert der Firmen einer Welt in Credits: der Teil der Beute, der am Kapital hängt.
pub fn firm_value(
    data: &GameData,
    galaxy: &Galaxy,
    economy: &Economy,
    logistics: &Logistics,
    planet: PlanetId,
) -> f64 {
    Sector::ALL
        .into_iter()
        .map(|sector| {
            book_value(
                data,
                galaxy,
                economy,
                logistics,
                IssuerId::Firm { planet, sector },
            )
        })
        .sum()
}

/// Wert der Beute heute: die Firmen der Welt und ihre Bevölkerung.
pub fn prize(
    data: &GameData,
    galaxy: &Galaxy,
    economy: &Economy,
    logistics: &Logistics,
    planet: PlanetId,
) -> f64 {
    firm_value(data, galaxy, economy, logistics, planet)
        + data.balance.siege.prize_per_million * galaxy.planet(planet).population
}

/// Der Planet heute, wie der Tagesrechner ihn braucht.
pub fn planet_facts(
    data: &GameData,
    galaxy: &Galaxy,
    economy: &Economy,
    finance: &Finance,
    planet: PlanetId,
    day: u64,
    forage: f64,
) -> PlanetFacts {
    PlanetFacts {
        day,
        population: galaxy.planet(planet).population,
        cohesion: finance.region(planet).cohesion,
        essentials_met: economy.planet(planet).essentials_met(&data.balance.economy),
        forage,
    }
}

/// Nimmt `strength` Verbände von Bord der genannten Flotten, in Nummernfolge.
pub(super) fn take_troops(sieges: &mut Sieges, fleets: &[FleetId], strength: f64) {
    let mut open = strength;
    for fleet in fleets {
        if open <= 0.0 {
            break;
        }
        if let Some(force) = sieges.embarked.get_mut(fleet) {
            let taken = force.troops.min(open);
            force.troops -= taken;
            open -= taken;
        }
    }
    sieges
        .embarked
        .retain(|_, force| force.troops > 0.0 || force.supply > 0.0);
}

/// Schifft die Truppe am Boden auf die genannten Flotten ein, soweit dort Platz ist, in
/// Nummernfolge; ihr Nachschub fährt anteilig mit. Was keinen Platz findet, bleibt im
/// Datensatz zurück.
pub(super) fn reembark(
    data: &GameData,
    sieges: &mut Sieges,
    orbit: &Orbit,
    siege: &mut Siege,
    fleets: &[FleetId],
) {
    for fleet in fleets {
        if siege.army <= 0.0 {
            break;
        }
        let aboard = sieges.embarked.get(fleet).map_or(0.0, |force| force.troops);
        let moved = (orbit.berths(data, *fleet) - aboard).clamp(0.0, siege.army);
        if moved <= 0.0 {
            continue;
        }
        let supply = siege.army_supply * moved / siege.army;
        let force = sieges.embarked.entry(*fleet).or_default();
        force.troops += moved;
        force.supply += supply;
        siege.army -= moved;
        siege.army_supply -= supply;
    }
}

/// Hebt die Belagerung eines Planeten auf. Hält der Belagerer den Orbit, schifft sich
/// seine Truppe am Boden wieder ein, soweit Platz an Bord ist; der Rest ist verloren.
pub fn lift(
    data: &GameData,
    galaxy: &Galaxy,
    polities: &Polities,
    diplomacy: &Diplomacy,
    finance: &mut Finance,
    sieges: &mut Sieges,
    orbit: &Orbit,
    chronicle: &mut Chronicle,
    planet: PlanetId,
    tick: u64,
) -> Option<Siege> {
    let mut siege = close(
        sieges,
        finance,
        planet,
        Outcome::Lifted,
        tick / TICKS_PER_DAY,
    )?;
    let target = galaxy.planet(planet);
    if siege.army > 0.0 {
        if orbit.control(
            diplomacy,
            &sieges.assign,
            planet,
            target.system,
            siege.attacker,
        ) {
            let fleets = orbit.besiegers(&sieges.assign, planet, target.system, siege.attacker);
            reembark(data, sieges, orbit, &mut siege, &fleets);
        }
        // Wer nicht an Bord kommt, ist verloren.
        sieges.stats.lost_attackers += siege.army;
        weary(
            finance,
            siege.attacker,
            data.balance.siege.wear_loss * siege.army,
        );
    }
    chronicle.push(
        tick,
        ChronicleKind::War,
        Some(siege.attacker),
        Some(target.system),
        format!(
            "Die Belagerung von {} durch {} ist aufgehoben.",
            target.name,
            polities.get(siege.attacker).name
        ),
    );
    Some(siege)
}

/// Jeder Takt: Zuordnungen und eingeschiffte Truppen folgen den Flotten, und eine
/// Belagerung endet, sobald ihr Grund entfällt: Der Planet hat den Besitzer gewechselt,
/// es herrscht Frieden, oder den Belagerer gibt es nicht mehr.
pub fn tidy(
    data: &GameData,
    galaxy: &Galaxy,
    polities: &Polities,
    diplomacy: &Diplomacy,
    finance: &mut Finance,
    sieges: &mut Sieges,
    orbit: &Orbit,
    chronicle: &mut Chronicle,
    tick: u64,
) {
    for (owner, lost) in orbit.settle(data, sieges) {
        weary(finance, owner, data.balance.siege.wear_loss * lost);
    }
    let ended: Vec<PlanetId> = sieges
        .active
        .iter()
        .filter(|(planet, siege)| {
            galaxy.planet(**planet).owner != Some(siege.defender)
                || !diplomacy.at_war(siege.attacker, siege.defender)
                || !polities
                    .list
                    .get(siege.attacker.index())
                    .is_some_and(|polity| polity.alive)
        })
        .map(|(planet, _)| *planet)
        .collect();
    for planet in ended {
        lift(
            data, galaxy, polities, diplomacy, finance, sieges, orbit, chronicle, planet, tick,
        );
    }
}

/// Legt den Datensatz einer Belagerung an: Anteile an den Betrieben der Welt, Miliz,
/// Zonen, Reguläre im Untergrund, Garnison und Beute bei Beginn.
fn open(
    data: &GameData,
    galaxy: &Galaxy,
    polities: &Polities,
    economy: &Economy,
    finance: &mut Finance,
    logistics: &Logistics,
    sieges: &mut Sieges,
    chronicle: &mut Chronicle,
    planet: PlanetId,
    attacker: PolityId,
    defender: PolityId,
    tick: u64,
) {
    let day = tick / TICKS_PER_DAY;
    let stakes = stakes_in(galaxy, polities, finance, logistics, planet, defender);
    let facts = planet_facts(data, galaxy, economy, finance, planet, day, 0.0);
    let prize_start = prize(data, galaxy, economy, logistics, planet);
    let siege = form(
        attacker,
        defender,
        sieges.defense_mut(planet),
        &facts,
        stakes,
        prize_start,
        &data.balance.siege,
    );
    sieges.active.insert(planet, siege);
    sieges.stats.begun += 1;
    finance.regions[planet.index()].besieged = true;
    let target = galaxy.planet(planet);
    chronicle.push(
        tick,
        ChronicleKind::War,
        Some(attacker),
        Some(target.system),
        format!(
            "{} beginnt die Belagerung von {} ({}).",
            polities.get(attacker).name,
            target.name,
            polities.get(defender).name
        ),
    );
}

/// Lebenslauf am Tagesbeginn: die Stelle, an der Datensätze entstehen, und an der endet,
/// was der Belagerer nicht mehr hält.
fn lifecycle(
    data: &GameData,
    galaxy: &Galaxy,
    polities: &Polities,
    diplomacy: &Diplomacy,
    economy: &Economy,
    finance: &mut Finance,
    logistics: &Logistics,
    sieges: &mut Sieges,
    orbit: &mut Orbit,
    chronicle: &mut Chronicle,
    tick: u64,
) {
    tidy(
        data, galaxy, polities, diplomacy, finance, sieges, orbit, chronicle, tick,
    );
    // Eine Flotte belagert nur, solange sie im System des Planeten liegt und mit dessen
    // Besitzer im Krieg steht; belagert ihn schon ein anderer, tritt sie zurück.
    let assigned: Vec<(FleetId, PlanetId)> = sieges.assign.iter().map(|(f, p)| (*f, *p)).collect();
    for (fleet, planet) in &assigned {
        let target = galaxy.planet(*planet);
        let stays = orbit.standing(*fleet).is_some_and(|fleet| {
            fleet.docked
                && fleet.system == target.system
                && target
                    .owner
                    .is_some_and(|owner| diplomacy.at_war(fleet.owner, owner))
                && sieges
                    .siege(*planet)
                    .is_none_or(|siege| siege.attacker == fleet.owner)
        });
        if !stays {
            sieges.assign.remove(fleet);
        }
    }
    // Ohne Orbitalhoheit und ohne Truppe am Boden ist die Belagerung aufgehoben.
    let lost: Vec<PlanetId> = sieges
        .active
        .iter()
        .filter(|(planet, siege)| {
            siege.army <= 0.0
                && !orbit.control(
                    diplomacy,
                    &sieges.assign,
                    **planet,
                    galaxy.planet(**planet).system,
                    siege.attacker,
                )
        })
        .map(|(planet, _)| *planet)
        .collect();
    for planet in lost {
        lift(
            data, galaxy, polities, diplomacy, finance, sieges, orbit, chronicle, planet, tick,
        );
    }
    // Neue Belagerungen in der Nummernfolge der Flotten.
    let assigned: Vec<(FleetId, PlanetId)> = sieges.assign.iter().map(|(f, p)| (*f, *p)).collect();
    for (fleet, planet) in assigned {
        if sieges.active.contains_key(&planet) {
            continue;
        }
        let target = galaxy.planet(planet);
        let (Some(fleet), Some(defender)) = (orbit.standing(fleet), target.owner) else {
            continue;
        };
        if orbit.control(
            diplomacy,
            &sieges.assign,
            planet,
            target.system,
            fleet.owner,
        ) {
            open(
                data,
                galaxy,
                polities,
                economy,
                finance,
                logistics,
                sieges,
                chronicle,
                planet,
                fleet.owner,
                defender,
                tick,
            );
        }
    }
    // Stationen halten ihr System offen, auch gegen die Blockade der Flottenebene.
    orbit.veto_blockades(galaxy, &sieges.defenses);
    for (index, region) in finance.regions.iter_mut().enumerate() {
        region.besieged = sieges.active.contains_key(&PlanetId::from_index(index));
    }
}

/// Die eingeschlossene Garnison deckt einen Teil ihres Bedarfs am Markt des Systems: Der
/// Besitzer kauft den Anteil `forage_share` dessen, was ihre Tagesration an
/// Lebensnotwendigem enthält. Gibt zurück, welchen Teil davon er bekam (0..1).
fn forage(
    data: &GameData,
    economy: &mut Economy,
    system: SystemId,
    owner: PolityId,
    garrison: f64,
) -> f64 {
    let balance = &data.balance.siege;
    let spread = data.balance.economy.spread;
    let (Some(market), Some(treasury)) = (
        economy.markets.get_mut(system.index()),
        economy.treasuries.get_mut(owner.index()),
    ) else {
        return 0.0;
    };
    let before = *treasury;
    let (mut wanted, mut got) = (0.0, 0.0);
    for (good, per_unit) in &balance.day_bundle_ids {
        let essential = data
            .balance
            .economy
            .needs
            .iter()
            .any(|need| need.essential && need.good_id == *good);
        if !essential {
            continue;
        }
        let amount = balance.forage_share * garrison * per_unit;
        let value = data.good(*good).base_price;
        wanted += amount * value;
        got += procure(market, *good, amount, treasury, spread).0 * value;
    }
    // Auch die Not der Eingeschlossenen ist eine Ausgabe der Verteidigung.
    let spent = before - *treasury;
    economy.book_paid(owner, Claim::Defense, spent);
    if wanted > 0.0 {
        (got / wanted).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Rechnet den Tag einer Belagerung und wendet an, was er außerhalb des Datensatzes
/// bewirkt. Gibt den Zielwert für den Stillstand der Wirtschaft zurück.
fn resolve_siege(
    data: &GameData,
    galaxy: &mut Galaxy,
    polities: &Polities,
    diplomacy: &Diplomacy,
    economy: &mut Economy,
    finance: &mut Finance,
    logistics: &Logistics,
    sieges: &mut Sieges,
    orbit: &mut Orbit,
    planet: PlanetId,
    day: u64,
) -> f64 {
    let balance = &data.balance.siege;
    let system = galaxy.planet(planet).system;
    // Ein fälliger Besitzwechsel lässt die Belagerung ruhen: Sie ist entschieden, bis zur
    // Übergabe geschieht nichts mehr.
    if sieges.pending.iter().any(|due| due.planet == planet) {
        let mut blocked = BlockedSet::default();
        blocked.insert(Blocked::Handover);
        if let Some(siege) = sieges.active.get_mut(&planet) {
            siege.report = SiegeReport {
                day,
                blocked,
                ..Default::default()
            };
        }
        return 0.0;
    }
    let Some((attacker, defender)) = sieges
        .siege(planet)
        .map(|siege| (siege.attacker, siege.defender))
    else {
        return 0.0;
    };
    let squadron = orbit.squadron(data, diplomacy, sieges, planet, system, attacker, defender);

    let Sieges {
        active,
        defenses,
        embarked,
        ..
    } = sieges;
    let (Some(siege), Some(defense)) = (active.get_mut(&planet), defenses.get_mut(planet.index()))
    else {
        return 0.0;
    };
    // Mit Orbitalhoheit geht der Nachschub an Bord zur Truppe am Boden.
    if squadron.view.control && siege.army > 0.0 {
        for fleet in &squadron.fleets {
            if let Some(force) = embarked.get_mut(fleet) {
                siege.army_supply += std::mem::take(&mut force.supply);
            }
        }
        embarked.retain(|_, force| force.troops > 0.0 || force.supply > 0.0);
    }
    // Eingeschlossen ist der Planet heute, wenn nach dem Duell des Tages keine Station
    // mehr steht. Solange dann Vorräte da sind und die Oberfläche hält, streckt der Markt
    // vor Ort sie.
    let cut_off = squadron.view.control
        && stations_after_duel(defense.stations as f64, squadron.view.firepower, balance) <= 0.0;
    let foraged = if cut_off && defense.depot > 0.0 && !siege.surface_lost() {
        forage(data, economy, system, defender, siege.defenders(defense))
    } else {
        0.0
    };
    let facts = planet_facts(data, galaxy, economy, finance, planet, day, foraged);
    let firms_before = firm_value(data, galaxy, economy, logistics, planet);
    let effects: DayEffects = resolve(siege, defense, &facts, &squadron.view, balance);
    let stake = siege.stake();
    let age = day.saturating_sub(siege.started_day);
    let sack = siege.orders.sack;
    // Dritte, deren Besitz der Tag vernichtet hat, merken es sich.
    let destroyed = firms_before * (1.0 - effects.capital());
    let wronged: Vec<(PolityId, f64)> = if destroyed > 0.0 {
        siege
            .stakes
            .iter()
            .filter(|(polity, stake)| **polity != attacker && **polity != defender && **stake > 0.0)
            .map(|(polity, stake)| (*polity, destroyed * stake))
            .collect()
    } else {
        Vec::new()
    };

    // Wer als Besatzung an Land ging, ist nicht mehr an Bord; danach die Schiffe: Rumpf,
    // Munition, Treibstoff, und was mit verlorenen Plätzen an Truppen untergeht.
    if effects.landed > 0.0 {
        take_troops(sieges, &squadron.fleets, effects.landed);
    }
    let toll = orbit.apply(data, sieges, &squadron.fleets, attacker, &effects);
    // Die Welt: Menschen, Kapital, Stimmung an der Börse.
    if effects.population < 1.0 {
        galaxy.planet_mut(planet).population *= effects.population;
    }
    let capital = effects.capital();
    if capital < 1.0 {
        for stock in &mut economy.planets[planet.index()].capital {
            *stock *= capital;
        }
    }
    if effects.shock < 1.0 {
        for id in local_issuers(finance, galaxy, logistics, polities, planet) {
            if let Some(issuer) = finance.issuers.get_mut(&id) {
                issuer.shock = (issuer.shock * effects.shock).max(SHOCK_FLOOR);
            }
        }
    }
    // Kriegsmüdigkeit: eigene Verluste, Opfer des Beschusses, und jeder volle Monat.
    let lost_attackers = effects.attacker_lost + toll.troops_lost;
    let (fire_attacker, fire_defender) = balance.wear_fire;
    let mut wear_attacker = balance.wear_loss * lost_attackers
        + fire_attacker * (1.0 + balance.wear_portfolio * stake) * effects.fire_victims;
    let mut wear_defender =
        balance.wear_loss * effects.defender_lost + fire_defender * effects.fire_victims;
    if age > 0 && age.is_multiple_of(DAYS_PER_MONTH) {
        wear_attacker += balance.wear_month.0;
        wear_defender += balance.wear_month.1;
    }
    weary(finance, attacker, wear_attacker);
    weary(finance, defender, wear_defender);
    for (polity, credits) in wronged {
        *sieges.grievances.entry((attacker, polity)).or_default() += credits;
    }
    sieges.stats.lost_attackers += lost_attackers;
    sieges.stats.lost_defenders += effects.defender_lost;
    if let Some(outcome) = effects.verdict {
        sieges.pending.push(Conquest {
            planet,
            to: attacker,
            outcome,
            sack,
        });
    }
    effects.disruption
}

/// Tagesschritt der Belagerung: Lebenslauf der Datensätze, dann je Belagerung in der
/// Reihenfolge der Planeten der Tagesrechner mit seiner Wirkung auf Schiffe, Wirtschaft
/// und Finanzen, zuletzt der Stillstand der Wirtschaft auf jeder Welt. Urteile stehen
/// danach in `Sieges::pending`.
pub fn run_day(
    data: &GameData,
    galaxy: &mut Galaxy,
    polities: &Polities,
    diplomacy: &Diplomacy,
    economy: &mut Economy,
    finance: &mut Finance,
    logistics: &Logistics,
    sieges: &mut Sieges,
    orbit: &mut Orbit,
    chronicle: &mut Chronicle,
    tick: u64,
) {
    let day = tick / TICKS_PER_DAY;
    lifecycle(
        data, galaxy, polities, diplomacy, economy, finance, logistics, sieges, orbit, chronicle,
        tick,
    );
    let mut disrupted: BTreeMap<PlanetId, f64> = BTreeMap::new();
    let besieged: Vec<PlanetId> = sieges.active.keys().copied().collect();
    for planet in besieged {
        let target = resolve_siege(
            data, galaxy, polities, diplomacy, economy, finance, logistics, sieges, orbit, planet,
            day,
        );
        if target > 0.0 {
            disrupted.insert(planet, target);
        }
    }
    // Stillstand: sofort auf den Stand des Tages, danach klingt er von selbst ab.
    let decay = 1.0 - data.balance.siege.disrupt_decay;
    for (index, local) in economy.planets.iter_mut().enumerate() {
        let target = disrupted
            .get(&PlanetId::from_index(index))
            .copied()
            .unwrap_or(0.0);
        if target <= 0.0 && local.disruption <= 0.0 {
            continue;
        }
        let level = target.max(local.disruption * decay).min(1.0);
        local.disruption = if level < DISRUPTION_FLOOR { 0.0 } else { level };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::siege::orbit::testing::{FOE, ME, Scene};
    use crate::siege::{Embarked, Phase, occupation_force};

    #[test]
    fn a_record_forms_under_control_and_the_garrison_lives_off_stores_and_market() {
        let mut scene = Scene::new();
        let balance = scene.data.balance.siege.clone();
        let money = scene.money();
        let treasury = scene.state.economy.treasury(FOE);
        let population = scene.state.galaxy.planet(scene.planet).population;
        let depot = scene.state.sieges.defense(scene.planet).depot;
        assert_eq!(depot, balance.home_start.depot_days as f64);
        scene.day();

        let siege = scene.siege();
        let defense = scene.state.sieges.defense(scene.planet);
        assert_eq!((siege.attacker, siege.defender), (ME, FOE));
        assert_eq!(siege.started_day, 1);
        assert_eq!(scene.state.sieges.stats.begun, 1);
        assert!(scene.state.finance.region(scene.planet).besieged);
        // Miliz aus Bevölkerung und Zusammenhalt, dazu die Regulären der Heimatwelt.
        let cohesion = scene.state.finance.region(scene.planet).cohesion;
        let militia = crate::siege::militia_strength(population, cohesion, &balance);
        assert!(militia > 10.0);
        assert!((siege.garrison_start - militia - balance.home_start.regulars).abs() < 1e-6);
        assert!((defense.militia - militia).abs() < 1e-6);
        // Die Heimatwelt hat Geschütze: also eine Batterie unter den Zonen.
        assert!(
            siege
                .zones
                .iter()
                .any(|zone| zone.kind == crate::siege::ZoneKind::Battery)
        );
        // Niemand sonst ist beteiligt: Die Betriebe gehören dem Verteidiger.
        assert_eq!(siege.stakes.get(&FOE), Some(&1.0));
        assert_eq!(siege.stake(), 0.0);
        // Die Beute: Buchwert der Firmen und die Bevölkerung.
        let state = &scene.state;
        let firms = firm_value(
            &scene.data,
            &state.galaxy,
            &state.economy,
            &state.logistics,
            scene.planet,
        );
        assert!(firms > 0.0);
        assert!(
            (siege.prize_start - firms - balance.prize_per_million * population).abs()
                < 1e-6 * siege.prize_start
        );
        assert_eq!(siege.phase(defense), Phase::Investment);
        // Eingeschlossen: Die Garnison zehrt vom Depot, der Markt vor Ort streckt es. Der
        // Besitzer bezahlt den Einkauf beim Kontor; kein Geld entsteht oder vergeht.
        let used = depot - defense.depot;
        assert!(
            used > 0.0 && used < 1.0,
            "ein Tag kostet mit Markt weniger als einen Vorratstag: {used}"
        );
        assert!((siege.report.depot_used - used).abs() < 1e-9);
        assert!(scene.state.economy.treasury(FOE) < treasury);
        assert_eq!(scene.money(), money);
        // Kein Feuer, also weder Rumpfschaden noch Munition. Den Treibstoff der Tage im
        // Orbit zieht die Flottenebene ab, nicht die Belagerung.
        let ships = scene.ships(0);
        assert!(
            ships
                .iter()
                .all(|ship| ship.hull == 1.0 && ship.ammo == 1.0 && ship.fuel == 1.0)
        );
        assert_eq!(
            scene.state.sieges.find_anomaly(state.galaxy.planets.len()),
            None
        );
        assert!(scene.state.sieges.pending.is_empty());
    }

    #[test]
    fn relief_lifts_the_siege_and_the_besieger_starts_over_when_it_leaves() {
        let mut scene = Scene::new();
        scene.day();
        scene.day();
        let militia = scene.state.sieges.defense(scene.planet).militia;
        let depot = scene.state.sieges.defense(scene.planet).depot;
        // Die Flotte des Verteidigers kehrt zurück: keine Orbitalhoheit mehr.
        let away = scene.fleet_system(1);
        scene.place(1, scene.system);
        scene.day();
        assert!(scene.state.sieges.siege(scene.planet).is_none());
        assert!(!scene.state.finance.region(scene.planet).besieged);
        let stats = &scene.state.sieges.stats;
        assert_eq!((stats.begun, stats.lifted, stats.days), (1, 1, 2));
        // Die Miliz steht wieder als Miliz der Welt, die Vorräte bleiben, wie sie sind.
        assert_eq!(scene.state.sieges.defense(scene.planet).militia, militia);
        assert_eq!(scene.state.sieges.defense(scene.planet).depot, depot);
        // Die Zuordnung bleibt: Die Flotte liegt noch im System und will belagern.
        assert_eq!(
            scene.state.sieges.assign.get(&FleetId(0)),
            Some(&scene.planet)
        );
        let lifted = scene
            .state
            .chronicle
            .entries
            .iter()
            .filter(|entry| entry.text.contains("aufgehoben"))
            .count();
        assert_eq!(lifted, 1);
        // Zieht der Entsatz ab, beginnt die Belagerung von vorn.
        scene.place(1, away);
        scene.day();
        assert_eq!(scene.siege().started_day, 4);
        assert_eq!(scene.state.sieges.stats.begun, 2);

        // Verlässt der Belagerer das System, fällt seine Zuordnung, und die Belagerung endet.
        scene.place(0, away);
        scene.day();
        assert!(scene.state.sieges.assign.is_empty());
        assert!(scene.state.sieges.active.is_empty());
        assert_eq!(scene.state.sieges.stats.lifted, 2);
    }

    #[test]
    fn a_station_holds_the_orbit_open_until_it_is_shot_down() {
        let mut scene = Scene::new();
        let balance = scene.data.balance.siege.clone();
        scene.state.sieges.defense_mut(scene.planet).stations = 1.0;
        scene.order_blockade(0);
        let depot = scene.state.sieges.defense(scene.planet).depot;
        let mut days = 0;
        while scene.state.sieges.defense(scene.planet).stations > 0.0 {
            // Die Flottenebene sperrt das System jeden Tag neu; die Station erhebt Einspruch.
            scene.blockades_of_the_fleet_layer();
            assert!(scene.blockaded());
            scene.day();
            days += 1;
            assert!(days < 200, "die Station fällt nie");
            let defense = scene.state.sieges.defense(scene.planet);
            if defense.stations > 0.0 {
                assert!(!scene.blockaded());
                assert_eq!(scene.siege().phase(defense), Phase::Approach);
                // Nicht eingeschlossen: Die Vorräte bleiben voll.
                assert_eq!(defense.depot, depot);
            }
        }
        // Eine Stufe hält etwa station_hp durch die Feuerkraft der Flotte in Tagen aus.
        assert!((10..=40).contains(&days), "{days} Tage");
        // Am Tag, an dem sie fällt, ist der Planet eingeschlossen: Die Garnison zehrt
        // erstmals vom Depot, und der Markt vor Ort streckt es schon.
        let used = depot - scene.state.sieges.defense(scene.planet).depot;
        assert!(
            used > 0.0 && used < 1.0,
            "erster Tag der Einschließung: {used}"
        );
        // Das Duell kostet Rumpf, die schweren Schiffe am meisten, und Munition.
        let ships = scene.ships(0);
        assert_eq!(ships.len(), 6);
        let (cruiser, corvette) = (&ships[0], &ships[5]);
        assert!(cruiser.hull < 0.8 && corvette.hull > cruiser.hull);
        assert!((1.0 - cruiser.hull) >= 4.9 * (1.0 - corvette.hull));
        assert!(cruiser.ammo < 1.0 - 0.5 * days as f32 * balance.ammo[0] as f32);
        assert_eq!(scene.fleet_anomaly(), None);
        // Danach ist der Planet eingeschlossen, und die Vorräte beginnen zu schwinden.
        scene.day();
        let defense = scene.state.sieges.defense(scene.planet);
        assert_eq!(scene.siege().phase(defense), Phase::Investment);
        assert!(defense.depot < depot);
        scene.blockades_of_the_fleet_layer();
        scene.day();
        assert!(scene.blockaded());
    }

    #[test]
    fn fire_costs_people_capital_prices_and_goodwill() {
        let mut scene = Scene::colony();
        let money = scene.money();
        let balance = scene.data.balance.siege.clone();
        let planet = scene.planet;
        let third = PolityId(2);
        // Eine dritte Gemeinschaft hält Anteile an den Betrieben der Kolonie.
        let firms = local_issuers(
            &scene.state.finance,
            &scene.state.galaxy,
            &scene.state.logistics,
            &scene.state.polities,
            planet,
        );
        assert!(!firms.is_empty());
        for id in &firms {
            let issuer = scene.state.finance.issuers.get_mut(id).unwrap();
            let shares = issuer
                .holdings
                .remove(&crate::ids::HolderId::Elite(planet))
                .unwrap_or(0);
            if shares > 0 {
                issuer
                    .holdings
                    .insert(crate::ids::HolderId::State(third), shares);
            }
        }
        scene.day();
        let stake = scene.siege().stakes.get(&third).copied().unwrap_or(0.0);
        assert!(stake > 0.05 && stake < 0.5, "Anteil der Dritten: {stake}");
        // Ein Tag ohne Feuer als Vergleich: nichts davon bewegt sich.
        let state = &scene.state;
        let population = state.galaxy.planet(planet).population;
        let capital = state.economy.planet(planet).total_capital();
        let shock = state.finance.issuers[&firms[0]].shock;
        let weariness =
            |scene: &Scene, polity: PolityId| scene.state.finance.polity(polity).war_weariness;
        let (weary_me, weary_foe) = (weariness(&scene, ME), weariness(&scene, FOE));
        assert!(capital > 0.0 && scene.state.sieges.grievances.is_empty());
        let militia = scene.state.sieges.defense(planet).militia;

        // Flächenbeschuss für einen Tag.
        scene
            .state
            .sieges
            .active
            .get_mut(&planet)
            .unwrap()
            .orders
            .fire = crate::siege::Fire::Area;
        let firms_before = firm_value(
            &scene.data,
            &scene.state.galaxy,
            &scene.state.economy,
            &scene.state.logistics,
            planet,
        );
        scene.day();
        let report = scene.siege().report.clone();
        let damage = report.fire_damage;
        assert!(damage > 0.0 && damage <= balance.area.1 * balance.bombard_cap + 1e-12);
        let state = &scene.state;
        // Bevölkerung und jeder Kapitalstock verlieren den Begleitschaden, die Kurse der
        // Betriebe der Welt geben im selben Maß nach, die Welt trägt die Narbe.
        let people = state.galaxy.planet(planet).population;
        assert!((people - population * (1.0 - damage)).abs() < 1e-9);
        let stock = state.economy.planet(planet).total_capital();
        assert!((stock - capital * (1.0 - damage)).abs() < 1e-9 * capital);
        for id in &firms {
            let now = state.finance.issuers[id].shock;
            assert!((now - shock * (1.0 - damage)).abs() < 1e-12);
        }
        let defense = state.sieges.defense(planet);
        assert!((defense.scar - damage).abs() < 1e-12);
        assert_eq!(defense.scar_by, Some(ME));
        // Die Miliz ist getroffen, die Magazine leeren sich.
        assert!(report.defender_bombed > 0.0 && defense.militia < militia);
        assert!(scene.ships(0).iter().all(|ship| ship.ammo < 1.0));
        assert!(state.sieges.stats.lost_defenders >= report.defender_bombed - 1e-12);
        // Kriegsmüdigkeit: Opfer des Beschusses bei beiden, beim Verteidiger dazu die
        // Verluste seiner Truppe.
        let victims = population - people;
        let (fire_me, fire_foe) = balance.wear_fire;
        let gained_me = weariness(&scene, ME) - weary_me;
        let gained_foe = weariness(&scene, FOE) - weary_foe;
        assert!((gained_me - fire_me * victims).abs() < 1e-9, "{gained_me}");
        let expected = fire_foe * victims + balance.wear_loss * report.defender_bombed;
        assert!((gained_foe - expected).abs() < 1e-9, "{gained_foe}");
        // Die Dritte merkt sich, was von ihrem Besitz vernichtet wurde.
        let destroyed = firms_before * damage;
        let grievance = state.sieges.grievances[&(ME, third)];
        assert!(destroyed > 0.0);
        assert!((grievance - destroyed * stake).abs() < 1e-9 * destroyed);
        assert_eq!(state.sieges.grievances.len(), 1);
        // Zerstört wird Vermögen, kein Geld.
        assert_eq!(scene.money(), money);
    }

    #[test]
    fn a_verdict_waits_in_the_pending_list_and_the_siege_rests() {
        let mut scene = Scene::new();
        scene.hold_verdicts = true;
        let balance = scene.data.balance.siege.clone();
        let population = scene.state.galaxy.planet(scene.planet).population;
        let needed = occupation_force(population, &balance);
        // Keine Vorräte, keine Regulären, eine Besatzungstruppe an Bord.
        let defense = scene.state.sieges.defense_mut(scene.planet);
        defense.depot = 0.0;
        defense.regulars = 0.0;
        assert!(needed < 17.5, "die Startflotte trägt die Besatzungstruppe");
        scene.state.sieges.embarked.insert(
            FleetId(0),
            Embarked {
                troops: 17.0,
                supply: 100.0,
            },
        );
        let mut days = 0;
        while scene.state.sieges.pending.is_empty() {
            scene.day();
            days += 1;
            assert!(days <= balance.max_siege_days, "kein Urteil");
        }
        // Ohne Vorräte löst sich die Miliz in Wochen auf.
        assert!(days > 30 && days < 200, "{days} Tage");
        let due = scene.state.sieges.pending[0].clone();
        assert_eq!(
            (due.planet, due.to, due.outcome),
            (scene.planet, ME, Outcome::Starved)
        );
        // Was zur Besatzung fehlte, ist ohne Verluste gelandet; der Rest bleibt an Bord.
        let siege = scene.siege();
        assert!((siege.army - needed).abs() < 1e-9);
        let aboard = scene.state.sieges.embarked[&FleetId(0)].troops;
        assert!((aboard - (17.0 - needed)).abs() < 1e-9);
        assert_eq!(scene.state.sieges.stats.lost_attackers, 0.0);
        // Kein Schuss fiel: Bevölkerung und Kapital sind unberührt.
        assert_eq!(
            scene.state.galaxy.planet(scene.planet).population,
            population
        );
        assert_eq!(scene.state.sieges.defense(scene.planet).scar, 0.0);

        // Solange der Besitzwechsel aussteht, ruht die Belagerung: kein zweites Urteil,
        // kein Verbrauch, und der Tagesbericht nennt den Grund.
        let (army, supply) = (siege.army, siege.army_supply);
        let fuel = scene.ships(0)[0].fuel;
        scene.day();
        scene.day();
        assert_eq!(scene.state.sieges.pending, vec![due]);
        let siege = scene.siege();
        assert_eq!((siege.army, siege.army_supply), (army, supply));
        assert!(siege.report.blocked.contains(Blocked::Handover));
        assert_eq!(siege.report.day, scene.day);
        assert_eq!(scene.ships(0)[0].fuel, fuel);
        assert_eq!(scene.state.sieges.stats.conquests(), 0);
    }

    #[test]
    fn peace_or_a_change_of_owner_ends_a_siege_at_once() {
        let mut scene = Scene::new();
        scene.day();
        assert!(scene.state.sieges.siege(scene.planet).is_some());
        // Solange Krieg herrscht und der Verteidiger die Welt hält, bleibt alles stehen.
        scene.tidy_at_midday();
        assert!(scene.state.sieges.siege(scene.planet).is_some());
        // Frieden mitten am Tag: Die Belagerung endet im selben Takt.
        scene.state.diplomacy.wars.clear();
        scene.tidy_at_midday();
        assert!(scene.state.sieges.active.is_empty());
        assert!(!scene.state.finance.region(scene.planet).besieged);
        assert_eq!(scene.state.sieges.stats.lifted, 1);
        // Am nächsten Tag fällt auch die Zuordnung: Mit dem Besitzer herrscht kein Krieg.
        scene.day();
        assert!(scene.state.sieges.assign.is_empty());

        // Wechselt die Welt den Besitzer, endet die Belagerung ebenso.
        let mut scene = Scene::new();
        scene.day();
        scene.state.galaxy.planet_mut(scene.planet).owner = Some(PolityId(2));
        scene.tidy_at_midday();
        assert!(scene.state.sieges.active.is_empty());
        assert_eq!(scene.state.sieges.stats.lifted, 1);
    }
}

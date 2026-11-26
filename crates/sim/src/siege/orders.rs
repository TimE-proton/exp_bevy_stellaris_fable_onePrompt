//! Befehle der Belagerung: was Spieler und KI anordnen, mit Prüfung und Ablehnungsgrund.
//!
//! Aufgerufen aus `command::execute`. Befehle wirken zwischen den Tagesgrenzen; der
//! Tagesschritt rechnet mit dem, was sie hinterlassen.

use super::day::lift;
use super::{Orbit, Posture, SiegeOrders, Sieges, ZoneKind};
use crate::chronicle::Chronicle;
use crate::data::GameData;
use crate::diplomacy::Diplomacy;
use crate::economy::{Economy, purchase, to_credits};
use crate::finance::Finance;
use crate::galaxy::{Galaxy, Planet};
use crate::ids::{FleetId, PlanetId, PolityId, SystemId};
use crate::polity::Polities;

fn known(galaxy: &Galaxy, planet: PlanetId) -> Result<&Planet, String> {
    galaxy
        .planets
        .get(planet.index())
        .ok_or_else(|| "Planet unbekannt".to_string())
}

/// Eine eigene Flotte belagert einen Planeten in ihrem System. Ersetzt ihr früheres Ziel.
/// Der Datensatz entsteht am nächsten Tagesbeginn, sobald die Flotte die Orbitalhoheit hat.
pub fn begin_siege(
    galaxy: &Galaxy,
    polities: &Polities,
    diplomacy: &Diplomacy,
    sieges: &mut Sieges,
    orbit: &Orbit,
    polity: PolityId,
    fleet: FleetId,
    planet: PlanetId,
) -> Result<(), String> {
    let target = known(galaxy, planet)?;
    let squadron = orbit.own(polity, fleet)?;
    if !squadron.docked || squadron.system != target.system {
        return Err(format!(
            "{} liegt nicht im System von {}",
            squadron.name, target.name
        ));
    }
    let Some(owner) = target.owner else {
        return Err(format!("{} ist unbesiedelt", target.name));
    };
    if owner == polity {
        return Err(format!("{} gehört der eigenen Gemeinschaft", target.name));
    }
    if !diplomacy.at_war(polity, owner) {
        return Err(format!(
            "mit {} herrscht kein Krieg",
            polities.get(owner).name
        ));
    }
    if let Some(siege) = sieges.siege(planet)
        && siege.attacker != polity
    {
        return Err(format!(
            "{} wird bereits von {} belagert",
            target.name,
            polities.get(siege.attacker).name
        ));
    }
    sieges.assign.insert(fleet, planet);
    Ok(())
}

/// Stehende Befehle für eine eigene Belagerung. Was davon nicht wirken kann, nennt der
/// Tagesbericht.
pub fn set_siege_orders(
    galaxy: &Galaxy,
    sieges: &mut Sieges,
    polity: PolityId,
    planet: PlanetId,
    orders: SiegeOrders,
) -> Result<(), String> {
    let target = known(galaxy, planet)?;
    let Some(siege) = sieges
        .active
        .get_mut(&planet)
        .filter(|siege| siege.attacker == polity)
    else {
        return Err(format!(
            "{} wird nicht von der eigenen Gemeinschaft belagert",
            target.name
        ));
    };
    if orders.focus as usize >= siege.zones.len() {
        return Err(format!(
            "Schwerpunkt: Zone {} gibt es auf {} nicht",
            orders.focus, target.name
        ));
    }
    siege.orders = orders;
    Ok(())
}

/// Hebt eine eigene Belagerung auf. Truppen am Boden schiffen sich auf die belagernden
/// Flotten ein, soweit dort Platz ist; der Rest ist verloren. Die Flotten sind danach
/// keinem Ziel mehr zugeordnet.
pub fn lift_siege(
    data: &GameData,
    galaxy: &Galaxy,
    polities: &Polities,
    diplomacy: &Diplomacy,
    finance: &mut Finance,
    sieges: &mut Sieges,
    orbit: &Orbit,
    chronicle: &mut Chronicle,
    polity: PolityId,
    planet: PlanetId,
    tick: u64,
) -> Result<(), String> {
    let target = known(galaxy, planet)?;
    let army = sieges
        .siege(planet)
        .filter(|siege| siege.attacker == polity)
        .map(|siege| siege.army);
    let fleets: Vec<FleetId> = sieges
        .assign
        .iter()
        .filter(|(fleet, assigned)| {
            **assigned == planet
                && orbit
                    .standing(**fleet)
                    .is_some_and(|fleet| fleet.owner == polity)
        })
        .map(|(fleet, _)| *fleet)
        .collect();
    if army.is_none() && fleets.is_empty() {
        return Err(format!(
            "{} wird nicht von der eigenen Gemeinschaft belagert",
            target.name
        ));
    }
    if let Some(army) = army {
        if army > 0.0 && !orbit.control(diplomacy, &sieges.assign, planet, target.system, polity) {
            return Err("ohne Orbitalhoheit kein Abzug".into());
        }
        lift(
            data, galaxy, polities, diplomacy, finance, sieges, orbit, chronicle, planet, tick,
        );
    }
    for fleet in fleets {
        sieges.assign.remove(&fleet);
    }
    Ok(())
}

/// Haltung der Garnison einer eigenen Welt und ob ihre Geschütze schweigen. Unter
/// Belagerung lässt sich die Haltung nur ändern, solange das Zentrum gehalten wird; die
/// Regulären wechseln dann sofort zwischen Oberfläche und Untergrund.
pub fn set_garrison_orders(
    data: &GameData,
    galaxy: &Galaxy,
    sieges: &mut Sieges,
    polity: PolityId,
    planet: PlanetId,
    posture: Posture,
    hold_fire: bool,
) -> Result<(), String> {
    let target = known(galaxy, planet)?;
    if target.owner != Some(polity) {
        return Err(format!(
            "{} gehört nicht der eigenen Gemeinschaft",
            target.name
        ));
    }
    let Sieges {
        active, defenses, ..
    } = sieges;
    let defense = &mut defenses[planet.index()];
    if posture != defense.posture {
        if let Some(siege) = active.get_mut(&planet) {
            if siege.zone_taken(ZoneKind::Center) {
                return Err(
                    "die Haltung lässt sich nur ändern, solange das Zentrum gehalten wird".into(),
                );
            }
            let room = data.balance.siege.deep_per_bunker * defense.bunkers as f64;
            let deep = match posture {
                Posture::Deep => defense.regulars.min(room),
                Posture::Forward => 0.0,
            };
            // Der Maßstab für die Oberfläche folgt den Regulären, die sie verlassen
            // oder wieder betreten.
            siege.surface_start = (siege.surface_start + siege.deep - deep).max(0.0);
            siege.deep = deep;
        }
        defense.posture = posture;
    }
    defense.hold_fire = hold_fire;
    Ok(())
}

/// Kauft Nachschub für `wanted` Verbandstage am Markt des Systems: je Verbandstag einen
/// Satz der Tagesration. Gekauft werden nur ganze Sätze, soweit Ware und Staatskasse
/// reichen; der bezahlbare Wunsch zählt als Nachfrage. Gibt die Verbandstage zurück.
fn buy_supply(
    data: &GameData,
    economy: &mut Economy,
    system: SystemId,
    polity: PolityId,
    wanted: f64,
) -> f64 {
    let bundle = &data.balance.siege.day_bundle_ids;
    let spread = data.balance.economy.spread;
    let (Some(market), Some(treasury)) = (
        economy.markets.get_mut(system.index()),
        economy.treasuries.get_mut(polity.index()),
    ) else {
        return 0.0;
    };
    if wanted <= 0.0 {
        return 0.0;
    }
    let cost: f64 = bundle
        .iter()
        .map(|(good, amount)| amount * market.ask(*good, spread))
        .sum();
    let affordable = if cost > 0.0 {
        wanted.min(to_credits((*treasury).max(0)) / cost)
    } else {
        wanted
    };
    let mut sets = affordable;
    for (good, amount) in bundle {
        market.demand_today[good.index()] += affordable * amount;
        sets = sets.min(market.available(*good) / amount);
    }
    let sets = sets.max(0.0);
    if sets > 0.0 {
        for (good, amount) in bundle {
            purchase(market, *good, sets * amount, treasury, spread);
        }
    }
    sets
}

/// Schifft reguläre Verbände einer eigenen Welt auf eine eigene Flotte in deren System
/// ein, mit Nachschub für `supply_days` Tage, soweit Markt und Staatskasse ihn hergeben.
pub fn embark_troops(
    data: &GameData,
    galaxy: &Galaxy,
    diplomacy: &Diplomacy,
    economy: &mut Economy,
    sieges: &mut Sieges,
    orbit: &Orbit,
    polity: PolityId,
    fleet: FleetId,
    planet: PlanetId,
    strength: f64,
    supply_days: u16,
) -> Result<(), String> {
    let balance = &data.balance.siege;
    let target = known(galaxy, planet)?;
    let squadron = orbit.own(polity, fleet)?;
    if target.owner != Some(polity) {
        return Err(format!(
            "{} gehört nicht der eigenen Gemeinschaft",
            target.name
        ));
    }
    if !squadron.docked || squadron.system != target.system {
        return Err(format!(
            "{} liegt nicht im System von {}",
            squadron.name, target.name
        ));
    }
    if squadron.in_battle {
        return Err(format!("{} steht im Gefecht", squadron.name));
    }
    if !(strength.is_finite() && strength > 0.0) {
        return Err("die Truppenstärke muss positiv sein".into());
    }
    if supply_days > balance.supply_days_cap {
        return Err(format!(
            "höchstens {} Tage Nachschub je Einschiffung",
            balance.supply_days_cap
        ));
    }
    let defense = sieges.defense(planet);
    let mut ready = defense.regulars;
    if let Some(siege) = sieges.siege(planet) {
        let control = orbit.control(
            diplomacy,
            &sieges.assign,
            planet,
            target.system,
            siege.attacker,
        );
        if control && defense.stations <= 0.0 {
            return Err(format!("{} ist eingeschlossen", target.name));
        }
        // Wer im Untergrund sitzt, kommt nicht an Bord.
        ready -= siege.deep;
    }
    if strength > ready + 1e-9 {
        return Err(format!(
            "auf {} stehen nur {:.1} reguläre Verbände bereit",
            target.name,
            ready.max(0.0)
        ));
    }
    let aboard = sieges
        .embarked
        .get(&fleet)
        .map_or(0.0, |force| force.troops);
    let free = orbit.berths(data, fleet) - aboard;
    if strength > free + 1e-9 {
        return Err(format!(
            "{} hat nur {:.1} freie Plätze für Truppen",
            squadron.name,
            free.max(0.0)
        ));
    }
    let supply = buy_supply(
        data,
        economy,
        target.system,
        polity,
        strength * supply_days as f64,
    );
    let defense = sieges.defense_mut(planet);
    defense.regulars = (defense.regulars - strength).max(0.0);
    let force = sieges.embarked.entry(fleet).or_default();
    force.troops += strength;
    force.supply += supply;
    Ok(())
}

/// Frieden kann nur der Belagerer anbieten: Solange `target` einen Planeten von `polity`
/// belagert, wird dessen Friedensschluss abgelehnt.
pub fn peace_veto(
    galaxy: &Galaxy,
    polities: &Polities,
    sieges: &Sieges,
    polity: PolityId,
    target: PolityId,
) -> Result<(), String> {
    match sieges
        .active
        .iter()
        .find(|(_, siege)| siege.attacker == target && siege.defender == polity)
    {
        Some((planet, _)) => Err(format!(
            "{} belagert {}: Frieden kann nur der Belagerer anbieten",
            polities.get(target).name,
            galaxy.planet(*planet).name
        )),
        None => Ok(()),
    }
}

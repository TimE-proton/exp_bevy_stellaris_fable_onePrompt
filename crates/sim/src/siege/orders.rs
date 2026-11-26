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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::economy::Money;
    use crate::siege::orbit::testing::{FOE, ME, Scene};
    use crate::siege::{Embarked, Outcome, land};

    fn embark(scene: &mut Scene, fleet: u32, troops: f64, supply: f64) {
        scene
            .state
            .sieges
            .embarked
            .insert(FleetId(fleet), Embarked { troops, supply });
    }

    /// Setzt einen Teil der eingeschifften Truppe an Land, wie es der Befehl dazu täte.
    fn put_ashore(scene: &mut Scene, strength: f64, supply: f64) {
        let balance = scene.data.balance.siege.clone();
        let planet = scene.planet;
        let sieges = &mut scene.state.sieges;
        let defense = sieges.defenses[planet.index()].clone();
        let siege = sieges.active.get_mut(&planet).unwrap();
        let landing = land(siege, &defense, strength, scene.day, &balance);
        assert_eq!(landing.lost, 0.0, "die Kolonie hat keine Geschütze");
        siege.army_supply += supply;
        sieges.embarked.get_mut(&FleetId(0)).unwrap().troops -= strength;
    }

    fn lift(scene: &mut Scene, polity: PolityId) -> Result<(), String> {
        let state = &mut scene.state;
        let orbit = Orbit::new(&mut state.fleets, &state.research);
        lift_siege(
            &scene.data,
            &state.galaxy,
            &state.polities,
            &state.diplomacy,
            &mut state.finance,
            &mut state.sieges,
            &orbit,
            &mut state.chronicle,
            polity,
            scene.planet,
            scene.day * crate::clock::TICKS_PER_DAY + 3,
        )
    }

    #[test]
    fn lifting_brings_the_troops_back_aboard_as_far_as_there_is_room() {
        // Alle zehn Verbände am Boden finden wieder Platz.
        let mut scene = Scene::colony();
        embark(&mut scene, 0, 17.5, 0.0);
        scene.day();
        put_ashore(&mut scene, 10.0, 50.0);
        assert_eq!(scene.siege().army, 10.0);
        // Nur der Belagerer hebt auf.
        let reason = lift(&mut scene, FOE).unwrap_err();
        assert!(reason.contains("wird nicht von der eigenen"), "{reason}");
        lift(&mut scene, ME).unwrap();
        let sieges = &scene.state.sieges;
        assert!(sieges.active.is_empty() && sieges.assign.is_empty());
        assert_eq!(sieges.embarked[&FleetId(0)].troops, 17.5);
        assert_eq!(sieges.embarked[&FleetId(0)].supply, 50.0);
        assert_eq!(sieges.stats.count(Outcome::Lifted), 1);
        assert_eq!(sieges.stats.lost_attackers, 0.0);

        // Ist an Bord nur noch für einen Teil Platz, ist der Rest verloren.
        let mut scene = Scene::colony();
        embark(&mut scene, 0, 17.5, 0.0);
        scene.day();
        put_ashore(&mut scene, 10.0, 50.0);
        scene
            .state
            .sieges
            .embarked
            .get_mut(&FleetId(0))
            .unwrap()
            .troops = 12.0;
        let weariness = scene.state.finance.polity(ME).war_weariness;
        lift(&mut scene, ME).unwrap();
        let sieges = &scene.state.sieges;
        assert_eq!(sieges.embarked[&FleetId(0)].troops, 17.5);
        assert!((sieges.embarked[&FleetId(0)].supply - 27.5).abs() < 1e-9);
        assert!((sieges.stats.lost_attackers - 4.5).abs() < 1e-9);
        let wear = scene.data.balance.siege.wear_loss * 4.5;
        let gained = scene.state.finance.polity(ME).war_weariness - weariness;
        assert!((gained - wear).abs() < 1e-9);

        // Ohne Orbitalhoheit kommt die Truppe nicht vom Boden weg.
        let mut scene = Scene::colony();
        embark(&mut scene, 0, 17.5, 0.0);
        scene.day();
        put_ashore(&mut scene, 10.0, 50.0);
        scene.place(1, scene.system);
        assert_eq!(
            lift(&mut scene, ME).unwrap_err(),
            "ohne Orbitalhoheit kein Abzug"
        );
        assert_eq!(scene.siege().army, 10.0);
        assert_eq!(scene.state.sieges.assign.len(), 1);
    }

    #[test]
    fn the_garrison_changes_posture_only_while_it_holds_the_center() {
        let mut scene = Scene::new();
        let balance = scene.data.balance.siege.clone();
        let planet = scene.planet;
        scene.state.sieges.defense_mut(planet).bunkers = 1;
        scene.day();
        let regulars = scene.state.sieges.defense(planet).regulars;
        let surface = scene.siege().surface_start;
        assert_eq!(scene.siege().deep, 0.0);
        let order = |scene: &mut Scene, polity, posture, hold_fire| {
            set_garrison_orders(
                &scene.data,
                &scene.state.galaxy,
                &mut scene.state.sieges,
                polity,
                planet,
                posture,
                hold_fire,
            )
        };
        // Nur der Besitzer befiehlt.
        assert!(order(&mut scene, ME, Posture::Deep, false).is_err());
        // In die Tiefe: Die Regulären gehen in den Untergrund, soweit die Bunker reichen,
        // und zählen nicht mehr zur Oberfläche.
        order(&mut scene, FOE, Posture::Deep, true).unwrap();
        let deep = regulars.min(balance.deep_per_bunker);
        assert!(deep > 0.0);
        assert_eq!(scene.siege().deep, deep);
        assert!((scene.siege().surface_start - (surface - deep)).abs() < 1e-9);
        let defense = scene.state.sieges.defense(planet);
        assert_eq!((defense.posture, defense.hold_fire), (Posture::Deep, true));
        assert_eq!(
            scene
                .state
                .sieges
                .find_anomaly(scene.state.galaxy.planets.len()),
            None
        );
        // Und zurück.
        order(&mut scene, FOE, Posture::Forward, true).unwrap();
        assert_eq!(scene.siege().deep, 0.0);
        assert!((scene.siege().surface_start - surface).abs() < 1e-9);
        // Ist das Zentrum gefallen, bleibt die Haltung, wie sie ist; die Feuerpause lässt
        // sich weiter befehlen.
        let siege = scene.state.sieges.active.get_mut(&planet).unwrap();
        let center = siege
            .zones
            .iter()
            .position(|zone| zone.kind == ZoneKind::Center)
            .unwrap();
        siege.zones[center].taken = true;
        let reason = order(&mut scene, FOE, Posture::Deep, false).unwrap_err();
        assert!(
            reason.contains("solange das Zentrum gehalten wird"),
            "{reason}"
        );
        assert_eq!(scene.state.sieges.defense(planet).posture, Posture::Forward);
        assert!(scene.state.sieges.defense(planet).hold_fire);
        order(&mut scene, FOE, Posture::Forward, false).unwrap();
        assert!(!scene.state.sieges.defense(planet).hold_fire);
    }

    #[test]
    fn troops_embark_from_worlds_that_are_not_invested_and_supply_comes_from_the_market() {
        let mut scene = Scene::new();
        let balance = scene.data.balance.siege.clone();
        let (planet, system) = (scene.planet, scene.system);
        scene.day();
        assert!(scene.state.sieges.siege(planet).is_some());
        // Die Flotte des Verteidigers liegt wieder über seiner Hauptwelt, sammelt sich
        // aber noch: Sie hält den Orbit nicht, die Welt bleibt eingeschlossen.
        scene.place(1, system);
        scene.regroup(1, 99);
        let embark_now = |scene: &mut Scene, strength: f64, days: u16| {
            let state = &mut scene.state;
            let orbit = Orbit::new(&mut state.fleets, &state.research);
            embark_troops(
                &scene.data,
                &state.galaxy,
                &state.diplomacy,
                &mut state.economy,
                &mut state.sieges,
                &orbit,
                FOE,
                FleetId(1),
                planet,
                strength,
                days,
            )
        };
        let reason = embark_now(&mut scene, 2.0, 0).unwrap_err();
        assert!(reason.contains("ist eingeschlossen"), "{reason}");
        // Hinter einer Station ist sie es nicht. An Bord kommt aber nur, wer nicht im
        // Untergrund sitzt.
        scene.state.sieges.defense_mut(planet).stations = 1.0;
        let regulars = scene.state.sieges.defense(planet).regulars;
        scene.state.sieges.active.get_mut(&planet).unwrap().deep = regulars - 3.0;
        let reason = embark_now(&mut scene, 4.0, 0).unwrap_err();
        assert!(
            reason.contains("nur 3.0 reguläre Verbände bereit"),
            "{reason}"
        );

        // Nachschub: Sätze der Tagesration, soweit jedes Gut am Markt zu haben ist. Die
        // Munition ist knapp und begrenzt den Kauf; bezahlt wird das Kontor.
        let market = &mut scene.state.economy.markets[system.index()];
        for (good, _) in &balance.day_bundle_ids {
            market.stock[good.index()] = 100.0;
        }
        let (scarce, per_set) = balance.day_bundle_ids[1];
        market.stock[scarce.index()] = 0.5;
        let cash = market.cash;
        let treasury = scene.state.economy.treasury(FOE);
        let money = scene.money();
        embark_now(&mut scene, 3.0, 60).unwrap();
        let sets = 0.5 * crate::economy::BUYER_SHARE / per_set;
        assert!(sets < 3.0 * 60.0);
        let aboard = &scene.state.sieges.embarked[&FleetId(1)];
        assert_eq!(aboard.troops, 3.0);
        assert!((aboard.supply - sets).abs() < 1e-9, "{}", aboard.supply);
        assert_eq!(scene.state.sieges.defense(planet).regulars, regulars - 3.0);
        let market = &scene.state.economy.markets[system.index()];
        let paid: Money = treasury - scene.state.economy.treasury(FOE);
        assert!(paid > 0);
        assert_eq!(market.cash - cash, paid);
        assert_eq!(scene.money(), money);
        for (good, amount) in &balance.day_bundle_ids {
            // Der ganze bezahlbare Wunsch steht als Nachfrage im Markt.
            assert!(market.demand_today[good.index()] >= 180.0 * amount - 1e-9);
        }
        assert!((market.stock[scarce.index()] - 0.25).abs() < 1e-9);
    }

    #[test]
    fn peace_is_refused_only_to_the_besieged() {
        let mut scene = Scene::new();
        let veto = |scene: &Scene, polity, target| {
            peace_veto(
                &scene.state.galaxy,
                &scene.state.polities,
                &scene.state.sieges,
                polity,
                target,
            )
        };
        // Ohne Belagerung steht dem Frieden nichts im Weg.
        assert_eq!(veto(&scene, FOE, ME), Ok(()));
        scene.day();
        let reason = veto(&scene, FOE, ME).unwrap_err();
        assert!(
            reason.ends_with("Frieden kann nur der Belagerer anbieten"),
            "{reason}"
        );
        assert!(reason.contains(&scene.state.galaxy.planet(scene.planet).name));
        assert_eq!(veto(&scene, ME, FOE), Ok(()));
        // Dritte betrifft es nicht.
        assert_eq!(veto(&scene, FOE, PolityId(2)), Ok(()));
    }
}

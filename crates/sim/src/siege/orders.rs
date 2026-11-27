//! Befehle der Belagerung: was Spieler und KI anordnen, mit Prüfung und Ablehnungsgrund.
//!
//! Aufgerufen aus `command::execute`. Befehle wirken zwischen den Tagesgrenzen; der
//! Tagesschritt rechnet mit dem, was sie hinterlassen.

use super::day::{lift, take_troops, weary};
use super::step::land;
use super::{Conquest, Orbit, Outcome, Posture, SiegeOrders, Sieges, ZoneKind, occupation_force};
use crate::chronicle::Chronicle;
use crate::clock::TICKS_PER_DAY;
use crate::data::GameData;
use crate::diplomacy::Diplomacy;
use crate::economy::{Claim, Economy, Money, purchase, to_credits, to_money};
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

/// Was eine Gemeinschaft heute für den Nachschub ihrer Truppen ausgeben darf, in
/// Tausendstel Credits: der Rest der Bewilligung der Verteidigung, und ist der verbraucht,
/// die freie Kasse wie die Beschaffung. Solange noch kein Haushaltsmonat eröffnet ist,
/// gilt die Kasse. Die eine Stelle, an der die Grenze gilt; der Aufrufer verbucht, was er
/// ausgegeben hat, beim Anspruch [`Claim::Defense`].
pub fn armament_allowance(economy: &Economy, _sieges: &Sieges, polity: PolityId) -> Money {
    let opened = economy.budget(polity).is_some_and(|budget| budget.opened);
    if opened {
        economy.allowance(polity, Claim::Defense) + economy.allowance(polity, Claim::Procurement)
    } else {
        economy.treasury(polity).max(0)
    }
}

/// Kauft Nachschub für `wanted` Verbandstage am Markt des Systems: je Verbandstag einen
/// Satz der Tagesration. Gekauft werden nur ganze Sätze, soweit Ware und Rüstungsetat
/// reichen; der bezahlbare Wunsch zählt als Nachfrage. Bezahlt die Staatskasse beim
/// Kontor. Gibt die Verbandstage zurück.
fn buy_supply(
    data: &GameData,
    economy: &mut Economy,
    sieges: &Sieges,
    system: SystemId,
    polity: PolityId,
    wanted: f64,
) -> f64 {
    let bundle = &data.balance.siege.day_bundle_ids;
    let spread = data.balance.economy.spread;
    let allowance = armament_allowance(economy, sieges, polity);
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
        wanted.min(to_credits(allowance) / cost)
    } else {
        wanted
    };
    let mut sets = affordable;
    for (good, amount) in bundle {
        market.demand_today[good.index()] += affordable * amount;
        sets = sets.min(market.available(*good) / amount);
    }
    let sets = sets.max(0.0);
    let before = *treasury;
    if sets > 0.0 {
        for (good, amount) in bundle {
            purchase(market, *good, sets * amount, treasury, spread);
        }
    }
    let spent = before - *treasury;
    economy.want(polity, Claim::Defense, to_money(affordable * cost));
    economy.book_paid(polity, Claim::Defense, spent);
    sets
}

/// Prüft, ob [`embark_troops`] die Verbände einschiffen würde, ohne etwas zu ändern, und
/// gibt das System zurück, an dessen Markt der Nachschub gekauft wird.
pub fn check_embark(
    data: &GameData,
    galaxy: &Galaxy,
    diplomacy: &Diplomacy,
    sieges: &Sieges,
    orbit: &Orbit,
    polity: PolityId,
    fleet: FleetId,
    planet: PlanetId,
    strength: f64,
    supply_days: u16,
) -> Result<SystemId, String> {
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
    Ok(target.system)
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
    let system = check_embark(
        data,
        galaxy,
        diplomacy,
        sieges,
        orbit,
        polity,
        fleet,
        planet,
        strength,
        supply_days,
    )?;
    let supply = buy_supply(
        data,
        economy,
        sieges,
        system,
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

/// Setzt eingeschiffte Verbände einer eigenen Flotte an Land.
///
/// Auf einer eigenen Welt verstärken sie die Garnison, und ihr Anteil am Nachschub an
/// Bord füllt das Depot; das geht nicht, solange ein Belagerer den Orbit hält oder die
/// Oberfläche. Auf einer fremden Welt landen sie als Welle der eigenen Belagerung: Die
/// Flotte muss sie belagern, und der Planet muss eingeschlossen sein. Aktive Geschütze
/// fangen einen Teil der Welle ab. Die Landung zählt zu dem Tag, dessen Tagesschritt
/// zuletzt gerechnet wurde: Der nächste sieht die Schiffe im niedrigen Orbit.
pub fn land_troops(
    data: &GameData,
    galaxy: &Galaxy,
    polities: &Polities,
    diplomacy: &Diplomacy,
    finance: &mut Finance,
    sieges: &mut Sieges,
    orbit: &Orbit,
    polity: PolityId,
    fleet: FleetId,
    planet: PlanetId,
    strength: f64,
    tick: u64,
) -> Result<(), String> {
    let balance = &data.balance.siege;
    let target = known(galaxy, planet)?;
    let squadron = orbit.own(polity, fleet)?;
    if !squadron.docked || squadron.system != target.system {
        return Err(format!(
            "{} liegt nicht im System von {}",
            squadron.name, target.name
        ));
    }
    if !(strength.is_finite() && strength > 0.0) {
        return Err("die Truppenstärke muss positiv sein".into());
    }
    let aboard = sieges
        .embarked
        .get(&fleet)
        .map_or(0.0, |force| force.troops);
    if strength > aboard + 1e-9 {
        return Err(format!(
            "an Bord von {} sind nur {:.1} Verbände",
            squadron.name,
            aboard.max(0.0)
        ));
    }
    let strength = strength.min(aboard);
    if strength <= 0.0 {
        return Err(format!("an Bord von {} sind keine Truppen", squadron.name));
    }
    if sieges.pending.iter().any(|due| due.planet == planet) {
        return Err(format!("die Übergabe von {} steht aus", target.name));
    }
    let besieger = sieges.siege(planet).map(|siege| siege.attacker);
    let held = besieger.is_some_and(|attacker| {
        orbit.control(diplomacy, &sieges.assign, planet, target.system, attacker)
    });

    if target.owner == Some(polity) {
        // Verstärkung für die eigene Garnison.
        if let (Some(attacker), true) = (besieger, held) {
            return Err(format!(
                "{} hält den Orbit über {}",
                polities.get(attacker).name,
                target.name
            ));
        }
        let Sieges {
            active,
            defenses,
            embarked,
            ..
        } = sieges;
        let defense = &mut defenses[planet.index()];
        let siege = active.get_mut(&planet);
        if siege.as_ref().is_some_and(|siege| siege.surface_lost()) {
            return Err(format!(
                "die Oberfläche von {} hält der Belagerer",
                target.name
            ));
        }
        let Some(force) = embarked.get_mut(&fleet) else {
            return Err(format!("an Bord von {} sind keine Truppen", squadron.name));
        };
        // Das Depot zählt in Tagen für die ganze Garnison: Wächst sie, reichen dieselben
        // Vorräte kürzer; was die Truppe mitbringt, kommt hinzu, bis zum größten Depot.
        let garrison = defense.militia + defense.regulars;
        let stores = defense.depot * garrison;
        let room = balance.depot_days_cap as f64 * (garrison + strength) - stores;
        let (strength, carried) = force.disembark(strength);
        let unloaded = carried.min(room.max(0.0));
        defense.depot = (stores + unloaded) / (garrison + strength);
        defense.regulars += strength;
        // Was das Depot nicht fasst, bleibt an Bord, solange dort noch Truppen sind.
        if force.troops > 0.0 {
            force.supply += carried - unloaded;
        }
        if let Some(siege) = siege {
            // Der Maßstab der Garnison wächst mit ihr. In der Tiefe gestaffelt geht die
            // Verstärkung in den Untergrund, soweit die Bunker noch Platz haben.
            let room = (balance.deep_per_bunker * defense.bunkers as f64 - siege.deep).max(0.0);
            let below = if defense.posture == Posture::Deep && !siege.zone_taken(ZoneKind::Center) {
                strength.min(room)
            } else {
                0.0
            };
            siege.deep += below;
            siege.garrison_start += strength;
            siege.surface_start += strength - below;
        }
        embarked.retain(|_, force| force.troops > 0.0 || force.supply > 0.0);
        return Ok(());
    }

    // Landung auf einer belagerten Welt.
    if besieger != Some(polity) {
        return Err(format!(
            "{} wird nicht von der eigenen Gemeinschaft belagert",
            target.name
        ));
    }
    if !orbit
        .besiegers(&sieges.assign, planet, target.system, polity)
        .contains(&fleet)
    {
        return Err(format!("{} belagert {} nicht", squadron.name, target.name));
    }
    if !held {
        return Err(format!("keine Orbitalhoheit über {}", target.name));
    }
    let Sieges {
        active,
        defenses,
        embarked,
        stats,
        ..
    } = sieges;
    let defense = &defenses[planet.index()];
    if defense.stations > 0.0 {
        return Err(format!("Stationen halten den Orbit über {}", target.name));
    }
    let (Some(siege), Some(force)) = (active.get_mut(&planet), embarked.get_mut(&fleet)) else {
        return Err(format!("an Bord von {} sind keine Truppen", squadron.name));
    };
    // Tag des letzten gerechneten Tagesschritts: Befehle des ersten Takts eines Tages
    // gehen ihm voraus und gehören deshalb noch zum Vortag.
    let day = tick.saturating_sub(1) / TICKS_PER_DAY;
    let landing = land(siege, defense, strength, day, balance);
    // Mit den letzten Verbänden geht auch der Nachschub von Bord; sonst bringt ihn der
    // nächste Tagesschritt zur Truppe.
    siege.army_supply += force.leave(strength);
    embarked.retain(|_, force| force.troops > 0.0 || force.supply > 0.0);
    if landing.lost > 0.0 {
        stats.lost_attackers += landing.lost;
        weary(finance, polity, balance.wear_loss * landing.lost);
    }
    Ok(())
}

/// Der Besitzer übergibt eine belagerte eigene Welt. Dazu muss eine Besatzungstruppe des
/// Belagerers zur Stelle sein: am Boden, oder an Bord seiner Flotten im Orbit; was am
/// Boden fehlt, geht ohne Verluste an Land. Der Besitzwechsel folgt am nächsten
/// Tagesabschluss, ohne Plünderung und mit milderer Besatzung.
pub fn capitulate(
    data: &GameData,
    galaxy: &Galaxy,
    diplomacy: &Diplomacy,
    sieges: &mut Sieges,
    orbit: &Orbit,
    polity: PolityId,
    planet: PlanetId,
) -> Result<(), String> {
    let target = known(galaxy, planet)?;
    if target.owner != Some(polity) {
        return Err(format!(
            "{} gehört nicht der eigenen Gemeinschaft",
            target.name
        ));
    }
    let Some((attacker, army)) = sieges
        .siege(planet)
        .map(|siege| (siege.attacker, siege.army))
    else {
        return Err(format!("{} wird nicht belagert", target.name));
    };
    if sieges.pending.iter().any(|due| due.planet == planet) {
        return Err(format!("die Übergabe von {} steht aus", target.name));
    }
    let squadron = orbit.squadron(
        data,
        diplomacy,
        sieges,
        planet,
        target.system,
        attacker,
        polity,
    );
    // Wer übergibt, wehrt sich nicht: Truppen an Bord zählen, sobald der Belagerer den
    // Orbit hält, auch wenn noch Stationen stehen.
    let aboard = if squadron.view.control {
        squadron.view.embarked.max(0.0)
    } else {
        0.0
    };
    let needed = occupation_force(target.population, &data.balance.siege);
    if army + aboard <= 0.0 || army + aboard < needed {
        return Err("niemand da, der die Übergabe annimmt".into());
    }
    let landed = (needed - army).clamp(0.0, aboard);
    if let Some(siege) = sieges.active.get_mut(&planet) {
        siege.army += landed;
        siege.army_peak = siege.army_peak.max(siege.army);
    }
    let supply = take_troops(sieges, &squadron.fleets, landed);
    if let Some(siege) = sieges.active.get_mut(&planet) {
        siege.army_supply += supply;
    }
    sieges.pending.push(Conquest {
        planet,
        to: attacker,
        outcome: Outcome::Capitulated,
        sack: false,
    });
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
    use crate::siege::Embarked;
    use crate::siege::orbit::testing::{FOE, ME, Scene};

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

    fn land_at(
        scene: &mut Scene,
        polity: PolityId,
        fleet: u32,
        planet: PlanetId,
        strength: f64,
        tick: u64,
    ) -> Result<(), String> {
        let state = &mut scene.state;
        let orbit = Orbit::new(&mut state.fleets, &state.research);
        land_troops(
            &scene.data,
            &state.galaxy,
            &state.polities,
            &state.diplomacy,
            &mut state.finance,
            &mut state.sieges,
            &orbit,
            polity,
            FleetId(fleet),
            planet,
            strength,
            tick,
        )
    }

    /// Landet mitten am laufenden Tag.
    fn land_now(
        scene: &mut Scene,
        polity: PolityId,
        fleet: u32,
        strength: f64,
    ) -> Result<(), String> {
        let tick = scene.day * TICKS_PER_DAY + 7;
        land_at(scene, polity, fleet, scene.planet, strength, tick)
    }

    fn surrender(scene: &mut Scene, polity: PolityId) -> Result<(), String> {
        let state = &mut scene.state;
        let orbit = Orbit::new(&mut state.fleets, &state.research);
        capitulate(
            &scene.data,
            &state.galaxy,
            &state.diplomacy,
            &mut state.sieges,
            &orbit,
            polity,
            scene.planet,
        )
    }

    #[test]
    fn a_wave_lands_only_from_a_besieging_fleet_over_an_invested_planet() {
        let mut scene = Scene::colony();
        let name = scene.state.galaxy.planet(scene.planet).name.clone();
        embark(&mut scene, 0, 10.0, 300.0);
        // Ohne Belagerung keine Landung; der Datensatz entsteht am nächsten Morgen.
        let reason = land_now(&mut scene, ME, 0, 4.0).unwrap_err();
        assert_eq!(
            reason,
            format!("{name} wird nicht von der eigenen Gemeinschaft belagert")
        );
        scene.day();
        // Nur was an Bord ist, nur in positiver Stärke, nur der Belagerer.
        let reason = land_now(&mut scene, ME, 0, 10.5).unwrap_err();
        assert!(reason.contains("sind nur 10.0 Verbände"), "{reason}");
        let reason = land_now(&mut scene, ME, 0, 0.0).unwrap_err();
        assert!(reason.contains("positiv"), "{reason}");
        let reason = land_now(&mut scene, FOE, 0, 1.0).unwrap_err();
        assert!(reason.contains("Flotte gehört einer anderen"), "{reason}");
        // Stationen halten den Orbit: nicht eingeschlossen, keine Landung.
        scene.state.sieges.defense_mut(scene.planet).stations = 0.5;
        let reason = land_now(&mut scene, ME, 0, 4.0).unwrap_err();
        assert_eq!(reason, format!("Stationen halten den Orbit über {name}"));
        scene.state.sieges.defense_mut(scene.planet).stations = 0.0;
        // Eine Flotte des Verteidigers im System: keine Orbitalhoheit.
        let away = scene.fleet_system(1);
        scene.place(1, scene.system);
        let reason = land_now(&mut scene, ME, 0, 4.0).unwrap_err();
        assert_eq!(reason, format!("keine Orbitalhoheit über {name}"));
        scene.place(1, away);
        // Eine Flotte, die den Planeten nicht belagert, landet nichts.
        scene.state.sieges.assign.clear();
        let reason = land_now(&mut scene, ME, 0, 4.0).unwrap_err();
        assert!(
            reason.ends_with(&format!("belagert {name} nicht")),
            "{reason}"
        );
        scene.state.sieges.assign.insert(FleetId(0), scene.planet);

        // Die Welle landet. Die Kolonie hat keine Geschütze: Alle kommen an. Der Nachschub
        // bleibt an Bord, bis der Tagesschritt ihn mit Orbitalhoheit zur Truppe bringt.
        land_now(&mut scene, ME, 0, 4.0).unwrap();
        let siege = scene.siege();
        assert_eq!((siege.army, siege.army_peak), (4.0, 4.0));
        assert_eq!(siege.landed_day, Some(scene.day));
        assert_eq!(siege.army_supply, 0.0);
        let aboard = &scene.state.sieges.embarked[&FleetId(0)];
        assert_eq!((aboard.troops, aboard.supply), (6.0, 300.0));
        assert_eq!(scene.state.sieges.stats.lost_attackers, 0.0);
        scene.day();
        assert_eq!(
            scene.siege().army_supply,
            300.0 - 4.0 * scene.data.balance.siege.army_idle_supply
        );
        assert_eq!(scene.state.sieges.embarked[&FleetId(0)].supply, 0.0);
        // Ein Befehl im ersten Takt eines Tages geht dessen Tagesschritt voraus: Die
        // Landung zählt noch zum Vortag, wie jede zwischen zwei Tagesschritten.
        let tick = scene.day * TICKS_PER_DAY;
        let planet = scene.planet;
        land_at(&mut scene, ME, 0, planet, 6.0, tick).unwrap();
        assert_eq!(scene.siege().landed_day, Some(scene.day - 1));
        assert_eq!(scene.siege().army, 10.0);
        // Alle sind von Bord; ohne Nachschub trägt die Flotte nichts mehr.
        assert!(!scene.state.sieges.embarked.contains_key(&FleetId(0)));
        assert_eq!(
            scene
                .state
                .sieges
                .find_anomaly(scene.state.galaxy.planets.len()),
            None
        );
    }

    #[test]
    fn active_guns_intercept_a_share_of_the_wave() {
        let mut scene = Scene::new();
        let balance = scene.data.balance.siege.clone();
        embark(&mut scene, 0, 10.0, 0.0);
        scene.day();
        let guns = scene.state.sieges.defense(scene.planet).guns as f64;
        assert!(guns > 0.0, "die Hauptwelt hat Geschütze");
        let weariness = scene.state.finance.polity(ME).war_weariness;
        land_now(&mut scene, ME, 0, 10.0).unwrap();
        let lost = 10.0 * (balance.landing_loss * guns).min(balance.landing_loss_cap);
        assert!(lost > 0.0);
        let siege = scene.siege();
        assert!((siege.army - (10.0 - lost)).abs() < 1e-9);
        // Die Abgefangenen sind verloren und drücken auf die Stimmung daheim.
        assert!((scene.state.sieges.stats.lost_attackers - lost).abs() < 1e-9);
        let gained = scene.state.finance.polity(ME).war_weariness - weariness;
        assert!((gained - balance.wear_loss * lost).abs() < 1e-9);
        assert!(!scene.state.sieges.embarked.contains_key(&FleetId(0)));
        // Am Tag danach stehen die Schiffe im niedrigen Orbit: Die Geschütze treffen sie.
        scene.day();
        assert!(scene.siege().report.hull > 0.0);
        assert!(scene.ships(0)[0].hull < 1.0);

        // Schweigen die Geschütze, kommt die ganze Welle an.
        let mut scene = Scene::new();
        embark(&mut scene, 0, 10.0, 0.0);
        scene.day();
        scene.state.sieges.defense_mut(scene.planet).hold_fire = true;
        land_now(&mut scene, ME, 0, 10.0).unwrap();
        assert_eq!(scene.siege().army, 10.0);
        assert_eq!(scene.state.sieges.stats.lost_attackers, 0.0);
    }

    #[test]
    fn troops_landed_on_an_own_world_join_the_garrison_and_fill_its_stores() {
        let mut scene = Scene::new();
        let balance = scene.data.balance.siege.clone();
        let (planet, system) = (scene.planet, scene.system);
        let name = scene.state.galaxy.planet(planet).name.clone();
        // Die Flotte des Verteidigers bringt Verstärkung und Nachschub.
        embark(&mut scene, 1, 6.0, 900.0);
        let reason = land_now(&mut scene, FOE, 1, 2.0).unwrap_err();
        assert!(reason.contains("liegt nicht im System"), "{reason}");
        scene.day();
        let siege = scene.siege().clone();
        let before = scene.state.sieges.defense(planet).clone();
        scene.place(1, system);
        // Sammelt sie sich noch, hält der Belagerer den Orbit: keine Landung.
        scene.regroup(1, 99);
        let reason = land_now(&mut scene, FOE, 1, 2.0).unwrap_err();
        let besieger = scene.state.polities.get(ME).name.clone();
        assert_eq!(reason, format!("{besieger} hält den Orbit über {name}"));
        scene.ready(1);
        // Einsatzbereit macht sie ihm den Orbit streitig, und die Truppen gehen an Land:
        // Sie zählen zur Garnison und zu ihrem Maßstab, ihr Anteil am Nachschub zum Depot.
        land_now(&mut scene, FOE, 1, 2.0).unwrap();
        let defense = scene.state.sieges.defense(planet);
        assert_eq!(defense.regulars, before.regulars + 2.0);
        let garrison = before.militia + before.regulars;
        let depot = (before.depot * garrison + 300.0) / (garrison + 2.0);
        assert!((defense.depot - depot).abs() < 1e-9, "{}", defense.depot);
        let aboard = &scene.state.sieges.embarked[&FleetId(1)];
        assert_eq!((aboard.troops, aboard.supply), (4.0, 600.0));
        let now = scene.siege();
        assert_eq!(now.garrison_start, siege.garrison_start + 2.0);
        assert_eq!(now.surface_start, siege.surface_start + 2.0);
        assert_eq!(now.deep, 0.0);
        // In der Tiefe gestaffelt geht die Verstärkung in die Bunker, soweit Platz ist.
        let defense = scene.state.sieges.defense_mut(planet);
        defense.bunkers = 1;
        defense.posture = Posture::Deep;
        let room = balance.deep_per_bunker;
        scene.state.sieges.active.get_mut(&planet).unwrap().deep = room - 1.5;
        land_now(&mut scene, FOE, 1, 4.0).unwrap();
        let now = scene.siege();
        assert_eq!(now.deep, room);
        assert_eq!(now.surface_start, siege.surface_start + 2.0 + 2.5);
        assert_eq!(now.garrison_start, siege.garrison_start + 6.0);
        assert!(!scene.state.sieges.embarked.contains_key(&FleetId(1)));
        assert_eq!(
            scene
                .state
                .sieges
                .find_anomaly(scene.state.galaxy.planets.len()),
            None
        );
        // Hält der Belagerer die Oberfläche, landet dort niemand mehr.
        embark(&mut scene, 1, 1.0, 0.0);
        for zone in &mut scene.state.sieges.active.get_mut(&planet).unwrap().zones {
            zone.taken = true;
        }
        let reason = land_now(&mut scene, FOE, 1, 1.0).unwrap_err();
        assert_eq!(
            reason,
            format!("die Oberfläche von {name} hält der Belagerer")
        );

        // Ohne Belagerung: Truppen und Nachschub kommen heim. Das Depot fasst nicht mehr
        // als das größte, das sich planen lässt; was darüber liegt, verfällt mit den
        // letzten Verbänden, denn ohne Truppe bleibt kein Nachschub an Bord.
        let mut scene = Scene::new();
        let home = scene.state.polities.get(ME).capital.unwrap();
        let system = scene.state.galaxy.planet(home).system;
        scene.place(0, system);
        scene.state.sieges.assign.clear();
        embark(&mut scene, 0, 5.0, 10_000.0);
        let before = scene.state.sieges.defense(home).clone();
        let tick = 7;
        land_at(&mut scene, ME, 0, home, 5.0, tick).unwrap();
        let defense = scene.state.sieges.defense(home);
        assert_eq!(defense.regulars, before.regulars + 5.0);
        assert_eq!(defense.depot, balance.depot_days_cap as f64);
        assert!(!scene.state.sieges.embarked.contains_key(&FleetId(0)));
    }

    #[test]
    fn supply_never_stays_aboard_without_troops_and_no_landing_conjures_stores() {
        // Die letzte Welle über einer belagerten Welt nimmt den Nachschub mit an Land.
        let mut scene = Scene::colony();
        embark(&mut scene, 0, 10.0, 210.0);
        scene.day();
        land_now(&mut scene, ME, 0, 10.0).unwrap();
        assert_eq!(scene.siege().army_supply, 210.0);
        assert!(!scene.state.sieges.embarked.contains_key(&FleetId(0)));

        // Ein Eintrag mit Nachschub, aber ohne Truppe (aus älteren Ständen): Auch eine
        // winzige Stärke entlädt ihn nicht, und das Depot der Welt bleibt, wie es war.
        let mut scene = Scene::new();
        let home = scene.state.polities.get(ME).capital.unwrap();
        let system = scene.state.galaxy.planet(home).system;
        scene.place(0, system);
        scene.state.sieges.assign.clear();
        embark(&mut scene, 0, 0.0, 210.0);
        let before = scene.state.sieges.defense(home).clone();
        let reason = land_at(&mut scene, ME, 0, home, 1e-10, 7).unwrap_err();
        assert!(reason.contains("sind keine Truppen"), "{reason}");
        assert_eq!(scene.state.sieges.defense(home), &before);
        // Auf einer Welt ohne Garnison bleibt das Depot eine Zahl.
        embark(&mut scene, 0, 2.0, 20.0);
        let defense = scene.state.sieges.defense_mut(home);
        (defense.militia, defense.regulars, defense.depot) = (0.0, 0.0, 0.0);
        land_at(&mut scene, ME, 0, home, 2.0, 7).unwrap();
        let defense = scene.state.sieges.defense(home);
        assert_eq!((defense.regulars, defense.depot), (2.0, 10.0));
    }

    #[test]
    fn a_world_is_handed_over_only_to_an_occupation_force() {
        let mut scene = Scene::colony();
        let balance = scene.data.balance.siege.clone();
        let planet = scene.planet;
        let name = scene.state.galaxy.planet(planet).name.clone();
        let population = scene.state.galaxy.planet(planet).population;
        let needed = occupation_force(population, &balance);
        // Nur der Besitzer übergibt, und nur eine belagerte Welt.
        let reason = surrender(&mut scene, ME).unwrap_err();
        assert_eq!(
            reason,
            format!("{name} gehört nicht der eigenen Gemeinschaft")
        );
        let reason = surrender(&mut scene, FOE).unwrap_err();
        assert_eq!(reason, format!("{name} wird nicht belagert"));
        scene.day();
        // Ohne Besatzungstruppe nimmt niemand die Übergabe an.
        let reason = surrender(&mut scene, FOE).unwrap_err();
        assert_eq!(reason, "niemand da, der die Übergabe annimmt");
        embark(&mut scene, 0, 0.5 * needed, 0.0);
        let reason = surrender(&mut scene, FOE).unwrap_err();
        assert_eq!(reason, "niemand da, der die Übergabe annimmt");
        // Truppen an Bord zählen nur, solange der Belagerer den Orbit hält.
        embark(&mut scene, 0, needed + 1.0, 40.0);
        let away = scene.fleet_system(1);
        scene.place(1, scene.system);
        let reason = surrender(&mut scene, FOE).unwrap_err();
        assert_eq!(reason, "niemand da, der die Übergabe annimmt");
        scene.place(1, away);

        let militia = scene.state.sieges.defense(planet).militia;
        assert!(militia > 0.0, "die Miliz steht noch");
        surrender(&mut scene, FOE).unwrap();
        // Was zur Besatzung fehlte, ist ohne Verluste gelandet; der Besitzwechsel steht aus.
        assert!((scene.siege().army - needed).abs() < 1e-9);
        assert!((scene.state.sieges.embarked[&FleetId(0)].troops - 1.0).abs() < 1e-9);
        assert_eq!(
            scene.state.sieges.pending,
            vec![Conquest {
                planet,
                to: ME,
                outcome: Outcome::Capitulated,
                sack: false,
            }]
        );
        let reason = surrender(&mut scene, FOE).unwrap_err();
        assert_eq!(reason, format!("die Übergabe von {name} steht aus"));
        let reason = land_now(&mut scene, ME, 0, 1.0).unwrap_err();
        assert_eq!(reason, format!("die Übergabe von {name} steht aus"));

        // Am nächsten Tagesabschluss gehört die Welt dem Belagerer, mit milderer Besatzung.
        scene.day();
        let state = &scene.state;
        assert_eq!(state.galaxy.planet(planet).owner, Some(ME));
        assert!(state.sieges.active.is_empty() && state.sieges.pending.is_empty());
        let stats = &state.sieges.stats;
        assert_eq!((stats.capitulated, stats.conquests()), (1, 1));
        assert_eq!((stats.lost_attackers, stats.lost_defenders), (0.0, 0.0));
        let defense = state.sieges.defense(planet);
        assert_eq!(defense.occupation, balance.capitulation_mercy);
        assert_eq!(defense.occupied_from, Some(FOE));
        assert!((defense.regulars - needed).abs() < 1e-9);
        assert_eq!(defense.scar, 0.0);
        let told = state
            .chronicle
            .entries
            .iter()
            .any(|entry| entry.text.contains("übergeben") && entry.text.contains("besetzt"));
        assert!(told, "die Chronik nennt die Übergabe");
    }

    #[test]
    fn the_siege_rests_while_a_handover_is_pending_after_a_capitulation() {
        let mut scene = Scene::new();
        scene.hold_verdicts = true;
        let planet = scene.planet;
        let needed = occupation_force(
            scene.state.galaxy.planet(planet).population,
            &scene.data.balance.siege,
        );
        embark(&mut scene, 0, 17.0, 0.0);
        assert!(needed < 17.0);
        scene.day();
        surrender(&mut scene, FOE).unwrap();
        let due = scene.state.sieges.pending.clone();
        assert_eq!(due.len(), 1);
        let depot = scene.state.sieges.defense(planet).depot;
        for _ in 0..3 {
            scene.day();
        }
        // Bis zum Tagesabschluss bleibt die Welt beim Verteidiger: Das Urteil steht, die
        // Belagerung ruht.
        assert_eq!(scene.state.galaxy.planet(planet).owner, Some(FOE));
        assert_eq!(scene.state.sieges.pending, due);
        let siege = scene.siege();
        assert!(
            siege
                .report
                .blocked
                .contains(crate::siege::Blocked::Handover)
        );
        assert!((siege.army - needed).abs() < 1e-9);
        assert_eq!(scene.state.sieges.defense(planet).depot, depot);
        assert_eq!(scene.state.sieges.stats.conquests(), 0);
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

//! Werften: Der Staat bestellt Schiffe, die Werft baut sie aus Bauteilen vom Markt ihres
//! Systems.
//!
//! Eine Welt hat eine Werft, wenn in ihrem Zweig Rüstung genug Kapital steht; die Hauptwelt
//! hat immer einen Bauplatz. Jeden Tag kauft der Staat für jeden Auftrag auf einem Bauplatz
//! den Tagesanteil der Bauteile zum lokalen Preis und zahlt der Rüstungsfirma der Welt
//! einen Aufschlag. Fehlt ein Bauteil am Markt oder Geld in der Kasse, rückt der Bau nur
//! so weit vor, wie der knappste Posten reicht: Eine blockierte oder schlecht versorgte
//! Werft steht still, ohne dass es dafür eine eigene Regel bräuchte.
//!
//! Jeder Auftrag trägt seinen Stand ([`BuildStatus`]): ob er gebaut wird, wartet, stockt
//! (und woran), fertig im Dock liegt oder ruht, weil die Welt keine Werft mehr hat.

use super::{Fleets, Ship, supply_base};
use crate::chronicle::{Chronicle, ChronicleKind};
use crate::clock::{Date, TICKS_PER_DAY};
use crate::data::{GameData, Sector, ShipClass};
use crate::diplomacy::Diplomacy;
use crate::economy::{Economy, Money, pay, purchase, recipes_of, to_credits, to_money};
use crate::galaxy::Galaxy;
use crate::ids::{GoodId, PlanetId, PolityId};
use crate::polity::Polities;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Ab diesem Fortschritt gilt ein Schiff als fertig.
const DONE: f32 = 1.0 - 1e-6;

/// Stand eines Bauauftrags: wo er in der Werft steht und, wenn er nicht vorankommt, warum.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BuildStatus {
    /// Auf einem Bauplatz; der Bau rückt vor.
    #[default]
    Building,
    /// Wartet auf einen freien Bauplatz.
    Queued,
    /// Auf einem Bauplatz, aber am letzten Bautag war dieses Bauteil am Markt zu knapp.
    MissingPart(GoodId),
    /// Auf einem Bauplatz, aber am letzten Bautag reichte das Geld des Staates nicht.
    MissingMoney,
    /// Auf einem Bauplatz, aber der Markt des Systems untersteht einer Gemeinschaft, mit
    /// der Krieg herrscht: Sie verkauft der Werft nichts.
    MarketClosed,
    /// Fertig, bleibt aber im Dock: Feindliche Flotten stehen im System, und keine eigene,
    /// einsatzbereite nimmt das Schiff auf.
    HeldInDock,
    /// Die Welt hat keine Werft mehr. Der halbfertige Rumpf liegt seit dem Tag `since`
    /// still; kehrt die Werft nicht binnen `military.yard_grace_days` zurück, verfällt der
    /// Auftrag.
    NoYard { since: u64 },
}

impl BuildStatus {
    /// Stockt der Bau auf seinem Bauplatz: Bauteile, Geld oder der Zugang zum Markt fehlen?
    /// Warten auf einen Bauplatz, im Dock liegen und Ruhen ohne Werft zählen nicht dazu.
    pub fn is_stalled(self) -> bool {
        matches!(
            self,
            BuildStatus::MissingPart(_) | BuildStatus::MissingMoney | BuildStatus::MarketClosed
        )
    }
}

/// Bauauftrag einer Werft.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BuildOrder {
    pub id: u32,
    pub polity: PolityId,
    /// Welt, deren Werft baut.
    pub planet: PlanetId,
    /// Index in [`GameData::designs`].
    pub design: u16,
    /// Baufortschritt 0..1.
    pub progress: f32,
    /// Bisher gezahlt: Bauteile und Werftaufschlag. Das Geld steckt im Rumpf: Es wird
    /// weder erstattet, wenn der Auftrag gestrichen wird, noch wenn er verfällt.
    pub spent: Money,
    /// Stand des Auftrags nach dem letzten Bautag.
    #[serde(default)]
    pub status: BuildStatus,
    /// Tag der Auftragserteilung.
    pub started: u64,
}

impl BuildOrder {
    /// Stockt der Bau, weil Bauteile, Geld oder der Zugang zum Markt fehlen?
    pub fn stalled(&self) -> bool {
        self.status.is_stalled()
    }

    /// Der Stand in Worten, für Listen: was mit dem Auftrag geschieht und, wenn er nicht
    /// vorankommt, warum.
    pub fn status_text(&self, data: &GameData) -> String {
        match self.status {
            BuildStatus::Building => "im Bau".into(),
            BuildStatus::Queued => "wartet auf einen Bauplatz".into(),
            BuildStatus::MissingPart(good) => {
                format!("stockt: {} am Markt zu knapp", data.good(good).name)
            }
            BuildStatus::MissingMoney => "stockt: dem Staat fehlt das Geld für die Bauteile".into(),
            BuildStatus::MarketClosed => {
                "stockt: Der Markt des Systems untersteht einem Kriegsgegner".into()
            }
            BuildStatus::HeldInDock => {
                "fertig, bleibt im Dock: feindliche Flotten im System".into()
            }
            BuildStatus::NoYard { since } => format!(
                "ruht seit dem {}: Die Welt hat keine Werft mehr; ohne sie verfällt der Auftrag am {}",
                Date::from_day(since),
                Date::from_day(since + data.balance.military.yard_grace_days)
            ),
        }
    }
}

/// Kapitalstock im Zweig Rüstung einer Welt.
pub fn arms_capital(data: &GameData, economy: &Economy, planet: PlanetId) -> f64 {
    let local = economy.planet(planet);
    recipes_of(data, Sector::Arms)
        .map(|(index, _)| local.capital[index])
        .sum()
}

/// Führt nach, welche Welten eine Werft haben ([`Fleets::yards`]). Eine Werft entsteht, wenn
/// das Rüstungskapital einer besiedelten Welt `yard_open_capital` erreicht, und besteht
/// fort, bis es unter `yard_close_capital` fällt: Reste von Kapital, die nur noch verfallen,
/// machen keine Werft, und eine Werft an der Schwelle wechselt nicht von Monat zu Monat.
/// Die Hauptwelt jeder Gemeinschaft hat ihre Werft unabhängig davon.
pub fn update_yards(data: &GameData, galaxy: &Galaxy, economy: &Economy, fleets: &mut Fleets) {
    let military = &data.balance.military;
    let mut yards = Vec::new();
    for planet in &galaxy.planets {
        if planet.owner.is_none() {
            continue;
        }
        let threshold = if fleets.yards.binary_search(&planet.id).is_ok() {
            military.yard_close_capital
        } else {
            military.yard_open_capital
        };
        if arms_capital(data, economy, planet.id) >= threshold {
            yards.push(planet.id);
        }
    }
    fleets.yards = yards;
}

/// Bauplätze der Werft einer Welt. Null heißt: keine Werft. Eine Werft haben die Hauptwelt
/// jeder Gemeinschaft und jede Welt mit genug Rüstungskapital ([`update_yards`]). Je
/// `yard_capital_per_slot` Rüstungskapital gibt es einen Platz, mindestens einen.
pub fn yard_slots(
    data: &GameData,
    galaxy: &Galaxy,
    economy: &Economy,
    polities: &Polities,
    fleets: &Fleets,
    planet: PlanetId,
) -> u32 {
    let Some(world) = galaxy.planets.get(planet.index()) else {
        return 0;
    };
    let Some(owner) = world.owner else {
        return 0;
    };
    let is_capital = polities
        .list
        .get(owner.index())
        .is_some_and(|polity| polity.capital == Some(planet));
    if !is_capital && fleets.yards.binary_search(&planet).is_err() {
        return 0;
    }
    let capital = arms_capital(data, economy, planet);
    ((capital / data.balance.military.yard_capital_per_slot) as u32).max(1)
}

/// Darf eine Gemeinschaft diesen Entwurf bauen? `known[tech]` sagt je Technologie, ob sie
/// erforscht ist; was die Liste nicht nennt, gilt als unbekannt. Bordjäger werden nie
/// einzeln gebaut.
pub fn design_buildable(data: &GameData, design: u16, known: &[bool]) -> Result<(), String> {
    let Some(def) = data.designs.get(design as usize) else {
        return Err("Entwurf unbekannt".into());
    };
    if def.stats.class == ShipClass::Fighter {
        return Err(format!(
            "{} ist ein Bordjäger und entsteht nur zusammen mit seinem Träger",
            def.name
        ));
    }
    for tech in &def.stats.techs {
        if !known.get(*tech as usize).copied().unwrap_or(false) {
            return Err(format!(
                "{} setzt die Technologie „{}“ voraus",
                def.name, data.techs[*tech as usize].name
            ));
        }
    }
    Ok(())
}

/// Gibt bei der Werft einer eigenen Welt ein Schiff in Auftrag. Gibt die Auftragsnummer
/// zurück.
pub fn order_ship(
    data: &GameData,
    galaxy: &Galaxy,
    economy: &Economy,
    polities: &Polities,
    fleets: &mut Fleets,
    polity: PolityId,
    planet: PlanetId,
    design: u16,
    known: &[bool],
    day: u64,
) -> Result<u32, String> {
    let Some(world) = galaxy.planets.get(planet.index()) else {
        return Err("Planet unbekannt".into());
    };
    if world.owner != Some(polity) {
        return Err(format!(
            "{} gehört nicht zur eigenen Gemeinschaft",
            world.name
        ));
    }
    if yard_slots(data, galaxy, economy, polities, fleets, planet) == 0 {
        return Err(format!(
            "{} hat keine Werft: Im Zweig Rüstung steht dort zu wenig Kapital",
            world.name
        ));
    }
    design_buildable(data, design, known)?;
    let limit = data.balance.military.yard_queue as usize;
    if fleets.orders_at(planet).count() >= limit {
        return Err(format!(
            "die Werft von {} nimmt nicht mehr als {limit} Aufträge an",
            world.name
        ));
    }
    let id = fleets.next_order;
    fleets.next_order += 1;
    fleets.orders.push(BuildOrder {
        id,
        polity,
        planet,
        design,
        progress: 0.0,
        spent: 0,
        status: BuildStatus::Queued,
        started: day,
    });
    Ok(id)
}

/// Streicht einen eigenen Bauauftrag. Was schon verbaut ist, wird nicht erstattet.
pub fn cancel_order(fleets: &mut Fleets, polity: PolityId, order: u32) -> Result<(), String> {
    let Some(index) = fleets.orders.iter().position(|entry| entry.id == order) else {
        return Err("Bauauftrag unbekannt".into());
    };
    if fleets.orders[index].polity != polity {
        return Err("Bauauftrag gehört einer anderen Gemeinschaft".into());
    }
    fleets.orders.remove(index);
    Ok(())
}

/// Warum ein Auftrag verfällt.
enum Lapse {
    /// Die Welt gehört jetzt einer anderen Gemeinschaft.
    OwnerChanged,
    /// Die Welt hat seit `yard_grace_days` keine Werft mehr.
    NoYard,
}

/// Tagesschritt aller Werften. Je Welt rücken die Aufträge in Nummernfolge vor, höchstens
/// so viele zugleich, wie die Werft Bauplätze hat. Fertige Schiffe stoßen zu einer eigenen,
/// einsatzbereiten Flotte, die im System liegt, sonst entsteht dort eine neue. Stehen
/// feindliche Flotten im System und nimmt keine eigene das Schiff auf, bleibt es im Dock
/// und belegt seinen Bauplatz, bis der Orbit frei ist: Ein einzelnes Schiff ohne Munition
/// liefe sonst geradewegs in sein erstes Gefecht.
///
/// Hat die Welt keine Werft mehr, ruhen ihre Aufträge. Fortschritt und Bezahltes bleiben
/// erhalten, und der Bau geht weiter, sobald es wieder eine Werft gibt. Bleibt sie
/// `yard_grace_days` lang aus, verfällt der Auftrag wie ein gestrichener: ohne Erstattung,
/// mit einem Eintrag in der Chronik. Ein Schiff, das schon fertig ist, läuft auch ohne
/// Werft aus.
pub fn advance_yards(
    galaxy: &Galaxy,
    economy: &mut Economy,
    fleets: &mut Fleets,
    polities: &Polities,
    diplomacy: &Diplomacy,
    chronicle: &mut Chronicle,
    data: &GameData,
    tick: u64,
) {
    if fleets.orders.is_empty() {
        return;
    }
    let balance = &data.balance.military;
    let spread = data.balance.economy.spread;
    let day = tick / TICKS_PER_DAY;
    // Bauplätze jeder Welt mit Aufträgen, wie sie heute sind.
    let mut slots: BTreeMap<PlanetId, u32> = BTreeMap::new();
    for order in &fleets.orders {
        slots
            .entry(order.planet)
            .or_insert_with(|| yard_slots(data, galaxy, economy, polities, fleets, order.planet));
    }
    let mut busy: BTreeMap<PlanetId, u32> = BTreeMap::new();
    let mut finished: Vec<u32> = Vec::new();
    let mut lapsed: Vec<(u32, Lapse)> = Vec::new();
    for index in 0..fleets.orders.len() {
        let order = &mut fleets.orders[index];
        let world = galaxy.planet(order.planet);
        if world.owner != Some(order.polity) {
            lapsed.push((order.id, Lapse::OwnerChanged));
            continue;
        }
        let slots = slots[&order.planet];
        let used = busy.entry(order.planet).or_default();
        if order.progress >= DONE {
            // Fertig: Das Schiff läuft aus, sobald der Orbit es zulässt, auch wenn die
            // Werft inzwischen fehlt. Solange es im Dock liegt, belegt es seinen Bauplatz.
            *used += 1;
            finished.push(order.id);
            continue;
        }
        if slots == 0 {
            let since = match order.status {
                BuildStatus::NoYard { since } => since,
                _ => day,
            };
            order.status = BuildStatus::NoYard { since };
            if day >= since + balance.yard_grace_days {
                lapsed.push((order.id, Lapse::NoYard));
            }
            continue;
        }
        if *used >= slots {
            order.status = BuildStatus::Queued;
            continue;
        }
        *used += 1;
        if !supply_base(galaxy, diplomacy, order.polity, world.system) {
            order.status = BuildStatus::MarketClosed;
            continue;
        }

        let stats = &data.designs[order.design as usize].stats;
        // Tagesanteil des Baus; am letzten Tag nur noch der Rest.
        let step = (1.0 / stats.build_days.max(1) as f32)
            .min(1.0 - order.progress)
            .max(0.0);
        let market = &mut economy.markets[world.system.index()];
        let treasury = &mut economy.treasuries[order.polity.index()];

        // Der knappste Posten bestimmt, wie weit der Tag reicht: zuerst die Ware …
        let mut in_stock = 1.0f64;
        let mut scarcest: Option<GoodId> = None;
        let mut cost = 0.0;
        for (good, amount) in &stats.parts {
            let wanted = *amount as f64 * step as f64;
            if wanted > 0.0 {
                let reach = market.available(*good) / wanted;
                if reach < in_stock {
                    in_stock = reach;
                    scarcest = Some(*good);
                }
                cost += wanted * market.ask(*good, spread);
            }
        }
        // … dann die Kasse, aus der auch der Aufschlag der Werft bezahlt wird.
        let total = cost * (1.0 + balance.yard_fee);
        let affordable = if total > 0.0 {
            (to_credits((*treasury).max(0)) / total).min(1.0)
        } else {
            1.0
        };
        // Der bezahlbare Bedarf zählt als Nachfrage, auch wenn die Ware fehlt: So steigen
        // die Preise, Frachter bringen Bauteile und die Rüstungsbetriebe wachsen.
        for (good, amount) in &stats.parts {
            market.demand_today[good.index()] += *amount as f64 * step as f64 * affordable;
        }
        let mut share = in_stock.min(affordable).clamp(0.0, 1.0);
        if to_money(cost) <= 0 {
            // Ein Rest unter einem Tausendstel Credit (Rundung am letzten Bautag) wird
            // nicht mehr gehandelt; der Bau schließt ab.
            share = 1.0;
            order.progress += step;
        } else if to_money(cost * share) <= 0 {
            share = 0.0;
        } else {
            let mut parts: Money = 0;
            for (good, amount) in &stats.parts {
                let quantity = *amount as f64 * step as f64 * share;
                parts += purchase(market, *good, quantity, treasury, spread).1;
            }
            let firm = &mut economy.planets[order.planet.index()].firms[Sector::Arms.index()];
            let fee = pay(
                treasury,
                &mut firm.cash,
                (parts as f64 * balance.yard_fee).round() as Money,
            );
            firm.revenue_today += to_credits(fee);
            firm.profit_today += to_credits(fee);
            order.spent += parts + fee;
            order.progress += step * share as f32;
        }
        // Woran es lag, wenn der Tag nicht voll genutzt wurde: an der Ware, wenn sie
        // knapper war als das Geld, sonst am Geld.
        order.status = if share >= 1.0 {
            BuildStatus::Building
        } else {
            match scarcest {
                Some(good) if in_stock <= affordable => BuildStatus::MissingPart(good),
                _ => BuildStatus::MissingMoney,
            }
        };
        if order.progress >= DONE {
            finished.push(order.id);
        }
    }

    for (id, cause) in lapsed {
        let index = fleets
            .orders
            .iter()
            .position(|order| order.id == id)
            .expect("eben gelesen");
        let order = fleets.orders.remove(index);
        let world = galaxy.planet(order.planet);
        let design = &data.designs[order.design as usize];
        let reason = match cause {
            Lapse::OwnerChanged => "die Welt hat den Besitzer gewechselt".to_string(),
            Lapse::NoYard => format!(
                "die Welt hat seit {} Tagen keine Werft mehr. Verbaut waren {:.0} Credits; \
                 sie sind abgeschrieben",
                balance.yard_grace_days,
                to_credits(order.spent)
            ),
        };
        chronicle.push(
            tick,
            ChronicleKind::War,
            Some(order.polity),
            Some(world.system),
            format!(
                "Werft {}: Der Auftrag über {} {} verfällt, {reason}.",
                world.name,
                design.stats.class.label(),
                design.name
            ),
        );
    }
    if finished.is_empty() {
        return;
    }
    let mut delivered = false;
    for id in finished {
        let index = fleets
            .orders
            .iter()
            .position(|order| order.id == id)
            .expect("eben gelesen");
        let (polity, system) = {
            let order = &fleets.orders[index];
            (order.polity, galaxy.planet(order.planet).system)
        };
        // Zur Flotte mit der kleinsten Nummer, die einsatzbereit im System liegt. Eine
        // Flotte, die kämpft oder sich sammelt, nimmt kein Schiff auf: Es stünde sonst
        // unter dem Schutz ihrer Sammelfrist.
        let host = fleets
            .list
            .iter()
            .find(|fleet| {
                fleet.is_ready()
                    && fleet.owner == polity
                    && fleet.system == system
                    && fleet.route.is_empty()
            })
            .map(|fleet| fleet.id);
        let contested = fleets
            .docked_in(system)
            .any(|fleet| diplomacy.at_war(fleet.owner, polity));
        if host.is_none() && contested {
            // Das Schiff bleibt im Dock; der Auftrag steht fertig in der Liste.
            fleets.orders[index].status = BuildStatus::HeldInDock;
            continue;
        }
        let order = fleets.orders.remove(index);
        let world = galaxy.planet(order.planet);
        let ship = fleets.next_ship_id();
        let fleet = host.unwrap_or_else(|| fleets.new_fleet(polities, polity, system));
        delivered = true;
        let fleet = &mut fleets.list[fleet.index()];
        fleet.ships.push(Ship::from_yard(data, ship, order.design));
        let design = &data.designs[order.design as usize];
        chronicle.push(
            tick,
            ChronicleKind::War,
            Some(order.polity),
            Some(world.system),
            format!(
                "Werft {}: {} {} fertiggestellt und in die {} eingereiht.",
                world.name,
                design.stats.class.label(),
                design.name,
                fleet.name
            ),
        );
    }
    if delivered {
        fleets.refresh(data);
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::setup;
    use super::*;
    use crate::economy::setup_economy;
    use crate::ids::FleetId;

    /// Niemand führt Krieg.
    fn peace() -> Diplomacy {
        Diplomacy::default()
    }

    #[test]
    fn slots_follow_arms_capital_and_the_capital_always_has_one() {
        let (data, galaxy, polities, _) = setup();
        let mut economy = setup_economy(&data, &galaxy, &polities);
        let home = polities.list[0].capital.unwrap();
        let per_slot = data.balance.military.yard_capital_per_slot;
        let arms = data.recipe_index("rumpfwerk").unwrap();
        let mut fleets = Fleets::default();
        let slots = |economy: &Economy, fleets: &Fleets, planet| {
            yard_slots(&data, &galaxy, economy, &polities, fleets, planet)
        };

        // Die Hauptwelt baut auch ohne Rüstungsbetriebe, auf genau einem Platz.
        assert_eq!(arms_capital(&data, &economy, home), 0.0);
        assert_eq!(slots(&economy, &fleets, home), 1);
        economy.planets[home.index()].capital[arms] = per_slot * 3.5;
        assert_eq!(slots(&economy, &fleets, home), 3);

        // Eine fremde, unbesiedelte Welt hat keine Werft, auch nicht mit Kapital.
        let free = galaxy
            .planets
            .iter()
            .find(|planet| planet.owner.is_none())
            .unwrap()
            .id;
        economy.planets[free.index()].capital[arms] = per_slot * 2.0;
        update_yards(&data, &galaxy, &economy, &mut fleets);
        assert_eq!(slots(&economy, &fleets, free), 0);
        assert_eq!(slots(&economy, &fleets, PlanetId(99_999)), 0);

        // Nur Kapital im Zweig Rüstung zählt.
        let other = polities.list[1].capital.unwrap();
        let mine = data.recipe_index("erzabbau").unwrap();
        economy.planets[other.index()].capital[mine] = 100.0;
        update_yards(&data, &galaxy, &economy, &mut fleets);
        assert_eq!(slots(&economy, &fleets, other), 1);
    }

    #[test]
    fn a_colony_yard_needs_real_arms_capital_and_does_not_flicker() {
        let (data, mut galaxy, polities, mut fleets) = setup();
        let mut economy = setup_economy(&data, &galaxy, &polities);
        let military = &data.balance.military;
        let (open, close) = (military.yard_open_capital, military.yard_close_capital);
        assert!(
            close < open,
            "ohne Abstand gäbe es kein Halten an der Schwelle"
        );
        let arms = data.recipe_index("rumpfwerk").unwrap();
        let me = PolityId(0);
        let colony = galaxy
            .planets
            .iter()
            .find(|planet| planet.owner.is_none())
            .unwrap()
            .id;
        galaxy.planet_mut(colony).owner = Some(me);
        galaxy.planet_mut(colony).population = 50.0;
        let mut has_yard = |capital: f64, fleets: &mut Fleets| {
            economy.planets[colony.index()].capital[arms] = capital;
            update_yards(&data, &galaxy, &economy, fleets);
            assert_eq!(fleets.find_anomaly(&data), None);
            yard_slots(&data, &galaxy, &economy, &polities, fleets, colony) > 0
        };
        // Reste von Rüstungskapital, wie sie auf fast jeder Kolonie liegen und verfallen,
        // machen keine Werft: weder der kleinste Rest noch alles unter der Schwelle.
        for capital in [0.0, 0.05, 0.1, 0.45, open * 0.99] {
            assert!(!has_yard(capital, &mut fleets), "Kapital {capital}");
        }
        // Ab der Schwelle steht die Werft …
        assert!(has_yard(open, &mut fleets));
        assert_eq!(fleets.yards, vec![colony]);
        // … und bleibt, auch wenn das Kapital um die Schwelle schwankt: Erst unter dem
        // unteren Wert erlischt sie, und kommt dann erst über dem oberen zurück.
        for capital in [open * 0.9, open * 1.1, close, close * 1.01, open * 0.8] {
            assert!(has_yard(capital, &mut fleets), "Kapital {capital}");
        }
        assert!(!has_yard(close * 0.99, &mut fleets));
        for capital in [close, close * 1.5, open * 0.99] {
            assert!(!has_yard(capital, &mut fleets), "Kapital {capital}");
        }
        assert!(has_yard(open * 1.2, &mut fleets));
        // Eine Welt ohne Besitzer hat keine, wie viel Kapital dort auch steht.
        galaxy.planet_mut(colony).owner = None;
        update_yards(&data, &galaxy, &economy, &mut fleets);
        assert!(fleets.yards.is_empty());
    }

    #[test]
    fn only_designs_without_missing_technology_are_buildable() {
        let (data, _, _, _) = setup();
        let none: [bool; 0] = [];
        let cruiser = data.design_index("speer").unwrap();
        let advanced = data.design_index("speer_2").unwrap();
        let fighter = data.design_index("wespe").unwrap();
        assert!(design_buildable(&data, cruiser, &none).is_ok());
        assert!(
            design_buildable(&data, fighter, &none)
                .unwrap_err()
                .contains("Bordjäger")
        );
        assert!(design_buildable(&data, 9_999, &none).is_err());
        // Ohne die Technologie nicht, mit ihr schon.
        let error = design_buildable(&data, advanced, &none).unwrap_err();
        assert!(error.contains("Technologie"), "{error}");
        let needed = data.designs[advanced as usize].stats.techs.clone();
        assert!(!needed.is_empty());
        let mut known = vec![false; data.techs.len()];
        for tech in &needed[1..] {
            known[*tech as usize] = true;
        }
        assert!(design_buildable(&data, advanced, &known).is_err());
        known[needed[0] as usize] = true;
        assert!(design_buildable(&data, advanced, &known).is_ok());
    }

    #[test]
    fn orders_are_validated_and_the_queue_is_bounded() {
        let (data, galaxy, polities, mut fleets) = setup();
        let economy = setup_economy(&data, &galaxy, &polities);
        let me = PolityId(0);
        let home = polities.list[0].capital.unwrap();
        let theirs = polities.list[1].capital.unwrap();
        let corvette = data.design_index("dolch").unwrap();
        let order = |fleets: &mut Fleets, planet, design| {
            order_ship(
                &data,
                &galaxy,
                &economy,
                &polities,
                fleets,
                me,
                planet,
                design,
                &[],
                7,
            )
        };
        assert!(order(&mut fleets, PlanetId(99_999), corvette).is_err());
        assert!(order(&mut fleets, theirs, corvette).is_err());
        assert!(order(&mut fleets, home, data.design_index("wespe").unwrap()).is_err());
        assert!(order(&mut fleets, home, data.design_index("speer_2").unwrap()).is_err());
        assert!(fleets.orders.is_empty());

        let limit = data.balance.military.yard_queue;
        for expected in 0..limit {
            assert_eq!(order(&mut fleets, home, corvette), Ok(expected));
        }
        let full = order(&mut fleets, home, corvette).unwrap_err();
        assert!(full.contains("Aufträge"), "{full}");
        assert_eq!(fleets.orders.len(), limit as usize);
        assert_eq!(fleets.orders[0].started, 7);

        // Streichen: nur eigene, nur vorhandene; danach ist wieder Platz.
        assert!(cancel_order(&mut fleets, PolityId(1), 0).is_err());
        assert!(cancel_order(&mut fleets, me, 999).is_err());
        cancel_order(&mut fleets, me, 0).unwrap();
        assert!(cancel_order(&mut fleets, me, 0).is_err());
        assert_eq!(order(&mut fleets, home, corvette), Ok(limit));
        assert_eq!(fleets.find_anomaly(&data), None);
    }

    /// Füllt den Markt des Systems mit allem, was der Entwurf braucht.
    fn stock_parts(data: &GameData, economy: &mut Economy, galaxy: &Galaxy, planet: PlanetId) {
        let market = &mut economy.markets[galaxy.planet(planet).system.index()];
        for key in ["rumpfsegmente", "waffensysteme", "antriebe"] {
            market.stock[data.good_id(key).unwrap().index()] = 500.0;
        }
    }

    #[test]
    fn a_day_of_building_moves_goods_and_money_exactly() {
        let (data, galaxy, polities, mut fleets) = setup();
        let mut economy = setup_economy(&data, &galaxy, &polities);
        let mut chronicle = Chronicle::default();
        let me = PolityId(0);
        let home = polities.list[0].capital.unwrap();
        let system = galaxy.planet(home).system;
        stock_parts(&data, &mut economy, &galaxy, home);
        let cruiser = data.design_index("speer").unwrap();
        let stats = &data.designs[cruiser as usize].stats;
        order_ship(
            &data,
            &galaxy,
            &economy,
            &polities,
            &mut fleets,
            me,
            home,
            cruiser,
            &[],
            0,
        )
        .unwrap();

        let before = economy.clone();
        advance_yards(
            &galaxy,
            &mut economy,
            &mut fleets,
            &polities,
            &peace(),
            &mut chronicle,
            &data,
            0,
        );
        let order = &fleets.orders[0];
        let step = 1.0 / stats.build_days as f64;
        assert!((order.progress as f64 - step).abs() < 1e-7);
        assert!(!order.stalled());

        // Die Bauteile des Tages haben den Markt verlassen …
        let spread = data.balance.economy.spread;
        let mut expected_cost = 0.0;
        for (good, amount) in &stats.parts {
            let taken = before.markets[system.index()].stock[good.index()]
                - economy.markets[system.index()].stock[good.index()];
            assert!((taken - *amount as f64 * step).abs() < 1e-6, "{good}");
            expected_cost +=
                *amount as f64 * step * before.markets[system.index()].ask(*good, spread);
        }
        // … der Staat hat sie zum lokalen Preis bezahlt, das Kontor hat das Geld …
        let paid = before.treasury(me) - economy.treasury(me);
        let to_market = economy.markets[system.index()].cash - before.markets[system.index()].cash;
        let firm = |economy: &Economy| economy.planet(home).firms[Sector::Arms.index()].cash;
        let to_yard = firm(&economy) - firm(&before);
        assert!((to_credits(to_market) - expected_cost).abs() < 0.01);
        // … und die Werft ihren Aufschlag, als Erlös der Rüstungsfirma.
        let fee = data.balance.military.yard_fee;
        assert!(to_yard > 0);
        assert!((to_yard as f64 - to_market as f64 * fee).abs() <= 1.0);
        assert!(economy.planet(home).firms[Sector::Arms.index()].revenue_today > 0.0);
        // Auf das Tausendstel: Was der Staat zahlt, kommt bei Kontor und Werft an.
        assert_eq!(paid, to_market + to_yard);
        assert_eq!(order.spent, paid);
        assert_eq!(economy.money_in_accounts(), before.money_in_accounts());
        // Niemand sonst hat sich bewegt.
        assert_eq!(economy.treasury(PolityId(1)), before.treasury(PolityId(1)));
        // Der Bedarf ist als Nachfrage gemeldet.
        let hull = data.balance.military.hull_good_id;
        assert!(economy.markets[system.index()].demand_today[hull.index()] > 0.0);
    }

    #[test]
    fn a_ship_takes_its_build_days_and_joins_the_fleet_in_the_system() {
        let (data, galaxy, polities, mut fleets) = setup();
        let mut economy = setup_economy(&data, &galaxy, &polities);
        let mut chronicle = Chronicle::default();
        let me = PolityId(0);
        let home = polities.list[0].capital.unwrap();
        stock_parts(&data, &mut economy, &galaxy, home);
        let corvette = data.design_index("dolch").unwrap();
        let days = data.designs[corvette as usize].stats.build_days;
        let ships = fleets.list[0].ships.len();
        // Zwei Aufträge, ein Bauplatz: Der zweite wartet.
        for _ in 0..2 {
            order_ship(
                &data,
                &galaxy,
                &economy,
                &polities,
                &mut fleets,
                me,
                home,
                corvette,
                &[],
                0,
            )
            .unwrap();
        }
        let day = |fleets: &mut Fleets, economy: &mut Economy, chronicle: &mut Chronicle| {
            advance_yards(
                &galaxy,
                economy,
                fleets,
                &polities,
                &peace(),
                chronicle,
                &data,
                0,
            )
        };
        for _ in 0..days - 1 {
            day(&mut fleets, &mut economy, &mut chronicle);
        }
        assert_eq!(
            fleets.list[0].ships.len(),
            ships,
            "vor der Bauzeit kein Schiff"
        );
        assert_eq!(fleets.orders.len(), 2);
        assert_eq!(fleets.orders[1].progress, 0.0);
        assert!(!fleets.orders[1].stalled(), "warten ist nicht stocken");
        day(&mut fleets, &mut economy, &mut chronicle);
        assert_eq!(fleets.list[0].ships.len(), ships + 1);
        assert_eq!(fleets.orders.len(), 1);
        let built = fleets.list[0].ships.last().unwrap();
        assert_eq!(built.design, corvette);
        assert_eq!((built.hull, built.ammo, built.fuel), (1.0, 0.0, 0.0));
        let gain =
            data.designs[corvette as usize].stats.value * data.balance.military.strength_per_value;
        let start = data.balance.fleets.start_strength as f64;
        assert!((fleets.list[0].strength as f64 - start - gain).abs() < 1e-3);
        assert_eq!(chronicle.entries.len(), 1);

        // Liegt keine eigene Flotte im System, entsteht eine neue.
        let away = galaxy.neighbors(fleets.list[0].system)[0].system;
        fleets.list[0].system = away;
        for _ in 0..days {
            day(&mut fleets, &mut economy, &mut chronicle);
        }
        assert!(fleets.orders.is_empty());
        let new = fleets.list.last().unwrap();
        assert_eq!(fleets.list.len(), polities.list.len() + 1);
        assert_eq!((new.owner, new.ships.len()), (me, 1));
        assert_eq!(new.system, galaxy.planet(home).system);
        assert!(new.alive && new.strength > 0.0);
        assert_eq!(fleets.find_anomaly(&data), None);
        assert!(economy.find_anomaly().is_none());
    }

    #[test]
    fn a_new_ship_joins_the_lowest_numbered_fleet_that_is_not_fighting() {
        let (data, galaxy, polities, mut fleets) = setup();
        let mut economy = setup_economy(&data, &galaxy, &polities);
        let mut chronicle = Chronicle::default();
        let me = PolityId(0);
        let home = polities.list[0].capital.unwrap();
        stock_parts(&data, &mut economy, &galaxy, home);
        let corvette = data.design_index("dolch").unwrap();
        let days = data.designs[corvette as usize].stats.build_days;
        // Zwei eigene Flotten liegen im Heimatsystem.
        let detached = vec![fleets.list[0].ships[0].id];
        let second =
            super::super::split_fleet(&data, &polities, &mut fleets, me, FleetId(0), &detached)
                .unwrap();
        let build = |fleets: &mut Fleets, economy: &mut Economy, chronicle: &mut Chronicle| {
            order_ship(
                &data,
                &galaxy,
                economy,
                &polities,
                fleets,
                me,
                home,
                corvette,
                &[],
                0,
            )
            .unwrap();
            for _ in 0..days {
                advance_yards(
                    &galaxy,
                    economy,
                    fleets,
                    &polities,
                    &peace(),
                    chronicle,
                    &data,
                    0,
                );
            }
        };
        let sizes = |fleets: &Fleets| {
            (
                fleets.list[0].ships.len(),
                fleets.list[second.index()].ships.len(),
            )
        };
        let before = sizes(&fleets);
        build(&mut fleets, &mut economy, &mut chronicle);
        assert_eq!(
            sizes(&fleets),
            (before.0 + 1, before.1),
            "kleinste Nummer zuerst"
        );
        // Steht die erste im Gefecht oder hat sie Marschbefehl, geht das Schiff an die zweite.
        fleets.list[0].in_battle = true;
        build(&mut fleets, &mut economy, &mut chronicle);
        assert_eq!(sizes(&fleets), (before.0 + 1, before.1 + 1));
        fleets.list[0].in_battle = false;
        fleets.list[0].route = vec![galaxy.neighbors(fleets.list[0].system)[0].system];
        build(&mut fleets, &mut economy, &mut chronicle);
        assert_eq!(sizes(&fleets), (before.0 + 1, before.1 + 2));
        assert_eq!(
            fleets.list.len(),
            polities.list.len() + 1,
            "keine neue Flotte nötig"
        );
        assert_eq!(fleets.find_anomaly(&data), None);
    }

    #[test]
    fn a_finished_ship_stays_in_the_dock_while_the_enemy_holds_the_orbit() {
        let (data, galaxy, polities, mut fleets) = setup();
        let mut economy = setup_economy(&data, &galaxy, &polities);
        let mut chronicle = Chronicle::default();
        let me = PolityId(0);
        let home = polities.list[0].capital.unwrap();
        let system = galaxy.planet(home).system;
        stock_parts(&data, &mut economy, &galaxy, home);
        let corvette = data.design_index("dolch").unwrap();
        let days = data.designs[corvette as usize].stats.build_days;
        for _ in 0..2 {
            order_ship(
                &data,
                &galaxy,
                &economy,
                &polities,
                &mut fleets,
                me,
                home,
                corvette,
                &[],
                0,
            )
            .unwrap();
        }
        // Die eigene Flotte ist fort, eine feindliche liegt im System.
        let away = galaxy.neighbors(system)[0].system;
        fleets.list[0].system = away;
        fleets.list[1].system = system;
        let mut war = Diplomacy::default();
        war.wars.insert((me, PolityId(1)));
        let day = |fleets: &mut Fleets,
                   economy: &mut Economy,
                   chronicle: &mut Chronicle,
                   diplomacy: &Diplomacy| {
            advance_yards(
                &galaxy, economy, fleets, &polities, diplomacy, chronicle, &data, 0,
            );
        };
        for _ in 0..days + 5 {
            day(&mut fleets, &mut economy, &mut chronicle, &war);
        }
        // Das Schiff ist fertig, bleibt aber im Dock: keine neue Flotte, kein Eintrag.
        assert_eq!(fleets.list.len(), polities.list.len());
        assert_eq!(fleets.orders.len(), 2);
        assert!(fleets.orders[0].progress >= 1.0 - 1e-6 && !fleets.orders[0].stalled());
        assert!(chronicle.entries.is_empty());
        // Es belegt seinen Bauplatz, und es kostet nichts mehr.
        assert_eq!(fleets.orders[1].progress, 0.0);
        let (spent, cash) = (fleets.orders[0].spent, economy.treasury(me));
        day(&mut fleets, &mut economy, &mut chronicle, &war);
        assert_eq!(
            (fleets.orders[0].spent, economy.treasury(me)),
            (spent, cash)
        );
        assert_eq!(fleets.find_anomaly(&data), None);

        // Ohne Krieg wäre dieselbe Flotte kein Hindernis gewesen; mit dem Frieden (oder
        // dem Abzug des Gegners) läuft das Schiff aus und der nächste Bau beginnt.
        day(&mut fleets, &mut economy, &mut chronicle, &peace());
        assert_eq!(fleets.orders.len(), 1);
        assert_eq!(fleets.list.len(), polities.list.len() + 1);
        let new = fleets.list.last().unwrap();
        assert_eq!((new.owner, new.system, new.ships.len()), (me, system, 1));
        assert_eq!(chronicle.entries.len(), 1);
        day(&mut fleets, &mut economy, &mut chronicle, &war);
        assert!(fleets.orders[0].progress > 0.0);

        // Liegt eine eigene Flotte im System, die nicht kämpft, nimmt sie das nächste Schiff
        // auch unter den Augen des Gegners auf.
        for _ in 0..days {
            day(&mut fleets, &mut economy, &mut chronicle, &war);
        }
        assert!(fleets.orders.is_empty());
        assert_eq!(fleets.list.last().unwrap().ships.len(), 2);
        assert_eq!(fleets.find_anomaly(&data), None);
    }

    #[test]
    fn the_scarcest_input_sets_the_pace_and_marks_the_order_stalled() {
        let (data, galaxy, polities, mut fleets) = setup();
        let mut economy = setup_economy(&data, &galaxy, &polities);
        let mut chronicle = Chronicle::default();
        let me = PolityId(0);
        let home = polities.list[0].capital.unwrap();
        let system = galaxy.planet(home).system;
        let corvette = data.design_index("dolch").unwrap();
        let stats = &data.designs[corvette as usize].stats;
        let step = 1.0 / stats.build_days as f32;
        order_ship(
            &data,
            &galaxy,
            &economy,
            &polities,
            &mut fleets,
            me,
            home,
            corvette,
            &[],
            0,
        )
        .unwrap();
        let day = |fleets: &mut Fleets, economy: &mut Economy, chronicle: &mut Chronicle| {
            advance_yards(
                &galaxy,
                economy,
                fleets,
                &polities,
                &peace(),
                chronicle,
                &data,
                0,
            );
        };

        // Leerer Markt: kein Fortschritt, kein Geld, aber gemeldete Nachfrage.
        let before = economy.clone();
        day(&mut fleets, &mut economy, &mut chronicle);
        assert_eq!(
            (fleets.orders[0].progress, fleets.orders[0].spent),
            (0.0, 0)
        );
        assert!(fleets.orders[0].stalled());
        assert_eq!(economy.treasury(me), before.treasury(me));
        for (good, _) in &stats.parts {
            assert!(economy.markets[system.index()].demand_today[good.index()] > 0.0);
        }

        // Ein Bauteil reicht nur für ein Viertel des Tages: Alle Teile werden nur zu einem
        // Viertel gekauft, der Bau rückt um ein Viertel vor.
        stock_parts(&data, &mut economy, &galaxy, home);
        let (scarce, amount) = stats.parts[0];
        let need = amount as f64 * step as f64;
        economy.markets[system.index()].stock[scarce.index()] =
            0.25 * need / crate::economy::BUYER_SHARE;
        let before = economy.clone();
        day(&mut fleets, &mut economy, &mut chronicle);
        assert!((fleets.orders[0].progress - 0.25 * step).abs() < 1e-6);
        assert!(fleets.orders[0].stalled());
        for (good, amount) in &stats.parts {
            let taken = before.markets[system.index()].stock[good.index()]
                - economy.markets[system.index()].stock[good.index()];
            assert!((taken - 0.25 * *amount as f64 * step as f64).abs() < 1e-6);
        }

        // Eine Kasse, die nur für den halben Tag reicht, bremst genauso; dabei bleibt nichts
        // im Minus und der Aufschlag der Werft ist mitbezahlt.
        stock_parts(&data, &mut economy, &galaxy, home);
        let spread = data.balance.economy.spread;
        let cost: f64 = stats
            .parts
            .iter()
            .map(|(good, amount)| {
                *amount as f64 * step as f64 * economy.markets[system.index()].ask(*good, spread)
            })
            .sum();
        economy.treasuries[0] = to_money(0.5 * cost * (1.0 + data.balance.military.yard_fee));
        let progress = fleets.orders[0].progress;
        day(&mut fleets, &mut economy, &mut chronicle);
        assert!((fleets.orders[0].progress - progress - 0.5 * step).abs() < 1e-4);
        assert!(fleets.orders[0].stalled());
        assert!(economy.treasury(me) >= 0 && economy.treasury(me) <= 2);
        assert!(economy.planet(home).firms[Sector::Arms.index()].cash > 0);
        assert!(economy.find_anomaly().is_none());

        // Leere Kasse: nichts, und auch keine Nachfrage mehr, die die Preise triebe.
        economy.treasuries[0] = 0;
        let hull = data.balance.military.hull_good_id;
        let demand = economy.markets[system.index()].demand_today[hull.index()];
        let progress = fleets.orders[0].progress;
        day(&mut fleets, &mut economy, &mut chronicle);
        assert_eq!(fleets.orders[0].progress, progress);
        assert!(fleets.orders[0].stalled());
        assert_eq!(
            economy.markets[system.index()].demand_today[hull.index()],
            demand
        );
    }

    /// Eine Kolonie der Gemeinschaft 0 mit so viel Rüstungskapital, wie eine Werft braucht,
    /// in einem System, das sonst niemand besiedelt hat.
    fn colony_yard(
        data: &GameData,
        galaxy: &mut Galaxy,
        economy: &mut Economy,
        fleets: &mut Fleets,
    ) -> (PlanetId, usize) {
        let colony = galaxy
            .planets
            .iter()
            .find(|planet| galaxy.system_owner(planet.system).is_none())
            .unwrap()
            .id;
        galaxy.planet_mut(colony).owner = Some(PolityId(0));
        galaxy.planet_mut(colony).population = 50.0;
        let arms = data.recipe_index("rumpfwerk").unwrap();
        economy.planets[colony.index()].capital[arms] = data.balance.military.yard_open_capital;
        update_yards(data, galaxy, economy, fleets);
        (colony, arms)
    }

    #[test]
    fn an_order_rests_while_its_yard_is_gone_and_lapses_after_the_grace_period() {
        let (data, mut galaxy, polities, mut fleets) = setup();
        let mut economy = setup_economy(&data, &galaxy, &polities);
        let mut chronicle = Chronicle::default();
        let me = PolityId(0);
        let military = &data.balance.military;
        let grace = military.yard_grace_days;
        let (colony, arms) = colony_yard(&data, &mut galaxy, &mut economy, &mut fleets);
        let system = galaxy.planet(colony).system;
        stock_parts(&data, &mut economy, &galaxy, colony);
        let corvette = data.design_index("dolch").unwrap();
        let order = |fleets: &mut Fleets, economy: &Economy| {
            order_ship(
                &data,
                &galaxy,
                economy,
                &polities,
                fleets,
                me,
                colony,
                corvette,
                &[],
                0,
            )
        };
        order(&mut fleets, &economy).unwrap();
        order(&mut fleets, &economy).unwrap();
        let day =
            |fleets: &mut Fleets, economy: &mut Economy, chronicle: &mut Chronicle, day: u64| {
                update_yards(&data, &galaxy, economy, fleets);
                advance_yards(
                    &galaxy,
                    economy,
                    fleets,
                    &polities,
                    &peace(),
                    chronicle,
                    &data,
                    day * TICKS_PER_DAY,
                );
            };
        for today in 0..10 {
            day(&mut fleets, &mut economy, &mut chronicle, today);
        }
        assert_eq!(fleets.orders[0].status, BuildStatus::Building);
        assert_eq!(fleets.orders[1].status, BuildStatus::Queued);
        let (progress, spent) = (fleets.orders[0].progress, fleets.orders[0].spent);
        assert!(progress > 0.1 && spent > 0);

        // Die Rüstungsbetriebe der Kolonie verfallen: Die Werft erlischt. Beide Aufträge
        // ruhen ab diesem Tag, sichtbar und mit Grund; nichts wird gekauft, nichts
        // nachgefragt, und was verbaut ist, bleibt, wie es war.
        economy.planets[colony.index()].capital[arms] = military.yard_close_capital * 0.5;
        let before = economy.clone();
        let hull = military.hull_good_id;
        let demand = economy.markets[system.index()].demand_today[hull.index()];
        day(&mut fleets, &mut economy, &mut chronicle, 10);
        assert_eq!(
            yard_slots(&data, &galaxy, &economy, &polities, &fleets, colony),
            0
        );
        for entry in &fleets.orders {
            assert_eq!(entry.status, BuildStatus::NoYard { since: 10 });
            assert!(!entry.stalled(), "ruhen ist nicht stocken");
            let text = entry.status_text(&data);
            assert!(
                text.contains("ruht seit dem 11.01.2400") && text.contains("keine Werft mehr"),
                "{text}"
            );
        }
        assert_eq!(
            (fleets.orders[0].progress, fleets.orders[0].spent),
            (progress, spent)
        );
        assert_eq!(economy.treasury(me), before.treasury(me));
        assert_eq!(
            economy.markets[system.index()].demand_today[hull.index()],
            demand
        );
        assert_eq!(fleets.orders_at(colony).count(), 2);
        assert!(chronicle.entries.is_empty());
        // Neue Aufträge nimmt die Welt nicht an; streichen lässt sich der alte weiterhin.
        let error = order(&mut fleets, &economy).unwrap_err();
        assert!(error.contains("hat keine Werft"), "{error}");

        // Kehrt die Werft innerhalb der Frist zurück, geht der Bau weiter, als wäre nichts
        // gewesen: Fortschritt und Bezahltes sind erhalten.
        let mut resumed = (fleets.clone(), economy.clone(), Chronicle::default());
        for today in 11..grace / 2 {
            day(&mut resumed.0, &mut resumed.1, &mut resumed.2, today);
        }
        assert_eq!(
            resumed.0.orders[0].status,
            BuildStatus::NoYard { since: 10 }
        );
        resumed.1.planets[colony.index()].capital[arms] = military.yard_open_capital;
        day(&mut resumed.0, &mut resumed.1, &mut resumed.2, grace / 2);
        assert_eq!(resumed.0.orders[0].status, BuildStatus::Building);
        assert_eq!(resumed.0.orders[1].status, BuildStatus::Queued);
        assert!(resumed.0.orders[0].progress > progress && resumed.0.orders[0].spent > spent);
        assert!(resumed.2.entries.is_empty());

        // Bleibt sie aus, verfällt der Auftrag nach der Frist wie ein gestrichener: Die
        // Chronik meldet es, erstattet wird nichts.
        let cash = economy.treasury(me);
        for today in 11..10 + grace {
            day(&mut fleets, &mut economy, &mut chronicle, today);
            assert_eq!(fleets.orders.len(), 2, "Tag {today}");
        }
        assert!(chronicle.entries.is_empty());
        day(&mut fleets, &mut economy, &mut chronicle, 10 + grace);
        assert!(fleets.orders.is_empty());
        assert_eq!(chronicle.entries.len(), 2);
        let text = &chronicle.entries[0].text;
        assert!(
            text.contains("verfällt") && text.contains("keine Werft mehr"),
            "{text}"
        );
        assert!(
            text.contains(&format!("{:.0} Credits", to_credits(spent))),
            "{text}"
        );
        assert_eq!(chronicle.entries[0].polity, Some(me));
        assert_eq!(economy.treasury(me), cash, "keine Erstattung");
        assert_eq!(economy.money_in_accounts(), before.money_in_accounts());
        assert_eq!(fleets.find_anomaly(&data), None);
    }

    #[test]
    fn a_finished_ship_leaves_the_dock_even_after_its_yard_is_gone() {
        let (data, mut galaxy, polities, mut fleets) = setup();
        let mut economy = setup_economy(&data, &galaxy, &polities);
        let mut chronicle = Chronicle::default();
        let me = PolityId(0);
        let (colony, arms) = colony_yard(&data, &mut galaxy, &mut economy, &mut fleets);
        let system = galaxy.planet(colony).system;
        stock_parts(&data, &mut economy, &galaxy, colony);
        let corvette = data.design_index("dolch").unwrap();
        let days = data.designs[corvette as usize].stats.build_days;
        order_ship(
            &data,
            &galaxy,
            &economy,
            &polities,
            &mut fleets,
            me,
            colony,
            corvette,
            &[],
            0,
        )
        .unwrap();
        // Eine feindliche Flotte liegt im System: Das fertige Schiff bleibt im Dock.
        fleets.list[1].system = system;
        let mut war = Diplomacy::default();
        war.wars.insert((me, PolityId(1)));
        for _ in 0..days {
            advance_yards(
                &galaxy,
                &mut economy,
                &mut fleets,
                &polities,
                &war,
                &mut chronicle,
                &data,
                0,
            );
        }
        assert_eq!(fleets.orders[0].status, BuildStatus::HeldInDock);
        assert!(
            fleets.orders[0]
                .status_text(&data)
                .contains("bleibt im Dock")
        );
        // Inzwischen erlischt die Werft. Das Schiff ist fertig: Es ruht nicht und verfällt
        // nicht, und sobald der Orbit frei ist, läuft es aus.
        economy.planets[colony.index()].capital[arms] = 0.0;
        update_yards(&data, &galaxy, &economy, &mut fleets);
        for day in 0..data.balance.military.yard_grace_days + 5 {
            advance_yards(
                &galaxy,
                &mut economy,
                &mut fleets,
                &polities,
                &war,
                &mut chronicle,
                &data,
                day * TICKS_PER_DAY,
            );
        }
        assert_eq!(fleets.orders.len(), 1);
        assert_eq!(fleets.orders[0].status, BuildStatus::HeldInDock);
        assert!(chronicle.entries.is_empty());
        advance_yards(
            &galaxy,
            &mut economy,
            &mut fleets,
            &polities,
            &peace(),
            &mut chronicle,
            &data,
            0,
        );
        assert!(fleets.orders.is_empty());
        let new = fleets.list.last().unwrap();
        assert_eq!((new.owner, new.system, new.ships.len()), (me, system, 1));
        assert!(chronicle.entries[0].text.contains("fertiggestellt"));
    }

    #[test]
    fn a_stalled_order_says_what_it_is_waiting_for() {
        let (data, galaxy, polities, mut fleets) = setup();
        let mut economy = setup_economy(&data, &galaxy, &polities);
        let mut chronicle = Chronicle::default();
        let me = PolityId(0);
        let home = polities.list[0].capital.unwrap();
        let system = galaxy.planet(home).system;
        let corvette = data.design_index("dolch").unwrap();
        order_ship(
            &data,
            &galaxy,
            &economy,
            &polities,
            &mut fleets,
            me,
            home,
            corvette,
            &[],
            0,
        )
        .unwrap();
        assert_eq!(fleets.orders[0].status, BuildStatus::Queued);
        let mut day = |fleets: &mut Fleets, economy: &mut Economy| {
            advance_yards(
                &galaxy,
                economy,
                fleets,
                &polities,
                &peace(),
                &mut chronicle,
                &data,
                0,
            );
            fleets.orders[0].status
        };
        // Alles da bis auf die Antriebe: Der Auftrag nennt das fehlende Bauteil.
        stock_parts(&data, &mut economy, &galaxy, home);
        let drives = data.good_id("antriebe").unwrap();
        economy.markets[system.index()].stock[drives.index()] = 0.0;
        assert_eq!(
            day(&mut fleets, &mut economy),
            BuildStatus::MissingPart(drives)
        );
        assert!(fleets.orders[0].stalled());
        let text = fleets.orders[0].status_text(&data);
        assert!(
            text.contains("stockt") && text.contains("Antriebe"),
            "{text}"
        );
        // Ware da, Kasse leer: Es fehlt das Geld.
        stock_parts(&data, &mut economy, &galaxy, home);
        let cash = std::mem::take(&mut economy.treasuries[0]);
        assert_eq!(day(&mut fleets, &mut economy), BuildStatus::MissingMoney);
        assert!(fleets.orders[0].status_text(&data).contains("Geld"));
        // Beides da: Es wird gebaut.
        economy.treasuries[0] = cash;
        assert_eq!(day(&mut fleets, &mut economy), BuildStatus::Building);
        assert!(!fleets.orders[0].stalled());
        assert_eq!(fleets.orders[0].status_text(&data), "im Bau");
    }

    #[test]
    fn a_yard_in_a_system_held_by_others_builds_and_its_ships_are_supplied_there() {
        let (data, mut galaxy, polities, mut fleets) = setup();
        let mut economy = setup_economy(&data, &galaxy, &polities);
        let mut chronicle = Chronicle::default();
        let me = PolityId(0);
        let military = &data.balance.military;
        // Eine kleine eigene Welt im Heimatsystem einer anderen Gemeinschaft: Das System
        // prägt deren Mehrheit, die Werft gehört trotzdem der eigenen.
        let (host, world) = polities
            .list
            .iter()
            .skip(1)
            .find_map(|polity| {
                let system = galaxy.planet(polity.capital?).system;
                galaxy
                    .planets_in(system)
                    .find(|planet| planet.owner.is_none())
                    .map(|planet| (polity.id, planet.id))
            })
            .expect("ein Heimatsystem mit einer freien Welt");
        let system = galaxy.planet(world).system;
        galaxy.planet_mut(world).owner = Some(me);
        galaxy.planet_mut(world).population = 20.0;
        assert_eq!(galaxy.system_owner(system), Some(host));
        let arms = data.recipe_index("rumpfwerk").unwrap();
        economy.planets[world.index()].capital[arms] = military.yard_open_capital;
        update_yards(&data, &galaxy, &economy, &mut fleets);
        stock_parts(&data, &mut economy, &galaxy, world);
        let mut war = Diplomacy::default();
        war.wars.insert((me, host));
        // Eingekauft wird, wo die Gemeinschaft eine Welt hat; nicht in fremden Systemen
        // und nicht beim Kriegsgegner.
        assert!(supply_base(&galaxy, &peace(), me, system));
        assert!(!supply_base(&galaxy, &war, me, system));
        assert!(supply_base(&galaxy, &war, host, system));
        let elsewhere = galaxy.planet(polities.list[2].capital.unwrap()).system;
        assert!(elsewhere != system && !supply_base(&galaxy, &peace(), me, elsewhere));

        let corvette = data.design_index("dolch").unwrap();
        let days = data.designs[corvette as usize].stats.build_days;
        order_ship(
            &data,
            &galaxy,
            &economy,
            &polities,
            &mut fleets,
            me,
            world,
            corvette,
            &[],
            0,
        )
        .unwrap();
        // Im Krieg mit der Mehrheit verkauft deren Kontor der Werft nichts: Der Auftrag
        // stockt und sagt, warum.
        let cash = economy.treasury(me);
        advance_yards(
            &galaxy,
            &mut economy,
            &mut fleets,
            &polities,
            &war,
            &mut chronicle,
            &data,
            0,
        );
        assert_eq!(fleets.orders[0].status, BuildStatus::MarketClosed);
        assert!(fleets.orders[0].stalled() && fleets.orders[0].progress == 0.0);
        assert!(fleets.orders[0].status_text(&data).contains("Kriegsgegner"));
        assert_eq!(economy.treasury(me), cash);
        // Im Frieden baut sie wie jede andere, …
        for _ in 0..days {
            advance_yards(
                &galaxy,
                &mut economy,
                &mut fleets,
                &polities,
                &peace(),
                &mut chronicle,
                &data,
                0,
            );
        }
        assert!(fleets.orders.is_empty());
        let built = fleets.list.last().unwrap().clone();
        assert_eq!((built.owner, built.system), (me, system));
        assert_eq!((built.ships[0].ammo, built.ships[0].fuel), (0.0, 0.0));
        // … und das Schiff fasst dort Munition und Treibstoff, wo es gebaut wurde: am
        // selben Markt, von dem seine Bauteile kamen.
        let ammo = military.ammo_good_id;
        economy.markets[system.index()].stock[ammo.index()] = 100.0;
        let index = built.id.index();
        crate::fleet::resupply_fleets(&galaxy, &mut economy, &mut fleets, &peace(), &data);
        let ship = &fleets.list[index].ships[0];
        assert!(ship.ammo > 0.0 && ship.fuel > 0.0, "{ship:?}");
        // Beschädigt wird es an seiner Werft auch instand gesetzt.
        fleets.list[index].ships[0].hull = 0.5;
        fleets.refresh(&data);
        crate::fleet::repair_fleets(
            &galaxy,
            &mut economy,
            &mut fleets,
            &polities,
            &peace(),
            &data,
        );
        assert!(fleets.list[index].ships[0].hull > 0.5);
        // Im Krieg mit der Mehrheit gibt es dort weder Nachschub noch Instandsetzung.
        let frozen = fleets.list[index].ships[0].clone();
        crate::fleet::resupply_fleets(&galaxy, &mut economy, &mut fleets, &war, &data);
        crate::fleet::repair_fleets(&galaxy, &mut economy, &mut fleets, &polities, &war, &data);
        assert_eq!(fleets.list[index].ships[0], frozen);
        assert_eq!(fleets.find_anomaly(&data), None);
    }

    #[test]
    fn an_order_lapses_when_the_world_changes_hands() {
        let (data, mut galaxy, polities, mut fleets) = setup();
        let mut economy = setup_economy(&data, &galaxy, &polities);
        let mut chronicle = Chronicle::default();
        let home = polities.list[0].capital.unwrap();
        let corvette = data.design_index("dolch").unwrap();
        order_ship(
            &data,
            &galaxy,
            &economy,
            &polities,
            &mut fleets,
            PolityId(0),
            home,
            corvette,
            &[],
            0,
        )
        .unwrap();
        galaxy.planet_mut(home).owner = Some(PolityId(1));
        advance_yards(
            &galaxy,
            &mut economy,
            &mut fleets,
            &polities,
            &peace(),
            &mut chronicle,
            &data,
            0,
        );
        assert!(fleets.orders.is_empty());
        assert!(chronicle.entries[0].text.contains("verfällt"));
    }
}

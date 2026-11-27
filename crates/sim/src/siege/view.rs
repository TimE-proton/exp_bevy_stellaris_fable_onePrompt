//! Lesemodell der Belagerung für Oberfläche, Werkzeuge und KI: je Belagerung eine Karte
//! mit Parteien, Phase, Fortschritt, Vorräten, Stärken und den drei vorausgerechneten
//! Wegen; je Welt der Verteidigungsplan mit seinen Kosten; und zu jedem Befehl der
//! Belagerung der Grund, aus dem der Kern ihn jetzt ablehnen würde.
//!
//! Alles hier liest nur. Die Gründe stammen aus denselben Funktionen, die die Befehle
//! ausführen ([`super::orders`], [`super::arms`]): Was hier möglich heißt, nimmt der Kern an.

use super::arms::{Works, level_caps, planet_upkeep, sets_missing, shortfall};
use super::orbit::{closes, presence};
use super::orders::check_embark;
use super::{
    Course, DefensePlan, Forecast, Orbit, Outcome, Phase, PlanetDefense, SiegeView, Sieges,
    begin_siege, capitulate, firm_value, forecast, guns_active, land_troops, lift_siege,
    occupation_force, planet_facts, set_defense_plan, set_garrison_orders, set_siege_orders,
};
use crate::chronicle::Chronicle;
use crate::command::SimCommand;
use crate::data::GameData;
use crate::diplomacy::Diplomacy;
use crate::economy::Economy;
use crate::finance::Finance;
use crate::fleet::Fleets;
use crate::galaxy::Galaxy;
use crate::ids::{FleetId, PlanetId, PolityId, SystemId};
use crate::logistics::Logistics;
use crate::polity::Polities;
use crate::state::SimState;
use crate::tech::Research;
use bevy_ecs::prelude::World;

/// Alles, was das Lesemodell liest.
#[derive(Clone, Copy)]
pub struct SiegeRefs<'a> {
    pub data: &'a GameData,
    pub galaxy: &'a Galaxy,
    pub polities: &'a Polities,
    pub diplomacy: &'a Diplomacy,
    pub economy: &'a Economy,
    pub finance: &'a Finance,
    pub logistics: &'a Logistics,
    pub fleets: &'a Fleets,
    pub research: &'a Research,
    pub sieges: &'a Sieges,
}

impl<'a> SiegeRefs<'a> {
    pub fn of_state(state: &'a SimState, data: &'a GameData) -> Self {
        Self {
            data,
            galaxy: &state.galaxy,
            polities: &state.polities,
            diplomacy: &state.diplomacy,
            economy: &state.economy,
            finance: &state.finance,
            logistics: &state.logistics,
            fleets: &state.fleets,
            research: &state.research,
            sieges: &state.sieges,
        }
    }

    pub fn of_world(world: &'a World) -> Self {
        Self {
            data: world.resource::<GameData>(),
            galaxy: world.resource::<Galaxy>(),
            polities: world.resource::<Polities>(),
            diplomacy: world.resource::<Diplomacy>(),
            economy: world.resource::<Economy>(),
            finance: world.resource::<Finance>(),
            logistics: world.resource::<Logistics>(),
            fleets: world.resource::<Fleets>(),
            research: world.resource::<Research>(),
            sieges: world.resource::<Sieges>(),
        }
    }
}

/// Anlagen der belagerten Welt.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DefenseStrength {
    pub stations: f64,
    pub guns: f64,
    /// Feuern die Geschütze, sobald Schiffe in den niedrigen Orbit kommen?
    pub guns_active: bool,
    pub shield: u8,
    pub charge: f64,
    pub bunkers: u8,
}

/// Truppen beider Seiten in Verbänden.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TroopStrength {
    /// Landungstruppe am Boden und Truppen an Bord der belagernden Flotten.
    pub army: f64,
    pub embarked: f64,
    /// Verteidiger: Miliz der Zonen, Reserve an der Oberfläche, Reguläre im Untergrund.
    pub militia: f64,
    pub reserve: f64,
    pub deep: f64,
    /// Garnison bei Beginn der Belagerung.
    pub garrison_start: f64,
    /// Besatzungstruppe, die eine Übergabe annehmen kann.
    pub occupation_force: f64,
}

/// Eine laufende Belagerung, wie eine Oberfläche sie zeigt.
#[derive(Clone, Debug, PartialEq)]
pub struct SiegeCard {
    pub planet: PlanetId,
    pub planet_name: String,
    pub system: SystemId,
    pub attacker: PolityId,
    pub defender: PolityId,
    pub phase: Phase,
    /// Nummer der Phase, 1 bis 5, und ihr Name.
    pub phase_number: u8,
    pub phase_label: &'static str,
    /// Tage seit Beginn.
    pub days: u64,
    /// Hält der Belagerer heute den Orbit? Sonst ruht die Belagerung.
    pub control: bool,
    /// Ist das System durch sie gesperrt?
    pub blockaded: bool,
    /// Der Besitzwechsel steht aus: wie die Welt fällt.
    pub handover: Option<Outcome>,
    /// Fortschritt 0..1: der Anteil der Garnison vom Beginn, der nicht mehr steht, oder
    /// der Anteil der genommenen Zonen, je nachdem, was weiter ist.
    pub progress: f64,
    /// Vorräte der Garnison in Tagen und Nachschub der Landungstruppe in Tagen.
    pub stores_days: f64,
    pub army_supply_days: f64,
    pub defense: DefenseStrength,
    pub troops: TroopStrength,
    /// Belagernde Flotten, ihre Feuerkraft, Füllung der Magazine und der Tanks (0..1).
    pub fleets: Vec<FleetId>,
    pub firepower: f64,
    pub ammo: f64,
    pub fuel: f64,
    /// Die drei Wege, vorausgerechnet mit dem Tagesrechner.
    pub forecasts: [(Course, Forecast); 3],
    /// Die Lage, auf der die Prognose rechnet.
    pub view: SiegeView,
}

/// Die Lage einer laufenden Belagerung für den Tagesrechner und seine Prognose, aus der
/// Welt gelesen: der Datensatz, die Verteidigung, der Planet am nächsten Tag und der
/// Orbit, wie ihn auch der Tagesschritt sieht. Ruht die Belagerung, weil ihre Flotten im
/// Gefecht stehen, nennt die Lage sie trotzdem, ohne Orbitalhoheit ([`presence`]).
pub fn siege_view(refs: &SiegeRefs, planet: PlanetId, day: u64) -> Option<SiegeView> {
    let siege = refs.sieges.siege(planet)?;
    let system = refs.galaxy.planet(planet).system;
    let squadron = presence(
        refs.fleets,
        refs.research,
        refs.data,
        refs.diplomacy,
        refs.sieges,
        planet,
        system,
        siege.attacker,
        siege.defender,
    );
    Some(SiegeView {
        planet,
        siege: siege.clone(),
        defense: refs.sieges.defense(planet).clone(),
        facts: planet_facts(
            refs.data,
            refs.galaxy,
            refs.economy,
            refs.finance,
            planet,
            day + 1,
            // Den Markt vor Ort erreicht die Garnison nur in einem System, das die
            // Einschließung mit sperrt.
            if closes(refs.galaxy, refs.diplomacy, system, siege.attacker) {
                1.0
            } else {
                0.0
            },
        ),
        orbit: squadron.view,
        toughness: squadron.toughness,
        fuel: squadron.fuel,
        supply_aboard: squadron.supply_aboard,
        firm_value: firm_value(refs.data, refs.galaxy, refs.economy, refs.logistics, planet),
    })
}

/// Die Karte der Belagerung eines Planeten am Tag `day`; `None`, wenn er nicht belagert wird.
pub fn siege_card(refs: &SiegeRefs, planet: PlanetId, day: u64) -> Option<SiegeCard> {
    let balance = &refs.data.balance.siege;
    let view = siege_view(refs, planet, day)?;
    let (siege, defense) = (&view.siege, &view.defense);
    let target = refs.galaxy.planet(planet);
    let squad = presence(
        refs.fleets,
        refs.research,
        refs.data,
        refs.diplomacy,
        refs.sieges,
        planet,
        target.system,
        siege.attacker,
        siege.defender,
    );
    let phase = siege.phase(defense);
    let standing = siege.defenders(defense);
    let fallen = if siege.garrison_start > 0.0 {
        (1.0 - standing / siege.garrison_start).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let taken: f64 = siege
        .zones
        .iter()
        .filter(|zone| zone.taken)
        .map(|zone| zone.share as f64)
        .sum();
    let forecasts = Course::ALL.map(|course| (course, forecast(&view, course, balance)));
    Some(SiegeCard {
        planet,
        planet_name: target.name.clone(),
        system: target.system,
        attacker: siege.attacker,
        defender: siege.defender,
        phase,
        phase_number: phase.number(),
        phase_label: phase.label(),
        days: day.saturating_sub(siege.started_day),
        control: view.orbit.control,
        blockaded: refs.fleets.is_blockaded(target.system),
        handover: refs
            .sieges
            .pending
            .iter()
            .find(|due| due.planet == planet)
            .map(|due| due.outcome),
        progress: fallen.max(taken.clamp(0.0, 1.0)),
        stores_days: defense.depot,
        army_supply_days: if siege.army > 0.0 {
            siege.army_supply / siege.army
        } else {
            0.0
        },
        defense: DefenseStrength {
            stations: defense.stations as f64,
            guns: defense.guns as f64,
            guns_active: guns_active(siege, defense),
            shield: defense.shield,
            charge: defense.charge as f64,
            bunkers: defense.bunkers,
        },
        troops: TroopStrength {
            army: siege.army,
            embarked: view.orbit.embarked,
            militia: siege.zone_militia(),
            reserve: siege.reserve(defense),
            deep: siege.deep,
            garrison_start: siege.garrison_start,
            occupation_force: occupation_force(target.population, balance),
        },
        fleets: squad.fleets,
        firepower: view.orbit.firepower,
        ammo: view.orbit.ammo,
        fuel: view.fuel,
        forecasts,
        view,
    })
}

/// Alle laufenden Belagerungen in der Reihenfolge der Planeten.
pub fn siege_cards(refs: &SiegeRefs, day: u64) -> Vec<SiegeCard> {
    refs.sieges
        .active
        .keys()
        .filter_map(|planet| siege_card(refs, *planet, day))
        .collect()
}

/// Der Grund, aus dem der Kern einen Befehl der Belagerung oder der Verteidigung jetzt
/// ablehnen würde, in seinen eigenen Worten; `None`, wenn er ihn annähme oder wenn der
/// Befehl nicht zur Belagerung gehört. Für gesperrte Schaltflächen.
///
/// Der Befehl wird dazu an einer Kopie von Belagerungen und Flotten ausgeführt: nichts
/// für jeden Frame, wohl aber für jede Auswahl und jeden Tag.
pub fn command_block(
    refs: &SiegeRefs,
    polity: PolityId,
    command: &SimCommand,
    tick: u64,
) -> Option<String> {
    let mut sieges = refs.sieges.clone();
    let mut fleets = refs.fleets.clone();
    let orbit = Orbit::new(&mut fleets, refs.research);
    let (data, galaxy, polities, diplomacy) =
        (refs.data, refs.galaxy, refs.polities, refs.diplomacy);
    // Senken für das, was die Befehle außerhalb der Belagerung verbuchen.
    let (mut finance, mut chronicle) = (Finance::default(), Chronicle::default());
    let result = match command {
        SimCommand::BeginSiege { fleet, planet } => begin_siege(
            galaxy,
            polities,
            diplomacy,
            &mut sieges,
            &orbit,
            polity,
            *fleet,
            *planet,
        ),
        SimCommand::SetSiegeOrders { planet, orders } => {
            set_siege_orders(galaxy, &mut sieges, polity, *planet, *orders)
        }
        SimCommand::LiftSiege { planet } => lift_siege(
            data,
            galaxy,
            polities,
            diplomacy,
            &mut finance,
            &mut sieges,
            &orbit,
            &mut chronicle,
            polity,
            *planet,
            tick,
        ),
        SimCommand::SetGarrisonOrders {
            planet,
            posture,
            hold_fire,
        } => set_garrison_orders(
            data,
            galaxy,
            &mut sieges,
            polity,
            *planet,
            *posture,
            *hold_fire,
        ),
        SimCommand::EmbarkTroops {
            fleet,
            planet,
            strength,
            supply_days,
        } => check_embark(
            data,
            galaxy,
            diplomacy,
            &sieges,
            &orbit,
            polity,
            *fleet,
            *planet,
            *strength,
            *supply_days,
        )
        .map(|_| ()),
        SimCommand::LandTroops {
            fleet,
            planet,
            strength,
        } => land_troops(
            data,
            galaxy,
            polities,
            diplomacy,
            &mut finance,
            &mut sieges,
            &orbit,
            polity,
            *fleet,
            *planet,
            *strength,
            tick,
        ),
        SimCommand::Capitulate { planet } => capitulate(
            data,
            galaxy,
            diplomacy,
            &mut sieges,
            &orbit,
            polity,
            *planet,
        ),
        SimCommand::SetDefensePlan { planet, plan } => set_defense_plan(
            data,
            galaxy,
            refs.research,
            &mut sieges,
            polity,
            *planet,
            plan,
        ),
        _ => Ok(()),
    };
    result.err()
}

/// Die stehende Verteidigung einer Welt und ihr Plan, wie eine Oberfläche sie zeigt.
#[derive(Clone, Debug, PartialEq)]
pub struct DefenseCard {
    pub planet: PlanetId,
    pub owner: PolityId,
    pub besieged: bool,
    pub plan: DefensePlan,
    /// Bestand, wie er steht.
    pub defense: PlanetDefense,
    /// Höchste Stufen nach dem Stand der Technik, in der Reihenfolge von [`Works::ALL`].
    pub level_caps: [u8; 4],
    /// So viele Reguläre und Depottage darf der Plan höchstens nennen.
    pub regulars_cap: f64,
    pub depot_days_cap: u16,
    /// Sold und Unterhalt je Monat heute und wenn der Plan erfüllt ist, in Credits.
    pub upkeep: f64,
    pub plan_upkeep: f64,
    /// Was der Ausbau bis zum Plan an Gütern kostet, zu heutigen Preisen in Credits:
    /// Depot, Aushebung und alle fehlenden Stufen.
    pub build_cost: f64,
    /// Garnison, die eine besetzte Welt ruhig hält.
    pub hold_need: f64,
}

/// Die Karte der Verteidigung einer besiedelten Welt.
pub fn defense_card(refs: &SiegeRefs, planet: PlanetId) -> Option<DefenseCard> {
    let balance = &refs.data.balance.siege;
    let spread = refs.data.balance.economy.spread;
    let target = refs.galaxy.planets.get(planet.index())?;
    let owner = target.owner?;
    let defense = refs.sieges.defense(planet);
    let plan = defense.plan.clone();
    let market = refs.economy.markets.get(target.system.index())?;
    let cost = |bundle: &[(crate::ids::GoodId, f64)]| -> f64 {
        bundle
            .iter()
            .map(|(good, amount)| amount * market.ask(*good, spread))
            .sum()
    };
    let missing = shortfall(balance, defense, target.population, false);
    let mut build_cost = missing.depot * cost(&balance.day_bundle_ids)
        + (plan.regulars as f64 - defense.regulars).max(0.0) * cost(&balance.regular_bundle_ids);
    let bundles = [
        &balance.station_ids,
        &balance.gun_ids,
        &balance.shield_ids,
        &balance.bunker_ids,
    ];
    let mut planned = defense.clone();
    planned.regulars = plan.regulars as f64;
    for (works, bundle) in Works::ALL.into_iter().zip(bundles) {
        let (level, target_level) = (works.level(defense).floor() as u8, works.planned(&plan));
        // Stufe n kostet das n-Fache des Satzes; was für die nächste schon geliefert
        // ist, kostet nichts mehr.
        let sets: f64 = (level + 1..=target_level)
            .map(|next| {
                if next == level + 1 {
                    sets_missing(defense, works, next)
                } else {
                    f64::from(next)
                }
            })
            .sum();
        build_cost += sets * cost(bundle);
        match works {
            Works::Stations => planned.stations = target_level as f32,
            Works::Guns => planned.guns = target_level as f32,
            Works::Shield => planned.shield = target_level,
            Works::Bunkers => planned.bunkers = target_level,
        }
    }
    Some(DefenseCard {
        planet,
        owner,
        besieged: refs.sieges.active.contains_key(&planet),
        level_caps: level_caps(balance, refs.research, owner),
        regulars_cap: (balance.regulars_cap * defense.militia).max(defense.regulars),
        depot_days_cap: balance.depot_days_cap,
        upkeep: planet_upkeep(balance, defense),
        plan_upkeep: planet_upkeep(balance, &planned),
        build_cost,
        hold_need: balance.hold_k * target.population.max(0.0).sqrt(),
        defense: defense.clone(),
        plan,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::TICKS_PER_DAY;
    use crate::siege::orbit::testing::{FOE, ME, Scene};
    use crate::siege::{Embarked, SiegeOrders};

    #[test]
    fn the_card_names_parties_phase_strengths_and_the_three_courses() {
        let mut scene = Scene::new();
        scene.state.sieges.embarked.insert(
            FleetId(0),
            Embarked {
                troops: 5.0,
                supply: 50.0,
            },
        );
        let refs = SiegeRefs::of_state(&scene.state, &scene.data);
        assert_eq!(siege_cards(&refs, 0), vec![], "noch besteht kein Datensatz");
        scene.day();
        let refs = SiegeRefs::of_state(&scene.state, &scene.data);
        let cards = siege_cards(&refs, scene.day);
        assert_eq!(cards.len(), 1);
        let card = &cards[0];
        assert_eq!(
            (card.planet, card.attacker, card.defender),
            (scene.planet, ME, FOE)
        );
        assert_eq!((card.phase_number, card.phase_label), (2, "Einschließung"));
        assert!(card.control && card.blockaded && card.handover.is_none());
        assert_eq!(card.fleets, vec![FleetId(0)]);
        assert!(card.firepower > 0.0 && card.fuel > 0.0);
        assert!(card.stores_days > 0.0 && card.troops.militia > 0.0);
        assert_eq!(card.troops.embarked, 5.0);
        assert!(card.troops.occupation_force > 0.0);
        assert_eq!(card.progress, 0.0);
        // Die Prognose ist die des Rechners auf derselben Lage.
        let balance = &scene.data.balance.siege;
        for (course, told) in card.forecasts {
            assert_eq!(told, forecast(&card.view, course, balance), "{course:?}");
        }
        assert_eq!(card.forecasts[0].0, Course::Starve);
    }

    #[test]
    fn a_resting_siege_still_shows_its_fleet_and_names_the_contested_orbit() {
        let mut scene = Scene::colony();
        scene.state.sieges.embarked.insert(
            FleetId(0),
            Embarked {
                troops: 5.0,
                supply: 50.0,
            },
        );
        scene.day();
        // Ein Entsatz trifft ein: Beide Flotten stehen im Gefecht.
        scene.place(1, scene.system);
        scene.state.fleets.list[0].in_battle = true;
        scene.state.fleets.list[1].in_battle = true;
        let refs = SiegeRefs::of_state(&scene.state, &scene.data);
        let card = siege_card(&refs, scene.planet, scene.day).unwrap();
        assert!(!card.control);
        assert_eq!(card.fleets, vec![FleetId(0)]);
        assert!(card.fuel > 0.0 && card.firepower > 0.0);
        assert_eq!(card.troops.embarked, 5.0);
        for (course, told) in card.forecasts {
            assert!(!told.falls, "{course:?}");
            assert_eq!(
                told.limited_by,
                Some(crate::siege::Limit::NoControl),
                "{course:?}"
            );
        }
    }

    #[test]
    fn every_siege_command_is_judged_in_the_words_of_the_core_without_any_effect() {
        let mut scene = Scene::new();
        scene.day();
        let planet = scene.planet;
        let before = (scene.state.sieges.clone(), scene.state.fleets.clone());
        let refs = SiegeRefs::of_state(&scene.state, &scene.data);
        let tick = scene.day * TICKS_PER_DAY + 1;
        let ask = |polity, command: SimCommand| command_block(&refs, polity, &command, tick);
        // Möglich: Befehle ändern, aufheben, erneut zuordnen.
        let orders = SimCommand::SetSiegeOrders {
            planet,
            orders: SiegeOrders::default(),
        };
        assert_eq!(ask(ME, orders.clone()), None);
        assert_eq!(ask(ME, SimCommand::LiftSiege { planet }), None);
        let begin = SimCommand::BeginSiege {
            fleet: FleetId(0),
            planet,
        };
        assert_eq!(ask(ME, begin), None);
        // Nicht möglich, mit dem Grund des Kerns.
        let reason = ask(FOE, orders).unwrap();
        assert!(reason.contains("wird nicht von der eigenen Gemeinschaft belagert"));
        let land = SimCommand::LandTroops {
            fleet: FleetId(0),
            planet,
            strength: 3.0,
        };
        assert!(ask(ME, land).unwrap().contains("sind nur 0.0 Verbände"));
        let surrender = SimCommand::Capitulate { planet };
        assert_eq!(
            ask(FOE, surrender).as_deref(),
            Some("niemand da, der die Übergabe annimmt")
        );
        let embark = SimCommand::EmbarkTroops {
            fleet: FleetId(1),
            planet,
            strength: 1.0,
            supply_days: 10,
        };
        assert!(ask(FOE, embark).unwrap().contains("liegt nicht im System"));
        let plan = SimCommand::SetDefensePlan {
            planet,
            plan: DefensePlan {
                stations: 200,
                ..Default::default()
            },
        };
        assert!(
            ask(FOE, plan)
                .unwrap()
                .contains("Ausbaustufe nicht verfügbar")
        );
        // Andere Befehle beurteilt das Lesemodell nicht.
        assert_eq!(ask(ME, SimCommand::MakePeace { target: FOE }), None);
        // Nichts hat sich geändert.
        assert_eq!(before.0.active, scene.state.sieges.active);
        assert_eq!(before.0.assign, scene.state.sieges.assign);
        assert_eq!(before.1.blockaded, scene.state.fleets.blockaded);
    }

    #[test]
    fn the_defence_card_prices_the_plan() {
        let scene = Scene::new();
        let home = scene.state.polities.get(ME).capital.unwrap();
        let mut state = scene.state.clone();
        let balance = &scene.data.balance.siege;
        let refs = SiegeRefs::of_state(&state, &scene.data);
        let card = defense_card(&refs, home).unwrap();
        // Der Plan nennt, was steht: nichts zu bauen, Unterhalt heute wie nach Plan.
        assert_eq!(card.owner, ME);
        assert_eq!(card.build_cost, 0.0);
        assert_eq!(card.upkeep, card.plan_upkeep);
        let expected =
            balance.pay * card.defense.regulars + balance.level_upkeep * card.defense.guns as f64;
        assert!((card.upkeep - expected).abs() < 1e-9);
        assert_eq!(card.level_caps[0], balance.station_max);
        // Ein größerer Plan kostet Güter und danach mehr Unterhalt.
        state.sieges.defense_mut(home).plan.guns += 2;
        let refs = SiegeRefs::of_state(&state, &scene.data);
        let larger = defense_card(&refs, home).unwrap();
        assert!(larger.build_cost > 0.0);
        assert!(
            (larger.plan_upkeep - card.upkeep - 2.0 * balance.level_upkeep).abs() < 1e-9,
            "zwei Stufen mehr kosten zweimal den Unterhalt einer Stufe"
        );
        // Eine unbesiedelte Welt hat keine Karte.
        let empty = state
            .galaxy
            .planets
            .iter()
            .find(|planet| planet.owner.is_none())
            .unwrap()
            .id;
        assert_eq!(defense_card(&refs, empty), None);
    }
}

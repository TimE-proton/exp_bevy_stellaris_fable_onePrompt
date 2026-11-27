//! Was eine Gemeinschaft von fremden Flotten sieht, und die Kräfteschätzung.
//!
//! Von einer fremden Flotte ist bekannt, was die Karte zeigt: wo sie steht, wohin sie
//! unterwegs ist, ob sie kämpft oder sich sammelt, und welche Schiffe sie führt (Klasse
//! und Bauwert). Zustand, Vorräte und Technik ihrer Schiffe kennt nur ihre eigene
//! Gemeinschaft; die Technik geht mit der Schätzung ein, die das Spiel anbietet
//! ([`Research::estimate`]). Die eigene Flotte rechnet mit ihrem wirklichen Zustand. Das
//! Im laufenden Gefecht ([`battle_values`]) zählt auf beiden Seiten, was noch kämpft,
//! mit seinem Bauwert.

use crate::battle::{Battle, ShipStatus};
use crate::data::{GameData, ShipClass, TechBranch};
use crate::fleet::{Battles, Fleet, Fleets, combat_value};
use crate::galaxy::Galaxy;
use crate::ids::{FleetId, PlanetId, PolityId, SystemId};
use crate::rng::{Domain, Rng};
use crate::siege::{Sieges, militia_strength};
use crate::tech::Research;

/// Wert je Schiffsklasse in Credits, in der Reihenfolge von [`ShipClass::ALL`].
pub type ClassValues = [f64; 6];

/// Eine fremde Flotte, wie sie von außen zu sehen ist.
#[derive(Clone, Debug, PartialEq)]
pub struct Sighting {
    pub fleet: FleetId,
    pub owner: PolityId,
    /// System, in dem sie steht oder das sie zuletzt verlassen hat.
    pub system: SystemId,
    /// Nächstes System auf ihrem Weg, wenn sie einen Marschbefehl hat.
    pub heading: Option<SystemId>,
    /// Sammelt sich nach einem Gefecht: Sie kämpft nicht und wird nicht angegriffen.
    pub regrouping: bool,
    pub in_battle: bool,
    /// Bauwert ihrer Schiffe je Klasse.
    pub values: ClassValues,
}

impl Sighting {
    /// Wo die Flotte steht oder gleich stehen wird: Wer marschiert, zählt am nächsten
    /// System seines Wegs.
    pub fn position(&self) -> SystemId {
        self.heading.unwrap_or(self.system)
    }
}

/// Bauwert der Schiffe einer Flotte je Klasse, wie ihn jeder sieht.
pub fn seen_values(data: &GameData, fleet: &Fleet) -> ClassValues {
    let mut values = [0.0; 6];
    for ship in &fleet.ships {
        let stats = &data.designs[ship.design as usize].stats;
        values[stats.class.index()] += stats.value;
    }
    values
}

/// Kampfwert der eigenen Flotte je Klasse: Bauwert mal Zustand, gemindert um leere
/// Magazine (ein leeres Magazin halbiert den Wert).
pub fn own_values(data: &GameData, fleet: &Fleet) -> ClassValues {
    let mut values = [0.0; 6];
    for ship in &fleet.ships {
        let stats = &data.designs[ship.design as usize].stats;
        let ammo = if stats.ammo > 0.0 { ship.ammo } else { 1.0 };
        values[stats.class.index()] += combat_value(data, ship) * (0.5 + 0.5 * ammo as f64);
    }
    values
}

pub fn total(values: &ClassValues) -> f64 {
    values.iter().sum()
}

pub fn add(into: &mut ClassValues, values: &ClassValues) {
    for (sum, value) in into.iter_mut().zip(values) {
        *sum += value;
    }
}

/// Alle bestehenden Flotten anderer Gemeinschaften, in Nummernfolge.
pub fn sightings(data: &GameData, fleets: &Fleets, viewer: PolityId) -> Vec<Sighting> {
    fleets
        .list
        .iter()
        .filter(|fleet| fleet.alive && fleet.owner != viewer)
        .map(|fleet| Sighting {
            fleet: fleet.id,
            owner: fleet.owner,
            system: fleet.system,
            heading: fleet.route.first().copied(),
            regrouping: fleet.is_regrouping(),
            in_battle: fleet.in_battle,
            values: seen_values(data, fleet),
        })
        .collect()
}

/// Die Kräfte in einem laufenden Gefecht aus Sicht der Partei `side`: was für ihre Seite
/// und was gegen sie noch kämpft, je Klasse, dazu die Gemeinschaften der Gegner. Gezählt
/// wird auf beiden Seiten dasselbe, nämlich was jeder sieht: die Schiffe, die noch
/// kämpfen (Jäger in der Luft eingeschlossen), mit ihrem Bauwert. Rumpf und Hangars der
/// Gegner kennt die Partei nicht; rechnete sie nur die eigenen Schäden ein, hielte sie
/// sich im Lauf jedes Gefechts für unterlegen.
pub fn battle_values(
    battles: &Battles,
    battle: &Battle,
    side: usize,
) -> (ClassValues, ClassValues, Vec<PolityId>) {
    let (mut mine, mut theirs): (ClassValues, ClassValues) = ([0.0; 6], [0.0; 6]);
    let mut foes: Vec<PolityId> = Vec::new();
    for ship in battle
        .ships
        .iter()
        .filter(|ship| ship.status == ShipStatus::Active)
    {
        let other = ship.side as usize;
        if battles.has_departed(battle.id, other) {
            continue;
        }
        if battle.is_hostile(side as u16, ship.side) {
            theirs[ship.class.index()] += ship.value as f64;
            let owner = battle.combatants[other].owner;
            if !foes.contains(&owner) {
                foes.push(owner);
            }
        } else {
            mine[ship.class.index()] += ship.value as f64;
        }
    }
    (mine, theirs, foes)
}

/// Wirksamkeit einer Klasse gegen die Mischung eines Gegners: das Mittel der Kontertabelle
/// über dessen Klassen, gewichtet mit ihrem Wert. Gegen nichts ist jede Klasse gleich gut.
pub fn class_score(data: &GameData, class: ShipClass, enemy: &ClassValues) -> f64 {
    let sum = total(enemy);
    if sum <= 0.0 {
        return 1.0;
    }
    ShipClass::ALL
        .iter()
        .map(|other| {
            enemy[other.index()] / sum * data.balance.military.counter_of(class, *other) as f64
        })
        .sum()
}

/// Kampfkraft einer Flotte gegen eine bestimmte andere: ihr Wert je Klasse, gewichtet mit
/// der Wirksamkeit der Klasse gegen die Mischung des Gegners.
pub fn power(data: &GameData, own: &ClassValues, enemy: &ClassValues) -> f64 {
    ShipClass::ALL
        .iter()
        .map(|class| own[class.index()] * class_score(data, *class, enemy))
        .sum()
}

/// Geschätztes Kräfteverhältnis: eigene Kampfkraft durch die des Gegners, mal dem
/// Vorsprung der eigenen Technik (`edge`, 1 = gleich). Über 1 ist die eigene Seite
/// stärker; ohne Gegner unendlich.
pub fn odds(data: &GameData, own: &ClassValues, enemy: &ClassValues, edge: f64) -> f64 {
    let theirs = power(data, enemy, own);
    if theirs <= 0.0 {
        return f64::INFINITY;
    }
    power(data, own, enemy) * edge / theirs
}

/// Vorsprung der eigenen Technik in Waffen und Verteidigung gegenüber dem, was der
/// Betrachter vom Gegner weiß (Mitte der geschätzten Spanne je Zweig), als Faktor auf die
/// eigene Kampfkraft.
pub fn tech_edge(data: &GameData, research: &Research, viewer: PolityId, enemy: PolityId) -> f64 {
    let own = research.estimate(data, viewer, viewer);
    let theirs = research.estimate(data, viewer, enemy);
    let lead: f64 = [TechBranch::Weapons, TechBranch::Defense]
        .iter()
        .map(|branch| {
            let (mine, other) = (own[branch.index()], theirs[branch.index()]);
            mine.lo as f64 - (other.lo as f64 + other.hi as f64) / 2.0
        })
        .sum::<f64>()
        / 2.0;
    (1.0 + data.balance.military.ai.tech_edge * lead).clamp(0.5, 2.0)
}

/// Die Verteidigung einer fremden Welt, wie sie von außen zu schätzen ist.
///
/// Anlagen sieht jeder aus dem Orbit, die Miliz folgt aus der Bevölkerung. Reguläre und
/// Vorräte sind nur als Spanne bekannt: so breit wie `siege.intel_spread` des wahren
/// Werts, und wo die Wahrheit darin liegt, steht je Betrachter und Welt fest (nachfragen
/// schärft das Bild nicht). Geplant wird mit der oberen Grenze.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldEstimate {
    pub stations: f64,
    pub guns: f64,
    pub shield: u8,
    pub militia: f64,
    /// Reguläre Verbände: untere und obere Grenze.
    pub regulars: (f64, f64),
    /// Vorräte in Tagen: untere und obere Grenze.
    pub depot_days: (f64, f64),
}

impl WorldEstimate {
    /// Verteidiger, mit denen ein Angreifer rechnen muss: Miliz und die obere Grenze
    /// der Regulären.
    pub fn defenders(&self) -> f64 {
        self.militia + self.regulars.1
    }
}

/// Schätzt die Verteidigung der Welt `planet` aus Sicht von `viewer`.
pub fn world_estimate(
    data: &GameData,
    galaxy: &Galaxy,
    sieges: &Sieges,
    seed: u64,
    viewer: PolityId,
    planet: PlanetId,
) -> WorldEstimate {
    let balance = &data.balance.siege;
    let defense = sieges.defense(planet);
    let spread = balance.intel_spread.clamp(0.0, 1.0);
    let mut rng = Rng::stream(
        seed,
        Domain::Intel,
        ((viewer.index() as u64) << 32) | planet.index() as u64,
        0,
    );
    // Die Spanne ist `spread` des wahren Werts breit und enthält ihn.
    let mut range = |truth: f64| {
        let low = truth * (1.0 - spread * rng.f64());
        (low.max(0.0), low.max(0.0) + spread * truth)
    };
    let regulars = range(defense.regulars);
    let depot_days = range(defense.depot);
    WorldEstimate {
        stations: defense.stations as f64,
        guns: defense.guns as f64,
        shield: defense.shield,
        // Die Miliz folgt Bevölkerung und Zusammenhalt; gerechnet wird mit dem
        // Zusammenhalt, bei dem sie ihr Grundmaß hat.
        militia: militia_strength(
            galaxy.planet(planet).population,
            balance.militia_cohesion.0,
            balance,
        ),
        regulars,
        depot_days,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_foreign_garrison_is_known_as_a_range_that_holds_the_truth() {
        use crate::siege::orbit::testing::{FOE, ME, Scene};
        let mut scene = Scene::new();
        let planet = scene.planet;
        let defense = scene.state.sieges.defense_mut(planet);
        defense.regulars = 20.0;
        defense.depot = 80.0;
        defense.guns = 2.0;
        defense.shield = 1;
        let spread = scene.data.balance.siege.intel_spread;
        let view = |viewer: PolityId| {
            world_estimate(
                &scene.data,
                &scene.state.galaxy,
                &scene.state.sieges,
                9,
                viewer,
                planet,
            )
        };
        let estimate = view(ME);
        // Anlagen sieht jeder; Reguläre und Vorräte sind eine Spanne um die Wahrheit,
        // so breit wie intel_spread des wahren Werts.
        assert_eq!(
            (estimate.guns, estimate.shield, estimate.stations),
            (2.0, 1, 0.0)
        );
        assert!(estimate.regulars.0 <= 20.0 && 20.0 <= estimate.regulars.1);
        assert!(estimate.depot_days.0 <= 80.0 && 80.0 <= estimate.depot_days.1);
        assert!((estimate.regulars.1 - estimate.regulars.0 - spread * 20.0).abs() < 1e-9);
        assert!(estimate.militia > 0.0);
        assert_eq!(estimate.defenders(), estimate.militia + estimate.regulars.1);
        // Nachfragen schärft das Bild nicht; ein anderer Betrachter sieht ein anderes.
        assert_eq!(estimate, view(ME));
        assert_ne!(estimate.regulars, view(FOE).regulars);
        assert_ne!(estimate.regulars, view(PolityId(2)).regulars);
    }

    #[test]
    fn the_counter_table_decides_between_fleets_of_equal_value() {
        let data = GameData::load_workspace().unwrap();
        let pure = |class: ShipClass| {
            let mut values = [0.0; 6];
            values[class.index()] = 10_000.0;
            values
        };
        // Gleiches gegen Gleiches ist ausgeglichen.
        for class in ShipClass::ALL {
            let fleet = pure(class);
            assert!((odds(&data, &fleet, &fleet, 1.0) - 1.0).abs() < 1e-9);
        }
        // Die Tabelle stammt aus der Gefechtsmatrix: Kreuzer schlagen Fregatten,
        // Schlachtschiffe schlagen Kreuzer, Korvetten schlagen Schlachtschiffe.
        let beats = |a: ShipClass, b: ShipClass| {
            let forward = odds(&data, &pure(a), &pure(b), 1.0);
            let backward = odds(&data, &pure(b), &pure(a), 1.0);
            assert!((forward * backward - 1.0).abs() < 1e-9);
            forward
        };
        assert!(beats(ShipClass::Cruiser, ShipClass::Frigate) > 1.5);
        assert!(beats(ShipClass::Battleship, ShipClass::Cruiser) > 1.5);
        assert!(beats(ShipClass::Corvette, ShipClass::Battleship) > 1.5);
        // Mehr vom Gleichen ist mehr; Technik zählt als Faktor; ohne Gegner unendlich.
        let cruisers = pure(ShipClass::Cruiser);
        let mut double = cruisers;
        add(&mut double, &cruisers);
        assert!((odds(&data, &double, &cruisers, 1.0) - 2.0).abs() < 1e-9);
        assert!((odds(&data, &cruisers, &cruisers, 1.2) - 1.2).abs() < 1e-9);
        assert!(odds(&data, &cruisers, &[0.0; 6], 1.0).is_infinite());
    }
}

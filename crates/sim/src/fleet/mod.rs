//! Flotten auf der Galaxiekarte: Bewegung, Blockade, Kaperfahrt.
//!
//! Stand Meilenstein 2: Eine Flotte ist eine Stärkezahl, Gefechte werden grob aufgelöst.
//! Meilenstein 4 ersetzt das durch Schiffe mit Modulen und den taktischen Resolver.

mod plugin;

pub use plugin::FleetPlugin;

use crate::chronicle::{Chronicle, ChronicleKind};
use crate::data::{FleetBalance, GameData};
use crate::diplomacy::Diplomacy;
use crate::galaxy::Galaxy;
use crate::ids::{FleetId, PolityId, SystemId};
use crate::polity::Polities;
use crate::rng::{Domain, Rng};
use bevy_ecs::prelude::Resource;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stance {
    /// Hält die Stellung und kämpft, wenn Feinde kommen.
    #[default]
    Guard,
    /// Sperrt ein feindliches System für jeden Handel.
    Blockade,
    /// Bringt feindliche Frachter auf.
    Raid,
    /// Schützt eigene Frachter vor Kaperern.
    Escort,
}

impl Stance {
    pub const ALL: [Stance; 4] = [
        Stance::Guard,
        Stance::Blockade,
        Stance::Raid,
        Stance::Escort,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Stance::Guard => "Wache",
            Stance::Blockade => "Blockade",
            Stance::Raid => "Kaperfahrt",
            Stance::Escort => "Geleitschutz",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Fleet {
    pub id: FleetId,
    pub owner: PolityId,
    pub name: String,
    pub strength: f32,
    pub stance: Stance,
    /// Aktuelles System, unterwegs das zuletzt verlassene.
    pub system: SystemId,
    /// Verbleibender Weg; das erste Element ist das nächste System.
    pub route: Vec<SystemId>,
    /// Fortschritt auf der Route zum nächsten System (0..1).
    pub progress: f32,
    pub alive: bool,
    pub in_battle: bool,
}

impl Fleet {
    /// Steht die Flotte in einem System (und ist nicht auf einer Sprungroute)?
    pub fn is_docked(&self) -> bool {
        self.alive && (self.route.is_empty() || self.progress <= 0.0)
    }
}

#[derive(Resource, Clone, Debug, Default, Serialize, Deserialize)]
pub struct Fleets {
    pub list: Vec<Fleet>,
    /// Blockierte Systeme, aufsteigend. Wird täglich aus der Lage neu bestimmt.
    pub blockaded: Vec<SystemId>,
}

impl Fleets {
    pub fn is_blockaded(&self, system: SystemId) -> bool {
        self.blockaded.binary_search(&system).is_ok()
    }

    pub fn docked_in(&self, system: SystemId) -> impl Iterator<Item = &Fleet> + '_ {
        self.list
            .iter()
            .filter(move |fleet| fleet.is_docked() && fleet.system == system)
    }

    pub fn of(&self, polity: PolityId) -> impl Iterator<Item = &Fleet> + '_ {
        self.list
            .iter()
            .filter(move |fleet| fleet.alive && fleet.owner == polity)
    }

    /// Stärke der Kaperer in `system`, die `victim` feindlich sind, und der Geleitschutz,
    /// der `victim` dort zur Seite steht.
    pub fn raid_pressure(
        &self,
        diplomacy: &Diplomacy,
        system: SystemId,
        victim: PolityId,
    ) -> (f32, f32, Option<PolityId>) {
        let mut raiders = 0.0;
        let mut escorts = 0.0;
        let mut raider = None;
        for fleet in self.docked_in(system) {
            if fleet.stance == Stance::Raid && diplomacy.at_war(fleet.owner, victim) {
                raiders += fleet.strength;
                raider.get_or_insert(fleet.owner);
            } else if fleet.owner == victim && fleet.stance == Stance::Escort {
                escorts += fleet.strength;
            }
        }
        (raiders, escorts, raider)
    }
}

pub fn setup_fleets(data: &GameData, galaxy: &Galaxy, polities: &Polities) -> Fleets {
    let mut fleets = Fleets::default();
    for polity in &polities.list {
        let Some(capital) = polity.capital else {
            continue;
        };
        let id = FleetId::from_index(fleets.list.len());
        fleets.list.push(Fleet {
            id,
            owner: polity.id,
            name: format!("1. Flotte ({})", polity.short),
            strength: data.balance.fleets.start_strength,
            stance: Stance::Guard,
            system: galaxy.planet(capital).system,
            route: Vec::new(),
            progress: 0.0,
            alive: true,
            in_battle: false,
        });
    }
    fleets
}

fn own_fleet(fleets: &mut Fleets, polity: PolityId, fleet: FleetId) -> Result<&mut Fleet, String> {
    match fleets.list.get_mut(fleet.index()) {
        Some(found) if found.alive && found.owner == polity => Ok(found),
        Some(_) => Err("Flotte gehört einer anderen Gemeinschaft oder besteht nicht mehr".into()),
        None => Err("Flotte unbekannt".into()),
    }
}

/// Schickt eine Flotte auf dem kürzesten Weg in ein System.
pub fn order_move(
    galaxy: &Galaxy,
    fleets: &mut Fleets,
    polity: PolityId,
    fleet: FleetId,
    to: SystemId,
) -> Result<(), String> {
    if to.index() >= galaxy.systems.len() {
        return Err("Zielsystem unbekannt".into());
    }
    let fleet = own_fleet(fleets, polity, fleet)?;
    if !fleet.is_docked() {
        // Unterwegs gilt der neue Befehl ab dem nächsten System.
        let next = fleet.route[0];
        let mut route = vec![next];
        if next != to {
            let path = galaxy
                .shortest_path(next, to, |_| true)
                .ok_or("kein Weg zum Ziel")?;
            route.extend(path.into_iter().skip(1));
        }
        fleet.route = route;
        return Ok(());
    }
    if fleet.system == to {
        fleet.route.clear();
        return Ok(());
    }
    let path = galaxy
        .shortest_path(fleet.system, to, |_| true)
        .ok_or("kein Weg zum Ziel")?;
    fleet.route = path.into_iter().skip(1).collect();
    fleet.progress = 0.0;
    Ok(())
}

pub fn set_stance(
    fleets: &mut Fleets,
    polity: PolityId,
    fleet: FleetId,
    stance: Stance,
) -> Result<(), String> {
    own_fleet(fleets, polity, fleet)?.stance = stance;
    Ok(())
}

/// Flotten rücken entlang ihrer Route vor.
pub fn move_fleets(galaxy: &Galaxy, fleets: &mut Fleets, balance: &FleetBalance) {
    for fleet in &mut fleets.list {
        if !fleet.alive || fleet.route.is_empty() || fleet.in_battle {
            continue;
        }
        let Some(lane) = galaxy.lane_between(fleet.system, fleet.route[0]) else {
            fleet.route.clear();
            fleet.progress = 0.0;
            continue;
        };
        fleet.progress += balance.speed / galaxy.lane(lane).length.max(1.0);
        if fleet.progress >= 1.0 {
            fleet.system = fleet.route.remove(0);
            fleet.progress = 0.0;
        }
    }
}

/// Grobe Gefechtsauflösung: Feindliche Flotten im selben System zehren einander auf.
pub fn resolve_skirmishes(
    fleets: &mut Fleets,
    diplomacy: &Diplomacy,
    polities: &Polities,
    chronicle: &mut Chronicle,
    balance: &FleetBalance,
    seed: u64,
    tick: u64,
) {
    let day = tick / crate::clock::TICKS_PER_DAY;
    let count = fleets.list.len();
    let mut damage = vec![0.0f32; count];
    let mut fighting = vec![false; count];
    for a in 0..count {
        if !fleets.list[a].is_docked() {
            continue;
        }
        for b in (a + 1)..count {
            let (first, second) = (&fleets.list[a], &fleets.list[b]);
            if !second.is_docked()
                || first.system != second.system
                || !diplomacy.at_war(first.owner, second.owner)
            {
                continue;
            }
            let mut rng = Rng::stream(seed, Domain::Battle, (a as u64) << 32 | b as u64, day);
            damage[b] += first.strength * balance.daily_damage * rng.range_f32(0.7, 1.3);
            damage[a] += second.strength * balance.daily_damage * rng.range_f32(0.7, 1.3);
            fighting[a] = true;
            fighting[b] = true;
        }
    }
    for index in 0..count {
        let fleet = &mut fleets.list[index];
        if !fleet.alive {
            continue;
        }
        if fighting[index] && !fleet.in_battle {
            chronicle.push(
                tick,
                ChronicleKind::War,
                Some(fleet.owner),
                Some(fleet.system),
                format!("{} steht im Gefecht.", fleet.name),
            );
        }
        fleet.in_battle = fighting[index];
        fleet.strength -= damage[index];
        if fleet.strength <= 0.0 {
            fleet.strength = 0.0;
            fleet.alive = false;
            fleet.in_battle = false;
            fleet.route.clear();
            chronicle.push(
                tick,
                ChronicleKind::War,
                Some(fleet.owner),
                Some(fleet.system),
                format!(
                    "{} der Gemeinschaft {} wurde vernichtet.",
                    fleet.name,
                    polities.get(fleet.owner).name
                ),
            );
        }
    }
}

/// Ein System ist blockiert, wenn eine feindliche Flotte im Blockadeauftrag dort steht
/// und der Besitzer keine eigene Flotte im Orbit hat: Erst die orbitale Überlegenheit,
/// dann die Sperre.
pub fn update_blockades(galaxy: &Galaxy, fleets: &mut Fleets, diplomacy: &Diplomacy) {
    let mut blockaded = Vec::new();
    for system in &galaxy.systems {
        let Some(owner) = galaxy.system_owner(system.id) else {
            continue;
        };
        let defended = fleets
            .docked_in(system.id)
            .any(|fleet| fleet.owner == owner);
        let besieged = fleets
            .docked_in(system.id)
            .any(|fleet| fleet.stance == Stance::Blockade && diplomacy.at_war(fleet.owner, owner));
        if besieged && !defended {
            blockaded.push(system.id);
        }
    }
    fleets.blockaded = blockaded;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::galaxy::generate_galaxy;
    use crate::polity::found_polities;

    fn setup() -> (GameData, Galaxy, Polities, Fleets) {
        let data = GameData::load_workspace().unwrap();
        let mut galaxy = generate_galaxy(&data, 9);
        let polities = found_polities(&data, &mut galaxy, 9);
        let fleets = setup_fleets(&data, &galaxy, &polities);
        (data, galaxy, polities, fleets)
    }

    fn travel(galaxy: &Galaxy, fleets: &mut Fleets, data: &GameData, fleet: FleetId) {
        for _ in 0..2000 {
            if fleets.list[fleet.index()].route.is_empty() {
                return;
            }
            move_fleets(galaxy, fleets, &data.balance.fleets);
        }
        panic!("Flotte kam nie an");
    }

    #[test]
    fn fleets_follow_lanes_to_their_target() {
        let (data, galaxy, polities, mut fleets) = setup();
        let target = galaxy.planet(polities.list[1].capital.unwrap()).system;
        order_move(&galaxy, &mut fleets, PolityId(0), FleetId(0), target).unwrap();
        let route = fleets.list[0].route.clone();
        let mut previous = fleets.list[0].system;
        for step in &route {
            assert!(galaxy.lane_between(previous, *step).is_some());
            previous = *step;
        }
        travel(&galaxy, &mut fleets, &data, FleetId(0));
        assert_eq!(fleets.list[0].system, target);
        assert!(order_move(&galaxy, &mut fleets, PolityId(1), FleetId(0), target).is_err());
    }

    #[test]
    fn blockade_needs_war_and_orbital_superiority() {
        let (data, galaxy, polities, mut fleets) = setup();
        let mut diplomacy = Diplomacy::default();
        let mut chronicle = Chronicle::default();
        let target = galaxy.planet(polities.list[1].capital.unwrap()).system;
        order_move(&galaxy, &mut fleets, PolityId(0), FleetId(0), target).unwrap();
        set_stance(&mut fleets, PolityId(0), FleetId(0), Stance::Blockade).unwrap();
        travel(&galaxy, &mut fleets, &data, FleetId(0));

        // Ohne Krieg keine Blockade.
        update_blockades(&galaxy, &mut fleets, &diplomacy);
        assert!(!fleets.is_blockaded(target));

        // Im Krieg verhindert die Heimatflotte die Sperre, solange sie steht.
        crate::diplomacy::declare_war(
            &mut diplomacy,
            &polities,
            &mut chronicle,
            0,
            PolityId(0),
            PolityId(1),
        )
        .unwrap();
        update_blockades(&galaxy, &mut fleets, &diplomacy);
        assert!(!fleets.is_blockaded(target));

        // Der Angreifer ist doppelt so stark: Er gewinnt das Gefecht und sperrt das System.
        fleets.list[0].strength = 200.0;
        for day in 0..400 {
            resolve_skirmishes(
                &mut fleets,
                &diplomacy,
                &polities,
                &mut chronicle,
                &data.balance.fleets,
                9,
                day * 20,
            );
            if !fleets.list[1].alive {
                break;
            }
        }
        assert!(!fleets.list[1].alive && fleets.list[0].alive);
        assert!(fleets.list[0].strength < 200.0, "auch der Sieger zahlt");
        update_blockades(&galaxy, &mut fleets, &diplomacy);
        assert!(fleets.is_blockaded(target));
    }
}

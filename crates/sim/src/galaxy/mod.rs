//! Galaxie: Sternsysteme, Sprungrouten, Planeten.

mod generate;
mod plugin;

pub use generate::{generate_galaxy, habitability, roll_planet};
pub use plugin::GalaxyPlugin;

use crate::data::Atmosphere;
use crate::ids::{GoodId, LaneId, PlanetId, PolityId, SystemId};
use bevy_ecs::prelude::Resource;
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, VecDeque};

pub type Vec2 = [f32; 2];

#[inline]
pub fn dist(a: Vec2, b: Vec2) -> f32 {
    dist2(a, b).sqrt()
}

#[inline]
pub fn dist2(a: Vec2, b: Vec2) -> f32 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    dx * dx + dy * dy
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Cluster {
    pub center: Vec2,
    pub systems: Vec<SystemId>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StarSystem {
    pub id: SystemId,
    pub name: String,
    pub pos: Vec2,
    pub cluster: u32,
    /// Index in `galaxy.ron: stars`.
    pub star: u32,
    pub planets: Vec<PlanetId>,
    /// Lage im Routennetz 0..1 (Betweenness): Anteil kürzester Wege, die hier durchlaufen.
    pub centrality: f32,
    /// Endpunkt einer Brücke zwischen Clustern.
    pub chokepoint: bool,
}

impl StarSystem {
    #[inline]
    pub fn index(&self) -> usize {
        self.id.index()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Lane {
    pub id: LaneId,
    pub a: SystemId,
    pub b: SystemId,
    pub length: f32,
    /// Verbindet zwei Cluster. Wer beide Enden hält, kontrolliert den Durchgang.
    pub bridge: bool,
}

impl Lane {
    pub fn other(&self, system: SystemId) -> SystemId {
        if self.a == system { self.b } else { self.a }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Deposit {
    pub good: GoodId,
    /// Ergiebigkeit; 1.0 ist ein gutes Vorkommen.
    pub richness: f32,
    /// Verbleibender Anteil 0..1. Abbau zehrt ihn auf, die Ergiebigkeit sinkt mit.
    pub reserve: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Planet {
    pub id: PlanetId,
    pub system: SystemId,
    pub name: String,
    /// Index in `planet_kinds.ron`.
    pub kind: u32,
    /// Bahnnummer im System (0 = innerste).
    pub orbit: u32,
    /// Schwerkraft in g. Hohe Schwerkraft verteuert jeden Start in den Orbit.
    pub gravity: f32,
    pub atmosphere: Atmosphere,
    /// Mittlere Temperatur in °C.
    pub temperature: f32,
    /// Wasseranteil der Oberfläche 0..1.
    pub water: f32,
    /// Abgeleitet aus Atmosphäre, Temperatur, Schwerkraft und Wasser, 0..1.
    pub habitability: f32,
    /// Tragfähigkeit in Millionen Einwohnern.
    pub capacity: f64,
    /// Millionen Einwohner.
    pub population: f64,
    pub deposits: Vec<Deposit>,
    pub owner: Option<PolityId>,
}

impl Planet {
    pub fn deposit(&self, good: GoodId) -> Option<&Deposit> {
        self.deposits.iter().find(|deposit| deposit.good == good)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Neighbor {
    pub system: SystemId,
    pub lane: LaneId,
}

#[derive(Resource, Clone, Debug, Default, Serialize, Deserialize)]
pub struct Galaxy {
    pub clusters: Vec<Cluster>,
    pub systems: Vec<StarSystem>,
    pub lanes: Vec<Lane>,
    pub planets: Vec<Planet>,
    /// Abgeleiteter Index; nicht Teil von Spielstand und Prüfsumme.
    #[serde(skip)]
    adjacency: Vec<Vec<Neighbor>>,
}

impl Galaxy {
    /// Muss nach jedem Laden und nach Änderungen an den Routen aufgerufen werden.
    pub fn rebuild_index(&mut self) {
        let mut adjacency = vec![Vec::new(); self.systems.len()];
        for lane in &self.lanes {
            adjacency[lane.a.index()].push(Neighbor {
                system: lane.b,
                lane: lane.id,
            });
            adjacency[lane.b.index()].push(Neighbor {
                system: lane.a,
                lane: lane.id,
            });
        }
        for list in &mut adjacency {
            list.sort_by_key(|neighbor| neighbor.system);
        }
        self.adjacency = adjacency;
    }

    #[inline]
    pub fn system(&self, id: SystemId) -> &StarSystem {
        &self.systems[id.index()]
    }

    #[inline]
    pub fn planet(&self, id: PlanetId) -> &Planet {
        &self.planets[id.index()]
    }

    #[inline]
    pub fn planet_mut(&mut self, id: PlanetId) -> &mut Planet {
        &mut self.planets[id.index()]
    }

    #[inline]
    pub fn lane(&self, id: LaneId) -> &Lane {
        &self.lanes[id.index()]
    }

    /// Nachbarn in aufsteigender ID-Reihenfolge.
    #[inline]
    pub fn neighbors(&self, id: SystemId) -> &[Neighbor] {
        debug_assert_eq!(
            self.adjacency.len(),
            self.systems.len(),
            "rebuild_index fehlt"
        );
        &self.adjacency[id.index()]
    }

    pub fn lane_between(&self, a: SystemId, b: SystemId) -> Option<LaneId> {
        self.neighbors(a)
            .iter()
            .find(|neighbor| neighbor.system == b)
            .map(|neighbor| neighbor.lane)
    }

    pub fn planets_in(&self, system: SystemId) -> impl Iterator<Item = &Planet> + '_ {
        self.system(system)
            .planets
            .iter()
            .map(|id| self.planet(*id))
    }

    pub fn planets_of(&self, polity: PolityId) -> impl Iterator<Item = &Planet> + '_ {
        self.planets
            .iter()
            .filter(move |planet| planet.owner == Some(polity))
    }

    pub fn population_of(&self, polity: PolityId) -> f64 {
        self.planets_of(polity)
            .map(|planet| planet.population)
            .sum()
    }

    pub fn owns_planet_in(&self, polity: PolityId, system: SystemId) -> bool {
        self.planets_in(system)
            .any(|planet| planet.owner == Some(polity))
    }

    /// Wer das System prägt: die Gemeinschaft mit der größten Bevölkerung dort.
    /// Bei Gleichstand gewinnt die kleinere ID.
    pub fn system_owner(&self, system: SystemId) -> Option<PolityId> {
        let mut by_owner: BTreeMap<PolityId, f64> = BTreeMap::new();
        for planet in self.planets_in(system) {
            if let Some(owner) = planet.owner {
                *by_owner.entry(owner).or_default() += planet.population;
            }
        }
        let mut best: Option<(PolityId, f64)> = None;
        for (owner, population) in by_owner {
            if best.is_none_or(|(_, best_population)| population > best_population) {
                best = Some((owner, population));
            }
        }
        best.map(|(owner, _)| owner)
    }

    /// Sprungdistanz von `start` zu jedem System (`u32::MAX` = unerreichbar).
    pub fn hops_from(&self, start: SystemId) -> Vec<u32> {
        let mut hops = vec![u32::MAX; self.systems.len()];
        let mut queue = VecDeque::new();
        hops[start.index()] = 0;
        queue.push_back(start);
        while let Some(current) = queue.pop_front() {
            for neighbor in self.neighbors(current) {
                if hops[neighbor.system.index()] == u32::MAX {
                    hops[neighbor.system.index()] = hops[current.index()] + 1;
                    queue.push_back(neighbor.system);
                }
            }
        }
        hops
    }

    pub fn is_connected(&self) -> bool {
        match self.systems.first() {
            None => true,
            Some(first) => self
                .hops_from(first.id)
                .iter()
                .all(|hops| *hops != u32::MAX),
        }
    }

    /// Zahl der Zusammenhangskomponenten, wenn alle Brücken gesperrt sind.
    pub fn components_without_bridges(&self) -> usize {
        let mut seen = vec![false; self.systems.len()];
        let mut components = 0;
        for start in 0..self.systems.len() {
            if seen[start] {
                continue;
            }
            components += 1;
            let mut stack = vec![SystemId::from_index(start)];
            seen[start] = true;
            while let Some(current) = stack.pop() {
                for neighbor in self.neighbors(current) {
                    if self.lane(neighbor.lane).bridge || seen[neighbor.system.index()] {
                        continue;
                    }
                    seen[neighbor.system.index()] = true;
                    stack.push(neighbor.system);
                }
            }
        }
        components
    }

    /// Kürzester Weg nach Routenlänge (Dijkstra). `passable` entscheidet, ob ein System
    /// betreten werden darf; Start und Ziel werden nicht geprüft. Gleich lange Wege
    /// werden über die System-ID entschieden, das Ergebnis ist also eindeutig.
    pub fn shortest_path(
        &self,
        from: SystemId,
        to: SystemId,
        passable: impl Fn(SystemId) -> bool,
    ) -> Option<Vec<SystemId>> {
        if from == to {
            return Some(vec![from]);
        }
        let count = self.systems.len();
        let mut best = vec![u64::MAX; count];
        let mut previous: Vec<Option<SystemId>> = vec![None; count];
        let mut heap = BinaryHeap::new();
        best[from.index()] = 0;
        heap.push(Reverse((0u64, from)));
        while let Some(Reverse((cost, current))) = heap.pop() {
            if cost > best[current.index()] {
                continue;
            }
            if current == to {
                break;
            }
            for neighbor in self.neighbors(current) {
                if neighbor.system != to && !passable(neighbor.system) {
                    continue;
                }
                // Ganzzahlige Kosten: keine Fließkomma-Vergleiche in der Wegsuche.
                let step = (self.lane(neighbor.lane).length * 16.0) as u64 + 1;
                let next_cost = cost + step;
                let slot = &mut best[neighbor.system.index()];
                if next_cost < *slot {
                    *slot = next_cost;
                    previous[neighbor.system.index()] = Some(current);
                    heap.push(Reverse((next_cost, neighbor.system)));
                }
            }
        }
        if best[to.index()] == u64::MAX {
            return None;
        }
        let mut path = vec![to];
        let mut cursor = to;
        while let Some(prev) = previous[cursor.index()] {
            path.push(prev);
            cursor = prev;
        }
        path.reverse();
        Some(path)
    }

    /// Routenlänge von `from` zu jedem System (`f32::INFINITY` = unerreichbar) und der
    /// jeweilige Vorgänger auf dem kürzesten Weg. `passable` gilt für jedes betretene System.
    pub fn distances_from(
        &self,
        from: SystemId,
        passable: impl Fn(SystemId) -> bool,
    ) -> (Vec<f32>, Vec<Option<SystemId>>) {
        let count = self.systems.len();
        let mut best = vec![u64::MAX; count];
        let mut length = vec![f32::INFINITY; count];
        let mut previous: Vec<Option<SystemId>> = vec![None; count];
        let mut heap = BinaryHeap::new();
        best[from.index()] = 0;
        length[from.index()] = 0.0;
        heap.push(Reverse((0u64, from)));
        while let Some(Reverse((cost, current))) = heap.pop() {
            if cost > best[current.index()] {
                continue;
            }
            for neighbor in self.neighbors(current) {
                if !passable(neighbor.system) {
                    continue;
                }
                let lane = self.lane(neighbor.lane);
                let next_cost = cost + (lane.length * 16.0) as u64 + 1;
                if next_cost < best[neighbor.system.index()] {
                    best[neighbor.system.index()] = next_cost;
                    length[neighbor.system.index()] = length[current.index()] + lane.length;
                    previous[neighbor.system.index()] = Some(current);
                    heap.push(Reverse((next_cost, neighbor.system)));
                }
            }
        }
        (length, previous)
    }

    /// Weg aus dem Vorgängerfeld von [`Galaxy::distances_from`], ohne den Startpunkt.
    pub fn route_to(previous: &[Option<SystemId>], to: SystemId) -> Vec<SystemId> {
        let mut route = vec![to];
        let mut cursor = to;
        while let Some(prev) = previous[cursor.index()] {
            route.push(prev);
            cursor = prev;
        }
        route.pop();
        route.reverse();
        route
    }

    /// Länge eines Wegs entlang der Routen.
    pub fn path_length(&self, path: &[SystemId]) -> f32 {
        path.windows(2)
            .filter_map(|pair| self.lane_between(pair[0], pair[1]))
            .map(|lane| self.lane(lane).length)
            .sum()
    }
}

//! Prozedurale Erzeugung: Cluster, Systeme, Routen mit Engpässen, Planeten.

use super::{Cluster, Deposit, Galaxy, Lane, Planet, StarSystem, Vec2, dist, dist2};
use crate::data::{GameData, PlanetKindDef};
use crate::ids::{LaneId, PlanetId, SystemId};
use crate::rng::{Domain, Rng};
use std::collections::{BTreeSet, VecDeque};

const ROMAN: [&str; 8] = ["I", "II", "III", "IV", "V", "VI", "VII", "VIII"];

/// Erzeugt die Galaxie für einen Seed. Reine Funktion von Daten und Seed.
pub fn generate_galaxy(data: &GameData, seed: u64) -> Galaxy {
    let params = &data.galaxy;
    let mut rng = Rng::stream(seed, Domain::Galaxy, 0, 0);
    let mut name_rng = Rng::stream(seed, Domain::Names, 0, 0);

    // 1. Clusterzentren mit Mindestabstand. Gelingt die Platzierung nicht, wird der
    //    Abstand schrittweise gelockert, damit jede Parametrierung terminiert.
    let mut centers: Vec<Vec2> = Vec::new();
    let mut spacing = params.cluster_spacing;
    let reach = (params.galaxy_radius - params.cluster_radius).max(1.0);
    while centers.len() < params.clusters as usize {
        let mut placed = false;
        for _ in 0..400 {
            let candidate = point_in_disc(&mut rng, [0.0, 0.0], reach);
            if centers.iter().all(|c| dist(*c, candidate) >= spacing) {
                centers.push(candidate);
                placed = true;
                break;
            }
        }
        if !placed {
            spacing *= 0.93;
        }
    }

    // 2. Systeme je Cluster.
    let mut systems: Vec<StarSystem> = Vec::new();
    let mut clusters: Vec<Cluster> = Vec::new();
    let star_weights: Vec<f32> = params.stars.iter().map(|star| star.weight).collect();
    let mut used_names = BTreeSet::new();
    for (cluster_index, center) in centers.iter().enumerate() {
        let wanted =
            rng.range_inclusive(params.systems_per_cluster.0, params.systems_per_cluster.1);
        let mut members = Vec::new();
        let mut spacing = params.system_spacing;
        while members.len() < wanted as usize {
            let mut placed = false;
            for _ in 0..200 {
                let candidate = point_in_disc(&mut rng, *center, params.cluster_radius);
                if systems.iter().all(|s| dist(s.pos, candidate) >= spacing) {
                    let id = SystemId::from_index(systems.len());
                    systems.push(StarSystem {
                        id,
                        name: unique_name(data, &mut name_rng, &mut used_names),
                        pos: candidate,
                        cluster: cluster_index as u32,
                        star: rng.pick_weighted(&star_weights) as u32,
                        planets: Vec::new(),
                        centrality: 0.0,
                        chokepoint: false,
                    });
                    members.push(id);
                    placed = true;
                    break;
                }
            }
            if !placed {
                spacing *= 0.9;
            }
        }
        clusters.push(Cluster {
            center: *center,
            systems: members,
        });
    }

    // 3. Routen im Cluster: Der Relative-Nachbarschafts-Graph hält jeden Cluster
    //    zusammen; ein Teil der übrigen Gabriel-Kanten kommt je nach `lane_density` dazu.
    let mut edges: Vec<(SystemId, SystemId, bool)> = Vec::new();
    for cluster in &clusters {
        let members = &cluster.systems;
        for i in 0..members.len() {
            for j in (i + 1)..members.len() {
                let a = systems[members[i].index()].pos;
                let b = systems[members[j].index()].pos;
                let d2 = dist2(a, b);
                let mid = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
                let mut gabriel = true;
                let mut relative = true;
                for (k, other) in members.iter().enumerate() {
                    if k == i || k == j {
                        continue;
                    }
                    let p = systems[other.index()].pos;
                    if dist2(p, mid) < d2 * 0.25 {
                        gabriel = false;
                    }
                    if dist2(p, a) < d2 && dist2(p, b) < d2 {
                        relative = false;
                    }
                }
                if relative || (gabriel && rng.chance(params.lane_density as f64)) {
                    edges.push((members[i], members[j], false));
                }
            }
        }
    }

    // 4. Brücken zwischen Clustern: minimaler Spannbaum über die engsten Systempaare,
    //    dazu wenige Zusatzbrücken. Genau hier entstehen die Engpässe.
    let mut pairs: Vec<(f32, usize, usize, SystemId, SystemId)> = Vec::new();
    for a in 0..clusters.len() {
        for b in (a + 1)..clusters.len() {
            let mut best: Option<(f32, SystemId, SystemId)> = None;
            for sa in &clusters[a].systems {
                for sb in &clusters[b].systems {
                    let d = dist(systems[sa.index()].pos, systems[sb.index()].pos);
                    if best.is_none_or(|(best_d, _, _)| d < best_d) {
                        best = Some((d, *sa, *sb));
                    }
                }
            }
            if let Some((d, sa, sb)) = best {
                pairs.push((d, a, b, sa, sb));
            }
        }
    }
    pairs.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)).then(x.2.cmp(&y.2)));
    let mut parent: Vec<usize> = (0..clusters.len()).collect();
    let mut leftover = Vec::new();
    let mut longest_bridge = 0.0f32;
    for pair in &pairs {
        let (ra, rb) = (find(&mut parent, pair.1), find(&mut parent, pair.2));
        if ra != rb {
            parent[ra] = rb;
            edges.push((pair.3, pair.4, true));
            longest_bridge = longest_bridge.max(pair.0);
        } else {
            leftover.push(*pair);
        }
    }
    let mut extra = 0;
    for pair in &leftover {
        if extra >= params.extra_bridges {
            break;
        }
        // Keine Abkürzungen quer durch die Galaxie.
        if pair.0 <= longest_bridge * 1.25 {
            edges.push((pair.3, pair.4, true));
            extra += 1;
        }
    }

    let lanes: Vec<Lane> = edges
        .iter()
        .enumerate()
        .map(|(index, (a, b, bridge))| Lane {
            id: LaneId::from_index(index),
            a: *a,
            b: *b,
            length: dist(systems[a.index()].pos, systems[b.index()].pos),
            bridge: *bridge,
        })
        .collect();
    for lane in &lanes {
        if lane.bridge {
            systems[lane.a.index()].chokepoint = true;
            systems[lane.b.index()].chokepoint = true;
        }
    }

    // 5. Planeten.
    let kind_weights: Vec<f32> = data.planet_kinds.iter().map(|kind| kind.weight).collect();
    let mut planets: Vec<Planet> = Vec::new();
    for system in &mut systems {
        let count = rng.range_inclusive(params.planets_per_system.0, params.planets_per_system.1);
        for orbit in 0..count {
            let id = PlanetId::from_index(planets.len());
            let kind = rng.pick_weighted(&kind_weights);
            let name = format!(
                "{} {}",
                system.name,
                ROMAN[(orbit as usize).min(ROMAN.len() - 1)]
            );
            planets.push(roll_planet(
                data, id, system.id, name, kind, orbit, &mut rng,
            ));
            system.planets.push(id);
        }
    }

    let mut galaxy = Galaxy {
        clusters,
        systems,
        lanes,
        planets,
        adjacency: Vec::new(),
    };
    galaxy.rebuild_index();
    let centrality = betweenness(&galaxy);
    for (system, value) in galaxy.systems.iter_mut().zip(centrality) {
        system.centrality = value;
    }
    galaxy
}

/// Würfelt einen Planeten des angegebenen Typs.
pub fn roll_planet(
    data: &GameData,
    id: PlanetId,
    system: SystemId,
    name: String,
    kind_index: usize,
    orbit: u32,
    rng: &mut Rng,
) -> Planet {
    let kind: &PlanetKindDef = &data.planet_kinds[kind_index];
    let gravity = rng.range_f32(kind.gravity.0, kind.gravity.1);
    let temperature = rng.range_f32(kind.temperature.0, kind.temperature.1);
    let water = rng.range_f32(kind.water.0, kind.water.1);
    let atmosphere_weights: Vec<f32> = kind.atmospheres.iter().map(|(_, weight)| *weight).collect();
    let atmosphere = kind.atmospheres[rng.pick_weighted(&atmosphere_weights)].0;
    let habitability = habitability(
        atmosphere.habitability_factor(),
        temperature,
        gravity,
        water,
    );
    let base_capacity = rng.range_f32(kind.capacity.0, kind.capacity.1) as f64;
    let capacity = base_capacity * (0.15 + 0.85 * habitability as f64);
    let mut deposits = Vec::new();
    for def in &kind.deposits {
        // Beide Würfe immer ziehen, damit die Zahlenfolge nicht vom Ergebnis abhängt.
        let roll = rng.f32();
        let richness = rng.range_f32(def.richness.0, def.richness.1);
        if roll < def.chance {
            deposits.push(Deposit {
                good: def.good_id,
                richness,
                reserve: 1.0,
            });
        }
    }
    deposits.sort_by_key(|deposit| deposit.good);
    Planet {
        id,
        system,
        name,
        kind: kind_index as u32,
        orbit,
        gravity,
        atmosphere,
        temperature,
        water,
        habitability,
        capacity,
        population: 0.0,
        deposits,
        owner: None,
    }
}

/// Bewohnbarkeit 0..1 aus den lokalen Gegebenheiten.
pub fn habitability(atmosphere_factor: f32, temperature: f32, gravity: f32, water: f32) -> f32 {
    let temperature_factor = (-((temperature - 15.0) / 35.0).powi(2)).exp().max(0.1);
    let gravity_factor = (1.0 - (gravity - 1.0).abs() * 0.5).clamp(0.2, 1.0);
    let water_factor = 0.6 + 0.4 * water.clamp(0.0, 1.0);
    (atmosphere_factor * temperature_factor * gravity_factor * water_factor).clamp(0.02, 1.0)
}

fn point_in_disc(rng: &mut Rng, center: Vec2, radius: f32) -> Vec2 {
    let angle = rng.range_f32(0.0, std::f32::consts::TAU);
    let r = radius * rng.f32().sqrt();
    [center[0] + r * angle.cos(), center[1] + r * angle.sin()]
}

fn unique_name(data: &GameData, rng: &mut Rng, used: &mut BTreeSet<String>) -> String {
    let names = &data.names;
    for attempt in 0..64 {
        let mut name = names.prefixes[rng.range_usize(0, names.prefixes.len())].clone();
        if !names.middles.is_empty() && (attempt > 16 || rng.chance(0.4)) {
            name.push_str(&names.middles[rng.range_usize(0, names.middles.len())]);
        }
        name.push_str(&names.suffixes[rng.range_usize(0, names.suffixes.len())]);
        if used.insert(name.clone()) {
            return name;
        }
    }
    let mut index = used.len();
    loop {
        let name = format!("{}-{}", names.prefixes[index % names.prefixes.len()], index);
        if used.insert(name.clone()) {
            return name;
        }
        index += 1;
    }
}

fn find(parent: &mut [usize], mut node: usize) -> usize {
    while parent[node] != node {
        parent[node] = parent[parent[node]];
        node = parent[node];
    }
    node
}

/// Betweenness-Zentralität nach Brandes (ungewichtet), auf 0..1 normiert.
fn betweenness(galaxy: &Galaxy) -> Vec<f32> {
    let n = galaxy.systems.len();
    let mut centrality = vec![0.0f64; n];
    for source in 0..n {
        let mut stack = Vec::with_capacity(n);
        let mut predecessors: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut sigma = vec![0.0f64; n];
        let mut distance = vec![-1i64; n];
        sigma[source] = 1.0;
        distance[source] = 0;
        let mut queue = VecDeque::new();
        queue.push_back(source);
        while let Some(v) = queue.pop_front() {
            stack.push(v);
            for neighbor in galaxy.neighbors(SystemId::from_index(v)) {
                let w = neighbor.system.index();
                if distance[w] < 0 {
                    distance[w] = distance[v] + 1;
                    queue.push_back(w);
                }
                if distance[w] == distance[v] + 1 {
                    sigma[w] += sigma[v];
                    predecessors[w].push(v);
                }
            }
        }
        let mut delta = vec![0.0f64; n];
        while let Some(w) = stack.pop() {
            for v in &predecessors[w] {
                delta[*v] += sigma[*v] / sigma[w] * (1.0 + delta[w]);
            }
            if w != source {
                centrality[w] += delta[w];
            }
        }
    }
    let max = centrality.iter().copied().fold(0.0f64, f64::max);
    centrality
        .into_iter()
        .map(|value| if max > 0.0 { (value / max) as f32 } else { 0.0 })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> GameData {
        GameData::load_workspace().unwrap()
    }

    #[test]
    fn galaxy_is_connected_for_many_seeds() {
        let data = data();
        for seed in 0..40 {
            let galaxy = generate_galaxy(&data, seed);
            assert!(galaxy.is_connected(), "Seed {seed}: Galaxie zerfällt");
        }
    }

    #[test]
    fn bridges_are_the_only_links_between_clusters() {
        let data = data();
        for seed in 0..40 {
            let galaxy = generate_galaxy(&data, seed);
            let clusters = data.galaxy.clusters as usize;
            // Ohne Brücken zerfällt die Galaxie genau in ihre Cluster: Engpässe sind echt.
            assert_eq!(galaxy.components_without_bridges(), clusters, "Seed {seed}");
            let bridges = galaxy.lanes.iter().filter(|lane| lane.bridge).count();
            assert!(bridges >= clusters - 1, "Seed {seed}");
            assert!(
                bridges <= clusters - 1 + data.galaxy.extra_bridges as usize,
                "Seed {seed}"
            );
            for lane in &galaxy.lanes {
                let same = galaxy.system(lane.a).cluster == galaxy.system(lane.b).cluster;
                assert_eq!(lane.bridge, !same, "Seed {seed}");
                if lane.bridge {
                    assert!(galaxy.system(lane.a).chokepoint && galaxy.system(lane.b).chokepoint);
                }
            }
        }
    }

    #[test]
    fn size_is_finite_and_within_parameters() {
        let data = data();
        let params = &data.galaxy;
        for seed in 0..20 {
            let galaxy = generate_galaxy(&data, seed);
            let systems = galaxy.systems.len() as u32;
            assert!(systems >= params.clusters * params.systems_per_cluster.0);
            assert!(systems <= params.clusters * params.systems_per_cluster.1);
            for system in &galaxy.systems {
                let count = system.planets.len() as u32;
                assert!(
                    count >= params.planets_per_system.0 && count <= params.planets_per_system.1
                );
                assert!((0.0..=1.0).contains(&system.centrality));
            }
        }
    }

    #[test]
    fn planets_have_plausible_local_conditions() {
        let data = data();
        let galaxy = generate_galaxy(&data, 11);
        for planet in &galaxy.planets {
            let kind = &data.planet_kinds[planet.kind as usize];
            assert!(planet.gravity >= kind.gravity.0 && planet.gravity <= kind.gravity.1);
            assert!((0.02..=1.0).contains(&planet.habitability));
            assert!(planet.capacity > 0.0);
            assert!(planet.owner.is_none());
            for deposit in &planet.deposits {
                assert!(deposit.richness > 0.0);
            }
        }
        // Standorte unterscheiden sich wirklich.
        let habitable = galaxy
            .planets
            .iter()
            .filter(|p| p.habitability > 0.5)
            .count();
        let hostile = galaxy
            .planets
            .iter()
            .filter(|p| p.habitability < 0.2)
            .count();
        assert!(habitable > 0 && hostile > 0);
    }

    #[test]
    fn same_seed_same_galaxy_different_seed_differs() {
        let data = data();
        let a = crate::hash::hash_value(&generate_galaxy(&data, 5));
        let b = crate::hash::hash_value(&generate_galaxy(&data, 5));
        let c = crate::hash::hash_value(&generate_galaxy(&data, 6));
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn shortest_path_follows_lanes_and_respects_blocks() {
        let data = data();
        let galaxy = generate_galaxy(&data, 3);
        let from = galaxy.systems.first().unwrap().id;
        let to = galaxy.systems.last().unwrap().id;
        let path = galaxy.shortest_path(from, to, |_| true).expect("verbunden");
        assert_eq!(path.first(), Some(&from));
        assert_eq!(path.last(), Some(&to));
        for pair in path.windows(2) {
            assert!(galaxy.lane_between(pair[0], pair[1]).is_some());
        }
        // Sind alle Zwischensysteme gesperrt, bleibt nur eine direkte Route.
        let blocked = galaxy.shortest_path(from, to, |_| false);
        assert_eq!(blocked.is_some(), galaxy.lane_between(from, to).is_some());
    }

    #[test]
    fn habitability_prefers_temperate_breathable_worlds() {
        let garden = habitability(1.0, 15.0, 1.0, 0.6);
        let frozen = habitability(1.0, -120.0, 1.0, 0.6);
        let heavy = habitability(1.0, 15.0, 2.2, 0.6);
        let airless = habitability(0.15, 15.0, 1.0, 0.6);
        assert!(garden > frozen && garden > heavy && garden > airless);
    }
}

//! Galaktische Gemeinschaften und ihr Besitz an Planeten.

pub mod succession;

pub use succession::{Absorbed, Founding, Succession, SuccessionParams, Transfer, TransferTerms};

use crate::chronicle::{Chronicle, ChronicleKind};
use crate::data::{ColonizationBalance, GameData, Rgb};
use crate::economy::{Economy, Money, seed_colony, to_money};
use crate::galaxy::{Galaxy, dist, roll_planet};
use crate::ids::{PlanetId, PolityId, SystemId};
use crate::rng::{Domain, Rng};
use bevy_ecs::prelude::Resource;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Polity {
    pub id: PolityId,
    pub name: String,
    pub short: String,
    pub motto: String,
    pub color: Rgb,
    /// Hauptwelt; `None`, wenn die Gemeinschaft keinen Planeten mehr hält.
    pub capital: Option<PlanetId>,
    pub alive: bool,
    /// Frühester Tag der nächsten Koloniegründung.
    pub next_claim_day: u64,
}

#[derive(Resource, Clone, Debug, Default, Serialize, Deserialize)]
pub struct Polities {
    pub list: Vec<Polity>,
}

impl Polities {
    #[inline]
    pub fn get(&self, id: PolityId) -> &Polity {
        &self.list[id.index()]
    }

    #[inline]
    pub fn get_mut(&mut self, id: PolityId) -> &mut Polity {
        &mut self.list[id.index()]
    }

    pub fn alive(&self) -> impl Iterator<Item = &Polity> + '_ {
        self.list.iter().filter(|polity| polity.alive)
    }
}

/// Gründet die Gemeinschaften: weit auseinanderliegende Heimatcluster, je eine
/// bewohnbare Heimatwelt. Die übrigen Cluster bleiben frei und werden umkämpft.
pub fn found_polities(data: &GameData, galaxy: &mut Galaxy, seed: u64) -> Polities {
    let mut rng = Rng::stream(seed, Domain::Polity, 0, 0);
    let wanted = data.galaxy.polities as usize;

    // Heimatcluster: zuerst der randständigste, dann jeweils der, der von allen
    // gewählten am weitesten entfernt ist.
    let centers: Vec<_> = galaxy
        .clusters
        .iter()
        .map(|cluster| cluster.center)
        .collect();
    let mut chosen: Vec<usize> = Vec::new();
    while chosen.len() < wanted.min(centers.len()) {
        let mut best: Option<(usize, f32)> = None;
        for (index, center) in centers.iter().enumerate() {
            if chosen.contains(&index) {
                continue;
            }
            let score = if chosen.is_empty() {
                dist(*center, [0.0, 0.0])
            } else {
                chosen
                    .iter()
                    .map(|other| dist(*center, centers[*other]))
                    .fold(f32::MAX, f32::min)
            };
            if best.is_none_or(|(_, best_score)| score > best_score) {
                best = Some((index, score));
            }
        }
        chosen.push(best.expect("genug Cluster laut Datenprüfung").0);
    }
    // Welche Vorlage welchen Cluster bekommt, entscheidet der Seed.
    rng.shuffle(&mut chosen);
    let mut templates: Vec<usize> = (0..data.polities.len()).collect();
    rng.shuffle(&mut templates);

    let home_kinds: Vec<usize> = data
        .planet_kinds
        .iter()
        .enumerate()
        .filter(|(_, kind)| kind.home)
        .map(|(index, _)| index)
        .collect();

    let mut list = Vec::new();
    for (slot, cluster_index) in chosen.iter().enumerate() {
        let id = PolityId::from_index(slot);
        let template = &data.polities[templates[slot]];
        let home_system = pick_home_system(galaxy, *cluster_index);

        // Die innerste Welt des Heimatsystems wird zur Heimatwelt umgewürfelt.
        let planet_id = galaxy.system(home_system).planets[0];
        let kind = home_kinds[rng.range_usize(0, home_kinds.len())];
        let old = galaxy.planet(planet_id).clone();
        let mut home = roll_planet(
            data,
            planet_id,
            home_system,
            old.name,
            kind,
            old.orbit,
            &mut rng,
        );
        home.population = data.galaxy.home_population.min(home.capacity * 0.8);
        home.owner = Some(id);
        *galaxy.planet_mut(planet_id) = home;

        list.push(Polity {
            id,
            name: template.name.clone(),
            short: template.short.clone(),
            motto: template.motto.clone(),
            color: template.color,
            capital: Some(planet_id),
            alive: true,
            next_claim_day: 0,
        });
    }
    Polities { list }
}

/// Heimatsystem eines Clusters: möglichst zentral und kein Brückenkopf.
fn pick_home_system(galaxy: &Galaxy, cluster_index: usize) -> SystemId {
    let cluster = &galaxy.clusters[cluster_index];
    let mut best: Option<(SystemId, f32)> = None;
    for id in &cluster.systems {
        let system = galaxy.system(*id);
        let penalty = if system.chokepoint { 10_000.0 } else { 0.0 };
        let score = dist(system.pos, cluster.center) + penalty;
        if best.is_none_or(|(_, best_score)| score < best_score) {
            best = Some((*id, score));
        }
    }
    best.expect("Cluster ohne Systeme").0
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClaimError {
    UnknownPlanet,
    PolityGone,
    AlreadyOwned,
    OutOfReach,
    CoolingDown { ready_on_day: u64 },
    NoSettlers,
    NoFunds { needed: Money },
}

impl fmt::Display for ClaimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClaimError::UnknownPlanet => write!(f, "Planet unbekannt"),
            ClaimError::PolityGone => write!(f, "Gemeinschaft besteht nicht mehr"),
            ClaimError::AlreadyOwned => write!(f, "Planet ist bereits besiedelt"),
            ClaimError::OutOfReach => {
                write!(
                    f,
                    "außer Reichweite: nur im eigenen oder in einem Nachbarsystem"
                )
            }
            ClaimError::CoolingDown { ready_on_day } => write!(
                f,
                "nächste Kolonie frühestens am {}",
                crate::clock::Date::from_day(*ready_on_day)
            ),
            ClaimError::NoSettlers => write!(f, "Hauptwelt kann keine Siedler abgeben"),
            ClaimError::NoFunds { needed } => write!(
                f,
                "Staatskasse zu leer: die Gründung kostet {:.0} Credits",
                crate::economy::to_credits(*needed)
            ),
        }
    }
}

/// Prüft, ob `polity` den Planeten am Tag `day` besiedeln darf.
pub fn can_claim(
    galaxy: &Galaxy,
    polities: &Polities,
    balance: &ColonizationBalance,
    treasury: Money,
    polity: PolityId,
    planet: PlanetId,
    day: u64,
) -> Result<(), ClaimError> {
    let Some(target) = galaxy.planets.get(planet.index()) else {
        return Err(ClaimError::UnknownPlanet);
    };
    let Some(owner) = polities.list.get(polity.index()).filter(|p| p.alive) else {
        return Err(ClaimError::PolityGone);
    };
    if target.owner.is_some() {
        return Err(ClaimError::AlreadyOwned);
    }
    let in_reach = galaxy.owns_planet_in(polity, target.system)
        || galaxy
            .neighbors(target.system)
            .iter()
            .any(|neighbor| galaxy.owns_planet_in(polity, neighbor.system));
    if !in_reach {
        return Err(ClaimError::OutOfReach);
    }
    if owner.next_claim_day > day {
        return Err(ClaimError::CoolingDown {
            ready_on_day: owner.next_claim_day,
        });
    }
    let capital_population = owner
        .capital
        .map(|id| galaxy.planet(id).population)
        .unwrap_or(0.0);
    if capital_population < balance.min_capital_population + balance.settlers {
        return Err(ClaimError::NoSettlers);
    }
    let needed = to_money(balance.cost);
    if treasury < needed {
        return Err(ClaimError::NoFunds { needed });
    }
    Ok(())
}

/// Gründet eine Kolonie. Die Siedler ziehen von der Hauptwelt um und nehmen ihren Anteil
/// am Geld mit; der Staat bezahlt die Erstausstattung.
pub fn claim_planet(
    galaxy: &mut Galaxy,
    polities: &mut Polities,
    economy: &mut Economy,
    chronicle: &mut Chronicle,
    data: &GameData,
    polity: PolityId,
    planet: PlanetId,
    tick: u64,
) -> Result<(), ClaimError> {
    let balance = &data.balance.colonization;
    let day = tick / crate::clock::TICKS_PER_DAY;
    can_claim(
        galaxy,
        polities,
        balance,
        economy.treasury(polity),
        polity,
        planet,
        day,
    )?;
    let capital = polities.get(polity).capital.expect("von can_claim geprüft");
    let capital_population = galaxy.planet(capital).population;
    galaxy.planet_mut(capital).population -= balance.settlers;
    let target = galaxy.planet_mut(planet);
    target.owner = Some(polity);
    target.population = balance.settlers;
    let (system, planet_name) = (target.system, target.name.clone());
    seed_colony(
        data,
        galaxy,
        economy,
        planet,
        capital,
        balance.settlers,
        capital_population,
        day,
    );
    let owner = polities.get_mut(polity);
    owner.next_claim_day = day + balance.cooldown_days;
    chronicle.push(
        tick,
        ChronicleKind::Colony,
        Some(polity),
        Some(system),
        format!("{} gründet eine Kolonie auf {}.", owner.name, planet_name),
    );
    Ok(())
}

/// Alle Planeten, die `polity` jetzt besiedeln dürfte.
pub fn claimable_planets(
    galaxy: &Galaxy,
    polities: &Polities,
    balance: &ColonizationBalance,
    treasury: Money,
    polity: PolityId,
    day: u64,
) -> Vec<PlanetId> {
    galaxy
        .planets
        .iter()
        .filter(|planet| planet.owner.is_none())
        .filter(|planet| {
            can_claim(galaxy, polities, balance, treasury, polity, planet.id, day).is_ok()
        })
        .map(|planet| planet.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::galaxy::generate_galaxy;

    fn setup(seed: u64) -> (GameData, Galaxy, Polities, Economy) {
        let data = GameData::load_workspace().unwrap();
        let mut galaxy = generate_galaxy(&data, seed);
        let polities = found_polities(&data, &mut galaxy, seed);
        let economy = crate::economy::setup_economy(&data, &galaxy, &polities);
        (data, galaxy, polities, economy)
    }

    #[test]
    fn every_polity_gets_a_distinct_habitable_home() {
        for seed in 0..20 {
            let (data, galaxy, polities, _) = setup(seed);
            assert_eq!(polities.list.len(), data.galaxy.polities as usize);
            let mut clusters = Vec::new();
            for polity in &polities.list {
                let home = galaxy.planet(polity.capital.unwrap());
                assert_eq!(home.owner, Some(polity.id));
                assert!(home.population > 1000.0, "Seed {seed}");
                assert!(
                    home.habitability > 0.4,
                    "Seed {seed}: {}",
                    home.habitability
                );
                clusters.push(galaxy.system(home.system).cluster);
            }
            clusters.sort_unstable();
            clusters.dedup();
            assert_eq!(
                clusters.len(),
                polities.list.len(),
                "Seed {seed}: Heimatcluster doppelt"
            );
        }
    }

    #[test]
    fn claims_need_reach_funds_and_respect_cooldown() {
        let (data, mut galaxy, mut polities, mut economy) = setup(1);
        let mut chronicle = Chronicle::default();
        let balance = &data.balance.colonization;
        let me = PolityId(0);
        let rich = to_money(1e9);
        let options = claimable_planets(&galaxy, &polities, balance, rich, me, 0);
        assert!(!options.is_empty());
        let home_system = galaxy.planet(polities.get(me).capital.unwrap()).system;
        let far = galaxy
            .planets
            .iter()
            .find(|planet| {
                planet.owner.is_none() && galaxy.hops_from(home_system)[planet.system.index()] > 1
            })
            .unwrap()
            .id;
        let verdict = |galaxy: &Galaxy, polities: &Polities, treasury, planet, day| {
            can_claim(galaxy, polities, balance, treasury, me, planet, day)
        };
        assert_eq!(
            verdict(&galaxy, &polities, rich, far, 0),
            Err(ClaimError::OutOfReach)
        );
        assert!(matches!(
            verdict(&galaxy, &polities, 0, options[0], 0),
            Err(ClaimError::NoFunds { .. })
        ));

        let before = galaxy.population_of(me);
        let money_before = economy.money_in_accounts();
        let target = options[0];
        claim_planet(
            &mut galaxy,
            &mut polities,
            &mut economy,
            &mut chronicle,
            &data,
            me,
            target,
            0,
        )
        .unwrap();
        assert_eq!(galaxy.planet(target).owner, Some(me));
        // Siedler ziehen um, niemand entsteht aus dem Nichts; ebenso das Geld.
        assert!((galaxy.population_of(me) - before).abs() < 1e-6);
        assert_eq!(economy.money_in_accounts(), money_before);
        assert!(economy.planet(target).total_capital() > 0.0);
        assert_eq!(chronicle.entries.len(), 1);
        assert_eq!(
            verdict(&galaxy, &polities, rich, target, 0),
            Err(ClaimError::AlreadyOwned)
        );
        if let Some(next) = options.get(1) {
            assert!(matches!(
                verdict(&galaxy, &polities, rich, *next, 5),
                Err(ClaimError::CoolingDown { .. })
            ));
            assert!(verdict(&galaxy, &polities, rich, *next, balance.cooldown_days).is_ok());
        }
    }
}

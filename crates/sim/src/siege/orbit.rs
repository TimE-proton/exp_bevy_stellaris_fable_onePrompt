//! Der Orbit einer Belagerung: was die Flotten des Belagerers über einem Planeten
//! aufbieten und was ein Tag ihre Schiffe kostet.
//!
//! Die einzige Datei des Moduls, die Flotten, Schiffe und Forschung kennt. Alles Übrige
//! rechnet mit einfachen Daten ([`OrbitView`], [`Squadron`], [`Standing`]); ändert sich die
//! Flottenebene, ändert sich nur diese Datei.

use super::step::{DayEffects, hull_shares};
use super::{OrbitView, PlanetDefense, Sieges, TechView};
use crate::data::{GameData, ShipClass, SiegeBalance, WeaponKind};
use crate::diplomacy::Diplomacy;
use crate::fleet::{Fleet, Fleets, Ship, update_blockades};
use crate::galaxy::Galaxy;
use crate::ids::{FleetId, PlanetId, PolityId, SystemId};
use crate::tech::{Research, TechEffects};
use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;
use std::collections::BTreeMap;

/// Unter diesem Rest gilt ein Rumpf als zerstört.
const WRECKED: f32 = 1e-6;

/// Systemparameter: Flotten und Forschung für die Systeme der Belagerung.
#[derive(SystemParam)]
pub struct OrbitAccess<'w> {
    fleets: ResMut<'w, Fleets>,
    research: Res<'w, Research>,
}

impl OrbitAccess<'_> {
    pub fn orbit(&mut self) -> Orbit<'_> {
        Orbit::new(&mut self.fleets, &self.research)
    }
}

/// Flotten und Forschung, wie die Belagerung sie liest und verändert.
pub struct Orbit<'a> {
    fleets: &'a mut Fleets,
    research: &'a Research,
}

/// Eine bestehende Flotte, soweit die Belagerung sie prüft.
#[derive(Clone, Debug, PartialEq)]
pub struct Standing {
    pub owner: PolityId,
    pub name: String,
    pub system: SystemId,
    /// Liegt im System und ist nicht auf einer Sprungroute.
    pub docked: bool,
    pub in_battle: bool,
}

/// Was die Flotten eines Belagerers heute über einem Planeten aufbieten.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Squadron {
    /// Die belagernden Flotten in Nummernfolge: dem Planeten zugeordnet, im System,
    /// einsatzbereit und mit Treibstoff in jedem Tank.
    pub fleets: Vec<FleetId>,
    pub ships: usize,
    /// Die Lage, wie der Tagesrechner sie braucht.
    pub view: OrbitView,
    /// Standfestigkeit: Summe aus Klassengewicht (`gun_weight`) mal Rumpf, Schild und
    /// Panzerung unter der Technik des Belagerers.
    pub toughness: f64,
    /// Knappster Tank der Flotte, die am längsten bleiben kann (0..1).
    pub fuel: f64,
    /// Plätze für Truppen an Bord und Nachschub an Bord in Verbandstagen.
    pub berths: f64,
    pub supply_aboard: f64,
}

/// Was ein Tag unter Stationen und Geschützen die Flotten gekostet hat.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Toll {
    pub ships_lost: u32,
    /// Verbände, die mit ihren Schiffen verloren gingen.
    pub troops_lost: f64,
}

/// Feuerkraft eines Schiffs gegen Stationen und Boden: die Leistung seiner Waffen ohne
/// Punktverteidigung, gewichtet mit dem Zustand des Schiffs, wie ihn die Flottenebene
/// rechnet (Rumpf, gemindert um ausgefallene Module). Waffen, die Munition verschießen,
/// schweigen mit leerem Magazin.
fn firepower(data: &GameData, ship: &Ship) -> f64 {
    let power: f64 = data.designs[ship.design as usize]
        .modules
        .iter()
        .filter_map(|mounted| data.modules[mounted.module_id as usize].weapon())
        .filter(|weapon| weapon.kind != WeaponKind::PointDefense)
        .filter(|weapon| weapon.ammo <= 0.0 || ship.ammo > 0.0)
        .map(|weapon| weapon.dps() as f64)
        .sum();
    ship.condition(data) as f64 * power
}

/// Fassungsvermögen der Magazine einer Flotte.
fn magazines_of(data: &GameData, fleet: &Fleet) -> f64 {
    fleet
        .ships
        .iter()
        .map(|ship| data.designs[ship.design as usize].stats.ammo as f64)
        .sum()
}

/// Rumpf, Schild und Panzerung eines Entwurfs unter der Technik seiner Gemeinschaft.
fn toughness(data: &GameData, effects: &TechEffects, ship: &Ship) -> f64 {
    let stats = &data.designs[ship.design as usize].stats;
    stats.hull as f64 * effects.hull
        + stats.shield as f64 * effects.shields
        + stats.armor.iter().sum::<f32>() as f64 * effects.armor
}

fn class_of(data: &GameData, ship: &Ship) -> ShipClass {
    data.designs[ship.design as usize].stats.class
}

/// Verbände, die eine Flotte tragen kann: Plätze je Rumpfsegment ihrer Schiffe.
fn berths_of(data: &GameData, fleet: &Fleet) -> f64 {
    if !fleet.alive {
        return 0.0;
    }
    let segments: f64 = fleet
        .ships
        .iter()
        .map(|ship| data.hulls[data.designs[ship.design as usize].hull_id as usize].segments as f64)
        .sum();
    data.balance.siege.berth_per_segment * segments
}

/// Kann die Flotte den Planeten im System `system` für `attacker` belagern? Sie liegt
/// dort, ist einsatzbereit (weder im Gefecht noch beim Sammeln) und hat Treibstoff in
/// jedem Tank: Ohne ihn hält sie keine Position im Orbit.
fn besieges(fleet: &Fleet, system: SystemId, attacker: PolityId) -> bool {
    fleet.owner == attacker
        && fleet.system == system
        && fleet.is_docked()
        && fleet.is_ready()
        && fleet.min_fuel() > 0.0
}

/// Technik einer Gemeinschaft, wie der Tagesrechner sie sieht.
fn tech_view(research: &Research, polity: PolityId, balance: &SiegeBalance) -> TechView {
    let effects = research.effects(polity);
    TechView {
        ground_attack: effects.ground_attack - 1.0,
        ground_defense: effects.ground_defense - 1.0,
        weapon_damage: effects.weapon_damage - 1.0,
        shields: effects.shields - 1.0,
        hull: effects.hull - 1.0,
        armor: effects.armor - 1.0,
        gates: balance.gate_ids.map(|tech| research.knows(polity, tech)),
    }
}

/// Führt Zuordnungen und eingeschiffte Truppen den Flotten nach: Eine Flotte, die nicht
/// mehr besteht, belagert nichts und trägt nichts, und was an Bord keinen Platz mehr hat,
/// ist mit seinen Schiffen verloren. Gibt die verlorenen Verbände je Gemeinschaft zurück.
fn settle(fleets: &Fleets, data: &GameData, sieges: &mut Sieges) -> Vec<(PolityId, f64)> {
    let alive = |id: FleetId| fleets.get(id).is_some_and(|fleet| fleet.alive);
    sieges.assign.retain(|fleet, _| alive(*fleet));
    let mut lost = Vec::new();
    sieges.embarked.retain(|id, force| {
        let Some(fleet) = fleets.get(*id) else {
            return false;
        };
        let room = berths_of(data, fleet);
        if force.troops > room {
            lost.push((fleet.owner, force.troops - room));
            // Der Nachschub der Verlorenen geht mit unter.
            force.supply *= room / force.troops;
            force.troops = room;
        }
        fleet.alive && (force.troops > 0.0 || force.supply > 0.0)
    });
    lost
}

/// Flotten, die `planet` heute für `attacker` belagern, in Nummernfolge.
fn besiegers(
    fleets: &Fleets,
    assign: &BTreeMap<FleetId, PlanetId>,
    planet: PlanetId,
    system: SystemId,
    attacker: PolityId,
) -> Vec<FleetId> {
    assign
        .iter()
        .filter(|(_, target)| **target == planet)
        .filter(|(fleet, _)| {
            fleets
                .get(**fleet)
                .is_some_and(|fleet| besieges(fleet, system, attacker))
        })
        .map(|(fleet, _)| *fleet)
        .collect()
}

/// Macht eine dem Belagerer feindliche Flotte ihm den Orbit streitig? Wer sich nach einem
/// Rückzug sammelt, hält keinen Orbit; so rechnet auch die Blockade.
fn contested(fleets: &Fleets, diplomacy: &Diplomacy, system: SystemId, attacker: PolityId) -> bool {
    fleets
        .docked_in(system)
        .any(|fleet| diplomacy.at_war(fleet.owner, attacker) && !fleet.is_regrouping())
}

/// Orbitalhoheit über `planet`: Eine zugeordnete Flotte des Belagerers belagert, und keine
/// ihm feindliche Flotte steht im System. Nur lesend, auch für Lesemodell und KI.
pub fn control(
    fleets: &Fleets,
    diplomacy: &Diplomacy,
    assign: &BTreeMap<FleetId, PlanetId>,
    planet: PlanetId,
    system: SystemId,
    attacker: PolityId,
) -> bool {
    !contested(fleets, diplomacy, system, attacker)
        && !besiegers(fleets, assign, planet, system, attacker).is_empty()
}

/// Die Lage im Orbit von `planet` aus den Flotten und dem Forschungsstand: Feuerkraft und
/// Magazine der belagernden Schiffe, Truppen an Bord, Technik beider Seiten. Nur lesend:
/// Tagesschritt, Lesemodell, KI und Oberfläche bekommen dieselben Zahlen.
pub fn squadron(
    fleets: &Fleets,
    research: &Research,
    data: &GameData,
    diplomacy: &Diplomacy,
    sieges: &Sieges,
    planet: PlanetId,
    system: SystemId,
    attacker: PolityId,
    defender: PolityId,
) -> Squadron {
    let balance = &data.balance.siege;
    let besieging = besiegers(fleets, &sieges.assign, planet, system, attacker);
    let effects = research.effects(attacker);
    let mut squadron = Squadron {
        view: OrbitView {
            control: !besieging.is_empty() && !contested(fleets, diplomacy, system, attacker),
            att: tech_view(research, attacker, balance),
            def: tech_view(research, defender, balance),
            ..Default::default()
        },
        ..Default::default()
    };
    let (mut rounds, mut magazines) = (0.0, 0.0);
    for id in &besieging {
        let fleet = &fleets.list[id.index()];
        for ship in &fleet.ships {
            squadron.view.firepower += firepower(data, ship);
            squadron.toughness +=
                balance.gun_weight[class_of(data, ship).index()] * toughness(data, &effects, ship);
        }
        // Füllung der Magazine, wie die Flotte sie selbst nennt, gewichtet mit dem
        // Fassungsvermögen.
        let capacity = magazines_of(data, fleet);
        rounds += fleet.supply(data).0 as f64 * capacity;
        magazines += capacity;
        squadron.ships += fleet.ships.len();
        squadron.fuel = squadron.fuel.max(fleet.min_fuel() as f64);
        squadron.berths += berths_of(data, fleet);
        if let Some(force) = sieges.embarked.get(id) {
            squadron.view.embarked += force.troops;
            squadron.supply_aboard += force.supply;
        }
    }
    // Schiffe ohne Magazin brauchen keine Munition: Dann fehlt auch keine.
    squadron.view.ammo = if magazines > 0.0 {
        rounds / magazines
    } else {
        1.0
    };
    squadron.fleets = besieging;
    squadron
}

impl<'a> Orbit<'a> {
    pub fn new(fleets: &'a mut Fleets, research: &'a Research) -> Self {
        Self { fleets, research }
    }

    /// Die Flotte, falls sie besteht.
    pub fn standing(&self, fleet: FleetId) -> Option<Standing> {
        let fleet = self.fleets.get(fleet).filter(|fleet| fleet.alive)?;
        Some(Standing {
            owner: fleet.owner,
            name: fleet.name.clone(),
            system: fleet.system,
            docked: fleet.is_docked(),
            in_battle: fleet.in_battle,
        })
    }

    /// Die Flotte, wenn sie besteht und `polity` gehört; sonst der Grund, mit dem auch
    /// die Befehle der Flottenebene ablehnen.
    pub fn own(&self, polity: PolityId, fleet: FleetId) -> Result<Standing, String> {
        if self.fleets.get(fleet).is_none() {
            return Err("Flotte unbekannt".into());
        }
        self.standing(fleet)
            .filter(|fleet| fleet.owner == polity)
            .ok_or_else(|| {
                "Flotte gehört einer anderen Gemeinschaft oder besteht nicht mehr".to_string()
            })
    }

    /// Verbände, die die Flotte tragen kann; null, wenn sie nicht besteht.
    pub fn berths(&self, data: &GameData, fleet: FleetId) -> f64 {
        self.fleets
            .get(fleet)
            .map_or(0.0, |fleet| berths_of(data, fleet))
    }

    /// Flotten, die `planet` heute für `attacker` belagern, in Nummernfolge.
    pub fn besiegers(
        &self,
        assign: &BTreeMap<FleetId, PlanetId>,
        planet: PlanetId,
        system: SystemId,
        attacker: PolityId,
    ) -> Vec<FleetId> {
        besiegers(self.fleets, assign, planet, system, attacker)
    }

    /// Orbitalhoheit über `planet` (siehe [`control`]).
    pub fn control(
        &self,
        diplomacy: &Diplomacy,
        assign: &BTreeMap<FleetId, PlanetId>,
        planet: PlanetId,
        system: SystemId,
        attacker: PolityId,
    ) -> bool {
        control(self.fleets, diplomacy, assign, planet, system, attacker)
    }

    /// Die Lage im Orbit von `planet` (siehe [`squadron`]).
    pub fn squadron(
        &self,
        data: &GameData,
        diplomacy: &Diplomacy,
        sieges: &Sieges,
        planet: PlanetId,
        system: SystemId,
        attacker: PolityId,
        defender: PolityId,
    ) -> Squadron {
        squadron(
            self.fleets,
            self.research,
            data,
            diplomacy,
            sieges,
            planet,
            system,
            attacker,
            defender,
        )
    }

    /// Wendet die Wirkung eines Tages auf die Schiffe der belagernden Flotten an: Rumpf
    /// nach dem Gewicht der Klasse, Munition und Treibstoff als Anteil von Magazin und
    /// Tank. Zerstörte Schiffe verlassen ihre Flotte; Truppen, für die danach kein Platz
    /// mehr an Bord ist, sind mit ihnen verloren.
    pub fn apply(
        &mut self,
        data: &GameData,
        sieges: &mut Sieges,
        fleets: &[FleetId],
        attacker: PolityId,
        effects: &DayEffects,
    ) -> Toll {
        let mut toll = Toll::default();
        if fleets.is_empty() || (effects.hull <= 0.0 && effects.ammo <= 0.0 && effects.fuel <= 0.0)
        {
            return toll;
        }
        let tech = self.research.effects(attacker);
        let ships: Vec<(ShipClass, f64)> = fleets
            .iter()
            .flat_map(|id| &self.fleets.list[id.index()].ships)
            .map(|ship| (class_of(data, ship), toughness(data, &tech, ship)))
            .collect();
        let mut shares = hull_shares(effects.hull, &ships, &data.balance.siege).into_iter();
        for id in fleets {
            let fleet = &mut self.fleets.list[id.index()];
            for ship in &mut fleet.ships {
                let share = shares.next().unwrap_or(0.0) as f32;
                ship.hull = (ship.hull - share).max(0.0);
                ship.ammo = (ship.ammo - effects.ammo as f32).max(0.0);
                ship.fuel = (ship.fuel - effects.fuel as f32).max(0.0);
            }
            let before = fleet.ships.len();
            fleet.ships.retain(|ship| ship.hull > WRECKED);
            toll.ships_lost += (before - fleet.ships.len()) as u32;
        }
        // Stärke und Bestand der Flotten folgen den Schiffen.
        self.fleets.refresh(data);
        toll.troops_lost = settle(self.fleets, data, sieges)
            .into_iter()
            .map(|(_, lost)| lost)
            .sum();
        toll
    }

    /// Führt Zuordnungen und eingeschiffte Truppen den Flotten nach (siehe [`settle`]).
    /// Gibt die verlorenen Verbände je Gemeinschaft zurück.
    pub fn settle(&self, data: &GameData, sieges: &mut Sieges) -> Vec<(PolityId, f64)> {
        settle(self.fleets, data, sieges)
    }

    /// Zwei Flotten wurden vereinigt: Truppen und Nachschub von `from` fahren mit `into`
    /// weiter; belagerte `from` einen Planeten und `into` keinen, übernimmt `into` das Ziel.
    pub fn merged(&self, sieges: &mut Sieges, from: FleetId, into: FleetId) {
        if let Some(force) = sieges.embarked.remove(&from) {
            let aboard = sieges.embarked.entry(into).or_default();
            aboard.troops += force.troops;
            aboard.supply += force.supply;
        }
        if let Some(planet) = sieges.assign.remove(&from) {
            sieges.assign.entry(into).or_insert(planet);
        }
    }

    /// Aus `parent` wurde die Flotte `child` gelöst: Truppen, für die `parent` keinen
    /// Platz mehr hat, fahren mit ihrem Nachschub auf den abgelösten Schiffen weiter.
    pub fn split(&self, data: &GameData, sieges: &mut Sieges, parent: FleetId, child: FleetId) {
        let room = self.berths(data, parent);
        let Some(force) = sieges.embarked.get_mut(&parent) else {
            return;
        };
        if force.troops <= room {
            return;
        }
        let moved = (force.troops - room).min(self.berths(data, child));
        let supply = force.supply * moved / force.troops;
        force.troops -= moved;
        force.supply -= supply;
        let aboard = sieges.embarked.entry(child).or_default();
        aboard.troops += moved;
        aboard.supply += supply;
    }

    /// Einspruch der Stationen: Wo eine Welt des Systembesitzers noch eine Station hat,
    /// steht keine Blockade. Blockade und Einschließung brauchen beide den freien Orbit.
    pub fn veto_blockades(&mut self, galaxy: &Galaxy, defenses: &[PlanetDefense]) {
        if self.fleets.blockaded.is_empty() {
            return;
        }
        self.fleets.blockaded.retain(|system| {
            let Some(owner) = galaxy.system_owner(*system) else {
                return true;
            };
            !galaxy.planets_in(*system).any(|planet| {
                planet.owner == Some(owner)
                    && defenses
                        .get(planet.id.index())
                        .is_some_and(|defense| defense.stations > 0.0)
            })
        });
    }

    /// Bestimmt die Blockaden neu, wie es die Flotten jeden Tag tun, samt dem Einspruch
    /// der Stationen. Nötig, wenn ein Planet mitten am Tag den Besitzer wechselt.
    pub fn refresh_blockades(
        &mut self,
        galaxy: &Galaxy,
        diplomacy: &Diplomacy,
        defenses: &[PlanetDefense],
    ) {
        update_blockades(galaxy, self.fleets, diplomacy);
        self.veto_blockades(galaxy, defenses);
    }

    /// Ist das System für den Handel gesperrt?
    pub fn is_blockaded(&self, system: SystemId) -> bool {
        self.fleets.is_blockaded(system)
    }
}

/// Invarianten zwischen Belagerung und Flotten: Zuordnungen zeigen auf bestehende
/// Flotten, und eingeschiffte Truppen haben Platz an Bord ihrer Flotte.
pub(super) fn check(world: &World) -> Result<(), String> {
    let fleets = world.resource::<Fleets>();
    let sieges = world.resource::<Sieges>();
    for fleet in sieges.assign.keys() {
        if !fleets.get(*fleet).is_some_and(|fleet| fleet.alive) {
            return Err(format!("Flotte {fleet} belagert, besteht aber nicht mehr"));
        }
    }
    for (id, force) in &sieges.embarked {
        let Some(fleet) = fleets.get(*id).filter(|fleet| fleet.alive) else {
            return Err(format!(
                "Truppen an Bord der Flotte {id}, die nicht mehr besteht"
            ));
        };
        // Plätze nennt erst der Entwurf; ohne Spieldaten in der Welt bleibt es beim Bestand.
        let Some(data) = world.get_resource::<GameData>() else {
            continue;
        };
        let room = berths_of(data, fleet);
        if force.troops > room + 1e-9 {
            return Err(format!(
                "{}: {:.2} Verbände an Bord bei {:.2} Plätzen",
                fleet.name, force.troops, room
            ));
        }
    }
    Ok(())
}

/// Von Hand gestellte Lagen für die Tests des Moduls. Sie prüfen Tagesschritt und
/// Eroberung selbst; durch Befehle gespielte Partien stehen in `tests/siege.rs`. Was die
/// Tests an Flotten stellen und lesen, läuft über diese Hilfen: Auch dort kennt nur diese
/// Datei die Flottenebene.
#[cfg(test)]
pub(super) mod testing {
    use super::*;
    use crate::clock::TICKS_PER_DAY;
    use crate::fleet::Stance;
    use crate::siege::{Siege, resolve_conquests, run_day, tidy};
    use crate::state::{SimConfig, SimState};

    pub const ME: PolityId = PolityId(0);
    pub const FOE: PolityId = PolityId(1);

    /// Ein Schiff, soweit die Tests es prüfen.
    pub struct ShipState {
        pub hull: f32,
        pub ammo: f32,
        pub fuel: f32,
    }

    /// Löst eine Flotte auf, ohne dass die Belagerung davon erfährt.
    pub fn disband(state: &mut SimState, fleet: usize) {
        state.fleets.list[fleet].alive = false;
    }

    /// Eine neue Partie, in der die Flotte der Gemeinschaft 0 im Krieg über einer Welt der
    /// Gemeinschaft 1 liegt und ihr zugeordnet ist; deren Flotte steht ein System weiter.
    pub struct Scene {
        pub data: GameData,
        pub state: SimState,
        pub planet: PlanetId,
        pub system: SystemId,
        pub day: u64,
    }

    impl Scene {
        /// Belagert wird die Hauptwelt der Gemeinschaft 1.
        pub fn new() -> Self {
            let data = GameData::load_workspace().unwrap();
            let mut state = SimState::generate(
                &data,
                SimConfig {
                    seed: 9,
                    player: None,
                },
            );
            state.galaxy.rebuild_index();
            let planet = state.polities.get(FOE).capital.unwrap();
            let system = state.galaxy.planet(planet).system;
            state.fleets.list[1].system = state.galaxy.neighbors(system)[0].system;
            state.fleets.list[0].system = system;
            state.diplomacy.wars.insert((ME, FOE));
            state.sieges.assign.insert(FleetId(0), planet);
            Self {
                data,
                state,
                planet,
                system,
                day: 0,
            }
        }

        /// Belagert wird eine Kolonie, die die Gemeinschaft 1 eben gegründet hat.
        pub fn colony() -> Self {
            let mut scene = Self::new();
            let state = &mut scene.state;
            let treasury = state.economy.treasury(FOE);
            let colony = crate::polity::claimable_planets(
                &state.galaxy,
                &state.polities,
                &scene.data.balance.colonization,
                treasury,
                FOE,
                0,
            )[0];
            crate::polity::claim_planet(
                &mut state.galaxy,
                &mut state.polities,
                &mut state.economy,
                &mut state.chronicle,
                &scene.data,
                FOE,
                colony,
                0,
            )
            .unwrap();
            // Die Firmen der Kolonie gehen an die Börse, wie am Monatsende.
            let capitals: Vec<_> = state.polities.list.iter().map(|p| p.capital).collect();
            crate::finance::found_missing_issuers(
                &scene.data,
                &state.galaxy,
                &state.economy,
                &state.logistics,
                &capitals,
                &mut state.finance,
            );
            let system = state.galaxy.planet(colony).system;
            state.fleets.list[0].system = system;
            // Die Flotte der Gemeinschaft 1 steht weder dort noch im Weg.
            let elsewhere = state.polities.get(PolityId(2)).capital.unwrap();
            state.fleets.list[1].system = state.galaxy.planet(elsewhere).system;
            state.sieges.assign.insert(FleetId(0), colony);
            scene.planet = colony;
            scene.system = system;
            scene
        }

        /// Rechnet den nächsten Tag: den Tagesschritt und danach die fälligen Besitzwechsel.
        pub fn day(&mut self) {
            self.day += 1;
            let state = &mut self.state;
            let mut orbit = Orbit::new(&mut state.fleets, &state.research);
            run_day(
                &self.data,
                &mut state.galaxy,
                &state.polities,
                &state.diplomacy,
                &mut state.economy,
                &mut state.finance,
                &state.logistics,
                &mut state.sieges,
                &mut orbit,
                &mut state.chronicle,
                self.day * TICKS_PER_DAY,
            );
            resolve_conquests(
                &self.data,
                &mut state.galaxy,
                &state.polities,
                &state.diplomacy,
                &mut state.economy,
                &mut state.finance,
                &state.logistics,
                &mut state.sieges,
                &mut orbit,
                &mut state.chronicle,
                self.day * TICKS_PER_DAY,
            );
        }

        /// Der Schritt, der in jedem Takt läuft, mitten im laufenden Tag.
        pub fn tidy_at_midday(&mut self) {
            let state = &mut self.state;
            let orbit = Orbit::new(&mut state.fleets, &state.research);
            tidy(
                &self.data,
                &state.galaxy,
                &state.polities,
                &state.diplomacy,
                &mut state.finance,
                &mut state.sieges,
                &orbit,
                &mut state.chronicle,
                self.day * TICKS_PER_DAY + TICKS_PER_DAY / 2,
            );
        }

        pub fn siege(&self) -> &Siege {
            self.state.sieges.siege(self.planet).expect("Belagerung")
        }

        /// Alles Geld der Partie.
        pub fn money(&self) -> i64 {
            self.state.economy.money_in_accounts()
                + self.state.logistics.money()
                + self.state.finance.money()
        }

        /// System, in dem eine Flotte liegt.
        pub fn fleet_system(&self, fleet: usize) -> SystemId {
            self.state.fleets.list[fleet].system
        }

        /// Versetzt eine Flotte in ein System.
        pub fn place(&mut self, fleet: usize, system: SystemId) {
            self.state.fleets.list[fleet].system = system;
        }

        pub fn ships(&self, fleet: usize) -> Vec<ShipState> {
            self.state.fleets.list[fleet]
                .ships
                .iter()
                .map(|ship| ShipState {
                    hull: ship.hull,
                    ammo: ship.ammo,
                    fuel: ship.fuel,
                })
                .collect()
        }

        /// Gibt einer Flotte den Blockadeauftrag.
        pub fn order_blockade(&mut self, fleet: usize) {
            self.state.fleets.list[fleet].stance = Stance::Blockade;
        }

        /// Bestimmt die Blockaden, wie es die Flottenebene jeden Tag vor der Belagerung
        /// tut: ohne den Einspruch der Stationen.
        pub fn blockades_of_the_fleet_layer(&mut self) {
            let state = &mut self.state;
            update_blockades(&state.galaxy, &mut state.fleets, &state.diplomacy);
        }

        /// Ist das System des belagerten Planeten gesperrt?
        pub fn blockaded(&self) -> bool {
            self.state.fleets.is_blockaded(self.system)
        }

        /// Erste Auffälligkeit, die die Flottenebene an ihren Flotten findet.
        pub fn fleet_anomaly(&self) -> Option<String> {
            self.state.fleets.find_anomaly(&self.data)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet::Stance;
    use crate::polity::Polities;
    use crate::siege::Embarked;

    const ME: PolityId = PolityId(0);
    const FOE: PolityId = PolityId(1);

    /// Die Startflotte der Gemeinschaft 0 liegt im Krieg über der Hauptwelt der
    /// Gemeinschaft 1; deren Flotte ist ein System weiter.
    struct Scene {
        data: GameData,
        galaxy: Galaxy,
        fleets: Fleets,
        research: Research,
        diplomacy: Diplomacy,
        sieges: Sieges,
        planet: PlanetId,
        system: SystemId,
    }

    impl Scene {
        fn new() -> Self {
            let (data, galaxy, polities, mut fleets): (GameData, Galaxy, Polities, Fleets) =
                crate::fleet::testing::setup();
            let planet = polities.get(FOE).capital.unwrap();
            let system = galaxy.planet(planet).system;
            fleets.list[1].system = galaxy.neighbors(system)[0].system;
            fleets.list[0].system = system;
            let mut diplomacy = Diplomacy::default();
            diplomacy.wars.insert((ME, FOE));
            let mut sieges = Sieges::new(galaxy.planets.len());
            sieges.assign.insert(FleetId(0), planet);
            let mut research = Research::default();
            research.sync(&data, polities.list.len());
            Self {
                data,
                galaxy,
                fleets,
                research,
                diplomacy,
                sieges,
                planet,
                system,
            }
        }

        fn squadron(&mut self) -> Squadron {
            Orbit::new(&mut self.fleets, &self.research).squadron(
                &self.data,
                &self.diplomacy,
                &self.sieges,
                self.planet,
                self.system,
                ME,
                FOE,
            )
        }

        fn control(&mut self) -> bool {
            Orbit::new(&mut self.fleets, &self.research).control(
                &self.diplomacy,
                &self.sieges.assign,
                self.planet,
                self.system,
                ME,
            )
        }

        fn apply(&mut self, effects: &DayEffects) -> Toll {
            Orbit::new(&mut self.fleets, &self.research).apply(
                &self.data,
                &mut self.sieges,
                &[FleetId(0)],
                ME,
                effects,
            )
        }

        fn design(&self, key: &str) -> u16 {
            self.data.design_index(key).unwrap()
        }
    }

    /// Waffenleistung eines Moduls aus den Spieldaten.
    fn dps(data: &GameData, key: &str) -> f64 {
        data.modules
            .iter()
            .find(|module| module.key == key)
            .and_then(|module| module.weapon())
            .unwrap()
            .dps() as f64
    }

    #[test]
    fn the_starting_fleet_brings_firepower_berths_and_toughness() {
        let mut scene = Scene::new();
        let squadron = scene.squadron();
        let data = &scene.data;
        assert_eq!(squadron.fleets, vec![FleetId(0)]);
        assert_eq!(squadron.ships, 6);
        assert!(squadron.view.control);
        // Ein Speer, zwei Wächter, drei Dolche; die Nahbereichsbatterien zählen nicht.
        let cruiser = 4.0 * dps(data, "kanone") + dps(data, "impulslaser");
        let frigate = 2.0 * dps(data, "impulslaser");
        let corvette = 2.0 * dps(data, "torpedo");
        let expected = cruiser + 2.0 * frigate + 3.0 * corvette;
        assert!(
            (squadron.view.firepower - expected).abs() < 1e-3,
            "Feuerkraft {} statt {expected}",
            squadron.view.firepower
        );
        assert!(expected > 100.0 && expected < 200.0);
        assert_eq!(squadron.view.ammo, 1.0);
        assert_eq!(squadron.fuel, 1.0);
        assert_eq!(squadron.view.embarked, 0.0);
        // Plätze je Rumpfsegment: 14 + 2 × 6 + 3 × 3 Segmente.
        let berths = data.balance.siege.berth_per_segment * 35.0;
        assert!((squadron.berths - berths).abs() < 1e-9);
        // Standfestigkeit: Rumpf, Schild und Panzerung, große Schiffe voll gewichtet.
        let weight = data.balance.siege.gun_weight;
        let tough = |key: &str| {
            let stats = &data.designs[data.design_index(key).unwrap() as usize].stats;
            (stats.hull + stats.shield + stats.armor.iter().sum::<f32>()) as f64
        };
        let expected = weight[ShipClass::Cruiser.index()] * tough("speer")
            + 2.0 * weight[ShipClass::Frigate.index()] * tough("waechter")
            + 3.0 * weight[ShipClass::Corvette.index()] * tough("dolch");
        assert!((squadron.toughness - expected).abs() < 1e-6);
        // Ohne Technik sieht der Tagesrechner den Grundstand.
        assert_eq!(squadron.view.att, TechView::default());
        assert_eq!(squadron.view.def, TechView::default());
    }

    #[test]
    fn firepower_follows_hull_weapons_and_magazines() {
        let mut scene = Scene::new();
        let full = scene.squadron().view.firepower;
        let cruiser_guns = 4.0 * dps(&scene.data, "kanone");
        let cruiser_laser = dps(&scene.data, "impulslaser");
        let torpedoes = 2.0 * dps(&scene.data, "torpedo");

        // Ein halber Rumpf trägt die halbe Feuerkraft.
        scene.fleets.list[0].ships[0].hull = 0.5;
        let hurt = scene.squadron().view.firepower;
        assert!((full - hurt - 0.5 * (cruiser_guns + cruiser_laser)).abs() < 1e-3);
        scene.fleets.list[0].ships[0].hull = 1.0;

        // Leere Magazine: Geschütze und Torpedos schweigen, Strahlwaffen feuern weiter.
        scene.fleets.list[0].ships[0].ammo = 0.0;
        let dry = scene.squadron();
        assert!((full - dry.view.firepower - cruiser_guns).abs() < 1e-3);
        assert!(dry.view.ammo > 0.0 && dry.view.ammo < 1.0);
        scene.fleets.list[0].ships[3].ammo = 0.0;
        let drier = scene.squadron().view.firepower;
        assert!((dry.view.firepower - drier - torpedoes).abs() < 1e-3);
        for ship in &mut scene.fleets.list[0].ships {
            ship.ammo = 0.0;
        }
        let empty = scene.squadron();
        assert_eq!(empty.view.ammo, 0.0);
        let lasers = cruiser_laser + 2.0 * 2.0 * dps(&scene.data, "impulslaser");
        assert!((empty.view.firepower - lasers).abs() < 1e-3);
        for ship in &mut scene.fleets.list[0].ships {
            ship.ammo = 1.0;
        }

        // Ausgefallene Module mindern die Feuerkraft, wie die Flottenebene den Zustand
        // eines Schiffs rechnet: Sind alle Module des Kreuzers zerstört, bleibt der Anteil
        // seines Kampfwerts, der am Rumpf hängt.
        let cruiser = scene.design("speer");
        let slots = scene.data.designs[cruiser as usize].modules.len();
        scene.fleets.list[0].ships[0].modules = vec![0.0; slots];
        let crippled = scene.squadron().view.firepower;
        let weight = scene.data.balance.military.module_weight as f64;
        assert!(weight > 0.0 && weight < 1.0);
        assert!((full - crippled - weight * (cruiser_guns + cruiser_laser)).abs() < 1e-3);
    }

    #[test]
    fn control_needs_a_ready_assigned_fleet_with_fuel_and_an_uncontested_orbit() {
        let mut scene = Scene::new();
        assert!(scene.control());

        // Ohne Zuordnung keine Belagerung, auch wenn die Flotte im System liegt.
        scene.sieges.assign.clear();
        assert!(!scene.control());
        assert!(scene.squadron().fleets.is_empty());
        scene.sieges.assign.insert(FleetId(0), scene.planet);

        // Im Gefecht, beim Sammeln, unterwegs oder mit leerem Tank hält sie den Orbit nicht.
        scene.fleets.list[0].in_battle = true;
        assert!(!scene.control());
        scene.fleets.list[0].in_battle = false;
        scene.fleets.list[0].regroup_until = Some(40);
        assert!(!scene.control());
        scene.fleets.list[0].regroup_until = None;
        let away = scene.galaxy.neighbors(scene.system)[0].system;
        scene.fleets.list[0].route = vec![away];
        scene.fleets.list[0].progress = 0.3;
        assert!(!scene.control());
        scene.fleets.list[0].route.clear();
        scene.fleets.list[0].progress = 0.0;
        scene.fleets.list[0].ships[4].fuel = 0.0;
        assert!(!scene.control());
        assert_eq!(scene.squadron().view.firepower, 0.0);
        scene.fleets.list[0].ships[4].fuel = 1.0;
        assert!(scene.control());

        // Eine feindliche Flotte im System macht den Orbit streitig, solange sie sich
        // nicht sammelt. Die Belagerer bleiben, wo sie sind, halten aber nichts.
        scene.fleets.list[1].system = scene.system;
        assert!(!scene.control());
        let contested = scene.squadron();
        assert!(!contested.view.control);
        assert_eq!(contested.fleets, vec![FleetId(0)]);
        scene.fleets.list[1].regroup_until = Some(40);
        assert!(scene.control());
        // Im Frieden stört sie nicht.
        scene.fleets.list[1].regroup_until = None;
        scene.diplomacy.wars.clear();
        assert!(scene.control());
    }

    #[test]
    fn a_day_under_fire_costs_big_ships_most_and_everyone_ammunition_and_fuel() {
        let mut scene = Scene::new();
        let total = scene.squadron().toughness;
        let weight = scene.data.balance.siege.gun_weight;
        let effects = DayEffects {
            hull: 0.1 * total,
            ammo: 0.05,
            fuel: 0.01,
            ..DayEffects::NONE
        };
        let toll = scene.apply(&effects);
        assert_eq!(toll, Toll::default());
        let fleet = &scene.fleets.list[0];
        // Kreuzer verlieren den vollen Anteil, Fregatten und Korvetten nach ihrem Gewicht.
        let lost = |ship: usize| 1.0 - fleet.ships[ship].hull as f64;
        assert!((lost(0) - 0.1 * weight[ShipClass::Cruiser.index()]).abs() < 1e-6);
        assert!((lost(1) - 0.1 * weight[ShipClass::Frigate.index()]).abs() < 1e-6);
        assert!((lost(5) - 0.1 * weight[ShipClass::Corvette.index()]).abs() < 1e-6);
        assert!(lost(0) >= 4.9 * lost(5));
        for ship in &fleet.ships {
            assert!((ship.ammo - 0.95).abs() < 1e-6 && (ship.fuel - 0.99).abs() < 1e-6);
        }
        // Die Stärke ist nachgeführt: Die Flotte besteht die Prüfung der Flottenebene.
        assert_eq!(scene.fleets.find_anomaly(&scene.data), None);
        assert!(scene.fleets.list[0].strength < scene.data.balance.fleets.start_strength);

        // Rumpftechnik des Belagerers macht seine Schiffe standfester.
        let mut hardened = Scene::new();
        let hull = hardened.data.tech_index("verteidigung_1").unwrap();
        assert!(hardened.research.grant(&hardened.data, ME, hull));
        assert!(hardened.squadron().toughness > total);
        hardened.apply(&effects);
        assert!(hardened.fleets.list[0].ships[0].hull > scene.fleets.list[0].ships[0].hull);

        // Ein Tag ohne Wirkung ändert nichts.
        let before = scene.fleets.list[0].ships.clone();
        assert_eq!(scene.apply(&DayEffects::NONE), Toll::default());
        assert_eq!(scene.fleets.list[0].ships, before);
    }

    #[test]
    fn troops_go_down_with_their_ships() {
        let mut scene = Scene::new();
        let total = scene.squadron().toughness;
        let berths = scene.squadron().berths;
        scene.sieges.embarked.insert(
            FleetId(0),
            Embarked {
                troops: berths,
                supply: 90.0 * berths,
            },
        );
        // Der angeschlagene Kreuzer übersteht den Tag nicht.
        scene.fleets.list[0].ships[0].hull = 0.05;
        let toll = scene.apply(&DayEffects {
            hull: 0.1 * total,
            ..DayEffects::NONE
        });
        assert_eq!(toll.ships_lost, 1);
        assert_eq!(scene.fleets.list[0].ships.len(), 5);
        let left = scene.squadron().berths;
        let per_segment = scene.data.balance.siege.berth_per_segment;
        assert!((berths - left - per_segment * 14.0).abs() < 1e-9);
        assert!((toll.troops_lost - (berths - left)).abs() < 1e-9);
        let aboard = &scene.sieges.embarked[&FleetId(0)];
        assert!((aboard.troops - left).abs() < 1e-9);
        assert!((aboard.supply - 90.0 * left).abs() < 1e-6);

        // Geht die ganze Flotte verloren, bleibt weder Zuordnung noch Truppe.
        for ship in &mut scene.fleets.list[0].ships {
            ship.hull = 0.001;
        }
        let toll = scene.apply(&DayEffects {
            hull: 10.0 * total,
            ..DayEffects::NONE
        });
        assert_eq!(toll.ships_lost, 5);
        assert!((toll.troops_lost - left).abs() < 1e-9);
        assert!(!scene.fleets.list[0].alive);
        assert!(scene.sieges.assign.is_empty() && scene.sieges.embarked.is_empty());
        assert_eq!(scene.fleets.find_anomaly(&scene.data), None);
        assert!(!scene.control());
    }

    #[test]
    fn merging_and_splitting_carry_troops_and_targets_along() {
        let mut scene = Scene::new();
        let (data, me) = (scene.data.clone(), FleetId(0));
        let berths = scene.squadron().berths;
        scene.sieges.embarked.insert(
            me,
            Embarked {
                troops: berths,
                supply: 300.0,
            },
        );
        // Die drei Korvetten lösen sich: Was auf den übrigen Schiffen keinen Platz mehr
        // hat, fährt mit ihnen.
        let child = FleetId(scene.fleets.list.len() as u32);
        let mut detached = scene.fleets.list[0].clone();
        detached.id = child;
        detached.ships = scene.fleets.list[0].ships.split_off(3);
        scene.fleets.list.push(detached);
        scene.fleets.refresh(&data);
        let orbit = Orbit::new(&mut scene.fleets, &scene.research);
        let (room, small) = (orbit.berths(&data, me), orbit.berths(&data, child));
        assert!((room + small - berths).abs() < 1e-9);
        orbit.split(&data, &mut scene.sieges, me, child);
        assert!((scene.sieges.embarked[&me].troops - room).abs() < 1e-9);
        assert!((scene.sieges.embarked[&child].troops - small).abs() < 1e-9);
        let supply = scene.sieges.embarked[&me].supply + scene.sieges.embarked[&child].supply;
        assert!((supply - 300.0).abs() < 1e-9);
        assert_eq!(orbit.settle(&data, &mut scene.sieges), vec![]);
        assert_eq!(scene.sieges.assign.get(&child), None);

        // Wieder vereinigt (die belagernde Flotte geht in der anderen auf): Die Truppen
        // sind beisammen, und das Ziel bleibt.
        let ships = std::mem::take(&mut scene.fleets.list[0].ships);
        scene.fleets.list[child.index()].ships.extend(ships);
        scene.fleets.refresh(&data);
        let orbit = Orbit::new(&mut scene.fleets, &scene.research);
        orbit.merged(&mut scene.sieges, me, child);
        assert_eq!(scene.sieges.embarked.len(), 1);
        assert!((scene.sieges.embarked[&child].troops - berths).abs() < 1e-9);
        assert_eq!(scene.sieges.assign.get(&child), Some(&scene.planet));
        assert_eq!(scene.sieges.assign.len(), 1);
        assert_eq!(orbit.settle(&data, &mut scene.sieges), vec![]);

        // Ohne Vereinigung wären Truppen einer aufgelösten Flotte verloren.
        scene.sieges.embarked.insert(
            me,
            Embarked {
                troops: 2.0,
                supply: 10.0,
            },
        );
        scene.sieges.assign.insert(me, scene.planet);
        let orbit = Orbit::new(&mut scene.fleets, &scene.research);
        assert_eq!(orbit.settle(&data, &mut scene.sieges), vec![(ME, 2.0)]);
        assert!(!scene.sieges.embarked.contains_key(&me));
        assert!(!scene.sieges.assign.contains_key(&me));
    }

    #[test]
    fn stations_keep_a_system_open() {
        let mut scene = Scene::new();
        scene.fleets.list[0].stance = Stance::Blockade;
        let (galaxy, diplomacy) = (scene.galaxy.clone(), scene.diplomacy.clone());
        let mut orbit = Orbit::new(&mut scene.fleets, &scene.research);
        orbit.refresh_blockades(&galaxy, &diplomacy, &scene.sieges.defenses);
        assert!(orbit.is_blockaded(scene.system));
        // Eine Station der Welt, die das System prägt, hält es offen.
        scene.sieges.defenses[scene.planet.index()].stations = 0.4;
        orbit.refresh_blockades(&galaxy, &diplomacy, &scene.sieges.defenses);
        assert!(!orbit.is_blockaded(scene.system));
        // Ist sie zerstört, steht die Sperre wieder.
        scene.sieges.defenses[scene.planet.index()].stations = 0.0;
        orbit.refresh_blockades(&galaxy, &diplomacy, &scene.sieges.defenses);
        assert!(orbit.is_blockaded(scene.system));
        // Die Station einer fremden Welt im System hilft dem Systembesitzer nicht.
        let other = galaxy
            .planets_in(scene.system)
            .find(|planet| planet.owner.is_none())
            .map(|planet| planet.id);
        if let Some(other) = other {
            scene.sieges.defenses[other.index()].stations = 1.0;
            orbit.refresh_blockades(&galaxy, &diplomacy, &scene.sieges.defenses);
            assert!(orbit.is_blockaded(scene.system));
        }
    }

    #[test]
    fn technology_reaches_the_resolver_as_surcharges_and_gates() {
        let mut scene = Scene::new();
        let data = scene.data.clone();
        let grant = |research: &mut Research, polity, key: &str| {
            assert!(research.grant(&data, polity, data.tech_index(key).unwrap()));
        };
        for key in ["boden_1", "boden_2", "boden_3"] {
            grant(&mut scene.research, ME, key);
        }
        grant(&mut scene.research, ME, "waffen_1");
        grant(&mut scene.research, ME, "waffen_2");
        grant(&mut scene.research, FOE, "boden_1");
        grant(&mut scene.research, FOE, "boden_2");
        let view = scene.squadron().view;
        let (mine, theirs) = (scene.research.effects(ME), scene.research.effects(FOE));
        assert!((view.att.ground_attack - (mine.ground_attack - 1.0)).abs() < 1e-12);
        assert!(view.att.ground_attack > 0.15 && view.att.weapon_damage > 0.05);
        assert!((view.def.ground_defense - (theirs.ground_defense - 1.0)).abs() < 1e-12);
        assert!(view.def.ground_defense > 0.1 && view.def.weapon_damage == 0.0);
        // Tiefbunkerbau und Feuerleitung beim Belagerer, nur der Bunkerbau beim Verteidiger.
        assert_eq!(view.att.gates, [true, true, false, false]);
        assert_eq!(view.def.gates, [true, false, false, false]);
        assert!(view.att.has(crate::siege::Gate::Support));
    }
}

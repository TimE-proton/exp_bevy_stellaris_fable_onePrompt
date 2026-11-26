//! Flotten auf der Galaxiekarte: Schiffe, Bewegung, Werften, Nachschub, Blockade.
//!
//! Eine Flotte besteht aus Schiffen mit Entwurf, Zustand und Bordvorräten. Ihre Stärke
//! ist kein eigener Zustand mehr, sondern aus den Schiffen abgeleitet (Bauwert mal
//! Zustand); Kaperfahrt, Geleitschutz und Blockade rechnen weiter mit dieser einen Zahl.
//! Schiffe entstehen in Werften aus echten Bauteilen vom Markt, fassen Munition und
//! Treibstoff nach, wo ihre Gemeinschaft zu Hause ist, und kosten jeden Monat Sold.
//!
//! Treffen Flotten verfeindeter Gemeinschaften in einem System aufeinander, rechnet der
//! taktische Rechner aus [`crate::battle`] das Gefecht Takt für Takt; `battles` stellt es
//! auf und schreibt das Ergebnis in die Flotten zurück.

mod battles;
mod movement;
mod plugin;
mod supply;
mod yard;

pub use battles::{
    BattlePause, BattleReport, Battles, Debris, SideReport, advance_battles, combat_modifiers,
    crew_morale, open_battles, order_retreat, relay_orders, retreat_target,
};
pub use movement::{day_travel, move_fleets, update_blockades};
pub use plugin::FleetPlugin;
pub use supply::{monthly_upkeep, pay_upkeep, repair_fleets, resupply_fleets};
pub use yard::{
    BuildOrder, advance_yards, arms_capital, cancel_order, design_buildable, order_ship, yard_slots,
};

use crate::battle::{Doctrine, Formation, ShipSpec};
use crate::data::GameData;
use crate::diplomacy::Diplomacy;
use crate::galaxy::Galaxy;
use crate::ids::{BattleId, FleetId, PlanetId, PolityId, ShipId, SystemId};
use crate::polity::Polities;
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

/// Ein Schiff der strategischen Ebene. Rumpf, Munition und Treibstoff sind Anteile 0..1
/// am Höchstwert des Entwurfs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ship {
    pub id: ShipId,
    /// Index in [`GameData::designs`].
    pub design: u16,
    pub hull: f32,
    /// Zustand 0..1 je Modul in der Reihenfolge des Entwurfs; leer heißt: alle heil.
    pub modules: Vec<f32>,
    pub ammo: f32,
    pub fuel: f32,
    /// Bordjäger im Hangar.
    pub fighters: u32,
}

impl Ship {
    /// Unversehrt und voll versorgt.
    pub fn fresh(data: &GameData, id: ShipId, design: u16) -> Self {
        Self {
            id,
            design,
            hull: 1.0,
            modules: Vec::new(),
            ammo: 1.0,
            fuel: 1.0,
            fighters: data.designs[design as usize].stats.fighters,
        }
    }

    /// Frisch aus der Werft: unversehrt, die Bordjäger an Bord, aber Magazine und Tanks
    /// leer. Munition und Treibstoff sind nicht Teil der Bauteile; das Schiff fasst sie
    /// wie jedes andere am Markt nach.
    pub fn from_yard(data: &GameData, id: ShipId, design: u16) -> Self {
        Self {
            ammo: 0.0,
            fuel: 0.0,
            ..Self::fresh(data, id, design)
        }
    }

    /// Mittlerer Zustand der Module (1 = alle heil).
    pub fn module_condition(&self) -> f32 {
        if self.modules.is_empty() {
            1.0
        } else {
            self.modules.iter().sum::<f32>() / self.modules.len() as f32
        }
    }

    /// Zustand 0..1: der Rumpf, gemindert um ausgefallene Module. Wie schwer die Module
    /// wiegen, sagt `military.module_weight`.
    pub fn condition(&self, data: &GameData) -> f32 {
        let weight = data.balance.military.module_weight;
        self.hull * (1.0 - weight + weight * self.module_condition())
    }

    /// Braucht das Schiff die Werft: Rumpf, Module oder Bordjäger unvollständig?
    pub fn is_damaged(&self, data: &GameData) -> bool {
        self.hull < 1.0
            || self.modules.iter().any(|module| *module < 1.0)
            || self.fighters < data.designs[self.design as usize].stats.fighters
    }

    /// Womit das Schiff in ein Gefecht geht: die Übergabe an den taktischen Rechner.
    pub fn spec(&self) -> ShipSpec {
        ShipSpec {
            source: Some(self.id),
            design: self.design,
            hull: self.hull,
            modules: self.modules.clone(),
            ammo: self.ammo,
            fuel: self.fuel,
            morale: None,
            fighters: self.fighters,
        }
    }
}

/// Bauwert eines Entwurfs, getrennt in das Schiff selbst und einen einzelnen Bordjäger.
fn value_parts(data: &GameData, design: u16) -> (f64, f64) {
    let stats = &data.designs[design as usize].stats;
    let craft = stats
        .craft
        .map_or(0.0, |craft| data.designs[craft as usize].stats.value);
    (
        (stats.value - craft * stats.fighters as f64).max(0.0),
        craft,
    )
}

/// Kampfwert eines Schiffs in Credits: Bauwert mal Zustand. Bordjäger zählen mit ihrem
/// eigenen Bauwert, soweit sie noch an Bord sind.
pub fn combat_value(data: &GameData, ship: &Ship) -> f64 {
    let (body, craft) = value_parts(data, ship.design);
    let fighters = ship
        .fighters
        .min(data.designs[ship.design as usize].stats.fighters);
    body * ship.condition(data) as f64 + craft * fighters as f64
}

/// Bauwert eines Schiffs in Credits, wie es dasteht: das Schiff selbst und die Bordjäger,
/// die es noch führt. Der Zustand zählt nicht. Maß für Verluste im Gefecht.
pub fn material_value(data: &GameData, ship: &Ship) -> f64 {
    let (body, craft) = value_parts(data, ship.design);
    let fighters = ship
        .fighters
        .min(data.designs[ship.design as usize].stats.fighters);
    body + craft * fighters as f64
}

/// Stärke einer Flotte: Kampfwert ihrer Schiffe, geeicht an der Startflotte
/// (`fleets.start_strength`).
pub fn fleet_strength(data: &GameData, fleet: &Fleet) -> f32 {
    let value: f64 = fleet
        .ships
        .iter()
        .map(|ship| combat_value(data, ship))
        .sum();
    (value * data.balance.military.strength_per_value) as f32
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Fleet {
    pub id: FleetId,
    pub owner: PolityId,
    pub name: String,
    /// Kampfwert, abgeleitet aus den Schiffen (siehe [`fleet_strength`]). Wer Schiffe
    /// ändert, führt ihn mit [`Fleets::refresh`] nach.
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
    // Die Felder ab hier fehlen in Spielständen des Formats 1. Mit dem Vorgabewert lassen
    // sie sich noch lesen und werden dann am Format erkannt und abgelehnt.
    /// Die Schiffe der Flotte. Eine Flotte ohne Schiffe besteht nicht mehr.
    #[serde(default)]
    pub ships: Vec<Ship>,
    /// Stehende Befehle für künftige Gefechte.
    #[serde(default)]
    pub formation: Formation,
    #[serde(default)]
    pub doctrine: Doctrine,
    /// Gefecht, in dem die Flotte steht. `in_battle` sagt dasselbe als Wahrheitswert.
    #[serde(default)]
    pub battle: Option<BattleId>,
    /// System, aus dem die Flotte in ihr jetziges kam. Dorthin weicht sie aus, wenn sie
    /// ein Gefecht verliert.
    #[serde(default)]
    pub came_from: Option<SystemId>,
    /// Tag, an dem die Flotte nach dem Abzug aus einem Gefecht wieder einsatzbereit ist.
    /// Bis dahin sammelt sie sich: Sie beginnt kein Gefecht, tritt keinem bei und zählt
    /// weder für Blockade noch für Kaperfahrt oder Geleitschutz.
    #[serde(default)]
    pub regroup_until: Option<u64>,
}

impl Fleet {
    /// Steht die Flotte in einem System (und ist nicht auf einer Sprungroute)?
    pub fn is_docked(&self) -> bool {
        self.alive && (self.route.is_empty() || self.progress <= 0.0)
    }

    /// Sammelt sich die Flotte nach dem Abzug aus einem Gefecht?
    pub fn is_regrouping(&self) -> bool {
        self.regroup_until.is_some()
    }

    /// Einsatzbereit: weder im Gefecht noch beim Sammeln. Nur solche Flotten beginnen
    /// Gefechte, treten ihnen bei, blockieren, kapern oder geben Geleit.
    pub fn is_ready(&self) -> bool {
        self.alive && !self.in_battle && !self.is_regrouping()
    }

    /// Bauwert aller Schiffe in Credits, ohne Rücksicht auf ihren Zustand.
    pub fn build_value(&self, data: &GameData) -> f64 {
        self.ships
            .iter()
            .map(|ship| data.designs[ship.design as usize].stats.value)
            .sum()
    }

    /// Der knappste Treibstoffvorrat (0..1). Er bestimmt, wie weit die Flotte in voller
    /// Fahrt kommt: Sie wartet auf ihr langsamstes Schiff.
    pub fn min_fuel(&self) -> f32 {
        self.ships.iter().map(|ship| ship.fuel).fold(1.0, f32::min)
    }

    /// Strecke, die die Flotte heute auf dem Marsch zurücklegen kann: volle Fahrt, solange
    /// der knappste Tank reicht, danach nur noch die gedrosselte.
    pub fn day_range(&self, data: &GameData) -> f32 {
        let balance = &data.balance.military;
        day_travel(
            self.min_fuel(),
            data.balance.fleets.speed,
            balance.fuel_per_distance,
            balance.dry_speed,
            f32::INFINITY,
        )
        .0
    }

    /// Mittlere Vorräte an Munition und Treibstoff (0..1), gewichtet mit dem
    /// Fassungsvermögen der Schiffe.
    pub fn supply(&self, data: &GameData) -> (f32, f32) {
        let (mut ammo, mut ammo_max, mut fuel, mut fuel_max) = (0.0, 0.0, 0.0, 0.0);
        for ship in &self.ships {
            let stats = &data.designs[ship.design as usize].stats;
            ammo += ship.ammo * stats.ammo;
            ammo_max += stats.ammo;
            fuel += ship.fuel * stats.fuel;
            fuel_max += stats.fuel;
        }
        let share = |have: f32, max: f32| if max > 0.0 { have / max } else { 1.0 };
        (share(ammo, ammo_max), share(fuel, fuel_max))
    }

    pub fn ship(&self, id: ShipId) -> Option<&Ship> {
        self.ships.iter().find(|ship| ship.id == id)
    }

    /// Alle Schiffe der Flotte, wie sie in ein Gefecht gehen.
    pub fn specs(&self) -> Vec<ShipSpec> {
        self.ships.iter().map(Ship::spec).collect()
    }
}

#[derive(Resource, Clone, Debug, Default, Serialize, Deserialize)]
pub struct Fleets {
    pub list: Vec<Fleet>,
    /// Blockierte Systeme, aufsteigend. Wird täglich aus der Lage neu bestimmt.
    pub blockaded: Vec<SystemId>,
    /// Bauaufträge aller Werften, aufsteigend nach Nummer.
    #[serde(default)]
    pub orders: Vec<BuildOrder>,
    /// Nächste freie Schiffsnummer.
    #[serde(default)]
    pub next_ship: u32,
    /// Nächste freie Auftragsnummer.
    #[serde(default)]
    pub next_order: u32,
    /// Soldrückstand je Gemeinschaft (0..1, geglättet): Anteil des Solds, den der Staat
    /// zuletzt schuldig blieb. Lesen mit [`Fleets::arrears_of`].
    #[serde(default)]
    pub arrears: Vec<f32>,
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

    pub fn get(&self, id: FleetId) -> Option<&Fleet> {
        self.list.get(id.index())
    }

    /// Schiffe einer Gemeinschaft in allen ihren Flotten.
    pub fn ship_count(&self, polity: PolityId) -> usize {
        self.of(polity).map(|fleet| fleet.ships.len()).sum()
    }

    /// Flotte, zu der ein Schiff gehört.
    pub fn fleet_of_ship(&self, ship: ShipId) -> Option<FleetId> {
        self.list
            .iter()
            .find(|fleet| fleet.alive && fleet.ship(ship).is_some())
            .map(|fleet| fleet.id)
    }

    /// Bauaufträge einer Gemeinschaft in Nummernfolge.
    pub fn orders_of(&self, polity: PolityId) -> impl Iterator<Item = &BuildOrder> + '_ {
        self.orders
            .iter()
            .filter(move |order| order.polity == polity)
    }

    /// Bauaufträge der Werft einer Welt in Nummernfolge: Die ersten belegen die Bauplätze,
    /// die übrigen warten.
    pub fn orders_at(&self, planet: PlanetId) -> impl Iterator<Item = &BuildOrder> + '_ {
        self.orders
            .iter()
            .filter(move |order| order.planet == planet)
    }

    /// Soldrückstand einer Gemeinschaft (0 = alles gezahlt, 1 = nichts).
    pub fn arrears_of(&self, polity: PolityId) -> f32 {
        self.arrears.get(polity.index()).copied().unwrap_or(0.0)
    }

    /// Vergibt die nächste Schiffsnummer.
    pub fn next_ship_id(&mut self) -> ShipId {
        let id = ShipId(self.next_ship);
        self.next_ship += 1;
        id
    }

    /// Reiht ein neues, voll versorgtes Schiff in eine bestehende Flotte ein. `None`, wenn
    /// es die Flotte oder den Entwurf nicht gibt.
    pub fn add_ship(&mut self, data: &GameData, fleet: FleetId, design: u16) -> Option<ShipId> {
        if design as usize >= data.designs.len()
            || !self.get(fleet).is_some_and(|fleet| fleet.alive)
        {
            return None;
        }
        let id = self.next_ship_id();
        let fleet = &mut self.list[fleet.index()];
        fleet.ships.push(Ship::fresh(data, id, design));
        fleet.strength = fleet_strength(data, fleet);
        Some(id)
    }

    /// Stellt eine neue, noch leere Flotte auf. Der Aufrufer gibt ihr Schiffe und ruft
    /// [`Fleets::refresh`]: Ohne Schiffe besteht sie nicht.
    pub fn new_fleet(&mut self, polities: &Polities, owner: PolityId, system: SystemId) -> FleetId {
        let id = FleetId::from_index(self.list.len());
        // Nummeriert wird je Gemeinschaft fortlaufend; aufgelöste Flotten zählen mit.
        let number = self
            .list
            .iter()
            .filter(|fleet| fleet.owner == owner)
            .count()
            + 1;
        self.list.push(Fleet {
            id,
            owner,
            name: format!("{number}. Flotte ({})", polities.get(owner).short),
            strength: 0.0,
            stance: Stance::Guard,
            system,
            route: Vec::new(),
            progress: 0.0,
            alive: true,
            in_battle: false,
            ships: Vec::new(),
            formation: Formation::default(),
            doctrine: Doctrine::default(),
            battle: None,
            came_from: None,
            regroup_until: None,
        });
        id
    }

    /// Führt nach, was aus den Schiffen folgt: die Stärke jeder Flotte, und dass eine
    /// Flotte ohne Schiffe nicht mehr besteht.
    pub fn refresh(&mut self, data: &GameData) {
        for fleet in &mut self.list {
            if !fleet.alive {
                continue;
            }
            if fleet.ships.is_empty() {
                fleet.alive = false;
                fleet.in_battle = false;
                fleet.battle = None;
                fleet.regroup_until = None;
                fleet.route.clear();
                fleet.progress = 0.0;
                fleet.strength = 0.0;
            } else {
                fleet.strength = fleet_strength(data, fleet);
            }
        }
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
        // Wer kämpft oder sich sammelt, kapert nicht und gibt kein Geleit.
        for fleet in self.docked_in(system).filter(|fleet| fleet.is_ready()) {
            if fleet.stance == Stance::Raid && diplomacy.at_war(fleet.owner, victim) {
                raiders += fleet.strength;
                raider.get_or_insert(fleet.owner);
            } else if fleet.owner == victim && fleet.stance == Stance::Escort {
                escorts += fleet.strength;
            }
        }
        (raiders, escorts, raider)
    }

    /// Prüft, dass Flotten, Schiffe und Bauaufträge in sich stimmen. Gibt die erste
    /// Auffälligkeit zurück.
    pub fn find_anomaly(&self, data: &GameData) -> Option<String> {
        let unit = |value: f32| value.is_finite() && (0.0..=1.0).contains(&value);
        let mut ids: Vec<ShipId> = Vec::new();
        for fleet in &self.list {
            if !fleet.strength.is_finite() || fleet.strength < 0.0 {
                return Some(format!("{}: Stärke {}", fleet.name, fleet.strength));
            }
            if !fleet.alive {
                continue;
            }
            if fleet.ships.is_empty() {
                return Some(format!("{}: besteht ohne Schiffe", fleet.name));
            }
            for ship in &fleet.ships {
                let Some(design) = data.designs.get(ship.design as usize) else {
                    return Some(format!("Schiff {}: Entwurf unbekannt", ship.id));
                };
                let modules_fit =
                    ship.modules.is_empty() || ship.modules.len() == design.modules.len();
                if !unit(ship.hull)
                    || !unit(ship.ammo)
                    || !unit(ship.fuel)
                    || !modules_fit
                    || !ship.modules.iter().all(|module| unit(*module))
                    || ship.fighters > design.stats.fighters
                {
                    return Some(format!(
                        "Schiff {} ({}): Zustand außerhalb der Grenzen: {ship:?}",
                        ship.id, fleet.name
                    ));
                }
                ids.push(ship.id);
            }
            // Erst wenn die Schiffe selbst stimmen, lässt sich die Stärke nachrechnen.
            let derived = fleet_strength(data, fleet);
            if (fleet.strength - derived).abs() > 1e-3 * derived.max(1.0) {
                return Some(format!(
                    "{}: Stärke {} passt nicht zu den Schiffen ({derived})",
                    fleet.name, fleet.strength
                ));
            }
        }
        ids.sort_unstable();
        if let Some(pair) = ids.windows(2).find(|pair| pair[0] == pair[1]) {
            return Some(format!("Schiffsnummer {} doppelt vergeben", pair[0]));
        }
        if ids.last().is_some_and(|last| last.0 >= self.next_ship) {
            return Some("Schiffsnummer jenseits des Zählers".into());
        }
        for pair in self.orders.windows(2) {
            if pair[0].id >= pair[1].id {
                return Some(format!("Bauauftrag {} steht außer der Reihe", pair[1].id));
            }
        }
        for order in &self.orders {
            if order.id >= self.next_order
                || !unit(order.progress)
                || order.spent < 0
                || order.design as usize >= data.designs.len()
            {
                return Some(format!("Bauauftrag {} entgleist: {order:?}", order.id));
            }
        }
        if let Some(arrears) = self.arrears.iter().find(|arrears| !unit(**arrears)) {
            return Some(format!("Soldrückstand {arrears}"));
        }
        None
    }
}

/// Jede Gemeinschaft beginnt mit der Flotte aus `balance.ron: military.starting_fleet`,
/// voll versorgt im System ihrer Hauptwelt.
pub fn setup_fleets(data: &GameData, galaxy: &Galaxy, polities: &Polities) -> Fleets {
    let mut fleets = Fleets::default();
    for polity in &polities.list {
        let Some(capital) = polity.capital else {
            continue;
        };
        let id = fleets.new_fleet(polities, polity.id, galaxy.planet(capital).system);
        for (design, count) in &data.balance.military.starting_fleet_ids {
            for _ in 0..*count {
                let ship = fleets.next_ship_id();
                fleets.list[id.index()]
                    .ships
                    .push(Ship::fresh(data, ship, *design));
            }
        }
    }
    fleets.refresh(data);
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
    if fleet.in_battle {
        return Err(format!(
            "{} steht im Gefecht; erst den Rückzug befehlen",
            fleet.name
        ));
    }
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

/// Stehende Befehle einer Flotte für künftige Gefechte: Aufstellung und Doktrin.
pub fn set_fleet_orders(
    fleets: &mut Fleets,
    polity: PolityId,
    fleet: FleetId,
    formation: Formation,
    doctrine: Doctrine,
) -> Result<(), String> {
    let fleet = own_fleet(fleets, polity, fleet)?;
    fleet.formation = formation;
    fleet.doctrine = doctrine;
    Ok(())
}

/// Darf an der Zusammensetzung der Flotte etwas geändert werden? Nur im Dock und nicht
/// mitten im Gefecht.
fn ready_to_reorganize(fleet: &Fleet) -> Result<(), String> {
    if !fleet.is_docked() {
        return Err(format!(
            "{} ist unterwegs; umgliedern lässt sich nur im Dock",
            fleet.name
        ));
    }
    if fleet.in_battle {
        return Err(format!("{} steht im Gefecht", fleet.name));
    }
    Ok(())
}

/// Vereinigt zwei Flotten derselben Gemeinschaft, die im selben System liegen. Die
/// Schiffe von `from` gehen an `into`; `from` besteht danach nicht mehr.
pub fn merge_fleets(
    data: &GameData,
    fleets: &mut Fleets,
    polity: PolityId,
    from: FleetId,
    into: FleetId,
) -> Result<(), String> {
    if from == into {
        return Err("eine Flotte lässt sich nicht mit sich selbst vereinigen".into());
    }
    let target_system = {
        let target = own_fleet(fleets, polity, into)?;
        ready_to_reorganize(target)?;
        target.system
    };
    let source = own_fleet(fleets, polity, from)?;
    ready_to_reorganize(source)?;
    if source.system != target_system {
        return Err("beide Flotten müssen im selben System liegen".into());
    }
    let ships = std::mem::take(&mut source.ships);
    let regrouping = source.regroup_until;
    let target = &mut fleets.list[into.index()];
    target.ships.extend(ships);
    // Die Sammelfrist geht mit: Vereinigt ist die Flotte erst bereit, wenn es beide wären.
    target.regroup_until = target.regroup_until.max(regrouping);
    fleets.refresh(data);
    Ok(())
}

/// Löst die genannten Schiffe aus einer Flotte und stellt mit ihnen im selben System
/// eine neue auf. Mindestens ein Schiff bleibt zurück.
pub fn split_fleet(
    data: &GameData,
    polities: &Polities,
    fleets: &mut Fleets,
    polity: PolityId,
    fleet: FleetId,
    ships: &[ShipId],
) -> Result<FleetId, String> {
    let parent = own_fleet(fleets, polity, fleet)?;
    ready_to_reorganize(parent)?;
    if ships.is_empty() {
        return Err("keine Schiffe für die neue Flotte genannt".into());
    }
    for (index, ship) in ships.iter().enumerate() {
        if parent.ship(*ship).is_none() {
            return Err(format!("Schiff {ship} gehört nicht zu {}", parent.name));
        }
        if ships[..index].contains(ship) {
            return Err(format!("Schiff {ship} ist mehrfach genannt"));
        }
    }
    if ships.len() >= parent.ships.len() {
        return Err("mindestens ein Schiff muss in der Flotte bleiben".into());
    }
    let (system, stance, formation, doctrine, came_from, regroup_until) = (
        parent.system,
        parent.stance,
        parent.formation,
        parent.doctrine,
        parent.came_from,
        parent.regroup_until,
    );
    // Die Schiffe behalten ihre Reihenfolge aus der alten Flotte.
    let (taken, kept): (Vec<Ship>, Vec<Ship>) = std::mem::take(&mut parent.ships)
        .into_iter()
        .partition(|ship| ships.contains(&ship.id));
    parent.ships = kept;
    let id = fleets.new_fleet(polities, polity, system);
    let created = &mut fleets.list[id.index()];
    created.ships = taken;
    created.stance = stance;
    created.formation = formation;
    created.doctrine = doctrine;
    // Auch Herkunft und Sammelfrist gehen mit: Teilen macht keine Flotte kampfbereit.
    created.came_from = came_from;
    created.regroup_until = regroup_until;
    fleets.refresh(data);
    Ok(id)
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use crate::galaxy::generate_galaxy;
    use crate::polity::found_polities;

    /// Galaxie, Gemeinschaften und Startflotten eines festen Seeds.
    pub fn setup() -> (GameData, Galaxy, Polities, Fleets) {
        let data = GameData::load_workspace().unwrap();
        let mut galaxy = generate_galaxy(&data, 9);
        let polities = found_polities(&data, &mut galaxy, 9);
        let fleets = setup_fleets(&data, &galaxy, &polities);
        (data, galaxy, polities, fleets)
    }
}

#[cfg(test)]
mod tests {
    use super::testing::setup;
    use super::*;

    #[test]
    fn every_polity_starts_with_the_fleet_from_the_data() {
        let (data, galaxy, polities, fleets) = setup();
        assert_eq!(fleets.list.len(), polities.list.len());
        let expected: Vec<u16> = data
            .balance
            .military
            .starting_fleet_ids
            .iter()
            .flat_map(|(design, count)| std::iter::repeat_n(*design, *count as usize))
            .collect();
        assert!(!expected.is_empty());
        for (fleet, polity) in fleets.list.iter().zip(&polities.list) {
            assert_eq!(fleet.owner, polity.id);
            assert_eq!(fleet.system, galaxy.planet(polity.capital.unwrap()).system);
            let designs: Vec<u16> = fleet.ships.iter().map(|ship| ship.design).collect();
            assert_eq!(designs, expected);
            assert!(
                fleet
                    .ships
                    .iter()
                    .all(|ship| !ship.is_damaged(&data) && ship.ammo == 1.0 && ship.fuel == 1.0)
            );
            // Die unversehrte Startflotte ist das Maß der Stärke.
            assert!((fleet.strength - data.balance.fleets.start_strength).abs() < 1e-3);
        }
        assert_eq!(
            fleets.next_ship as usize,
            expected.len() * polities.list.len()
        );
        assert_eq!(fleets.find_anomaly(&data), None);
    }

    #[test]
    fn strength_follows_value_and_condition() {
        let (data, _, _, mut fleets) = setup();
        let full = fleets.list[0].strength;
        let per_value = data.balance.military.strength_per_value;
        let value = |fleets: &Fleets, index: usize| {
            data.designs[fleets.list[0].ships[index].design as usize]
                .stats
                .value
        };

        // Ein halb zerstörter Rumpf zählt halb.
        fleets.list[0].ships[0].hull = 0.5;
        fleets.refresh(&data);
        let expected = full as f64 - 0.5 * value(&fleets, 0) * per_value;
        assert!((fleets.list[0].strength as f64 - expected).abs() < 1e-3);

        // Ausgefallene Module mindern den Wert um ihr Gewicht, aber nicht auf null.
        let slots = data.designs[fleets.list[0].ships[1].design as usize]
            .modules
            .len();
        fleets.list[0].ships[1].modules = vec![0.0; slots];
        fleets.refresh(&data);
        let weight = data.balance.military.module_weight as f64;
        assert!(weight > 0.0 && weight < 1.0);
        let expected = expected - weight * value(&fleets, 1) * per_value;
        assert!((fleets.list[0].strength as f64 - expected).abs() < 1e-3);
        // Halb beschädigte Module, halber Abzug.
        fleets.list[0].ships[1].modules = vec![0.5; slots];
        fleets.refresh(&data);
        let expected = expected + 0.5 * weight * value(&fleets, 1) * per_value;
        assert!((fleets.list[0].strength as f64 - expected).abs() < 1e-3);

        // Ein zusätzliches Schiff hebt die Stärke um seinen Bauwert.
        let before = fleets.list[0].strength;
        let cruiser = data.design_index("speer").unwrap();
        fleets.add_ship(&data, FleetId(0), cruiser).unwrap();
        let gain = data.designs[cruiser as usize].stats.value * per_value;
        assert!((fleets.list[0].strength as f64 - before as f64 - gain).abs() < 1e-3);

        // Ohne Schiffe besteht die Flotte nicht mehr.
        fleets.list[0].ships.clear();
        fleets.refresh(&data);
        assert!(!fleets.list[0].alive);
        assert_eq!(fleets.list[0].strength, 0.0);
    }

    #[test]
    fn supply_and_day_range_reflect_the_ships() {
        let (data, _, _, mut fleets) = setup();
        let speed = data.balance.fleets.speed;
        assert_eq!(fleets.list[0].supply(&data), (1.0, 1.0));
        assert_eq!(fleets.list[0].day_range(&data), speed);
        // Gewichtet mit dem Fassungsvermögen: Der Kreuzer fasst weit mehr als eine Korvette.
        let total: f32 = fleets.list[0]
            .ships
            .iter()
            .map(|ship| data.designs[ship.design as usize].stats.ammo)
            .sum();
        let cruiser = data.designs[fleets.list[0].ships[0].design as usize]
            .stats
            .ammo;
        fleets.list[0].ships[0].ammo = 0.0;
        let (ammo, fuel) = fleets.list[0].supply(&data);
        assert!((ammo - (1.0 - cruiser / total)).abs() < 1e-6 && fuel == 1.0);
        assert!(
            ammo < 0.5,
            "der Kreuzer trägt den größten Teil der Munition"
        );
        // Ein leerer Tank drosselt die ganze Flotte.
        fleets.list[0].ships[2].fuel = 0.0;
        assert_eq!(fleets.list[0].min_fuel(), 0.0);
        let crawl = speed * data.balance.military.dry_speed;
        assert!((fleets.list[0].day_range(&data) - crawl).abs() < 1e-4);
    }

    #[test]
    fn the_anomaly_check_finds_what_it_should() {
        let (data, _, _, clean) = setup();
        assert_eq!(clean.find_anomaly(&data), None);
        let broken = |change: fn(&mut Fleets)| {
            let mut fleets = clean.clone();
            change(&mut fleets);
            fleets.find_anomaly(&data)
        };
        // Schiffswerte außerhalb 0..1 oder nicht endlich.
        assert!(broken(|fleets| fleets.list[0].ships[0].hull = 1.2).is_some());
        assert!(broken(|fleets| fleets.list[0].ships[0].ammo = -0.1).is_some());
        assert!(broken(|fleets| fleets.list[0].ships[0].fuel = f32::NAN).is_some());
        assert!(broken(|fleets| fleets.list[0].ships[0].modules = vec![1.0]).is_some());
        assert!(broken(|fleets| fleets.list[0].ships[0].fighters = 3).is_some());
        assert!(broken(|fleets| fleets.list[0].ships[0].design = 9_999).is_some());
        // Eine bestehende Flotte ohne Schiffe; eine doppelte Schiffsnummer.
        assert!(broken(|fleets| fleets.list[0].ships.clear()).is_some());
        assert!(
            broken(|fleets| fleets.list[1].ships[0].id = fleets.list[0].ships[0].id)
                .unwrap()
                .contains("doppelt")
        );
        assert!(broken(|fleets| fleets.next_ship = 3).is_some());
        // Eine Stärke, die nicht endlich ist oder nicht zu den Schiffen passt.
        assert!(broken(|fleets| fleets.list[0].strength = f32::INFINITY).is_some());
        assert!(
            broken(|fleets| fleets.list[0].strength = 250.0)
                .unwrap()
                .contains("passt nicht")
        );
        // Ein beschädigtes Schiff ist kein Fehler, solange die Stärke nachgeführt ist.
        let mut damaged = clean.clone();
        damaged.list[0].ships[0].hull = 0.3;
        assert!(damaged.find_anomaly(&data).is_some());
        damaged.refresh(&data);
        assert_eq!(damaged.find_anomaly(&data), None);
        // Bauaufträge und Soldrückstand.
        let order = BuildOrder {
            id: 0,
            polity: PolityId(0),
            planet: PlanetId(0),
            design: 1,
            progress: 0.5,
            spent: 0,
            stalled: false,
            started: 0,
        };
        let mut with_order = clean.clone();
        with_order.orders.push(order.clone());
        assert!(
            with_order.find_anomaly(&data).is_some(),
            "Nummer jenseits des Zählers"
        );
        with_order.next_order = 1;
        assert_eq!(with_order.find_anomaly(&data), None);
        with_order.orders[0].progress = 1.5;
        assert!(with_order.find_anomaly(&data).is_some());
        with_order.orders[0].progress = 0.5;
        with_order.orders.push(order);
        with_order.next_order = 5;
        assert!(with_order.find_anomaly(&data).is_some(), "Nummer doppelt");
        assert!(broken(|fleets| fleets.arrears = vec![0.0, 1.5]).is_some());
    }

    #[test]
    fn carriers_count_their_craft_separately() {
        let (data, _, _, mut fleets) = setup();
        let carrier = data.design_index("horst").unwrap();
        let id = fleets.add_ship(&data, FleetId(0), carrier).unwrap();
        let stats = &data.designs[carrier as usize].stats;
        let craft = data.designs[stats.craft.unwrap() as usize].stats.value;
        let ship = fleets.list[0].ship(id).unwrap().clone();
        assert_eq!(ship.fighters, stats.fighters);
        assert!((combat_value(&data, &ship) - stats.value).abs() < 1e-6);
        // Ohne Jäger fehlt genau deren Bauwert.
        let empty = Ship {
            fighters: 0,
            ..ship.clone()
        };
        assert!(empty.is_damaged(&data));
        let lost = stats.value - combat_value(&data, &empty);
        assert!((lost - craft * stats.fighters as f64).abs() < 1e-6);
        // Die Übergabe an den Gefechtsrechner trägt den Zustand unverändert.
        let spec = empty.spec();
        assert_eq!(
            (spec.source, spec.design, spec.fighters),
            (Some(id), carrier, 0)
        );
        assert_eq!((spec.hull, spec.ammo, spec.fuel), (1.0, 1.0, 1.0));
        let specs = fleets.list[0].specs();
        assert_eq!(specs.len(), fleets.list[0].ships.len());
        assert_eq!(specs.last().unwrap().fighters, stats.fighters);
    }

    #[test]
    fn merging_and_splitting_keep_every_ship() {
        let (data, galaxy, polities, mut fleets) = setup();
        let me = PolityId(0);
        let all: Vec<ShipId> = fleets.list[0].ships.iter().map(|ship| ship.id).collect();
        let strength = fleets.list[0].strength;

        // Abgelehnt: nichts genannt, fremdes Schiff, doppelt genannt, alle Schiffe.
        let foreign = fleets.list[1].ships[0].id;
        let split = |fleets: &mut Fleets, ships: &[ShipId]| {
            split_fleet(&data, &polities, fleets, me, FleetId(0), ships)
        };
        assert!(split(&mut fleets, &[]).is_err());
        assert!(split(&mut fleets, &[foreign]).is_err());
        assert!(split(&mut fleets, &[all[0], all[0]]).is_err());
        assert!(split(&mut fleets, &all).is_err());
        assert!(
            split_fleet(
                &data,
                &polities,
                &mut fleets,
                PolityId(1),
                FleetId(0),
                &all[..1]
            )
            .is_err()
        );
        assert_eq!(fleets.list.len(), polities.list.len(), "nichts entstanden");

        let second = split(&mut fleets, &all[..2]).unwrap();
        assert_eq!(fleets.list[second.index()].ships.len(), 2);
        assert_eq!(fleets.list[0].ships.len(), all.len() - 2);
        assert_eq!(fleets.list[second.index()].system, fleets.list[0].system);
        assert!(fleets.list[second.index()].name.starts_with("2. Flotte"));
        let total = fleets.list[0].strength + fleets.list[second.index()].strength;
        assert!((total - strength).abs() < 1e-3);
        assert_eq!(fleets.find_anomaly(&data), None);

        // Abgelehnt: mit sich selbst, fremde Flotte, anderes System, unterwegs.
        assert!(merge_fleets(&data, &mut fleets, me, second, second).is_err());
        assert!(merge_fleets(&data, &mut fleets, me, FleetId(1), FleetId(0)).is_err());
        let elsewhere = galaxy.neighbors(fleets.list[0].system)[0].system;
        order_move(&galaxy, &mut fleets, me, second, elsewhere).unwrap();
        move_fleets(&galaxy, &mut fleets, &data);
        assert!(merge_fleets(&data, &mut fleets, me, second, FleetId(0)).is_err());
        for _ in 0..200 {
            move_fleets(&galaxy, &mut fleets, &data);
        }
        assert_eq!(fleets.list[second.index()].system, elsewhere);
        assert!(merge_fleets(&data, &mut fleets, me, second, FleetId(0)).is_err());
        order_move(&galaxy, &mut fleets, me, FleetId(0), elsewhere).unwrap();
        for _ in 0..200 {
            move_fleets(&galaxy, &mut fleets, &data);
        }

        merge_fleets(&data, &mut fleets, me, second, FleetId(0)).unwrap();
        assert!(!fleets.list[second.index()].alive);
        let mut merged: Vec<ShipId> = fleets.list[0].ships.iter().map(|ship| ship.id).collect();
        merged.sort_unstable();
        assert_eq!(merged, all, "kein Schiff verloren, keines doppelt");
        assert_eq!(fleets.find_anomaly(&data), None);
        // Die aufgelöste Flotte nimmt keine Befehle mehr an.
        assert!(merge_fleets(&data, &mut fleets, me, FleetId(0), second).is_err());
    }

    #[test]
    fn the_old_layout_still_parses_so_that_old_saves_fail_on_their_format() {
        // So sah eine Flotte im Spielstandformat 1 aus: eine Stärkezahl, keine Schiffe.
        let old = "(list:[(id:0,owner:0,name:\"1. Flotte\",strength:100.0,stance:Guard,system:3,\
                   route:[],progress:0.0,alive:true,in_battle:false)],blockaded:[])";
        let fleets: Fleets = ron::from_str(old).expect("altes Format lesbar");
        assert!(fleets.list[0].ships.is_empty() && fleets.orders.is_empty());
        // Auch die Felder der Gefechte fehlen dort und nehmen ihre Vorgabewerte an.
        assert!(fleets.list[0].battle.is_none() && fleets.list[0].regroup_until.is_none());
        // Als Zustand einer Partie taugt das nicht: Deshalb lehnt `SaveGame` die alten
        // Formate ab.
        let data = GameData::load_workspace().unwrap();
        assert!(fleets.find_anomaly(&data).is_some());
        let mut save = crate::state::SaveGame {
            format: 1,
            data_hash: data.hash,
            state: crate::state::SimState::default(),
        };
        for format in 1..crate::state::SAVE_FORMAT {
            save.format = format;
            let text = save.to_ron().unwrap();
            assert!(crate::state::SaveGame::from_ron(&text).is_err(), "{format}");
        }
        save.format = crate::state::SAVE_FORMAT;
        assert!(crate::state::SaveGame::from_ron(&save.to_ron().unwrap()).is_ok());
    }

    #[test]
    fn the_regroup_period_survives_splitting_and_merging() {
        let (data, _, polities, mut fleets) = setup();
        let me = PolityId(0);
        fleets.list[0].regroup_until = Some(50);
        fleets.list[0].came_from = Some(SystemId(3));
        let part: Vec<ShipId> = fleets.list[0].ships[..2]
            .iter()
            .map(|ship| ship.id)
            .collect();
        // Teilen macht keine Flotte kampfbereit: Auch der abgelöste Teil sammelt sich noch
        // und weiß, woher er kam.
        let second = split_fleet(&data, &polities, &mut fleets, me, FleetId(0), &part).unwrap();
        let detached = &fleets.list[second.index()];
        assert_eq!(detached.regroup_until, Some(50));
        assert_eq!(detached.came_from, Some(SystemId(3)));
        assert!(!detached.is_ready() && detached.is_regrouping());
        // Vereinigen ebenso wenig: Die längere Sammelfrist gilt für die ganze Flotte.
        fleets.list[0].regroup_until = Some(20);
        merge_fleets(&data, &mut fleets, me, second, FleetId(0)).unwrap();
        assert_eq!(fleets.list[0].regroup_until, Some(50));
        let third = split_fleet(&data, &polities, &mut fleets, me, FleetId(0), &part).unwrap();
        fleets.list[0].regroup_until = None;
        assert!(fleets.list[0].is_ready());
        merge_fleets(&data, &mut fleets, me, third, FleetId(0)).unwrap();
        assert_eq!(fleets.list[0].regroup_until, Some(50));
        // Die aufgelöste Flotte trägt keine Frist mehr.
        assert_eq!(fleets.list[third.index()].regroup_until, None);
        assert_eq!(fleets.find_anomaly(&data), None);
    }

    #[test]
    fn only_ready_fleets_raid_or_escort() {
        let (_, _, _, mut fleets) = setup();
        let mut diplomacy = Diplomacy::default();
        diplomacy.wars.insert((PolityId(0), PolityId(1)));
        let system = fleets.list[1].system;
        fleets.list[0].system = system;
        fleets.list[0].stance = Stance::Raid;
        fleets.list[1].stance = Stance::Escort;
        let (raider, escort) = (fleets.list[0].strength, fleets.list[1].strength);
        let pressure = |fleets: &Fleets| fleets.raid_pressure(&diplomacy, system, PolityId(1));
        assert_eq!(pressure(&fleets), (raider, escort, Some(PolityId(0))));
        // Im Gefecht kapert niemand; wer sich sammelt, gibt kein Geleit.
        fleets.list[0].in_battle = true;
        assert_eq!(pressure(&fleets), (0.0, escort, None));
        fleets.list[0].in_battle = false;
        fleets.list[1].regroup_until = Some(12);
        assert_eq!(pressure(&fleets), (raider, 0.0, Some(PolityId(0))));
        assert!(!fleets.list[1].is_ready() && fleets.list[1].is_regrouping());
        assert!(fleets.list[0].is_ready());
    }

    #[test]
    fn standing_orders_belong_to_the_owner() {
        let (_, _, _, mut fleets) = setup();
        set_fleet_orders(
            &mut fleets,
            PolityId(0),
            FleetId(0),
            Formation::Wedge,
            Doctrine::Aggressive,
        )
        .unwrap();
        assert_eq!(fleets.list[0].formation, Formation::Wedge);
        assert_eq!(fleets.list[0].doctrine, Doctrine::Aggressive);
        assert!(
            set_fleet_orders(
                &mut fleets,
                PolityId(1),
                FleetId(0),
                Formation::Line,
                Doctrine::Defensive
            )
            .is_err()
        );
        assert!(
            set_fleet_orders(
                &mut fleets,
                PolityId(0),
                FleetId(99),
                Formation::Line,
                Doctrine::Defensive
            )
            .is_err()
        );
        assert_eq!(fleets.list[0].formation, Formation::Wedge);
    }
}

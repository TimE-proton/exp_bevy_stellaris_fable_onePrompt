//! Raumgefecht: der taktische Rechner.
//!
//! Ein Gefecht ist reiner Zustand plus eine Schrittfunktion. Gerechnet wird in einer
//! Ebene, mit festen 0,1 Sekunden Gefechtszeit je Takt. Schiffe, Geschosse und Effekte
//! liegen in dichten Feldern; ein uniformes Gitter beantwortet die Zielsuche. Derselbe
//! Rechner löst jedes Gefecht der Partie auf, ob jemand zusieht oder nicht.
//!
//! Was sich daraus ergibt statt festgelegt zu sein:
//! - Konter zwischen Klassen folgen aus Größe, Wendigkeit, Nachführung der Waffen und
//!   Flugzeit der Geschosse.
//! - Flankieren lohnt, weil das Heck dünn gepanzert ist, dort die Antriebe sitzen und
//!   nur wenige Türme nach achtern wirken.
//! - Nachschub zählt, weil Geschütze und Lenkwaffen Munition verbrauchen.

mod formation;
mod grid;
pub mod matchup;
pub mod math;
mod step;

pub use math::V2;

use crate::data::{DesignDef, GameData, ModuleKind, PerDoctrine, ShipClass, WeaponKind};
use crate::ids::{BattleId, FleetId, PolityId, ShipId, SystemId};
use grid::Grid;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Formation {
    /// Schlachtlinie: schwere Schiffe nebeneinander, Geleit davor, Korvetten an den Enden.
    #[default]
    Line,
    /// Keil: das stärkste Schiff an der Spitze, die übrigen gestaffelt dahinter.
    Wedge,
    /// Schirm: Geleit in weitem Bogen voraus, schwere Schiffe dicht in der Mitte.
    Screen,
    /// Zange: zwei Flügel weit außen, ein schwächeres Zentrum hält die Mitte.
    Pincer,
}

impl Formation {
    pub const ALL: [Formation; 4] = [
        Formation::Line,
        Formation::Wedge,
        Formation::Screen,
        Formation::Pincer,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Formation::Line => "Linie",
            Formation::Wedge => "Keil",
            Formation::Screen => "Schirm",
            Formation::Pincer => "Zange",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Doctrine {
    /// Dicht heran, spät zurück.
    Aggressive,
    #[default]
    Balanced,
    /// Auf größte Reichweite halten, früh lösen.
    Defensive,
}

impl Doctrine {
    pub const ALL: [Doctrine; 3] = [
        Doctrine::Aggressive,
        Doctrine::Balanced,
        Doctrine::Defensive,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Doctrine::Aggressive => "offensiv",
            Doctrine::Balanced => "ausgewogen",
            Doctrine::Defensive => "defensiv",
        }
    }

    pub fn pick(self, values: &PerDoctrine) -> f32 {
        match self {
            Doctrine::Aggressive => values.0,
            Doctrine::Balanced => values.1,
            Doctrine::Defensive => values.2,
        }
    }
}

/// Aufschläge aus der Technologie einer Gemeinschaft (1 = keine).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CombatModifiers {
    pub damage: f32,
    pub range: f32,
    pub shields: f32,
    pub armor: f32,
    pub hull: f32,
    pub speed: f32,
}

impl Default for CombatModifiers {
    fn default() -> Self {
        Self {
            damage: 1.0,
            range: 1.0,
            shields: 1.0,
            armor: 1.0,
            hull: 1.0,
            speed: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShipStatus {
    #[default]
    Active,
    /// Moral gebrochen oder Rückzug befohlen: Das Schiff verlässt das Feld.
    Routing,
    /// Dem Gefecht entkommen.
    Escaped,
    /// Ohne Energie: treibt, schießt nicht mehr.
    Disabled,
    Destroyed,
}

impl ShipStatus {
    /// Noch auf dem Feld und damit ein mögliches Ziel.
    pub fn in_play(self) -> bool {
        matches!(
            self,
            ShipStatus::Active | ShipStatus::Routing | ShipStatus::Disabled
        )
    }

    /// Kann steuern und schießen.
    pub fn can_act(self) -> bool {
        matches!(self, ShipStatus::Active | ShipStatus::Routing)
    }
}

/// Zustand eines Moduls im Gefecht. Die Stelle im Feld entspricht der im Entwurf.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ModuleState {
    pub hp: f32,
    pub cooldown: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BattleShip {
    /// Schiff der strategischen Ebene; `None` für Bordjäger.
    pub source: Option<ShipId>,
    /// Partei (Index in [`Battle::combatants`]).
    pub side: u16,
    pub design: u16,
    pub class: ShipClass,
    pub status: ShipStatus,
    pub pos: V2,
    pub vel: V2,
    pub heading: f32,
    pub speed: f32,
    pub hull: f32,
    pub hull_max: f32,
    pub shield: f32,
    pub shield_max: f32,
    pub shield_regen: f32,
    /// Panzerung je Seite: Bug, Steuerbord, Heck, Backbord.
    pub armor: [f32; 4],
    pub armor_max: [f32; 4],
    pub modules: Vec<ModuleState>,
    pub ammo: f32,
    pub ammo_max: f32,
    pub fuel: f32,
    pub fuel_max: f32,
    pub morale: f32,
    pub target: Option<u32>,
    /// Platz in der Formation, im Bezugssystem der Flotte (x voraus, y nach Backbord).
    pub slot: V2,
    /// Mutterschiff eines Bordjägers.
    pub carrier: Option<u32>,
    /// Träger: Jäger, die noch im Hangar stehen, und Zeit bis zum nächsten Start.
    pub hangar: u32,
    pub launch_timer: f32,
    /// Jäger: verbleibende Zeit des Abdrehens nach einem Anflug.
    pub breakaway: f32,
    // --- Kennwerte nach Technologie und Schäden ---
    pub max_speed: f32,
    pub accel: f32,
    pub turn: f32,
    pub radius: f32,
    pub signature: f32,
    pub main_range: f32,
    pub broadside: f32,
    /// Bauwert; Maß des Kampfwerts.
    pub value: f32,
    /// Anteil des Strombedarfs, den die Reaktoren decken (0..1).
    pub power: f32,
    /// Anteil der Antriebsleistung, der noch arbeitet (0..1).
    pub mobility: f32,
}

/// Womit ein Schiff ins Gefecht geht.
#[derive(Clone, Debug)]
pub struct ShipSpec {
    pub source: Option<ShipId>,
    pub design: u16,
    /// Anteile 0..1.
    pub hull: f32,
    /// Zustand je Modul 0..1; leer heißt: alle heil.
    pub modules: Vec<f32>,
    pub ammo: f32,
    pub fuel: f32,
    /// Ausgangsmoral; `None` nimmt die der Partei.
    pub morale: Option<f32>,
    /// Bordjäger im Hangar.
    pub fighters: u32,
}

impl ShipSpec {
    /// Ein fabrikneues, voll versorgtes Schiff.
    pub fn fresh(data: &GameData, design: u16) -> Self {
        Self {
            source: None,
            design,
            hull: 1.0,
            modules: Vec::new(),
            ammo: 1.0,
            fuel: 1.0,
            morale: None,
            fighters: data.designs[design as usize].stats.fighters,
        }
    }
}

/// Einstellungen einer Partei.
#[derive(Clone, Debug)]
pub struct CombatantSetup {
    pub fleet: Option<FleetId>,
    pub owner: PolityId,
    pub formation: Formation,
    pub doctrine: Doctrine,
    pub modifiers: CombatModifiers,
    /// Ausgangsmoral der Besatzungen.
    pub morale: f32,
}

impl CombatantSetup {
    pub fn new(owner: PolityId, data: &GameData) -> Self {
        Self {
            fleet: None,
            owner,
            formation: Formation::default(),
            doctrine: Doctrine::default(),
            modifiers: CombatModifiers::default(),
            morale: data.balance.battle.morale_start,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Combatant {
    pub fleet: Option<FleetId>,
    pub owner: PolityId,
    pub formation: Formation,
    pub doctrine: Doctrine,
    pub modifiers: CombatModifiers,
    pub morale: f32,
    /// Bezugspunkt der Formation, ihre Blickrichtung und ihre Bewegung.
    pub anchor: V2,
    pub anchor_vel: V2,
    pub facing: f32,
    /// Richtung, aus der die Partei kam und in die sie sich zurückzieht.
    pub home_angle: f32,
    /// Umlaufsinn, wenn die Linie ihre Breitseite zeigt (+1 oder -1).
    pub orbit: f32,
    /// In Gefechtsreichweite.
    pub engaged: bool,
    pub retreating: bool,
    /// Schwerpunkt der Gegner, zuletzt bestimmt.
    pub enemy_center: V2,
    // --- aus der Zusammensetzung abgeleitet ---
    pub fleet_speed: f32,
    pub fleet_range: f32,
    pub broadside: bool,
    // --- Bilanz ---
    pub initial: [u32; 6],
    pub lost: [u32; 6],
    pub escaped: [u32; 6],
    pub initial_value: f32,
    pub lost_value: f32,
}

/// Geschoss oder Lenkwaffe im Flug.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Shot {
    pub pos: V2,
    pub vel: V2,
    pub side: u16,
    pub kind: WeaponKind,
    /// Ziel einer Lenkwaffe.
    pub target: Option<u32>,
    pub damage: f32,
    pub vs: [f32; 3],
    /// Verbleibende Flugzeit.
    pub ttl: f32,
    /// Strukturpunkte einer Lenkwaffe gegen Punktverteidigung.
    pub hp: f32,
    pub speed: f32,
    pub turn: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BeamKind {
    Light,
    Heavy,
    PointDefense,
}

/// Strahl, der gerade zu sehen ist. Nur für die Darstellung; klingt nach Sekundenbruchteilen ab.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Beam {
    pub from: V2,
    pub to: V2,
    pub side: u16,
    pub kind: BeamKind,
    pub hit: bool,
    pub ttl: f32,
}

/// Einschlag oder Explosion, nur für die Darstellung.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Impact {
    pub pos: V2,
    pub size: f32,
    pub ttl: f32,
}

/// Wrack eines zerstörten Schiffs. Treibt weiter und bleibt als Trümmerfeld zurück.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Wreck {
    pub pos: V2,
    pub vel: V2,
    pub heading: f32,
    pub class: ShipClass,
    pub side: u16,
    pub radius: f32,
    /// Sekunden seit der Zerstörung.
    pub age: f32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Battle {
    pub id: BattleId,
    pub system: SystemId,
    pub seed: u64,
    /// Takte seit Beginn und Gefechtszeit in Sekunden.
    pub tick: u64,
    pub time: f32,
    pub combatants: Vec<Combatant>,
    /// Wer gegen wen kämpft: `hostile[a * n + b]`.
    pub hostile: Vec<bool>,
    pub ships: Vec<BattleShip>,
    pub shots: Vec<Shot>,
    pub beams: Vec<Beam>,
    pub impacts: Vec<Impact>,
    pub wrecks: Vec<Wreck>,
    pub finished: bool,
    /// Ohne Entscheidung an der Zeitgrenze beendet.
    pub timed_out: bool,
    #[serde(skip)]
    grid: Grid,
    #[serde(skip)]
    scratch: Vec<u32>,
}

impl Battle {
    pub fn new(id: BattleId, system: SystemId, seed: u64) -> Self {
        Self {
            id,
            system,
            seed,
            ..Default::default()
        }
    }

    #[inline]
    pub fn is_hostile(&self, a: u16, b: u16) -> bool {
        a != b && self.hostile[a as usize * self.combatants.len() + b as usize]
    }

    pub fn set_hostile(&mut self, a: usize, b: usize, hostile: bool) {
        let count = self.combatants.len();
        if a != b && a < count && b < count {
            self.hostile[a * count + b] = hostile;
            self.hostile[b * count + a] = hostile;
        }
    }

    /// Nimmt eine Partei auf, stellt sie am Rand des Feldes auf und gibt ihren Index zurück.
    /// Geht auch mitten im Gefecht: So treffen Verstärkungen ein.
    pub fn add_combatant(
        &mut self,
        data: &GameData,
        setup: CombatantSetup,
        ships: &[ShipSpec],
    ) -> usize {
        let balance = &data.balance.battle;
        let side = self.combatants.len();
        // Die ersten beiden Parteien stehen einander gegenüber, weitere dazwischen.
        let home_angle = match side {
            0 => std::f32::consts::PI,
            1 => 0.0,
            2 => std::f32::consts::FRAC_PI_2,
            3 => -std::f32::consts::FRAC_PI_2,
            more => math::wrap_angle(more as f32 * 2.399_963),
        };
        let facing = math::wrap_angle(home_angle + std::f32::consts::PI);
        let anchor = V2::from_angle(home_angle) * balance.spawn_radius;

        // Feindschaftsmatrix um eine Zeile und Spalte erweitern.
        let old = side;
        let mut hostile = vec![false; (old + 1) * (old + 1)];
        for a in 0..old {
            for b in 0..old {
                hostile[a * (old + 1) + b] = self.hostile[a * old + b];
            }
        }
        self.hostile = hostile;

        let mut combatant = Combatant {
            fleet: setup.fleet,
            owner: setup.owner,
            formation: setup.formation,
            doctrine: setup.doctrine,
            modifiers: setup.modifiers,
            morale: setup.morale,
            anchor,
            anchor_vel: V2::ZERO,
            facing,
            home_angle,
            orbit: if side.is_multiple_of(2) { 1.0 } else { -1.0 },
            engaged: false,
            retreating: false,
            enemy_center: V2::ZERO,
            fleet_speed: 30.0,
            fleet_range: 500.0,
            broadside: false,
            initial: [0; 6],
            lost: [0; 6],
            escaped: [0; 6],
            initial_value: 0.0,
            lost_value: 0.0,
        };
        let first = self.ships.len();
        for spec in ships {
            let ship = make_ship(data, side as u16, spec, &combatant);
            combatant.initial[ship.class.index()] += 1;
            combatant.initial_value += ship.value;
            // Bordjäger zählen von Anfang an zur Stärke.
            if let Some(craft) = data.designs[spec.design as usize].stats.craft {
                combatant.initial[ShipClass::Fighter.index()] += ship.hangar;
                combatant.initial_value +=
                    ship.hangar as f32 * data.designs[craft as usize].stats.value as f32;
            }
            self.ships.push(ship);
        }
        self.combatants.push(combatant);
        self.refresh_fleet_profile(side);
        let members: Vec<usize> = (first..self.ships.len()).collect();
        formation::assign_slots(
            &mut self.ships,
            &members,
            self.combatants[side].formation,
            balance.formation_spacing,
        );
        for index in members {
            let ship = &mut self.ships[index];
            ship.pos = anchor + ship.slot.rotated(facing);
            ship.heading = facing;
        }
        side
    }

    /// Geschwindigkeit, Gefechtsentfernung und Breitseitenneigung einer Partei aus den
    /// Schiffen, die sie noch hat.
    fn refresh_fleet_profile(&mut self, side: usize) {
        let (mut slowest, mut range_sum, mut weight, mut broad, mut broad_weight) =
            (f32::MAX, 0.0, 0.0, 0.0, 0.0);
        for ship in self
            .ships
            .iter()
            .filter(|ship| ship.side as usize == side && ship.status.can_act())
        {
            if ship.class == ShipClass::Fighter {
                continue;
            }
            slowest = slowest.min(ship.max_speed);
            if ship.class != ShipClass::Carrier {
                range_sum += ship.main_range * ship.value;
                weight += ship.value;
            }
            if matches!(ship.class, ShipClass::Cruiser | ShipClass::Battleship) {
                broad += ship.broadside * ship.value;
                broad_weight += ship.value;
            }
        }
        let combatant = &mut self.combatants[side];
        combatant.fleet_speed = if slowest == f32::MAX {
            60.0
        } else {
            slowest * 0.9
        };
        combatant.fleet_range = if weight > 0.0 {
            range_sum / weight
        } else {
            500.0
        };
        combatant.broadside = broad_weight > 0.0 && broad / broad_weight > 1.05;
    }

    /// Ändert Formation und Doktrin einer Partei mitten im Gefecht.
    pub fn set_orders(
        &mut self,
        data: &GameData,
        side: usize,
        formation: Formation,
        doctrine: Doctrine,
    ) {
        let Some(combatant) = self.combatants.get_mut(side) else {
            return;
        };
        combatant.doctrine = doctrine;
        if combatant.formation != formation {
            combatant.formation = formation;
            let members: Vec<usize> = (0..self.ships.len())
                .filter(|index| {
                    let ship = &self.ships[*index];
                    ship.side as usize == side
                        && ship.status.can_act()
                        && ship.class != ShipClass::Fighter
                })
                .collect();
            formation::assign_slots(
                &mut self.ships,
                &members,
                formation,
                data.balance.battle.formation_spacing,
            );
        }
    }

    /// Befiehlt den Rückzug: Alle Schiffe der Partei verlassen das Feld.
    pub fn order_retreat(&mut self, side: usize) {
        let Some(combatant) = self.combatants.get_mut(side) else {
            return;
        };
        combatant.retreating = true;
        for ship in self
            .ships
            .iter_mut()
            .filter(|ship| ship.side as usize == side)
        {
            if ship.status == ShipStatus::Active {
                ship.status = ShipStatus::Routing;
            }
        }
    }

    /// Kampfwert der Schiffe einer Partei, die noch kämpfen (samt Jägern im Hangar).
    pub fn fighting_value(&self, data: &GameData, side: usize) -> f32 {
        self.ships
            .iter()
            .filter(|ship| ship.side as usize == side && ship.status == ShipStatus::Active)
            .map(|ship| {
                let craft = data.designs[ship.design as usize].stats.craft;
                let hangar = craft.map_or(0.0, |craft| {
                    ship.hangar as f32 * data.designs[craft as usize].stats.value as f32
                });
                ship.value * (0.3 + 0.7 * ship.hull / ship.hull_max.max(1e-6)) + hangar
            })
            .sum()
    }

    /// Zahl der Schiffe einer Partei je Klasse, die noch auf dem Feld kämpfen.
    pub fn active_by_class(&self, side: usize) -> [u32; 6] {
        let mut counts = [0; 6];
        for ship in self.ships.iter().filter(|ship| ship.side as usize == side) {
            if ship.status == ShipStatus::Active {
                counts[ship.class.index()] += 1;
            }
        }
        counts
    }

    /// Hat die Partei noch kämpfende Schiffe?
    pub fn is_fighting(&self, side: usize) -> bool {
        self.ships
            .iter()
            .any(|ship| ship.side as usize == side && ship.status == ShipStatus::Active)
    }

    /// Parteien, die am Ende noch kämpfende Schiffe auf dem Feld haben.
    pub fn victors(&self) -> Vec<usize> {
        (0..self.combatants.len())
            .filter(|side| self.is_fighting(*side))
            .collect()
    }
}

/// Baut ein Schiff für das Gefecht aus Entwurf, Zustand und Technologie seiner Partei.
fn make_ship(data: &GameData, side: u16, spec: &ShipSpec, combatant: &Combatant) -> BattleShip {
    let design: &DesignDef = &data.designs[spec.design as usize];
    let stats = &design.stats;
    let modifiers = &combatant.modifiers;
    let hull_max = stats.hull * modifiers.hull;
    let condition = spec.hull.clamp(0.0, 1.0);
    let armor_max = stats.armor.map(|points| points * modifiers.armor);
    let modules: Vec<ModuleState> = design
        .modules
        .iter()
        .enumerate()
        .map(|(index, slot)| {
            let fraction = spec
                .modules
                .get(index)
                .copied()
                .unwrap_or(1.0)
                .clamp(0.0, 1.0);
            ModuleState {
                hp: data.modules[slot.module_id as usize].hp * fraction,
                cooldown: 0.0,
            }
        })
        .collect();
    let mut ship = BattleShip {
        source: spec.source,
        side,
        design: spec.design,
        class: stats.class,
        status: ShipStatus::Active,
        pos: V2::ZERO,
        vel: V2::ZERO,
        heading: 0.0,
        speed: 0.0,
        hull: (hull_max * condition).max(1.0),
        hull_max,
        shield: 0.0,
        shield_max: 0.0,
        shield_regen: 0.0,
        // Panzerung wird mit dem Rumpf instand gesetzt und ist so gut wie er.
        armor: armor_max.map(|points| points * condition),
        armor_max,
        modules,
        ammo: stats.ammo * spec.ammo.clamp(0.0, 1.0),
        ammo_max: stats.ammo,
        fuel: stats.fuel * spec.fuel.clamp(0.0, 1.0),
        fuel_max: stats.fuel,
        morale: spec.morale.unwrap_or(combatant.morale),
        target: None,
        slot: V2::ZERO,
        carrier: None,
        hangar: spec.fighters.min(stats.fighters),
        launch_timer: 3.0,
        breakaway: 0.0,
        max_speed: stats.speed * modifiers.speed,
        accel: stats.accel * modifiers.speed,
        turn: stats.turn,
        radius: stats.radius,
        signature: stats.signature,
        main_range: stats.main_range * modifiers.range,
        broadside: stats.broadside,
        value: stats.value as f32,
        power: 1.0,
        mobility: 1.0,
    };
    refresh_ship(data, &mut ship, modifiers);
    ship.shield = ship.shield_max;
    ship
}

/// Leitet aus dem Zustand der Module ab, was das Schiff noch kann: Schildstärke,
/// Energieversorgung, Beweglichkeit. Nach jedem Modulschaden aufzurufen.
pub(crate) fn refresh_ship(data: &GameData, ship: &mut BattleShip, modifiers: &CombatModifiers) {
    let design = &data.designs[ship.design as usize];
    let (mut output, mut draw) = (0.0f32, 0.0f32);
    let (mut thrust, mut thrust_full) = (0.0f32, 0.0f32);
    let (mut shield, mut regen) = (0.0f32, 0.0f32);
    for (slot, state) in design.modules.iter().zip(&ship.modules) {
        let module = &data.modules[slot.module_id as usize];
        let health = (state.hp / module.hp).clamp(0.0, 1.0);
        let working = state.hp > 0.0;
        if working {
            draw += module.power;
        }
        match &module.kind {
            ModuleKind::Reactor { output: value } => output += value * health,
            ModuleKind::Engine { thrust: value } => {
                thrust_full += value;
                thrust += value * health;
            }
            ModuleKind::Shield {
                capacity,
                regen: value,
            } if working => {
                // Ein beschädigter Generator hält weniger.
                shield += capacity * (0.5 + 0.5 * health);
                regen += value * health;
            }
            _ => {}
        }
    }
    ship.power = if draw > 0.0 {
        (output / draw).clamp(0.0, 1.0)
    } else {
        1.0
    };
    ship.mobility = if thrust_full > 0.0 {
        (thrust / thrust_full).clamp(0.0, 1.0)
    } else {
        0.0
    };
    ship.shield_max = shield * modifiers.shields;
    ship.shield_regen = regen * modifiers.shields;
    ship.shield = ship.shield.min(ship.shield_max);
}

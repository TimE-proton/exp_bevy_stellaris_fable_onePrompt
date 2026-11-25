//! Raumgefecht: der taktische Rechner.
//!
//! Ein Gefecht ist reiner Zustand plus eine Schrittfunktion. Gerechnet wird in einer
//! Ebene, mit festen 0,1 Sekunden Gefechtszeit je Takt. Schiffe, Geschosse und Effekte
//! liegen in dichten Feldern; ein uniformes Gitter beantwortet die Zielsuche. Derselbe
//! Rechner löst jedes Gefecht der Partie auf, ob jemand zusieht oder nicht.
//!
//! Was sich daraus ergibt statt festgelegt zu sein:
//! - Konter zwischen Klassen folgen aus Größe, Wendigkeit, Nachführung der Waffen,
//!   Flugzeit der Geschosse, Punktverteidigung und Reichweite; nirgends steht ein Bonus
//!   einer Klasse gegen eine andere.
//! - Flankieren lohnt, weil das Heck dünn gepanzert ist, dort die Antriebe sitzen und
//!   nur wenige Türme nach achtern wirken.
//! - Nachschub zählt, weil Geschütze und Lenkwaffen Munition verbrauchen.
//! - Moral setzt sich aus bleibendem Rumpfschaden, abklingendem Schreck und der Lage der
//!   eigenen Seite zusammen. Wer bricht, verlässt das Feld; ganze Flotten ziehen ab, wenn
//!   sie zu viel verloren haben und nicht klar vorn liegen.
//!
//! Gerecht zwischen den Seiten: Alle Entscheidungen eines Takts lesen den Zustand vom
//! Anfang des Takts, alle Treffer wirken gemeinsam am Ende (siehe `step`).

mod formation;
mod grid;
pub mod matchup;
pub mod math;
mod step;

pub use math::V2;

use crate::data::{
    DesignDef, DesignModule, GameData, ModuleKind, PerDoctrine, ShipClass, WeaponKind,
};
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
    /// Moral, mit der das Schiff ins Gefecht ging.
    pub morale_base: f32,
    /// Vorübergehender Schreck (positiv) oder Auftrieb (negativ); klingt ab.
    pub shock: f32,
    /// Sekunden auf der Flucht.
    pub rout_time: f32,
    /// Jäger: Sekunden, die er schon beim Träger nachlädt.
    pub rearm: f32,
    /// Hat noch eine Waffe, die feuern kann (oder Jäger im Hangar).
    pub armed: bool,
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
    /// Bauwert; Maß des Kampfwerts. Bei Trägern ohne die Bordjäger: Die zählen je Jäger,
    /// ob im Hangar oder in der Luft, sonst stünden sie doppelt in der Bilanz.
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

    /// Ausgangsmoral nach der Kriegsmüdigkeit (0..100) der eigenen Gemeinschaft: je Punkt
    /// `morale_per_weariness` weniger als `morale_start`. Für das Aufstellen aus der Partie.
    pub fn with_war_weariness(mut self, data: &GameData, weariness: f64) -> Self {
        let battle = &data.balance.battle;
        let weariness = if weariness.is_finite() {
            weariness.clamp(0.0, 100.0) as f32
        } else {
            0.0
        };
        self.morale = (battle.morale_start - battle.morale_per_weariness * weariness).max(0.0);
        self
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
    /// Umlaufsinn, wenn die Linie ihre Breitseite zeigt. Für alle Parteien gleich (+1):
    /// So kreisen die Flotten um ihre gemeinsame Mitte, statt nebeneinander vom Feld zu laufen.
    pub orbit: f32,
    /// In Gefechtsreichweite.
    pub engaged: bool,
    /// War schon einmal in Gefechtsreichweite.
    pub contact: bool,
    pub retreating: bool,
    /// Moralabzug, weil die eigene Seite unterlegen ist (0 bei Gleichstand oder Vorsprung).
    pub outlook: f32,
    /// Gefechtszeit, zu der die Partei zuletzt durch Schilde gedrungen ist.
    pub last_hit: f32,
    /// Gefechtszeit, zu der der Rückzug befohlen wurde.
    #[serde(default)]
    pub retreat_time: Option<f32>,
    /// Hat sich im Patt gelöst: Keine Seite kam lange durch die Schilde der anderen, beide
    /// zogen gemeinsam ab. Ein solches Gefecht hat keinen Sieger.
    #[serde(default)]
    pub stalemate: bool,
    /// Schaden an Panzerung und Rumpf des Gegners je Waffenart (Strahl, Geschütz,
    /// Lenkwaffe, Punktverteidigung), für Auswertungen.
    pub damage_dealt: [f32; 4],
    /// Schaden an Panzerung und Rumpf der eigenen Schiffe.
    pub damage_taken: f32,
    /// Schwerpunkt der Gegner, zuletzt bestimmt.
    pub enemy_center: V2,
    // --- aus der Zusammensetzung abgeleitet ---
    pub fleet_speed: f32,
    pub fleet_range: f32,
    /// Schwerpunkt der Schiffe, die `fleet_range` bestimmen: Von dort, nicht vom
    /// Bezugspunkt, wird der Abstand zum Gegner gemessen. Dazu ihr mittlerer Platz im
    /// Bezugssystem der Flotte, um zu sehen, wie weit sie dem Bezugspunkt nachhängen.
    #[serde(default)]
    pub range_center: V2,
    #[serde(default)]
    pub range_offset: V2,
    /// Träger ohne Jäger stehen in der Linie (einmal gesetzt, bleibt es).
    #[serde(default)]
    pub carriers_in_line: bool,
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
    /// Gefechtszeit, zu der zuerst eine Partei in Gefechtsreichweite kam.
    #[serde(default)]
    pub contact_time: Option<f32>,
    /// Ohne Entscheidung an der Zeitgrenze beendet.
    pub timed_out: bool,
    #[serde(skip)]
    grid: Grid,
    /// Lenkwaffen im Flug, für die Punktverteidigung; jeden Takt neu gefüllt.
    #[serde(skip)]
    missiles: Grid,
    #[serde(skip)]
    scratch: Vec<u32>,
    /// Treffer eines Takts; sie wirken erst, wenn alle geschossen haben. Am Ende jedes
    /// Takts leer.
    #[serde(skip)]
    hits: Vec<step::Hit>,
    #[serde(skip)]
    weights: Vec<f32>,
}

/// Was aus einem Schiff der strategischen Ebene geworden ist.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Fate {
    /// Noch auf dem Feld, als das Gefecht endete: kämpfend, oder treibend auf der Seite,
    /// die das Feld hält.
    Holding,
    /// Hat das Feld verlassen.
    Escaped,
    /// Zerstört, oder treibend dem Gegner überlassen.
    Lost,
}

/// Zustand eines Schiffs nach dem Gefecht (oder als Momentaufnahme mittendrin).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShipResult {
    pub source: ShipId,
    pub side: u16,
    pub fate: Fate,
    /// Anteile 0..1 am Höchstwert des Entwurfs.
    pub hull: f32,
    pub modules: Vec<f32>,
    pub ammo: f32,
    pub fuel: f32,
    /// Bordjäger, die wieder im Hangar stehen.
    pub fighters: u32,
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
        let anchor = math::exact_direction(home_angle) * balance.spawn_radius;

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
            orbit: 1.0,
            engaged: false,
            contact: false,
            retreating: false,
            outlook: 0.0,
            last_hit: 0.0,
            retreat_time: None,
            stalemate: false,
            damage_dealt: [0.0; 4],
            damage_taken: 0.0,
            enemy_center: V2::ZERO,
            fleet_speed: 30.0,
            fleet_range: 500.0,
            range_center: anchor,
            range_offset: V2::ZERO,
            carriers_in_line: false,
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
        let members: Vec<usize> = (first..self.ships.len()).collect();
        // Träger ohne Jäger stehen gleich in der Linie.
        self.combatants[side].carriers_in_line = !self.has_fighters(data, side);
        formation::assign_slots(
            &mut self.ships,
            &members,
            self.combatants[side].formation,
            balance.formation_spacing,
            self.combatants[side].carriers_in_line,
        );
        self.refresh_fleet_profile(data, side);
        let (forward, left) = (
            math::exact_direction(facing),
            math::exact_direction(facing).perp(),
        );
        for index in members {
            let ship = &mut self.ships[index];
            // Genau gespiegelt: Die zweite Partei steht punktsymmetrisch zur ersten.
            ship.pos = anchor + forward * ship.slot.x + left * ship.slot.y;
            ship.heading = facing;
        }
        side
    }

    /// Hat die Partei noch Jäger, die kämpfen, oder Träger, die welche starten können?
    pub fn has_fighters(&self, data: &GameData, side: usize) -> bool {
        self.ships.iter().any(|ship| {
            ship.side as usize == side
                && ship.status == ShipStatus::Active
                && (ship.class == ShipClass::Fighter || can_launch(data, ship))
        })
    }

    /// Schiffe einer Partei, die einen Platz in der Formation haben.
    fn formation_members(&self, side: usize) -> Vec<usize> {
        (0..self.ships.len())
            .filter(|index| {
                let ship = &self.ships[*index];
                ship.side as usize == side
                    && ship.status.can_act()
                    && ship.class != ShipClass::Fighter
            })
            .collect()
    }

    /// Geschwindigkeit, Gefechtsentfernung und Breitseitenneigung einer Partei aus den
    /// Schiffen, die sie noch hat.
    pub(crate) fn refresh_fleet_profile(&mut self, data: &GameData, side: usize) {
        let fighters = self.has_fighters(data, side);
        // Ohne Jäger rücken die Träger in die Linie, sonst kämen ihre Geschütze nie zum
        // Zug: Sie stünden weit hinter dem Punkt, von dem aus der Abstand gehalten wird.
        if !fighters && !self.combatants[side].carriers_in_line {
            self.combatants[side].carriers_in_line = true;
            let members = self.formation_members(side);
            formation::assign_slots(
                &mut self.ships,
                &members,
                self.combatants[side].formation,
                data.balance.battle.formation_spacing,
                true,
            );
        }
        /// Schiffe, die gemeinsam den Abstand bestimmen: Reichweite, Bauwert, dazu Platz und
        /// Lage, mit dem Bauwert gewichtet.
        #[derive(Default)]
        struct Group {
            range: f32,
            weight: f32,
            slot: V2,
            pos: V2,
        }
        impl Group {
            fn add(&mut self, ship: &BattleShip, range: f32) {
                self.range = range;
                self.weight += ship.value;
                self.slot += ship.slot * ship.value;
                self.pos += ship.pos * ship.value;
            }
            fn mean(&self, sum: V2) -> V2 {
                sum * (1.0 / self.weight.max(1e-6))
            }
        }
        // Die Linie hält den Abstand, auf den auch ihre kürzesten Hauptwaffen wirken; Geleit
        // allein das Mittel seiner Reichweiten; Träger allein die größte.
        let (mut line, mut escorts, mut carriers) =
            (Group::default(), Group::default(), Group::default());
        line.range = f32::MAX;
        let mut escort_range_sum = 0.0;
        let (mut slowest, mut broad, mut broad_weight) = (f32::MAX, 0.0, 0.0);
        let range_factor = self.combatants[side].modifiers.range;
        for ship in self
            .ships
            .iter()
            .filter(|ship| ship.side as usize == side && ship.status.can_act())
        {
            if ship.class == ShipClass::Fighter {
                continue;
            }
            slowest = slowest.min(ship.max_speed);
            match ship.class {
                ShipClass::Carrier => {
                    // Träger halten nur Abstand, solange es Jäger gibt; danach zählen ihre
                    // Geschütze.
                    let range = if fighters {
                        ship.main_range
                    } else {
                        data.designs[ship.design as usize].stats.gun_range * range_factor
                    };
                    carriers.add(ship, carriers.range.max(range));
                }
                ShipClass::Cruiser | ShipClass::Battleship => {
                    line.add(ship, line.range.min(ship.main_range));
                    broad += ship.broadside * ship.value;
                    broad_weight += ship.value;
                }
                _ => {
                    escort_range_sum += ship.main_range * ship.value;
                    escorts.add(ship, 0.0);
                }
            }
        }
        escorts.range = escort_range_sum / escorts.weight.max(1e-6);
        let anchor = self.combatants[side].anchor;
        let (range, offset, center) = [line, escorts, carriers]
            .into_iter()
            .find(|group| group.weight > 0.0)
            .map_or((500.0, V2::ZERO, anchor), |group| {
                (group.range, group.mean(group.slot), group.mean(group.pos))
            });
        let combatant = &mut self.combatants[side];
        combatant.range_center = center;
        combatant.fleet_speed = if slowest == f32::MAX {
            60.0
        } else {
            slowest * 0.9
        };
        combatant.fleet_range = range;
        combatant.range_offset = offset;
        combatant.broadside =
            broad_weight > 0.0 && broad / broad_weight > data.balance.battle.broadside_min;
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
            let carriers_in_line = combatant.carriers_in_line;
            let members = self.formation_members(side);
            formation::assign_slots(
                &mut self.ships,
                &members,
                formation,
                data.balance.battle.formation_spacing,
                carriers_in_line,
            );
            self.refresh_fleet_profile(data, side);
        }
    }

    /// Befiehlt den Rückzug: Alle Schiffe der Partei verlassen das Feld.
    pub fn order_retreat(&mut self, side: usize) {
        let time = self.time;
        let Some(combatant) = self.combatants.get_mut(side) else {
            return;
        };
        if !combatant.retreating {
            combatant.retreat_time = Some(time);
        }
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
                // Nur Jäger, die noch starten können, zählen.
                let craft = data.designs[ship.design as usize]
                    .stats
                    .craft
                    .filter(|_| can_launch(data, ship));
                let hangar = craft.map_or(0.0, |craft| {
                    ship.hangar as f32 * data.designs[craft as usize].stats.value as f32
                });
                let floor = data.balance.battle.value_hull_floor;
                ship.value * (floor + (1.0 - floor) * ship.hull / ship.hull_max.max(1e-6)) + hangar
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

    /// Stellt eine Partei an einer anderen Stelle auf: Bezugspunkt, Blickrichtung und die
    /// Schiffe auf ihren Plätzen. Für Übungslagen, etwa einen Verband, der im Rücken des
    /// Gegners eintrifft. Die Rückzugsrichtung zeigt danach von `anchor` weg zur Feldmitte
    /// entgegen.
    pub fn deploy(&mut self, side: usize, anchor: V2, facing: f32) {
        let Some(combatant) = self.combatants.get_mut(side) else {
            return;
        };
        combatant.anchor = anchor;
        combatant.anchor_vel = V2::ZERO;
        combatant.facing = facing;
        combatant.home_angle = math::wrap_angle(facing + std::f32::consts::PI);
        for ship in self
            .ships
            .iter_mut()
            .filter(|ship| ship.side as usize == side && ship.status.in_play())
        {
            ship.pos = anchor + ship.slot.rotated(facing);
            ship.heading = facing;
            ship.vel = V2::ZERO;
            ship.speed = 0.0;
        }
    }

    /// Ist ein treibendes Schiff dieser Partei dem Gegner überlassen? Ja, wenn ein Gegner
    /// noch kämpft und die eigene Seite nicht mehr. Dieselbe Regel gilt am Ende des Gefechts
    /// (dort wird es zerstört) und in [`Battle::results`].
    pub(crate) fn adrift_lost(&self, fighting: &[bool], side: usize) -> bool {
        !fighting[side]
            && (0..self.combatants.len())
                .any(|other| self.is_hostile(side as u16, other as u16) && fighting[other])
    }

    /// Freie Plätze für heimkehrende Bordjäger je Partei: in den Hangars der Träger, die
    /// nicht verloren sind.
    fn hangar_room(&self, data: &GameData, fighting: &[bool]) -> Vec<u32> {
        let mut room = vec![0u32; self.combatants.len()];
        for ship in &self.ships {
            if ship.class == ShipClass::Fighter || self.fate(ship, fighting) == Fate::Lost {
                continue;
            }
            let capacity = data.designs[ship.design as usize].stats.fighters;
            room[ship.side as usize] += capacity - ship.hangar.min(capacity);
        }
        room
    }

    fn fate(&self, ship: &BattleShip, fighting: &[bool]) -> Fate {
        match ship.status {
            ShipStatus::Destroyed => Fate::Lost,
            ShipStatus::Escaped => Fate::Escaped,
            ShipStatus::Active | ShipStatus::Routing => Fate::Holding,
            ShipStatus::Disabled if self.adrift_lost(fighting, ship.side as usize) => Fate::Lost,
            ShipStatus::Disabled => Fate::Holding,
        }
    }

    /// Was aus jedem Schiff der strategischen Ebene geworden ist, in der Reihenfolge der
    /// Schiffe. Überlebende Bordjäger kehren in die Hangars überlebender Träger ihrer Seite
    /// zurück, in Reihenfolge der Träger. Am Ende des Gefechts passen sie hinein: Wer keinen
    /// Platz fand, ist schon als verloren gezählt. Mitten im Gefecht (als Momentaufnahme)
    /// bleiben Jäger ohne Platz unberücksichtigt.
    pub fn results(&self, data: &GameData) -> Vec<ShipResult> {
        let fighting: Vec<bool> = (0..self.combatants.len())
            .map(|side| self.is_fighting(side))
            .collect();
        let fate_of = |ship: &BattleShip| self.fate(ship, &fighting);
        // Überlebende Bordjäger je Seite.
        let mut returning = vec![0u32; self.combatants.len()];
        for ship in &self.ships {
            if ship.source.is_none()
                && ship.class == ShipClass::Fighter
                && fate_of(ship) != Fate::Lost
            {
                returning[ship.side as usize] += 1;
            }
        }
        let fraction = |value: f32, max: f32| {
            if max > 0.0 {
                (value / max).clamp(0.0, 1.0)
            } else {
                1.0
            }
        };
        let mut results = Vec::new();
        for ship in &self.ships {
            let Some(source) = ship.source else {
                continue;
            };
            let fate = fate_of(ship);
            let design = &data.designs[ship.design as usize];
            let lost = fate == Fate::Lost;
            let modules = design
                .modules
                .iter()
                .zip(&ship.modules)
                .map(|(slot, state)| {
                    if lost {
                        0.0
                    } else {
                        fraction(state.hp, data.modules[slot.module_id as usize].hp)
                    }
                })
                .collect();
            let mut fighters = 0;
            if !lost {
                let capacity = design.stats.fighters;
                fighters = ship.hangar.min(capacity);
                let pool = &mut returning[ship.side as usize];
                let room = capacity - fighters;
                let back = room.min(*pool);
                fighters += back;
                *pool -= back;
            }
            results.push(ShipResult {
                source,
                side: ship.side,
                fate,
                hull: if lost {
                    0.0
                } else {
                    fraction(ship.hull, ship.hull_max)
                },
                modules,
                ammo: if lost {
                    0.0
                } else {
                    fraction(ship.ammo, ship.ammo_max)
                },
                fuel: if lost {
                    0.0
                } else {
                    fraction(ship.fuel, ship.fuel_max)
                },
                fighters,
            });
        }
        results
    }
}

impl Battle {
    /// Am Ende des Gefechts: Bordjäger, für die in keinem überlebenden Träger ihrer Seite
    /// Platz ist, gehen verloren und werden so auch gezählt. Die ersten (nach Index) finden
    /// Platz, die übrigen nicht.
    pub(crate) fn settle_fighters(&mut self, data: &GameData) {
        let fighting: Vec<bool> = (0..self.combatants.len())
            .map(|side| self.is_fighting(side))
            .collect();
        let mut room = self.hangar_room(data, &fighting);
        for index in 0..self.ships.len() {
            let ship = &self.ships[index];
            if ship.source.is_some()
                || ship.class != ShipClass::Fighter
                || self.fate(ship, &fighting) == Fate::Lost
            {
                continue;
            }
            let side = ship.side as usize;
            if room[side] > 0 {
                room[side] -= 1;
                continue;
            }
            let (escaped, value) = (ship.status == ShipStatus::Escaped, ship.value);
            let ship = &mut self.ships[index];
            ship.status = ShipStatus::Destroyed;
            ship.target = None;
            let combatant = &mut self.combatants[side];
            if escaped {
                combatant.escaped[ShipClass::Fighter.index()] -= 1;
            }
            combatant.lost[ShipClass::Fighter.index()] += 1;
            combatant.lost_value += value;
        }
    }
}

/// Kann der Träger Jäger starten? Er muss kämpfen, Jäger im Hangar haben und noch ein
/// heiles Hangarmodul.
pub(crate) fn can_launch(data: &GameData, ship: &BattleShip) -> bool {
    ship.hangar > 0 && ship.status == ShipStatus::Active && launch_bays(data, ship) > 0
}

/// Zahl der heilen Hangarmodule.
pub(crate) fn launch_bays(data: &GameData, ship: &BattleShip) -> usize {
    data.designs[ship.design as usize]
        .modules
        .iter()
        .zip(&ship.modules)
        .filter(|(slot, state)| {
            state.hp > 0.0
                && matches!(
                    data.modules[slot.module_id as usize].kind,
                    ModuleKind::Hangar { .. }
                )
        })
        .count()
}

/// Ein Anteil auf 0..1 gebracht; Unsinn (NaN, unendlich) gilt als voll.
fn unit(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        1.0
    }
}

/// Baut ein Schiff für das Gefecht aus Entwurf, Zustand und Technologie seiner Partei.
fn make_ship(data: &GameData, side: u16, spec: &ShipSpec, combatant: &Combatant) -> BattleShip {
    let design: &DesignDef = &data.designs[spec.design as usize];
    let stats = &design.stats;
    let modifiers = &combatant.modifiers;
    // Auch ein unsinniger Aufschlag darf keinen Rumpf von null ergeben (Teilung).
    let hull_max = (stats.hull * modifiers.hull).max(1.0);
    let condition = unit(spec.hull);
    let armor_max = stats.armor.map(|points| points * modifiers.armor);
    let modules: Vec<ModuleState> = design
        .modules
        .iter()
        .enumerate()
        .map(|(index, slot)| {
            let fraction = unit(spec.modules.get(index).copied().unwrap_or(1.0));
            ModuleState {
                hp: data.modules[slot.module_id as usize].hp * fraction,
                cooldown: 0.0,
            }
        })
        .collect();
    let morale_base = spec.morale.unwrap_or(combatant.morale).clamp(0.0, 100.0);
    let morale_base = if morale_base.is_nan() {
        combatant.morale
    } else {
        morale_base
    };
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
        ammo: stats.ammo * unit(spec.ammo),
        ammo_max: stats.ammo,
        fuel: stats.fuel * unit(spec.fuel),
        fuel_max: stats.fuel,
        morale: morale_base,
        morale_base,
        shock: 0.0,
        rout_time: 0.0,
        rearm: 0.0,
        armed: true,
        target: None,
        slot: V2::ZERO,
        carrier: None,
        hangar: spec.fighters.min(stats.fighters),
        launch_timer: data.balance.battle.first_launch,
        breakaway: 0.0,
        max_speed: stats.speed * modifiers.speed,
        accel: stats.accel * modifiers.speed,
        turn: stats.turn,
        radius: stats.radius,
        signature: stats.signature,
        main_range: stats.main_range * modifiers.range,
        broadside: stats.broadside,
        value: (stats.value
            - stats.craft.map_or(0.0, |craft| {
                stats.fighters as f64 * data.designs[craft as usize].stats.value
            })) as f32,
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
    // Ob die Punktverteidigung zählt, hängt vom Gegner ab; das entscheidet jeder Takt neu
    // (siehe `upkeep`). Bis dahin zählt sie.
    refresh_armed(data, ship, modifiers.range, true);
}

/// Kann das Schiff noch wirken? Eine Waffe muss arbeiten und, wenn sie Munition braucht,
/// welche haben; ein Träger wirkt, solange er Jäger starten kann. Die Punktverteidigung
/// zählt nur, wenn `point_defense_counts` (der Gegner hat kleine Schiffe oder Lenkwaffen):
/// Gegen große Schiffe richtet sie nichts aus. Dazu die Reichweite der stärksten
/// Waffengruppe, die noch feuern kann: Ist die Munition fort, rückt ein Schiff auf die
/// Reichweite seiner Strahlwaffen heran; bleibt nur die Punktverteidigung, auf deren.
pub(crate) fn refresh_armed(
    data: &GameData,
    ship: &mut BattleShip,
    range_factor: f32,
    point_defense_counts: bool,
) {
    let design = &data.designs[ship.design as usize];
    let ready = |slot: &DesignModule, state: &ModuleState| {
        state.hp > 0.0
            && data.modules[slot.module_id as usize]
                .weapon()
                .is_some_and(|weapon| weapon.ammo <= 0.0 || ship.ammo >= weapon.ammo)
    };
    let mut armed = ship.hangar > 0 && launch_bays(data, ship) > 0;
    let mut best: Option<(f32, f32)> = None;
    let mut point_defense: f32 = 0.0;
    for (index, (slot, state)) in design.modules.iter().zip(&ship.modules).enumerate() {
        if !ready(slot, state) {
            continue;
        }
        let Some(weapon) = data.modules[slot.module_id as usize].weapon() else {
            continue;
        };
        if weapon.kind == WeaponKind::PointDefense {
            armed |= point_defense_counts;
            point_defense = point_defense.max(weapon.range);
            continue;
        }
        armed = true;
        // Gleiche Waffen zählen zusammen; die erste ihrer Art steht für die Gruppe.
        let first = design
            .modules
            .iter()
            .position(|other| other.module_id == slot.module_id)
            .unwrap_or(index);
        if first != index
            && design.modules[first..index]
                .iter()
                .zip(&ship.modules[first..index])
                .any(|(other, other_state)| {
                    other.module_id == slot.module_id && ready(other, other_state)
                })
        {
            continue;
        }
        let dps: f32 = design
            .modules
            .iter()
            .zip(&ship.modules)
            .filter(|(other, other_state)| {
                other.module_id == slot.module_id && ready(other, other_state)
            })
            .map(|_| weapon.dps())
            .sum();
        if best.is_none_or(|(best_dps, _)| dps > best_dps) {
            best = Some((dps, weapon.range));
        }
    }
    ship.armed = armed;
    // Träger halten den Abstand ihrer Jäger; der steht in ihren Kennwerten.
    if design.stats.fighters == 0 {
        if let Some((_, range)) = best {
            ship.main_range = range * range_factor;
        } else if point_defense > 0.0 && point_defense_counts {
            ship.main_range = point_defense * range_factor;
        }
    }
}

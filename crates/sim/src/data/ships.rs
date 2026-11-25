//! Formate für Rümpfe, Module, Schiffsentwürfe und Technologien, dazu die aus einem
//! Entwurf abgeleiteten Kennwerte.

use crate::ids::GoodId;
use serde::{Deserialize, Serialize};

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub enum ShipClass {
    Fighter,
    Corvette,
    Frigate,
    #[default]
    Cruiser,
    Battleship,
    Carrier,
}

impl ShipClass {
    pub const ALL: [ShipClass; 6] = [
        ShipClass::Fighter,
        ShipClass::Corvette,
        ShipClass::Frigate,
        ShipClass::Cruiser,
        ShipClass::Battleship,
        ShipClass::Carrier,
    ];

    pub const fn index(self) -> usize {
        self as usize
    }

    pub fn label(self) -> &'static str {
        match self {
            ShipClass::Fighter => "Jäger",
            ShipClass::Corvette => "Korvette",
            ShipClass::Frigate => "Fregatte",
            ShipClass::Cruiser => "Kreuzer",
            ShipClass::Battleship => "Schlachtschiff",
            ShipClass::Carrier => "Träger",
        }
    }

    pub fn plural(self) -> &'static str {
        match self {
            ShipClass::Fighter => "Jäger",
            ShipClass::Corvette => "Korvetten",
            ShipClass::Frigate => "Fregatten",
            ShipClass::Cruiser => "Kreuzer",
            ShipClass::Battleship => "Schlachtschiffe",
            ShipClass::Carrier => "Träger",
        }
    }

    /// Großkampfschiffe: Ihr Verlust wiegt für die Moral doppelt.
    pub fn is_capital(self) -> bool {
        matches!(
            self,
            ShipClass::Cruiser | ShipClass::Battleship | ShipClass::Carrier
        )
    }
}

/// Zielvorlieben einer Rumpfklasse gegenüber jeder Klasse.
#[derive(Clone, Copy, Debug, Default, Deserialize)]
pub struct TargetWeights {
    pub fighter: f32,
    pub corvette: f32,
    pub frigate: f32,
    pub cruiser: f32,
    pub battleship: f32,
    pub carrier: f32,
}

impl TargetWeights {
    pub fn of(&self, class: ShipClass) -> f32 {
        match class {
            ShipClass::Fighter => self.fighter,
            ShipClass::Corvette => self.corvette,
            ShipClass::Frigate => self.frigate,
            ShipClass::Cruiser => self.cruiser,
            ShipClass::Battleship => self.battleship,
            ShipClass::Carrier => self.carrier,
        }
    }
}

/// `hulls.ron`
#[derive(Clone, Debug, Deserialize)]
pub struct HullDef {
    pub key: String,
    pub name: String,
    pub class: ShipClass,
    pub hull: f32,
    pub radius: f32,
    pub signature: f32,
    pub speed: f32,
    pub accel: f32,
    pub turn: f32,
    /// Anteil der Panzerung an Bug, jeder Flanke, Heck.
    pub armor_split: (f32, f32, f32),
    /// Rumpfsegmente für den Bau.
    pub segments: f32,
    pub build_days: u32,
    pub slots: u32,
    pub ammo: f32,
    pub fuel: f32,
    pub targets: TargetWeights,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WeaponKind {
    /// Trifft sofort; Trefferchance aus Nachführung und Zielgröße.
    Beam,
    /// Geschoss mit Flugzeit, braucht Munition.
    Kinetic,
    /// Lenkwaffe, abfangbar.
    Missile,
    /// Fängt Lenkwaffen und Jäger ab.
    PointDefense,
}

#[derive(Clone, Debug, Deserialize)]
pub struct WeaponDef {
    pub kind: WeaponKind,
    pub damage: f32,
    pub range: f32,
    /// Sekunden zwischen zwei Schüssen.
    pub cooldown: f32,
    /// Nachführung in rad/s.
    pub tracking: f32,
    /// Geschwindigkeit des Geschosses oder der Lenkwaffe.
    #[serde(default)]
    pub speed: f32,
    /// Streuung in rad.
    #[serde(default)]
    pub spread: f32,
    /// Munition je Schuss.
    #[serde(default)]
    pub ammo: f32,
    /// Drehrate einer Lenkwaffe in rad/s.
    #[serde(default)]
    pub turn: f32,
    /// Strukturpunkte einer Lenkwaffe gegen Punktverteidigung.
    #[serde(default)]
    pub shot_hp: f32,
    #[serde(default = "one")]
    pub vs_shield: f32,
    #[serde(default = "one")]
    pub vs_armor: f32,
    #[serde(default = "one")]
    pub vs_hull: f32,
}

impl WeaponDef {
    /// Schaden je Sekunde ohne Ausfälle und Fehlschüsse.
    pub fn dps(&self) -> f32 {
        self.damage / self.cooldown.max(0.05)
    }
}

#[derive(Clone, Debug, Deserialize)]
pub enum ModuleKind {
    Engine {
        thrust: f32,
    },
    Reactor {
        output: f32,
    },
    Tank {
        fuel: f32,
    },
    Magazine {
        ammo: f32,
    },
    Shield {
        capacity: f32,
        regen: f32,
    },
    Armor {
        points: f32,
    },
    Weapon(WeaponDef),
    Hangar {
        /// Entwurf der Bordjäger.
        craft: String,
        count: u32,
        #[serde(skip)]
        craft_id: u16,
    },
}

/// `modules.ron`
#[derive(Clone, Debug, Deserialize)]
pub struct ModuleDef {
    pub key: String,
    pub name: String,
    pub kind: ModuleKind,
    /// Strukturpunkte des Moduls selbst.
    pub hp: f32,
    /// Strombedarf.
    pub power: f32,
    #[serde(default)]
    pub cost: Vec<(String, f32)>,
    #[serde(default)]
    pub tech: Option<String>,
    #[serde(skip)]
    pub cost_ids: Vec<(GoodId, f32)>,
    #[serde(skip)]
    pub tech_id: Option<u16>,
}

impl ModuleDef {
    pub fn weapon(&self) -> Option<&WeaponDef> {
        match &self.kind {
            ModuleKind::Weapon(weapon) => Some(weapon),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct DesignModule {
    pub module: String,
    /// Mittelachse des Feuerwinkels in Grad vom Bug.
    #[serde(default)]
    pub facing: f32,
    /// Breite des Feuerwinkels in Grad.
    #[serde(default = "full_circle")]
    pub arc: f32,
    #[serde(skip)]
    pub module_id: u16,
}

/// Seite eines Schiffs, von der ein Treffer kommt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Facing {
    Front = 0,
    Right = 1,
    Rear = 2,
    Left = 3,
}

/// Aus Rumpf und Modulen abgeleitete Kennwerte eines Entwurfs.
#[derive(Clone, Debug, Default)]
pub struct DesignStats {
    pub class: ShipClass,
    pub hull: f32,
    pub shield: f32,
    pub shield_regen: f32,
    /// Panzerung je Seite: Bug, Steuerbord, Heck, Backbord.
    pub armor: [f32; 4],
    pub speed: f32,
    pub accel: f32,
    pub turn: f32,
    pub radius: f32,
    pub signature: f32,
    pub power_output: f32,
    pub power_draw: f32,
    pub ammo: f32,
    pub fuel: f32,
    /// Bauteile vom Markt, einschließlich der Bordjäger.
    pub parts: Vec<(GoodId, f32)>,
    /// Bauwert in Credits zu Richtpreisen; Maß für Kosten und Kampfwert.
    pub value: f64,
    pub build_days: u32,
    /// Reichweite der Waffengruppe, die den meisten Schaden bringt.
    pub main_range: f32,
    /// Anteil der Feuerkraft, der zur Seite wirken kann (0..1): Maß dafür, wie sehr
    /// sich eine Breitseite lohnt.
    pub broadside: f32,
    /// Zahl und Entwurf der Bordjäger.
    pub fighters: u32,
    pub craft: Option<u16>,
    /// Alle Technologien, die der Entwurf voraussetzt.
    pub techs: Vec<u16>,
}

/// `designs.ron`
#[derive(Clone, Debug, Deserialize)]
pub struct DesignDef {
    pub key: String,
    pub name: String,
    pub hull: String,
    #[serde(default)]
    pub tech: Option<String>,
    pub modules: Vec<DesignModule>,
    #[serde(skip)]
    pub hull_id: u16,
    #[serde(skip)]
    pub stats: DesignStats,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum TechBranch {
    Economy,
    Propulsion,
    Weapons,
    Defense,
    Ground,
}

impl TechBranch {
    pub const ALL: [TechBranch; 5] = [
        TechBranch::Economy,
        TechBranch::Propulsion,
        TechBranch::Weapons,
        TechBranch::Defense,
        TechBranch::Ground,
    ];

    pub const fn index(self) -> usize {
        self as usize
    }

    pub fn label(self) -> &'static str {
        match self {
            TechBranch::Economy => "Wirtschaft",
            TechBranch::Propulsion => "Antrieb",
            TechBranch::Weapons => "Waffen",
            TechBranch::Defense => "Verteidigung",
            TechBranch::Ground => "Bodenkrieg",
        }
    }
}

/// Aufschlag durch eine Technologie (0.08 = +8 %).
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
pub enum TechEffect {
    Yield(f64),
    FreighterSpeed(f64),
    FleetSpeed(f64),
    ShipSpeed(f64),
    WeaponDamage(f64),
    WeaponRange(f64),
    Shields(f64),
    Armor(f64),
    Hull(f64),
    GroundAttack(f64),
    GroundDefense(f64),
    Research(f64),
}

impl TechEffect {
    /// Höhe des Aufschlags, gleich welcher Art.
    pub fn amount(self) -> f64 {
        match self {
            TechEffect::Yield(amount)
            | TechEffect::FreighterSpeed(amount)
            | TechEffect::FleetSpeed(amount)
            | TechEffect::ShipSpeed(amount)
            | TechEffect::WeaponDamage(amount)
            | TechEffect::WeaponRange(amount)
            | TechEffect::Shields(amount)
            | TechEffect::Armor(amount)
            | TechEffect::Hull(amount)
            | TechEffect::GroundAttack(amount)
            | TechEffect::GroundDefense(amount)
            | TechEffect::Research(amount) => amount,
        }
    }

    /// Beschriftung für Listen, z. B. „Ausstoß der Werke".
    pub fn label(self) -> &'static str {
        match self {
            TechEffect::Yield(_) => "Ausstoß der Werke",
            TechEffect::FreighterSpeed(_) => "Fahrt der Frachter",
            TechEffect::FleetSpeed(_) => "Fahrt der Flotten",
            TechEffect::ShipSpeed(_) => "Fahrt im Gefecht",
            TechEffect::WeaponDamage(_) => "Waffenschaden",
            TechEffect::WeaponRange(_) => "Waffenreichweite",
            TechEffect::Shields(_) => "Schilde",
            TechEffect::Armor(_) => "Panzerung",
            TechEffect::Hull(_) => "Rumpf",
            TechEffect::GroundAttack(_) => "Angriff am Boden",
            TechEffect::GroundDefense(_) => "Verteidigung am Boden",
            TechEffect::Research(_) => "Forschung",
        }
    }
}

/// `techs.ron`
#[derive(Clone, Debug, Deserialize)]
pub struct TechDef {
    pub key: String,
    pub name: String,
    pub branch: TechBranch,
    pub tier: u32,
    /// Forschungspunkte.
    pub cost: f64,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default)]
    pub effects: Vec<TechEffect>,
    #[serde(skip)]
    pub requires_ids: Vec<u16>,
}

/// `balance.ron: military`
#[derive(Clone, Debug, Deserialize)]
pub struct MilitaryBalance {
    pub starting_fleet: Vec<(String, u32)>,
    pub upkeep_per_month: f64,
    pub yard_capital_per_slot: f64,
    pub repair_per_day: f32,
    pub repair_parts: f64,
    pub fuel_per_distance: f32,
    pub dry_speed: f32,
    pub ammo_goods: f64,
    pub fuel_goods: f64,
    pub hull_good: String,
    pub ammo_good: String,
    #[serde(skip)]
    pub starting_fleet_ids: Vec<(u16, u32)>,
    #[serde(skip)]
    pub hull_good_id: GoodId,
    #[serde(skip)]
    pub ammo_good_id: GoodId,
}

/// Drei Werte je Doktrin: offensiv, ausgewogen, defensiv.
pub type PerDoctrine = (f32, f32, f32);

/// `balance.ron: battle`
#[derive(Clone, Debug, Deserialize)]
pub struct BattleBalance {
    pub dt: f32,
    pub spawn_radius: f32,
    pub exit_radius: f32,
    pub time_limit: f32,
    pub grid_cell: f32,
    pub sensor_range: f32,
    pub retarget_every: f32,
    pub command_every: f32,
    pub beam_signature_weight: f32,
    pub module_damage: f32,
    pub reactor_breach: f64,
    pub morale_start: f32,
    pub morale_per_weariness: f32,
    pub morale_hull_loss: f32,
    pub morale_friend_lost: f32,
    pub morale_enemy_lost: f32,
    pub morale_radius: f32,
    pub morale_rear_factor: f32,
    pub morale_break: f32,
    pub retreat_ratio: PerDoctrine,
    pub range_keep: PerDoctrine,
    pub fuel_per_second: f32,
    pub formation_spacing: f32,
}

/// Ein Wert je Forschungszweig.
#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
pub struct BranchWeights {
    pub economy: f64,
    pub propulsion: f64,
    pub weapons: f64,
    pub defense: f64,
    pub ground: f64,
}

impl BranchWeights {
    pub fn of(&self, branch: TechBranch) -> f64 {
        match branch {
            TechBranch::Economy => self.economy,
            TechBranch::Propulsion => self.propulsion,
            TechBranch::Weapons => self.weapons,
            TechBranch::Defense => self.defense,
            TechBranch::Ground => self.ground,
        }
    }
}

/// `balance.ron: research`
#[derive(Clone, Debug, Deserialize)]
pub struct ResearchBalance {
    /// Anteil der Staatseinnahmen, mit dem jede Gemeinschaft beginnt.
    pub budget_share: f64,
    /// Höchster Anteil, den eine Gemeinschaft einstellen kann.
    pub max_budget_share: f64,
    /// Forschungspunkte je ausgegebenem Credit bei einer Milliarde Einwohnern.
    pub efficiency: f64,
    /// Wie stark die Bevölkerung zählt (0 = gar nicht, 1 = proportional).
    pub population_exponent: f64,
    /// Größter Nachlass auf eine Technologie, die Gemeinschaften in Kontakt schon beherrschen.
    pub spillover_max: f64,
    /// Sprünge im Routennetz, nach denen das Gewicht eines Nachbarn halbiert ist.
    pub spillover_half_hops: f64,
    /// Vorlieben der automatischen Zielwahl im Frieden und im Krieg.
    pub auto_peace: BranchWeights,
    pub auto_war: BranchWeights,
    /// Einblick in fremde Forschung (0..1) ohne jede Verflechtung.
    pub intel_base: f32,
    /// Einblick, den Besitz an den Betrieben der anderen höchstens verschafft.
    pub intel_ties_max: f32,
    /// Anteil an den Betrieben der anderen, ab dem dieser Einblick erreicht ist.
    pub intel_ties_share: f64,
    /// Anteil, um den Einblick aus Ereignissen je Monat auf das Niveau der Verflechtung
    /// zurückfällt.
    pub intel_decay: f32,
    /// Größte Unschärfe einer Schätzung in Stufen nach oben und nach unten.
    pub estimate_spread: u8,
}

fn one() -> f32 {
    1.0
}

fn full_circle() -> f32 {
    360.0
}

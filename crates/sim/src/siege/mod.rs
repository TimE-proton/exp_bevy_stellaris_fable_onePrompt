//! Belagerung: der Kampf um einen Planeten zwischen einer Flotte im Orbit und der
//! Verteidigung am Boden.
//!
//! Je Planet steht dauerhaft eine Handvoll Verteidigungswerte ([`PlanetDefense`]); eine
//! Belagerung legt darüber genau einen Datensatz ([`Siege`]). Die Phasen sind Bedingungen
//! auf diesem Zustand und werden nie gespeichert ([`Siege::phase`]). Drei Wege führen zum
//! Ziel: aushungern (langsam, die Beute bleibt heil), stürmen (Wochen, kostet Truppen),
//! beschießen (schnell, kostet Beute).
//!
//! Der Tagesrechner ([`step::resolve`]) ist eine reine Funktion ohne Zufall über einfachen
//! Daten. Prognose, KI, Oberfläche und Tests rufen dieselbe Funktion; an Flotten, Märkte
//! und Forschung reicht sie nicht heran. Was ein Tag außerhalb des Datensatzes bewirkt,
//! steht in [`step::DayEffects`].
//!
//! In der Partie führt [`day`] den Lebenslauf der Datensätze und wendet an, was der
//! Tagesrechner bewirkt; [`conquest`] macht aus einem Urteil den Besitzwechsel, [`orders`]
//! prüft die Befehle. Allein [`orbit`] liest Flotten, Schiffe und Forschung und übersetzt
//! sie in die einfachen Daten, mit denen alles Übrige rechnet. Datensätze enden nur über
//! [`close`].

pub mod conquest;
pub mod day;
pub mod orbit;
pub mod orders;
mod plugin;
pub mod step;

pub use conquest::{HandOverTerms, hand_over, mirror_region, resolve_conquests};
pub use day::{firm_value, lift, planet_facts, prize, run_day, tidy};
pub use orbit::{Orbit, OrbitAccess, Squadron, Standing, Toll};
pub use orders::{
    begin_siege, embark_troops, lift_siege, peace_veto, set_garrison_orders, set_siege_orders,
};
pub use plugin::SiegePlugin;
pub use step::{
    DayEffects, Landing, Projection, build_zones, forecast, form, hull_shares, land,
    militia_strength, resolve, trace,
};

use crate::data::{GameData, SiegeBalance};
use crate::diplomacy::Diplomacy;
use crate::economy::{Economy, Money};
use crate::finance::Finance;
use crate::galaxy::Galaxy;
use crate::ids::{FleetId, PlanetId, PolityId};
use crate::polity::Polities;
use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Feuerstufe der Flotte im Orbit, aufsteigend.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Fire {
    /// Die Flotte bleibt im hohen Orbit und schießt nicht auf den Planeten.
    #[default]
    Hold,
    /// Stützfeuer für die Truppen am Boden.
    Support,
    /// Gezielter Beschuss: viel militärische Wirkung, wenig Begleitschaden.
    Precise,
    /// Flächenbeschuss: schnell, auf Kosten von Bevölkerung und Kapital.
    Area,
}

impl Fire {
    pub const ALL: [Fire; 4] = [Fire::Hold, Fire::Support, Fire::Precise, Fire::Area];

    pub const fn index(self) -> usize {
        self as usize
    }

    pub fn label(self) -> &'static str {
        match self {
            Fire::Hold => "kein Feuer",
            Fire::Support => "stützen",
            Fire::Precise => "gezielt",
            Fire::Area => "flächig",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ZoneKind {
    Outskirts,
    City,
    Center,
    Battery,
    ShieldWorks,
}

impl ZoneKind {
    /// Index in `SiegeBalance::fort`.
    pub const fn index(self) -> usize {
        self as usize
    }

    pub fn label(self) -> &'static str {
        match self {
            ZoneKind::Outskirts => "Umland",
            ZoneKind::City => "Stadt",
            ZoneKind::Center => "Zentrum",
            ZoneKind::Battery => "Batterie",
            ZoneKind::ShieldWorks => "Schildwerk",
        }
    }
}

/// Haltung der Garnison: Wo kämpfen die Regulären?
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Posture {
    /// Die Reserve stellt sich der Landung in jeder Zone.
    #[default]
    Forward,
    /// Die Regulären sitzen im Untergrund; was dort keinen Platz hat, hält das Zentrum.
    Deep,
}

impl Posture {
    pub fn label(self) -> &'static str {
        match self {
            Posture::Forward => "vorwärts",
            Posture::Deep => "Tiefe",
        }
    }
}

/// Ausgang einer Belagerung.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Outcome {
    /// Oberfläche und Untergrund genommen.
    Stormed,
    /// Vorräte leer, Garnison aufgelöst, Besatzungstruppe zur Stelle.
    Starved,
    /// Übergabe auf Befehl des Verteidigers.
    Capitulated,
    /// Aufgehoben: Entsatz, Frieden, Abzug oder Besitzwechsel.
    Lifted,
}

impl Outcome {
    pub const ALL: [Outcome; 4] = [
        Outcome::Stormed,
        Outcome::Starved,
        Outcome::Capitulated,
        Outcome::Lifted,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Outcome::Stormed => "gestürmt",
            Outcome::Starved => "ausgehungert",
            Outcome::Capitulated => "übergeben",
            Outcome::Lifted => "aufgehoben",
        }
    }
}

/// Abgeleitete Phase einer Belagerung. Nie gespeichert.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Phase {
    /// Stationen halten den hohen Orbit.
    Approach,
    Investment,
    Bombardment,
    Ground,
    Underground,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Phase::Approach => "Anmarsch",
            Phase::Investment => "Einschließung",
            Phase::Bombardment => "Beschuss",
            Phase::Ground => "Bodenkrieg",
            Phase::Underground => "Untergrund",
        }
    }
}

/// Warum ein stehender Befehl heute nicht wirken konnte.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Blocked {
    NoControl,
    Stations,
    NoFirepower,
    NoAmmo,
    ShieldHolds,
    Underground,
    NoArmy,
    /// Die Belagerung ist entschieden und ruht, bis der Besitzwechsel vollzogen ist.
    Handover,
}

impl Blocked {
    pub const ALL: [Blocked; 8] = [
        Blocked::NoControl,
        Blocked::Stations,
        Blocked::NoFirepower,
        Blocked::NoAmmo,
        Blocked::ShieldHolds,
        Blocked::Underground,
        Blocked::NoArmy,
        Blocked::Handover,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Blocked::NoControl => "keine Orbitalhoheit",
            Blocked::Stations => "Stationen halten den Orbit",
            Blocked::NoFirepower => "keine Feuerkraft im Orbit",
            Blocked::NoAmmo => "Magazine leer",
            Blocked::ShieldHolds => "Schild hält",
            Blocked::Underground => "Feuer erreicht den Untergrund nicht",
            Blocked::NoArmy => "keine Truppen am Boden",
            Blocked::Handover => "Übergabe steht aus",
        }
    }
}

/// Menge von Gründen aus [`Blocked`], als Bitfeld: Der Tagesrechner legt nichts auf den Heap.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BlockedSet(pub u16);

impl BlockedSet {
    pub fn insert(&mut self, reason: Blocked) {
        self.0 |= 1 << reason as u16;
    }

    pub fn contains(self, reason: Blocked) -> bool {
        self.0 & (1 << reason as u16) != 0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn iter(self) -> impl Iterator<Item = Blocked> {
        Blocked::ALL
            .into_iter()
            .filter(move |reason| self.contains(*reason))
    }
}

/// Ausbauziel einer Welt; die Stufen wachsen monatlich darauf zu, soweit Güter und Kasse
/// reichen.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DefensePlan {
    pub stations: u8,
    pub guns: u8,
    pub shield: u8,
    pub bunkers: u8,
    /// Reguläre Verbände.
    pub regulars: f32,
    pub depot_days: u16,
}

/// Stehende Verteidigung eines Planeten, auch im Frieden.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PlanetDefense {
    pub plan: DefensePlan,
    /// Intakte Stufen der Orbitalstationen und der Bodengeschütze.
    pub stations: f32,
    pub guns: f32,
    pub shield: u8,
    /// Ladung des Schilds.
    pub charge: f32,
    pub bunkers: u8,
    /// Reguläre Verbände: besoldet, beweglich, aus der Bevölkerung ausgehoben.
    pub regulars: f64,
    /// Miliz in Verbänden: kostenlos und an ihre Zone gebunden. Während einer Belagerung
    /// die Summe über die Zonen.
    pub militia: f64,
    /// Vorräte in Tagen für die ganze Garnison.
    pub depot: f64,
    pub posture: Posture,
    /// Geschütze schweigen, um Vorräte zu sparen.
    pub hold_fire: bool,
    /// Kriegsschäden 0..1 und wer sie verursacht hat.
    pub scar: f64,
    pub scar_by: Option<PolityId>,
    /// Besatzung 0..1 und wem die Welt genommen wurde.
    pub occupation: f64,
    pub occupied_from: Option<PolityId>,
}

/// Stehende Befehle des Belagerers. Was nicht wirken kann, steht in
/// [`SiegeReport::blocked`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SiegeOrders {
    pub fire: Fire,
    /// Sturm statt Halten.
    pub assault: bool,
    /// Schwerpunkt: Index in [`Siege::zones`], sobald das Umland genommen ist.
    pub focus: u8,
    /// Nach dem Fall plündern.
    pub sack: bool,
}

/// Teil der Oberfläche eines belagerten Planeten.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Zone {
    pub kind: ZoneKind,
    /// Anteil an Bevölkerung und Wirtschaft.
    pub share: f32,
    /// Miliz der Zone in Verbänden.
    pub militia: f64,
    /// Vom Belagerer genommen.
    pub taken: bool,
}

/// Bilanz des letzten gerechneten Tages.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SiegeReport {
    pub day: u64,
    /// Zone, in der gekämpft wurde.
    pub zone: Option<u8>,
    /// Zone, die gefallen ist.
    pub fell: Option<u8>,
    /// Gefecht im Untergrund.
    pub underground: bool,
    /// Ausfall der Verteidiger gegen eine haltende Truppe.
    pub sortie: bool,
    /// Die Landung wurde abgeschlagen, die Zonen sind zurück.
    pub repelled: bool,
    /// Verluste der Landungstruppe im Gefecht und durch fehlenden Nachschub (V).
    pub attacker_lost: f64,
    pub attacker_stranded: f64,
    /// Verluste der Verteidiger im Gefecht und durch Beschuss, dazu aufgelöste Verbände:
    /// Hunger, gefallene Zonen, Reste ohne Platz im Untergrund (V).
    pub defender_lost: f64,
    pub defender_bombed: f64,
    pub defender_dissolved: f64,
    /// Rumpfschaden an den Schiffen im Orbit und Anteil der Magazine.
    pub hull: f64,
    pub ammo: f64,
    /// Was der Schild vom Beschuss geschluckt hat.
    pub shield_absorbed: f64,
    /// Zerstörte Stufen.
    pub stations_lost: f64,
    pub guns_lost: f64,
    /// Zerstörter Anteil von Bevölkerung und Kapital: durch Beschuss, durch Bodenkrieg.
    pub fire_damage: f64,
    pub ground_damage: f64,
    /// Verbrauchte Vorratstage.
    pub depot_used: f64,
    pub blocked: BlockedSet,
}

/// Datensatz einer laufenden Belagerung. Höchstens einer je Planet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Siege {
    pub attacker: PolityId,
    pub defender: PolityId,
    pub started_day: u64,
    pub orders: SiegeOrders,
    pub zones: Vec<Zone>,
    /// Landungstruppe am Boden, ihr Höchststand und ihr Nachschub in Verbandstagen.
    pub army: f64,
    pub army_peak: f64,
    pub army_supply: f64,
    /// Tag der letzten Landung. Befehle wirken zwischen den Tagesgrenzen; der Tag danach
    /// sieht die Schiffe noch im niedrigen Orbit.
    pub landed_day: Option<u64>,
    /// Reguläre im Untergrund; die Reserve ist der Rest.
    pub deep: f64,
    /// Garnison bei Beginn (Miliz und Reguläre) und der Teil davon an der Oberfläche.
    pub garrison_start: f64,
    pub surface_start: f64,
    /// Wert der Beute und Bevölkerung bei Beginn.
    pub prize_start: f64,
    pub population_start: f64,
    /// Anteile am Wert der Betriebe der Welt je Gemeinschaft; monatlich aufgefrischt.
    pub stakes: BTreeMap<PolityId, f64>,
    pub report: SiegeReport,
}

impl Siege {
    /// Anteil des Belagerers an den Betrieben der Welt (0..1).
    pub fn stake(&self) -> f64 {
        self.stakes
            .get(&self.attacker)
            .copied()
            .unwrap_or(0.0)
            .clamp(0.0, 1.0)
    }

    /// Miliz aller Zonen.
    pub fn zone_militia(&self) -> f64 {
        self.zones.iter().map(|zone| zone.militia).sum()
    }

    /// Bewegliche Reguläre an der Oberfläche.
    pub fn reserve(&self, defense: &PlanetDefense) -> f64 {
        (defense.regulars - self.deep).max(0.0)
    }

    /// Alle Verteidiger: Miliz, Reserve und Untergrund.
    pub fn defenders(&self, defense: &PlanetDefense) -> f64 {
        self.zone_militia() + defense.regulars
    }

    /// Hat der Belagerer eine Zone dieser Art genommen?
    pub fn zone_taken(&self, kind: ZoneKind) -> bool {
        self.zones
            .iter()
            .any(|zone| zone.kind == kind && zone.taken)
    }

    /// Die Oberfläche ist verloren, sobald jede Zone genommen ist. Verliert der Belagerer
    /// seine Truppe, fallen die Zonen zurück.
    pub fn surface_lost(&self) -> bool {
        !self.zones.is_empty() && self.zones.iter().all(|zone| zone.taken)
    }

    /// Zone, um die als nächste gekämpft wird: das Umland, bis es genommen ist, dann der
    /// Schwerpunkt, sonst die nächste gehaltene in Listenreihenfolge.
    pub fn front(&self) -> Option<usize> {
        if let Some(outskirts) = self
            .zones
            .iter()
            .position(|zone| zone.kind == ZoneKind::Outskirts && !zone.taken)
        {
            return Some(outskirts);
        }
        let count = self.zones.len();
        let focus = (self.orders.focus as usize).min(count.saturating_sub(1));
        (0..count)
            .map(|offset| (focus + offset) % count)
            .find(|index| !self.zones[*index].taken)
    }

    pub fn phase(&self, defense: &PlanetDefense) -> Phase {
        if self.surface_lost() {
            Phase::Underground
        } else if self.army > 0.0 {
            Phase::Ground
        } else if defense.stations > 0.0 {
            Phase::Approach
        } else if self.orders.fire >= Fire::Precise {
            Phase::Bombardment
        } else {
            Phase::Investment
        }
    }
}

/// Technologien, die Ausbaustufen und Kampfweisen freischalten; Index in [`TechView::gates`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    /// „Tiefbunkerbau": Bunker über die erste Stufe hinaus.
    Bunker,
    /// „Orbitale Feuerleitung": volles Stützfeuer.
    Support,
    /// „Tunnelkriegführung": Vorteil im Untergrund.
    Tunnel,
    /// „Schildgeneratoren": Schilde über die erste Stufe hinaus.
    Shield,
}

/// Technik einer Seite, wie der Tagesrechner sie sieht: Aufschläge (0.1 = +10 %), null
/// ist der Grundstand.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TechView {
    pub ground_attack: f64,
    pub ground_defense: f64,
    pub weapon_damage: f64,
    pub shields: f64,
    pub hull: f64,
    pub armor: f64,
    /// Beherrschte Technologien in der Reihenfolge von [`Gate`].
    pub gates: [bool; 4],
}

impl TechView {
    pub fn has(&self, gate: Gate) -> bool {
        self.gates[gate as usize]
    }
}

/// Der Planet an einem Tag, soweit der Tagesrechner ihn braucht.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PlanetFacts {
    /// Heutiger Tag.
    pub day: u64,
    /// Bevölkerung in Millionen.
    pub population: f64,
    /// Zusammenhalt der Region, 0..100.
    pub cohesion: f64,
    /// Deckung des Lebensnotwendigen, 0..1.
    pub essentials_met: f64,
    /// Anteil dessen, was die Garnison heute am Markt kaufen wollte und bekam, 0..1.
    pub forage: f64,
}

/// Die Lage im Orbit an einem Tag.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct OrbitView {
    /// Orbitalhoheit: Eine zugeordnete Flotte des Belagerers steht im System, ist nicht
    /// im Gefecht, und keine ihm feindliche Flotte steht dort.
    pub control: bool,
    /// Feuerkraft der zugeordneten Schiffe.
    pub firepower: f64,
    /// Füllung der Magazine, 0..1.
    pub ammo: f64,
    /// Eingeschiffte Truppen der zugeordneten Flotten im System (V).
    pub embarked: f64,
    /// Technik des Belagerers und des Verteidigers.
    pub att: TechView,
    pub def: TechView,
}

/// Durchhaltewille der Verteidiger (über 0, höchstens 1): Zusammenhalt trägt, ein Anteil
/// des Belagerers an den Betrieben der Welt zehrt.
pub fn will(cohesion: f64, stake: f64, balance: &SiegeBalance) -> f64 {
    let cohesion = cohesion.clamp(0.0, 100.0) / 100.0;
    (balance.will_base + (1.0 - balance.will_base) * cohesion)
        * (1.0 - balance.will_stake * stake.clamp(0.0, 1.0))
}

/// Eingeschlossen: Orbitalhoheit, und keine Station steht mehr.
pub fn invested(orbit: &OrbitView, defense: &PlanetDefense) -> bool {
    orbit.control && defense.stations <= 0.0
}

/// Können die Bodengeschütze feuern? Sie brauchen Vorräte, die Batterie und die Freigabe.
pub fn guns_active(siege: &Siege, defense: &PlanetDefense) -> bool {
    defense.guns > 0.0
        && !siege.zone_taken(ZoneKind::Battery)
        && defense.depot > 0.0
        && !defense.hold_fire
}

/// Besatzungstruppe in Verbänden, die eine Übergabe annehmen kann.
pub fn occupation_force(population: f64, balance: &SiegeBalance) -> f64 {
    balance.occupy_k * population.max(0.0).sqrt()
}

/// Weg des Belagerers, für den [`forecast`] den Verlauf vorausrechnet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Course {
    /// Kein Feuer, keine Landung: warten, bis die Vorräte enden.
    Starve,
    /// Jetzt alles landen und stürmen, mit Stützfeuer.
    Storm,
    /// Flächenbeschuss; gelandet wird, sobald der Sturm mit tragbaren Verlusten gelingt.
    Area,
}

impl Course {
    pub const ALL: [Course; 3] = [Course::Starve, Course::Storm, Course::Area];

    pub fn label(self) -> &'static str {
        match self {
            Course::Starve => "Aushungern",
            Course::Storm => "Sturm",
            Course::Area => "Flächenbeschuss",
        }
    }
}

/// Woran ein Weg scheitert.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Limit {
    NoControl,
    Stations,
    /// Die Flotte hält dem Feuer von Stationen und Geschützen nicht stand.
    Fleet,
    Fuel,
    Ammunition,
    Shield,
    /// Keine Truppen an Bord, oder zu wenige für die Besatzung.
    NoTroops,
    /// Die Landung würde abgeschlagen.
    Repelled,
    /// Die Frist `max_siege_days` läuft ab.
    Time,
}

impl Limit {
    pub fn label(self) -> &'static str {
        match self {
            Limit::NoControl => "keine Orbitalhoheit",
            Limit::Stations => "Stationen halten den Orbit",
            Limit::Fleet => "Flotte hält dem Feuer nicht stand",
            Limit::Fuel => "Treibstoff reicht nicht",
            Limit::Ammunition => "Munition reicht nicht",
            Limit::Shield => "Schild hält",
            Limit::NoTroops => "Truppen fehlen",
            Limit::Repelled => "Landung wird abgeschlagen",
            Limit::Time => "Frist läuft ab",
        }
    }
}

/// Vorausgerechneter Verlauf eines Wegs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Forecast {
    /// Der Planet fällt auf diesem Weg.
    pub falls: bool,
    /// Wie er fällt.
    pub outcome: Option<Outcome>,
    /// Tage bis zum Fall, sonst bis der Weg endet.
    pub days: u32,
    /// Eigene Verluste am Boden in Verbänden und die der Verteidiger durch Gefecht und
    /// Beschuss.
    pub own_losses: f64,
    pub enemy_losses: f64,
    /// Rumpfanteil (0..1), den die schweren Schiffe der Flotte verlieren.
    pub hull_loss: f64,
    /// Anteil der heutigen Beute (0..1), der verloren geht.
    pub prize_loss: f64,
    /// Woran der Weg scheitert; `None`, wenn er gelingt.
    pub limited_by: Option<Limit>,
}

/// Lesemodell einer Belagerung für Prognose, KI und Oberfläche: der Datensatz und alles,
/// was der Tagesrechner dazu braucht, aus der Sicht eines Betrachters. Für fremde Welten
/// stehen hier Schätzungen. Einfache Daten; gebaut aus der Welt oder von Hand.
#[derive(Clone, Debug, PartialEq)]
pub struct SiegeView {
    pub planet: PlanetId,
    pub siege: Siege,
    pub defense: PlanetDefense,
    pub facts: PlanetFacts,
    pub orbit: OrbitView,
    /// Standfestigkeit der zugeordneten Schiffe: Summe aus Gewicht der Klasse
    /// (`SiegeBalance::gun_weight`) mal Rumpf, Schild und Panzerung. Null heißt unbekannt:
    /// Die Prognose lässt die Feuerkraft dann nicht schwinden.
    pub toughness: f64,
    /// Füllung der Tanks, 0..1.
    pub fuel: f64,
    /// Nachschub an Bord in Verbandstagen.
    pub supply_aboard: f64,
    /// Buchwert der Betriebe der Welt heute: der Teil der Beute, der am Kapital hängt.
    pub firm_value: f64,
}

impl SiegeView {
    /// Wert der Beute heute: Betriebe und Bevölkerung.
    pub fn prize(&self, balance: &SiegeBalance) -> f64 {
        self.firm_value + balance.prize_per_million * self.facts.population
    }
}

/// Truppen an Bord einer Flotte.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Embarked {
    /// Verbände.
    pub troops: f64,
    /// Nachschub in Verbandstagen.
    pub supply: f64,
}

/// Fälliger Besitzwechsel, den der Abschluss des Tages ausführt.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Conquest {
    pub planet: PlanetId,
    pub to: PolityId,
    pub outcome: Outcome,
    pub sack: bool,
}

/// Gespeicherte Operation einer KI-Gemeinschaft; ihr Stand ergibt sich aus dem Zustand.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AiPlan {
    pub target: PlanetId,
    /// Welt, auf der die Truppen gesammelt werden.
    pub staging: PlanetId,
    pub fleets: Vec<FleetId>,
    pub started_day: u64,
}

/// Kennzahlen aller Belagerungen einer Partie, für Berichte und Sweeps.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SiegeStats {
    pub begun: u32,
    pub stormed: u32,
    pub starved: u32,
    pub capitulated: u32,
    pub lifted: u32,
    /// Tage aller beendeten Belagerungen zusammen und die längste.
    pub days: u64,
    pub longest: u64,
    /// Wert der Beute aller eroberten Welten bei Beginn und beim Fall.
    pub prize_start: f64,
    pub prize_kept: f64,
    /// Verlorene Verbände der Belagerer und der Verteidiger.
    pub lost_attackers: f64,
    pub lost_defenders: f64,
}

impl SiegeStats {
    pub fn count(&self, outcome: Outcome) -> u32 {
        match outcome {
            Outcome::Stormed => self.stormed,
            Outcome::Starved => self.starved,
            Outcome::Capitulated => self.capitulated,
            Outcome::Lifted => self.lifted,
        }
    }

    /// Eroberungen: jeder Ausgang außer der Aufhebung.
    pub fn conquests(&self) -> u32 {
        self.stormed + self.starved + self.capitulated
    }

    /// Verbucht das Ende einer Belagerung nach `days` Tagen.
    pub fn record(&mut self, outcome: Outcome, days: u64) {
        match outcome {
            Outcome::Stormed => self.stormed += 1,
            Outcome::Starved => self.starved += 1,
            Outcome::Capitulated => self.capitulated += 1,
            Outcome::Lifted => self.lifted += 1,
        }
        self.days += days;
        self.longest = self.longest.max(days);
    }
}

/// Zustand aller Belagerungen und der stehenden Verteidigung.
#[derive(Resource, Clone, Debug, Default, Serialize, Deserialize)]
pub struct Sieges {
    /// Je Planet, dauerhaft.
    pub defenses: Vec<PlanetDefense>,
    /// Laufende Belagerungen, höchstens eine je Planet.
    pub active: BTreeMap<PlanetId, Siege>,
    /// Eine Flotte belagert genau einen Planeten.
    pub assign: BTreeMap<FleetId, PlanetId>,
    pub embarked: BTreeMap<FleetId, Embarked>,
    /// Fällige Besitzwechsel.
    pub pending: Vec<Conquest>,
    /// Rest des Rüstungsetats dieses Monats je Gemeinschaft.
    pub budget: BTreeMap<PolityId, Money>,
    pub plans: BTreeMap<PolityId, AiPlan>,
    /// (Verursacher, Geschädigter) → vernichtetes oder geplündertes Vermögen in Credits.
    pub grievances: BTreeMap<(PolityId, PolityId), f64>,
    pub stats: SiegeStats,
}

impl Sieges {
    /// Jeder Planet ohne Verteidigung, keine Belagerung.
    pub fn new(planets: usize) -> Self {
        Self {
            defenses: vec![PlanetDefense::default(); planets],
            ..Default::default()
        }
    }

    pub fn defense(&self, planet: PlanetId) -> &PlanetDefense {
        &self.defenses[planet.index()]
    }

    pub fn defense_mut(&mut self, planet: PlanetId) -> &mut PlanetDefense {
        &mut self.defenses[planet.index()]
    }

    pub fn siege(&self, planet: PlanetId) -> Option<&Siege> {
        self.active.get(&planet)
    }

    /// Gehört die Flotte zu einer geplanten Operation? Dann lässt die Flotten-KI sie in Ruhe.
    pub fn reserved(&self, fleet: FleetId) -> bool {
        self.plans.values().any(|plan| plan.fleets.contains(&fleet))
    }

    /// Prüft, dass kein Wert entgleist ist. Gibt die erste Auffälligkeit zurück.
    pub fn find_anomaly(&self, planets: usize) -> Option<String> {
        let sound = |value: f64| value.is_finite() && value >= 0.0;
        let share = |value: f64| value.is_finite() && (0.0..=1.0).contains(&value);
        if self.defenses.len() != planets {
            return Some(format!(
                "Verteidigung für {} Planeten, die Galaxie hat {planets}",
                self.defenses.len()
            ));
        }
        for (index, defense) in self.defenses.iter().enumerate() {
            let strengths = [
                defense.stations as f64,
                defense.guns as f64,
                defense.charge as f64,
                defense.regulars,
                defense.militia,
                defense.depot,
                defense.plan.regulars as f64,
            ];
            if strengths.into_iter().any(|value| !sound(value)) {
                return Some(format!(
                    "Planet {index}: Stärke oder Vorrat der Verteidigung entgleist"
                ));
            }
            if !share(defense.scar) || !share(defense.occupation) {
                return Some(format!(
                    "Planet {index}: Kriegsschäden oder Besatzung außerhalb von 0..1"
                ));
            }
        }
        for (planet, siege) in &self.active {
            if planet.index() >= planets {
                return Some(format!("Belagerung eines unbekannten Planeten {planet}"));
            }
            let defense = &self.defenses[planet.index()];
            let strengths = [
                siege.army,
                siege.army_peak,
                siege.army_supply,
                siege.deep,
                siege.garrison_start,
                siege.surface_start,
                siege.prize_start,
                siege.population_start,
            ];
            if strengths.into_iter().any(|value| !sound(value))
                || siege.zones.iter().any(|zone| !sound(zone.militia))
            {
                return Some(format!(
                    "Belagerung von {planet}: Stärke oder Vorrat entgleist"
                ));
            }
            if siege.deep > defense.regulars + 1e-9 {
                return Some(format!(
                    "Belagerung von {planet}: mehr Reguläre im Untergrund als vorhanden"
                ));
            }
            let shares: f64 = siege.zones.iter().map(|zone| zone.share as f64).sum();
            if siege.zones.is_empty()
                || siege.zones.iter().any(|zone| !share(zone.share as f64))
                || (shares - 1.0).abs() > 1e-3
            {
                return Some(format!(
                    "Belagerung von {planet}: Zonenanteile ergeben nicht das Ganze"
                ));
            }
            if siege.stakes.values().any(|stake| !share(*stake)) {
                return Some(format!(
                    "Belagerung von {planet}: Besitzanteil außerhalb von 0..1"
                ));
            }
        }
        if self.assign.values().any(|planet| planet.index() >= planets) {
            return Some("Flotte einem unbekannten Planeten zugeordnet".into());
        }
        if self
            .embarked
            .values()
            .any(|force| !sound(force.troops) || !sound(force.supply))
        {
            return Some("Eingeschiffte Truppen oder ihr Nachschub entgleist".into());
        }
        if self
            .pending
            .iter()
            .any(|conquest| conquest.planet.index() >= planets)
        {
            return Some("Besitzwechsel eines unbekannten Planeten".into());
        }
        if self.budget.values().any(|budget| *budget < 0) {
            return Some("Rüstungsetat negativ".into());
        }
        if self.grievances.values().any(|credits| !sound(*credits)) {
            return Some("Vermerk über geschädigte Dritte entgleist".into());
        }
        let stats = &self.stats;
        if [
            stats.prize_start,
            stats.prize_kept,
            stats.lost_attackers,
            stats.lost_defenders,
        ]
        .into_iter()
        .any(|value| !sound(value))
        {
            return Some("Kennzahlen der Belagerungen entgleist".into());
        }
        None
    }
}

/// Schließt die Belagerung eines Planeten: die eine Stelle, an der ein Datensatz endet.
/// Was von der Miliz der Zonen übrig ist, steht wieder als Miliz der Welt, ein fälliger
/// Besitzwechsel verfällt, die Region gilt nicht mehr als belagert, und die Kennzahlen
/// verbuchen Ausgang und Dauer. Gibt den Datensatz zurück; was aus einer Truppe am Boden
/// wird, entscheidet der Aufrufer.
pub fn close(
    sieges: &mut Sieges,
    finance: &mut Finance,
    planet: PlanetId,
    outcome: Outcome,
    day: u64,
) -> Option<Siege> {
    let siege = sieges.active.remove(&planet)?;
    if let Some(defense) = sieges.defenses.get_mut(planet.index()) {
        defense.militia = siege.zone_militia();
    }
    sieges.pending.retain(|due| due.planet != planet);
    if let Some(region) = finance.regions.get_mut(planet.index()) {
        region.besieged = false;
    }
    sieges
        .stats
        .record(outcome, day.saturating_sub(siege.started_day));
    Some(siege)
}

/// Zustand bei Spielbeginn: Jede Heimatwelt hat die stehende Verteidigung aus
/// `siege.home_start`, alle anderen Welten keine. Keine Belagerung.
pub fn setup_sieges(data: &GameData, galaxy: &Galaxy, polities: &Polities) -> Sieges {
    let start = &data.balance.siege.home_start;
    let mut sieges = Sieges::new(galaxy.planets.len());
    for capital in polities.list.iter().filter_map(|polity| polity.capital) {
        let defense = sieges.defense_mut(capital);
        defense.guns = start.guns as f32;
        defense.regulars = start.regulars;
        defense.depot = start.depot_days as f64;
        // Der Plan nennt, was steht: So wird später weder abgebaut noch nachgerüstet.
        defense.plan = DefensePlan {
            guns: start.guns,
            regulars: start.regulars as f32,
            depot_days: start.depot_days,
            ..Default::default()
        };
    }
    sieges
}

/// Was in jedem Takt zwischen den Belagerungen und dem Rest der Welt stimmen muss: Der
/// Verteidiger hält die Welt und steht mit dem Belagerer im Krieg, die Miliz der Welt ist
/// die ihrer Zonen, die Region gilt genau dann als belagert, wenn ein Datensatz besteht,
/// und jeder fällige Besitzwechsel gehört zu einer Belagerung dessen, der ihn antritt.
pub fn check_records(
    galaxy: &Galaxy,
    diplomacy: &Diplomacy,
    finance: &Finance,
    sieges: &Sieges,
) -> Result<(), String> {
    for (planet, siege) in &sieges.active {
        let target = galaxy.planet(*planet);
        if target.owner != Some(siege.defender) {
            return Err(format!(
                "{}: Der Verteidiger der Belagerung hält die Welt nicht mehr",
                target.name
            ));
        }
        if !diplomacy.at_war(siege.attacker, siege.defender) {
            return Err(format!(
                "{}: Belagerung ohne Krieg zwischen Belagerer und Besitzer",
                target.name
            ));
        }
        let militia = siege.zone_militia();
        if (sieges.defense(*planet).militia - militia).abs() > 1e-6 * militia.max(1.0) {
            return Err(format!(
                "{}: Miliz der Welt weicht von der Summe ihrer Zonen ab",
                target.name
            ));
        }
    }
    for (planet, region) in galaxy.planets.iter().zip(&finance.regions) {
        if region.besieged != sieges.active.contains_key(&planet.id) {
            return Err(format!(
                "{}: Die Region gilt als {}, der Datensatz sagt das Gegenteil",
                planet.name,
                if region.besieged {
                    "belagert"
                } else {
                    "nicht belagert"
                }
            ));
        }
    }
    for due in &sieges.pending {
        let backed = sieges
            .siege(due.planet)
            .is_some_and(|siege| siege.attacker == due.to);
        if !backed {
            return Err(format!(
                "Besitzwechsel von {} steht aus, ohne dass {} die Welt belagert",
                galaxy.planet(due.planet).name,
                due.to
            ));
        }
    }
    Ok(())
}

/// Invarianten der Belagerung: Stärken und Vorräte endlich und nicht negativ, Listen so
/// lang wie die Galaxie, der Stillstand der Wirtschaft und die Spiegelwerte der Regionen
/// in 0..1; Datensätze passen zu Besitz und Kriegslage ([`check_records`]), Zuordnungen
/// und eingeschiffte Truppen zu den Flotten.
pub fn check(world: &World) -> Result<(), String> {
    let galaxy = world.resource::<Galaxy>();
    let planets = galaxy.planets.len();
    if let Some(anomaly) = world.resource::<Sieges>().find_anomaly(planets) {
        return Err(anomaly);
    }
    let share = |value: f64| value.is_finite() && (0.0..=1.0).contains(&value);
    let economy = world.resource::<Economy>();
    if economy.planets.len() != planets {
        return Err("Wirtschaft und Galaxie zählen verschieden viele Planeten".into());
    }
    for (planet, local) in galaxy.planets.iter().zip(&economy.planets) {
        if !share(local.disruption) {
            return Err(format!(
                "{}: Stillstand der Wirtschaft {} außerhalb von 0..1",
                planet.name, local.disruption
            ));
        }
    }
    let finance = world.resource::<Finance>();
    if finance.regions.len() != planets {
        return Err("Finanzen und Galaxie zählen verschieden viele Regionen".into());
    }
    for (planet, region) in galaxy.planets.iter().zip(&finance.regions) {
        if !share(region.occupation) || !share(region.garrison_cover) || !share(region.war_damage) {
            return Err(format!(
                "{}: Besatzung, Garnison oder Kriegsschäden außerhalb von 0..1",
                planet.name
            ));
        }
    }
    check_records(
        galaxy,
        world.resource::<Diplomacy>(),
        finance,
        world.resource::<Sieges>(),
    )?;
    orbit::check(world)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finance::{CohesionFactors, cohesion_target};
    use crate::harness::Headless;
    use crate::state::{SaveGame, SimConfig, SimState};

    fn data() -> GameData {
        GameData::load_workspace().unwrap()
    }

    #[test]
    fn a_new_game_starts_with_defended_home_worlds_and_without_sieges() {
        let data = data();
        let state = SimState::generate(
            &data,
            SimConfig {
                seed: 4,
                player: None,
            },
        );
        assert_eq!(state.sieges.defenses.len(), state.galaxy.planets.len());
        // Jede Heimatwelt beginnt mit den Startwerten aus den Spieldaten: Geschütze,
        // reguläre Verbände und Vorräte, und ihr Plan nennt genau das.
        let start = &data.balance.siege.home_start;
        assert!(start.regulars > 0.0 && start.depot_days > 0);
        let capitals: Vec<PlanetId> = state
            .polities
            .list
            .iter()
            .map(|polity| polity.capital.unwrap())
            .collect();
        assert_eq!(capitals.len(), data.galaxy.polities as usize);
        for (index, defense) in state.sieges.defenses.iter().enumerate() {
            if !capitals.contains(&PlanetId::from_index(index)) {
                assert_eq!(*defense, PlanetDefense::default(), "Planet {index}");
                continue;
            }
            assert_eq!(defense.guns, start.guns as f32);
            assert_eq!(defense.regulars, start.regulars);
            assert_eq!(defense.depot, start.depot_days as f64);
            assert_eq!(defense.plan.guns, start.guns);
            assert_eq!(defense.plan.regulars as f64, start.regulars);
            assert_eq!(defense.plan.depot_days, start.depot_days);
            let rest = PlanetDefense {
                guns: 0.0,
                regulars: 0.0,
                depot: 0.0,
                plan: DefensePlan::default(),
                ..defense.clone()
            };
            assert_eq!(rest, PlanetDefense::default());
        }
        assert!(state.sieges.active.is_empty() && state.sieges.pending.is_empty());
        assert!(state.sieges.assign.is_empty() && state.sieges.embarked.is_empty());
        assert_eq!(state.sieges.find_anomaly(state.galaxy.planets.len()), None);
        // Die neuen Größen in Wirtschaft und Finanzen beginnen ohne Wirkung.
        assert!(
            state
                .economy
                .planets
                .iter()
                .all(|local| local.disruption == 0.0)
        );
        assert!(state.finance.regions.iter().all(|region| {
            !region.besieged
                && region.occupation == 0.0
                && region.garrison_cover == 0.0
                && region.war_damage == 0.0
        }));
        let mut world = World::new();
        state.install(&mut world);
        assert_eq!(check(&world), Ok(()));
    }

    #[test]
    fn derailed_values_are_reported() {
        let mut sieges = Sieges::new(3);
        assert_eq!(sieges.find_anomaly(3), None);
        assert!(sieges.find_anomaly(4).unwrap().contains("Planeten"));
        sieges.defenses[1].depot = -1.0;
        assert!(sieges.find_anomaly(3).unwrap().contains("Planet 1"));
        sieges.defenses[1].depot = f64::NAN;
        assert!(sieges.find_anomaly(3).is_some());
        sieges.defenses[1].depot = 10.0;
        sieges.defenses[2].scar = 1.5;
        assert!(sieges.find_anomaly(3).unwrap().contains("Kriegsschäden"));
        sieges.defenses[2].scar = 0.5;
        sieges.embarked.insert(
            FleetId(0),
            Embarked {
                troops: f64::INFINITY,
                supply: 0.0,
            },
        );
        assert!(sieges.find_anomaly(3).unwrap().contains("Eingeschiffte"));
        sieges.embarked.clear();
        sieges.assign.insert(FleetId(0), PlanetId(7));
        assert!(sieges.find_anomaly(3).unwrap().contains("zugeordnet"));
    }

    #[test]
    fn disruption_and_region_mirrors_are_checked_in_the_world() {
        let data = data();
        let state = SimState::generate(
            &data,
            SimConfig {
                seed: 4,
                player: None,
            },
        );
        let mut world = World::new();
        state.install(&mut world);
        world.resource_mut::<Economy>().planets[0].disruption = 1.2;
        assert!(check(&world).unwrap_err().contains("Stillstand"));
        world.resource_mut::<Economy>().planets[0].disruption = 0.4;
        assert_eq!(check(&world), Ok(()));
        world.resource_mut::<Finance>().regions[2].war_damage = -0.1;
        assert!(check(&world).unwrap_err().contains("Kriegsschäden"));
    }

    /// Datensätze, die nicht zu Besitz, Kriegslage, Spiegelwerten oder Flotten passen,
    /// fallen der Prüfung auf; ein stimmiger Zustand nicht.
    #[test]
    fn records_out_of_step_with_the_world_are_reported() {
        let mut scene = day::testing::Scene::colony();
        let (me, foe) = (day::testing::ME, day::testing::FOE);
        scene.state.sieges.embarked.insert(
            FleetId(0),
            Embarked {
                troops: 5.0,
                supply: 0.0,
            },
        );
        scene.day();
        let planet = scene.planet;
        assert!(scene.state.sieges.siege(planet).is_some());
        let verdict = |state: &SimState| {
            let mut world = World::new();
            world.insert_resource(scene.data.clone());
            state.clone().install(&mut world);
            check(&world)
        };
        assert_eq!(verdict(&scene.state), Ok(()));

        // Der Verteidiger hält die Welt nicht mehr; es herrscht kein Krieg mehr.
        let mut state = scene.state.clone();
        state.galaxy.planet_mut(planet).owner = Some(PolityId(2));
        assert!(verdict(&state).unwrap_err().contains("Verteidiger"));
        let mut state = scene.state.clone();
        state.diplomacy.wars.clear();
        assert!(verdict(&state).unwrap_err().contains("ohne Krieg"));
        // Der Spiegelwert der Region und die Miliz der Welt folgen dem Datensatz.
        let mut state = scene.state.clone();
        state.finance.regions[planet.index()].besieged = false;
        assert!(verdict(&state).unwrap_err().contains("Region"));
        let mut state = scene.state.clone();
        state.finance.regions[0].besieged = true;
        assert!(verdict(&state).unwrap_err().contains("Region"));
        let mut state = scene.state.clone();
        state.sieges.defense_mut(planet).militia += 1.0;
        assert!(verdict(&state).unwrap_err().contains("Miliz"));
        // Ein fälliger Besitzwechsel braucht die Belagerung dessen, der ihn antritt.
        let mut state = scene.state.clone();
        state.sieges.pending.push(Conquest {
            planet,
            to: foe,
            outcome: Outcome::Starved,
            sack: false,
        });
        assert!(verdict(&state).unwrap_err().contains("Besitzwechsel"));
        state.sieges.pending[0].to = me;
        assert_eq!(verdict(&state), Ok(()));
        // Zuordnungen zeigen auf bestehende Flotten, Truppen haben Platz an Bord.
        let mut state = scene.state.clone();
        state.fleets.list[0].alive = false;
        assert!(verdict(&state).unwrap_err().contains("besteht"));
        let mut state = scene.state.clone();
        state.sieges.assign.clear();
        state.sieges.embarked.get_mut(&FleetId(0)).unwrap().troops = 18.0;
        assert!(verdict(&state).unwrap_err().contains("Plätzen"));
        state.sieges.embarked.get_mut(&FleetId(0)).unwrap().troops = 17.5;
        assert_eq!(verdict(&state), Ok(()));
    }

    /// Ein gefüllter Zustand übersteht Speichern und Laden unverändert: RON für den
    /// Spielstand, postcard für die Prüfsumme.
    #[test]
    fn a_running_siege_survives_saving_and_loading() {
        let data = data();
        let balance = &data.balance.siege;
        let mut state = SimState::generate(
            &data,
            SimConfig {
                seed: 4,
                player: None,
            },
        );
        let planet = PlanetId(1);
        let facts = PlanetFacts {
            day: 12,
            population: 1500.0,
            cohesion: 64.0,
            essentials_met: 0.9,
            forage: 0.5,
        };
        let defense = state.sieges.defense_mut(planet);
        defense.guns = 1.5;
        defense.shield = 1;
        defense.charge = 1200.0;
        defense.bunkers = 2;
        defense.regulars = 12.25;
        defense.depot = 47.5;
        defense.posture = Posture::Deep;
        defense.scar = 0.125;
        defense.scar_by = Some(PolityId(1));
        let stakes = BTreeMap::from([(PolityId(0), 0.75), (PolityId(1), 0.25)]);
        let mut siege = form(
            PolityId(1),
            PolityId(0),
            defense,
            &facts,
            stakes,
            90_000.0,
            balance,
        );
        siege.orders = SiegeOrders {
            fire: Fire::Precise,
            assault: true,
            focus: 2,
            sack: true,
        };
        land(&mut siege, defense, 20.0, 12, balance);
        let orbit = OrbitView {
            control: true,
            firepower: 300.0,
            ammo: 0.8,
            embarked: 5.0,
            ..Default::default()
        };
        resolve(&mut siege, defense, &facts, &orbit, balance);
        state.sieges.active.insert(planet, siege);
        state.sieges.assign.insert(FleetId(1), planet);
        state.sieges.embarked.insert(
            FleetId(1),
            Embarked {
                troops: 5.0,
                supply: 450.0,
            },
        );
        state.sieges.pending.push(Conquest {
            planet,
            to: PolityId(1),
            outcome: Outcome::Starved,
            sack: false,
        });
        state.sieges.budget.insert(PolityId(1), 123_456);
        state.sieges.plans.insert(
            PolityId(1),
            AiPlan {
                target: planet,
                staging: PlanetId(0),
                fleets: vec![FleetId(1)],
                started_day: 3,
            },
        );
        state
            .sieges
            .grievances
            .insert((PolityId(1), PolityId(2)), 1234.5);
        state.sieges.stats.record(Outcome::Lifted, 40);
        assert_eq!(state.sieges.find_anomaly(state.galaxy.planets.len()), None);

        let save = SaveGame {
            format: crate::state::SAVE_FORMAT,
            data_hash: data.hash,
            state: state.clone(),
        };
        let loaded = SaveGame::from_ron(&save.to_ron().unwrap()).unwrap();
        assert_eq!(loaded.state.sieges.active, state.sieges.active);
        assert_eq!(loaded.state.sieges.defenses, state.sieges.defenses);
        assert_eq!(loaded.state.sieges.grievances, state.sieges.grievances);
        assert_eq!(loaded.state.sieges.stats, state.sieges.stats);
        assert_eq!(loaded.state.hash(), state.hash());
        assert!(state.sieges.reserved(FleetId(1)) && !state.sieges.reserved(FleetId(0)));
    }

    #[test]
    fn will_follows_cohesion_and_the_besiegers_stake() {
        let data = data();
        let balance = &data.balance.siege;
        let firm = will(100.0, 0.0, balance);
        let shaky = will(0.0, 0.0, balance);
        assert!((firm - 1.0).abs() < 1e-12);
        assert!((shaky - balance.will_base).abs() < 1e-12);
        // Wem die Betriebe schon gehören, dem leistet man weniger Widerstand.
        assert!(will(70.0, 0.6, balance) < will(70.0, 0.0, balance));
        // Nie null: Sonst wäre die Auflösung ohne Vorräte unbegrenzt.
        assert!(will(-20.0, 3.0, balance) > 0.0);
    }

    #[test]
    fn blocked_reasons_form_a_set() {
        let mut set = BlockedSet::default();
        assert!(set.is_empty());
        set.insert(Blocked::ShieldHolds);
        set.insert(Blocked::NoArmy);
        set.insert(Blocked::ShieldHolds);
        assert_eq!(
            set.iter().collect::<Vec<_>>(),
            vec![Blocked::ShieldHolds, Blocked::NoArmy]
        );
        assert!(!set.contains(Blocked::NoAmmo));
    }

    #[test]
    fn occupation_and_war_damage_weigh_on_cohesion_only_when_present() {
        let data = data();
        let balance = &data.balance.finance.cohesion;
        let neutral = CohesionFactors {
            supply: 1.0,
            tax: balance.tax_neutral,
            years: 2.0,
            ..Default::default()
        };
        let points = |factors: &CohesionFactors, label: &str| {
            factors
                .contributions(balance)
                .into_iter()
                .find(|(name, _)| *name == label)
                .map(|(_, points)| points)
                .unwrap_or_else(|| panic!("Beitrag „{label}“ fehlt"))
        };
        // Ohne Besatzung und Schäden bleibt alles, wie es war.
        assert_eq!(points(&neutral, "Besatzung"), 0.0);
        assert_eq!(points(&neutral, "Kriegsschäden"), 0.0);
        let honeymoon = points(&neutral, "Aufbruchstimmung");
        assert!(
            (honeymoon - balance.honeymoon * (1.0 - 2.0 / balance.honeymoon_years)).abs() < 1e-12
        );
        let target = cohesion_target(&neutral, balance);
        assert!((target - (balance.base + honeymoon)).abs() < 1e-12);

        // Volle Besatzung ohne Garnison: der ganze Abzug, und keine Aufbruchstimmung.
        let occupied = CohesionFactors {
            occupation: 1.0,
            ..neutral.clone()
        };
        assert_eq!(points(&occupied, "Besatzung"), -balance.occupation);
        assert_eq!(points(&occupied, "Aufbruchstimmung"), 0.0);
        assert!(
            (cohesion_target(&occupied, balance) - (balance.base - balance.occupation)).abs()
                < 1e-12
        );
        // Eine volle Garnison fängt einen Teil davon auf: Halten kostet Truppen.
        let held = CohesionFactors {
            garrison_cover: 1.0,
            ..occupied.clone()
        };
        let softened = -balance.occupation * (1.0 - balance.occupation_hold);
        assert!((points(&held, "Besatzung") - softened).abs() < 1e-12);
        assert!(cohesion_target(&held, balance) > cohesion_target(&occupied, balance));
        // Halbe Besatzung lässt die halbe Aufbruchstimmung zu.
        let easing = CohesionFactors {
            occupation: 0.5,
            ..neutral.clone()
        };
        assert!((points(&easing, "Aufbruchstimmung") - 0.5 * honeymoon).abs() < 1e-12);

        // Kriegsschäden wiegen nach ihrem Ausmaß.
        let scarred = CohesionFactors {
            war_damage: 0.4,
            ..neutral.clone()
        };
        assert!((points(&scarred, "Kriegsschäden") + 0.4 * balance.war_damage).abs() < 1e-12);
        assert!(
            (cohesion_target(&scarred, balance) - (target - 0.4 * balance.war_damage)).abs() < 1e-9
        );
    }

    /// Was die Belagerung in eine Region schreibt, kommt beim nächsten Monatsabschluss im
    /// Zusammenhalt an.
    #[test]
    fn region_mirrors_reach_the_cohesion_factors() {
        let data = data();
        let config = SimConfig {
            seed: 4,
            player: None,
        };
        let mut sim = Headless::new(data.clone(), config.clone());
        let mut twin = Headless::new(data, config);
        let home = sim.polities().list[0].capital.unwrap();
        {
            let mut finance = sim.world_mut().resource_mut::<Finance>();
            let region = &mut finance.regions[home.index()];
            region.occupation = 0.8;
            region.garrison_cover = 0.5;
            region.war_damage = 0.3;
        }
        sim.run_days(31);
        twin.run_days(31);
        let factors = &sim.world().resource::<Finance>().region(home).factors;
        assert_eq!(factors.occupation, 0.8);
        assert_eq!(factors.garrison_cover, 0.5);
        assert_eq!(factors.war_damage, 0.3);
        let untouched = &twin.world().resource::<Finance>().region(home).factors;
        assert_eq!(untouched.occupation, 0.0);
        assert!(
            factors.target < untouched.target - 20.0,
            "Ziel {} gegen {}",
            factors.target,
            untouched.target
        );
        assert_eq!(check(sim.world()), Ok(()));
        // Der Zwilling ohne Eingriff rechnet wie eine Partie ohne Belagerungszustand.
        assert_eq!(check(twin.world()), Ok(()));
    }
}

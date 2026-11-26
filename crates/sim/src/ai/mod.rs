//! KI der Gemeinschaften. Sie wirkt wie der Spieler ausschließlich über Befehle.
//!
//! - Ausdehnung und Finanzen (`plugin`): Kolonien, Besitz streuen, Kriegskasse.
//! - Militärische Grundhaltung ([`military`]): Rüstung im Rahmen des Staatshaushalts,
//!   Forschungsetat und -ziel, Führung der Flotten im Frieden und im Krieg. Ob Krieg oder
//!   Frieden herrscht, entscheidet sie nicht; das kommt mit der Diplomatie.
//! - [`estimate`]: was eine Gemeinschaft von fremden Flotten sieht, und die
//!   Kräfteschätzung mit der Kontertabelle.
//! - [`levers`]: die einzige Stelle, an der die KI Flotten-, Werft- und Forschungsbefehle
//!   erteilt. Spätere Planer steuern über diese Hebel.

pub mod estimate;
pub mod levers;
pub mod military;
mod plugin;

pub use plugin::AiPlugin;

use crate::data::GameData;
use crate::economy::Economy;
use crate::galaxy::{Galaxy, Planet};
use crate::ids::{FleetId, PlanetId, PolityId, SystemId};
use crate::polity::{Polities, claimable_planets};
use bevy_ecs::prelude::Resource;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Wie weit die KI eine Gemeinschaft militärisch führt.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MilitaryMode {
    /// Rüstung, Forschungspolitik und Führung der Flotten.
    #[default]
    Active,
    /// Rüstung und Forschungspolitik wie sonst, aber die Flotten erhalten keine Befehle:
    /// Sie bleiben, wo sie sind. Der Vergleichsfall für Messungen.
    PassiveFleet,
    /// Die militärische Führung ruht ganz: keine Bestellungen, keine Stilllegung, keine
    /// Flottenbefehle; der Forschungsetat folgt nur der Vorlage. Für Drehbücher und
    /// Prüfungen, die Flotten selbst führen.
    Resting,
}

/// Wozu eine Flotte gerade unterwegs ist.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mission {
    /// Sucht die Hauptflotte des Gegners.
    Hunt,
    /// Sperrt ein System des Gegners.
    Blockade,
    /// Läuft einen Stützpunkt an, um Vorräte zu fassen und instand gesetzt zu werden.
    Refit,
}

/// Eine laufende Unternehmung einer Flotte.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Operation {
    pub mission: Mission,
    pub target: SystemId,
    /// Bauwert der Flotte bei Beginn: das Maß, an dem Verluste zum Abbruch führen.
    pub start_value: f64,
}

/// Ein Gefecht mit einer fremden Flotte, wie es die Führung in Erinnerung behält.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Meeting {
    /// Letzter Tag des Gefechts.
    pub day: u64,
    /// Ob in dem Gefecht auf irgendeiner Seite etwas verloren ging. Wenn nicht, ist die
    /// eine Seite der anderen nur ausgewichen.
    pub fought: bool,
}

/// Was sich die militärische Führung einer Gemeinschaft von Tag zu Tag merkt.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MilitaryPlan {
    /// Wo die Hauptflotte wartet; monatlich neu bestimmt. `None`: am Sitz.
    pub station: Option<SystemId>,
    /// Monate unter den letzten zwölf, in denen Sold ausblieb; seit der letzten
    /// Stilllegung neu gezählt.
    pub short_months: u32,
    /// Dieselben Monate als Bitfolge, der jüngste zuunterst.
    pub short_mask: u16,
    /// Soldrückstand beim letzten Monatsbeginn; aus der Änderung folgt, ob wieder Sold
    /// ausblieb.
    pub last_arrears: f32,
    /// Faktor auf den Forschungsetat der Vorlage (0..1): sinkt, solange der Haushalt
    /// nicht aufgeht, und erholt sich danach.
    pub research_factor: f64,
    /// Forschungsziel, das die Führung zuletzt selbst gesetzt hat. Ein Ziel, das von
    /// außen gesetzt wurde (Drehbuch, früherer Spieler), lässt sie bis zu seinem Abschluss
    /// stehen.
    pub research_target: Option<u16>,
    /// Laufende Unternehmungen je Flotte.
    pub operations: BTreeMap<FleetId, Operation>,
    /// Begonnene Angriffe (Jagd oder Blockade) über die ganze Partie, für Berichte.
    pub offensives: u32,
    /// Monate, seit zuletzt Sold ausblieb oder ein Schiff stillgelegt wurde.
    #[serde(default = "long_calm")]
    pub calm_months: u32,
    /// Monate seit der letzten Stilllegung.
    #[serde(default = "long_calm")]
    pub months_since_layup: u32,
    /// Was die Gemeinschaft auf lange Sicht im Monat einnimmt, in Credits: ein träges
    /// Mittel über Jahre. 0, solange noch kein Monat abgeschlossen ist.
    #[serde(default)]
    pub usual_revenue: f64,
    /// Fremde Flotten, mit denen eine eigene im Gefecht stand.
    #[serde(default)]
    pub met: BTreeMap<FleetId, Meeting>,
    /// Vor diesem Tag beginnt die Gemeinschaft keinen neuen Angriff und verlässt ihre
    /// Häfen nicht für den Warteplatz: Sie gab einen Angriff vor einem stärkeren Gegner
    /// auf, musste sich aus einem Gefecht zurückziehen oder ihm ausweichen.
    #[serde(default)]
    pub no_offensive_before: u64,
    /// Eigenes System, in dem die Hauptflotte im Krieg wartet: dort stellte sie zuletzt
    /// einen Eindringling. `None`: am Warteplatz.
    #[serde(default)]
    pub post: Option<SystemId>,
}

fn long_calm() -> u32 {
    u32::MAX
}

impl Default for MilitaryPlan {
    fn default() -> Self {
        Self {
            station: None,
            short_months: 0,
            short_mask: 0,
            last_arrears: 0.0,
            research_factor: 1.0,
            research_target: None,
            operations: BTreeMap::new(),
            offensives: 0,
            calm_months: long_calm(),
            months_since_layup: long_calm(),
            usual_revenue: 0.0,
            met: BTreeMap::new(),
            no_offensive_before: 0,
            post: None,
        }
    }
}

/// Gedächtnis und Einstellungen der KI. Teil des Spielstands.
#[derive(Resource, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AiState {
    /// Führung, die für alle gilt, die in `modes` nicht stehen.
    pub default_mode: MilitaryMode,
    /// Abweichende Führung einzelner Gemeinschaften.
    pub modes: BTreeMap<PolityId, MilitaryMode>,
    /// Pläne je Gemeinschaft (Stelle = Nummer der Gemeinschaft).
    pub plans: Vec<MilitaryPlan>,
    /// Flotten, die ein anderer Planer für sich beansprucht. Die Grundhaltung lässt sie
    /// in Ruhe. Heute trägt niemand etwas ein.
    pub reserved: BTreeSet<FleetId>,
}

impl AiState {
    /// Wie die KI diese Gemeinschaft militärisch führt.
    pub fn mode(&self, polity: PolityId) -> MilitaryMode {
        self.modes
            .get(&polity)
            .copied()
            .unwrap_or(self.default_mode)
    }

    /// Hat ein anderer Planer diese Flotte für sich beansprucht? Dann lässt die
    /// Grundhaltung sie in Ruhe.
    pub fn is_reserved(&self, fleet: FleetId) -> bool {
        self.reserved.contains(&fleet)
    }

    /// Der Plan einer Gemeinschaft, wie er ist (ohne Eintrag der leere).
    pub fn plan(&self, polity: PolityId) -> MilitaryPlan {
        self.plans.get(polity.index()).cloned().unwrap_or_default()
    }

    /// Der Plan einer Gemeinschaft zum Fortschreiben.
    pub fn plan_mut(&mut self, polity: PolityId) -> &mut MilitaryPlan {
        if self.plans.len() <= polity.index() {
            self.plans
                .resize(polity.index() + 1, MilitaryPlan::default());
        }
        &mut self.plans[polity.index()]
    }
}

/// Wert einer freien Welt aus Sicht eines Siedlers.
pub fn colony_value(galaxy: &Galaxy, planet: &Planet) -> f64 {
    let living_space = planet.capacity * planet.habitability as f64 / 1000.0;
    let deposits: f64 = planet
        .deposits
        .iter()
        .map(|deposit| (deposit.richness * deposit.reserve) as f64)
        .sum();
    let position = galaxy.system(planet.system).centrality as f64;
    // Lebensfeindliche Welten ohne reiche Vorkommen tragen keine Kolonie.
    let viability = 0.3 + 0.7 * planet.habitability as f64;
    (living_space + deposits * 1.5 + position * 2.0) * viability
}

/// Beste freie Welt, die `polity` heute besiedeln darf. Gleichstand entscheidet die ID.
pub fn best_colony_target(
    data: &GameData,
    galaxy: &Galaxy,
    polities: &Polities,
    economy: &Economy,
    polity: PolityId,
    day: u64,
) -> Option<PlanetId> {
    let mut best: Option<(PlanetId, f64)> = None;
    let treasury = economy.treasury(polity);
    for id in claimable_planets(
        galaxy,
        polities,
        &data.balance.colonization,
        treasury,
        polity,
        day,
    ) {
        let value = colony_value(galaxy, galaxy.planet(id));
        if best.is_none_or(|(_, best_value)| value > best_value) {
            best = Some((id, value));
        }
    }
    best.map(|(id, _)| id)
}

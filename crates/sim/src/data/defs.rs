//! Formate der Datendateien.

use crate::ids::GoodId;
use serde::{Deserialize, Serialize};

/// Farbe als sRGB-Anteile 0..1.
pub type Rgb = (f32, f32, f32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum GoodTier {
    /// Rohstoff aus Vorkommen.
    Raw,
    /// Zwischenprodukt.
    Intermediate,
    /// Endgut für Bevölkerung und Wirtschaft.
    Final,
    /// Rüstungsgut: Munition, Schiffsteile, Waffen.
    Military,
}

/// `goods.ron`
#[derive(Clone, Debug, Deserialize)]
pub struct GoodDef {
    pub key: String,
    pub name: String,
    pub tier: GoodTier,
    /// Richtpreis, um den sich lokale Preise bilden.
    pub base_price: f64,
    /// Laderaum je Einheit.
    #[serde(default = "one_f32")]
    pub mass: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Atmosphere {
    #[default]
    None,
    Thin,
    Breathable,
    Dense,
    Toxic,
    Corrosive,
}

impl Atmosphere {
    pub fn label(self) -> &'static str {
        match self {
            Atmosphere::None => "keine",
            Atmosphere::Thin => "dünn",
            Atmosphere::Breathable => "atembar",
            Atmosphere::Dense => "dicht",
            Atmosphere::Toxic => "giftig",
            Atmosphere::Corrosive => "ätzend",
        }
    }

    /// Anteil an der Bewohnbarkeit.
    pub fn habitability_factor(self) -> f32 {
        match self {
            Atmosphere::Breathable => 1.0,
            Atmosphere::Thin => 0.55,
            Atmosphere::Dense => 0.5,
            Atmosphere::Toxic => 0.2,
            Atmosphere::None => 0.15,
            Atmosphere::Corrosive => 0.08,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct DepositDef {
    pub good: String,
    /// Wahrscheinlichkeit, dass das Vorkommen existiert.
    pub chance: f32,
    /// Ergiebigkeit (min, max); 1.0 ist ein gutes Vorkommen.
    pub richness: (f32, f32),
    #[serde(skip)]
    pub good_id: GoodId,
}

/// `planet_kinds.ron`
#[derive(Clone, Debug, Deserialize)]
pub struct PlanetKindDef {
    pub key: String,
    pub name: String,
    pub weight: f32,
    /// Taugt als Heimatwelt einer Gemeinschaft.
    #[serde(default)]
    pub home: bool,
    pub color: Rgb,
    /// Schwerkraft in g.
    pub gravity: (f32, f32),
    /// Mittlere Temperatur in °C.
    pub temperature: (f32, f32),
    /// Wasseranteil der Oberfläche 0..1.
    pub water: (f32, f32),
    pub atmospheres: Vec<(Atmosphere, f32)>,
    /// Tragfähigkeit in Millionen Einwohnern bei voller Bewohnbarkeit.
    pub capacity: (f32, f32),
    #[serde(default)]
    pub deposits: Vec<DepositDef>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct StarKindDef {
    pub name: String,
    pub weight: f32,
    pub color: Rgb,
    /// Relative Darstellungsgröße.
    pub radius: f32,
}

/// `galaxy.ron`
#[derive(Clone, Debug, Deserialize)]
pub struct GalaxyParams {
    pub clusters: u32,
    pub systems_per_cluster: (u32, u32),
    pub galaxy_radius: f32,
    pub cluster_radius: f32,
    /// Mindestabstand zwischen Clusterzentren.
    pub cluster_spacing: f32,
    /// Mindestabstand zwischen Systemen.
    pub system_spacing: f32,
    /// Anteil der zusätzlichen Routen im Cluster (0 = nur das Nötigste, 1 = dichtes Netz).
    pub lane_density: f32,
    /// Brücken über das Minimum hinaus. Weniger Brücken heißt härtere Engpässe.
    pub extra_bridges: u32,
    pub planets_per_system: (u32, u32),
    pub polities: u32,
    /// Startbevölkerung der Heimatwelt in Millionen.
    pub home_population: f64,
    pub stars: Vec<StarKindDef>,
}

/// `polities.ron`: Vorlagen, aus denen die Gemeinschaften einer Partie gezogen werden.
#[derive(Clone, Debug, Deserialize)]
pub struct PolityDef {
    pub key: String,
    pub name: String,
    /// Kurzform für Listen und Kartenbeschriftung.
    pub short: String,
    pub color: Rgb,
    pub motto: String,
}

/// `names.ron`
#[derive(Clone, Debug, Deserialize)]
pub struct NameData {
    pub prefixes: Vec<String>,
    #[serde(default)]
    pub middles: Vec<String>,
    pub suffixes: Vec<String>,
}

/// `balance.ron`: Stellschrauben, die zu keinem einzelnen Katalog gehören.
#[derive(Clone, Debug, Deserialize)]
pub struct Balance {
    pub colonization: ColonizationBalance,
    pub population: PopulationBalance,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ColonizationBalance {
    /// Siedler der ersten Welle in Millionen; sie kommen von der Hauptwelt.
    pub settlers: f64,
    /// Tage zwischen zwei Koloniegründungen derselben Gemeinschaft.
    pub cooldown_days: u64,
    /// Mindestbevölkerung der Hauptwelt, um Siedler abzugeben.
    pub min_capital_population: f64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PopulationBalance {
    /// Jährliche Wachstumsrate weit unter der Tragfähigkeit.
    pub growth_per_year: f64,
}

fn one_f32() -> f32 {
    1.0
}

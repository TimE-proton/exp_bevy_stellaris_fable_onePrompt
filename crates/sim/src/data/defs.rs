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
    /// Nachwachsender Rohstoff: Das Vorkommen erschöpft sich nicht.
    #[serde(default)]
    pub renewable: bool,
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

/// Wirtschaftszweig; je Planet und Zweig gibt es eine Firma.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Sector {
    Mining,
    Agrar,
    Industry,
    Arms,
}

impl Sector {
    pub const ALL: [Sector; 4] = [
        Sector::Mining,
        Sector::Agrar,
        Sector::Industry,
        Sector::Arms,
    ];

    pub const fn index(self) -> usize {
        self as usize
    }

    pub fn label(self) -> &'static str {
        match self {
            Sector::Mining => "Bergbau",
            Sector::Agrar => "Agrar",
            Sector::Industry => "Industrie",
            Sector::Arms => "Rüstung",
        }
    }
}

/// Woran der Ausstoß eines Rezepts lokal hängt.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub enum YieldSource {
    /// Überall gleich.
    None,
    /// Bewohnbarkeit der Welt.
    Soil,
    /// Ergiebigkeit und Restvorrat des Vorkommens.
    Deposit(String),
}

/// `recipes.ron`. Mengen je Einheit Kapitalstock und Tag bei voller Auslastung.
#[derive(Clone, Debug, Deserialize)]
pub struct RecipeDef {
    pub key: String,
    pub name: String,
    pub sector: Sector,
    pub source: YieldSource,
    pub output: (String, f32),
    #[serde(default)]
    pub inputs: Vec<(String, f32)>,
    /// Arbeitskräfte in Millionen.
    pub labor: f32,
    /// Treibstoffbedarf vor dem lokalen Energiefaktor.
    #[serde(default)]
    pub energy: f32,
    #[serde(skip)]
    pub output_id: GoodId,
    #[serde(skip)]
    pub input_ids: Vec<(GoodId, f32)>,
    /// Aufgelöstes Vorkommen bei `source: Deposit`.
    #[serde(skip)]
    pub deposit_id: Option<GoodId>,
}

/// `balance.ron`: Stellschrauben, die zu keinem einzelnen Katalog gehören.
#[derive(Clone, Debug, Deserialize)]
pub struct Balance {
    pub colonization: ColonizationBalance,
    pub population: PopulationBalance,
    pub economy: EconomyBalance,
    pub logistics: LogisticsBalance,
    pub fleets: FleetBalance,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ColonizationBalance {
    /// Siedler der ersten Welle in Millionen; sie kommen von der Hauptwelt.
    pub settlers: f64,
    /// Tage zwischen zwei Koloniegründungen derselben Gemeinschaft.
    pub cooldown_days: u64,
    /// Mindestbevölkerung der Hauptwelt, um Siedler abzugeben.
    pub min_capital_population: f64,
    /// Erstausstattung in Credits, vom Staat an die Kolonie.
    pub cost: f64,
    /// Kapitalstock, den die Siedler mitbringen.
    pub seed_capital: f32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PopulationBalance {
    pub growth_per_year: f64,
    pub starvation_per_year: f64,
    pub migration_per_month: f64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct NeedDef {
    pub good: String,
    /// Täglicher Bedarf je Million Einwohner.
    pub per_million: f64,
    /// Lebensnotwendig: wird zuerst gekauft, Mangel lässt die Bevölkerung schrumpfen.
    #[serde(default)]
    pub essential: bool,
    #[serde(skip)]
    pub good_id: GoodId,
}

#[derive(Clone, Debug, Deserialize)]
pub struct EconomyBalance {
    pub workforce_share: f64,
    pub base_wage: f64,
    pub needs: Vec<NeedDef>,
    pub fuel: String,
    /// Leistung ohne Treibstoff (Notbetrieb), 0..1.
    pub emergency_output: f64,
    pub capital_good: String,
    pub cover_days: f64,
    pub price_elasticity: f64,
    pub price_floor: f64,
    pub price_ceiling: f64,
    pub price_step: f64,
    pub spread: f64,
    pub lift_cost: f64,
    pub wage_floor: f64,
    pub wage_ceiling: f64,
    pub wage_step: f64,
    pub capital_cost: f64,
    pub depreciation_per_year: f64,
    pub scrap_per_year: f64,
    pub hurdle_per_year: f64,
    pub max_growth_per_month: f64,
    pub seed_investment: f64,
    /// Bis zu diesem Vielfachen des Arbeitskräfteangebots wird Kapital aufgebaut.
    pub overhire: f64,
    /// Jahre nach der Gründung, in denen verlustreiche Werke einer Kolonie nicht abgebaut werden.
    pub infant_years: u64,
    pub income_tax: f64,
    pub cash_per_million: f64,
    pub spend_rate: f64,
    pub depletion: f64,
    pub state_reserve_per_million: f64,
    pub market_reserve_days: f64,
    pub firm_reserve_days: f64,
    #[serde(skip)]
    pub fuel_id: GoodId,
    #[serde(skip)]
    pub capital_good_id: GoodId,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LogisticsBalance {
    pub freighter_capacity: f64,
    pub speed: f32,
    pub fuel_per_distance: f64,
    pub freighters_per_billion: f64,
    pub min_profit_per_day: f64,
    pub freighter_hull: f64,
    pub freighter_engines: f64,
    /// Heuer je Frachter und Fahrtag.
    pub upkeep_per_day: f64,
    pub raid_chance: f64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct FleetBalance {
    pub start_strength: f32,
    pub speed: f32,
    pub daily_damage: f32,
}

fn one_f32() -> f32 {
    1.0
}

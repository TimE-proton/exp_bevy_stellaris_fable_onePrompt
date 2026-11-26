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
    /// Technologien, die die Gemeinschaft von Beginn an beherrscht (Schlüssel aus
    /// `techs.ron`, samt allen Voraussetzungen).
    #[serde(default)]
    pub start_techs: Vec<String>,
    #[serde(skip)]
    pub start_tech_ids: Vec<u16>,
    /// Vorlieben der Forschung: Faktor je Zweig auf die Gewichte der automatischen
    /// Zielwahl. Ohne Angabe wiegen alle Zweige gleich.
    #[serde(default = "super::ships::BranchWeights::even")]
    pub research_focus: super::ships::BranchWeights,
    /// Anteil der laufenden Staatseinnahmen, den die Gemeinschaft für Forschung ansetzt.
    /// Ohne Angabe gilt `balance.ron: research.budget_share`.
    #[serde(default)]
    pub research_share: Option<f64>,
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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
    pub finance: FinanceBalance,
    pub military: super::ships::MilitaryBalance,
    pub battle: super::ships::BattleBalance,
    pub research: super::ships::ResearchBalance,
    pub siege: SiegeBalance,
}

#[derive(Clone, Debug, Deserialize)]
pub struct FinanceBalance {
    /// Anteil des Geldes einer Heimatwelt, den zu Beginn ihre Elite hält.
    pub elite_cash_share: f64,
    /// Kurs als Vielfaches des Jahresgewinns.
    pub earnings_multiple: f64,
    /// Untergrenze des Kurses als Anteil am Buchwert.
    pub book_floor: f64,
    /// Kursaufschlag, wenn alle Anteile auf einmal gekauft würden.
    pub price_impact: f64,
    /// Anteil eines Kursschocks, der je Monat abklingt.
    pub shock_recovery: f64,
    /// Monatliches Verblassen von Enteignungs- und Ausfallerinnerung.
    pub memory_decay: f64,
    pub expropriation_risk_foreign: f64,
    pub expropriation_risk_everyone: f64,
    pub expropriation_shock: f64,
    pub war_shock: f64,
    pub base_yield: f64,
    pub yield_per_debt: f64,
    pub yield_per_weariness: f64,
    pub yield_after_default: f64,
    /// Über diesem Zins zeichnet niemand mehr.
    pub max_yield: f64,
    pub bond_years: u64,
    pub default_haircut: f64,
    pub default_shock: f64,
    /// Monatliche Abgabe der Eliten an die Haushalte ihrer Welt.
    pub elite_spending: f64,
    /// Anteil ihrer Kasse, den Eliten bzw. Fonds je Monat anlegen.
    pub elite_invest_share: f64,
    pub public_invest_share: f64,
    /// Davon im Ausland.
    pub foreign_invest_share: f64,
    /// Anteil der Kasse der Hauptwelt-Elite, der je Monat Koloniebetriebe finanziert.
    pub colony_fund_share: f64,
    pub weariness_per_month: f64,
    pub weariness_recovery: f64,
    pub cohesion: CohesionBalance,
}

/// Gewichte des Zusammenhalts in Punkten (Skala 0..100) und die Schwellen der Stufen.
#[derive(Clone, Debug, Deserialize)]
pub struct CohesionBalance {
    pub base: f64,
    pub interlock: f64,
    pub dispersion: f64,
    pub center: f64,
    pub drain: f64,
    /// Nettoabfluss (Anteil am Einkommen vor Ort), ab dem der volle Abzug gilt.
    pub drain_scale: f64,
    pub foreign: f64,
    pub supply: f64,
    /// Punkte je Punkt Kriegsmüdigkeit.
    pub weariness: f64,
    /// Punkte je 100 % Steuersatz über dem neutralen Satz.
    pub tax: f64,
    pub tax_neutral: f64,
    pub blockade: f64,
    pub capital: f64,
    /// Punkte je Sprung Entfernung zur Hauptwelt, gedeckelt.
    pub distance: f64,
    pub distance_cap: f64,
    /// Bonus nach Gründung oder Besitzwechsel, der über die Jahre verblasst.
    pub honeymoon: f64,
    pub honeymoon_years: f64,
    pub expropriation: f64,
    /// Anteil der Lücke zum Zielwert, der je Monat geschlossen wird.
    pub adjust: f64,
    pub restless_below: f64,
    pub loyal_above: f64,
    pub autonomy_below: f64,
    pub autonomy_months: u32,
    pub recover_above: f64,
    pub recover_months: u32,
    pub secession_below: f64,
    pub secession_months: u32,
    /// Fremder Besitzanteil, ab dem eine autonome Region zum Eigner übertritt.
    pub foreign_pull: f64,
    /// Mehr Gemeinschaften als diese gibt es nie; darüber bleibt eine Region autonom.
    pub max_polities: u32,
    /// Mindestbevölkerung (Mio.), damit eine Region einen eigenen Staat ausrufen kann.
    pub independence_min_population: f64,
    /// Abzug bei voller Besatzung (`Region::occupation` = 1) ohne stationierte Truppen.
    pub occupation: f64,
    /// Anteil dieses Abzugs, den eine volle Garnison (`Region::garrison_cover` = 1) auffängt.
    pub occupation_hold: f64,
    /// Abzug bei vollen Kriegsschäden (`Region::war_damage` = 1).
    pub war_damage: f64,
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
    /// Preisniveau am Heimatmarkt (1 = Richtpreise), unter dem die Notenbank Geld schöpft.
    pub price_target: f64,
    /// Höchste Geldschöpfung je Monat als Anteil am Geld der Haushalte.
    pub mint_per_month: f64,
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
}

/// Technologien, die Ausbaustufen und Kampfweisen der Belagerung freischalten
/// (Schlüssel aus `techs.ron`).
#[derive(Clone, Debug, Deserialize)]
pub struct SiegeGates {
    /// Bunker über die erste Stufe hinaus.
    pub bunker: String,
    /// Volle Feuerunterstützung aus dem Orbit.
    pub support: String,
    /// Vorteil im Untergrundkampf.
    pub tunnel: String,
    /// Schilde über die erste Stufe hinaus.
    pub shield: String,
}

/// Stehende Verteidigung, mit der Heimatwelten beginnen.
#[derive(Clone, Debug, Deserialize)]
pub struct HomeStart {
    pub guns: u8,
    /// Reguläre Verbände.
    pub regulars: f64,
    pub depot_days: u16,
}

/// `balance.ron: siege`: der Kampf um einen Planeten zwischen einer Flotte im Orbit und
/// der Verteidigung am Boden.
///
/// Einheiten: Ein Verband (V) steht für `pop_per_strength` Mio. Menschen. Feuerkraft ist
/// die Summe der Waffenleistung der Schiffe im Orbit; die Vorräte der Garnison zählen in
/// Tagen, der Nachschub der Landungstruppe in Verbandstagen.
#[derive(Clone, Debug, Deserialize)]
pub struct SiegeBalance {
    // --- Orbit ---
    /// Feuerkraft-Tage, die eine Stationsstufe aushält.
    pub station_hp: f64,
    /// Rumpfschaden je Stationsstufe und Tag an den Schiffen im Orbit.
    pub station_damage: f64,
    /// Rumpfschaden je Geschützstufe und Tag an Schiffen im niedrigen Orbit.
    pub gun_damage: f64,
    /// Vorratstage, die eine feuernde Geschützstufe je Tag verbraucht.
    pub gun_depot: f64,
    /// Geschützstufen, die ein Punkt Beschuss je Tag ausschaltet (gezielt das Doppelte).
    pub counter_battery: f64,
    /// Gewicht je Schiffsklasse (Jäger bis Träger), mit dem Stationen und Geschütze
    /// treffen: Große Schiffe verlieren den größeren Anteil ihres Rumpfs.
    pub gun_weight: [f64; 6],
    /// Anteil der Magazine je Tag: Duell mit Stationen, Stützfeuer, gezielter Beschuss,
    /// Flächenbeschuss.
    pub ammo: [f64; 4],
    /// Anteil des Tanks, den eine Flotte je Tag im Orbit verbraucht.
    pub station_fuel: f64,
    /// Verbände je Rumpfsegment, die ein Schiff tragen kann.
    pub berth_per_segment: f64,
    /// Anteil einer Landungswelle, den aktive Geschütze je Stufe abfangen, und Obergrenze.
    pub landing_loss: f64,
    pub landing_loss_cap: f64,

    // --- Schild und Beschuss ---
    /// Ladung je Schildstufe und was davon je Tag nachlädt.
    pub shield_cap: f64,
    pub shield_regen: f64,
    /// Vorratstage, die eine Schildstufe verbraucht, wenn sie einen Tag voll nachlädt.
    pub shield_depot: f64,
    /// Wirkung des Beschusses: Rate = min(bombard_cap, Beschuss × bombard_k /
    /// (Bevölkerung + bombard_pop0)). Große Welten verteilen das Feuer.
    pub bombard_k: f64,
    pub bombard_pop0: f64,
    pub bombard_cap: f64,
    /// (militärische Wirkung, Begleitschaden) als Vielfaches der Rate.
    pub precise: (f64, f64),
    pub area: (f64, f64),
    /// Unter diesen Anteil der Bevölkerung bei Beginn drückt Beschuss eine Welt nicht.
    pub pop_floor: f64,

    // --- Boden und Untergrund ---
    /// Anteil der wirksamen Stärke, den eine Seite der anderen je Tag abnimmt.
    pub r: f64,
    /// Faktor auf die Stärke der Landungstruppe in den ersten `beach_days` Tagen.
    pub beach: f64,
    pub beach_days: u64,
    /// Faktor auf die Stärke einer Seite ohne Nachschub bzw. ohne Vorräte.
    pub unsupplied: f64,
    /// Eine Zone fällt, wenn ihre Verteidiger unter diesen Anteil der Landungstruppe
    /// sinken; die Oberfläche bricht, wenn sie unter diesen Anteil ihres Anfangs sinken;
    /// die Landung ist abgeschlagen, wenn die Truppe unter diesen Anteil ihres Höchststands
    /// fällt.
    pub zone_fall: f64,
    /// Befestigungsfaktor je Zone: Umland, Stadt, Zentrum, Batterie, Schildwerk.
    pub fort: [f64; 5],
    /// Bevölkerung (Mio.), ab der eine Welt eine und ab der sie zwei Stadtzonen hat.
    pub city_from: (f64, f64),
    /// Anteil des Zentrums und jedes Werks (Batterie, Schildwerk) an Bevölkerung und
    /// Wirtschaft; den Rest teilen sich Umland und Städte.
    pub center_share: f64,
    pub works_share: f64,
    /// Faktor auf die Miliz der übrigen Zonen, wenn das Zentrum fällt.
    pub center_shock: f64,
    /// Größter Aufschlag durch Stützfeuer (ohne, mit Technologie) und Beschuss je Verband
    /// Verteidiger, ab dem er voll gilt.
    pub support_max: (f64, f64),
    pub support_ref: f64,
    /// Ausfall: Verteidiger greifen eine haltende Truppe an, wenn sie ihr ohne
    /// Befestigung um diesen Faktor überlegen sind.
    pub sortie_ratio: f64,
    /// Anteil von Bevölkerung und Kapital einer Zone, den jeder verlorene Verband kostet.
    pub damage_per_loss: f64,
    /// Verbände je Seite, die im Untergrund zugleich kämpfen können.
    pub tunnel_front: f64,
    /// Befestigungsfaktor im Untergrund: Sockel und Aufschlag je Bunkerstufe.
    pub under_base: f64,
    pub under_per_bunker: f64,
    /// Faktor im Untergrund für die Seite, die die Tunneltechnologie beherrscht.
    pub tunnel_tech: f64,
    /// Reguläre Verbände, die eine Bunkerstufe aufnimmt, und Vorratstage, die sie birgt.
    pub deep_per_bunker: f64,
    pub bunker_days: f64,

    // --- Vorräte und Ende ---
    /// Anteil ihres Bedarfs, den eine eingeschlossene Garnison am Markt vor Ort deckt.
    pub forage_share: f64,
    /// Anteil der Verteidiger, der sich je Tag ohne Vorräte auflöst (geteilt durch den
    /// Durchhaltewillen), und Anteil der Landungstruppe ohne Nachschub.
    pub starve_rate: f64,
    pub strand_rate: f64,
    /// Anteil der Garnison bei Beginn, unter dem eine ausgehungerte Welt aufgibt.
    pub capitulate_frac: f64,
    /// Verbrauch einer Landungstruppe an Tagen ohne Gefecht, als Anteil des Kampftags.
    pub army_idle_supply: f64,
    /// Größtes Depot, das sich planen lässt, in Tagen.
    pub depot_days_cap: u16,
    /// Obergrenze für die Dauer einer Einschließung mit Besatzungstruppe; beim Laden
    /// gegen Depot, Marktanteil und Auflösungsrate geprüft.
    pub max_siege_days: u32,
    /// Güter je Verband und Tag, aus denen das Depot besteht.
    pub day_bundle: Vec<(String, f64)>,

    // --- Truppen und Anlagen ---
    /// Miliz = militia_k × √Bevölkerung × Faktor aus dem Zusammenhalt.
    pub militia_k: f64,
    /// Zusammenhalt, bei dem dieser Faktor 1 ist, und seine Schranken.
    pub militia_cohesion: (f64, f64, f64),
    /// Millionen Menschen je Verband.
    pub pop_per_strength: f64,
    /// Güter je ausgehobenem regulären Verband.
    pub regular_bundle: Vec<(String, f64)>,
    /// Aushebung je Monat als Anteil der Miliz und Obergrenze der Regulären als
    /// Vielfaches der Miliz.
    pub recruit_per_month: f64,
    pub regulars_cap: f64,
    /// Besatzungstruppe, die eine Übergabe annimmt, und Garnison, die eine besetzte Welt
    /// ruhig hält: jeweils mal √Bevölkerung.
    pub occupy_k: f64,
    pub hold_k: f64,
    /// Sold je regulärem Verband und Unterhalt je Ausbaustufe, in Credits je Monat.
    pub pay: f64,
    pub level_upkeep: f64,
    /// Güter je Stufe (Stufe n kostet das n-Fache) und höchste Stufe.
    pub station: Vec<(String, f64)>,
    pub station_max: u8,
    pub gun: Vec<(String, f64)>,
    pub gun_max: u8,
    /// Höchste Stufe ohne und mit der Technologie aus `gates`.
    pub shield: Vec<(String, f64)>,
    pub shield_max: (u8, u8),
    pub bunker: Vec<(String, f64)>,
    pub bunker_max: (u8, u8),
    pub gates: SiegeGates,
    /// Anteil des Marktbestands, den der Staat je Kauf höchstens nimmt, und Anteil der
    /// freien Staatskasse, der je Monat für Rüstung bereitsteht (Frieden, Krieg).
    pub buy_share: f64,
    pub budget_share: (f64, f64),
    pub home_start: HomeStart,

    // --- Wille, Eroberung, Rückwirkung ---
    /// Durchhaltewille = (will_base + (1 − will_base) × Zusammenhalt/100)
    /// × (1 − will_stake × Anteil des Belagerers an den Betrieben).
    pub will_base: f64,
    pub will_stake: f64,
    /// Zusammenhalt nach der Eroberung: Sockel, Punkte je vollem Vorbesitz, Abzug je
    /// vollem Kriegsschaden; Mindestwert nach einer Befreiung.
    pub conquest_base: f64,
    pub stake_pts: f64,
    pub scar_pts: f64,
    pub liberation: f64,
    /// Faktor auf die Besatzung nach einer Übergabe.
    pub capitulation_mercy: f64,
    /// Monatliches Verblassen von Besatzung und Kriegsschäden.
    pub occupation_decay: f64,
    pub scar_decay: f64,
    /// Plünderung: Anteil der Kassen, zusätzlicher Kriegsschaden, Faktor auf die Kurse.
    pub sack_share: f64,
    pub sack_scar: f64,
    pub sack_shock: f64,
    /// Faktor auf die Kurse der Betriebe einer eroberten Welt.
    pub conquest_shock: f64,
    /// Anteil der Bestände und der Kasse der Elite einer gefallenen Hauptwelt, der mit
    /// ihr in die neue Hauptwelt zieht.
    pub elite_flight: f64,
    /// Stillstand der Wirtschaft: Anteil einer umkämpften bzw. besetzten Zone, und wie
    /// schnell er je Tag abklingt.
    pub disrupt: (f64, f64),
    pub disrupt_decay: f64,
    /// Wert der Beute je Mio. Einwohner, neben dem Buchwert der Betriebe.
    pub prize_per_million: f64,
    /// Kriegsmüdigkeit: je eigenem verlorenen Verband; je Mio. Einwohner, die Beschuss
    /// tötet (Belagerer, Verteidiger); Faktor je vollem Anteil des Belagerers am
    /// Beschossenen; je Belagerungsmonat (Belagerer, Verteidiger); bei der Eroberung
    /// (Eroberer, Verlierer).
    pub wear_loss: f64,
    pub wear_fire: (f64, f64),
    pub wear_portfolio: f64,
    pub wear_month: (f64, f64),
    pub wear_planet: (f64, f64),
    /// Unschärfe der Schätzungen über fremde Garnisonen ohne jeden Einblick.
    pub intel_spread: f64,
    /// Verlustanteil der Landungswelle, bis zu dem ein Sturm in der Prognose als tragbar gilt.
    pub storm_loss_limit: f64,

    #[serde(skip)]
    pub day_bundle_ids: Vec<(GoodId, f64)>,
    #[serde(skip)]
    pub regular_bundle_ids: Vec<(GoodId, f64)>,
    #[serde(skip)]
    pub station_ids: Vec<(GoodId, f64)>,
    #[serde(skip)]
    pub gun_ids: Vec<(GoodId, f64)>,
    #[serde(skip)]
    pub shield_ids: Vec<(GoodId, f64)>,
    #[serde(skip)]
    pub bunker_ids: Vec<(GoodId, f64)>,
    /// Aufgelöste Technologien in der Reihenfolge Bunker, Stützfeuer, Tunnel, Schild.
    #[serde(skip)]
    pub gate_ids: [u16; 4],
}

fn one_f32() -> f32 {
    1.0
}

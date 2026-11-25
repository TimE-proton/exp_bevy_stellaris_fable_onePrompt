//! Spieldaten aus `assets/data/*.ron`.
//!
//! Alles, was Balance ist, steht in Datendateien und wird zur Laufzeit geladen. Der Kern
//! löst Schlüssel (`"erz"`) beim Laden in Indizes auf und prüft die Daten, damit Fehler
//! mit Dateiname und Ursache gemeldet werden statt irgendwo in der Simulation.

mod defs;
mod ship_loader;
mod ships;

pub use defs::*;
pub use ship_loader::arc_covers;
pub use ships::*;

use crate::hash::Fnv64;
use crate::ids::GoodId;
use bevy_ecs::prelude::Resource;
use serde::de::DeserializeOwned;
use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

/// Umgebungsvariable, die das Datenverzeichnis überschreibt.
pub const DATA_DIR_ENV: &str = "KONFLUENZ_DATA";

#[derive(Debug)]
pub enum DataError {
    NotFound(Vec<PathBuf>),
    Io {
        file: PathBuf,
        error: std::io::Error,
    },
    Parse {
        file: PathBuf,
        error: String,
    },
    Invalid {
        file: &'static str,
        message: String,
    },
}

impl fmt::Display for DataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DataError::NotFound(tried) => {
                write!(f, "Datenverzeichnis nicht gefunden. Versucht:")?;
                for path in tried {
                    write!(f, " {}", path.display())?;
                }
                write!(f, ". Mit --data-dir oder {DATA_DIR_ENV} angeben.")
            }
            DataError::Io { file, error } => write!(f, "{}: {error}", file.display()),
            DataError::Parse { file, error } => write!(f, "{}: {error}", file.display()),
            DataError::Invalid { file, message } => write!(f, "{file}: {message}"),
        }
    }
}

impl std::error::Error for DataError {}

/// Alle geladenen Spieldaten. Unveränderlich während einer Partie.
#[derive(Resource, Clone, Debug)]
pub struct GameData {
    pub goods: Vec<GoodDef>,
    pub planet_kinds: Vec<PlanetKindDef>,
    pub recipes: Vec<RecipeDef>,
    pub hulls: Vec<HullDef>,
    pub modules: Vec<ModuleDef>,
    pub designs: Vec<DesignDef>,
    pub techs: Vec<TechDef>,
    pub galaxy: GalaxyParams,
    pub polities: Vec<PolityDef>,
    pub names: NameData,
    pub balance: Balance,
    /// Prüfsumme über die Rohdateien; steht im Spielstand und im Befehlsprotokoll.
    pub hash: u64,
    /// Verzeichnis, aus dem geladen wurde (für das Neuladen im Spiel).
    pub dir: PathBuf,
}

impl GameData {
    /// Sucht das Datenverzeichnis: ausdrückliche Angabe, Umgebungsvariable, Arbeits-
    /// verzeichnis, neben dem Programm, zuletzt der Workspace, aus dem gebaut wurde.
    pub fn locate(explicit: Option<&Path>) -> Result<PathBuf, DataError> {
        let mut candidates = Vec::new();
        if let Some(path) = explicit {
            candidates.push(path.to_path_buf());
        } else {
            if let Some(path) = std::env::var_os(DATA_DIR_ENV) {
                candidates.push(PathBuf::from(path));
            }
            candidates.push(PathBuf::from("assets/data"));
            if let Ok(exe) = std::env::current_exe()
                && let Some(dir) = exe.parent()
            {
                candidates.push(dir.join("assets/data"));
            }
            candidates.push(workspace_data_dir());
        }
        for candidate in &candidates {
            if candidate.join("goods.ron").is_file() {
                return Ok(candidate.clone());
            }
        }
        Err(DataError::NotFound(candidates))
    }

    pub fn load_located(explicit: Option<&Path>) -> Result<Self, DataError> {
        Self::load(&Self::locate(explicit)?)
    }

    pub fn load(dir: &Path) -> Result<Self, DataError> {
        let mut hasher = Fnv64::new();
        let goods: Vec<GoodDef> = load_ron(dir, "goods.ron", &mut hasher)?;
        let planet_kinds: Vec<PlanetKindDef> = load_ron(dir, "planet_kinds.ron", &mut hasher)?;
        let recipes: Vec<RecipeDef> = load_ron(dir, "recipes.ron", &mut hasher)?;
        let hulls: Vec<HullDef> = load_ron(dir, "hulls.ron", &mut hasher)?;
        let modules: Vec<ModuleDef> = load_ron(dir, "modules.ron", &mut hasher)?;
        let designs: Vec<DesignDef> = load_ron(dir, "designs.ron", &mut hasher)?;
        let techs: Vec<TechDef> = load_ron(dir, "techs.ron", &mut hasher)?;
        let galaxy: GalaxyParams = load_ron(dir, "galaxy.ron", &mut hasher)?;
        let polities: Vec<PolityDef> = load_ron(dir, "polities.ron", &mut hasher)?;
        let names: NameData = load_ron(dir, "names.ron", &mut hasher)?;
        let balance: Balance = load_ron(dir, "balance.ron", &mut hasher)?;
        let mut data = Self {
            goods,
            planet_kinds,
            recipes,
            hulls,
            modules,
            designs,
            techs,
            galaxy,
            polities,
            names,
            balance,
            hash: hasher.finish(),
            dir: dir.to_path_buf(),
        };
        data.resolve_and_validate()?;
        ship_loader::resolve_ships(&mut data)?;
        Ok(data)
    }

    /// Daten des Workspace, aus dem gebaut wurde. Für Tests und Werkzeuge.
    pub fn load_workspace() -> Result<Self, DataError> {
        Self::load(&workspace_data_dir())
    }

    pub fn good_id(&self, key: &str) -> Option<GoodId> {
        self.goods
            .iter()
            .position(|good| good.key == key)
            .map(GoodId::from_index)
    }

    pub fn good(&self, id: GoodId) -> &GoodDef {
        &self.goods[id.index()]
    }

    pub fn recipe_index(&self, key: &str) -> Option<usize> {
        self.recipes.iter().position(|recipe| recipe.key == key)
    }

    pub fn design_index(&self, key: &str) -> Option<u16> {
        self.designs
            .iter()
            .position(|design| design.key == key)
            .map(|index| index as u16)
    }

    pub fn tech_index(&self, key: &str) -> Option<u16> {
        self.techs
            .iter()
            .position(|tech| tech.key == key)
            .map(|index| index as u16)
    }

    /// Der wertvollste Entwurf einer Klasse, den eine Gemeinschaft mit den erforschten
    /// Technologien (`known[tech]`) bauen kann.
    pub fn best_design(&self, class: ShipClass, known: &[bool]) -> Option<u16> {
        self.designs
            .iter()
            .enumerate()
            .filter(|(_, design)| design.stats.class == class)
            .filter(|(_, design)| {
                design
                    .stats
                    .techs
                    .iter()
                    .all(|tech| known.get(*tech as usize).copied().unwrap_or(false))
            })
            .max_by(|a, b| {
                a.1.stats
                    .value
                    .total_cmp(&b.1.stats.value)
                    .then(b.0.cmp(&a.0))
            })
            .map(|(index, _)| index as u16)
    }

    pub fn raw_goods(&self) -> impl Iterator<Item = GoodId> + '_ {
        self.goods
            .iter()
            .enumerate()
            .filter(|(_, good)| good.tier == GoodTier::Raw)
            .map(|(index, _)| GoodId::from_index(index))
    }

    fn resolve_and_validate(&mut self) -> Result<(), DataError> {
        let invalid = |file: &'static str, message: String| DataError::Invalid { file, message };

        // Güter
        if self.goods.is_empty() {
            return Err(invalid("goods.ron", "keine Güter definiert".into()));
        }
        let mut keys = BTreeSet::new();
        for good in &self.goods {
            if !keys.insert(good.key.as_str()) {
                return Err(invalid(
                    "goods.ron",
                    format!("Schlüssel `{}` doppelt", good.key),
                ));
            }
            if good.base_price.is_nan() || good.base_price <= 0.0 {
                return Err(invalid(
                    "goods.ron",
                    format!("`{}`: base_price muss positiv sein", good.key),
                ));
            }
        }

        // Planetentypen
        if !self.planet_kinds.iter().any(|kind| kind.home) {
            return Err(invalid(
                "planet_kinds.ron",
                "mindestens ein Typ braucht `home: true`".into(),
            ));
        }
        let good_keys: Vec<String> = self.goods.iter().map(|g| g.key.clone()).collect();
        let good_tiers: Vec<GoodTier> = self.goods.iter().map(|g| g.tier).collect();
        let mut kind_keys = BTreeSet::new();
        for kind in &mut self.planet_kinds {
            let file = "planet_kinds.ron";
            if !kind_keys.insert(kind.key.clone()) {
                return Err(invalid(file, format!("Schlüssel `{}` doppelt", kind.key)));
            }
            if kind.weight < 0.0 || kind.atmospheres.is_empty() {
                return Err(invalid(
                    file,
                    format!("`{}`: Gewicht negativ oder keine Atmosphäre", kind.key),
                ));
            }
            for (name, range) in [
                ("gravity", kind.gravity),
                ("temperature", kind.temperature),
                ("water", kind.water),
                ("capacity", kind.capacity),
            ] {
                if range.0 > range.1 {
                    return Err(invalid(
                        file,
                        format!("`{}`: Bereich `{name}` ist verkehrt herum", kind.key),
                    ));
                }
            }
            for deposit in &mut kind.deposits {
                let Some(index) = good_keys.iter().position(|key| *key == deposit.good) else {
                    return Err(invalid(
                        file,
                        format!("`{}`: unbekanntes Gut `{}`", kind.key, deposit.good),
                    ));
                };
                if good_tiers[index] != GoodTier::Raw {
                    return Err(invalid(
                        file,
                        format!("`{}`: `{}` ist kein Rohstoff", kind.key, deposit.good),
                    ));
                }
                if deposit.richness.0 > deposit.richness.1 || !(0.0..=1.0).contains(&deposit.chance)
                {
                    return Err(invalid(
                        file,
                        format!("`{}`: Vorkommen `{}` ungültig", kind.key, deposit.good),
                    ));
                }
                deposit.good_id = GoodId::from_index(index);
            }
        }

        // Rezepte
        let resolve = |file: &'static str, owner: &str, key: &str| {
            good_keys
                .iter()
                .position(|candidate| candidate == key)
                .map(GoodId::from_index)
                .ok_or_else(|| invalid(file, format!("`{owner}`: unbekanntes Gut `{key}`")))
        };
        let mut recipe_keys = BTreeSet::new();
        for recipe in &mut self.recipes {
            let file = "recipes.ron";
            if !recipe_keys.insert(recipe.key.clone()) {
                return Err(invalid(file, format!("Schlüssel `{}` doppelt", recipe.key)));
            }
            recipe.output_id = resolve(file, &recipe.key, &recipe.output.0)?;
            recipe.input_ids.clear();
            for (key, amount) in &recipe.inputs {
                if *amount <= 0.0 {
                    return Err(invalid(
                        file,
                        format!("`{}`: Menge von `{key}` nicht positiv", recipe.key),
                    ));
                }
                recipe
                    .input_ids
                    .push((resolve(file, &recipe.key, key)?, *amount));
            }
            recipe.deposit_id = match &recipe.source {
                YieldSource::Deposit(key) => Some(resolve(file, &recipe.key, key)?),
                _ => None,
            };
            if recipe.output.1 <= 0.0 || recipe.labor <= 0.0 || recipe.energy < 0.0 {
                return Err(invalid(
                    file,
                    format!("`{}`: Ausstoß und Arbeit müssen positiv sein", recipe.key),
                ));
            }
        }
        // Jeder Rohstoff braucht eine Förderung, jedes andere Gut einen Hersteller.
        for (index, key) in good_keys.iter().enumerate() {
            let id = GoodId::from_index(index);
            if !self.recipes.iter().any(|recipe| recipe.output_id == id) {
                return Err(invalid(
                    "recipes.ron",
                    format!("kein Rezept stellt `{key}` her"),
                ));
            }
        }

        // Wirtschaftsbalance
        let economy = &mut self.balance.economy;
        economy.fuel_id = resolve("balance.ron", "economy.fuel", &economy.fuel)?;
        economy.capital_good_id =
            resolve("balance.ron", "economy.capital_good", &economy.capital_good)?;
        for need in &mut economy.needs {
            need.good_id = resolve("balance.ron", "economy.needs", &need.good)?;
        }
        if !economy.needs.iter().any(|need| need.essential) {
            return Err(invalid(
                "balance.ron",
                "mindestens ein Bedarf braucht `essential: true`".into(),
            ));
        }
        if economy.price_floor <= 0.0
            || economy.price_floor >= 1.0
            || economy.price_ceiling <= 1.0
            || economy.cover_days <= 0.0
        {
            return Err(invalid(
                "balance.ron",
                "Preisschranken oder cover_days unplausibel".into(),
            ));
        }

        // Galaxie
        let g = &self.galaxy;
        let file = "galaxy.ron";
        if g.clusters < 2
            || g.systems_per_cluster.0 < 2
            || g.systems_per_cluster.0 > g.systems_per_cluster.1
        {
            return Err(invalid(
                file,
                "zu wenige Cluster oder Systeme je Cluster".into(),
            ));
        }
        if g.polities < 2 || g.polities > g.clusters {
            return Err(invalid(
                file,
                format!(
                    "polities ({}) muss zwischen 2 und clusters ({}) liegen",
                    g.polities, g.clusters
                ),
            ));
        }
        if g.planets_per_system.0 < 1 || g.planets_per_system.0 > g.planets_per_system.1 {
            return Err(invalid(file, "planets_per_system ungültig".into()));
        }
        if g.stars.is_empty() {
            return Err(invalid(file, "keine Sterntypen".into()));
        }
        if !(0.0..=1.0).contains(&g.lane_density) {
            return Err(invalid(file, "lane_density muss in 0..1 liegen".into()));
        }

        // Gemeinschaften
        if self.polities.len() < g.polities as usize {
            return Err(invalid(
                "polities.ron",
                format!(
                    "{} Vorlagen, aber galaxy.ron verlangt {} Gemeinschaften",
                    self.polities.len(),
                    g.polities
                ),
            ));
        }

        // Namen
        if self.names.prefixes.is_empty() || self.names.suffixes.is_empty() {
            return Err(invalid(
                "names.ron",
                "prefixes und suffixes dürfen nicht leer sein".into(),
            ));
        }
        Ok(())
    }
}

/// `assets/data` des Workspace, aus dem dieses Crate gebaut wurde.
pub fn workspace_data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/data")
}

fn load_ron<T: DeserializeOwned>(
    dir: &Path,
    name: &str,
    hasher: &mut Fnv64,
) -> Result<T, DataError> {
    let file = dir.join(name);
    let text = std::fs::read_to_string(&file).map_err(|error| DataError::Io {
        file: file.clone(),
        error,
    })?;
    hasher.write(name.as_bytes());
    hasher.write(text.as_bytes());
    ron::from_str(&text).map_err(|error| DataError::Parse {
        file,
        error: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_data_loads_and_validates() {
        let data = GameData::load_workspace().expect("Spieldaten müssen ladbar sein");
        assert!(data.raw_goods().count() >= 4);
        assert!(data.good_id("erz").is_some());
        assert!(data.planet_kinds.iter().any(|kind| kind.home));
        assert!(data.recipes.len() >= 12);
        let hut = &data.recipes[data.recipe_index("huette").unwrap()];
        assert_eq!(hut.output_id, data.good_id("legierungen").unwrap());
        assert_eq!(hut.input_ids[0].0, data.good_id("erz").unwrap());
        assert_eq!(data.techs.len(), 30);
        assert_ne!(data.hash, 0);
    }

    #[test]
    fn missing_directory_is_reported() {
        let error = GameData::locate(Some(Path::new("/nicht/vorhanden"))).unwrap_err();
        assert!(error.to_string().contains("nicht gefunden"));
    }

    #[test]
    fn every_class_has_a_buildable_design_with_sane_stats() {
        let data = GameData::load_workspace().unwrap();
        let none = vec![false; data.techs.len()];
        let all = vec![true; data.techs.len()];
        for class in ShipClass::ALL {
            let basic = data.best_design(class, &none).expect("Grundentwurf");
            let stats = &data.designs[basic as usize].stats;
            assert!(
                stats.value > 0.0 && stats.speed > 0.0 && stats.hull > 0.0,
                "{class:?}"
            );
            assert!(stats.power_output >= stats.power_draw, "{class:?}");
            assert!(stats.main_range > 0.0, "{class:?}");
            // Mit mehr Forschung wird nie ein schlechterer Entwurf gewählt.
            let advanced = data.best_design(class, &all).unwrap();
            assert!(data.designs[advanced as usize].stats.value >= stats.value);
        }
        // Größere Klassen kosten mehr und sind träger.
        let value = |key: &str| {
            data.designs[data.design_index(key).unwrap() as usize]
                .stats
                .clone()
        };
        assert!(value("bastion").value > value("speer").value);
        assert!(value("speer").value > value("waechter").value);
        assert!(value("waechter").value > value("dolch").value);
        assert!(value("bastion").speed < value("dolch").speed);
        // Das Heck ist die dünnste Stelle.
        let armor = value("bastion").armor;
        assert!(armor[2] < armor[1] && armor[1] < armor[0]);
        // Schlachtschiffe und Kreuzer gewinnen durch die Breitseite Feuerkraft.
        assert!(value("bastion").broadside > 1.05);
        assert!(value("speer").broadside > 1.05);
        // Ein Träger führt Jäger mit und rechnet ihren Preis ein.
        let carrier = value("horst");
        assert_eq!(carrier.fighters, 24);
        assert!(carrier.craft.is_some());
    }

    #[test]
    fn firing_arcs_cover_the_right_bearings() {
        assert!(arc_covers(0.0, 90.0, 40.0));
        assert!(!arc_covers(0.0, 90.0, 50.0));
        assert!(arc_covers(90.0, 150.0, 20.0));
        assert!(!arc_covers(90.0, 150.0, 0.0));
        assert!(arc_covers(180.0, 240.0, -100.0));
        assert!(
            arc_covers(-120.0, 220.0, 170.0),
            "über die 180-Grad-Naht hinweg"
        );
        assert!(arc_covers(33.0, 360.0, -147.0));
    }
}

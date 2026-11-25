//! Technologie und Forschung.
//!
//! Jede Gemeinschaft gibt monatlich einen Teil ihrer Staatseinnahmen für Forschung aus,
//! soweit ihre Kasse über der Rücklage liegt. Das Geld ist das Gehalt der Forschenden und
//! fließt an die Haushalte; aus ihm werden Forschungspunkte, mehr davon bei großer
//! Bevölkerung. Was Gemeinschaften in der Nähe schon beherrschen, ist billiger zu
//! erforschen: Wer zentral liegt, lernt schneller.
//!
//! Den genauen Stand kennt nur die Gemeinschaft selbst. Andere sehen je Zweig eine Spanne
//! ([`Research::estimate`]), die mit ihrem Einblick enger wird. Einblick entsteht aus
//! Besitz an den Betrieben der anderen und aus Ereignissen wie beobachteten Gefechten.
//!
//! Wirkungen: Die Aufschläge aller erforschten Technologien stehen gesammelt in
//! [`TechEffects`]. Ertrag und Frachterfahrt greifen in Wirtschaft und Logistik ein
//! (über `Economy::tech_yield` und `Logistics::tech_speed`); alles Übrige lesen Flotten,
//! Gefechte und Bodenkampf über [`Research::effects`].

mod intel;
mod plugin;
mod step;

pub use intel::{estimate_tier, intel_from_ties, ownership_ties, update_intel};
pub use plugin::TechPlugin;
pub use step::{
    choose_target, contact_weights, research_month, research_points, set_budget, set_target,
    spillover_discount,
};

use crate::data::{GameData, TechBranch, TechEffect};
use crate::economy::Money;
use crate::ids::PolityId;
use crate::polity::Polities;
use crate::rng::{Domain, Rng};
use bevy_ecs::prelude::Resource;
use serde::{Deserialize, Serialize};

/// Gesammelte Wirkung aller erforschten Technologien als Faktoren; 1,0 heißt keine Wirkung.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TechEffects {
    /// Ausstoß aller Werke.
    pub yield_factor: f64,
    /// Fahrt der Frachter.
    pub freighter_speed: f64,
    /// Fahrt der Flotten auf der Karte.
    pub fleet_speed: f64,
    /// Fahrt der Schiffe im Gefecht.
    pub ship_speed: f64,
    pub weapon_damage: f64,
    pub weapon_range: f64,
    pub shields: f64,
    pub armor: f64,
    pub hull: f64,
    pub ground_attack: f64,
    pub ground_defense: f64,
    /// Forschung selbst.
    pub research: f64,
}

impl Default for TechEffects {
    fn default() -> Self {
        Self {
            yield_factor: 1.0,
            freighter_speed: 1.0,
            fleet_speed: 1.0,
            ship_speed: 1.0,
            weapon_damage: 1.0,
            weapon_range: 1.0,
            shields: 1.0,
            armor: 1.0,
            hull: 1.0,
            ground_attack: 1.0,
            ground_defense: 1.0,
            research: 1.0,
        }
    }
}

impl TechEffects {
    /// Wirkung der erforschten Technologien (`known[tech]`). Aufschläge addieren sich.
    pub fn of(data: &GameData, known: &[bool]) -> Self {
        let mut effects = Self::default();
        for (tech, _) in data.techs.iter().zip(known).filter(|(_, known)| **known) {
            for effect in &tech.effects {
                effects.add(*effect);
            }
        }
        effects
    }

    fn add(&mut self, effect: TechEffect) {
        match effect {
            TechEffect::Yield(amount) => self.yield_factor += amount,
            TechEffect::FreighterSpeed(amount) => self.freighter_speed += amount,
            TechEffect::FleetSpeed(amount) => self.fleet_speed += amount,
            TechEffect::ShipSpeed(amount) => self.ship_speed += amount,
            TechEffect::WeaponDamage(amount) => self.weapon_damage += amount,
            TechEffect::WeaponRange(amount) => self.weapon_range += amount,
            TechEffect::Shields(amount) => self.shields += amount,
            TechEffect::Armor(amount) => self.armor += amount,
            TechEffect::Hull(amount) => self.hull += amount,
            TechEffect::GroundAttack(amount) => self.ground_attack += amount,
            TechEffect::GroundDefense(amount) => self.ground_defense += amount,
            TechEffect::Research(amount) => self.research += amount,
        }
    }
}

/// Forschungsstand einer Gemeinschaft.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PolityResearch {
    /// Erforscht, je Technologie aus `techs.ron`.
    pub known: Vec<bool>,
    /// Angesammelte Forschungspunkte je Technologie. Bleibt erhalten, wenn das Ziel wechselt.
    pub progress: Vec<f64>,
    /// Technologie, in die die Punkte gerade fließen.
    pub target: Option<u16>,
    /// Die Automatik wählt das Ziel. Sonst gilt das zuletzt befohlene; ist es erforscht,
    /// ruht die Forschung, bis ein neues gesetzt wird.
    pub auto: bool,
    /// Anteil der geglätteten Staatseinnahmen, der je Monat in die Forschung geht.
    pub budget_share: f64,
    /// Im letzten Monat erarbeitete Forschungspunkte.
    pub points_last_month: f64,
    /// Im letzten Monat an die Forschenden gezahlt.
    pub spent_last_month: Money,
    /// Wirkung der erforschten Technologien; folgt aus `known`.
    pub effects: TechEffects,
    /// Nachlass je Technologie (0..1) aus dem Wissen der Gemeinschaften in Kontakt.
    pub spillover: Vec<f64>,
    /// Punkte, die nach einem Abschluss übrig blieben und auf das nächste Ziel warten.
    pub carry: f64,
    /// Seit Spielbeginn gezahlt und erarbeitet.
    pub spent_total: Money,
    pub points_total: f64,
    /// Zahl der selbst abgeschlossenen Technologien (ohne Startwissen und Erbe).
    pub completed: u32,
}

impl PolityResearch {
    /// Leerer Stand: kein Wissen, automatische Zielwahl, üblicher Etat.
    pub fn new(data: &GameData) -> Self {
        let techs = data.techs.len();
        Self {
            known: vec![false; techs],
            progress: vec![0.0; techs],
            target: None,
            auto: true,
            budget_share: data.balance.research.budget_share,
            points_last_month: 0.0,
            spent_last_month: 0,
            effects: TechEffects::default(),
            spillover: vec![0.0; techs],
            carry: 0.0,
            spent_total: 0,
            points_total: 0.0,
            completed: 0,
        }
    }

    /// Sind alle Voraussetzungen der Technologie erforscht?
    pub fn prerequisites_met(&self, data: &GameData, tech: u16) -> bool {
        data.techs.get(tech as usize).is_some_and(|def| {
            def.requires_ids
                .iter()
                .all(|required| self.known.get(*required as usize).copied().unwrap_or(false))
        })
    }

    /// Kosten nach dem Nachlass aus fremdem Wissen.
    pub fn cost(&self, data: &GameData, tech: u16) -> f64 {
        let discount = self.spillover.get(tech as usize).copied().unwrap_or(0.0);
        data.techs[tech as usize].cost * (1.0 - discount)
    }

    /// Punkte, die bis zum Abschluss noch fehlen.
    pub fn remaining(&self, data: &GameData, tech: u16) -> f64 {
        let progress = self.progress.get(tech as usize).copied().unwrap_or(0.0);
        (self.cost(data, tech) - progress).max(0.0)
    }
}

/// Spanne, in der die Stufe einer fremden Gemeinschaft in einem Zweig liegt.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TierEstimate {
    pub lo: u8,
    pub hi: u8,
}

impl TierEstimate {
    pub fn exact(tier: u8) -> Self {
        Self { lo: tier, hi: tier }
    }

    pub fn is_exact(self) -> bool {
        self.lo == self.hi
    }

    pub fn contains(self, tier: u8) -> bool {
        (self.lo..=self.hi).contains(&tier)
    }

    /// Zahl der Stufen, um die die Schätzung offen ist (0 = genau).
    pub fn width(self) -> u8 {
        self.hi - self.lo
    }
}

impl std::fmt::Display for TierEstimate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_exact() {
            write!(f, "{}", self.lo)
        } else {
            write!(f, "{}–{}", self.lo, self.hi)
        }
    }
}

#[derive(Resource, Clone, Debug, Default, Serialize, Deserialize)]
pub struct Research {
    /// Je Gemeinschaft, in der Reihenfolge von `Polities::list`.
    pub polities: Vec<PolityResearch>,
    /// Einblick `intel[Betrachter][Ziel]` in die Forschung der anderen, 0..1.
    pub intel: Vec<Vec<f32>>,
}

impl Research {
    pub fn get(&self, polity: PolityId) -> Option<&PolityResearch> {
        self.polities.get(polity.index())
    }

    /// Wirkung der Technologien einer Gemeinschaft; neutral, wenn sie keinen Eintrag hat.
    pub fn effects(&self, polity: PolityId) -> TechEffects {
        self.get(polity)
            .map_or_else(TechEffects::default, |entry| entry.effects)
    }

    /// Erforschte Technologien (`known[tech]`); leer, wenn die Gemeinschaft keinen Eintrag hat.
    pub fn known(&self, polity: PolityId) -> &[bool] {
        self.get(polity).map_or(&[], |entry| &entry.known)
    }

    pub fn knows(&self, polity: PolityId, tech: u16) -> bool {
        self.known(polity)
            .get(tech as usize)
            .copied()
            .unwrap_or(false)
    }

    /// Zahl der erforschten Technologien.
    pub fn known_count(&self, polity: PolityId) -> usize {
        self.known(polity).iter().filter(|known| **known).count()
    }

    /// Höchste erforschte Stufe im Zweig, 0 = keine.
    pub fn tier(&self, data: &GameData, polity: PolityId, branch: TechBranch) -> u8 {
        tier_of(data, self.known(polity), branch)
    }

    /// Technologien, die jetzt erforschbar sind: noch unbekannt, alle Voraussetzungen erfüllt.
    pub fn available(&self, data: &GameData, polity: PolityId) -> Vec<u16> {
        available_of(data, self.known(polity))
    }

    /// Kosten einer Technologie für diese Gemeinschaft, nach dem Nachlass aus fremdem Wissen.
    pub fn cost(&self, data: &GameData, polity: PolityId, tech: u16) -> f64 {
        match self.get(polity) {
            Some(entry) => entry.cost(data, tech),
            None => data.techs[tech as usize].cost,
        }
    }

    /// Einblick des Betrachters in die Forschung des Ziels, 0..1; 1 für sich selbst.
    pub fn intel(&self, viewer: PolityId, target: PolityId) -> f32 {
        if viewer == target {
            return 1.0;
        }
        self.intel
            .get(viewer.index())
            .and_then(|row| row.get(target.index()))
            .copied()
            .unwrap_or(0.0)
    }

    /// Ein Ereignis (etwa ein beobachtetes Gefecht) verschafft Einblick. Gedeckelt bei 1;
    /// was über dem Niveau der wirtschaftlichen Verflechtung liegt, verblasst wieder.
    pub fn add_intel(&mut self, viewer: PolityId, target: PolityId, amount: f32) {
        if viewer == target || amount.is_nan() || amount <= 0.0 {
            return;
        }
        self.grow_intel(viewer.index().max(target.index()) + 1, 0.0);
        let cell = &mut self.intel[viewer.index()][target.index()];
        *cell = (*cell + amount).min(1.0);
    }

    /// Was der Betrachter über die Stufen des Ziels weiß: je Zweig eine Spanne, die die
    /// wahre Stufe immer enthält. Bei vollem Einblick ist sie genau, bei wenig Einblick
    /// bis zu `2 * estimate_spread` Stufen weit. Wo die Wahrheit in der Spanne liegt,
    /// folgt aus Seed, Betrachter, Ziel, Zweig und Jahr: Innerhalb eines Jahres gibt jede
    /// Abfrage dasselbe Bild, Nachfragen mittelt den Fehler nicht weg.
    pub fn estimate(
        &self,
        data: &GameData,
        viewer: PolityId,
        target: PolityId,
        seed: u64,
        year: u64,
    ) -> [TierEstimate; 5] {
        let intel = self.intel(viewer, target);
        let spread = data.balance.research.estimate_spread;
        let known = self.known(target);
        TechBranch::ALL.map(|branch| {
            let truth = tier_of(data, known, branch);
            let top = top_tier(data, branch);
            let roll = Rng::stream(
                seed,
                Domain::Intel,
                (viewer.0 as u64) << 32 | target.0 as u64,
                year << 8 | branch.index() as u64,
            )
            .f64();
            estimate_tier(truth, top, intel, spread, roll)
        })
    }

    /// Gewährt eine Technologie ohne Forschung (Startwissen, Szenarien, Tests), samt allen
    /// Voraussetzungen: Der Baum bleibt lückenlos. Gibt zurück, ob sich etwas geändert hat.
    pub fn grant(&mut self, data: &GameData, polity: PolityId, tech: u16) -> bool {
        if tech as usize >= data.techs.len() {
            return false;
        }
        self.sync(data, polity.index() + 1);
        let entry = &mut self.polities[polity.index()];
        let mut changed = false;
        let mut open = vec![tech];
        while let Some(tech) = open.pop() {
            if entry.known[tech as usize] {
                continue;
            }
            entry.known[tech as usize] = true;
            entry.progress[tech as usize] = 0.0;
            if entry.target == Some(tech) {
                entry.target = None;
            }
            open.extend(&data.techs[tech as usize].requires_ids);
            changed = true;
        }
        entry.effects = TechEffects::of(data, &entry.known);
        changed
    }

    /// Eine neue Gemeinschaft übernimmt den Wissensstand derjenigen, aus der sie hervorgeht
    /// (Abspaltung): erforschte Technologien, angefangene Arbeiten und den Etat. Was das
    /// alte Reich über andere wusste, weiß auch sie; die anderen sehen sie so gut wie das
    /// alte Reich. Beide kennen einander zunächst genau.
    pub fn inherit(&mut self, data: &GameData, parent: PolityId, child: PolityId) {
        if parent == child {
            return;
        }
        self.sync(data, parent.index().max(child.index()) + 1);
        let source = self.polities[parent.index()].clone();
        self.polities[child.index()] = PolityResearch {
            known: source.known,
            progress: source.progress,
            budget_share: source.budget_share,
            effects: source.effects,
            spillover: source.spillover,
            ..PolityResearch::new(data)
        };
        let (parent, child) = (parent.index(), child.index());
        for other in 0..self.intel.len() {
            if other == parent || other == child {
                continue;
            }
            self.intel[child][other] = self.intel[parent][other];
            self.intel[other][child] = self.intel[other][parent];
        }
        self.intel[child][parent] = 1.0;
        self.intel[parent][child] = 1.0;
    }

    /// Stellt sicher, dass es für jede Gemeinschaft einen vollständigen Eintrag gibt, und
    /// leitet die Wirkungen neu aus dem Wissen ab. Läuft zu Beginn jedes Forschungsschritts
    /// und vor jedem Befehl: Eine Gemeinschaft ohne Eintrag gibt es danach nicht.
    pub fn sync(&mut self, data: &GameData, polities: usize) {
        let techs = data.techs.len();
        let count = polities.max(self.polities.len()).max(self.intel.len());
        while self.polities.len() < count {
            self.polities.push(PolityResearch::new(data));
        }
        for entry in &mut self.polities {
            entry.known.resize(techs, false);
            entry.progress.resize(techs, 0.0);
            entry.spillover.resize(techs, 0.0);
            entry.effects = TechEffects::of(data, &entry.known);
            if entry.target.is_some_and(|tech| tech as usize >= techs) {
                entry.target = None;
            }
        }
        self.grow_intel(count, data.balance.research.intel_base);
    }

    /// Hat jede der `polities` Gemeinschaften vollständige Einträge? Für Prüfungen.
    pub fn is_consistent(&self, data: &GameData, polities: usize) -> bool {
        let techs = data.techs.len();
        self.polities.len() == polities
            && self.intel.len() == polities
            && self.intel.iter().all(|row| row.len() == polities)
            && self.polities.iter().all(|entry| {
                entry.known.len() == techs
                    && entry.progress.len() == techs
                    && entry.spillover.len() == techs
                    && entry.effects == TechEffects::of(data, &entry.known)
            })
    }

    /// Prüft, dass der Forschungsstand vollständig und nichts entgleist ist. Gibt die
    /// erste Auffälligkeit zurück.
    pub fn find_anomaly(&self, data: &GameData, polities: usize) -> Option<String> {
        if !self.is_consistent(data, polities) {
            return Some(format!(
                "Forschung: Einträge für {polities} Gemeinschaften unvollständig oder \
                 Wirkungen passen nicht zum Wissen"
            ));
        }
        let max_share = data.balance.research.max_budget_share;
        for (index, entry) in self.polities.iter().enumerate() {
            let amounts = entry.progress.iter().chain([
                &entry.carry,
                &entry.points_last_month,
                &entry.points_total,
            ]);
            if amounts
                .into_iter()
                .any(|value| !value.is_finite() || *value < 0.0)
            {
                return Some(format!("Forschung G{index}: Fortschritt entgleist"));
            }
            if entry.spent_last_month < 0 || entry.spent_total < 0 {
                return Some(format!("Forschung G{index}: negative Ausgaben"));
            }
            if !(0.0..=max_share).contains(&entry.budget_share) {
                return Some(format!(
                    "Forschung G{index}: Etat {} außerhalb der Grenzen",
                    entry.budget_share
                ));
            }
            if entry
                .spillover
                .iter()
                .any(|discount| !(0.0..1.0).contains(discount))
            {
                return Some(format!("Forschung G{index}: Nachlass entgleist"));
            }
            for (tech, def) in data.techs.iter().enumerate() {
                if entry.known[tech]
                    && def
                        .requires_ids
                        .iter()
                        .any(|required| !entry.known[*required as usize])
                {
                    return Some(format!(
                        "Forschung G{index}: {} ohne Voraussetzung erforscht",
                        def.name
                    ));
                }
            }
        }
        for (viewer, row) in self.intel.iter().enumerate() {
            for (target, intel) in row.iter().enumerate() {
                let expected = if viewer == target {
                    1.0..=1.0
                } else {
                    0.0..=1.0
                };
                if !expected.contains(intel) {
                    return Some(format!(
                        "Forschung: Einblick von G{viewer} in G{target} ist {intel}"
                    ));
                }
            }
        }
        None
    }

    /// Vergrößert die Einblick-Matrix auf `count` Gemeinschaften; neue Paare beginnen mit `fill`.
    fn grow_intel(&mut self, count: usize, fill: f32) {
        let count = count.max(self.intel.len());
        self.intel.resize(count, Vec::new());
        for (viewer, row) in self.intel.iter_mut().enumerate() {
            if row.len() < count {
                row.resize(count, fill);
            }
            row[viewer] = 1.0;
        }
    }
}

/// Höchste erforschte Stufe eines Zweigs, 0 = keine.
pub fn tier_of(data: &GameData, known: &[bool], branch: TechBranch) -> u8 {
    data.techs
        .iter()
        .zip(known)
        .filter(|(tech, known)| **known && tech.branch == branch)
        .map(|(tech, _)| tech.tier as u8)
        .max()
        .unwrap_or(0)
}

/// Höchste Stufe, die es in einem Zweig gibt.
pub fn top_tier(data: &GameData, branch: TechBranch) -> u8 {
    data.techs
        .iter()
        .filter(|tech| tech.branch == branch)
        .map(|tech| tech.tier as u8)
        .max()
        .unwrap_or(0)
}

/// Erforschbare Technologien bei gegebenem Wissen, aufsteigend.
pub fn available_of(data: &GameData, known: &[bool]) -> Vec<u16> {
    let knows = |tech: u16| known.get(tech as usize).copied().unwrap_or(false);
    (0..data.techs.len() as u16)
        .filter(|tech| !knows(*tech))
        .filter(|tech| {
            data.techs[*tech as usize]
                .requires_ids
                .iter()
                .all(|required| knows(*required))
        })
        .collect()
}

/// Forschungsstand zu Spielbeginn: Jede Gemeinschaft beherrscht die Starttechnologien
/// ihrer Vorlage aus `polities.ron`, alle anderen wissen davon zunächst nur das Übliche.
pub fn setup_research(data: &GameData, polities: &Polities) -> Research {
    let mut research = Research::default();
    research.sync(data, polities.list.len());
    for polity in &polities.list {
        // Die Gemeinschaft trägt den Namen ihrer Vorlage; daran wird sie wiedererkannt.
        let Some(template) = data.polities.iter().find(|def| def.name == polity.name) else {
            continue;
        };
        for tech in &template.start_tech_ids {
            research.grant(data, polity.id, *tech);
        }
    }
    research
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> GameData {
        GameData::load_workspace().unwrap()
    }

    #[test]
    fn effects_add_up_and_start_neutral() {
        let data = data();
        let none = vec![false; data.techs.len()];
        assert_eq!(TechEffects::of(&data, &none), TechEffects::default());
        let mut known = none.clone();
        for key in ["wirtschaft_1", "wirtschaft_2", "wirtschaft_3"] {
            known[data.tech_index(key).unwrap() as usize] = true;
        }
        let effects = TechEffects::of(&data, &known);
        assert!((effects.yield_factor - 1.09).abs() < 1e-12, "{effects:?}");
        assert!((effects.freighter_speed - 1.10).abs() < 1e-12);
        assert_eq!(effects.weapon_damage, 1.0);
        // Der ganze Baum: jeder Faktor liegt über eins, keiner ist entgleist.
        let all = TechEffects::of(&data, &vec![true; data.techs.len()]);
        for factor in [
            all.yield_factor,
            all.freighter_speed,
            all.fleet_speed,
            all.ship_speed,
            all.weapon_damage,
            all.weapon_range,
            all.shields,
            all.armor,
            all.hull,
            all.ground_attack,
            all.ground_defense,
            all.research,
        ] {
            assert!(factor > 1.0 && factor < 2.0, "{all:?}");
        }
    }

    #[test]
    fn tiers_and_availability_follow_the_tree() {
        let data = data();
        let me = PolityId(0);
        let mut research = Research::default();
        // Ohne Eintrag: neutral und leer, aber die Wurzeln des Baums sind erforschbar.
        assert_eq!(research.effects(me), TechEffects::default());
        assert!(research.known(me).is_empty());
        assert!(!research.knows(me, 0));
        assert_eq!(research.tier(&data, me, TechBranch::Economy), 0);
        let roots = research.available(&data, me);
        assert_eq!(roots.len(), 5);
        assert!(
            roots
                .iter()
                .all(|tech| data.techs[*tech as usize].tier == 1)
        );

        let first = data.tech_index("waffen_1").unwrap();
        let second = data.tech_index("waffen_2").unwrap();
        assert!(research.grant(&data, me, first));
        assert!(!research.grant(&data, me, first), "doppelt gewährt");
        assert_eq!(research.tier(&data, me, TechBranch::Weapons), 1);
        assert_eq!(research.tier(&data, me, TechBranch::Defense), 0);
        let available = research.available(&data, me);
        assert!(available.contains(&second) && !available.contains(&first));
        assert_eq!(available.len(), 5);
        assert!(research.is_consistent(&data, 1));
        assert_eq!(top_tier(&data, TechBranch::Ground), 6);
    }

    #[test]
    fn the_better_design_unlocks_with_its_technology() {
        let data = data();
        let me = PolityId(0);
        let mut research = Research::default();
        research.sync(&data, 1);
        let basic = data.design_index("speer").unwrap();
        let improved = data.design_index("speer_2").unwrap();
        let class = data.designs[basic as usize].stats.class;
        assert_eq!(data.best_design(class, research.known(me)), Some(basic));
        research.grant(&data, me, data.tech_index("waffen_1").unwrap());
        assert_eq!(data.best_design(class, research.known(me)), Some(basic));
        research.grant(&data, me, data.tech_index("waffen_2").unwrap());
        assert_eq!(data.best_design(class, research.known(me)), Some(improved));
    }

    #[test]
    fn intel_events_are_capped_and_never_touch_oneself() {
        let data = data();
        let mut research = Research::default();
        research.sync(&data, 3);
        let base = data.balance.research.intel_base;
        let (a, b) = (PolityId(0), PolityId(1));
        assert_eq!(research.intel(a, a), 1.0);
        assert_eq!(research.intel(a, b), base);
        research.add_intel(a, b, 0.3);
        assert!((research.intel(a, b) - (base + 0.3)).abs() < 1e-6);
        assert_eq!(research.intel(b, a), base, "Einblick ist gerichtet");
        research.add_intel(a, b, 5.0);
        assert_eq!(research.intel(a, b), 1.0);
        research.add_intel(a, a, -1.0);
        research.add_intel(a, b, f32::NAN);
        assert_eq!(research.intel(a, a), 1.0);
        assert_eq!(research.intel(a, b), 1.0);
        // Eine Gemeinschaft, die noch keinen Eintrag hat, verliert das Ereignis nicht.
        let late = PolityId(5);
        assert_eq!(research.intel(a, late), 0.0);
        research.add_intel(a, late, 0.25);
        assert_eq!(research.intel(a, late), 0.25);
        research.sync(&data, 3);
        assert!(research.is_consistent(&data, 6));
    }

    #[test]
    fn a_new_community_inherits_knowledge_and_insight() {
        let data = data();
        let mut research = Research::default();
        research.sync(&data, 3);
        let (parent, other, child) = (PolityId(0), PolityId(1), PolityId(3));
        let tech = data.tech_index("wirtschaft_1").unwrap();
        let begun = data.tech_index("wirtschaft_2").unwrap();
        research.grant(&data, parent, tech);
        research.polities[0].progress[begun as usize] = 1234.0;
        research.polities[0].budget_share = 0.2;
        research.polities[0].auto = false;
        research.polities[0].target = Some(begun);
        research.add_intel(parent, other, 0.4);
        research.add_intel(other, parent, 0.1);
        research.inherit(&data, parent, child);

        assert!(research.is_consistent(&data, 4));
        assert_eq!(research.known(child), research.known(parent));
        assert_eq!(research.effects(child), research.effects(parent));
        let entry = research.get(child).unwrap();
        assert_eq!(entry.progress[begun as usize], 1234.0);
        assert_eq!(entry.budget_share, 0.2);
        assert!(
            entry.auto && entry.target.is_none(),
            "die neue Führung wählt selbst"
        );
        assert_eq!(research.intel(child, other), research.intel(parent, other));
        assert_eq!(research.intel(other, child), research.intel(other, parent));
        assert_eq!(research.intel(child, parent), 1.0);
        assert_eq!(research.intel(parent, child), 1.0);
        // Das alte Reich behält, was es hatte.
        assert_eq!(research.polities[0].target, Some(begun));
    }

    #[test]
    fn starting_knowledge_comes_from_the_template() {
        let data = data();
        let state = crate::state::SimState::generate(
            &data,
            crate::state::SimConfig {
                seed: 11,
                player: None,
            },
        );
        let research = &state.research;
        assert!(research.is_consistent(&data, state.polities.list.len()));
        for polity in &state.polities.list {
            let template = data
                .polities
                .iter()
                .find(|def| def.name == polity.name)
                .expect("Vorlage");
            let expected: Vec<u16> = template.start_tech_ids.clone();
            let known: Vec<u16> = (0..data.techs.len() as u16)
                .filter(|tech| research.knows(polity.id, *tech))
                .collect();
            assert_eq!(known, expected, "{}", polity.name);
            assert_eq!(
                research.effects(polity.id),
                TechEffects::of(&data, research.known(polity.id))
            );
        }
        // Die Vorlagen unterscheiden sich: Nicht alle beginnen mit demselben Wissen.
        let first = research.known(PolityId(0)).to_vec();
        assert!(
            state
                .polities
                .list
                .iter()
                .any(|polity| research.known(polity.id) != first.as_slice())
        );
    }
}

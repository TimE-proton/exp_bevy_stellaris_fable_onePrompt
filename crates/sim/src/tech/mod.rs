//! Technologie und Forschung.
//!
//! Jede Gemeinschaft gibt monatlich einen Teil ihrer laufenden Staatseinnahmen (Lohnsteuer,
//! Zölle, Dividenden; `Economy::state_income`) für Forschung aus, soweit ihr Haushalt es
//! nach den vorrangigen Ansprüchen hergibt; die Rücklage rührt die Forschung nie an. Das
//! Geld ist das Gehalt der Forschenden. Es geht an den Staatsdienst, nicht an die Haushalte
//! der Welten (`Economy::staff`): Was die Forschung kostet, fehlt den übrigen. Aus ihm
//! werden Forschungspunkte, mehr davon bei großer Bevölkerung. Welche Zweige eine
//! Gemeinschaft vorzieht und wie viel sie ansetzt, bringt ihre Vorlage mit.
//!
//! Den genauen Stand kennt nur die Gemeinschaft selbst. Andere halten je Zweig eine Spanne
//! für möglich ([`Research::estimate`]), die mit ihrem Einblick enger wird. Einblick
//! entsteht aus Besitz an den Betrieben der anderen und aus Ereignissen wie beobachteten
//! Gefechten. Auch der Wissenstransfer speist sich nur aus diesem Bild: Billiger wird, was
//! Gemeinschaften in der Nähe erkennbar beherrschen. Wer zentral liegt und Einblick hat,
//! lernt schneller, und aus den eigenen Kosten ist nichts abzulesen, was die Spannen nicht
//! schon zeigen.
//!
//! Wirkungen: Die Aufschläge aller erforschten Technologien stehen gesammelt in
//! [`TechEffects`]. Ertrag und Frachterfahrt greifen in Wirtschaft und Logistik ein
//! (über `Economy::tech_yield` und `Logistics::tech_speed`); alles Übrige lesen Flotten,
//! Gefechte und Bodenkampf über [`Research::effects`].

mod intel;
mod plugin;
mod step;

pub use intel::{
    advance_belief, estimate_tier, estimate_width, intel_from_ties, ownership_ties, update_beliefs,
    update_intel,
};
pub use plugin::TechPlugin;
pub use step::{
    ai_budget_share, choose_target, contact_weights, research_claim, research_month,
    research_points, set_budget, set_target, spillover_discount, visible_spillover,
};

use crate::data::{BranchWeights, GameData, TechBranch, TechEffect};
use crate::economy::{Economy, Money};
use crate::ids::PolityId;
use crate::logistics::Logistics;
use crate::polity::Polities;
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
    /// Anteil der laufenden Staatseinnahmen des Vormonats (`Economy::state_income`), der
    /// je Monat in die Forschung geht.
    pub budget_share: f64,
    /// Im letzten Monat erarbeitete Forschungspunkte.
    pub points_last_month: f64,
    /// Im letzten Monat an die Forschenden gezahlt.
    pub spent_last_month: Money,
    /// Wirkung der erforschten Technologien; folgt aus `known`.
    pub effects: TechEffects,
    /// Nachlass je Technologie (0..1) aus dem, was die Gemeinschaft bei anderen in Kontakt
    /// sicher erkannt hat (untere Grenzen ihrer Schätzungen), nie aus deren wahrem Stand.
    pub spillover: Vec<f64>,
    /// Punkte, die nach einem Abschluss übrig blieben und auf das nächste Ziel warten.
    pub carry: f64,
    /// Seit Spielbeginn gezahlt und erarbeitet.
    pub spent_total: Money,
    pub points_total: f64,
    /// Zahl der selbst abgeschlossenen Technologien (ohne Startwissen und Erbe).
    pub completed: u32,
    /// Vorlieben der automatischen Zielwahl: Faktor je Zweig auf die Gewichte aus
    /// `balance.ron`. Stammt aus der Vorlage; ein Freistaat erbt sie.
    #[serde(default = "BranchWeights::even")]
    pub focus: BranchWeights,
    /// Etat, den eine KI-Führung dieser Gemeinschaft im Frieden ansetzt. Stammt aus der
    /// Vorlage; ein Freistaat erbt ihn.
    pub usual_share: f64,
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
            focus: BranchWeights::EVEN,
            usual_share: data.balance.research.budget_share,
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

    /// Kosten nach dem Nachlass aus dem erkennbaren Wissen der anderen.
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

/// Spanne, in der die Stufe einer fremden Gemeinschaft in einem Zweig liegt. `lo` ist
/// bestätigt: Mindestens diese Stufe beherrscht sie sicher.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
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
    /// Bild `beliefs[Betrachter][Ziel]`, das sich der Betrachter von den Stufen des Ziels
    /// macht: je Zweig (Reihenfolge von `TechBranch::ALL`) eine Spanne, die die Wahrheit
    /// enthält. Sie ist Zustand, keine Momentaufnahme: Die untere Grenze fällt nie, und
    /// ohne neuen Einblick bewegt sich die Spanne nur, wenn die Wahrheit sie verlässt oder
    /// sich eine weitere Stufe bestätigt ([`advance_belief`]). Wer das Bild über Jahre
    /// mitschreibt, weiß deshalb nicht mehr, als es gerade zeigt.
    #[serde(default)]
    pub beliefs: Vec<Vec<[TierEstimate; 5]>>,
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

    /// Kosten einer Technologie für diese Gemeinschaft, nach dem Nachlass aus dem
    /// erkennbaren Wissen der anderen.
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
    /// Die Schätzungen ([`Research::estimate`]) folgen dem neuen Einblick mit dem
    /// nächsten Forschungsschritt am Monatsersten.
    pub fn add_intel(&mut self, viewer: PolityId, target: PolityId, amount: f32) {
        if viewer == target || amount.is_nan() || amount <= 0.0 {
            return;
        }
        self.grow_intel(viewer.index().max(target.index()) + 1, 0.0);
        let cell = &mut self.intel[viewer.index()][target.index()];
        *cell = (*cell + amount).min(1.0);
    }

    /// Was der Betrachter über die Stufen des Ziels weiß: je Zweig (Reihenfolge von
    /// `TechBranch::ALL`) eine Spanne, die die wahre Stufe immer enthält. Bei vollem
    /// Einblick ist sie genau, bei wenig Einblick bis zu `2 * estimate_spread` Stufen
    /// weit. Sich selbst kennt jede Gemeinschaft genau.
    ///
    /// Das Bild ist gespeichert ([`Research::beliefs`]) und hängt weder vom Zeitpunkt
    /// noch von der Zahl der Abfragen ab: Nachfragen, auch über Jahre, schärft es nicht.
    pub fn estimate(
        &self,
        data: &GameData,
        viewer: PolityId,
        target: PolityId,
    ) -> [TierEstimate; 5] {
        let known = self.known(target);
        if viewer == target {
            return TechBranch::ALL.map(|branch| TierEstimate::exact(tier_of(data, known, branch)));
        }
        if let Some(stored) = self
            .beliefs
            .get(viewer.index())
            .and_then(|row| row.get(target.index()))
        {
            return *stored;
        }
        // Noch kein Bild (die Gemeinschaft hat noch keinen Eintrag): das weiteste, das der
        // Einblick zulässt, ohne dass irgendetwas bestätigt wäre.
        let width = estimate_width(
            self.intel(viewer, target),
            data.balance.research.estimate_spread,
        );
        TechBranch::ALL.map(|branch| {
            intel::initial_belief(tier_of(data, known, branch), top_tier(data, branch), width)
        })
    }

    /// Höchste Stufe im Zweig, die der Betrachter beim Ziel sicher erkannt hat: die untere
    /// Grenze seiner Schätzung. Nur daraus speist sich der Wissenstransfer.
    pub fn confirmed(&self, viewer: PolityId, target: PolityId, branch: TechBranch) -> u8 {
        self.beliefs
            .get(viewer.index())
            .and_then(|row| row.get(target.index()))
            .map_or(0, |cell| cell[branch.index()].lo)
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
        if changed {
            // Die Bilder der anderen müssen die neue Wahrheit weiter enthalten.
            self.settle_beliefs(data);
        }
        changed
    }

    /// Eine neue Gemeinschaft übernimmt den Wissensstand derjenigen, aus der sie hervorgeht
    /// (Abspaltung): erforschte Technologien, angefangene Arbeiten, Etat und Vorlieben.
    /// Was das alte Reich über andere wusste, weiß auch sie; die anderen sehen sie so gut
    /// wie das alte Reich. Beide kennen einander zunächst genau.
    pub fn inherit(&mut self, data: &GameData, parent: PolityId, child: PolityId) {
        if parent == child {
            return;
        }
        self.sync(data, parent.index().max(child.index()) + 1);
        // Erschöpfend zerlegt und gefüllt: Bei einem neuen Feld des Forschungsstands muss
        // hier entschieden werden, ob die Neue es erbt oder frisch beginnt.
        let PolityResearch {
            known,
            progress,
            target: _,
            auto: _,
            budget_share,
            points_last_month: _,
            spent_last_month: _,
            effects,
            spillover,
            carry: _,
            spent_total: _,
            points_total: _,
            completed: _,
            focus,
            usual_share,
        } = self.polities[parent.index()].clone();
        let fresh = PolityResearch::new(data);
        self.polities[child.index()] = PolityResearch {
            known,
            progress,
            target: fresh.target,
            auto: fresh.auto,
            budget_share,
            points_last_month: fresh.points_last_month,
            spent_last_month: fresh.spent_last_month,
            effects,
            spillover,
            carry: fresh.carry,
            spent_total: fresh.spent_total,
            points_total: fresh.points_total,
            completed: fresh.completed,
            focus,
            usual_share,
        };
        let (parent, child) = (parent.index(), child.index());
        for other in 0..self.intel.len() {
            if other == parent || other == child {
                continue;
            }
            self.intel[child][other] = self.intel[parent][other];
            self.intel[other][child] = self.intel[other][parent];
            self.beliefs[child][other] = self.beliefs[parent][other];
            self.beliefs[other][child] = self.beliefs[other][parent];
        }
        self.intel[child][parent] = 1.0;
        self.intel[parent][child] = 1.0;
        let exact = TechBranch::ALL
            .map(|branch| TierEstimate::exact(tier_of(data, &self.polities[child].known, branch)));
        self.beliefs[child][parent] = exact;
        self.beliefs[parent][child] = exact;
        self.beliefs[child][child] = exact;
    }

    /// Eine Gemeinschaft endet, und `into` tritt ihr Erbe an: Die Erbin beherrscht danach,
    /// was eine von beiden beherrschte, und führt je Technologie die weiter gediehene
    /// Arbeit fort. Ziel, Etat, Vorlieben und Bilanz bleiben ihre eigenen. Über Dritte weiß
    /// sie, was eine von beiden wusste: den besseren Einblick und, was beide Bilder
    /// zugleich zulassen. Der Stand der beendeten Gemeinschaft bleibt als Geschichte stehen.
    pub fn absorb(&mut self, data: &GameData, from: PolityId, into: PolityId) {
        if from == into {
            return;
        }
        self.sync(data, from.index().max(into.index()) + 1);
        // Erschöpfend zerlegt: Ein neues Feld des Forschungsstands fällt hier auf.
        let PolityResearch {
            known: inherited,
            progress: begun,
            target: _,
            auto: _,
            budget_share: _,
            points_last_month: _,
            spent_last_month: _,
            effects: _,
            spillover: _,
            carry: left_over,
            spent_total: _,
            points_total: _,
            completed: _,
            focus: _,
            usual_share: _,
        } = self.polities[from.index()].clone();
        let PolityResearch {
            known,
            progress,
            target,
            auto: _,
            budget_share: _,
            points_last_month: _,
            spent_last_month: _,
            effects,
            spillover: _,
            carry,
            spent_total: _,
            points_total: _,
            completed: _,
            focus: _,
            usual_share: _,
        } = &mut self.polities[into.index()];
        for tech in 0..known.len() {
            known[tech] |= inherited[tech];
            // Was erforscht ist, braucht keine angefangene Arbeit mehr.
            progress[tech] = if known[tech] {
                0.0
            } else {
                progress[tech].max(begun[tech])
            };
        }
        *carry = carry.max(left_over);
        if target.is_some_and(|tech| known[tech as usize]) {
            *target = None;
        }
        *effects = TechEffects::of(data, known);

        let (from, into) = (from.index(), into.index());
        for other in 0..self.intel.len() {
            if other == from || other == into {
                continue;
            }
            self.intel[into][other] = self.intel[into][other].max(self.intel[from][other]);
            // Beide Bilder enthalten die Wahrheit, also auch ihr Durchschnitt.
            let theirs = self.beliefs[from][other];
            for (mine, seen) in self.beliefs[into][other].iter_mut().zip(theirs) {
                mine.lo = mine.lo.max(seen.lo);
                mine.hi = mine.hi.min(seen.hi).max(mine.lo);
            }
        }
        // Die Bilder der anderen müssen die neue Wahrheit weiter enthalten.
        self.settle_beliefs(data);
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
        self.settle_beliefs(data);
    }

    /// Hat jede der `polities` Gemeinschaften vollständige Einträge, und enthält jedes
    /// Bild die Wahrheit? Für Prüfungen.
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
            && self.beliefs.len() == polities
            && self.beliefs.iter().all(|row| row.len() == polities)
            && self.belief_anomaly(data).is_none()
    }

    /// Erste Spanne, die die Wahrheit nicht enthält oder über den Baum hinausragt.
    fn belief_anomaly(&self, data: &GameData) -> Option<String> {
        let tops = TechBranch::ALL.map(|branch| top_tier(data, branch));
        for (viewer, row) in self.beliefs.iter().enumerate() {
            for (target, cell) in row.iter().enumerate() {
                let known = self.known(PolityId::from_index(target));
                for branch in TechBranch::ALL {
                    let truth = tier_of(data, known, branch);
                    let range = cell[branch.index()];
                    let sound = range.lo <= truth
                        && truth <= range.hi
                        && range.hi <= tops[branch.index()]
                        && (viewer != target || range.is_exact());
                    if !sound {
                        return Some(format!(
                            "Forschung: G{viewer} schätzt {} von G{target} auf {range}, wahr ist {truth}",
                            branch.label()
                        ));
                    }
                }
            }
        }
        None
    }

    /// Prüft, dass der Forschungsstand vollständig und nichts entgleist ist. Gibt die
    /// erste Auffälligkeit zurück.
    pub fn find_anomaly(&self, data: &GameData, polities: usize) -> Option<String> {
        if let Some(anomaly) = self.belief_anomaly(data) {
            return Some(anomaly);
        }
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
            for (name, share) in [
                ("Etat", entry.budget_share),
                ("üblicher Etat", entry.usual_share),
            ] {
                if !(0.0..=max_share).contains(&share) {
                    return Some(format!(
                        "Forschung G{index}: {name} {share} außerhalb der Grenzen"
                    ));
                }
            }
            if entry
                .focus
                .values()
                .iter()
                .any(|weight| !weight.is_finite() || *weight < 0.0)
            {
                return Some(format!("Forschung G{index}: Vorlieben entgleist"));
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

    /// Übergibt Wirtschaft und Logistik die Faktoren, mit denen sie rechnen: Ertrag der
    /// Werke und Fahrt der Frachter je Gemeinschaft. Läuft bei Spielbeginn, nach jedem
    /// Forschungsschritt und bei einer Abspaltung; die Tagessysteme frischen sie auf.
    pub fn publish_effects(&self, economy: &mut Economy, logistics: &mut Logistics) {
        self.publish_yield(economy);
        self.publish_speed(logistics);
    }

    /// Ertragsfaktor der Technik je Gemeinschaft nach `Economy::tech_yield`.
    pub fn publish_yield(&self, economy: &mut Economy) {
        // Nur für Gemeinschaften, die es in der Wirtschaft gibt; fehlende gelten als 1.
        let count = economy.treasuries.len();
        economy.tech_yield.clear();
        economy.tech_yield.extend(
            self.polities
                .iter()
                .take(count)
                .map(|entry| entry.effects.yield_factor),
        );
        economy.sync_polities();
    }

    /// Fahrtfaktor der Technik je Gemeinschaft nach `Logistics::tech_speed`.
    pub fn publish_speed(&self, logistics: &mut Logistics) {
        logistics.tech_speed.clear();
        logistics.tech_speed.extend(
            self.polities
                .iter()
                .map(|entry| entry.effects.freighter_speed),
        );
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

    /// Gibt jedem Paar ein Bild und hält jedes bei der Wahrheit, ohne Zufall: Neue Paare
    /// beginnen mit der weitesten Spanne, die ihr Einblick zulässt und in der nichts
    /// bestätigt ist; hat die Wahrheit eine Spanne verlassen (Wissen von außen gewährt),
    /// rückt sie gerade so weit nach. Alles Übrige regelt der Monatsschritt
    /// ([`update_beliefs`]).
    fn settle_beliefs(&mut self, data: &GameData) {
        let count = self.polities.len();
        let spread = data.balance.research.estimate_spread;
        let tops = TechBranch::ALL.map(|branch| top_tier(data, branch));
        let tiers: Vec<[u8; 5]> = self
            .polities
            .iter()
            .map(|entry| TechBranch::ALL.map(|branch| tier_of(data, &entry.known, branch)))
            .collect();
        self.beliefs.resize(count, Vec::new());
        for viewer in 0..count {
            let row = &mut self.beliefs[viewer];
            for target in 0..count {
                let truth = tiers[target];
                if viewer == target {
                    let exact = truth.map(TierEstimate::exact);
                    if target < row.len() {
                        row[target] = exact;
                    } else {
                        row.push(exact);
                    }
                    continue;
                }
                if target < row.len() {
                    for branch in 0..5 {
                        row[target][branch] =
                            intel::contain(row[target][branch], truth[branch], tops[branch]);
                    }
                } else {
                    let width = estimate_width(self.intel[viewer][target], spread);
                    row.push(std::array::from_fn(|branch| {
                        intel::initial_belief(truth[branch], tops[branch], width)
                    }));
                }
            }
            row.truncate(count);
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
/// ihrer Vorlage aus `polities.ron` und übernimmt deren Vorlieben und Etat. Von den
/// anderen ist zunächst nichts bestätigt.
pub fn setup_research(data: &GameData, polities: &Polities) -> Research {
    let mut research = Research::default();
    research.sync(data, polities.list.len());
    for polity in &polities.list {
        // Die Gemeinschaft trägt den Namen ihrer Vorlage; daran wird sie wiedererkannt.
        // Die Datenprüfung stellt sicher, dass kein Name zweimal vorkommt.
        let Some(template) = data.polities.iter().find(|def| def.name == polity.name) else {
            continue;
        };
        for tech in &template.start_tech_ids {
            research.grant(data, polity.id, *tech);
        }
        let share = template
            .research_share
            .unwrap_or(data.balance.research.budget_share);
        let entry = &mut research.polities[polity.id.index()];
        entry.focus = template.research_focus;
        entry.budget_share = share;
        entry.usual_share = share;
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
        research.polities[0].usual_share = 0.17;
        research.polities[0].focus.weapons = 1.9;
        research.polities[0].auto = false;
        research.polities[0].target = Some(begun);
        research.add_intel(parent, other, 0.4);
        research.add_intel(other, parent, 0.1);
        // Was das alte Reich von der dritten Gemeinschaft sicher weiß und umgekehrt.
        let seen = TierEstimate { lo: 0, hi: 2 };
        let seen_by_other = TierEstimate { lo: 1, hi: 3 };
        research.beliefs[0][1][TechBranch::Weapons.index()] = seen;
        research.beliefs[1][0][TechBranch::Economy.index()] = seen_by_other;
        research.inherit(&data, parent, child);

        assert!(research.is_consistent(&data, 4));
        assert_eq!(research.known(child), research.known(parent));
        assert_eq!(research.effects(child), research.effects(parent));
        let entry = research.get(child).unwrap();
        assert_eq!(entry.progress[begun as usize], 1234.0);
        assert_eq!(entry.budget_share, 0.2);
        assert_eq!(entry.usual_share, 0.17);
        assert_eq!(entry.focus, research.polities[0].focus);
        assert!(
            entry.auto && entry.target.is_none(),
            "die neue Führung wählt selbst"
        );
        assert_eq!(research.intel(child, other), research.intel(parent, other));
        assert_eq!(research.intel(other, child), research.intel(other, parent));
        assert_eq!(research.intel(child, parent), 1.0);
        assert_eq!(research.intel(parent, child), 1.0);
        // Die Bilder gehen mit: Die Neue sieht die anderen wie das alte Reich, die anderen
        // sehen sie wie das alte Reich, und beide kennen einander genau.
        assert_eq!(
            research.estimate(&data, child, other),
            research.estimate(&data, parent, other)
        );
        assert_eq!(
            research.estimate(&data, child, other)[TechBranch::Weapons.index()],
            seen
        );
        assert_eq!(
            research.estimate(&data, other, child),
            research.estimate(&data, other, parent)
        );
        assert_eq!(
            research.estimate(&data, other, child)[TechBranch::Economy.index()],
            seen_by_other
        );
        let truth = TechBranch::ALL.map(|branch| research.tier(&data, parent, branch));
        assert_eq!(
            research.estimate(&data, child, parent),
            truth.map(TierEstimate::exact)
        );
        assert_eq!(
            research.estimate(&data, parent, child),
            truth.map(TierEstimate::exact)
        );
        // Das alte Reich behält, was es hatte.
        assert_eq!(research.polities[0].target, Some(begun));
    }

    #[test]
    fn an_heir_knows_what_either_knew_and_keeps_the_further_work() {
        let data = data();
        let mut research = Research::default();
        research.sync(&data, 4);
        let (from, into, other) = (PolityId(1), PolityId(2), PolityId(3));
        let index = |key: &str| data.tech_index(key).unwrap();
        research.grant(&data, from, index("waffen_2"));
        research.grant(&data, into, index("wirtschaft_1"));
        research.grant(&data, other, index("antrieb_3"));
        // Angefangene Arbeit: An einer Technologie ist die eine weiter, an einer die andere,
        // und eine hat die Erbin begonnen, die die andere schon beherrscht.
        let (ahead, behind, known_there) = (
            index("verteidigung_1") as usize,
            index("antrieb_1") as usize,
            index("waffen_1") as usize,
        );
        research.polities[1].progress[ahead] = 900.0;
        research.polities[2].progress[ahead] = 300.0;
        research.polities[1].progress[behind] = 100.0;
        research.polities[2].progress[behind] = 700.0;
        research.polities[2].progress[known_there] = 50.0;
        research.polities[1].carry = 40.0;
        research.polities[2].carry = 15.0;
        research.polities[2].auto = false;
        research.polities[2].target = Some(known_there as u16);
        research.polities[2].budget_share = 0.25;
        research.polities[1].budget_share = 0.05;
        // Über die Dritte weiß jede etwas anderes; die Beendete hat den besseren Einblick.
        let propulsion = TechBranch::Propulsion.index();
        research.add_intel(from, other, 0.6);
        research.beliefs[1][3][propulsion] = TierEstimate { lo: 2, hi: 4 };
        research.beliefs[2][3][propulsion] = TierEstimate { lo: 0, hi: 3 };
        let seen_by_other = research.estimate(&data, other, into);
        let before = research.polities[1].clone();

        research.absorb(&data, from, into);

        assert!(research.is_consistent(&data, 4));
        for key in ["waffen_1", "waffen_2", "wirtschaft_1"] {
            assert!(research.knows(into, index(key)), "{key}");
        }
        assert!(!research.knows(into, index("antrieb_3")));
        assert_eq!(
            research.effects(into),
            TechEffects::of(&data, research.known(into))
        );
        let entry = research.get(into).unwrap();
        assert_eq!(entry.progress[ahead], 900.0);
        assert_eq!(entry.progress[behind], 700.0);
        assert_eq!(entry.progress[known_there], 0.0, "schon erforscht");
        assert_eq!(entry.carry, 40.0);
        assert_eq!(entry.target, None, "das Ziel ist mit dem Erbe erreicht");
        assert_eq!(entry.budget_share, 0.25, "der Etat bleibt der eigene");
        // Der bessere Einblick, und was beide Bilder zugleich zulassen.
        assert_eq!(research.intel(into, other), research.intel(from, other));
        assert_eq!(
            research.estimate(&data, into, other)[propulsion],
            TierEstimate { lo: 2, hi: 3 }
        );
        // Die Dritte sieht die Erbin weiter so, dass die Wahrheit hineinpasst: Der Zweig
        // Waffen ist von null auf zwei gestiegen.
        let weapons = TechBranch::Weapons.index();
        let now = research.estimate(&data, other, into)[weapons];
        assert!(now.contains(2) && now.lo >= seen_by_other[weapons].lo);
        // Die beendete Gemeinschaft bleibt als Geschichte stehen; sich selbst zu erben
        // ändert nichts.
        assert_eq!(research.polities[1], before);
        let unchanged = research.clone();
        research.absorb(&data, into, into);
        assert_eq!(research.polities, unchanged.polities);
        assert_eq!(research.beliefs, unchanged.beliefs);
    }

    #[test]
    fn pictures_start_wide_and_follow_granted_knowledge_without_chance() {
        let data = data();
        let spread = data.balance.research.estimate_spread;
        let mut research = Research::default();
        research.sync(&data, 3);
        let (viewer, target) = (PolityId(0), PolityId(1));
        let widest = estimate_width(data.balance.research.intel_base, spread);
        assert!(widest >= 2, "die Prüfung braucht eine weite Spanne");
        // Zu Beginn ist nichts bestätigt: die weiteste Spanne ab null.
        for range in research.estimate(&data, viewer, target) {
            assert_eq!((range.lo, range.hi), (0, widest));
        }
        assert_eq!(research.confirmed(viewer, target, TechBranch::Weapons), 0);
        // Sich selbst kennt jede genau, auch ohne gespeichertes Bild.
        assert!(
            research
                .estimate(&data, viewer, viewer)
                .iter()
                .all(|range| range.is_exact())
        );
        // Wissen, das innerhalb der Spanne liegt, ändert das Bild nicht …
        let inside = data
            .techs
            .iter()
            .position(|tech| tech.branch == TechBranch::Weapons && tech.tier == widest as u32)
            .unwrap() as u16;
        let before = research.estimate(&data, viewer, target);
        research.grant(&data, target, inside);
        assert_eq!(research.estimate(&data, viewer, target), before);
        // … und was darüber hinausgeht, schiebt die Spanne gerade so weit nach.
        let beyond = data
            .techs
            .iter()
            .position(|tech| tech.branch == TechBranch::Weapons && tech.tier == widest as u32 + 2)
            .unwrap() as u16;
        research.grant(&data, target, beyond);
        let after = research.estimate(&data, viewer, target)[TechBranch::Weapons.index()];
        assert_eq!((after.lo, after.hi), (2, widest + 2));
        assert_eq!(research.confirmed(viewer, target, TechBranch::Weapons), 2);
        assert!(research.is_consistent(&data, 3));
        // Eine Gemeinschaft ohne Eintrag: das weiteste Bild, das ihr Einblick zulässt.
        let stranger = PolityId(9);
        for range in research.estimate(&data, viewer, stranger) {
            assert_eq!(range.lo, 0);
            assert!(range.contains(0));
        }
        assert_eq!(research.confirmed(viewer, stranger, TechBranch::Economy), 0);
    }

    #[test]
    fn effects_reach_economy_and_logistics_for_every_community() {
        let data = data();
        let state = crate::state::SimState::generate(
            &data,
            crate::state::SimConfig {
                seed: 4,
                player: None,
            },
        );
        // Schon vor dem ersten Takt rechnen Wirtschaft und Logistik mit dem Startwissen.
        let count = state.polities.list.len();
        assert_eq!(state.economy.tech_yield.len(), count);
        assert_eq!(state.logistics.tech_speed.len(), count);
        let mut with_bonus = 0;
        for polity in &state.polities.list {
            let effects = state.research.effects(polity.id);
            assert_eq!(state.economy.yield_factor(polity.id), effects.yield_factor);
            let base = data.balance.logistics.speed;
            assert_eq!(
                state.logistics.freighter_speed(polity.id, base),
                base * effects.freighter_speed as f32
            );
            if effects.yield_factor > 1.0 {
                with_bonus += 1;
            }
        }
        assert!(
            with_bonus > 0,
            "Seed 4 zieht Vorlagen mit Fertigungsstraßen"
        );
        assert_eq!(state.economy.find_anomaly(), None);

        // Später gewährtes Wissen wird mit `publish_effects` übergeben; Gemeinschaften, die
        // es in der Wirtschaft nicht gibt, bekommen dort keinen Eintrag.
        let mut research = state.research.clone();
        let mut economy = state.economy.clone();
        let mut logistics = state.logistics.clone();
        let me = PolityId(0);
        research.grant(&data, me, data.tech_index("wirtschaft_5").unwrap());
        research.add_intel(me, PolityId::from_index(count + 2), 0.3);
        research.sync(&data, count);
        assert!(research.polities.len() > count);
        research.publish_effects(&mut economy, &mut logistics);
        assert_eq!(economy.tech_yield.len(), count);
        assert_eq!(economy.yield_factor(me), research.effects(me).yield_factor);
        assert!(economy.yield_factor(me) > state.economy.yield_factor(me));
        assert!(logistics.freighter_speed(me, 10.0) > state.logistics.freighter_speed(me, 10.0));
        assert_eq!(economy.find_anomaly(), None);
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
            // Vorlieben und Etat kommen ebenfalls aus der Vorlage.
            let entry = research.get(polity.id).unwrap();
            let share = template
                .research_share
                .unwrap_or(data.balance.research.budget_share);
            assert_eq!(entry.focus, template.research_focus, "{}", polity.name);
            assert_eq!(entry.budget_share, share, "{}", polity.name);
            assert_eq!(entry.usual_share, share, "{}", polity.name);
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

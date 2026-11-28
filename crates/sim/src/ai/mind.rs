//! Verborgene Haltungen der Gemeinschaften.
//!
//! Jede Gemeinschaft hat eine Haltung: eine Mischung der vier Grundhaltungen aus
//! `diplomacy.ron` (kooperativ, opportunistisch, misstrauisch, expansionistisch). Sie
//! beginnt mit einem Anker (eine Grundhaltung mit `anchor_main`, die übrigen mit
//! `anchor_rest`; welche, zieht jede Gemeinschaft für sich aus dem Seed nach den Gewichten
//! `anchor_draw`, so dass manche Galaxie von Kooperativen, manche von Expansionistischen
//! geprägt ist) und treibt monatlich unter öffentlichem Druck davon weg:
//! Verflechtung nährt das Kooperative, Gelegenheit das Opportunistische, Bedrohung das
//! Misstrauische, Not das Expansionistische. Erschütterungen (ein erlittener
//! Vertragsbruch) stoßen sie unmittelbar an.
//!
//! Die Felder sind außerhalb von `ai` nicht lesbar. Die KI entscheidet mit dem gemischten
//! Profil ([`Minds::profile`]) über dieselbe Wahlfunktion, mit der Beobachter Taten
//! bewerten (`diplomacy::occasion::choice_probs`); Beobachter sehen nur die Taten.

use crate::data::{ArchetypeDef, GameData};
use crate::diplomacy::{Archetype, WarAim};
use crate::ids::PolityId;
use crate::rng::{Domain, Rng};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Haltung einer Gemeinschaft.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Mind {
    /// Anker: Gewichte, zu denen sie zurückkehrt.
    pub(in crate::ai) anchor: [f32; 4],
    /// Gegenwärtige Gewichte (Summe 1).
    pub(in crate::ai) weights: [f32; 4],
    /// Zuletzt gemessener Druck (Verflechtung, Gelegenheit, Bedrohung, Not), 0..1.
    pub(in crate::ai) pressure: [f32; 4],
    /// Laufende Pläne gegenüber anderen Gemeinschaften: zuschlagen oder binden.
    #[serde(default)]
    pub(in crate::ai) plans: BTreeMap<PolityId, Plan>,
}

/// Was eine Gemeinschaft gegenüber einer anderen vorhat.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanKind {
    /// Krieg mit diesem Ziel, sobald die Vorbereitung steht und die Aussicht reicht.
    Strike(WarAim),
    /// Annäherung: Vertragsstufen, Beteiligungen, am Ende die Vereinigung.
    Bind,
}

/// Ein Plan gegenüber einer anderen Gemeinschaft.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub(in crate::ai) kind: PlanKind,
    /// Tag des Entschlusses.
    pub(in crate::ai) made_day: u64,
    /// Frühester Tag der Erklärung (Zuschlagen).
    pub(in crate::ai) ready_day: u64,
    /// Danach verfällt der Plan, wenn er nicht ausgeführt ist.
    pub(in crate::ai) until_day: u64,
    /// Die Verträge mit dem Ziel sind gekündigt: Die Gemeinschaft wartet die Frist ab,
    /// statt zu brechen.
    pub(in crate::ai) denounced: bool,
}

/// Haltungen aller Gemeinschaften, in der Reihenfolge von `Polities::list`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Minds {
    pub(in crate::ai) list: Vec<Mind>,
}

/// Erschütterungen, die eine Haltung unmittelbar verschieben.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Jolt {
    /// Ein Vertragspartner hat gebrochen: misstrauischer.
    Betrayed,
    /// Eine Unterwerfung ist gelungen: expansionistischer.
    Subjugated,
}

fn normalize(weights: [f32; 4]) -> [f32; 4] {
    let clean = weights.map(|w| if w.is_finite() { w.max(0.0) } else { 0.0 });
    let sum: f32 = clean.iter().sum();
    if sum <= 0.0 {
        [0.25; 4]
    } else {
        clean.map(|w| w / sum)
    }
}

/// Grundhaltung, mit der die `k`-te Gemeinschaft beginnt: für jede Gemeinschaft für sich
/// gezogen, nach den Gewichten `anchor_draw`.
fn draw_anchor(data: &GameData, seed: u64, k: usize) -> usize {
    let weights = data.diplomacy.levers.anchor_draw.map(|w| w.max(0.0) as f64);
    let total: f64 = weights.iter().sum();
    let mut rng = Rng::stream(seed, Domain::Ai, 0, k as u64);
    let mut roll = rng.f64() * total;
    for (index, weight) in weights.iter().enumerate() {
        if roll < *weight {
            return index;
        }
        roll -= weight;
    }
    3
}

impl Minds {
    /// Haltungen zu Spielbeginn für `count` Gemeinschaften.
    pub fn deal(data: &GameData, seed: u64, count: usize) -> Self {
        let mut minds = Minds::default();
        minds.ensure(data, seed, count);
        minds
    }

    /// Ergänzt Haltungen für Gemeinschaften, die noch keine haben (neu gegründet, oder
    /// ein Spielstand ohne Haltungen). Wer schon eine hat, behält sie.
    pub fn ensure(&mut self, data: &GameData, seed: u64, count: usize) {
        let levers = &data.diplomacy.levers;
        while self.list.len() < count {
            let k = self.list.len();
            let mut anchor = [levers.anchor_rest; 4];
            anchor[draw_anchor(data, seed, k)] = levers.anchor_main;
            let anchor = normalize(anchor);
            self.list.push(Mind {
                anchor,
                weights: anchor,
                pressure: [0.0; 4],
                plans: BTreeMap::new(),
            });
        }
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// Gemischtes Profil einer Gemeinschaft: nur für Entscheidungen der KI.
    pub(in crate::ai) fn profile(&self, data: &GameData, polity: PolityId) -> ArchetypeDef {
        let weights = self
            .list
            .get(polity.index())
            .map_or([0.25; 4], |mind| mind.weights);
        ArchetypeDef::blend(&data.diplomacy.archetypes, &weights)
    }

    /// Monatlich: Die Gewichte folgen dem Ziel aus Anker und Druck.
    pub(in crate::ai) fn drift(&mut self, data: &GameData, polity: PolityId, pressure: [f32; 4]) {
        let levers = &data.diplomacy.levers;
        let Some(mind) = self.list.get_mut(polity.index()) else {
            return;
        };
        mind.pressure = pressure.map(|p| p.clamp(0.0, 1.0));
        let mut target = [0.0; 4];
        for (index, slot) in target.iter_mut().enumerate() {
            *slot = levers.anchor_weight * mind.anchor[index] + mind.pressure[index];
        }
        let target = normalize(target);
        let mut weights = mind.weights;
        for (index, slot) in weights.iter_mut().enumerate() {
            *slot += levers.drift * (target[index] - *slot);
        }
        mind.weights = normalize(weights);
    }

    /// Eine Erschütterung.
    pub fn jolt(&mut self, polity: PolityId, jolt: Jolt) {
        let Some(mind) = self.list.get_mut(polity.index()) else {
            return;
        };
        let (archetype, amount) = match jolt {
            Jolt::Betrayed => (Archetype::Paranoid, 0.20),
            Jolt::Subjugated => (Archetype::Expansionist, 0.06),
        };
        mind.weights[archetype.index()] += amount;
        mind.weights = normalize(mind.weights);
    }

    /// Der Plan einer Gemeinschaft gegenüber einer anderen.
    pub(in crate::ai) fn plan(&self, polity: PolityId, other: PolityId) -> Option<Plan> {
        self.list
            .get(polity.index())
            .and_then(|mind| mind.plans.get(&other))
            .copied()
    }

    pub(in crate::ai) fn set_plan(&mut self, polity: PolityId, other: PolityId, plan: Option<Plan>) {
        let Some(mind) = self.list.get_mut(polity.index()) else {
            return;
        };
        match plan {
            Some(plan) => {
                mind.plans.insert(other, plan);
            }
            None => {
                mind.plans.remove(&other);
            }
        }
    }

    /// Rüstungsfaktor aus der Haltung: Ihr Rüstungsziel (`arm0 + arm1 · Bedrohung`) gegen
    /// das mittlere der vier Grundhaltungen bei derselben Bedrohung.
    pub(in crate::ai) fn arming_factor(&self, data: &GameData, polity: PolityId) -> f64 {
        let Some(mind) = self.list.get(polity.index()) else {
            return 1.0;
        };
        let threat = mind.pressure[Archetype::Paranoid.index()];
        let profile = self.profile(data, polity);
        let archetypes = &data.diplomacy.archetypes;
        let mean = archetypes
            .iter()
            .map(|archetype| archetype.arm0 + archetype.arm1 * threat)
            .sum::<f32>()
            / archetypes.len().max(1) as f32;
        let own = profile.arm0 + profile.arm1 * threat;
        if mean > 0.0 {
            (own / mean) as f64
        } else {
            1.0
        }
    }

    /// Rüstet die Gemeinschaft für einen Schlag (ein Plan zum Zuschlagen besteht)?
    pub(in crate::ai) fn mobilizing(&self, polity: PolityId) -> bool {
        self.list.get(polity.index()).is_some_and(|mind| {
            mind.plans
                .values()
                .any(|plan| matches!(plan.kind, PlanKind::Strike(_)))
        })
    }

    /// Vergisst alle Pläne gegenüber einer Gemeinschaft, die endete oder nicht mehr frei
    /// ist, und ihre eigenen.
    pub(crate) fn forget(&mut self, polity: PolityId) {
        for mind in &mut self.list {
            mind.plans.remove(&polity);
        }
        if let Some(mind) = self.list.get_mut(polity.index()) {
            mind.plans.clear();
        }
    }

    /// Grundhaltung, mit der eine Gemeinschaft begann (ihr Anker). Nur für Auswertungen
    /// der Läufe ohne Grafik (Merkmal `reveal`) und für Prüfungen: Die Oberfläche liest
    /// sie nie.
    #[cfg(any(test, feature = "reveal"))]
    pub fn anchor_archetype(&self, polity: PolityId) -> Option<Archetype> {
        let mind = self.list.get(polity.index())?;
        let main = (0..4).max_by(|a, b| mind.anchor[*a].total_cmp(&mind.anchor[*b]))?;
        Some(Archetype::from_index(main))
    }

    /// Gewichte einer Gemeinschaft, nur für Prüfungen innerhalb der KI.
    #[cfg(test)]
    pub(in crate::ai) fn weights(&self, polity: PolityId) -> [f32; 4] {
        self.list[polity.index()].weights
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_archetype_anchors_one_of_four_communities() {
        let data = GameData::load_workspace().unwrap();
        for seed in 0..5 {
            let minds = Minds::deal(&data, seed, 8);
            let mut seen = [0; 4];
            for k in 0..8 {
                let weights = minds.weights(PolityId(k));
                assert!((weights.iter().sum::<f32>() - 1.0).abs() < 1e-5);
                let main = (0..4)
                    .max_by(|a, b| weights[*a].total_cmp(&weights[*b]))
                    .unwrap();
                seen[main] += 1;
                // Gemeinschaft k und k+4 teilen den Anker.
                if k >= 4 {
                    assert_eq!(
                        minds.list[k as usize].anchor,
                        minds.list[k as usize - 4].anchor
                    );
                }
            }
            assert_eq!(seen, [2; 4]);
        }
    }

    #[test]
    fn pressure_moves_the_weights_and_the_anchor_pulls_back() {
        let data = GameData::load_workspace().unwrap();
        let mut minds = Minds::deal(&data, 1, 4);
        let me = PolityId(0);
        let start = minds.weights(me);
        for _ in 0..120 {
            minds.drift(&data, me, [0.0, 0.0, 1.0, 0.0]);
        }
        let pressed = minds.weights(me);
        assert!(pressed[Archetype::Paranoid.index()] > start[Archetype::Paranoid.index()] + 0.1);
        for _ in 0..240 {
            minds.drift(&data, me, [0.0; 4]);
        }
        let back = minds.weights(me);
        for index in 0..4 {
            assert!(
                (back[index] - start[index]).abs() < 0.02,
                "{back:?} gegen {start:?}"
            );
        }
        minds.jolt(me, Jolt::Betrayed);
        assert!(minds.weights(me)[Archetype::Paranoid.index()] > back[Archetype::Paranoid.index()]);
    }
}

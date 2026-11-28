//! Öffentliche Taten, Dossiers und Indizien.
//!
//! Jede diplomatische Tat (Kriegserklärung, Kriegsziel, Antrag einer Bindung, Antwort auf
//! einen Vorschlag) wird mit dem öffentlichen Bild der Lage festgehalten, in der sie
//! geschah ([`Act`]). Täglich bewerten die Beobachter die Taten des Vortags: Für jede der
//! vier Grundhaltungen fragt [`super::occasion::log_likelihood`], wie gut sie die Tat
//! erklärt, und das Dossier des Beobachters ([`Dossier`]) verschiebt sich zugunsten der
//! besser erklärenden (`logit += k · Sichtbarkeit · ln P`, verschoben auf höchstens 0 und
//! begrenzt auf −3). Die Einschätzung ist der Softmax der Logits, ihre Unsicherheit
//! ±0,35/√(1 + Gewicht). Indizien verblassen mit `half_life_months`.
//!
//! Dieses Modul kennt keine verborgene Haltung: Es sieht nur, was getan wurde.

use super::occasion::{Archetype, Observed, Occasion, log_likelihood};
use crate::data::{ArchetypeDef, DiplomacyLevers};
use crate::ids::PolityId;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Art einer Tat und des Indizes, das sie hinterlässt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActKind {
    /// Kriegserklärung (Eröffnung: Zuschlagen).
    Strike,
    /// Antrag einer Bindung, der erste eines Jahres (Eröffnung: Binden).
    Bind,
    /// Kriegsziel bei der Erklärung.
    Aim,
    /// Antwort auf einen Vorschlag.
    Answer,
    /// Rüstung im Verhältnis zur Bedrohung (stetig).
    Arming,
}

impl ActKind {
    pub fn label(self) -> &'static str {
        match self {
            ActKind::Strike => "Kriegserklärung",
            ActKind::Bind => "Annäherung",
            ActKind::Aim => "Kriegsziel",
            ActKind::Answer => "Antwort",
            ActKind::Arming => "Rüstung",
        }
    }

    /// Gewicht des Indizes nach `diplomacy.ron`.
    pub fn weight(self, levers: &DiplomacyLevers) -> f32 {
        let weights = &levers.clue_weight;
        match self {
            ActKind::Strike | ActKind::Bind => weights.opening,
            ActKind::Aim => weights.terms,
            ActKind::Answer => weights.answer,
            ActKind::Arming => weights.continuous,
        }
    }
}

/// Wer eine Tat sieht.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Audience {
    /// Alle, auch das öffentliche Bild.
    Public,
    /// Nur der, dem sie galt (eine abgelehnte Anfrage etwa).
    Counterpart,
}

/// Eine diplomatische Tat mit dem öffentlichen Bild der Lage, in der sie geschah.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Act {
    pub day: u64,
    pub actor: PolityId,
    pub toward: PolityId,
    pub kind: ActKind,
    pub occasion: Occasion,
    pub observed: Observed,
    pub audience: Audience,
    /// Quelle in Worten, für das Indiz („lehnt den Vertrag Handel ab“).
    pub source: String,
}

/// Ein Indiz im Dossier: was die Einschätzung bewegt hat.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Clue {
    pub day: u64,
    pub kind: ActKind,
    pub toward: PolityId,
    /// Änderung der Einschätzung je Grundhaltung (Reihenfolge von [`Archetype::ALL`]).
    pub delta: [f32; 4],
    pub source: String,
}

impl Clue {
    /// Die Grundhaltung, für die das Indiz am stärksten spricht.
    pub fn points_to(&self) -> Archetype {
        let best = (0..4)
            .max_by(|a, b| self.delta[*a].total_cmp(&self.delta[*b]))
            .unwrap_or(0);
        Archetype::from_index(best)
    }
}

/// Was ein Beobachter aus den Taten einer Gemeinschaft schließt.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Dossier {
    /// Logits je Grundhaltung.
    pub logit: [f32; 4],
    /// Gesammeltes Gewicht der Indizien.
    pub weight: f32,
    /// Geglättete Überraschung: wie schlecht die bisherige Einschätzung die jüngsten Taten
    /// vorhersagte, über das Erwartete hinaus.
    pub surprise: f32,
    /// Tag, an dem die Einschätzung zuletzt als überholt galt.
    pub break_day: Option<u64>,
    /// Jüngste Indizien, älteste zuerst.
    pub clues: VecDeque<Clue>,
}

impl Dossier {
    /// Einschätzung: Wahrscheinlichkeit je Grundhaltung (Softmax der Logits).
    pub fn belief(&self) -> [f32; 4] {
        let top = self.logit.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let exp = self.logit.map(|logit| (logit - top).exp());
        let sum: f32 = exp.iter().sum();
        exp.map(|value| value / sum)
    }

    /// Halbe Breite der Unsicherheit jeder Einschätzung.
    pub fn band(&self) -> f32 {
        0.35 / (1.0 + self.weight.max(0.0)).sqrt()
    }

    /// Vertrauen, das die Einschätzung rechtfertigt: Σ Einschätzung · Vertragstreue.
    pub fn trust(&self, profiles: &[ArchetypeDef]) -> f32 {
        self.belief()
            .iter()
            .zip(profiles)
            .map(|(belief, profile)| belief * profile.honor)
            .sum()
    }

    /// Bewertet eine beobachtete Tat und legt das Indiz ab. `visibility` 0..1: wie
    /// deutlich der Beobachter sie sah.
    pub fn observe(
        &mut self,
        act: &Act,
        visibility: f32,
        profiles: &[ArchetypeDef],
        levers: &DiplomacyLevers,
    ) {
        let k = act.kind.weight(levers) * visibility.clamp(0.0, 1.0);
        if k <= 0.0 || profiles.len() < 4 {
            return;
        }
        let mut ll = [0.0f32; 4];
        for (slot, profile) in ll.iter_mut().zip(profiles) {
            *slot = log_likelihood(profile, &act.occasion, act.observed, levers);
        }
        let top = ll.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        if !top.is_finite() {
            return;
        }
        let before = self.belief();
        // Überraschung: Wie unwahrscheinlich war die Tat nach der bisherigen Einschätzung,
        // gemessen an dem, was diese Einschätzung im Mittel erwartete?
        if !matches!(act.occasion, Occasion::Arming { .. }) {
            let predicted: f32 = before
                .iter()
                .zip(ll)
                .map(|(belief, ll)| belief * ll.exp())
                .sum();
            let surprisal = -predicted.max(1e-6).ln();
            let options = act.occasion.options().len().max(2) as f32;
            let expected = options.ln() * 0.5;
            self.surprise += 0.3 * (surprisal - expected - self.surprise);
            if self.surprise > levers.surprise_break && self.weight > 2.0 {
                self.break_day = Some(act.day);
                self.logit = self.logit.map(|logit| logit * 0.5);
                self.weight *= 0.5;
                self.surprise = 0.0;
            }
        }
        for (logit, ll) in self.logit.iter_mut().zip(ll) {
            *logit += k * (ll - top).clamp(-3.0, 0.0);
        }
        // Die Logits nur relativ: der größte bei 0, damit nichts davonläuft.
        let top = self.logit.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        self.logit = self.logit.map(|logit| logit - top);
        self.weight += k;
        let after = self.belief();
        let mut delta = [0.0; 4];
        for index in 0..4 {
            delta[index] = after[index] - before[index];
        }
        self.clues.push_back(Clue {
            day: act.day,
            kind: act.kind,
            toward: act.toward,
            delta,
            source: act.source.clone(),
        });
        while self.clues.len() > levers.clues_kept {
            self.clues.pop_front();
        }
    }

    /// Monatlich: Alte Indizien verblassen. Die Logits und das Gewicht schrumpfen mit der
    /// Halbwertszeit; die Einschätzung kehrt so langsam zur Gleichverteilung zurück.
    pub fn decay(&mut self, levers: &DiplomacyLevers) {
        let factor = 0.5f32.powf(1.0 / levers.half_life_months.max(1.0));
        self.logit = self.logit.map(|logit| logit * factor);
        self.weight *= factor;
        self.surprise *= factor;
    }

    /// Die führende Grundhaltung und ihre Einschätzung.
    pub fn leading(&self) -> (Archetype, f32) {
        let belief = self.belief();
        let best = (0..4)
            .max_by(|a, b| belief[*a].total_cmp(&belief[*b]).then(b.cmp(a)))
            .unwrap_or(0);
        (Archetype::from_index(best), belief[best])
    }
}

/// Stellt eine Tat zur Bewertung ein. Bewertet wird am nächsten Tagesbeginn.
pub fn log_act(diplomacy: &mut super::Diplomacy, act: Act) {
    diplomacy.pending_acts.push(act);
}

/// Täglich: Die Beobachter bewerten die eingestellten Taten. Wer eine Tat sieht, hängt an
/// ihrem Publikum; der Täter bewertet sich nicht selbst. Danach stehen die Taten im
/// öffentlichen Protokoll (auch die nicht öffentlichen: Das Protokoll ist Geschichte, die
/// Oberfläche zeigt nur, was der Betrachter sehen durfte).
pub fn score_pending(
    diplomacy: &mut super::Diplomacy,
    polities: &crate::polity::Polities,
    data: &crate::data::GameData,
) {
    let (profiles, levers) = (&data.diplomacy.archetypes, &data.diplomacy.levers);
    let acts = std::mem::take(&mut diplomacy.pending_acts);
    for act in acts {
        for observer in polities.alive().map(|polity| polity.id) {
            if observer == act.actor
                || (act.audience == Audience::Counterpart && observer != act.toward)
            {
                continue;
            }
            diplomacy
                .dossiers
                .entry((observer, act.actor))
                .or_default()
                .observe(&act, 1.0, profiles, levers);
        }
        if act.audience == Audience::Public {
            diplomacy
                .public_face
                .entry(act.actor)
                .or_default()
                .observe(&act, 1.0, profiles, levers);
        }
        diplomacy.acts.push_back(act);
        while diplomacy.acts.len() > levers.acts_kept {
            diplomacy.acts.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::GameData;
    use crate::diplomacy::occasion::{Answering, Picture, choice_probs};
    use crate::diplomacy::relations::TreatyKind;
    use crate::rng::{Domain, Rng};

    fn varied_picture(rng: &mut Rng) -> Picture {
        Picture {
            ratio: (rng.f64() as f32 - 0.5) * 2.0,
            trend_gap: (rng.f64() as f32 - 0.5) * 0.1,
            share_me: 0.2,
            share_other: 0.2,
            own_fleet: 100.0,
            other_fleet: (
                50.0 + 100.0 * rng.f64() as f32,
                150.0 + 100.0 * rng.f64() as f32,
            ),
            hops: 2.0 + 4.0 * rng.f64() as f32,
            dep: 0.4 * rng.f64() as f32,
            aff: 0.5 + 0.5 * rng.f64() as f32,
            trust: 0.4 + 0.4 * rng.f64() as f32,
            tension: 50.0 * rng.f64() as f32,
            want: 0.5 * rng.f64() as f32,
            rungs: (3.0 * rng.f64()).floor() as f32,
            lead: (rng.f64() as f32 - 0.5) * 80.0,
            weariness: 60.0 * rng.f64() as f32,
            ..Picture::default()
        }
    }

    fn occasion(rng: &mut Rng) -> Occasion {
        let picture = varied_picture(rng);
        match (rng.f64() * 4.0) as usize {
            0 => Occasion::Opening(picture),
            1 => Occasion::Aim(picture),
            2 => Occasion::Answer(
                Answering::Treaty(TreatyKind::ALL[(rng.f64() * 6.0) as usize]),
                picture,
            ),
            _ => Occasion::Answer(
                Answering::Peace {
                    cost: 0.4 * rng.f64() as f32,
                    gain: 0.0,
                },
                picture,
            ),
        }
    }

    fn draw(probs: &[f32], roll: f64) -> usize {
        let mut acc = 0.0;
        for (index, p) in probs.iter().enumerate() {
            acc += *p as f64;
            if roll < acc {
                return index;
            }
        }
        probs.len() - 1
    }

    fn act(occasion: Occasion, option: usize, day: u64) -> Act {
        Act {
            day,
            actor: PolityId(1),
            toward: PolityId(0),
            kind: match occasion {
                Occasion::Opening(_) => ActKind::Bind,
                Occasion::Aim(_) => ActKind::Aim,
                _ => ActKind::Answer,
            },
            occasion,
            observed: Observed::Option(option),
            audience: Audience::Public,
            source: "Prüfung".into(),
        }
    }

    #[test]
    fn an_observer_converges_on_the_true_archetype() {
        let data = GameData::load_workspace().unwrap();
        let (profiles, levers) = (&data.diplomacy.archetypes, &data.diplomacy.levers);
        for truth in Archetype::ALL {
            let mut hits = 0;
            for seed in 0..6u64 {
                let mut rng = Rng::stream(seed, Domain::Diplomacy, truth.index() as u64, 7);
                let mut dossier = Dossier::default();
                for day in 0..120 {
                    let occasion = occasion(&mut rng);
                    let probs = choice_probs(&profiles[truth.index()], &occasion, levers);
                    let option = draw(&probs, rng.f64());
                    dossier.observe(&act(occasion, option, day), 1.0, profiles, levers);
                }
                if dossier.leading().0 == truth {
                    hits += 1;
                }
                assert!(dossier.band() < 0.1);
            }
            assert!(hits >= 5, "{truth:?}: nur {hits} von 6 erkannt");
        }
    }

    #[test]
    fn random_acts_do_not_single_out_an_archetype() {
        let data = GameData::load_workspace().unwrap();
        let (profiles, levers) = (&data.diplomacy.archetypes, &data.diplomacy.levers);
        // Taten ohne jeden Bezug zur Lage: Jede Wahl gleich wahrscheinlich.
        let mut leaders = [0usize; 4];
        let mut confident = 0;
        for seed in 0..12u64 {
            let mut rng = Rng::stream(seed, Domain::Diplomacy, 99, 3);
            let mut dossier = Dossier::default();
            for day in 0..120 {
                let occasion = occasion(&mut rng);
                let options = occasion.options().len();
                let option = ((rng.f64() * options as f64) as usize).min(options - 1);
                dossier.observe(&act(occasion, option, day), 1.0, profiles, levers);
                dossier.decay(levers);
            }
            let (leader, belief) = dossier.leading();
            leaders[leader.index()] += 1;
            if belief > 0.9 {
                confident += 1;
            }
        }
        // Zufall ist niemandes Handschrift: Keine Haltung wird fast sicher, oder die
        // Einschätzung bricht immer wieder als überholt zusammen.
        assert!(
            confident <= 6,
            "{confident} von 12 Dossiers sicher: {leaders:?}"
        );
    }

    #[test]
    fn decay_returns_toward_uniform() {
        let data = GameData::load_workspace().unwrap();
        let levers = &data.diplomacy.levers;
        let mut dossier = Dossier {
            logit: [0.0, -2.0, -2.0, -2.0],
            weight: 10.0,
            ..Dossier::default()
        };
        let first = dossier.belief()[0];
        for _ in 0..120 {
            dossier.decay(levers);
        }
        assert!(dossier.belief()[0] < first);
        assert!(dossier.weight < 3.0);
        assert!((Dossier::default().belief()[0] - 0.25).abs() < 1e-6);
    }
}

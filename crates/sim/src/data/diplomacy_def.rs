//! Hebel der Diplomatie und die vier Grundhaltungen aus `diplomacy.ron`.
//!
//! Die Grundhaltungen sind öffentliches Wissen: Jeder Beobachter misst die Taten einer
//! Gemeinschaft an denselben vier reinen Profilen. Wie stark eine Gemeinschaft welcher
//! Haltung folgt, weiß nur ihre eigene Führung (`ai::mind`).

use super::DataError;
use serde::Deserialize;

/// Reines Profil einer Grundhaltung: die Gewichte, mit denen sie Gelegenheiten bewertet.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ArchetypeDef {
    pub key: String,
    /// Deutsche Bezeichnung, kleingeschrieben („kooperativ“).
    pub label: String,
    /// Rüstung: Grundanteil des Unterhalts an den Einnahmen und Zuschlag je Bedrohung.
    pub arm0: f32,
    pub arm1: f32,
    /// Anteil der Flotte, den sie an die Grenze eines Ziels schiebt.
    pub forward: f32,
    /// Gewicht der Beute.
    pub grab: f32,
    /// Gewicht des Vorbeugens: zuschlagen oder sich binden, bevor ein anderer zu stark wird.
    pub prevent: f32,
    /// Gewicht der Verbindung mit anderen.
    pub merge: f32,
    /// Vertragstreue: wie schwer ein Bruch wiegt, und was andere ihr zutrauen.
    pub honor: f32,
    /// Streuung der Beteiligungen im Ausland.
    pub spread: f32,
    /// Offenheit für Verflechtung.
    pub open: f32,
    /// Geduld in Jahren: so lange sieht sie einem Aufsteiger zu.
    pub patience: f32,
    /// Vorsicht: wo in der Spanne einer Schätzung sie die fremde Flotte ansetzt (0 = unten).
    pub caution: f32,
}

impl ArchetypeDef {
    /// Mischung mehrerer Profile mit nichtnegativen Gewichten (Summe 1).
    pub fn blend(profiles: &[ArchetypeDef], weights: &[f32]) -> ArchetypeDef {
        let mix = |field: fn(&ArchetypeDef) -> f32| -> f32 {
            profiles
                .iter()
                .zip(weights)
                .map(|(profile, weight)| field(profile) * weight)
                .sum()
        };
        ArchetypeDef {
            key: "mischung".into(),
            label: "gemischt".into(),
            arm0: mix(|p| p.arm0),
            arm1: mix(|p| p.arm1),
            forward: mix(|p| p.forward),
            grab: mix(|p| p.grab),
            prevent: mix(|p| p.prevent),
            merge: mix(|p| p.merge),
            honor: mix(|p| p.honor),
            spread: mix(|p| p.spread),
            open: mix(|p| p.open),
            patience: mix(|p| p.patience),
            caution: mix(|p| p.caution),
        }
    }
}

/// Gewicht eines Indizes je Art der Gelegenheit.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
pub struct ClueWeights {
    /// Eröffnung: Zuschlagen, Binden.
    pub opening: f32,
    /// Antwort auf einen Vorschlag.
    pub answer: f32,
    /// Bedingungen: Kriegsziel, Friedensbedingungen.
    pub terms: f32,
    /// Stetige Größen wie die Rüstung.
    pub continuous: f32,
}

/// Hebel der Diplomatie. Tage und Monate im Kalender der Simulation.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct DiplomacyLevers {
    /// Mindestnutzen, ab dem eine Gelegenheit überhaupt verlockt.
    pub act: f32,
    /// Temperatur der Wahl: je kleiner, desto sicherer folgt sie dem höchsten Nutzen.
    pub temperature: f32,
    /// Restwahrscheinlichkeit jeder Wahl, wenn keine verlockt.
    pub tremble: f32,
    /// Gewicht der Grundhaltung, mit der eine Gemeinschaft beginnt, und das der übrigen.
    pub anchor_main: f32,
    pub anchor_rest: f32,
    /// Zug der Grundhaltung gegen den öffentlichen Druck.
    pub anchor_weight: f32,
    /// Anteil, um den die Gewichte monatlich ihrem Ziel folgen.
    pub drift: f32,
    /// Vorteil des Verteidigers im Kräftevergleich.
    pub def_adv: f32,
    /// Unter diesem Anteil an der Macht aller gilt eine Gemeinschaft als klein.
    pub minor_share: f32,
    /// Über diesem Anteil gilt eine Gemeinschaft als Vormacht.
    pub leader_share: f32,
    /// Glättung des Wachstums in Monaten.
    pub trend_months: f32,
    /// Anteil, um den die Spannung monatlich ihrem Ziel folgt.
    pub tension_adjust: f32,
    /// Monatliches Verblassen eines Grolls (0..1).
    pub grievance_decay: f32,
    /// Halbwertszeit eines Indizes in Monaten.
    pub half_life_months: f32,
    /// Ab dieser geglätteten Überraschung gilt die bisherige Einschätzung als überholt.
    pub surprise_break: f32,
    /// Kündigungsfrist eines Vertrags.
    pub notice_days: u64,
    /// Ein Vorschlag verfällt nach so vielen Tagen.
    pub proposal_days: u64,
    /// Nach einer Ablehnung ruht derselbe Vorschlag so lange.
    pub refusal_block_days: u64,
    /// Nach einem abgelehnten Frieden ruht ein neuer Friedensvorschlag so lange.
    pub peace_block_days: u64,
    /// Eine KI antwortet frühestens und spätestens so viele Tage nach dem Vorschlag.
    pub answer_days: (u64, u64),
    /// Kosten einer Vertragsstufe in der Reihenfolge von `TreatyKind::ALL`.
    pub rung_cost: [f32; 6],
    /// Vereinigung: Monate unter Unionsvertrag, Mindestverflechtung, Mindestnähe und wie
    /// lange alles gehalten haben muss.
    pub merge_union_months: u32,
    pub merge_tie: f32,
    pub merge_affinity: f32,
    pub merge_hold_months: u32,
    /// Preis, sich einer anderen Führung anzuschließen.
    pub lead_cost: f32,
    /// Monatlicher Tribut als Anteil der Einnahmen: unterworfen, unter Schutz, als
    /// Wiedergutmachung nach einem Krieg.
    pub vassal_tribute: f64,
    pub protectorate_tribute: f64,
    pub reparations: f64,
    /// Höchstdauer eines Tributs aus einem Frieden, in Jahren.
    pub terms_years: u8,
    /// Ab diesem Vorsprung im Krieg (0..100) darf der Sieger Tribut und Abtretungen
    /// fordern, ab dem zweiten die Unterwerfung.
    pub terms_tribute: f32,
    pub terms_subjugate: f32,
    /// Höchstens so viele Welten tritt ein Frieden ab.
    pub max_cede: usize,
    /// Nach so vielen Monaten Krieg diktiert der Vorsprung den Frieden.
    pub war_max_months: u32,
    /// Sind beide Seiten so kriegsmüde (0..100), endet der Krieg ohne Sieger.
    pub exhaustion: f64,
    /// Gewichte der Kriegsbilanz: genommene Bevölkerung, verlorene Flotte, Kriegsmüdigkeit.
    pub score_weights: (f32, f32, f32),
    /// Ein Krieg muss so viele Monate laufen, bevor die KI selbst Frieden anbietet.
    pub peace_min_months: u32,
    /// Kursschock für den, der einen Vertrag bricht.
    pub breach_shock: f64,
    /// Zuschlag auf die Forschungspunkte beider Seiten eines Forschungsvertrags.
    pub research_bonus: f64,
    pub clue_weight: ClueWeights,
    /// So viele Indizien behält ein Dossier, so viele Taten das öffentliche Protokoll.
    pub clues_kept: usize,
    pub acts_kept: usize,
    /// Stetige Indizien (Rüstung) erfassen. Erst sinnvoll, wenn die Führung ihre Rüstung
    /// nach ihrer Haltung bemisst.
    pub continuous_evidence: bool,
    /// Die KI eröffnet selbst (trägt Bindungen an). Kommt mit der strategischen Führung.
    pub ai_openings: bool,
    /// Die KI bietet im Krieg selbst Frieden an oder fordert Bedingungen.
    pub ai_peace_offers: bool,
    /// Geschlossene Grenzen: Flotten betreten fremdes Gebiet nur im eigenen Verbund, bei
    /// Verbündeten, im Krieg oder mit gewährter Durchfahrt. Falsch: Alle Grenzen stehen
    /// offen wie bisher, und gewährte Durchfahrt ändert nichts.
    pub closed_borders: bool,
}

/// Inhalt von `diplomacy.ron`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct DiplomacyData {
    pub levers: DiplomacyLevers,
    /// Genau vier Grundhaltungen in der Reihenfolge kooperativ, opportunistisch,
    /// misstrauisch, expansionistisch.
    pub archetypes: Vec<ArchetypeDef>,
}

/// Schlüssel der vier Grundhaltungen in ihrer festen Reihenfolge.
pub const ARCHETYPE_KEYS: [&str; 4] = [
    "kooperativ",
    "opportunistisch",
    "misstrauisch",
    "expansionistisch",
];

impl DiplomacyData {
    pub(super) fn validate(&self) -> Result<(), DataError> {
        let invalid = |message: String| DataError::Invalid {
            file: "diplomacy.ron",
            message,
        };
        let keys: Vec<&str> = self.archetypes.iter().map(|a| a.key.as_str()).collect();
        if keys != ARCHETYPE_KEYS {
            return Err(invalid(format!(
                "genau die vier Grundhaltungen {ARCHETYPE_KEYS:?} in dieser Reihenfolge erwartet, \
                 gefunden {keys:?}"
            )));
        }
        for archetype in &self.archetypes {
            let unit = [
                archetype.arm0,
                archetype.arm1,
                archetype.forward,
                archetype.grab,
                archetype.prevent,
                archetype.merge,
                archetype.honor,
                archetype.spread,
                archetype.open,
                archetype.caution,
            ];
            if unit.iter().any(|value| !(0.0..=1.0).contains(value)) {
                return Err(invalid(format!(
                    "`{}`: Gewichte müssen zwischen 0 und 1 liegen",
                    archetype.key
                )));
            }
            if !archetype.patience.is_finite() || archetype.patience <= 0.0 {
                return Err(invalid(format!(
                    "`{}`: patience muss positiv sein",
                    archetype.key
                )));
            }
        }
        let levers = &self.levers;
        let positive = [
            ("temperature", levers.temperature),
            ("trend_months", levers.trend_months),
            ("half_life_months", levers.half_life_months),
            ("def_adv", levers.def_adv),
            ("merge_tie", levers.merge_tie),
        ];
        for (name, value) in positive {
            if !value.is_finite() || value <= 0.0 {
                return Err(invalid(format!("{name} muss positiv sein")));
            }
        }
        let unit = [
            ("tremble", levers.tremble),
            ("anchor_main", levers.anchor_main),
            ("anchor_rest", levers.anchor_rest),
            ("drift", levers.drift),
            ("tension_adjust", levers.tension_adjust),
            ("grievance_decay", levers.grievance_decay),
            ("merge_affinity", levers.merge_affinity),
            ("minor_share", levers.minor_share),
            ("leader_share", levers.leader_share),
        ];
        for (name, value) in unit {
            if !(0.0..=1.0).contains(&value) {
                return Err(invalid(format!("{name} muss zwischen 0 und 1 liegen")));
            }
        }
        if (levers.anchor_main + 3.0 * levers.anchor_rest - 1.0).abs() > 1e-4 {
            return Err(invalid(
                "anchor_main + 3 · anchor_rest muss 1 ergeben".into(),
            ));
        }
        if levers.answer_days.0 > levers.answer_days.1
            || levers.answer_days.1 >= levers.proposal_days
        {
            return Err(invalid(
                "answer_days muss aufsteigen und vor dem Verfall (proposal_days) enden".into(),
            ));
        }
        if !(levers.terms_tribute < levers.terms_subjugate && levers.terms_subjugate <= 100.0) {
            return Err(invalid(
                "terms_tribute < terms_subjugate ≤ 100 erwartet".into(),
            ));
        }
        for share in [
            levers.vassal_tribute,
            levers.protectorate_tribute,
            levers.reparations,
            levers.research_bonus,
        ] {
            if !(0.0..=1.0).contains(&share) {
                return Err(invalid(
                    "Tribut- und Zuschlagsanteile zwischen 0 und 1".into(),
                ));
            }
        }
        if !(0.0..=100.0).contains(&levers.exhaustion) {
            return Err(invalid("exhaustion muss zwischen 0 und 100 liegen".into()));
        }
        if levers.clues_kept == 0 || levers.acts_kept == 0 || levers.war_max_months == 0 {
            return Err(invalid(
                "clues_kept, acts_kept und war_max_months müssen positiv sein".into(),
            ));
        }
        if levers
            .rung_cost
            .iter()
            .any(|cost| !(0.0..=5.0).contains(cost))
        {
            return Err(invalid("rung_cost zwischen 0 und 5 erwartet".into()));
        }
        Ok(())
    }
}

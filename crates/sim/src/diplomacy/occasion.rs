//! Gelegenheiten und die eine Wahlfunktion [`choice_probs`].
//!
//! Eine Gelegenheit ist eine Lage ([`Picture`]) und die Wahl, die sie stellt. Dieselbe
//! reine Funktion dient zweimal:
//!
//! - Die KI entscheidet mit ihr: mit ihrem gemischten, verborgenen Profil und ihrem
//!   eigenen Bild der Lage (`ai::mind`).
//! - Jeder Beobachter bewertet mit ihr eine beobachtete Tat: mit den vier reinen Profilen
//!   aus `diplomacy.ron` und dem öffentlichen Bild der Lage ([`super::evidence`]). Welche
//!   Grundhaltung die Tat am ehesten erklärt, verschiebt sein Dossier.
//!
//! Die Funktion liest keinen Zustand; sie kennt nur Profil, Hebel und Lage.

use crate::data::{ArchetypeDef, DiplomacyLevers};
use serde::{Deserialize, Serialize};

use super::relations::TreatyKind;

/// Die vier Grundhaltungen in der Reihenfolge von `diplomacy.ron`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Archetype {
    Cooperative,
    Opportunistic,
    Paranoid,
    Expansionist,
}

impl Archetype {
    pub const ALL: [Archetype; 4] = [
        Archetype::Cooperative,
        Archetype::Opportunistic,
        Archetype::Paranoid,
        Archetype::Expansionist,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(index: usize) -> Self {
        Self::ALL[index % 4]
    }

    pub fn label(self) -> &'static str {
        match self {
            Archetype::Cooperative => "kooperativ",
            Archetype::Opportunistic => "opportunistisch",
            Archetype::Paranoid => "misstrauisch",
            Archetype::Expansionist => "expansionistisch",
        }
    }
}

/// Lage einer Gemeinschaft `i` gegenüber einer anderen `j`, wie `i` sie sieht oder wie
/// sie öffentlich erscheint. Alle Größen sind aus dem Zustand der Welt berechnet, keine
/// aus einer verborgenen Haltung.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Picture {
    /// ln(P_i / P̂_j): Verhältnis der Macht (Wirtschaft, Flotte, Technik).
    pub ratio: f32,
    /// Jährliches Wachstum von j minus dem von i (Logarithmus der Einnahmen).
    pub trend_gap: f32,
    /// j ist Vormacht (Machtanteil über `leader_share`).
    pub other_leads: bool,
    /// i ist klein (Machtanteil unter `minor_share`).
    pub minor: bool,
    /// Machtanteile beider an allen, 0..1.
    pub share_me: f32,
    pub share_other: f32,
    /// Eigene Flotte (Bauwert) und die Spanne der geschätzten von j.
    pub own_fleet: f32,
    pub other_fleet: (f32, f32),
    /// Sprünge zwischen den Hauptwelten.
    pub hops: f32,
    /// Abhängigkeit von i von j, 0..1.
    pub dep: f32,
    /// Kulturelle Nähe, 0..1.
    pub aff: f32,
    /// Vertrauen in j: Σ Einschätzung · Vertragstreue der Grundhaltung.
    pub trust: f32,
    /// Eingeschätzte Anteile der Grundhaltungen von j, die drohen.
    pub belief_opp: f32,
    pub belief_exp: f32,
    /// Spannung des Paares, 0..100.
    pub tension: f32,
    /// Größte Bedrohung, die beide zugleich trifft, 0..1.
    pub common: f32,
    /// Größte Bedrohung von i durch irgendwen, 0..1.
    pub threat: f32,
    /// Not von i: Schrumpfen, ohne noch Welten besiedeln zu können, 0..1.
    pub want: f32,
    /// Geltende Verträge des Paares.
    pub rungs: f32,
    /// Zwischen beiden gilt eine Waffenruhe.
    pub truce: bool,
    /// Kriegsmüdigkeit von i, 0..100.
    pub weariness: f32,
    /// Vorsprung von i im Krieg gegen j, −100..100 (0 ohne Krieg).
    pub lead: f32,
}

/// Wie eine Gemeinschaft auf einen Vorschlag antwortet: Annahme oder Ablehnung.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Answering {
    Treaty(TreatyKind),
    /// Frieden: was sie selbst gibt (`cost`) und bekommt (`gain`), je grob als Anteil der
    /// eigenen Bevölkerung und Einnahmen bemessen.
    Peace {
        cost: f32,
        gain: f32,
    },
    /// Schutz: als künftiger Schützling oder als Schutzmacht.
    Protectorate {
        as_ward: bool,
    },
    /// Vereinigung: führt sie selbst oder die andere?
    Merge {
        i_lead: bool,
    },
    /// Anteilstausch.
    Swap,
}

/// Eine Gelegenheit zur Wahl.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Occasion {
    /// Eröffnung gegenüber einem anderen: Zuschlagen, Binden, Abwarten.
    Opening(Picture),
    /// Antwort auf einen Vorschlag: annehmen, ablehnen.
    Answer(Answering, Picture),
    /// Kriegsziel bei der Erklärung: Unterwerfung, begrenzt.
    Aim(Picture),
    /// Stetig: Anteil des Flottenunterhalts an den Einnahmen bei gegebener Bedrohung.
    /// `blur` weitet die Streuung, wenn der Beobachter wenig Einblick hat.
    Arming { threat: f32, blur: f32 },
}

/// Wahlen der Eröffnung.
pub const STRIKE: usize = 0;
pub const BIND: usize = 1;
pub const WAIT: usize = 2;
/// Wahlen der Antwort.
pub const ACCEPT: usize = 0;
pub const DECLINE: usize = 1;
/// Wahlen des Kriegsziels.
pub const SUBJUGATE: usize = 0;
pub const LIMITED: usize = 1;

impl Occasion {
    /// Bezeichnungen der Wahlen; leer für stetige Gelegenheiten.
    pub fn options(&self) -> &'static [&'static str] {
        match self {
            Occasion::Opening(_) => &["Zuschlagen", "Binden", "Abwarten"],
            Occasion::Answer(..) => &["Annahme", "Ablehnung"],
            Occasion::Aim(_) => &["Unterwerfung", "begrenzt"],
            Occasion::Arming { .. } => &[],
        }
    }
}

/// Fenster: Wie dringend ist es, vor dem Aufstieg von j zu handeln? 0..1,3.
fn window(picture: &Picture, profile: &ArchetypeDef) -> f32 {
    let mut window = 0.0;
    if picture.trend_gap > 1e-6 {
        let cross = picture.ratio.max(0.0) / picture.trend_gap;
        window = (1.0 - cross / profile.patience).clamp(0.0, 1.0);
    }
    if picture.other_leads {
        window += 0.3;
    }
    window
}

/// Beute: Je kleiner j im Verhältnis, desto weniger lohnt es, 0,3..1.
fn gain(picture: &Picture) -> f32 {
    (-picture.ratio).exp().clamp(0.3, 1.0)
}

/// Aussicht, einen Krieg gegen j zu gewinnen, 0..1. Die fremde Flotte setzt sie nach
/// ihrer Vorsicht in der Spanne der Schätzung an.
pub fn win_chance(picture: &Picture, profile: &ArchetypeDef, levers: &DiplomacyLevers) -> f32 {
    let (lo, hi) = picture.other_fleet;
    let foe = lo + profile.caution * (hi - lo).max(0.0);
    let ratio = levers.def_adv * foe * (1.0 + picture.hops / 8.0) / picture.own_fleet.max(1e-3);
    1.0 / (1.0 + ratio * ratio)
}

/// Nutzen, zuzuschlagen.
pub fn u_strike(picture: &Picture, profile: &ArchetypeDef, levers: &DiplomacyLevers) -> f32 {
    let lure = profile.grab * gain(picture) * (1.0 + picture.want)
        + profile.prevent * window(picture, profile);
    lure * win_chance(picture, profile, levers)
        - picture.dep
        // Not kennt kein Gebot: Wer nicht mehr wachsen kann, wiegt Verträge leichter.
        - profile.honor
            * (picture.rungs / 5.0 * (1.0 - 0.5 * picture.want)
                + if picture.truce { 1.0 } else { 0.0 })
        - picture.weariness / 100.0
}

/// Nutzen, sich zu binden.
pub fn u_bind(picture: &Picture, profile: &ArchetypeDef) -> f32 {
    let window = window(picture, profile);
    profile.merge * (window + picture.dep + picture.aff) * picture.trust
        + profile.prevent * picture.common
        - 0.6 * (1.0 - picture.want) * (1.0 - window.min(1.0))
}

/// Nutzen, einen Vorschlag anzunehmen (Ablehnung = 0).
pub fn u_accept(
    answering: Answering,
    picture: &Picture,
    profile: &ArchetypeDef,
    levers: &DiplomacyLevers,
) -> f32 {
    match answering {
        Answering::Treaty(kind) => {
            let treaty = u_bind(picture, profile) + picture.trust
                - levers.rung_cost[kind.index()] * (1.0 - profile.open);
            if kind == TreatyKind::Union {
                // Die Union führt in die Vereinigung: Sie schließt nur, wer diese auch
                // annähme, als führende oder als geführte Seite. Führen will nur, wem die
                // Vereinigung etwas gilt (`merge_lead_min`); wer das nicht will, muss bereit
                // sein, zu folgen.
                let i_lead = picture.share_me >= picture.share_other
                    && profile.merge >= levers.merge_lead_min;
                treaty.min(u_accept(Answering::Merge { i_lead }, picture, profile, levers) + picture.trust)
            } else {
                treaty
            }
        }
        Answering::Peace { cost, gain } => {
            let weary = picture.weariness / levers.exhaustion.max(1.0) as f32;
            0.8 * weary - picture.lead / 50.0 - cost + profile.grab * gain - 0.1
        }
        Answering::Protectorate { as_ward: true } => {
            let big = if picture.minor { 0.0 } else { 1.0 };
            picture.threat + profile.merge * picture.trust * picture.aff
                - 0.8 * (1.0 - picture.want) * big
        }
        Answering::Protectorate { as_ward: false } => {
            profile.grab + profile.merge * picture.aff - 0.2
        }
        Answering::Swap => u_bind(picture, profile) + profile.open - 0.5,
        Answering::Merge { i_lead } => {
            let follow = if i_lead { 0.0 } else { 1.0 };
            let weight = picture.share_me / (picture.share_me + picture.share_other).max(1e-6);
            // Der Preis der Eigenständigkeit schwindet mit der Not, bis auf die Hälfte, aber
            // nur für die, denen Verbindung etwas gilt: Wer wenig auf sie gibt, sucht in
            // der Not eher Beute als einen Herrn.
            u_bind(picture, profile)
                - levers.lead_cost
                    * follow
                    * weight
                    * (1.0 - window(picture, profile).min(1.0))
                    * (1.0 - 0.5 * picture.want * profile.merge.clamp(0.0, 1.0))
        }
    }
}

/// Nutzen, die Unterwerfung zum Kriegsziel zu machen (begrenzt = 0).
pub fn u_subjugate(picture: &Picture, profile: &ArchetypeDef) -> f32 {
    profile.grab * gain(picture) * (1.0 + picture.want) + profile.prevent * window(picture, profile)
        - 0.5
}

/// Softmax über Nutzen mit der Temperatur der Hebel; jede Wahl behält mindestens
/// `tremble`.
fn softmax(utilities: &[f32], levers: &DiplomacyLevers) -> Vec<f32> {
    let temperature = levers.temperature.max(1e-3);
    let top = utilities.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let exp: Vec<f32> = utilities
        .iter()
        .map(|u| ((u - top) / temperature).exp())
        .collect();
    let sum: f32 = exp.iter().sum();
    let n = utilities.len() as f32;
    let tremble = levers.tremble.min(0.5 / n);
    exp.iter()
        .map(|e| tremble + (1.0 - n * tremble) * e / sum)
        .collect()
}

/// Wahrscheinlichkeit jeder Wahl einer Gelegenheit für ein Profil. Für stetige
/// Gelegenheiten leer (siehe [`log_likelihood`]).
pub fn choice_probs(
    profile: &ArchetypeDef,
    occasion: &Occasion,
    levers: &DiplomacyLevers,
) -> Vec<f32> {
    match occasion {
        Occasion::Opening(picture) => {
            let strike = u_strike(picture, profile, levers);
            let bind = u_bind(picture, profile);
            if strike < levers.act && bind < levers.act {
                // Nichts verlockt: Abwarten, nur das Zittern hält jede Tür offen.
                let tremble = levers.tremble.min(0.25);
                vec![tremble, tremble, 1.0 - 2.0 * tremble]
            } else {
                softmax(&[strike, bind, 0.0], levers)
            }
        }
        Occasion::Answer(answering, picture) => softmax(
            &[u_accept(*answering, picture, profile, levers), 0.0],
            levers,
        ),
        Occasion::Aim(picture) => softmax(&[u_subjugate(picture, profile), 0.0], levers),
        Occasion::Arming { .. } => Vec::new(),
    }
}

/// Was an einer Gelegenheit beobachtet wurde.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Observed {
    /// Die gewählte Wahl (Stelle in [`Occasion::options`]).
    Option(usize),
    /// Ein gemessener Wert einer stetigen Gelegenheit.
    Value(f32),
}

/// Logarithmus der Wahrscheinlichkeit (bei stetigen Gelegenheiten der Dichte bis auf eine
/// Konstante), dass ein Profil das Beobachtete hervorbringt.
pub fn log_likelihood(
    profile: &ArchetypeDef,
    occasion: &Occasion,
    observed: Observed,
    levers: &DiplomacyLevers,
) -> f32 {
    match (occasion, observed) {
        (Occasion::Arming { threat, blur }, Observed::Value(value)) => {
            let mean = profile.arm0 + profile.arm1 * threat;
            let spread = 0.08 + blur.max(0.0);
            -(value - mean).powi(2) / (2.0 * spread * spread)
        }
        (_, Observed::Option(option)) => choice_probs(profile, occasion, levers)
            .get(option)
            .map_or(f32::NEG_INFINITY, |p| p.max(1e-9).ln()),
        _ => f32::NEG_INFINITY,
    }
}

/// Gründe einer Ablehnung, wie sie der Ablehnende nennt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeclineReason {
    Distrust,
    TooLittleEntanglement,
    CulturalDistance,
    RungTooHigh,
    Ahead,
    TermsTooHarsh,
    Sovereignty,
}

impl DeclineReason {
    pub fn label(self) -> &'static str {
        match self {
            DeclineReason::Distrust => "Misstrauen",
            DeclineReason::TooLittleEntanglement => "zu wenig Verflechtung",
            DeclineReason::CulturalDistance => "kulturelle Ferne",
            DeclineReason::RungTooHigh => "Stufe zu hoch",
            DeclineReason::Ahead => "wir liegen vorn",
            DeclineReason::TermsTooHarsh => "Bedingungen zu hart",
            DeclineReason::Sovereignty => "Eigenständigkeit",
        }
    }
}

/// Der genannte Grund einer Ablehnung: der schwerste Posten gegen die Annahme unter dem
/// gleichmäßig gemischten Profil und dem öffentlichen Bild. So nennt jede Haltung
/// denselben Grund, und eine Ablehnung verrät über die Haltung nichts, was die Ablehnung
/// selbst nicht schon verrät.
pub fn decline_reason(
    answering: Answering,
    picture: &Picture,
    uniform: &ArchetypeDef,
    levers: &DiplomacyLevers,
) -> DeclineReason {
    let window = window(picture, uniform).min(1.0);
    let sovereignty = -0.6 * (1.0 - picture.want) * (1.0 - window);
    let terms: Vec<(DeclineReason, f32)> = match answering {
        Answering::Treaty(kind) => vec![
            (DeclineReason::Distrust, picture.trust - 0.7),
            (
                DeclineReason::TooLittleEntanglement,
                uniform.merge * (window + picture.dep) * picture.trust + sovereignty,
            ),
            (
                DeclineReason::CulturalDistance,
                uniform.merge * (picture.aff - 0.8) * picture.trust,
            ),
            (
                DeclineReason::RungTooHigh,
                -levers.rung_cost[kind.index()] * (1.0 - uniform.open),
            ),
        ],
        Answering::Peace { cost, .. } => {
            let mut terms = vec![(DeclineReason::Ahead, -picture.lead / 50.0)];
            // Wer nichts gibt, nennt keine harten Bedingungen.
            if cost > 0.0 {
                terms.push((DeclineReason::TermsTooHarsh, -cost));
            }
            terms
        }
        Answering::Swap => vec![
            (DeclineReason::Distrust, picture.trust - 0.7),
            (
                DeclineReason::TooLittleEntanglement,
                uniform.merge * (window + picture.dep) * picture.trust + sovereignty,
            ),
            (DeclineReason::Sovereignty, uniform.open - 0.5),
        ],
        Answering::Protectorate { .. } | Answering::Merge { .. } => vec![
            (DeclineReason::Sovereignty, sovereignty),
            (DeclineReason::Distrust, picture.trust - 0.7),
            (
                DeclineReason::CulturalDistance,
                uniform.merge * (picture.aff - 0.8),
            ),
        ],
    };
    terms
        .into_iter()
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(DeclineReason::Distrust, |(reason, _)| reason)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::GameData;

    fn data() -> GameData {
        GameData::load_workspace().unwrap()
    }

    fn calm() -> Picture {
        Picture {
            ratio: 0.0,
            share_me: 0.25,
            share_other: 0.25,
            own_fleet: 100.0,
            other_fleet: (80.0, 120.0),
            hops: 3.0,
            dep: 0.1,
            aff: 0.8,
            trust: 0.6,
            tension: 30.0,
            ..Picture::default()
        }
    }

    #[test]
    fn probabilities_are_distributions_and_never_zero() {
        let data = data();
        let levers = &data.diplomacy.levers;
        let mut picture = calm();
        for ratio in [-2.0, -0.5, 0.0, 0.5, 2.0] {
            for dep in [0.0, 0.5, 1.0] {
                picture.ratio = ratio;
                picture.dep = dep;
                for occasion in [
                    Occasion::Opening(picture),
                    Occasion::Answer(Answering::Treaty(TreatyKind::Trade), picture),
                    Occasion::Answer(
                        Answering::Peace {
                            cost: 0.3,
                            gain: 0.0,
                        },
                        picture,
                    ),
                    Occasion::Answer(Answering::Merge { i_lead: false }, picture),
                    Occasion::Aim(picture),
                ] {
                    for profile in &data.diplomacy.archetypes {
                        let probs = choice_probs(profile, &occasion, levers);
                        assert_eq!(probs.len(), occasion.options().len());
                        let sum: f32 = probs.iter().sum();
                        assert!((sum - 1.0).abs() < 1e-4, "{occasion:?}: {probs:?}");
                        assert!(probs.iter().all(|p| *p > 0.0 && p.is_finite()));
                    }
                }
            }
        }
    }

    #[test]
    fn drives_point_the_right_way() {
        let data = data();
        let levers = &data.diplomacy.levers;
        let greedy = &data.diplomacy.archetypes[Archetype::Expansionist.index()];
        let base = Picture {
            ratio: 0.5,
            ..calm()
        };
        let strike = |picture: Picture| u_strike(&picture, greedy, levers);
        // Ein Aufsteiger öffnet ein Fenster.
        assert!(
            strike(Picture {
                trend_gap: 0.1,
                ..base
            }) > strike(base)
        );
        // Abhängigkeit und Verträge halten zurück.
        assert!(strike(Picture { dep: 0.6, ..base }) < strike(base));
        assert!(strike(Picture { rungs: 3.0, ..base }) < strike(base));
        // Eine überlegene fremde Flotte schreckt ab.
        assert!(
            strike(Picture {
                other_fleet: (400.0, 600.0),
                ..base
            }) < strike(base)
        );
        let kind = &data.diplomacy.archetypes[Archetype::Cooperative.index()];
        let bind = |picture: Picture| u_bind(&picture, kind);
        assert!(bind(Picture { trust: 0.9, ..base }) > bind(base));
        assert!(bind(Picture { dep: 0.5, ..base }) > bind(base));
        assert!(bind(Picture { aff: 1.0, ..base }) > bind(Picture { aff: 0.3, ..base }));
        assert!(
            bind(Picture {
                common: 0.8,
                ..base
            }) > bind(base)
        );
    }

    #[test]
    fn archetypes_answer_a_trade_offer_differently() {
        let data = data();
        let levers = &data.diplomacy.levers;
        let occasion = Occasion::Answer(Answering::Treaty(TreatyKind::Trade), calm());
        let accept = |archetype: Archetype| {
            choice_probs(
                &data.diplomacy.archetypes[archetype.index()],
                &occasion,
                levers,
            )[ACCEPT]
        };
        assert!(accept(Archetype::Cooperative) > 0.8);
        assert!(accept(Archetype::Cooperative) > accept(Archetype::Paranoid));
        assert!(accept(Archetype::Opportunistic) > accept(Archetype::Expansionist));
    }

    #[test]
    fn a_leader_declines_white_peace_a_loser_accepts_it() {
        let data = data();
        let levers = &data.diplomacy.levers;
        let profile = &data.diplomacy.archetypes[Archetype::Opportunistic.index()];
        let white = Answering::Peace {
            cost: 0.0,
            gain: 0.0,
        };
        let ahead = Picture {
            lead: 40.0,
            weariness: 20.0,
            ..calm()
        };
        let behind = Picture {
            lead: -40.0,
            weariness: 20.0,
            ..calm()
        };
        let p = |picture: Picture| choice_probs(profile, &Occasion::Answer(white, picture), levers);
        assert!(p(ahead)[ACCEPT] < 0.05);
        assert!(p(behind)[ACCEPT] > 0.95);
        assert_eq!(
            decline_reason(white, &ahead, profile, levers),
            DeclineReason::Ahead
        );
    }

    #[test]
    fn arming_likelihood_peaks_at_the_profile_mean() {
        let data = data();
        let levers = &data.diplomacy.levers;
        let paranoid = &data.diplomacy.archetypes[Archetype::Paranoid.index()];
        let occasion = Occasion::Arming {
            threat: 0.5,
            blur: 0.0,
        };
        let mean = paranoid.arm0 + 0.5 * paranoid.arm1;
        let at = |value: f32| log_likelihood(paranoid, &occasion, Observed::Value(value), levers);
        assert!(at(mean) > at(mean + 0.1));
        assert!(at(mean) > at(mean - 0.1));
        assert!(at(mean).abs() < 1e-6);
    }
}

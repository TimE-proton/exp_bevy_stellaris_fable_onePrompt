//! Beziehung zweier Gemeinschaften: Verträge, Krieg, Embargo, Durchfahrt, Tribut, Groll,
//! Spannung und Verflechtung.
//!
//! Je Paar gibt es höchstens eine [`Relation`], abgelegt unter dem Paar mit der kleineren
//! Nummer zuerst. Felder, die je Seite gelten (`[T; 2]`), führen an Stelle 0 die kleinere
//! Nummer ([`side`]).

use crate::ids::{PlanetId, PolityId};
use serde::{Deserialize, Serialize};

/// Paar mit der kleineren Nummer zuerst.
pub fn pair(a: PolityId, b: PolityId) -> (PolityId, PolityId) {
    if a <= b { (a, b) } else { (b, a) }
}

/// Stelle einer Gemeinschaft in den Feldern je Seite ihres Paares mit `other`.
pub fn side(polity: PolityId, other: PolityId) -> usize {
    usize::from(polity > other)
}

/// Die sechs Vertragsstufen, von der lockersten zur engsten.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum TreatyKind {
    /// Handel: kein Embargo, gegenseitige Offenheit der Märkte.
    Trade,
    /// Nichtangriff: Wer trotzdem Krieg erklärt, bricht den Vertrag.
    NonAggression,
    /// Forschung: Zuschlag auf die Forschungspunkte beider Seiten.
    Research,
    /// Investition: geschützte Beteiligungen; eine Enteignung ist Vertragsbruch.
    Investment,
    /// Verteidigung: Durchfahrt und Versorgung in den Häfen des anderen.
    Defence,
    /// Union: Vorstufe der Vereinigung.
    Union,
}

impl TreatyKind {
    pub const ALL: [TreatyKind; 6] = [
        TreatyKind::Trade,
        TreatyKind::NonAggression,
        TreatyKind::Research,
        TreatyKind::Investment,
        TreatyKind::Defence,
        TreatyKind::Union,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn label(self) -> &'static str {
        match self {
            TreatyKind::Trade => "Handel",
            TreatyKind::NonAggression => "Nichtangriff",
            TreatyKind::Research => "Forschung",
            TreatyKind::Investment => "Investition",
            TreatyKind::Defence => "Verteidigung",
            TreatyKind::Union => "Union",
        }
    }

    /// Verträge, die bestehen müssen, bevor dieser geschlossen werden kann.
    pub fn requires(self) -> &'static [TreatyKind] {
        match self {
            TreatyKind::Trade => &[],
            TreatyKind::NonAggression | TreatyKind::Research | TreatyKind::Investment => {
                &[TreatyKind::Trade]
            }
            TreatyKind::Defence => &[TreatyKind::NonAggression],
            TreatyKind::Union => &[TreatyKind::Investment, TreatyKind::Defence],
        }
    }

    /// Ein Krieg zwischen den Vertragspartnern bricht diesen Vertrag.
    pub fn forbids_war(self) -> bool {
        matches!(
            self,
            TreatyKind::NonAggression | TreatyKind::Defence | TreatyKind::Union
        )
    }
}

/// Ein geschlossener Vertrag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Treaty {
    pub since_day: u64,
    /// Gekündigt: endet an diesem Tag. Bis dahin gilt er.
    pub ends_day: Option<u64>,
}

/// Wozu ein Krieg geführt wird.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WarAim {
    /// Begrenzt: Tribut und Abtretungen, keine Unterwerfung.
    #[default]
    Limited,
    /// Unterwerfung: Der Gegner soll zum Vasallen werden.
    Subjugation,
}

impl WarAim {
    pub fn label(self) -> &'static str {
        match self {
            WarAim::Limited => "begrenzt",
            WarAim::Subjugation => "Unterwerfung",
        }
    }
}

/// Ein laufender Krieg zwischen zwei Häuptern, mit den Zahlen seit seinem Beginn. Ob
/// Krieg herrscht, steht in `Diplomacy::wars`; hier stehen nur seine Umstände. Ein Krieg,
/// der von Hand in die Kriegsliste gesetzt wurde, hat keinen Eintrag: Er gilt dann als
/// begrenzt, ohne Angreifer und ohne Bilanz.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct War {
    pub attacker: PolityId,
    pub aim: WarAim,
    pub since_day: u64,
    /// Je Seite: Bevölkerung (Mio.) und Bauwert der Flotten (Credits) zu Kriegsbeginn.
    pub pop0: [f64; 2],
    pub fleet0: [f64; 2],
    /// Je Seite: genommene Bevölkerung (Mio.) und verlorener Bauwert der Flotten.
    pub taken: [f64; 2],
    pub lost: [f64; 2],
    /// Welten, die eine Seite der anderen genommen hat, in der Reihenfolge der Einnahme.
    pub conquered: Vec<(PolityId, PlanetId)>,
    /// Je Seite: Sie hat die Hauptwelt der anderen genommen.
    #[serde(default)]
    pub took_capital: [bool; 2],
}

/// Laufender Tribut: `payer` zahlt dem anderen jeden Monat `share` seiner Einnahmen.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tribute {
    pub payer: PolityId,
    pub share: f64,
    /// Letzter Tag, an dem noch gezahlt wird.
    pub until_day: u64,
}

/// Benannte Gründe der Spannung zwischen zweien.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TensionReason {
    Base,
    War,
    Embargo,
    Breach,
    Conqueror,
    CulturalDistance,
    Treaties,
    Entanglement,
}

impl TensionReason {
    pub fn label(self) -> &'static str {
        match self {
            TensionReason::Base => "Nachbarschaft",
            TensionReason::War => "Krieg",
            TensionReason::Embargo => "Embargo",
            TensionReason::Breach => "Vertragsbruch",
            TensionReason::Conqueror => "Eroberer",
            TensionReason::CulturalDistance => "Kulturelle Ferne",
            TensionReason::Treaties => "Verträge",
            TensionReason::Entanglement => "Verflechtung",
        }
    }
}

/// Beziehung eines Paares. Alle Felder sind öffentlich bekannt.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Relation {
    /// Verträge nach Art, in der Reihenfolge von [`TreatyKind::ALL`].
    pub treaties: [Option<Treaty>; 6],
    /// Umstände des laufenden Krieges, wenn einer herrscht.
    pub war: Option<War>,
    /// Je Seite: Diese Seite hat ein Embargo gegen die andere verhängt.
    pub embargo: [bool; 2],
    /// Je Seite: Diese Seite gewährt den Flotten der anderen Durchfahrt.
    pub transit: [bool; 2],
    pub tribute: Option<Tribute>,
    /// Je Seite: Groll dieser Seite über einen Vertragsbruch der anderen, 0..1.
    pub grievance: [f32; 2],
    /// Spannung 0..100 und ihre Gründe beim letzten Monatsabschluss.
    pub tension: f32,
    pub reasons: Vec<(TensionReason, f32)>,
    /// Je Seite: Anteil am Marktwert der Betriebe der anderen, den Halter dieser Seite
    /// besitzen.
    pub stake: [f32; 2],
    /// Je Seite: Abhängigkeit vom anderen, 0..1 (Handel und Beteiligungen).
    pub dep: [f32; 2],
    /// Gegenseitige Verflechtung, größenneutral: kleinere der beiden Beteiligungen am
    /// Marktwert der kleineren Wirtschaft.
    pub tie: f32,
    /// Monate in Folge, in denen alle Bedingungen einer Vereinigung erfüllt waren.
    pub merge_months: u32,
}

impl Relation {
    pub fn treaty(&self, kind: TreatyKind) -> Option<Treaty> {
        self.treaties[kind.index()]
    }

    pub fn has(&self, kind: TreatyKind) -> bool {
        self.treaties[kind.index()].is_some()
    }

    /// Zahl der geltenden Verträge.
    pub fn rungs(&self) -> usize {
        self.treaties.iter().flatten().count()
    }

    /// Hat eine der beiden Seiten ein Embargo verhängt?
    pub fn any_embargo(&self) -> bool {
        self.embargo[0] || self.embargo[1]
    }

    /// Steht nichts in der Beziehung, was über die Messung hinausgeht? Dann darf sie fallen.
    pub fn is_blank(&self) -> bool {
        self.treaties.iter().all(Option::is_none)
            && self.war.is_none()
            && !self.any_embargo()
            && !self.transit[0]
            && !self.transit[1]
            && self.tribute.is_none()
            && self.grievance == [0.0; 2]
    }
}

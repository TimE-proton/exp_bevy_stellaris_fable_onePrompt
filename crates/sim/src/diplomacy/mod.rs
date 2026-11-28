//! Beziehungen zwischen den Gemeinschaften.
//!
//! Krieg und Frieden, wer wem untersteht, Verträge, Vorschläge, Embargo und Durchfahrt,
//! Spannung und Verflechtung, und was eine Gemeinschaft aus den Taten der anderen über
//! deren verborgene Haltung schließt.
//!
//! Begriffe: Eine Gemeinschaft ist *frei*, solange sie kein Oberhaupt über sich hat. Ihr
//! *Haupt* ist das Oberhaupt, sonst sie selbst. Ein *Verbund* ist ein Haupt mit allen, die
//! ihm unterstehen. Kriege und Verträge bestehen nur zwischen Häuptern; wer untersteht,
//! teilt die Kriege und Verträge seines Haupts, und im selben Verbund führt niemand Krieg
//! gegeneinander. *Verbündet* sind zwei, die zum selben Verbund gehören oder deren Häupter
//! einen Verteidigungs- oder Unionsvertrag verbindet ([`Diplomacy::allied`]).
//!
//! Nach einem Friedensschluss gilt zwischen den beiden eine Waffenruhe
//! (`military.truce_days`): So lange nimmt keiner dem anderen gegenüber eine neue
//! Kriegserklärung vor. Frieden und sofort wieder Krieg wäre sonst ein Mittel, ein
//! laufendes Gefecht von vorn zu beginnen oder eine Sammelfrist zu umgehen.
//!
//! Teile:
//! - [`relations`]: die Beziehung eines Paares ([`Relation`]): Verträge nach Art, Krieg mit
//!   Angreifer, Ziel und Bilanz, Embargo und Durchfahrt je Seite, Tribut, Groll, Spannung
//!   mit Gründen, Beteiligungen, Abhängigkeit und Verflechtung.
//! - [`treaties`]: Vertragsstufen, Kündigung, Vertragsbruch.
//! - [`proposals`]: Vorschläge ([`Deal`]) mit Annahme, Ablehnung, Rückzug und Verfall, und
//!   was eine Annahme vollzieht: Vertrag, Frieden mit Bedingungen, Schutz, Vereinigung.
//! - [`war`]: Kriegsziel, Bilanz, zulässige Friedensbedingungen, Diktat.
//! - [`survey`]: der Monatsabschluss: Macht, Wachstum, Beteiligungen, Abhängigkeit,
//!   Spannung, Tribut, Fortschritt zur Vereinigung.
//! - [`occasion`]: Gelegenheiten und die eine Wahlfunktion `choice_probs`.
//! - [`evidence`]: öffentliche Taten, Dossiers und Indizien.
//! - [`sight`]: Schätzungen fremder Flotten und Technik, und das Bild einer Lage.
//! - [`view`]: was eine Gemeinschaft über eine andere wissen darf (für die Oberfläche).
//!
//! Die verborgene Haltung einer KI-Gemeinschaft liegt in `ai::mind` und ist von hier aus
//! nicht lesbar: Kein Teil dieses Moduls nimmt sie entgegen. Beobachter sehen nur Taten.

pub mod evidence;
pub mod occasion;
pub mod proposals;
pub mod relations;
pub mod sight;
pub mod survey;
pub mod treaties;
pub mod view;
pub mod war;

pub use evidence::{Act, ActKind, Audience, Clue, Dossier};
pub use occasion::{
    Answering, Archetype, DeclineReason, Observed, Occasion, Picture, choice_probs,
};
pub use proposals::{Deal, PeaceTerms, Proposal, ProposalRecord, ProposalStatus};
pub use relations::{Relation, TensionReason, Treaty, TreatyKind, Tribute, War, WarAim};
pub use survey::SurveyEntry;

use crate::chronicle::{Chronicle, ChronicleKind};
use crate::clock::TICKS_PER_DAY;
use crate::data::GameData;
use crate::ids::PolityId;
use crate::polity::Polities;
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub use relations::pair;

/// Stellung einer Gemeinschaft: frei oder einem Oberhaupt unterstellt.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Standing {
    /// Oberhaupt; `None` = frei.
    pub overlord: Option<PolityId>,
    /// Die Unterstellung war gewollt, nicht erzwungen.
    pub voluntary: bool,
    /// Tag, seit dem die Stellung gilt.
    pub since_day: u64,
}

/// Kultur ohne Vorlage: die Mitte in allen drei Zügen.
pub const NEUTRAL_CULTURE: [f32; 3] = [0.5; 3];

#[derive(Resource, Clone, Debug, Default, Serialize, Deserialize)]
pub struct Diplomacy {
    /// Kriege als geordnete Paare (kleinere ID zuerst), nur zwischen Häuptern.
    pub wars: BTreeSet<(PolityId, PolityId)>,
    /// Waffenruhen nach einem Friedensschluss: je Paar (kleinere ID zuerst) der erste
    /// Tag, an dem wieder ein Krieg erklärt werden kann.
    #[serde(default)]
    pub truces: BTreeMap<(PolityId, PolityId), u64>,
    /// Stellung je Gemeinschaft, in der Reihenfolge von `Polities::list`. Fehlt ein
    /// Eintrag, gilt die Gemeinschaft als frei.
    #[serde(default)]
    pub standing: Vec<Standing>,
    /// Gelebte Kultur je Gemeinschaft: Offenheit, Gemeinsinn, Wehrhaftigkeit, je 0..1.
    #[serde(default)]
    pub culture: Vec<[f32; 3]>,
    /// Beziehungen je Paar (kleinere Nummer zuerst). Ein Paar ohne Eintrag hat keine
    /// Verträge, kein Embargo, keine Durchfahrt und keinen Groll.
    #[serde(default)]
    pub relations: BTreeMap<(PolityId, PolityId), Relation>,
    /// Offene Vorschläge in der Reihenfolge ihres Eingangs.
    #[serde(default)]
    pub proposals: Vec<Proposal>,
    /// Erledigte Vorschläge, älteste zuerst (höchstens [`proposals::HISTORY`]).
    #[serde(default)]
    pub history: VecDeque<ProposalRecord>,
    /// Nächste freie Nummer eines Vorschlags.
    #[serde(default)]
    pub next_proposal: u32,
    /// Nach einer Ablehnung: (von, an, Art des Vorschlags) → erster Tag, an dem derselbe
    /// Vorschlag wieder gemacht werden kann.
    #[serde(default)]
    pub blocked: BTreeMap<(PolityId, PolityId, u8), u64>,
    /// Öffentliches Protokoll diplomatischer Taten, älteste zuerst.
    #[serde(default)]
    pub acts: VecDeque<Act>,
    /// Taten dieses Tages, die die Beobachter noch nicht bewertet haben.
    #[serde(default)]
    pub pending_acts: Vec<Act>,
    /// Dossier je (Beobachter, Beobachteter).
    #[serde(default)]
    pub dossiers: BTreeMap<(PolityId, PolityId), Dossier>,
    /// Öffentliches Bild je Gemeinschaft: aus den öffentlichen Taten allein.
    #[serde(default)]
    pub public_face: BTreeMap<PolityId, Dossier>,
    /// Monatliche Messung je Gemeinschaft (Stelle = Nummer).
    #[serde(default)]
    pub survey: Vec<SurveyEntry>,
    /// Gefechtsbilanzen, die vor diesem Takt endeten, sind in die Kriege eingerechnet.
    #[serde(default)]
    pub battles_counted_until: u64,
    /// Jahr, in dem eine Gemeinschaft einer anderen zuletzt eine Bindung angetragen hat:
    /// Nur der erste Antrag eines Jahres zählt als Indiz.
    #[serde(default)]
    pub bind_year: BTreeMap<(PolityId, PolityId), u64>,
}

impl Diplomacy {
    /// Oberhaupt einer Gemeinschaft; `None`, wenn sie frei ist.
    pub fn overlord_of(&self, polity: PolityId) -> Option<PolityId> {
        self.standing
            .get(polity.index())
            .and_then(|standing| standing.overlord)
    }

    /// Haupt einer Gemeinschaft: ihr Oberhaupt, sonst sie selbst.
    pub fn head(&self, polity: PolityId) -> PolityId {
        self.overlord_of(polity).unwrap_or(polity)
    }

    /// Frei ist, wer niemandem untersteht.
    pub fn is_free(&self, polity: PolityId) -> bool {
        self.overlord_of(polity).is_none()
    }

    /// Gemeinschaften, die `polity` unterstehen, aufsteigend.
    pub fn wards_of(&self, polity: PolityId) -> impl Iterator<Item = PolityId> + '_ {
        self.standing
            .iter()
            .enumerate()
            .filter(move |(_, standing)| standing.overlord == Some(polity))
            .map(|(index, _)| PolityId::from_index(index))
    }

    /// Verbund einer Gemeinschaft: ihr Haupt zuerst, dann alle, die ihm unterstehen,
    /// aufsteigend.
    pub fn bloc(&self, polity: PolityId) -> Vec<PolityId> {
        let head = self.head(polity);
        std::iter::once(head).chain(self.wards_of(head)).collect()
    }

    /// Kultur einer Gemeinschaft; ohne Eintrag die neutrale Mitte.
    pub fn culture_of(&self, polity: PolityId) -> [f32; 3] {
        self.culture
            .get(polity.index())
            .copied()
            .unwrap_or(NEUTRAL_CULTURE)
    }

    /// Kulturelle Nähe zweier Gemeinschaften, 0..1: eins minus dem Abstand ihrer Kulturen,
    /// gemessen am größten möglichen. 1 = gleich, 0 = entgegengesetzt in allen drei Zügen.
    pub fn affinity(&self, a: PolityId, b: PolityId) -> f32 {
        let (first, second) = (self.culture_of(a), self.culture_of(b));
        let distance = first
            .iter()
            .zip(&second)
            .map(|(x, y)| (x - y) * (x - y))
            .sum::<f32>()
            .sqrt();
        (1.0 - distance / 3.0f32.sqrt()).clamp(0.0, 1.0)
    }

    /// Herrscht zwischen den Verbünden der beiden Krieg? Wer untersteht, teilt die Kriege
    /// seines Haupts; im selben Verbund herrscht nie Krieg.
    pub fn at_war(&self, a: PolityId, b: PolityId) -> bool {
        let (a, b) = (self.head(a), self.head(b));
        a != b && self.wars.contains(&pair(a, b))
    }

    /// Alle Gemeinschaften, mit denen `polity` im Krieg liegt: die verfeindeten Häupter,
    /// jedes gefolgt von denen, die ihm unterstehen.
    pub fn enemies_of(&self, polity: PolityId) -> impl Iterator<Item = PolityId> + '_ {
        let own = self.head(polity);
        self.wars
            .iter()
            .filter_map(move |(a, b)| {
                if *a == own {
                    Some(*b)
                } else if *b == own {
                    Some(*a)
                } else {
                    None
                }
            })
            .flat_map(move |enemy| std::iter::once(enemy).chain(self.wards_of(enemy)))
    }

    pub fn is_at_war(&self, polity: PolityId) -> bool {
        self.enemies_of(polity).next().is_some()
    }

    /// Erster Tag, an dem zwischen den beiden wieder ein Krieg erklärt werden kann, wenn
    /// nach einem Friedensschluss noch eine Waffenruhe gilt.
    pub fn truce_until(&self, a: PolityId, b: PolityId, day: u64) -> Option<u64> {
        self.truces
            .get(&pair(a, b))
            .copied()
            .filter(|until| *until > day)
    }

    /// Beziehung zweier Gemeinschaften, wenn es einen Eintrag gibt.
    pub fn relation(&self, a: PolityId, b: PolityId) -> Option<&Relation> {
        self.relations.get(&pair(a, b))
    }

    /// Beziehung zum Fortschreiben; legt den Eintrag an, wenn es keinen gibt.
    pub fn relation_mut(&mut self, a: PolityId, b: PolityId) -> &mut Relation {
        self.relations.entry(pair(a, b)).or_default()
    }

    /// Besteht zwischen den Häuptern der beiden der Vertrag?
    pub fn has_treaty(&self, a: PolityId, b: PolityId, kind: TreatyKind) -> bool {
        let (a, b) = (self.head(a), self.head(b));
        a != b
            && self
                .relation(a, b)
                .is_some_and(|relation| relation.has(kind))
    }

    /// Verbündet: im selben Verbund, oder die Häupter verbindet Verteidigung oder Union.
    pub fn allied(&self, a: PolityId, b: PolityId) -> bool {
        self.head(a) == self.head(b)
            || self.has_treaty(a, b, TreatyKind::Defence)
            || self.has_treaty(a, b, TreatyKind::Union)
    }

    /// Hat eine der beiden Seiten (über ihre Häupter) ein Embargo gegen die andere
    /// verhängt?
    pub fn embargoed(&self, a: PolityId, b: PolityId) -> bool {
        let (a, b) = (self.head(a), self.head(b));
        a != b && self.relation(a, b).is_some_and(Relation::any_embargo)
    }

    /// Gewährt das Haupt von `owner` dem Haupt von `mover` Durchfahrt?
    pub fn grants_transit(&self, owner: PolityId, mover: PolityId) -> bool {
        let (owner, mover) = (self.head(owner), self.head(mover));
        owner != mover
            && self
                .relation(owner, mover)
                .is_some_and(|relation| relation.transit[relations::side(owner, mover)])
    }

    /// Darf eine Flotte von `mover` ein System betreten, das (nach der Mehrheit seiner
    /// Bevölkerung) `owner` untersteht? Ja im eigenen Verbund, bei Verbündeten, im Krieg
    /// und mit gewährter Durchfahrt. Ein System ohne Besitzer steht allen offen.
    pub fn may_enter(&self, mover: PolityId, owner: Option<PolityId>) -> bool {
        owner.is_none_or(|owner| {
            self.allied(mover, owner)
                || self.at_war(mover, owner)
                || self.grants_transit(owner, mover)
        })
    }

    /// Zahl der Verträge zwischen den Häuptern der beiden.
    pub fn rungs(&self, a: PolityId, b: PolityId) -> usize {
        let (a, b) = (self.head(a), self.head(b));
        if a == b {
            return 0;
        }
        self.relation(a, b).map_or(0, Relation::rungs)
    }

    /// Umstände des Krieges zwischen den Häuptern der beiden, wenn sie bekannt sind.
    pub fn war_between(&self, a: PolityId, b: PolityId) -> Option<&War> {
        if !self.at_war(a, b) {
            return None;
        }
        self.relation(self.head(a), self.head(b))
            .and_then(|relation| relation.war.as_ref())
    }

    /// Spannung zwischen den Häuptern der beiden, 0..100.
    pub fn tension(&self, a: PolityId, b: PolityId) -> f32 {
        self.relation(self.head(a), self.head(b))
            .map_or(0.0, |relation| relation.tension)
    }

    /// Messung einer Gemeinschaft (ohne Eintrag die leere).
    pub fn surveyed(&self, polity: PolityId) -> SurveyEntry {
        self.survey.get(polity.index()).copied().unwrap_or_default()
    }

    /// Öffentliches Bild einer Gemeinschaft (ohne Eintrag das unbeschriebene).
    pub fn public_face_of(&self, polity: PolityId) -> Dossier {
        self.public_face.get(&polity).cloned().unwrap_or_default()
    }

    /// Dossier eines Beobachters über eine andere Gemeinschaft (ohne Eintrag das
    /// unbeschriebene).
    pub fn dossier(&self, observer: PolityId, subject: PolityId) -> Dossier {
        self.dossiers
            .get(&(observer, subject))
            .cloned()
            .unwrap_or_default()
    }

    /// Offene Vorschläge zwischen den beiden, in beide Richtungen.
    pub fn proposals_between(&self, a: PolityId, b: PolityId) -> impl Iterator<Item = &Proposal> {
        self.proposals.iter().filter(move |proposal| {
            (proposal.from == a && proposal.to == b) || (proposal.from == b && proposal.to == a)
        })
    }

    /// Geld, das zurzeit in Sperrguthaben liegt. Bisher gibt es keine.
    pub fn escrow_total(&self) -> crate::economy::Money {
        0
    }
}

/// Beziehungen zu Spielbeginn: Alle sind frei, niemand führt Krieg, und jede Gemeinschaft
/// lebt die Kultur ihrer Vorlage aus `polities.ron`.
pub fn setup_diplomacy(data: &GameData, polities: &Polities) -> Diplomacy {
    Diplomacy {
        standing: vec![Standing::default(); polities.list.len()],
        culture: polities
            .list
            .iter()
            .map(|polity| {
                // Wie beim Startwissen: Die Gemeinschaft trägt den Namen ihrer Vorlage.
                data.polities
                    .iter()
                    .find(|def| def.name == polity.name)
                    .map_or(NEUTRAL_CULTURE, |def| {
                        [def.culture.0, def.culture.1, def.culture.2]
                    })
            })
            .collect(),
        ..Diplomacy::default()
    }
}

/// Erklärt den Krieg mit begrenztem Ziel und ohne Bilanz der Ausgangslage. Für Tests
/// und Werkzeuge; Befehle gehen über [`declare_war_with`].
pub fn declare_war(
    diplomacy: &mut Diplomacy,
    polities: &Polities,
    chronicle: &mut Chronicle,
    tick: u64,
    aggressor: PolityId,
    target: PolityId,
) -> Result<(), String> {
    declare_war_with(
        diplomacy,
        polities,
        chronicle,
        tick,
        aggressor,
        target,
        WarAim::Limited,
        None,
    )
    .map(|_| ())
}

/// Ausgangslage eines Krieges je Seite (Angreifer zuerst): Bevölkerung und Bauwert der
/// Flotten der beiden Verbünde.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WarFigures {
    pub population: [f64; 2],
    pub fleet: [f64; 2],
}

/// Was eine Kriegserklärung gebrochen hat.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Declared {
    /// Verträge, die den Krieg verboten: Die Erklärung war ein Vertragsbruch.
    pub breached: Vec<TreatyKind>,
}

/// Erklärt den Krieg mit einem Ziel. Bestehende Verträge des Paares enden; verbot einer
/// davon den Krieg (Nichtangriff, Verteidigung, Union), ist die Erklärung ein
/// Vertragsbruch ([`treaties::breach`]). Eine Waffenruhe dagegen hält: Während sie gilt,
/// wird keine Erklärung angenommen.
#[allow(clippy::too_many_arguments)]
pub fn declare_war_with(
    diplomacy: &mut Diplomacy,
    polities: &Polities,
    chronicle: &mut Chronicle,
    tick: u64,
    aggressor: PolityId,
    target: PolityId,
    aim: WarAim,
    figures: Option<WarFigures>,
) -> Result<Declared, String> {
    check_pair(diplomacy, polities, aggressor, target)?;
    let day = tick / TICKS_PER_DAY;
    diplomacy.truces.retain(|_, until| *until > day);
    if diplomacy.at_war(aggressor, target) {
        return Err("es herrscht bereits Krieg".into());
    }
    if let Some(until) = diplomacy.truce_until(aggressor, target, day) {
        return Err(format!(
            "nach dem Friedensschluss gilt noch {} Tage Waffenruhe",
            until - day
        ));
    }
    let breached: Vec<TreatyKind> = diplomacy
        .relation(aggressor, target)
        .map(|relation| {
            TreatyKind::ALL
                .into_iter()
                .filter(|kind| kind.forbids_war() && relation.has(*kind))
                .collect()
        })
        .unwrap_or_default();
    if !breached.is_empty() {
        treaties::breach(
            diplomacy, polities, chronicle, tick, aggressor, target, &breached,
        );
    } else if let Some(relation) = diplomacy.relations.get_mut(&pair(aggressor, target)) {
        // Verträge ohne Kriegsverbot enden mit dem Krieg, ohne Bruch.
        relation.treaties = [None; 6];
    }
    // Wer Krieg führt, handelt nicht mehr miteinander und lässt den anderen nicht durch.
    let relation = diplomacy.relation_mut(aggressor, target);
    relation.transit = [false; 2];
    relation.merge_months = 0;
    let attacker = relations::side(aggressor, target);
    let figures = figures.unwrap_or_default();
    let mut pop0 = [0.0; 2];
    let mut fleet0 = [0.0; 2];
    pop0[attacker] = figures.population[0];
    pop0[1 - attacker] = figures.population[1];
    fleet0[attacker] = figures.fleet[0];
    fleet0[1 - attacker] = figures.fleet[1];
    relation.war = Some(War {
        attacker: aggressor,
        aim,
        since_day: day,
        pop0,
        fleet0,
        taken: [0.0; 2],
        lost: [0.0; 2],
        conquered: Vec::new(),
        took_capital: [false; 2],
    });
    diplomacy.wars.insert(pair(aggressor, target));
    // Offene Vorschläge zwischen den beiden außer Frieden verfallen.
    proposals::drop_between(diplomacy, aggressor, target, |deal| {
        !matches!(deal, Deal::Peace(_))
    });
    let aim_text = match aim {
        WarAim::Limited => String::new(),
        WarAim::Subjugation => " Ziel ist die Unterwerfung.".into(),
    };
    chronicle.push(
        tick,
        ChronicleKind::War,
        Some(aggressor),
        None,
        format!(
            "{} erklärt {} den Krieg.{aim_text}",
            polities.get(aggressor).name,
            polities.get(target).name
        ),
    );
    Ok(Declared { breached })
}

/// Schließt Frieden. Danach gilt `truce_days` Tage lang Waffenruhe zwischen den beiden.
pub fn make_peace(
    diplomacy: &mut Diplomacy,
    polities: &Polities,
    chronicle: &mut Chronicle,
    tick: u64,
    a: PolityId,
    b: PolityId,
    truce_days: u64,
) -> Result<(), String> {
    check_pair(diplomacy, polities, a, b)?;
    if !diplomacy.wars.remove(&pair(a, b)) {
        return Err("es herrscht kein Krieg".into());
    }
    if let Some(relation) = diplomacy.relations.get_mut(&pair(a, b)) {
        relation.war = None;
    }
    proposals::drop_between(diplomacy, a, b, |deal| matches!(deal, Deal::Peace(_)));
    let day = tick / TICKS_PER_DAY;
    diplomacy.truces.retain(|_, until| *until > day);
    if truce_days > 0 {
        diplomacy.truces.insert(pair(a, b), day + truce_days);
    }
    chronicle.push(
        tick,
        ChronicleKind::Diplomacy,
        Some(a),
        None,
        format!(
            "{} und {} schließen Frieden.",
            polities.get(a).name,
            polities.get(b).name
        ),
    );
    Ok(())
}

/// Beendet alle Kriege einer Gemeinschaft, ohne Friedensschluss: wenn sie endet oder sich
/// einem Oberhaupt unterstellt. Sonst blieben ihre Gegner für immer im Krieg mit
/// niemandem. Gibt die bisherigen Gegner zurück, aufsteigend.
pub fn drop_wars_of(diplomacy: &mut Diplomacy, polity: PolityId) -> Vec<PolityId> {
    for relation in diplomacy
        .relations
        .iter_mut()
        .filter(|((a, b), _)| *a == polity || *b == polity)
        .map(|(_, relation)| relation)
    {
        relation.war = None;
    }
    let mut former = Vec::new();
    diplomacy.wars.retain(|(a, b)| {
        if *a == polity {
            former.push(*b);
            false
        } else if *b == polity {
            former.push(*a);
            false
        } else {
            true
        }
    });
    former.sort_unstable();
    former
}

/// Hebt alle Waffenruhen einer Gemeinschaft auf: wenn sie endet. Mit einer Gemeinschaft,
/// die es nicht mehr gibt, ruht nichts.
pub fn drop_truces_of(diplomacy: &mut Diplomacy, polity: PolityId) {
    diplomacy
        .truces
        .retain(|(a, b), _| *a != polity && *b != polity);
}

/// Vergisst eine Gemeinschaft, die endet: Beziehungen, Vorschläge, Sperren, Dossiers
/// von ihr und über sie, ihr öffentliches Bild. Das Protokoll der Taten bleibt als
/// Geschichte stehen; noch unbewertete Taten von ihr oder gegen sie verfallen.
pub fn forget_polity(diplomacy: &mut Diplomacy, polity: PolityId) {
    let involved = |a: PolityId, b: PolityId| a == polity || b == polity;
    diplomacy.relations.retain(|(a, b), _| !involved(*a, *b));
    diplomacy.relations.values_mut().for_each(|relation| {
        if relation
            .tribute
            .is_some_and(|tribute| tribute.payer == polity)
        {
            relation.tribute = None;
        }
    });
    diplomacy
        .proposals
        .retain(|proposal| !involved(proposal.from, proposal.to) && !proposal.deal.names(polity));
    diplomacy.blocked.retain(|(a, b, _), _| !involved(*a, *b));
    diplomacy.dossiers.retain(|(a, b), _| !involved(*a, *b));
    diplomacy.public_face.remove(&polity);
    diplomacy
        .pending_acts
        .retain(|act| !involved(act.actor, act.toward));
    diplomacy.bind_year.retain(|(a, b), _| !involved(*a, *b));
}

/// Eine Gemeinschaft untersteht fortan einem Oberhaupt: Ihre eigenen Verträge,
/// Embargos, Durchfahrtsrechte und Vorschläge enden; sie teilt die ihres Haupts.
/// Groll, Spannung und Messungen bleiben.
pub fn on_subordinated(diplomacy: &mut Diplomacy, ward: PolityId) {
    for ((a, b), relation) in diplomacy.relations.iter_mut() {
        if *a == ward || *b == ward {
            relation.treaties = [None; 6];
            relation.embargo = [false; 2];
            relation.transit = [false; 2];
            relation.merge_months = 0;
            if relation
                .tribute
                .is_some_and(|tribute| tribute.payer == ward)
            {
                relation.tribute = None;
            }
        }
    }
    diplomacy.proposals.retain(|proposal| {
        proposal.from != ward && proposal.to != ward && !proposal.deal.names(ward)
    });
}

/// Invarianten der Diplomatie: Beziehungen, Vorschläge und Dossiers nennen nur lebende
/// Gemeinschaften; Verträge, Kriegsumstände, Embargos und Durchfahrt bestehen nur
/// zwischen freien; Kriegsumstände gibt es nur für herrschende Kriege; Vorschläge laufen
/// nur zwischen freien.
pub fn check(diplomacy: &Diplomacy, polities: &Polities) -> Result<(), String> {
    let alive = |id: PolityId| {
        polities
            .list
            .get(id.index())
            .is_some_and(|polity| polity.alive)
    };
    for ((a, b), relation) in &diplomacy.relations {
        if a >= b {
            return Err(format!("Beziehung {a}–{b} ist nicht geordnet"));
        }
        if !alive(*a) || !alive(*b) {
            return Err(format!(
                "Beziehung {a}–{b} nennt eine beendete Gemeinschaft"
            ));
        }
        let free = diplomacy.is_free(*a) && diplomacy.is_free(*b);
        if !free
            && (relation.rungs() > 0
                || relation.any_embargo()
                || relation.transit != [false; 2]
                || relation.war.is_some())
        {
            return Err(format!(
                "Beziehung {a}–{b}: Verträge, Krieg, Embargo oder Durchfahrt, obwohl eine \
                 Seite nicht frei ist"
            ));
        }
        if relation.war.is_some() && !diplomacy.wars.contains(&(*a, *b)) {
            return Err(format!("Beziehung {a}–{b}: Kriegsumstände ohne Krieg"));
        }
        if relation.war.is_some() && relation.rungs() > 0 {
            return Err(format!("Beziehung {a}–{b}: Verträge im Krieg"));
        }
        for (kind, treaty) in TreatyKind::ALL.iter().zip(&relation.treaties) {
            if treaty.is_some() && kind.requires().iter().any(|need| !relation.has(*need)) {
                return Err(format!(
                    "Beziehung {a}–{b}: {} ohne seine Voraussetzungen",
                    kind.label()
                ));
            }
        }
    }
    for proposal in &diplomacy.proposals {
        if !alive(proposal.from) || !alive(proposal.to) {
            return Err(format!(
                "Vorschlag {} nennt eine beendete Gemeinschaft",
                proposal.id
            ));
        }
        if !diplomacy.is_free(proposal.from) || !diplomacy.is_free(proposal.to) {
            return Err(format!(
                "Vorschlag {} zwischen Gemeinschaften, die nicht frei sind",
                proposal.id
            ));
        }
    }
    for (observer, subject) in diplomacy.dossiers.keys() {
        if !alive(*observer) || !alive(*subject) {
            return Err(format!(
                "Dossier {observer} über {subject} nennt eine beendete Gemeinschaft"
            ));
        }
    }
    if let Some(gone) = diplomacy.public_face.keys().find(|id| !alive(**id)) {
        return Err(format!(
            "öffentliches Bild einer beendeten Gemeinschaft {gone}"
        ));
    }
    Ok(())
}

/// Krieg und Frieden sind Sache freier, lebender Gemeinschaften.
pub(crate) fn check_pair(
    diplomacy: &Diplomacy,
    polities: &Polities,
    a: PolityId,
    b: PolityId,
) -> Result<(), String> {
    if a == b {
        return Err("eine Gemeinschaft kann sich nicht selbst meinen".into());
    }
    for id in [a, b] {
        if !polities
            .list
            .get(id.index())
            .is_some_and(|polity| polity.alive)
        {
            return Err("Gemeinschaft besteht nicht mehr".into());
        }
    }
    for id in [a, b] {
        if let Some(overlord) = diplomacy.overlord_of(id) {
            let head = polities
                .list
                .get(overlord.index())
                .map_or("einem Oberhaupt", |polity| polity.name.as_str());
            return Err(format!(
                "{} untersteht {head}: Über Krieg und Frieden entscheidet das Oberhaupt",
                polities.get(id).name
            ));
        }
    }
    Ok(())
}

/// Diplomatie: täglich Fristen, Gefechtsbilanzen und Indizien, monatlich die Messung,
/// der Tribut und das Diktat über Kriege, die von selbst enden.
pub struct DiplomacyPlugin;

impl Plugin for DiplomacyPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            crate::sim_schedule(),
            (
                daily_diplomacy.run_if(crate::clock::day_start),
                monthly_survey.run_if(crate::clock::month_start),
                settle_wars.run_if(crate::clock::month_start),
            )
                .chain()
                .in_set(crate::SimSet::Diplomacy),
        );
    }
}

/// Tagesbeginn: Vorschläge verfallen, gekündigte Verträge laufen aus, Gefechte gehen in
/// die Kriegsbilanzen ein, die Beobachter bewerten die Taten des Vortags.
fn daily_diplomacy(
    data: Res<GameData>,
    clock: Res<crate::clock::SimClock>,
    polities: Res<Polities>,
    battles: Res<crate::fleet::Battles>,
    mut diplomacy: ResMut<Diplomacy>,
    mut chronicle: ResMut<Chronicle>,
) {
    let tick = clock.tick;
    proposals::expire(&mut diplomacy, clock.day());
    treaties::expire(&mut diplomacy, &polities, &mut chronicle, tick);
    war::record_battles(&mut diplomacy, &battles, tick);
    evidence::score_pending(&mut diplomacy, &polities, &data);
}

/// Monatsbeginn: Messung, Spannung, Tribut.
#[allow(clippy::too_many_arguments)]
fn monthly_survey(
    data: Res<GameData>,
    clock: Res<crate::clock::SimClock>,
    galaxy: Res<crate::galaxy::Galaxy>,
    polities: Res<Polities>,
    finance: Res<crate::finance::Finance>,
    logistics: Res<crate::logistics::Logistics>,
    fleets: Res<crate::fleet::Fleets>,
    research: Res<crate::tech::Research>,
    mut diplomacy: ResMut<Diplomacy>,
    mut economy: ResMut<crate::economy::Economy>,
    mut chronicle: ResMut<Chronicle>,
) {
    let inputs = survey::SurveyInputs {
        data: &data,
        galaxy: &galaxy,
        polities: &polities,
        finance: &finance,
        logistics: &logistics,
        fleets: &fleets,
        research: &research,
    };
    survey::survey(&inputs, &mut diplomacy, &economy, clock.day());
    survey::pay_tributes(
        &data,
        &polities,
        &finance,
        &mut diplomacy,
        &mut economy,
        &mut chronicle,
        clock.tick,
    );
}

/// Monatsbeginn: Kriege, die von selbst enden ([`war::diktat`]).
fn settle_wars(
    clock: Res<crate::clock::SimClock>,
    mut params: crate::polity::succession::SuccessionParams,
) {
    let tick = clock.tick;
    let mut succession = params.borrow();
    let levers = succession.data.diplomacy.levers.clone();
    let wars: Vec<(PolityId, PolityId)> = succession.diplomacy.wars.iter().copied().collect();
    for (a, b) in wars {
        let Some(known) = succession.diplomacy.war_between(a, b).cloned() else {
            continue;
        };
        let wear = [
            war::weariness(succession.finance, a),
            war::weariness(succession.finance, b),
        ];
        // Stelle 0 ist die kleinere Nummer, wie in den Feldern je Seite.
        let Some(diktat) = war::diktat(&known, a, b, wear, known.took_capital, tick, &levers)
        else {
            continue;
        };
        let truce = succession.data.balance.military.truce_days;
        let imposed = match diktat {
            war::Diktat::White => None,
            war::Diktat::Imposed { victor, terms } => Some((victor, terms)),
        };
        let concluded = imposed.is_some_and(|(victor, terms)| {
            let yielder = if victor == a { b } else { a };
            let proposal = Proposal {
                id: u32::MAX,
                from: victor,
                to: yielder,
                deal: Deal::Peace(terms),
                made_day: tick / TICKS_PER_DAY,
                expires_day: tick / TICKS_PER_DAY,
            };
            proposals::conclude(&mut succession, &proposal, tick).is_ok()
        });
        if !concluded {
            let _ = make_peace(
                succession.diplomacy,
                succession.polities,
                succession.chronicle,
                tick,
                a,
                b,
                truce,
            );
        }
        succession.chronicle.push(
            tick,
            ChronicleKind::Diplomacy,
            Some(a),
            None,
            format!(
                "Der Krieg zwischen {} und {} endet von selbst: {}.",
                succession.polities.get(a).name,
                succession.polities.get(b).name,
                if concluded {
                    "Der Vorsprung diktiert die Bedingungen"
                } else {
                    "Beide sind erschöpft, oder keiner liegt deutlich vorn"
                }
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Beziehungen unter `count` freien Gemeinschaften, ohne Kriege.
    fn free(count: usize) -> Diplomacy {
        Diplomacy {
            standing: vec![Standing::default(); count],
            culture: vec![NEUTRAL_CULTURE; count],
            ..Diplomacy::default()
        }
    }

    fn subordinate(diplomacy: &mut Diplomacy, ward: u32, lord: u32) {
        diplomacy.standing[ward as usize].overlord = Some(PolityId(lord));
    }

    #[test]
    fn war_is_symmetric() {
        let mut diplomacy = Diplomacy::default();
        diplomacy.wars.insert(pair(PolityId(3), PolityId(1)));
        assert!(diplomacy.at_war(PolityId(1), PolityId(3)));
        assert!(diplomacy.at_war(PolityId(3), PolityId(1)));
        assert!(!diplomacy.at_war(PolityId(1), PolityId(2)));
        assert!(!diplomacy.at_war(PolityId(1), PolityId(1)));
        assert_eq!(
            diplomacy.enemies_of(PolityId(3)).collect::<Vec<_>>(),
            vec![PolityId(1)]
        );
    }

    #[test]
    fn without_entries_everyone_is_free_and_their_own_head() {
        // Von Hand gebaute Lagen (etwa in Tests der Flotten) führen keine Stellungen.
        let diplomacy = Diplomacy::default();
        let far = PolityId(7);
        assert!(diplomacy.is_free(far));
        assert_eq!(diplomacy.head(far), far);
        assert_eq!(diplomacy.bloc(far), vec![far]);
        assert_eq!(diplomacy.wards_of(far).count(), 0);
        assert_eq!(diplomacy.culture_of(far), NEUTRAL_CULTURE);
        assert!(!diplomacy.is_at_war(far));
    }

    #[test]
    fn wards_share_the_wars_of_their_head() {
        let mut diplomacy = free(6);
        let id = PolityId;
        // 1 und 4 unterstehen 0, 3 untersteht 2; 5 bleibt für sich.
        subordinate(&mut diplomacy, 1, 0);
        subordinate(&mut diplomacy, 4, 0);
        subordinate(&mut diplomacy, 3, 2);
        assert_eq!(diplomacy.head(id(1)), id(0));
        assert_eq!(diplomacy.head(id(0)), id(0));
        assert!(!diplomacy.is_free(id(4)) && diplomacy.is_free(id(5)));
        assert_eq!(
            diplomacy.wards_of(id(0)).collect::<Vec<_>>(),
            vec![id(1), id(4)]
        );
        assert_eq!(diplomacy.bloc(id(4)), vec![id(0), id(1), id(4)]);
        assert_eq!(diplomacy.bloc(id(2)), vec![id(2), id(3)]);
        assert_eq!(diplomacy.bloc(id(5)), vec![id(5)]);

        // Krieg zwischen den Häuptern 0 und 2: Alle sechs Paare über die Verbünde hinweg
        // liegen im Krieg, kein Paar innerhalb eines Verbunds.
        diplomacy.wars.insert(pair(id(0), id(2)));
        for ours in [0, 1, 4] {
            for theirs in [2, 3] {
                assert!(
                    diplomacy.at_war(id(ours), id(theirs)),
                    "{ours} gegen {theirs}"
                );
                assert!(diplomacy.at_war(id(theirs), id(ours)));
            }
        }
        for (a, b) in [(0, 1), (0, 4), (1, 4), (2, 3)] {
            assert!(!diplomacy.at_war(id(a), id(b)), "{a} und {b}: ein Verbund");
        }
        for member in 0..5 {
            assert!(diplomacy.is_at_war(id(member)));
            assert!(!diplomacy.at_war(id(member), id(5)));
        }
        assert!(!diplomacy.is_at_war(id(5)));
        // Feinde eines Unterstellten sind die seines Haupts, samt deren Unterstellten.
        assert_eq!(
            diplomacy.enemies_of(id(4)).collect::<Vec<_>>(),
            vec![id(2), id(3)]
        );
        assert_eq!(
            diplomacy.enemies_of(id(3)).collect::<Vec<_>>(),
            vec![id(0), id(1), id(4)]
        );
    }

    #[test]
    fn dropping_wars_leaves_no_trace_of_the_community() {
        let mut diplomacy = free(4);
        let id = PolityId;
        diplomacy.wars.insert(pair(id(2), id(0)));
        diplomacy.wars.insert(pair(id(2), id(3)));
        diplomacy.wars.insert(pair(id(0), id(1)));
        assert_eq!(drop_wars_of(&mut diplomacy, id(2)), vec![id(0), id(3)]);
        assert!(!diplomacy.is_at_war(id(2)) && !diplomacy.is_at_war(id(3)));
        assert!(diplomacy.at_war(id(0), id(1)), "andere Kriege bleiben");
        assert_eq!(drop_wars_of(&mut diplomacy, id(2)), vec![]);
        assert_eq!(diplomacy.wars.len(), 1);
    }

    #[test]
    fn affinity_measures_the_distance_of_cultures() {
        let mut diplomacy = free(4);
        diplomacy.culture[0] = [0.0, 0.0, 0.0];
        diplomacy.culture[1] = [1.0, 1.0, 1.0];
        diplomacy.culture[2] = [0.2, 0.5, 0.9];
        diplomacy.culture[3] = [0.2, 0.5, 0.4];
        let id = PolityId;
        assert_eq!(diplomacy.affinity(id(2), id(2)), 1.0);
        assert!(diplomacy.affinity(id(0), id(1)).abs() < 1e-6);
        // Symmetrisch, und ein Abstand von 0,5 in einem Zug kostet 0,5 / √3.
        let near = diplomacy.affinity(id(2), id(3));
        assert_eq!(near, diplomacy.affinity(id(3), id(2)));
        assert!((near - (1.0 - 0.5 / 3.0f32.sqrt())).abs() < 1e-6);
        // Wer keinen Eintrag hat, gilt als Mitte.
        assert_eq!(
            diplomacy.affinity(id(9), id(0)),
            1.0 - (0.75f32).sqrt() / 3.0f32.sqrt()
        );
    }

    #[test]
    fn the_shipped_templates_give_every_community_its_culture() {
        let data = GameData::load_workspace().unwrap();
        let mut galaxy = crate::galaxy::generate_galaxy(&data, 3);
        let polities = crate::polity::found_polities(&data, &mut galaxy, 3);
        let diplomacy = setup_diplomacy(&data, &polities);
        assert_eq!(diplomacy.standing.len(), polities.list.len());
        assert_eq!(diplomacy.culture.len(), polities.list.len());
        for polity in &polities.list {
            let template = data
                .polities
                .iter()
                .find(|def| def.name == polity.name)
                .unwrap();
            let (o, g, w) = template.culture;
            assert_eq!(diplomacy.culture_of(polity.id), [o, g, w]);
            assert!(diplomacy.is_free(polity.id));
        }
        // Keine zwei Gemeinschaften der Partie sind kulturell gleich.
        for a in &polities.list {
            for b in &polities.list {
                assert_eq!(diplomacy.affinity(a.id, b.id) == 1.0, a.id == b.id);
            }
        }
    }
}

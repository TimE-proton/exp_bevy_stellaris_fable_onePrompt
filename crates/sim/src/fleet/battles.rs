//! Gefechte in der Partie.
//!
//! Stehen einsatzbereite Flotten verfeindeter Gemeinschaften im selben System, beginnt am
//! Tagesanfang ein Gefecht. Gerechnet wird es vom taktischen Rechner aus
//! [`crate::battle`], einen Schritt je Takt; dieses Modul stellt es auf, führt
//! Verstärkungen heran, reicht Befehle durch und schreibt am Ende das Ergebnis in die
//! Flotten zurück. Was aus jedem Schiff geworden ist, sagt der Rechner selbst
//! ([`Battle::results`]): verloren, entkommen oder noch auf dem Feld, dazu Schäden,
//! verbrauchte Vorräte und heimgekehrte Bordjäger.
//!
//! Wie ein Gefecht ausgeht, entscheidet, wer bleibt. Gefragt wird je Seite, nicht je
//! Flotte: Zur Seite einer Partei gehören alle, mit denen sie nicht im Krieg liegt.
//! - Wer am Ende noch Schiffe auf dem Feld hat, bleibt im System und ist einsatzbereit.
//! - Wessen Schiffe das Feld geräumt haben, während ein Gegner es hält, wurde geschlagen:
//!   Die Flotte weicht aus und sammelt sich.
//! - Wessen Schiffe das Feld geräumt haben, während die eigene Seite es hält (eine
//!   Schwesterflotte, ein Mitstreiter gegen denselben Gegner), bleibt im System und
//!   sammelt sich dort, hinter den eigenen Linien.
//! - Ohne Entscheidung (Patt oder Zeitgrenze): Niemand hat gewonnen, alle lösen sich
//!   voneinander. Wer im eigenen System steht, bleibt dort; alle anderen weichen aus und
//!   sammeln sich. So beginnt dasselbe Gefecht nicht am nächsten Morgen von vorn.
//! - Gegner aus demselben Gefecht weichen nie in dasselbe System aus, notfalls über
//!   mehrere Sprünge: Sonst stünden sie sich nach der Sammelfrist dort wieder gegenüber.
//!
//! Wer sich sammelt, ist aus dem Spiel genommen, bis die Frist abläuft: Die Flotte
//! beginnt kein Gefecht und wird nicht angegriffen, zählt nicht für Blockade, Kaperfahrt,
//! Geleit oder die Verteidigung eines Orbits, und sie nimmt keinen Marschbefehl an. Der
//! Schutz ist an den Abzug gebunden, nicht an die Flotte: Dieselben Gegner stehen
//! einander frühestens nach Ablauf der Sammelfrist wieder gegenüber, und nur, wenn dann
//! eine Seite auf die andere marschiert oder neben ihr gewartet hat.
//!
//! Solange ein Gefecht läuft, stehen seine Folgen jeden Morgen in den Flotten
//! ([`muster_battles`]): Zerstörte Schiffe sind fort, die übrigen tragen ihre Schäden. Wer
//! keinen Gegner mehr auf dem Feld hat (etwa nach einem Friedensschluss) oder kein eigenes
//! Schiff mehr, scheidet am selben Morgen aus, auch wenn andere weiterkämpfen.
//!
//! Technik: Die Aufschläge einer Partei (Waffenschaden, Reichweite, Schilde, Panzerung,
//! Rumpf, Fahrt) stammen aus der Forschung ihrer Gemeinschaft zum Zeitpunkt des Eintritts
//! ins Gefecht. Wer gegeneinander gekämpft hat, kennt danach die Technik des anderen
//! besser ([`Research::add_intel`]).
//!
//! Die Folgen tragen den Krieg in die übrige Partie: Verluste heben die Kriegsmüdigkeit
//! (und damit Zinsen und Unruhe), eine schwere Niederlage drückt die Kurse, und was die
//! Flotte verschossen und eingebüßt hat, kauft der Staat später am Markt nach.

use super::{Fleet, Fleets, material_value};
use crate::battle::{
    Battle, CombatModifiers, CombatantSetup, Doctrine, Fate, Formation, ShipResult, ShipStatus,
};
use crate::chronicle::{Chronicle, ChronicleKind};
use crate::clock::{DAYS_PER_MONTH, TICKS_PER_DAY};
use crate::data::{GameData, ShipClass};
use crate::diplomacy::Diplomacy;
use crate::finance::Finance;
use crate::galaxy::Galaxy;
use crate::ids::{BattleId, FleetId, PolityId, ShipId, SystemId};
use crate::logistics::Logistics;
use crate::polity::Polities;
use crate::rng::{Domain, Rng};
use crate::tech::Research;
use bevy_ecs::prelude::Resource;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Unter diesem Bauwert in Credits gilt ein Trümmerfeld als verweht.
const DEBRIS_FLOOR: f32 = 1.0;

/// Bilanz einer Partei eines beendeten Gefechts. Jede Flotte ist eine Partei.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SideReport {
    pub owner: PolityId,
    pub fleet: Option<FleetId>,
    /// Name der Flotte bei Gefechtsende; sie selbst besteht vielleicht nicht mehr.
    pub fleet_name: String,
    /// Schiffe je Klasse (Stelle = [`ShipClass::index`]), Bordjäger eingeschlossen: zu
    /// Beginn, verloren, dem Feld entkommen.
    pub initial: [u32; 6],
    pub lost: [u32; 6],
    pub escaped: [u32; 6],
    /// Bauwert in Credits: zu Beginn und verloren.
    pub initial_value: f32,
    pub lost_value: f32,
    /// Am Ende stand noch mindestens ein Schiff der Flotte auf dem Feld
    /// ([`Fate::Holding`]). Bordjäger allein halten kein Feld.
    pub held_field: bool,
    /// Der Rückzug war befohlen: von der eigenen Führung im Gefecht, auf Befehl der
    /// Gemeinschaft oder im Patt.
    pub retreated: bool,
    /// Die Partei hat sich im Patt gelöst: Weder sie noch ihre Gegner kamen lange durch
    /// die Schilde der anderen, alle zogen gemeinsam ab.
    pub stalemate: bool,
    /// Takt, in dem die Partei das Gefecht vorzeitig verlassen hat, während andere
    /// weiterkämpften; `None`, wenn sie bis zum Ende dabei war. `held_field` sagt dann, ob
    /// sie beim Ausscheiden noch auf dem Feld stand (kein Gegner mehr) oder es geräumt hatte.
    #[serde(default)]
    pub left_early: Option<u64>,
}

/// Warum eine Partei ein laufendes Gefecht vorzeitig verlassen hat.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Departure {
    /// Kein Gegner mehr auf dem Feld, etwa nach einem Friedensschluss: Die Flotte bleibt im
    /// System und ist einsatzbereit.
    NoOpponent,
    /// Kein eigenes Schiff mehr auf dem Feld: entkommen oder verloren.
    LeftField,
}

/// Eine Partei, die ein laufendes Gefecht vorzeitig verlassen hat. Im Rechner bleibt sie
/// als Partei stehen, mit ihrer Bilanz und ohne Schiffe auf dem Feld; ihre Flotte ist
/// frei.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Departed {
    pub battle: BattleId,
    /// Stelle in [`Battle::combatants`].
    pub side: u16,
    pub fleet: Option<FleetId>,
    pub tick: u64,
    pub cause: Departure,
    /// Wohin die Flotte ausgewichen ist; `None`, wenn sie im System blieb oder nicht mehr
    /// besteht.
    pub destination: Option<SystemId>,
}

/// Wie ein Gefecht ausging.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BattleOutcome {
    /// Entschieden: Mindestens eine Partei hält das Feld, ihre Gegner nicht.
    Decided,
    /// Patt: Keine Seite kam gegen die andere an, alle lösten sich gemeinsam. Niemand hat
    /// gewonnen.
    Stalemate,
    /// An der Zeitgrenze des Rechners ohne Entscheidung abgebrochen. Niemand hat gewonnen.
    TimedOut,
    /// Eingestellt: Unter den Beteiligten herrschte kein Krieg mehr.
    CalledOff,
    /// Niemand hält das Feld: Alle Parteien sind vernichtet oder geflohen.
    Abandoned,
}

impl BattleOutcome {
    pub const ALL: [BattleOutcome; 5] = [
        BattleOutcome::Decided,
        BattleOutcome::Stalemate,
        BattleOutcome::TimedOut,
        BattleOutcome::CalledOff,
        BattleOutcome::Abandoned,
    ];

    pub fn label(self) -> &'static str {
        match self {
            BattleOutcome::Decided => "entschieden",
            BattleOutcome::Stalemate => "Patt",
            BattleOutcome::TimedOut => "ohne Entscheidung an der Zeitgrenze",
            BattleOutcome::CalledOff => "eingestellt",
            BattleOutcome::Abandoned => "niemand hält das Feld",
        }
    }

    /// Ohne Entscheidung: Patt oder Zeitgrenze. Dann gibt es keinen Sieger, auch wenn noch
    /// Schiffe auf dem Feld stehen.
    pub fn undecided(self) -> bool {
        matches!(self, BattleOutcome::Stalemate | BattleOutcome::TimedOut)
    }
}

/// Bilanz eines beendeten Gefechts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BattleReport {
    pub id: BattleId,
    pub system: SystemId,
    /// Takt des Beginns und des Endes.
    pub started: u64,
    pub ended: u64,
    pub outcome: BattleOutcome,
    pub sides: Vec<SideReport>,
}

impl BattleReport {
    /// Dauer in Kalendertagen, angefangene Tage voll gezählt.
    pub fn days(&self) -> u64 {
        self.ended
            .saturating_sub(self.started)
            .div_ceil(TICKS_PER_DAY)
            .max(1)
    }
}

/// Trümmerfeld eines Systems: was dort in Gefechten verloren ging. Vorerst nur Auskunft
/// für die Karte.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Debris {
    pub system: SystemId,
    /// Bauwert der verlorenen Schiffe in Credits; verblasst mit den Jahren.
    pub amount: f32,
    /// Takt des jüngsten Gefechts, das dazu beitrug.
    pub tick: u64,
}

/// Alle Gefechte der Partie: die laufenden, die Bilanzen der jüngsten, die Trümmer.
#[derive(Resource, Clone, Debug, Default, Serialize, Deserialize)]
pub struct Battles {
    /// Laufende Gefechte in der Reihenfolge ihres Beginns; höchstens eines je System.
    pub active: Vec<Battle>,
    /// Bilanzen der jüngsten Gefechte, älteste zuerst.
    pub reports: Vec<BattleReport>,
    /// Trümmerfelder, eines je System, aufsteigend nach System.
    pub debris: Vec<Debris>,
    /// Nächste freie Gefechtsnummer; zugleich die Zahl aller begonnenen Gefechte.
    pub next_id: u32,
    /// Verlorene Schiffe je Gemeinschaft über die ganze Partie, ohne Bordjäger. Lesen mit
    /// [`Battles::ships_lost_by`].
    pub ships_lost: Vec<u32>,
    /// Parteien laufender Gefechte, die vorzeitig ausgeschieden sind, in der Reihenfolge
    /// ihres Ausscheidens. Mit dem Ende des Gefechts fallen sie heraus.
    #[serde(default)]
    pub departed: Vec<Departed>,
    /// Beendete Gefechte je Ausgang über die ganze Partie, in der Reihenfolge von
    /// [`BattleOutcome::ALL`].
    #[serde(default)]
    pub outcomes: [u32; 5],
    /// Flotten je Gemeinschaft, die in einem Gefecht mit allen Schiffen verloren gingen.
    /// Lesen mit [`Battles::fleets_lost_by`].
    #[serde(default)]
    pub fleets_lost: Vec<u32>,
    /// Entschiedene Gefechte je Gemeinschaft, in denen sie am Ende das Feld hielt. Lesen
    /// mit [`Battles::won_by`].
    #[serde(default)]
    pub won: Vec<u32>,
}

/// Ist die Partei aus dem Gefecht ausgeschieden?
fn has_departed(departed: &[Departed], battle: BattleId, side: usize) -> bool {
    departed
        .iter()
        .any(|entry| entry.battle == battle && entry.side as usize == side)
}

/// Stelle der Partei, als die eine Flotte in einem Gefecht steht. War die Flotte schon
/// einmal dabei, ist ausgeschieden und später wieder eingetreten, zählt der jüngste Eintrag.
fn side_of_fleet(departed: &[Departed], battle: &Battle, fleet: FleetId) -> Option<usize> {
    (0..battle.combatants.len()).rev().find(|side| {
        battle.combatants[*side].fleet == Some(fleet) && !has_departed(departed, battle.id, *side)
    })
}

impl Battles {
    /// So viele Bilanzen bleiben im Zustand; ältere fallen heraus.
    pub const REPORTS: usize = 40;

    /// Das Gefecht, das in einem System läuft.
    pub fn in_system(&self, system: SystemId) -> Option<&Battle> {
        self.active.iter().find(|battle| battle.system == system)
    }

    /// Das Gefecht, in dem eine Flotte steht, und ihre Partei darin (Stelle in
    /// [`Battle::combatants`]). Wer ausgeschieden ist, steht in keinem Gefecht mehr.
    pub fn of_fleet(&self, fleet: FleetId) -> Option<(&Battle, usize)> {
        self.active.iter().find_map(|battle| {
            side_of_fleet(&self.departed, battle, fleet).map(|side| (battle, side))
        })
    }

    /// Ist die Partei (Stelle in [`Battle::combatants`]) aus dem laufenden Gefecht
    /// ausgeschieden?
    pub fn has_departed(&self, battle: BattleId, side: usize) -> bool {
        has_departed(&self.departed, battle, side)
    }

    /// Kämpft die Flotte gerade: Steht sie in einem Gefecht und hat dort noch Schiffe, die
    /// weder fliehen noch treiben?
    pub fn is_fighting(&self, fleet: FleetId) -> bool {
        self.of_fleet(fleet)
            .is_some_and(|(battle, side)| battle.is_fighting(side))
    }

    pub fn get(&self, id: BattleId) -> Option<&Battle> {
        self.active.iter().find(|battle| battle.id == id)
    }

    /// Bilanz eines beendeten Gefechts, solange sie noch aufbewahrt wird.
    pub fn report(&self, id: BattleId) -> Option<&BattleReport> {
        self.reports.iter().find(|report| report.id == id)
    }

    /// Trümmer in einem System: Bauwert der dort verlorenen Schiffe in Credits.
    pub fn debris_in(&self, system: SystemId) -> f32 {
        self.debris
            .iter()
            .find(|debris| debris.system == system)
            .map_or(0.0, |debris| debris.amount)
    }

    /// Schiffe, die eine Gemeinschaft in Gefechten verloren hat (ohne Bordjäger).
    pub fn ships_lost_by(&self, polity: PolityId) -> u32 {
        self.ships_lost.get(polity.index()).copied().unwrap_or(0)
    }

    /// Flotten, die eine Gemeinschaft in Gefechten mit allen Schiffen verloren hat.
    pub fn fleets_lost_by(&self, polity: PolityId) -> u32 {
        self.fleets_lost.get(polity.index()).copied().unwrap_or(0)
    }

    /// Entschiedene Gefechte, in denen die Gemeinschaft am Ende das Feld hielt.
    pub fn won_by(&self, polity: PolityId) -> u32 {
        self.won.get(polity.index()).copied().unwrap_or(0)
    }

    /// Beendete Gefechte mit diesem Ausgang über die ganze Partie.
    pub fn ended_with(&self, outcome: BattleOutcome) -> u32 {
        BattleOutcome::ALL
            .iter()
            .position(|other| *other == outcome)
            .map_or(0, |index| self.outcomes[index])
    }

    /// Schiffe, die in allen Gefechten der Partie verloren gingen (ohne Bordjäger).
    pub fn total_ships_lost(&self) -> u32 {
        self.ships_lost.iter().sum()
    }

    fn side_mut(&mut self, fleet: FleetId) -> Option<(&mut Battle, usize)> {
        let Battles {
            active, departed, ..
        } = self;
        active
            .iter_mut()
            .find_map(|battle| side_of_fleet(departed, battle, fleet).map(|side| (battle, side)))
    }

    /// Prüft, dass Gefechte und Flotten zueinander passen. Gibt die erste Auffälligkeit
    /// zurück.
    pub fn find_anomaly(&self, fleets: &Fleets) -> Option<String> {
        for (index, battle) in self.active.iter().enumerate() {
            if battle.id.0 >= self.next_id {
                return Some(format!(
                    "Gefecht {}: Nummer jenseits des Zählers",
                    battle.id
                ));
            }
            if battle.finished {
                return Some(format!(
                    "Gefecht {}: beendet, aber nicht abgeschlossen",
                    battle.id
                ));
            }
            if self.active[..index]
                .iter()
                .any(|other| other.system == battle.system || other.id == battle.id)
            {
                return Some(format!(
                    "Gefecht {}: zweites Gefecht im selben System oder Nummer doppelt",
                    battle.id
                ));
            }
            for (side, combatant) in battle.combatants.iter().enumerate() {
                let Some(fleet) = combatant.fleet.and_then(|id| fleets.get(id)) else {
                    return Some(format!("Gefecht {}: Partei ohne Flotte", battle.id));
                };
                // Wer ausgeschieden ist, bleibt als Partei stehen, bis das Gefecht endet;
                // seine Flotte ist frei. Ebenso eine Flotte, die es nicht mehr gibt.
                if self.has_departed(battle.id, side) {
                    if battle
                        .ships
                        .iter()
                        .any(|ship| ship.side as usize == side && ship.status.in_play())
                    {
                        return Some(format!(
                            "Gefecht {}: ausgeschiedene Partei steht noch auf dem Feld",
                            battle.id
                        ));
                    }
                    continue;
                }
                if fleet.alive && (fleet.battle != Some(battle.id) || fleet.system != battle.system)
                {
                    return Some(format!(
                        "Gefecht {}: {} gehört dazu, weiß aber nichts davon",
                        battle.id, fleet.name
                    ));
                }
            }
        }
        for fleet in &fleets.list {
            if fleet.in_battle != fleet.battle.is_some() {
                return Some(format!(
                    "{}: im Gefecht ({}) passt nicht zum Gefecht {:?}",
                    fleet.name, fleet.in_battle, fleet.battle
                ));
            }
            if let Some(id) = fleet.battle
                && !self
                    .of_fleet(fleet.id)
                    .is_some_and(|(battle, _)| battle.id == id)
            {
                return Some(format!(
                    "{}: steht in Gefecht {id}, das sie nicht kennt",
                    fleet.name
                ));
            }
            if !fleet.alive && (fleet.battle.is_some() || fleet.regroup_until.is_some()) {
                return Some(format!("{}: aufgelöst, aber noch im Einsatz", fleet.name));
            }
        }
        if let Some(entry) = self.departed.iter().find(|entry| {
            self.get(entry.battle)
                .is_none_or(|battle| entry.side as usize >= battle.combatants.len())
        }) {
            return Some(format!(
                "Gefecht {}: ausgeschiedene Partei ohne laufendes Gefecht",
                entry.battle
            ));
        }
        if self.reports.len() > Self::REPORTS {
            return Some("mehr Gefechtsbilanzen als vorgesehen".into());
        }
        for pair in self.debris.windows(2) {
            if pair[0].system >= pair[1].system {
                return Some("Trümmerfelder stehen außer der Reihe".into());
            }
        }
        if let Some(debris) = self
            .debris
            .iter()
            .find(|debris| !debris.amount.is_finite() || debris.amount < 0.0)
        {
            return Some(format!(
                "Trümmerfeld {}: Wert {}",
                debris.system, debris.amount
            ));
        }
        None
    }
}

/// Aufschläge aus der Technologie einer Gemeinschaft für ihre Schiffe im Gefecht: die eine
/// Stelle, an der die Wirkungen der Forschung (Waffenschaden, Reichweite, Schilde,
/// Panzerung, Rumpf, Fahrt im Gefecht) in [`CombatModifiers`] übersetzt werden.
pub fn combat_modifiers(research: &Research, owner: PolityId) -> CombatModifiers {
    let effects = research.effects(owner);
    CombatModifiers {
        damage: effects.weapon_damage as f32,
        range: effects.weapon_range as f32,
        shields: effects.shields as f32,
        armor: effects.armor as f32,
        hull: effects.hull as f32,
        speed: effects.ship_speed as f32,
    }
}

/// Moral, mit der die Besatzungen einer Gemeinschaft in ein Gefecht gehen: der Grundwert,
/// gemindert um die Kriegsmüdigkeit daheim (so rechnet es der Gefechtsrechner selbst) und
/// um ausstehenden Sold.
pub fn crew_morale(data: &GameData, finance: &Finance, fleets: &Fleets, owner: PolityId) -> f32 {
    let weariness = finance
        .polities
        .get(owner.index())
        .map_or(0.0, |state| state.war_weariness);
    let weary = CombatantSetup::new(owner, data)
        .with_war_weariness(data, weariness)
        .morale;
    (weary - data.balance.military.morale_per_arrears * fleets.arrears_of(owner)).clamp(0.0, 100.0)
}

/// Womit eine Flotte als Partei in ein Gefecht geht: ihre stehenden Befehle, die Moral
/// ihrer Gemeinschaft und deren Technik, wie sie in diesem Augenblick steht.
pub fn combatant_setup(
    data: &GameData,
    finance: &Finance,
    research: &Research,
    fleets: &Fleets,
    fleet: &Fleet,
) -> CombatantSetup {
    CombatantSetup {
        fleet: Some(fleet.id),
        formation: fleet.formation,
        doctrine: fleet.doctrine,
        modifiers: combat_modifiers(research, fleet.owner),
        morale: crew_morale(data, finance, fleets, fleet.owner),
        ..CombatantSetup::new(fleet.owner, data)
    }
}

/// Hat die Partei noch Schiffe, die steuern und schießen können?
fn on_field(battle: &Battle, side: usize) -> bool {
    battle
        .ships
        .iter()
        .any(|ship| ship.side as usize == side && ship.status.can_act())
}

/// Nimmt eine Flotte als Partei in ein Gefecht auf: mit ihren stehenden Befehlen, der
/// Moral und der Technik ihrer Gemeinschaft und jedem Schiff in dem Zustand, in dem es
/// gerade ist.
fn join(
    data: &GameData,
    finance: &Finance,
    research: &Research,
    diplomacy: &Diplomacy,
    fleets: &mut Fleets,
    battle: &mut Battle,
    index: usize,
) {
    let owner = fleets.list[index].owner;
    let setup = combatant_setup(data, finance, research, fleets, &fleets.list[index]);
    let fleet = &mut fleets.list[index];
    let side = battle.add_combatant(data, setup, &fleet.specs());
    fleet.battle = Some(battle.id);
    fleet.in_battle = true;
    for other in 0..side {
        let hostile = diplomacy.at_war(owner, battle.combatants[other].owner);
        battle.set_hostile(side, other, hostile);
    }
}

/// „A“, „A und B“, „A, B und C“.
fn listed(parts: &[String]) -> String {
    match parts {
        [] => String::new(),
        [only] => only.clone(),
        [first @ .., last] => format!("{} und {last}", first.join(", ")),
    }
}

fn ship_count(count: usize) -> String {
    if count == 1 {
        "einem Schiff".into()
    } else {
        format!("{count} Schiffen")
    }
}

/// Schiffe je Klasse in Worten, die schweren zuerst; `None`, wenn es keine sind.
fn class_counts(counts: &[u32; 6]) -> Option<String> {
    let parts: Vec<String> = ShipClass::ALL
        .iter()
        .rev()
        .filter(|class| counts[class.index()] > 0)
        .map(|class| {
            let count = counts[class.index()];
            format!(
                "{count} {}",
                if count == 1 {
                    class.label()
                } else {
                    class.plural()
                }
            )
        })
        .collect();
    (!parts.is_empty()).then(|| listed(&parts))
}

/// Die Flotten einer Aufzählung, nach Gemeinschaften gebündelt: je Gemeinschaft ihre
/// Flotten und die Zahl ihrer Schiffe.
fn muster(fleets: &Fleets, members: &[usize]) -> Vec<String> {
    let mut owners: Vec<PolityId> = Vec::new();
    for index in members {
        let owner = fleets.list[*index].owner;
        if !owners.contains(&owner) {
            owners.push(owner);
        }
    }
    owners
        .iter()
        .map(|owner| {
            let own: Vec<&Fleet> = members
                .iter()
                .map(|index| &fleets.list[*index])
                .filter(|fleet| fleet.owner == *owner)
                .collect();
            let names: Vec<String> = own.iter().map(|fleet| fleet.name.clone()).collect();
            let ships: usize = own.iter().map(|fleet| fleet.ships.len()).sum();
            format!("{} mit {}", listed(&names), ship_count(ships))
        })
        .collect()
}

/// Stelle der Flotte, die als Partei in einem Gefecht steht, in [`Fleets::list`]: nur,
/// solange es die Flotte noch gibt und sie von diesem Gefecht weiß.
fn attached(fleets: &Fleets, battle: &Battle, side: usize) -> Option<usize> {
    battle.combatants[side]
        .fleet
        .map(FleetId::index)
        .filter(|index| {
            fleets
                .list
                .get(*index)
                .is_some_and(|fleet| fleet.alive && fleet.battle == Some(battle.id))
        })
}

/// Zählt verlorene Schiffe einer Gemeinschaft (ohne Bordjäger).
fn count_lost(ships_lost: &mut Vec<u32>, owner: PolityId, lost: u32) {
    if lost == 0 {
        return;
    }
    if ships_lost.len() <= owner.index() {
        ships_lost.resize(owner.index() + 1, 0);
    }
    ships_lost[owner.index()] += lost;
}

/// Tagesbeginn, vor Marsch, Sold und Einkauf: Die laufenden Gefechte folgen der Kriegslage,
/// ihre bisherigen Folgen stehen in den Flotten, und wer keinen Gegner oder kein eigenes
/// Schiff mehr auf dem Feld hat, scheidet aus.
///
/// - Wer gegen wen kämpft, folgt der Kriegslage: Ein Friedensschluss nimmt zwei Parteien
///   die Feindschaft, eine neue Kriegserklärung unter Anwesenden weitet das Gefecht aus.
/// - Zerstörte Schiffe sind ab jetzt fort, die übrigen tragen Rumpf, Module, Vorräte und
///   Bordjäger, wie der Rechner sie führt. Sold, Stärke und Schiffszahlen rechnen so nie
///   mit Wracks.
/// - Solange andere weiterkämpfen, scheidet aus, wer kein eigenes Schiff mehr auf dem Feld
///   hat oder keinen Gegner mehr. Kämpft ohnehin kein Paar von Gegnern mehr, beendet der
///   Rechner das Gefecht selbst, und [`advance_battles`] schließt es ab.
pub fn muster_battles(
    galaxy: &Galaxy,
    fleets: &mut Fleets,
    battles: &mut Battles,
    diplomacy: &Diplomacy,
    chronicle: &mut Chronicle,
    data: &GameData,
    tick: u64,
) {
    if battles.active.is_empty() {
        return;
    }
    let Battles {
        active,
        departed,
        ships_lost,
        ..
    } = battles;
    for battle in active.iter_mut() {
        let parties = battle.combatants.len();
        for a in 0..parties {
            for b in (a + 1)..parties {
                let hostile =
                    diplomacy.at_war(battle.combatants[a].owner, battle.combatants[b].owner);
                battle.set_hostile(a, b, hostile);
            }
        }

        // Zwischenstand in die Flotten: Was zerstört ist, ist fort. Ein treibendes Schiff
        // bleibt, bis sein Los am Ende feststeht.
        let results = battle.results(data);
        let destroyed: BTreeSet<ShipId> = battle
            .ships
            .iter()
            .filter(|ship| ship.status == ShipStatus::Destroyed)
            .filter_map(|ship| ship.source)
            .collect();
        for side in 0..parties {
            if has_departed(departed, battle.id, side) {
                continue;
            }
            let Some(index) = attached(fleets, battle, side) else {
                continue;
            };
            let lost = write_back(&results, side, &mut fleets.list[index], Some(&destroyed));
            count_lost(ships_lost, battle.combatants[side].owner, lost);
        }

        // Wer scheidet aus? Maßgeblich ist die Lage zu Tagesbeginn.
        let live: Vec<bool> = (0..parties)
            .map(|side| !has_departed(departed, battle.id, side))
            .collect();
        let fighting: Vec<bool> = (0..parties)
            .map(|side| live[side] && battle.is_fighting(side))
            .collect();
        let acting: Vec<bool> = (0..parties)
            .map(|side| live[side] && on_field(battle, side))
            .collect();
        let contested = (0..parties).any(|a| {
            (0..parties).any(|b| battle.is_hostile(a as u16, b as u16) && fighting[a] && acting[b])
        });
        if !contested {
            continue;
        }
        // Noch auf dem Feld: kämpfend, fliehend oder treibend. Für die eigene Partei zählen
        // nur Schiffe der Flotte; Bordjäger ohne ihren Träger sind keine.
        let in_play = |battle: &Battle, side: usize, own_only: bool| {
            battle.ships.iter().any(|ship| {
                ship.side as usize == side
                    && ship.status.in_play()
                    && (!own_only || ship.source.is_some())
            })
        };
        for side in 0..parties {
            if !live[side] {
                continue;
            }
            let cause = if !in_play(battle, side, true) {
                Departure::LeftField
            } else if !(0..parties).any(|other| {
                live[other]
                    && battle.is_hostile(side as u16, other as u16)
                    && in_play(battle, other, false)
            }) {
                Departure::NoOpponent
            } else {
                continue;
            };
            depart(
                battle, side, cause, &fighting, &acting, galaxy, fleets, departed, diplomacy,
                chronicle, data, tick,
            );
        }
    }
    // Stärken nachführen; eine Flotte ohne Schiffe besteht nicht mehr.
    fleets.refresh(data);
}

/// Eine Partei scheidet aus einem laufenden Gefecht aus. Ihre Flotte ist frei; im Rechner
/// bleibt die Partei mit ihrer Bilanz stehen, aber ohne Schiffe auf dem Feld.
///
/// Ohne Gegner bleibt die Flotte im System und ist einsatzbereit. Hat sie das Feld
/// geräumt, sammelt sie sich: im System, solange dort noch jemand für ihre Seite gegen
/// ihre Gegner kämpft, sonst in einem System, in das kein Gegner aus diesem Gefecht
/// ausgewichen ist.
fn depart(
    battle: &mut Battle,
    side: usize,
    cause: Departure,
    fighting: &[bool],
    acting: &[bool],
    galaxy: &Galaxy,
    fleets: &mut Fleets,
    departed: &mut Vec<Departed>,
    diplomacy: &Diplomacy,
    chronicle: &mut Chronicle,
    data: &GameData,
    tick: u64,
) {
    let until = tick / TICKS_PER_DAY + data.balance.military.regroup_days;
    let parties = battle.combatants.len();
    let system = battle.system;
    // Was von der Partei noch auf dem Feld steht, verlässt es: ihre Schiffe, wenn sie
    // keinen Gegner mehr hat, sonst nur noch Bordjäger in der Luft.
    for ship in battle
        .ships
        .iter_mut()
        .filter(|ship| ship.side as usize == side && ship.status.in_play())
    {
        ship.status = ShipStatus::Escaped;
        ship.target = None;
        battle.combatants[side].escaped[ship.class.index()] += 1;
    }
    let combatant = &battle.combatants[side];
    let owner = combatant.owner;
    let losses = class_counts(&combatant.lost);
    let lost = |rest: &str| match &losses {
        Some(losses) => format!("verliert {losses} und {rest}"),
        None => rest.to_string(),
    };
    let index = attached(fleets, battle, side);
    let name = combatant
        .fleet
        .and_then(|id| fleets.get(id))
        .map_or_else(|| "aufgelöste Flotte".into(), |fleet| fleet.name.clone());
    let mut destination = None;
    let fate = match index {
        None => lost("besteht nicht mehr"),
        Some(index) => {
            let fleet = &mut fleets.list[index];
            fleet.battle = None;
            fleet.in_battle = false;
            if fleet.ships.is_empty() {
                lost("besteht nicht mehr")
            } else if cause == Departure::NoOpponent {
                "hat keinen Gegner mehr auf dem Feld und scheidet aus dem Gefecht aus".into()
            } else {
                // Wird für ihre Seite noch gekämpft: von jemandem, mit dem sie nicht im Krieg
                // liegt, gegen einen ihrer Gegner?
                let backed = (0..parties).any(|friend| {
                    friend != side
                        && fighting[friend]
                        && !battle.is_hostile(side as u16, friend as u16)
                        && (0..parties).any(|foe| {
                            acting[foe]
                                && battle.is_hostile(side as u16, foe as u16)
                                && battle.is_hostile(friend as u16, foe as u16)
                        })
                });
                if backed {
                    fleet.regroup_until = Some(until);
                    lost("räumt das Feld; sie sammelt sich hinter den eigenen Linien")
                } else {
                    let avoid: Vec<SystemId> = departed
                        .iter()
                        .filter(|entry| {
                            entry.battle == battle.id && battle.is_hostile(side as u16, entry.side)
                        })
                        .filter_map(|entry| entry.destination)
                        .collect();
                    destination = withdraw(galaxy, fleets, diplomacy, index, &avoid, until);
                    match destination {
                        Some(target) => lost(&format!(
                            "räumt das Feld; sie weicht nach {} aus",
                            galaxy.system(target).name
                        )),
                        None => lost("räumt das Feld; sie sammelt sich"),
                    }
                }
            }
        }
    };
    chronicle.push(
        tick,
        ChronicleKind::War,
        Some(owner),
        Some(system),
        format!("Gefecht bei {}: {name} {fate}.", galaxy.system(system).name),
    );
    departed.push(Departed {
        battle: battle.id,
        side: side as u16,
        fleet: battle.combatants[side].fleet,
        tick,
        cause,
        destination,
    });
}

/// Tagesbeginn: Sammelfristen laufen ab, Trümmer verblassen, Verstärkungen greifen ein, und
/// wo sich einsatzbereite Flotten verfeindeter Gemeinschaften gegenüberstehen, beginnt ein
/// Gefecht. Dass laufende Gefechte der Kriegslage folgen, besorgt [`muster_battles`].
pub fn open_battles(
    galaxy: &Galaxy,
    fleets: &mut Fleets,
    battles: &mut Battles,
    diplomacy: &Diplomacy,
    finance: &Finance,
    research: &Research,
    chronicle: &mut Chronicle,
    data: &GameData,
    seed: u64,
    tick: u64,
) {
    let balance = &data.balance.military;
    let day = tick / TICKS_PER_DAY;
    for fleet in &mut fleets.list {
        if fleet.regroup_until.is_some_and(|until| until <= day) {
            fleet.regroup_until = None;
        }
    }
    if day.is_multiple_of(DAYS_PER_MONTH) && !battles.debris.is_empty() {
        for debris in &mut battles.debris {
            debris.amount *= 1.0 - balance.debris_fade_per_month;
        }
        battles
            .debris
            .retain(|debris| debris.amount >= DEBRIS_FLOOR);
    }

    // Einsatzbereite Flotten je System, in Nummernfolge.
    let mut present: BTreeMap<SystemId, Vec<usize>> = BTreeMap::new();
    for (index, fleet) in fleets.list.iter().enumerate() {
        if fleet.is_docked() && fleet.is_ready() {
            present.entry(fleet.system).or_default().push(index);
        }
    }
    for (system, here) in present {
        let system_name = &galaxy.system(system).name;
        if let Some(position) = battles
            .active
            .iter()
            .position(|battle| battle.system == system)
        {
            // Verstärkung: wer mit einer Partei im Krieg liegt, die noch auf dem Feld steht.
            // Wer in diesem Gefecht das Feld schon geräumt hat, kehrt nicht von selbst
            // zurück, auch wenn seine Sammelfrist inzwischen abgelaufen ist.
            let battle = &mut battles.active[position];
            for index in here {
                let owner = fleets.list[index].owner;
                let gave_way = battles.departed.iter().any(|entry| {
                    entry.battle == battle.id
                        && entry.fleet == Some(fleets.list[index].id)
                        && entry.cause == Departure::LeftField
                });
                let has_enemy = (0..battle.combatants.len()).any(|side| {
                    diplomacy.at_war(owner, battle.combatants[side].owner) && on_field(battle, side)
                });
                if gave_way || !has_enemy {
                    continue;
                }
                join(data, finance, research, diplomacy, fleets, battle, index);
                let fleet = &fleets.list[index];
                chronicle.push(
                    tick,
                    ChronicleKind::War,
                    Some(owner),
                    Some(system),
                    format!(
                        "{} greift mit {} in das Gefecht bei {system_name} ein.",
                        fleet.name,
                        ship_count(fleet.ships.len())
                    ),
                );
            }
            continue;
        }
        let at_war =
            |a: usize, b: usize| diplomacy.at_war(fleets.list[a].owner, fleets.list[b].owner);
        let mut order: Vec<usize> = here
            .iter()
            .copied()
            .filter(|a| here.iter().any(|b| at_war(*a, *b)))
            .collect();
        if order.is_empty() {
            continue;
        }
        let id = BattleId(battles.next_id);
        battles.next_id += 1;
        let battle_seed =
            Rng::stream(seed, Domain::Battle, id.0 as u64, system.0 as u64).next_u64();
        // Die Parteien stehen in Nummernfolge der Flotten; der Rechner behandelt alle
        // gleich. Nur die ersten beiden stellt er einander gegenüber auf: Deshalb rückt
        // der erste Gegner der ersten Flotte an die zweite Stelle.
        let listed_by_number = order.clone();
        if let Some(foe) = order.iter().position(|other| at_war(order[0], *other)) {
            let foe = order.remove(foe);
            order.insert(1, foe);
        }
        let mut battle = Battle::new(id, system, battle_seed);
        for index in &order {
            join(
                data,
                finance,
                research,
                diplomacy,
                fleets,
                &mut battle,
                *index,
            );
        }
        let sides = muster(fleets, &listed_by_number);
        let text = if sides.len() == 2 {
            format!(
                "Bei {system_name} beginnt ein Gefecht: {} gegen {}.",
                sides[0], sides[1]
            )
        } else {
            format!(
                "Bei {system_name} beginnt ein Gefecht mehrerer Gemeinschaften: {}.",
                sides.join("; ")
            )
        };
        chronicle.push(tick, ChronicleKind::War, None, Some(system), text);
        battles.active.push(battle);
    }
}

/// Jeder Takt: Alle laufenden Gefechte rücken um einen Schritt vor, in der Reihenfolge
/// ihres Beginns; beendete werden abgeschlossen und ihr Ergebnis in die Flotten
/// zurückgeschrieben.
pub fn advance_battles(
    galaxy: &Galaxy,
    fleets: &mut Fleets,
    battles: &mut Battles,
    diplomacy: &Diplomacy,
    finance: &mut Finance,
    research: &mut Research,
    logistics: &Logistics,
    polities: &Polities,
    chronicle: &mut Chronicle,
    data: &GameData,
    tick: u64,
) {
    if battles.active.is_empty() {
        return;
    }
    for battle in &mut battles.active {
        battle.step(data);
    }
    if !battles.active.iter().any(|battle| battle.finished) {
        return;
    }
    let (finished, running): (Vec<Battle>, Vec<Battle>) = std::mem::take(&mut battles.active)
        .into_iter()
        .partition(|battle| battle.finished);
    battles.active = running;
    for battle in &finished {
        conclude(
            battle, galaxy, fleets, battles, diplomacy, finance, research, logistics, polities,
            chronicle, data, tick,
        );
    }
    // Stärken nachführen; eine Flotte ohne Schiffe besteht nicht mehr.
    fleets.refresh(data);
}

/// Schreibt in die Flotte einer Partei zurück, was der Rechner über ihre Schiffe sagt
/// (`results` aus [`Battle::results`]): Verlorene sind fort; alle anderen tragen Rumpf,
/// Module, Munition und Treibstoff, wie das Gefecht sie zurückließ, und die Bordjäger, die
/// der Rechner auf die Träger verteilt hat. Ein Schiff, das nicht im Gefecht stand, bleibt,
/// wie es ist. Gibt die Zahl der verlorenen Schiffe zurück (ohne Bordjäger).
///
/// `settled` nennt die Schiffe, deren Verlust schon feststeht. Am Ende des Gefechts sind
/// das alle verlorenen (`None`); solange es läuft, nur die zerstörten: Ein treibendes
/// Schiff kann noch geborgen werden und bleibt bis dahin, wie es zuletzt war.
fn write_back(
    results: &[ShipResult],
    side: usize,
    fleet: &mut Fleet,
    settled: Option<&BTreeSet<ShipId>>,
) -> u32 {
    let outcomes: BTreeMap<ShipId, &ShipResult> = results
        .iter()
        .filter(|result| result.side as usize == side)
        .map(|result| (result.source, result))
        .collect();
    let mut lost = 0;
    fleet.ships.retain_mut(|ship| {
        let Some(result) = outcomes.get(&ship.id) else {
            return true;
        };
        if result.fate == Fate::Lost {
            if settled.is_some_and(|settled| !settled.contains(&ship.id)) {
                return true;
            }
            lost += 1;
            return false;
        }
        ship.hull = result.hull;
        // Eine leere Liste heißt: alle Module heil.
        ship.modules = if result.modules.iter().all(|module| *module >= 1.0) {
            Vec::new()
        } else {
            result.modules.clone()
        };
        ship.ammo = result.ammo;
        ship.fuel = result.fuel;
        ship.fighters = result.fighters;
        true
    });
    lost
}

/// Steht am Ende noch ein Schiff der Partei auf dem Feld? Das allein heißt hier „das Feld
/// halten“. Bordjäger zählen nicht: Ohne ihren Träger gehören sie zu keinem Schiff der
/// Flotte mehr.
fn holds_field(results: &[ShipResult], side: usize) -> bool {
    results
        .iter()
        .any(|result| result.side as usize == side && result.fate == Fate::Holding)
}

/// Wohin eine Flotte ausweicht, die das Feld räumt: dorthin, woher sie kam, wenn das ein
/// Nachbarsystem ist; sonst in ein Nachbarsystem der eigenen Gemeinschaft; sonst in eines
/// ohne feindliche Flotten; sonst in das mit der kleinsten Nummer. Die Systeme in `avoid`
/// kommen nicht in Frage: Dorthin weichen schon Gegner aus demselben Gefecht aus. `None`,
/// wenn kein Nachbarsystem bleibt; dann führt [`withdrawal_route`] weiter.
pub fn retreat_target(
    galaxy: &Galaxy,
    fleets: &Fleets,
    diplomacy: &Diplomacy,
    fleet: &Fleet,
    avoid: &[SystemId],
) -> Option<SystemId> {
    let neighbors: Vec<SystemId> = galaxy
        .neighbors(fleet.system)
        .iter()
        .map(|neighbor| neighbor.system)
        .filter(|system| !avoid.contains(system))
        .collect();
    if let Some(origin) = fleet.came_from.filter(|origin| neighbors.contains(origin)) {
        return Some(origin);
    }
    let own = neighbors
        .iter()
        .find(|system| galaxy.system_owner(**system) == Some(fleet.owner));
    let quiet = || {
        neighbors.iter().find(|system| {
            !fleets
                .docked_in(**system)
                .any(|other| diplomacy.at_war(other.owner, fleet.owner))
        })
    };
    own.or_else(quiet).or(neighbors.first()).copied()
}

/// Weg, auf dem eine Flotte das System räumt: in das Nachbarsystem nach
/// [`retreat_target`]. Weichen schon in jedes Nachbarsystem Gegner aus demselben Gefecht
/// aus (`avoid`), führt der Weg weiter: in das nächste System (nach Sprüngen, dann in der
/// Reihenfolge der Suche), das noch keiner von ihnen gewählt hat, notfalls durch ihre
/// Systeme hindurch. Leer nur, wenn es in der ganzen Galaxie kein solches System gibt; dann
/// sammelt sich die Flotte, wo sie steht.
pub fn withdrawal_route(
    galaxy: &Galaxy,
    fleets: &Fleets,
    diplomacy: &Diplomacy,
    fleet: &Fleet,
    avoid: &[SystemId],
) -> Vec<SystemId> {
    if let Some(target) = retreat_target(galaxy, fleets, diplomacy, fleet, avoid) {
        return vec![target];
    }
    let start = fleet.system;
    let mut previous: Vec<Option<SystemId>> = vec![None; galaxy.systems.len()];
    let mut seen = vec![false; galaxy.systems.len()];
    let mut queue = VecDeque::new();
    seen[start.index()] = true;
    queue.push_back(start);
    while let Some(current) = queue.pop_front() {
        for neighbor in galaxy.neighbors(current) {
            let next = neighbor.system;
            if seen[next.index()] {
                continue;
            }
            seen[next.index()] = true;
            previous[next.index()] = Some(current);
            if !avoid.contains(&next) {
                let mut route = vec![next];
                let mut cursor = next;
                while let Some(before) = previous[cursor.index()].filter(|before| *before != start)
                {
                    route.push(before);
                    cursor = before;
                }
                route.reverse();
                return route;
            }
            queue.push_back(next);
        }
    }
    Vec::new()
}

/// Lässt eine Flotte das System räumen und sich bis zum Tag `until` sammeln. Gibt das Ziel
/// zurück; `None`, wenn kein System bleibt und sie sich an Ort und Stelle sammelt.
fn withdraw(
    galaxy: &Galaxy,
    fleets: &mut Fleets,
    diplomacy: &Diplomacy,
    index: usize,
    avoid: &[SystemId],
    until: u64,
) -> Option<SystemId> {
    let route = withdrawal_route(galaxy, fleets, diplomacy, &fleets.list[index], avoid);
    let fleet = &mut fleets.list[index];
    fleet.regroup_until = Some(until);
    let target = route.last().copied();
    if !route.is_empty() {
        fleet.route = route;
        fleet.progress = 0.0;
    }
    target
}

/// Was aus einer Flotte wird, wenn ihr Gefecht endet.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Aftermath {
    /// Bleibt im System und ist einsatzbereit.
    Stays,
    /// Bleibt im System, das die eigene Seite hält, und sammelt sich dort.
    Rallies,
    /// Räumt das System und sammelt sich.
    Leaves,
}

/// Schließt ein beendetes Gefecht ab: Rückgabe an die Flotten, Abzug derer, die das
/// System räumen, Bilanz, Trümmer, Kriegsmüdigkeit, Kursschock, Einblick in die Technik
/// des Gegners, Chronik.
fn conclude(
    battle: &Battle,
    galaxy: &Galaxy,
    fleets: &mut Fleets,
    battles: &mut Battles,
    diplomacy: &Diplomacy,
    finance: &mut Finance,
    research: &mut Research,
    logistics: &Logistics,
    polities: &Polities,
    chronicle: &mut Chronicle,
    data: &GameData,
    tick: u64,
) {
    let balance = &data.balance.military;
    let until = tick / TICKS_PER_DAY + balance.regroup_days;
    let system = battle.system;
    let system_name = &galaxy.system(system).name;
    let home = galaxy.system_owner(system);
    let parties = battle.combatants.len();
    // Was aus jedem Schiff geworden ist, sagt der Rechner.
    let results = battle.results(data);
    // Wer vorzeitig ausgeschieden ist, steht nicht mehr auf dem Feld.
    let left: Vec<Option<Departed>> = (0..parties)
        .map(|side| {
            battles
                .departed
                .iter()
                .find(|entry| entry.battle == battle.id && entry.side as usize == side)
                .cloned()
        })
        .collect();
    let holding: Vec<bool> = (0..parties)
        .map(|side| left[side].is_none() && holds_field(&results, side))
        .collect();
    // Je Seite gefragt: Hält ein Gegner das Feld, oder jemand von der eigenen Seite?
    let foe_holds = |side: usize| {
        (0..parties).any(|other| holding[other] && battle.is_hostile(side as u16, other as u16))
    };
    // Die eigene Seite ist, wer einen Gegner mit der Partei teilt. Wer nur nicht (oder
    // nach einem Friedensschluss nicht mehr) ihr Gegner ist, deckt ihren Rückzug nicht.
    let friend_holds = |side: usize| {
        (0..parties).any(|other| {
            other != side
                && holding[other]
                && !battle.is_hostile(side as u16, other as u16)
                && (0..parties).any(|foe| {
                    battle.is_hostile(side as u16, foe as u16)
                        && battle.is_hostile(other as u16, foe as u16)
                })
        })
    };

    let mut tally: BTreeMap<PolityId, OwnerTally> = BTreeMap::new();
    let mut sides: Vec<SideReport> = Vec::with_capacity(parties);
    let mut fates: Vec<String> = Vec::new();
    let mut debris = 0.0f32;
    // Wohin die Parteien ausweichen, die das System räumen; die schon früher ausgewichen
    // sind, eingeschlossen.
    let mut withdrawn: Vec<(usize, SystemId)> = left
        .iter()
        .enumerate()
        .filter_map(|(side, entry)| Some((side, entry.as_ref()?.destination?)))
        .collect();
    for (side, combatant) in battle.combatants.iter().enumerate() {
        let held = holding[side];
        // Die Bilanz führt der Rechner: Verluste samt den Bordjägern, die mit ihrem Träger
        // untergingen oder keinen Hangar mehr fanden; im Bauwert zählt jeder Jäger einmal.
        let mut report = SideReport {
            owner: combatant.owner,
            fleet: combatant.fleet,
            fleet_name: combatant
                .fleet
                .and_then(|id| fleets.get(id))
                .map_or_else(|| "aufgelöste Flotte".into(), |fleet| fleet.name.clone()),
            initial: combatant.initial,
            lost: combatant.lost,
            escaped: combatant.escaped,
            initial_value: combatant.initial_value,
            lost_value: combatant.lost_value,
            held_field: held,
            retreated: combatant.retreating,
            stalemate: combatant.stalemate,
            left_early: None,
        };
        let owner = tally.entry(combatant.owner).or_default();
        owner.lost += report.lost_value as f64;
        debris += report.lost_value;
        if let Some(entry) = &left[side] {
            // Schon früher ausgeschieden: Die Flotte ist längst frei, es bleibt die Bilanz.
            report.held_field = entry.cause == Departure::NoOpponent;
            report.left_early = Some(entry.tick);
            owner.beaten |= entry.cause == Departure::LeftField && foe_holds(side);
            sides.push(report);
            continue;
        }
        owner.held |= held;
        owner.beaten |= !held && foe_holds(side);
        let Some(index) = attached(fleets, battle, side) else {
            // Die Flotte gibt es nicht mehr: Es bleibt bei der Bilanz.
            fates.push(format!("{} besteht nicht mehr.", report.fleet_name));
            sides.push(report);
            continue;
        };
        let ships_lost = write_back(&results, side, &mut fleets.list[index], None);
        count_lost(&mut battles.ships_lost, combatant.owner, ships_lost);

        let fleet = &mut fleets.list[index];
        fleet.battle = None;
        fleet.in_battle = false;
        let losses = class_counts(&report.lost);
        let at_home = home == Some(combatant.owner);
        let aftermath = if battle.timed_out && held {
            // An der Zeitgrenze noch auf dem Feld: ohne Entscheidung. Es bleibt nur, wer im
            // eigenen System steht.
            if at_home {
                Aftermath::Stays
            } else {
                Aftermath::Leaves
            }
        } else if held {
            Aftermath::Stays
        } else if foe_holds(side) {
            // Das Feld einem Gegner überlassen: geschlagen.
            Aftermath::Leaves
        } else if friend_holds(side) {
            Aftermath::Rallies
        } else if combatant.stalemate && at_home {
            // Im Patt gelöst, und niemand hält das Feld: Wer zu Hause ist, bleibt.
            Aftermath::Stays
        } else {
            Aftermath::Leaves
        };
        let fate = if fleet.ships.is_empty() {
            match &losses {
                Some(losses) => format!("verliert {losses} und besteht nicht mehr"),
                None => "besteht nicht mehr".into(),
            }
        } else {
            match aftermath {
                Aftermath::Stays => match (&losses, combatant.stalemate) {
                    (Some(losses), false) => format!("verliert {losses}"),
                    (None, false) => "bleibt ohne Verluste".into(),
                    (Some(losses), true) => {
                        format!("verliert {losses}, löst sich und bleibt im eigenen System")
                    }
                    (None, true) => "löst sich ohne Verluste und bleibt im eigenen System".into(),
                },
                Aftermath::Rallies => {
                    // Die eigene Seite hält das Feld: Die Flotte muss das System nicht
                    // räumen, ist aber aus dem Spiel, bis sie sich gesammelt hat.
                    fleet.regroup_until = Some(until);
                    match &losses {
                        Some(losses) => {
                            format!("verliert {losses} und sammelt sich hinter den eigenen Linien")
                        }
                        None => "sammelt sich ohne Verluste hinter den eigenen Linien".into(),
                    }
                }
                Aftermath::Leaves => {
                    // Wer das System räumt, sammelt sich. Gegner aus demselben Gefecht
                    // weichen nie in dasselbe System aus: Sie stünden einander sonst nach
                    // der Sammelfrist wieder gegenüber, und ein Patt wiederholte sich ohne
                    // Ende. Die Partei mit der kleineren Nummer wählt zuerst; den anderen
                    // bleibt notfalls ein weiterer Weg.
                    let avoid: Vec<SystemId> = withdrawn
                        .iter()
                        .filter(|(other, _)| battle.is_hostile(side as u16, *other as u16))
                        .map(|(_, target)| *target)
                        .collect();
                    let target = withdraw(galaxy, fleets, diplomacy, index, &avoid, until);
                    if let Some(target) = target {
                        withdrawn.push((side, target));
                    }
                    let place = match target {
                        Some(target) => format!("weicht nach {} aus", galaxy.system(target).name),
                        None => "sammelt sich".into(),
                    };
                    match &losses {
                        Some(losses) => format!("verliert {losses} und {place}"),
                        None => format!("{place}, ohne Verluste"),
                    }
                }
            }
        };
        fates.push(format!("{} {fate}.", report.fleet_name));
        sides.push(report);
    }

    // Wer früher das Feld geräumt und sich hinter den eigenen Linien gesammelt hat, räumt
    // jetzt das System, wenn am Ende doch ein Gegner das Feld hält.
    for (side, entry) in left.iter().enumerate() {
        let Some(entry) = entry else {
            continue;
        };
        if entry.cause != Departure::LeftField || entry.destination.is_some() || !foe_holds(side) {
            continue;
        }
        let rallying = entry.fleet.map(FleetId::index).filter(|index| {
            fleets.list.get(*index).is_some_and(|fleet| {
                fleet.alive
                    && !fleet.in_battle
                    && fleet.system == system
                    && fleet.route.is_empty()
                    && fleet.is_regrouping()
            })
        });
        let Some(index) = rallying else {
            continue;
        };
        let avoid: Vec<SystemId> = withdrawn
            .iter()
            .filter(|(other, _)| battle.is_hostile(side as u16, *other as u16))
            .map(|(_, target)| *target)
            .collect();
        if let Some(target) = withdraw(galaxy, fleets, diplomacy, index, &avoid, until) {
            withdrawn.push((side, target));
            fates.push(format!(
                "{} sammelte sich im System und weicht nun nach {} aus.",
                fleets.list[index].name,
                galaxy.system(target).name
            ));
        }
    }

    // Verluste machen kriegsmüde, im Verhältnis zum Anteil an der ganzen Flotte; eine
    // schwere Niederlage erschüttert dazu die Kurse. Eine Niederlage ist es nur, wenn das
    // Gefecht gegen die Gemeinschaft entschieden wurde: Eine ihrer Flotten hat das Feld
    // einem Gegner überlassen, und keine hält es.
    let mut shaken: Vec<PolityId> = Vec::new();
    for (polity, owner) in &tally {
        if owner.lost <= 0.0 {
            continue;
        }
        // Die Flotte der Gemeinschaft, wie sie in das Gefecht ging: was sie jetzt noch
        // hat, und was sie darin verlor. Gemessen wird mindestens an einer Startflotte:
        // Sonst wöge das letzte Schiff einer aufgeriebenen Flotte so schwer wie einst die
        // ganze, und jede einzelne Korvette wäre eine schwere Niederlage.
        let navy = owner.lost
            + fleets
                .of(*polity)
                .flat_map(|fleet| fleet.ships.iter())
                .map(|ship| material_value(data, ship))
                .sum::<f64>();
        let share = loss_share(data, owner.lost, navy);
        if let Some(state) = finance.polities.get_mut(polity.index()) {
            state.war_weariness =
                (state.war_weariness + balance.weariness_per_navy_lost * share).clamp(0.0, 100.0);
        }
        if owner.beaten && !owner.held && share >= balance.heavy_defeat_share {
            crate::finance::shares::shock_polity(
                finance,
                galaxy,
                logistics,
                *polity,
                balance.defeat_shock,
            );
            shaken.push(*polity);
        }
    }

    // Wer gegeneinander gekämpft hat, hat die Technik des anderen am Werk gesehen: Jedes
    // Paar verfeindeter Gemeinschaften, von dem eine Flotte in Gefechtsreichweite kam,
    // gewinnt Einblick in die Forschung des jeweils anderen.
    let mut observed: BTreeSet<(PolityId, PolityId)> = BTreeSet::new();
    for a in 0..parties {
        for b in (a + 1)..parties {
            let (first, second) = (&battle.combatants[a], &battle.combatants[b]);
            if first.owner != second.owner
                && battle.is_hostile(a as u16, b as u16)
                && (first.contact || second.contact)
            {
                observed.insert((first.owner.min(second.owner), first.owner.max(second.owner)));
            }
        }
    }
    for (first, second) in observed {
        let insight = data.balance.research.intel_per_battle;
        research.add_intel(first, second, insight);
        research.add_intel(second, first, insight);
    }

    if debris > 0.0 {
        match battles
            .debris
            .binary_search_by_key(&system, |entry| entry.system)
        {
            Ok(found) => {
                battles.debris[found].amount += debris;
                battles.debris[found].tick = tick;
            }
            Err(place) => battles.debris.insert(
                place,
                Debris {
                    system,
                    amount: debris,
                    tick,
                },
            ),
        }
    }

    // Führt unter den Beteiligten niemand mehr Krieg, hat ein Friedensschluss das Gefecht
    // beendet.
    let at_peace =
        !(0..parties).any(|a| (0..parties).any(|b| battle.is_hostile(a as u16, b as u16)));
    let outcome = if battle.timed_out {
        BattleOutcome::TimedOut
    } else if at_peace {
        BattleOutcome::CalledOff
    } else if holding.iter().any(|held| *held) {
        BattleOutcome::Decided
    } else if sides
        .iter()
        .any(|side| side.stalemate && side.left_early.is_none())
    {
        BattleOutcome::Stalemate
    } else {
        BattleOutcome::Abandoned
    };
    let report = BattleReport {
        id: battle.id,
        system,
        // Jedes Gefecht rechnet vom Takt seines Beginns an einen Schritt je Takt.
        started: (tick + 1).saturating_sub(battle.tick),
        ended: tick,
        outcome,
        sides,
    };
    let holders: Vec<&SideReport> = report
        .sides
        .iter()
        .zip(&holding)
        .filter(|(_, held)| **held)
        .map(|(side, _)| side)
        .collect();
    let duration = match report.days() {
        1 => "einem Tag".to_string(),
        days => format!("{days} Tagen"),
    };
    let headline = match outcome {
        BattleOutcome::TimedOut => {
            format!("Gefecht bei {system_name} nach {duration} ohne Entscheidung abgebrochen.")
        }
        BattleOutcome::CalledOff => format!(
            "Gefecht bei {system_name} nach {duration} eingestellt: Zwischen den Beteiligten herrscht kein Krieg mehr."
        ),
        BattleOutcome::Stalemate => format!(
            "Gefecht bei {system_name} nach {duration} im Patt beendet: Keine Seite kommt gegen die andere an."
        ),
        BattleOutcome::Abandoned => {
            format!("Gefecht bei {system_name} nach {duration} beendet: Niemand hält das Feld.")
        }
        BattleOutcome::Decided => {
            let names: Vec<String> = holders.iter().map(|side| side.fleet_name.clone()).collect();
            format!(
                "Gefecht bei {system_name} nach {duration} entschieden: {} {} das Feld.",
                listed(&names),
                if names.len() == 1 { "hält" } else { "halten" }
            )
        }
    };
    // Die Farbe des Siegers, wenn es genau einen gibt.
    let victor = holders.first().map(|side| side.owner).filter(|owner| {
        outcome == BattleOutcome::Decided && holders.iter().all(|side| side.owner == *owner)
    });
    chronicle.push(
        tick,
        ChronicleKind::War,
        victor,
        Some(system),
        format!("{headline} {}", fates.join(" ")),
    );
    for polity in shaken {
        chronicle.push(
            tick,
            ChronicleKind::Finance,
            Some(polity),
            Some(system),
            format!(
                "Die Niederlage bei {system_name} erschüttert das Vertrauen in {}: Die Kurse geben nach.",
                polities.get(polity).name
            ),
        );
    }
    battles.departed.retain(|entry| entry.battle != battle.id);
    if let Some(index) = BattleOutcome::ALL
        .iter()
        .position(|other| *other == outcome)
    {
        battles.outcomes[index] += 1;
    }
    if outcome == BattleOutcome::Decided {
        let winners: BTreeSet<PolityId> = report
            .sides
            .iter()
            .filter(|side| side.held_field)
            .map(|side| side.owner)
            .collect();
        for winner in winners {
            if battles.won.len() <= winner.index() {
                battles.won.resize(winner.index() + 1, 0);
            }
            battles.won[winner.index()] += 1;
        }
    }
    for side in &report.sides {
        let ships = |counts: &[u32; 6]| counts.iter().sum::<u32>() - counts[0];
        if ships(&side.initial) > 0 && ships(&side.lost) >= ships(&side.initial) {
            let index = side.owner.index();
            if battles.fleets_lost.len() <= index {
                battles.fleets_lost.resize(index + 1, 0);
            }
            battles.fleets_lost[index] += 1;
        }
    }
    if battles.reports.len() >= Battles::REPORTS {
        battles.reports.remove(0);
    }
    battles.reports.push(report);
}

/// Anteil eines Verlusts an der Flotte einer Gemeinschaft (0..1): der Maßstab für
/// Kriegsmüdigkeit und schwere Niederlage. `navy` ist der Bauwert ihrer ganzen Flotte vor
/// dem Gefecht; gemessen wird mindestens am Bauwert der Startflotte.
pub fn loss_share(data: &GameData, lost: f64, navy: f64) -> f64 {
    let reference = navy.max(data.balance.military.starting_fleet_value);
    if reference > 0.0 {
        (lost / reference).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Bilanz einer Gemeinschaft über alle ihre Flotten in einem Gefecht.
#[derive(Clone, Copy, Debug, Default)]
struct OwnerTally {
    /// Bauwert dessen, was sie in dem Gefecht verlor, in Credits.
    lost: f64,
    /// Mindestens eine ihrer Flotten hielt am Ende das Feld.
    held: bool,
    /// Mindestens eine ihrer Flotten hat das Feld einem Gegner überlassen.
    beaten: bool,
}

/// Reicht geänderte Befehle einer Flotte an das Gefecht weiter, in dem sie steht.
pub fn relay_orders(
    data: &GameData,
    battles: &mut Battles,
    fleet: FleetId,
    formation: Formation,
    doctrine: Doctrine,
) {
    if let Some((battle, side)) = battles.side_mut(fleet) {
        battle.set_orders(data, side, formation, doctrine);
    }
}

/// Befiehlt einer eigenen Flotte, das Gefecht abzubrechen: Ihre Schiffe verlassen das Feld.
pub fn order_retreat(
    fleets: &Fleets,
    battles: &mut Battles,
    polity: PolityId,
    fleet: FleetId,
) -> Result<(), String> {
    let Some(found) = fleets.get(fleet) else {
        return Err("Flotte unbekannt".into());
    };
    if !found.alive || found.owner != polity {
        return Err("Flotte gehört einer anderen Gemeinschaft oder besteht nicht mehr".into());
    }
    let Some((battle, side)) = battles.side_mut(fleet).filter(|_| found.in_battle) else {
        return Err(format!("{} steht in keinem Gefecht", found.name));
    };
    if battle.combatants[side].retreating {
        return Err(format!("{} ist bereits auf dem Rückzug", found.name));
    }
    battle.order_retreat(side);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::testing::setup;
    use super::super::{Ship, material_value};
    use super::*;
    use crate::battle::ShipStatus;
    use crate::data::TechBranch;

    /// Niemand hat geforscht: Alle kämpfen mit neutralen Werten.
    fn no_research() -> Research {
        Research::default()
    }

    /// Ein Gefecht zweier Flotten, die einander feind sind, noch vor dem ersten Schritt. Es
    /// findet im System der ersten statt.
    fn duel(data: &GameData, fleets: &mut Fleets, first: usize, second: usize) -> Battle {
        let mut diplomacy = Diplomacy::default();
        diplomacy
            .wars
            .insert((fleets.list[first].owner, fleets.list[second].owner));
        let finance = Finance::default();
        fleets.list[second].system = fleets.list[first].system;
        let mut battle = Battle::new(BattleId(0), fleets.list[first].system, 1);
        for index in [first, second] {
            join(
                data,
                &finance,
                &no_research(),
                &diplomacy,
                fleets,
                &mut battle,
                index,
            );
        }
        battle
    }

    /// Platz eines Schiffs der strategischen Ebene im Gefecht.
    fn place(battle: &Battle, id: ShipId) -> usize {
        battle
            .ships
            .iter()
            .position(|fought| fought.source == Some(id))
            .expect("das Schiff steht im Gefecht")
    }

    /// Setzt alle Schiffe einer Partei, die noch kämpfen, auf einen anderen Stand.
    fn set_active(battle: &mut Battle, side: u16, status: ShipStatus) {
        for ship in &mut battle.ships {
            if ship.side == side && ship.status == ShipStatus::Active {
                ship.status = status;
            }
        }
    }

    #[test]
    fn a_fleet_joins_with_its_orders_its_ships_and_the_morale_of_its_community() {
        let (data, _, _, mut fleets) = setup();
        fleets.list[0].formation = Formation::Wedge;
        fleets.list[0].doctrine = Doctrine::Aggressive;
        fleets.list[0].ships[1].hull = 0.5;
        fleets.list[0].ships[2].ammo = 0.25;
        fleets.arrears = vec![0.0, 0.5];
        let mut finance = Finance {
            polities: vec![Default::default(); 2],
            ..Default::default()
        };
        finance.polities[0].war_weariness = 40.0;
        let battle_balance = &data.balance.battle;
        // Die Kriegsmüdigkeit rechnet der Gefechtsrechner ein, der Soldrückstand kommt dazu.
        let weary = CombatantSetup::new(PolityId(0), &data)
            .with_war_weariness(&data, 40.0)
            .morale;
        assert_eq!(
            weary,
            battle_balance.morale_start - 40.0 * battle_balance.morale_per_weariness
        );
        let unpaid = battle_balance.morale_start - 0.5 * data.balance.military.morale_per_arrears;
        assert_eq!(crew_morale(&data, &finance, &fleets, PolityId(0)), weary);
        assert_eq!(crew_morale(&data, &finance, &fleets, PolityId(1)), unpaid);
        assert!(weary < battle_balance.morale_start && unpaid < battle_balance.morale_start);
        // Eine Gemeinschaft ohne Finanzdaten (frisch abgespalten) kämpft mit dem Grundwert.
        assert_eq!(
            crew_morale(&data, &finance, &fleets, PolityId(7)),
            battle_balance.morale_start
        );
        // Beides zusammen drückt nie unter null.
        finance.polities[1].war_weariness = 100.0;
        fleets.arrears = vec![0.0, 1.0];
        let worst = battle_balance.morale_start
            - 100.0 * battle_balance.morale_per_weariness
            - data.balance.military.morale_per_arrears;
        assert_eq!(
            crew_morale(&data, &finance, &fleets, PolityId(1)),
            worst.max(0.0)
        );
        finance.polities[1].war_weariness = 0.0;
        fleets.arrears = vec![0.0, 0.5];

        let mut diplomacy = Diplomacy::default();
        diplomacy.wars.insert((PolityId(0), PolityId(1)));
        let mut battle = Battle::new(BattleId(3), fleets.list[0].system, 1);
        for index in [0, 1, 2] {
            join(
                &data,
                &finance,
                &no_research(),
                &diplomacy,
                &mut fleets,
                &mut battle,
                index,
            );
        }
        let first = &battle.combatants[0];
        assert_eq!((first.fleet, first.owner), (Some(FleetId(0)), PolityId(0)));
        assert_eq!(
            (first.formation, first.doctrine),
            (Formation::Wedge, Doctrine::Aggressive)
        );
        assert_eq!(first.morale, weary);
        assert_eq!(battle.combatants[1].morale, unpaid);
        assert_eq!(first.modifiers, CombatModifiers::default());
        // Feindschaft folgt der Kriegslage: Der Dritte ist mit niemandem im Krieg.
        assert!(battle.is_hostile(0, 1) && battle.is_hostile(1, 0));
        assert!(!battle.is_hostile(0, 2) && !battle.is_hostile(1, 2));
        // Jedes Schiff ist mit seiner Nummer und seinem Zustand dabei.
        let ships = fleets.list[0].ships.clone();
        for ship in &ships {
            let fought = &battle.ships[place(&battle, ship.id)];
            assert_eq!((fought.side, fought.design), (0, ship.design));
            assert!((fought.hull - ship.hull * fought.hull_max).abs() < 1e-3);
            assert!((fought.ammo - ship.ammo * fought.ammo_max).abs() < 1e-3);
        }
        for index in [0, 1, 2] {
            assert_eq!(fleets.list[index].battle, Some(BattleId(3)));
            assert!(fleets.list[index].in_battle);
        }
    }

    #[test]
    fn the_technology_of_a_community_becomes_the_modifiers_of_its_ships() {
        let (data, _, _, mut fleets) = setup();
        let mut research = Research::default();
        research.sync(&data, 2);
        assert_eq!(
            combat_modifiers(&research, PolityId(0)),
            CombatModifiers::default()
        );
        // Wer keinen Eintrag hat, kämpft neutral.
        assert_eq!(
            combat_modifiers(&research, PolityId(99)),
            CombatModifiers::default()
        );
        // Gemeinschaft 0 beherrscht Waffen, Verteidigung und Antrieb bis zur dritten Stufe.
        for key in ["waffen_3", "verteidigung_3", "antrieb_3"] {
            assert!(research.grant(&data, PolityId(0), data.tech_index(key).unwrap()));
        }
        assert_eq!(research.tier(&data, PolityId(0), TechBranch::Weapons), 3);
        let effects = research.effects(PolityId(0));
        let modifiers = combat_modifiers(&research, PolityId(0));
        assert_eq!(modifiers.damage, effects.weapon_damage as f32);
        assert_eq!(modifiers.range, effects.weapon_range as f32);
        assert_eq!(modifiers.shields, effects.shields as f32);
        assert_eq!(modifiers.armor, effects.armor as f32);
        assert_eq!(modifiers.hull, effects.hull as f32);
        assert_eq!(modifiers.speed, effects.ship_speed as f32);
        assert!(
            modifiers.damage > 1.0
                && modifiers.range > 1.0
                && modifiers.shields > 1.0
                && modifiers.hull > 1.0
                && modifiers.speed > 1.0,
            "{modifiers:?}"
        );
        // Die Fahrt auf der Karte (fleet_speed) ist keine Sache des Gefechts.
        assert!(effects.fleet_speed > 1.0);
        assert_ne!(modifiers.speed, effects.fleet_speed as f32);

        // Im Gefecht tragen ihre Schiffe die Aufschläge, die des Gegners nicht.
        let mut diplomacy = Diplomacy::default();
        diplomacy.wars.insert((PolityId(0), PolityId(1)));
        fleets.list[1].system = fleets.list[0].system;
        let mut battle = Battle::new(BattleId(0), fleets.list[0].system, 1);
        for index in [0, 1] {
            join(
                &data,
                &Finance::default(),
                &research,
                &diplomacy,
                &mut fleets,
                &mut battle,
                index,
            );
        }
        assert_eq!(battle.combatants[0].modifiers, modifiers);
        assert_eq!(battle.combatants[1].modifiers, CombatModifiers::default());
        let setup = combatant_setup(
            &data,
            &Finance::default(),
            &research,
            &fleets,
            &fleets.list[0],
        );
        assert_eq!((setup.fleet, setup.owner), (Some(FleetId(0)), PolityId(0)));
        assert_eq!(setup.modifiers, modifiers);
        for fought in &battle.ships {
            let stats = &data.designs[fought.design as usize].stats;
            let factor = if fought.side == 0 {
                modifiers
            } else {
                CombatModifiers::default()
            };
            assert!((fought.hull_max - stats.hull * factor.hull).abs() < 1e-3);
            assert!((fought.max_speed - stats.speed * factor.speed).abs() < 1e-3);
        }
    }

    #[test]
    fn survivors_keep_what_the_battle_left_of_them_and_the_lost_are_gone() {
        let (data, _, _, mut fleets) = setup();
        let mut battle = duel(&data, &mut fleets, 0, 1);
        let before = fleets.list[0].ships.clone();
        let value = fleets.list[0].build_value(&data);
        // Das Ende von Hand gesetzt: ein Schiff zerstört, eines beschädigt, eines treibt.
        let (wreck, hurt, adrift) = (
            place(&battle, before[0].id),
            place(&battle, before[1].id),
            place(&battle, before[2].id),
        );
        battle.ships[wreck].status = ShipStatus::Destroyed;
        battle.ships[wreck].hull = 0.0;
        battle.ships[hurt].hull *= 0.25;
        battle.ships[hurt].modules[0].hp = 0.0;
        battle.ships[hurt].ammo *= 0.5;
        battle.ships[hurt].fuel *= 0.75;
        battle.ships[adrift].status = ShipStatus::Disabled;
        battle.finished = true;
        let value_of = |fleet: &Fleet| -> f64 {
            fleet
                .ships
                .iter()
                .map(|ship| material_value(&data, ship))
                .sum()
        };

        // Die Partei hält das Feld: Das treibende Schiff wird geborgen.
        let results = battle.results(&data);
        assert!(holds_field(&results, 0) && holds_field(&results, 1));
        let mut holding = fleets.list[0].clone();
        assert_eq!(write_back(&results, 0, &mut holding, None), 1);
        let cruiser = data.designs[before[0].design as usize].stats.value;
        assert!((value - value_of(&holding) - cruiser).abs() < 1e-6);
        let ids: Vec<ShipId> = holding.ships.iter().map(|ship| ship.id).collect();
        let expected: Vec<ShipId> = before[1..].iter().map(|ship| ship.id).collect();
        assert_eq!(ids, expected, "die Überlebenden in ihrer Reihenfolge");
        let damaged = &holding.ships[0];
        assert!((damaged.hull - 0.25).abs() < 1e-6);
        assert!((damaged.ammo - 0.5).abs() < 1e-6 && (damaged.fuel - 0.75).abs() < 1e-6);
        let slots = data.designs[damaged.design as usize].modules.len();
        assert_eq!(damaged.modules.len(), slots);
        assert_eq!(damaged.modules[0], 0.0);
        assert!(damaged.modules[1..].iter().all(|module| *module == 1.0));
        // Wer nichts abbekam, ist wie zuvor; die Modulliste bleibt leer.
        assert_eq!(holding.ships[2], before[3]);
        // Die Rückgabe an die andere Partei rührt diese Flotte nicht an.
        let mut other = fleets.list[0].clone();
        assert_eq!(write_back(&results, 1, &mut other, None), 0);
        assert_eq!(other.ships, before);

        // Kämpft die Partei nicht mehr, der Gegner aber schon, ist das treibende Schiff
        // dem Gegner überlassen und verloren.
        set_active(&mut battle, 0, ShipStatus::Escaped);
        let results = battle.results(&data);
        assert!(!holds_field(&results, 0) && holds_field(&results, 1));
        let mut routed = fleets.list[0].clone();
        assert_eq!(write_back(&results, 0, &mut routed, None), 2);
        assert!(routed.ships.iter().all(|ship| ship.id != before[2].id));
        assert_eq!(routed.ships.len(), before.len() - 2);

        // Hat auch der Gegner das Feld geräumt, überlässt es niemand dem anderen: Das
        // treibende Schiff bleibt der Flotte, und mit ihm steht sie noch auf dem Feld.
        set_active(&mut battle, 1, ShipStatus::Escaped);
        let results = battle.results(&data);
        assert!(holds_field(&results, 0) && !holds_field(&results, 1));
        let mut last = fleets.list[0].clone();
        assert_eq!(write_back(&results, 0, &mut last, None), 1);
        assert!(last.ships.iter().any(|ship| ship.id == before[2].id));
    }

    #[test]
    fn a_friendly_fleet_on_the_field_saves_ships_adrift() {
        let (data, _, _, mut fleets) = setup();
        // Gemeinschaft 0 und 2 liegen beide mit 1 im Krieg, aber nicht miteinander.
        let mut diplomacy = Diplomacy::default();
        diplomacy.wars.insert((PolityId(0), PolityId(1)));
        diplomacy.wars.insert((PolityId(1), PolityId(2)));
        for index in [1, 2] {
            fleets.list[index].system = fleets.list[0].system;
        }
        let mut battle = Battle::new(BattleId(0), fleets.list[0].system, 1);
        for index in [0, 1, 2] {
            join(
                &data,
                &Finance::default(),
                &no_research(),
                &diplomacy,
                &mut fleets,
                &mut battle,
                index,
            );
        }
        assert!(battle.is_hostile(0, 1) && battle.is_hostile(1, 2) && !battle.is_hostile(0, 2));
        let adrift = fleets.list[0].ships[0].id;
        let spot = place(&battle, adrift);
        battle.ships[spot].status = ShipStatus::Disabled;
        set_active(&mut battle, 0, ShipStatus::Escaped);
        // Der Gegner ist geschlagen, die befreundete Flotte steht noch.
        set_active(&mut battle, 1, ShipStatus::Escaped);
        battle.finished = true;
        let results = battle.results(&data);
        assert!(holds_field(&results, 0) && holds_field(&results, 2));
        assert!(!holds_field(&results, 1));
        let mut rescued = fleets.list[0].clone();
        assert_eq!(write_back(&results, 0, &mut rescued, None), 0);
        assert!(rescued.ship(adrift).is_some());

        // Steht dagegen der Gegner noch, während die eigene Seite nicht mehr kämpft, ist
        // das treibende Schiff verloren, auch wenn die befreundete Flotte weiterkämpft:
        // Sie ist eine andere Partei.
        for ship in &mut battle.ships {
            if ship.side == 1 && ship.status == ShipStatus::Escaped {
                ship.status = ShipStatus::Active;
            }
        }
        let results = battle.results(&data);
        assert!(!holds_field(&results, 0));
        let mut abandoned = fleets.list[0].clone();
        assert_eq!(write_back(&results, 0, &mut abandoned, None), 1);
        assert!(abandoned.ship(adrift).is_none());
    }

    #[test]
    fn craft_in_the_air_do_not_hold_the_field_for_a_fleet_without_ships() {
        let (data, _, _, mut fleets) = setup();
        let carrier = data.design_index("horst").unwrap();
        let mother = fleets.add_ship(&data, FleetId(1), carrier).unwrap();
        let ships = fleets.list[1].ships.len();
        let mut battle = duel(&data, &mut fleets, 0, 1);
        // Die Staffel startet, lange bevor die Gegner einander erreichen.
        for _ in 0..150 {
            battle.step(&data);
        }
        let spot = place(&battle, mother) as u32;
        let aloft = battle
            .ships
            .iter()
            .filter(|ship| ship.carrier == Some(spot) && ship.status == ShipStatus::Active)
            .count();
        assert!(aloft > 0, "die Staffel ist gestartet");
        // Alle Schiffe der Flotte gehen verloren; ihre Jäger kämpfen weiter.
        for ship in &mut battle.ships {
            if ship.side == 1 && ship.source.is_some() {
                ship.status = ShipStatus::Destroyed;
            }
        }
        assert!(battle.is_fighting(1), "die Jäger kämpfen noch");
        let results = battle.results(&data);
        assert!(!holds_field(&results, 1) && holds_field(&results, 0));
        let mut gone = fleets.list[1].clone();
        assert_eq!(write_back(&results, 1, &mut gone, None) as usize, ships);
        assert!(gone.ships.is_empty());
    }

    #[test]
    fn craft_return_to_the_carriers_of_their_fleet_only_while_there_is_room() {
        let (data, _, _, mut fleets) = setup();
        let carrier = data.design_index("horst").unwrap();
        let stats = &data.designs[carrier as usize].stats;
        let craft_value = data.designs[stats.craft.unwrap() as usize].stats.value;
        let first = fleets.add_ship(&data, FleetId(0), carrier).unwrap();
        let second = fleets.add_ship(&data, FleetId(0), carrier).unwrap();
        // Der zweite Träger geht mit halb leerem Hangar ins Gefecht.
        let missing = 5;
        fleets.list[0]
            .ships
            .iter_mut()
            .find(|ship| ship.id == second)
            .unwrap()
            .fighters = stats.fighters - missing;
        let mut battle = duel(&data, &mut fleets, 0, 1);
        let (first_place, second_place) = (place(&battle, first), place(&battle, second));
        assert_eq!(battle.ships[second_place].hangar, stats.fighters - missing);
        // Beide Träger starten ihre Staffeln, lange bevor die Gegner einander erreichen.
        for _ in 0..150 {
            battle.step(&data);
        }
        let launched = |battle: &Battle, carrier: usize| {
            battle
                .ships
                .iter()
                .filter(|ship| ship.carrier == Some(carrier as u32))
                .count() as u32
        };
        assert_eq!(battle.ships[first_place].hangar, 0);
        assert_eq!(launched(&battle, first_place), stats.fighters);
        assert_eq!(launched(&battle, second_place), stats.fighters - missing);
        assert!(
            battle
                .ships
                .iter()
                .all(|ship| ship.status == ShipStatus::Active),
            "noch ist nichts verloren"
        );
        let value_of = |fleet: &Fleet| -> f64 {
            fleet
                .ships
                .iter()
                .map(|ship| material_value(&data, ship))
                .sum()
        };
        let before = value_of(&fleets.list[0]);
        let on_board = |fleet: &Fleet, id: ShipId| fleet.ship(id).unwrap().fighters;

        // Geht kein Träger verloren, kehren alle Jäger heim: so viele, wie gestartet sind.
        let whole = battle.clone();
        let mut fleet = fleets.list[0].clone();
        assert_eq!(write_back(&whole.results(&data), 0, &mut fleet, None), 0);
        assert_eq!(
            on_board(&fleet, first) + on_board(&fleet, second),
            2 * stats.fighters - missing
        );
        assert!((value_of(&fleet) - before).abs() < 1e-6);
        // Zerstörte Jäger fehlen danach in den Hangars.
        let mut thinned = battle.clone();
        let downed = thinned
            .ships
            .iter()
            .position(|ship| ship.carrier == Some(second_place as u32))
            .unwrap();
        thinned.ships[downed].status = ShipStatus::Destroyed;
        let mut fleet = fleets.list[0].clone();
        assert_eq!(write_back(&thinned.results(&data), 0, &mut fleet, None), 0);
        assert_eq!(
            on_board(&fleet, first) + on_board(&fleet, second),
            2 * stats.fighters - missing - 1
        );
        assert!((before - value_of(&fleet) - craft_value).abs() < 1e-6);

        // Der erste Träger geht verloren, seine Staffel ist in der Luft und überlebt. Am
        // Ende des Gefechts zählt der Rechner die Jäger ohne Hangar als verloren.
        battle.ships[first_place].status = ShipStatus::Destroyed;
        battle.finished = true;
        let lost_before = battle.combatants[0].lost[ShipClass::Fighter.index()];
        battle.settle_fighters(&data);
        let homeless = battle.combatants[0].lost[ShipClass::Fighter.index()] - lost_before;
        assert_eq!(homeless, stats.fighters - missing);
        let mut fleet = fleets.list[0].clone();
        assert_eq!(write_back(&battle.results(&data), 0, &mut fleet, None), 1);
        assert!(fleet.ship(first).is_none());
        // Der zweite Träger ist voll: die eigenen Jäger und so viele Heimatlose, wie Platz
        // war. Mehr als sein Hangar fasst, kommt nicht an Bord.
        assert_eq!(on_board(&fleet, second), stats.fighters);
        // Verloren ist der Träger samt Staffel, abzüglich der Jäger, die unterkamen.
        let lost = stats.value - missing as f64 * craft_value;
        assert!((before - value_of(&fleet) - lost).abs() < 1e-6);
        assert!(
            fleet
                .ships
                .iter()
                .all(|ship| { ship.fighters <= data.designs[ship.design as usize].stats.fighters })
        );
    }

    #[test]
    fn a_beaten_fleet_falls_back_where_it_came_from_then_home_then_anywhere_quiet() {
        let (_, mut galaxy, polities, mut fleets) = setup();
        let mut diplomacy = Diplomacy::default();
        diplomacy.wars.insert((PolityId(0), PolityId(1)));
        // Ein System mit mindestens drei Nachbarn, die niemandem gehören.
        let free = |galaxy: &Galaxy, system: SystemId| galaxy.system_owner(system).is_none();
        let place = galaxy
            .systems
            .iter()
            .find(|system| {
                let neighbors = galaxy.neighbors(system.id);
                free(&galaxy, system.id)
                    && neighbors.len() >= 3
                    && neighbors.iter().all(|n| free(&galaxy, n.system))
            })
            .expect("ein freies System mit drei freien Nachbarn")
            .id;
        let neighbors: Vec<SystemId> = galaxy
            .neighbors(place)
            .iter()
            .map(|neighbor| neighbor.system)
            .collect();
        fleets.list[0].system = place;
        let target = |galaxy: &Galaxy, fleets: &Fleets| {
            retreat_target(galaxy, fleets, &diplomacy, &fleets.list[0], &[])
        };

        // Ohne Herkunft, ohne eigenes Gebiet, ohne Feinde: das Nachbarsystem mit der
        // kleinsten Nummer.
        assert_eq!(target(&galaxy, &fleets), Some(neighbors[0]));
        // Steht dort der Feind, das nächste ohne ihn; eine neutrale Flotte stört nicht.
        fleets.list[1].system = neighbors[0];
        fleets.list[2].system = neighbors[1];
        assert_eq!(target(&galaxy, &fleets), Some(neighbors[1]));
        // Ein eigenes Nachbarsystem geht vor.
        let colony = galaxy.system(neighbors[2]).planets[0];
        galaxy.planet_mut(colony).owner = Some(PolityId(0));
        galaxy.planet_mut(colony).population = 10.0;
        assert_eq!(target(&galaxy, &fleets), Some(neighbors[2]));
        // Woher die Flotte kam, geht allem vor, selbst wenn dort der Feind steht.
        fleets.list[0].came_from = Some(neighbors[0]);
        assert_eq!(target(&galaxy, &fleets), Some(neighbors[0]));
        // Eine Herkunft, die kein Nachbar ist (die Flotte wurde hier aufgestellt), zählt nicht.
        let far = polities.list[3]
            .capital
            .map(|planet| galaxy.planet(planet).system);
        assert!(far.is_some_and(|far| !neighbors.contains(&far) && far != place));
        fleets.list[0].came_from = far;
        assert_eq!(target(&galaxy, &fleets), Some(neighbors[2]));
        // Steht überall der Feind und gehört nichts der eigenen Gemeinschaft, bleibt die
        // kleinste Nummer.
        galaxy.planet_mut(colony).owner = None;
        fleets.list[0].came_from = None;
        for (index, system) in neighbors.iter().enumerate() {
            let enemy = fleets.new_fleet(&polities, PolityId(1), *system);
            assert_eq!(enemy.index(), polities.list.len() + index);
        }
        assert_eq!(target(&galaxy, &fleets), Some(neighbors[0]));

        // Wohin schon ein Gegner aus demselben Gefecht ausweicht, dorthin geht die Flotte
        // nicht, auch nicht, wenn sie von dort kam; bleibt kein Nachbar, bleibt sie.
        let avoiding = |fleets: &Fleets, avoid: &[SystemId]| {
            retreat_target(&galaxy, fleets, &diplomacy, &fleets.list[0], avoid)
        };
        assert_eq!(avoiding(&fleets, &neighbors[..1]), Some(neighbors[1]));
        fleets.list[0].came_from = Some(neighbors[1]);
        assert_eq!(avoiding(&fleets, &[]), Some(neighbors[1]));
        assert_eq!(avoiding(&fleets, &neighbors[1..2]), Some(neighbors[0]));
        assert_eq!(avoiding(&fleets, &neighbors), None);
    }

    /// Was `conclude` aus einem von Hand beendeten Gefecht macht.
    struct Settled {
        fleets: Fleets,
        battles: Battles,
        research: Research,
        chronicle: Chronicle,
    }

    impl Settled {
        fn report(&self) -> &BattleReport {
            &self.battles.reports[0]
        }

        fn text(&self) -> &str {
            &self.chronicle.entries[0].text
        }

        /// Hat eine schwere Niederlage die Kurse erschüttert?
        fn shaken(&self) -> bool {
            self.chronicle
                .entries
                .iter()
                .any(|entry| entry.text.contains("Niederlage"))
        }
    }

    const END_TICK: u64 = 20_000;

    fn settle(
        data: &GameData,
        galaxy: &Galaxy,
        polities: &Polities,
        fleets: &Fleets,
        diplomacy: &Diplomacy,
        battle: &Battle,
    ) -> Settled {
        settle_after(
            data,
            galaxy,
            polities,
            fleets,
            diplomacy,
            battle,
            Vec::new(),
        )
    }

    /// Wie [`settle`], für ein Gefecht, aus dem schon Parteien ausgeschieden sind.
    fn settle_after(
        data: &GameData,
        galaxy: &Galaxy,
        polities: &Polities,
        fleets: &Fleets,
        diplomacy: &Diplomacy,
        battle: &Battle,
        departed: Vec<Departed>,
    ) -> Settled {
        let mut settled = Settled {
            fleets: fleets.clone(),
            battles: Battles {
                next_id: 1,
                departed,
                ..Default::default()
            },
            research: Research::default(),
            chronicle: Chronicle::default(),
        };
        conclude(
            battle,
            galaxy,
            &mut settled.fleets,
            &mut settled.battles,
            diplomacy,
            &mut Finance::default(),
            &mut settled.research,
            &Logistics::default(),
            polities,
            &mut settled.chronicle,
            data,
            END_TICK,
        );
        assert_eq!(settled.battles.find_anomaly(&settled.fleets), None);
        settled
    }

    #[test]
    fn whoever_leaves_the_field_falls_back_and_whoever_holds_it_stays() {
        let (data, galaxy, polities, mut fleets) = setup();
        let balance = &data.balance.military;
        let mut diplomacy = Diplomacy::default();
        diplomacy.wars.insert((PolityId(0), PolityId(1)));
        // Gekämpft wird im Heimatsystem der ersten Flotte; die zweite ist eingedrungen.
        let mut battle = duel(&data, &mut fleets, 0, 1);
        let system = battle.system;
        assert_eq!(galaxy.system_owner(system), Some(PolityId(0)));
        let day = END_TICK / TICKS_PER_DAY;
        let fallback = galaxy.neighbors(system)[0].system;
        battle.finished = true;
        battle.tick = 9_000;

        // Die Eingedrungene flieht: Sie räumt das System und sammelt sich.
        let mut won = battle.clone();
        set_active(&mut won, 1, ShipStatus::Escaped);
        let settled = settle(&data, &galaxy, &polities, &fleets, &diplomacy, &won);
        let report = settled.report();
        assert_eq!(report.outcome, BattleOutcome::Decided);
        assert!(!report.outcome.undecided());
        assert!(report.sides[0].held_field && !report.sides[1].held_field);
        assert_eq!(
            (report.started, report.ended),
            (END_TICK + 1 - 9_000, END_TICK)
        );
        assert_eq!(report.days(), 450);
        let (held, left) = (&settled.fleets.list[0], &settled.fleets.list[1]);
        assert!(held.is_ready() && held.route.is_empty() && !held.in_battle);
        assert_eq!(left.route, vec![fallback]);
        assert_eq!(left.regroup_until, Some(day + balance.regroup_days));
        assert!(!left.in_battle && left.battle.is_none() && !left.is_ready());
        assert!(
            settled.text().contains("hält das Feld") && settled.text().contains("weicht nach"),
            "{}",
            settled.text()
        );
        assert_eq!(settled.chronicle.entries[0].polity, Some(PolityId(0)));

        // Flieht die Heimatflotte, räumt auch sie ihr eigenes System: Sie wurde geschlagen.
        let mut lost = battle.clone();
        set_active(&mut lost, 0, ShipStatus::Escaped);
        let settled = settle(&data, &galaxy, &polities, &fleets, &diplomacy, &lost);
        assert_eq!(settled.report().outcome, BattleOutcome::Decided);
        assert!(settled.fleets.list[0].is_regrouping() && settled.fleets.list[1].is_ready());
        assert_eq!(settled.fleets.list[0].route.len(), 1);
        assert_eq!(settled.chronicle.entries[0].polity, Some(PolityId(1)));

        // Fliehen beide, hält niemand das Feld, und beide räumen das System.
        let mut empty = battle.clone();
        set_active(&mut empty, 0, ShipStatus::Escaped);
        set_active(&mut empty, 1, ShipStatus::Escaped);
        let settled = settle(&data, &galaxy, &polities, &fleets, &diplomacy, &empty);
        assert_eq!(settled.report().outcome, BattleOutcome::Abandoned);
        assert!(
            settled.text().contains("Niemand hält das Feld"),
            "{}",
            settled.text()
        );
        assert!(settled.fleets.list[..2].iter().all(Fleet::is_regrouping));
        assert_eq!(settled.chronicle.entries[0].polity, None);

        // Friede: Niemand ist mehr feind, alle stehen noch, alle bleiben.
        let mut peace = battle.clone();
        peace.set_hostile(0, 1, false);
        let settled = settle(
            &data,
            &galaxy,
            &polities,
            &fleets,
            &Diplomacy::default(),
            &peace,
        );
        assert_eq!(settled.report().outcome, BattleOutcome::CalledOff);
        assert!(
            settled.text().contains("kein Krieg mehr"),
            "{}",
            settled.text()
        );
        for fleet in &settled.fleets.list[..2] {
            assert!(fleet.is_ready() && fleet.route.is_empty() && !fleet.in_battle);
        }
        assert!(settled.battles.debris.is_empty() && settled.battles.total_ships_lost() == 0);
    }

    #[test]
    fn without_a_decision_only_the_fleet_at_home_stays() {
        let (data, galaxy, polities, mut fleets) = setup();
        let balance = &data.balance.military;
        let mut diplomacy = Diplomacy::default();
        diplomacy.wars.insert((PolityId(0), PolityId(1)));
        let mut battle = duel(&data, &mut fleets, 0, 1);
        let system = battle.system;
        let day = END_TICK / TICKS_PER_DAY;
        battle.finished = true;
        battle.tick = 9_000;
        // Beide Seiten haben die halbe Flotte verloren: als Niederlage wöge das schwer.
        for combatant in &mut battle.combatants {
            combatant.lost_value = combatant.initial_value * 0.5;
        }
        assert!(balance.heavy_defeat_share <= 0.5);

        // Patt: Der Rechner hat beide Seiten gemeinsam abziehen lassen.
        let mut stalemate = battle.clone();
        for side in 0..2 {
            stalemate.order_retreat(side);
            stalemate.combatants[side].stalemate = true;
            set_active(&mut stalemate, side as u16, ShipStatus::Escaped);
        }
        for ship in &mut stalemate.ships {
            if ship.status == ShipStatus::Routing {
                ship.status = ShipStatus::Escaped;
            }
        }
        let settled = settle(&data, &galaxy, &polities, &fleets, &diplomacy, &stalemate);
        let report = settled.report();
        assert_eq!(report.outcome, BattleOutcome::Stalemate);
        assert!(report.outcome.undecided());
        for side in &report.sides {
            assert!(side.stalemate && side.retreated && !side.held_field);
        }
        // Wer zu Hause ist, bleibt und ist sofort wieder einsatzbereit; wer eingedrungen
        // ist, weicht aus und sammelt sich.
        let (home, intruder) = (&settled.fleets.list[0], &settled.fleets.list[1]);
        assert!(home.system == system && home.route.is_empty() && home.is_ready());
        assert_eq!(intruder.route.len(), 1);
        assert_eq!(intruder.regroup_until, Some(day + balance.regroup_days));
        assert!(!intruder.in_battle && !intruder.is_ready());
        assert!(
            settled.text().contains("im Patt")
                && settled.text().contains("bleibt im eigenen System")
                && settled.text().contains("weicht nach"),
            "{}",
            settled.text()
        );
        // Niemand hat gewonnen, niemand eine Niederlage erlitten.
        assert_eq!(settled.chronicle.entries[0].polity, None);
        assert!(!settled.shaken());

        // Dieselben Verluste in einem verlorenen Gefecht sind eine schwere Niederlage.
        let mut beaten = battle.clone();
        set_active(&mut beaten, 1, ShipStatus::Escaped);
        let settled = settle(&data, &galaxy, &polities, &fleets, &diplomacy, &beaten);
        assert_eq!(settled.report().outcome, BattleOutcome::Decided);
        assert!(settled.shaken());

        // Zeitgrenze: Beide stehen noch auf dem Feld. Auch dann bleibt nur, wer zu Hause ist.
        let mut limit = battle.clone();
        limit.timed_out = true;
        let settled = settle(&data, &galaxy, &polities, &fleets, &diplomacy, &limit);
        let report = settled.report();
        assert_eq!(report.outcome, BattleOutcome::TimedOut);
        assert!(
            report
                .sides
                .iter()
                .all(|side| side.held_field && !side.stalemate)
        );
        let (home, intruder) = (&settled.fleets.list[0], &settled.fleets.list[1]);
        assert!(home.system == system && home.route.is_empty() && home.is_ready());
        assert!(intruder.is_regrouping() && intruder.route.len() == 1);
        assert!(
            settled.text().contains("ohne Entscheidung"),
            "{}",
            settled.text()
        );
        assert!(!settled.shaken());
        // Wer an der Zeitgrenze schon geflohen war, wurde geschlagen, auch zu Hause.
        set_active(&mut limit, 0, ShipStatus::Escaped);
        let settled = settle(&data, &galaxy, &polities, &fleets, &diplomacy, &limit);
        assert!(settled.fleets.list[0].is_regrouping());
        assert!(
            settled.fleets.list[1].is_regrouping(),
            "die andere ist nicht zu Hause"
        );

        // In einem System, das keinem der Beteiligten gehört, weichen im Patt alle aus.
        let nowhere = galaxy
            .systems
            .iter()
            .find(|candidate| {
                galaxy.system_owner(candidate.id).is_none()
                    && galaxy.neighbors(candidate.id).len() >= 2
            })
            .expect("ein freies System mit zwei Nachbarn")
            .id;
        let mut abroad = fleets.clone();
        abroad.list[0].system = nowhere;
        abroad.list[1].system = nowhere;
        let mut neutral = stalemate.clone();
        neutral.system = nowhere;
        let settled = settle(&data, &galaxy, &polities, &abroad, &diplomacy, &neutral);
        assert_eq!(settled.report().outcome, BattleOutcome::Stalemate);
        assert!(settled.fleets.list[..2].iter().all(Fleet::is_regrouping));
    }

    #[test]
    fn a_stalemate_is_no_draw_once_another_enemy_holds_the_field() {
        let (data, galaxy, polities, mut fleets) = setup();
        // Gemeinschaft 0 ist zu Hause und liegt mit 1 und 2 im Krieg; die beiden anderen
        // sind einander nicht feind.
        let mut diplomacy = Diplomacy::default();
        diplomacy.wars.insert((PolityId(0), PolityId(1)));
        diplomacy.wars.insert((PolityId(0), PolityId(2)));
        for index in [1, 2] {
            fleets.list[index].system = fleets.list[0].system;
        }
        let mut battle = Battle::new(BattleId(0), fleets.list[0].system, 1);
        for index in [0, 1, 2] {
            join(
                &data,
                &Finance::default(),
                &no_research(),
                &diplomacy,
                &mut fleets,
                &mut battle,
                index,
            );
        }
        // Heimatflotte und erster Gegner haben sich im Patt gelöst; danach traf der zweite
        // Gegner ein und steht am Ende allein auf dem Feld.
        for side in 0..2 {
            battle.order_retreat(side);
            battle.combatants[side].stalemate = true;
        }
        for ship in &mut battle.ships {
            if ship.side < 2 {
                ship.status = ShipStatus::Escaped;
            }
        }
        battle.combatants[0].lost_value = battle.combatants[0].initial_value * 0.5;
        battle.finished = true;
        let settled = settle(&data, &galaxy, &polities, &fleets, &diplomacy, &battle);
        let report = settled.report();
        assert_eq!(report.outcome, BattleOutcome::Decided);
        assert!(report.sides[2].held_field && !report.sides[0].held_field);
        // Die Heimatflotte hat das Feld einem Gegner überlassen: Sie räumt auch ihr eigenes
        // System, und ihre Verluste zählen als Niederlage.
        assert!(settled.fleets.list[0].is_regrouping());
        assert_eq!(settled.fleets.list[0].route.len(), 1);
        assert!(settled.shaken());
        // Für den ersten Gegner bleibt es beim Patt: Wer das Feld hält, ist ihm nicht
        // feind. Er ist hier nicht zu Hause und weicht aus.
        assert!(report.sides[1].stalemate);
        assert!(settled.fleets.list[1].is_regrouping());
        // Wer das Feld hält, bleibt.
        assert!(settled.fleets.list[2].is_ready() && settled.fleets.list[2].route.is_empty());
        assert_eq!(settled.chronicle.entries[0].polity, Some(PolityId(2)));
    }

    #[test]
    fn opponents_never_fall_back_into_the_same_system() {
        let (data, galaxy, polities, mut fleets) = setup();
        // Ein freies System mit möglichst wenigen Nachbarn und eine Flotte mehr, als es
        // Auswege gibt; alle liegen miteinander im Krieg.
        let field = galaxy
            .systems
            .iter()
            .filter(|system| {
                galaxy.system_owner(system.id).is_none() && !galaxy.neighbors(system.id).is_empty()
            })
            .min_by_key(|system| (galaxy.neighbors(system.id).len(), system.id))
            .expect("ein freies System mit Nachbarn")
            .id;
        let exits: Vec<SystemId> = galaxy
            .neighbors(field)
            .iter()
            .map(|neighbor| neighbor.system)
            .collect();
        let count = exits.len() + 1;
        assert!(
            count <= polities.list.len(),
            "zu viele Auswege für diese Probe"
        );
        let mut diplomacy = Diplomacy::default();
        for a in 0..count as u32 {
            for b in (a + 1)..count as u32 {
                diplomacy.wars.insert((PolityId(a), PolityId(b)));
            }
        }
        let mut battle = Battle::new(BattleId(0), field, 1);
        for index in 0..count {
            fleets.list[index].system = field;
            // Alle kamen aus demselben Nachbarsystem: Dorthin wollten sie alle zurück.
            fleets.list[index].came_from = Some(exits[0]);
            join(
                &data,
                &Finance::default(),
                &no_research(),
                &diplomacy,
                &mut fleets,
                &mut battle,
                index,
            );
        }
        // Patt: Alle lösen sich, niemand ist hier zu Hause.
        for side in 0..count {
            battle.order_retreat(side);
            battle.combatants[side].stalemate = true;
        }
        for ship in &mut battle.ships {
            ship.status = ShipStatus::Escaped;
        }
        battle.finished = true;
        let settled = settle(&data, &galaxy, &polities, &fleets, &diplomacy, &battle);
        assert_eq!(settled.report().outcome, BattleOutcome::Stalemate);
        let after = &settled.fleets.list[..count];
        assert!(after.iter().all(Fleet::is_regrouping));
        // Die erste Partei weicht aus, woher sie kam; jede weitere in ein System, das noch
        // kein Gegner gewählt hat. Alle räumen das Feld, und keine zwei haben dasselbe Ziel.
        assert_eq!(after[0].route, vec![exits[0]]);
        let mut chosen: Vec<SystemId> = after
            .iter()
            .map(|fleet| *fleet.route.last().expect("jede Flotte räumt das System"))
            .collect();
        assert!(!chosen.contains(&field));
        chosen.sort();
        chosen.dedup();
        assert_eq!(chosen.len(), count, "kein Ziel doppelt");
        // Jeder Nachbar ist vergeben, bevor die letzte wählt: Ihr Weg führt durch eines der
        // Nachbarsysteme hindurch in das nächste freie dahinter.
        for fleet in &after[..exits.len()] {
            assert_eq!(fleet.route.len(), 1);
            assert!(exits.contains(&fleet.route[0]));
        }
        let last = &after[count - 1];
        assert_eq!(last.route.len(), 2, "{:?}", last.route);
        assert!(exits.contains(&last.route[0]) && !exits.contains(&last.route[1]));
        assert!(
            galaxy.lane_between(last.route[0], last.route[1]).is_some(),
            "der Weg folgt den Routen"
        );
        assert_eq!(
            withdrawal_route(&galaxy, &fleets, &diplomacy, &fleets.list[0], &exits),
            last.route,
            "derselbe Weg, wenn alle Nachbarn vergeben sind"
        );
        // Nur wenn es in der ganzen Galaxie kein freies System gäbe, bliebe eine Flotte.
        let everywhere: Vec<SystemId> = galaxy.systems.iter().map(|system| system.id).collect();
        assert!(
            withdrawal_route(&galaxy, &fleets, &diplomacy, &fleets.list[0], &everywhere).is_empty()
        );
        assert!(
            !settled.text().contains("sammelt sich"),
            "{}",
            settled.text()
        );

        // Wer nicht miteinander im Krieg liegt, darf denselben Weg nehmen.
        let mut friends = battle.clone();
        friends.set_hostile(0, 1, false);
        let settled = settle(&data, &galaxy, &polities, &fleets, &diplomacy, &friends);
        assert_eq!(settled.fleets.list[0].route, vec![exits[0]]);
        assert_eq!(settled.fleets.list[1].route, vec![exits[0]]);
    }

    /// Drei Startflotten im Heimatsystem der ersten, als Parteien eines Gefechts, das noch
    /// keinen Schritt gerechnet hat. Wer wem feind ist, sagt die Kriegslage.
    fn melee(
        data: &GameData,
        fleets: &mut Fleets,
        diplomacy: &Diplomacy,
        members: &[usize],
    ) -> Battles {
        let system = fleets.list[members[0]].system;
        let mut battle = Battle::new(BattleId(0), system, 1);
        for index in members {
            fleets.list[*index].system = system;
            join(
                data,
                &Finance::default(),
                &no_research(),
                diplomacy,
                fleets,
                &mut battle,
                *index,
            );
        }
        Battles {
            active: vec![battle],
            next_id: 1,
            ..Default::default()
        }
    }

    fn muster_at(
        data: &GameData,
        galaxy: &Galaxy,
        fleets: &mut Fleets,
        battles: &mut Battles,
        diplomacy: &Diplomacy,
        tick: u64,
    ) -> Chronicle {
        let mut chronicle = Chronicle::default();
        muster_battles(
            galaxy,
            fleets,
            battles,
            diplomacy,
            &mut chronicle,
            data,
            tick,
        );
        assert_eq!(battles.find_anomaly(fleets), None);
        assert_eq!(fleets.find_anomaly(data), None);
        chronicle
    }

    #[test]
    fn what_a_running_battle_has_cost_shows_in_the_fleet_each_morning() {
        let (data, galaxy, _, mut fleets) = setup();
        let mut diplomacy = Diplomacy::default();
        diplomacy.wars.insert((PolityId(0), PolityId(1)));
        let mut battles = melee(&data, &mut fleets, &diplomacy, &[0, 1]);
        let before = fleets.list[0].clone();
        let pay = crate::fleet::monthly_upkeep(&data, &fleets, PolityId(0));
        let strength = before.strength;
        let battle = &mut battles.active[0];
        // Ein Schiff ist zerstört, eines treibt, eines ist angeschlagen.
        let (wreck, adrift, hurt) = (
            place(battle, before.ships[0].id),
            place(battle, before.ships[1].id),
            place(battle, before.ships[2].id),
        );
        battle.ships[wreck].status = ShipStatus::Destroyed;
        battle.ships[adrift].status = ShipStatus::Disabled;
        battle.ships[adrift].hull *= 0.5;
        battle.ships[hurt].hull *= 0.25;
        battle.ships[hurt].ammo *= 0.5;

        muster_at(&data, &galaxy, &mut fleets, &mut battles, &diplomacy, 400);
        let now = &fleets.list[0];
        // Das Wrack ist fort: aus der Flotte, aus dem Sold, aus der Stärke.
        assert!(now.ship(before.ships[0].id).is_none());
        assert_eq!(now.ships.len(), before.ships.len() - 1);
        assert_eq!(battles.ships_lost_by(PolityId(0)), 1);
        let cruiser = data.designs[before.ships[0].design as usize].stats.value;
        let paid = crate::fleet::monthly_upkeep(&data, &fleets, PolityId(0));
        assert_eq!(
            pay - paid,
            crate::economy::to_money(cruiser * data.balance.military.upkeep_per_month)
        );
        assert!(now.strength < strength);
        // Die Überlebenden tragen ihren Zustand; das treibende Schiff ist noch dabei.
        let damaged = now.ship(before.ships[2].id).unwrap();
        assert!((damaged.hull - 0.25).abs() < 1e-6 && (damaged.ammo - 0.5).abs() < 1e-6);
        assert!((now.ship(before.ships[1].id).unwrap().hull - 0.5).abs() < 1e-6);
        // Die Flotte steht weiter im Gefecht, und die andere ist unberührt.
        assert!(now.in_battle && battles.of_fleet(now.id).is_some());
        assert!(battles.departed.is_empty());
        assert_eq!(fleets.list[1].ships.len(), before.ships.len());

        // Ein zweiter Morgen zählt nichts doppelt, und am Ende bleibt es bei dem einen
        // Verlust, solange das treibende Schiff geborgen wird.
        muster_at(&data, &galaxy, &mut fleets, &mut battles, &diplomacy, 420);
        assert_eq!(battles.ships_lost_by(PolityId(0)), 1);
        assert_eq!(fleets.list[0].ships.len(), before.ships.len() - 1);

        // Kämpft die eigene Seite nicht mehr, der Gegner aber schon, steht das Los des
        // treibenden Schiffs erst am Ende fest: Bis dahin bleibt es in der Flotte.
        set_active(&mut battles.active[0], 0, ShipStatus::Routing);
        muster_at(&data, &galaxy, &mut fleets, &mut battles, &diplomacy, 440);
        assert!(fleets.list[0].ship(before.ships[1].id).is_some());
        assert_eq!(battles.ships_lost_by(PolityId(0)), 1);
    }

    #[test]
    fn a_party_without_an_opponent_leaves_while_the_others_fight_on() {
        let (data, galaxy, polities, mut fleets) = setup();
        // 0 und 2 führen Krieg gegen 1, aber nicht gegeneinander.
        let mut diplomacy = Diplomacy::default();
        diplomacy.wars.insert((PolityId(0), PolityId(1)));
        diplomacy.wars.insert((PolityId(1), PolityId(2)));
        let mut battles = melee(&data, &mut fleets, &diplomacy, &[0, 1, 2]);
        let chronicle = muster_at(&data, &galaxy, &mut fleets, &mut battles, &diplomacy, 400);
        assert!(battles.departed.is_empty() && chronicle.entries.is_empty());

        // 0 schließt Frieden mit 1. Am nächsten Morgen hat die Flotte keinen Gegner mehr
        // und scheidet aus; 1 und 2 kämpfen weiter.
        diplomacy.wars.remove(&(PolityId(0), PolityId(1)));
        let chronicle = muster_at(&data, &galaxy, &mut fleets, &mut battles, &diplomacy, 420);
        assert_eq!(battles.departed.len(), 1);
        let entry = battles.departed[0].clone();
        assert_eq!(
            (entry.side, entry.fleet, entry.cause, entry.destination),
            (0, Some(FleetId(0)), Departure::NoOpponent, None)
        );
        assert!(battles.has_departed(BattleId(0), 0) && !battles.has_departed(BattleId(0), 1));
        assert!(
            chronicle.entries[0].text.contains("keinen Gegner mehr"),
            "{}",
            chronicle.entries[0].text
        );
        let free = &fleets.list[0];
        assert!(free.is_ready() && !free.in_battle && free.battle.is_none());
        assert!(free.route.is_empty() && free.ships.len() == fleets.list[1].ships.len());
        assert!(battles.of_fleet(FleetId(0)).is_none());
        assert!(battles.of_fleet(FleetId(1)).is_some() && battles.of_fleet(FleetId(2)).is_some());
        let battle = &battles.active[0];
        assert_eq!(
            battle.combatants.len(),
            3,
            "die Partei bleibt für die Bilanz"
        );
        assert!(
            battle
                .ships
                .iter()
                .filter(|ship| ship.side == 0)
                .all(|ship| ship.status == ShipStatus::Escaped)
        );
        assert!(battle.is_fighting(1) && battle.is_fighting(2));
        // Die Flotte marschiert wieder, wohin sie will, und wird nicht erneut aufgenommen.
        let exit = galaxy.neighbors(free.system)[0].system;
        let mut gone = fleets.clone();
        crate::fleet::order_move(&galaxy, &mut gone, PolityId(0), FleetId(0), exit).unwrap();
        let chronicle = muster_at(&data, &galaxy, &mut fleets, &mut battles, &diplomacy, 440);
        assert!(battles.departed.len() == 1 && chronicle.entries.is_empty());

        // Am Ende steht sie in der Bilanz als früh ausgeschieden; mit ihr geschieht nichts.
        let mut battle = battles.active[0].clone();
        set_active(&mut battle, 1, ShipStatus::Escaped);
        battle.finished = true;
        let settled = settle_after(
            &data,
            &galaxy,
            &polities,
            &fleets,
            &diplomacy,
            &battle,
            battles.departed.clone(),
        );
        let report = settled.report();
        assert_eq!(report.outcome, BattleOutcome::Decided);
        assert_eq!(report.sides[0].left_early, Some(420));
        assert!(
            report.sides[0].held_field,
            "sie stand bis zuletzt auf dem Feld"
        );
        assert_eq!(report.sides[1].left_early, None);
        assert!(report.sides[2].held_field && !report.sides[1].held_field);
        assert!(settled.fleets.list[0].is_ready() && settled.fleets.list[0].route.is_empty());
        assert!(settled.battles.departed.is_empty());
        assert!(
            settled.text().contains("hält das Feld") && !settled.text().contains("halten"),
            "{}",
            settled.text()
        );

        // Schließen dagegen in einem Gefecht zu zweit beide Frieden, scheidet niemand
        // einzeln aus: Das Gefecht endet als Ganzes.
        let (_, _, _, mut pair) = setup();
        let mut war = Diplomacy::default();
        war.wars.insert((PolityId(3), PolityId(4)));
        let mut duel = melee(&data, &mut pair, &war, &[3, 4]);
        let chronicle = muster_at(
            &data,
            &galaxy,
            &mut pair,
            &mut duel,
            &Diplomacy::default(),
            400,
        );
        assert!(duel.departed.is_empty() && chronicle.entries.is_empty());
        assert!(!duel.active[0].is_hostile(0, 1));
        assert!(pair.list[3].in_battle && pair.list[4].in_battle);
    }

    #[test]
    fn a_fleet_with_no_ship_left_on_the_field_leaves_at_once() {
        let (data, galaxy, polities, mut fleets) = setup();
        let balance = &data.balance.military;
        // 0 und 2 führen Krieg gegen 1, aber nicht gegeneinander. Gekämpft wird im
        // Heimatsystem von 1.
        let mut diplomacy = Diplomacy::default();
        diplomacy.wars.insert((PolityId(0), PolityId(1)));
        diplomacy.wars.insert((PolityId(1), PolityId(2)));
        let mut battles = melee(&data, &mut fleets, &diplomacy, &[1, 0, 2]);
        let system = fleets.list[1].system;
        let ships = fleets.list[0].ships.len();
        // Die Flotte von 0 verliert ein Schiff, die übrigen entkommen.
        {
            let battle = &mut battles.active[0];
            let wreck = place(battle, fleets.list[0].ships[0].id);
            battle.ships[wreck].status = ShipStatus::Destroyed;
            set_active(battle, 1, ShipStatus::Escaped);
            battle.combatants[1].lost_value = battle.combatants[1].initial_value * 0.5;
        }
        let tick = 400;
        let chronicle = muster_at(&data, &galaxy, &mut fleets, &mut battles, &diplomacy, tick);
        // Sie ist frei, sobald der Tag beginnt. Weil 2 gegen denselben Gegner weiterkämpft,
        // räumt sie das System nicht, sondern sammelt sich hinter dessen Linien.
        let entry = battles.departed[0].clone();
        assert_eq!(
            (entry.side, entry.cause, entry.destination),
            (1, Departure::LeftField, None)
        );
        let rallying = fleets.list[0].clone();
        assert!(!rallying.in_battle && rallying.battle.is_none());
        assert_eq!(rallying.ships.len(), ships - 1);
        assert_eq!(
            rallying.regroup_until,
            Some(tick / TICKS_PER_DAY + balance.regroup_days)
        );
        assert!(rallying.route.is_empty() && rallying.system == system);
        assert!(
            chronicle.entries[0]
                .text
                .contains("hinter den eigenen Linien"),
            "{}",
            chronicle.entries[0].text
        );
        assert!(fleets.list[1].in_battle && fleets.list[2].in_battle);

        // Hält am Ende die eigene Seite das Feld, bleibt es dabei, und es war keine
        // Niederlage, wie schwer die Verluste auch wiegen.
        let mut won = battles.active[0].clone();
        set_active(&mut won, 0, ShipStatus::Escaped);
        won.finished = true;
        let settled = settle_after(
            &data,
            &galaxy,
            &polities,
            &fleets,
            &diplomacy,
            &won,
            battles.departed.clone(),
        );
        assert_eq!(settled.report().outcome, BattleOutcome::Decided);
        let side = &settled.report().sides[1];
        assert_eq!(side.left_early, Some(tick));
        assert!(!side.held_field);
        assert_eq!(settled.fleets.list[0].route, rallying.route);
        assert_eq!(settled.fleets.list[0].regroup_until, rallying.regroup_until);
        assert!(!settled.shaken());
        assert!(settled.fleets.list[1].is_regrouping(), "der Gegner räumt");

        // Hält am Ende doch der Gegner das Feld, räumt auch sie das System: in ein anderes
        // als die Mitstreiterin nur dann nicht, wenn sie einander nicht feind sind.
        let mut lost = battles.active[0].clone();
        set_active(&mut lost, 2, ShipStatus::Escaped);
        lost.finished = true;
        let settled = settle_after(
            &data,
            &galaxy,
            &polities,
            &fleets,
            &diplomacy,
            &lost,
            battles.departed.clone(),
        );
        assert!(settled.report().sides[0].held_field);
        let driven = &settled.fleets.list[0];
        assert_eq!(driven.route.len(), 1, "{}", settled.text());
        assert_eq!(
            driven.regroup_until,
            Some(END_TICK / TICKS_PER_DAY + balance.regroup_days)
        );
        assert!(
            settled.text().contains("weicht nun nach"),
            "{}",
            settled.text()
        );
        assert!(
            settled.shaken(),
            "das Feld einem Gegner überlassen: geschlagen"
        );
        assert!(settled.fleets.list[2].is_regrouping());

        // Kämpft niemand mehr gegen ihre Gegner, weicht sie sofort aus: Dass anderswo auf
        // dem Feld noch zwei andere kämpfen, hilft ihr nicht.
        let (_, _, _, mut fleets) = setup();
        let mut diplomacy = Diplomacy::default();
        diplomacy.wars.insert((PolityId(0), PolityId(1)));
        diplomacy.wars.insert((PolityId(2), PolityId(3)));
        let mut battles = melee(&data, &mut fleets, &diplomacy, &[1, 0, 2, 3]);
        set_active(&mut battles.active[0], 1, ShipStatus::Escaped);
        let chronicle = muster_at(&data, &galaxy, &mut fleets, &mut battles, &diplomacy, tick);
        // Beide Parteien des ersten Paars scheiden aus: die eine ohne Schiff auf dem Feld,
        // die andere ohne Gegner.
        assert_eq!(battles.departed.len(), 2);
        let beaten = &fleets.list[0];
        assert!(beaten.is_regrouping() && beaten.route.len() == 1 && !beaten.in_battle);
        assert_eq!(
            battles
                .departed
                .iter()
                .find(|entry| entry.fleet == Some(FleetId(0)))
                .unwrap()
                .destination,
            beaten.route.last().copied()
        );
        let winner = &fleets.list[1];
        assert!(winner.is_ready() && winner.route.is_empty() && !winner.in_battle);
        assert!(fleets.list[2].in_battle && fleets.list[3].in_battle);
        assert!(
            chronicle
                .entries
                .iter()
                .any(|entry| entry.text.contains("weicht nach")),
        );
    }

    #[test]
    fn whose_side_holds_the_field_need_not_leave_the_system() {
        let (data, galaxy, polities, mut fleets) = setup();
        let balance = &data.balance.military;
        // Gemeinschaft 0 kämpft mit zwei Flotten gegen 1, in dessen Heimatsystem.
        let mut diplomacy = Diplomacy::default();
        diplomacy.wars.insert((PolityId(0), PolityId(1)));
        let part: Vec<ShipId> = fleets.list[0].ships[..3]
            .iter()
            .map(|ship| ship.id)
            .collect();
        let sister = crate::fleet::split_fleet(
            &data,
            &polities,
            &mut fleets,
            PolityId(0),
            FleetId(0),
            &part,
        )
        .unwrap();
        let battles = melee(&data, &mut fleets, &diplomacy, &[1, 0, sister.index()]);
        let mut battle = battles.active[0].clone();
        assert!(
            !battle.is_hostile(1, 2),
            "Schwesterflotten sind einander nie feind"
        );
        // Die erste Flotte hat das Feld geräumt und dabei schwer verloren, die Schwester
        // schlägt den Gegner im selben Augenblick.
        set_active(&mut battle, 1, ShipStatus::Escaped);
        set_active(&mut battle, 0, ShipStatus::Escaped);
        battle.combatants[1].lost_value = battle.combatants[1].initial_value * 0.9;
        battle.finished = true;
        let settled = settle(&data, &galaxy, &polities, &fleets, &diplomacy, &battle);
        let report = settled.report();
        assert_eq!(report.outcome, BattleOutcome::Decided);
        assert!(report.sides[2].held_field && !report.sides[1].held_field);
        // Wer das Feld hält, entscheidet sich je Seite: Die Flotte bleibt im System und
        // sammelt sich hinter den eigenen Linien. Geschlagen wurde die Gemeinschaft nicht.
        let rallying = &settled.fleets.list[0];
        assert!(rallying.route.is_empty() && rallying.system == battle.system);
        assert_eq!(
            rallying.regroup_until,
            Some(END_TICK / TICKS_PER_DAY + balance.regroup_days)
        );
        assert!(
            settled.text().contains("hinter den eigenen Linien"),
            "{}",
            settled.text()
        );
        assert!(!settled.shaken());
        assert!(settled.fleets.list[sister.index()].is_ready());
        // Der Gegner hat das Feld einem Feind überlassen und räumt sein eigenes System.
        assert!(settled.fleets.list[1].is_regrouping());
        assert_eq!(settled.fleets.list[1].route.len(), 1);

        // Hält dagegen der Gegner das Feld, räumen beide Schwestern das System, und beide
        // dürfen denselben Weg nehmen.
        let mut lost = battles.active[0].clone();
        for side in [1, 2] {
            set_active(&mut lost, side, ShipStatus::Escaped);
            lost.combatants[side as usize].lost_value =
                lost.combatants[side as usize].initial_value * 0.9;
        }
        lost.finished = true;
        let settled = settle(&data, &galaxy, &polities, &fleets, &diplomacy, &lost);
        let (first, second) = (
            &settled.fleets.list[0],
            &settled.fleets.list[sister.index()],
        );
        assert!(first.is_regrouping() && second.is_regrouping());
        assert_eq!(first.route.len(), 1);
        assert_eq!(first.route, second.route);
        assert!(settled.shaken());
    }

    #[test]
    fn the_last_ship_of_a_worn_down_navy_weighs_what_it_is_worth() {
        let (data, galaxy, polities, mut fleets) = setup();
        let balance = &data.balance.military;
        let start = balance.starting_fleet_value;
        let full = fleets.list[0].build_value(&data);
        assert!((start - full).abs() < 1e-6, "die Startflotte ist das Maß");
        // Der Maßstab: der Bauwert der Flotte vor dem Gefecht, mindestens eine Startflotte.
        assert_eq!(loss_share(&data, start, start), 1.0);
        assert_eq!(loss_share(&data, start, 10.0 * start), 0.1);
        assert_eq!(loss_share(&data, 0.5 * start, 0.5 * start), 0.5);
        assert_eq!(loss_share(&data, 0.0, 0.0), 0.0);
        assert_eq!(loss_share(&data, 3.0 * start, start), 1.0);

        // Gemeinschaft 1 ist auf eine einzige Korvette zusammengeschmolzen und verliert
        // auch die gegen eine ganze Flotte.
        let mut diplomacy = Diplomacy::default();
        diplomacy.wars.insert((PolityId(0), PolityId(1)));
        let corvette = fleets.list[1].ships.pop().unwrap();
        fleets.list[1].ships = vec![corvette];
        fleets.refresh(&data);
        let value = fleets.list[1].build_value(&data);
        assert!(value < balance.heavy_defeat_share * start);
        let outcome = |fleets: &Fleets, wipe_out: usize| {
            let mut fleets = fleets.clone();
            let mut battle = duel(&data, &mut fleets, 0, 1);
            assert!(battle.is_hostile(0, 1));
            for ship in &mut battle.ships {
                if ship.side as usize == wipe_out {
                    ship.status = ShipStatus::Destroyed;
                }
            }
            battle.combatants[wipe_out].lost_value = battle.combatants[wipe_out].initial_value;
            battle.finished = true;
            let mut finance = Finance {
                polities: vec![Default::default(); polities.list.len()],
                ..Default::default()
            };
            let mut chronicle = Chronicle::default();
            let mut battles = Battles {
                next_id: 1,
                ..Default::default()
            };
            conclude(
                &battle,
                &galaxy,
                &mut fleets,
                &mut battles,
                &diplomacy,
                &mut finance,
                &mut Research::default(),
                &Logistics::default(),
                &polities,
                &mut chronicle,
                &data,
                END_TICK,
            );
            let shaken = chronicle
                .entries
                .iter()
                .any(|entry| entry.text.contains("Niederlage"));
            (
                finance.polities[0].war_weariness,
                finance.polities[1].war_weariness,
                shaken,
            )
        };
        // Das wiegt so viel wie die Korvette an einer Startflotte, nicht so viel wie eine
        // ganze Flotte, und es ist keine schwere Niederlage.
        let (_, weary, shaken) = outcome(&fleets, 1);
        let expected = balance.weariness_per_navy_lost * value / start;
        assert!((weary - expected).abs() < 1e-6, "{weary} statt {expected}");
        assert!(weary < 0.2 * balance.weariness_per_navy_lost);
        assert!(!shaken);
        // Verliert dagegen die andere Seite ihre ganze Startflotte, zählt das voll.
        let (weary, _, shaken) = outcome(&fleets, 0);
        assert!((weary - balance.weariness_per_navy_lost).abs() < 1e-6);
        assert!(shaken);
    }

    #[test]
    fn opponents_who_came_to_blows_learn_about_each_other() {
        let (data, galaxy, polities, mut fleets) = setup();
        let insight = data.balance.research.intel_per_battle;
        assert!(insight > 0.0 && insight <= 1.0);
        // 0 und 2 kämpfen gegen 1, aber nicht gegeneinander.
        let mut diplomacy = Diplomacy::default();
        diplomacy.wars.insert((PolityId(0), PolityId(1)));
        diplomacy.wars.insert((PolityId(1), PolityId(2)));
        for index in [1, 2] {
            fleets.list[index].system = fleets.list[0].system;
        }
        let mut battle = Battle::new(BattleId(0), fleets.list[0].system, 1);
        for index in [0, 1, 2] {
            join(
                &data,
                &Finance::default(),
                &no_research(),
                &diplomacy,
                &mut fleets,
                &mut battle,
                index,
            );
        }
        battle.finished = true;
        let pairs = |research: &Research| {
            let mut found = Vec::new();
            for viewer in 0..3u32 {
                for target in 0..3u32 {
                    if viewer != target {
                        found.push(research.intel(PolityId(viewer), PolityId(target)));
                    }
                }
            }
            found
        };

        // Ohne Berührung hat niemand etwas gesehen.
        let settled = settle(&data, &galaxy, &polities, &fleets, &diplomacy, &battle);
        assert!(pairs(&settled.research).iter().all(|intel| *intel == 0.0));

        // Nach einem Gefecht mit Berührung kennen die Gegner einander besser, beide gleich;
        // wer nicht gegeneinander kämpfte, hat nichts voneinander gelernt.
        battle.combatants[1].contact = true;
        let settled = settle(&data, &galaxy, &polities, &fleets, &diplomacy, &battle);
        let research = &settled.research;
        for (a, b) in [(0, 1), (1, 0), (1, 2), (2, 1)] {
            assert_eq!(
                research.intel(PolityId(a), PolityId(b)),
                insight,
                "{a} über {b}"
            );
        }
        assert_eq!(research.intel(PolityId(0), PolityId(2)), 0.0);
        assert_eq!(research.intel(PolityId(2), PolityId(0)), 0.0);

        // Hat ein Friedensschluss das Gefecht beendet, gilt niemand mehr als Gegner.
        battle.set_hostile(0, 1, false);
        battle.set_hostile(1, 2, false);
        let settled = settle(
            &data,
            &galaxy,
            &polities,
            &fleets,
            &Diplomacy::default(),
            &battle,
        );
        assert!(pairs(&settled.research).iter().all(|intel| *intel == 0.0));
    }

    #[test]
    fn the_report_carries_the_tally_of_the_calculator() {
        let (data, galaxy, polities, mut fleets) = setup();
        let mut diplomacy = Diplomacy::default();
        diplomacy.wars.insert((PolityId(0), PolityId(1)));
        let carrier = data.design_index("horst").unwrap();
        fleets.add_ship(&data, FleetId(1), carrier).unwrap();
        let before = fleets.clone();
        let mut battle = duel(&data, &mut fleets, 0, 1);
        // Der Rechner zählt jeden Jäger einmal: Der Bauwert der Partei ist der der Flotte.
        for (side, fleet) in before.list[..2].iter().enumerate() {
            let value: f64 = fleet
                .ships
                .iter()
                .map(|ship| material_value(&data, ship))
                .sum();
            assert!((battle.combatants[side].initial_value as f64 - value).abs() < 0.5);
        }
        // Gerechnet bis zum Ende: Was die Bilanz als verloren ausweist, fehlt den Flotten.
        let mut steps = 0;
        while !battle.finished {
            battle.step(&data);
            steps += 1;
            assert!(steps < 20_000, "das Gefecht endet nie");
        }
        let settled = settle(&data, &galaxy, &polities, &fleets, &diplomacy, &battle);
        let report = settled.report();
        let mut ships_lost = 0;
        for (side, old) in before.list[..2].iter().enumerate() {
            let entry = &report.sides[side];
            let now = &settled.fleets.list[side];
            assert_eq!(entry.fleet, Some(old.id));
            assert_eq!(entry.initial, battle.combatants[side].initial);
            assert_eq!(entry.lost, battle.combatants[side].lost);
            let count = |fleet: &Fleet| {
                let mut counts = [0u32; 6];
                for ship in &fleet.ships {
                    counts[data.designs[ship.design as usize].stats.class.index()] += 1;
                    counts[ShipClass::Fighter.index()] += ship.fighters;
                }
                counts
            };
            let (initial, left) = (count(old), count(now));
            assert_eq!(entry.initial, initial);
            for class in 0..6 {
                assert_eq!(
                    entry.lost[class],
                    initial[class] - left[class],
                    "Klasse {class}"
                );
            }
            let value = |fleet: &Fleet| -> f64 {
                fleet
                    .ships
                    .iter()
                    .map(|ship| material_value(&data, ship))
                    .sum()
            };
            assert!((entry.lost_value as f64 - (value(old) - value(now))).abs() < 0.5);
            ships_lost += old.ships.len() - now.ships.len();
        }
        assert!(ships_lost > 0, "in diesem Gefecht geht etwas verloren");
        assert_eq!(settled.battles.total_ships_lost() as usize, ships_lost);
        let debris: f32 = report.sides.iter().map(|side| side.lost_value).sum();
        assert!((settled.battles.debris_in(battle.system) - debris).abs() < 0.5);
    }

    #[test]
    fn losses_are_named_heaviest_first() {
        let mut counts = [0; 6];
        assert_eq!(class_counts(&counts), None);
        counts[ShipClass::Corvette.index()] = 3;
        assert_eq!(class_counts(&counts).unwrap(), "3 Korvetten");
        counts[ShipClass::Cruiser.index()] = 1;
        counts[ShipClass::Fighter.index()] = 24;
        assert_eq!(
            class_counts(&counts).unwrap(),
            "1 Kreuzer, 3 Korvetten und 24 Jäger"
        );
        assert_eq!(listed(&["A".into(), "B".into()]), "A und B");
    }

    #[test]
    fn retreat_is_ordered_only_for_an_own_fleet_in_a_battle() {
        let (data, _, _, mut fleets) = setup();
        let mut battles = Battles::default();
        let me = PolityId(0);
        let error = order_retreat(&fleets, &mut battles, me, FleetId(0)).unwrap_err();
        assert!(error.contains("in keinem Gefecht"), "{error}");
        assert!(order_retreat(&fleets, &mut battles, me, FleetId(99)).is_err());
        battles.active.push(duel(&data, &mut fleets, 0, 1));
        battles.next_id = 1;
        assert_eq!(battles.find_anomaly(&fleets), None);
        assert!(order_retreat(&fleets, &mut battles, PolityId(1), FleetId(0)).is_err());
        order_retreat(&fleets, &mut battles, me, FleetId(0)).unwrap();
        let (battle, side) = battles.of_fleet(FleetId(0)).unwrap();
        assert!(battle.combatants[side].retreating);
        // Befohlen ist kein Patt.
        assert!(!battle.combatants[side].stalemate);
        assert!(!battle.is_fighting(side) && battle.is_fighting(1 - side));
        let again = order_retreat(&fleets, &mut battles, me, FleetId(0)).unwrap_err();
        assert!(again.contains("bereits"), "{again}");

        // Geänderte Befehle erreichen das laufende Gefecht.
        relay_orders(
            &data,
            &mut battles,
            FleetId(1),
            Formation::Pincer,
            Doctrine::Defensive,
        );
        let (battle, side) = battles.of_fleet(FleetId(1)).unwrap();
        assert_eq!(battle.combatants[side].formation, Formation::Pincer);
        assert_eq!(battle.combatants[side].doctrine, Doctrine::Defensive);
        assert_eq!(
            battles.in_system(fleets.list[0].system).unwrap().id,
            BattleId(0)
        );
        assert!(battles.get(BattleId(0)).is_some() && battles.get(BattleId(1)).is_none());
    }

    #[test]
    fn the_anomaly_check_ties_battles_and_fleets_together() {
        let (data, _, _, mut fleets) = setup();
        let mut battles = Battles::default();
        assert_eq!(battles.find_anomaly(&fleets), None);
        battles.active.push(duel(&data, &mut fleets, 0, 1));
        assert!(
            battles.find_anomaly(&fleets).is_some(),
            "Nummer jenseits des Zählers"
        );
        battles.next_id = 1;
        assert_eq!(battles.find_anomaly(&fleets), None);
        // Eine Flotte, die im Gefecht stehen soll, es aber nicht weiß, und umgekehrt.
        let mut broken = fleets.clone();
        broken.list[0].battle = None;
        assert!(battles.find_anomaly(&broken).is_some());
        let mut broken = fleets.clone();
        broken.list[2].in_battle = true;
        assert!(battles.find_anomaly(&broken).is_some());
        let mut broken = fleets.clone();
        broken.list[2].battle = Some(BattleId(0));
        broken.list[2].in_battle = true;
        assert!(battles.find_anomaly(&broken).is_some());
        // Ein zweites Gefecht im selben System.
        let mut doubled = battles.clone();
        doubled.active.push(battles.active[0].clone());
        doubled.next_id = 2;
        assert!(doubled.find_anomaly(&fleets).is_some());
        // Trümmer außer der Reihe oder ohne endlichen Wert.
        let debris = |system: u32, amount: f32| Debris {
            system: SystemId(system),
            amount,
            tick: 0,
        };
        let mut littered = battles.clone();
        littered.debris = vec![debris(4, 10.0), debris(2, 10.0)];
        assert!(littered.find_anomaly(&fleets).is_some());
        littered.debris = vec![debris(2, f32::NAN)];
        assert!(littered.find_anomaly(&fleets).is_some());
        littered.debris = vec![debris(2, 10.0), debris(4, 10.0)];
        assert_eq!(littered.find_anomaly(&fleets), None);
        assert_eq!(littered.debris_in(SystemId(4)), 10.0);
        assert_eq!(littered.debris_in(SystemId(3)), 0.0);
    }

    /// Ein Schiff, das nicht im Gefecht stand, lässt die Rückgabe unberührt.
    #[test]
    fn a_ship_that_was_not_in_the_battle_is_left_alone() {
        let (data, _, _, mut fleets) = setup();
        let battle = duel(&data, &mut fleets, 0, 1);
        let late = ShipId(9_999);
        let mut fleet = fleets.list[0].clone();
        let cruiser = data.design_index("speer").unwrap();
        fleet.ships.push(Ship {
            hull: 0.4,
            ..Ship::fresh(&data, late, cruiser)
        });
        assert_eq!(write_back(&battle.results(&data), 0, &mut fleet, None), 0);
        assert_eq!(fleet.ship(late).unwrap().hull, 0.4);
        assert_eq!(fleet.ships.len(), fleets.list[0].ships.len() + 1);
    }
}

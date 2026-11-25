//! Gefechte in der Partie.
//!
//! Stehen einsatzbereite Flotten verfeindeter Gemeinschaften im selben System, beginnt am
//! Tagesanfang ein Gefecht. Gerechnet wird es vom taktischen Rechner aus
//! [`crate::battle`], einen Schritt je Takt; dieses Modul stellt es auf, führt
//! Verstärkungen heran, reicht Befehle durch und schreibt am Ende das Ergebnis in die
//! Flotten zurück: verlorene Schiffe, Schäden, verbrauchte Vorräte, heimgekehrte
//! Bordjäger. Wer das Feld nicht hält, weicht in ein Nachbarsystem aus und sammelt sich.
//!
//! Die Folgen tragen den Krieg in die übrige Partie: Verluste heben die Kriegsmüdigkeit
//! (und damit Zinsen und Unruhe), eine schwere Niederlage drückt die Kurse, und was die
//! Flotte verschossen und eingebüßt hat, kauft der Staat später am Markt nach.

use super::{Fleet, Fleets, Ship, material_value};
use crate::battle::{
    Battle, BattleShip, CombatModifiers, CombatantSetup, Doctrine, Formation, ShipStatus,
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
use bevy_ecs::prelude::Resource;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
    /// Stand am Ende mit kämpfenden Schiffen auf dem Feld.
    pub held_field: bool,
    /// Der Rückzug war befohlen, von der eigenen Führung oder auf Befehl.
    pub retreated: bool,
}

/// Bilanz eines beendeten Gefechts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BattleReport {
    pub id: BattleId,
    pub system: SystemId,
    /// Takt des Beginns und des Endes.
    pub started: u64,
    pub ended: u64,
    /// An der Zeitgrenze ohne Entscheidung abgebrochen.
    pub timed_out: bool,
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

/// Gefechtspause in einem System nach einem Gefecht ohne Entscheidung.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BattlePause {
    pub system: SystemId,
    /// Erster Tag, an dem dort wieder ein Gefecht beginnen kann.
    pub until: u64,
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
    /// Gefechtspausen, aufsteigend nach System.
    pub pauses: Vec<BattlePause>,
    /// Verlorene Schiffe je Gemeinschaft über die ganze Partie, ohne Bordjäger. Lesen mit
    /// [`Battles::ships_lost_by`].
    pub ships_lost: Vec<u32>,
}

impl Battles {
    /// So viele Bilanzen bleiben im Zustand; ältere fallen heraus.
    pub const REPORTS: usize = 40;

    /// Das Gefecht, das in einem System läuft.
    pub fn in_system(&self, system: SystemId) -> Option<&Battle> {
        self.active.iter().find(|battle| battle.system == system)
    }

    /// Das Gefecht, in dem eine Flotte steht, und ihre Partei darin (Stelle in
    /// [`Battle::combatants`]).
    pub fn of_fleet(&self, fleet: FleetId) -> Option<(&Battle, usize)> {
        self.active.iter().find_map(|battle| {
            battle
                .combatants
                .iter()
                .position(|combatant| combatant.fleet == Some(fleet))
                .map(|side| (battle, side))
        })
    }

    pub fn get(&self, id: BattleId) -> Option<&Battle> {
        self.active.iter().find(|battle| battle.id == id)
    }

    /// Bilanz eines beendeten Gefechts, solange sie noch aufbewahrt wird.
    pub fn report(&self, id: BattleId) -> Option<&BattleReport> {
        self.reports.iter().find(|report| report.id == id)
    }

    /// Tag, an dem in einem System nach einer Gefechtspause wieder gekämpft werden kann.
    pub fn paused_until(&self, system: SystemId) -> Option<u64> {
        self.pauses
            .iter()
            .find(|pause| pause.system == system)
            .map(|pause| pause.until)
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

    /// Schiffe, die in allen Gefechten der Partie verloren gingen (ohne Bordjäger).
    pub fn total_ships_lost(&self) -> u32 {
        self.ships_lost.iter().sum()
    }

    fn side_mut(&mut self, fleet: FleetId) -> Option<(&mut Battle, usize)> {
        self.active.iter_mut().find_map(|battle| {
            battle
                .combatants
                .iter()
                .position(|combatant| combatant.fleet == Some(fleet))
                .map(|side| (battle, side))
        })
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
            for combatant in &battle.combatants {
                let Some(fleet) = combatant.fleet.and_then(|id| fleets.get(id)) else {
                    return Some(format!("Gefecht {}: Partei ohne Flotte", battle.id));
                };
                // Eine Flotte, die es nicht mehr gibt, bleibt als Partei stehen, bis das
                // Gefecht endet.
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

/// Aufschläge aus der Technologie einer Gemeinschaft für ihre Schiffe im Gefecht.
///
/// ANSCHLUSS FORSCHUNG: Dies ist die eine Stelle, an der die erforschten Technologien
/// einer Gemeinschaft (Waffenschaden, Reichweite, Schilde, Panzerung, Rumpf, Fahrt) in
/// [`CombatModifiers`] übersetzt werden. Bis die Forschung angeschlossen ist, kämpfen alle
/// mit neutralen Werten.
pub fn combat_modifiers(_data: &GameData, _owner: PolityId) -> CombatModifiers {
    CombatModifiers::default()
}

/// Moral, mit der die Besatzungen einer Gemeinschaft in ein Gefecht gehen: der Grundwert,
/// gemindert um die Kriegsmüdigkeit daheim und um ausstehenden Sold.
pub fn crew_morale(data: &GameData, finance: &Finance, fleets: &Fleets, owner: PolityId) -> f32 {
    let weariness = finance
        .polities
        .get(owner.index())
        .map_or(0.0, |state| state.war_weariness) as f32;
    (data.balance.battle.morale_start
        - data.balance.battle.morale_per_weariness * weariness
        - data.balance.military.morale_per_arrears * fleets.arrears_of(owner))
    .clamp(0.0, 100.0)
}

/// Hat die Partei noch Schiffe, die steuern und schießen können?
fn on_field(battle: &Battle, side: usize) -> bool {
    battle
        .ships
        .iter()
        .any(|ship| ship.side as usize == side && ship.status.can_act())
}

/// Nimmt eine Flotte als Partei in ein Gefecht auf: mit ihren stehenden Befehlen, der
/// Moral ihrer Gemeinschaft und jedem Schiff in dem Zustand, in dem es gerade ist.
fn join(
    data: &GameData,
    finance: &Finance,
    diplomacy: &Diplomacy,
    fleets: &mut Fleets,
    battle: &mut Battle,
    index: usize,
) {
    let owner = fleets.list[index].owner;
    let morale = crew_morale(data, finance, fleets, owner);
    let fleet = &mut fleets.list[index];
    let setup = CombatantSetup {
        fleet: Some(fleet.id),
        owner,
        formation: fleet.formation,
        doctrine: fleet.doctrine,
        modifiers: combat_modifiers(data, owner),
        morale,
    };
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

/// Tagesbeginn: Sammelfristen und Gefechtspausen laufen ab, Trümmer verblassen, laufende
/// Gefechte folgen der Kriegslage, Verstärkungen greifen ein, und wo sich einsatzbereite
/// Flotten verfeindeter Gemeinschaften gegenüberstehen, beginnt ein Gefecht.
pub fn open_battles(
    galaxy: &Galaxy,
    fleets: &mut Fleets,
    battles: &mut Battles,
    diplomacy: &Diplomacy,
    finance: &Finance,
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
    battles.pauses.retain(|pause| pause.until > day);
    if day.is_multiple_of(DAYS_PER_MONTH) && !battles.debris.is_empty() {
        for debris in &mut battles.debris {
            debris.amount *= 1.0 - balance.debris_fade_per_month;
        }
        battles
            .debris
            .retain(|debris| debris.amount >= DEBRIS_FLOOR);
    }

    // Wer gegen wen kämpft, folgt der Kriegslage: Ein Friedensschluss beendet das Gefecht,
    // eine neue Kriegserklärung unter Anwesenden weitet es aus.
    for battle in &mut battles.active {
        for a in 0..battle.combatants.len() {
            for b in (a + 1)..battle.combatants.len() {
                let hostile =
                    diplomacy.at_war(battle.combatants[a].owner, battle.combatants[b].owner);
                battle.set_hostile(a, b, hostile);
            }
        }
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
            let battle = &mut battles.active[position];
            for index in here {
                let owner = fleets.list[index].owner;
                let has_enemy = (0..battle.combatants.len()).any(|side| {
                    diplomacy.at_war(owner, battle.combatants[side].owner) && on_field(battle, side)
                });
                if !has_enemy {
                    continue;
                }
                join(data, finance, diplomacy, fleets, battle, index);
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
        if battles.paused_until(system).is_some() {
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
        let mut rng = Rng::stream(seed, Domain::Battle, id.0 as u64, system.0 as u64);
        let battle_seed = rng.next_u64();
        // Die Reihenfolge der Parteien lost das Gefecht aus, nicht die Flottennummer: Der
        // Rechner lässt die Schiffe der ersten Partei in jedem Takt zuerst handeln, und
        // dieser Vorteil soll nicht immer denselben Flotten zufallen. Die ersten beiden
        // stehen einander gegenüber: die ausgeloste Flotte und ihr erster Gegner.
        let listed_by_number = order.clone();
        rng.shuffle(&mut order);
        if let Some(foe) = order.iter().position(|other| at_war(order[0], *other)) {
            let foe = order.remove(foe);
            order.insert(1, foe);
        }
        let mut battle = Battle::new(id, system, battle_seed);
        for index in &order {
            join(data, finance, diplomacy, fleets, &mut battle, *index);
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
            battle, galaxy, fleets, battles, diplomacy, finance, logistics, polities, chronicle,
            data, tick,
        );
    }
    // Stärken nachführen; eine Flotte ohne Schiffe besteht nicht mehr.
    fleets.refresh(data);
}

/// Was die Rückgabe an eine Flotte über das hinaus ergab, was der Rechner gebucht hat.
#[derive(Clone, Debug, Default, PartialEq)]
struct WriteBack {
    /// Bauwert der Flotte vor dem Gefecht und was davon verloren ging, in Credits.
    value_before: f64,
    value_lost: f64,
    /// Verlorene Schiffe (ohne Bordjäger).
    ships_lost: u32,
    /// Verluste je Klasse, die der Rechner nicht als solche gezählt hat: treibende
    /// Schiffe einer Partei, die das Feld nicht hält, und Bordjäger ohne Platz.
    unbooked: [u32; 6],
}

/// Zustand eines Schiffs nach dem Gefecht: Rumpf, Module, Munition und Treibstoff als
/// Anteile am Höchstwert, im Hangar die Jäger, die nicht gestartet waren.
fn after_battle(data: &GameData, mut ship: Ship, fought: &BattleShip) -> Ship {
    let design = &data.designs[ship.design as usize];
    let share = |now: f32, full: f32, unchanged: f32| {
        if full > 0.0 {
            (now / full).clamp(0.0, 1.0)
        } else {
            unchanged
        }
    };
    ship.hull = share(fought.hull, fought.hull_max, ship.hull);
    ship.modules = design
        .modules
        .iter()
        .zip(&fought.modules)
        .map(|(slot, state)| share(state.hp, data.modules[slot.module_id as usize].hp, 1.0))
        .collect();
    if ship.modules.iter().all(|module| *module >= 1.0) {
        ship.modules.clear();
    }
    ship.ammo = share(fought.ammo, fought.ammo_max, ship.ammo);
    ship.fuel = share(fought.fuel, fought.fuel_max, ship.fuel);
    ship.fighters = fought.hangar.min(design.stats.fighters);
    ship
}

/// Schreibt das Ergebnis eines beendeten Gefechts in die Flotte einer Partei zurück.
///
/// Verloren ist ein Schiff, das zerstört wurde oder am Ende treibt, ohne dass seine
/// Partei das Feld hält. Jedes andere behält, was das Gefecht von ihm übrig ließ.
/// Überlebende Bordjäger kehren heim: zuerst auf den eigenen Träger, sonst auf einen
/// anderen der Flotte mit freiem Platz; für wen kein Platz bleibt, der ist verloren.
fn write_back(data: &GameData, battle: &Battle, side: usize, fleet: &mut Fleet) -> WriteBack {
    let held = battle.is_fighting(side);
    let survives = |ship: &BattleShip| match ship.status {
        ShipStatus::Destroyed => false,
        // Ein treibendes Schiff birgt nur, wer das Feld hält.
        ShipStatus::Disabled => held,
        ShipStatus::Active | ShipStatus::Routing | ShipStatus::Escaped => true,
    };
    let fighter = ShipClass::Fighter.index();
    let mut result = WriteBack {
        value_before: fleet
            .ships
            .iter()
            .map(|ship| material_value(data, ship))
            .sum(),
        ..Default::default()
    };
    let places: BTreeMap<ShipId, usize> = battle
        .ships
        .iter()
        .enumerate()
        .filter(|(_, ship)| ship.side as usize == side)
        .filter_map(|(index, ship)| ship.source.map(|source| (source, index)))
        .collect();

    // Platz im Gefecht → Platz in der Flotte danach: dorthin kehren die Jäger heim.
    let mut kept: Vec<Ship> = Vec::with_capacity(fleet.ships.len());
    let mut homes: BTreeMap<u32, usize> = BTreeMap::new();
    for ship in std::mem::take(&mut fleet.ships) {
        let Some(place) = places.get(&ship.id).copied() else {
            // War nicht im Gefecht: bleibt, wie es ist.
            kept.push(ship);
            continue;
        };
        let fought = &battle.ships[place];
        if !survives(fought) {
            result.ships_lost += 1;
            if fought.status != ShipStatus::Destroyed {
                result.unbooked[fought.class.index()] += 1;
                result.unbooked[fighter] += fought.hangar;
            }
            continue;
        }
        homes.insert(place as u32, kept.len());
        kept.push(after_battle(data, ship, fought));
    }

    let has_room = |ship: &Ship, craft: u16| {
        let stats = &data.designs[ship.design as usize].stats;
        stats.craft == Some(craft) && ship.fighters < stats.fighters
    };
    let mut homeless: Vec<u16> = Vec::new();
    for craft in battle
        .ships
        .iter()
        .filter(|ship| ship.side as usize == side && ship.source.is_none())
        .filter(|ship| survives(ship))
    {
        let home = craft
            .carrier
            .and_then(|carrier| homes.get(&carrier).copied());
        match home {
            Some(home) if has_room(&kept[home], craft.design) => kept[home].fighters += 1,
            _ => homeless.push(craft.design),
        }
    }
    for craft in homeless {
        match kept.iter_mut().find(|ship| has_room(ship, craft)) {
            Some(ship) => ship.fighters += 1,
            None => result.unbooked[fighter] += 1,
        }
    }

    let value_after: f64 = kept.iter().map(|ship| material_value(data, ship)).sum();
    result.value_lost = (result.value_before - value_after).max(0.0);
    fleet.ships = kept;
    result
}

/// Wohin eine Flotte ausweicht, die das Feld räumt: dorthin, woher sie kam, wenn das ein
/// Nachbarsystem ist; sonst in ein Nachbarsystem der eigenen Gemeinschaft; sonst in eines
/// ohne feindliche Flotten; sonst in das mit der kleinsten Nummer. `None` nur, wenn das
/// System keine Nachbarn hat.
pub fn retreat_target(
    galaxy: &Galaxy,
    fleets: &Fleets,
    diplomacy: &Diplomacy,
    fleet: &Fleet,
) -> Option<SystemId> {
    let neighbors = galaxy.neighbors(fleet.system);
    let adjacent = |system: SystemId| neighbors.iter().any(|neighbor| neighbor.system == system);
    if let Some(origin) = fleet.came_from.filter(|origin| adjacent(*origin)) {
        return Some(origin);
    }
    let own = neighbors
        .iter()
        .find(|neighbor| galaxy.system_owner(neighbor.system) == Some(fleet.owner));
    let quiet = || {
        neighbors.iter().find(|neighbor| {
            !fleets
                .docked_in(neighbor.system)
                .any(|other| diplomacy.at_war(other.owner, fleet.owner))
        })
    };
    own.or_else(quiet)
        .or(neighbors.first())
        .map(|neighbor| neighbor.system)
}

/// Schließt ein beendetes Gefecht ab: Rückgabe an die Flotten, Abzug der Unterlegenen,
/// Bilanz, Trümmer, Kriegsmüdigkeit, Kursschock, Chronik.
fn conclude(
    battle: &Battle,
    galaxy: &Galaxy,
    fleets: &mut Fleets,
    battles: &mut Battles,
    diplomacy: &Diplomacy,
    finance: &mut Finance,
    logistics: &Logistics,
    polities: &Polities,
    chronicle: &mut Chronicle,
    data: &GameData,
    tick: u64,
) {
    let balance = &data.balance.military;
    let day = tick / TICKS_PER_DAY;
    let system = battle.system;
    let system_name = &galaxy.system(system).name;

    // Bauwert der gesamten Flotte jeder beteiligten Gemeinschaft vor der Rückgabe: Daran
    // misst sich, wie schwer ein Verlust wiegt.
    let mut tally: BTreeMap<PolityId, OwnerTally> = BTreeMap::new();
    for combatant in &battle.combatants {
        tally.entry(combatant.owner).or_insert_with(|| OwnerTally {
            navy: fleets
                .of(combatant.owner)
                .flat_map(|fleet| fleet.ships.iter())
                .map(|ship| material_value(data, ship))
                .sum(),
            ..Default::default()
        });
    }

    let mut sides: Vec<SideReport> = Vec::with_capacity(battle.combatants.len());
    let mut fates: Vec<String> = Vec::new();
    let mut debris = 0.0f32;
    for (side, combatant) in battle.combatants.iter().enumerate() {
        let held = battle.is_fighting(side);
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
        };
        let owner = tally.get_mut(&combatant.owner).expect("eben angelegt");
        owner.held |= held;
        let attached = combatant.fleet.map(FleetId::index).filter(|index| {
            fleets
                .list
                .get(*index)
                .is_some_and(|fleet| fleet.alive && fleet.battle == Some(battle.id))
        });
        let Some(index) = attached else {
            // Die Flotte gibt es nicht mehr: Es bleibt bei dem, was der Rechner gebucht hat.
            owner.lost += report.lost_value as f64;
            debris += report.lost_value;
            fates.push(format!("{} besteht nicht mehr.", report.fleet_name));
            sides.push(report);
            continue;
        };
        let result = write_back(data, battle, side, &mut fleets.list[index]);
        for class in 0..report.lost.len() {
            report.lost[class] += result.unbooked[class];
        }
        // Gerechnet wird mit dem Bauwert der Flotte, wie sie dastand; der Rechner zählt
        // die Bordjäger eines Trägers doppelt.
        report.initial_value = result.value_before as f32;
        report.lost_value = result.value_lost as f32;
        owner.lost += result.value_lost;
        debris += report.lost_value;
        if battles.ships_lost.len() <= combatant.owner.index() {
            battles.ships_lost.resize(combatant.owner.index() + 1, 0);
        }
        battles.ships_lost[combatant.owner.index()] += result.ships_lost;

        let fleet = &mut fleets.list[index];
        fleet.battle = None;
        fleet.in_battle = false;
        let losses = class_counts(&report.lost);
        let fate = if fleet.ships.is_empty() {
            match &losses {
                Some(losses) => format!("verliert {losses} und besteht nicht mehr"),
                None => "besteht nicht mehr".into(),
            }
        } else if held || battle.timed_out {
            // Wer das Feld hält, bleibt. An der Zeitgrenze zieht niemand ab, auch wer
            // gerade keine kämpfenden Schiffe mehr auf dem Feld hatte.
            match &losses {
                Some(losses) => format!("verliert {losses}"),
                None => "bleibt ohne Verluste".into(),
            }
        } else {
            // Wer das Feld nicht hält, räumt das System und sammelt sich.
            let target = retreat_target(galaxy, fleets, diplomacy, &fleets.list[index]);
            let fleet = &mut fleets.list[index];
            fleet.regroup_until = Some(day + balance.regroup_days);
            if let Some(target) = target {
                fleet.route = vec![target];
                fleet.progress = 0.0;
            }
            let place = match target {
                Some(target) => format!("weicht nach {} aus", galaxy.system(target).name),
                None => "sammelt sich".into(),
            };
            match &losses {
                Some(losses) => format!("verliert {losses} und {place}"),
                None => format!("{place}, ohne Verluste"),
            }
        };
        fates.push(format!("{} {fate}.", report.fleet_name));
        sides.push(report);
    }

    // Verluste machen kriegsmüde, im Verhältnis zum Anteil an der ganzen Flotte; eine
    // schwere Niederlage erschüttert dazu die Kurse.
    let mut shaken: Vec<PolityId> = Vec::new();
    for (polity, owner) in &tally {
        if owner.lost <= 0.0 || owner.navy <= 0.0 {
            continue;
        }
        let share = (owner.lost / owner.navy).clamp(0.0, 1.0);
        if let Some(state) = finance.polities.get_mut(polity.index()) {
            state.war_weariness =
                (state.war_weariness + balance.weariness_per_navy_lost * share).clamp(0.0, 100.0);
        }
        if !owner.held && share >= balance.heavy_defeat_share {
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
    if battle.timed_out {
        // Ohne Entscheidung: Im System ruhen die Waffen eine Weile, sonst begänne dasselbe
        // Gefecht am nächsten Morgen von vorn.
        let pause = BattlePause {
            system,
            until: day + balance.battle_pause_days,
        };
        match battles
            .pauses
            .binary_search_by_key(&system, |entry| entry.system)
        {
            Ok(found) => battles.pauses[found] = pause,
            Err(place) => battles.pauses.insert(place, pause),
        }
    }

    let report = BattleReport {
        id: battle.id,
        system,
        // Jedes Gefecht rechnet vom Takt seines Beginns an einen Schritt je Takt.
        started: (tick + 1).saturating_sub(battle.tick),
        ended: tick,
        timed_out: battle.timed_out,
        sides,
    };
    let holders: Vec<&SideReport> = report.sides.iter().filter(|side| side.held_field).collect();
    let duration = match report.days() {
        1 => "einem Tag".to_string(),
        days => format!("{days} Tagen"),
    };
    let headline = if report.timed_out {
        format!("Gefecht bei {system_name} nach {duration} ohne Entscheidung abgebrochen.")
    } else if holders.is_empty() {
        format!("Gefecht bei {system_name} nach {duration} beendet: Niemand hält das Feld.")
    } else if holders.len() == report.sides.len() {
        format!(
            "Gefecht bei {system_name} nach {duration} eingestellt: Zwischen den Beteiligten herrscht kein Krieg mehr."
        )
    } else {
        let names: Vec<String> = holders.iter().map(|side| side.fleet_name.clone()).collect();
        format!(
            "Gefecht bei {system_name} nach {duration} entschieden: {} {} das Feld.",
            listed(&names),
            if names.len() == 1 { "hält" } else { "halten" }
        )
    };
    // Die Farbe des Siegers, wenn es genau einen gibt.
    let victor = holders
        .first()
        .map(|side| side.owner)
        .filter(|owner| !report.timed_out && holders.iter().all(|side| side.owner == *owner));
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
    if battles.reports.len() >= Battles::REPORTS {
        battles.reports.remove(0);
    }
    battles.reports.push(report);
}

/// Bilanz einer Gemeinschaft über alle ihre Flotten in einem Gefecht.
#[derive(Clone, Copy, Debug, Default)]
struct OwnerTally {
    /// Bauwert ihrer gesamten Flotte vor der Rückgabe, in Credits.
    navy: f64,
    lost: f64,
    /// Mindestens eine ihrer Flotten hielt das Feld.
    held: bool,
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
    use super::*;

    /// Ein Gefecht zweier Flotten, die einander feind sind, noch vor dem ersten Schritt.
    fn duel(data: &GameData, fleets: &mut Fleets, first: usize, second: usize) -> Battle {
        let mut diplomacy = Diplomacy::default();
        diplomacy
            .wars
            .insert((fleets.list[first].owner, fleets.list[second].owner));
        let finance = Finance::default();
        fleets.list[second].system = fleets.list[first].system;
        let mut battle = Battle::new(BattleId(0), fleets.list[first].system, 1);
        for index in [first, second] {
            join(data, &finance, &diplomacy, fleets, &mut battle, index);
        }
        battle
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
        let weary = battle_balance.morale_start - 40.0 * battle_balance.morale_per_weariness;
        let unpaid = battle_balance.morale_start - 0.5 * data.balance.military.morale_per_arrears;
        assert_eq!(crew_morale(&data, &finance, &fleets, PolityId(0)), weary);
        assert_eq!(crew_morale(&data, &finance, &fleets, PolityId(1)), unpaid);
        assert!(weary < battle_balance.morale_start && unpaid < battle_balance.morale_start);
        // Eine Gemeinschaft ohne Finanzdaten (frisch abgespalten) kämpft mit dem Grundwert.
        assert_eq!(
            crew_morale(&data, &finance, &fleets, PolityId(7)),
            battle_balance.morale_start
        );

        let mut diplomacy = Diplomacy::default();
        diplomacy.wars.insert((PolityId(0), PolityId(1)));
        let mut battle = Battle::new(BattleId(3), fleets.list[0].system, 1);
        for index in [0, 1, 2] {
            join(&data, &finance, &diplomacy, &mut fleets, &mut battle, index);
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
            let fought = battle
                .ships
                .iter()
                .find(|fought| fought.source == Some(ship.id))
                .expect("Schiff im Gefecht");
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
    fn survivors_keep_what_the_battle_left_of_them_and_the_lost_are_gone() {
        let (data, _, _, mut fleets) = setup();
        let mut battle = duel(&data, &mut fleets, 0, 1);
        let before = fleets.list[0].ships.clone();
        let value = fleets.list[0].build_value(&data);
        // Das Ende von Hand gesetzt: ein Schiff zerstört, eines beschädigt, eines treibt.
        let place = |battle: &Battle, ship: &Ship| {
            battle
                .ships
                .iter()
                .position(|fought| fought.source == Some(ship.id))
                .unwrap()
        };
        let (wreck, hurt, adrift) = (
            place(&battle, &before[0]),
            place(&battle, &before[1]),
            place(&battle, &before[2]),
        );
        battle.ships[wreck].status = ShipStatus::Destroyed;
        battle.ships[wreck].hull = 0.0;
        battle.ships[hurt].hull *= 0.25;
        battle.ships[hurt].modules[0].hp = 0.0;
        battle.ships[hurt].ammo *= 0.5;
        battle.ships[hurt].fuel *= 0.75;
        battle.ships[adrift].status = ShipStatus::Disabled;
        battle.finished = true;

        // Die Partei hält das Feld: Das treibende Schiff wird geborgen.
        let mut holding = fleets.list[0].clone();
        let result = write_back(&data, &battle, 0, &mut holding);
        assert_eq!(result.ships_lost, 1);
        assert_eq!(result.unbooked, [0; 6]);
        assert!((result.value_before - value).abs() < 1e-6);
        let cruiser = data.designs[before[0].design as usize].stats.value;
        assert!((result.value_lost - cruiser).abs() < 1e-6);
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

        // Hält die Partei das Feld nicht, ist das treibende Schiff verloren, und die
        // Bilanz holt nach, was der Rechner nicht gebucht hat.
        for ship in &mut battle.ships {
            if ship.side == 0 && ship.status == ShipStatus::Active {
                ship.status = ShipStatus::Escaped;
            }
        }
        let mut routed = fleets.list[0].clone();
        let result = write_back(&data, &battle, 0, &mut routed);
        assert_eq!(result.ships_lost, 2);
        let class = data.designs[before[2].design as usize].stats.class;
        let mut unbooked = [0; 6];
        unbooked[class.index()] = 1;
        assert_eq!(result.unbooked, unbooked);
        assert!(routed.ships.iter().all(|ship| ship.id != before[2].id));
        assert_eq!(routed.ships.len(), before.len() - 2);
    }

    #[test]
    fn craft_return_to_carriers_of_their_fleet_only_while_there_is_room() {
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
        let place = |battle: &Battle, id: ShipId| {
            battle
                .ships
                .iter()
                .position(|fought| fought.source == Some(id))
                .unwrap()
        };
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

        // Der erste Träger geht verloren, seine Staffel ist in der Luft und überlebt.
        battle.ships[first_place].status = ShipStatus::Destroyed;
        battle.finished = true;
        let mut fleet = fleets.list[0].clone();
        let result = write_back(&data, &battle, 0, &mut fleet);
        assert!(fleet.ships.iter().all(|ship| ship.id != first));
        let survivor = fleet.ships.iter().find(|ship| ship.id == second).unwrap();
        // Der zweite Träger nimmt seine eigenen Jäger zurück und füllt die Lücke mit
        // Heimatlosen; mehr als sein Hangar fasst, kommt nicht an Bord.
        assert_eq!(survivor.fighters, stats.fighters);
        let mut unbooked = [0; 6];
        unbooked[ShipClass::Fighter.index()] = stats.fighters - missing;
        assert_eq!(result.unbooked, unbooked);
        assert_eq!(result.ships_lost, 1);
        // Verloren ist der Träger samt Staffel, abzüglich der Jäger, die unterkamen.
        let lost = stats.value - missing as f64 * craft_value;
        assert!(
            (result.value_lost - lost).abs() < 1e-6,
            "{}",
            result.value_lost
        );

        // Geht kein Träger verloren, kehrt jede Staffel auf ihr eigenes Schiff zurück.
        battle.ships[first_place].status = ShipStatus::Active;
        let mut fleet = fleets.list[0].clone();
        let result = write_back(&data, &battle, 0, &mut fleet);
        let on_board = |fleet: &Fleet, id: ShipId| fleet.ship(id).unwrap().fighters;
        assert_eq!(on_board(&fleet, first), stats.fighters);
        assert_eq!(on_board(&fleet, second), stats.fighters - missing);
        assert_eq!((result.ships_lost, result.unbooked), (0, [0; 6]));
        assert!(result.value_lost.abs() < 1e-6);
        // Zerstörte Jäger fehlen danach im Hangar.
        let downed = battle
            .ships
            .iter()
            .position(|ship| ship.carrier == Some(second_place as u32))
            .unwrap();
        battle.ships[downed].status = ShipStatus::Destroyed;
        let mut fleet = fleets.list[0].clone();
        let result = write_back(&data, &battle, 0, &mut fleet);
        assert_eq!(on_board(&fleet, second), stats.fighters - missing - 1);
        assert!((result.value_lost - craft_value).abs() < 1e-6);
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
            retreat_target(galaxy, fleets, &diplomacy, &fleets.list[0])
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
    }

    #[test]
    fn at_the_time_limit_nobody_leaves_and_the_system_gets_a_pause() {
        let (data, galaxy, polities, mut fleets) = setup();
        let balance = &data.balance.military;
        let mut diplomacy = Diplomacy::default();
        diplomacy.wars.insert((PolityId(0), PolityId(1)));
        let logistics = Logistics::default();
        let mut battle = duel(&data, &mut fleets, 0, 1);
        let system = battle.system;
        // Am Ende ist die zweite Partei auf der Flucht: Sie hält das Feld nicht.
        for ship in &mut battle.ships {
            if ship.side == 1 {
                ship.status = ShipStatus::Escaped;
            }
        }
        battle.finished = true;
        battle.tick = 9_000;
        let (tick, day) = (20_000, 20_000 / TICKS_PER_DAY);
        let settle = |battle: &Battle, fleets: &mut Fleets| {
            let mut battles = Battles {
                next_id: 1,
                ..Default::default()
            };
            let mut chronicle = Chronicle::default();
            conclude(
                battle,
                &galaxy,
                fleets,
                &mut battles,
                &diplomacy,
                &mut Finance::default(),
                &logistics,
                &polities,
                &mut chronicle,
                &data,
                tick,
            );
            (battles, chronicle.entries[0].text.clone())
        };

        // An der Zeitgrenze: Beide bleiben im System, niemand sammelt sich, die Waffen ruhen.
        battle.timed_out = true;
        let mut stalemate = fleets.clone();
        let (battles, text) = settle(&battle, &mut stalemate);
        for fleet in &stalemate.list[..2] {
            assert!(!fleet.in_battle && fleet.battle.is_none());
            assert!(fleet.route.is_empty() && fleet.is_ready());
        }
        assert_eq!(
            battles.paused_until(system),
            Some(day + balance.battle_pause_days)
        );
        let report = &battles.reports[0];
        assert!(report.timed_out);
        assert!(report.sides[0].held_field && !report.sides[1].held_field);
        assert_eq!((report.started, report.ended), (tick + 1 - 9_000, tick));
        assert_eq!(report.days(), 450);
        assert!(text.contains("ohne Entscheidung"), "{text}");
        assert!(battles.debris.is_empty() && battles.total_ships_lost() == 0);
        assert_eq!(battles.find_anomaly(&stalemate), None);

        // Dasselbe Ende vor der Zeitgrenze: Wer das Feld nicht hält, weicht aus und sammelt
        // sich; eine Gefechtspause gibt es dann nicht.
        battle.timed_out = false;
        let mut decided = fleets.clone();
        let (battles, text) = settle(&battle, &mut decided);
        let (held, left) = (&decided.list[0], &decided.list[1]);
        assert!(held.is_ready() && held.route.is_empty());
        assert_eq!(left.route, vec![galaxy.neighbors(system)[0].system]);
        assert_eq!(left.regroup_until, Some(day + balance.regroup_days));
        assert!(!left.in_battle && !left.is_ready());
        assert_eq!(battles.paused_until(system), None);
        assert!(
            text.contains("hält das Feld") && text.contains("weicht nach"),
            "{text}"
        );
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
}

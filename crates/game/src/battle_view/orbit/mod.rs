//! Orbitansicht einer belagerten Welt.
//!
//! Die Belagerung gehört der Simulation. Hier wird sie gezeigt: der Planet, die Flotten des
//! Belagerers im Orbit, Stationen, Geschütze und Schild der Welt, Beschuss, Landung und der
//! Kampf am Boden. Gelesen wird allein das Lesemodell des Kerns (`sim::siege::view`), Tag
//! für Tag mit der Uhr der Partie; gerechnet wird in der Ansicht nichts. Was die Szene an
//! Bewegung zeigt (Geschosse, Landungsboote), ist Schaustellung dessen, was der Kern für
//! den Tag meldet: Wo er keinen Beschuss meldet, fällt kein Schuss.
//!
//! Kamera, Himmel und Schiffsmodelle sind die der Schlachtansicht; das Licht ist eigenes
//! (eine kräftige Sonne, dunkle Nachtseite). Die Oberfläche der Welt entsteht prozedural
//! (`planet_look`) abseits des Hauptfadens.

mod atmosphere;
pub mod hud;
mod night_lights;
pub mod planet_look;
mod scene;

pub use scene::OrbitCensus;

use super::BattleFeedSet;
use super::campaign::{WatchBattle, can_watch};
use crate::session::{Notice, SessionEpoch};
use crate::state::{AppState, ViewState};
use crate::time_control::SimSpeed;
use crate::ui::UiState;
use crate::ui::siege::{may_see, names, units};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy_egui::EguiPrimaryContextPass;
use sim::chronicle::{Chronicle, ChronicleKind};
use sim::clock::{Date, SimClock, TICKS_PER_DAY};
use sim::data::Rgb;
use sim::diplomacy::Diplomacy;
use sim::economy::Economy;
use sim::finance::Finance;
use sim::fleet::{Battles, Fleets};
use sim::galaxy::Galaxy;
use sim::ids::{BattleId, PlanetId};
use sim::logistics::Logistics;
use sim::polity::Polities;
use sim::siege::{Phase, SiegeCard, SiegeRefs, Sieges, ZoneKind, siege_card};
use sim::state::SimConfig;
use sim::tech::Research;
use sim::{GameData, SimRoot};
use std::hash::{Hash, Hasher};

/// Beschriftungen. Das README nennt sie; ein Test hält beide beieinander.
pub const WATCH: &str = "Ansehen";
pub const TITLE: &str = "Orbitansicht";
pub const BACK: &str = "Zur Karte";
pub const TO_WINDOW: &str = "Belagerungsfenster";
pub const RESULT_FELL: &str = "Die Welt ist gefallen";
pub const RESULT_OVER: &str = "Die Belagerung ist beendet";

/// So viele Rümpfe zeigt die Szene höchstens; größere Flotten stehen im Maßstab da.
pub const MAX_HULLS: u32 = 48;
/// So viele Meldungen hält die Ansicht.
const KEPT_EVENTS: usize = 7;

/// Bitte, die Belagerung dieser Welt in der Orbitansicht zu zeigen.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct WatchSiege(pub PlanetId);

/// Wie die gezeigte Belagerung ausging, in den Worten der Chronik.
#[derive(Clone, Debug, PartialEq)]
pub struct SiegeResult {
    /// Die Welt hat den Besitzer gewechselt.
    pub fell: bool,
    pub headline: String,
    pub lines: Vec<String>,
}

/// Die Belagerung, die die Orbitansicht gerade zeigt.
#[derive(Resource, Clone, Debug)]
pub struct WatchedSiege {
    pub planet: PlanetId,
    /// Partie, zu der sie gehört; nach Laden oder Neubeginn gilt sie nicht mehr.
    epoch: u32,
    /// Die Karte des Kerns vom letzten gelesenen Stand.
    pub card: SiegeCard,
    /// Woran die Ansicht erkennt, dass sie neu lesen muss.
    stamp: u64,
    /// Aufstellung der Szene zu dieser Karte.
    pub layout: Layout,
    /// Schiffe der belagernden Flotten beim letzten Lesen.
    ships: u32,
    /// Was sich seit dem Öffnen geändert hat: Datum und Satz, die jüngste zuletzt.
    pub events: Vec<(Date, String)>,
    /// Zahl der Landungen, die die Szene zeigen soll (zählt hoch).
    pub landings: u32,
    /// Ausgang, sobald die Simulation die Belagerung beendet hat.
    pub result: Option<SiegeResult>,
    /// Der Spieler hat das Fenster mit dem Ausgang geschlossen.
    pub result_closed: bool,
}

impl WatchedSiege {
    pub fn ended(&self) -> bool {
        self.result.is_some()
    }

    fn note(&mut self, date: Date, text: impl Into<String>) {
        self.events.push((date, text.into()));
        if self.events.len() > KEPT_EVENTS {
            self.events.remove(0);
        }
    }

    /// Übernimmt einen neuen Stand der Karte: hält als Meldungen fest, was sich geändert
    /// hat, und zählt eine Landung, damit die Szene ihre Boote schickt. Die Aufstellung
    /// der Szene setzt der Aufrufer.
    pub fn take(&mut self, date: Date, card: SiegeCard, ships: u32) {
        let (notes, landed) = changes(&self.card, &card, (self.ships, ships));
        for note in notes {
            self.note(date, note);
        }
        if landed {
            self.landings += 1;
        }
        self.ships = ships;
        self.card = card;
    }
}

/// Was die Szene aufstellt. Ändert es sich, baut sie sich neu auf; solange es gleich
/// bleibt, bewegt sich nur, was sich bewegen soll.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    /// Gezeigte Rümpfe je Klasse (Index wie `ShipClass::index()`).
    pub hulls: [u32; 6],
    /// So viele Schiffe stehen für einen gezeigten Rumpf.
    pub per_hull: u32,
    pub stations: u32,
    pub guns: u32,
    pub guns_active: bool,
    pub shield: u8,
    /// Hält der Schild noch Ladung?
    pub shielded: bool,
    pub bunkers: u8,
    /// Zonen der Oberfläche: Art, Anteil, genommen.
    pub zones: Vec<(ZoneKind, f32, bool)>,
    pub phase: Phase,
    pub control: bool,
    /// Steht eine Landungstruppe am Boden?
    pub landed: bool,
    pub attacker: Rgb,
    pub defender: Rgb,
}

/// Schiffe je Klasse in den belagernden Flotten.
pub fn ships_by_class(data: &GameData, fleets: &Fleets, card: &SiegeCard) -> [u32; 6] {
    let mut counts = [0u32; 6];
    for fleet in card.fleets.iter().filter_map(|id| fleets.get(*id)) {
        for ship in &fleet.ships {
            if let Some(design) = data.designs.get(ship.design as usize) {
                counts[design.stats.class.index()] += 1;
            }
        }
    }
    counts
}

/// Gezeigte Rümpfe je Klasse und der Maßstab dazu: Jede vertretene Klasse zeigt
/// mindestens einen Rumpf, zusammen höchstens etwa [`MAX_HULLS`].
pub fn shown_hulls(counts: [u32; 6]) -> ([u32; 6], u32) {
    let total: u32 = counts.iter().sum();
    let per_hull = total.div_ceil(MAX_HULLS).max(1);
    (counts.map(|count| count.div_ceil(per_hull)), per_hull)
}

impl Layout {
    pub fn of(data: &GameData, polities: &Polities, fleets: &Fleets, card: &SiegeCard) -> Self {
        let (hulls, per_hull) = shown_hulls(ships_by_class(data, fleets, card));
        let defense = &card.defense;
        Self {
            hulls,
            per_hull,
            stations: defense.stations.ceil().max(0.0) as u32,
            guns: defense.guns.ceil().max(0.0) as u32,
            guns_active: defense.guns_active,
            shield: defense.shield,
            shielded: defense.shield > 0 && defense.charge > 0.0,
            bunkers: defense.bunkers,
            zones: card
                .view
                .siege
                .zones
                .iter()
                .map(|zone| (zone.kind, zone.share, zone.taken))
                .collect(),
            phase: card.phase,
            control: card.control,
            landed: card.troops.army > 0.0,
            attacker: polities.get(card.attacker).color,
            defender: polities.get(card.defender).color,
        }
    }
}

/// Lesender Zugriff auf alles, was das Lesemodell der Belagerung braucht.
#[derive(SystemParam)]
pub struct SiegeWorld<'w> {
    pub data: Res<'w, GameData>,
    pub galaxy: Res<'w, Galaxy>,
    pub polities: Res<'w, Polities>,
    pub diplomacy: Res<'w, Diplomacy>,
    pub economy: Res<'w, Economy>,
    pub finance: Res<'w, Finance>,
    pub logistics: Res<'w, Logistics>,
    pub fleets: Res<'w, Fleets>,
    pub research: Res<'w, Research>,
    pub sieges: Res<'w, Sieges>,
    pub clock: Res<'w, SimClock>,
    pub config: Res<'w, SimConfig>,
    pub chronicle: Res<'w, Chronicle>,
    pub battles: Res<'w, Battles>,
    pub epoch: Res<'w, SessionEpoch>,
}

impl SiegeWorld<'_> {
    pub fn refs(&self) -> SiegeRefs<'_> {
        SiegeRefs {
            data: &self.data,
            galaxy: &self.galaxy,
            polities: &self.polities,
            diplomacy: &self.diplomacy,
            economy: &self.economy,
            finance: &self.finance,
            logistics: &self.logistics,
            fleets: &self.fleets,
            research: &self.research,
            sieges: &self.sieges,
        }
    }

    pub fn card(&self, planet: PlanetId) -> Option<SiegeCard> {
        siege_card(&self.refs(), planet, self.clock.day())
    }

    /// Darf der Spieler die Belagerung dieser Welt sehen?
    pub fn may_see(&self, card: &SiegeCard) -> bool {
        may_see(
            &self.galaxy,
            &self.fleets,
            self.config.player,
            card.attacker,
            card.defender,
            card.system,
        )
    }

    /// Das Gefecht im System der Belagerung, wenn der Spieler es ansehen darf.
    pub fn battle_at(&self, card: &SiegeCard) -> Option<BattleId> {
        self.battles
            .in_system(card.system)
            .filter(|battle| can_watch(&self.galaxy, &self.fleets, self.config.player, battle))
            .map(|battle| battle.id)
    }

    /// Woran die Ansicht erkennt, dass sich die Lage geändert hat: der Tag, die stehenden
    /// Befehle, Truppen am Boden, die ausstehende Übergabe und die Flotten im System.
    fn stamp(&self, planet: PlanetId) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.clock.day().hash(&mut hasher);
        self.sieges.pending.len().hash(&mut hasher);
        match self.sieges.siege(planet) {
            Some(siege) => {
                let orders = siege.orders;
                (orders.fire as u8, orders.assault, orders.focus, orders.sack).hash(&mut hasher);
                siege.army.to_bits().hash(&mut hasher);
                siege.deep.to_bits().hash(&mut hasher);
                let defense = self.sieges.defense(planet);
                (defense.posture as u8, defense.hold_fire).hash(&mut hasher);
            }
            None => 0u8.hash(&mut hasher),
        }
        let system = self.galaxy.planet(planet).system;
        for fleet in self.fleets.docked_in(system) {
            (fleet.id, fleet.in_battle, fleet.ships.len()).hash(&mut hasher);
        }
        hasher.finish()
    }
}

/// Was sich von einem Stand der Karte zum nächsten geändert hat, als Sätze für die
/// Meldungen der Ansicht; dazu, ob Truppen gelandet sind.
pub fn changes(old: &SiegeCard, new: &SiegeCard, ships: (u32, u32)) -> (Vec<String>, bool) {
    let mut notes = Vec::new();
    if old.control != new.control {
        notes.push(if new.control {
            "Der Belagerer hält den Orbit wieder; die Belagerung geht weiter.".to_string()
        } else {
            "Der Orbit ist umstritten: Die Belagerung ruht.".to_string()
        });
    }
    if old.phase_number != new.phase_number {
        notes.push(format!(
            "Phase {} von 5: {}.",
            new.phase_number, new.phase_label
        ));
    }
    let lost = |before: f64, after: f64| before - after > 0.05;
    if lost(old.defense.stations, new.defense.stations) {
        notes.push(format!(
            "Orbitalstationen getroffen: noch {} Stufen.",
            units(new.defense.stations)
        ));
    }
    if lost(old.defense.guns, new.defense.guns) {
        notes.push(format!(
            "Bodengeschütze getroffen: noch {} Stufen.",
            units(new.defense.guns)
        ));
    }
    if old.defense.charge > 0.0 && new.defense.charge <= 0.0 && new.defense.shield > 0 {
        notes.push("Der Schild ist zusammengebrochen.".to_string());
    } else if old.defense.charge <= 0.0 && new.defense.charge > 0.0 {
        notes.push("Der Schild steht wieder.".to_string());
    }
    let landed = new.troops.army - old.troops.army > 0.05
        && new.view.siege.landed_day != old.view.siege.landed_day;
    if landed {
        notes.push(format!(
            "Landung: Die Truppe am Boden zählt {} Verbände.",
            units(new.troops.army)
        ));
    }
    let (old_zones, new_zones) = (&old.view.siege.zones, &new.view.siege.zones);
    for (before, after) in old_zones.iter().zip(new_zones) {
        if !before.taken && after.taken {
            notes.push(format!("Zone {} genommen.", after.kind.label()));
        } else if before.taken && !after.taken {
            notes.push(format!(
                "Zone {} wieder in der Hand der Verteidiger.",
                after.kind.label()
            ));
        }
    }
    if new.view.siege.report.repelled && !old.view.siege.report.repelled {
        notes.push("Die Landung ist abgeschlagen.".to_string());
    }
    if ships.1 < ships.0 {
        let gone = ships.0 - ships.1;
        notes.push(if gone == 1 {
            "Der Belagerer verliert ein Schiff im Orbit.".to_string()
        } else {
            format!("Der Belagerer verliert {gone} Schiffe im Orbit.")
        });
    }
    if let (None, Some(outcome)) = (old.handover, new.handover) {
        notes.push(format!(
            "Entschieden ({}): Die Übergabe der Welt steht aus.",
            outcome.label()
        ));
    }
    (notes, landed)
}

/// Der Ausgang einer Belagerung, die die Simulation beendet hat: ob die Welt den Besitzer
/// gewechselt hat, und was die Chronik der letzten Tage über sie sagt.
pub fn result_of(
    galaxy: &Galaxy,
    chronicle: &Chronicle,
    clock: &SimClock,
    last: &SiegeCard,
) -> SiegeResult {
    let fell = galaxy.planet(last.planet).owner == Some(last.attacker);
    let since = clock.tick.saturating_sub(2 * TICKS_PER_DAY);
    // Was der Krieg an der Welt entschieden hat, steht voran; Folgen (ein verfallener
    // Bauauftrag, eine neue Hauptwelt) danach.
    let mut entries: Vec<_> = chronicle
        .entries
        .iter()
        .filter(|entry| entry.tick >= since && names(&entry.text, &last.planet_name))
        .collect();
    entries.sort_by_key(|entry| {
        !(entry.kind == ChronicleKind::War
            && (entry.text.contains("besetzt") || entry.text.contains("aufgehoben")))
    });
    let lines: Vec<String> = entries.iter().map(|entry| entry.text.clone()).collect();
    let headline = if fell {
        format!("{RESULT_FELL}: {}", last.planet_name)
    } else {
        format!("{RESULT_OVER}: {}", last.planet_name)
    };
    SiegeResult {
        fell,
        headline,
        lines,
    }
}

/// Öffnet die Orbitansicht für eine laufende Belagerung. Die Zeit der Partie läuft weiter,
/// wie sie eingestellt ist.
fn open_requested(
    mut commands: Commands,
    mut requests: MessageReader<WatchSiege>,
    world: SiegeWorld,
    mut notice: ResMut<Notice>,
    mut ui_state: ResMut<UiState>,
    mut next: ResMut<NextState<ViewState>>,
) {
    let Some(WatchSiege(planet)) = requests.read().last().copied() else {
        return;
    };
    let Some(card) = world.card(planet) else {
        notice.info("Diese Belagerung ist schon zu Ende; ihr Ausgang steht in der Chronik.");
        return;
    };
    if !world.may_see(&card) {
        notice.info(
            "Diese Belagerung ist nicht zu sehen: Dort steht weder eine eigene Flotte noch \
             liegt dort eine eigene Welt.",
        );
        return;
    }
    let layout = Layout::of(&world.data, &world.polities, &world.fleets, &card);
    let ships = ships_by_class(&world.data, &world.fleets, &card)
        .iter()
        .sum();
    // Eine Landung der letzten beiden Tage ist noch zu sehen.
    let today = world.clock.day();
    let fresh_landing = card
        .view
        .siege
        .landed_day
        .is_some_and(|day| day + 1 >= today);
    commands.insert_resource(WatchedSiege {
        planet,
        epoch: world.epoch.0,
        stamp: world.stamp(planet),
        layout,
        ships,
        events: Vec::new(),
        landings: u32::from(fresh_landing),
        result: None,
        result_closed: false,
        card,
    });
    ui_state.menu_open = false;
    next.set(ViewState::Siege);
}

/// Nach jedem Takt der Simulation: Hat sich die Lage geändert (ein neuer Tag, ein Befehl,
/// ein Gefecht im Orbit), liest die Ansicht die Karte neu und hält fest, was anders ist.
/// Gibt es die Belagerung nicht mehr, zeigt sie den Ausgang, und die Partie hält an.
fn follow(watched: Option<ResMut<WatchedSiege>>, world: SiegeWorld, mut speed: ResMut<SimSpeed>) {
    let Some(mut watched) = watched else {
        return;
    };
    if watched.epoch != world.epoch.0 || watched.ended() {
        return;
    }
    let stamp = world.stamp(watched.planet);
    if stamp == watched.stamp {
        return;
    }
    watched.stamp = stamp;
    let date = world.clock.date();
    match world.card(watched.planet) {
        Some(card) => {
            let ships: u32 = ships_by_class(&world.data, &world.fleets, &card)
                .iter()
                .sum();
            watched.layout = Layout::of(&world.data, &world.polities, &world.fleets, &card);
            watched.take(date, card, ships);
        }
        None => {
            let result = result_of(&world.galaxy, &world.chronicle, &world.clock, &watched.card);
            watched.note(date, result.headline.clone());
            watched.result = Some(result);
            // Während der Spieler den Ausgang liest, vergeht keine Zeit ungesehen.
            speed.paused = true;
        }
    }
}

/// Gehört die gezeigte Belagerung nicht mehr zur laufenden Partie (geladen, neu begonnen),
/// geht es zurück zur Karte.
fn guard(
    watched: Option<Res<WatchedSiege>>,
    epoch: Res<SessionEpoch>,
    mut next: ResMut<NextState<ViewState>>,
) {
    if watched.is_none_or(|watched| watched.epoch != epoch.0) {
        next.set(ViewState::Galaxy);
    }
}

fn leave(mut commands: Commands, mut ui_state: ResMut<UiState>) {
    commands.remove_resource::<WatchedSiege>();
    ui_state.help_open = false;
}

/// Aus der Orbitansicht führt „Gefecht ansehen“ in die Schlachtansicht: Sie wird über
/// die Karte betreten, also geht es erst dorthin zurück und die Bitte reist mit.
#[derive(Resource, Default)]
struct BattleAfterLeave(Option<BattleId>);

fn forward_battle(mut pending: ResMut<BattleAfterLeave>, mut watch: MessageWriter<WatchBattle>) {
    if let Some(battle) = pending.0.take() {
        watch.write(WatchBattle(battle));
    }
}

pub(super) fn plugin(app: &mut App) {
    app.add_message::<WatchSiege>()
        .init_resource::<BattleAfterLeave>()
        .add_systems(
            Update,
            (
                (forward_battle, open_requested).run_if(in_state(ViewState::Galaxy)),
                guard.run_if(in_state(ViewState::Siege)),
            )
                .run_if(in_state(AppState::InGame)),
        )
        .add_systems(
            FixedUpdate,
            follow
                .in_set(BattleFeedSet::Siege)
                .after(SimRoot)
                .run_if(in_state(ViewState::Siege)),
        )
        .add_systems(
            OnEnter(ViewState::Siege),
            (scene::show_view, scene::enter).chain(),
        )
        .add_systems(
            OnExit(ViewState::Siege),
            (scene::exit, super::scene::hide_view, leave).chain(),
        )
        .add_systems(
            EguiPrimaryContextPass,
            hud::draw_hud.run_if(in_state(ViewState::Siege)),
        );
    scene::plugin(app);
    atmosphere::plugin(app);
    night_lights::plugin(app);
}

#[cfg(test)]
mod tests;

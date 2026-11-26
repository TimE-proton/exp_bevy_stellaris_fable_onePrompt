//! Gefechte der laufenden Partie in der Schlachtansicht.
//!
//! Das Gefecht gehört der Simulation (`Battles::active`) und rückt mit ihrem Takt vor. Hier
//! wird nach jedem Takt nur sein Stand in den [`BattleFeed`] gelegt; gerechnet wird in der
//! Ansicht nichts. Befehle aus der Ansicht gehen als `SimCommand` in die Warteschlange der
//! Partie, wie jeder andere Befehl des Spielers.

use super::feed::{
    BattleFeed, BattleOrder, BattleResult, BattleViewRequest, FeedInfo, ResultSide, SideFate,
    SideInfo,
};
use super::{BattleFeedSet, ViewOptions};
use crate::map::MapCamera;
use crate::session::{Notice, SessionEpoch};
use crate::state::{AppState, ViewState};
use crate::time_control::SimSpeed;
use crate::ui::format::decimal;
use crate::ui::{COMMAND_WAITS, UiState};
use bevy::prelude::*;
use sim::battle::Battle;
use sim::clock::{Date, SimClock, TICKS_PER_DAY};
use sim::command::{CommandQueue, CommandSource, SimCommand};
use sim::fleet::{BattleOutcome, BattleReport, Battles, Fleets, SideReport};
use sim::galaxy::Galaxy;
use sim::ids::{BattleId, PolityId};
use sim::polity::Polities;
use sim::state::SimConfig;
use sim::{GameData, SimRoot};

/// Beschriftung des Knopfs, der ein Gefecht der Partie in der Schlachtansicht öffnet.
pub const WATCH: &str = "Gefecht ansehen";

/// Bitte, ein laufendes Gefecht der Partie in der Schlachtansicht zu zeigen.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct WatchBattle(pub BattleId);

/// Das Gefecht der Partie, das die Schlachtansicht gerade zeigt.
#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub struct WatchedBattle {
    pub id: BattleId,
    /// Partie, zu der das Gefecht gehört; nach Laden oder Neubeginn gilt die Nummer nicht
    /// mehr.
    epoch: u32,
    /// Die Simulation hat das Gefecht abgeschlossen; gezeigt wird sein Ergebnis.
    pub ended: bool,
}

/// Wo auf dem Bildschirm die Gefechte stehen, die der Spieler ansehen darf: An dieser
/// Stelle der Karte bietet die Oberfläche den Knopf zur Schlachtansicht an.
#[derive(Resource, Default, Debug, Clone, PartialEq)]
pub struct BattleMarkers(pub Vec<BattleMarker>);

#[derive(Debug, Clone, PartialEq)]
pub struct BattleMarker {
    pub id: BattleId,
    /// Ort des Systems im Fenster (logische Pixel).
    pub at: Vec2,
    pub system: String,
}

/// Darf der Spieler dieses Gefecht ansehen? Sein eigenes immer; ein fremdes nur in einem
/// System, in dem er selbst eine Flotte oder eine Welt hat: Dort sieht er, was geschieht.
/// Beobachter ohne eigene Gemeinschaft sehen jedes.
pub fn can_watch(
    galaxy: &Galaxy,
    fleets: &Fleets,
    player: Option<PolityId>,
    battle: &Battle,
) -> bool {
    let Some(player) = player else {
        return true;
    };
    battle
        .combatants
        .iter()
        .any(|combatant| combatant.owner == player)
        || fleets
            .docked_in(battle.system)
            .any(|fleet| fleet.owner == player)
        || galaxy
            .planets_of(player)
            .any(|planet| planet.system == battle.system)
}

/// Sekunden Gefechtszeit je Kalendertag der Partie.
pub fn battle_seconds_per_day(data: &GameData) -> f32 {
    TICKS_PER_DAY as f32 * data.balance.battle.dt
}

/// Datum der Partie und das Verhältnis der Zeiten, für die Kopfleiste der Ansicht.
fn calendar(data: &GameData, clock: &SimClock) -> String {
    format!(
        "{} · ein Kalendertag = {} s Gefechtszeit",
        clock.date(),
        decimal(battle_seconds_per_day(data) as f64, 1)
    )
}

/// Parteien des Gefechts: jede Flotte eine, benannt nach der Flotte und gefärbt wie ihre
/// Gemeinschaft. Wer ausgeschieden ist, wird nicht mehr geführt.
fn feed_info(
    data: &GameData,
    galaxy: &Galaxy,
    polities: &Polities,
    fleets: &Fleets,
    battles: &Battles,
    clock: &SimClock,
    player: Option<PolityId>,
    battle: &Battle,
) -> FeedInfo {
    let sides = battle
        .combatants
        .iter()
        .map(|combatant| {
            let polity = polities.get(combatant.owner);
            let name = combatant
                .fleet
                .and_then(|fleet| fleets.get(fleet))
                .map_or_else(|| polity.name.clone(), |fleet| fleet.name.clone());
            SideInfo {
                name,
                color: polity.color,
            }
        })
        .collect();
    let mut own = (0..battle.combatants.len()).filter(|side| {
        Some(battle.combatants[*side].owner) == player && !battles.has_departed(battle.id, *side)
    });
    FeedInfo {
        title: format!("Gefecht bei {}", galaxy.system(battle.system).name),
        sides,
        player_side: own.next(),
        own_sides: own.collect(),
        can_restart: false,
        still: false,
        retreat_waiting: Vec::new(),
        calendar: calendar(data, clock),
        result: None,
    }
}

/// Wo die Flotte einer Partei nach dem Gefecht ist.
fn whereabouts(galaxy: &Galaxy, fleets: &Fleets, report: &BattleReport, side: usize) -> String {
    let entry = &report.sides[side];
    let fleet = entry
        .fleet
        .and_then(|fleet| fleets.get(fleet))
        .filter(|fleet| fleet.alive);
    let Some(fleet) = fleet else {
        return "Die Flotte besteht nicht mehr.".into();
    };
    let here = &galaxy.system(report.system).name;
    match (fleet.regroup_until, fleet.route.last()) {
        (Some(until), Some(target)) => format!(
            "Weicht aus nach {} und sammelt sich bis zum {}.",
            galaxy.system(*target).name,
            Date::from_day(until)
        ),
        (Some(until), None) if fleet.system != report.system => format!(
            "Ist nach {} ausgewichen und sammelt sich bis zum {}.",
            galaxy.system(fleet.system).name,
            Date::from_day(until)
        ),
        (Some(until), None) => format!(
            "Bleibt hinter den eigenen Linien in {here} und sammelt sich bis zum {}.",
            Date::from_day(until)
        ),
        (None, _) if fleet.system == report.system => format!("Bleibt einsatzbereit in {here}."),
        (None, _) => format!("Steht in {}.", galaxy.system(fleet.system).name),
    }
}

/// Hält die Partei am Ende das Feld? Wer vorzeitig ausschied, hält es nicht mehr: Bei ihm
/// sagt `held_field` nur, ob er beim Ausscheiden noch auf dem Feld stand. So zählt auch
/// der Kern in seiner Chronik.
fn holds_at_end(side: &SideReport) -> bool {
    side.held_field && side.left_early.is_none()
}

/// Das Ergebnis in Worten, aus der Bilanz der Simulation.
pub fn result_of(
    galaxy: &Galaxy,
    fleets: &Fleets,
    polities: &Polities,
    report: &BattleReport,
) -> BattleResult {
    let place = &galaxy.system(report.system).name;
    let holders: Vec<&str> = report
        .sides
        .iter()
        .filter(|side| holds_at_end(side))
        .map(|side| side.fleet_name.as_str())
        .collect();
    let headline = match report.outcome {
        BattleOutcome::Decided => match holders.as_slice() {
            [one] => format!("Gefecht bei {place} entschieden: {one} hält das Feld."),
            many => format!(
                "Gefecht bei {place} entschieden: {} halten das Feld.",
                many.join(" und ")
            ),
        },
        BattleOutcome::Stalemate => format!(
            "Gefecht bei {place} im Patt beendet: Keine Seite kam gegen die andere an, niemand \
             hat gewonnen."
        ),
        BattleOutcome::TimedOut => {
            format!("Gefecht bei {place} ohne Entscheidung abgebrochen: Niemand hat gewonnen.")
        }
        BattleOutcome::CalledOff => format!(
            "Gefecht bei {place} eingestellt: Zwischen den Beteiligten herrscht kein Krieg mehr."
        ),
        BattleOutcome::Abandoned => {
            format!("Gefecht bei {place} beendet: Niemand hält das Feld.")
        }
    };
    let sides = report
        .sides
        .iter()
        .enumerate()
        .map(|(index, side)| {
            let note = if side.left_early.is_some() && side.held_field {
                "vorzeitig ausgeschieden, als kein Gegner mehr auf dem Feld stand"
            } else if side.left_early.is_some() {
                "vorzeitig ausgeschieden, hat das Feld geräumt"
            } else if side.stalemate {
                "im Patt gelöst"
            } else if side.retreated {
                "Rückzug"
            } else {
                ""
            };
            ResultSide {
                name: side.fleet_name.clone(),
                color: polities.get(side.owner).color,
                initial: side.initial,
                lost: side.lost,
                escaped: side.escaped,
                loss_share: if side.initial_value > 0.0 {
                    side.lost_value / side.initial_value
                } else {
                    0.0
                },
                // Ohne Entscheidung hält niemand das Feld als Sieger.
                holds_field: holds_at_end(side) && !report.outcome.undecided(),
                note: note.into(),
                whereabouts: whereabouts(galaxy, fleets, report, index),
            }
        })
        .collect();
    BattleResult {
        headline,
        days: report.days(),
        sides,
    }
}

/// Führt nach, wo die Marken der sichtbaren Gefechte auf der Karte stehen.
fn locate_markers(
    camera: Single<(&Camera, &GlobalTransform), With<MapCamera>>,
    galaxy: Res<Galaxy>,
    fleets: Res<Fleets>,
    battles: Res<Battles>,
    config: Res<SimConfig>,
    mut markers: ResMut<BattleMarkers>,
) {
    let (camera, global) = *camera;
    let found: Vec<BattleMarker> = battles
        .active
        .iter()
        .filter(|battle| can_watch(&galaxy, &fleets, config.player, battle))
        .filter_map(|battle| {
            let system = galaxy.system(battle.system);
            let at = camera
                .world_to_viewport(global, Vec3::new(system.pos[0], system.pos[1], 0.0))
                .ok()?;
            Some(BattleMarker {
                id: battle.id,
                at,
                system: system.name.clone(),
            })
        })
        .collect();
    if markers.0 != found {
        markers.0 = found;
    }
}

fn clear_markers(mut markers: ResMut<BattleMarkers>) {
    markers.0.clear();
}

/// Öffnet die Schlachtansicht für ein laufendes Gefecht der Partie. Die Zeit der Partie
/// läuft weiter, wie sie eingestellt ist.
fn open_requested(
    mut commands: Commands,
    mut requests: MessageReader<WatchBattle>,
    data: Res<GameData>,
    galaxy: Res<Galaxy>,
    polities: Res<Polities>,
    fleets: Res<Fleets>,
    battles: Res<Battles>,
    clock: Res<SimClock>,
    config: Res<SimConfig>,
    epoch: Res<SessionEpoch>,
    mut feed: ResMut<BattleFeed>,
    mut notice: ResMut<Notice>,
    mut ui_state: ResMut<UiState>,
    mut next: ResMut<NextState<ViewState>>,
) {
    let Some(WatchBattle(id)) = requests.read().last().copied() else {
        return;
    };
    let Some(battle) = battles.get(id) else {
        notice.info("Das Gefecht ist schon zu Ende; sein Ausgang steht in der Chronik.");
        return;
    };
    if !can_watch(&galaxy, &fleets, config.player, battle) {
        notice.info(
            "Dieses Gefecht ist nicht zu sehen: Dort steht weder eine eigene Flotte noch \
             liegt dort eine eigene Welt.",
        );
        return;
    }
    let info = feed_info(
        &data,
        &galaxy,
        &polities,
        &fleets,
        &battles,
        &clock,
        config.player,
        battle,
    );
    feed.begin(battle, info);
    commands.insert_resource(WatchedBattle {
        id,
        epoch: epoch.0,
        ended: false,
    });
    ui_state.menu_open = false;
    next.set(ViewState::Battle);
}

/// Nach jedem Takt der Simulation: den Stand des Gefechts in den Feed legen. Ist es dort
/// abgeschlossen, zeigt die Ansicht das Ergebnis aus der Bilanz der Simulation, und die
/// Partie hält an: Während der Spieler das Ergebnis liest, vergeht keine Zeit ungesehen.
/// Lässt er sie weiterlaufen, führt die Kopfleiste das Datum weiter mit.
fn follow(
    watched: Option<ResMut<WatchedBattle>>,
    data: Res<GameData>,
    galaxy: Res<Galaxy>,
    polities: Res<Polities>,
    fleets: Res<Fleets>,
    battles: Res<Battles>,
    clock: Res<SimClock>,
    config: Res<SimConfig>,
    epoch: Res<SessionEpoch>,
    mut feed: ResMut<BattleFeed>,
    mut options: ResMut<ViewOptions>,
    mut speed: ResMut<SimSpeed>,
) {
    let Some(mut watched) = watched else {
        return;
    };
    if watched.epoch != epoch.0 {
        return;
    }
    if watched.ended {
        feed.info.calendar = calendar(&data, &clock);
        return;
    }
    match battles.get(watched.id) {
        Some(battle) => {
            // Parteien kommen hinzu (Verstärkung) und scheiden aus; Namen, Farben und wen
            // der Spieler führt, gelten jeweils für den Stand dieses Takts.
            feed.info = feed_info(
                &data,
                &galaxy,
                &polities,
                &fleets,
                &battles,
                &clock,
                config.player,
                battle,
            );
            feed.refresh(battle);
        }
        None => {
            watched.ended = true;
            match battles.report(watched.id) {
                Some(report) => {
                    feed.info.player_side = None;
                    feed.info.own_sides.clear();
                    feed.info.retreat_waiting.clear();
                    feed.info.calendar = calendar(&data, &clock);
                    // Die Simulation räumt das Gefecht im Takt seines letzten Schritts ab
                    // und bewahrt den Endstand des Rechners einige Tage auf: Er ist der
                    // letzte gezeigte Stand. Fehlt er (ein Spielstand, der ihn nicht
                    // führt), wird der vorletzte Stand der Bilanz nach Anzahl angeglichen.
                    let fates: Vec<SideFate> = match battles.concluded(watched.id) {
                        Some(last) => {
                            feed.refresh(last);
                            Vec::new()
                        }
                        None => report
                            .sides
                            .iter()
                            .map(|side| SideFate {
                                lost: side.lost,
                                escaped: side.escaped,
                            })
                            .collect(),
                    };
                    feed.conclude(result_of(&galaxy, &fleets, &polities, report), &fates);
                    options.summary = true;
                    speed.paused = true;
                }
                // Ohne Bilanz gibt es nichts mehr zu zeigen.
                None => feed.clear(),
            }
        }
    }
}

/// Merkt an den Parteien vor, für welche der Rückzug schon befohlen ist und nur auf den
/// nächsten Takt wartet (die Zeit steht): Die Ansicht bietet ihn dann nicht noch einmal
/// an, denn die Wiederholung lehnte die Simulation ab.
fn mark_waiting(
    watched: Option<Res<WatchedBattle>>,
    battles: Res<Battles>,
    queue: Res<CommandQueue>,
    mut feed: ResMut<BattleFeed>,
) {
    let Some(battle) = watched.and_then(|watched| battles.get(watched.id)) else {
        return;
    };
    let waiting: Vec<usize> = (0..battle.combatants.len())
        .filter(|side| {
            battle.combatants[*side].fleet.is_some_and(|fleet| {
                queue
                    .pending
                    .iter()
                    .any(|entry| entry.command == SimCommand::Retreat { fleet })
            })
        })
        .collect();
    if feed.info.retreat_waiting != waiting {
        feed.info.retreat_waiting = waiting;
    }
}

/// Befehle aus der Ansicht gehen als Befehle des Spielers an die Simulation. Sie gelten
/// mit dem nächsten Takt; steht die Zeit, erfährt der Spieler, dass sie vorgemerkt sind.
fn relay_orders(
    mut orders: MessageReader<BattleOrder>,
    watched: Option<Res<WatchedBattle>>,
    battles: Res<Battles>,
    config: Res<SimConfig>,
    speed: Res<SimSpeed>,
    mut queue: ResMut<CommandQueue>,
    mut notice: ResMut<Notice>,
) {
    let (Some(watched), Some(player)) = (watched, config.player) else {
        orders.clear();
        return;
    };
    let Some(battle) = battles.get(watched.id) else {
        orders.clear();
        return;
    };
    for order in orders.read() {
        let (side, command) = match *order {
            BattleOrder::SetOrders {
                side,
                formation,
                doctrine,
            } => (
                side,
                battle
                    .combatants
                    .get(side)
                    .and_then(|c| c.fleet)
                    .map(|fleet| SimCommand::SetFleetOrders {
                        fleet,
                        formation,
                        doctrine,
                    }),
            ),
            BattleOrder::Retreat { side } => (
                side,
                battle
                    .combatants
                    .get(side)
                    .and_then(|c| c.fleet)
                    .map(|fleet| SimCommand::Retreat { fleet }),
            ),
        };
        // Nur die eigenen Parteien nehmen Befehle des Spielers an.
        let own = battle
            .combatants
            .get(side)
            .is_some_and(|combatant| combatant.owner == player)
            && !battles.has_departed(battle.id, side);
        if let (true, Some(command)) = (own, command) {
            // Was schon wartet, wird nicht doppelt gegeben: Den zweiten Rückzug lehnte die
            // Simulation ab.
            if queue.pending.iter().any(|entry| entry.command == command) {
                continue;
            }
            queue.push(player, CommandSource::Player, command);
            if speed.paused {
                notice.info(COMMAND_WAITS);
            }
        }
    }
}

/// „Verlassen“ führt zurück zur Karte, jederzeit; das Gefecht läuft in der Partie weiter.
fn handle_view_requests(
    mut requests: MessageReader<BattleViewRequest>,
    mut next: ResMut<NextState<ViewState>>,
) {
    for request in requests.read() {
        if *request == BattleViewRequest::Leave {
            next.set(ViewState::Galaxy);
        }
    }
}

/// Gehört das gezeigte Gefecht nicht mehr zur laufenden Partie (geladen, neu begonnen) oder
/// gibt es von ihm nichts mehr zu zeigen, geht es zurück zur Karte.
fn guard(
    watched: Option<Res<WatchedBattle>>,
    epoch: Res<SessionEpoch>,
    feed: Res<BattleFeed>,
    mut next: ResMut<NextState<ViewState>>,
) {
    if watched.is_some_and(|watched| watched.epoch != epoch.0 || !feed.is_active()) {
        next.set(ViewState::Galaxy);
    }
}

/// Beim Verlassen der Ansicht bleibt vom gezeigten Gefecht der Partie nichts im Feed.
fn leave(
    mut commands: Commands,
    watched: Option<Res<WatchedBattle>>,
    mut feed: ResMut<BattleFeed>,
    mut ui_state: ResMut<UiState>,
) {
    if watched.is_some() {
        commands.remove_resource::<WatchedBattle>();
        feed.clear();
    }
    ui_state.help_open = false;
}

/// Meldet dem Spieler ein Gefecht, an dem er beteiligt ist, sobald es beginnt.
fn announce(
    battles: Res<Battles>,
    galaxy: Res<Galaxy>,
    config: Res<SimConfig>,
    epoch: Res<SessionEpoch>,
    mut notice: ResMut<Notice>,
    mut seen: Local<Option<(u32, u32)>>,
) {
    let known = match *seen {
        Some((session, next)) if session == epoch.0 => next,
        // Eine andere Partie: Was dort schon läuft, ist keine Neuigkeit.
        _ => battles.next_id,
    };
    if let Some(player) = config.player
        && let Some(battle) = battles.active.iter().find(|battle| {
            battle.id.0 >= known && battle.combatants.iter().any(|c| c.owner == player)
        })
    {
        notice.info(format!(
            "Bei {} beginnt ein Gefecht mit eigenen Schiffen. „{WATCH}“ an der Karte, im \
             Flottenfenster oder in der Chronik zeigt es in der Schlachtansicht.",
            galaxy.system(battle.system).name
        ));
    }
    *seen = Some((epoch.0, battles.next_id));
}

pub(super) fn plugin(app: &mut App) {
    app.add_message::<WatchBattle>()
        .init_resource::<BattleMarkers>()
        .add_systems(OnExit(ViewState::Galaxy), clear_markers)
        .add_systems(
            Update,
            (
                (locate_markers, open_requested).run_if(in_state(ViewState::Galaxy)),
                announce,
                (handle_view_requests, relay_orders, mark_waiting, guard)
                    .chain()
                    .run_if(in_state(ViewState::Battle)),
            )
                .run_if(in_state(AppState::InGame)),
        )
        .add_systems(
            FixedUpdate,
            follow
                .in_set(BattleFeedSet::Campaign)
                .after(SimRoot)
                .run_if(in_state(AppState::InGame)),
        )
        .add_systems(OnExit(ViewState::Battle), leave);
}

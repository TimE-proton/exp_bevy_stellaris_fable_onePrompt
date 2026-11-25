//! Selbsttest: startet eine Partie oder eine Gefechtsübung, lässt sie einige Frames
//! laufen, speichert auf Wunsch Bildschirmfotos und beendet sich. Damit lässt sich prüfen,
//! dass Fenster, Karte, Schlachtansicht, Oberfläche und Simulation gemeinsam hochkommen,
//! auch ohne Menschen davor.

use crate::args::{Args, numbered};
use crate::battle_view::feed::BattleFeed;
use crate::battle_view::practice::LINEUP_ROW_GAP;
use crate::battle_view::{
    CameraRequest, EffectStats, PART_LABELS, SelectedShip, ViewOptions, ViewTimings, to_world,
};
use crate::map::{FocusSystem, Selection};
use crate::session::SessionRequest;
use crate::state::AppState;
use crate::time_control::{SPEEDS, SimSpeed};
use crate::ui::{LeftTab, UiState};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use sim::battle::{Battle, BeamKind};
use sim::clock::SimClock;
use sim::galaxy::Galaxy;

#[derive(Resource)]
struct Selftest {
    total: u32,
    frame: u32,
    shot_taken: bool,
}

pub struct SelftestPlugin;

impl Plugin for SelftestPlugin {
    fn build(&self, app: &mut App) {
        let args = app.world().resource::<Args>();
        let Some(frames) = args.selftest else {
            return;
        };
        if args.practice {
            app.insert_resource(PracticeTest::new(frames, args.lineup))
                .add_systems(
                    Update,
                    (
                        drive_practice.run_if(in_state(AppState::Practice)),
                        practice_never_started.run_if(not(in_state(AppState::Practice))),
                    ),
                );
            return;
        }
        app.insert_resource(Selftest {
            total: frames.max(30),
            frame: 0,
            shot_taken: false,
        })
        .add_systems(OnEnter(AppState::MainMenu), start)
        .add_systems(Update, drive.run_if(in_state(AppState::InGame)));
    }
}

fn start(args: Res<Args>, test: Res<Selftest>, mut requests: MessageWriter<SessionRequest>) {
    if test.frame == 0 && args.seed.is_none() && args.load.is_none() {
        requests.write(SessionRequest::NewGame { seed: 1 });
    }
}

fn drive(
    mut commands: Commands,
    args: Res<Args>,
    mut test: ResMut<Selftest>,
    mut speed: ResMut<SimSpeed>,
    selection: Res<Selection>,
    mut ui_state: ResMut<UiState>,
    clock: Res<SimClock>,
    galaxy: Res<Galaxy>,
    mut focus: MessageWriter<FocusSystem>,
    mut exit: MessageWriter<AppExit>,
) {
    test.frame += 1;
    let total = test.total;
    if test.frame == 5 {
        // Zeit laufen lassen, schnellste Stufe.
        speed.paused = false;
        speed.level = SPEEDS.len() - 1;
    }
    if test.frame == total / 2
        && let Some(system) = selection.system
    {
        // Zur Halbzeit heranzoomen und die datenlastigen Tafeln öffnen.
        focus.write(FocusSystem(system));
        ui_state.tab = LeftTab::Ownership;
        ui_state.exchange_open = true;
    }
    if test.frame == total.saturating_sub(12)
        && !test.shot_taken
        && let Some(path) = args.screenshot.clone()
    {
        test.shot_taken = true;
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
    }
    if test.frame >= total {
        let owned = galaxy
            .planets
            .iter()
            .filter(|planet| planet.owner.is_some())
            .count();
        println!(
            "SELBSTTEST OK: {} Frames, Datum {}, Takt {}, {} von {} Planeten besiedelt",
            test.frame,
            clock.date(),
            clock.tick,
            owned,
            galaxy.planets.len()
        );
        exit.write(AppExit::Success);
    }
}

// --- Gefechtsübung ---------------------------------------------------------------------

/// Frames, die nach einem Kamerasprung vergehen, bevor fotografiert wird, und danach,
/// bevor es weitergeht (das Foto wird erst einige Frames später geschrieben).
const SETTLE_FRAMES: u32 = 6;
/// Stufe der Zeitsteuerung während des Gefechts: schnell, aber langsam genug, um den
/// richtigen Augenblick zu erwischen.
const FIGHT_SPEED: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    /// Annäherung, bis der erste Schuss fällt.
    Approach,
    /// Mitten im Gefecht: Übersicht mit beiden Flotten.
    Overview,
    /// Nahaufnahme eines Schusswechsels.
    CloseUp,
    /// Spät im Gefecht: Wracks und Trümmer.
    Aftermath,
    /// Bis zum Ende laufen lassen.
    Finish,
    /// Schaustellung: alle Klassen, dann die kleinen, dann die großen.
    Lineup(u32),
    Done,
}

/// Was nach dem Einschwingen der Kamera geschieht.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pending {
    frames_left: u32,
    /// Nummer des Fotos; 0 heißt: nur warten.
    photo: u32,
    next: Phase,
    /// Stufe, mit der es danach weitergeht; `None` lässt die Zeit angehalten.
    resume: Option<usize>,
}

#[derive(Resource)]
struct PracticeTest {
    limit: u32,
    lineup: bool,
    frame: u32,
    phase: Phase,
    pending: Option<Pending>,
    /// Gefechtszeit des ersten Schusses.
    contact: Option<f32>,
    photos: u32,
    peak_in_play: usize,
}

impl PracticeTest {
    fn new(limit: u32, lineup: bool) -> Self {
        Self {
            limit: limit.max(60),
            lineup,
            frame: 0,
            phase: if lineup {
                Phase::Lineup(1)
            } else {
                Phase::Approach
            },
            pending: None,
            contact: None,
            photos: 0,
            peak_in_play: 0,
        }
    }
}

/// Die Übung kam nie zustande (etwa wegen einer fehlerhaften Flottenliste).
fn practice_never_started(mut frames: Local<u32>, mut exit: MessageWriter<AppExit>) {
    *frames += 1;
    if *frames > 20 {
        println!("SELBSTTEST FEHLER: Die Gefechtsübung hat nicht begonnen");
        exit.write(AppExit::error());
    }
}

/// Ein Strahl, um den herum gerade viel geschieht: Schütze, Ziel, Nummer des Schützen.
fn busy_exchange(battle: &Battle, feed: &BattleFeed, relaxed: bool) -> Option<(Vec3, Vec3, u32)> {
    let mut best: Option<(f32, Vec3, Vec3, u32)> = None;
    for (index, beam) in battle.beams.iter().enumerate() {
        // Nur frische Strahlen: Sie sind auf dem Foto noch kräftig.
        if beam.kind == BeamKind::PointDefense || beam.ttl + 1e-4 < feed.beam_life(index) {
            continue;
        }
        let Some(shooter) = battle
            .ships
            .iter()
            .position(|ship| ship.pos == beam.from && ship.status.in_play())
        else {
            continue;
        };
        let ship = &battle.ships[shooter];
        if !relaxed && !ship.class.is_capital() {
            continue;
        }
        let near = |point: sim::battle::V2| point.distance(beam.from) < 380.0;
        let tracers = battle.shots.iter().filter(|shot| near(shot.pos)).count();
        let beams = battle.beams.iter().filter(|other| near(other.from)).count();
        let company = battle
            .ships
            .iter()
            .filter(|other| other.status.in_play() && near(other.pos))
            .count();
        let mut score = tracers.min(12) as f32 + beams.min(8) as f32 + company.min(8) as f32;
        if beam.kind == BeamKind::Heavy {
            score += 6.0;
        }
        if beam.hit {
            score += 2.0;
        }
        if best.is_none_or(|(most, ..)| score > most) {
            best = Some((score, to_world(beam.from), to_world(beam.to), shooter as u32));
        }
    }
    let threshold = if relaxed { 0.0 } else { 9.0 };
    best.filter(|(score, ..)| *score >= threshold)
        .map(|(_, from, to, shooter)| (from, to, shooter))
}

/// Das Wrack, um das herum am meisten liegt; bevorzugt ein auseinandergebrochenes
/// Großkampfschiff, dessen Hälften schon ein Stück getrieben sind.
fn wreck_cluster(battle: &Battle) -> Option<(Vec3, f32)> {
    battle
        .wrecks
        .iter()
        .map(|wreck| {
            let neighbours = battle
                .wrecks
                .iter()
                .filter(|other| other.pos.distance(wreck.pos) < 320.0)
                .count();
            let mut score = neighbours as f32;
            if wreck.class.is_capital() {
                score += 6.0;
                if wreck.age > 3.0 {
                    score += 4.0;
                }
            }
            (score, to_world(wreck.pos), wreck.radius)
        })
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, position, radius)| (position, radius))
}

fn drive_practice(
    mut commands: Commands,
    args: Res<Args>,
    mut test: ResMut<PracticeTest>,
    feed: Res<BattleFeed>,
    stats: Res<EffectStats>,
    mut timings: ResMut<ViewTimings>,
    mut speed: ResMut<SimSpeed>,
    mut options: ResMut<ViewOptions>,
    mut selected: ResMut<SelectedShip>,
    mut camera: MessageWriter<CameraRequest>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(battle) = feed.battle() else {
        return;
    };
    test.frame += 1;
    let frame = test.frame;
    let in_play = battle
        .ships
        .iter()
        .filter(|ship| ship.status.in_play())
        .count();
    test.peak_in_play = test.peak_in_play.max(in_play);

    if frame == 5 && !test.lineup {
        // Zeit laufen lassen, schnellste Stufe; ab hier wird die Rechenzeit gemessen.
        speed.paused = false;
        speed.level = SPEEDS.len() - 1;
        timings.reset();
    }

    // Erst warten, dann fotografieren, dann weiter.
    if let Some(mut pending) = test.pending {
        pending.frames_left = pending.frames_left.saturating_sub(1);
        if pending.frames_left == SETTLE_FRAMES && pending.photo > 0 {
            if let Some(path) = args.screenshot.as_deref() {
                commands
                    .spawn(Screenshot::primary_window())
                    .observe(save_to_disk(numbered(path, pending.photo)));
            }
            test.photos += 1;
        }
        if pending.frames_left == 0 {
            test.phase = pending.next;
            test.pending = None;
            if let Some(level) = pending.resume {
                speed.level = level;
                speed.paused = false;
            }
        } else {
            test.pending = Some(pending);
        }
        return;
    }

    let mut hold = |test: &mut PracticeTest, photo: u32, next: Phase, resume: Option<usize>| {
        test.pending = Some(Pending {
            frames_left: 2 * SETTLE_FRAMES,
            photo,
            next,
            resume,
        });
    };
    let since_contact = test.contact.map(|contact| battle.time - contact);
    let out_of_time = frame + 40 >= test.limit;

    match test.phase {
        Phase::Approach => {
            if !battle.shots.is_empty() || !battle.beams.is_empty() || battle.finished {
                test.contact = Some(battle.time);
                speed.level = FIGHT_SPEED;
                test.phase = Phase::Overview;
            }
        }
        Phase::Overview => {
            // Einige Sekunden nach dem ersten Schuss, wenn Geschosse unterwegs sind.
            let ripe = since_contact.is_some_and(|time| time >= 8.0) && battle.shots.len() >= 6;
            if ripe || since_contact.is_some_and(|time| time >= 25.0) || battle.finished || out_of_time {
                speed.paused = true;
                camera.write(CameraRequest::Overview);
                hold(&mut test, 1, Phase::CloseUp, Some(FIGHT_SPEED));
            }
        }
        Phase::CloseUp => {
            let relaxed = since_contact.is_some_and(|time| time >= 30.0) || out_of_time;
            let found = busy_exchange(battle, &feed, relaxed);
            let give_up = battle.finished || since_contact.is_some_and(|time| time >= 70.0) || out_of_time;
            if let Some((from, to, shooter)) = found {
                speed.paused = true;
                let radius = battle.ships[shooter as usize].radius;
                let along = (to - from).normalize_or_zero();
                let across = along.cross(Vec3::Y);
                // Schräg hinter dem Schützen, Blick am Strahl entlang zum Ziel.
                let eye = from - along * (radius * 3.4 + 20.0) + across * (radius * 2.0 + 14.0)
                    + Vec3::Y * (radius * 1.5 + 12.0);
                let target = from + along * (from.distance(to) * 0.4).min(200.0);
                camera.write(CameraRequest::Place { eye, target });
                selected.0 = Some(shooter);
                hold(&mut test, 2, Phase::Aftermath, Some(FIGHT_SPEED));
            } else if give_up {
                speed.paused = true;
                camera.write(CameraRequest::FocusFight);
                hold(&mut test, 2, Phase::Aftermath, Some(FIGHT_SPEED));
            }
        }
        Phase::Aftermath => {
            let initial: u32 = battle
                .combatants
                .iter()
                .map(|combatant| combatant.initial.iter().sum::<u32>())
                .sum();
            let enough = battle.wrecks.len() as u32 >= (initial / 5).max(4);
            let late = since_contact.is_some_and(|time| time >= 40.0);
            if (enough && late) || battle.finished || out_of_time {
                speed.paused = true;
                selected.0 = None;
                match wreck_cluster(battle) {
                    Some((position, radius)) => {
                        let reach = (radius * 5.5).clamp(110.0, 380.0);
                        camera.write(CameraRequest::Place {
                            eye: position + Vec3::new(-0.55, 0.6, 1.0).normalize() * reach,
                            target: position,
                        });
                    }
                    None => {
                        camera.write(CameraRequest::FocusFight);
                    }
                }
                hold(&mut test, 3, Phase::Finish, Some(SPEEDS.len() - 1));
            }
        }
        Phase::Finish => {
            if battle.finished || frame >= test.limit {
                // Noch ein paar Frames mit der Bilanz, dann Schluss.
                camera.write(CameraRequest::Overview);
                hold(&mut test, 0, Phase::Done, None);
            }
        }
        Phase::Lineup(step) => {
            options.labels = true;
            options.overlay = false;
            // Vordere Reihe: Schiffe der ersten Partei, vom Jäger bis zum Träger.
            let row: Vec<(Vec3, f32)> = {
                let mut row: Vec<_> = battle
                    .ships
                    .iter()
                    .filter(|ship| ship.side == 0)
                    .map(|ship| (to_world(ship.pos), ship.radius))
                    .collect();
                row.sort_by(|a, b| a.0.x.total_cmp(&b.0.x));
                row
            };
            let span = |members: &[(Vec3, f32)]| {
                let left = members.first().map_or(0.0, |(at, radius)| at.x - radius);
                let right = members.last().map_or(0.0, |(at, radius)| at.x + radius);
                ((left + right) * 0.5, right - left)
            };
            let (members, back): (&[(Vec3, f32)], f32) = match step {
                1 => (&row[..], LINEUP_ROW_GAP * 0.5),
                2 => (&row[..row.len().min(3)], 0.0),
                _ => (&row[row.len().saturating_sub(3)..], 0.0),
            };
            let (center, width) = span(members);
            let target = Vec3::new(center, 0.0, -back);
            let distance = width * 0.72 + 16.0;
            camera.write(CameraRequest::Place {
                eye: target + Vec3::new(0.12, 0.62, 1.0).normalize() * distance,
                target,
            });
            let next = if step >= 3 {
                Phase::Done
            } else {
                Phase::Lineup(step + 1)
            };
            hold(&mut test, step, next, None);
        }
        Phase::Done => {
            let wanted = 3;
            let complete = test.photos >= wanted;
            let ended = if test.lineup {
                "Schaustellung".to_string()
            } else if battle.finished {
                "beendet".to_string()
            } else {
                "läuft noch".to_string()
            };
            let parts: Vec<String> = PART_LABELS
                .iter()
                .enumerate()
                .map(|(part, label)| format!("{label} {:.0}", timings.part_micros(part)))
                .collect();
            let verdict = if complete { "OK" } else { "FEHLER" };
            println!(
                "SELBSTTEST {verdict}: Gefechtsübung, {frame} Frames, Gefechtszeit {:.1} s ({ended}), \
                 {} Schiffe ({} zugleich im Feld), {} Wracks, höchstens {} Geschosse und {} Strahlen gezeigt, \
                 {} Effektplätze, {} Fotos, Ansicht {:.0} µs im Mittel und {:.0} µs höchstens je Frame \
                 ({}; {} Frames gemessen)",
                battle.time,
                battle.ships.len(),
                test.peak_in_play,
                battle.wrecks.len(),
                stats.peak_shots,
                stats.peak_beams,
                stats.entities,
                test.photos,
                timings.average_micros(),
                timings.worst_micros(),
                parts.join(", "),
                timings.frames,
            );
            exit.write(if complete {
                AppExit::Success
            } else {
                AppExit::error()
            });
        }
    }
}

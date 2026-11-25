//! Selbsttest: startet eine Partie oder eine Gefechtsübung, lässt sie einige Frames
//! laufen, speichert auf Wunsch Bildschirmfotos und beendet sich. Damit lässt sich prüfen,
//! dass Fenster, Karte, Schlachtansicht, Oberfläche und Simulation gemeinsam hochkommen,
//! auch ohne Menschen davor.

use crate::args::{Args, numbered};
use crate::battle_view::feed::BattleFeed;
use crate::battle_view::practice::{LINEUP_ROW_GAP, PracticeBattle};
use crate::battle_view::{
    CameraRequest, EffectStats, PART_LABELS, SelectedShip, ViewOptions, ViewTimings, to_world,
};
use crate::map::{FocusSystem, Selection};
use crate::session::SessionRequest;
use crate::state::AppState;
use crate::time_control::{SPEEDS, SimSpeed};
use crate::ui::format::decimal;
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
/// So viele Frames braucht die Engine nach dem Start, bis alle Zeichenprogramme der
/// Grafikkarte übersetzt sind; vorher bleibt das Bild leer.
const WARMUP_FRAMES: u32 = 45;
/// Fotos der Schaustellung: alle Klassen und drei Paare aus der Nähe.
const LINEUP_PHOTOS: u32 = 4;
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
    /// Schaustellung: alle Klassen, dann je zwei aus der Nähe.
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
    /// Gefechtszeit, zu der die laufende Phase begann.
    phase_since: f32,
    photos: u32,
    peak_in_play: usize,
}

impl PracticeTest {
    fn new(limit: u32, lineup: bool) -> Self {
        Self {
            limit: limit.max(120),
            lineup,
            frame: 0,
            phase: if lineup {
                Phase::Lineup(1)
            } else {
                Phase::Approach
            },
            pending: None,
            contact: None,
            phase_since: 0.0,
            photos: 0,
            peak_in_play: 0,
        }
    }

    /// Hält die Zeit an, wartet, bis die Kamera steht, macht das Foto `photo` (0 = keines)
    /// und geht dann in die Phase `next`. Die Zeit hält sofort an, nicht erst mit dem
    /// nächsten Frame: Auf dem Bild soll genau der Augenblick sein, der ausgesucht wurde.
    fn hold(
        &mut self,
        speed: &mut SimSpeed,
        clock: &mut Time<Virtual>,
        photo: u32,
        next: Phase,
        resume: Option<usize>,
    ) {
        speed.paused = true;
        clock.pause();
        self.pending = Some(Pending {
            frames_left: 2 * SETTLE_FRAMES,
            photo,
            next,
            resume,
        });
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
            best = Some((
                score,
                to_world(beam.from),
                to_world(beam.to),
                shooter as u32,
            ));
        }
    }
    let threshold = if relaxed { 0.0 } else { 9.0 };
    best.filter(|(score, ..)| *score >= threshold)
        .map(|(_, from, to, shooter)| (from, to, shooter))
}

/// Ein Wrack mit seiner Umgebung, wie das Foto des Trümmerfelds sie braucht.
#[derive(Clone, Copy, Debug)]
struct WreckSite {
    position: Vec3,
    /// Abstand, aus dem das Wrack und seine nächsten Nachbarn ins Bild passen.
    reach: f32,
    /// Sekunden seit der Zerstörung.
    age: f32,
}

/// Das Wrack, um das herum am meisten liegt; bevorzugt ein auseinandergebrochenes
/// Großkampfschiff, dessen Hälften schon ein Stück getrieben sind.
fn wreck_cluster(battle: &Battle) -> Option<WreckSite> {
    battle
        .wrecks
        .iter()
        .map(|wreck| {
            let mut distances: Vec<f32> = battle
                .wrecks
                .iter()
                .map(|other| other.pos.distance(wreck.pos))
                .filter(|distance| *distance > 0.0 && *distance < 320.0)
                .collect();
            distances.sort_by(f32::total_cmp);
            let mut score = distances.len().min(15) as f32;
            if wreck.class.is_capital() {
                score += 40.0;
                if wreck.age > 3.0 {
                    score += 20.0;
                }
            }
            // So weit zurück, dass auch die drei nächsten Wracks zu sehen sind, aber nicht so
            // weit, dass das Wrack selbst zum Punkt wird.
            let company = distances
                .get(2)
                .or(distances.last())
                .copied()
                .unwrap_or(0.0);
            let site = WreckSite {
                position: to_world(wreck.pos),
                reach: (wreck.radius * 6.0).max(company).clamp(45.0, 260.0),
                age: wreck.age,
            };
            (score, site)
        })
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, site)| site)
}

fn drive_practice(
    mut commands: Commands,
    args: Res<Args>,
    mut test: ResMut<PracticeTest>,
    feed: Res<BattleFeed>,
    practice: Option<Res<PracticeBattle>>,
    stats: Res<EffectStats>,
    mut timings: ResMut<ViewTimings>,
    mut speed: ResMut<SimSpeed>,
    mut clock: ResMut<Time<Virtual>>,
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

    if frame == WARMUP_FRAMES && !test.lineup {
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
            test.phase_since = battle.time;
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

    let since_contact = test.contact.map(|contact| battle.time - contact);
    let out_of_time = frame + 40 >= test.limit;

    match test.phase {
        Phase::Approach => {
            let contact = !battle.shots.is_empty() || !battle.beams.is_empty();
            if contact || battle.finished || out_of_time {
                test.contact = Some(battle.time);
                speed.level = FIGHT_SPEED;
                test.phase = Phase::Overview;
            }
        }
        Phase::Overview => {
            // Einige Sekunden nach dem ersten Schuss, wenn nicht nur die Vorhut feuert: Die
            // Zahl der Geschosse im Flug muss zur Größe der Flotten passen.
            let ripe = since_contact.is_some_and(|time| time >= 8.0)
                && battle.shots.len() >= (in_play / 12).max(6);
            if ripe
                || since_contact.is_some_and(|time| time >= 25.0)
                || battle.finished
                || out_of_time
            {
                camera.write(CameraRequest::Overview);
                test.hold(&mut speed, &mut clock, 1, Phase::CloseUp, Some(FIGHT_SPEED));
            }
        }
        Phase::CloseUp => {
            // Erst auf einen dichten Schusswechsel um ein Großkampfschiff warten; kommt
            // einige Sekunden lang keiner, genügt jeder frische Strahl.
            let relaxed = battle.time - test.phase_since >= 6.0 || out_of_time;
            let found = busy_exchange(battle, &feed, relaxed);
            let give_up = battle.finished || out_of_time;
            if let Some((from, to, shooter)) = found {
                let radius = battle.ships[shooter as usize].radius;
                let along = (to - from).normalize_or_zero();
                let across = along.cross(Vec3::Y);
                // Schräg hinter dem Schützen, Blick am Strahl entlang zum Ziel.
                let eye = from - along * (radius * 3.4 + 20.0)
                    + across * (radius * 2.0 + 14.0)
                    + Vec3::Y * (radius * 1.5 + 12.0);
                let target = from + along * (from.distance(to) * 0.4).min(200.0);
                camera.write(CameraRequest::Place { eye, target });
                selected.0 = Some(shooter);
                test.hold(
                    &mut speed,
                    &mut clock,
                    2,
                    Phase::Aftermath,
                    Some(FIGHT_SPEED),
                );
            } else if give_up {
                camera.write(CameraRequest::FocusFight);
                test.hold(
                    &mut speed,
                    &mut clock,
                    2,
                    Phase::Aftermath,
                    Some(FIGHT_SPEED),
                );
            }
        }
        Phase::Aftermath => {
            let initial: u32 = battle
                .combatants
                .iter()
                .map(|combatant| combatant.initial.iter().sum::<u32>())
                .sum();
            let enough = battle.wrecks.len() as u32 >= (initial / 5).max(4);
            let late = since_contact.is_some_and(|time| time >= 20.0);
            let cluster = wreck_cluster(battle);
            // Ein eben erst zerbrochenes Schiff glüht noch und liegt eng beisammen; ein paar
            // Sekunden später ist daraus ein Trümmerfeld geworden.
            let fresh =
                cluster.is_some_and(|site| site.age < 5.0) && battle.time - test.phase_since < 30.0;
            let ready = enough && late && !fresh;
            if ready || battle.finished || out_of_time {
                selected.0 = None;
                // Ist das Gefecht schon vorbei, verdeckt sonst die Bilanz das Trümmerfeld.
                options.summary = false;
                match cluster {
                    Some(site) => {
                        camera.write(CameraRequest::Place {
                            eye: site.position
                                + Vec3::new(-0.55, 0.6, 1.0).normalize() * site.reach,
                            target: site.position,
                        });
                    }
                    None => {
                        camera.write(CameraRequest::FocusFight);
                    }
                }
                test.hold(
                    &mut speed,
                    &mut clock,
                    3,
                    Phase::Finish,
                    Some(SPEEDS.len() - 1),
                );
            }
        }
        Phase::Finish => {
            if battle.finished || frame >= test.limit {
                // Zum Schluss die Übersicht mit der Bilanz; ein viertes Foto, wenn das
                // Gefecht zu Ende kam.
                camera.write(CameraRequest::Overview);
                options.summary = true;
                let photo = if battle.finished { 4 } else { 0 };
                test.hold(&mut speed, &mut clock, photo, Phase::Done, None);
            }
        }
        Phase::Lineup(_) if frame < WARMUP_FRAMES => {}
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
            // Erst beide Reihen ganz, dann die vordere Reihe paarweise aus der Nähe.
            let pair = |first: usize| &row[first.min(row.len())..(first + 2).min(row.len())];
            let (members, back, zoom): (&[(Vec3, f32)], f32, f32) = match step {
                1 => (&row[..], LINEUP_ROW_GAP * 0.45, 0.9),
                2 => (pair(0), 0.0, 1.05),
                3 => (pair(2), 0.0, 1.05),
                _ => (pair(4), 0.0, 0.95),
            };
            let (center, width) = span(members);
            let target = Vec3::new(center, 0.0, -back);
            let distance = width * zoom + 14.0;
            camera.write(CameraRequest::Place {
                eye: target + Vec3::new(0.1, 0.66, 1.0).normalize() * distance,
                target,
            });
            let next = if step >= LINEUP_PHOTOS {
                Phase::Done
            } else {
                Phase::Lineup(step + 1)
            };
            test.hold(&mut speed, &mut clock, step, next, None);
        }
        Phase::Done => {
            let wanted = if test.lineup { LINEUP_PHOTOS } else { 3 };
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
            let worst_parts: Vec<String> = PART_LABELS
                .iter()
                .enumerate()
                .map(|(part, label)| format!("{label} {:.0}", timings.worst_part_micros(part)))
                .collect();
            let per_tick = practice.as_ref().map_or(String::new(), |practice| {
                let ticks = practice.ticks.max(1) as f64;
                format!(
                    "; Gefechtsrechner {:.0} µs und Feed {:.0} µs je Takt",
                    practice.step_time.as_secs_f64() * 1e6 / ticks,
                    practice.feed_time.as_secs_f64() * 1e6 / ticks
                )
            });
            let verdict = if complete { "OK" } else { "FEHLER" };
            println!(
                "SELBSTTEST {verdict}: Gefechtsübung, {frame} Frames, Gefechtszeit {} s ({ended}), \
                 {} Schiffe ({} zugleich im Feld), {} Wracks, höchstens {} Geschosse und {} Strahlen gezeigt, \
                 {} Effektplätze, {} Fotos, Ansicht {:.0} µs im Mittel (Median {:.0} µs) und {:.0} µs höchstens je Frame \
                 (im Mittel: {}; im teuersten Frame: {}; {} Frames gemessen){per_tick}",
                decimal(battle.time as f64, 1),
                battle.ships.len(),
                test.peak_in_play,
                battle.wrecks.len(),
                stats.peak_shots,
                stats.peak_beams,
                stats.entities,
                test.photos,
                timings.average_micros(),
                timings.median_micros(),
                timings.worst_micros(),
                parts.join(", "),
                worst_parts.join(", "),
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

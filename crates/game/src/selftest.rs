//! Selbsttest: startet eine Partie oder eine Gefechtsübung, lässt sie einige Frames
//! laufen, speichert auf Wunsch Bildschirmfotos und beendet sich. Damit lässt sich prüfen,
//! dass Fenster, Karte, Schlachtansicht, Oberfläche und Simulation gemeinsam hochkommen,
//! auch ohne Menschen davor.
//!
//! „OK“ heißt mehr als „nicht abgestürzt“: Zu jedem Foto wird festgehalten, was die Engine
//! in diesem Augenblick wirklich zeichnet, also im Sichtfeld der Kamera hat und nicht
//! ausblendet (Schiffe und Wrackteile des Gefechts, Objekte der Karte), und am Ende müssen
//! die verlangten Fotodateien geschrieben sein. Gezählt wird für die Kamera, die die
//! Ansicht ins Fenster bringt: die Schlachtkamera für das Gefecht, die Kartenkamera für die
//! Karte. Eine leere Szene, eine Kamera, die am Geschehen vorbeisieht, ausgeschaltet ist
//! oder übermalt wird, und ein Foto, das nie ankam, ergeben „FEHLER“ und einen Fehlercode.
//! Ob eine Tafel der Oberfläche etwas verdeckt, prüft der Selbsttest nicht; dafür sind die
//! Fotos da.

use crate::args::{Args, numbered};
use crate::battle_view::feed::BattleFeed;
use crate::battle_view::orbit::{OrbitCensus, WatchSiege, WatchedSiege};
use crate::battle_view::practice::{LINEUP_ROW_GAP, PracticeBattle};
use crate::battle_view::{
    CameraRequest, EffectStats, PART_LABELS, SceneCensus, SelectedShip, ViewOptions, ViewTimings,
    to_world,
};
use crate::map::{FocusSystem, MapCensus, Selection};
use crate::session::SessionRequest;
use crate::siege_markers::SiegeMarkers;
use crate::state::{AppState, ViewState};
use crate::time_control::{SPEEDS, SimSpeed};
use crate::ui::format::decimal;
use crate::ui::{LeftTab, UiState};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use sim::battle::{Battle, BeamKind};
use sim::clock::SimClock;
use sim::galaxy::Galaxy;
use sim::polity::Polities;
use sim::state::SimConfig;
use std::path::{Path, PathBuf};

/// So viele Frames wartet der Selbsttest am Ende höchstens darauf, dass die Engine die
/// angeforderten Fotos geschrieben hat (sie tut das einige Frames nach der Anforderung).
const PHOTO_WAIT_FRAMES: u32 = 90;

/// Fordert ein Bildschirmfoto an. Eine alte Datei gleichen Namens verschwindet vorher:
/// Liegt die Datei am Ende da, stammt sie aus diesem Lauf.
fn take_photo(commands: &mut Commands, path: PathBuf) {
    let _ = std::fs::remove_file(&path);
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path));
}

/// Warum eine Kamera nichts ins Fenster bringt.
const CAMERA_SILENT: &str = "ausgeschaltet oder von einer anderen Kamera übermalt";

/// Liegt das Foto vor, mit Inhalt?
fn photo_written(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|file| file.len() > 0)
}

#[derive(Resource)]
struct Selftest {
    total: u32,
    frame: u32,
    shot_taken: bool,
    /// Datei des angeforderten Fotos.
    photo: Option<PathBuf>,
    /// Zahl der Kartenobjekte im Sichtfeld der Kamera, als das Foto ausgelöst wurde.
    visible: Option<usize>,
    /// Brachte die Kartenkamera in diesem Augenblick ihr Bild ins Fenster?
    camera_draws: bool,
    /// Frames, die am Ende schon auf das Foto gewartet wurde.
    waited: u32,
    /// Der Abstecher in die Orbitansicht, wenn die Partie eine sichtbare Belagerung hat.
    siege: SiegeVisit,
    /// Foto der Orbitansicht.
    siege_photo: Option<PathBuf>,
    /// Was an der Orbitansicht nicht stimmte.
    siege_problems: Vec<String>,
    /// Foto des Fensters „Belagerung“ (ab [`WINDOW_TOUR_MIN_FRAMES`] Frames).
    window_photo: Option<PathBuf>,
}

/// Ab so vielen Frames zeigt der Selbsttest die Fenster nacheinander, damit keines das
/// andere verdeckt: erst „Belagerung“ (mit eigenem Foto), dann „Staatsfinanzen“.
const WINDOW_TOUR_MIN_FRAMES: u32 = 60;

/// Frames der Fensterrunde bei `total` Frames: Fenster „Belagerung“ öffnen, es
/// fotografieren, zu „Staatsfinanzen“ und der Tafel der eigenen Hauptwelt wechseln. Alle
/// liegen nach der Halbzeit und vor dem Schlussfoto (`total − 12`).
fn window_tour(total: u32) -> Option<(u32, u32, u32)> {
    (total >= WINDOW_TOUR_MIN_FRAMES).then(|| (total * 5 / 8, total * 11 / 16, total * 3 / 4))
}

/// Stand des Abstechers in die Orbitansicht.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SiegeVisit {
    /// Noch nicht versucht.
    Pending,
    /// Erbeten im Frame `asked`.
    Watching { asked: u32, judged: bool },
    /// Vorbei, oder es gab keine sichtbare Belagerung.
    Done { seen: bool },
}

/// Ab so vielen Frames macht der Selbsttest den Abstecher in die Orbitansicht.
const SIEGE_VISIT_MIN_FRAMES: u32 = 200;
/// Frame, in dem er sie erbittet, und wie lange er danach bis zum Foto und bis zur
/// Rückkehr wartet.
const SIEGE_VISIT_START: u32 = 12;
const SIEGE_VISIT_PHOTO: u32 = 50;
const SIEGE_VISIT_LEAVE: u32 = 75;

impl Selftest {
    /// Das verlangte Foto, solange es (noch) nicht vorliegt.
    fn missing_photo(&self) -> Option<&Path> {
        [
            self.photo.as_deref(),
            self.siege_photo.as_deref(),
            self.window_photo.as_deref(),
        ]
        .into_iter()
        .flatten()
        .find(|path| !photo_written(path))
    }

    /// Was am Ende gegen ein „OK“ spricht: eine Kartenkamera, die nicht zeichnet, eine
    /// Karte, von der nichts in ihrem Sichtfeld lag, und ein Foto, das nicht geschrieben
    /// wurde.
    fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if !self.camera_draws {
            problems.push(format!("die Kartenkamera zeichnet nicht ({CAMERA_SILENT})"));
        } else if self.visible.unwrap_or(0) == 0 {
            problems.push("von der Karte liegt nichts im Sichtfeld der Kamera".to_string());
        }
        if let Some(path) = self.missing_photo() {
            problems.push(format!(
                "das Foto {} wurde nicht geschrieben",
                path.display()
            ));
        }
        problems.extend(self.siege_problems.iter().cloned());
        problems
    }
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
            photo: None,
            visible: None,
            camera_draws: false,
            waited: 0,
            siege: SiegeVisit::Pending,
            siege_photo: None,
            siege_problems: Vec::new(),
            window_photo: None,
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
    mut selection: ResMut<Selection>,
    mut ui_state: ResMut<UiState>,
    clock: Res<SimClock>,
    galaxy: Res<Galaxy>,
    (polities, config): (Res<Polities>, Res<SimConfig>),
    census: MapCensus,
    mut focus: MessageWriter<FocusSystem>,
    mut exit: MessageWriter<AppExit>,
    mut siege: SiegeVisitParams,
) {
    test.frame += 1;
    let total = test.total;
    visit_siege(&mut commands, &args, &mut test, &mut speed, &mut siege);
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
    // Die neuen Fenster nacheinander, damit keines das andere verdeckt. Ein kurzer Lauf
    // zeigt nur das letzte Bild der Runde.
    let own_world = config
        .player
        .and_then(|player| polities.get(player).capital)
        .map(|planet| Selection {
            system: Some(galaxy.planet(planet).system),
            planet: Some(planet),
        });
    let show_state = |ui_state: &mut UiState, selection: &mut Selection| {
        // „Staatsfinanzen“ mit der Verteidigung aller Welten und ihrer Haushaltszeile,
        // links die Tafel der eigenen Hauptwelt mit ihrer Verteidigung.
        ui_state.exchange_open = false;
        ui_state.siege.open = false;
        ui_state.treasury_open = true;
        ui_state.tab = LeftTab::Overview;
        if let Some(own_world) = own_world {
            *selection = own_world;
        }
    };
    match window_tour(total) {
        Some((open, photo, swap)) => {
            if test.frame == open {
                ui_state.exchange_open = false;
                ui_state.siege.open = true;
            }
            if test.frame == photo
                && let Some(path) = args.screenshot.as_deref()
            {
                let path = numbered(path, 3);
                take_photo(&mut commands, path.clone());
                test.window_photo = Some(path);
            }
            if test.frame == swap {
                show_state(&mut ui_state, &mut selection);
            }
        }
        None if test.frame == total / 2 + 1 => show_state(&mut ui_state, &mut selection),
        None => {}
    }
    if test.frame >= total.saturating_sub(12) && !test.shot_taken {
        test.shot_taken = true;
        // Was die Kartenkamera in diesem Augenblick zeichnet, gehört zum Urteil.
        test.camera_draws = census.camera_shows();
        test.visible = Some(census.visible());
        if let Some(path) = args.screenshot.clone() {
            take_photo(&mut commands, path.clone());
            test.photo = Some(path);
        }
    }
    if test.frame >= total {
        // Die Engine schreibt das Foto einige Frames nach der Anforderung.
        if test.missing_photo().is_some() && test.waited < PHOTO_WAIT_FRAMES {
            test.waited += 1;
            return;
        }
        let problems = test.problems();
        if !problems.is_empty() {
            println!("SELBSTTEST FEHLER: {}", problems.join("; "));
            exit.write(AppExit::error());
            return;
        }
        let sieges_shown = siege.markers.0.len();
        let owned = galaxy
            .planets
            .iter()
            .filter(|planet| planet.owner.is_some())
            .count();
        println!(
            "SELBSTTEST OK: {} Frames, Datum {}, Takt {}, {} von {} Planeten besiedelt, \
             {} Kartenobjekte im Sichtfeld der Kamera{}",
            test.frame,
            clock.date(),
            clock.tick,
            owned,
            galaxy.planets.len(),
            test.visible.unwrap_or(0),
            match (
                test.siege == (SiegeVisit::Done { seen: true }),
                sieges_shown
            ) {
                (true, _) => ", Orbitansicht einer Belagerung gezeigt".to_string(),
                (false, 0) => ", keine Belagerung in Sicht (Fenster „Belagerung“ leer)".to_string(),
                (false, count) => format!(", {count} Belagerungen in Sicht"),
            },
        );
        exit.write(AppExit::Success);
    }
}

/// Was der Abstecher in die Orbitansicht liest und auslöst.
#[derive(bevy::ecs::system::SystemParam)]
struct SiegeVisitParams<'w, 's> {
    markers: Res<'w, SiegeMarkers>,
    watched: Option<Res<'w, WatchedSiege>>,
    view: Res<'w, State<ViewState>>,
    next: ResMut<'w, NextState<ViewState>>,
    watch: MessageWriter<'w, WatchSiege>,
    census: OrbitCensus<'w, 's>,
}

/// Hat die Partie gleich zu Beginn eine Belagerung, die der Spieler sehen darf (ein
/// geladener Spielstand), sieht der Selbsttest sie sich in der Orbitansicht an: hinein,
/// einige Frames laufen lassen, festhalten, was die Kamera zeichnet, Foto, zurück zur
/// Karte. Der Planet muss im Bild sein und, wenn Schiffe im Orbit stehen, ein Rumpf.
fn visit_siege(
    commands: &mut Commands,
    args: &Args,
    test: &mut Selftest,
    speed: &mut SimSpeed,
    siege: &mut SiegeVisitParams,
) {
    let frame = test.frame;
    match test.siege {
        SiegeVisit::Pending if frame >= SIEGE_VISIT_START => {
            let wanted = test.total >= SIEGE_VISIT_MIN_FRAMES;
            test.siege = match siege.markers.0.first() {
                Some(marker) if wanted => {
                    siege.watch.write(WatchSiege(marker.planet));
                    // In der Ansicht läuft die Zeit in Echtzeit: So zeigt das Foto, was
                    // der Tagesbericht meldet, und nicht schon das Ende der Belagerung.
                    speed.level = 1;
                    SiegeVisit::Watching {
                        asked: frame,
                        judged: false,
                    }
                }
                _ => SiegeVisit::Done { seen: false },
            };
        }
        SiegeVisit::Watching { asked, judged } => {
            if !judged && frame >= asked + SIEGE_VISIT_PHOTO {
                test.siege = SiegeVisit::Watching {
                    asked,
                    judged: true,
                };
                if *siege.view.get() != ViewState::Siege || siege.watched.is_none() {
                    test.siege_problems
                        .push("die Orbitansicht der Belagerung öffnete sich nicht".to_string());
                } else if !siege.census.camera_shows() {
                    test.siege_problems.push(format!(
                        "die Kamera der Orbitansicht zeichnet nicht ({CAMERA_SILENT})"
                    ));
                } else {
                    if !siege.census.planet_visible() {
                        test.siege_problems.push(
                            "die Orbitansicht zeigt den Planeten nicht im Sichtfeld der Kamera"
                                .to_string(),
                        );
                    }
                    if siege.census.hulls() > 0 && siege.census.visible_hulls() == 0 {
                        test.siege_problems.push(
                            "von der Flotte des Belagerers liegt kein Rumpf im Sichtfeld der \
                             Kamera"
                                .to_string(),
                        );
                    }
                }
                if let Some(path) = args.screenshot.as_deref() {
                    let path = numbered(path, 2);
                    take_photo(commands, path.clone());
                    test.siege_photo = Some(path);
                }
            }
            if frame >= asked + SIEGE_VISIT_LEAVE {
                siege.next.set(ViewState::Galaxy);
                speed.level = SPEEDS.len() - 1;
                test.siege = SiegeVisit::Done { seen: true };
            }
        }
        _ => {}
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
    /// Was auf dem Foto zu sehen sein muss.
    subject: Subject,
    next: Phase,
    /// Stufe, mit der es danach weitergeht; `None` lässt die Zeit angehalten.
    resume: Option<usize>,
}

/// Was ein Foto zeigen muss.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Subject {
    /// Jedes Schiff auf dem Feld (Übersicht, alle Klassen der Schaustellung).
    AllShips,
    /// Wenigstens so viele Schiffe (Nahaufnahmen).
    Ships(usize),
    /// Wracks und Trümmer, wenn es welche gibt; sonst wenigstens ein Schiff.
    Wreckage,
}

/// Was in der Szene stand und im Sichtfeld der Kamera lag, als ein Foto ausgelöst wurde.
#[derive(Clone, Debug, PartialEq)]
struct Photo {
    number: u32,
    /// Datei, wenn eine verlangt war.
    path: Option<PathBuf>,
    subject: Subject,
    /// Brachte die Schlachtkamera in diesem Augenblick ihr Bild ins Fenster?
    camera_draws: bool,
    /// Schiffe auf dem Feld laut Gefecht.
    in_play: usize,
    /// Rümpfe in der Szene und davon im Sichtfeld der Kamera.
    hulls: usize,
    visible_hulls: usize,
    /// Wracks laut Gefecht und Wrackteile im Sichtfeld der Kamera.
    wrecks: usize,
    visible_wreck_parts: usize,
}

impl Photo {
    /// Was an dem Foto nicht stimmt: Die Schlachtkamera zeichnet nicht, die Szene zeigt
    /// nicht, was das Gefecht hält, oder das Verlangte liegt nicht im Sichtfeld der Kamera.
    fn problem(&self) -> Option<String> {
        let number = self.number;
        if !self.camera_draws {
            return Some(format!(
                "Foto {number}: die Schlachtkamera zeichnet nicht ({CAMERA_SILENT})"
            ));
        }
        if self.hulls != self.in_play {
            return Some(format!(
                "Foto {number}: {} Schiffe auf dem Feld, aber {} Rümpfe in der Szene",
                self.in_play, self.hulls
            ));
        }
        // Gleich, was das Foto zeigen soll: Vom Gefecht muss etwas zu sehen sein.
        if self.in_play + self.wrecks > 0 && self.visible_hulls + self.visible_wreck_parts == 0 {
            return Some(format!(
                "Foto {number}: weder Schiff noch Wrack im Sichtfeld der Kamera"
            ));
        }
        match self.subject {
            Subject::AllShips if self.visible_hulls < self.in_play => Some(format!(
                "Foto {number}: nur {} von {} Schiffen im Sichtfeld der Kamera",
                self.visible_hulls, self.in_play
            )),
            Subject::Ships(least) if self.visible_hulls < least.min(self.in_play) => Some(format!(
                "Foto {number}: {} Schiffe im Sichtfeld der Kamera, mindestens {} verlangt",
                self.visible_hulls,
                least.min(self.in_play)
            )),
            Subject::Wreckage if self.wrecks > 0 && self.visible_wreck_parts == 0 => Some(format!(
                "Foto {number}: {} Wracks, aber kein Wrackteil im Sichtfeld der Kamera",
                self.wrecks
            )),
            _ => None,
        }
    }
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
    /// Die ausgelösten Fotos und was dabei im Sichtfeld der Kamera lag.
    photos: Vec<Photo>,
    /// Frames, die am Ende schon auf die Fotodateien gewartet wurde.
    waited: u32,
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
            photos: Vec::new(),
            waited: 0,
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
        (photo, subject): (u32, Subject),
        next: Phase,
        resume: Option<usize>,
    ) {
        speed.paused = true;
        clock.pause();
        self.pending = Some(Pending {
            frames_left: 2 * SETTLE_FRAMES,
            photo,
            subject,
            next,
            resume,
        });
    }

    /// So viele Fotos gehören mindestens zu einem vollständigen Lauf (das vierte Foto des
    /// Gefechts gibt es nur, wenn es zu Ende kam).
    fn wanted(&self) -> u32 {
        if self.lineup { LINEUP_PHOTOS } else { 3 }
    }

    /// Was am Ende gegen ein „OK“ spricht: Fotos, die fehlen oder nicht zeigen, was sie
    /// sollen, und Dateien, die nicht geschrieben wurden.
    fn problems(&self) -> Vec<String> {
        let wanted = self.wanted();
        let mut problems: Vec<String> = self.photos.iter().filter_map(Photo::problem).collect();
        if (self.photos.len() as u32) < wanted {
            problems.push(format!("nur {} von {wanted} Fotos", self.photos.len()));
        }
        for path in self.missing_files() {
            problems.push(format!(
                "das Foto {} wurde nicht geschrieben",
                path.display()
            ));
        }
        problems
    }

    /// Verlangte Fotodateien, die (noch) nicht vorliegen.
    fn missing_files(&self) -> Vec<&Path> {
        self.photos
            .iter()
            .filter_map(|photo| photo.path.as_deref())
            .filter(|path| !photo_written(path))
            .collect()
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
    census: SceneCensus,
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
            let path = args
                .screenshot
                .as_deref()
                .map(|path| numbered(path, pending.photo));
            if let Some(path) = &path {
                take_photo(&mut commands, path.clone());
            }
            // Was die Kamera jetzt erfasst, gehört zum Urteil über dieses Foto.
            test.photos.push(Photo {
                number: pending.photo,
                path,
                subject: pending.subject,
                camera_draws: census.camera_shows(),
                in_play,
                hulls: census.hulls(),
                visible_hulls: census.visible_hulls(),
                wrecks: battle.wrecks.len(),
                visible_wreck_parts: census.visible_wreck_parts(),
            });
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
                test.hold(
                    &mut speed,
                    &mut clock,
                    (1, Subject::AllShips),
                    Phase::CloseUp,
                    Some(FIGHT_SPEED),
                );
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
                    (2, Subject::Ships(1)),
                    Phase::Aftermath,
                    Some(FIGHT_SPEED),
                );
            } else if give_up {
                camera.write(CameraRequest::FocusFight);
                test.hold(
                    &mut speed,
                    &mut clock,
                    (2, Subject::Ships(1)),
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
                    (3, Subject::Wreckage),
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
                test.hold(
                    &mut speed,
                    &mut clock,
                    (photo, Subject::AllShips),
                    Phase::Done,
                    None,
                );
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
            // Erst alle Klassen, dann je zwei aus der Nähe.
            let subject = if step == 1 {
                Subject::AllShips
            } else {
                Subject::Ships(2)
            };
            test.hold(&mut speed, &mut clock, (step, subject), next, None);
        }
        Phase::Done => {
            // Die Engine schreibt die Fotos einige Frames nach der Anforderung.
            if !test.missing_files().is_empty() && test.waited < PHOTO_WAIT_FRAMES {
                test.waited += 1;
                return;
            }
            let problems = test.problems();
            if !problems.is_empty() {
                println!("SELBSTTEST FEHLER: {}", problems.join("; "));
                exit.write(AppExit::error());
                return;
            }
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
            let seen: Vec<String> = test
                .photos
                .iter()
                .map(|photo| {
                    format!(
                        "{}: {} von {} Schiffen und {} Wrackteile",
                        photo.number, photo.visible_hulls, photo.in_play, photo.visible_wreck_parts
                    )
                })
                .collect();
            println!(
                "SELBSTTEST OK: Gefechtsübung, {frame} Frames, Gefechtszeit {} s ({ended}), \
                 {} Schiffe ({} zugleich im Feld), {} Wracks, höchstens {} Geschosse und {} Strahlen gezeigt, \
                 {} Effektplätze, {} Fotos (im Sichtfeld der Kamera je Foto: {}), Ansicht {:.0} µs im Mittel (Median {:.0} µs) und {:.0} µs höchstens je Frame \
                 (im Mittel: {}; im teuersten Frame: {}; {} Frames gemessen){per_tick}",
                decimal(battle.time as f64, 1),
                battle.ships.len(),
                test.peak_in_play,
                battle.wrecks.len(),
                stats.peak_shots,
                stats.peak_beams,
                stats.entities,
                test.photos.len(),
                seen.join("; "),
                timings.average_micros(),
                timings.median_micros(),
                timings.worst_micros(),
                parts.join(", "),
                worst_parts.join(", "),
                timings.frames,
            );
            exit.write(AppExit::Success);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle_view::{BattleCamera, ShipVisual};
    use crate::map::MapCamera;
    use crate::testing::TestGame;

    /// Ein Foto, an dem nichts auszusetzen ist: zehn Schiffe, alle im Bild, dazu Wracks.
    fn photo(subject: Subject) -> Photo {
        Photo {
            number: 1,
            path: None,
            subject,
            camera_draws: true,
            in_play: 10,
            hulls: 10,
            visible_hulls: 10,
            wrecks: 3,
            visible_wreck_parts: 12,
        }
    }

    fn problem(photo: Photo) -> String {
        photo.problem().unwrap_or_default()
    }

    #[test]
    fn a_photo_is_judged_by_what_is_in_view() {
        for subject in [Subject::AllShips, Subject::Ships(2), Subject::Wreckage] {
            assert_eq!(photo(subject).problem(), None, "{subject:?}");
            // Das Gefecht hält Schiffe, die Szene zeigt keine.
            let empty = Photo {
                hulls: 0,
                visible_hulls: 0,
                ..photo(subject)
            };
            assert!(
                problem(empty).contains("aber 0 Rümpfe in der Szene"),
                "{subject:?}"
            );
            // Die Szene steht, aber die Kamera sieht nichts davon.
            let blank = Photo {
                visible_hulls: 0,
                visible_wreck_parts: 0,
                ..photo(subject)
            };
            assert!(
                problem(blank).contains("weder Schiff noch Wrack im Sichtfeld der Kamera"),
                "{subject:?}"
            );
        }

        // Die Übersicht muss jedes Schiff zeigen.
        let partial = Photo {
            visible_hulls: 9,
            ..photo(Subject::AllShips)
        };
        assert!(problem(partial).contains("nur 9 von 10 Schiffen im Sichtfeld der Kamera"));

        // Eine Nahaufnahme braucht die verlangten Schiffe, aber nicht mehr, als es gibt.
        let alone = Photo {
            visible_hulls: 1,
            ..photo(Subject::Ships(2))
        };
        assert!(
            problem(alone).contains("1 Schiffe im Sichtfeld der Kamera, mindestens 2 verlangt")
        );
        let last_ship = Photo {
            in_play: 1,
            hulls: 1,
            visible_hulls: 1,
            ..photo(Subject::Ships(2))
        };
        assert_eq!(last_ship.problem(), None);

        // Das Trümmerfoto braucht Wrackteile, wenn es Wracks gibt; Schiffe im Bild ersetzen
        // sie nicht.
        let no_parts = Photo {
            visible_wreck_parts: 0,
            ..photo(Subject::Wreckage)
        };
        assert!(
            problem(no_parts).contains("3 Wracks, aber kein Wrackteil im Sichtfeld der Kamera")
        );
        let no_wrecks = Photo {
            wrecks: 0,
            visible_wreck_parts: 0,
            visible_hulls: 1,
            ..photo(Subject::Wreckage)
        };
        assert_eq!(no_wrecks.problem(), None);

        // Nach einem Gefecht ohne Überlebende zeigt die Übersicht die Wracks.
        let graveyard = Photo {
            in_play: 0,
            hulls: 0,
            visible_hulls: 0,
            ..photo(Subject::AllShips)
        };
        assert_eq!(graveyard.problem(), None);

        // Zeichnet die Schlachtkamera nicht, hilft alles andere nichts.
        for subject in [Subject::AllShips, Subject::Ships(2), Subject::Wreckage] {
            let dark = Photo {
                camera_draws: false,
                ..photo(subject)
            };
            assert!(
                problem(dark).contains("die Schlachtkamera zeichnet nicht"),
                "{subject:?}"
            );
        }
    }

    /// Ein Ort für ein Foto dieses Tests.
    fn photo_file(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("konfluenz_test_{}_{name}.png", std::process::id()))
    }

    #[test]
    fn missing_photos_and_files_count_against_the_run() {
        let mut test = PracticeTest::new(400, false);
        assert_eq!(test.problems(), ["nur 0 von 3 Fotos"]);
        for number in 1..=3 {
            test.photos.push(Photo {
                number,
                ..photo(Subject::AllShips)
            });
        }
        assert_eq!(test.problems(), Vec::<String>::new());

        // Ein verlangtes Foto zählt erst, wenn die Datei mit Inhalt vorliegt.
        let path = photo_file("urteil");
        let _ = std::fs::remove_file(&path);
        test.photos[1].path = Some(path.clone());
        assert_eq!(test.missing_files(), [path.as_path()]);
        assert_eq!(
            test.problems(),
            [format!(
                "das Foto {} wurde nicht geschrieben",
                path.display()
            )]
        );
        std::fs::write(&path, b"").unwrap();
        assert_eq!(test.problems().len(), 1, "eine leere Datei ist kein Foto");
        std::fs::write(&path, b"bild").unwrap();
        assert_eq!(test.problems(), Vec::<String>::new());
        std::fs::remove_file(&path).unwrap();

        // Die Schaustellung braucht alle vier Fotos.
        let mut lineup = PracticeTest::new(200, true);
        lineup.photos = test.photos.clone();
        lineup.photos[1].path = None;
        assert_eq!(lineup.problems(), ["nur 3 von 4 Fotos"]);
    }

    /// Lässt den Selbsttest der Gefechtsübung ohne Fenster bis zum Urteil laufen; `disturb`
    /// darf die Szene vorher stören.
    fn practice_run(
        command_line: &str,
        disturb: impl FnOnce(&mut App),
    ) -> (AppExit, Vec<String>, Vec<Photo>) {
        let mut game = TestGame::start(command_line);
        disturb(&mut game.app);
        let exit = game.run_to_exit(1500);
        let test = game.resource::<PracticeTest>();
        (exit, test.problems(), test.photos.clone())
    }

    const BATTLE: &str = "--practice --selftest 400 --seed 3";
    const LINEUP: &str = "--practice --lineup --selftest 200";

    #[test]
    fn the_practice_selftest_passes_with_the_battle_in_view() {
        let (exit, problems, photos) = practice_run(BATTLE, |_| {});
        assert_eq!(exit, AppExit::Success, "{problems:?}");
        assert!(photos.len() >= 3, "{photos:?}");
        // Die Übersicht hat jedes Schiff im Bild, und es sind welche da.
        let overview = &photos[0];
        assert_eq!(overview.subject, Subject::AllShips);
        assert!(overview.in_play > 0);
        assert_eq!(overview.hulls, overview.in_play);
        assert_eq!(overview.visible_hulls, overview.in_play);
        // Die Nahaufnahme zeigt Schiffe, das Trümmerfoto Wrackteile.
        assert!(photos[1].visible_hulls >= 1, "{:?}", photos[1]);
        assert!(photos[2].wrecks > 0, "{:?}", photos[2]);
        assert!(photos[2].visible_wreck_parts > 0, "{:?}", photos[2]);
    }

    #[test]
    fn the_practice_selftest_passes_for_every_preset_fleet() {
        // Vom Geplänkel bis zum Großgefecht mit Bordjägern: Die Übersicht erfasst jedes
        // Schiff, und der Selbsttest hat nichts auszusetzen.
        for preset in &crate::battle_view::practice::PRESETS {
            let [a, b] = preset.fleets;
            let (exit, problems, photos) =
                practice_run(&format!("{BATTLE} --fleet-a {a} --fleet-b {b}"), |_| {});
            assert_eq!(exit, AppExit::Success, "{}: {problems:?}", preset.name);
            assert!(photos[0].in_play > 0, "{}", preset.name);
            assert_eq!(
                photos[0].visible_hulls, photos[0].in_play,
                "{}",
                preset.name
            );
        }
    }

    #[test]
    fn the_lineup_selftest_passes_with_every_class_in_view() {
        let (exit, problems, photos) = practice_run(LINEUP, |_| {});
        assert_eq!(exit, AppExit::Success, "{problems:?}");
        assert_eq!(photos.len(), LINEUP_PHOTOS as usize);
        assert!(photos[0].in_play > 0);
        assert_eq!(photos[0].visible_hulls, photos[0].in_play);
        for photo in &photos[1..] {
            assert!(photo.visible_hulls >= 2, "{photo:?}");
        }
    }

    fn hide_hulls(mut hulls: Query<&mut Visibility, With<ShipVisual>>) {
        for mut visibility in &mut hulls {
            *visibility = Visibility::Hidden;
        }
    }

    fn hide_one_hull(mut hulls: Query<(&ShipVisual, &mut Visibility)>) {
        for (hull, mut visibility) in &mut hulls {
            if hull.index == 0 {
                *visibility = Visibility::Hidden;
            }
        }
    }

    fn remove_hulls(mut commands: Commands, hulls: Query<Entity, With<ShipVisual>>) {
        for hull in &hulls {
            commands.entity(hull).despawn();
        }
    }

    /// Stellt die Schlachtkamera weit über das Feld, mit dem Rücken zu ihm.
    fn look_away(mut camera: Single<&mut Transform, With<BattleCamera>>) {
        **camera = Transform::from_xyz(0.0, 90_000.0, 0.0).looking_to(Vec3::Y, Vec3::Z);
    }

    #[test]
    fn the_practice_selftest_fails_when_the_ships_are_hidden() {
        let (exit, problems, photos) = practice_run(LINEUP, |app| {
            app.add_systems(Update, hide_hulls);
        });
        assert!(exit.is_error(), "{photos:?}");
        assert_eq!(problems.len(), LINEUP_PHOTOS as usize, "{problems:?}");
        for problem in &problems {
            assert!(
                problem.contains("weder Schiff noch Wrack im Sichtfeld der Kamera"),
                "{problem}"
            );
        }
    }

    #[test]
    fn the_practice_selftest_fails_when_the_camera_looks_elsewhere() {
        let (exit, problems, photos) = practice_run(BATTLE, |app| {
            app.add_systems(PostUpdate, look_away.before(TransformSystems::Propagate));
        });
        assert!(exit.is_error(), "{photos:?}");
        assert!(photos.len() >= 3);
        // Jedes Foto zeigt den leeren Raum, und jedes wird beanstandet.
        assert_eq!(problems.len(), photos.len(), "{problems:?}");
        for problem in &problems {
            assert!(
                problem.contains("weder Schiff noch Wrack im Sichtfeld der Kamera"),
                "{problem}"
            );
        }
    }

    #[test]
    fn the_practice_selftest_fails_when_one_ship_is_missing_from_the_overview() {
        let (exit, problems, photos) = practice_run(LINEUP, |app| {
            app.add_systems(Update, hide_one_hull);
        });
        assert!(exit.is_error(), "{photos:?}");
        let all = photos[0].in_play;
        assert!(
            problems.contains(&format!(
                "Foto 1: nur {} von {all} Schiffen im Sichtfeld der Kamera",
                all - 1
            )),
            "{problems:?}"
        );
    }

    #[test]
    fn the_practice_selftest_fails_when_the_scene_holds_no_ships() {
        let (exit, problems, photos) = practice_run(LINEUP, |app| {
            app.add_systems(Update, remove_hulls);
        });
        assert!(exit.is_error(), "{photos:?}");
        let all = photos[0].in_play;
        assert!(all > 0);
        assert!(
            problems.contains(&format!(
                "Foto 1: {all} Schiffe auf dem Feld, aber 0 Rümpfe in der Szene"
            )),
            "{problems:?}"
        );
    }

    /// Welche der beiden Kameras der Szenen eingeschaltet sein soll: die der Schlacht, die
    /// der Karte.
    #[derive(Resource, Clone, Copy)]
    struct CamerasOn(bool, bool);

    /// Hält die Kameras in dem Zustand, den `CamerasOn` verlangt, was auch immer die
    /// Ansichten selbst schalten.
    fn force_cameras(
        wanted: Res<CamerasOn>,
        mut battle: Single<&mut Camera, (With<BattleCamera>, Without<MapCamera>)>,
        mut map: Single<&mut Camera, (With<MapCamera>, Without<BattleCamera>)>,
    ) {
        battle.is_active = wanted.0;
        map.is_active = wanted.1;
    }

    fn with_cameras(battle: bool, map: bool) -> impl FnOnce(&mut App) {
        move |app| {
            app.insert_resource(CamerasOn(battle, map)).add_systems(
                PostUpdate,
                force_cameras.before(bevy::camera::visibility::VisibilitySystems::CheckVisibility),
            );
        }
    }

    #[test]
    fn the_practice_selftest_fails_when_the_battle_camera_is_off() {
        // Die Kamera der Karte ist stattdessen an. Sie erfasst die Rümpfe (sie liegen in der
        // Ebene vor ihr), zeichnet aber keine: Auf dem Foto wäre nichts vom Gefecht.
        for command in [LINEUP, BATTLE] {
            let (exit, problems, photos) = practice_run(command, with_cameras(false, true));
            assert!(exit.is_error(), "{command}: {photos:?}");
            assert_eq!(problems.len(), photos.len(), "{command}: {problems:?}");
            for problem in &problems {
                assert!(
                    problem.contains("die Schlachtkamera zeichnet nicht"),
                    "{command}: {problem}"
                );
            }
        }
        // Ganz ohne eingeschaltete Kamera ebenso.
        let (exit, problems, _) = practice_run(LINEUP, with_cameras(false, false));
        assert!(exit.is_error(), "{problems:?}");
    }

    /// Das eine Problem eines Laufs, an dem die Kartenkamera nichts ins Bild brachte.
    fn assert_map_camera_blamed(exit: AppExit, problems: &[String], what: &str) {
        assert!(exit.is_error(), "{what}");
        assert_eq!(problems.len(), 1, "{what}: {problems:?}");
        assert!(
            problems[0].contains("die Kartenkamera zeichnet nicht"),
            "{what}: {problems:?}"
        );
    }

    #[test]
    fn the_campaign_selftest_fails_when_the_map_camera_is_off() {
        // Die Schlachtkamera ist stattdessen an. Die Karte liegt in ihrem Sichtfeld, aber
        // sie zeichnet keine Karte.
        let (exit, problems, _) = campaign_run(CAMPAIGN, with_cameras(true, false));
        assert_map_camera_blamed(exit, &problems, "Schlachtkamera statt Kartenkamera");
        let (exit, problems, _) = campaign_run(CAMPAIGN, with_cameras(false, false));
        assert_map_camera_blamed(exit, &problems, "keine Kamera");
    }

    #[test]
    fn the_campaign_selftest_fails_when_the_battle_camera_paints_over_the_map() {
        // Beide Kameras an: Die Schlachtkamera zeichnet nach der Kartenkamera und füllt das
        // Bild mit ihrem Hintergrund. Die Karte liegt im Sichtfeld ihrer Kamera, zu sehen
        // ist sie nicht.
        let (exit, problems, _) = campaign_run(CAMPAIGN, with_cameras(true, true));
        assert_map_camera_blamed(exit, &problems, "beide Kameras");
    }

    #[test]
    fn a_practice_photo_that_was_never_written_fails_the_selftest() {
        // Ohne Grafikkarte entsteht kein Foto. Eine alte Datei gleichen Namens darf nicht
        // als Foto dieses Laufs durchgehen.
        let path = photo_file("schau");
        let stale = numbered(&path, 1);
        std::fs::write(&stale, b"alt").unwrap();
        let (exit, problems, photos) =
            practice_run(&format!("{LINEUP} --screenshot {}", path.display()), |_| {});
        assert!(exit.is_error(), "{photos:?}");
        assert_eq!(problems.len(), LINEUP_PHOTOS as usize, "{problems:?}");
        assert!(
            problems.contains(&format!(
                "das Foto {} wurde nicht geschrieben",
                stale.display()
            )),
            "{problems:?}"
        );
        assert!(!stale.exists());
    }

    fn campaign_run(
        command_line: &str,
        disturb: impl FnOnce(&mut App),
    ) -> (AppExit, Vec<String>, Option<usize>) {
        let mut game = TestGame::start(command_line);
        disturb(&mut game.app);
        let exit = game.run_to_exit(600);
        let test = game.resource::<Selftest>();
        (exit, test.problems(), test.visible)
    }

    const CAMPAIGN: &str = "--selftest 60 --seed 3";

    #[test]
    fn the_campaign_selftest_passes_with_the_map_in_view() {
        let (exit, problems, visible) = campaign_run(CAMPAIGN, |_| {});
        assert_eq!(exit, AppExit::Success, "{problems:?}");
        assert!(visible.is_some_and(|visible| visible > 0), "{visible:?}");
    }

    #[test]
    fn the_campaign_selftest_opens_the_siege_window() {
        let mut game = TestGame::start(CAMPAIGN);
        // Die Fenster kommen nacheinander, damit keines das andere verdeckt: erst
        // „Belagerung“ allein …
        let (open, photo, swap) = window_tour(60).expect("Fensterrunde ab 60 Frames");
        assert!(30 < open && open < photo && photo < swap && swap < 60 - 12);
        for _ in 0..600 {
            if game.resource::<Selftest>().frame >= photo {
                break;
            }
            game.frames(1);
        }
        assert_eq!(game.resource::<Selftest>().frame, photo);
        let ui = game.resource::<UiState>();
        assert!(ui.siege.open && !ui.exchange_open && !ui.treasury_open);
        assert!(game.window_rect(crate::ui::siege::TITLE).is_some());
        // … dann „Staatsfinanzen“ mit der Verteidigung, links die Tafel der eigenen
        // Hauptwelt mit ihrer Verteidigung. So steht es auf dem Schlussfoto.
        let exit = game.run_to_exit(600);
        assert_eq!(exit, AppExit::Success);
        let ui = game.resource::<UiState>();
        assert!(ui.treasury_open && !ui.siege.open && !ui.exchange_open);
        assert_eq!(ui.tab, LeftTab::Overview);
        let capital = {
            let player = game.resource::<SimConfig>().player.expect("Spieler");
            game.resource::<Polities>().get(player).capital
        };
        assert!(capital.is_some());
        assert_eq!(game.resource::<Selection>().planet, capital);
        assert!(game.window_rect("Staatsfinanzen").is_some());
        assert!(game.text(crate::ui::siege::defense::REGULARS).is_some());
        // Ein kurzer Lauf zeigt nur das Schlussbild.
        assert_eq!(window_tour(59), None);
        assert_eq!(window_tour(240), Some((150, 165, 180)));
    }

    /// Ein geladener Spielstand mit einer Belagerung der eigenen Gemeinschaft.
    fn siege_run(name: &str, disturb: impl FnOnce(&mut App)) -> (AppExit, Vec<String>, SiegeVisit) {
        use crate::testing::conquest;
        let path = conquest::save_file(conquest::BOMBARDMENT, Some(sim::ids::PolityId(0)), name);
        let mut game = TestGame::start(&format!("--selftest 220 --load {}", path.display()));
        disturb(&mut game.app);
        let exit = game.run_to_exit(900);
        let _ = std::fs::remove_file(&path);
        let test = game.resource::<Selftest>();
        (exit, test.problems(), test.siege)
    }

    #[test]
    fn the_campaign_selftest_visits_the_orbit_view_of_a_loaded_siege() {
        let (exit, problems, visit) = siege_run("selbsttest_belagerung", |_| {});
        assert_eq!(exit, AppExit::Success, "{problems:?}");
        assert_eq!(visit, SiegeVisit::Done { seen: true });
        // Eine frische Partie hat keine Belagerung: kein Abstecher, trotzdem „OK“.
        let mut game = TestGame::start("--selftest 220 --seed 3");
        assert_eq!(game.run_to_exit(900), AppExit::Success);
        assert_eq!(
            game.resource::<Selftest>().siege,
            SiegeVisit::Done { seen: false }
        );
    }

    #[test]
    fn the_campaign_selftest_fails_when_the_orbit_camera_looks_elsewhere() {
        let (exit, problems, _) = siege_run("selbsttest_belagerung_leer", |app| {
            app.add_systems(PostUpdate, look_away.before(TransformSystems::Propagate));
        });
        assert!(exit.is_error());
        assert_eq!(
            problems,
            [
                "die Orbitansicht zeigt den Planeten nicht im Sichtfeld der Kamera",
                "von der Flotte des Belagerers liegt kein Rumpf im Sichtfeld der Kamera"
            ]
        );
    }

    /// Schiebt die Kartenkamera weit neben die Galaxie.
    fn lose_the_map(mut camera: Single<&mut Transform, With<MapCamera>>) {
        camera.translation.x = 4.0e6;
    }

    #[test]
    fn the_campaign_selftest_fails_when_the_map_is_out_of_view() {
        let (exit, problems, visible) = campaign_run(CAMPAIGN, |app| {
            app.add_systems(PostUpdate, lose_the_map.before(TransformSystems::Propagate));
        });
        assert!(exit.is_error(), "{visible:?}");
        assert_eq!(
            problems,
            ["von der Karte liegt nichts im Sichtfeld der Kamera"]
        );
    }

    #[test]
    fn a_campaign_photo_that_was_never_written_fails_the_selftest() {
        let path = photo_file("partie");
        std::fs::write(&path, b"alt").unwrap();
        let (exit, problems, visible) = campaign_run(
            &format!("{CAMPAIGN} --screenshot {}", path.display()),
            |_| {},
        );
        assert!(exit.is_error());
        assert!(visible.is_some_and(|visible| visible > 0));
        assert_eq!(
            problems,
            [format!(
                "das Foto {} wurde nicht geschrieben",
                path.display()
            )]
        );
        assert!(!path.exists());
    }
}

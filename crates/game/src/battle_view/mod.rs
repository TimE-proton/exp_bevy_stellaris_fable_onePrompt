//! 3D-Schlachtansicht.
//!
//! Gerechnet wird ein Gefecht im Kern, in einer Ebene; hier wird es gezeigt. Die Ansicht
//! liest allein den [`BattleFeed`] und schickt Befehle des Spielers als [`BattleOrder`]
//! zurück. Wem das Gefecht gehört, ist ihr gleich: Die Gefechtsübung ([`practice`]) und
//! später die Partie füllen denselben Feed.
//!
//! Ebene und Welt: x bleibt x, die y-Achse der Ebene zeigt in der Welt nach −z, oben ist
//! +y. Ein Kurswinkel der Ebene ist damit eine Drehung um die y-Achse.

mod camera;
mod effects;
pub mod feed;
mod hud;
mod meshes;
mod overlay;
pub mod practice;
mod scene;

pub use camera::{CameraRequest, SelectedShip, ViewOptions};
pub use effects::EffectStats;
pub use feed::{BattleFeed, BattleOrder, BattleViewRequest};

use crate::state::{AppState, ViewState};
use bevy::prelude::*;
use bevy_egui::EguiPrimaryContextPass;
use sim::battle::V2;
use std::time::{Duration, Instant};

/// Punkt der Gefechtsebene in der Welt.
#[inline]
pub fn to_world(point: V2) -> Vec3 {
    Vec3::new(point.x, 0.0, -point.y)
}

/// Punkt der Welt, senkrecht auf die Gefechtsebene gelotet.
#[cfg(test)]
pub fn to_plane(point: Vec3) -> V2 {
    V2::new(point.x, -point.z)
}

/// Drehung eines Körpers, dessen Bug nach +x zeigt, auf den Kurs `heading`.
#[inline]
pub fn heading_rotation(heading: f32) -> Quat {
    Quat::from_rotation_y(heading)
}

/// Ist die Schlachtansicht gerade auf dem Bildschirm? Gesetzt beim Betreten der
/// Gefechtsübung und der Schlachtansicht der Partie.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct BattleViewShown(pub bool);

/// Die Ansicht arbeitet, sobald sie gezeigt wird und der Feed ein Gefecht hält.
pub fn view_active(shown: Res<BattleViewShown>, feed: Res<BattleFeed>) -> bool {
    shown.0 && feed.is_active()
}

/// Wer im festen Takt den Feed füllt, gehört in eines dieser Sets. Das Schedule der
/// Simulation duldet keine ungeordneten Zugriffe auf dieselbe Resource; die Sets legen die
/// Reihenfolge fest: nach dem Takt der Simulation, erst die Übung, dann die Partie.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BattleFeedSet {
    /// Gefechtsübung ([`practice`]).
    Practice,
    /// Gefechte der Partie.
    Campaign,
}

/// Reihenfolge der Ansicht innerhalb eines Frames.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BattleViewSet {
    /// Lage aller Schiffe zwischen den Takten.
    Poses,
    /// Eingaben, Kamerafahrten, Auswahl.
    Camera,
    /// Schiffe, Wracks und Effekte in die Szene übertragen.
    Scene,
    /// Linien und Markierungen.
    Overlay,
}

/// Was sich die Systeme eines Frames teilen: die Lage jedes Schiffs zwischen zwei Takten
/// und der Blick der Kamera.
#[derive(Resource, Default)]
pub struct ViewFrame {
    /// Anteil des laufenden Takts, 0..1.
    pub alpha: f32,
    /// Gefechtszeit dieses Frames, zwischen den Takten fortgeschrieben.
    pub time: f32,
    /// Dauer eines Takts in Sekunden Gefechtszeit.
    pub tick_seconds: f32,
    /// Ort (Welt) und Kurs jedes Schiffs, Index wie im Gefecht.
    pub poses: Vec<(Vec3, f32)>,
    pub projector: camera::Projector,
    /// Fluggeschwindigkeit der Kamera als Vielfaches der Grundgeschwindigkeit (das Mausrad
    /// ändert sie; die Kopfleiste zeigt sie an).
    pub fly_speed: f32,
}

impl ViewFrame {
    pub fn position(&self, ship: usize) -> Option<Vec3> {
        self.poses.get(ship).map(|pose| pose.0)
    }
}

fn update_poses(
    mut timings: ResMut<ViewTimings>,
    feed: Res<BattleFeed>,
    data: Res<sim::GameData>,
    fixed: Res<Time<Fixed>>,
    mut frame: ResMut<ViewFrame>,
) {
    let _span = timings.span(Part::Scene);
    let Some(battle) = feed.battle() else {
        return;
    };
    // Nach dem Ende rechnet niemand mehr weiter: Dann gilt der letzte Takt ohne Abstriche.
    let alpha = if battle.finished {
        1.0
    } else {
        fixed.overstep_fraction().clamp(0.0, 1.0)
    };
    let tick_seconds = data.balance.battle.dt;
    frame.alpha = alpha;
    frame.tick_seconds = tick_seconds;
    frame.time = battle.time - (1.0 - alpha) * tick_seconds;
    frame.poses.clear();
    frame.poses.extend((0..battle.ships.len()).map(|index| {
        let pose = feed.pose(index, alpha);
        (to_world(pose.pos), pose.heading)
    }));
}

/// Teile der Ansicht, deren Rechenzeit einzeln ausgewiesen wird.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Scene = 0,
    Effects = 1,
    Overlay = 2,
    Hud = 3,
}

pub const PART_LABELS: [&str; 4] = ["Szene", "Effekte", "Linien", "Tafeln"];

/// Rechenzeit der Ansicht je Frame. Jedes ihrer Systeme misst sich selbst; so zählt nur
/// die eigene Arbeit und nicht das Warten auf andere Systeme.
#[derive(Resource, Default, Debug, Clone)]
pub struct ViewTimings {
    current: [Duration; 4],
    /// Gemessene Frames und ihre Summe je Teil.
    pub frames: u32,
    pub total: [Duration; 4],
    /// Teuerstes Frame, alle Teile zusammen, und wie es sich auf die Teile verteilt.
    pub worst: Duration,
    pub worst_parts: [Duration; 4],
    /// Rechenzeit der letzten Frames in Mikrosekunden, für den Median: Er zeigt, was ein
    /// gewöhnliches Frame kostet, auch wenn einzelne Ausreißer den Mittelwert heben.
    samples: Vec<u32>,
}

/// So viele Frames gehen höchstens in den Median ein.
const MAX_SAMPLES: usize = 20_000;

impl ViewTimings {
    /// Misst von jetzt bis zum Ende des umgebenden Blocks.
    pub fn span(&mut self, part: Part) -> Span<'_> {
        Span {
            timings: self,
            part,
            start: Instant::now(),
        }
    }

    fn end_frame(&mut self) {
        let sum: Duration = self.current.iter().sum();
        if sum > Duration::ZERO {
            self.frames += 1;
            for (total, current) in self.total.iter_mut().zip(self.current) {
                *total += current;
            }
            if sum > self.worst {
                self.worst = sum;
                self.worst_parts = self.current;
            }
            if self.samples.len() < MAX_SAMPLES {
                self.samples
                    .push(sum.as_micros().min(u32::MAX as u128) as u32);
            }
        }
        self.current = [Duration::ZERO; 4];
    }

    /// Mittlere Rechenzeit je Frame in Mikrosekunden, alle Teile zusammen.
    pub fn average_micros(&self) -> f64 {
        let sum: Duration = self.total.iter().sum();
        sum.as_secs_f64() * 1e6 / self.frames.max(1) as f64
    }

    pub fn part_micros(&self, part: usize) -> f64 {
        self.total[part].as_secs_f64() * 1e6 / self.frames.max(1) as f64
    }

    /// Rechenzeit eines gewöhnlichen Frames in Mikrosekunden (Median).
    pub fn median_micros(&self) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let mut sorted = self.samples.clone();
        let middle = sorted.len() / 2;
        *sorted.select_nth_unstable(middle).1 as f64
    }

    pub fn worst_micros(&self) -> f64 {
        self.worst.as_secs_f64() * 1e6
    }

    /// Anteil eines Teils am teuersten Frame in Mikrosekunden.
    pub fn worst_part_micros(&self, part: usize) -> f64 {
        self.worst_parts[part].as_secs_f64() * 1e6
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

pub struct Span<'a> {
    timings: &'a mut ViewTimings,
    part: Part,
    start: Instant,
}

impl Drop for Span<'_> {
    fn drop(&mut self) {
        self.timings.current[self.part as usize] += self.start.elapsed();
    }
}

fn close_timing_frame(mut timings: ResMut<ViewTimings>) {
    timings.end_frame();
}

pub struct BattleViewPlugin;

impl Plugin for BattleViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BattleFeed>()
            .init_resource::<BattleViewShown>()
            .init_resource::<ViewFrame>()
            .init_resource::<ViewTimings>()
            .add_message::<BattleOrder>()
            .add_message::<BattleViewRequest>()
            .configure_sets(
                Update,
                (
                    BattleViewSet::Poses,
                    BattleViewSet::Camera,
                    BattleViewSet::Scene,
                    BattleViewSet::Overlay,
                )
                    .chain()
                    .run_if(view_active),
            )
            .configure_sets(
                FixedUpdate,
                (BattleFeedSet::Practice, BattleFeedSet::Campaign)
                    .chain()
                    .after(sim::SimRoot),
            )
            .add_systems(Update, update_poses.in_set(BattleViewSet::Poses))
            .add_systems(Last, close_timing_frame)
            // Dieselbe Ansicht für die Gefechtsübung und für Gefechte der Partie.
            .add_systems(OnEnter(AppState::Practice), scene::show_view)
            .add_systems(OnExit(AppState::Practice), scene::hide_view)
            .add_systems(OnEnter(ViewState::Battle), scene::show_view)
            .add_systems(OnExit(ViewState::Battle), scene::hide_view)
            .add_systems(EguiPrimaryContextPass, hud::draw_hud.run_if(view_active));
        camera::plugin(app);
        scene::plugin(app);
        effects::plugin(app);
        overlay::plugin(app);
        practice::plugin(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plane_and_world_map_onto_each_other() {
        let point = V2::new(120.0, -45.0);
        let world = to_world(point);
        assert_eq!(world, Vec3::new(120.0, 0.0, 45.0));
        assert_eq!(to_plane(world + Vec3::Y * 30.0), point);
        // Kurs 0 zeigt nach +x, Kurs 90° nach +y der Ebene, also nach −z der Welt.
        let bow = heading_rotation(std::f32::consts::FRAC_PI_2) * Vec3::X;
        assert!((bow - to_world(V2::new(0.0, 1.0))).length() < 1e-6);
        for heading in [-2.5f32, -0.4, 0.0, 1.1, 3.0] {
            let forward = heading_rotation(heading) * Vec3::X;
            let expected = to_world(V2::from_angle(heading));
            assert!((forward - expected).length() < 1e-5);
            // Backbord (links) liegt in der Ebene bei +90° zum Kurs.
            let port = heading_rotation(heading) * Vec3::NEG_Z;
            assert!((port - to_world(V2::from_angle(heading).perp())).length() < 1e-5);
        }
    }

    #[test]
    fn timings_sum_per_frame() {
        let mut timings = ViewTimings::default();
        timings.current[Part::Scene as usize] = Duration::from_micros(300);
        timings.current[Part::Hud as usize] = Duration::from_micros(200);
        timings.end_frame();
        timings.current[Part::Effects as usize] = Duration::from_micros(100);
        timings.end_frame();
        // Frames ohne Arbeit der Ansicht zählen nicht mit.
        timings.end_frame();
        assert_eq!(timings.frames, 2);
        assert!((timings.average_micros() - 300.0).abs() < 1e-6);
        assert!((timings.part_micros(Part::Hud as usize) - 100.0).abs() < 1e-6);
        // Das teuerste Frame und wie es sich verteilt.
        assert!((timings.worst_micros() - 500.0).abs() < 1e-6);
        assert!((timings.worst_part_micros(Part::Scene as usize) - 300.0).abs() < 1e-6);
        assert!((timings.worst_part_micros(Part::Effects as usize)).abs() < 1e-6);
        // Median: Ein einzelner Ausreißer verschiebt ihn nicht, wohl aber den Mittelwert.
        for micros in [120, 130, 125, 90_000] {
            timings.current[Part::Hud as usize] = Duration::from_micros(micros);
            timings.end_frame();
        }
        assert_eq!(timings.median_micros(), 130.0);
        assert!(timings.average_micros() > 10_000.0);
        timings.reset();
        assert_eq!(timings.frames, 0);
        assert_eq!(timings.median_micros(), 0.0);
    }
}

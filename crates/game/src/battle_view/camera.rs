//! Kamera der Schlachtansicht: freier Flug mit der Freiflugkamera der Engine, dazu
//! Sprünge zur Übersicht, zum Gefechtsschwerpunkt und zum gewählten Schiff, Verfolgen
//! und die Auswahl per Mausklick.

use super::feed::BattleFeed;
use super::{BattleViewSet, Part, ViewFrame, ViewTimings, to_world};
use crate::pointer::{PointerOwner, WorldMouse, track_pointer};
use bevy::camera_controller::free_camera::{FreeCamera, FreeCameraPlugin, FreeCameraState};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::input::{EguiWantsInput, egui_wants_any_keyboard_input};
use sim::battle::Battle;

/// Senkrechter Bildwinkel der Schlachtkamera.
pub const FOV: f32 = 50.0 * std::f32::consts::PI / 180.0;
const NEAR: f32 = 2.0;
/// Weiter als so kommt die Kamera im freien Flug nicht vom Feld weg (die Übersicht darf
/// weiter zurück, wenn die Schiffe es verlangen).
const MAX_DISTANCE: f32 = 14_000.0;
/// Neigung der Übersicht unter die Waagerechte.
const OVERVIEW_PITCH: f32 = 0.654;
/// Kleinster und größter Halbmesser, den die Übersicht zeigt. Was weiter als der größte
/// von der Feldmitte treibt, bleibt außen vor: Sonst schrumpfte das Gefecht zum Punkt.
const OVERVIEW_MIN: f32 = 650.0;
const OVERVIEW_MAX: f32 = 12_000.0;
/// Neigung der Blicke von vorn und von der Seite: flach, aber nicht so flach, dass die
/// Schiffe einander verdecken.
const SIDE_PITCH: f32 = 0.3;
/// So viele Pixel am oberen Bildrand belegt die Kopfleiste; Übersicht und Richtungsblicke
/// halten sie frei.
const TOP_BAR_PIXELS: f32 = 34.0;
/// Freier Rand solcher Blicke, als Anteil der halben Bildbreite und -höhe.
const FIT_MARGIN: f32 = 0.04;
/// Platz der Lagetafeln in den beiden oberen Ecken des Bilds (Pixel). Übersicht und
/// Richtungsblicke stellen kein Schiff darunter.
const PANEL_CORNER: Vec2 = Vec2::new(272.0, 340.0);
/// Näher als auf diesen Umkreis um die Mitte der Schiffe gehen die Richtungsblicke nicht
/// heran.
const SIDE_VIEW_MIN: f32 = 150.0;
/// Markierungen kleiner Schiffe lassen sich in diesem Umkreis (Pixel) anklicken.
const PICK_PIXELS: f32 = 11.0;

#[derive(Component)]
pub struct BattleCamera;

/// Das gewählte Schiff (Index im Gefecht).
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectedShip(pub Option<u32>);

/// Schalter der Ansicht.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewOptions {
    /// Taktische Einblendung: Flottenanker, Reichweiten, Ziellinien.
    pub overlay: bool,
    /// Namen an nahen Schiffen.
    pub labels: bool,
    /// Die Kamera wandert mit dem gewählten Schiff.
    pub follow: bool,
    /// Bilanz nach dem Ende des Gefechts zeigen (zum Betrachten des Felds abschaltbar).
    pub summary: bool,
}

impl Default for ViewOptions {
    fn default() -> Self {
        Self {
            overlay: true,
            labels: false,
            follow: false,
            summary: true,
        }
    }
}

/// Bitte an die Kamera.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub enum CameraRequest {
    /// So weit zurück, dass alle Schiffe auf dem Feld im Bild sind.
    Overview,
    /// Zum gewählten Schiff; ohne Auswahl zum Schwerpunkt des Gefechts.
    Focus,
    /// Zum Schwerpunkt des Gefechts.
    FocusFight,
    FocusShip(u32),
    /// Blick aus einer festen Richtung auf das gewählte Schiff, ohne Auswahl auf alle
    /// Schiffe auf dem Feld.
    View(ViewSide),
    /// Feste Stelle und Blickpunkt (Selbsttest).
    Place {
        eye: Vec3,
        target: Vec3,
    },
}

/// Richtung, aus der ein fester Blick auf das Gefecht fällt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewSide {
    /// Senkrecht von oben; Norden ist im Bild oben, die erste Partei steht links.
    Top,
    Bottom,
    /// Von Süden, wie die Übersicht, aber flach.
    Front,
    Back,
    /// Von Osten, über die zweite Partei hinweg.
    Right,
    Left,
}

impl ViewSide {
    pub fn opposite(self) -> Self {
        match self {
            ViewSide::Top => ViewSide::Bottom,
            ViewSide::Bottom => ViewSide::Top,
            ViewSide::Front => ViewSide::Back,
            ViewSide::Back => ViewSide::Front,
            ViewSide::Right => ViewSide::Left,
            ViewSide::Left => ViewSide::Right,
        }
    }

    /// Gier- und Nickwinkel der Kamera.
    fn angles(self) -> (f32, f32) {
        use std::f32::consts::{FRAC_PI_2, PI};
        match self {
            ViewSide::Top => (0.0, -FRAC_PI_2),
            ViewSide::Bottom => (0.0, FRAC_PI_2),
            ViewSide::Front => (0.0, -SIDE_PITCH),
            ViewSide::Back => (PI, -SIDE_PITCH),
            ViewSide::Right => (FRAC_PI_2, -SIDE_PITCH),
            ViewSide::Left => (-FRAC_PI_2, -SIDE_PITCH),
        }
    }
}

/// Blick der Kamera als reine Rechnung: bildet Weltpunkte auf den Bildschirm ab und
/// Bildschirmpunkte auf Strahlen. Entspricht der perspektivischen Projektion der Engine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Projector {
    pub eye: Vec3,
    rotation: Quat,
    tan_half: f32,
    /// Größe des Bilds in logischen Pixeln.
    pub size: Vec2,
}

/// Bildgröße, mit der gerechnet wird, solange das Fenster noch nicht bekannt ist.
const DEFAULT_SIZE: Vec2 = Vec2::new(1600.0, 900.0);

impl Default for Projector {
    fn default() -> Self {
        Self::new(&overview_transform(2400.0, DEFAULT_SIZE), FOV, DEFAULT_SIZE)
    }
}

impl Projector {
    pub fn new(camera: &Transform, fov: f32, size: Vec2) -> Self {
        Self {
            eye: camera.translation,
            rotation: camera.rotation,
            tan_half: (fov * 0.5).tan(),
            size: size.max(Vec2::ONE),
        }
    }

    /// Ausrichtung der Kamera.
    pub fn rotation(&self) -> Quat {
        self.rotation
    }

    /// Weltgröße eines Pixels in der Entfernung 1 (längs der Blickachse).
    pub fn pixel(&self) -> f32 {
        2.0 * self.tan_half / self.size.y
    }

    /// Bildschirmpunkt und Tiefe (Abstand längs der Blickachse) eines Weltpunkts; `None`,
    /// wenn er hinter der Kamera liegt.
    pub fn project(&self, world: Vec3) -> Option<(Vec2, f32)> {
        let local = self.rotation.inverse() * (world - self.eye);
        let depth = -local.z;
        if depth <= NEAR {
            return None;
        }
        let aspect = self.size.x / self.size.y;
        let ndc = Vec2::new(
            local.x / (depth * self.tan_half * aspect),
            local.y / (depth * self.tan_half),
        );
        Some((
            Vec2::new(
                (ndc.x + 1.0) * 0.5 * self.size.x,
                (1.0 - ndc.y) * 0.5 * self.size.y,
            ),
            depth,
        ))
    }

    /// Strahl vom Auge durch einen Bildschirmpunkt: Ursprung und Richtung (Länge 1).
    pub fn ray(&self, screen: Vec2) -> (Vec3, Vec3) {
        let aspect = self.size.x / self.size.y;
        let ndc = Vec2::new(
            screen.x / self.size.x * 2.0 - 1.0,
            1.0 - screen.y / self.size.y * 2.0,
        );
        let local = Vec3::new(ndc.x * self.tan_half * aspect, ndc.y * self.tan_half, -1.0);
        (self.eye, (self.rotation * local).normalize())
    }
}

/// Schnittpunkt eines Strahls mit der Gefechtsebene (y = 0); `None`, wenn er sie nicht
/// vor der Kamera trifft.
pub fn ray_plane(origin: Vec3, direction: Vec3) -> Option<Vec3> {
    if direction.y.abs() < 1e-6 {
        return None;
    }
    let distance = -origin.y / direction.y;
    (distance > 0.0).then(|| {
        let mut hit = origin + direction * distance;
        hit.y = 0.0;
        hit
    })
}

/// Das Schiff, auf das ein Strahl vom Mauszeiger zeigt.
///
/// Zuerst zählt der Punkt, an dem der Strahl die Gefechtsebene trifft: Gewählt ist das
/// nächste Schiff, in dessen Radius er liegt; für weit entfernte, kleine Schiffe zählt
/// stattdessen der Umkreis ihrer Markierung (`pixel` = Weltgröße eines Pixels je Einheit
/// Entfernung). Liegt dort keines (bei sehr flachem Blick ragt ein Rumpf über die Stelle
/// hinaus, an der der Strahl die Ebene erst viel weiter hinten trifft), gewinnt das
/// Schiff, an dessen Mitte der Strahl am genauesten vorbeigeht.
/// `ships` liefert Index, Ort und Radius der wählbaren Schiffe.
pub fn pick_ship(
    origin: Vec3,
    direction: Vec3,
    pixel: f32,
    ships: impl IntoIterator<Item = (u32, Vec3, f32)>,
) -> Option<u32> {
    let on_plane = ray_plane(origin, direction);
    let mut by_plane: Option<(u32, f32)> = None;
    let mut by_ray: Option<(u32, f32)> = None;
    for (index, position, radius) in ships {
        let offset = position - origin;
        let along = offset.dot(direction);
        if along <= 0.0 {
            continue;
        }
        let reach = radius.max(PICK_PIXELS * pixel * along);
        if let Some(point) = on_plane {
            let score = point.distance(position) / reach;
            if score <= 1.0 && by_plane.is_none_or(|(_, least)| score < least) {
                by_plane = Some((index, score));
            }
        }
        let miss = (offset - direction * along).length();
        let score = miss / reach;
        if score <= 1.0 && by_ray.is_none_or(|(_, least)| score < least) {
            by_ray = Some((index, score));
        }
    }
    by_plane.or(by_ray).map(|(index, _)| index)
}

/// Wie weit ein Punkt seitlich, nach oben und nach unten von der Blickachse abliegen darf,
/// je Einheit Tiefe.
#[derive(Clone, Copy, Debug)]
struct Limits {
    side: f32,
    above: f32,
    below: f32,
}

/// Blick aus der Richtung `yaw`/`pitch` auf einen Kreis in der Gefechtsebene, so nah wie
/// möglich und doch so, dass der ganze Kreis zu sehen ist: mit etwas Rand, unterhalb der
/// Kopfleiste und nicht unter den Lagetafeln in den oberen Ecken. Dafür liegt er entweder
/// in der Spalte zwischen den Tafeln oder ganz unter ihnen, je nachdem, was näher
/// heranführt. `size` ist die Größe des Bilds in Pixeln.
pub fn frame_circle(center: Vec3, radius: f32, yaw: f32, pitch: f32, size: Vec2) -> Transform {
    let rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0);
    let (forward, right, up) = (
        rotation * Vec3::NEG_Z,
        rotation * Vec3::X,
        rotation * Vec3::Y,
    );
    let size = size.max(Vec2::splat(64.0));
    let tan_y = (FOV * 0.5).tan();
    let tan_x = tan_y * size.x / size.y;
    let full = Limits {
        side: (1.0 - FIT_MARGIN) * tan_x,
        above: (1.0 - FIT_MARGIN - 2.0 * TOP_BAR_PIXELS / size.y).max(0.2) * tan_y,
        below: (1.0 - FIT_MARGIN) * tan_y,
    };
    let between_panels = Limits {
        side: (1.0 - FIT_MARGIN - 2.0 * PANEL_CORNER.x / size.x) * tan_x,
        ..full
    };
    let under_panels = Limits {
        above: (1.0 - FIT_MARGIN - 2.0 * PANEL_CORNER.y / size.y) * tan_y,
        ..full
    };
    // Der Blickpunkt wandert längs der Blickrichtung über den Boden: Die nahe Hälfte des
    // Kreises erscheint größer als die ferne, die Kreismitte gehört also nicht in die
    // Bildmitte.
    let along = Vec3::new(forward.x + up.x, 0.0, forward.z + up.z).normalize_or(Vec3::NEG_Z);
    const SAMPLES: usize = 72;
    // Der Kreis, bezogen auf seine Mitte und zerlegt nach den Achsen der Kamera.
    let rim: Vec<Vec3> = (0..SAMPLES)
        .map(|step| {
            let angle = std::f32::consts::TAU * step as f32 / SAMPLES as f32;
            let offset = Vec3::new(angle.cos(), 0.0, angle.sin()) * radius;
            Vec3::new(offset.dot(right), offset.dot(up), offset.dot(forward))
        })
        .collect();
    let slide = Vec3::new(along.dot(right), along.dot(up), along.dot(forward));
    // Für einen Blickpunkt, der um `shift` vor der Kreismitte liegt: der Abstand, den die
    // Kamera braucht, damit der ganze Kreis innerhalb der Grenzen liegt, und wie viel
    // mehr Luft dann über dem Kreis bleibt als unter ihm.
    let placed = |limits: Limits, shift: f32| {
        let seen = |point: &Vec3| *point - slide * shift;
        let distance = rim
            .iter()
            .map(seen)
            .map(|point| {
                (point.x.abs() / limits.side)
                    .max(point.y / limits.above)
                    .max(-point.y / limits.below)
                    - point.z
            })
            .fold(NEAR * 2.0, f32::max);
        let (top, bottom) =
            rim.iter()
                .map(seen)
                .fold((f32::MIN, f32::MIN), |(top, bottom), point| {
                    let depth = distance + point.z;
                    (
                        top.max(point.y / depth / limits.above),
                        bottom.max(-point.y / depth / limits.below),
                    )
                });
        (distance, bottom - top)
    };
    // Gesucht ist der Blickpunkt, bei dem der Kreis mittig zwischen oberer und unterer
    // Grenze sitzt. Stößt er oben und unten zugleich an, ist das auch der nächste Blick;
    // begrenzt die Breite, bleibt das Bild so ausgewogen. Je weiter vorn der Blickpunkt,
    // desto tiefer sitzt der Kreis im Bild: Halbieren genügt.
    let centered = |limits: Limits| {
        let (mut low, mut high) = (-4.0 * radius, 4.0 * radius);
        for _ in 0..48 {
            let middle = (low + high) * 0.5;
            if placed(limits, middle).1 > 0.0 {
                high = middle;
            } else {
                low = middle;
            }
        }
        let shift = (low + high) * 0.5;
        (shift, placed(limits, shift).0)
    };
    // In sehr kleinen Fenstern bleibt neben oder unter den Tafeln kein brauchbarer Platz;
    // ist beides so, zählen die Tafeln nicht.
    let (shift, distance) = [between_panels, under_panels]
        .into_iter()
        .filter(|limits| limits.side > 0.25 * tan_x && limits.above > 0.1 * tan_y)
        .map(centered)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap_or_else(|| centered(full));
    Transform {
        translation: center + along * shift - forward * distance,
        rotation,
        scale: Vec3::ONE,
    }
}

/// Blick schräg von oben auf einen Kreis mit Halbmesser `extent` um die Feldmitte, der
/// ganz zu sehen ist. Die erste Partei steht dabei links, die zweite rechts.
pub fn overview_transform(extent: f32, size: Vec2) -> Transform {
    frame_circle(Vec3::ZERO, extent, 0.0, -OVERVIEW_PITCH, size)
}

/// Schiffe auf dem Feld, die für die Blicke zählen (ohne die, die weit abgetrieben sind).
fn ships_in_view(battle: &Battle) -> impl Iterator<Item = &sim::battle::BattleShip> {
    battle
        .ships
        .iter()
        .filter(|ship| ship.status.in_play() && ship.pos.length() <= OVERVIEW_MAX)
}

/// Halbmesser um die Feldmitte, in dem alle Schiffe auf dem Feld mit ganzem Rumpf liegen,
/// mindestens das Mindestmaß der Übersicht. Auch jenseits des Feldrands: Fliehende sind
/// bis dorthin noch im Gefecht, und große Linien stehen vor dem ersten Takt weit draußen.
/// Nach dem Ende gehört auch das Trümmerfeld dazu, soweit es auf dem Feld (Halbmesser
/// `field`) liegt.
pub fn overview_extent(battle: &Battle, field: f32) -> f32 {
    let ships = ships_in_view(battle)
        .map(|ship| ship.pos.length() + ship.radius)
        .fold(OVERVIEW_MIN, f32::max);
    if !battle.finished {
        return ships;
    }
    battle
        .wrecks
        .iter()
        .map(|wreck| wreck.pos.length() + wreck.radius)
        .filter(|reach| *reach <= field)
        .fold(ships, f32::max)
}

/// Die Übersicht: schräg von Süden auf das Feld, so nah, dass gerade noch jedes Schiff auf
/// dem Feld zu sehen ist.
pub fn overview_view(battle: &Battle, field: f32, size: Vec2) -> Transform {
    overview_transform(overview_extent(battle, field), size)
}

/// Kleinster Kreis um die Mitte der Schiffe auf dem Feld, der sie alle enthält.
pub fn fleet_circle(battle: &Battle) -> (Vec3, f32) {
    let (mut min, mut max) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for ship in ships_in_view(battle) {
        let at = to_world(ship.pos);
        min = min.min(at - ship.radius);
        max = max.max(at + ship.radius);
    }
    if min.x > max.x {
        return (Vec3::ZERO, OVERVIEW_MIN);
    }
    let center = Vec3::new((min.x + max.x) * 0.5, 0.0, (min.z + max.z) * 0.5);
    let radius = ships_in_view(battle)
        .map(|ship| to_world(ship.pos).distance(center) + ship.radius)
        .fold(SIDE_VIEW_MIN, f32::max);
    (center, radius)
}

/// Blick aus einer festen Richtung auf alle Schiffe auf dem Feld.
pub fn side_view(battle: &Battle, side: ViewSide, size: Vec2) -> Transform {
    let (center, radius) = fleet_circle(battle);
    let (yaw, pitch) = side.angles();
    frame_circle(center, radius, yaw, pitch, size)
}

/// Kamera im Abstand `distance` vor `target`, mit Blickrichtung aus Gierwinkel und Neigung.
fn framing(target: Vec3, yaw: f32, pitch: f32, distance: f32) -> Transform {
    let rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0);
    Transform {
        translation: target - rotation * Vec3::NEG_Z * distance,
        rotation,
        scale: Vec3::ONE,
    }
}

/// Schwerpunkt und Ausdehnung des Geschehens: wo geschossen wird, sonst wo die Schiffe
/// stehen.
pub fn fight_center(battle: &Battle) -> Option<(Vec3, f32)> {
    let mut points: Vec<Vec3> = battle
        .beams
        .iter()
        .map(|beam| to_world(beam.from))
        .chain(battle.shots.iter().map(|shot| to_world(shot.pos)))
        .take(600)
        .collect();
    if points.len() < 4 {
        points = battle
            .ships
            .iter()
            .filter(|ship| ship.status.in_play())
            .map(|ship| to_world(ship.pos))
            .collect();
    }
    if points.is_empty() {
        return None;
    }
    let center = points.iter().sum::<Vec3>() / points.len() as f32;
    let spread = (points
        .iter()
        .map(|point| point.distance_squared(center))
        .sum::<f32>()
        / points.len() as f32)
        .sqrt();
    Some((center, spread))
}

pub(super) fn plugin(app: &mut App) {
    app.add_plugins(FreeCameraPlugin)
        .init_resource::<SelectedShip>()
        .init_resource::<ViewOptions>()
        .init_resource::<FollowAnchor>()
        .add_message::<CameraRequest>()
        .add_systems(Startup, spawn_camera)
        // Vor der Freiflugsteuerung der Engine, die danach im selben Frame läuft.
        .add_systems(PreUpdate, hold_mouse_look.after(track_pointer))
        .add_systems(
            Update,
            (
                tame_free_camera,
                hotkeys.run_if(not(egui_wants_any_keyboard_input)),
                apply_requests,
                follow_selected,
                pick_on_click,
                update_projector,
            )
                .chain()
                .in_set(BattleViewSet::Camera),
        );
}

fn spawn_camera(mut commands: Commands) {
    // Die Kamera lebt von Anfang an und wird nur ein- und ausgeschaltet: So bleibt die
    // Freiflugsteuerung der Engine in einem sauberen Zustand (kein gefangener Mauszeiger).
    let mut flight = FreeCameraState::default();
    flight.enabled = false;
    commands.spawn((
        BattleCamera,
        Camera3d::default(),
        Camera {
            is_active: false,
            order: 1,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.004, 0.005, 0.012)),
            ..default()
        },
        Projection::Perspective(PerspectiveProjection {
            fov: FOV,
            near: NEAR,
            far: 200_000.0,
            ..default()
        }),
        Tonemapping::TonyMcMapface,
        Bloom {
            intensity: 0.22,
            ..Bloom::NATURAL
        },
        AmbientLight {
            color: Color::srgb(0.62, 0.7, 1.0),
            brightness: 260.0,
            ..default()
        },
        flight_controls(),
        flight,
        overview_transform(2400.0, DEFAULT_SIZE),
    ));
}

/// Tasten, auf denen das Drehen auf der Stelle der Engine geparkt ist.
const ENGINE_UNUSED: [KeyCode; 4] = [KeyCode::F21, KeyCode::F22, KeyCode::F23, KeyCode::F24];

/// Belegung und Tempo der Freiflugsteuerung der Engine.
fn flight_controls() -> FreeCamera {
    FreeCamera {
        walk_speed: 380.0,
        run_speed: 1500.0,
        friction: 14.0,
        scroll_factor: 0.18,
        // Die Engine dreht die Kamera mit dem Ziffernblock nur auf der Stelle (7 lässt sie
        // in den Himmel blicken). Die Ansicht hat eigene Blicke auf das Gefecht; die
        // Tasten der Engine liegen deshalb auf Tasten, die keine Tastatur hat.
        axis_top: ENGINE_UNUSED[0],
        axis_front: ENGINE_UNUSED[1],
        axis_right: ENGINE_UNUSED[2],
        key_snap_reverse: ENGINE_UNUSED[3],
        ..default()
    }
}

/// Schaltet die Schlachtkamera samt Steuerung ein oder aus.
pub(super) fn set_active(
    active: bool,
    camera: &mut Camera,
    flight: &mut FreeCameraState,
    transform: &mut Transform,
) {
    camera.is_active = active;
    flight.enabled = active;
    flight.velocity = Vec3::ZERO;
    flight.speed_multiplier = 1.0;
    if active {
        *transform = overview_transform(2400.0, DEFAULT_SIZE);
        sync_flight(flight, transform);
    }
}

/// Die Steuerung der Engine führt Gier- und Nickwinkel selbst mit; nach einem Sprung
/// müssen sie zur neuen Blickrichtung passen.
fn sync_flight(flight: &mut FreeCameraState, transform: &Transform) {
    let (yaw, pitch, _) = transform.rotation.to_euler(EulerRot::YXZ);
    set_flight_angles(flight, yaw, pitch);
}

/// Wie [`sync_flight`] mit bekannten Winkeln. Bei senkrechtem Blick lassen sie sich aus
/// der Drehung nicht mehr eindeutig zurückrechnen.
fn set_flight_angles(flight: &mut FreeCameraState, yaw: f32, pitch: f32) {
    flight.yaw = yaw;
    flight.pitch = pitch;
    flight.rotation_curve = None;
}

/// Umsehen beginnt nur über der Szene. Wird die Maustaste dafür über einer Tafel oder
/// Leiste gedrückt, ruht die Freiflugsteuerung der Engine für dieses eine Frame: Sie
/// sieht den Tastendruck dann nie und fängt den Zeiger nicht.
fn hold_mouse_look(
    buttons: Res<ButtonInput<MouseButton>>,
    owner: Res<PointerOwner>,
    mut camera: Single<(&FreeCamera, &mut FreeCameraState), With<BattleCamera>>,
) {
    let (controls, flight) = &mut *camera;
    let look = controls.mouse_key_cursor_grab;
    if flight.enabled && buttons.just_pressed(look) && owner.taken_by_ui(look) {
        flight.enabled = false;
    }
}

/// Hält die Freiflugkamera im Zaum: Tempo in Grenzen, kein Tempowechsel durch das Mausrad
/// über einer Tafel oder Leiste, keine Bewegung beim Tippen in ein Textfeld, nicht zu weit
/// weg.
fn tame_free_camera(
    wants: Res<EguiWantsInput>,
    mouse: WorldMouse,
    feed: Res<BattleFeed>,
    data: Res<sim::GameData>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut camera: Single<(&mut Transform, &mut FreeCameraState), With<BattleCamera>>,
    mut kept_speed: Local<Option<f32>>,
) {
    let (transform, flight) = &mut *camera;
    // Die Engine hat das Mausrad schon verrechnet; über der Oberfläche gilt das nicht.
    let speed = if mouse.wheel_is_free() {
        flight.speed_multiplier.clamp(0.04, 12.0)
    } else {
        kept_speed.unwrap_or(1.0)
    };
    if flight.speed_multiplier != speed {
        flight.speed_multiplier = speed;
    }
    *kept_speed = Some(speed);
    let enabled = !wants.wants_any_keyboard_input();
    if flight.enabled != enabled {
        flight.enabled = enabled;
    }
    // Nicht zu weit weg; nur wenn die Schiffe selbst weit draußen stehen, darf die Kamera
    // so weit zurück, wie die Übersicht es braucht.
    if transform.translation.length() > MAX_DISTANCE {
        let overview = feed.battle().map_or(0.0, |battle| {
            overview_view(battle, data.balance.battle.exit_radius, window.size())
                .translation
                .length()
        });
        let limit = MAX_DISTANCE.max(overview * 1.05);
        if transform.translation.length() > limit {
            transform.translation = transform.translation.clamp_length_max(limit);
        }
    }
}

/// Tasten der Ansicht. Keine davon ist anderswo belegt (Zeitsteuerung, Menü, Hilfe,
/// Schnellspeichern, Freiflug); ein Test wacht darüber.
const KEY_OVERVIEW: KeyCode = KeyCode::KeyU;
const KEY_FOCUS: KeyCode = KeyCode::KeyF;
const KEY_FIGHT: KeyCode = KeyCode::KeyG;
const KEY_FOLLOW: KeyCode = KeyCode::KeyV;
const KEY_OVERLAY: KeyCode = KeyCode::KeyT;
const KEY_LABELS: KeyCode = KeyCode::KeyN;
/// Feste Blicke auf das Gefecht; mit Strg jeweils von der Gegenseite.
const VIEW_KEYS: [(KeyCode, ViewSide); 3] = [
    (KeyCode::Numpad7, ViewSide::Top),
    (KeyCode::Numpad1, ViewSide::Front),
    (KeyCode::Numpad3, ViewSide::Right),
];
const VIEW_REVERSE: [KeyCode; 2] = [KeyCode::ControlLeft, KeyCode::ControlRight];

fn hotkeys(
    keys: Res<ButtonInput<KeyCode>>,
    mut options: ResMut<ViewOptions>,
    selected: Res<SelectedShip>,
    mut requests: MessageWriter<CameraRequest>,
) {
    if keys.just_pressed(KEY_OVERVIEW) {
        requests.write(CameraRequest::Overview);
    }
    if keys.just_pressed(KEY_FOCUS) {
        requests.write(CameraRequest::Focus);
    }
    if keys.just_pressed(KEY_FIGHT) {
        requests.write(CameraRequest::FocusFight);
    }
    if keys.just_pressed(KEY_FOLLOW) {
        options.follow = !options.follow && selected.0.is_some();
    }
    if keys.just_pressed(KEY_OVERLAY) {
        options.overlay = !options.overlay;
    }
    if keys.just_pressed(KEY_LABELS) {
        options.labels = !options.labels;
    }
    let reverse = keys.any_pressed(VIEW_REVERSE);
    for (key, side) in VIEW_KEYS {
        if keys.just_pressed(key) {
            requests.write(CameraRequest::View(if reverse {
                side.opposite()
            } else {
                side
            }));
        }
    }
}

fn apply_requests(
    mut requests: MessageReader<CameraRequest>,
    feed: Res<BattleFeed>,
    data: Res<sim::GameData>,
    frame: Res<ViewFrame>,
    selected: Res<SelectedShip>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut options: ResMut<ViewOptions>,
    mut camera: Single<(&mut Transform, &mut FreeCameraState), With<BattleCamera>>,
) {
    let Some(battle) = feed.battle() else {
        requests.clear();
        return;
    };
    let size = window.size();
    let (transform, flight) = &mut *camera;
    for request in requests.read() {
        let (yaw, _, _) = transform.rotation.to_euler(EulerRot::YXZ);
        // Winkel des neuen Blicks, wenn sie feststehen.
        let mut angles = None;
        let ship_view = |ship: u32| {
            let position = frame.position(ship as usize)?;
            let radius = battle.ships.get(ship as usize)?.radius;
            Some(framing(
                position,
                yaw,
                -0.42,
                (radius * 6.5).clamp(45.0, 600.0),
            ))
        };
        let fight_view = || {
            let (center, spread) = fight_center(battle)?;
            Some(framing(
                center,
                yaw,
                -0.7,
                (spread * 2.6).clamp(350.0, 4500.0),
            ))
        };
        let target = match *request {
            CameraRequest::Overview => {
                options.follow = false;
                Some(overview_view(battle, data.balance.battle.exit_radius, size))
            }
            CameraRequest::Focus => match selected.0 {
                Some(ship) => ship_view(ship),
                None => fight_view(),
            },
            CameraRequest::FocusFight => {
                options.follow = false;
                fight_view()
            }
            CameraRequest::FocusShip(ship) => ship_view(ship),
            CameraRequest::View(side) => {
                let (yaw, pitch) = side.angles();
                angles = Some((yaw, pitch));
                // Das gewählte Schiff, solange es auf dem Feld ist; sonst alle Schiffe.
                let chosen = selected.0.and_then(|ship| {
                    let state = battle.ships.get(ship as usize)?;
                    let position = frame.position(ship as usize)?;
                    state.status.in_play().then_some((position, state.radius))
                });
                Some(match chosen {
                    Some((position, radius)) => {
                        framing(position, yaw, pitch, (radius * 6.5).clamp(45.0, 600.0))
                    }
                    None => {
                        options.follow = false;
                        side_view(battle, side, size)
                    }
                })
            }
            CameraRequest::Place { eye, target } => {
                options.follow = false;
                Some(Transform::from_translation(eye).looking_at(target, Vec3::Y))
            }
        };
        if let Some(target) = target {
            **transform = target;
            flight.velocity = Vec3::ZERO;
            match angles {
                Some((yaw, pitch)) => set_flight_angles(flight, yaw, pitch),
                None => sync_flight(flight, transform),
            }
        }
    }
}

/// Wo das verfolgte Schiff im letzten Frame stand.
#[derive(Resource, Default)]
struct FollowAnchor(Option<(u32, Vec3)>);

/// Verfolgen: Die Kamera macht jede Bewegung des gewählten Schiffs mit; der Spieler kann
/// dabei weiter frei um es herumfliegen.
fn follow_selected(
    frame: Res<ViewFrame>,
    feed: Res<BattleFeed>,
    selected: Res<SelectedShip>,
    mut options: ResMut<ViewOptions>,
    mut anchor: ResMut<FollowAnchor>,
    mut camera: Single<&mut Transform, With<BattleCamera>>,
) {
    let followed = selected.0.filter(|_| options.follow).and_then(|ship| {
        let in_play = feed
            .battle()
            .and_then(|battle| battle.ships.get(ship as usize))
            .is_some_and(|ship| ship.status.in_play());
        in_play
            .then(|| frame.position(ship as usize))
            .flatten()
            .map(|position| (ship, position))
    });
    let Some((ship, position)) = followed else {
        // Schiff verloren, entkommen oder abgewählt: Die Kamera bleibt stehen.
        if options.follow {
            options.follow = false;
        }
        anchor.0 = None;
        return;
    };
    if let Some((last_ship, last_position)) = anchor.0
        && last_ship == ship
    {
        camera.translation += position - last_position;
    }
    anchor.0 = Some((ship, position));
}

/// Linksklick in die Szene wählt das Schiff unter dem Mauszeiger; ein Klick ins Leere hebt
/// die Auswahl auf. Klicks auf Tafeln und Leisten gelten nur diesen.
fn pick_on_click(
    mouse: WorldMouse,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<&Transform, With<BattleCamera>>,
    feed: Res<BattleFeed>,
    frame: Res<ViewFrame>,
    mut selected: ResMut<SelectedShip>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let (Some(cursor), Some(battle)) = (window.cursor_position(), feed.battle()) else {
        return;
    };
    let projector = Projector::new(&camera, FOV, window.size());
    let (origin, direction) = projector.ray(cursor);
    let picked = pick_ship(
        origin,
        direction,
        projector.pixel(),
        battle
            .ships
            .iter()
            .enumerate()
            .filter(|(_, ship)| ship.status.in_play())
            .filter_map(|(index, ship)| Some((index as u32, frame.position(index)?, ship.radius))),
    );
    if selected.0 != picked {
        selected.0 = picked;
    }
}

fn update_projector(
    mut timings: ResMut<ViewTimings>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Transform, &FreeCameraState), With<BattleCamera>>,
    mut frame: ResMut<ViewFrame>,
) {
    let _span = timings.span(Part::Overlay);
    let (transform, flight) = *camera;
    frame.projector = Projector::new(transform, FOV, window.size());
    frame.fly_speed = flight.speed_multiplier;
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim::battle::V2;

    fn camera() -> Projector {
        Projector::new(
            &Transform::from_xyz(300.0, 800.0, 900.0)
                .looking_at(Vec3::new(50.0, 0.0, -100.0), Vec3::Y),
            FOV,
            Vec2::new(1600.0, 900.0),
        )
    }

    #[test]
    fn projection_and_rays_are_inverse() {
        let projector = camera();
        // Der Blickpunkt liegt in der Bildmitte.
        let (center, depth) = projector.project(Vec3::new(50.0, 0.0, -100.0)).unwrap();
        assert!((center - Vec2::new(800.0, 450.0)).length() < 1e-2);
        assert!(depth > 1000.0);
        // Hin und zurück: Ein Punkt der Ebene, auf den Bildschirm gebracht und als Strahl
        // zurückverfolgt, wird in der Ebene wieder getroffen.
        for point in [
            V2::new(0.0, 0.0),
            V2::new(-400.0, 250.0),
            V2::new(620.0, -80.0),
            V2::new(120.0, 900.0),
        ] {
            let world = to_world(point);
            let (screen, _) = projector.project(world).expect("vor der Kamera");
            let (origin, direction) = projector.ray(screen);
            let hit = ray_plane(origin, direction).expect("trifft die Ebene");
            assert!(hit.distance(world) < 0.05, "{hit} statt {world}");
            assert_eq!(hit.y, 0.0);
        }
        // Was hinter der Kamera liegt, hat keinen Bildschirmpunkt.
        assert!(
            projector
                .project(Vec3::new(300.0, 1600.0, 2000.0))
                .is_none()
        );
        // Ein Pixel in der Bildmitte entspricht in der Tiefe d der Strecke d · pixel().
        let (a, _) = projector.project(Vec3::new(50.0, 0.0, -100.0)).unwrap();
        let right = projector.rotation * Vec3::X;
        let (b, _) = projector
            .project(Vec3::new(50.0, 0.0, -100.0) + right * (10.0 * projector.pixel() * depth))
            .unwrap();
        assert!(((b.x - a.x) - 10.0).abs() < 1e-2);
    }

    #[test]
    fn rays_meet_the_plane_only_in_front_of_the_camera() {
        let down = ray_plane(Vec3::new(10.0, 50.0, 5.0), Vec3::NEG_Y).unwrap();
        assert_eq!(down, Vec3::new(10.0, 0.0, 5.0));
        let slanted = ray_plane(
            Vec3::new(0.0, 100.0, 0.0),
            Vec3::new(1.0, -1.0, 0.0).normalize(),
        );
        assert!(slanted.unwrap().distance(Vec3::new(100.0, 0.0, 0.0)) < 1e-3);
        // Nach oben oder waagerecht: kein Schnittpunkt.
        assert!(ray_plane(Vec3::new(0.0, 100.0, 0.0), Vec3::Y).is_none());
        assert!(ray_plane(Vec3::new(0.0, 100.0, 0.0), Vec3::X).is_none());
        // Von unten nach oben trifft die Ebene ebenfalls.
        assert!(ray_plane(Vec3::new(0.0, -100.0, 0.0), Vec3::Y).is_some());
    }

    #[test]
    fn picking_selects_the_ship_under_the_cursor() {
        let projector = camera();
        let ships = [
            (0u32, to_world(V2::new(0.0, 0.0)), 32.0f32),
            (1, to_world(V2::new(120.0, 40.0)), 12.0),
            (2, to_world(V2::new(-300.0, 500.0)), 3.0),
            (3, to_world(V2::new(60.0, -20.0)), 8.0),
        ];
        // Ein Klick auf jedes Schiff wählt genau dieses.
        for (index, position, _) in ships {
            let (screen, _) = projector.project(position).unwrap();
            let (origin, direction) = projector.ray(screen);
            assert_eq!(
                pick_ship(origin, direction, projector.pixel(), ships),
                Some(index)
            );
        }
        // Knapp daneben: innerhalb des Radius eines großen Schiffs trifft, außerhalb nicht.
        let (origin, direction) =
            projector.ray(projector.project(to_world(V2::new(20.0, 0.0))).unwrap().0);
        assert_eq!(
            pick_ship(origin, direction, projector.pixel(), ships),
            Some(0)
        );
        let (origin, direction) = projector.ray(
            projector
                .project(to_world(V2::new(-150.0, -300.0)))
                .unwrap()
                .0,
        );
        assert_eq!(pick_ship(origin, direction, projector.pixel(), ships), None);
        // Ein Jäger (Radius 3) ist aus der Ferne nur über seine Markierung zu treffen:
        // fünf Pixel daneben zählt noch, dreißig nicht mehr.
        let (screen, _) = projector.project(ships[2].1).unwrap();
        let near = projector.ray(screen + Vec2::new(5.0, 0.0));
        assert_eq!(pick_ship(near.0, near.1, projector.pixel(), ships), Some(2));
        let far = projector.ray(screen + Vec2::new(30.0, 0.0));
        assert_eq!(pick_ship(far.0, far.1, projector.pixel(), ships), None);
        // Schiffe hinter der Kamera werden nie gewählt.
        let behind = [(7u32, projector.eye - direction * 50.0, 500.0f32)];
        assert_eq!(
            pick_ship(origin, direction, projector.pixel(), behind),
            None
        );
    }

    #[test]
    fn picking_prefers_the_plane_and_falls_back_to_the_ray() {
        // Zwei Schiffe hintereinander, die Kamera blickt flach über das vordere hinweg.
        let near = (0u32, Vec3::new(0.0, 0.0, 0.0), 30.0f32);
        let far = (1u32, Vec3::new(10.0, 0.0, -400.0), 30.0f32);
        let eye = Vec3::new(0.0, 40.0, 300.0);
        // Der Strahl trifft die Ebene mitten im hinteren Schiff, geht aber in zehn
        // Einheiten Höhe auch über das vordere: Es zählt der Punkt in der Ebene.
        let direction = (far.1 - eye).normalize();
        assert_eq!(pick_ship(eye, direction, 0.0, [near, far]), Some(1));
        // Waagerechter Blick knapp über die Ebene: Der Strahl trifft sie nie. Dann zählt,
        // an wessen Mitte er am genauesten vorbeigeht.
        let level = Vec3::new(0.0, 0.0, -1.0);
        let low = Vec3::new(4.0, 6.0, 300.0);
        assert_eq!(ray_plane(low, level), None);
        assert_eq!(pick_ship(low, level, 0.0, [near, far]), Some(0));
        // Weit daneben trifft auch der Strahl nichts.
        assert_eq!(
            pick_ship(low + Vec3::X * 80.0, level, 0.0, [near, far]),
            None
        );
    }

    /// Liegt ein Punkt der Gefechtsebene im Bild, unterhalb der Kopfleiste?
    fn in_view(projector: &Projector, point: V2) -> bool {
        projector.project(to_world(point)).is_some_and(|(at, _)| {
            at.x >= 0.0
                && at.x <= projector.size.x
                && at.y >= TOP_BAR_PIXELS
                && at.y <= projector.size.y
        })
    }

    /// Halbmesser des Gefechtsfelds in den Spieldaten.
    const FIELD: f32 = 2400.0;

    /// Bildgrößen, mit denen die Blicke zurechtkommen müssen: breit, schmal, hochkant.
    const SIZES: [Vec2; 4] = [
        Vec2::new(1600.0, 900.0),
        Vec2::new(1024.0, 600.0),
        Vec2::new(900.0, 900.0),
        Vec2::new(700.0, 1000.0),
    ];

    /// Liegt ein Punkt der Gefechtsebene unter einer der Lagetafeln in den oberen Ecken?
    fn under_a_panel(projector: &Projector, point: V2) -> bool {
        projector.project(to_world(point)).is_some_and(|(at, _)| {
            at.y < PANEL_CORNER.y
                && (at.x < PANEL_CORNER.x || at.x > projector.size.x - PANEL_CORNER.x)
        })
    }

    #[test]
    fn overview_shows_the_whole_circle() {
        for size in SIZES {
            for extent in [650.0, 1300.0, 2400.0, 7400.0] {
                let view = overview_transform(extent, size);
                let projector = Projector::new(&view, FOV, size);
                // Jeder Punkt des Kreises ist zu sehen, auch der dem Betrachter nächste im
                // Süden, und keiner liegt unter einer Lagetafel; die Feldmitte ohnehin.
                let rim: Vec<Vec2> = (0..360)
                    .step_by(3)
                    .map(|degrees| {
                        let point = V2::from_angle((degrees as f32).to_radians()) * extent;
                        assert!(
                            in_view(&projector, point),
                            "{point:?} fehlt (Halbmesser {extent}, Bild {size})"
                        );
                        assert!(
                            !under_a_panel(&projector, point),
                            "{point:?} liegt unter einer Tafel (Halbmesser {extent}, Bild {size})"
                        );
                        projector.project(to_world(point)).unwrap().0
                    })
                    .collect();
                assert!(in_view(&projector, V2::ZERO));
                // Und der Kreis füllt das Bild, soweit die Tafeln es zulassen.
                let (mut min, mut max) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
                for at in rim {
                    min = min.min(at);
                    max = max.max(at);
                }
                // Er reicht in der Breite an die Spalte zwischen den Tafeln heran oder in
                // der Höhe an den Raum unter der Kopfleiste oder unter den Tafeln.
                let share = (max - min) / size;
                let column = 1.0 - FIT_MARGIN - 2.0 * PANEL_CORNER.x / size.x;
                let below_bar = 1.0 - FIT_MARGIN - TOP_BAR_PIXELS / size.y;
                let below_panels = 1.0 - FIT_MARGIN - PANEL_CORNER.y / size.y;
                assert!(
                    share.x > 0.95 * column
                        || share.y > 0.95 * below_bar
                        || share.y > 0.95 * below_panels,
                    "der Kreis ist klein im Bild ({share}) bei Halbmesser {extent}, Bild {size}"
                );
                // Die Übersicht blickt von Süden schräg nach unten; die erste Partei steht
                // links.
                assert!(view.translation.z > 0.0 && view.translation.y > 0.0);
                let left = projector
                    .project(to_world(V2::new(-extent, 0.0)))
                    .unwrap()
                    .0;
                let right = projector.project(to_world(V2::new(extent, 0.0))).unwrap().0;
                assert!(left.x < right.x);
            }
        }
        // In einem winzigen Fenster, in dem die Tafeln alles belegen, zählt nur das Bild.
        let size = Vec2::new(400.0, 300.0);
        let projector = Projector::new(&overview_transform(1300.0, size), FOV, size);
        for degrees in (0..360).step_by(15) {
            assert!(in_view(
                &projector,
                V2::from_angle((degrees as f32).to_radians()) * 1300.0
            ));
        }
    }

    #[test]
    fn overview_shows_every_ship_of_every_preset() {
        use super::super::practice::{PRESETS, PracticeSetup};
        let data = super::super::feed::testing::data();
        let mut farthest = 0.0f32;
        for preset in &PRESETS {
            let mut battle = PracticeSetup::from_preset(preset)
                .resolve(&data)
                .unwrap()
                .build(&data);
            for tick in 0..600 {
                if tick % 40 == 0 || tick == 1 {
                    let ships = || battle.ships.iter().filter(|ship| ship.status.in_play());
                    farthest = ships()
                        .map(|ship| ship.pos.length())
                        .fold(farthest, f32::max);
                    for size in SIZES {
                        let projector =
                            Projector::new(&overview_view(&battle, FIELD, size), FOV, size);
                        let outside = ships()
                            .filter(|ship| !in_view(&projector, ship.pos))
                            .count();
                        assert_eq!(
                            outside, 0,
                            "{}: {outside} Schiffe außerhalb in Takt {tick}, Bild {size}",
                            preset.name
                        );
                        // Auch unter den Lagetafeln steht keines.
                        let hidden = ships()
                            .filter(|ship| under_a_panel(&projector, ship.pos))
                            .count();
                        assert_eq!(
                            hidden, 0,
                            "{}: {hidden} Schiffe unter den Tafeln in Takt {tick}, Bild {size}",
                            preset.name
                        );
                    }
                }
                battle.step(&data);
            }
        }
        // Große Linien stehen vor dem ersten Takt weit jenseits des Feldrands; auch dann
        // sind alle Schiffe im Bild.
        assert!(
            farthest > data.balance.battle.exit_radius * 1.5,
            "{farthest}"
        );
    }

    #[test]
    fn overview_follows_the_fleets() {
        use super::super::feed::testing::{MIXED, data, stepped};
        let data = data();
        let size = Vec2::new(1600.0, 900.0);
        // Zu Beginn stehen die Flotten weit auseinander, im Gefecht eng beisammen: Die
        // Übersicht rückt entsprechend näher.
        let (start, _) = stepped(&data, MIXED, 0);
        let (fight, _) = stepped(&data, MIXED, 400);
        let wide = overview_view(&start, FIELD, size);
        let tight = overview_view(&fight, FIELD, size);
        assert!(
            tight.translation.length() < wide.translation.length() * 0.8,
            "{} gegen {}",
            tight.translation.length(),
            wide.translation.length()
        );
        // Sie blickt von Süden schräg nach unten, die Feldmitte in der Bildmitte der Breite.
        for view in [wide, tight] {
            assert!(view.translation.z > 0.0 && view.translation.y > 0.0);
            assert_eq!(view.translation.x, 0.0);
        }
        // Ein leeres Feld zeigt die Übersicht nicht kleiner als ihr Mindestmaß, und ein weit
        // abgetriebenes Schiff zieht sie nicht ins Unendliche.
        let mut empty = start.clone();
        empty.ships.clear();
        let least = overview_view(&empty, FIELD, size);
        assert!(
            least
                .translation
                .distance(overview_transform(OVERVIEW_MIN, size).translation)
                < 1e-3
        );
        assert!(least.translation.length() < tight.translation.length());
        let mut adrift = start.clone();
        adrift.ships[0].pos = V2::new(90_000.0, 0.0);
        assert!(
            overview_view(&adrift, FIELD, size).translation.length()
                < wide.translation.length() * 1.01
        );
        // Nach dem Ende gehört das Trümmerfeld zur Übersicht, soweit es auf dem Feld liegt:
        // Sie zeigt dann mehr als nur die Überlebenden.
        let (mut ended, _) = stepped(&data, MIXED, 0);
        while !ended.finished {
            ended.step(&data);
        }
        let wrecks_reach = ended
            .wrecks
            .iter()
            .map(|wreck| wreck.pos.length() + wreck.radius)
            .filter(|reach| *reach <= FIELD)
            .fold(0.0, f32::max);
        assert!(wrecks_reach > OVERVIEW_MIN);
        assert!(overview_extent(&ended, FIELD) >= wrecks_reach);
        let mut running = ended.clone();
        running.finished = false;
        assert!(overview_extent(&running, FIELD) <= overview_extent(&ended, FIELD));
        // Ein Wrack, das weit vom Feld getrieben ist, zählt nicht.
        ended.wrecks[0].pos = V2::new(0.0, FIELD * 3.0);
        assert!(overview_extent(&ended, FIELD) <= FIELD);
    }

    /// Welt ohne Fenster und Grafikkarte: ein Gefecht im Feed, die Schlachtkamera und ein
    /// Fenster, das nur seine Größe und den Mauszeiger kennt.
    fn world(ticks: u32) -> (World, Entity) {
        use super::super::feed::testing::{MIXED, data, stepped};
        use bevy::ecs::message::Messages;

        let data = data();
        let (battle, feed) = stepped(&data, MIXED, ticks);
        let view = overview_view(&battle, FIELD, Vec2::new(1600.0, 900.0));
        let frame = ViewFrame {
            epoch: Some(feed.epoch),
            alpha: 1.0,
            time: battle.time,
            tick_seconds: 0.1,
            poses: battle
                .ships
                .iter()
                .map(|ship| (to_world(ship.pos), ship.heading))
                .collect(),
            projector: Projector::new(&view, FOV, Vec2::new(1600.0, 900.0)),
            fly_speed: 1.0,
        };
        let mut world = World::new();
        world.insert_resource(data);
        world.insert_resource(feed);
        world.insert_resource(frame);
        world.insert_resource(ViewTimings::default());
        world.init_resource::<SelectedShip>();
        world.init_resource::<ViewOptions>();
        world.init_resource::<FollowAnchor>();
        world.init_resource::<Messages<CameraRequest>>();
        world.init_resource::<ButtonInput<KeyCode>>();
        crate::pointer::testing::free_mouse(&mut world);
        let camera = world
            .spawn((BattleCamera, view, FreeCameraState::default()))
            .id();
        let mut window = Window::default();
        window.resolution.set(1600.0, 900.0);
        world.spawn((window, PrimaryWindow));
        (world, camera)
    }

    fn click(world: &mut World, at: Option<Vec2>) {
        use bevy::ecs::system::RunSystemOnce;
        let mut windows = world.query::<&mut Window>();
        windows.single_mut(world).unwrap().set_cursor_position(at);
        world
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        world.run_system_once(pick_on_click).unwrap();
        let mut buttons = world.resource_mut::<ButtonInput<MouseButton>>();
        buttons.release(MouseButton::Left);
        buttons.clear();
    }

    #[test]
    fn a_click_selects_the_ship_under_the_cursor() {
        let (mut world, camera) = world(120);
        let view = *world.get::<Transform>(camera).unwrap();
        let projector = Projector::new(&view, FOV, Vec2::new(1600.0, 900.0));
        let battle = world.resource::<BattleFeed>().battle().unwrap().clone();
        // Jedes Schiff auf dem Feld lässt sich an seinem Ort im Bild anklicken.
        let mut chosen = 0;
        for (index, ship) in battle.ships.iter().enumerate() {
            if !ship.status.in_play() {
                continue;
            }
            let (screen, _) = projector.project(to_world(ship.pos)).expect("im Bild");
            click(&mut world, Some(screen));
            let picked = world
                .resource::<SelectedShip>()
                .0
                .expect("ein Schiff gewählt");
            // Liegen zwei Schiffe im Bild übereinander, gewinnt eines von beiden; es steht
            // dann aber an derselben Stelle.
            let other = &battle.ships[picked as usize];
            assert!(
                picked as usize == index
                    || other.pos.distance(ship.pos) <= other.radius.max(ship.radius) * 1.6,
                "Klick auf {index} wählte {picked}"
            );
            chosen += 1;
        }
        assert!(chosen > 10);
        // Ein Klick ins Leere hebt die Auswahl auf; ohne Mauszeiger im Fenster bleibt sie.
        click(&mut world, Some(Vec2::new(1590.0, 890.0)));
        assert_eq!(world.resource::<SelectedShip>().0, None);
        world.resource_mut::<SelectedShip>().0 = Some(3);
        click(&mut world, None);
        assert_eq!(world.resource::<SelectedShip>().0, Some(3));
    }

    #[test]
    fn keys_request_camera_moves_and_toggle_options() {
        use bevy::ecs::message::Messages;
        use bevy::ecs::system::RunSystemOnce;

        let (mut world, _) = world(60);
        let press = |world: &mut World, key: KeyCode| {
            let mut keys = world.resource_mut::<ButtonInput<KeyCode>>();
            keys.press(key);
            world.run_system_once(hotkeys).unwrap();
            let mut keys = world.resource_mut::<ButtonInput<KeyCode>>();
            keys.release(key);
            keys.clear();
            world
                .resource_mut::<Messages<CameraRequest>>()
                .drain()
                .collect::<Vec<_>>()
        };
        assert_eq!(
            press(&mut world, KeyCode::KeyU),
            vec![CameraRequest::Overview]
        );
        assert_eq!(
            press(&mut world, KeyCode::KeyG),
            vec![CameraRequest::FocusFight]
        );
        assert_eq!(press(&mut world, KeyCode::KeyF), vec![CameraRequest::Focus]);
        // Ziffernblock: von oben, von vorn, von der Seite; mit Strg von der Gegenseite.
        for (key, side, other) in [
            (KeyCode::Numpad7, ViewSide::Top, ViewSide::Bottom),
            (KeyCode::Numpad1, ViewSide::Front, ViewSide::Back),
            (KeyCode::Numpad3, ViewSide::Right, ViewSide::Left),
        ] {
            assert_eq!(press(&mut world, key), vec![CameraRequest::View(side)]);
            for control in [KeyCode::ControlLeft, KeyCode::ControlRight] {
                world.resource_mut::<ButtonInput<KeyCode>>().press(control);
                assert_eq!(press(&mut world, key), vec![CameraRequest::View(other)]);
                world
                    .resource_mut::<ButtonInput<KeyCode>>()
                    .release(control);
            }
            assert_eq!(side.opposite(), other);
            assert_eq!(other.opposite(), side);
        }
        let before = *world.resource::<ViewOptions>();
        assert!(press(&mut world, KeyCode::KeyT).is_empty());
        assert!(press(&mut world, KeyCode::KeyN).is_empty());
        let after = *world.resource::<ViewOptions>();
        assert_eq!(after.overlay, !before.overlay);
        assert_eq!(after.labels, !before.labels);
        // Verfolgen braucht ein gewähltes Schiff.
        press(&mut world, KeyCode::KeyV);
        assert!(!world.resource::<ViewOptions>().follow);
        world.resource_mut::<SelectedShip>().0 = Some(0);
        press(&mut world, KeyCode::KeyV);
        assert!(world.resource::<ViewOptions>().follow);
        press(&mut world, KeyCode::KeyV);
        assert!(!world.resource::<ViewOptions>().follow);
        // Tasten der Zeitsteuerung und des Menüs lösen hier nichts aus.
        for key in [
            KeyCode::Space,
            KeyCode::Digit3,
            KeyCode::Escape,
            KeyCode::F1,
        ] {
            assert!(press(&mut world, key).is_empty());
        }
        assert_eq!(
            *world.resource::<ViewOptions>(),
            ViewOptions {
                follow: false,
                ..after
            }
        );
    }

    #[test]
    fn requests_move_the_camera_and_following_keeps_the_ship_in_view() {
        use bevy::ecs::message::Messages;
        use bevy::ecs::system::RunSystemOnce;

        let (mut world, camera) = world(200);
        let ship = world
            .resource::<BattleFeed>()
            .battle()
            .unwrap()
            .ships
            .iter()
            .position(|ship| ship.status.in_play())
            .unwrap();
        let position = world.resource::<ViewFrame>().poses[ship].0;
        let centered = |world: &World| {
            let view = world.get::<Transform>(camera).unwrap();
            let projector = Projector::new(view, FOV, Vec2::new(1600.0, 900.0));
            projector.project(world.resource::<ViewFrame>().poses[ship].0)
        };

        // Heranholen: Das Schiff steht danach in der Bildmitte, nah genug, um es zu sehen.
        world
            .resource_mut::<Messages<CameraRequest>>()
            .write(CameraRequest::FocusShip(ship as u32));
        world.run_system_once(apply_requests).unwrap();
        let (screen, depth) = centered(&world).unwrap();
        assert!((screen - Vec2::new(800.0, 450.0)).length() < 0.5);
        assert!(depth > 40.0 && depth < 650.0);
        // Die Steuerung der Engine kennt die neue Blickrichtung.
        let view = *world.get::<Transform>(camera).unwrap();
        let flight = world.get::<FreeCameraState>(camera).unwrap();
        let (yaw, pitch, _) = view.rotation.to_euler(EulerRot::YXZ);
        assert!((flight.yaw - yaw).abs() < 1e-5 && (flight.pitch - pitch).abs() < 1e-5);

        // Verfolgen: Wandert das Schiff, wandert die Kamera mit; es bleibt in der Mitte.
        world.resource_mut::<SelectedShip>().0 = Some(ship as u32);
        world.resource_mut::<ViewOptions>().follow = true;
        world.run_system_once(follow_selected).unwrap();
        for step in 1..=5 {
            world.resource_mut::<ViewFrame>().poses[ship].0 =
                position + Vec3::new(12.0, 0.0, -7.0) * step as f32;
            world.run_system_once(follow_selected).unwrap();
            let (screen, _) = centered(&world).unwrap();
            assert!((screen - Vec2::new(800.0, 450.0)).length() < 0.5);
        }
        // Ohne Auswahl endet das Verfolgen, die Kamera bleibt stehen.
        world.resource_mut::<SelectedShip>().0 = None;
        let before = *world.get::<Transform>(camera).unwrap();
        world.resource_mut::<ViewFrame>().poses[ship].0 += Vec3::X * 500.0;
        world.run_system_once(follow_selected).unwrap();
        assert!(!world.resource::<ViewOptions>().follow);
        assert_eq!(*world.get::<Transform>(camera).unwrap(), before);

        // Übersicht und Gefechtsschwerpunkt zeigen von oben auf das Feld.
        for request in [CameraRequest::Overview, CameraRequest::FocusFight] {
            world
                .resource_mut::<Messages<CameraRequest>>()
                .write(request);
            world.run_system_once(apply_requests).unwrap();
            let view = world.get::<Transform>(camera).unwrap();
            assert!(
                view.translation.y > 100.0 && view.forward().y < -0.3,
                "{request:?}"
            );
        }
    }

    /// Eine Gefechtsübung ohne Fenster, pausiert vor dem ersten Takt.
    fn practice() -> crate::testing::TestGame {
        let mut game = crate::testing::TestGame::start("--practice --seed 3");
        game.frames(4);
        assert_eq!(game.state(), crate::state::AppState::Practice);
        game
    }

    /// Ein Schiff, das frei in der Szene steht (nicht unter einer Tafel), und sein Ort im
    /// Bild.
    fn ship_in_the_open(game: &crate::testing::TestGame) -> (u32, Vec2) {
        let frame = game.resource::<ViewFrame>();
        let battle = game.resource::<BattleFeed>().battle().unwrap();
        battle
            .ships
            .iter()
            .enumerate()
            .filter(|(_, ship)| ship.status.in_play())
            .filter_map(|(index, _)| {
                let (screen, _) = frame.projector.project(frame.position(index)?)?;
                Some((index as u32, screen))
            })
            // Links oben und rechts stehen Tafeln (rechts unten erscheint die des gewählten
            // Schiffs); dazwischen ist die Szene frei.
            .filter(|(_, at)| at.x > 285.0 && at.x < 1215.0 && at.y > 60.0 && at.y < 880.0)
            .min_by(|a, b| {
                let center = Vec2::new(800.0, 500.0);
                a.1.distance(center).total_cmp(&b.1.distance(center))
            })
            .expect("ein Schiff in der freien Szene")
    }

    fn orientation(game: &mut crate::testing::TestGame) -> Quat {
        let world = game.world_mut();
        world
            .query_filtered::<&Transform, With<BattleCamera>>()
            .single(world)
            .unwrap()
            .rotation
    }

    /// Höhe, in der die Kopfleiste liegt, und eine Stelle auf der Lagetafel links.
    const BAR_Y: f32 = 11.0;
    const ON_SIDE_PANEL: Vec2 = Vec2::new(150.0, 70.0);
    const OPEN_SCENE: Vec2 = Vec2::new(800.0, 620.0);

    #[test]
    fn clicks_on_the_top_bar_keep_the_selected_ship() {
        use crate::state::AppState;
        use crate::time_control::SimSpeed;

        let mut game = practice();
        let (ship, at) = ship_in_the_open(&game);
        game.click(at);
        let selected = game.resource::<SelectedShip>().0;
        assert!(
            selected.is_some(),
            "Klick auf Schiff {ship} wählt ein Schiff"
        );

        // Einmal quer über die ganze Kopfleiste klicken, auf Knöpfe wie auf leere Stellen.
        // Jeder Knopf tut dabei, was er soll; die Auswahl bleibt, bis „Verlassen“ die
        // Übung beendet.
        let (mut paused, mut options, mut clicks) = (false, false, 0);
        let mut x = 5.0;
        while x < crate::testing::WINDOW.x && game.state() == AppState::Practice {
            let before = (
                *game.resource::<SimSpeed>(),
                *game.resource::<ViewOptions>(),
            );
            game.click(Vec2::new(x, BAR_Y));
            if game.state() != AppState::Practice {
                break;
            }
            assert_eq!(
                game.resource::<SelectedShip>().0,
                selected,
                "Klick auf die Kopfleiste bei x = {x}"
            );
            paused |= before.0.paused != game.resource::<SimSpeed>().paused;
            options |= before.1.labels != game.resource::<ViewOptions>().labels
                || before.1.overlay != game.resource::<ViewOptions>().overlay;
            clicks += 1;
            x += 9.0;
        }
        assert!(paused, "die Zeitsteuerung der Leiste hat reagiert");
        assert!(options, "die Schalter der Leiste haben reagiert");
        assert!(clicks > 100, "nur {clicks} Klicks bis zum Verlassen");
        game.frames(3);
        assert_eq!(
            game.state(),
            AppState::MainMenu,
            "„Verlassen“ am rechten Rand beendet die Übung"
        );
    }

    #[test]
    fn clicks_on_panels_keep_the_selection_and_clicks_into_space_clear_it() {
        let mut game = practice();
        let (_, at) = ship_in_the_open(&game);
        game.click(at);
        let selected = game.resource::<SelectedShip>().0;
        assert!(selected.is_some());
        // Lagetafel, Tafel des gewählten Schiffs (unten rechts) und eine leere Stelle der
        // Kopfleiste; auch dann, wenn Zeigerbewegung und Tastendruck im selben Frame
        // eintreffen.
        for point in [
            ON_SIDE_PANEL,
            Vec2::new(1400.0, 700.0),
            Vec2::new(900.0, BAR_Y),
        ] {
            game.click(point);
            assert_eq!(game.resource::<SelectedShip>().0, selected, "{point}");
            game.move_pointer(OPEN_SCENE);
            game.frames(2);
            game.click_at_once(point);
            assert_eq!(
                game.resource::<SelectedShip>().0,
                selected,
                "schnell: {point}"
            );
        }
        // Ein Klick in den leeren Raum hebt die Auswahl auf; die Tafel des Schiffs
        // verschwindet, und die Stelle gehört wieder der Szene.
        game.click(OPEN_SCENE);
        assert_eq!(game.resource::<SelectedShip>().0, None);
        game.click_at_once(at);
        assert_eq!(game.resource::<SelectedShip>().0, selected);
    }

    #[test]
    fn wheel_and_mouse_look_over_panels_leave_the_camera_alone() {
        let mut game = practice();
        let (_, at) = ship_in_the_open(&game);
        game.click(at);
        let rest = orientation(&mut game);
        for point in [
            ON_SIDE_PANEL,
            Vec2::new(1400.0, 700.0),
            Vec2::new(900.0, BAR_Y),
        ] {
            game.scroll(point, 4.0);
            assert_eq!(
                game.resource::<ViewFrame>().fly_speed,
                1.0,
                "Mausrad bei {point}"
            );
            game.drag(MouseButton::Right, point, Vec2::new(160.0, 40.0));
            assert_eq!(orientation(&mut game), rest, "Umsehen ab {point}");
        }
        // In der freien Szene ändert das Mausrad das Flugtempo, und die rechte Maustaste
        // dreht den Blick.
        game.scroll(OPEN_SCENE, 4.0);
        assert!(game.resource::<ViewFrame>().fly_speed > 1.5);
        game.drag(MouseButton::Right, OPEN_SCENE, Vec2::new(160.0, 40.0));
        assert!(orientation(&mut game).angle_between(rest) > 0.1);
    }

    #[test]
    fn keys_do_not_collide() {
        use std::collections::BTreeSet;

        let own = [
            KEY_OVERVIEW,
            KEY_FOCUS,
            KEY_FIGHT,
            KEY_FOLLOW,
            KEY_OVERLAY,
            KEY_LABELS,
        ];
        // Was die Freiflugkamera der Engine belegt, so wie die Ansicht sie einstellt.
        let flight = flight_controls();
        let engine = [
            flight.key_forward,
            flight.key_back,
            flight.key_left,
            flight.key_right,
            flight.key_up,
            flight.key_down,
            flight.key_run,
            flight.keyboard_key_toggle_cursor_grab,
            flight.key_snap_reverse,
            flight.axis_top,
            flight.axis_right,
            flight.axis_front,
        ];
        // Was im ganzen Spiel gilt: Pause, Geschwindigkeit, Hilfe, Schnellspeichern, Menü.
        let global = [
            KeyCode::Space,
            KeyCode::Digit1,
            KeyCode::Digit2,
            KeyCode::Digit3,
            KeyCode::Digit4,
            KeyCode::Digit5,
            KeyCode::Digit6,
            KeyCode::NumpadAdd,
            KeyCode::NumpadSubtract,
            KeyCode::Equal,
            KeyCode::Minus,
            KeyCode::BracketRight,
            KeyCode::Slash,
            KeyCode::F1,
            KeyCode::F5,
            KeyCode::F9,
            KeyCode::Escape,
        ];
        let views: Vec<KeyCode> = VIEW_KEYS
            .iter()
            .map(|(key, _)| *key)
            .chain(VIEW_REVERSE)
            .collect();
        let all: Vec<String> = own
            .iter()
            .chain(&views)
            .chain(&engine)
            .chain(&global)
            .map(|key| format!("{key:?}"))
            .collect();
        let distinct: BTreeSet<&String> = all.iter().collect();
        assert_eq!(distinct.len(), all.len(), "doppelt belegte Taste: {all:?}");
        // Die Hilfe nennt jede Taste der Ansicht.
        let help: String = crate::ui::panels::BATTLE_KEYS
            .iter()
            .map(|(keys, _)| format!(" {keys} "))
            .collect();
        for key in own {
            let name = format!("{key:?}").replace("Key", "");
            assert!(
                help.contains(&format!(" {name} ")),
                "Taste {name} fehlt in der Hilfe"
            );
        }
        for word in [
            "W A S D",
            "E / Q",
            "Maustaste",
            "Mausrad",
            "Umschalt",
            "Esc",
        ] {
            assert!(help.contains(word), "{word} fehlt in der Hilfe");
        }
        // Die Zeile zum Ziffernblock nennt die Tasten und Blickrichtungen in der
        // Reihenfolge, in der sie belegt sind.
        let (keys, what) = crate::ui::panels::BATTLE_KEYS
            .iter()
            .find(|(keys, _)| keys.contains("Ziffernblock"))
            .expect("Zeile zum Ziffernblock");
        let digits: Vec<String> = VIEW_KEYS
            .iter()
            .map(|(key, _)| format!("{key:?}").replace("Numpad", ""))
            .collect();
        assert_eq!(*keys, format!("Ziffernblock {}", digits.join(" / ")));
        let sides: Vec<&str> = VIEW_KEYS
            .iter()
            .map(|(_, side)| match side {
                ViewSide::Top => "von oben",
                ViewSide::Front => "von vorn",
                ViewSide::Right => "von der Seite",
                other => panic!("{other:?} hat keine eigene Taste"),
            })
            .collect();
        assert!(
            what.starts_with(&format!("Blick {}", sides.join(" / "))),
            "{what}"
        );
        assert!(what.contains("Strg") && what.contains("Gegenseite"));
    }

    #[test]
    fn fixed_views_show_the_whole_battle_from_the_named_side() {
        use bevy::ecs::message::Messages;
        use bevy::ecs::system::RunSystemOnce;

        let (mut world, camera) = world(150);
        let battle = world.resource::<BattleFeed>().battle().unwrap().clone();
        let (center, _) = fleet_circle(&battle);
        let size = Vec2::new(1600.0, 900.0);
        for side in [
            ViewSide::Top,
            ViewSide::Bottom,
            ViewSide::Front,
            ViewSide::Back,
            ViewSide::Right,
            ViewSide::Left,
        ] {
            world
                .resource_mut::<Messages<CameraRequest>>()
                .write(CameraRequest::View(side));
            world.run_system_once(apply_requests).unwrap();
            let view = *world.get::<Transform>(camera).unwrap();
            let projector = Projector::new(&view, FOV, size);
            // Jedes Schiff auf dem Feld ist im Bild.
            for ship in battle.ships.iter().filter(|ship| ship.status.in_play()) {
                assert!(in_view(&projector, ship.pos), "{side:?}: Schiff fehlt");
            }
            // Die Kamera steht dort, wo der Name des Blicks es sagt, und blickt zum Gefecht.
            let offset = view.translation - center;
            let forward = view.rotation * Vec3::NEG_Z;
            match side {
                ViewSide::Top => assert!(offset.y > 100.0 && forward.y < -0.999),
                ViewSide::Bottom => assert!(offset.y < -100.0 && forward.y > 0.999),
                ViewSide::Front => assert!(offset.z > 100.0 && offset.y > 0.0 && forward.z < -0.9),
                ViewSide::Back => assert!(offset.z < -100.0 && offset.y > 0.0 && forward.z > 0.9),
                ViewSide::Right => assert!(offset.x > 100.0 && offset.y > 0.0 && forward.x < -0.9),
                ViewSide::Left => assert!(offset.x < -100.0 && offset.y > 0.0 && forward.x > 0.9),
            }
            // Von oben gesehen ist Norden oben und die erste Partei links.
            if side == ViewSide::Top {
                let at = |point: V2| projector.project(to_world(point)).unwrap().0;
                assert!(at(V2::new(0.0, 300.0)).y < at(V2::new(0.0, -300.0)).y);
                assert!(at(V2::new(-300.0, 0.0)).x < at(V2::new(300.0, 0.0)).x);
            }
            // Die Steuerung der Engine kennt genau diese Blickrichtung: Die erste
            // Mausbewegung danach lässt das Bild nicht springen.
            let flight = world.get::<FreeCameraState>(camera).unwrap();
            let engine = Quat::from_euler(EulerRot::ZYX, 0.0, flight.yaw, flight.pitch);
            assert!(
                engine.dot(view.rotation).abs() > 1.0 - 1e-6,
                "{side:?}: {engine} gegen {}",
                view.rotation
            );
        }

        // Mit einem gewählten Schiff zeigt der Blick dieses Schiff, aus derselben Richtung.
        let ship = battle
            .ships
            .iter()
            .position(|ship| ship.status.in_play())
            .unwrap();
        world.resource_mut::<SelectedShip>().0 = Some(ship as u32);
        let position = world.resource::<ViewFrame>().poses[ship].0;
        for side in [ViewSide::Top, ViewSide::Left] {
            world
                .resource_mut::<Messages<CameraRequest>>()
                .write(CameraRequest::View(side));
            world.run_system_once(apply_requests).unwrap();
            let view = *world.get::<Transform>(camera).unwrap();
            let (screen, depth) = Projector::new(&view, FOV, size).project(position).unwrap();
            assert!(
                (screen - Vec2::new(800.0, 450.0)).length() < 0.5,
                "{side:?}"
            );
            assert!(depth > 40.0 && depth < 650.0);
        }
    }

    #[test]
    fn numpad_keys_turn_the_view_to_the_battle() {
        let mut game = practice();
        let forward = |game: &mut crate::testing::TestGame| orientation(game) * Vec3::NEG_Z;
        let all_in_view = |game: &crate::testing::TestGame| {
            let frame = game.resource::<ViewFrame>();
            let battle = game.resource::<BattleFeed>().battle().unwrap();
            battle
                .ships
                .iter()
                .filter(|ship| ship.status.in_play())
                .all(|ship| in_view(&frame.projector, ship.pos))
        };
        game.key(KeyCode::Numpad7);
        assert!(forward(&mut game).y < -0.999, "Ziffernblock 7: von oben");
        assert!(all_in_view(&game));
        game.key_with(&[KeyCode::ControlLeft], KeyCode::Numpad7);
        assert!(forward(&mut game).y > 0.999, "mit Strg: von unten");
        assert!(all_in_view(&game));
        game.key(KeyCode::Numpad1);
        assert!(forward(&mut game).z < -0.9, "Ziffernblock 1: von vorn");
        assert!(all_in_view(&game));
        game.key(KeyCode::Numpad3);
        assert!(forward(&mut game).x < -0.9, "Ziffernblock 3: von der Seite");
        assert!(all_in_view(&game));
        game.key_with(&[KeyCode::ControlLeft], KeyCode::Numpad3);
        assert!(
            forward(&mut game).x > 0.9,
            "mit Strg: von der anderen Seite"
        );
        // Nach dem senkrechten Blick dreht die Maus weich weiter, ohne Sprung.
        game.key(KeyCode::Numpad7);
        let before = orientation(&mut game);
        game.drag(MouseButton::Right, OPEN_SCENE, Vec2::new(30.0, 0.0));
        let turned = orientation(&mut game).angle_between(before);
        assert!(turned > 0.01 && turned < 0.2, "gedreht um {turned}");
        // Die Übersicht bringt den gewohnten Blick zurück.
        game.key(KeyCode::KeyU);
        assert!(forward(&mut game).z < -0.5 && all_in_view(&game));
    }

    #[test]
    fn framing_looks_at_its_target() {
        let target = Vec3::new(40.0, 0.0, -70.0);
        let view = framing(target, 0.8, -0.5, 250.0);
        assert!((view.translation.distance(target) - 250.0).abs() < 1e-3);
        let projector = Projector::new(&view, FOV, Vec2::new(1600.0, 900.0));
        let (screen, depth) = projector.project(target).unwrap();
        assert!((screen - Vec2::new(800.0, 450.0)).length() < 1e-2);
        assert!((depth - 250.0).abs() < 1e-2);
        assert!(view.translation.y > 0.0, "die Kamera blickt von oben");
    }
}

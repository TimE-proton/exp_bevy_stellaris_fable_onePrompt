//! Kamera der Schlachtansicht: freier Flug mit der Freiflugkamera der Engine, dazu
//! Sprünge zur Übersicht, zum Gefechtsschwerpunkt und zum gewählten Schiff, Verfolgen
//! und die Auswahl per Mausklick.

use super::feed::BattleFeed;
use super::{BattleViewSet, Part, ViewFrame, ViewTimings, to_world};
use bevy::camera_controller::free_camera::{FreeCamera, FreeCameraPlugin, FreeCameraState};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::input::{
    EguiWantsInput, egui_wants_any_keyboard_input, egui_wants_any_pointer_input,
};
use sim::battle::Battle;

/// Senkrechter Bildwinkel der Schlachtkamera.
pub const FOV: f32 = 50.0 * std::f32::consts::PI / 180.0;
const NEAR: f32 = 2.0;
/// Weiter als so kommt die Kamera nicht vom Feld weg.
const MAX_DISTANCE: f32 = 14_000.0;
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
    /// Über das ganze Feld.
    Overview,
    /// Zum gewählten Schiff; ohne Auswahl zum Schwerpunkt des Gefechts.
    Focus,
    /// Zum Schwerpunkt des Gefechts.
    FocusFight,
    FocusShip(u32),
    /// Feste Stelle und Blickpunkt (Selbsttest).
    Place {
        eye: Vec3,
        target: Vec3,
    },
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

impl Default for Projector {
    fn default() -> Self {
        Self::new(&overview_transform(2400.0), FOV, Vec2::new(1600.0, 900.0))
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

/// Blick schräg von oben auf einen Kreis mit Halbmesser `extent` um die Feldmitte. Die
/// erste Partei steht dabei links, die zweite rechts.
pub fn overview_transform(extent: f32) -> Transform {
    Transform::from_xyz(0.0, extent * 0.95, extent * 1.3)
        .looking_at(Vec3::new(0.0, 0.0, extent * 0.06), Vec3::Y)
}

/// Halbmesser, den die Übersicht zeigen muss, damit alle Schiffe auf dem Feld im Bild
/// sind: so eng wie möglich, höchstens das ganze Feld.
pub fn overview_extent(battle: &Battle, field: f32) -> f32 {
    let reach = battle
        .ships
        .iter()
        .filter(|ship| ship.status.in_play())
        .map(|ship| ship.pos.length() + ship.radius)
        .fold(0.0, f32::max);
    (reach * 1.12 + 120.0).clamp(650.0_f32.min(field), field)
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
        .add_systems(
            Update,
            (
                tame_free_camera,
                hotkeys.run_if(not(egui_wants_any_keyboard_input)),
                apply_requests,
                follow_selected,
                pick_on_click.run_if(not(egui_wants_any_pointer_input)),
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
        FreeCamera {
            walk_speed: 380.0,
            run_speed: 1500.0,
            friction: 14.0,
            scroll_factor: 0.18,
            ..default()
        },
        flight,
        overview_transform(2400.0),
    ));
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
        *transform = overview_transform(2400.0);
        sync_flight(flight, transform);
    }
}

/// Die Steuerung der Engine führt Gier- und Nickwinkel selbst mit; nach einem Sprung
/// müssen sie zur neuen Blickrichtung passen.
fn sync_flight(flight: &mut FreeCameraState, transform: &Transform) {
    let (yaw, pitch, _) = transform.rotation.to_euler(EulerRot::YXZ);
    flight.yaw = yaw;
    flight.pitch = pitch;
    flight.rotation_curve = None;
}

/// Hält die Freiflugkamera im Zaum: Tempo in Grenzen, kein Tempowechsel durch das Mausrad
/// über einer Tafel, keine Bewegung beim Tippen in ein Textfeld, nicht zu weit weg.
fn tame_free_camera(
    wants: Res<EguiWantsInput>,
    mut camera: Single<(&mut Transform, &mut FreeCameraState), With<BattleCamera>>,
    mut kept_speed: Local<Option<f32>>,
) {
    let (transform, flight) = &mut *camera;
    let speed = if wants.wants_any_pointer_input() {
        kept_speed.unwrap_or(1.0)
    } else {
        flight.speed_multiplier.clamp(0.04, 12.0)
    };
    if flight.speed_multiplier != speed {
        flight.speed_multiplier = speed;
    }
    *kept_speed = Some(speed);
    let enabled = !wants.wants_any_keyboard_input();
    if flight.enabled != enabled {
        flight.enabled = enabled;
    }
    if transform.translation.length() > MAX_DISTANCE {
        transform.translation = transform.translation.clamp_length_max(MAX_DISTANCE);
    }
}

fn hotkeys(
    keys: Res<ButtonInput<KeyCode>>,
    mut options: ResMut<ViewOptions>,
    selected: Res<SelectedShip>,
    mut requests: MessageWriter<CameraRequest>,
) {
    if keys.just_pressed(KeyCode::KeyU) {
        requests.write(CameraRequest::Overview);
    }
    if keys.just_pressed(KeyCode::KeyF) {
        requests.write(CameraRequest::Focus);
    }
    if keys.just_pressed(KeyCode::KeyG) {
        requests.write(CameraRequest::FocusFight);
    }
    if keys.just_pressed(KeyCode::KeyV) {
        options.follow = !options.follow && selected.0.is_some();
    }
    if keys.just_pressed(KeyCode::KeyT) {
        options.overlay = !options.overlay;
    }
    if keys.just_pressed(KeyCode::KeyN) {
        options.labels = !options.labels;
    }
}

fn apply_requests(
    mut requests: MessageReader<CameraRequest>,
    feed: Res<BattleFeed>,
    data: Res<sim::GameData>,
    frame: Res<ViewFrame>,
    selected: Res<SelectedShip>,
    mut options: ResMut<ViewOptions>,
    mut camera: Single<(&mut Transform, &mut FreeCameraState), With<BattleCamera>>,
) {
    let Some(battle) = feed.battle() else {
        requests.clear();
        return;
    };
    let (transform, flight) = &mut *camera;
    for request in requests.read() {
        let (yaw, _, _) = transform.rotation.to_euler(EulerRot::YXZ);
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
                Some(overview_transform(overview_extent(
                    battle,
                    data.balance.battle.exit_radius,
                )))
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
            CameraRequest::Place { eye, target } => {
                options.follow = false;
                Some(Transform::from_translation(eye).looking_at(target, Vec3::Y))
            }
        };
        if let Some(target) = target {
            **transform = target;
            flight.velocity = Vec3::ZERO;
            sync_flight(flight, transform);
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

/// Linksklick wählt das Schiff unter dem Mauszeiger; ein Klick ins Leere hebt die Auswahl
/// auf.
fn pick_on_click(
    buttons: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<&Transform, With<BattleCamera>>,
    feed: Res<BattleFeed>,
    frame: Res<ViewFrame>,
    mut selected: ResMut<SelectedShip>,
) {
    if !buttons.just_pressed(MouseButton::Left) {
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
    camera: Single<&Transform, With<BattleCamera>>,
    mut frame: ResMut<ViewFrame>,
) {
    let _span = timings.span(Part::Overlay);
    frame.projector = Projector::new(&camera, FOV, window.size());
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

    #[test]
    fn overview_shows_the_whole_field() {
        let extent = 2400.0;
        let projector = Projector::new(&overview_transform(extent), FOV, Vec2::new(1600.0, 900.0));
        // Beide Aufstellungsräume und die Feldmitte liegen im Bild.
        for point in [
            V2::new(-1300.0, 0.0),
            V2::new(1300.0, 0.0),
            V2::new(0.0, 0.0),
            V2::new(0.0, 1300.0),
            V2::new(0.0, -1300.0),
        ] {
            let (screen, _) = projector.project(to_world(point)).expect("im Bild");
            assert!(
                screen.x > 0.0 && screen.x < 1600.0,
                "{point:?} bei {screen}"
            );
            assert!(screen.y > 0.0 && screen.y < 900.0, "{point:?} bei {screen}");
        }
    }

    #[test]
    fn overview_follows_the_fleets() {
        use super::super::feed::testing::{MIXED, data, stepped};
        let data = data();
        let field = data.balance.battle.exit_radius;
        // Zu Beginn stehen die Flotten weit auseinander, im Gefecht eng beisammen: Die
        // Übersicht rückt entsprechend näher, zeigt aber immer alle Schiffe.
        let (start, _) = stepped(&data, MIXED, 0);
        let (fight, _) = stepped(&data, MIXED, 400);
        let wide = overview_extent(&start, field);
        let tight = overview_extent(&fight, field);
        assert!(wide > 1300.0 && wide <= field);
        assert!(tight < wide, "{tight} gegen {wide}");
        for (battle, extent) in [(&start, wide), (&fight, tight)] {
            let projector =
                Projector::new(&overview_transform(extent), FOV, Vec2::new(1600.0, 900.0));
            for ship in battle.ships.iter().filter(|ship| ship.status.in_play()) {
                let (screen, _) = projector
                    .project(to_world(ship.pos))
                    .expect("vor der Kamera");
                assert!(screen.x > 0.0 && screen.x < 1600.0 && screen.y > 0.0 && screen.y < 900.0);
            }
        }
        // Ein leeres Feld zeigt die Übersicht nicht kleiner als ihr Mindestmaß.
        let mut empty = start.clone();
        empty.ships.clear();
        assert_eq!(overview_extent(&empty, field), 650.0);
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

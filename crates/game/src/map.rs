//! Galaxiekarte: Draufsicht mit Systemen, Sprungrouten, Planeten und Besitzfarben.

use crate::pointer::WorldMouse;
use crate::session::SessionEpoch;
use crate::state::{AppState, ViewState};
use bevy::camera::visibility::{RenderLayers, VisibleEntities};
use bevy::camera::{CameraOutputMode, ClearColorConfig};
use bevy::ecs::message::Messages;
use bevy::ecs::system::SystemParam;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::render::render_resource::BlendState;
use bevy::sprite::Anchor;
use bevy::window::PrimaryWindow;
use bevy_egui::input::EguiWantsInput;
use bevy_egui::{EguiGlobalSettings, PrimaryEguiContext};
use sim::data::Rgb;
use sim::galaxy::Galaxy;
use sim::ids::{PlanetId, PolityId, SystemId};
use sim::polity::Polities;
use sim::state::SimConfig;

const Z_TERRITORY: f32 = 0.0;
const Z_LANE: f32 = 1.0;
const Z_PLANET_RING: f32 = 2.0;
const Z_PLANET: f32 = 2.1;
const Z_STAR: f32 = 3.0;
const Z_LABEL: f32 = 4.0;

const STAR_RADIUS: f32 = 5.0;
const TERRITORY_RADIUS: f32 = 34.0;
const PLANET_RADIUS: f32 = 1.9;
const MIN_SCALE: f32 = 0.12;
const MAX_SCALE: f32 = 3.0;
/// Ab dieser Zoomstufe (Weltmaß je Pixel) werden Planeten ausgeblendet.
const PLANET_LOD_SCALE: f32 = 1.1;

/// Was der Spieler auf der Karte gewählt hat.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub system: Option<SystemId>,
    pub planet: Option<PlanetId>,
}

/// Bittet die Kartenkamera, ein System zu zentrieren.
#[derive(Message, Clone, Copy, Debug)]
pub struct FocusSystem(pub SystemId);

#[derive(Component)]
pub struct MapCamera;

/// Alles, was zur Karte der laufenden Partie gehört.
#[derive(Component)]
struct MapEntity;

#[derive(Component)]
struct Territory(SystemId);

#[derive(Component)]
struct SystemLabel;

#[derive(Component)]
struct PlanetDot;

#[derive(Component)]
struct PlanetRing(PlanetId);

/// Bringt diese Kamera ihr Bild ins Fenster? Sie muss eingeschaltet sein, und keine andere
/// eingeschaltete Kamera darf nach ihr zeichnen und das Bild dabei mit ihrem Hintergrund
/// füllen. (Die Kamera der Oberfläche zeichnet zuletzt, aber auf durchsichtigem Grund.)
pub fn camera_shows(camera: Entity, cameras: &Query<(Entity, &Camera)>) -> bool {
    let Ok((_, own)) = cameras.get(camera) else {
        return false;
    };
    let paints_over = |other: &Camera| {
        other.is_active
            && other.order > own.order
            && match other.clear_color {
                ClearColorConfig::None => false,
                ClearColorConfig::Custom(color) => color.alpha() > 0.0,
                ClearColorConfig::Default => true,
            }
    };
    own.is_active
        && !cameras
            .iter()
            .any(|(entity, other)| entity != camera && paints_over(other))
}

/// Zählt, was die Kartenkamera von der Karte wirklich zeichnet: was die Engine im letzten
/// Frame für genau diese Kamera als sichtbar eingestuft hat (in ihrem Sichtfeld und nicht
/// ausgeblendet). Der Selbsttest prüft damit, dass sein Foto nicht bloß die Oberfläche
/// zeigt. Dass irgendeine Kamera die Karte im Sichtfeld hat, genügt nicht: Die
/// Schlachtkamera erfasst sie auch, zeichnet aber keine Karte.
#[derive(SystemParam)]
pub struct MapCensus<'w, 's> {
    camera: Query<'w, 's, (Entity, &'static VisibleEntities), With<MapCamera>>,
    cameras: Query<'w, 's, (Entity, &'static Camera)>,
    entities: Query<'w, 's, (), With<MapEntity>>,
}

impl MapCensus<'_, '_> {
    /// Bringt die Kartenkamera ihr Bild ins Fenster?
    pub fn camera_shows(&self) -> bool {
        self.camera
            .single()
            .is_ok_and(|(camera, _)| camera_shows(camera, &self.cameras))
    }

    /// Sterne, Routen, Planeten und Beschriftungen, die die Kartenkamera zeichnet.
    pub fn visible(&self) -> usize {
        if !self.camera_shows() {
            return 0;
        }
        self.camera.single().map_or(0, |(_, visible)| {
            visible
                .entities
                .values()
                .flatten()
                .filter(|entity| self.entities.contains(**entity))
                .count()
        })
    }
}

/// Geteilte Meshes und Materialien: gleiche Handles lassen Bevy die Karte bündeln.
#[derive(Resource)]
struct MapAssets {
    circle: Handle<Mesh>,
    ring: Handle<Mesh>,
    square: Handle<Mesh>,
    lane: Handle<ColorMaterial>,
    bridge: Handle<ColorMaterial>,
    stars: Vec<Handle<ColorMaterial>>,
    planets: Vec<Handle<ColorMaterial>>,
    polity_solid: Vec<Handle<ColorMaterial>>,
    polity_soft: Vec<Handle<ColorMaterial>>,
}

pub struct GalaxyViewPlugin;

impl Plugin for GalaxyViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Selection>()
            .add_message::<FocusSystem>()
            .add_systems(Startup, spawn_cameras)
            .add_systems(OnExit(AppState::InGame), clear_selection)
            .add_systems(
                Update,
                (
                    rebuild_map,
                    (
                        camera_controls,
                        focus_camera,
                        pick_system,
                        sync_ownership,
                        level_of_detail,
                        draw_overlays,
                    )
                        .chain()
                        .run_if(in_state(ViewState::Galaxy)),
                )
                    .chain()
                    .run_if(in_state(AppState::InGame)),
            );
    }
}

fn srgb(color: Rgb) -> Color {
    Color::srgb(color.0, color.1, color.2)
}

fn spawn_cameras(mut commands: Commands, mut egui_settings: ResMut<EguiGlobalSettings>) {
    commands.spawn((
        MapCamera,
        Camera2d,
        Projection::from(OrthographicProjection {
            scale: 1.5,
            ..OrthographicProjection::default_2d()
        }),
    ));
    // Eigene Kamera nur für die Oberfläche: Sie bleibt aktiv, egal ob gerade die Karte
    // (2D) oder eine Schlacht (3D) gezeigt wird.
    egui_settings.auto_create_primary_context = false;
    commands.spawn((
        PrimaryEguiContext,
        Camera2d,
        RenderLayers::none(),
        Camera {
            order: 10,
            output_mode: CameraOutputMode::Write {
                blend_state: Some(BlendState::ALPHA_BLENDING),
                clear_color: ClearColorConfig::None,
            },
            clear_color: ClearColorConfig::Custom(Color::NONE),
            ..default()
        },
    ));
}

fn clear_selection(mut selection: ResMut<Selection>) {
    *selection = Selection::default();
}

/// Position eines Planeten auf der Karte: feste Bahn um seinen Stern.
pub fn planet_map_position(galaxy: &Galaxy, planet: PlanetId) -> Vec2 {
    let planet = galaxy.planet(planet);
    let center = Vec2::from(galaxy.system(planet.system).pos);
    let radius = 10.0 + 5.0 * planet.orbit as f32;
    // Goldener Winkel: verteilt die Planeten ohne Zufall gleichmäßig.
    let angle = planet.id.0 as f32 * 2.399_963;
    center + radius * Vec2::from_angle(angle)
}

/// Baut die Karte neu auf, sobald eine andere Partie in der Welt liegt.
fn rebuild_map(
    mut commands: Commands,
    epoch: Res<SessionEpoch>,
    mut built: Local<Option<SessionEpoch>>,
    galaxy: Res<Galaxy>,
    polities: Res<Polities>,
    config: Res<SimConfig>,
    data: Res<sim::GameData>,
    old: Query<Entity, With<MapEntity>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut selection: ResMut<Selection>,
    mut focus_requests: ResMut<Messages<FocusSystem>>,
    mut camera: Single<(&mut Transform, &mut Projection), With<MapCamera>>,
    window: Single<&Window, With<PrimaryWindow>>,
) {
    if *built == Some(*epoch) {
        return;
    }
    *built = Some(*epoch);
    for entity in &old {
        commands.entity(entity).despawn();
    }
    // Bitten an die Kamera, die noch unterwegs sind, meinen Systeme der vorigen Partie (ein
    // Klick auf einen Verweis im Frame des Wechsels). In dieser gibt es sie nicht oder
    // unter gleicher Nummer ein anderes.
    focus_requests.clear();

    let mut solid = |color: Color| materials.add(ColorMaterial::from_color(color));
    let assets = MapAssets {
        circle: meshes.add(Circle::new(1.0)),
        ring: meshes.add(Annulus::new(0.78, 1.0)),
        square: meshes.add(Rectangle::new(1.0, 1.0)),
        lane: solid(Color::srgba(0.35, 0.45, 0.62, 0.55)),
        bridge: solid(Color::srgba(0.98, 0.70, 0.25, 0.85)),
        stars: data
            .galaxy
            .stars
            .iter()
            .map(|star| solid(srgb(star.color)))
            .collect(),
        planets: data
            .planet_kinds
            .iter()
            .map(|kind| solid(srgb(kind.color)))
            .collect(),
        polity_solid: polities.list.iter().map(|p| solid(srgb(p.color))).collect(),
        polity_soft: polities
            .list
            .iter()
            .map(|p| solid(srgb(p.color).with_alpha(0.07)))
            .collect(),
    };

    for lane in &galaxy.lanes {
        let a = Vec2::from(galaxy.system(lane.a).pos);
        let b = Vec2::from(galaxy.system(lane.b).pos);
        let delta = b - a;
        let (material, width) = if lane.bridge {
            (&assets.bridge, 2.2)
        } else {
            (&assets.lane, 1.1)
        };
        commands.spawn((
            map_scope(),
            Mesh2d(assets.square.clone()),
            MeshMaterial2d(material.clone()),
            Transform::from_translation(((a + b) * 0.5).extend(Z_LANE))
                .with_rotation(Quat::from_rotation_z(delta.to_angle()))
                .with_scale(Vec3::new(delta.length(), width, 1.0)),
        ));
    }

    for system in &galaxy.systems {
        let pos = Vec2::from(system.pos);
        let star = &data.galaxy.stars[system.star as usize];
        commands.spawn((
            map_scope(),
            Territory(system.id),
            Mesh2d(assets.circle.clone()),
            MeshMaterial2d(assets.polity_soft[0].clone()),
            Transform::from_translation(pos.extend(Z_TERRITORY))
                .with_scale(Vec3::splat(TERRITORY_RADIUS)),
            Visibility::Hidden,
        ));
        commands.spawn((
            map_scope(),
            Mesh2d(assets.circle.clone()),
            MeshMaterial2d(assets.stars[system.star as usize].clone()),
            Transform::from_translation(pos.extend(Z_STAR))
                .with_scale(Vec3::splat(STAR_RADIUS * star.radius)),
        ));
        commands.spawn((
            map_scope(),
            SystemLabel,
            Text2d::new(system.name.clone()),
            TextFont {
                font_size: FontSize::Px(13.0),
                ..default()
            },
            TextColor(Color::srgb(0.82, 0.86, 0.92)),
            Anchor::TOP_CENTER,
            Transform::from_translation((pos - Vec2::new(0.0, 9.0)).extend(Z_LABEL)),
        ));
        for planet_id in &system.planets {
            let planet = galaxy.planet(*planet_id);
            let at = planet_map_position(&galaxy, *planet_id);
            commands.spawn((
                map_scope(),
                PlanetDot,
                Mesh2d(assets.circle.clone()),
                MeshMaterial2d(assets.planets[planet.kind as usize].clone()),
                Transform::from_translation(at.extend(Z_PLANET))
                    .with_scale(Vec3::splat(PLANET_RADIUS)),
            ));
            commands.spawn((
                map_scope(),
                PlanetRing(*planet_id),
                Mesh2d(assets.ring.clone()),
                MeshMaterial2d(assets.polity_solid[0].clone()),
                Transform::from_translation(at.extend(Z_PLANET_RING))
                    .with_scale(Vec3::splat(PLANET_RADIUS * 1.9)),
                Visibility::Hidden,
            ));
        }
    }
    commands.insert_resource(assets);

    // Ausgangsblick: ganze Galaxie; ausgewählt ist das eigene Heimatsystem.
    let extent = galaxy
        .systems
        .iter()
        .map(|system| Vec2::from(system.pos).length())
        .fold(100.0, f32::max)
        + 80.0;
    let (transform, projection) = &mut *camera;
    transform.translation = Vec3::ZERO;
    if let Projection::Orthographic(ortho) = &mut **projection {
        ortho.scale = (2.0 * extent / window.height().max(1.0)).clamp(MIN_SCALE, MAX_SCALE);
    }
    let home = config
        .player
        .and_then(|player| polities.get(player).capital)
        .map(|planet| galaxy.planet(planet).system);
    *selection = Selection {
        system: home,
        planet: None,
    };
}

/// Markierung für alles, was zur Karte gehört und mit der Partie verschwindet.
fn map_scope() -> (MapEntity, DespawnOnExit<AppState>) {
    (MapEntity, DespawnOnExit(AppState::InGame))
}

/// Verschieben (rechte/mittlere Maustaste, WASD, Pfeile) und Zoomen zum Mauszeiger.
/// Die Maus wirkt nur, wo sie der Karte gehört: Ein Zug, der auf einer Tafel beginnt, und
/// das Mausrad über einer Tafel bewegen die Karte nicht. Die Tasten ruhen, solange in
/// ein Feld der Oberfläche getippt wird.
fn camera_controls(
    time: Res<Time<Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: WorldMouse,
    wants: Res<EguiWantsInput>,
    motion: Res<AccumulatedMouseMotion>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut camera: Single<
        (&Camera, &GlobalTransform, &mut Transform, &mut Projection),
        With<MapCamera>,
    >,
) {
    let (camera, global, transform, projection) = &mut *camera;
    let Projection::Orthographic(ortho) = &mut **projection else {
        return;
    };

    if mouse.any_pressed([MouseButton::Right, MouseButton::Middle]) {
        transform.translation.x -= motion.delta.x * ortho.scale;
        transform.translation.y += motion.delta.y * ortho.scale;
    }

    let mut direction = Vec2::ZERO;
    if !wants.wants_any_keyboard_input() {
        if keys.any_pressed([KeyCode::KeyW, KeyCode::ArrowUp]) {
            direction.y += 1.0;
        }
        if keys.any_pressed([KeyCode::KeyS, KeyCode::ArrowDown]) {
            direction.y -= 1.0;
        }
        if keys.any_pressed([KeyCode::KeyD, KeyCode::ArrowRight]) {
            direction.x += 1.0;
        }
        if keys.any_pressed([KeyCode::KeyA, KeyCode::ArrowLeft]) {
            direction.x -= 1.0;
        }
    }
    if direction != Vec2::ZERO {
        let step = direction.normalize() * 600.0 * ortho.scale * time.delta_secs();
        transform.translation += step.extend(0.0);
    }

    let lines = mouse.wheel_lines();
    if lines != 0.0 {
        let old_scale = ortho.scale;
        let new_scale = (old_scale * (-lines * 0.18).exp()).clamp(MIN_SCALE, MAX_SCALE);
        // Der Punkt unter dem Mauszeiger bleibt beim Zoomen an seiner Stelle.
        if let Some(cursor) = window.cursor_position()
            && let Ok(anchor) = camera.viewport_to_world_2d(global, cursor)
        {
            let offset = transform.translation.truncate() - anchor;
            let moved = anchor + offset * (new_scale / old_scale);
            transform.translation = moved.extend(transform.translation.z);
        }
        ortho.scale = new_scale;
    }

    let limit = 900.0;
    transform.translation.x = transform.translation.x.clamp(-limit, limit);
    transform.translation.y = transform.translation.y.clamp(-limit, limit);
}

fn focus_camera(
    mut requests: MessageReader<FocusSystem>,
    galaxy: Res<Galaxy>,
    mut camera: Single<(&mut Transform, &mut Projection), With<MapCamera>>,
) {
    let Some(FocusSystem(system)) = requests.read().last().copied() else {
        return;
    };
    let (transform, projection) = &mut *camera;
    let pos = Vec2::from(galaxy.system(system).pos);
    transform.translation = pos.extend(transform.translation.z);
    if let Projection::Orthographic(ortho) = &mut **projection {
        ortho.scale = ortho.scale.min(0.45);
    }
}

/// Linksklick auf die Karte wählt das nächste System unter dem Mauszeiger; ein Klick auf
/// eine Tafel oder Leiste gilt nur dieser.
fn pick_system(
    mouse: WorldMouse,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform, &Projection), With<MapCamera>>,
    galaxy: Res<Galaxy>,
    mut selection: ResMut<Selection>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let (camera, global, projection) = *camera;
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Ok(world) = camera.viewport_to_world_2d(global, cursor) else {
        return;
    };
    let scale = match projection {
        Projection::Orthographic(ortho) => ortho.scale,
        _ => 1.0,
    };
    let picked = nearest_system(&galaxy, world, (18.0 * scale).max(22.0));
    if picked != selection.system {
        *selection = Selection {
            system: picked,
            planet: None,
        };
    }
}

/// Nächstes System innerhalb von `radius` um `point`.
pub fn nearest_system(galaxy: &Galaxy, point: Vec2, radius: f32) -> Option<SystemId> {
    galaxy
        .systems
        .iter()
        .map(|system| (system.id, Vec2::from(system.pos).distance(point)))
        .filter(|(_, distance)| *distance <= radius)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

/// Besitzfarben folgen dem Zustand der Simulation.
fn sync_ownership(
    galaxy: Res<Galaxy>,
    assets: Option<Res<MapAssets>>,
    mut territories: Query<
        (
            &Territory,
            &mut MeshMaterial2d<ColorMaterial>,
            &mut Visibility,
        ),
        Without<PlanetRing>,
    >,
    mut rings: Query<
        (
            &PlanetRing,
            &mut MeshMaterial2d<ColorMaterial>,
            &mut Visibility,
        ),
        Without<Territory>,
    >,
    projection: Single<&Projection, With<MapCamera>>,
) {
    let Some(assets) = assets else {
        return;
    };
    let planets_visible = match &**projection {
        Projection::Orthographic(ortho) => ortho.scale < PLANET_LOD_SCALE,
        _ => true,
    };
    for (territory, mut material, mut visibility) in &mut territories {
        let owner = galaxy.system_owner(territory.0);
        apply_owner(
            owner,
            &assets.polity_soft,
            true,
            &mut material,
            &mut visibility,
        );
    }
    for (ring, mut material, mut visibility) in &mut rings {
        let owner = galaxy.planet(ring.0).owner;
        apply_owner(
            owner,
            &assets.polity_solid,
            planets_visible,
            &mut material,
            &mut visibility,
        );
    }
}

fn apply_owner(
    owner: Option<PolityId>,
    materials: &[Handle<ColorMaterial>],
    shown: bool,
    material: &mut Mut<MeshMaterial2d<ColorMaterial>>,
    visibility: &mut Mut<Visibility>,
) {
    let wanted = match owner.and_then(|id| materials.get(id.index())) {
        Some(handle) if shown => {
            if material.0 != *handle {
                material.0 = handle.clone();
            }
            Visibility::Inherited
        }
        _ => Visibility::Hidden,
    };
    if **visibility != wanted {
        **visibility = wanted;
    }
}

/// Planeten nur aus der Nähe zeigen; Beschriftungen behalten ihre Bildschirmgröße.
fn level_of_detail(
    projection: Single<&Projection, With<MapCamera>>,
    mut dots: Query<&mut Visibility, With<PlanetDot>>,
    mut labels: Query<&mut Transform, With<SystemLabel>>,
) {
    let Projection::Orthographic(ortho) = &**projection else {
        return;
    };
    let wanted = if ortho.scale < PLANET_LOD_SCALE {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for mut visibility in &mut dots {
        if *visibility != wanted {
            *visibility = wanted;
        }
    }
    let label_scale = Vec3::splat(ortho.scale.clamp(0.3, 1.6));
    for mut transform in &mut labels {
        if transform.scale != label_scale {
            transform.scale = label_scale;
        }
    }
}

/// Auswahlring, Bahnen des gewählten Systems und Markierung der Engpässe.
fn draw_overlays(
    mut gizmos: Gizmos,
    galaxy: Res<Galaxy>,
    selection: Res<Selection>,
    time: Res<Time<Real>>,
) {
    for system in galaxy.systems.iter().filter(|system| system.chokepoint) {
        let pos = Vec2::from(system.pos);
        gizmos.rect_2d(
            Isometry2d::new(pos, Rot2::degrees(45.0)),
            Vec2::splat(15.0),
            Color::srgba(0.98, 0.70, 0.25, 0.7),
        );
    }
    let Some(system_id) = selection.system else {
        return;
    };
    let system = galaxy.system(system_id);
    let pos = Vec2::from(system.pos);
    let pulse = 1.0 + 0.08 * (time.elapsed_secs() * 3.0).sin();
    gizmos.circle_2d(pos, 9.0 * pulse, Color::WHITE);
    for planet_id in &system.planets {
        let planet = galaxy.planet(*planet_id);
        gizmos.circle_2d(
            pos,
            10.0 + 5.0 * planet.orbit as f32,
            Color::srgba(1.0, 1.0, 1.0, 0.12),
        );
        if selection.planet == Some(*planet_id) {
            gizmos.circle_2d(planet_map_position(&galaxy, *planet_id), 4.6, Color::WHITE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim::state::SimState;

    fn galaxy() -> Galaxy {
        let data = sim::GameData::load_workspace().unwrap();
        let mut state = SimState::generate(
            &data,
            SimConfig {
                seed: 4,
                player: None,
            },
        );
        state.galaxy.rebuild_index();
        state.galaxy
    }

    #[test]
    fn picking_finds_the_system_under_the_cursor() {
        let galaxy = galaxy();
        for system in &galaxy.systems {
            let near = Vec2::from(system.pos) + Vec2::new(3.0, -2.0);
            assert_eq!(nearest_system(&galaxy, near, 20.0), Some(system.id));
        }
        assert_eq!(
            nearest_system(&galaxy, Vec2::new(9_000.0, 9_000.0), 20.0),
            None
        );
    }

    use crate::testing::TestGame;

    /// Eine laufende Partie ohne Fenster; das Heimatsystem ist gewählt.
    fn campaign() -> TestGame {
        let mut game = TestGame::start("--seed 3");
        game.frames(4);
        assert_eq!(game.state(), AppState::InGame);
        assert!(game.resource::<Selection>().system.is_some());
        game
    }

    fn selected(game: &TestGame) -> Option<SystemId> {
        game.resource::<Selection>().system
    }

    /// Ort der Kartenkamera und ihr Maßstab (Weltmaß je Pixel).
    fn view(game: &mut TestGame) -> (Vec2, f32) {
        let world = game.world_mut();
        let (transform, projection) = world
            .query_filtered::<(&Transform, &Projection), With<MapCamera>>()
            .single(world)
            .unwrap();
        let Projection::Orthographic(ortho) = projection else {
            panic!("die Karte wird ohne Perspektive gezeigt");
        };
        (transform.translation.truncate(), ortho.scale)
    }

    /// Punkt der Karte unter einer Stelle des Fensters.
    fn map_point(game: &mut TestGame, screen: Vec2) -> Vec2 {
        let world = game.world_mut();
        let (camera, transform) = world
            .query_filtered::<(&Camera, &GlobalTransform), With<MapCamera>>()
            .single(world)
            .unwrap();
        camera.viewport_to_world_2d(transform, screen).unwrap()
    }

    /// Stelle des Fensters, an der ein System liegt.
    fn on_screen(game: &mut TestGame, system: SystemId) -> Vec2 {
        let pos = Vec2::from(game.resource::<Galaxy>().system(system).pos);
        let world = game.world_mut();
        let (camera, transform) = world
            .query_filtered::<(&Camera, &GlobalTransform), With<MapCamera>>()
            .single(world)
            .unwrap();
        camera
            .world_to_viewport(transform, pos.extend(0.0))
            .unwrap()
    }

    /// Stellen auf den vier Leisten und Tafeln der Partie (Fenster 1600 × 900), jeweils
    /// dort, wo kein Knopf liegt.
    const ON_PANELS: [(Vec2, &str); 4] = [
        (Vec2::new(200.0, 620.0), "linke Tafel"),
        (Vec2::new(1450.0, 500.0), "rechte Tafel"),
        (Vec2::new(950.0, 11.0), "Kopfleiste"),
        (Vec2::new(800.0, 850.0), "Chronik"),
    ];
    /// Mitte der freien Karte zwischen den Tafeln.
    const FREE_MAP: Vec2 = Vec2::new(860.0, 400.0);

    #[test]
    fn clicks_on_panels_and_bars_leave_the_map_alone() {
        let mut game = campaign();
        let home = selected(&game);
        for (point, what) in ON_PANELS {
            game.click(point);
            assert_eq!(selected(&game), home, "Klick auf {what}");
            // Auch wenn Zeigerbewegung und Tastendruck im selben Frame eintreffen.
            game.move_pointer(FREE_MAP);
            game.frames(2);
            game.click_at_once(point);
            assert_eq!(selected(&game), home, "schneller Klick auf {what}");
        }

        // Die Karte selbst reagiert weiter: Ein Klick auf ein anderes System wählt es.
        let systems: Vec<SystemId> = game
            .resource::<Galaxy>()
            .systems
            .iter()
            .map(|system| system.id)
            .collect();
        let free = |at: Vec2| at.x > 470.0 && at.x < 1270.0 && at.y > 60.0 && at.y < 740.0;
        let other = systems
            .iter()
            .copied()
            .find(|system| Some(*system) != home && free(on_screen(&mut game, *system)))
            .expect("ein zweites System auf der freien Karte");
        let at = on_screen(&mut game, other);
        game.click(at);
        assert_eq!(selected(&game), Some(other));
        // Ein Klick ins Leere hebt die Auswahl auf.
        let empty = (0..40)
            .flat_map(|row| (0..60).map(move |column| (column, row)))
            .map(|(column, row)| Vec2::new(480.0 + column as f32 * 13.0, 70.0 + row as f32 * 16.0))
            .find(|at| {
                let point = map_point(&mut game, *at);
                nearest_system(game.resource::<Galaxy>(), point, 60.0).is_none()
            })
            .expect("eine leere Stelle der Karte");
        game.click(empty);
        assert_eq!(selected(&game), None);
        // Und ein Klick auf eine Tafel ändert auch daran nichts.
        game.click(at);
        assert_eq!(selected(&game), Some(other));
        game.click(ON_PANELS[0].0);
        assert_eq!(selected(&game), Some(other));
    }

    #[test]
    fn wheel_and_drags_over_panels_leave_the_map_alone() {
        let mut game = campaign();
        let start = view(&mut game);
        for (point, what) in ON_PANELS {
            game.scroll(point, 3.0);
            assert_eq!(view(&mut game), start, "Mausrad über {what}");
            for button in [MouseButton::Right, MouseButton::Middle] {
                game.drag(button, point, Vec2::new(-90.0, 40.0));
                assert_eq!(view(&mut game), start, "Zug mit {button:?} auf {what}");
            }
        }

        // Über der Karte zoomt das Mausrad, und ein Zug verschiebt sie.
        game.scroll(FREE_MAP, 3.0);
        let zoomed = view(&mut game);
        assert!(zoomed.1 < start.1, "Mausrad über der Karte zoomt hinein");
        game.drag(MouseButton::Right, FREE_MAP, Vec2::new(-80.0, 0.0));
        let moved = view(&mut game);
        assert!(
            (moved.0.x - zoomed.0.x - 80.0 * zoomed.1).abs() < 1e-3,
            "die Karte folgt dem Zug: {moved:?} nach {zoomed:?}"
        );
        // Ein Zug, der über der Karte beginnt, gilt weiter, wenn er über eine Tafel gerät.
        game.drag(MouseButton::Middle, FREE_MAP, Vec2::new(620.0, 0.0));
        let dragged = view(&mut game);
        assert!(
            (moved.0.x - dragged.0.x - 620.0 * moved.1).abs() < 1e-3,
            "der Zug endet nicht am Rand der Tafel: {dragged:?} nach {moved:?}"
        );
        // Die Tasten verschieben die Karte, wo auch immer der Mauszeiger steht.
        game.move_pointer(ON_PANELS[0].0);
        game.frames(2);
        game.key(KeyCode::KeyW);
        assert!(view(&mut game).0.y > dragged.0.y);
    }

    /// Eine Bitte an die Kartenkamera, die erst am Ende eines Frames eintrifft: so spät, wie
    /// die Oberfläche sie nach einem Klick schickt.
    #[derive(Resource, Default)]
    struct LateFocus(Option<SystemId>);

    fn focus_late(mut late: ResMut<LateFocus>, mut focus: MessageWriter<FocusSystem>) {
        if let Some(system) = late.0.take() {
            focus.write(FocusSystem(system));
        }
    }

    #[test]
    fn the_map_follows_a_game_loaded_inside_a_running_campaign() {
        use crate::session::SessionRequest;
        use crate::testing::{save_of_new_game, small_and_large_galaxy};

        let data = sim::GameData::load_workspace().unwrap();
        let (small, large) = small_and_large_galaxy(&data);
        let save = save_of_new_game(&data, small, "karte");
        let mut game = TestGame::start(&format!("--seed {large}"));
        game.app
            .init_resource::<LateFocus>()
            .add_systems(PostUpdate, focus_late);
        game.frames(4);
        let old_systems = game.resource::<Galaxy>().systems.len();
        assert_eq!(game.count::<Territory>(), old_systems);
        // Der Spieler hat das letzte System der großen Galaxie gewählt und die Karte dorthin
        // geholt: Dieses System gibt es in der kleinen nicht.
        let last = SystemId((old_systems - 1) as u32);
        game.world_mut().resource_mut::<Selection>().system = Some(last);
        game.send(FocusSystem(last));
        game.frames(2);
        assert_ne!(view(&mut game).0, Vec2::ZERO);

        // Laden; im selben Frame klickt der Spieler noch auf Verweise der alten Partie: Die
        // Bitten an die Karte treffen ein, wenn die Partie schon gewechselt hat. Einmal
        // nennt die Bitte ein System, das es danach nicht mehr gibt, einmal eines, das es
        // unter gleicher Nummer wieder gibt.
        for stale in [last, SystemId(0)] {
            game.send(SessionRequest::Load(save.clone()));
            game.world_mut().resource_mut::<LateFocus>().0 = Some(stale);
            game.frames(6);
            let galaxy = game.resource::<Galaxy>();
            let (systems, planets) = (galaxy.systems.len(), galaxy.planets.len());
            assert!(systems < old_systems);
            // Die Karte zeigt die geladene Partie und nur sie.
            assert_eq!(game.count::<Territory>(), systems);
            assert_eq!(game.count::<PlanetRing>(), planets);
            assert_eq!(game.count::<SystemLabel>(), systems);
            let world = game.world_mut();
            assert!(
                world
                    .query::<&Territory>()
                    .iter(world)
                    .all(|territory| territory.0.index() < systems)
            );
            // Der Blick steht wie bei jeder neuen Partie auf der ganzen Galaxie; die Bitte
            // aus der alten Partie hat ihn nicht verschoben.
            assert_eq!(view(&mut game).0, Vec2::ZERO, "Bitte um {stale:?}");
            let chosen = selected(&game).expect("das Heimatsystem ist gewählt");
            assert!(chosen.index() < systems);
        }
        let _ = std::fs::remove_file(&save);

        // Bitten der geladenen Partie gelten wie gewohnt.
        game.send(FocusSystem(SystemId(1)));
        game.frames(2);
        let expected = Vec2::from(game.resource::<Galaxy>().system(SystemId(1)).pos);
        assert!(view(&mut game).0.distance(expected) < 1e-3);
    }

    #[test]
    fn a_press_beside_a_panel_has_one_owner_even_without_a_run_up() {
        use crate::pointer::UiCover;
        use crate::time_control::SimSpeed;

        let mut game = campaign();
        let home = selected(&game);
        // Rechter Rand der linken Tafel: Der Griff zum Größerziehen ragt ein Stück über sie
        // hinaus, auf die Karte.
        let edge = (0..1600)
            .map(|x| x as f32 + 0.5)
            .find(|x| !game.resource::<UiCover>().covers(Vec2::new(*x, 400.0)))
            .expect("freie Karte");
        let on_handle = Vec2::new(edge + 1.5, 400.0);
        // Der Zeiger kommt von der freien Karte und drückt im selben Frame, in dem er
        // eintrifft: Die Oberfläche hatte keine Gelegenheit, den Griff anzukündigen.
        game.move_pointer(FREE_MAP);
        game.frames(3);
        game.move_pointer(on_handle);
        game.press(MouseButton::Left);
        game.frames(1);
        assert_eq!(selected(&game), home, "der Druck auf den Griff wählte ab");
        assert!(
            game.egui().egui_is_using_pointer(),
            "die Tafel hat den Griff nicht gefasst"
        );
        game.release(MouseButton::Left);
        game.frames(2);
        assert_eq!(selected(&game), home);

        // Knapp unter einem Knopf der Kopfleiste, über der freien Karte: Die Oberfläche lässt
        // einen Klick, der einen Knopf um wenige Pixel verfehlt, noch für ihn gelten. Dann
        // gilt er nur ihm.
        let button = game.button("5×").expect("Knopf „5ד");
        let below = (0..60)
            .map(|y| Vec2::new(button.x, button.y + y as f32 + 0.5))
            .find(|at| !game.resource::<UiCover>().covers(*at))
            .expect("Karte unter der Kopfleiste");
        let near_button = below + Vec2::new(0.0, 1.0);
        assert!(game.resource::<SimSpeed>().paused);
        game.move_pointer(FREE_MAP);
        game.frames(3);
        game.click_at_once(near_button);
        assert_eq!(
            *game.resource::<SimSpeed>(),
            SimSpeed {
                paused: false,
                level: 3
            },
            "der Knopf hat den Klick knapp daneben nicht angenommen"
        );
        assert_eq!(selected(&game), home, "derselbe Klick traf auch die Karte");

        // Weit genug von jedem Knopf gehört die Stelle der Karte: Ein schneller Klick ins
        // Leere hebt die Auswahl auf.
        let empty = (0..40)
            .flat_map(|row| (0..60).map(move |column| (column, row)))
            .map(|(column, row)| Vec2::new(480.0 + column as f32 * 13.0, 70.0 + row as f32 * 16.0))
            .find(|at| {
                let point = map_point(&mut game, *at);
                nearest_system(game.resource::<Galaxy>(), point, 60.0).is_none()
            })
            .expect("eine leere Stelle der Karte");
        game.move_pointer(FREE_MAP);
        game.frames(3);
        game.click_at_once(empty);
        assert_eq!(selected(&game), None);
    }

    #[test]
    fn planets_sit_on_their_orbits() {
        let galaxy = galaxy();
        for planet in &galaxy.planets {
            let center = Vec2::from(galaxy.system(planet.system).pos);
            let distance = planet_map_position(&galaxy, planet.id).distance(center);
            assert!((distance - (10.0 + 5.0 * planet.orbit as f32)).abs() < 1e-3);
        }
    }
}

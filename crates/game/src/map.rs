//! Galaxiekarte: Draufsicht mit Systemen, Sprungrouten, Planeten und Besitzfarben.

use crate::session::SessionEpoch;
use crate::state::{AppState, ViewState};
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{CameraOutputMode, ClearColorConfig};
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy::render::render_resource::BlendState;
use bevy::sprite::Anchor;
use bevy::window::PrimaryWindow;
use bevy_egui::input::egui_wants_any_pointer_input;
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
                        camera_controls.run_if(not(egui_wants_any_pointer_input)),
                        focus_camera,
                        pick_system.run_if(not(egui_wants_any_pointer_input)),
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
fn camera_controls(
    time: Res<Time<Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
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

    if buttons.any_pressed([MouseButton::Right, MouseButton::Middle]) {
        transform.translation.x -= motion.delta.x * ortho.scale;
        transform.translation.y += motion.delta.y * ortho.scale;
    }

    let mut direction = Vec2::ZERO;
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
    if direction != Vec2::ZERO {
        let step = direction.normalize() * 600.0 * ortho.scale * time.delta_secs();
        transform.translation += step.extend(0.0);
    }

    if scroll.delta.y != 0.0 {
        let lines = match scroll.unit {
            MouseScrollUnit::Line => scroll.delta.y,
            MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
        };
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

/// Linksklick wählt das nächste System unter dem Mauszeiger.
fn pick_system(
    buttons: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform, &Projection), With<MapCamera>>,
    galaxy: Res<Galaxy>,
    mut selection: ResMut<Selection>,
) {
    if !buttons.just_pressed(MouseButton::Left) {
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

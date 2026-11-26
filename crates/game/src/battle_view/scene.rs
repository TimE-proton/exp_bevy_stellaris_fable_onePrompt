//! Szene der Schlacht: Schiffe, Wracks und Trümmer, Licht und Sternenhimmel.
//!
//! Je Schiff des Gefechts gibt es höchstens ein Entity, angelegt, sobald das Schiff zum
//! ersten Mal auf dem Feld gesehen wird. Meshes und Materialien sind je Klasse und Partei
//! geteilt, damit die Engine gleiche Schiffe gebündelt zeichnet.

use super::camera::{self, BattleCamera, SelectedShip, ViewOptions};
use super::feed::BattleFeed;
use super::meshes::{self, ClassModel, MeshData};
use super::{
    BattleViewSet, BattleViewShown, CameraRequest, Part, ViewFrame, ViewTimings, heading_rotation,
    to_world,
};
use crate::map::MapCamera;
use bevy::camera::visibility::ViewVisibility;
use bevy::camera_controller::free_camera::FreeCameraState;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use sim::battle::{ShipStatus, Wreck};
use sim::data::{Rgb, ShipClass};

/// Nach so vielen Sekunden Gefechtszeit liegen Trümmerstücke still.
const DEBRIS_SETTLE_SECONDS: f32 = 26.0;
const DEBRIS_TAU: f32 = 4.5;
/// Zahl der Helligkeitsstufen eines Einschlags und eines Schildschimmers.
pub const FLASH_LEVELS: usize = 4;
pub const SHIELD_LEVELS: usize = 3;

/// Bauteile einer Schiffsklasse, noch ohne Parteifarbe.
pub struct ClassAssets {
    pub hull: MeshData,
    /// Vorder- und Achterteil: Ein zerstörtes Schiff liegt in zwei Teilen da.
    pub halves: (MeshData, MeshData),
    pub glow: Handle<Mesh>,
}

/// Was jede Schlacht braucht, einmal angelegt.
#[derive(Resource)]
pub struct BattleAssets {
    /// Index wie `ShipClass::index()`.
    pub classes: Vec<ClassAssets>,
    /// Rumpfmaterial nach Zustand: kämpfend, auf dem Rückzug (matter), treibend (dunkel).
    /// Die Farben der Parteien stecken in den Meshes; so teilen sich alle Schiffe drei
    /// Materialien.
    pub hull_materials: [Handle<StandardMaterial>; 3],
    pub tracer: Handle<Mesh>,
    pub missile: Handle<Mesh>,
    pub beam: Handle<Mesh>,
    pub flash: Handle<Mesh>,
    pub shield: Handle<Mesh>,
    pub debris: [Handle<Mesh>; 3],
    pub stars: Handle<Mesh>,
    /// Wrack: glühend, abkühlend, kalt.
    pub wreck: [Handle<StandardMaterial>; 3],
    pub debris_material: Handle<StandardMaterial>,
    pub flash_materials: [Handle<StandardMaterial>; FLASH_LEVELS],
    pub shield_materials: [Handle<StandardMaterial>; SHIELD_LEVELS],
    pub star_material: Handle<StandardMaterial>,
}

/// Leuchtende Farbe ohne Beleuchtung; Werte über 1 strahlen über (Bloom).
fn glowing(color: LinearRgba) -> StandardMaterial {
    StandardMaterial {
        base_color: color.into(),
        unlit: true,
        ..default()
    }
}

/// Wie [`glowing`], aber addiert auf das, was dahinter liegt, und von beiden Seiten zu
/// sehen.
fn additive(color: LinearRgba) -> StandardMaterial {
    StandardMaterial {
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        double_sided: true,
        ..glowing(color)
    }
}

impl BattleAssets {
    pub fn build(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> Self {
        let classes = ShipClass::ALL
            .iter()
            .map(|class| {
                let model = ClassModel::of(*class);
                ClassAssets {
                    halves: model.halves(),
                    glow: meshes.add(model.glow().to_mesh()),
                    hull: model.hull,
                }
            })
            .collect();
        // Sternenhimmel: feste Zufallsfolge, bei jedem Start derselbe Himmel.
        let mut random = Cosmetic::new(0x51a7);
        let stars = meshes::star_field(1800, 1.0, || random.unit());
        let plating = |brightness: f32| StandardMaterial {
            base_color: Color::linear_rgb(brightness, brightness, brightness * 1.03),
            perceptual_roughness: 0.5,
            metallic: 0.25,
            ..default()
        };
        // Verrußt, aber noch so hell, dass ein Wrack aus der Übersicht zu erkennen ist.
        let wreck = |glow: LinearRgba| StandardMaterial {
            base_color: Color::linear_rgb(0.46, 0.4, 0.36),
            emissive: glow,
            perceptual_roughness: 0.85,
            ..default()
        };
        let flash = |color: (f32, f32, f32), strength: f32, alpha: f32| {
            additive(LinearRgba::new(
                color.0 * strength,
                color.1 * strength,
                color.2 * strength,
                alpha,
            ))
        };
        Self {
            classes,
            hull_materials: [
                materials.add(plating(1.0)),
                materials.add(plating(0.4)),
                materials.add(plating(0.2)),
            ],
            tracer: meshes.add(meshes::tracer().to_mesh()),
            missile: meshes.add(meshes::missile().to_mesh()),
            beam: meshes.add(meshes::beam().to_mesh()),
            flash: meshes.add(meshes::glow_disc().to_mesh()),
            shield: meshes.add(meshes::shield_cap().to_mesh()),
            debris: meshes::debris().map(|piece| meshes.add(piece.tinted([1.0; 3]).to_mesh())),
            stars: meshes.add(stars.to_mesh()),
            wreck: [
                materials.add(wreck(LinearRgba::rgb(0.95, 0.24, 0.04))),
                materials.add(wreck(LinearRgba::rgb(0.22, 0.05, 0.008))),
                // Auch ausgekühlt glimmt ein Wrack nach: Seine Schattenseite verschwände
                // sonst vor dem schwarzen Himmel.
                materials.add(wreck(LinearRgba::rgb(0.05, 0.026, 0.014))),
            ],
            debris_material: materials.add(StandardMaterial {
                base_color: Color::linear_rgb(0.52, 0.48, 0.45),
                emissive: LinearRgba::rgb(0.03, 0.018, 0.012),
                perceptual_roughness: 0.8,
                ..default()
            }),
            flash_materials: [
                materials.add(flash((1.0, 0.9, 0.7), 6.0, 1.0)),
                materials.add(flash((1.0, 0.62, 0.26), 3.4, 0.85)),
                materials.add(flash((1.0, 0.36, 0.1), 1.8, 0.6)),
                materials.add(flash((0.9, 0.2, 0.06), 1.0, 0.3)),
            ],
            shield_materials: [
                materials.add(additive(LinearRgba::new(0.3, 0.7, 2.0, 0.22))),
                materials.add(additive(LinearRgba::new(0.3, 0.7, 2.0, 0.13))),
                materials.add(additive(LinearRgba::new(0.3, 0.7, 2.0, 0.06))),
            ],
            star_material: materials.add(glowing(LinearRgba::WHITE)),
        }
    }
}

/// Rumpf-Meshes einer Klasse im Anstrich einer Partei.
pub struct SideClass {
    pub hull: Handle<Mesh>,
    pub halves: (Handle<Mesh>, Handle<Mesh>),
}

/// Was eine Partei an eigenen Meshes und Materialien braucht.
pub struct SideKit {
    /// Farbe der Partei, in der die Bauteile gestrichen sind.
    pub color: Rgb,
    /// Index wie `ShipClass::index()`.
    pub classes: Vec<SideClass>,
    pub glow: Handle<StandardMaterial>,
    pub tracer: Handle<StandardMaterial>,
    pub missile: Handle<StandardMaterial>,
    pub beam_light: Handle<StandardMaterial>,
    pub beam_heavy: Handle<StandardMaterial>,
    pub beam_pd: Handle<StandardMaterial>,
}

/// Meshes und Materialien aller Parteien des gezeigten Gefechts. Sie entstehen mit jedem
/// Neuaufbau der Szene; stößt eine Partei erst im Gefecht dazu, kommt ihre Ausstattung
/// hinzu.
#[derive(Resource, Default)]
pub struct SideAssets {
    /// Zählt jeden Neuaufbau und jedes Abräumen der Szene. Wer Entities für die Szene
    /// führt (die Vorräte der Effekte), erkennt daran, dass seine verschwunden sind.
    pub generation: u32,
    pub sides: Vec<SideKit>,
}

impl SideAssets {
    /// Die Szene wurde abgeräumt: Alle Ausstattungen entfallen.
    fn reset(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.sides.clear();
    }
}

fn mix(a: LinearRgba, b: LinearRgba, t: f32) -> LinearRgba {
    LinearRgba::rgb(
        a.red + (b.red - a.red) * t,
        a.green + (b.green - a.green) * t,
        a.blue + (b.blue - a.blue) * t,
    )
}

fn scaled(color: LinearRgba, factor: f32) -> LinearRgba {
    LinearRgba::rgb(
        color.red * factor,
        color.green * factor,
        color.blue * factor,
    )
}

/// Parteifarbe als Anstrich: linear, auf gleiche Helligkeit gebracht, damit dunkle und
/// helle Parteifarben gleich gut zu sehen sind.
pub fn side_tint(color: Rgb) -> [f32; 3] {
    let linear = LinearRgba::from(Color::srgb(color.0, color.1, color.2));
    let peak = linear.red.max(linear.green).max(linear.blue).max(1e-3);
    [linear.red / peak, linear.green / peak, linear.blue / peak]
}

/// Leuchtfarben einer Partei: Triebwerk, Geschoss, Lenkwaffe, leichter und schwerer Strahl,
/// Punktverteidigung.
fn side_glows(color: Rgb) -> [StandardMaterial; 6] {
    let tint = side_tint(color);
    let side = LinearRgba::rgb(tint[0], tint[1], tint[2]);
    let white = LinearRgba::WHITE;
    [
        additive(scaled(mix(side, white, 0.3), 2.6)),
        glowing(scaled(mix(side, white, 0.4), 9.0)),
        glowing(scaled(mix(side, white, 0.55), 7.0)),
        glowing(scaled(mix(side, white, 0.15), 11.0)),
        glowing(scaled(mix(side, white, 0.45), 20.0)),
        glowing(scaled(mix(side, LinearRgba::rgb(1.0, 0.85, 0.5), 0.6), 6.0)),
    ]
}

impl SideKit {
    pub fn build(
        color: Rgb,
        assets: &BattleAssets,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
    ) -> Self {
        let tint = side_tint(color);
        let classes = assets
            .classes
            .iter()
            .map(|class| SideClass {
                hull: meshes.add(class.hull.tinted(tint).to_mesh()),
                halves: (
                    meshes.add(class.halves.0.tinted(tint).to_mesh()),
                    meshes.add(class.halves.1.tinted(tint).to_mesh()),
                ),
            })
            .collect();
        let [glow, tracer, missile, beam_light, beam_heavy, beam_pd] =
            side_glows(color).map(|material| materials.add(material));
        Self {
            color,
            classes,
            glow,
            tracer,
            missile,
            beam_light,
            beam_heavy,
            beam_pd,
        }
    }

    /// Streicht die Ausstattung in einer anderen Farbe. Meshes und Materialien behalten
    /// ihre Handles: Was schon in der Szene steht, wechselt die Farbe mit.
    pub fn repaint(
        &mut self,
        color: Rgb,
        assets: &BattleAssets,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
    ) {
        self.color = color;
        let tint = side_tint(color);
        for (own, class) in self.classes.iter().zip(&assets.classes) {
            let _ = meshes.insert(&own.hull, class.hull.tinted(tint).to_mesh());
            let _ = meshes.insert(&own.halves.0, class.halves.0.tinted(tint).to_mesh());
            let _ = meshes.insert(&own.halves.1, class.halves.1.tinted(tint).to_mesh());
        }
        let handles = [
            &self.glow,
            &self.tracer,
            &self.missile,
            &self.beam_light,
            &self.beam_heavy,
            &self.beam_pd,
        ];
        for (handle, material) in handles.into_iter().zip(side_glows(color)) {
            let _ = materials.insert(handle, material);
        }
    }
}

/// Einfache Zufallsfolge für rein kosmetische Dinge (Sternenhimmel, Taumeln der Trümmer).
/// Sie hat mit dem Zufall der Simulation nichts zu tun.
#[derive(Clone, Copy, Debug)]
pub struct Cosmetic(u32);

impl Cosmetic {
    pub fn new(seed: u32) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B)
    }

    fn next(&mut self) -> u32 {
        // xorshift32
        let mut x = self.0.max(1);
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// Gleichverteilt in 0..1.
    pub fn unit(&mut self) -> f32 {
        (self.next() >> 8) as f32 / (1u32 << 24) as f32
    }

    /// Gleichverteilt in -1..1.
    pub fn signed(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }

    /// Zufällige Richtung.
    pub fn direction(&mut self) -> Vec3 {
        loop {
            let v = Vec3::new(self.signed(), self.signed(), self.signed());
            let length = v.length_squared();
            if length > 0.01 && length <= 1.0 {
                return v / length.sqrt();
            }
        }
    }
}

/// Alles, was zum gezeigten Gefecht gehört und mit ihm verschwindet (nur Wurzeln).
#[derive(Component)]
pub struct BattleVisual;

/// Licht und Himmel; bestehen, solange die Ansicht gezeigt wird.
#[derive(Component)]
struct SceneProp;

#[derive(Component)]
struct StarField;

/// Aussehen eines Schiffs nach seinem Zustand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Look {
    Active = 0,
    Routing = 1,
    Disabled = 2,
}

impl Look {
    fn of(status: ShipStatus) -> Self {
        match status {
            ShipStatus::Routing => Look::Routing,
            ShipStatus::Disabled => Look::Disabled,
            _ => Look::Active,
        }
    }

    fn material(self, assets: &BattleAssets) -> &Handle<StandardMaterial> {
        &assets.hull_materials[self as usize]
    }
}

#[derive(Component)]
pub struct ShipVisual {
    pub index: u32,
    look: Look,
    /// Schräglage in der Kurve (nur Anschauung).
    roll: f32,
}

#[derive(Component)]
struct EngineGlow {
    ship: u32,
}

#[derive(Component)]
struct WreckRoot {
    index: u32,
}

/// Teil eines Wracks: Rumpf, Rumpfhälfte oder Trümmerstück. Liegt als Kind unter der
/// Wurzel des Wracks und bewegt sich relativ zu ihr.
#[derive(Component)]
struct WreckPart {
    wreck: u32,
    base: Quat,
    axis: Vec3,
    /// Drehrate in rad je Sekunde Gefechtszeit.
    spin: f32,
    /// Wohin das Teil am Ende gedriftet ist (relativ zur Wurzel).
    drift: Vec3,
    /// Rumpfteile taumeln weiter und kühlen ab; Trümmer kommen zur Ruhe.
    hull: bool,
    heat: usize,
    settled: bool,
}

#[derive(Resource, Default)]
pub struct SceneState {
    /// Epoche des Feeds, für die die Szene steht.
    epoch: Option<u32>,
    /// Je Schiff des Gefechts: Steht dafür gerade ein Entity in der Szene? Jedes Frame
    /// bekommt jedes Schiff auf dem Feld, das noch keines hat, seines; so fehlt keines, ob
    /// es von Anfang an da war, als Bordjäger startet oder mit einer Verstärkung eintrifft.
    shown: Vec<bool>,
    wrecks_seen: usize,
}

/// Zählt, was die Engine von der Szene wirklich zeichnet. Maßgeblich ist ihr Urteil aus
/// dem letzten Frame: im Sichtfeld einer eingeschalteten Kamera und nicht ausgeblendet.
/// Der Selbsttest prüft damit, dass die Fotos nicht bloß den leeren Raum zeigen.
#[derive(SystemParam)]
pub struct SceneCensus<'w, 's> {
    hulls: Query<'w, 's, &'static ViewVisibility, With<ShipVisual>>,
    wreck_parts: Query<'w, 's, &'static ViewVisibility, With<WreckPart>>,
}

impl SceneCensus<'_, '_> {
    /// Rümpfe in der Szene, ob die Kamera sie erfasst oder nicht.
    pub fn hulls(&self) -> usize {
        self.hulls.iter().count()
    }

    /// Rümpfe im Sichtfeld der Kamera.
    pub fn visible_hulls(&self) -> usize {
        self.hulls
            .iter()
            .filter(|visibility| visibility.get())
            .count()
    }

    /// Rumpfhälften und Trümmerstücke von Wracks im Sichtfeld der Kamera.
    pub fn visible_wreck_parts(&self) -> usize {
        self.wreck_parts
            .iter()
            .filter(|visibility| visibility.get())
            .count()
    }
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<SideAssets>()
        .init_resource::<SceneState>()
        .add_systems(Startup, setup_assets)
        .add_systems(
            Update,
            (
                prepare_scene,
                sync_ships,
                sync_wrecks,
                animate_wrecks,
                keep_stars_around_camera,
            )
                .chain()
                .in_set(BattleViewSet::Scene),
        )
        .add_systems(Update, clear_idle_scene.run_if(shown_without_battle));
}

fn setup_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(BattleAssets::build(&mut meshes, &mut materials));
}

/// Schlachtansicht einschalten: Schlachtkamera an, Kartenkamera aus, Licht und Himmel.
pub(super) fn show_view(
    mut commands: Commands,
    mut shown: ResMut<BattleViewShown>,
    assets: Res<BattleAssets>,
    mut battle_camera: Single<
        (&mut Camera, &mut FreeCameraState, &mut Transform),
        (With<BattleCamera>, Without<MapCamera>),
    >,
    mut map_camera: Single<&mut Camera, (With<MapCamera>, Without<BattleCamera>)>,
) {
    shown.0 = true;
    let (camera, flight, transform) = &mut *battle_camera;
    camera::set_active(true, camera, flight, transform);
    map_camera.is_active = false;

    // Hauptlicht von schräg oben, dazu ein schwaches, kühles Gegenlicht: So bleiben auch
    // abgewandte Flächen lesbar, und die Rümpfe wirken plastisch.
    commands.spawn((
        SceneProp,
        DirectionalLight {
            color: Color::srgb(1.0, 0.96, 0.9),
            illuminance: 2_700.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(-0.55, 1.0, 0.65).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        SceneProp,
        DirectionalLight {
            color: Color::srgb(0.55, 0.7, 1.0),
            illuminance: 650.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(0.7, -0.5, -0.8).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        SceneProp,
        StarField,
        Mesh3d(assets.stars.clone()),
        MeshMaterial3d(assets.star_material.clone()),
        Transform::from_scale(Vec3::splat(60_000.0)),
    ));
}

/// Schlachtansicht ausschalten: alles zurück, Szene abräumen. Wird sie danach wieder
/// gezeigt, baut sich alles neu auf, auch für dasselbe Gefecht.
pub(super) fn hide_view(
    mut commands: Commands,
    mut shown: ResMut<BattleViewShown>,
    mut state: ResMut<SceneState>,
    mut sides: ResMut<SideAssets>,
    mut selected: ResMut<SelectedShip>,
    mut options: ResMut<ViewOptions>,
    mut battle_camera: Single<
        (&mut Camera, &mut FreeCameraState, &mut Transform),
        (With<BattleCamera>, Without<MapCamera>),
    >,
    mut map_camera: Single<&mut Camera, (With<MapCamera>, Without<BattleCamera>)>,
) {
    shown.0 = false;
    let (camera, flight, transform) = &mut *battle_camera;
    camera::set_active(false, camera, flight, transform);
    map_camera.is_active = true;
    commands.despawn_all::<With<SceneProp>>();
    commands.despawn_all::<With<BattleVisual>>();
    *state = SceneState::default();
    sides.reset();
    selected.0 = None;
    options.follow = false;
}

/// Die Ansicht wird gezeigt, aber der Feed hält kein Gefecht (mehr).
fn shown_without_battle(shown: Res<BattleViewShown>, feed: Res<BattleFeed>) -> bool {
    shown.0 && !feed.is_active()
}

/// Ohne Gefecht bleibt nichts von ihm stehen.
fn clear_idle_scene(
    mut commands: Commands,
    mut state: ResMut<SceneState>,
    mut sides: ResMut<SideAssets>,
    mut selected: ResMut<SelectedShip>,
) {
    if state.epoch.is_none() {
        return;
    }
    commands.despawn_all::<With<BattleVisual>>();
    *state = SceneState::default();
    sides.reset();
    selected.0 = None;
}

/// Bringt die Szene auf den Stand des Feeds, bevor Schiffe, Wracks und Effekte übertragen
/// werden:
/// - Ein anderes Gefecht: alte Szene abräumen, Auswahl und Kamera zurücksetzen.
/// - Lage der Schiffe: Kam das Gefecht erst in diesem Frame in den Feed, gelten die
///   schon berechneten Lagen noch für das alte; sie werden neu berechnet.
/// - Parteien: Jede Partei des Gefechts bekommt ihre Ausstattung, auch eine, die erst
///   mittendrin dazustößt; ändert sich ihre Farbe, wird nachgestrichen.
pub(super) fn prepare_scene(
    mut timings: ResMut<ViewTimings>,
    mut commands: Commands,
    feed: Res<BattleFeed>,
    data: Res<sim::GameData>,
    mut frame: ResMut<ViewFrame>,
    mut state: ResMut<SceneState>,
    mut sides: ResMut<SideAssets>,
    assets: Res<BattleAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut selected: ResMut<SelectedShip>,
    mut options: ResMut<ViewOptions>,
    mut requests: MessageWriter<CameraRequest>,
) {
    let _span = timings.span(Part::Scene);
    let Some(battle) = feed.battle() else {
        return;
    };
    if state.epoch != Some(feed.epoch) {
        commands.despawn_all::<With<BattleVisual>>();
        *state = SceneState {
            epoch: Some(feed.epoch),
            ..default()
        };
        sides.reset();
        selected.0 = None;
        options.follow = false;
        options.summary = true;
        requests.write(CameraRequest::Overview);
    }
    if !frame.matches(&feed) {
        frame.refresh(&feed, 1.0, data.balance.battle.dt);
    }
    let count = battle.combatants.len().max(feed.info.sides.len());
    for side in 0..count {
        let color = feed.side_color(side);
        match sides.sides.get_mut(side) {
            Some(kit) if kit.color != color => {
                kit.repaint(color, &assets, &mut meshes, &mut materials);
            }
            Some(_) => {}
            None => {
                sides
                    .sides
                    .push(SideKit::build(color, &assets, &mut meshes, &mut materials));
            }
        }
    }
}

/// Wie stark sich eine Klasse in die Kurve legt (rad Schräglage je rad/s Drehung) und wie
/// weit höchstens.
fn banking(class: ShipClass) -> (f32, f32) {
    match class {
        ShipClass::Fighter => (0.24, 0.7),
        ShipClass::Corvette => (0.2, 0.4),
        ShipClass::Frigate => (0.12, 0.2),
        ShipClass::Cruiser => (0.07, 0.1),
        ShipClass::Battleship | ShipClass::Carrier => (0.04, 0.05),
    }
}

fn sync_ships(
    mut timings: ResMut<ViewTimings>,
    mut commands: Commands,
    feed: Res<BattleFeed>,
    frame: Res<ViewFrame>,
    assets: Res<BattleAssets>,
    sides: Res<SideAssets>,
    time: Res<Time<Real>>,
    mut state: ResMut<SceneState>,
    mut hulls: Query<
        (
            Entity,
            &mut ShipVisual,
            &mut Transform,
            &mut MeshMaterial3d<StandardMaterial>,
        ),
        Without<EngineGlow>,
    >,
    mut glows: Query<(&EngineGlow, &mut Transform, &mut Visibility), Without<ShipVisual>>,
) {
    let _span = timings.span(Part::Scene);
    let Some(battle) = feed.battle() else {
        return;
    };
    let smoothing = 1.0 - (-7.0 * time.delta_secs()).exp();
    let tick_seconds = frame.tick_seconds.max(1e-3);
    state.shown.resize(battle.ships.len(), false);

    for (entity, mut visual, mut transform, mut material) in &mut hulls {
        let index = visual.index as usize;
        let in_play = battle
            .ships
            .get(index)
            .filter(|ship| ship.status.in_play())
            .zip(frame.poses.get(index));
        let Some((ship, (position, heading))) = in_play else {
            // Zerstört oder entkommen: Das Schiff verlässt die Szene; ein Wrack übernimmt.
            commands.entity(entity).despawn();
            if let Some(shown) = state.shown.get_mut(index) {
                *shown = false;
            }
            continue;
        };
        let (lean, limit) = banking(ship.class);
        let wanted = if ship.status.can_act() {
            (-feed.turn(index) / tick_seconds * lean).clamp(-limit, limit)
        } else {
            0.0
        };
        visual.roll += (wanted - visual.roll) * smoothing;
        transform.translation = *position;
        transform.rotation = heading_rotation(*heading) * Quat::from_rotation_x(visual.roll);
        let look = Look::of(ship.status);
        if look != visual.look {
            visual.look = look;
            material.0 = look.material(&assets).clone();
        }
    }

    for (glow, mut transform, mut visibility) in &mut glows {
        let Some(ship) = battle.ships.get(glow.ship as usize) else {
            continue;
        };
        let throttle = if ship.status.can_act() {
            (ship.speed / ship.max_speed.max(1.0)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let wanted = if throttle > 0.04 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
        if throttle > 0.04 {
            let scale = Vec3::new(
                0.2 + 0.8 * throttle,
                0.75 + 0.25 * throttle,
                0.75 + 0.25 * throttle,
            );
            if transform.scale != scale {
                transform.scale = scale;
            }
        }
    }

    // Jedes Schiff auf dem Feld, das noch kein Entity hat, bekommt seines: anfangs alle,
    // später gestartete Bordjäger und die Schiffe einer Verstärkung. Fehlt dafür noch
    // etwas (Lage oder Ausstattung der Partei), ist das Schiff im nächsten Frame wieder
    // an der Reihe.
    for (index, ship) in battle.ships.iter().enumerate() {
        if state.shown[index] || !ship.status.in_play() {
            continue;
        }
        let (Some(side), Some((position, heading))) =
            (sides.sides.get(ship.side as usize), frame.poses.get(index))
        else {
            continue;
        };
        let class = ship.class.index();
        let look = Look::of(ship.status);
        commands.spawn((
            BattleVisual,
            ShipVisual {
                index: index as u32,
                look,
                roll: 0.0,
            },
            Mesh3d(side.classes[class].hull.clone()),
            MeshMaterial3d(look.material(&assets).clone()),
            Transform {
                translation: *position,
                rotation: heading_rotation(*heading),
                scale: Vec3::splat(ship.radius),
            },
            Visibility::Inherited,
            children![(
                EngineGlow { ship: index as u32 },
                Mesh3d(assets.classes[class].glow.clone()),
                MeshMaterial3d(side.glow.clone()),
                Transform::from_xyz(-1.0, 0.0, 0.0),
                Visibility::Inherited,
            )],
        ));
        state.shown[index] = true;
    }
}

/// Zahl der Trümmerstücke je Klasse.
fn debris_count(class: ShipClass) -> usize {
    match class {
        ShipClass::Fighter => 5,
        ShipClass::Corvette => 7,
        ShipClass::Frigate => 10,
        ShipClass::Cruiser => 15,
        ShipClass::Battleship | ShipClass::Carrier => 22,
    }
}

fn sync_wrecks(
    mut timings: ResMut<ViewTimings>,
    mut commands: Commands,
    feed: Res<BattleFeed>,
    assets: Res<BattleAssets>,
    sides: Res<SideAssets>,
    mut state: ResMut<SceneState>,
    roots: Query<Entity, With<WreckRoot>>,
) {
    let _span = timings.span(Part::Scene);
    let Some(battle) = feed.battle() else {
        return;
    };
    if battle.wrecks.len() < state.wrecks_seen {
        // Der Kern hängt Wracks nur an. Ist die Liste doch kürzer geworden, stimmen die
        // Nummern der vorhandenen nicht mehr: von vorn.
        for entity in &roots {
            commands.entity(entity).despawn();
        }
        state.wrecks_seen = 0;
    }
    while let Some(wreck) = battle.wrecks.get(state.wrecks_seen) {
        // Die Ausstattung jeder Partei des Gefechts liegt vor (`prepare_scene`); ein Wrack
        // ohne gültige Partei kann nie gezeigt werden und wird übergangen.
        if let Some(side) = sides.sides.get(wreck.side as usize) {
            spawn_wreck(
                &mut commands,
                &assets,
                side,
                feed.epoch,
                state.wrecks_seen,
                wreck,
            );
        }
        state.wrecks_seen += 1;
    }
}

/// Legt ein Wrack an: den verrußten Rumpf in zwei auseinandertreibenden Hälften und einen
/// Schwarm Trümmerstücke, der sich über mehrere Rumpflängen ausbreitet und liegen bleibt.
fn spawn_wreck(
    commands: &mut Commands,
    assets: &BattleAssets,
    side: &SideKit,
    epoch: u32,
    index: usize,
    wreck: &Wreck,
) {
    let mut random = Cosmetic::new(epoch.wrapping_mul(7919).wrapping_add(index as u32));
    let class = &side.classes[wreck.class.index()];
    let base = heading_rotation(wreck.heading);
    let forward = base * Vec3::X;
    let radius = wreck.radius;
    let id = index as u32;
    let hull_part = |mesh: Handle<Mesh>, drift: Vec3, random: &mut Cosmetic| {
        (
            WreckPart {
                wreck: id,
                base,
                axis: (random.direction() + forward * 1.5).normalize(),
                spin: 0.03 + 0.1 * random.unit(),
                drift,
                hull: true,
                heat: 0,
                settled: false,
            },
            Mesh3d(mesh),
            MeshMaterial3d(assets.wreck[0].clone()),
            Transform::from_rotation(base).with_scale(Vec3::splat(radius)),
        )
    };
    let mut root = commands.spawn((
        BattleVisual,
        WreckRoot { index: id },
        Transform::from_translation(to_world(wreck.pos)),
        Visibility::default(),
    ));
    // Die Hälften treiben längs auseinander, leicht versetzt.
    let (fore, aft) = &class.halves;
    let apart = radius * (0.38 + 0.2 * random.unit());
    let skew = Vec3::new(0.0, random.signed() * 0.12, random.signed() * 0.1) * radius;
    let fore_part = hull_part(fore.clone(), forward * apart + base * skew, &mut random);
    let aft_part = hull_part(aft.clone(), -forward * apart - base * skew, &mut random);
    root.with_child(fore_part);
    root.with_child(aft_part);
    let count = debris_count(wreck.class);
    for piece in 0..count {
        let mut direction = random.direction();
        direction.y *= 0.55;
        // Die Stücke verteilen sich von dicht am Wrack bis einige Rumpflängen weit; die
        // ersten sind die großen Brocken.
        let spread = (piece as f32 + random.unit()) / count as f32;
        let reach = radius * (1.1 + 5.2 * spread * spread);
        let size = radius * (0.1 + 0.26 * (1.0 - spread) * random.unit().max(0.35));
        let stretch = Vec3::new(1.0, 0.5 + 0.5 * random.unit(), 0.6 + 0.4 * random.unit());
        root.with_child((
            WreckPart {
                wreck: id,
                base: Quat::from_axis_angle(random.direction(), random.unit() * 6.0),
                axis: random.direction(),
                spin: 0.4 + 1.8 * random.unit(),
                drift: direction * reach,
                hull: false,
                heat: 0,
                settled: false,
            },
            Mesh3d(assets.debris[piece % assets.debris.len()].clone()),
            MeshMaterial3d(assets.debris_material.clone()),
            Transform::from_scale(stretch * size),
        ));
    }
}

/// Wie weit ein Teil nach `age` Sekunden auf dem Weg zu seiner Endlage ist (0..1).
fn settle(age: f32, tau: f32) -> f32 {
    1.0 - (-age.max(0.0) / tau).exp()
}

fn animate_wrecks(
    mut timings: ResMut<ViewTimings>,
    feed: Res<BattleFeed>,
    frame: Res<ViewFrame>,
    assets: Res<BattleAssets>,
    mut roots: Query<(&WreckRoot, &mut Transform), Without<WreckPart>>,
    mut parts: Query<(
        &mut WreckPart,
        &mut Transform,
        &mut MeshMaterial3d<StandardMaterial>,
    )>,
) {
    let _span = timings.span(Part::Scene);
    let Some(battle) = feed.battle() else {
        return;
    };
    // Zwischen zwei Takten liegt die Anzeige noch ein Stück vor dem letzten Takt.
    let back = if battle.finished {
        0.0
    } else {
        (1.0 - frame.alpha) * frame.tick_seconds
    };
    for (root, mut transform) in &mut roots {
        let Some(wreck) = battle.wrecks.get(root.index as usize) else {
            continue;
        };
        let position = to_world(wreck.pos - wreck.vel * back);
        if transform.translation != position {
            transform.translation = position;
        }
    }
    for (mut part, mut transform, mut material) in &mut parts {
        if part.settled {
            continue;
        }
        let Some(wreck) = battle.wrecks.get(part.wreck as usize) else {
            continue;
        };
        let age = (wreck.age - back).max(0.0);
        if part.hull {
            transform.translation = part.drift * settle(age, 7.0);
            transform.rotation = Quat::from_axis_angle(part.axis, part.spin * age) * part.base;
            let heat = if age < 2.5 {
                0
            } else if age < 8.0 {
                1
            } else {
                2
            };
            if heat != part.heat {
                part.heat = heat;
                material.0 = assets.wreck[heat].clone();
            }
        } else {
            let progress = settle(age, DEBRIS_TAU);
            transform.translation = part.drift * progress;
            transform.rotation =
                Quat::from_axis_angle(part.axis, part.spin * DEBRIS_TAU * progress) * part.base;
            if age > DEBRIS_SETTLE_SECONDS {
                part.settled = true;
            }
        }
    }
}

/// Der Himmel ist unendlich fern: Er wandert mit der Kamera mit.
fn keep_stars_around_camera(
    camera: Single<&Transform, (With<BattleCamera>, Without<StarField>)>,
    mut stars: Query<&mut Transform, With<StarField>>,
) {
    for mut transform in &mut stars {
        if transform.translation != camera.translation {
            transform.translation = camera.translation;
        }
    }
}

#[cfg(test)]
pub mod testing {
    use super::*;

    /// Assets ohne Fenster und Grafikkarte.
    pub fn assets() -> (BattleAssets, Assets<Mesh>, Assets<StandardMaterial>) {
        let mut meshes = Assets::<Mesh>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let assets = BattleAssets::build(&mut meshes, &mut materials);
        (assets, meshes, materials)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assets_share_handles_per_class_and_side() {
        let (assets, mut meshes, mut materials) = testing::assets();
        assert_eq!(assets.classes.len(), ShipClass::ALL.len());
        for class in ShipClass::ALL {
            let entry = &assets.classes[class.index()];
            assert!(meshes.get(&entry.glow).is_some());
            assert!(entry.halves.0.triangle_count() > 0 && entry.halves.1.triangle_count() > 0);
        }
        let (mesh_count, material_count) = (meshes.len(), materials.len());
        let side = SideKit::build((0.2, 0.6, 1.0), &assets, &mut meshes, &mut materials);
        // Je Partei ein Rumpf und zwei Hälften je Klasse, dazu sechs Leuchtfarben. Mehr
        // entsteht nicht, egal wie viele Schiffe kämpfen.
        assert_eq!(meshes.len(), mesh_count + 6 + 2 * 6);
        assert_eq!(materials.len(), material_count + 6);
        for class in ShipClass::ALL {
            let entry = &side.classes[class.index()];
            assert!(meshes.get(&entry.hull).is_some());
            assert!(meshes.get(&entry.halves.0).is_some());
            assert!(meshes.get(&entry.halves.1).is_some());
        }
        // Fliehende und treibende Schiffe sind dunkler als kämpfende.
        let brightness = |handle: &Handle<StandardMaterial>| {
            let color = LinearRgba::from(materials.get(handle).unwrap().base_color);
            color.red + color.green + color.blue
        };
        let [active, routing, disabled] = &assets.hull_materials;
        assert!(brightness(routing) < brightness(active) * 0.6);
        assert!(brightness(disabled) < brightness(routing));
        // Leuchtendes strahlt über: Werte über 1 für den Bloom.
        assert!(brightness(&side.beam_heavy) > 20.0);
    }

    #[test]
    fn side_colour_shows_in_the_hull_paint() {
        use bevy::mesh::VertexAttributeValues;
        let (assets, mut meshes, mut materials) = testing::assets();
        let blue = SideKit::build((0.2, 0.5, 1.0), &assets, &mut meshes, &mut materials);
        let red = SideKit::build((1.0, 0.3, 0.2), &assets, &mut meshes, &mut materials);
        let colors = |handle: &Handle<Mesh>| {
            let Some(VertexAttributeValues::Float32x4(colors)) =
                meshes.get(handle).unwrap().attribute(Mesh::ATTRIBUTE_COLOR)
            else {
                panic!("Farben fehlen");
            };
            colors.clone()
        };
        for class in ShipClass::ALL {
            let (blue, red) = (
                colors(&blue.classes[class.index()].hull),
                colors(&red.classes[class.index()].hull),
            );
            assert_eq!(blue.len(), red.len());
            // Jeder Rumpf trägt Abzeichen in voller Parteifarbe und blanke, dunkle Teile,
            // die bei beiden Parteien gleich aussehen.
            let bluish = blue.iter().filter(|c| c[2] > 0.8 && c[0] < 0.2).count();
            let reddish = red.iter().filter(|c| c[0] > 0.8 && c[2] < 0.2).count();
            assert!(
                bluish >= 12 && bluish == reddish,
                "{}: {bluish} / {reddish}",
                class.label()
            );
            let same = blue.iter().zip(&red).filter(|(a, b)| a == b).count();
            assert!(same >= 12, "{}: keine neutralen Teile", class.label());
            assert!(
                blue.iter().chain(&red).all(|c| c[3] == 1.0),
                "Rümpfe sind deckend"
            );
        }
        assert_eq!(side_tint((0.5, 0.5, 0.5)), [1.0, 1.0, 1.0]);
        let tint = side_tint((1.0, 0.4, 0.3));
        assert!(tint[0] == 1.0 && tint[1] < 0.2 && tint[2] < tint[1]);
    }

    fn luminance(color: LinearRgba) -> f32 {
        0.2126 * color.red + 0.7152 * color.green + 0.0722 * color.blue
    }

    #[test]
    fn wrecks_and_debris_are_bright_enough_to_be_seen() {
        use bevy::mesh::VertexAttributeValues;
        let (assets, meshes, materials) = testing::assets();
        let material = |handle: &Handle<StandardMaterial>| materials.get(handle).unwrap();
        let [hot, cooling, cold] = [0, 1, 2].map(|stage| material(&assets.wreck[stage]));
        // Ein ausgekühltes Wrack wirft noch einen guten Teil des Lichts zurück und glimmt
        // aus eigener Kraft: Auch seine Schattenseite hebt sich vom schwarzen Himmel ab.
        assert!(luminance(cold.base_color.into()) >= 0.35);
        assert!(luminance(cold.emissive) >= 0.02);
        // Es bleibt dunkler als ein Schiff, das noch kämpft, und kühlt sichtbar ab.
        let alive = material(&assets.hull_materials[0]);
        assert!(luminance(cold.base_color.into()) < 0.6 * luminance(alive.base_color.into()));
        assert!(luminance(hot.emissive) > 3.0 * luminance(cooling.emissive));
        assert!(luminance(cooling.emissive) > 1.5 * luminance(cold.emissive));
        // Trümmer ebenso: helles Material, kein Stück fast schwarz gestrichen.
        let debris = material(&assets.debris_material);
        assert!(luminance(debris.base_color.into()) >= 0.4);
        assert!(luminance(debris.emissive) >= 0.01);
        for handle in &assets.debris {
            let Some(VertexAttributeValues::Float32x4(colors)) =
                meshes.get(handle).unwrap().attribute(Mesh::ATTRIBUTE_COLOR)
            else {
                panic!("Farben fehlen");
            };
            let darkest = colors
                .iter()
                .map(|c| luminance(LinearRgba::rgb(c[0], c[1], c[2])))
                .fold(f32::MAX, f32::min);
            assert!(darkest >= 0.25, "Trümmerstück mit Anstrich {darkest}");
        }
    }

    #[test]
    fn every_wreck_breaks_apart_and_scatters_debris() {
        let (assets, mut meshes, mut materials) = testing::assets();
        let side = SideKit::build((0.3, 0.6, 1.0), &assets, &mut meshes, &mut materials);
        for class in ShipClass::ALL {
            let radius = 20.0;
            let mut world = World::new();
            // Mehrere Wracks derselben Klasse: Die Trümmer fallen jedes Mal anders.
            for index in 0..6 {
                let wreck = Wreck {
                    pos: sim::battle::V2::new(100.0 * index as f32, 0.0),
                    vel: sim::battle::V2::ZERO,
                    heading: 0.4,
                    class,
                    side: 0,
                    radius,
                    age: 0.0,
                };
                let mut commands = world.commands();
                spawn_wreck(&mut commands, &assets, &side, 1, index, &wreck);
                world.flush();
            }
            let mut parts = world.query::<(&WreckPart, &Transform)>();
            let hulls: Vec<_> = parts.iter(&world).filter(|(part, _)| part.hull).collect();
            let debris: Vec<_> = parts.iter(&world).filter(|(part, _)| !part.hull).collect();
            // Jedes Wrack, auch das kleinste, liegt in zwei Teilen da: So ist es auch aus
            // der Nähe nicht mit einem Schiff zu verwechseln, das noch fährt.
            assert_eq!(hulls.len(), 6 * 2, "{}", class.label());
            for (part, _) in &hulls {
                assert!(
                    part.drift.length() > 0.2 * radius,
                    "die Hälften treiben auseinander"
                );
            }
            // Dazu ein Trümmerfeld: etliche Stücke, die größten gut zu sehen, verteilt
            // über mehrere Rumpflängen (eine Rumpflänge sind zwei Halbmesser).
            assert!(debris.len() >= 6 * 5, "{}: {}", class.label(), debris.len());
            let reach = debris
                .iter()
                .map(|(part, _)| part.drift.length())
                .fold(0.0, f32::max);
            assert!(
                reach >= 4.0 * radius,
                "{}: Trümmer bis {reach}",
                class.label()
            );
            let largest = debris
                .iter()
                .map(|(_, transform)| transform.scale.max_element())
                .fold(0.0, f32::max);
            assert!(
                largest >= 0.25 * radius,
                "{}: größtes Stück {largest}",
                class.label()
            );
            let smallest = debris
                .iter()
                .map(|(_, transform)| transform.scale.max_element())
                .fold(f32::MAX, f32::min);
            assert!(
                smallest >= 0.09 * radius,
                "{}: kleinstes Stück {smallest}",
                class.label()
            );
        }
    }

    #[test]
    fn cosmetic_random_is_repeatable_and_bounded() {
        let mut a = Cosmetic::new(42);
        let mut b = Cosmetic::new(42);
        let mut sum = 0.0;
        for _ in 0..2000 {
            let value = a.unit();
            assert_eq!(value, b.unit());
            assert!((0.0..1.0).contains(&value));
            sum += value;
        }
        assert!((sum / 2000.0 - 0.5).abs() < 0.05);
        for _ in 0..100 {
            assert!((a.direction().length() - 1.0).abs() < 1e-4);
        }
        assert_ne!(Cosmetic::new(1).unit(), Cosmetic::new(2).unit());
    }

    #[test]
    fn debris_comes_to_rest() {
        assert_eq!(settle(0.0, DEBRIS_TAU), 0.0);
        assert!(settle(DEBRIS_TAU, DEBRIS_TAU) > 0.6);
        // Wenn ein Trümmerstück als ruhend gilt, fehlt ihm weniger als ein halbes Prozent
        // seines Wegs: Das Anhalten ist nicht zu sehen.
        assert!(settle(DEBRIS_SETTLE_SECONDS, DEBRIS_TAU) > 0.995);
    }
}

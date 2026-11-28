//! Szene der Orbitansicht: Planet, Schild, Stationen, Geschütze, Zonen der Oberfläche, die
//! Flotten des Belagerers und die Schaustellung von Beschuss, Gegenfeuer und Landung.
//!
//! Aufgestellt wird nach [`Layout`]: Ändert es sich (ein Schiff weniger, eine Zone
//! genommen, der Schild gebrochen), baut sich die Aufstellung neu auf. Geschosse und
//! Landungsboote sind kurzlebige Entities; wie viele fliegen, folgt dem Tagesbericht des
//! Kerns. Ihre Zufallsfolge ist rein kosmetisch und hat mit der Simulation nichts zu tun.

use super::super::BattleViewShown;
use super::super::camera::{self, BattleCamera, FLY_SPEED_MAX, FLY_SPEED_MIN};
use super::super::meshes;
use super::super::overlay::BattleGizmos;
use super::super::scene::{
    BattleAssets, Cosmetic, Lighting, SideKit, additive, show_lit, side_tint,
};
use super::atmosphere::AtmosphereMaterial;
use super::planet_look::{self, LookInput, Surface};
use super::{Layout, WatchedSiege};
use crate::map::{MapCamera, camera_shows};
use crate::state::ViewState;
use bevy::camera::visibility::VisibleEntities;
use bevy::camera_controller::free_camera::FreeCameraState;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use bevy_egui::input::{EguiWantsInput, egui_wants_any_keyboard_input};
use sim::GameData;
use sim::clock::SimClock;
use sim::data::ShipClass;
use sim::galaxy::Galaxy;
use sim::siege::{Fire, Phase};
use std::f32::consts::TAU;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Halbmesser des Planeten in der Szene. Die Schiffe behalten ihre Größe aus der
/// Schlachtansicht; die Welt ist dagegen klein gehalten, damit beides ins Bild passt.
pub const PLANET_RADIUS: f32 = 420.0;
/// Der Schild liegt als Schale über der Oberfläche.
pub const SHIELD_RADIUS: f32 = PLANET_RADIUS * 1.09;
/// Höhe der Wolkendecke und der Zonen über der Oberfläche (Anteil am Halbmesser). Die
/// Zonen liegen über den Wolken: Sie sind Lagebild, nicht Gelände.
const CLOUD_LIFT: f32 = 1.012;
const ZONE_LIFT: f32 = 1.015;
/// So schnell dreht sich die Wolkendecke (Bogenmaß je Sekunde Szenenzeit).
const CLOUD_DRIFT: f32 = 0.006;
/// Stärke des Leuchtens: Glut und Stadtlichter (direkt in Bildwerten, ohne Belichtung).
const LAVA_GLOW: f32 = 2.6;
const CITY_GLOW: f32 = 0.32;
/// So viele Oberflächen behält das Spiel, damit ein zweiter Besuch sofort fertig ist.
const KEPT_LOOKS: usize = 3;
/// Bahn der Orbitalstationen.
const STATION_ORBIT: f32 = PLANET_RADIUS * 1.34;
const STATION_SIZE: f32 = 46.0;
/// Weiter als das entfernt sich die Kamera nicht vom Planeten.
const MAX_DISTANCE: f32 = 9_000.0;
/// Breite der Aufstellung der Flotte.
const FLEET_WIDTH: f32 = 860.0;
/// Die Rümpfe stehen größer da als in der Schlachtansicht: Neben einer Welt wären sie in
/// ihrem Gefechtsmaß kaum zu sehen.
const HULL_SCALE: f32 = 3.0;
/// Winkelabstand der Zonen von der Mitte des Landegebiets.
const ZONE_RING: f32 = 0.56;
/// Boote einer Landungswelle, ihre Flugzeit in Sekunden Szenenzeit und der Abstand, in
/// dem sie starten.
pub const LANDERS: usize = 14;
const LANDER_FLIGHT: f32 = 6.0;
const LANDER_GAP: f32 = 0.3;
const LANDER_SIZE: f32 = 26.0;
/// Kürzel der Taste „Übersicht“, dieselbe wie in der Schlachtansicht.
const KEY_OVERVIEW: KeyCode = KeyCode::KeyU;

/// Richtung vom Planeten zur Flotte des Belagerers.
pub fn attack_direction() -> Vec3 {
    Vec3::new(0.42, 0.3, 0.86).normalize()
}

/// Richtung der Mitte des Landegebiets: unter der Flotte, ein Stück zur Übersicht hin
/// gedreht, damit die Zonen von dort ganz zu sehen sind.
pub fn landing_direction() -> Vec3 {
    Quat::from_rotation_y(-0.34) * attack_direction()
}

/// Zwei Richtungen quer zu `direction`: nach Osten und nach Norden.
fn tangents(direction: Vec3) -> (Vec3, Vec3) {
    let east = Vec3::Y.cross(direction).normalize_or(Vec3::X);
    (east, direction.cross(east))
}

/// Richtung im Winkelabstand `away` von `center`, im Umlauf bei `around`.
fn offset_direction(center: Vec3, away: f32, around: f32) -> Vec3 {
    let (east, north) = tangents(center);
    (center * away.cos() + (east * around.cos() + north * around.sin()) * away.sin()).normalize()
}

/// Richtung der Mitte einer Zone: Die Zonen liegen im Kreis um die Mitte des Landegebiets.
pub fn zone_direction(index: usize, count: usize) -> Vec3 {
    let center = landing_direction();
    if count <= 1 {
        return center;
    }
    offset_direction(center, ZONE_RING, TAU * index as f32 / count as f32 + 0.4)
}

/// Halbmesser der Scheibe einer Zone nach ihrem Anteil an der Welt.
pub fn zone_radius(share: f32) -> f32 {
    PLANET_RADIUS * (0.1 + 0.3 * share.max(0.0).sqrt()).clamp(0.1, 0.25)
}

/// Wie hoch die Flotte über der Welt steht: Sie rückt mit den Phasen näher und zieht sich
/// zurück, solange der Orbit umstritten ist.
pub fn fleet_altitude(phase: Phase, control: bool) -> f32 {
    if !control {
        return PLANET_RADIUS * 2.8;
    }
    PLANET_RADIUS
        * match phase {
            Phase::Approach => 2.5,
            Phase::Investment => 2.25,
            Phase::Bombardment => 1.95,
            Phase::Ground | Phase::Underground => 1.8,
        }
}

/// Größe eines Rumpfs je Klasse aus den Spieldaten (Index wie `ShipClass::index()`).
fn class_radii(data: &GameData) -> [f32; 6] {
    let mut radii = [3.0, 8.0, 12.0, 20.0, 32.0, 36.0];
    // Die kleinsten Rümpfe stehen noch etwas größer da, sonst verschwänden sie.
    let floor = 7.0;
    for class in ShipClass::ALL {
        if let Some(design) = data
            .designs
            .iter()
            .find(|design| design.stats.class == class)
        {
            radii[class.index()] = design.stats.radius.max(floor);
        }
    }
    radii.map(|radius| radius * HULL_SCALE)
}

/// Plätze der gezeigten Rümpfe: Klasse und Ort. Die großen Schiffe stehen in der Mitte,
/// die kleinen davor und dahinter, in Reihen quer zur Bahn.
pub fn fleet_slots(layout: &Layout, radii: [f32; 6]) -> Vec<(ShipClass, Vec3)> {
    let altitude = fleet_altitude(layout.phase, layout.control);
    let center = attack_direction();
    let (east, north) = tangents(center);
    let order = [
        ShipClass::Fighter,
        ShipClass::Corvette,
        ShipClass::Cruiser,
        ShipClass::Carrier,
        ShipClass::Battleship,
        ShipClass::Frigate,
    ];
    // Erst die Reihen, dann ihre Lage: So steht die Aufstellung mittig über dem Landegebiet.
    let mut rows: Vec<(ShipClass, u32, f32)> = Vec::new();
    for class in order {
        let count = layout.hulls[class.index()];
        let spacing = radii[class.index()] * 3.0;
        let per_row = ((FLEET_WIDTH / spacing).floor() as u32).max(1);
        let mut left = count;
        while left > 0 {
            let here = left.min(per_row);
            rows.push((class, here, spacing));
            left -= here;
        }
    }
    let depth: f32 = rows.iter().map(|row| row.2 * 0.85).sum();
    let mut slots = Vec::new();
    let mut at = -depth / 2.0;
    for (class, count, spacing) in rows {
        at += spacing * 0.425;
        for index in 0..count {
            let across = (index as f32 - (count - 1) as f32 / 2.0) * spacing;
            let flat = center * altitude + east * across + north * at;
            slots.push((class, flat.normalize() * altitude));
        }
        at += spacing * 0.425;
    }
    slots
}

/// Lage eines Körpers über der Welt: Bug entlang der Bahn nach Osten, Oberseite von der
/// Welt weg.
fn orbital_rotation(position: Vec3) -> Quat {
    let up = position.normalize_or(Vec3::Y);
    let (east, _) = tangents(up);
    Quat::from_mat3(&Mat3::from_cols(east, up, east.cross(up)))
}

/// Ort einer Station zur Szenenzeit `time`: gleichmäßig auf einer geneigten Bahn verteilt,
/// langsam umlaufend.
pub fn station_position(index: u32, count: u32, time: f32) -> Vec3 {
    let axis = Vec3::new(0.3, 1.0, -0.12).normalize();
    let (east, north) = tangents(axis);
    let angle = 1.1 + TAU * index as f32 / count.max(1) as f32 + 0.045 * time;
    (east * angle.cos() + north * angle.sin()) * STATION_ORBIT
}

/// Richtung eines Bodengeschützes: rings um das Landegebiet, außerhalb der Zonen.
fn gun_direction(index: u32) -> Vec3 {
    let around = 2.1 + index as f32 * 2.399_963;
    let away = 0.98 + 0.11 * (index % 3) as f32;
    offset_direction(landing_direction(), away, around)
}

/// Richtung eines Bunkers: in der Mitte des Landegebiets.
fn bunker_direction(index: u32) -> Vec3 {
    offset_direction(landing_direction(), 0.17, 0.9 + index as f32 * 2.399_963)
}

/// Richtung zur Sonne der Orbitansicht: seitlich hinter der Übersicht, damit die
/// Tag-Nacht-Grenze über die Scheibe läuft und das Landegebiet im Licht liegt.
pub fn sun_direction() -> Vec3 {
    (Quat::from_rotation_y(0.9) * attack_direction() + Vec3::Y * 0.55).normalize()
}

/// Licht der Orbitansicht: eine kräftige Sonne, kaum Gegen- und Umgebungslicht. So ist
/// die Nachtseite wirklich dunkel, und Glut und Stadtlichter leuchten darin.
pub fn orbit_lighting() -> Lighting {
    let sun = sun_direction();
    Lighting {
        key: (sun, 4_000.0),
        key_color: Color::srgb(1.0, 0.985, 0.96),
        fill: ((-sun + Vec3::new(0.0, -0.3, 0.2)).normalize(), 70.0),
        ambient: 22.0,
    }
}

/// Orbitansicht einschalten: wie die Schlachtansicht, aber mit eigenem Licht.
pub(super) fn show_view(
    commands: Commands,
    shown: ResMut<BattleViewShown>,
    assets: Res<BattleAssets>,
    battle_camera: Single<
        (
            &mut Camera,
            &mut FreeCameraState,
            &mut Transform,
            &mut AmbientLight,
        ),
        (With<BattleCamera>, Without<MapCamera>),
    >,
    map_camera: Single<&mut Camera, (With<MapCamera>, Without<BattleCamera>)>,
) {
    show_lit(
        orbit_lighting(),
        commands,
        shown,
        assets,
        battle_camera,
        map_camera,
    );
}

/// Kriegsschäden der Welt, 0..1.
fn scar_of(watched: &WatchedSiege) -> f32 {
    (watched.card.view.defense.scar as f32).clamp(0.0, 1.0)
}

/// Wie weit die Oberfläche der gezeigten Welt ist.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum LookStage {
    None,
    Preview,
    Full,
}

/// Texturen einer fertigen Oberfläche.
#[derive(Clone)]
struct LookImages {
    albedo: Handle<Image>,
    normal: Option<Handle<Image>>,
    roughness: Handle<Image>,
    emissive: Option<Handle<Image>>,
    clouds: Option<Handle<Image>>,
}

/// Eine fertige Oberfläche auf dem Weg zur Grafikkarte: eine Textur je Frame, damit kein
/// einzelner Frame alles auf einmal hochlädt.
struct Staging {
    input: LookInput,
    stage: LookStage,
    /// Noch hochzuladen: Platz (Farbe, Normalen, Rauheit, Leuchten, Wolken) und Bild.
    layers: Vec<(usize, planet_look::Layer)>,
    handles: [Option<Handle<Image>>; 5],
}

impl Staging {
    fn new(input: LookInput, stage: LookStage, surface: Surface) -> Self {
        let layers = [
            Some(surface.albedo),
            surface.normal,
            Some(surface.roughness),
            surface.emissive,
            surface.clouds,
        ]
        .into_iter()
        .enumerate()
        .filter_map(|(slot, layer)| layer.map(|layer| (slot, layer)))
        .collect();
        Self {
            input,
            stage,
            layers,
            handles: Default::default(),
        }
    }

    /// Lädt das nächste Bild hoch; liefert die Texturen, sobald alle da sind.
    fn step(&mut self, images: &mut Assets<Image>) -> Option<LookImages> {
        if let Some((slot, layer)) = self.layers.pop() {
            self.handles[slot] = Some(images.add(layer.image()));
            return None;
        }
        let [albedo, normal, roughness, emissive, clouds] = self.handles.clone();
        Some(LookImages {
            albedo: albedo?,
            normal,
            roughness: roughness?,
            emissive,
            clouds,
        })
    }
}

/// Eine Oberfläche in Arbeit, abseits des Hauptfadens.
struct LookJob {
    input: LookInput,
    stage: LookStage,
    task: Task<Option<Surface>>,
    cancel: Arc<AtomicBool>,
}

/// Oberflächen der zuletzt gezeigten Welten und die, die gerade entsteht. Bleibt über
/// die Besuche der Orbitansicht hinweg bestehen: Ein zweiter Besuch ist sofort fertig.
#[derive(Resource, Default)]
pub struct PlanetLooks {
    ready: Vec<(LookInput, LookStage, LookImages)>,
    job: Option<LookJob>,
    staging: Option<Staging>,
}

impl PlanetLooks {
    fn best(&self, input: &LookInput) -> Option<(LookStage, LookImages)> {
        self.ready
            .iter()
            .filter(|entry| entry.0 == *input)
            .max_by_key(|entry| entry.1)
            .map(|entry| (entry.1, entry.2.clone()))
    }

    fn keep(&mut self, input: LookInput, stage: LookStage, images: LookImages) {
        self.ready.retain(|entry| entry.0 != input);
        self.ready.push((input, stage, images));
        if self.ready.len() > KEPT_LOOKS {
            self.ready.remove(0);
        }
    }

    fn start(&mut self, input: LookInput, stage: LookStage) {
        self.stop();
        let cancel = Arc::new(AtomicBool::new(false));
        let size = if stage == LookStage::Full {
            planet_look::FULL
        } else {
            planet_look::PREVIEW
        };
        let task = {
            let (input, cancel) = (input.clone(), cancel.clone());
            AsyncComputeTaskPool::get()
                .spawn(async move { planet_look::generate(&input, size, &cancel) })
        };
        self.job = Some(LookJob {
            input,
            stage,
            task,
            cancel,
        });
    }

    fn stop(&mut self) {
        self.staging = None;
        if let Some(job) = self.job.take() {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }
}

impl Drop for PlanetLooks {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Bringt die Oberfläche der gezeigten Welt voran: erst ein schnelles Vorabbild, dann
/// die volle Auflösung; beides entsteht abseits des Hauptfadens. Dreht die Wolken und
/// dunkelt die Oberfläche nach den Kriegsschäden ab.
fn update_look(
    watched: Res<WatchedSiege>,
    scene: Option<ResMut<OrbitScene>>,
    mut looks: ResMut<PlanetLooks>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut clouds: Query<(&mut Transform, &mut Visibility), With<CloudLayer>>,
) {
    let Some(mut scene) = scene else {
        return;
    };
    for (mut transform, _) in &mut clouds {
        transform.rotation = Quat::from_rotation_y(scene.time * CLOUD_DRIFT);
    }
    // Fertige Arbeit abholen.
    let finished = looks.job.as_mut().is_some_and(|job| job.task.is_finished());
    if finished
        && let Some(job) = looks.job.take()
        && let Some(Some(surface)) = block_on(poll_once(job.task))
    {
        looks.staging = Some(Staging::new(job.input, job.stage, surface));
    }
    if let Some(staging) = looks.staging.as_mut()
        && let Some(ready) = staging.step(&mut images)
        && let Some(staging) = looks.staging.take()
    {
        looks.keep(staging.input, staging.stage, ready);
    }
    let input = scene.look.clone();
    let best = looks.best(&input);
    let have = best.as_ref().map_or(LookStage::None, |best| best.0);
    let working = looks.job.as_ref().is_some_and(|job| job.input == input)
        || looks
            .staging
            .as_ref()
            .is_some_and(|staging| staging.input == input);
    if have < LookStage::Full && !working {
        let next = if have == LookStage::None {
            LookStage::Preview
        } else {
            LookStage::Full
        };
        looks.start(input, next);
    }
    let scar = scar_of(&watched);
    if let Some((stage, images)) = best
        && (stage > scene.shown_look || scar != scene.scar)
    {
        scene.shown_look = stage;
        scene.scar = scar;
        let dark = 1.0 - 0.45 * scar;
        if let Some(mut material) = materials.get_mut(&scene.planet_material) {
            material.base_color = Color::linear_rgb(dark, dark, dark);
            material.base_color_texture = Some(images.albedo.clone());
            material.normal_map_texture = images.normal.clone();
            material.metallic_roughness_texture = Some(images.roughness.clone());
            material.perceptual_roughness = 1.0;
            let glow = if scene.look.style == planet_look::Style::Lava {
                LAVA_GLOW
            } else {
                CITY_GLOW
            };
            material.emissive = if images.emissive.is_some() {
                LinearRgba::new(glow, glow, glow, 0.0)
            } else {
                LinearRgba::BLACK
            };
            material.emissive_texture = images.emissive.clone();
        }
        if let Some(texture) = images.clouds.clone() {
            if let Some(mut material) = materials.get_mut(&scene.cloud_material) {
                material.base_color_texture = Some(texture);
            }
            for (_, mut visibility) in &mut clouds {
                *visibility = Visibility::Inherited;
            }
        }
    }
}

fn linear(color: sim::data::Rgb) -> LinearRgba {
    let tint = side_tint(color);
    LinearRgba::rgb(tint[0], tint[1], tint[2])
}

/// Alles, was zur Orbitansicht gehört und mit ihr verschwindet.
#[derive(Component)]
pub struct OrbitVisual;

/// Teil der Aufstellung; verschwindet, wenn sie neu aufgebaut wird.
#[derive(Component)]
struct Prop;

#[derive(Component)]
struct PlanetBody;

/// Wolkendecke über dem Planeten; dreht sich langsam mit der Szenenzeit.
#[derive(Component)]
struct CloudLayer;

/// Saum der Atmosphäre.
#[derive(Component)]
struct HazeShell;

#[derive(Component)]
pub struct OrbitHull;

#[derive(Component)]
struct Station(u32, u32);

#[derive(Component)]
struct ShieldShell;

/// Wo ein Schuss endet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hit {
    Shield,
    Surface,
    Hull,
}

#[derive(Component)]
struct Shot {
    from: Vec3,
    to: Vec3,
    age: f32,
    ttl: f32,
    beam: bool,
    hit: Hit,
}

#[derive(Component)]
struct Flash {
    age: f32,
    ttl: f32,
    size: f32,
}

#[derive(Component)]
struct ShieldFlash {
    age: f32,
    ttl: f32,
}

/// Landungsboot auf dem Weg von der Flotte zur Oberfläche.
#[derive(Component)]
pub struct Lander {
    from: Vec3,
    to: Vec3,
    age: f32,
    ttl: f32,
}

/// Meshes und Materialien der gezeigten Belagerung und der Stand der Schaustellung.
#[derive(Resource)]
pub struct OrbitScene {
    built: Option<Layout>,
    attacker: SideKit,
    defender: SideKit,
    station: Handle<Mesh>,
    gun: Handle<Mesh>,
    bunker: Handle<Mesh>,
    shell: Handle<Mesh>,
    /// Aussehen der Welt, Material ihrer Oberfläche und ihrer Wolken.
    look: LookInput,
    planet_material: Handle<StandardMaterial>,
    cloud_material: Handle<StandardMaterial>,
    /// Gezeigte Stufe der Oberfläche: keine, Vorabbild, fertig.
    shown_look: LookStage,
    /// Kriegsschäden, wie sie die Oberfläche gerade abdunkeln.
    scar: f32,
    /// Zonen in der Hand der Verteidiger und des Belagerers.
    zone_materials: [Handle<StandardMaterial>; 2],
    shield_material: Handle<StandardMaterial>,
    radii: [f32; 6],
    /// Szenenzeit in Sekunden; steht, wenn die Partie steht.
    pub time: f32,
    random: Cosmetic,
    /// Angesparte Bruchteile: Beschuss, Gegenfeuer, Bodenkampf, Untergrund.
    budget: [f32; 4],
    landings_shown: u32,
    /// Orte der gezeigten Rümpfe und ihre Größe.
    pub hulls: Vec<(Vec3, f32)>,
    /// Richtungen der Geschütze und Zonen (Mitte, Halbmesser).
    guns: Vec<Vec3>,
    pub zones: Vec<(Vec3, f32)>,
    /// Die laufende Landungswelle: Beginn (Szenenzeit), Start und Ziel ihrer Mitte.
    wave: Option<(f32, Vec3, Vec3)>,
}

impl OrbitScene {
    /// Ort der Stationen jetzt.
    pub fn stations(&self) -> Vec<Vec3> {
        let count = self.built.as_ref().map_or(0, |layout| layout.stations);
        (0..count)
            .map(|index| station_position(index, count, self.time))
            .collect()
    }

    /// Wo die Mitte der laufenden Landungswelle gerade ist; `None`, wenn keine unterwegs
    /// ist.
    pub fn landing_spot(&self) -> Option<Vec3> {
        let (start, from, to) = self.wave?;
        let progress = (self.time - start) / (LANDER_FLIGHT + LANDER_GAP * LANDERS as f32);
        (0.0..1.0)
            .contains(&progress)
            .then(|| from.lerp(to, 1.0 - (1.0 - progress) * (1.0 - progress)))
    }

    /// Zeigt die Welt ihre fertige Oberfläche (volle Auflösung)?
    #[cfg(test)]
    pub fn surface_ready(&self) -> bool {
        self.shown_look == LookStage::Full
    }

    /// Material der Oberfläche.
    #[cfg(test)]
    pub fn planet_material(&self) -> &Handle<StandardMaterial> {
        &self.planet_material
    }

    /// Mitte der Flotte des Belagerers.
    pub fn fleet_center(&self) -> Option<Vec3> {
        let count = self.hulls.len();
        (count > 0).then(|| self.hulls.iter().map(|hull| hull.0).sum::<Vec3>() / count as f32)
    }
}

/// Zählt, was die Schlachtkamera von der Orbitansicht wirklich zeichnet (siehe
/// `SceneCensus`): für den Selbsttest und die Tests.
#[derive(SystemParam)]
pub struct OrbitCensus<'w, 's> {
    camera: Query<'w, 's, (Entity, &'static VisibleEntities), With<BattleCamera>>,
    cameras: Query<'w, 's, (Entity, &'static Camera)>,
    planet: Query<'w, 's, (), With<PlanetBody>>,
    hulls: Query<'w, 's, (), With<OrbitHull>>,
}

impl OrbitCensus<'_, '_> {
    pub fn camera_shows(&self) -> bool {
        self.camera
            .single()
            .is_ok_and(|(camera, _)| camera_shows(camera, &self.cameras))
    }

    fn drawn(&self) -> impl Iterator<Item = Entity> + '_ {
        self.camera
            .single()
            .ok()
            .filter(|_| self.camera_shows())
            .into_iter()
            .flat_map(|(_, visible)| visible.entities.values().flatten().copied())
    }

    /// Zeichnet die Kamera den Planeten?
    pub fn planet_visible(&self) -> bool {
        self.drawn().any(|entity| self.planet.contains(entity))
    }

    /// Rümpfe der belagernden Flotten in der Szene und die davon gezeichneten.
    pub fn hulls(&self) -> usize {
        self.hulls.iter().count()
    }

    pub fn visible_hulls(&self) -> usize {
        self.drawn()
            .filter(|entity| self.hulls.contains(*entity))
            .count()
    }
}

/// Blick auf Welt und Flotte beim Betreten und mit der Taste „Übersicht“. Die Welt steht
/// etwas rechts der Mitte: Links liegt die Tafel der Ansicht.
pub fn overview() -> Transform {
    // Schräg neben der Flotte: Sie steht vor dem Rand der Welt, das Landegebiet darunter
    // ist ganz zu sehen.
    let side = Quat::from_rotation_y(-0.62) * attack_direction();
    let eye = (side + Vec3::Y * 0.12).normalize() * PLANET_RADIUS * 4.5;
    let forward = -eye.normalize();
    let right = forward.cross(Vec3::Y).normalize();
    // Im Blick liegt die Strecke zwischen Welt und Flotte.
    let target = attack_direction() * PLANET_RADIUS * 0.75 - right * PLANET_RADIUS * 0.6;
    Transform::from_translation(eye).looking_at(target, Vec3::Y)
}

fn place_camera(camera: &mut Single<(&mut Transform, &mut FreeCameraState), With<BattleCamera>>) {
    let (transform, flight) = &mut **camera;
    **transform = overview();
    camera::sync_flight(flight, transform);
    flight.velocity = Vec3::ZERO;
}

/// Beim Betreten: Planet, Ausstattung beider Seiten, Kamera.
pub(super) fn enter(
    mut commands: Commands,
    watched: Option<Res<WatchedSiege>>,
    data: Res<GameData>,
    galaxy: Res<Galaxy>,
    assets: Res<BattleAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut atmospheres: ResMut<Assets<AtmosphereMaterial>>,
    mut camera: Single<(&mut Transform, &mut FreeCameraState), With<BattleCamera>>,
) {
    let Some(watched) = watched else {
        return;
    };
    let layout = &watched.layout;
    let planet = galaxy.planet(watched.planet);
    let look = LookInput::of(planet, &data);
    let scar = scar_of(&watched);
    // Bis die Oberfläche fertig ist, steht die Kugel in der Farbe ihrer Art da.
    let globe = meshes.add(meshes::uv_globe(128, 256));
    let tint = look.tint;
    let planet_material = materials.add(StandardMaterial {
        base_color: Color::srgb(tint[0], tint[1], tint[2]),
        perceptual_roughness: 0.9,
        metallic: 0.0,
        reflectance: 0.35,
        ..default()
    });
    commands.spawn((
        OrbitVisual,
        PlanetBody,
        Mesh3d(globe.clone()),
        MeshMaterial3d(planet_material.clone()),
        Transform::from_scale(Vec3::splat(PLANET_RADIUS)),
    ));
    let cloud_material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 1.0,
        reflectance: 0.05,
        ..default()
    });
    commands.spawn((
        OrbitVisual,
        CloudLayer,
        Mesh3d(globe.clone()),
        MeshMaterial3d(cloud_material.clone()),
        Transform::from_scale(Vec3::splat(PLANET_RADIUS * CLOUD_LIFT)),
        Visibility::Hidden,
    ));
    if let Some(haze) = look.haze() {
        let scale = PLANET_RADIUS * haze.thickness;
        let outer = PLANET_RADIUS + scale * 5.0;
        commands.spawn((
            OrbitVisual,
            HazeShell,
            Mesh3d(globe),
            MeshMaterial3d(atmospheres.add(AtmosphereMaterial {
                haze: Vec4::new(haze.color[0], haze.color[1], haze.color[2], haze.strength),
                sun: sun_direction().extend(0.0),
                body: Vec4::new(0.0, 0.0, 0.0, PLANET_RADIUS),
                shape: Vec4::new(outer, scale, 0.0, 0.0),
            })),
            Transform::from_scale(Vec3::splat(outer)),
        ));
    }
    let attacker = SideKit::build(layout.attacker, &assets, &mut meshes, &mut materials);
    let defender = SideKit::build(layout.defender, &assets, &mut meshes, &mut materials);
    let own = side_tint(layout.defender);
    let zone_material = |color: sim::data::Rgb| StandardMaterial {
        base_color: Color::from(linear(color)).with_alpha(0.62),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        double_sided: true,
        ..default()
    };
    commands.insert_resource(OrbitScene {
        built: None,
        station: meshes.add(meshes::station().tinted(own).to_mesh()),
        gun: meshes.add(meshes::surface_gun().tinted(own).to_mesh()),
        bunker: meshes.add(meshes::bunker().tinted(own).to_mesh()),
        shell: meshes.add(meshes::globe(24, 48, |_| [1.0; 4]).to_mesh()),
        look,
        planet_material,
        cloud_material,
        shown_look: LookStage::None,
        scar,
        zone_materials: [
            materials.add(zone_material(layout.defender)),
            materials.add(zone_material(layout.attacker)),
        ],
        shield_material: materials.add(additive(LinearRgba::new(0.25, 0.6, 1.8, 0.08))),
        attacker,
        defender,
        radii: class_radii(&data),
        time: 0.0,
        random: Cosmetic::new(watched.planet.0 ^ 0x0b17),
        budget: [0.0; 4],
        landings_shown: 0,
        hulls: Vec::new(),
        guns: Vec::new(),
        zones: Vec::new(),
        wave: None,
    });
    place_camera(&mut camera);
}

pub(super) fn exit(mut commands: Commands) {
    commands.despawn_all::<With<OrbitVisual>>();
    commands.remove_resource::<OrbitScene>();
}

/// Körper auf der Oberfläche: Fuß am Boden, Oberseite von der Welt weg.
fn grounded(direction: Vec3, size: f32) -> Transform {
    Transform {
        translation: direction * PLANET_RADIUS,
        rotation: Quat::from_rotation_arc(Vec3::Y, direction),
        scale: Vec3::splat(size),
    }
}

/// Baut die Aufstellung neu auf, sobald sie nicht mehr zur Karte des Kerns passt.
fn sync_layout(
    mut commands: Commands,
    watched: Res<WatchedSiege>,
    scene: Option<ResMut<OrbitScene>>,
    assets: Res<BattleAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let Some(mut scene) = scene else {
        return;
    };
    if scene.built.as_ref() == Some(&watched.layout) {
        return;
    }
    let layout = watched.layout.clone();
    commands.despawn_all::<With<Prop>>();

    // Flotte des Belagerers.
    scene.hulls.clear();
    for (class, position) in fleet_slots(&layout, scene.radii) {
        let radius = scene.radii[class.index()];
        scene.hulls.push((position, radius));
        commands.spawn((
            OrbitVisual,
            Prop,
            OrbitHull,
            Mesh3d(scene.attacker.classes[class.index()].hull.clone()),
            MeshMaterial3d(assets.hull_materials[0].clone()),
            Transform {
                translation: position,
                rotation: orbital_rotation(position),
                scale: Vec3::splat(radius),
            },
        ));
    }

    // Stationen: Ihr Ort folgt der Szenenzeit.
    for index in 0..layout.stations {
        let position = station_position(index, layout.stations, scene.time);
        commands.spawn((
            OrbitVisual,
            Prop,
            Station(index, layout.stations),
            Mesh3d(scene.station.clone()),
            MeshMaterial3d(assets.hull_materials[0].clone()),
            Transform {
                translation: position,
                rotation: orbital_rotation(position),
                scale: Vec3::splat(STATION_SIZE),
            },
        ));
    }

    // Geschütze und Bunker auf der Oberfläche. Schweigende Geschütze stehen matt da.
    scene.guns = (0..layout.guns * 2).map(gun_direction).collect();
    let gun_material = if layout.guns_active { 0 } else { 1 };
    for direction in scene.guns.clone() {
        commands.spawn((
            OrbitVisual,
            Prop,
            Mesh3d(scene.gun.clone()),
            MeshMaterial3d(assets.hull_materials[gun_material].clone()),
            grounded(direction, 17.0),
        ));
    }
    for index in 0..u32::from(layout.bunkers) * 2 {
        commands.spawn((
            OrbitVisual,
            Prop,
            Mesh3d(scene.bunker.clone()),
            MeshMaterial3d(assets.hull_materials[0].clone()),
            grounded(bunker_direction(index), 15.0),
        ));
    }

    // Zonen: Kappen auf der Oberfläche in der Farbe dessen, der sie hält. Gezeigt, sobald
    // am Boden gekämpft wird oder eine Zone genommen ist.
    let count = layout.zones.len();
    scene.zones = layout
        .zones
        .iter()
        .enumerate()
        .map(|(index, zone)| (zone_direction(index, count), zone_radius(zone.1)))
        .collect();
    let ground_war = layout.landed || layout.zones.iter().any(|zone| zone.2);
    if ground_war {
        for (zone, (direction, radius)) in layout.zones.iter().zip(scene.zones.clone()) {
            let cap = meshes::zone_cap(radius / PLANET_RADIUS);
            commands.spawn((
                OrbitVisual,
                Prop,
                Mesh3d(meshes.add(cap.to_mesh())),
                MeshMaterial3d(scene.zone_materials[usize::from(zone.2)].clone()),
                Transform {
                    translation: Vec3::ZERO,
                    rotation: Quat::from_rotation_arc(Vec3::Y, direction),
                    scale: Vec3::splat(PLANET_RADIUS * ZONE_LIFT),
                },
            ));
        }
    }

    // Schild: Schale, solange er Ladung hält.
    if layout.shielded {
        commands.spawn((
            OrbitVisual,
            Prop,
            ShieldShell,
            Mesh3d(scene.shell.clone()),
            MeshMaterial3d(scene.shield_material.clone()),
            Transform::from_scale(Vec3::splat(SHIELD_RADIUS)),
        ));
    }
    scene.built = Some(layout);
}

/// Anteil der Ladung des Schilds an seiner vollen Ladung (Stufe mal `shield_cap` der
/// Spieldaten). Nur für die Anzeige in Prozent und die Stärke der Schale; ob der Schild
/// hält, sagt die Ladung der Karte selbst.
pub fn shield_share(data: &GameData, watched: &WatchedSiege) -> f32 {
    let defense = &watched.card.defense;
    let full = defense.shield as f64 * data.balance.siege.shield_cap;
    if full > 0.0 {
        (defense.charge / full).clamp(0.0, 1.0) as f32
    } else {
        0.0
    }
}

/// Was die Szene je Sekunde zeigt, aus dem Tagesbericht des Kerns: Schüsse der Flotte,
/// Schüsse der Verteidigung, Einschläge im Bodenkampf und im Untergrund. Aus dem Orbit
/// fällt nur ein Schuss, solange der Belagerer ihn hält; die Truppe am Boden kämpft auch
/// ohne ihn weiter. Nach dem Ende der Belagerung zeigt die Szene nichts mehr.
pub fn show_rates(watched: &WatchedSiege, today: u64) -> [f32; 4] {
    let card = &watched.card;
    let report = &card.view.siege.report;
    if watched.ended() || report.day + 1 < today {
        return [0.0; 4];
    }
    let fired = report.ammo > 0.0
        || report.shield_absorbed > 0.0
        || report.defender_bombed > 0.0
        || report.fire_damage > 0.0
        || report.stations_lost > 0.0
        || report.guns_lost > 0.0;
    let bombard = if fired && card.control {
        match card.view.siege.orders.fire {
            Fire::Area => 9.0,
            Fire::Precise => 6.0,
            Fire::Support => 3.5,
            Fire::Hold => 4.0,
        }
    } else {
        0.0
    };
    let answer = if report.hull > 0.0 && card.control {
        3.0
    } else {
        0.0
    };
    let fought = report.attacker_lost > 0.0 || report.defender_lost > 0.0;
    let ground = if (report.zone.is_some() || report.sortie) && fought && !report.underground {
        6.0
    } else {
        0.0
    };
    let below = if report.underground { 3.5 } else { 0.0 };
    [bombard, answer, ground, below]
}

/// Liegt zwischen zwei Punkten über der Welt freie Sicht, oder steht die Welt dazwischen?
pub fn clear_path(from: Vec3, to: Vec3) -> bool {
    let along = to - from;
    let length = along.length();
    if length < 1.0 {
        return true;
    }
    let direction = along / length;
    let reach = (-from).dot(direction).clamp(0.0, length);
    (from + direction * reach).length() >= PLANET_RADIUS * 1.03
}

/// Zufälliger Punkt in einer Zone, als Richtung vom Mittelpunkt der Welt.
fn spot_in_zone(random: &mut Cosmetic, zone: (Vec3, f32)) -> Vec3 {
    let away = (zone.1 / PLANET_RADIUS) * random.unit().sqrt() * 0.9;
    offset_direction(zone.0, away, TAU * random.unit())
}

/// Lässt die Szene leben: Szenenzeit, Umlauf der Stationen, neue Schüsse und Einschläge
/// nach dem Tagesbericht, Landungsboote nach jeder gemeldeten Landung.
fn stage(
    mut commands: Commands,
    watched: Res<WatchedSiege>,
    scene: Option<ResMut<OrbitScene>>,
    assets: Res<BattleAssets>,
    data: Res<GameData>,
    clock: Res<SimClock>,
    game_time: Res<Time<Virtual>>,
    real_time: Res<Time<Real>>,
    mut stations: Query<(&Station, &mut Transform)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(mut scene) = scene else {
        return;
    };
    // Die Szene läuft mit der Partie: Sie steht, wenn die Zeit steht, und läuft auch bei
    // schnellem Vorlauf nur so schnell, dass man ihr folgen kann.
    let step = game_time
        .delta_secs()
        .min(real_time.delta_secs() * 2.0)
        .min(0.1);
    scene.time += step;
    let time = scene.time;
    for (station, mut transform) in &mut stations {
        let position = station_position(station.0, station.1, time);
        transform.translation = position;
        transform.rotation = orbital_rotation(position) * Quat::from_rotation_y(time * 0.2);
    }
    // Die Schale des Schilds ist so hell wie seine Ladung.
    let share = shield_share(&data, &watched);
    if let Some(mut material) = materials.get_mut(&scene.shield_material) {
        let wanted = Color::from(LinearRgba::new(0.25, 0.6, 1.8, 0.03 + 0.09 * share));
        if material.base_color != wanted {
            material.base_color = wanted;
        }
    }
    if step <= 0.0 {
        return;
    }
    let Some(layout) = scene.built.clone() else {
        return;
    };
    let rates = show_rates(&watched, clock.day());
    for (index, rate) in rates.iter().enumerate() {
        scene.budget[index] = (scene.budget[index] + rate * step).min(4.0);
    }
    let station_spots = scene.stations();
    let shooters: Vec<Vec3> = {
        let heavy: Vec<Vec3> = scene
            .hulls
            .iter()
            .filter(|hull| hull.1 >= 12.0 * HULL_SCALE)
            .map(|hull| hull.0)
            .collect();
        if heavy.is_empty() {
            scene.hulls.iter().map(|hull| hull.0).collect()
        } else {
            heavy
        }
    };

    // Beschuss aus dem Orbit: auf die Stationen, solange sie den Orbit halten, sonst auf
    // die Welt, wo ihn der Schild abfängt, solange er steht.
    while scene.budget[0] >= 1.0 {
        scene.budget[0] -= 1.0;
        if shooters.is_empty() {
            continue;
        }
        let from =
            shooters[(scene.random.unit() * shooters.len() as f32) as usize % shooters.len()];
        let at_stations = layout.phase == Phase::Approach && !station_spots.is_empty();
        let (to, hit) = if at_stations {
            // Nur Stationen, die von diesem Schiff aus nicht hinter der Welt stehen.
            let in_sight: Vec<Vec3> = station_spots
                .iter()
                .copied()
                .filter(|station| clear_path(from, *station))
                .collect();
            if in_sight.is_empty() {
                continue;
            }
            let pick = (scene.random.unit() * in_sight.len() as f32) as usize;
            (in_sight[pick % in_sight.len()], Hit::Hull)
        } else {
            let direction = if !scene.zones.is_empty() && scene.random.unit() < 0.7 {
                let pick = (scene.random.unit() * scene.zones.len() as f32) as usize;
                let zone = scene.zones[pick % scene.zones.len()];
                spot_in_zone(&mut scene.random, zone)
            } else {
                let away = 0.2 + 0.8 * scene.random.unit();
                let around = TAU * scene.random.unit();
                offset_direction(landing_direction(), away, around)
            };
            if layout.shielded {
                (direction * SHIELD_RADIUS, Hit::Shield)
            } else {
                (direction * PLANET_RADIUS, Hit::Surface)
            }
        };
        let beam = scene.random.unit() < 0.2;
        spawn_shot(&mut commands, &assets, &scene.attacker, from, to, beam, hit);
    }

    // Gegenfeuer: aktive Geschütze und Stationen schießen auf die Flotte, soweit die Welt
    // nicht dazwischen steht.
    while scene.budget[1] >= 1.0 {
        scene.budget[1] -= 1.0;
        if scene.hulls.is_empty() {
            continue;
        }
        let pick = (scene.random.unit() * scene.hulls.len() as f32) as usize;
        let to = scene.hulls[pick % scene.hulls.len()].0;
        let mut sources: Vec<Vec3> = station_spots.clone();
        if layout.guns_active {
            sources.extend(scene.guns.iter().map(|gun| *gun * (PLANET_RADIUS + 16.0)));
        }
        sources.retain(|source| clear_path(*source * 1.04, to));
        if sources.is_empty() {
            continue;
        }
        let from = sources[(scene.random.unit() * sources.len() as f32) as usize % sources.len()];
        let beam = scene.random.unit() < 0.5;
        spawn_shot(
            &mut commands,
            &assets,
            &scene.defender,
            from,
            to,
            beam,
            Hit::Hull,
        );
    }

    // Bodenkampf: Einschläge in der umkämpften Zone.
    while scene.budget[2] >= 1.0 {
        scene.budget[2] -= 1.0;
        let report = &watched.card.view.siege.report;
        let Some(zone) = report
            .zone
            .and_then(|zone| scene.zones.get(zone as usize).copied())
            .or_else(|| scene.zones.first().copied())
        else {
            continue;
        };
        let direction = spot_in_zone(&mut scene.random, zone);
        let size = 14.0 + 14.0 * scene.random.unit();
        spawn_flash(
            &mut commands,
            &assets,
            direction * (PLANET_RADIUS + 5.0),
            size,
            0.7,
        );
    }

    // Untergrund: kleine, verstreute Einschläge im ganzen Landegebiet.
    while scene.budget[3] >= 1.0 {
        scene.budget[3] -= 1.0;
        let away = 0.9 * scene.random.unit().sqrt();
        let around = TAU * scene.random.unit();
        let direction = offset_direction(landing_direction(), away, around);
        spawn_flash(
            &mut commands,
            &assets,
            direction * (PLANET_RADIUS + 4.0),
            11.0,
            0.8,
        );
    }

    // Landung: Für jede gemeldete Landung geht eine Welle Boote von der Flotte zu Boden.
    if scene.landings_shown < watched.landings {
        scene.landings_shown = watched.landings;
        let center = scene
            .fleet_center()
            .unwrap_or(attack_direction() * PLANET_RADIUS * 1.8);
        let fighter = ShipClass::Fighter.index();
        scene.wave = Some((
            scene.time,
            center,
            landing_direction() * (PLANET_RADIUS + 3.0),
        ));
        for index in 0..LANDERS {
            let from = if scene.hulls.is_empty() {
                center
            } else {
                scene.hulls[(index * 5) % scene.hulls.len()].0
            };
            let away = 0.5 * scene.random.unit().sqrt();
            let around = TAU * scene.random.unit();
            let to = offset_direction(landing_direction(), away, around) * (PLANET_RADIUS + 3.0);
            commands.spawn((
                OrbitVisual,
                Lander {
                    from,
                    to,
                    // Die Boote starten nacheinander.
                    age: -(index as f32) * LANDER_GAP,
                    ttl: LANDER_FLIGHT,
                },
                Mesh3d(scene.attacker.classes[fighter].hull.clone()),
                MeshMaterial3d(assets.hull_materials[0].clone()),
                Transform::from_translation(from).with_scale(Vec3::ZERO),
                children![(
                    Mesh3d(assets.classes[fighter].glow.clone()),
                    MeshMaterial3d(scene.attacker.glow.clone()),
                    Transform::from_xyz(-1.0, 0.0, 0.0).with_scale(Vec3::new(2.2, 1.6, 1.6)),
                )],
            ));
        }
    }
}

fn spawn_shot(
    commands: &mut Commands,
    assets: &BattleAssets,
    side: &SideKit,
    from: Vec3,
    to: Vec3,
    beam: bool,
    hit: Hit,
) {
    let distance = from.distance(to).max(1.0);
    let (mesh, material, ttl) = if beam {
        (assets.beam.clone(), side.beam_heavy.clone(), 0.28)
    } else {
        (
            assets.tracer.clone(),
            side.tracer.clone(),
            (distance / 900.0).clamp(0.3, 1.4),
        )
    };
    commands.spawn((
        OrbitVisual,
        Shot {
            from,
            to,
            age: 0.0,
            ttl,
            beam,
            hit,
        },
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::from_translation(from).with_scale(Vec3::ZERO),
    ));
}

fn spawn_flash(commands: &mut Commands, assets: &BattleAssets, at: Vec3, size: f32, ttl: f32) {
    commands.spawn((
        OrbitVisual,
        Flash {
            age: 0.0,
            ttl,
            size,
        },
        Mesh3d(assets.flash.clone()),
        MeshMaterial3d(assets.flash_materials[0].clone()),
        Transform::from_translation(at).with_scale(Vec3::splat(size * 0.5)),
    ));
}

/// Bewegt Geschosse, Strahlen, Einschläge und Landungsboote und räumt ab, was vorbei ist.
fn animate(
    mut commands: Commands,
    assets: Res<BattleAssets>,
    game_time: Res<Time<Virtual>>,
    real_time: Res<Time<Real>>,
    camera: Single<&Transform, (With<BattleCamera>, Without<Shot>, Without<Flash>)>,
    mut shots: Query<(Entity, &mut Shot, &mut Transform), (Without<Flash>, Without<BattleCamera>)>,
    mut flashes: Query<
        (
            Entity,
            &mut Flash,
            &mut Transform,
            &mut MeshMaterial3d<StandardMaterial>,
        ),
        (Without<Shot>, Without<BattleCamera>, Without<ShieldFlash>),
    >,
    mut shield_flashes: Query<
        (
            Entity,
            &mut ShieldFlash,
            &mut MeshMaterial3d<StandardMaterial>,
        ),
        Without<Flash>,
    >,
    mut landers: Query<
        (Entity, &mut Lander, &mut Transform),
        (Without<Shot>, Without<Flash>, Without<BattleCamera>),
    >,
) {
    let step = game_time
        .delta_secs()
        .min(real_time.delta_secs() * 2.0)
        .min(0.1);
    for (entity, mut shot, mut transform) in &mut shots {
        shot.age += step;
        let along = shot.to - shot.from;
        let distance = along.length().max(1.0);
        let direction = along / distance;
        transform.rotation = Quat::from_rotation_arc(Vec3::X, direction);
        if shot.beam {
            transform.translation = shot.from;
            transform.scale = Vec3::new(distance, 3.0, 3.0);
        } else {
            let progress = (shot.age / shot.ttl).clamp(0.0, 1.0);
            transform.translation = shot.from + along * progress;
            transform.scale = Vec3::new(70.0, 6.0, 6.0);
        }
        if shot.age >= shot.ttl {
            commands.entity(entity).despawn();
            match shot.hit {
                Hit::Surface => spawn_flash(&mut commands, &assets, shot.to, 30.0, 0.6),
                Hit::Hull => spawn_flash(&mut commands, &assets, shot.to, 20.0, 0.45),
                Hit::Shield => {
                    spawn_flash(&mut commands, &assets, shot.to, 16.0, 0.35);
                    commands.spawn((
                        OrbitVisual,
                        ShieldFlash { age: 0.0, ttl: 0.6 },
                        Mesh3d(assets.shield.clone()),
                        MeshMaterial3d(assets.shield_materials[0].clone()),
                        Transform {
                            translation: Vec3::ZERO,
                            rotation: Quat::from_rotation_arc(Vec3::X, shot.to.normalize()),
                            scale: Vec3::splat(SHIELD_RADIUS * 1.003),
                        },
                    ));
                }
            }
        }
    }
    for (entity, mut flash, mut transform, mut material) in &mut flashes {
        flash.age += step;
        let progress = (flash.age / flash.ttl).clamp(0.0, 1.0);
        if flash.age >= flash.ttl {
            commands.entity(entity).despawn();
            continue;
        }
        // Die Leuchtscheibe ist stets der Kamera zugewandt.
        transform.rotation = camera.rotation;
        transform.scale = Vec3::splat(flash.size * (0.5 + 0.9 * progress));
        let levels = assets.flash_materials.len();
        let level = ((progress * levels as f32) as usize).min(levels - 1);
        if material.0 != assets.flash_materials[level] {
            material.0 = assets.flash_materials[level].clone();
        }
    }
    for (entity, mut flash, mut material) in &mut shield_flashes {
        flash.age += step;
        let progress = (flash.age / flash.ttl).clamp(0.0, 1.0);
        if flash.age >= flash.ttl {
            commands.entity(entity).despawn();
            continue;
        }
        let levels = assets.shield_materials.len();
        let level = ((progress * levels as f32) as usize).min(levels - 1);
        if material.0 != assets.shield_materials[level] {
            material.0 = assets.shield_materials[level].clone();
        }
    }
    for (entity, mut lander, mut transform) in &mut landers {
        lander.age += step;
        if lander.age >= lander.ttl {
            commands.entity(entity).despawn();
            continue;
        }
        if lander.age < 0.0 {
            continue;
        }
        let progress = lander.age / lander.ttl;
        // Erst zügig, zum Boden hin langsamer.
        let eased = 1.0 - (1.0 - progress) * (1.0 - progress);
        let along = lander.to - lander.from;
        transform.translation = lander.from + along * eased;
        transform.rotation = Quat::from_rotation_arc(Vec3::X, along.normalize_or(Vec3::X));
        transform.scale = Vec3::splat(LANDER_SIZE * (1.0 - 0.45 * progress));
    }
}

/// Linien der Szene: Ränder der Zonen, die umkämpfte Zone, die Schale des Schilds und
/// die Bahn der Stationen.
fn draw_lines(
    mut gizmos: Gizmos<BattleGizmos>,
    watched: Res<WatchedSiege>,
    scene: Option<Res<OrbitScene>>,
    data: Res<GameData>,
    real_time: Res<Time<Real>>,
) {
    let Some(scene) = scene else {
        return;
    };
    let Some(layout) = &scene.built else {
        return;
    };
    let attacker = Color::from(linear(layout.attacker));
    let defender = Color::from(linear(layout.defender));
    let report = &watched.card.view.siege.report;
    let pulse = 0.5 + 0.5 * (real_time.elapsed_secs() * 4.0).sin();
    let ground_war = layout.landed || layout.zones.iter().any(|zone| zone.2);
    for (index, (zone, (direction, radius))) in layout.zones.iter().zip(&scene.zones).enumerate() {
        let color = if zone.2 { attacker } else { defender };
        // Ringe auf der Oberfläche: Sie folgen der Krümmung wie die Kappen.
        let ring = |lift: f32, radius: f32| {
            (0..=64).map(move |step| {
                offset_direction(*direction, radius / PLANET_RADIUS, TAU * step as f32 / 64.0)
                    * (PLANET_RADIUS * lift)
            })
        };
        // Vor der Landung liegt das Landegebiet noch ungeteilt da.
        if !ground_war {
            continue;
        }
        gizmos.linestrip(ring(ZONE_LIFT + 0.002, *radius), color.with_alpha(0.95));
        // Wo heute gekämpft wird, pulsiert ein zweiter Ring: die Front.
        if !watched.ended() && report.zone == Some(index as u8) {
            gizmos.linestrip(
                ring(ZONE_LIFT + 0.006, *radius * (0.72 + 0.2 * pulse)),
                Color::srgb(1.0, 0.85, 0.4).with_alpha(0.9),
            );
        }
    }
    if layout.shielded {
        let share = shield_share(&data, &watched);
        let color = Color::srgb(0.45, 0.75, 1.0).with_alpha(0.25 + 0.6 * share);
        for rotation in [
            Quat::IDENTITY,
            Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
            Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        ] {
            gizmos
                .circle(Isometry3d::new(Vec3::ZERO, rotation), SHIELD_RADIUS, color)
                .resolution(96);
        }
    }
    if layout.stations > 0 {
        let axis = Vec3::new(0.3, 1.0, -0.12).normalize();
        gizmos
            .circle(
                Isometry3d::new(Vec3::ZERO, Quat::from_rotation_arc(Vec3::Z, axis)),
                STATION_ORBIT,
                defender.with_alpha(0.25),
            )
            .resolution(96);
    }
}

/// Hält die Kamera im Zaum (wie in der Schlachtansicht): Tempo in Grenzen, keine Bewegung
/// beim Tippen, nicht zu weit weg und nicht in die Welt hinein.
fn tame_camera(
    wants: Res<EguiWantsInput>,
    mut camera: Single<(&mut Transform, &mut FreeCameraState), With<BattleCamera>>,
) {
    let (transform, flight) = &mut *camera;
    let speed = flight.speed_multiplier.clamp(FLY_SPEED_MIN, FLY_SPEED_MAX);
    if flight.speed_multiplier != speed {
        flight.speed_multiplier = speed;
    }
    let enabled = !wants.wants_any_keyboard_input();
    if flight.enabled != enabled {
        flight.enabled = enabled;
    }
    let distance = transform.translation.length();
    let wanted = distance.clamp(SHIELD_RADIUS * 1.06, MAX_DISTANCE);
    if distance != wanted {
        transform.translation = transform.translation.normalize_or(Vec3::Z) * wanted;
    }
}

fn hotkeys(
    keys: Res<ButtonInput<KeyCode>>,
    mut camera: Single<(&mut Transform, &mut FreeCameraState), With<BattleCamera>>,
) {
    if keys.just_pressed(KEY_OVERVIEW) {
        place_camera(&mut camera);
    }
}

/// Bitte der Tafel, zur Übersicht zurückzukehren.
#[derive(Message, Clone, Copy, Debug)]
pub struct OverviewRequest;

fn apply_overview(
    mut requests: MessageReader<OverviewRequest>,
    mut camera: Single<(&mut Transform, &mut FreeCameraState), With<BattleCamera>>,
) {
    if requests.read().last().is_some() {
        place_camera(&mut camera);
    }
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<PlanetLooks>();
    app.add_message::<OverviewRequest>().add_systems(
        Update,
        (
            tame_camera,
            hotkeys.run_if(not(egui_wants_any_keyboard_input)),
            apply_overview,
            sync_layout,
            stage,
            update_look,
            animate,
            draw_lines,
        )
            .chain()
            .run_if(in_state(ViewState::Siege))
            .run_if(resource_exists::<WatchedSiege>),
    );
}

//! Prozedurale Meshes der Schlachtansicht: je Schiffsklasse ein eigener Umriss aus
//! einfachen Körpern, dazu die Formen für Geschosse, Strahlen, Schilde und Trümmer.
//!
//! Alles entsteht im Code, ohne Dateien. Die Rümpfe sind auf die Länge 2 normiert (Bug bei
//! x = +1, Heck bei x = −1, y nach oben, z nach Steuerbord) und werden in der Szene mit dem
//! Radius des Schiffs aus den Spieldaten skaliert. Jede Fläche hat eigene Ecken und damit
//! eine eigene Normale: Die Rümpfe sollen kantig aussehen.

use bevy::asset::RenderAssetUsages;
use bevy::math::Affine3A;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use sim::data::ShipClass;
use std::f32::consts::{FRAC_PI_2, FRAC_PI_6, TAU};

/// Farbe einer Ecke (linear). Bei Rümpfen ist sie ein Anstrich: Die ersten drei Werte
/// sind die Grundfarbe, der vierte sagt, wie stark die Farbe der Partei durchschlägt
/// (0 = blanker Stahl, 1 = volle Parteifarbe). [`MeshData::tinted`] rechnet das für eine
/// Partei aus. Bei Effekten ist der vierte Wert die Deckkraft.
pub type Paint = [f32; 4];

/// Rumpfplatten: Stahl mit einem Hauch Parteifarbe.
pub const HULL: Paint = [0.62, 0.64, 0.68, 0.22];
/// Aufbauten und Panzerplatten: dunkler abgesetzt.
pub const PLATE: Paint = [0.36, 0.38, 0.42, 0.12];
/// Öffnungen, Rohre, Düsen.
pub const DARK: Paint = [0.055, 0.06, 0.075, 0.0];
/// Abzeichen: Streifen, Leitwerke und Kuppeln in voller Parteifarbe.
pub const TRIM: Paint = [0.95, 0.95, 0.95, 1.0];

/// Unter dieser doppelten Fläche gilt ein Dreieck als entartet und wird verworfen.
const MIN_DOUBLE_AREA: f32 = 1e-9;

/// Dreiecksliste mit Normalen und Farben; jede Ecke gehört zu genau einem Dreieck.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshData {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub colors: Vec<Paint>,
    pub indices: Vec<u32>,
}

impl MeshData {
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Ecken des Dreiecks `index`.
    pub fn triangle(&self, index: usize) -> [Vec3; 3] {
        let corner = |offset: usize| self.positions[self.indices[index * 3 + offset] as usize];
        [corner(0), corner(1), corner(2)]
    }

    /// Dreieck mit einer Farbe; die Vorderseite ist die, von der aus die Ecken gegen den
    /// Uhrzeigersinn laufen. Entartete Dreiecke entfallen.
    pub fn tri(&mut self, a: Vec3, b: Vec3, c: Vec3, paint: Paint) {
        self.tri_painted([a, b, c], [paint, paint, paint]);
    }

    /// Dreieck mit einer Farbe je Ecke (für Verläufe).
    pub fn tri_painted(&mut self, corners: [Vec3; 3], paints: [Paint; 3]) {
        let cross = (corners[1] - corners[0]).cross(corners[2] - corners[0]);
        if cross.length_squared() < MIN_DOUBLE_AREA * MIN_DOUBLE_AREA {
            return;
        }
        let normal = cross.normalize();
        let first = self.positions.len() as u32;
        for (corner, paint) in corners.into_iter().zip(paints) {
            self.positions.push(corner);
            self.normals.push(normal);
            self.colors.push(paint);
        }
        self.indices.extend([first, first + 1, first + 2]);
    }

    /// Viereck aus zwei Dreiecken. Fallen zwei Ecken zusammen, bleibt ein Dreieck übrig.
    pub fn quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3, paint: Paint) {
        self.tri(a, b, c, paint);
        self.tri(a, c, d, paint);
    }

    /// Konvexes Vieleck als Fächer um die erste Ecke.
    pub fn polygon(&mut self, points: &[Vec3], paint: Paint) {
        for index in 1..points.len().saturating_sub(1) {
            self.tri(points[0], points[index], points[index + 1], paint);
        }
    }

    pub fn append(&mut self, other: &MeshData) {
        let offset = self.positions.len() as u32;
        self.positions.extend_from_slice(&other.positions);
        self.normals.extend_from_slice(&other.normals);
        self.colors.extend_from_slice(&other.colors);
        self.indices
            .extend(other.indices.iter().map(|index| index + offset));
    }

    /// Hängt `other` an und gibt sich selbst zurück (für Ketten).
    pub fn with(mut self, other: &MeshData) -> Self {
        self.append(other);
        self
    }

    /// Dasselbe Mesh, verschoben, gedreht, skaliert oder gespiegelt. Eine Spiegelung dreht
    /// den Umlaufsinn um, damit die Außenseiten außen bleiben.
    pub fn transformed(&self, transform: Affine3A) -> MeshData {
        let mirrored = transform.matrix3.determinant() < 0.0;
        let mut result = MeshData::default();
        for index in 0..self.triangle_count() {
            let base = index * 3;
            let mut corners = [0, 1, 2].map(|offset| {
                let vertex = self.indices[base + offset] as usize;
                (
                    transform.transform_point3(self.positions[vertex]),
                    self.colors[vertex],
                )
            });
            if mirrored {
                corners.swap(1, 2);
            }
            result.tri_painted(corners.map(|corner| corner.0), corners.map(|corner| corner.1));
        }
        result
    }

    pub fn translated(&self, offset: Vec3) -> MeshData {
        self.transformed(Affine3A::from_translation(offset))
    }

    /// An der Mittelebene gespiegelt (Steuerbord wird Backbord).
    pub fn mirrored_z(&self) -> MeshData {
        self.transformed(Affine3A::from_scale(Vec3::new(1.0, 1.0, -1.0)))
    }

    /// Das Teil und sein Spiegelbild.
    pub fn both_sides(&self) -> MeshData {
        self.clone().with(&self.mirrored_z())
    }

    /// Alle Dreiecke umgedreht.
    fn flipped(&self) -> MeshData {
        let mut result = MeshData::default();
        for index in 0..self.triangle_count() {
            let base = index * 3;
            let vertex = |offset: usize| self.indices[base + offset] as usize;
            let (a, b, c) = (vertex(0), vertex(2), vertex(1));
            result.tri_painted(
                [self.positions[a], self.positions[b], self.positions[c]],
                [self.colors[a], self.colors[b], self.colors[c]],
            );
        }
        result
    }

    /// Rechnet den Anstrich eines Rumpfs für eine Partei aus: Jede Ecke bekommt ihre
    /// Grundfarbe, je nach Anteil mit der Parteifarbe `tint` (linear) eingefärbt.
    pub fn tinted(&self, tint: [f32; 3]) -> MeshData {
        let mut result = self.clone();
        for color in &mut result.colors {
            let amount = color[3].clamp(0.0, 1.0);
            for channel in 0..3 {
                color[channel] *= 1.0 + (tint[channel] - 1.0) * amount;
            }
            color[3] = 1.0;
        }
        result
    }

    /// Kleinste und größte Koordinaten.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        let mut min = Vec3::splat(f32::MAX);
        let mut max = Vec3::splat(f32::MIN);
        for position in &self.positions {
            min = min.min(*position);
            max = max.max(*position);
        }
        (min, max)
    }

    pub fn surface_area(&self) -> f32 {
        (0..self.triangle_count())
            .map(|index| {
                let [a, b, c] = self.triangle(index);
                0.5 * (b - a).cross(c - a).length()
            })
            .sum()
    }

    /// Eingeschlossenes Volumen; positiv, wenn die Vorderseiten nach außen zeigen.
    pub fn signed_volume(&self) -> f32 {
        (0..self.triangle_count())
            .map(|index| {
                let [a, b, c] = self.triangle(index);
                a.dot(b.cross(c)) / 6.0
            })
            .sum()
    }

    /// Geschlossener Körper zwischen Querschnitten: Aufeinanderfolgende Ringe gleicher
    /// Eckenzahl werden mit Vierecken verbunden, der erste und der letzte mit einer Fläche
    /// verschlossen. Die Außenseite ergibt sich von selbst: Zeigen die Flächen nach innen,
    /// wird der Körper umgedreht.
    pub fn loft(rings: &[Vec<Vec3>], paint: Paint) -> MeshData {
        let painted: Vec<(Vec<Vec3>, Paint)> =
            rings.iter().map(|ring| (ring.clone(), paint)).collect();
        Self::loft_painted(&painted)
    }

    /// Wie [`MeshData::loft`], aber mit einer Farbe je Ring; dazwischen verläuft sie.
    pub fn loft_painted(rings: &[(Vec<Vec3>, Paint)]) -> MeshData {
        let mut mesh = MeshData::default();
        let Some((first, first_paint)) = rings.first() else {
            return mesh;
        };
        let count = first.len();
        for pair in rings.windows(2) {
            let ((back, back_paint), (front, front_paint)) = (&pair[0], &pair[1]);
            debug_assert!(back.len() == count && front.len() == count);
            for index in 0..count {
                let next = (index + 1) % count;
                mesh.tri_painted(
                    [back[index], front[index], front[next]],
                    [*back_paint, *front_paint, *front_paint],
                );
                mesh.tri_painted(
                    [back[index], front[next], back[next]],
                    [*back_paint, *front_paint, *back_paint],
                );
            }
        }
        mesh.polygon(first, *first_paint);
        if let Some((last, last_paint)) = rings.last() {
            let reversed: Vec<Vec3> = last.iter().rev().copied().collect();
            mesh.polygon(&reversed, *last_paint);
        }
        if mesh.signed_volume() < 0.0 {
            mesh = mesh.flipped();
        }
        mesh
    }

    /// Schneidet das Mesh an der Ebene x = `at` in ein Vorder- und ein Achterteil.
    /// Dreiecke, die die Ebene kreuzen, werden geteilt; die Schnittflächen bleiben offen.
    pub fn split_x(&self, at: f32) -> (MeshData, MeshData) {
        let mut fore = MeshData::default();
        let mut aft = MeshData::default();
        for index in 0..self.triangle_count() {
            let base = index * 3;
            let corners = [0, 1, 2].map(|offset| {
                let vertex = self.indices[base + offset] as usize;
                (self.positions[vertex], self.colors[vertex])
            });
            for (target, keep_front) in [(&mut fore, true), (&mut aft, false)] {
                let clipped = clip_at_x(&corners, at, keep_front);
                for fan in 1..clipped.len().saturating_sub(1) {
                    target.tri_painted(
                        [clipped[0].0, clipped[fan].0, clipped[fan + 1].0],
                        [clipped[0].1, clipped[fan].1, clipped[fan + 1].1],
                    );
                }
            }
        }
        (fore, aft)
    }

    /// Verschließt eine offene Schnittfläche bei x = `at` mit einer ebenen Platte über alle
    /// Ecken, die dort liegen. `facing` ist +1, wenn die Platte nach vorn zeigen soll.
    pub fn cap_x(&mut self, at: f32, facing: f32, paint: Paint) {
        let on_plane: Vec<Vec2> = self
            .positions
            .iter()
            .filter(|position| (position.x - at).abs() < 1e-4)
            .map(|position| Vec2::new(position.y, position.z))
            .collect();
        let hull = convex_hull(on_plane);
        if hull.len() < 3 {
            return;
        }
        // Die Hülle läuft in der (y, z)-Ebene gegen den Uhrzeigersinn; von vorn gesehen
        // ist das die Vorderseite.
        let mut points: Vec<Vec3> = hull
            .iter()
            .map(|point| Vec3::new(at, point.x, point.y))
            .collect();
        if facing < 0.0 {
            points.reverse();
        }
        self.polygon(&points, paint);
    }

    /// Vorder- und Achterteil mit dunkel verschlossenen Bruchflächen.
    pub fn halves(&self, at: f32) -> (MeshData, MeshData) {
        let (mut fore, mut aft) = self.split_x(at);
        fore.cap_x(at, -1.0, DARK);
        aft.cap_x(at, 1.0, DARK);
        (fore, aft)
    }

    pub fn to_mesh(&self) -> Mesh {
        let positions: Vec<[f32; 3]> = self.positions.iter().map(|p| p.to_array()).collect();
        let normals: Vec<[f32; 3]> = self.normals.iter().map(|n| n.to_array()).collect();
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors.clone())
        .with_inserted_indices(Indices::U32(self.indices.clone()))
    }
}

/// Teil eines Dreiecks vor (`keep_front`) oder hinter der Ebene x = `at`.
fn clip_at_x(corners: &[(Vec3, Paint); 3], at: f32, keep_front: bool) -> Vec<(Vec3, Paint)> {
    let inside = |point: Vec3| {
        if keep_front {
            point.x >= at
        } else {
            point.x <= at
        }
    };
    let mut result = Vec::with_capacity(4);
    for index in 0..3 {
        let (current, next) = (corners[index], corners[(index + 1) % 3]);
        let (current_in, next_in) = (inside(current.0), inside(next.0));
        if current_in {
            result.push(current);
        }
        if current_in != next_in {
            let t = (at - current.0.x) / (next.0.x - current.0.x);
            let mut point = current.0.lerp(next.0, t);
            point.x = at;
            let mut paint = [0.0; 4];
            for channel in 0..4 {
                paint[channel] = current.1[channel] + (next.1[channel] - current.1[channel]) * t;
            }
            result.push((point, paint));
        }
    }
    result
}

/// Konvexe Hülle, gegen den Uhrzeigersinn (Andrews Verfahren).
fn convex_hull(mut points: Vec<Vec2>) -> Vec<Vec2> {
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    points.dedup_by(|a, b| a.distance_squared(*b) < 1e-10);
    if points.len() < 3 {
        return points;
    }
    let turn = |a: Vec2, b: Vec2, c: Vec2| (b - a).perp_dot(c - a);
    let mut hull: Vec<Vec2> = Vec::with_capacity(points.len() * 2);
    for pass in 0..2 {
        let start = hull.len();
        let ordered: Box<dyn Iterator<Item = &Vec2>> = if pass == 0 {
            Box::new(points.iter())
        } else {
            Box::new(points.iter().rev())
        };
        for point in ordered {
            while hull.len() >= start + 2
                && turn(hull[hull.len() - 2], hull[hull.len() - 1], *point) <= 1e-9
            {
                hull.pop();
            }
            hull.push(*point);
        }
        hull.pop();
    }
    hull
}

// --- Querschnitte ---------------------------------------------------------------------

/// Elliptischer Ring mit `sides` Ecken um die x-Achse, Mitte bei `center`.
fn ring(center: Vec3, sides: usize, half_height: f32, half_width: f32, phase: f32) -> Vec<Vec3> {
    (0..sides)
        .map(|index| {
            let angle = phase + TAU * index as f32 / sides as f32;
            center + Vec3::new(0.0, half_height * angle.sin(), half_width * angle.cos())
        })
        .collect()
}

/// Rechteckiger Querschnitt bei `x`.
fn rect_ring(x: f32, y: (f32, f32), z: (f32, f32)) -> Vec<Vec3> {
    vec![
        Vec3::new(x, y.0, z.1),
        Vec3::new(x, y.1, z.1),
        Vec3::new(x, y.1, z.0),
        Vec3::new(x, y.0, z.0),
    ]
}

/// Achteckiger Rumpfquerschnitt bei `x`: Rechteck mit abgeschrägten Kanten.
fn hull_ring(x: f32, half_width: f32, top: f32, bottom: f32, bevel: f32) -> Vec<Vec3> {
    let bevel = bevel.min(half_width * 0.95).min((top - bottom) * 0.45);
    let (w, inner) = (half_width, half_width - bevel);
    [
        (bottom + bevel, w),
        (top - bevel, w),
        (top, inner),
        (top, -inner),
        (top - bevel, -w),
        (bottom + bevel, -w),
        (bottom, -inner),
        (bottom, inner),
    ]
    .into_iter()
    .map(|(y, z)| Vec3::new(x, y, z))
    .collect()
}

// --- Grundkörper ----------------------------------------------------------------------

/// Quader zwischen zwei Ecken.
fn cuboid(min: Vec3, max: Vec3, paint: Paint) -> MeshData {
    MeshData::loft(
        &[
            rect_ring(min.x, (min.y, max.y), (min.z, max.z)),
            rect_ring(max.x, (min.y, max.y), (min.z, max.z)),
        ],
        paint,
    )
}

/// Kegelstumpf entlang x von `from` nach `to` mit je eigenem Radius; Radius 0 gibt eine
/// Spitze.
fn frustum(from: Vec3, to: Vec3, radius: (f32, f32), sides: usize, paint: Paint) -> MeshData {
    MeshData::loft(
        &[
            ring(from, sides, radius.0, radius.0, 0.0),
            ring(to, sides, radius.1, radius.1, 0.0),
        ],
        paint,
    )
}

/// Dreht ein entlang +x gebautes Teil so, dass seine Achse nach `axis` zeigt.
fn along(mesh: &MeshData, axis: Vec3, offset: Vec3) -> MeshData {
    mesh.transformed(Affine3A::from_rotation_translation(
        Quat::from_rotation_arc(Vec3::X, axis.normalize()),
        offset,
    ))
}

/// Flache Kuppel mit Fuß bei x = 0, Scheitel bei x = `height`.
fn dome(radius: f32, height: f32, sides: usize, paint: Paint) -> MeshData {
    let steps = 3;
    let rings: Vec<Vec<Vec3>> = (0..=steps)
        .map(|step| {
            let angle = FRAC_PI_2 * step as f32 / steps as f32;
            let r = radius * angle.cos();
            ring(Vec3::X * (height * angle.sin()), sides, r, r, 0.0)
        })
        .collect();
    MeshData::loft(&rings, paint)
}

/// Geschützturm: Sockel, Turmhaus und Zwillingsrohre in Richtung +x. Fuß bei y = 0.
fn turret(size: f32, barrel: f32, paint: Paint) -> MeshData {
    let base = along(
        &frustum(Vec3::ZERO, Vec3::X * 0.22 * size, (0.56 * size, 0.5 * size), 8, PLATE),
        Vec3::Y,
        Vec3::ZERO,
    );
    let house = MeshData::loft(
        &[
            rect_ring(-0.55 * size, (0.2 * size, 0.62 * size), (-0.42 * size, 0.42 * size)),
            rect_ring(0.25 * size, (0.2 * size, 0.62 * size), (-0.46 * size, 0.46 * size)),
            rect_ring(0.6 * size, (0.2 * size, 0.42 * size), (-0.3 * size, 0.3 * size)),
        ],
        paint,
    );
    let tube = frustum(
        Vec3::new(0.5 * size, 0.38 * size, 0.2 * size),
        Vec3::new(0.5 * size + barrel, 0.38 * size, 0.2 * size),
        (0.085 * size, 0.07 * size),
        6,
        DARK,
    );
    base.with(&house).with(&tube.both_sides())
}

/// Triebwerksdüse: dunkler Kegelstumpf, dessen Öffnung bei `at` liegt.
fn nozzle(at: Vec3, radius: f32, length: f32) -> MeshData {
    frustum(
        at + Vec3::X * length,
        at,
        (radius * 0.78, radius),
        8,
        DARK,
    )
}

// --- Schiffsklassen -------------------------------------------------------------------

/// Triebwerk eines Rumpfs: Mitte der Düsenöffnung und ihr Radius (normierte Maße).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Engine {
    pub at: Vec3,
    pub radius: f32,
}

/// Bauplan einer Klasse in normierten Maßen.
#[derive(Clone, Debug)]
pub struct ClassModel {
    pub hull: MeshData,
    pub engines: Vec<Engine>,
    /// Stelle, an der ein Großkampfschiff auseinanderbricht.
    pub break_at: f32,
}

impl ClassModel {
    pub fn of(class: ShipClass) -> ClassModel {
        match class {
            ShipClass::Fighter => fighter(),
            ShipClass::Corvette => corvette(),
            ShipClass::Frigate => frigate(),
            ShipClass::Cruiser => cruiser(),
            ShipClass::Battleship => battleship(),
            ShipClass::Carrier => carrier(),
        }
    }

    /// Vorder- und Achterteil für das Auseinanderbrechen.
    pub fn halves(&self) -> (MeshData, MeshData) {
        self.hull.halves(self.break_at)
    }

    /// Triebwerksstrahlen aller Düsen als ein Mesh. Ursprung ist die Heckebene x = −1;
    /// die Strahlen reichen bei voller Fahrt das Siebenfache des Düsenradius dahinter.
    /// An der Düse hell, zur Spitze hin dunkel.
    pub fn glow(&self) -> MeshData {
        let mut mesh = MeshData::default();
        for engine in &self.engines {
            let length = engine.radius * 7.0;
            let center = Vec3::new(engine.at.x + 1.0, engine.at.y, engine.at.z);
            let section = |back: f32, scale: f32| {
                let radius = engine.radius * scale;
                ring(center - Vec3::X * (length * back), 6, radius, radius, 0.0)
            };
            mesh.append(&MeshData::loft_painted(&[
                (section(1.0, 0.0), GLOW_COLD),
                (section(0.3, 0.55), GLOW_WARM),
                (section(0.0, 0.8), GLOW_HOT),
            ]));
        }
        mesh
    }
}

const GLOW_HOT: Paint = [1.0, 1.0, 1.0, 1.0];
const GLOW_WARM: Paint = [0.45, 0.45, 0.45, 1.0];
const GLOW_COLD: Paint = [0.06, 0.06, 0.06, 1.0];

/// Jäger: kleiner Pfeil mit gepfeilten Flügeln und Endscheiben.
fn fighter() -> ClassModel {
    let body = MeshData::loft(
        &[
            ring(Vec3::X * -0.88, 6, 0.085, 0.11, FRAC_PI_6),
            ring(Vec3::X * -0.45, 6, 0.13, 0.17, FRAC_PI_6),
            ring(Vec3::X * 0.3, 6, 0.10, 0.13, FRAC_PI_6),
            ring(Vec3::X * 1.0, 6, 0.0, 0.0, FRAC_PI_6),
        ],
        HULL,
    );
    let wing = MeshData::loft(
        &[
            vec![
                Vec3::new(-0.75, -0.03, 0.12),
                Vec3::new(0.2, -0.02, 0.12),
                Vec3::new(0.2, 0.02, 0.12),
                Vec3::new(-0.75, 0.03, 0.12),
            ],
            vec![
                Vec3::new(-0.86, -0.012, 0.62),
                Vec3::new(-0.52, -0.012, 0.62),
                Vec3::new(-0.52, 0.012, 0.62),
                Vec3::new(-0.86, 0.012, 0.62),
            ],
        ],
        PLATE,
    );
    let fin = MeshData::loft(
        &[
            vec![
                Vec3::new(-0.9, -0.05, 0.61),
                Vec3::new(-0.5, -0.02, 0.61),
                Vec3::new(-0.62, 0.17, 0.61),
                Vec3::new(-0.9, 0.17, 0.61),
            ],
            vec![
                Vec3::new(-0.9, -0.05, 0.64),
                Vec3::new(-0.5, -0.02, 0.64),
                Vec3::new(-0.62, 0.17, 0.64),
                Vec3::new(-0.9, 0.17, 0.64),
            ],
        ],
        TRIM,
    );
    let canopy = MeshData::loft(
        &[
            rect_ring(0.12, (0.09, 0.105), (-0.05, 0.05)),
            rect_ring(0.3, (0.07, 0.17), (-0.06, 0.06)),
            rect_ring(0.58, (0.05, 0.075), (-0.03, 0.03)),
        ],
        DARK,
    );
    let engine = Engine {
        at: Vec3::new(-1.0, 0.0, 0.0),
        radius: 0.1,
    };
    let hull = body
        .with(&wing.both_sides())
        .with(&fin.both_sides())
        .with(&canopy)
        .with(&nozzle(engine.at, engine.radius, 0.14));
    ClassModel {
        hull,
        engines: vec![engine],
        break_at: 0.0,
    }
}

/// Korvette: schlanker Rumpf, zwei Triebwerksgondeln an gepfeilten Trägern.
fn corvette() -> ClassModel {
    let body = MeshData::loft(
        &[
            hull_ring(-0.92, 0.09, 0.07, -0.06, 0.03),
            hull_ring(-0.75, 0.14, 0.11, -0.09, 0.04),
            hull_ring(-0.2, 0.16, 0.12, -0.10, 0.05),
            hull_ring(0.55, 0.11, 0.09, -0.08, 0.04),
            hull_ring(1.0, 0.015, 0.02, -0.02, 0.008),
        ],
        HULL,
    );
    let pod_center = Vec3::new(0.0, 0.0, 0.37);
    let pod = MeshData::loft(
        &[
            ring(pod_center + Vec3::X * -0.92, 8, 0.085, 0.085, 0.0),
            ring(pod_center + Vec3::X * -0.3, 8, 0.085, 0.085, 0.0),
            ring(pod_center + Vec3::X * -0.05, 8, 0.0, 0.0, 0.0),
        ],
        PLATE,
    );
    let pylon = MeshData::loft(
        &[
            vec![
                Vec3::new(-0.72, -0.02, 0.13),
                Vec3::new(-0.3, -0.02, 0.13),
                Vec3::new(-0.3, 0.02, 0.13),
                Vec3::new(-0.72, 0.02, 0.13),
            ],
            vec![
                Vec3::new(-0.8, -0.018, 0.31),
                Vec3::new(-0.5, -0.018, 0.31),
                Vec3::new(-0.5, 0.018, 0.31),
                Vec3::new(-0.8, 0.018, 0.31),
            ],
        ],
        HULL,
    );
    let bridge = MeshData::loft(
        &[
            rect_ring(-0.15, (0.11, 0.17), (-0.07, 0.07)),
            rect_ring(0.12, (0.11, 0.17), (-0.07, 0.07)),
            rect_ring(0.3, (0.1, 0.115), (-0.04, 0.04)),
        ],
        PLATE,
    );
    let fin = cuboid(
        Vec3::new(-0.72, 0.11, -0.012),
        Vec3::new(-0.42, 0.27, 0.012),
        TRIM,
    );
    let tube = frustum(
        Vec3::new(0.4, -0.085, 0.065),
        Vec3::new(0.82, -0.07, 0.05),
        (0.032, 0.028),
        6,
        DARK,
    );
    let engines = vec![
        Engine {
            at: Vec3::new(-1.0, 0.0, 0.37),
            radius: 0.092,
        },
        Engine {
            at: Vec3::new(-1.0, 0.0, -0.37),
            radius: 0.092,
        },
    ];
    let band = cuboid(
        Vec3::new(-0.74, 0.0, 0.305),
        Vec3::new(-0.6, 0.089, 0.435),
        TRIM,
    );
    let hull = body
        .with(&pod.both_sides())
        .with(&band.both_sides())
        .with(&pylon.both_sides())
        .with(&bridge)
        .with(&fin)
        .with(&tube.both_sides())
        .with(&nozzle(engines[0].at, engines[0].radius, 0.09).both_sides());
    ClassModel {
        hull,
        engines,
        break_at: 0.0,
    }
}

/// Fregatte: gedrungenes Geleitschiff mit Kuppeln der Punktverteidigung ringsum.
fn frigate() -> ClassModel {
    let body = MeshData::loft(
        &[
            hull_ring(-0.93, 0.21, 0.15, -0.13, 0.06),
            hull_ring(-0.6, 0.30, 0.20, -0.17, 0.08),
            hull_ring(0.15, 0.30, 0.20, -0.17, 0.08),
            hull_ring(0.8, 0.19, 0.14, -0.12, 0.06),
            hull_ring(1.0, 0.09, 0.07, -0.07, 0.03),
        ],
        HULL,
    );
    let house = MeshData::loft(
        &[
            rect_ring(-0.45, (0.2, 0.32), (-0.15, 0.15)),
            rect_ring(0.0, (0.2, 0.32), (-0.15, 0.15)),
            rect_ring(0.18, (0.2, 0.24), (-0.1, 0.1)),
        ],
        PLATE,
    );
    let mast = cuboid(
        Vec3::new(-0.34, 0.32, -0.015),
        Vec3::new(-0.26, 0.45, 0.015),
        TRIM,
    );
    let blister = dome(0.105, 0.085, 8, TRIM);
    let mut blisters = MeshData::default();
    // Oben vorn und achtern, unten mittig, an jeder Flanke zwei.
    blisters.append(&along(&blister, Vec3::Y, Vec3::new(0.48, 0.185, 0.0)));
    blisters.append(&along(&blister, Vec3::Y, Vec3::new(-0.7, 0.19, 0.0)));
    blisters.append(&along(&blister, Vec3::NEG_Y, Vec3::new(0.0, -0.17, 0.0)));
    let flank = along(&blister, Vec3::Z, Vec3::new(0.3, 0.01, 0.285))
        .with(&along(&blister, Vec3::Z, Vec3::new(-0.38, 0.01, 0.3)));
    blisters.append(&flank.both_sides());
    let engines = vec![
        Engine {
            at: Vec3::new(-1.0, 0.0, 0.11),
            radius: 0.085,
        },
        Engine {
            at: Vec3::new(-1.0, 0.0, -0.11),
            radius: 0.085,
        },
    ];
    let hull = body
        .with(&house)
        .with(&mast)
        .with(&blisters)
        .with(&nozzle(engines[0].at, engines[0].radius, 0.09).both_sides());
    ClassModel {
        hull,
        engines,
        break_at: 0.0,
    }
}

/// Kreuzer: langer, schmaler Rumpf mit Breitseitenkasematten an beiden Flanken.
fn cruiser() -> ClassModel {
    let body = MeshData::loft(
        &[
            hull_ring(-0.94, 0.10, 0.09, -0.08, 0.03),
            hull_ring(-0.7, 0.13, 0.11, -0.11, 0.04),
            hull_ring(0.2, 0.13, 0.11, -0.11, 0.04),
            hull_ring(0.7, 0.10, 0.09, -0.09, 0.035),
            hull_ring(1.0, 0.02, 0.035, -0.035, 0.008),
        ],
        HULL,
    );
    let sponson = MeshData::loft(
        &[
            rect_ring(-0.56, (-0.04, 0.04), (0.11, 0.15)),
            rect_ring(-0.45, (-0.075, 0.075), (0.11, 0.275)),
            rect_ring(0.33, (-0.075, 0.075), (0.11, 0.275)),
            rect_ring(0.46, (-0.04, 0.04), (0.11, 0.15)),
        ],
        PLATE,
    );
    let mut guns = MeshData::default();
    for x in [-0.36, -0.16, 0.04, 0.24] {
        guns.append(&along(
            &frustum(Vec3::ZERO, Vec3::X * 0.085, (0.024, 0.019), 6, DARK),
            Vec3::Z,
            Vec3::new(x, 0.0, 0.27),
        ));
    }
    let tower = MeshData::loft(
        &[
            rect_ring(-0.62, (0.11, 0.2), (-0.075, 0.075)),
            rect_ring(-0.4, (0.11, 0.25), (-0.075, 0.075)),
            rect_ring(-0.3, (0.11, 0.25), (-0.06, 0.06)),
            rect_ring(-0.22, (0.11, 0.14), (-0.05, 0.05)),
        ],
        PLATE,
    );
    let mast = cuboid(
        Vec3::new(-0.5, 0.25, -0.012),
        Vec3::new(-0.42, 0.34, 0.012),
        TRIM,
    );
    let keel = MeshData::loft(
        &[
            rect_ring(-0.35, (-0.115, -0.105), (-0.02, 0.02)),
            rect_ring(-0.2, (-0.2, -0.105), (-0.02, 0.02)),
            rect_ring(0.2, (-0.2, -0.105), (-0.02, 0.02)),
            rect_ring(0.4, (-0.115, -0.105), (-0.02, 0.02)),
        ],
        TRIM,
    );
    let bow_turret = turret(0.085, 0.13, PLATE).translated(Vec3::new(0.47, 0.105, 0.0));
    let engines = vec![
        Engine {
            at: Vec3::new(-1.0, 0.0, 0.058),
            radius: 0.058,
        },
        Engine {
            at: Vec3::new(-1.0, 0.0, -0.058),
            radius: 0.058,
        },
    ];
    let stripe = cuboid(
        Vec3::new(-0.4, 0.075, 0.16),
        Vec3::new(0.28, 0.081, 0.245),
        TRIM,
    );
    let hull = body
        .with(&sponson.both_sides())
        .with(&stripe.both_sides())
        .with(&guns.both_sides())
        .with(&tower)
        .with(&mast)
        .with(&keel)
        .with(&bow_turret)
        .with(&nozzle(engines[0].at, engines[0].radius, 0.07).both_sides());
    ClassModel {
        hull,
        engines,
        break_at: 0.02,
    }
}

/// Schlachtschiff: breiter, gepanzerter Keil mit Panzergürtel und drei schweren Türmen.
fn battleship() -> ClassModel {
    let body = MeshData::loft(
        &[
            hull_ring(-0.95, 0.24, 0.12, -0.13, 0.05),
            hull_ring(-0.5, 0.31, 0.15, -0.17, 0.07),
            hull_ring(0.3, 0.29, 0.15, -0.17, 0.07),
            hull_ring(0.75, 0.19, 0.12, -0.14, 0.05),
            hull_ring(1.0, 0.07, 0.06, -0.08, 0.025),
        ],
        HULL,
    );
    let belt = MeshData::loft(
        &[
            rect_ring(-0.68, (-0.06, 0.04), (0.27, 0.3)),
            rect_ring(-0.55, (-0.1, 0.085), (0.27, 0.37)),
            rect_ring(0.36, (-0.1, 0.085), (0.27, 0.37)),
            rect_ring(0.52, (-0.06, 0.04), (0.2, 0.25)),
        ],
        PLATE,
    );
    let barbette = cuboid(
        Vec3::new(0.12, 0.15, -0.1),
        Vec3::new(0.34, 0.2, 0.1),
        PLATE,
    );
    let front = turret(0.14, 0.2, HULL).translated(Vec3::new(0.54, 0.14, 0.0));
    let raised = turret(0.14, 0.2, HULL).translated(Vec3::new(0.24, 0.2, 0.0));
    let rear = turret(0.14, 0.2, HULL)
        .transformed(Affine3A::from_rotation_y(std::f32::consts::PI))
        .translated(Vec3::new(-0.62, 0.15, 0.0));
    let citadel = MeshData::loft(
        &[
            rect_ring(-0.38, (0.15, 0.24), (-0.13, 0.13)),
            rect_ring(-0.02, (0.15, 0.27), (-0.13, 0.13)),
            rect_ring(0.06, (0.15, 0.2), (-0.1, 0.1)),
        ],
        PLATE,
    );
    let bridge = MeshData::loft(
        &[
            rect_ring(-0.28, (0.26, 0.36), (-0.07, 0.07)),
            rect_ring(-0.1, (0.26, 0.4), (-0.07, 0.07)),
            rect_ring(-0.04, (0.26, 0.3), (-0.05, 0.05)),
        ],
        HULL,
    );
    let mast = cuboid(
        Vec3::new(-0.24, 0.38, -0.012),
        Vec3::new(-0.18, 0.47, 0.012),
        TRIM,
    );
    let lance = frustum(
        Vec3::new(0.3, -0.16, 0.1),
        Vec3::new(0.88, -0.115, 0.075),
        (0.036, 0.026),
        6,
        TRIM,
    );
    let engines = vec![
        Engine {
            at: Vec3::new(-1.0, -0.01, 0.17),
            radius: 0.062,
        },
        Engine {
            at: Vec3::new(-1.0, -0.01, -0.17),
            radius: 0.062,
        },
        Engine {
            at: Vec3::new(-1.0, -0.01, 0.055),
            radius: 0.052,
        },
        Engine {
            at: Vec3::new(-1.0, -0.01, -0.055),
            radius: 0.052,
        },
    ];
    let mut nozzles = MeshData::default();
    for engine in &engines {
        nozzles.append(&nozzle(engine.at, engine.radius, 0.06));
    }
    let stripe = cuboid(
        Vec3::new(-0.5, 0.085, 0.285),
        Vec3::new(0.3, 0.091, 0.35),
        TRIM,
    );
    let hull = body
        .with(&belt.both_sides())
        .with(&stripe.both_sides())
        .with(&barbette)
        .with(&front)
        .with(&raised)
        .with(&rear)
        .with(&citadel)
        .with(&bridge)
        .with(&mast)
        .with(&lance.both_sides())
        .with(&nozzles);
    ClassModel {
        hull,
        engines,
        break_at: 0.09,
    }
}

/// Träger: breiter, flacher Rumpf mit zwei Flugdecks und offenen Hangars vorn und achtern.
fn carrier() -> ClassModel {
    let body = MeshData::loft(
        &[
            hull_ring(-0.95, 0.2, 0.08, -0.09, 0.03),
            hull_ring(-0.7, 0.27, 0.09, -0.1, 0.035),
            hull_ring(0.5, 0.27, 0.09, -0.1, 0.035),
            hull_ring(0.8, 0.12, 0.06, -0.07, 0.025),
        ],
        HULL,
    );
    let deck = cuboid(
        Vec3::new(-0.76, -0.08, 0.31),
        Vec3::new(0.985, 0.06, 0.56),
        HULL,
    );
    // Hangaröffnungen: dunkle Flächen knapp vor den Stirnseiten der Flugdecks.
    let mouth_front = cuboid(
        Vec3::new(0.985, -0.06, 0.33),
        Vec3::new(1.0, 0.04, 0.54),
        DARK,
    );
    let mouth_rear = cuboid(
        Vec3::new(-0.775, -0.06, 0.33),
        Vec3::new(-0.76, 0.04, 0.54),
        DARK,
    );
    let mut bays = MeshData::default();
    for x in [-0.45, 0.05, 0.55] {
        bays.append(&cuboid(
            Vec3::new(x - 0.13, -0.055, 0.56),
            Vec3::new(x + 0.13, 0.03, 0.566),
            DARK,
        ));
    }
    let bridge_arm = MeshData::loft(
        &[
            rect_ring(-0.62, (-0.05, 0.04), (0.24, 0.33)),
            rect_ring(0.5, (-0.05, 0.04), (0.24, 0.33)),
            rect_ring(0.68, (-0.04, 0.03), (0.3, 0.33)),
        ],
        PLATE,
    );
    let stripe = cuboid(
        Vec3::new(-0.66, 0.06, 0.425),
        Vec3::new(0.9, 0.064, 0.445),
        TRIM,
    );
    let island = MeshData::loft(
        &[
            rect_ring(-0.36, (0.09, 0.2), (0.09, 0.22)),
            rect_ring(0.02, (0.09, 0.22), (0.09, 0.22)),
            rect_ring(0.1, (0.09, 0.13), (0.11, 0.2)),
        ],
        PLATE,
    );
    let top = MeshData::loft(
        &[
            rect_ring(-0.28, (0.2, 0.27), (0.12, 0.2)),
            rect_ring(-0.08, (0.2, 0.29), (0.12, 0.2)),
            rect_ring(-0.02, (0.2, 0.23), (0.13, 0.19)),
        ],
        HULL,
    );
    let mast = cuboid(
        Vec3::new(-0.22, 0.28, 0.15),
        Vec3::new(-0.17, 0.36, 0.17),
        TRIM,
    );
    let engines = vec![
        Engine {
            at: Vec3::new(-1.0, -0.005, 0.135),
            radius: 0.055,
        },
        Engine {
            at: Vec3::new(-1.0, -0.005, -0.135),
            radius: 0.055,
        },
        Engine {
            at: Vec3::new(-1.0, -0.005, 0.045),
            radius: 0.045,
        },
        Engine {
            at: Vec3::new(-1.0, -0.005, -0.045),
            radius: 0.045,
        },
    ];
    let mut nozzles = MeshData::default();
    for engine in &engines {
        nozzles.append(&nozzle(engine.at, engine.radius, 0.06));
    }
    let hull = body
        .with(&deck.both_sides())
        .with(&mouth_front.both_sides())
        .with(&mouth_rear.both_sides())
        .with(&bays.both_sides())
        .with(&bridge_arm.both_sides())
        .with(&stripe.both_sides())
        .with(&island)
        .with(&top)
        .with(&mast)
        .with(&nozzles);
    ClassModel {
        hull,
        engines,
        break_at: 0.13,
    }
}

// --- Effekte ---------------------------------------------------------------------------

/// Leuchtspur eines Geschosses: gestreckter Doppelkegel entlang x, Länge 1, Dicke 1.
pub fn tracer() -> MeshData {
    MeshData::loft(
        &[
            ring(Vec3::X * -0.5, 4, 0.0, 0.0, 0.0),
            ring(Vec3::X * 0.15, 4, 0.5, 0.5, 0.0),
            ring(Vec3::X * 0.5, 4, 0.0, 0.0, 0.0),
        ],
        [1.0; 4],
    )
}

/// Lenkwaffe: heller Körper (Länge 1) mit kurzem, dunkler werdendem Schweif dahinter.
pub fn missile() -> MeshData {
    let section = |x: f32, radius: f32| ring(Vec3::X * x, 6, radius, radius, 0.0);
    let body = MeshData::loft(
        &[section(-0.5, 0.32), section(0.15, 0.5), section(0.5, 0.0)],
        [1.0; 4],
    );
    let trail = MeshData::loft_painted(&[
        (section(-4.5, 0.0), [0.08, 0.02, 0.01, 1.0]),
        (section(-0.5, 0.36), [0.9, 0.5, 0.2, 1.0]),
    ]);
    body.with(&trail)
}

/// Strahl: sechseckiger Stab von x = 0 bis x = 1, Dicke 1.
pub fn beam() -> MeshData {
    MeshData::loft(
        &[
            ring(Vec3::ZERO, 6, 0.5, 0.5, 0.0),
            ring(Vec3::X, 6, 0.5, 0.5, 0.0),
        ],
        [1.0; 4],
    )
}

/// Verbindet Ringe gleicher Eckenzahl zu einer offenen Fläche; jeder Ring hat seine
/// eigene Deckkraft, dazwischen verläuft sie.
fn band(rings: &[(Vec<Vec3>, f32)]) -> MeshData {
    let mut mesh = MeshData::default();
    let paint = |alpha: f32| [1.0, 1.0, 1.0, alpha];
    for pair in rings.windows(2) {
        let ((back, back_alpha), (front, front_alpha)) = (&pair[0], &pair[1]);
        let sides = back.len();
        for index in 0..sides {
            let next = (index + 1) % sides;
            mesh.tri_painted(
                [back[index], front[index], front[next]],
                [paint(*back_alpha), paint(*front_alpha), paint(*front_alpha)],
            );
            mesh.tri_painted(
                [back[index], front[next], back[next]],
                [paint(*back_alpha), paint(*front_alpha), paint(*back_alpha)],
            );
        }
    }
    mesh
}

/// Schildschimmer: Kappe einer Kugel (Radius 1) um +x, gut 60° weit. In der Mitte deckend,
/// zum Rand hin durchsichtig, damit nur die getroffene Stelle aufleuchtet.
pub fn shield_cap() -> MeshData {
    let (steps, sides) = (5, 18);
    let widest = 64f32.to_radians();
    let rings: Vec<(Vec<Vec3>, f32)> = (0..=steps)
        .map(|step| {
            let inward = step as f32 / steps as f32;
            let angle = widest * (1.0 - inward);
            let radius = angle.sin();
            (
                ring(Vec3::X * angle.cos(), sides, radius, radius, 0.0),
                inward.powf(1.5),
            )
        })
        .collect();
    band(&rings)
}

/// Leuchtscheibe für Einschläge: Kreis mit Radius 1 in der x-y-Ebene, Vorderseite nach +z.
/// In der Mitte deckend, zum Rand hin weich auslaufend. Wird stets der Kamera zugewandt.
pub fn glow_disc() -> MeshData {
    let sides = 20;
    let circle = |radius: f32| -> Vec<Vec3> {
        (0..sides)
            .map(|index| {
                let angle = TAU * index as f32 / sides as f32;
                Vec3::new(radius * angle.cos(), radius * angle.sin(), 0.0)
            })
            .collect()
    };
    band(&[
        (circle(1.0), 0.0),
        (circle(0.62), 0.16),
        (circle(0.3), 0.6),
        (circle(0.0), 1.0),
    ])
}

/// Drei unregelmäßige Trümmerstücke, jedes etwa eine Einheit groß.
pub fn debris() -> [MeshData; 3] {
    let shard = MeshData::loft(
        &[
            vec![
                Vec3::new(-0.5, -0.12, 0.3),
                Vec3::new(-0.45, 0.16, 0.22),
                Vec3::new(-0.4, 0.1, -0.28),
                Vec3::new(-0.5, -0.1, -0.2),
            ],
            vec![
                Vec3::new(0.5, -0.03, 0.08),
                Vec3::new(0.42, 0.06, 0.05),
                Vec3::new(0.45, 0.03, -0.07),
                Vec3::new(0.5, -0.04, -0.05),
            ],
        ],
        PLATE,
    );
    let plate = MeshData::loft(
        &[
            vec![
                Vec3::new(-0.45, -0.05, 0.42),
                Vec3::new(-0.5, 0.05, 0.36),
                Vec3::new(-0.38, 0.06, -0.4),
                Vec3::new(-0.42, -0.04, -0.3),
            ],
            vec![
                Vec3::new(0.4, -0.06, 0.25),
                Vec3::new(0.5, 0.04, 0.3),
                Vec3::new(0.32, 0.05, -0.45),
                Vec3::new(0.3, -0.05, -0.38),
            ],
        ],
        HULL,
    );
    let block = MeshData::loft(
        &[
            ring(Vec3::X * -0.35, 5, 0.3, 0.36, 0.4),
            ring(Vec3::new(0.1, 0.05, 0.0), 5, 0.26, 0.2, 0.9),
            ring(Vec3::new(0.4, -0.04, 0.03), 5, 0.1, 0.14, 0.2),
        ],
        DARK,
    );
    [shard, plate, block]
}

/// Sternenhimmel: kleine Vierecke auf einer Kugel um den Ursprung, dem Mittelpunkt
/// zugewandt. `sample` liefert Zufallszahlen in 0..1 (nur Kosmetik).
pub fn star_field(count: usize, radius: f32, mut sample: impl FnMut() -> f32) -> MeshData {
    let mut mesh = MeshData::default();
    for _ in 0..count {
        // Gleichmäßig auf der Kugel.
        let height = 2.0 * sample() - 1.0;
        let angle = TAU * sample();
        let ring_radius = (1.0 - height * height).max(0.0).sqrt();
        let direction = Vec3::new(ring_radius * angle.cos(), height, ring_radius * angle.sin());
        // Wenige helle, viele schwache Sterne; leichte Farbstiche.
        let brightness = 0.16 + 2.4 * sample().powi(6);
        let warmth = sample();
        let paint = [
            brightness * (0.85 + 0.3 * warmth),
            brightness * 0.95,
            brightness * (1.15 - 0.3 * warmth),
            1.0,
        ];
        let size = radius * (0.00045 + 0.0008 * sample().powi(3));
        let side = direction.any_orthonormal_vector();
        let up = direction.cross(side);
        let center = direction * radius;
        // Ecken so, dass die Vorderseite zum Mittelpunkt zeigt.
        mesh.quad(
            center - side * size - up * size,
            center - side * size + up * size,
            center + side * size + up * size,
            center + side * size - up * size,
            paint,
        );
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;

    /// Erwartete halbe Breite und Höhenbereich je Klasse (normiert). Die Zahlen halten den
    /// Charakter der Umrisse fest: Träger breit und flach, Kreuzer schmal, Fregatte gedrungen.
    fn intended(class: ShipClass) -> (f32, (f32, f32)) {
        match class {
            ShipClass::Fighter => (0.64, (-0.13, 0.17)),
            ShipClass::Corvette => (0.462, (-0.10, 0.27)),
            ShipClass::Frigate => (0.385, (-0.255, 0.45)),
            ShipClass::Cruiser => (0.355, (-0.2, 0.34)),
            ShipClass::Battleship => (0.37, (-0.17, 0.47)),
            ShipClass::Carrier => (0.566, (-0.1, 0.36)),
        }
    }

    fn check(mesh: &MeshData, what: &str) {
        let vertices = mesh.positions.len();
        assert!(vertices > 0, "{what}: leer");
        assert_eq!(mesh.normals.len(), vertices, "{what}: Normalen");
        assert_eq!(mesh.colors.len(), vertices, "{what}: Farben");
        assert_eq!(mesh.indices.len() % 3, 0, "{what}: Indexzahl");
        assert!(
            mesh.indices.iter().all(|index| (*index as usize) < vertices),
            "{what}: Index außerhalb"
        );
        for index in 0..mesh.triangle_count() {
            let [a, b, c] = mesh.triangle(index);
            let cross = (b - a).cross(c - a);
            assert!(cross.length() > 1e-8, "{what}: entartetes Dreieck {index}");
            // Die gespeicherte Normale gehört zur Vorderseite des Dreiecks.
            for offset in 0..3 {
                let normal = mesh.normals[mesh.indices[index * 3 + offset] as usize];
                assert!((normal.length() - 1.0).abs() < 1e-4, "{what}: Normale nicht normiert");
                assert!(normal.dot(cross.normalize()) > 0.999, "{what}: Normale falsch herum");
            }
        }
        assert!(
            mesh.positions.iter().all(|p| p.is_finite()),
            "{what}: keine endlichen Koordinaten"
        );
    }

    #[test]
    fn primitives_are_closed_and_face_outward() {
        let unit = cuboid(Vec3::splat(-0.5), Vec3::splat(0.5), HULL);
        check(&unit, "Quader");
        assert_eq!(unit.triangle_count(), 12);
        assert!((unit.signed_volume() - 1.0).abs() < 1e-5);
        assert!((unit.surface_area() - 6.0).abs() < 1e-5);
        // Spiegeln, Drehen und Verschieben erhalten Außenseite und Volumen.
        let moved = along(&unit.mirrored_z(), Vec3::new(0.3, 1.0, -0.2), Vec3::splat(4.0));
        check(&moved, "bewegter Quader");
        assert!((moved.signed_volume() - 1.0).abs() < 1e-4);
        // Ein Kegel hat eine Spitze: Die entarteten Dreiecke dort entfallen.
        let cone = frustum(Vec3::ZERO, Vec3::X, (1.0, 0.0), 12, HULL);
        check(&cone, "Kegel");
        assert_eq!(cone.triangle_count(), 12 + 10);
        assert!(cone.signed_volume() > 0.9 && cone.signed_volume() < 1.05);
        for (mesh, what) in [
            (dome(1.0, 0.8, 8, HULL), "Kuppel"),
            (turret(1.0, 1.0, HULL), "Turm"),
            (tracer(), "Leuchtspur"),
            (beam(), "Strahl"),
        ] {
            check(&mesh, what);
            assert!(mesh.signed_volume() > 0.0, "{what}: Flächen zeigen nach innen");
        }
        check(&missile(), "Lenkwaffe");
        // Schild und Leuchtscheibe sind offene Flächen mit auslaufender Deckkraft.
        let shield = shield_cap();
        check(&shield, "Schild");
        let (min, max) = shield.bounds();
        assert!(min.x > 0.4 && (max.x - 1.0).abs() < 1e-5, "Kappe um +x");
        let disc = glow_disc();
        check(&disc, "Leuchtscheibe");
        assert!(disc.normals.iter().all(|normal| normal.z > 0.999), "Scheibe zeigt nach +z");
        for mesh in [&shield, &disc] {
            let alphas: Vec<f32> = mesh.colors.iter().map(|color| color[3]).collect();
            assert!(alphas.iter().any(|alpha| *alpha == 0.0), "Rand läuft aus");
            assert!(alphas.iter().any(|alpha| *alpha == 1.0), "Mitte deckt");
        }
        for piece in debris() {
            check(&piece, "Trümmer");
            assert!(piece.signed_volume() > 0.0);
            let (min, max) = piece.bounds();
            assert!((max - min).max_element() <= 1.01);
        }
    }

    #[test]
    fn every_class_mesh_is_valid_and_has_its_intended_size() {
        for class in ShipClass::ALL {
            let model = ClassModel::of(class);
            let what = class.label();
            check(&model.hull, what);
            assert!(model.hull.signed_volume() > 0.0, "{what}: Flächen zeigen nach innen");
            let (min, max) = model.hull.bounds();
            // Länge genau 2: Bug bei +1, Heck bei −1.
            assert!((max.x - 1.0).abs() < 1e-4, "{what}: Bug bei {}", max.x);
            assert!((min.x + 1.0).abs() < 1e-4, "{what}: Heck bei {}", min.x);
            // Spiegelsymmetrisch in der Breite (der Träger trägt seine Insel an Steuerbord,
            // die Flugdecks sind aber gleich breit).
            let (half_width, (bottom, top)) = intended(class);
            assert!((max.z - half_width).abs() < 0.01, "{what}: Breite {}", max.z);
            assert!((min.z + half_width).abs() < 0.01, "{what}: Breite {}", min.z);
            assert!((max.y - top).abs() < 0.01, "{what}: Höhe {}", max.y);
            assert!((min.y - bottom).abs() < 0.01, "{what}: Tiefe {}", min.y);
            // Jedes Triebwerk sitzt an der Heckebene und innerhalb des Rumpfs.
            assert!(!model.engines.is_empty(), "{what}: kein Triebwerk");
            for engine in &model.engines {
                assert!((engine.at.x + 1.0).abs() < 1e-5);
                assert!(engine.at.z.abs() + engine.radius <= half_width + 1e-3);
            }
            let glow = model.glow();
            check(&glow, what);
            let (glow_min, glow_max) = glow.bounds();
            assert!(glow_max.x.abs() < 1e-5 && glow_min.x < -0.2, "{what}: Triebwerksstrahl");
        }
    }

    #[test]
    fn classes_are_distinguishable_by_their_proportions() {
        let width = |class: ShipClass| ClassModel::of(class).hull.bounds().1.z;
        let height = |class: ShipClass| {
            let (min, max) = ClassModel::of(class).hull.bounds();
            max.y - min.y
        };
        // Der Träger ist der breiteste und im Verhältnis flachste Rumpf.
        for class in ShipClass::ALL {
            if class != ShipClass::Carrier && class != ShipClass::Fighter {
                assert!(width(ShipClass::Carrier) > width(class) + 0.09);
            }
        }
        assert!(height(ShipClass::Carrier) / width(ShipClass::Carrier) < 0.85);
        // Der Kreuzer ist schlanker als Schlachtschiff und Fregatte.
        assert!(width(ShipClass::Cruiser) < width(ShipClass::Battleship));
        assert!(width(ShipClass::Cruiser) < width(ShipClass::Frigate));
        // Die Fregatte ist das gedrungenste Schiff: am höchsten im Verhältnis zur Länge.
        for class in [ShipClass::Corvette, ShipClass::Cruiser, ShipClass::Carrier] {
            assert!(height(ShipClass::Frigate) > height(class));
        }
        // Kein Umriss gleicht einem anderen: Die Flächen unterscheiden sich deutlich.
        let areas: Vec<f32> = ShipClass::ALL
            .iter()
            .map(|class| ClassModel::of(*class).hull.surface_area())
            .collect();
        for a in 0..areas.len() {
            for b in (a + 1)..areas.len() {
                assert!((areas[a] - areas[b]).abs() > 0.05, "{a} und {b} sind gleich groß");
            }
        }
    }

    #[test]
    fn fore_and_aft_halves_cover_the_hull() {
        for class in ShipClass::ALL.into_iter().filter(|class| class.is_capital()) {
            let model = ClassModel::of(class);
            let what = class.label();
            let (fore, aft) = model.hull.split_x(model.break_at);
            check(&fore, what);
            check(&aft, what);
            // Schneiden verliert und erfindet keine Fläche.
            let whole = model.hull.surface_area();
            let parts = fore.surface_area() + aft.surface_area();
            assert!((whole - parts).abs() < 1e-3 * whole, "{what}: {whole} gegen {parts}");
            // Jede Hälfte bleibt auf ihrer Seite, zusammen reichen sie von Heck bis Bug.
            let (hull_min, hull_max) = model.hull.bounds();
            let (fore_min, fore_max) = fore.bounds();
            let (aft_min, aft_max) = aft.bounds();
            assert!(fore_min.x >= model.break_at - 1e-5 && aft_max.x <= model.break_at + 1e-5);
            assert!((fore_max.x - hull_max.x).abs() < 1e-5 && (aft_min.x - hull_min.x).abs() < 1e-5);
            assert!((fore_min.min(aft_min) - hull_min).length() < 1e-4);
            assert!((fore_max.max(aft_max) - hull_max).length() < 1e-4);
            // Die verschlossenen Hälften sind gültige Meshes mit einer Bruchfläche, die zur
            // jeweils anderen Hälfte zeigt.
            let (capped_fore, capped_aft) = model.halves();
            check(&capped_fore, what);
            check(&capped_aft, what);
            assert!(capped_fore.triangle_count() > fore.triangle_count());
            assert!(capped_aft.triangle_count() > aft.triangle_count());
            let cap_normal = |mesh: &MeshData| *mesh.normals.last().unwrap();
            assert!(cap_normal(&capped_fore).x < -0.99 && cap_normal(&capped_aft).x > 0.99);
        }
    }

    #[test]
    fn bevy_mesh_attributes_match_the_builder() {
        for class in ShipClass::ALL {
            let data = ClassModel::of(class).hull;
            let mesh = data.to_mesh();
            let count = data.positions.len();
            assert_eq!(mesh.count_vertices(), count);
            let Some(VertexAttributeValues::Float32x3(normals)) =
                mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
            else {
                panic!("Normalen fehlen");
            };
            let Some(VertexAttributeValues::Float32x4(colors)) =
                mesh.attribute(Mesh::ATTRIBUTE_COLOR)
            else {
                panic!("Farben fehlen");
            };
            assert_eq!(normals.len(), count);
            assert_eq!(colors.len(), count);
            let Some(Indices::U32(indices)) = mesh.indices() else {
                panic!("Indizes fehlen");
            };
            assert_eq!(indices.len(), data.indices.len());
            assert!(indices.iter().all(|index| (*index as usize) < count));
        }
    }

    #[test]
    fn star_field_faces_the_viewer_inside() {
        let mut state = 0.37f32;
        let stars = star_field(200, 1000.0, || {
            state = (state * 97.31 + 0.137).fract();
            state
        });
        check(&stars, "Sterne");
        assert_eq!(stars.triangle_count(), 400);
        for index in 0..stars.triangle_count() {
            let [a, b, c] = stars.triangle(index);
            let center = (a + b + c) / 3.0;
            let normal = (b - a).cross(c - a);
            assert!(normal.dot(center) < 0.0, "Stern zeigt nach außen");
            assert!((center.length() - 1000.0).abs() < 5.0);
        }
    }
}

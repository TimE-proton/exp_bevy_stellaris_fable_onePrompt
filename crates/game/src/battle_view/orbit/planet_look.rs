//! Aussehen eines Planeten in der Orbitansicht: prozedurale Oberfläche als Textur.
//!
//! Aus dem, was der Kern über eine Welt weiß (Art, Temperatur, Wasser, Atmosphäre,
//! Bevölkerung), entstehen deterministisch – Seed ist die Kennung des Planeten –
//! äquirektangulare Bilder: Farbe, Normalen aus einer Höhenkarte, Rauheit, Leuchten
//! (Glut, Stadtlichter) und eine Wolkendecke. Abgetastet wird ein 3D-Gradientenrauschen
//! auf der Einheitskugel; deshalb gibt es weder eine Naht an der Datumsgrenze noch
//! verzerrte Pole.
//!
//! Alles hier ist reine Rechnung ohne Engine: Das Spiel ruft [`generate`] abseits des
//! Hauptfadens auf und macht aus dem Ergebnis mit [`Layer::image`] Texturen.

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::math::{Quat, Vec3};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use sim::GameData;
use sim::data::Atmosphere;
use sim::galaxy::Planet;
use std::f32::consts::{PI, TAU};
use std::sync::atomic::{AtomicBool, Ordering};

/// Größe der fertigen Oberfläche (äquirektangular, 2:1).
pub const FULL: (usize, usize) = (2048, 1024);
/// Größe des schnellen Vorabbilds, das bis zur fertigen Oberfläche gezeigt wird.
pub const PREVIEW: (usize, usize) = (512, 256);

/// Gestaltung je Planetenart (`key` in `planet_kinds.ron`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Style {
    Garden,
    Ocean,
    Steppe,
    Ice,
    Rock,
    Lava,
    Toxic,
    Gas,
}

impl Style {
    #[cfg(test)]
    pub const ALL: [Style; 8] = [
        Style::Garden,
        Style::Ocean,
        Style::Steppe,
        Style::Ice,
        Style::Rock,
        Style::Lava,
        Style::Toxic,
        Style::Gas,
    ];

    /// Gestaltung zur Art aus den Spieldaten. Unbekannte Arten sehen aus wie Fels.
    pub fn of_key(key: &str) -> Style {
        match key {
            "gartenwelt" => Style::Garden,
            "ozeanwelt" => Style::Ocean,
            "steppenwelt" => Style::Steppe,
            "eiswelt" => Style::Ice,
            "glutwelt" => Style::Lava,
            "giftwelt" => Style::Toxic,
            "gasriese" => Style::Gas,
            _ => Style::Rock,
        }
    }

    /// Wie stark die Höhenkarte das Licht formt (Neigung je Höheneinheit).
    fn relief(self) -> f32 {
        match self {
            Style::Garden | Style::Ocean | Style::Toxic => 0.025,
            Style::Steppe => 0.03,
            Style::Ice => 0.012,
            Style::Rock => 0.11,
            Style::Lava => 0.04,
            Style::Gas => 0.0,
        }
    }
}

/// Was in das Aussehen eingeht. Gleiche Eingabe, gleiches Bild.
#[derive(Clone, Debug, PartialEq)]
pub struct LookInput {
    pub seed: u64,
    pub style: Style,
    /// Mittlere Temperatur in °C.
    pub temperature: f32,
    /// Wasseranteil 0..1.
    pub water: f32,
    pub atmosphere: Atmosphere,
    /// Bevölkerung im Verhältnis zur Tragfähigkeit, 0..1: Stadtlichter auf der Nachtseite.
    pub settled: f32,
    /// Farbe der Art aus den Spieldaten (sRGB), Grundton der Gasriesen.
    pub tint: [f32; 3],
}

impl LookInput {
    pub fn of(planet: &Planet, data: &GameData) -> Self {
        let kind = data.planet_kinds.get(planet.kind as usize);
        let style = kind.map_or(Style::Rock, |kind| Style::of_key(&kind.key));
        let tint = kind.map_or([0.55, 0.52, 0.5], |kind| {
            [kind.color.0, kind.color.1, kind.color.2]
        });
        let settled = if planet.capacity > 0.0 {
            (planet.population / planet.capacity).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        Self {
            seed: u64::from(planet.id.0)
                .wrapping_mul(0x9e37_79b9_7f4a_7c15)
                .wrapping_add(0x4b4f_4e46),
            style,
            temperature: planet.temperature,
            water: planet.water.clamp(0.0, 1.0),
            atmosphere: planet.atmosphere,
            settled,
            tint,
        }
    }

    /// Saum der Atmosphäre: Farbe (linear) und Stärke. Luftlose Welten haben keinen; ein
    /// Gasriese ist ganz Atmosphäre und trägt einen weichen Dunst in seiner Farbe.
    pub fn haze(&self) -> Option<Haze> {
        if self.style == Style::Gas {
            let tint = srgb_to_linear(mix3(self.tint, [0.9, 0.9, 0.95], 0.5));
            return Some(Haze {
                color: tint,
                strength: 0.55,
                thickness: 0.022,
            });
        }
        let (color, strength, thickness) = match self.atmosphere {
            Atmosphere::None => return None,
            Atmosphere::Thin => ([0.55, 0.65, 0.95], 0.45, 0.012),
            Atmosphere::Breathable => ([0.30, 0.55, 1.0], 1.0, 0.016),
            Atmosphere::Dense => ([0.42, 0.62, 1.0], 1.35, 0.022),
            Atmosphere::Toxic => ([0.62, 0.85, 0.25], 0.85, 0.02),
            Atmosphere::Corrosive => ([1.0, 0.62, 0.25], 1.0, 0.022),
        };
        // Über der Glut färbt sich jeder Dunst rötlich.
        let color = if self.style == Style::Lava {
            mix3(color, [1.0, 0.42, 0.18], 0.7)
        } else {
            color
        };
        Some(Haze {
            color,
            strength,
            thickness,
        })
    }

    /// Wolken: Farbe (sRGB), bedeckter Anteil und größte Deckkraft. Keine ohne Luft.
    fn clouds(&self) -> Option<([f32; 3], f32, f32)> {
        if self.style == Style::Gas {
            return None;
        }
        // Wo viel Wasser ist, ziehen mehr Wolken; über trockenen Welten weniger.
        let wet = (0.45 + 0.75 * self.water).min(1.15);
        match (self.atmosphere, self.style) {
            (Atmosphere::None, _) => None,
            (Atmosphere::Thin, _) => Some(([0.95, 0.95, 0.97], 0.1, 0.4)),
            (Atmosphere::Breathable, _) => Some(([1.0, 1.0, 1.0], 0.34 * wet, 0.85)),
            (Atmosphere::Dense, _) => Some(([0.98, 0.98, 1.0], 0.46 * wet, 0.88)),
            (Atmosphere::Toxic | Atmosphere::Corrosive, Style::Lava) => {
                Some(([0.42, 0.36, 0.32], 0.3, 0.7))
            }
            (Atmosphere::Toxic, _) => Some(([0.84, 0.84, 0.46], 0.4, 0.75)),
            (Atmosphere::Corrosive, _) => Some(([0.88, 0.74, 0.46], 0.48, 0.8)),
        }
    }
}

/// Saum der Atmosphäre.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Haze {
    /// Lineare Farbe.
    pub color: [f32; 3],
    pub strength: f32,
    /// Höhe, auf die der Saum abfällt, in Halbmessern des Planeten.
    pub thickness: f32,
}

/// Ein Bild mit allen Mip-Stufen.
#[derive(Clone, Debug)]
pub struct Layer {
    pub width: usize,
    pub height: usize,
    /// RGBA8, Stufe 0 zuerst.
    pub data: Vec<u8>,
    pub levels: u32,
    pub srgb: bool,
}

impl Layer {
    fn new(width: usize, height: usize, base: Vec<u8>, srgb: bool) -> Self {
        let (data, levels) = with_mips(width, height, base);
        Self {
            width,
            height,
            data,
            levels,
            srgb,
        }
    }

    /// RGBA-Wert eines Bildpunkts der Stufe 0.
    #[cfg(test)]
    pub fn pixel(&self, x: usize, y: usize) -> [u8; 4] {
        let at = (y * self.width + x) * 4;
        [
            self.data[at],
            self.data[at + 1],
            self.data[at + 2],
            self.data[at + 3],
        ]
    }

    /// Mittlere Farbe der Stufe 0 (0..1, nach Fläche gewichtet).
    #[cfg(test)]
    pub fn mean(&self) -> [f32; 4] {
        let mut sum = [0.0f64; 4];
        let mut weight = 0.0f64;
        for y in 0..self.height {
            let w = ((y as f64 + 0.5) / self.height as f64 * std::f64::consts::PI).sin();
            for x in 0..self.width {
                let pixel = self.pixel(x, y);
                for channel in 0..4 {
                    sum[channel] += w * f64::from(pixel[channel]) / 255.0;
                }
                weight += w;
            }
        }
        sum.map(|value| (value / weight) as f32)
    }

    /// Textur für die Engine: linear gefiltert, mit Mip-Stufen, in Längsrichtung
    /// umlaufend (keine Naht an der Datumsgrenze).
    pub fn image(self) -> Image {
        let format = if self.srgb {
            TextureFormat::Rgba8UnormSrgb
        } else {
            TextureFormat::Rgba8Unorm
        };
        let mut image = Image::new_uninit(
            Extent3d {
                width: self.width as u32,
                height: self.height as u32,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            format,
            RenderAssetUsages::default(),
        );
        image.data = Some(self.data);
        image.texture_descriptor.mip_level_count = self.levels;
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::ClampToEdge,
            mag_filter: ImageFilterMode::Linear,
            min_filter: ImageFilterMode::Linear,
            mipmap_filter: ImageFilterMode::Linear,
            anisotropy_clamp: 8,
            ..default_sampler()
        });
        image
    }
}

fn default_sampler() -> ImageSamplerDescriptor {
    ImageSamplerDescriptor::default()
}

/// Halbiert ein RGBA8-Bild (Mittel aus 2×2).
fn halve(width: usize, height: usize, data: &[u8]) -> Vec<u8> {
    let (w, h) = ((width / 2).max(1), (height / 2).max(1));
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            for channel in 0..4 {
                let mut sum = 0u32;
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let sx = (x * 2 + dx).min(width - 1);
                    let sy = (y * 2 + dy).min(height - 1);
                    sum += u32::from(data[(sy * width + sx) * 4 + channel]);
                }
                out[(y * w + x) * 4 + channel] = ((sum + 2) / 4) as u8;
            }
        }
    }
    out
}

/// Hängt die Mip-Stufen an (Mittel aus 2×2, bis 1×1).
fn with_mips(width: usize, height: usize, base: Vec<u8>) -> (Vec<u8>, u32) {
    let mut data = base;
    let (mut w, mut h) = (width, height);
    let mut start = 0;
    let mut levels = 1;
    while w > 1 || h > 1 {
        let next = halve(w, h, &data[start..]);
        start = data.len();
        data.extend_from_slice(&next);
        (w, h) = ((w / 2).max(1), (h / 2).max(1));
        levels += 1;
    }
    (data, levels)
}

/// Die fertige Oberfläche einer Welt.
#[derive(Clone, Debug)]
pub struct Surface {
    pub albedo: Layer,
    /// Normalen im Tangentenraum (Tangente nach Osten, Bitangente nach Süden).
    pub normal: Option<Layer>,
    /// Grün: Rauheit, Blau: Metall (stets 0).
    pub roughness: Layer,
    /// Leuchten (sRGB), nur wo etwas glüht oder Städte leuchten.
    pub emissive: Option<Layer>,
    /// Wolken: weiß bzw. getönt, Deckkraft im Alpha-Kanal.
    pub clouds: Option<Layer>,
}

// --- Rauschen --------------------------------------------------------------------------

/// Zufallsfolge zum Würfeln der Tabellen (splitmix64).
struct Dice(u64);

impl Dice {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }

    fn direction(&mut self) -> Vec3 {
        let y = self.unit() * 2.0 - 1.0;
        let around = self.unit() * TAU;
        let ring = (1.0 - y * y).max(0.0).sqrt();
        Vec3::new(ring * around.cos(), y, ring * around.sin())
    }
}

/// 3D-Gradientenrauschen (verbessertes Perlin-Rauschen) mit eigener Permutation.
struct Noise {
    perm: [u8; 512],
    /// Versatz je Oktave, damit sich die Gitter der Oktaven nicht decken.
    offsets: [[f32; 3]; 8],
}

#[inline]
fn fade(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[inline]
fn grad(hash: u8, x: f32, y: f32, z: f32) -> f32 {
    match hash & 15 {
        0 => x + y,
        1 => -x + y,
        2 => x - y,
        3 => -x - y,
        4 => x + z,
        5 => -x + z,
        6 => x - z,
        7 => -x - z,
        8 => y + z,
        9 => -y + z,
        10 => y - z,
        11 => -y - z,
        12 => x + y,
        13 => -x + y,
        14 => -y + z,
        _ => -y - z,
    }
}

impl Noise {
    fn new(seed: u64) -> Self {
        let mut dice = Dice(seed);
        let mut table: [u8; 256] = std::array::from_fn(|index| index as u8);
        for index in (1..256).rev() {
            let other = (dice.next() % (index as u64 + 1)) as usize;
            table.swap(index, other);
        }
        let perm = std::array::from_fn(|index| table[index & 255]);
        let offsets = std::array::from_fn(|_| {
            [
                dice.unit() * 97.0 - 48.0,
                dice.unit() * 97.0 - 48.0,
                dice.unit() * 97.0 - 48.0,
            ]
        });
        Self { perm, offsets }
    }

    /// Rauschen in etwa −1..1.
    #[inline]
    fn at(&self, x: f32, y: f32, z: f32) -> f32 {
        let (fx, fy, fz) = (x.floor(), y.floor(), z.floor());
        let (x, y, z) = (x - fx, y - fy, z - fz);
        let xi = (fx as i32 & 255) as usize;
        let yi = (fy as i32 & 255) as usize;
        let zi = (fz as i32 & 255) as usize;
        let (u, v, w) = (fade(x), fade(y), fade(z));
        let p = &self.perm;
        let a = p[xi] as usize + yi;
        let aa = p[a] as usize + zi;
        let ab = p[a + 1] as usize + zi;
        let b = p[xi + 1] as usize + yi;
        let ba = p[b] as usize + zi;
        let bb = p[b + 1] as usize + zi;
        lerp(
            lerp(
                lerp(grad(p[aa], x, y, z), grad(p[ba], x - 1.0, y, z), u),
                lerp(
                    grad(p[ab], x, y - 1.0, z),
                    grad(p[bb], x - 1.0, y - 1.0, z),
                    u,
                ),
                v,
            ),
            lerp(
                lerp(
                    grad(p[aa + 1], x, y, z - 1.0),
                    grad(p[ba + 1], x - 1.0, y, z - 1.0),
                    u,
                ),
                lerp(
                    grad(p[ab + 1], x, y - 1.0, z - 1.0),
                    grad(p[bb + 1], x - 1.0, y - 1.0, z - 1.0),
                    u,
                ),
                v,
            ),
            w,
        )
    }

    #[inline]
    fn at_point(&self, p: Vec3) -> f32 {
        self.at(p.x, p.y, p.z)
    }

    /// Fraktales Rauschen aus `octaves` Oktaven, etwa −1..1.
    fn fbm(&self, p: Vec3, octaves: usize) -> f32 {
        let mut sum = 0.0;
        let mut amplitude = 0.5;
        let mut frequency = 1.0;
        for offset in self.offsets.iter().take(octaves) {
            sum += amplitude
                * self.at(
                    p.x * frequency + offset[0],
                    p.y * frequency + offset[1],
                    p.z * frequency + offset[2],
                );
            amplitude *= 0.5;
            frequency *= 2.03;
        }
        sum * 1.6
    }

    /// Kammrauschen für Gebirge und Risse, 0..1: hoch auf den Graten.
    fn ridged(&self, p: Vec3, octaves: usize) -> f32 {
        let mut sum = 0.0;
        let mut amplitude = 0.5;
        let mut frequency = 1.0;
        let mut weight = 1.0;
        let mut norm = 0.0;
        for offset in self.offsets.iter().take(octaves) {
            let n = self.at(
                p.x * frequency + offset[1],
                p.y * frequency + offset[2],
                p.z * frequency + offset[0],
            );
            let ridge = (1.0 - n.abs() * 1.4).max(0.0);
            let ridge = ridge * ridge * weight;
            weight = (ridge * 1.8).clamp(0.0, 1.0);
            sum += ridge * amplitude;
            norm += amplitude;
            amplitude *= 0.5;
            frequency *= 2.1;
        }
        sum / norm
    }

    /// Rauschen über einem verbogenen Raum: Küsten und Wirbel statt runder Flecken.
    fn warped(&self, p: Vec3, scale: f32, warp: f32, octaves: usize) -> f32 {
        let q = p * scale;
        let w = Vec3::new(
            self.fbm(q + Vec3::new(5.2, 1.3, 2.8), 2),
            self.fbm(q + Vec3::new(1.7, 9.2, 4.1), 2),
            self.fbm(q + Vec3::new(8.3, 2.8, 7.4), 2),
        );
        // Die groben Oktaven folgen der Verbiegung ganz, die feinen immer weniger: So
        // biegen sich Küsten und Wirbel, ohne dass das Feine zu Schlieren verschmiert.
        let mut sum = 0.0;
        let mut amplitude = 0.5;
        let mut frequency = 1.0;
        for (octave, offset) in self.offsets.iter().take(octaves).enumerate() {
            let at = (q + w * (warp / (1.0 + 0.6 * octave as f32))) * frequency;
            sum += amplitude * self.at(at.x + offset[0], at.y + offset[1], at.z + offset[2]);
            amplitude *= 0.5;
            frequency *= 2.03;
        }
        sum * 1.6
    }
}

// --- Hilfen -----------------------------------------------------------------------------

#[inline]
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[inline]
fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

#[inline]
fn scale3(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn srgb_to_linear(c: [f32; 3]) -> [f32; 3] {
    c.map(|v| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    })
}

#[inline]
fn byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// Richtung vom Mittelpunkt zum Bildpunkt (Mitte des Pixels). Zeile 0 liegt am Nordpol
/// (+y), die Spalten laufen von x über z nach −x: dieselbe Abbildung wie die Kugel in
/// `meshes::uv_globe`.
pub fn direction(x: usize, y: usize, width: usize, height: usize) -> Vec3 {
    let polar = PI * (y as f32 + 0.5) / height as f32;
    let around = TAU * (x as f32 + 0.5) / width as f32;
    Vec3::new(
        polar.sin() * around.cos(),
        polar.cos(),
        polar.sin() * around.sin(),
    )
}

/// Teilt die Zeilen eines Bilds auf die Kerne des Rechners auf. `row` füllt eine Zeile
/// (Index, Ausschnitt). Liefert `false`, wenn abgebrochen wurde.
fn rows_parallel<T: Send>(
    buffer: &mut [T],
    width: usize,
    cancel: &AtomicBool,
    row: impl Fn(usize, &mut [T]) + Sync,
) -> bool {
    let height = buffer.len() / width;
    let workers = std::thread::available_parallelism()
        .map_or(2, |count| count.get())
        .clamp(1, 8);
    let band = height.div_ceil(workers).max(1);
    std::thread::scope(|scope| {
        for (index, chunk) in buffer.chunks_mut(band * width).enumerate() {
            let row = &row;
            scope.spawn(move || {
                for (offset, line) in chunk.chunks_mut(width).enumerate() {
                    if cancel.load(Ordering::Relaxed) {
                        return;
                    }
                    row(index * band + offset, line);
                }
            });
        }
    });
    !cancel.load(Ordering::Relaxed)
}

/// Wert, unter dem der Anteil `share` der Fläche liegt (Zeilen nach ihrer Fläche
/// gewichtet).
fn area_quantile(values: &[f32], width: usize, share: f32) -> f32 {
    let height = values.len() / width;
    let (mut low, mut high) = (f32::MAX, f32::MIN);
    for value in values {
        low = low.min(*value);
        high = high.max(*value);
    }
    if high <= low {
        return low;
    }
    const BINS: usize = 2048;
    let mut bins = vec![0.0f64; BINS];
    let mut total = 0.0;
    for y in 0..height {
        let weight = (PI * (y as f32 + 0.5) / height as f32).sin() as f64;
        for value in &values[y * width..(y + 1) * width] {
            let bin = (((value - low) / (high - low)) * (BINS - 1) as f32) as usize;
            bins[bin.min(BINS - 1)] += weight;
            total += weight;
        }
    }
    let wanted = total * f64::from(share.clamp(0.0, 1.0));
    let mut sum = 0.0;
    for (index, bin) in bins.iter().enumerate() {
        sum += bin;
        if sum >= wanted {
            return low + (high - low) * (index as f32 + 0.5) / (BINS - 1) as f32;
        }
    }
    high
}

// --- Gelände ----------------------------------------------------------------------------

/// Was die erste Runde je Bildpunkt festhält: Höhe, Feuchte bzw. Risse, Feinstruktur.
#[derive(Clone, Copy, Default)]
struct Field {
    height: f32,
    moist: f32,
    detail: f32,
}

fn field(noise: &Noise, style: Style, p: Vec3) -> Field {
    match style {
        Style::Garden | Style::Steppe | Style::Toxic | Style::Ocean => {
            let (scale, warp) = if style == Style::Ocean {
                (2.1, 0.75)
            } else {
                (1.35, 0.8)
            };
            let base = noise.warped(p, scale, warp, 7);
            let mountains = noise.ridged(p * 2.6 + Vec3::new(3.1, 7.7, 1.9), 5);
            let inland = smoothstep(-0.05, 0.35, base);
            Field {
                height: base + 0.42 * mountains * (0.25 + 0.75 * inland),
                moist: noise.fbm(p * 2.2 + Vec3::new(11.0, 3.0, 7.0), 4),
                detail: noise.fbm(p * 21.0 + Vec3::new(2.0, 5.0, 9.0), 2),
            }
        }
        Style::Ice => {
            let base = noise.warped(p, 1.7, 0.8, 6);
            let wide = 1.0 - noise.fbm(p * 2.4 + Vec3::new(4.0, 1.0, 6.0), 3).abs();
            let fine = 1.0 - noise.fbm(p * 7.5 + Vec3::new(9.0, 2.0, 3.0), 2).abs();
            let cracks = (wide.powi(14)).max(0.65 * fine.powi(18));
            Field {
                height: base,
                moist: cracks,
                detail: noise.fbm(p * 16.0, 3),
            }
        }
        Style::Rock => Field {
            height: noise.fbm(p * 2.2, 6) * 0.3 + 0.08 * noise.ridged(p * 3.0, 3),
            moist: noise.fbm(p * 1.3 + Vec3::new(7.0, 7.0, 1.0), 3),
            detail: noise.fbm(p * 9.0, 4) * 0.55,
        },
        Style::Lava => {
            let base = noise.warped(p, 1.6, 1.2, 6);
            let q = p * 3.6;
            let warp = Vec3::new(
                noise.fbm(q + Vec3::new(1.0, 4.0, 2.0), 2),
                noise.fbm(q + Vec3::new(6.0, 2.0, 8.0), 2),
                noise.fbm(q + Vec3::new(3.0, 9.0, 5.0), 2),
            );
            // Nähte zwischen erkalteten Schollen: Nulllinien einzelner Oktaven, grob und fein.
            let wide = 1.0 - noise.at_point(q + warp * 0.7).abs();
            let fine = 1.0
                - noise
                    .at_point(p * 11.0 + warp * 0.4 + Vec3::splat(3.3))
                    .abs();
            let seams = smoothstep(0.93, 0.995, wide).max(0.7 * smoothstep(0.94, 0.995, fine));
            let embers = 0.2 * smoothstep(0.8, 0.95, wide);
            Field {
                height: base,
                moist: seams.max(embers),
                detail: noise.fbm(p * 20.0, 2),
            }
        }
        Style::Gas => {
            // Bänder nach Breite, verwirbelt; die Turbulenz ist in Längsrichtung gestreckt.
            let stretched = Vec3::new(p.x * 2.2, p.y * 9.0, p.z * 2.2);
            let turbulence = noise.warped(stretched, 1.0, 1.6, 5);
            Field {
                height: p.y + 0.05 * turbulence,
                moist: noise.fbm(Vec3::new(p.x * 4.0, p.y * 30.0, p.z * 4.0), 4),
                detail: 0.0,
            }
        }
    }
}

/// Krater auf der Kugel: Mitte, Winkelhalbmesser, Tiefe, Helligkeit des Auswurfs.
struct Crater {
    center: Vec3,
    radius: f32,
    depth: f32,
    rays: f32,
}

fn craters(seed: u64, style: Style) -> Vec<Crater> {
    let count = match style {
        Style::Rock => 700,
        Style::Ice => 40,
        _ => 0,
    };
    let mut dice = Dice(seed ^ 0x0c7a_7e75);
    (0..count)
        .map(|_| {
            let u = dice.unit();
            // Wenige große, viele kleine (Potenzgesetz).
            let radius = (0.008 * (1.0 - u * 0.985).powf(-1.0 / 1.3)).min(0.28);
            let young = dice.unit();
            Crater {
                center: dice.direction(),
                radius,
                depth: 1.3 * radius.powf(0.85),
                rays: if young > 0.94 {
                    0.5 + young - 0.94
                } else {
                    0.0
                },
            }
        })
        .collect()
}

/// Prägt die Krater in eine Zeile.
fn stamp_craters(craters: &[&Crater], y: usize, width: usize, height: usize, line: &mut [Field]) {
    for (x, cell) in line.iter_mut().enumerate() {
        let p = direction(x, y, width, height);
        for crater in craters {
            let cos = p.dot(crater.center);
            if cos < (crater.radius * 2.6).min(PI).cos() {
                continue;
            }
            let distance = cos.clamp(-1.0, 1.0).acos() / crater.radius;
            if distance < 1.0 {
                cell.height += crater.depth * (distance * distance - 1.0) * 0.9;
            }
            let rim = (distance - 1.0) / 0.22;
            let ridge = (-rim * rim).exp();
            cell.height += crater.depth * 0.35 * ridge;
            // Helle Wälle, dunklere Böden, bei jungen Kratern ein heller Auswurf.
            cell.detail += 0.35 * ridge;
            if crater.rays > 0.0 && distance > 0.9 {
                let streak = (distance - 0.9) / 1.7;
                cell.detail += crater.rays * (1.0 - streak).max(0.0) * 0.9;
            } else if distance < 1.0 {
                cell.detail -= 0.35 * (1.0 - distance * distance);
            }
        }
    }
}

// --- Farben -----------------------------------------------------------------------------

/// Höhenlagen, die die Farbgebung braucht (aus der Verteilung der Höhen).
struct Levels {
    sea: f32,
    low: f32,
    high: f32,
    /// Für die Glutwelt: Spiegel der Lavaseen.
    lava: f32,
}

/// Farbe (sRGB), Rauheit und Leuchten (sRGB, 0..1) eines Bildpunkts.
struct Shade {
    color: [f32; 3],
    roughness: f32,
    glow: [f32; 3],
}

fn shade(input: &LookInput, levels: &Levels, cell: Field, p: Vec3) -> Shade {
    let lat = p.y.clamp(-1.0, 1.0).asin().abs() / (PI / 2.0);
    let style = input.style;
    match style {
        Style::Garden | Style::Ocean | Style::Steppe | Style::Toxic => {
            terrestrial(input, levels, cell, lat)
        }
        Style::Ice => {
            let ice = mix3(
                [0.70, 0.78, 0.86],
                [0.93, 0.95, 0.98],
                cell.detail * 0.8 + 0.55,
            );
            let lines = mix3([0.55, 0.40, 0.31], [0.36, 0.48, 0.60], lat);
            let mut color = mix3(ice, lines, cell.moist * 0.85);
            let mut roughness = 0.55;
            // Offene Rinnen und Meeresöffnungen in den Senken.
            let open = smoothstep(levels.sea + 0.02, levels.sea - 0.015, cell.height);
            let fringe = smoothstep(levels.sea + 0.06, levels.sea + 0.01, cell.height);
            color = mix3(color, [0.62, 0.72, 0.82], fringe * 0.5);
            color = mix3(color, [0.10, 0.17, 0.26], open);
            roughness = lerp(roughness, 0.3, open);
            // Fels ragt auf trockenen Eiswelten aus dem Panzer.
            let rock = smoothstep(levels.high - 0.04, levels.high + 0.04, cell.height);
            color = mix3(color, [0.42, 0.42, 0.44], rock * 0.7);
            Shade {
                color,
                roughness,
                glow: [0.0; 3],
            }
        }
        Style::Rock => {
            let light = smoothstep(-0.8, 0.8, cell.detail);
            let mut color = mix3([0.32, 0.31, 0.30], [0.58, 0.56, 0.53], light);
            let maria = smoothstep(0.05, 0.35, cell.moist);
            color = mix3(color, scale3(color, 0.55), maria * 0.8);
            let hot = smoothstep(40.0, 140.0, input.temperature);
            color = [
                color[0] * (1.0 + 0.12 * hot),
                color[1] * (1.0 - 0.02 * hot),
                color[2] * (1.0 - 0.14 * hot),
            ];
            Shade {
                color,
                roughness: 0.95,
                glow: [0.0; 3],
            }
        }
        Style::Lava => {
            let crust = mix3(
                [0.05, 0.045, 0.045],
                [0.21, 0.17, 0.14],
                smoothstep(-0.5, 0.6, cell.detail),
            );
            let ash = smoothstep(levels.high - 0.1, levels.high + 0.05, cell.height);
            let crust = mix3(crust, [0.34, 0.31, 0.29], ash * 0.7);
            let lake = smoothstep(levels.lava + 0.01, levels.lava - 0.05, cell.height);
            let cracks = cell.moist * (1.0 - 0.7 * ash);
            let heat = lake.max(cracks).min(1.0);
            let glow = if heat < 0.35 {
                mix3([0.0, 0.0, 0.0], [0.45, 0.04, 0.0], heat / 0.35)
            } else if heat < 0.75 {
                mix3([0.45, 0.04, 0.0], [1.0, 0.32, 0.03], (heat - 0.35) / 0.4)
            } else {
                mix3([1.0, 0.32, 0.03], [1.0, 0.72, 0.25], (heat - 0.75) / 0.25)
            };
            Shade {
                color: mix3(crust, [0.30, 0.07, 0.02], heat),
                roughness: lerp(0.9, 0.55, heat),
                glow,
            }
        }
        Style::Gas => gas(input, cell, p),
    }
}

/// Gartenwelt, Ozeanwelt, Steppe und Giftwelt: Meer, Küste, Land nach Breite und
/// Temperatur, Gebirge und Eis.
fn terrestrial(input: &LookInput, levels: &Levels, cell: Field, lat: f32) -> Shade {
    let style = input.style;
    let elevation = ((cell.height - levels.sea) / (levels.high - levels.sea).max(1e-3)).max(0.0);
    let depth = ((levels.sea - cell.height) / (levels.sea - levels.low).max(1e-3)).max(0.0);
    let local =
        input.temperature + 16.0 - 52.0 * lat.powf(1.6) - 22.0 * elevation + 5.0 * cell.detail;
    let (deep, mid, shelf) = match style {
        Style::Toxic => ([0.20, 0.27, 0.03], [0.42, 0.50, 0.06], [0.68, 0.74, 0.18]),
        Style::Steppe => ([0.03, 0.12, 0.16], [0.05, 0.22, 0.26], [0.18, 0.42, 0.42]),
        Style::Ocean => ([0.01, 0.04, 0.14], [0.02, 0.12, 0.30], [0.08, 0.48, 0.55]),
        _ => ([0.01, 0.035, 0.12], [0.02, 0.10, 0.26], [0.06, 0.32, 0.42]),
    };
    let mut glow = [0.0; 3];
    let (mut color, mut roughness) = if cell.height < levels.sea {
        let water = mix3(mid, deep, smoothstep(0.05, 0.6, depth));
        let water = mix3(shelf, water, smoothstep(0.0, 0.07, depth));
        (water, if style == Style::Toxic { 0.22 } else { 0.3 })
    } else {
        let land = match style {
            Style::Toxic => {
                let veg = smoothstep(-0.25, 0.3, cell.moist);
                let low = mix3([0.52, 0.44, 0.24], [0.38, 0.38, 0.16], veg);
                let high = mix3(low, [0.30, 0.26, 0.19], smoothstep(0.35, 0.7, elevation));
                let sulfur = smoothstep(0.35, 0.7, cell.detail) * smoothstep(0.1, 0.4, elevation);
                mix3(high, [0.80, 0.74, 0.30], sulfur * 0.7)
            }
            Style::Steppe => {
                let wet = cell.moist - 0.05;
                let rust = mix3(
                    [0.62, 0.38, 0.22],
                    [0.78, 0.62, 0.38],
                    smoothstep(-0.3, 0.1, wet),
                );
                let grass = mix3(rust, [0.46, 0.45, 0.24], smoothstep(0.1, 0.45, wet));
                let rock = mix3(grass, [0.45, 0.36, 0.30], smoothstep(0.4, 0.8, elevation));
                // Salzpfannen in den Senken nahe dem Wasserspiegel.
                let salt = smoothstep(0.06, 0.0, elevation) * smoothstep(-0.2, 0.3, cell.detail);
                mix3(rock, [0.88, 0.86, 0.80], salt * 0.8)
            }
            _ => {
                let wet_band = if lat < 0.75 {
                    0.22 * (lat * 11.4).cos()
                } else {
                    0.0
                };
                let wet = cell.moist + wet_band;
                let veg = smoothstep(-0.3, 0.25, wet);
                let hot = smoothstep(18.0, 30.0, local);
                let cold = smoothstep(8.0, -2.0, local);
                let dry = mix3([0.55, 0.50, 0.28], [0.80, 0.66, 0.44], hot);
                let lush = mix3([0.13, 0.27, 0.10], [0.08, 0.22, 0.06], hot);
                let lush = mix3(lush, [0.15, 0.22, 0.14], cold);
                let dry = mix3(dry, [0.44, 0.42, 0.34], cold);
                let grass = mix3([0.30, 0.40, 0.16], [0.36, 0.38, 0.26], cold);
                let land = if veg < 0.5 {
                    mix3(dry, grass, veg * 2.0)
                } else {
                    mix3(grass, lush, (veg - 0.5) * 2.0)
                };
                mix3(land, [0.40, 0.36, 0.32], smoothstep(0.45, 0.85, elevation))
            }
        };
        let land = scale3(land, 0.9 + 0.2 * (cell.detail * 0.5 + 0.5));
        // Ein schmaler, heller Saum an der Küste.
        let beach = if style == Style::Toxic {
            0.0
        } else {
            smoothstep(0.02, 0.0, elevation) * 0.55
        };
        let land = mix3(land, [0.76, 0.70, 0.52], beach);
        if input.settled > 0.0 && style != Style::Toxic {
            let towns = smoothstep(0.42, 0.66, cell.detail)
                * smoothstep(0.75, 0.2, elevation)
                * smoothstep(-6.0, 6.0, local);
            let lights = towns * (0.25 + 0.75 * input.settled.sqrt());
            glow = scale3([1.0, 0.8, 0.52], lights);
        }
        (land, 0.88)
    };
    // Eis: an Land nach Temperatur und Höhe, auf dem Meer als Packeis.
    let frost = if style == Style::Toxic {
        0.0
    } else {
        smoothstep(-2.0, -9.0, local)
    };
    color = mix3(color, [0.90, 0.93, 0.97], frost);
    roughness = lerp(roughness, 0.6, frost);
    glow = scale3(glow, 1.0 - frost);
    Shade {
        color,
        roughness,
        glow,
    }
}

/// Gasriese: Bänder, Streifen und Wirbelstürme.
fn gas(input: &LookInput, cell: Field, p: Vec3) -> Shade {
    let tint = input.tint;
    let cold = smoothstep(-140.0, -180.0, input.temperature) * 0.75;
    let tint = mix3(tint, [0.55, 0.72, 0.86], cold);
    let palette = [
        scale3(tint, 0.48),
        [tint[0] * 0.86, tint[1] * 0.62, tint[2] * 0.48],
        tint,
        mix3(tint, [1.0, 0.97, 0.9], 0.45),
        mix3(tint, [0.97, 0.95, 0.92], 0.78),
    ];
    let bands = 9.0 + (input.seed % 5) as f32;
    let phase = (input.seed % 7) as f32;
    let wave = (cell.height * bands * PI + phase).sin() * 0.6
        + (cell.height * bands * 2.3 * PI + phase * 2.0).sin() * 0.25
        + cell.moist * 0.35;
    let at = (wave * 0.5 + 0.5).clamp(0.0, 0.999) * (palette.len() - 1) as f32;
    let index = at as usize;
    let mut color = mix3(palette[index], palette[index + 1], at - index as f32);
    // Wirbel: ein großer ovaler Sturm und kleinere weiße Flecken.
    let mut dice = Dice(input.seed ^ 0x0005_7073);
    for storm in 0..4 {
        let lat = (dice.unit() - 0.5) * 1.1;
        let lon = dice.unit() * TAU;
        let size = if storm == 0 {
            0.24
        } else {
            0.05 + 0.04 * dice.unit()
        };
        let center = Vec3::new(lat.cos() * lon.cos(), lat.sin(), lat.cos() * lon.sin());
        let east = Vec3::Y.cross(center).normalize_or_zero();
        let north = center.cross(east);
        let offset = p - center;
        let (u, v) = (offset.dot(east) / size, offset.dot(north) / (size * 0.55));
        let r = (u * u + v * v).sqrt();
        if r < 1.4 && p.dot(center) > 0.0 {
            let swirl = (v.atan2(u) * 2.0 + r * 9.0).sin() * 0.5 + 0.5;
            let core = if storm == 0 {
                [0.78, 0.40, 0.26]
            } else {
                [0.96, 0.94, 0.9]
            };
            let ring = mix3(core, palette[3], swirl * 0.5);
            color = mix3(color, ring, smoothstep(1.3, 0.7, r));
        }
    }
    Shade {
        color,
        roughness: 0.9,
        glow: [0.0; 3],
    }
}

// --- Erzeugung --------------------------------------------------------------------------

/// Erzeugt die Oberfläche in der Größe `width × height` (2:1). `None`, wenn `cancel`
/// unterwegs gesetzt wurde.
pub fn generate(
    input: &LookInput,
    (width, height): (usize, usize),
    cancel: &AtomicBool,
) -> Option<Surface> {
    let noise = Noise::new(input.seed);
    let style = input.style;

    // Erste Runde: Höhe, Feuchte, Feinstruktur.
    let mut fields = vec![Field::default(); width * height];
    let all_craters = craters(input.seed, style);
    let finished = rows_parallel(&mut fields, width, cancel, |y, line| {
        for (x, cell) in line.iter_mut().enumerate() {
            *cell = field(&noise, style, direction(x, y, width, height));
        }
        if !all_craters.is_empty() {
            let polar = PI * (y as f32 + 0.5) / height as f32;
            let near: Vec<&Crater> = all_craters
                .iter()
                .filter(|crater| {
                    let crater_polar = crater.center.y.clamp(-1.0, 1.0).acos();
                    (crater_polar - polar).abs() < crater.radius * 2.7
                })
                .collect();
            stamp_craters(&near, y, width, height, line);
        }
    });
    if !finished {
        return None;
    }

    // Lagen aus der Verteilung der Höhen.
    let heights: Vec<f32> = fields.iter().map(|cell| cell.height).collect();
    let sea_share = match style {
        Style::Garden | Style::Ocean | Style::Steppe | Style::Toxic => input.water,
        Style::Ice => 0.03 + 0.07 * smoothstep(-160.0, -30.0, input.temperature),
        _ => 0.0,
    };
    let levels = Levels {
        sea: if sea_share > 0.0 {
            area_quantile(&heights, width, sea_share)
        } else {
            f32::MIN
        },
        low: area_quantile(&heights, width, 0.01),
        high: area_quantile(
            &heights,
            width,
            match style {
                Style::Ice => 1.0 - (1.0 - input.water) * 0.15,
                _ => 0.985,
            },
        ),
        lava: area_quantile(&heights, width, 0.05),
    };
    let levels = Levels {
        low: levels.low.min(levels.sea - 1e-3),
        ..levels
    };

    // Zweite Runde: Farbe, Rauheit, Leuchten.
    let mut albedo = vec![0u8; width * height * 4];
    let mut rough = vec![0u8; width * height * 4];
    let mut glow = vec![0u8; width * height * 4];
    let mut glows = false;
    {
        let mut rows: Vec<(&mut [u8], &mut [u8], &mut [u8])> = albedo
            .chunks_mut(width * 4)
            .zip(rough.chunks_mut(width * 4))
            .zip(glow.chunks_mut(width * 4))
            .map(|((a, r), g)| (a, r, g))
            .collect();
        let lit = std::sync::atomic::AtomicBool::new(false);
        let finished = rows_parallel(&mut rows, 1, cancel, |y, line| {
            let (albedo, rough, glow) = &mut line[0];
            let mut any = false;
            for x in 0..width {
                let cell = fields[y * width + x];
                let s = shade(input, &levels, cell, direction(x, y, width, height));
                let at = x * 4;
                albedo[at..at + 4].copy_from_slice(&[
                    byte(s.color[0]),
                    byte(s.color[1]),
                    byte(s.color[2]),
                    255,
                ]);
                rough[at..at + 4].copy_from_slice(&[255, byte(s.roughness), 0, 255]);
                let g = [byte(s.glow[0]), byte(s.glow[1]), byte(s.glow[2]), 255];
                any |= g[0] > 0 || g[1] > 0 || g[2] > 0;
                glow[at..at + 4].copy_from_slice(&g);
            }
            if any {
                lit.store(true, Ordering::Relaxed);
            }
        });
        if !finished {
            return None;
        }
        glows = glows || lit.load(Ordering::Relaxed);
    }

    // Normalen aus der Höhe (Meere glatt).
    let normal = (style.relief() > 0.0).then(|| {
        let relief = style.relief();
        let ground: Vec<f32> = heights.iter().map(|h| h.max(levels.sea)).collect();
        let mut normal = vec![0u8; width * height * 4];
        let _ = rows_parallel(&mut normal, width * 4, cancel, |y, line| {
            let polar = PI * (y as f32 + 0.5) / height as f32;
            let east_step = TAU * polar.sin().max(0.02) / width as f32;
            let south_step = PI / height as f32;
            let up = y.saturating_sub(1);
            let down = (y + 1).min(height - 1);
            for x in 0..width {
                let left = (x + width - 1) % width;
                let right = (x + 1) % width;
                let dx = (ground[y * width + right] - ground[y * width + left]) * relief
                    / (2.0 * east_step);
                let dy = (ground[down * width + x] - ground[up * width + x]) * relief
                    / ((down - up).max(1) as f32 * south_step);
                let n = Vec3::new(-dx, -dy, 1.0).normalize();
                let at = x * 4;
                line[at..at + 4].copy_from_slice(&[
                    byte(n.x * 0.5 + 0.5),
                    byte(n.y * 0.5 + 0.5),
                    byte(n.z * 0.5 + 0.5),
                    255,
                ]);
            }
        });
        Layer::new(width, height, normal, false)
    });
    if cancel.load(Ordering::Relaxed) {
        return None;
    }

    let clouds = match input.clouds() {
        Some(look) => Some(clouds(input, look, (width, height), cancel)?),
        None => None,
    };

    Some(Surface {
        albedo: Layer::new(width, height, albedo, true),
        normal,
        // Die Rauheit braucht keine volle Auflösung: halb so groß, viermal weniger zum
        // Hochladen.
        roughness: Layer::new(width / 2, height / 2, halve(width, height, &rough), false),
        emissive: glows.then(|| Layer::new(width, height, glow, true)),
        clouds,
    })
}

/// Wolkendecke: verwirbelte Felder, dichter in den Zonen der Tiefdruckgürtel.
fn clouds(
    input: &LookInput,
    (color, cover, opacity): ([f32; 3], f32, f32),
    (width, height): (usize, usize),
    cancel: &AtomicBool,
) -> Option<Layer> {
    let noise = Noise::new(input.seed ^ 0xc10d);
    let banded = matches!(input.atmosphere, Atmosphere::Toxic | Atmosphere::Corrosive);
    // Wirbelstürme: Um ihre Mitte wird der Raum verdreht, die Wolken ziehen Spiralen.
    let mut dice = Dice(input.seed ^ 0x5717);
    let storms: Vec<(Vec3, f32, f32)> = (0..5)
        .map(|_| {
            let lat = (0.25 + 0.5 * dice.unit()) * if dice.unit() < 0.5 { -1.0 } else { 1.0 };
            let lon = dice.unit() * TAU;
            let center = Vec3::new(lat.cos() * lon.cos(), lat.sin(), lat.cos() * lon.sin());
            // Drehsinn nach Halbkugel.
            (center, 0.07 + 0.08 * dice.unit(), 3.5 * lat.signum())
        })
        .collect();
    let mut values = vec![0.0f32; width * height];
    let mut fine = vec![0.0f32; width * height];
    let mut both: Vec<(&mut [f32], &mut [f32])> = values
        .chunks_mut(width)
        .zip(fine.chunks_mut(width))
        .collect();
    let finished = rows_parallel(&mut both, 1, cancel, |y, line| {
        let (values, fine) = &mut line[0];
        for x in 0..width {
            let mut p = direction(x, y, width, height);
            for (center, size, turns) in &storms {
                let near = p.dot(*center);
                if near > 0.9 {
                    let distance = near.clamp(-1.0, 1.0).acos() / size;
                    let angle = turns * (-distance * distance).exp();
                    p = Quat::from_axis_angle(*center, angle) * p;
                }
            }
            let lat = p.y.clamp(-1.0, 1.0).asin().abs() / (PI / 2.0);
            let swirl = noise.warped(p, 2.8, 0.8, 7);
            let streaks = if banded {
                0.3 * noise.fbm(Vec3::new(p.x * 2.0, p.y * 10.0, p.z * 2.0), 3)
            } else {
                0.0
            };
            let belts = 0.12 * (lat * 11.0).cos() + 0.1 * smoothstep(0.55, 0.75, lat);
            values[x] = swirl + streaks + belts;
            fine[x] = noise.fbm(p * 11.0 + Vec3::splat(4.4), 5);
        }
    });
    if !finished {
        return None;
    }
    let threshold = area_quantile(&values, width, 1.0 - cover);
    let peak = area_quantile(&values, width, 0.995).max(threshold + 1e-3);
    let mut data = vec![0u8; width * height * 4];
    for (index, value) in values.iter().enumerate() {
        // Die Feinstruktur verschiebt den Rand (fransig); innen wächst die Dichte zu den
        // Kernen hin, so bleibt Struktur in den Feldern.
        let detail = fine[index];
        let density = (*value + 0.25 * detail - threshold) / (peak - threshold);
        let edge = smoothstep(0.0, 0.12, density);
        let core = 0.5 + 0.5 * smoothstep(0.05, 0.6, density);
        let texture = 0.8 + 0.2 * smoothstep(-0.4, 0.4, detail);
        let alpha = edge * core * texture * opacity;
        // Dichte Wolken sind innen etwas grauer: So zeichnet sich ihre Form ab.
        let shade = 1.0 - 0.1 * smoothstep(0.6, 1.0, alpha / opacity.max(1e-3));
        let at = index * 4;
        data[at..at + 4].copy_from_slice(&[
            byte(color[0] * shade),
            byte(color[1] * shade),
            byte(color[2] * shade),
            byte(alpha),
        ]);
    }
    Some(Layer::new(width, height, data, true))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(style: Style) -> LookInput {
        let (temperature, water, atmosphere) = match style {
            Style::Garden => (16.0, 0.6, Atmosphere::Breathable),
            Style::Ocean => (18.0, 0.92, Atmosphere::Dense),
            Style::Steppe => (34.0, 0.15, Atmosphere::Thin),
            Style::Ice => (-90.0, 0.8, Atmosphere::None),
            Style::Rock => (20.0, 0.02, Atmosphere::None),
            Style::Lava => (600.0, 0.0, Atmosphere::None),
            Style::Toxic => (90.0, 0.3, Atmosphere::Toxic),
            Style::Gas => (-120.0, 0.0, Atmosphere::None),
        };
        LookInput {
            seed: 77,
            style,
            temperature,
            water,
            atmosphere,
            settled: 0.5,
            tint: [0.8, 0.66, 0.5],
        }
    }

    const SMALL: (usize, usize) = (128, 64);

    fn make(style: Style) -> Surface {
        generate(&input(style), SMALL, &AtomicBool::new(false)).expect("nicht abgebrochen")
    }

    #[test]
    fn the_same_seed_gives_the_same_surface_and_another_seed_another() {
        let a = make(Style::Garden);
        let b = make(Style::Garden);
        assert_eq!(a.albedo.data, b.albedo.data);
        assert_eq!(
            a.clouds.as_ref().map(|layer| &layer.data),
            b.clouds.as_ref().map(|layer| &layer.data)
        );
        let other = generate(
            &LookInput {
                seed: 78,
                ..input(Style::Garden)
            },
            SMALL,
            &AtomicBool::new(false),
        )
        .expect("nicht abgebrochen");
        assert_ne!(a.albedo.data, other.albedo.data);
    }

    #[test]
    fn there_is_no_seam_at_the_date_line() {
        // Die erste und die letzte Spalte liegen auf der Kugel nebeneinander: Ihre Farben
        // unterscheiden sich nicht mehr als zwei benachbarte Spalten mitten im Bild.
        let surface = generate(&input(Style::Garden), (512, 256), &AtomicBool::new(false))
            .expect("nicht abgebrochen");
        let layer = &surface.albedo;
        let step = |a: usize, b: usize| -> f32 {
            (0..layer.height)
                .map(|y| {
                    let (p, q) = (layer.pixel(a, y), layer.pixel(b, y));
                    (0..3)
                        .map(|c| (f32::from(p[c]) - f32::from(q[c])).abs())
                        .sum::<f32>()
                })
                .sum::<f32>()
                / layer.height as f32
        };
        let seam = step(0, layer.width - 1);
        let inside = (1..8)
            .map(|offset| step(layer.width / 2 + offset, layer.width / 2 + offset + 1))
            .sum::<f32>()
            / 7.0;
        assert!(seam < inside * 2.0 + 4.0, "Naht {seam}, innen {inside}");
    }

    #[test]
    fn each_kind_looks_different() {
        let means: Vec<(Style, [f32; 4])> = Style::ALL
            .iter()
            .map(|style| (*style, make(*style).albedo.mean()))
            .collect();
        for (index, (a, mean_a)) in means.iter().enumerate() {
            for (b, mean_b) in &means[index + 1..] {
                let distance: f32 = (0..3).map(|c| (mean_a[c] - mean_b[c]).abs()).sum();
                assert!(
                    distance > 0.04,
                    "{a:?} und {b:?} sehen sich zu ähnlich: {mean_a:?} gegen {mean_b:?}"
                );
            }
        }
        // Das Meer der Ozeanwelt ist blau, die Eiswelt hell, die Glutwelt dunkel.
        let mean = |style: Style| means.iter().find(|entry| entry.0 == style).unwrap().1;
        assert!(mean(Style::Ocean)[2] > mean(Style::Ocean)[0] * 1.5);
        assert!(mean(Style::Ice)[0] > 0.6);
        assert!(mean(Style::Lava)[0] < 0.35);
    }

    #[test]
    fn lava_glows_rock_has_neither_clouds_nor_haze_and_air_has_both() {
        let lava = make(Style::Lava);
        let glow = lava.emissive.expect("die Glutwelt leuchtet");
        assert!(glow.mean()[0] > 0.02);
        let rock = make(Style::Rock);
        assert!(rock.clouds.is_none());
        assert!(rock.emissive.is_none());
        assert!(input(Style::Rock).haze().is_none());
        let garden = make(Style::Garden);
        assert!(garden.clouds.is_some());
        assert!(input(Style::Garden).haze().is_some());
        assert!(
            make(Style::Gas).normal.is_none(),
            "Gasriesen haben kein Relief"
        );
    }

    #[test]
    fn mips_go_down_to_one_pixel() {
        let layer = make(Style::Rock).albedo;
        assert_eq!(layer.levels, 8);
        let total: usize = (0..layer.levels)
            .map(|level| ((128 >> level).max(1)) * ((64 >> level).max(1)) * 4)
            .sum();
        assert_eq!(layer.data.len(), total);
    }

    /// Schreibt die Oberflächen jeder Art als Bilder in das Verzeichnis aus
    /// `KONFLUENZ_OBERFLAECHEN` und nennt die Zeit je Art. Aufruf:
    /// `KONFLUENZ_OBERFLAECHEN=/pfad cargo test -p game -- --ignored oberflaechen`.
    #[test]
    #[ignore = "schreibt die Oberflächen als Bilder"]
    fn oberflaechen() {
        let Ok(dir) = std::env::var("KONFLUENZ_OBERFLAECHEN") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        let save = |layer: &Layer, name: String| {
            let base = layer.data[..layer.width * layer.height * 4].to_vec();
            let image = Image::new(
                Extent3d {
                    width: layer.width as u32,
                    height: layer.height as u32,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                base,
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::default(),
            );
            image
                .try_into_dynamic()
                .expect("Bild")
                .save(dir.join(name))
                .expect("Bild schreiben");
        };
        for style in Style::ALL {
            let start = std::time::Instant::now();
            let surface = generate(&input(style), FULL, &AtomicBool::new(false)).unwrap();
            println!("{style:?}: {:?}", start.elapsed());
            let name = format!("{style:?}").to_lowercase();
            save(&surface.albedo, format!("{name}_farbe.png"));
            if let Some(layer) = &surface.clouds {
                save(layer, format!("{name}_wolken.png"));
            }
            if let Some(layer) = &surface.emissive {
                save(layer, format!("{name}_leuchten.png"));
            }
            if let Some(layer) = &surface.normal {
                save(layer, format!("{name}_normalen.png"));
            }
        }
    }

    #[test]
    fn a_cancelled_generation_stops() {
        assert!(generate(&input(Style::Garden), SMALL, &AtomicBool::new(true)).is_none());
    }

    #[test]
    fn the_full_surface_is_made_in_reasonable_time() {
        // Die Grenze ist großzügig: Auch der Debug-Bau auf einem langsamen Rechner muss
        // sie halten. Gemessen wird die teuerste Art.
        let start = std::time::Instant::now();
        let _ = generate(&input(Style::Garden), FULL, &AtomicBool::new(false));
        let spent = start.elapsed();
        assert!(spent.as_secs_f32() < 20.0, "{spent:?}");
    }
}

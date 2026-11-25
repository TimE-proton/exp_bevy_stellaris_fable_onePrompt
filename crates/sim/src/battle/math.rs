//! Zweidimensionale Vektoren für die Gefechtsebene.

use serde::{Deserialize, Serialize};
use std::ops::{Add, AddAssign, Mul, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct V2 {
    pub x: f32,
    pub y: f32,
}

impl V2 {
    pub const ZERO: V2 = V2 { x: 0.0, y: 0.0 };

    #[inline]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Einheitsvektor in Richtung `angle` (Bogenmaß, gegen den Uhrzeigersinn).
    #[inline]
    pub fn from_angle(angle: f32) -> Self {
        Self {
            x: angle.cos(),
            y: angle.sin(),
        }
    }

    #[inline]
    pub fn angle(self) -> f32 {
        self.y.atan2(self.x)
    }

    #[inline]
    pub fn dot(self, other: V2) -> f32 {
        self.x * other.x + self.y * other.y
    }

    /// Kreuzprodukt (z-Komponente).
    #[inline]
    pub fn cross(self, other: V2) -> f32 {
        self.x * other.y - self.y * other.x
    }

    #[inline]
    pub fn length_squared(self) -> f32 {
        self.dot(self)
    }

    #[inline]
    pub fn length(self) -> f32 {
        self.length_squared().sqrt()
    }

    #[inline]
    pub fn distance(self, other: V2) -> f32 {
        (self - other).length()
    }

    /// Normiert; der Nullvektor bleibt null.
    #[inline]
    pub fn normalized(self) -> V2 {
        let length = self.length();
        if length > 1e-6 {
            self * (1.0 / length)
        } else {
            V2::ZERO
        }
    }

    /// Um 90 Grad nach links gedreht.
    #[inline]
    pub fn perp(self) -> V2 {
        V2 {
            x: -self.y,
            y: self.x,
        }
    }

    #[inline]
    pub fn rotated(self, angle: f32) -> V2 {
        let (sin, cos) = angle.sin_cos();
        V2 {
            x: self.x * cos - self.y * sin,
            y: self.x * sin + self.y * cos,
        }
    }

    /// Auf höchstens `max` gekürzt.
    #[inline]
    pub fn clamped(self, max: f32) -> V2 {
        let length = self.length();
        if length > max && length > 1e-6 {
            self * (max / length)
        } else {
            self
        }
    }
}

impl Add for V2 {
    type Output = V2;
    #[inline]
    fn add(self, other: V2) -> V2 {
        V2 {
            x: self.x + other.x,
            y: self.y + other.y,
        }
    }
}

impl AddAssign for V2 {
    #[inline]
    fn add_assign(&mut self, other: V2) {
        self.x += other.x;
        self.y += other.y;
    }
}

impl Sub for V2 {
    type Output = V2;
    #[inline]
    fn sub(self, other: V2) -> V2 {
        V2 {
            x: self.x - other.x,
            y: self.y - other.y,
        }
    }
}

impl Mul<f32> for V2 {
    type Output = V2;
    #[inline]
    fn mul(self, factor: f32) -> V2 {
        V2 {
            x: self.x * factor,
            y: self.y * factor,
        }
    }
}

/// Winkel auf (-π, π] gebracht.
#[inline]
pub fn wrap_angle(angle: f32) -> f32 {
    let mut wrapped = angle % std::f32::consts::TAU;
    if wrapped > std::f32::consts::PI {
        wrapped -= std::f32::consts::TAU;
    } else if wrapped <= -std::f32::consts::PI {
        wrapped += std::f32::consts::TAU;
    }
    wrapped
}

/// Dreht `from` höchstens um `max_step` auf `to` zu.
#[inline]
pub fn turn_toward(from: f32, to: f32, max_step: f32) -> f32 {
    let diff = wrap_angle(to - from);
    wrap_angle(from + diff.clamp(-max_step, max_step))
}

/// Abstand eines Punkts von der Strecke `a`–`b`, quadriert.
#[inline]
pub fn distance_to_segment_squared(point: V2, a: V2, b: V2) -> f32 {
    let ab = b - a;
    let length_squared = ab.length_squared();
    if length_squared < 1e-9 {
        return (point - a).length_squared();
    }
    let t = ((point - a).dot(ab) / length_squared).clamp(0.0, 1.0);
    (point - (a + ab * t)).length_squared()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::{FRAC_PI_2, PI};

    #[test]
    fn angles_wrap_and_turn_the_short_way() {
        // π und -π sind dieselbe Richtung; an der Naht entscheidet die Rundung, welche der
        // beiden Darstellungen herauskommt.
        let same_direction =
            |a: f32, b: f32| (V2::from_angle(a) - V2::from_angle(b)).length() < 1e-4;
        assert!(same_direction(wrap_angle(3.0 * PI), PI));
        assert!(same_direction(wrap_angle(-3.0 * PI), PI));
        assert!(wrap_angle(7.0).abs() <= PI && wrap_angle(-7.0).abs() <= PI);
        // Von 170° nach -170° sind es 20° über die Naht, nicht 340° zurück.
        let from = 170f32.to_radians();
        let to = -170f32.to_radians();
        let stepped = turn_toward(from, to, 0.1);
        assert!(wrap_angle(stepped - from) > 0.0);
        assert!((wrap_angle(turn_toward(from, to, 1.0) - to)).abs() < 1e-5);
    }

    #[test]
    fn segment_distance_handles_ends_and_middle() {
        let a = V2::new(0.0, 0.0);
        let b = V2::new(10.0, 0.0);
        assert!((distance_to_segment_squared(V2::new(5.0, 3.0), a, b) - 9.0).abs() < 1e-5);
        assert!((distance_to_segment_squared(V2::new(-4.0, 0.0), a, b) - 16.0).abs() < 1e-5);
        assert!((distance_to_segment_squared(V2::new(13.0, 4.0), a, b) - 25.0).abs() < 1e-5);
    }

    #[test]
    fn rotation_and_perp_agree() {
        let v = V2::new(1.0, 0.0);
        let rotated = v.rotated(FRAC_PI_2);
        assert!((rotated.x).abs() < 1e-6 && (rotated.y - 1.0).abs() < 1e-6);
        assert_eq!(v.perp(), V2::new(0.0, 1.0));
        assert!((V2::from_angle(FRAC_PI_2).angle() - FRAC_PI_2).abs() < 1e-6);
    }
}

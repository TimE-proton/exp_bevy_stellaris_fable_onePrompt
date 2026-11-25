//! Eigener, serialisierbarer Zufallsgenerator (xoshiro256++ mit SplitMix64-Seeding).
//!
//! Bewusst ohne fremde Crate: Die Zahlenfolge darf sich durch kein Abhängigkeits-Update
//! ändern, sonst wären alte Seeds und Spielstände nicht mehr reproduzierbar.
//!
//! Ströme werden aus `(Seed, Teilsystem, a, b)` abgeleitet. Ein zusätzlicher Zufallszug
//! in einem Teilsystem verschiebt so nie die Zahlen eines anderen.

use serde::{Deserialize, Serialize};

/// Kennung eines Teilsystems für die Stromableitung.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u64)]
pub enum Domain {
    Galaxy = 1,
    Names = 2,
    Polity = 3,
    Economy = 4,
    Logistics = 5,
    Market = 6,
    Cohesion = 7,
    Tech = 8,
    Battle = 9,
    Siege = 10,
    Diplomacy = 11,
    Ai = 12,
    Intel = 13,
}

#[inline]
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// xoshiro256++.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rng {
    s: [u64; 4],
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        let mut sm = seed;
        let mut s = [0u64; 4];
        for slot in &mut s {
            *slot = splitmix64(&mut sm);
        }
        // Der Nullzustand ist der einzige unzulässige; SplitMix liefert ihn praktisch nie.
        if s == [0; 4] {
            s[0] = 1;
        }
        Self { s }
    }

    /// Unabhängiger Strom für ein Teilsystem und bis zu zwei Schlüssel (z. B. ID und Tag).
    pub fn stream(seed: u64, domain: Domain, a: u64, b: u64) -> Self {
        let mut sm = seed ^ 0xA076_1D64_78BD_642F;
        let mut mixed = splitmix64(&mut sm);
        for part in [domain as u64, a, b] {
            sm ^= part.wrapping_mul(0xE703_7ED1_A0B4_28DB);
            mixed ^= splitmix64(&mut sm).rotate_left(23);
        }
        Self::new(mixed)
    }

    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let result = self.s[0]
            .wrapping_add(self.s[3])
            .rotate_left(23)
            .wrapping_add(self.s[0]);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        result
    }

    /// Gleichverteilt in `[0, 1)`.
    #[inline]
    pub fn f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// Gleichverteilt in `[0, 1)`.
    #[inline]
    pub fn f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 * (1.0 / (1u32 << 24) as f32)
    }

    /// Gleichverteilt in `[lo, hi)`. Liefert `lo`, wenn der Bereich leer ist.
    #[inline]
    pub fn range_u32(&mut self, lo: u32, hi: u32) -> u32 {
        if hi <= lo {
            return lo;
        }
        let span = (hi - lo) as u64;
        // Multiply-Shift; die winzige Schiefe ist für Spielzwecke belanglos.
        lo + (((self.next_u64() >> 32) * span) >> 32) as u32
    }

    #[inline]
    pub fn range_usize(&mut self, lo: usize, hi: usize) -> usize {
        self.range_u32(lo as u32, hi as u32) as usize
    }

    /// Gleichverteilt in `[lo, hi]` (beide Grenzen eingeschlossen).
    #[inline]
    pub fn range_inclusive(&mut self, lo: u32, hi: u32) -> u32 {
        self.range_u32(lo, hi.saturating_add(1))
    }

    #[inline]
    pub fn range_f32(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f32()
    }

    #[inline]
    pub fn range_f64(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.f64()
    }

    #[inline]
    pub fn chance(&mut self, probability: f64) -> bool {
        self.f64() < probability
    }

    /// Standardnormalverteilt (Box-Muller).
    pub fn normal(&mut self) -> f64 {
        let u1 = (1.0 - self.f64()).max(f64::MIN_POSITIVE);
        let u2 = self.f64();
        (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
    }

    /// Index nach Gewichten. Nicht positive Gewichte werden nie gewählt; sind alle
    /// Gewichte nicht positiv, wird gleichverteilt gezogen.
    pub fn pick_weighted(&mut self, weights: &[f32]) -> usize {
        debug_assert!(!weights.is_empty());
        let total: f32 = weights.iter().filter(|w| **w > 0.0).sum();
        if total <= 0.0 {
            return self.range_usize(0, weights.len());
        }
        let mut roll = self.f32() * total;
        let mut last_positive = 0;
        for (index, weight) in weights.iter().enumerate() {
            if *weight <= 0.0 {
                continue;
            }
            last_positive = index;
            if roll < *weight {
                return index;
            }
            roll -= *weight;
        }
        last_positive
    }

    /// Fisher-Yates.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.range_usize(0, i + 1);
            items.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_sequence() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn reference_values_are_stable() {
        // Schutz gegen versehentliche Änderungen am Generator: alte Seeds müssen
        // denselben Verlauf liefern.
        let mut rng = Rng::new(0);
        let first: Vec<u64> = (0..3).map(|_| rng.next_u64()).collect();
        assert_eq!(
            first,
            vec![0x53175D61490B23DF, 0x61DA6F3DC380D507, 0x5C0FDF91EC9A7BFC]
        );
    }

    #[test]
    fn streams_differ_by_domain_and_keys() {
        let a = Rng::stream(7, Domain::Galaxy, 0, 0).next_u64();
        let b = Rng::stream(7, Domain::Economy, 0, 0).next_u64();
        let c = Rng::stream(7, Domain::Galaxy, 1, 0).next_u64();
        let d = Rng::stream(7, Domain::Galaxy, 0, 1).next_u64();
        let e = Rng::stream(8, Domain::Galaxy, 0, 0).next_u64();
        let all = [a, b, c, d, e];
        for i in 0..all.len() {
            for j in (i + 1)..all.len() {
                assert_ne!(all[i], all[j]);
            }
        }
    }

    #[test]
    fn ranges_stay_in_bounds() {
        let mut rng = Rng::new(3);
        for _ in 0..10_000 {
            let v = rng.range_u32(5, 9);
            assert!((5..9).contains(&v));
            let f = rng.f64();
            assert!((0.0..1.0).contains(&f));
            let g = rng.f32();
            assert!((0.0..1.0).contains(&g));
        }
        assert_eq!(rng.range_u32(4, 4), 4);
    }

    #[test]
    fn weighted_pick_skips_zero_weights() {
        let mut rng = Rng::new(9);
        for _ in 0..1000 {
            let i = rng.pick_weighted(&[0.0, 2.0, 0.0, 1.0]);
            assert!(i == 1 || i == 3);
        }
    }
}

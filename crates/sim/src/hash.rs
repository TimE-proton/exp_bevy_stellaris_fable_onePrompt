//! Zustandsprüfsumme für Determinismusprüfungen.
//!
//! Der Zustand wird kanonisch in Bytes gewandelt (postcard: feste Feldreihenfolge, keine
//! Zeiger, keine Hash-Tabellen) und mit FNV-1a gehasht.

use serde::Serialize;

#[derive(Clone, Copy, Debug)]
pub struct Fnv64(u64);

impl Default for Fnv64 {
    fn default() -> Self {
        Self::new()
    }
}

impl Fnv64 {
    pub const fn new() -> Self {
        Self(0xCBF2_9CE4_8422_2325)
    }

    #[inline]
    pub fn write(&mut self, bytes: &[u8]) {
        let mut hash = self.0;
        for byte in bytes {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
        }
        self.0 = hash;
    }

    /// Hängt die kanonische Byteform eines Werts an. `scratch` wird wiederverwendet.
    pub fn write_value<T: Serialize + ?Sized>(&mut self, value: &T, scratch: &mut Vec<u8>) {
        scratch.clear();
        let buffer = std::mem::take(scratch);
        let buffer = postcard::to_extend(value, buffer)
            .expect("Simulationszustand muss serialisierbar sein");
        self.write(&buffer);
        *scratch = buffer;
    }

    pub fn finish(&self) -> u64 {
        self.0
    }
}

/// Prüfsumme eines einzelnen Werts.
pub fn hash_value<T: Serialize + ?Sized>(value: &T) -> u64 {
    let mut hasher = Fnv64::new();
    let mut scratch = Vec::new();
    hasher.write_value(value, &mut scratch);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv_reference_vector() {
        let mut hasher = Fnv64::new();
        hasher.write(b"a");
        assert_eq!(hasher.finish(), 0xAF63_DC4C_8601_EC8C);
    }

    #[test]
    fn equal_values_hash_equal() {
        assert_eq!(
            hash_value(&(1u32, "x", 2.5f64)),
            hash_value(&(1u32, "x", 2.5f64))
        );
        assert_ne!(
            hash_value(&(1u32, "x", 2.5f64)),
            hash_value(&(1u32, "x", 2.6f64))
        );
    }
}

//! Uniformes Gitter als räumlicher Index. Wird jeden Takt neu gefüllt (Zählsortierung,
//! keine Allokation im Dauerbetrieb) und beantwortet „wer ist in der Nähe" in einer Zeit,
//! die nicht mit der Zahl aller Schiffe wächst.

use super::math::V2;

#[derive(Clone, Debug, Default)]
pub struct Grid {
    cell: f32,
    half_extent: f32,
    side: usize,
    /// Anfang jeder Zelle in `items` (Länge `side * side + 1`).
    starts: Vec<u32>,
    items: Vec<u32>,
    scratch: Vec<u32>,
}

impl Grid {
    /// Füllt das Gitter neu. `positions` liefert zu jedem Eintrag Position und ob er
    /// aufgenommen wird.
    pub fn rebuild(
        &mut self,
        cell: f32,
        half_extent: f32,
        positions: impl Iterator<Item = Option<V2>> + Clone,
    ) {
        let side = ((2.0 * half_extent / cell).ceil() as usize).max(1);
        self.cell = cell;
        self.half_extent = half_extent;
        self.side = side;
        self.starts.clear();
        self.starts.resize(side * side + 1, 0);
        let mut count = 0usize;
        for position in positions.clone().flatten() {
            let index = self.cell_of(position);
            self.starts[index + 1] += 1;
            count += 1;
        }
        for index in 0..side * side {
            self.starts[index + 1] += self.starts[index];
        }
        self.items.clear();
        self.items.resize(count, 0);
        self.scratch.clear();
        self.scratch.extend_from_slice(&self.starts[..side * side]);
        for (item, position) in positions.enumerate() {
            if let Some(position) = position {
                let cell = self.cell_of(position);
                let slot = self.scratch[cell] as usize;
                self.items[slot] = item as u32;
                self.scratch[cell] += 1;
            }
        }
    }

    #[inline]
    fn coordinate(&self, value: f32) -> usize {
        let scaled = ((value + self.half_extent) / self.cell).floor();
        (scaled.max(0.0) as usize).min(self.side - 1)
    }

    #[inline]
    fn cell_of(&self, position: V2) -> usize {
        self.coordinate(position.y) * self.side + self.coordinate(position.x)
    }

    /// Ruft `visit` für jeden Eintrag auf, dessen Zelle das Rechteck um `center` mit
    /// Halbmesser `radius` berührt. Einträge einer Zelle kommen in aufsteigender Folge,
    /// Zellen zeilenweise: Die Reihenfolge ist fest.
    #[inline]
    pub fn for_each_near(&self, center: V2, radius: f32, mut visit: impl FnMut(u32)) {
        if self.side == 0 {
            return;
        }
        let (x0, x1) = (
            self.coordinate(center.x - radius),
            self.coordinate(center.x + radius),
        );
        let (y0, y1) = (
            self.coordinate(center.y - radius),
            self.coordinate(center.y + radius),
        );
        for y in y0..=y1 {
            let row = y * self.side;
            let (start, end) = (
                self.starts[row + x0] as usize,
                self.starts[row + x1 + 1] as usize,
            );
            for item in &self.items[start..end] {
                visit(*item);
            }
        }
    }

    /// Wie [`Grid::for_each_near`], für das Rechteck, das die Strecke `a`–`b` samt Rand
    /// `margin` umschließt.
    #[inline]
    pub fn for_each_along(&self, a: V2, b: V2, margin: f32, mut visit: impl FnMut(u32)) {
        if self.side == 0 {
            return;
        }
        let (x0, x1) = (
            self.coordinate(a.x.min(b.x) - margin),
            self.coordinate(a.x.max(b.x) + margin),
        );
        let (y0, y1) = (
            self.coordinate(a.y.min(b.y) - margin),
            self.coordinate(a.y.max(b.y) + margin),
        );
        for y in y0..=y1 {
            let row = y * self.side;
            let (start, end) = (
                self.starts[row + x0] as usize,
                self.starts[row + x1 + 1] as usize,
            );
            for item in &self.items[start..end] {
                visit(*item);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Rng;

    #[test]
    fn grid_finds_exactly_what_brute_force_finds() {
        let mut rng = Rng::new(11);
        let points: Vec<Option<V2>> = (0..600)
            .map(|index| {
                // Jeder siebte Eintrag ist ausgeblendet (zerstörtes Schiff).
                (index % 7 != 0).then(|| {
                    V2::new(
                        rng.range_f32(-2900.0, 2900.0),
                        rng.range_f32(-2900.0, 2900.0),
                    )
                })
            })
            .collect();
        let mut grid = Grid::default();
        grid.rebuild(220.0, 3000.0, points.iter().copied());
        for _ in 0..200 {
            let center = V2::new(
                rng.range_f32(-3200.0, 3200.0),
                rng.range_f32(-3200.0, 3200.0),
            );
            let radius = rng.range_f32(10.0, 700.0);
            let mut found: Vec<u32> = Vec::new();
            grid.for_each_near(center, radius, |item| {
                if points[item as usize].unwrap().distance(center) <= radius {
                    found.push(item);
                }
            });
            found.sort_unstable();
            let expected: Vec<u32> = points
                .iter()
                .enumerate()
                .filter(|(_, point)| point.is_some_and(|p| p.distance(center) <= radius))
                .map(|(index, _)| index as u32)
                .collect();
            assert_eq!(found, expected);
        }
    }

    #[test]
    fn points_outside_the_field_land_in_border_cells() {
        let points = [Some(V2::new(99_999.0, -99_999.0)), Some(V2::new(0.0, 0.0))];
        let mut grid = Grid::default();
        grid.rebuild(100.0, 500.0, points.iter().copied());
        let mut seen = Vec::new();
        grid.for_each_near(V2::new(480.0, -480.0), 50.0, |item| seen.push(item));
        assert_eq!(seen, vec![0]);
    }

    #[test]
    fn segment_query_covers_the_whole_path() {
        let points = [
            Some(V2::new(-300.0, 5.0)),
            Some(V2::new(0.0, -5.0)),
            Some(V2::new(300.0, 5.0)),
            Some(V2::new(0.0, 900.0)),
        ];
        let mut grid = Grid::default();
        grid.rebuild(100.0, 1000.0, points.iter().copied());
        let mut seen = Vec::new();
        grid.for_each_along(V2::new(-320.0, 0.0), V2::new(320.0, 0.0), 20.0, |item| {
            seen.push(item)
        });
        seen.sort_unstable();
        assert_eq!(seen, vec![0, 1, 2]);
    }
}

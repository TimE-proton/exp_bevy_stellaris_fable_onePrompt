//! Formationen: Wer wo steht. Plätze sind Versätze im Bezugssystem der Flotte
//! (x voraus zum Gegner, y nach Backbord).

use super::{BattleShip, Formation, V2};
use crate::data::ShipClass;

/// Verteilt `count` Plätze symmetrisch um die Mitte; gibt den Versatz des Platzes `index`.
fn centered(index: usize, count: usize, spacing: f32) -> f32 {
    (index as f32 - (count as f32 - 1.0) * 0.5) * spacing
}

/// Weist den Schiffen `members` ihre Plätze zu. Bordjäger haben keinen Platz; sie folgen
/// ihren Zielen und ihrem Träger. Träger stehen weit hinten, solange sie Jäger führen;
/// mit `carriers_in_line` (keine Jäger mehr) stehen sie in der Linie, denn dann kämpfen
/// sie mit ihren eigenen, kurz reichenden Geschützen.
pub fn assign_slots(
    ships: &mut [BattleShip],
    members: &[usize],
    formation: Formation,
    spacing: f32,
    carriers_in_line: bool,
) {
    // Schwere zuerst, bei Gleichstand in fester Reihenfolge.
    let of_class = |class: ShipClass| -> Vec<usize> {
        members
            .iter()
            .copied()
            .filter(|index| ships[*index].class == class)
            .collect()
    };
    let battleships = of_class(ShipClass::Battleship);
    let cruisers = of_class(ShipClass::Cruiser);
    let frigates = of_class(ShipClass::Frigate);
    let corvettes = of_class(ShipClass::Corvette);
    let mut carriers = of_class(ShipClass::Carrier);
    let mut line: Vec<usize> = battleships.iter().chain(cruisers.iter()).copied().collect();
    if carriers_in_line {
        line.append(&mut carriers);
    }
    // Die schwersten Schiffe in die Mitte der Linie.
    line.sort_by(|a, b| ships[*b].value.total_cmp(&ships[*a].value).then(a.cmp(b)));
    let mut centered_line: Vec<usize> = Vec::with_capacity(line.len());
    for (rank, index) in line.iter().enumerate() {
        if rank % 2 == 0 {
            centered_line.push(*index);
        } else {
            centered_line.insert(0, *index);
        }
    }
    let line = centered_line;
    let wide = spacing * 1.25;

    // Plätze erst sammeln, dann eintragen: Die Zuteilung liest die Schiffe noch.
    let mut placed: Vec<(usize, V2)> = Vec::with_capacity(members.len());
    let mut place = |index: usize, slot: V2| placed.push((index, slot));

    match formation {
        Formation::Line => {
            for (position, index) in line.iter().enumerate() {
                place(*index, V2::new(0.0, centered(position, line.len(), wide)));
            }
            for (position, index) in frigates.iter().enumerate() {
                place(
                    *index,
                    V2::new(
                        spacing * 1.7,
                        centered(position, frigates.len(), spacing * 1.1),
                    ),
                );
            }
            let half = (line.len().max(frigates.len()) as f32 * 0.5 + 1.0) * wide;
            for (position, index) in corvettes.iter().enumerate() {
                let wing = if position % 2 == 0 { 1.0 } else { -1.0 };
                let rank = (position / 2) as f32;
                place(
                    *index,
                    V2::new(spacing * 0.8, wing * (half + rank * spacing * 0.8)),
                );
            }
            for (position, index) in carriers.iter().enumerate() {
                place(
                    *index,
                    V2::new(
                        -spacing * 4.0,
                        centered(position, carriers.len(), spacing * 1.6),
                    ),
                );
            }
        }
        Formation::Wedge => {
            // Spitze voraus, je zwei Schiffe gestaffelt dahinter.
            let mut order: Vec<usize> = line.clone();
            order.sort_by(|a, b| ships[*b].value.total_cmp(&ships[*a].value).then(a.cmp(b)));
            order.extend(frigates.iter().copied());
            order.extend(corvettes.iter().copied());
            for (position, index) in order.iter().enumerate() {
                let row = position.div_ceil(2) as f32;
                let wing = if position % 2 == 1 { 1.0 } else { -1.0 };
                let lateral = if position == 0 {
                    0.0
                } else {
                    wing * row * spacing * 0.85
                };
                place(
                    *index,
                    V2::new(spacing * 2.0 - row * spacing * 0.9, lateral),
                );
            }
            let depth = order.len().div_ceil(2) as f32 * spacing * 0.9 + spacing * 2.0;
            for (position, index) in carriers.iter().enumerate() {
                place(
                    *index,
                    V2::new(-depth, centered(position, carriers.len(), spacing * 1.6)),
                );
            }
        }
        Formation::Screen => {
            // Geleit in weitem Bogen voraus.
            let screen: Vec<usize> = frigates.iter().chain(corvettes.iter()).copied().collect();
            let radius = spacing * 3.4;
            for (position, index) in screen.iter().enumerate() {
                let spread = if screen.len() > 1 {
                    centered(position, screen.len(), 2.2 / (screen.len() as f32 - 1.0))
                } else {
                    0.0
                };
                place(*index, V2::from_angle(spread) * radius);
            }
            // Schwere dicht gestaffelt in zwei Reihen.
            for (position, index) in line.iter().enumerate() {
                let row = (position % 2) as f32;
                let column = position / 2;
                let columns = line.len().div_ceil(2);
                place(
                    *index,
                    V2::new(-row * spacing, centered(column, columns, wide)),
                );
            }
            for (position, index) in carriers.iter().enumerate() {
                place(
                    *index,
                    V2::new(
                        -spacing * 3.6,
                        centered(position, carriers.len(), spacing * 1.6),
                    ),
                );
            }
        }
        Formation::Pincer => {
            // Ein Drittel hält die Mitte, zwei Drittel gehen als Flügel weit nach außen.
            let wing_offset = spacing * 9.0;
            let mut wings = [Vec::new(), Vec::new()];
            let mut center = Vec::new();
            for (position, index) in line.iter().enumerate() {
                match position % 3 {
                    0 => center.push(*index),
                    1 => wings[0].push(*index),
                    _ => wings[1].push(*index),
                }
            }
            for (position, index) in corvettes.iter().enumerate() {
                wings[position % 2].push(*index);
            }
            for (position, index) in center.iter().enumerate() {
                place(
                    *index,
                    V2::new(-spacing, centered(position, center.len(), wide)),
                );
            }
            for (position, index) in frigates.iter().enumerate() {
                place(
                    *index,
                    V2::new(
                        spacing * 0.6,
                        centered(position, frigates.len(), spacing * 1.1),
                    ),
                );
            }
            for (side, wing) in wings.iter().enumerate() {
                let sign = if side == 0 { 1.0 } else { -1.0 };
                for (position, index) in wing.iter().enumerate() {
                    let row = (position / 2) as f32;
                    let column = if position % 2 == 0 { 0.0 } else { spacing };
                    place(
                        *index,
                        V2::new(spacing * 2.5 - row * spacing, sign * (wing_offset + column)),
                    );
                }
            }
            for (position, index) in carriers.iter().enumerate() {
                place(
                    *index,
                    V2::new(
                        -spacing * 4.5,
                        centered(position, carriers.len(), spacing * 1.6),
                    ),
                );
            }
        }
    }
    for (index, slot) in placed {
        ships[index].slot = slot;
    }
}
